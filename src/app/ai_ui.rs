use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    thread,
};

use eframe::egui::{self, RichText};
use pdf_merger::{
    llama_backend::LlamaCppBackend,
    lm_studio_backend::{self, LmStudioBackend, StudioServer, StudioVisionReader},
    local_ocr::{self, TesseractOcr},
    model::PageSource,
    model::PreviewData,
    summarization::{
        ExtractedDocument, ExtractionLimits, ModelConfig, SummarizationBackend, SummaryAudience,
        SummaryLanguage, SummaryLength, SummaryPhase, SummaryProgress, SummaryRequest,
        extract_pdf_text_controlled, run_summary_job,
    },
    summary_scope::{SourcePage, SummaryCoverage, SummaryScope, select_scope},
};

use super::{AppMessage, PdfMergerApp, jobs::JobPhase, style};

const RECOMMENDED_MODEL_NAME: &str = "Qwen3.5 4B · Q4_K_M";
const RECOMMENDED_MODEL_FILE: &str = "Qwen3.5-4B-Q4_K_M.gguf";
const RECOMMENDED_MODEL_DOWNLOAD: &str =
    "https://huggingface.co/unsloth/Qwen3.5-4B-GGUF/resolve/main/Qwen3.5-4B-Q4_K_M.gguf";
const RECOMMENDED_MODEL_PAGE: &str = "https://huggingface.co/unsloth/Qwen3.5-4B-GGUF";

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScanReader {
    AiVision,
    Tesseract,
    Disabled,
}

pub(super) struct AiUiState {
    pub open: bool,
    pub active_summary: Option<super::jobs::JobId>,
    summary_sources: Vec<(PathBuf, Option<Vec<u32>>)>,
    studio: Option<StudioServer>,
    discovery: Option<Receiver<Result<StudioServer, String>>>,
    studio_status: String,
    studio_model: String,
    studio_port: u16,
    auto_discover: bool,
    use_studio: bool,
    backend_chosen: bool,
    pub model_path: Option<PathBuf>,
    pub source_path: Option<PathBuf>,
    pub length: SummaryLength,
    pub audience: SummaryAudience,
    pub language: SummaryLanguage,
    pub result: String,
    pub diagnostics: String,
    pub coverage: SummaryCoverage,
    pub cited: Vec<u32>,
    pub warnings: Vec<String>,
    scope: SummaryScope,
    group: Option<u64>,
    context_size: usize,
    scan_reader: ScanReader,
    ocr_language: String,
    ocr_executable: Option<PathBuf>,
    pub navigate_page: Option<u64>,
    citation_view: Option<(String, Receiver<Result<PreviewData, String>>)>,
    citation_texture: Option<(String, egui::TextureHandle)>,
}

impl Default for AiUiState {
    fn default() -> Self {
        Self {
            open: false,
            active_summary: None,
            summary_sources: Vec::new(),
            studio: None,
            discovery: None,
            studio_status: String::new(),
            studio_model: String::new(),
            studio_port: 1234,
            auto_discover: true,
            use_studio: false,
            backend_chosen: false,
            model_path: None,
            source_path: None,
            length: SummaryLength::Standard,
            audience: SummaryAudience::General,
            language: SummaryLanguage::SameAsDocument,
            result: String::new(),
            diagnostics: String::new(),
            coverage: SummaryCoverage::default(),
            cited: Vec::new(),
            warnings: Vec::new(),
            scope: SummaryScope::Selected,
            group: None,
            context_size: 8192,
            scan_reader: ScanReader::AiVision,
            ocr_language: "eng".into(),
            ocr_executable: local_ocr::available(),
            navigate_page: None,
            citation_view: None,
            citation_texture: None,
        }
    }
}

impl AiUiState {
    pub(super) fn clear_summary(&mut self) -> Option<super::jobs::JobId> {
        self.result.clear();
        self.diagnostics.clear();
        self.coverage = SummaryCoverage::default();
        self.cited.clear();
        self.warnings.clear();
        self.navigate_page = None;
        self.citation_view = None;
        self.citation_texture = None;
        self.summary_sources.clear();
        self.active_summary.take()
    }

    fn copy_summary(&self) -> String {
        let mut text = self.result.clone();
        if self.coverage.partial() {
            text.insert_str(0, "PARTIAL SUMMARY — review source coverage.\n\n");
        }
        for warning in &self.warnings {
            text.push_str(&format!("\nWarning: {warning}"));
        }
        text.push_str("\n\nSource references (not fact verification):\n");
        for id in &self.cited {
            text.push_str(&format!("[p. {id}] {}\n", self.coverage.label(*id)));
        }
        for (path, report) in &self.coverage.sources {
            text.push_str(&format!(
                "{}: processed {:?}; skipped {:?}; failed {:?}; truncated {:?}; OCR {:?}; AI vision {:?}\n",
                path.display(),
                report.processed,
                report.skipped,
                report.failed,
                report.truncated,
                report.ocr,
                report.vision
            ));
        }
        text
    }

    fn invalidate_discovery(&mut self) {
        self.auto_discover = false;
        self.studio = None;
        self.discovery = None; // Drop the old receiver so late results cannot revalidate another port.
        self.studio_model.clear();
        self.studio_status = "Port changed. Refresh LM Studio to reconnect.".into();
    }
}

impl PdfMergerApp {
    pub(super) fn reset_ai_document(&mut self, first_new_page: usize) {
        if let Some(job) = self.ai_ui.clear_summary() {
            self.jobs.cancel(job);
        }
        let pages = &self.workspace.pages()[first_new_page..];
        self.ai_ui.source_path = pages.iter().find_map(|page| match &page.source {
            PageSource::Pdf { path, .. } => Some(path.clone()),
            _ => None,
        });
        self.ai_ui.group = pages.first().map(|page| page.group_id);
    }

    pub(super) fn focus_ai_group(&mut self, group_id: u64) {
        let Some(group) = self
            .workspace
            .groups()
            .into_iter()
            .find(|g| g.id == group_id)
        else {
            return;
        };
        self.ai_ui.group = Some(group.id);
        self.ai_ui.source_path = Some(group.source_path);
        self.ai_ui.scope = SummaryScope::Group;
        self.selected = self.workspace.group_page_ids(group.id);
        self.sync_ai_scope();
        self.collapsed_groups.remove(&group.id);
        self.ai_ui.navigate_page = self.workspace.pages().get(group.start).map(|p| p.id);
    }

    pub(super) fn focused_ai_group(&self) -> Option<u64> {
        self.ai_ui.group
    }

    fn sync_ai_scope(&mut self) {
        let sources = select_scope(
            self.workspace.pages(),
            &self.selected,
            self.ai_ui.scope,
            self.ai_ui.group,
            self.ai_ui.source_path.as_ref(),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|s| (s.path, s.pages))
        .collect::<Vec<_>>();
        if sources != self.ai_ui.summary_sources {
            if let Some(job) = self.ai_ui.clear_summary() {
                self.jobs.cancel(job);
            }
            self.ai_ui.summary_sources = sources;
        }
    }

    pub(super) fn open_ai_dialog(&mut self) {
        if self.ai_ui.model_path.is_none() {
            self.ai_ui.model_path = discover_recommended_model();
        }
        if self.ai_ui.source_path.is_none() {
            self.ai_ui.source_path = self.pdf_sources().into_iter().next();
        }
        if self.ai_ui.group.is_none() {
            self.ai_ui.group = self.workspace.groups().first().map(|g| g.id);
        }
        self.ai_ui.open = true;
        self.discover_lm_studio();
    }

    fn discover_lm_studio(&mut self) {
        if self.ai_ui.discovery.is_some() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let port = self.ai_ui.studio_port;
        let auto_discover = self.ai_ui.auto_discover;
        self.ai_ui.studio = None;
        self.ai_ui.studio_status = "Looking for LM Studio…".into();
        self.ai_ui.discovery = Some(receiver);
        thread::spawn(move || {
            let _ = sender.send(
                lm_studio_backend::discover_auto(port, auto_discover)
                    .map_err(|error| format!("{error:#}")),
            );
        });
    }

    fn pdf_sources(&self) -> Vec<PathBuf> {
        let mut sources = Vec::new();
        for page in self.workspace.pages() {
            if let PageSource::Pdf { path, .. } = &page.source
                && !sources.contains(path)
            {
                sources.push(path.clone());
            }
        }
        sources
    }

    pub(super) fn show_ai_dialog(&mut self, root_ui: &mut egui::Ui, context: &egui::Context) {
        let sources = self.pdf_sources();
        if self
            .ai_ui
            .source_path
            .as_ref()
            .is_some_and(|path| !sources.contains(path))
        {
            self.ai_ui.source_path = None;
        }
        self.sync_ai_scope();
        self.show_summary_citation(context);
        if !self.ai_ui.open {
            return;
        }
        if let Some(receiver) = &self.ai_ui.discovery {
            context.request_repaint_after(std::time::Duration::from_millis(100));
            if let Ok(result) = receiver.try_recv() {
                self.ai_ui.discovery = None;
                match result {
                    Ok(server) => {
                        self.ai_ui.studio_port = server.port;
                        self.ai_ui.auto_discover = false;
                        if !server
                            .models
                            .iter()
                            .any(|m| m.id == self.ai_ui.studio_model)
                        {
                            self.ai_ui.studio_model = server
                                .models
                                .first()
                                .map(|m| m.id.clone())
                                .unwrap_or_default();
                        }
                        self.ai_ui.studio_status = if server.models.is_empty() {
                            "LM Studio found. Download a language model in LM Studio, then refresh."
                                .into()
                        } else {
                            if !self.ai_ui.backend_chosen {
                                self.ai_ui.use_studio = true;
                            }
                            "LM Studio found on this computer.".into()
                        };
                        self.ai_ui.studio = Some(server);
                    }
                    Err(error) => self.ai_ui.studio_status = error,
                }
            }
        }
        let sources = self.pdf_sources();
        let mut open = self.ai_ui.open;
        egui::Panel::right("ai_summary_panel")
            .resizable(true)
            .default_size(380.0)
            .size_range(300.0..=560.0)
            .show(root_ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.heading("AI summary");
                    if ui.button("Close").clicked() { open = false; }
                });
                ui.label("Local processing · verify important facts");
                ui.separator();
                egui::ScrollArea::vertical().id_salt("ai_panel_content").show(ui, |ui| {
                if self.ai_ui.scope == SummaryScope::Original {
                ui.horizontal_wrapped(|ui| {
                    ui.label("PDF:");
                    egui::ComboBox::from_id_salt("ai_source_pdf")
                        .selected_text(
                            self.ai_ui
                                .source_path
                                .as_ref()
                                .and_then(|path| path.file_name())
                                .and_then(|name| name.to_str())
                                .unwrap_or("No PDF available"),
                        )
                        .show_ui(ui, |ui| {
                            for path in &sources {
                                let label = path
                                    .file_name()
                                    .and_then(|name| name.to_str())
                                    .unwrap_or("PDF");
                                ui.selectable_value(
                                    &mut self.ai_ui.source_path,
                                    Some(path.clone()),
                                    label,
                                );
                            }
                        });
                });
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label("Scope:");
                    ui.selectable_value(&mut self.ai_ui.scope, SummaryScope::Selected, "Selected pages");
                    ui.selectable_value(&mut self.ai_ui.scope, SummaryScope::Group, "Document group");
                    ui.selectable_value(&mut self.ai_ui.scope, SummaryScope::Original, "Original PDF");
                });
                if self.ai_ui.scope == SummaryScope::Group {
                    egui::ComboBox::from_id_salt("summary_group").selected_text(self.workspace.groups().iter().find(|g| Some(g.id) == self.ai_ui.group).map(|g| g.source_path.file_name().unwrap_or_default().to_string_lossy().into_owned()).unwrap_or_else(|| "Choose a document".into())).show_ui(ui, |ui| {
                        for group in self.workspace.groups() {
                            ui.selectable_value(&mut self.ai_ui.group, Some(group.id), format!("{} · {} pages", group.source_path.file_name().unwrap_or_default().to_string_lossy(), group.page_count()));
                        }
                    });
                }
                self.sync_ai_scope();
                let scope = select_scope(self.workspace.pages(), &self.selected, self.ai_ui.scope, self.ai_ui.group, self.ai_ui.source_path.as_ref());
                match &scope {
                    Ok(sources) => { for source in sources { ui.strong(source.path.file_name().unwrap_or_default().to_string_lossy()); } ui.label(format!("{} source PDF(s); {}", sources.len(), if self.ai_ui.scope == SummaryScope::Original { "all pages in the original file (including pages removed from the workspace)" } else { "only pages in the chosen scope" })); }
                    Err(error) => { ui.colored_label(ui.visuals().warn_fg_color, error.to_string()); }
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label("Length:");
                    ui.selectable_value(&mut self.ai_ui.length, SummaryLength::Short, "Short");
                    ui.selectable_value(
                        &mut self.ai_ui.length,
                        SummaryLength::Standard,
                        "Standard",
                    );
                    ui.selectable_value(
                        &mut self.ai_ui.length,
                        SummaryLength::Detailed,
                        "Detailed",
                    );
                });
                ui.collapsing("Summary preferences", |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Audience:");
                    ui.selectable_value(
                        &mut self.ai_ui.audience,
                        SummaryAudience::General,
                        "General",
                    );
                    ui.selectable_value(
                        &mut self.ai_ui.audience,
                        SummaryAudience::Technical,
                        "Technical",
                    );
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Output language:");
                    ui.selectable_value(
                        &mut self.ai_ui.language,
                        SummaryLanguage::SameAsDocument,
                        "Same as document",
                    );
                    ui.selectable_value(
                        &mut self.ai_ui.language,
                        SummaryLanguage::English,
                        "English",
                    );
                    ui.selectable_value(
                        &mut self.ai_ui.language,
                        SummaryLanguage::French,
                        "French",
                    );
                    if ui
                        .selectable_label(
                            matches!(self.ai_ui.language, SummaryLanguage::Custom(_)),
                            "Custom",
                        )
                        .clicked()
                    {
                        self.ai_ui.language = SummaryLanguage::Custom(String::new());
                    }
                });
                if let SummaryLanguage::Custom(language) = &mut self.ai_ui.language {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Language name:");
                        ui.add(
                            egui::TextEdit::singleline(language)
                                .hint_text("e.g. Spanish")
                                .char_limit(40),
                        );
                    });
                }
                });
                let can_start = (if self.ai_ui.use_studio { self.ai_ui.studio.is_some() && !self.ai_ui.studio_model.is_empty() && self.ai_ui.discovery.is_none() } else { self.ai_ui.model_path.is_some() })
                    && scope.is_ok()
                    && !matches!(
                        &self.ai_ui.language,
                        SummaryLanguage::Custom(language) if language.trim().is_empty()
                    )
                    && self.jobs.active_count() == 0;
                if ui
                    .add_enabled(can_start, egui::Button::new("Summarize locally"))
                    .clicked()
                {
                    self.start_ai_summary(context);
                }
                if let Some(id) = self.ai_ui.active_summary
                    && let Some(job) = self.jobs.primary().filter(|job| job.id == id) {
                        ui.add(egui::ProgressBar::new(job.completed as f32 / job.total.max(1) as f32).text(&job.detail));
                        if ui.add_enabled(!job.cancelling, egui::Button::new("Cancel summary")).clicked() { self.jobs.cancel(id); }
                }
                if !self.ai_ui.diagnostics.is_empty() {
                    ui.label(RichText::new(&self.ai_ui.diagnostics).color(style::muted_text(ui)));
                }
                ui.collapsing("Extraction coverage (not fact verification)", |ui| {
                    for (path, report) in &self.ai_ui.coverage.sources {
                        ui.strong(path.display().to_string());
                        ui.label(format!("Processed: {:?}\nSkipped: {:?}\nFailed: {:?}\nTruncated by extraction limits: {:?}\nTesseract OCR: {:?}\nAI vision: {:?}", report.processed, report.skipped, report.failed, report.truncated, report.ocr, report.vision));
                        for warning in &report.warnings { ui.colored_label(ui.visuals().warn_fg_color, warning); }
                    }
                });
                if !self.ai_ui.result.is_empty() {
                    ui.separator();
                    ui.heading(if self.ai_ui.coverage.partial() || !self.ai_ui.warnings.is_empty() { "Partial or unverified summary" } else { "Generated summary" });
                    ui.label(
                        RichText::new("AI-generated; verify important details.")
                            .color(ui.visuals().warn_fg_color),
                    );
                    for warning in &self.ai_ui.warnings { ui.colored_label(ui.visuals().warn_fg_color, warning); }
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        if let Some(id) = summary_text(ui, &self.ai_ui.result, &self.ai_ui.cited, &self.ai_ui.coverage) {
                            self.open_summary_citation(id, context);
                        }
                    });
                    if ui.button("Copy summary").clicked() {
                        ui.ctx().copy_text(self.ai_ui.copy_summary());
                    }
                }
                ui.separator();
                egui::CollapsingHeader::new("AI setup")
                    .default_open(!self.ai_ui.use_studio && self.ai_ui.model_path.is_none())
                    .show(ui, |ui| {
                ui.label(
                    RichText::new("Runs locally. PDF text and summaries are never uploaded.")
                        .color(style::muted_text(ui)),
                );
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.label("AI backend:");
                    if ui.selectable_value(&mut self.ai_ui.use_studio, false, "Built-in GGUF").clicked()
                        | ui.selectable_value(&mut self.ai_ui.use_studio, true, "LM Studio").clicked() {
                        self.ai_ui.backend_chosen = true;
                    }
                });
                ui.label(&self.ai_ui.studio_status);
                if self.ai_ui.use_studio {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Local server port:");
                        if ui.add(egui::DragValue::new(&mut self.ai_ui.studio_port).range(1..=65535)).changed() {
                            self.ai_ui.invalidate_discovery();
                        }
                        if ui.add_enabled(self.ai_ui.discovery.is_none(), egui::Button::new("Refresh LM Studio")).clicked() {
                            self.discover_lm_studio();
                        }
                    });
                    ui.label("Enable the local server in LM Studio’s Developer tab. Models stay managed by LM Studio.");
                    egui::ComboBox::from_id_salt("studio_model").selected_text(&self.ai_ui.studio_model).show_ui(ui, |ui| {
                        if let Some(server) = &self.ai_ui.studio {
                            for model in &server.models {
                                ui.selectable_value(&mut self.ai_ui.studio_model, model.id.clone(), format!("{}{}", model.id, if model.vision { " · vision" } else { " · text only" }));
                            }
                        }
                    });
                } else {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("GGUF model:");
                        ui.label(self.ai_ui.model_path.as_ref().map_or(
                            "No model selected".to_owned(),
                            |path| {
                                path.file_name()
                                    .and_then(|name| name.to_str())
                                    .unwrap_or("Selected model")
                                    .to_owned()
                            },
                        ));
                        if ui.button("Choose GGUF…").clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .add_filter("GGUF model", &["gguf"])
                                .pick_file()
                        {
                            self.ai_ui.model_path = Some(path);
                        }
                    });
                    ui.group(|ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.strong("Recommended model:");
                            ui.label(RECOMMENDED_MODEL_NAME);
                        });
                        ui.label(
                            RichText::new(
                                "About 2.7 GB · multilingual · suitable for an 8 GB NVIDIA GPU or Apple Silicon Mac",
                            )
                            .color(style::muted_text(ui)),
                        );
                        ui.horizontal_wrapped(|ui| {
                            ui.hyperlink_to(
                                format!("Download {RECOMMENDED_MODEL_FILE}"),
                                RECOMMENDED_MODEL_DOWNLOAD,
                            );
                            ui.separator();
                            ui.hyperlink_to("Model details and license", RECOMMENDED_MODEL_PAGE);
                        });
                        ui.label(
                            RichText::new(
                                "Model downloads use your browser. After it finishes, click Choose GGUF above.",
                            )
                            .color(style::muted_text(ui)),
                        );
                    });
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label("Context budget (tokens):");
                    ui.add(egui::DragValue::new(&mut self.ai_ui.context_size).range(4096..=131072).speed(512));
                });
                ui.label("Keep the budget at or below the model’s loaded context size. LM Studio uses conservative sizing with automatic overflow retries.");
                ui.horizontal_wrapped(|ui| {
                    ui.label("Read scanned pages with:");
                    ui.selectable_value(&mut self.ai_ui.scan_reader, ScanReader::AiVision, "LM Studio AI vision");
                    ui.selectable_value(&mut self.ai_ui.scan_reader, ScanReader::Tesseract, "Tesseract OCR");
                    ui.selectable_value(&mut self.ai_ui.scan_reader, ScanReader::Disabled, "Text extraction only");
                });
                if self.ai_ui.scan_reader == ScanReader::AiVision {
                    let vision = self.ai_ui.use_studio && self.ai_ui.studio.as_ref().is_some_and(|server| server.models.iter().any(|model| model.id == self.ai_ui.studio_model && model.vision));
                    ui.label(if vision { "The selected AI model can read page images. No Tesseract or language packs are needed; accuracy depends on the model and scan quality." } else { "For scans, choose an LM Studio model marked ‘vision’. Text-only models and the built-in GGUF backend cannot read page images." });
                }
                if self.ai_ui.scan_reader == ScanReader::Tesseract {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("OCR language codes:"); ui.text_edit_singleline(&mut self.ai_ui.ocr_language);
                        if ui.button("Check Tesseract").clicked() { self.ai_ui.ocr_executable = local_ocr::available(); }
                    });
                    ui.label(if self.ai_ui.ocr_executable.is_some() { "Tesseract found. Language data must be installed (e.g. eng or eng+fra)." } else { "Tesseract not found. Install it on PATH and restart or check again. Text summaries and PDF editing remain available." });
                    ui.hyperlink_to("Local OCR installation", "https://tesseract-ocr.github.io/tessdoc/Installation.html");
                }
                    });
                });
            });
        self.ai_ui.open = open;
    }

    fn open_summary_citation(&mut self, id: u32, context: &egui::Context) {
        if self.ai_ui.citation_view.is_some() {
            return;
        }
        let Some(source) = self.ai_ui.coverage.references.get(&id).cloned() else {
            return;
        };
        if let Some(page) = self.workspace.pages().iter().find(|p| matches!(&p.source, PageSource::Pdf { path, page_number } if *path == source.path && *page_number == source.page)) {
            self.selected.clear(); self.selected.insert(page.id);
            self.collapsed_groups.remove(&page.group_id);
            self.ai_ui.navigate_page = Some(page.id);
        }
        // Citation navigation changes the grid selection, but still belongs to this summary.
        self.ai_ui.summary_sources = select_scope(
            self.workspace.pages(),
            &self.selected,
            self.ai_ui.scope,
            self.ai_ui.group,
            self.ai_ui.source_path.as_ref(),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|s| (s.path, s.pages))
        .collect();
        let label = self.ai_ui.coverage.label(id);
        let password = self.pdf_passwords.get(&source.path).cloned();
        let (sender, receiver) = mpsc::channel();
        self.ai_ui.citation_texture = None;
        self.ai_ui.citation_view = Some((label, receiver));
        let repaint = context.clone();
        thread::spawn(move || {
            let _ = sender.send(
                local_ocr::render_page(
                    &source.path,
                    password.as_ref().map(|p| p.as_str()),
                    source.page,
                    1200.0,
                )
                .map_err(|_| {
                    "Could not render original source page; the file may have moved or changed."
                        .into()
                }),
            );
            repaint.request_repaint();
        });
    }
    fn show_summary_citation(&mut self, context: &egui::Context) {
        if let Some((label, receiver)) = &self.ai_ui.citation_view {
            if let Ok(result) = receiver.try_recv() {
                match result {
                    Ok(preview) => {
                        let texture = context.load_texture(
                            "summary_source",
                            egui::ColorImage::from_rgba_unmultiplied(preview.size, &preview.rgba),
                            Default::default(),
                        );
                        self.ai_ui.citation_texture = Some((label.clone(), texture));
                    }
                    Err(error) => self.ai_ui.diagnostics = error,
                }
                self.ai_ui.citation_view = None;
            } else {
                context.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
    }

    pub(super) fn show_cited_page(&mut self, ui: &mut egui::Ui) -> bool {
        if self.ai_ui.citation_texture.is_none() && self.ai_ui.citation_view.is_none() {
            return false;
        }
        if ui.button("Back to pages").clicked() {
            self.ai_ui.citation_view = None;
            self.ai_ui.citation_texture = None;
            return false;
        }
        if self.ai_ui.citation_view.is_some() {
            ui.spinner();
            ui.label("Opening source page…");
        }
        if let Some((label, texture)) = &self.ai_ui.citation_texture {
            ui.label(label);
            egui::ScrollArea::both()
                .id_salt("cited_page_canvas")
                .show(ui, |ui| {
                    ui.add(
                        egui::Image::new(texture)
                            .max_width(ui.available_width())
                            .maintain_aspect_ratio(true),
                    );
                });
        }
        true
    }

    fn start_ai_summary(&mut self, context: &egui::Context) {
        let studio = if self.ai_ui.use_studio {
            let Some(server) = &self.ai_ui.studio else {
                return;
            };
            Some((server.port, self.ai_ui.studio_model.clone()))
        } else {
            None
        };
        let model_path = self.ai_ui.model_path.clone().unwrap_or_default();
        let sources = match select_scope(
            self.workspace.pages(),
            &self.selected,
            self.ai_ui.scope,
            self.ai_ui.group,
            self.ai_ui.source_path.as_ref(),
        ) {
            Ok(sources) => sources,
            Err(error) => {
                self.ai_ui.diagnostics = error.to_string();
                return;
            }
        };
        let passwords = self.pdf_passwords.clone();
        let context_size = self.ai_ui.context_size;
        let scan_reader = self.ai_ui.scan_reader;
        let supports_vision = self.ai_ui.use_studio
            && self.ai_ui.studio.as_ref().is_some_and(|server| {
                server
                    .models
                    .iter()
                    .any(|model| model.id == self.ai_ui.studio_model && model.vision)
            });
        let ocr_executable = self.ai_ui.ocr_executable.clone();
        let ocr_language = self.ai_ui.ocr_language.clone();
        let length = self.ai_ui.length;
        let audience = self.ai_ui.audience;
        let language = self.ai_ui.language.clone();
        self.ai_ui.result.clear();
        self.ai_ui.diagnostics.clear();
        self.ai_ui.coverage = SummaryCoverage::default();
        self.ai_ui.cited.clear();
        self.ai_ui.warnings.clear();
        let token = self
            .jobs
            .start("Local AI summary", JobPhase::Summarizing, 1);
        self.ai_ui.active_summary = Some(token.id());
        let sender = self.sender.clone();
        let repaint = context.clone();
        thread::spawn(move || {
            let mut report_progress = |progress: SummaryProgress| {
                let detail = match progress.phase {
                    SummaryPhase::Extracting => "Extracting PDF text",
                    SummaryPhase::Ocr => {
                        if scan_reader == ScanReader::AiVision {
                            "AI reading scanned page image"
                        } else {
                            "Recognizing scanned page with Tesseract"
                        }
                    }
                    SummaryPhase::LoadingModel => "Preparing model",
                    SummaryPhase::Generating => "Summarizing sections",
                    SummaryPhase::Synthesizing => "Synthesizing final summary",
                    SummaryPhase::UnloadingModel => "Finishing backend session",
                };
                let _ = sender.send(AppMessage::JobProgress {
                    job_id: token.id(),
                    phase: JobPhase::Summarizing,
                    completed: progress.completed,
                    total: progress.total.max(1),
                    detail: detail.into(),
                });
                repaint.request_repaint();
            };
            let mut coverage = SummaryCoverage::default();
            let extracted = (|| -> anyhow::Result<ExtractedDocument> {
                let mut all = ExtractedDocument {
                    pages: Vec::new(),
                    total_characters: 0,
                    truncated: false,
                };
                let mut ocr_pages_left = 50;
                for (source_index, source) in sources.iter().enumerate() {
                    let password = passwords.get(&source.path);
                    let mut ocr = TesseractOcr {
                        path: source.path.clone(),
                        password: password.cloned(),
                        executable: ocr_executable.clone(),
                        language: ocr_language.clone(),
                        pages_left: ocr_pages_left,
                    };
                    let mut vision = StudioVisionReader {
                        path: source.path.clone(),
                        password: password.cloned(),
                        port: studio.as_ref().map_or(1234, |s| s.0),
                        model: studio.as_ref().map_or_else(String::new, |s| s.1.clone()),
                        supports_vision,
                        pages_left: ocr_pages_left,
                        context_size,
                    };
                    let remaining = ExtractionLimits::default()
                        .max_characters_per_document
                        .saturating_sub(all.total_characters);
                    if remaining == 0 {
                        anyhow::bail!(
                            "Extraction limit reached before all sources were read; reduce the scope"
                        );
                    }
                    let (mut document, report) = extract_pdf_text_controlled(
                        &source.path,
                        password.map(|p| p.as_str()),
                        source.pages.as_deref(),
                        ExtractionLimits {
                            max_characters_per_document: remaining,
                            ..ExtractionLimits::default()
                        },
                        &|| token.is_cancelled(),
                        &mut report_progress,
                        match scan_reader {
                            ScanReader::AiVision => Some(&mut vision),
                            ScanReader::Tesseract => Some(&mut ocr),
                            ScanReader::Disabled => None,
                        },
                    )?;
                    ocr_pages_left = if scan_reader == ScanReader::AiVision {
                        vision.pages_left
                    } else {
                        ocr.pages_left
                    };
                    for page in &mut document.pages {
                        let original = page.page_number;
                        let id = coverage.references.len() as u32 + 1;
                        coverage.references.insert(
                            id,
                            SourcePage {
                                path: source.path.clone(),
                                page: original,
                            },
                        );
                        page.page_number = id;
                        if page.has_searchable_text {
                            page.text = format!(
                                "Source S{}, original PDF page {}. Reference [p. {}].\n{}",
                                source_index + 1,
                                original,
                                id,
                                page.text
                            );
                        }
                    }
                    all.total_characters += document.total_characters;
                    all.truncated |= document.truncated;
                    all.pages.extend(document.pages);
                    coverage.sources.push((source.path.clone(), report));
                    let _ = sender.send(AppMessage::SummaryCoverage {
                        job_id: token.id(),
                        coverage: coverage.clone(),
                    });
                    repaint.request_repaint();
                }
                if !all.pages.iter().any(|p| p.has_searchable_text) {
                    anyhow::bail!(
                        "No readable page text was obtained. For scanned PDFs, select LM Studio AI vision and a model marked ‘vision’, or install Tesseract for OCR. See Extraction coverage for the specific page errors."
                    );
                }
                Ok(all)
            })();
            let result = extracted.and_then(|document| {
                let request = SummaryRequest {
                    document,
                    length,
                    audience,
                    language,
                };
                let mut model = ModelConfig {
                    id: model_path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("local-model")
                        .to_owned(),
                    path: model_path,
                    context_size,
                };
                let mut backend: Box<dyn SummarizationBackend> = if let Some((port, id)) = studio {
                    model.id = id;
                    Box::new(LmStudioBackend::new(port))
                } else {
                    Box::new(LlamaCppBackend::new())
                };
                run_summary_job(
                    backend.as_mut(),
                    &model,
                    &request,
                    &|| token.is_cancelled(),
                    &mut report_progress,
                )
            });
            let cancelled = token.is_cancelled();
            let _ = sender.send(AppMessage::SummaryComplete {
                job_id: token.id(),
                result: result.map_err(|error| format!("{error:#}")),
                cancelled,
            });
            repaint.request_repaint();
        });
        self.set_status("Preparing a completely local summary…", false);
    }
}

fn summary_layout(
    text: &str,
    cited: &[u32],
    font: egui::FontId,
    color: egui::Color32,
    link_color: egui::Color32,
    warning_color: egui::Color32,
) -> (egui::text::LayoutJob, Vec<(std::ops::Range<usize>, u32)>) {
    let mut job = egui::text::LayoutJob::default();
    let normal = egui::TextFormat::simple(font, color);
    let mut links = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("[p.") {
        job.append(&rest[..start], 0.0, normal.clone());
        rest = &rest[start..];
        let Some(end) = rest.find(']') else { break };
        let tag = &rest[..=end];
        let id = tag[3..tag.len() - 1].trim().parse::<u32>().ok();
        if let Some(id) = id.filter(|id| cited.contains(id)) {
            let first = job.text.chars().count();
            let label = format!("[{id}]");
            let mut format = normal.clone();
            format.color = link_color;
            format.underline = egui::Stroke::new(1.0, link_color);
            job.append(&label, 0.0, format);
            links.push((first..first + label.chars().count(), id));
        } else {
            let mut format = normal.clone();
            format.color = warning_color;
            job.append(&format!("{tag} invalid citation"), 0.0, format);
        }
        rest = &rest[end + 1..];
    }
    job.append(rest, 0.0, normal);
    (job, links)
}

fn summary_text(
    ui: &mut egui::Ui,
    text: &str,
    cited: &[u32],
    coverage: &SummaryCoverage,
) -> Option<u32> {
    let (mut job, links) = summary_layout(
        text,
        cited,
        egui::TextStyle::Body.resolve(ui.style()),
        ui.visuals().text_color(),
        ui.visuals().hyperlink_color,
        ui.visuals().warn_fg_color,
    );
    job.wrap.max_width = ui.available_width().max(1.0);
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    let (rect, response) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
    ui.painter()
        .galley(rect.min, galley.clone(), ui.visuals().text_color());
    let mut clicked = None;
    let mut char_index = 0;
    for (row_index, row) in galley.rows.iter().enumerate() {
        for (link_index, (range, id)) in links.iter().enumerate() {
            let mut bounds = egui::Rect::NOTHING;
            for (offset, glyph) in row.glyphs.iter().enumerate() {
                if range.contains(&(char_index + offset)) {
                    let min = rect.min + row.pos.to_vec2() + egui::vec2(glyph.pos.x, 0.0);
                    bounds = bounds.union(egui::Rect::from_min_size(
                        min,
                        egui::vec2(glyph.advance_width, row.size.y),
                    ));
                }
            }
            if bounds.is_positive() {
                let response = ui
                    .interact(
                        bounds,
                        response.id.with((row_index, link_index)),
                        egui::Sense::click(),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(coverage.label(*id));
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Link, true, coverage.label(*id))
                });
                if response.clicked() {
                    clicked = Some(*id);
                }
            }
        }
        char_index += row.glyphs.len() + usize::from(row.ends_with_newline);
    }
    clicked
}

fn discover_recommended_model() -> Option<PathBuf> {
    recommended_model_candidates()
        .into_iter()
        .find(|path| path.is_file())
}

fn recommended_model_candidates() -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
    {
        directories.push(directory.to_owned());
        directories.push(directory.join("models"));
    }

    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from);
    if let Some(home) = &home {
        directories.push(home.join("Downloads"));
    }
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        directories.push(
            PathBuf::from(local_app_data)
                .join("PdfMerger")
                .join("models"),
        );
    } else if cfg!(target_os = "macos") {
        if let Some(home) = &home {
            directories.push(
                home.join("Library")
                    .join("Application Support")
                    .join("PdfMerger")
                    .join("models"),
            );
        }
    } else if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
        directories.push(PathBuf::from(data_home).join("PdfMerger").join("models"));
    } else if let Some(home) = &home {
        directories.push(home.join(".local/share/PdfMerger/models"));
    }

    candidate_paths(&directories)
}

fn candidate_paths(directories: &[PathBuf]) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for directory in directories {
        let candidate = directory.join(RECOMMENDED_MODEL_FILE);
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::{RECOMMENDED_MODEL_FILE, candidate_paths};
    use std::path::PathBuf;

    fn workspace_app() -> super::PdfMergerApp {
        use super::super::*;
        let (sender, receiver) = std::sync::mpsc::channel();
        let workspace = Workspace::default();
        let export_settings = ExportSettings::default();
        PdfMergerApp {
            project_ui: project_ui::ProjectUiState::new(
                workspace.fingerprint(),
                export_settings.clone(),
            ),
            export_dialog: export_dialog::ExportDialogState::new(export_settings.clone()),
            workspace,
            export_settings,
            sender,
            receiver,
            jobs: jobs::JobManager::default(),
            status: String::new(),
            status_is_error: false,
            preview_textures: Default::default(),
            pdf_previews: Default::default(),
            pdf_preview_order: Default::default(),
            pending_pdf_previews: Default::default(),
            failed_pdf_previews: Default::default(),
            selected: Default::default(),
            collapsed_groups: Default::default(),
            split_dialog: Default::default(),
            pdf_passwords: Default::default(),
            password_prompt: Default::default(),
            modal_focus: Default::default(),
            appearance: Default::default(),
            ai_ui: Default::default(),
        }
    }

    #[test]
    fn choosing_another_document_changes_scope_and_cancels_old_summary() {
        use pdf_merger::model::{PageDraft, PageSource};
        let mut app = workspace_app();
        for name in ["one.pdf", "two.pdf"] {
            app.workspace.append(vec![PageDraft {
                source: PageSource::Pdf {
                    path: name.into(),
                    page_number: 1,
                },
                title: name.into(),
                subtitle: String::new(),
                preview: None,
            }]);
        }
        let groups = app.workspace.groups();
        app.focus_ai_group(groups[0].id);
        let token = app
            .jobs
            .start("Summary", super::super::jobs::JobPhase::Summarizing, 1);
        app.ai_ui.active_summary = Some(token.id());
        app.ai_ui.result = "Old summary".into();
        app.focus_ai_group(groups[1].id);
        assert!(token.is_cancelled());
        assert!(app.ai_ui.result.is_empty());
        assert_eq!(app.ai_ui.source_path, Some("two.pdf".into()));
        assert_eq!(app.ai_ui.scope, super::SummaryScope::Group);
        assert_eq!(app.selected, app.workspace.group_page_ids(groups[1].id));
        assert_eq!(
            app.ai_ui.summary_sources,
            vec![(PathBuf::from("two.pdf"), Some(vec![1]))]
        );
    }

    #[test]
    fn docked_panels_leave_space_for_document_canvas() {
        use eframe::egui;
        use pdf_merger::model::{PageDraft, PageSource};
        for width in [920.0, 1280.0] {
            let context = egui::Context::default();
            context.begin_pass(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 800.0),
                )),
                ..Default::default()
            });
            let mut app = workspace_app();
            app.workspace.append(vec![PageDraft {
                source: PageSource::Pdf {
                    path: "synthetic.pdf".into(),
                    page_number: 1,
                },
                title: "Synthetic".into(),
                subtitle: String::new(),
                preview: None,
            }]);
            app.focus_ai_group(app.workspace.groups()[0].id);
            app.ai_ui.open = true;
            app.ai_ui.model_path = Some("mock.gguf".into());
            let mut ui = egui::Ui::new(
                context.clone(),
                egui::Id::new("root"),
                egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 800.0),
                )),
            );
            app.document_sidebar(&mut ui, &context);
            app.show_ai_dialog(&mut ui, &context);
            assert!(
                ui.available_width() >= 300.0,
                "canvas width {}",
                ui.available_width()
            );
            assert!(!app.has_active_modal());
            let _ = context.end_pass();
        }
    }

    #[test]
    fn summary_layout_wraps_without_overlapping_rows_or_losing_text() {
        use eframe::egui;
        let context = egui::Context::default();
        context.begin_pass(Default::default());
        let text = format!(
            "{} [p. 1] Après la date.\n\nNext paragraph [p. 2].",
            "A long sentence with an exact amount €123.45 and a date 07/09/2026. ".repeat(12)
        );
        for width in [140.0, 320.0, 600.0] {
            let (mut job, links) = super::summary_layout(
                &text,
                &[1, 2],
                egui::FontId::proportional(18.0),
                egui::Color32::WHITE,
                egui::Color32::BLUE,
                egui::Color32::YELLOW,
            );
            assert_eq!(
                job.text,
                text.replace("[p. 1]", "[1]").replace("[p. 2]", "[2]")
            );
            for (range, id) in &links {
                assert_eq!(
                    job.text
                        .chars()
                        .skip(range.start)
                        .take(range.len())
                        .collect::<String>(),
                    format!("[{id}]")
                );
            }
            job.wrap.max_width = width;
            let galley = context.fonts_mut(|fonts| fonts.layout_job(job));
            assert!(!galley.elided);
            assert!(galley.rows.len() > 3);
            for rows in galley.rows.windows(2) {
                assert!(rows[0].rect().bottom() <= rows[1].rect().top() + 0.01);
            }
            assert!(galley.size().y >= galley.rows.last().unwrap().rect().bottom());
        }
        let _ = context.end_pass();
    }

    #[test]
    fn switching_document_clears_summary_and_detaches_late_preview() {
        let mut state = super::AiUiState {
            active_summary: Some(7),
            result: "Old document".into(),
            diagnostics: "Old error".into(),
            cited: vec![1],
            warnings: vec!["Old warning".into()],
            studio_model: "keep-model".into(),
            studio_port: 1235,
            ..Default::default()
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        state.citation_view = Some(("Old PDF".into(), receiver));
        state.coverage.references.insert(
            1,
            super::SourcePage {
                path: "old.pdf".into(),
                page: 1,
            },
        );
        assert_eq!(state.clear_summary(), Some(7));
        assert!(state.active_summary.is_none());
        assert!(state.result.is_empty() && state.diagnostics.is_empty());
        assert!(state.cited.is_empty() && state.warnings.is_empty());
        assert!(state.coverage.references.is_empty());
        assert!(sender.send(Err("late old preview".into())).is_err());
        assert_eq!(state.studio_model, "keep-model");
        assert_eq!(state.studio_port, 1235);
        // A subsequent job cannot accept coverage/completion tagged with the old ID.
        state.active_summary = Some(8);
        assert_ne!(state.active_summary, Some(7));
    }

    #[test]
    fn changing_port_discards_discovery_and_late_results() {
        let mut state = super::AiUiState::default();
        let (sender, receiver) = std::sync::mpsc::channel();
        state.discovery = Some(receiver);
        state.studio = Some(pdf_merger::lm_studio_backend::StudioServer {
            port: 1234,
            models: Vec::new(),
        });
        state.studio_model = "old".into();
        state.studio_port = 4321;
        state.invalidate_discovery();
        assert!(
            state.studio.is_none() && state.discovery.is_none() && state.studio_model.is_empty()
        );
        assert!(sender.send(Err("old result".into())).is_err());
    }
    #[test]
    fn copied_summary_keeps_source_identity_and_partial_warning() {
        let mut state = super::AiUiState {
            result: "Fact [p. 1] [p. 2]".into(),
            cited: vec![1, 2],
            ..Default::default()
        };
        for (id, path) in [(1, "a.pdf"), (2, "b.pdf")] {
            state.coverage.references.insert(
                id,
                pdf_merger::summary_scope::SourcePage {
                    path: path.into(),
                    page: 1,
                },
            );
        }
        state.coverage.sources.push((
            "a.pdf".into(),
            pdf_merger::summarization::ExtractionReport {
                skipped: vec![2],
                ..Default::default()
            },
        ));
        let text = state.copy_summary();
        assert!(text.contains("PARTIAL"));
        assert!(text.contains("a.pdf · p. 1"));
        assert!(text.contains("b.pdf · p. 1"));
    }

    #[test]
    fn creates_unique_recommended_model_candidates() {
        let directory = PathBuf::from("model-directory");
        assert_eq!(
            candidate_paths(&[directory.clone(), directory.clone()]),
            vec![directory.join(RECOMMENDED_MODEL_FILE)]
        );
    }
}

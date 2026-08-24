use std::{path::PathBuf, thread};

use eframe::egui::{self, RichText};
use pdf_merger::{
    llama_backend::LlamaCppBackend,
    model::PageSource,
    summarization::{
        ExtractionLimits, ModelConfig, SummaryAudience, SummaryLanguage, SummaryLength,
        SummaryPhase, SummaryRequest, extract_pdf_text, run_summary_job,
    },
};

use super::{AppMessage, PdfMergerApp, jobs::JobPhase, style};

const RECOMMENDED_MODEL_NAME: &str = "Qwen3.5 4B · Q4_K_M";
const RECOMMENDED_MODEL_FILE: &str = "Qwen3.5-4B-Q4_K_M.gguf";
const RECOMMENDED_MODEL_DOWNLOAD: &str =
    "https://huggingface.co/unsloth/Qwen3.5-4B-GGUF/resolve/main/Qwen3.5-4B-Q4_K_M.gguf";
const RECOMMENDED_MODEL_PAGE: &str = "https://huggingface.co/unsloth/Qwen3.5-4B-GGUF";

pub(super) struct AiUiState {
    pub open: bool,
    pub model_path: Option<PathBuf>,
    pub source_path: Option<PathBuf>,
    pub length: SummaryLength,
    pub audience: SummaryAudience,
    pub language: SummaryLanguage,
    pub result: String,
    pub diagnostics: String,
}

impl Default for AiUiState {
    fn default() -> Self {
        Self {
            open: false,
            model_path: None,
            source_path: None,
            length: SummaryLength::Standard,
            audience: SummaryAudience::General,
            language: SummaryLanguage::SameAsDocument,
            result: String::new(),
            diagnostics: String::new(),
        }
    }
}

impl PdfMergerApp {
    pub(super) fn open_ai_dialog(&mut self) {
        if self.ai_ui.source_path.is_none() {
            self.ai_ui.source_path = self.pdf_sources().into_iter().next();
        }
        self.ai_ui.open = true;
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

    pub(super) fn show_ai_dialog(&mut self, context: &egui::Context) {
        if !self.ai_ui.open {
            return;
        }
        let sources = self.pdf_sources();
        let mut open = self.ai_ui.open;
        egui::Window::new("Local AI summarization · experimental")
            .open(&mut open)
            .resizable(true)
            .default_width(680.0)
            .show(context, |ui| {
                ui.label(
                    RichText::new("Runs locally. PDF text and summaries are never uploaded.")
                        .color(style::muted_text(ui)),
                );
                ui.separator();
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
                ui.horizontal(|ui| {
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
                ui.horizontal(|ui| {
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
                    ui.horizontal(|ui| {
                        ui.label("Language name:");
                        ui.add(
                            egui::TextEdit::singleline(language)
                                .hint_text("e.g. Spanish")
                                .char_limit(40),
                        );
                    });
                }
                let can_start = self.ai_ui.model_path.is_some()
                    && self.ai_ui.source_path.is_some()
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
                if !self.ai_ui.diagnostics.is_empty() {
                    ui.label(RichText::new(&self.ai_ui.diagnostics).color(style::muted_text(ui)));
                }
                if !self.ai_ui.result.is_empty() {
                    ui.separator();
                    ui.heading("Generated summary");
                    ui.label(
                        RichText::new("AI-generated; verify important details.")
                            .color(ui.visuals().warn_fg_color),
                    );
                    ui.add(
                        egui::TextEdit::multiline(&mut self.ai_ui.result)
                            .desired_rows(18)
                            .desired_width(f32::INFINITY),
                    );
                    if ui.button("Copy summary").clicked() {
                        ui.ctx().copy_text(self.ai_ui.result.clone());
                    }
                }
            });
        self.ai_ui.open = open;
    }

    fn start_ai_summary(&mut self, context: &egui::Context) {
        let Some(model_path) = self.ai_ui.model_path.clone() else {
            return;
        };
        let Some(source_path) = self.ai_ui.source_path.clone() else {
            return;
        };
        let selected_pages = self
            .workspace
            .pages()
            .iter()
            .filter(|page| self.selected.contains(&page.id))
            .filter_map(|page| match &page.source {
                PageSource::Pdf { path, page_number } if path == &source_path => Some(*page_number),
                _ => None,
            })
            .collect::<Vec<_>>();
        let requested_pages = (!selected_pages.is_empty()).then_some(selected_pages);
        let password = self
            .pdf_passwords
            .get(&source_path)
            .map(|password| password.to_string());
        let length = self.ai_ui.length;
        let audience = self.ai_ui.audience;
        let language = self.ai_ui.language.clone();
        self.ai_ui.result.clear();
        self.ai_ui.diagnostics.clear();
        let token = self
            .jobs
            .start("Local AI summary", JobPhase::Summarizing, 1);
        let sender = self.sender.clone();
        let repaint = context.clone();
        thread::spawn(move || {
            let extracted = extract_pdf_text(
                &source_path,
                password.as_deref(),
                requested_pages.as_deref(),
                ExtractionLimits::default(),
            );
            let result = extracted.and_then(|document| {
                let scanned_pages = document
                    .pages
                    .iter()
                    .filter(|page| !page.has_searchable_text)
                    .map(|page| page.page_number)
                    .collect::<Vec<_>>();
                let request = SummaryRequest {
                    document,
                    length,
                    audience,
                    language,
                };
                let model = ModelConfig {
                    id: model_path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("local-model")
                        .to_owned(),
                    path: model_path,
                    context_size: 8192,
                };
                let mut backend = LlamaCppBackend::new();
                let progress_sender = sender.clone();
                let progress_repaint = repaint.clone();
                run_summary_job(
                    &mut backend,
                    &model,
                    &request,
                    &|| token.is_cancelled(),
                    &mut |progress| {
                        let detail = match progress.phase {
                            SummaryPhase::LoadingModel => "Loading model",
                            SummaryPhase::Generating => "Generating summary",
                            SummaryPhase::UnloadingModel => "Unloading model",
                        };
                        let _ = progress_sender.send(AppMessage::JobProgress {
                            job_id: token.id(),
                            phase: JobPhase::Summarizing,
                            completed: progress.completed,
                            total: progress.total.max(1),
                            detail: detail.to_owned(),
                        });
                        progress_repaint.request_repaint();
                    },
                )
                .map(|(summary, diagnostics)| (summary, diagnostics, scanned_pages))
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

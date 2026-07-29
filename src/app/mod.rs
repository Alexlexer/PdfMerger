use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
};

use eframe::egui;
use pdf_merger::{
    document::ExportReport,
    export_settings::ExportSettings,
    model::{PageDraft, Workspace},
    split::SplitReport,
};

mod actions;
mod editing;
mod export_dialog;
mod page_grid;
mod password_ui;
mod project_ui;
mod split_dialog;
mod style;
mod ui;

pub(super) enum AppMessage {
    ImportFinished {
        files: usize,
        pages: Vec<PageDraft>,
        errors: Vec<String>,
        password_requests: Vec<password_ui::PasswordRequest>,
    },
    ExportFinished(Result<ExportReport, String>),
    SplitFinished(SplitReport),
    ProjectFinished {
        path: PathBuf,
        result: Result<project_ui::ProjectOpenResult, String>,
    },
}

pub struct PdfMergerApp {
    pub(super) workspace: Workspace,
    pub(super) sender: Sender<AppMessage>,
    pub(super) receiver: Receiver<AppMessage>,
    pub(super) active_jobs: usize,
    pub(super) status: String,
    pub(super) status_is_error: bool,
    pub(super) preview_textures: HashMap<u64, egui::TextureHandle>,
    pub(super) selected: HashSet<u64>,
    pub(super) split_dialog: split_dialog::SplitDialogState,
    pub(super) project_ui: project_ui::ProjectUiState,
    pub(super) export_settings: ExportSettings,
    pub(super) export_dialog: export_dialog::ExportDialogState,
    pub(super) pdf_passwords: HashMap<PathBuf, zeroize::Zeroizing<String>>,
    pub(super) password_prompt: password_ui::PasswordPromptState,
}

impl PdfMergerApp {
    pub fn new(creation_context: &eframe::CreationContext<'_>) -> Self {
        style::configure(&creation_context.egui_ctx);
        let (sender, receiver) = mpsc::channel();
        let workspace = Workspace::default();
        let export_settings = ExportSettings::default();
        let project_ui =
            project_ui::ProjectUiState::new(workspace.fingerprint(), export_settings.clone());
        let export_dialog = export_dialog::ExportDialogState::new(export_settings.clone());
        Self {
            workspace,
            sender,
            receiver,
            active_jobs: 0,
            status: "Drop PDFs or images here to begin.".to_owned(),
            status_is_error: false,
            preview_textures: HashMap::new(),
            selected: HashSet::new(),
            split_dialog: split_dialog::SplitDialogState::default(),
            project_ui,
            export_settings,
            export_dialog,
            pdf_passwords: HashMap::new(),
            password_prompt: password_ui::PasswordPromptState::default(),
        }
    }

    fn receive_messages(&mut self) {
        while let Ok(message) = self.receiver.try_recv() {
            self.active_jobs = self.active_jobs.saturating_sub(1);
            match message {
                AppMessage::ImportFinished {
                    files,
                    pages,
                    errors,
                    password_requests,
                } => {
                    let imported = pages.len();
                    let requested = password_requests.len();
                    self.workspace.append(pages);
                    self.enqueue_password_requests(password_requests);
                    if errors.is_empty() && requested == 0 {
                        self.set_status(
                            format!("Added {files} file(s) as {imported} page(s)."),
                            false,
                        );
                    } else if errors.is_empty() {
                        self.set_status(
                            format!("{requested} protected PDF(s) need a password."),
                            false,
                        );
                    } else {
                        self.set_status(errors.join("  "), true);
                    }
                }
                AppMessage::ExportFinished(result) => match result {
                    Ok(report) => {
                        let warning_suffix = if report.warnings.is_empty() {
                            String::new()
                        } else {
                            format!(" ({} warning(s))", report.warnings.len())
                        };
                        self.set_status(
                            format!(
                                "Saved {} page(s) to {}{warning_suffix}",
                                report.page_count,
                                report.path.display()
                            ),
                            false,
                        );
                    }
                    Err(error) => self.set_status(error, true),
                },
                AppMessage::SplitFinished(report) => {
                    let warning_suffix = if report.warning_count == 0 {
                        String::new()
                    } else {
                        format!("; {} warning(s)", report.warning_count)
                    };
                    if report.failures.is_empty() {
                        self.set_status(
                            format!(
                                "Created {} PDF(s) in {}{warning_suffix}",
                                report.written.len(),
                                report.directory.display()
                            ),
                            false,
                        );
                    } else {
                        self.set_status(
                            format!(
                                "Created {} PDF(s); {} failed: {}",
                                report.written.len(),
                                report.failures.len(),
                                report.failures[0]
                            ),
                            true,
                        );
                    }
                }
                AppMessage::ProjectFinished { path, result } => {
                    self.finish_project_open(path, result);
                }
            }
        }
    }

    pub(super) fn set_status(&mut self, message: impl Into<String>, is_error: bool) {
        self.status = message.into();
        self.status_is_error = is_error;
    }
}

impl eframe::App for PdfMergerApp {
    fn ui(&mut self, root_ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = root_ui.ctx().clone();
        self.receive_messages();
        self.update_project_chrome(&context);
        self.handle_shortcuts(&context);

        let dropped_paths = context.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect::<Vec<_>>()
        });
        if !dropped_paths.is_empty() {
            self.start_import(dropped_paths, &context);
        }

        self.top_bar(root_ui, &context);
        self.bottom_bar(root_ui);
        self.central_panel(root_ui, &context);
        self.show_export_dialog(&context);
        self.show_split_dialog(&context);
        self.show_project_dialogs(&context);
        self.show_password_prompt(&context);
        self.file_drop_overlay(&context);
    }
}

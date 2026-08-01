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

mod accessibility;
mod actions;
mod editing;
mod export_dialog;
mod jobs;
mod page_grid;
mod password_ui;
mod project_ui;
mod split_dialog;
mod style;
mod ui;

pub(super) enum AppMessage {
    JobProgress {
        job_id: jobs::JobId,
        phase: jobs::JobPhase,
        completed: usize,
        total: usize,
        detail: String,
    },
    ImportComplete {
        job_id: jobs::JobId,
        files: usize,
        pages: Vec<PageDraft>,
        errors: Vec<String>,
        password_requests: Vec<password_ui::PasswordRequest>,
        cancelled: bool,
    },
    ExportComplete {
        job_id: jobs::JobId,
        result: Result<ExportReport, String>,
        cancelled: bool,
    },
    SplitComplete {
        job_id: jobs::JobId,
        report: SplitReport,
        warnings: Vec<String>,
        cancelled: bool,
    },
    ProjectComplete {
        job_id: jobs::JobId,
        path: PathBuf,
        result: Result<project_ui::ProjectOpenResult, String>,
        cancelled: bool,
    },
}

pub struct PdfMergerApp {
    pub(super) workspace: Workspace,
    pub(super) sender: Sender<AppMessage>,
    pub(super) receiver: Receiver<AppMessage>,
    pub(super) jobs: jobs::JobManager,
    pub(super) status: String,
    pub(super) status_is_error: bool,
    pub(super) preview_textures: HashMap<u64, egui::TextureHandle>,
    pub(super) selected: HashSet<u64>,
    pub(super) collapsed_groups: HashSet<u64>,
    pub(super) split_dialog: split_dialog::SplitDialogState,
    pub(super) project_ui: project_ui::ProjectUiState,
    pub(super) export_settings: ExportSettings,
    pub(super) export_dialog: export_dialog::ExportDialogState,
    pub(super) pdf_passwords: HashMap<PathBuf, zeroize::Zeroizing<String>>,
    pub(super) password_prompt: password_ui::PasswordPromptState,
    modal_focus: accessibility::ModalFocusState,
    appearance: style::AppearanceSettings,
}

impl PdfMergerApp {
    pub fn new(creation_context: &eframe::CreationContext<'_>) -> Self {
        let appearance = style::configure(&creation_context.egui_ctx);
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
            jobs: jobs::JobManager::default(),
            status: "Drop PDFs or images here to begin.".to_owned(),
            status_is_error: false,
            preview_textures: HashMap::new(),
            selected: HashSet::new(),
            collapsed_groups: HashSet::new(),
            split_dialog: split_dialog::SplitDialogState::default(),
            project_ui,
            export_settings,
            export_dialog,
            pdf_passwords: HashMap::new(),
            password_prompt: password_ui::PasswordPromptState::default(),
            modal_focus: accessibility::ModalFocusState::default(),
            appearance,
        }
    }

    fn receive_messages(&mut self) {
        while let Ok(message) = self.receiver.try_recv() {
            match message {
                AppMessage::JobProgress {
                    job_id,
                    phase,
                    completed,
                    total,
                    detail,
                } => self.jobs.update(job_id, phase, completed, total, detail),
                AppMessage::ImportComplete {
                    job_id,
                    files,
                    pages,
                    errors,
                    password_requests,
                    cancelled,
                } => {
                    self.jobs.finish(job_id);
                    let imported = pages.len();
                    let requested = password_requests.len();
                    self.workspace.append(pages);
                    self.enqueue_password_requests(password_requests);
                    for error in &errors {
                        self.jobs
                            .record(jobs::DiagnosticLevel::Error, "File import failed", error);
                    }
                    if cancelled {
                        self.set_status(
                            format!("Import cancelled after adding {imported} page(s)."),
                            false,
                        );
                    } else if errors.is_empty() && requested == 0 {
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
                        self.set_status(
                            format!(
                                "Added {imported} page(s); {} file(s) failed. See Details.",
                                errors.len()
                            ),
                            true,
                        );
                    }
                }
                AppMessage::ExportComplete {
                    job_id,
                    result,
                    cancelled,
                } => {
                    self.jobs.finish(job_id);
                    if cancelled {
                        self.set_status("Export cancelled. No incomplete output was kept.", false);
                        continue;
                    }
                    match result {
                        Ok(report) => {
                            for warning in &report.warnings {
                                self.jobs.record(
                                    jobs::DiagnosticLevel::Warning,
                                    format!("Export warning for {}", report.path.display()),
                                    warning,
                                );
                            }
                            let warning_suffix = if report.warnings.is_empty() {
                                String::new()
                            } else {
                                format!(" ({} warning(s); see Details)", report.warnings.len())
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
                        Err(error) => {
                            self.jobs.record(
                                jobs::DiagnosticLevel::Error,
                                "PDF export failed",
                                &error,
                            );
                            self.set_status("PDF export failed. See Details.", true);
                        }
                    }
                }
                AppMessage::SplitComplete {
                    job_id,
                    report,
                    warnings,
                    cancelled,
                } => {
                    self.jobs.finish(job_id);
                    for failure in &report.failures {
                        self.jobs.record(
                            jobs::DiagnosticLevel::Error,
                            "Split output failed",
                            failure,
                        );
                    }
                    for warning in &warnings {
                        self.jobs.record(
                            jobs::DiagnosticLevel::Warning,
                            "Split export warning",
                            warning,
                        );
                    }
                    if cancelled {
                        self.set_status(
                            format!(
                                "Split cancelled after creating {} complete PDF(s).",
                                report.written.len()
                            ),
                            false,
                        );
                    } else if report.failures.is_empty() {
                        let suffix = if report.warning_count == 0 {
                            String::new()
                        } else {
                            format!("; {} warning(s), see Details", report.warning_count)
                        };
                        self.set_status(
                            format!(
                                "Created {} PDF(s) in {}{suffix}",
                                report.written.len(),
                                report.directory.display()
                            ),
                            false,
                        );
                    } else {
                        self.set_status(
                            format!(
                                "Created {} PDF(s); {} failed. See Details.",
                                report.written.len(),
                                report.failures.len()
                            ),
                            true,
                        );
                    }
                }
                AppMessage::ProjectComplete {
                    job_id,
                    path,
                    result,
                    cancelled,
                } => {
                    self.jobs.finish(job_id);
                    if cancelled {
                        self.set_status("Project open cancelled.", false);
                    } else {
                        if let Err(error) = &result {
                            self.jobs.record(
                                jobs::DiagnosticLevel::Error,
                                "Project open failed",
                                error,
                            );
                        }
                        self.finish_project_open(path, result);
                    }
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
        self.sync_modal_focus(&context);
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
        self.sync_modal_focus(&context);
        self.show_export_dialog(&context);
        self.show_split_dialog(&context);
        self.show_project_dialogs(&context);
        self.show_password_prompt(&context);
        self.jobs.show_details(&context);
        self.sync_modal_focus(&context);
        self.file_drop_overlay(&context);
    }
}

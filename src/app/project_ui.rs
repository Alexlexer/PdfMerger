use std::{collections::HashMap, path::PathBuf, thread};

use eframe::egui::{self, Color32, RichText};
use pdf_merger::{
    export_settings::ExportSettings,
    model::{PageDraft, PageRotation},
    project::{self, PROJECT_EXTENSION, ProjectFile},
};

use super::{
    AppMessage, PdfMergerApp,
    jobs::JobPhase,
    password_ui::{PasswordPurpose, PasswordRequest},
};

pub(crate) enum ProjectOpenResult {
    Loaded {
        pages: Vec<(PageDraft, PageRotation, Option<u64>)>,
        settings: ExportSettings,
    },
    Missing {
        project: ProjectFile,
        missing: Vec<PathBuf>,
    },
    PasswordRequired {
        source: PathBuf,
        error: pdf_merger::document::PdfAccessError,
    },
}

#[derive(Clone)]
enum PendingAction {
    ClearWorkspace,
    OpenProject(PathBuf),
    Exit,
}

struct MissingSourcesState {
    project_path: PathBuf,
    project: ProjectFile,
    missing: Vec<PathBuf>,
    replacements: HashMap<PathBuf, PathBuf>,
}

pub(crate) struct ProjectUiState {
    current_project: Option<PathBuf>,
    saved_fingerprint: u64,
    saved_export_settings: ExportSettings,
    recent_projects: Vec<PathBuf>,
    pending_action: Option<PendingAction>,
    missing_sources: Option<MissingSourcesState>,
    pending_focus_requested: bool,
    missing_focus_requested: bool,
    allow_close: bool,
}

impl ProjectUiState {
    pub(crate) fn new(saved_fingerprint: u64, saved_export_settings: ExportSettings) -> Self {
        Self {
            current_project: None,
            saved_fingerprint,
            saved_export_settings,
            recent_projects: project::load_recent_projects(),
            pending_action: None,
            missing_sources: None,
            pending_focus_requested: false,
            missing_focus_requested: false,
            allow_close: false,
        }
    }

    pub(crate) fn has_open_dialog(&self) -> bool {
        self.pending_action.is_some() || self.missing_sources.is_some()
    }
}

impl PdfMergerApp {
    pub(super) fn is_project_dirty(&self) -> bool {
        self.workspace.fingerprint() != self.project_ui.saved_fingerprint
            || self.export_settings != self.project_ui.saved_export_settings
    }

    pub(super) fn update_project_chrome(&mut self, context: &egui::Context) {
        let dirty = self.is_project_dirty();
        let project_name = self
            .project_ui
            .current_project
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("Untitled");
        context.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "PdfMerger — {project_name}{}",
            if dirty { " *" } else { "" }
        )));

        let close_requested = context.input(|input| input.viewport().close_requested());
        if close_requested && dirty && !self.project_ui.allow_close {
            context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.project_ui.pending_action.is_none() {
                self.project_ui.pending_focus_requested = true;
            }
            self.project_ui.pending_action = Some(PendingAction::Exit);
        }
    }

    pub(super) fn project_menu(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        let recent = self.project_ui.recent_projects.clone();
        ui.menu_button("Project", |ui| {
            if ui.button("New workspace").clicked() {
                ui.close();
                self.request_clear_workspace();
            }
            if ui.button("Open project…").clicked() {
                ui.close();
                self.choose_open_project(context);
            }
            ui.separator();
            if ui
                .add_enabled(
                    !self.workspace.is_empty(),
                    egui::Button::new("Save project"),
                )
                .clicked()
            {
                ui.close();
                self.save_project(false);
            }
            if ui
                .add_enabled(
                    !self.workspace.is_empty(),
                    egui::Button::new("Save project as…"),
                )
                .clicked()
            {
                ui.close();
                self.save_project(true);
            }

            if !recent.is_empty() {
                ui.separator();
                ui.label(RichText::new("Recent projects").strong());
                for path in recent {
                    let label = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("Project");
                    if ui
                        .button(label)
                        .on_hover_text(path.display().to_string())
                        .clicked()
                    {
                        ui.close();
                        self.request_open_project(path, context);
                    }
                }
            }
        });
    }

    pub(super) fn show_project_dialogs(&mut self, context: &egui::Context) {
        self.show_discard_prompt(context);
        self.show_missing_sources(context);
    }

    pub(super) fn finish_project_open(
        &mut self,
        path: PathBuf,
        result: Result<ProjectOpenResult, String>,
    ) {
        match result {
            Err(error) => self.set_status(error, true),
            Ok(ProjectOpenResult::Loaded { pages, settings }) => {
                let page_count = pages.len();
                self.workspace.replace_project_pages_grouped(pages);
                self.selected.clear();
                self.preview_textures.clear();
                self.collapsed_groups.clear();
                self.project_ui.current_project = Some(path.clone());
                self.export_settings = settings.clone();
                self.project_ui.saved_fingerprint = self.workspace.fingerprint();
                self.project_ui.saved_export_settings = settings;
                self.record_recent_project(path.clone());
                self.set_status(
                    format!("Opened {page_count} page(s) from {}", path.display()),
                    false,
                );
            }
            Ok(ProjectOpenResult::PasswordRequired { source, error }) => {
                self.enqueue_password_requests([PasswordRequest {
                    path: source,
                    error,
                    purpose: PasswordPurpose::OpenProject(path),
                }]);
                self.set_status("A protected project source needs a password.", false);
            }
            Ok(ProjectOpenResult::Missing { project, missing }) => {
                let count = missing.len();
                self.project_ui.missing_sources = Some(MissingSourcesState {
                    project_path: path,
                    project,
                    missing,
                    replacements: HashMap::new(),
                });
                self.project_ui.missing_focus_requested = true;
                self.set_status(
                    format!("Project needs {count} missing source file(s)."),
                    true,
                );
            }
        }
    }

    pub(super) fn request_clear_workspace(&mut self) {
        if self.is_project_dirty() {
            self.project_ui.pending_focus_requested = true;
            self.project_ui.pending_action = Some(PendingAction::ClearWorkspace);
        } else {
            self.clear_to_new_workspace();
        }
    }

    pub(super) fn choose_open_project(&mut self, context: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PdfMerger project", &[PROJECT_EXTENSION])
            .pick_file()
        else {
            return;
        };
        self.request_open_project(path, context);
    }

    fn request_open_project(&mut self, path: PathBuf, context: &egui::Context) {
        if self.is_project_dirty() {
            self.project_ui.pending_focus_requested = true;
            self.project_ui.pending_action = Some(PendingAction::OpenProject(path));
        } else {
            self.start_project_open(path, context);
        }
    }

    pub(super) fn save_project(&mut self, save_as: bool) -> bool {
        if self.workspace.is_empty() {
            self.set_status("Add at least one page before saving a project.", true);
            return false;
        }
        let path = if !save_as {
            self.project_ui.current_project.clone()
        } else {
            None
        }
        .or_else(|| {
            rfd::FileDialog::new()
                .add_filter("PdfMerger project", &[PROJECT_EXTENSION])
                .set_file_name("layout.pdfmerger")
                .save_file()
        });
        let Some(path) = path else {
            return false;
        };
        let path = if path.extension().and_then(|extension| extension.to_str())
            == Some(PROJECT_EXTENSION)
        {
            path
        } else {
            path.with_extension(PROJECT_EXTENSION)
        };

        match project::save_project(&path, self.workspace.pages(), &self.export_settings) {
            Ok(()) => {
                self.project_ui.current_project = Some(path.clone());
                self.project_ui.saved_fingerprint = self.workspace.fingerprint();
                self.project_ui.saved_export_settings = self.export_settings.clone();
                self.record_recent_project(path.clone());
                self.set_status(format!("Saved project to {}", path.display()), false);
                true
            }
            Err(error) => {
                self.set_status(format!("{error:#}"), true);
                false
            }
        }
    }

    pub(super) fn start_project_open(&mut self, path: PathBuf, context: &egui::Context) {
        let passwords = self.passwords_for_worker();
        let token = self.jobs.start(
            format!(
                "Open {}",
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("project")
            ),
            JobPhase::OpeningProject,
            0,
        );
        let sender = self.sender.clone();
        let context = context.clone();
        let message_path = path.clone();
        self.set_status(format!("Opening project {}…", path.display()), false);
        thread::spawn(move || {
            let progress_sender = sender.clone();
            let progress_context = context.clone();
            let mut progress = |completed: usize, total: usize, source: &std::path::Path| {
                let _ = progress_sender.send(AppMessage::JobProgress {
                    job_id: token.id(),
                    phase: JobPhase::Importing,
                    completed,
                    total,
                    detail: source.display().to_string(),
                });
                progress_context.request_repaint();
            };
            let result = (|| {
                if token.is_cancelled() {
                    anyhow::bail!("project open cancelled");
                }
                let project = project::read_project(&path)?;
                let replacements = HashMap::new();
                let missing = project::missing_sources(&path, &project, &replacements);
                if missing.is_empty() {
                    let settings = project.export.clone();
                    match project::materialize_project_with_passwords_controlled(
                        &path,
                        &project,
                        &replacements,
                        &passwords,
                        &mut progress,
                        &|| token.is_cancelled(),
                    ) {
                        Ok(pages) => Ok(ProjectOpenResult::Loaded { pages, settings }),
                        Err(project::MaterializeFailure::Access {
                            path,
                            error:
                                pdf_merger::document::PdfAccessError::UnsupportedEncryption(error),
                        }) => Err(anyhow::anyhow!(
                            "{}: unsupported PDF encryption: {error}",
                            path.display()
                        )),
                        Err(project::MaterializeFailure::Access { path, error }) => {
                            Ok(ProjectOpenResult::PasswordRequired {
                                source: path,
                                error,
                            })
                        }
                        Err(project::MaterializeFailure::Other(error)) => Err(error),
                    }
                } else {
                    Ok(ProjectOpenResult::Missing { project, missing })
                }
            })()
            .map_err(|error: anyhow::Error| format!("{error:#}"));
            let _ = sender.send(AppMessage::ProjectComplete {
                job_id: token.id(),
                path: message_path,
                result,
                cancelled: token.is_cancelled(),
            });
            context.request_repaint();
        });
    }
    fn start_project_recovery(&mut self, state: MissingSourcesState, context: &egui::Context) {
        let passwords = self.passwords_for_worker();
        let token = self
            .jobs
            .start("Restore project sources", JobPhase::OpeningProject, 0);
        let sender = self.sender.clone();
        let context = context.clone();
        let path = state.project_path.clone();
        let message_path = path.clone();
        self.set_status("Restoring project sources…", false);
        thread::spawn(move || {
            let progress_sender = sender.clone();
            let progress_context = context.clone();
            let mut progress = |completed: usize, total: usize, source: &std::path::Path| {
                let _ = progress_sender.send(AppMessage::JobProgress {
                    job_id: token.id(),
                    phase: JobPhase::Importing,
                    completed,
                    total,
                    detail: source.display().to_string(),
                });
                progress_context.request_repaint();
            };
            let settings = state.project.export.clone();
            let result = match project::materialize_project_with_passwords_controlled(
                &path,
                &state.project,
                &state.replacements,
                &passwords,
                &mut progress,
                &|| token.is_cancelled(),
            ) {
                Ok(pages) => Ok(ProjectOpenResult::Loaded { pages, settings }),
                Err(project::MaterializeFailure::Access {
                    path,
                    error: pdf_merger::document::PdfAccessError::UnsupportedEncryption(error),
                }) => Err(format!(
                    "{}: unsupported PDF encryption: {error}",
                    path.display()
                )),
                Err(project::MaterializeFailure::Access { path, error }) => {
                    Ok(ProjectOpenResult::PasswordRequired {
                        source: path,
                        error,
                    })
                }
                Err(project::MaterializeFailure::Other(error)) => Err(format!("{error:#}")),
            };
            let _ = sender.send(AppMessage::ProjectComplete {
                job_id: token.id(),
                path: message_path,
                result,
                cancelled: token.is_cancelled(),
            });
            context.request_repaint();
        });
    }
    fn show_discard_prompt(&mut self, context: &egui::Context) {
        let Some(pending) = self.project_ui.pending_action.clone() else {
            return;
        };
        let mut save = false;
        let mut discard = false;
        let mut cancel = false;
        let modal =
            egui::Modal::new(egui::Id::new("discard_project_changes")).show(context, |ui| {
                ui.set_width(430.0);
                ui.heading("Unsaved project changes");
                ui.separator();
                ui.label("Save your project before continuing?");
                ui.label(
                    RichText::new("Unsaved page order, rotation, and removals will be lost.")
                        .color(Color32::from_gray(150)),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    if ui.button("Discard changes").clicked() {
                        discard = true;
                    }
                    let save_project = ui.button(RichText::new("Save project").strong());
                    if self.project_ui.pending_focus_requested {
                        save_project.request_focus();
                        self.project_ui.pending_focus_requested = false;
                    }
                    if save_project.clicked() {
                        save = true;
                    }
                });
            });
        if modal.should_close() {
            cancel = true;
        } else if context
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
        {
            save = true;
        }
        if cancel {
            self.project_ui.pending_action = None;
        } else if discard || (save && self.save_project(false)) {
            self.project_ui.pending_action = None;
            self.execute_pending_action(pending, context);
        }
    }

    fn show_missing_sources(&mut self, context: &egui::Context) {
        let Some(mut state) = self.project_ui.missing_sources.take() else {
            return;
        };
        let mut cancel = false;
        let mut locate = None;
        let mut retry = false;
        let ready = state
            .missing
            .iter()
            .all(|path| state.replacements.contains_key(path));
        let modal =
            egui::Modal::new(egui::Id::new("missing_project_sources")).show(context, |ui| {
                ui.set_width(520.0);
                ui.heading("Locate missing project sources");
                ui.separator();
                ui.label("Choose a replacement for each missing source file.");
                ui.add_space(8.0);
                for (index, missing) in state.missing.iter().enumerate() {
                    ui.group(|ui| {
                        ui.label(RichText::new(missing.display().to_string()).strong());
                        if let Some(replacement) = state.replacements.get(missing) {
                            ui.label(
                                RichText::new(format!("→ {}", replacement.display()))
                                    .color(Color32::from_rgb(120, 200, 145)),
                            );
                        }
                        let locate_button = ui.small_button("Locate…");
                        if self.project_ui.missing_focus_requested {
                            locate_button.request_focus();
                            self.project_ui.missing_focus_requested = false;
                        }
                        if locate_button.clicked() {
                            locate = Some(index);
                        }
                    });
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    if ui
                        .add_enabled(ready, egui::Button::new("Retry project open"))
                        .clicked()
                    {
                        retry = true;
                    }
                });
            });
        if modal.should_close() {
            cancel = true;
        } else if ready
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
        {
            retry = true;
        }

        if let Some(index) = locate
            && let Some(replacement) = rfd::FileDialog::new().pick_file()
        {
            state
                .replacements
                .insert(state.missing[index].clone(), replacement);
        }
        if retry {
            self.start_project_recovery(state, context);
        } else if !cancel {
            self.project_ui.missing_sources = Some(state);
        }
    }

    fn execute_pending_action(&mut self, action: PendingAction, context: &egui::Context) {
        match action {
            PendingAction::ClearWorkspace => self.clear_to_new_workspace(),
            PendingAction::OpenProject(path) => self.start_project_open(path, context),
            PendingAction::Exit => {
                self.project_ui.allow_close = true;
                context.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn clear_to_new_workspace(&mut self) {
        self.workspace.replace_project_pages([]);
        self.preview_textures.clear();
        self.collapsed_groups.clear();
        self.selected.clear();
        self.project_ui.current_project = None;
        self.export_settings = ExportSettings::default();
        self.project_ui.saved_fingerprint = self.workspace.fingerprint();
        self.project_ui.saved_export_settings = self.export_settings.clone();
        self.set_status("Started a new workspace.", false);
    }

    fn record_recent_project(&mut self, path: PathBuf) {
        self.project_ui
            .recent_projects
            .retain(|recent| recent != &path);
        self.project_ui.recent_projects.insert(0, path);
        self.project_ui.recent_projects.truncate(8);
        if let Err(error) = project::save_recent_projects(&self.project_ui.recent_projects) {
            self.set_status(
                format!("Project saved, but recent projects could not be updated: {error:#}"),
                true,
            );
        }
    }
}

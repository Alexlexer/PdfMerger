use std::{collections::HashMap, path::PathBuf, thread};

use eframe::egui::{self, Color32, RichText};
use pdf_merger::{
    export_settings::ExportSettings,
    model::{PageDraft, PageRotation},
    project::{self, PROJECT_EXTENSION, ProjectFile},
};

use super::{AppMessage, PdfMergerApp};

pub(crate) enum ProjectOpenResult {
    Loaded {
        pages: Vec<(PageDraft, PageRotation)>,
        settings: ExportSettings,
    },
    Missing {
        project: ProjectFile,
        missing: Vec<PathBuf>,
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
            allow_close: false,
        }
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
                self.workspace.replace_project_pages(pages);
                self.selected.clear();
                self.preview_textures.clear();
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
            Ok(ProjectOpenResult::Missing { project, missing }) => {
                let count = missing.len();
                self.project_ui.missing_sources = Some(MissingSourcesState {
                    project_path: path,
                    project,
                    missing,
                    replacements: HashMap::new(),
                });
                self.set_status(
                    format!("Project needs {count} missing source file(s)."),
                    true,
                );
            }
        }
    }

    pub(super) fn request_clear_workspace(&mut self) {
        if self.is_project_dirty() {
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

    fn start_project_open(&mut self, path: PathBuf, context: &egui::Context) {
        let sender = self.sender.clone();
        let context = context.clone();
        let message_path = path.clone();
        self.active_jobs += 1;
        self.set_status(format!("Opening project {}…", path.display()), false);
        thread::spawn(move || {
            let result = (|| {
                let project = project::read_project(&path)?;
                let replacements = HashMap::new();
                let missing = project::missing_sources(&path, &project, &replacements);
                if missing.is_empty() {
                    let settings = project.export.clone();
                    Ok(ProjectOpenResult::Loaded {
                        pages: project::materialize_project(&path, &project, &replacements)?,
                        settings,
                    })
                } else {
                    Ok(ProjectOpenResult::Missing { project, missing })
                }
            })()
            .map_err(|error: anyhow::Error| format!("{error:#}"));
            let _ = sender.send(AppMessage::ProjectFinished {
                path: message_path,
                result,
            });
            context.request_repaint();
        });
    }

    fn start_project_recovery(&mut self, state: MissingSourcesState, context: &egui::Context) {
        let sender = self.sender.clone();
        let context = context.clone();
        let path = state.project_path.clone();
        let message_path = path.clone();
        self.active_jobs += 1;
        self.set_status("Restoring project sources…", false);
        thread::spawn(move || {
            let settings = state.project.export.clone();
            let result = project::materialize_project(&path, &state.project, &state.replacements)
                .map(|pages| ProjectOpenResult::Loaded { pages, settings })
                .map_err(|error| format!("{error:#}"));
            let _ = sender.send(AppMessage::ProjectFinished {
                path: message_path,
                result,
            });
            context.request_repaint();
        });
    }

    fn show_discard_prompt(&mut self, context: &egui::Context) {
        let Some(pending) = self.project_ui.pending_action.clone() else {
            return;
        };
        let mut open = true;
        let mut save = false;
        let mut discard = false;
        let mut cancel = false;
        egui::Window::new("Unsaved project changes")
            .id(egui::Id::new("discard_project_changes"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(context, |ui| {
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
                    if ui.button(RichText::new("Save project").strong()).clicked() {
                        save = true;
                    }
                });
            });
        if !open || cancel {
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
        let mut open = true;
        let mut cancel = false;
        let mut locate = None;
        let mut retry = false;
        egui::Window::new("Locate missing project sources")
            .id(egui::Id::new("missing_project_sources"))
            .open(&mut open)
            .collapsible(false)
            .default_width(520.0)
            .show(context, |ui| {
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
                        if ui.small_button("Locate…").clicked() {
                            locate = Some(index);
                        }
                    });
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    let ready = state
                        .missing
                        .iter()
                        .all(|path| state.replacements.contains_key(path));
                    if ui
                        .add_enabled(ready, egui::Button::new("Retry project open"))
                        .clicked()
                    {
                        retry = true;
                    }
                });
            });

        if let Some(index) = locate
            && let Some(replacement) = rfd::FileDialog::new().pick_file()
        {
            state
                .replacements
                .insert(state.missing[index].clone(), replacement);
        }
        if retry {
            self.start_project_recovery(state, context);
        } else if open && !cancel {
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

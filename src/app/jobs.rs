use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use eframe::egui::{self, Color32, RichText};

pub(crate) type JobId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JobPhase {
    Importing,
    Exporting,
    OpeningProject,
}

impl JobPhase {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Importing => "Importing",
            Self::Exporting => "Exporting",
            Self::OpeningProject => "Opening project",
        }
    }
}

#[derive(Clone)]
pub(crate) struct JobToken {
    id: JobId,
    cancelled: Arc<AtomicBool>,
}

impl JobToken {
    pub(crate) fn id(&self) -> JobId {
        self.id
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

#[derive(Clone)]
pub(crate) struct JobView {
    pub id: JobId,
    pub title: String,
    pub phase: JobPhase,
    pub completed: usize,
    pub total: usize,
    pub detail: String,
    pub cancelling: bool,
}

struct JobState {
    view: JobView,
    cancelled: Arc<AtomicBool>,
}

#[derive(Clone, Copy)]
pub(crate) enum DiagnosticLevel {
    Warning,
    Error,
}

struct Diagnostic {
    level: DiagnosticLevel,
    summary: String,
    details: String,
}

#[derive(Default)]
pub(crate) struct JobManager {
    next_id: JobId,
    active: BTreeMap<JobId, JobState>,
    diagnostics: VecDeque<Diagnostic>,
    details_open: bool,
    details_focus_requested: bool,
}

impl JobManager {
    pub(crate) fn start(
        &mut self,
        title: impl Into<String>,
        phase: JobPhase,
        total: usize,
    ) -> JobToken {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let id = self.next_id;
        let cancelled = Arc::new(AtomicBool::new(false));
        self.active.insert(
            id,
            JobState {
                view: JobView {
                    id,
                    title: title.into(),
                    phase,
                    completed: 0,
                    total,
                    detail: String::new(),
                    cancelling: false,
                },
                cancelled: cancelled.clone(),
            },
        );
        JobToken { id, cancelled }
    }

    pub(crate) fn update(
        &mut self,
        id: JobId,
        phase: JobPhase,
        completed: usize,
        total: usize,
        detail: String,
    ) {
        if let Some(job) = self.active.get_mut(&id) {
            job.view.phase = phase;
            job.view.completed = completed.min(total);
            job.view.total = total;
            job.view.detail = detail;
        }
    }

    pub(crate) fn finish(&mut self, id: JobId) {
        self.active.remove(&id);
    }

    pub(crate) fn cancel(&mut self, id: JobId) {
        if let Some(job) = self.active.get_mut(&id) {
            job.cancelled.store(true, Ordering::Relaxed);
            job.view.cancelling = true;
            job.view.detail = "Waiting for the current document operation to stop…".to_owned();
        }
    }

    pub(crate) fn primary(&self) -> Option<JobView> {
        self.active.values().next_back().map(|job| job.view.clone())
    }

    pub(crate) fn active_count(&self) -> usize {
        self.active.len()
    }

    pub(crate) fn has_diagnostics(&self) -> bool {
        !self.diagnostics.is_empty()
    }

    pub(crate) fn open_details(&mut self) {
        self.details_open = true;
        self.details_focus_requested = true;
    }

    pub(crate) fn details_are_open(&self) -> bool {
        self.details_open
    }

    pub(crate) fn record(
        &mut self,
        level: DiagnosticLevel,
        summary: impl Into<String>,
        details: impl Into<String>,
    ) {
        const LIMIT: usize = 200;
        if self.diagnostics.len() == LIMIT {
            self.diagnostics.pop_front();
        }
        self.diagnostics.push_back(Diagnostic {
            level,
            summary: summary.into(),
            details: details.into(),
        });
    }

    fn details_text(&self) -> String {
        self.diagnostics
            .iter()
            .map(|entry| {
                let level = match entry.level {
                    DiagnosticLevel::Warning => "WARNING",
                    DiagnosticLevel::Error => "ERROR",
                };
                if entry.details.is_empty() || entry.details == entry.summary {
                    format!("[{level}] {}", entry.summary)
                } else {
                    format!("[{level}] {}\n{}", entry.summary, entry.details)
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub(crate) fn show_details(&mut self, context: &egui::Context) {
        if !self.details_open {
            return;
        }
        let mut open = self.details_open;
        let mut clear = false;
        let mut details = self.details_text();
        let modal = egui::Modal::new(egui::Id::new("job_diagnostics")).show(context, |ui| {
            ui.set_width(620.0);
            ui.heading("Warnings and errors");
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{} diagnostic(s)", self.diagnostics.len()))
                        .color(Color32::from_gray(155)),
                );
                let copy_all = ui.button("Copy all");
                if self.details_focus_requested {
                    copy_all.request_focus();
                    self.details_focus_requested = false;
                }
                if copy_all.clicked() {
                    ui.ctx().copy_text(details.clone());
                }
                if ui.button("Clear").clicked() {
                    clear = true;
                }
            });
            ui.separator();
            ui.add(
                egui::TextEdit::multiline(&mut details)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(20)
                    .interactive(false),
            );
        });
        if modal.should_close() {
            open = false;
        }
        if clear {
            self.diagnostics.clear();
        }
        self.details_open = open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_progress_and_cancellation_by_job_id() {
        let mut jobs = JobManager::default();
        let token = jobs.start("Import", JobPhase::Importing, 3);
        jobs.update(token.id(), JobPhase::Importing, 2, 3, "two.pdf".to_owned());
        let view = jobs.primary().unwrap();
        assert_eq!(view.id, token.id());
        assert_eq!((view.completed, view.total), (2, 3));

        jobs.cancel(token.id());
        assert!(token.is_cancelled());
        assert!(jobs.primary().unwrap().cancelling);
        jobs.finish(token.id());
        assert!(jobs.primary().is_none());
    }
}

use std::{collections::HashSet, path::PathBuf, thread};

use eframe::egui;
use pdf_merger::document;

use super::{
    AppMessage, PdfMergerApp,
    export_dialog::ExportTarget,
    jobs::JobPhase,
    password_ui::{PasswordPurpose, PasswordRequest},
};

impl PdfMergerApp {
    pub(super) fn choose_files(&mut self, context: &egui::Context) {
        if let Some(paths) = rfd::FileDialog::new()
            .add_filter(
                "PDFs and images",
                &[
                    "pdf", "png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff",
                ],
            )
            .pick_files()
        {
            self.start_import(paths, context);
        }
    }

    pub(super) fn start_import(&mut self, paths: Vec<PathBuf>, context: &egui::Context) {
        let mut unique_paths = HashSet::new();
        let paths = paths
            .into_iter()
            .filter(|path| document::is_supported(path))
            .filter(|path| unique_paths.insert(path.clone()))
            .collect::<Vec<_>>();
        if paths.is_empty() {
            self.set_status("No supported PDF or image files were selected.", true);
            return;
        }

        let file_count = paths.len();
        let passwords = self.passwords_for_worker();
        let token = self
            .jobs
            .start("Import files", JobPhase::Importing, file_count);
        self.set_status(format!("Importing {file_count} file(s)…"), false);
        let sender = self.sender.clone();
        let context = context.clone();
        thread::spawn(move || {
            let mut pages = Vec::new();
            let mut errors = Vec::new();
            let mut password_requests = Vec::new();
            let mut completed = 0;
            for path in paths {
                if token.is_cancelled() {
                    break;
                }
                match document::import_file_with_password(
                    &path,
                    passwords.get(&path).map(|password| password.as_str()),
                ) {
                    Ok(mut imported) => pages.append(&mut imported),
                    Err(document::ImportFailure::Access(
                        document::PdfAccessError::UnsupportedEncryption(error),
                    )) => {
                        errors.push(format!(
                            "{}: unsupported PDF encryption: {error}",
                            path.display()
                        ));
                    }
                    Err(document::ImportFailure::Access(error)) => {
                        password_requests.push(PasswordRequest {
                            path: path.clone(),
                            error,
                            purpose: PasswordPurpose::Import,
                        });
                    }
                    Err(document::ImportFailure::Other(error)) => {
                        errors.push(format!("{}: {error:#}", path.display()));
                    }
                }
                completed += 1;
                let _ = sender.send(AppMessage::JobProgress {
                    job_id: token.id(),
                    phase: JobPhase::Importing,
                    completed,
                    total: file_count,
                    detail: path.display().to_string(),
                });
                context.request_repaint();
            }
            let _ = sender.send(AppMessage::ImportComplete {
                job_id: token.id(),
                files: completed,
                pages,
                errors,
                password_requests,
                cancelled: token.is_cancelled(),
            });
            context.request_repaint();
        });
    }

    pub(super) fn choose_export_path(&mut self, _context: &egui::Context) {
        self.open_export_dialog(ExportTarget::AllPages);
    }

    pub(super) fn choose_selected_export_path(&mut self, _context: &egui::Context) {
        self.open_export_dialog(ExportTarget::SelectedPages);
    }

    pub(super) fn choose_export_pages(
        &mut self,
        context: &egui::Context,
        pages: Vec<pdf_merger::model::PageItem>,
        suggested_name: &str,
    ) {
        if pages.is_empty() {
            self.set_status("Select or add at least one page before exporting.", true);
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PDF document", &["pdf"])
            .set_file_name(suggested_name)
            .save_file()
        else {
            return;
        };

        let path = if path.extension().is_none() {
            path.with_extension("pdf")
        } else {
            path
        };
        let settings = self.export_settings.clone();
        let passwords = self.passwords_for_worker();
        let page_count = pages.len();
        let token = self
            .jobs
            .start("Export PDF", JobPhase::Exporting, page_count);
        let sender = self.sender.clone();
        let context = context.clone();
        self.set_status(format!("Building PDF from {page_count} page(s)…"), false);
        thread::spawn(move || {
            let progress_sender = sender.clone();
            let progress_context = context.clone();
            let mut progress = |completed, total| {
                let _ = progress_sender.send(AppMessage::JobProgress {
                    job_id: token.id(),
                    phase: JobPhase::Exporting,
                    completed,
                    total,
                    detail: format!("Page {completed} of {total}"),
                });
                progress_context.request_repaint();
            };
            let result = document::export_pages_with_settings_and_passwords_controlled(
                &pages,
                &path,
                &settings,
                &passwords,
                &mut progress,
                &|| token.is_cancelled(),
            )
            .map_err(|error| format!("{error:#}"));
            let cancelled = token.is_cancelled() && result.is_err();
            let _ = sender.send(AppMessage::ExportComplete {
                job_id: token.id(),
                result,
                cancelled,
            });
            context.request_repaint();
        });
    }
}

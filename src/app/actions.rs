use std::{collections::HashSet, path::PathBuf, thread};

use eframe::egui;
use pdf_merger::document;

use super::{AppMessage, PdfMergerApp, export_dialog::ExportTarget};

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
        self.active_jobs += 1;
        self.set_status(format!("Importing {file_count} file(s)…"), false);
        let sender = self.sender.clone();
        let context = context.clone();
        thread::spawn(move || {
            let mut pages = Vec::new();
            let mut errors = Vec::new();
            for path in paths {
                match document::import_file(&path) {
                    Ok(mut imported) => pages.append(&mut imported),
                    Err(error) => errors.push(format!("{}: {error:#}", path.display())),
                }
            }
            let _ = sender.send(AppMessage::ImportFinished {
                files: file_count,
                pages,
                errors,
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
        let sender = self.sender.clone();
        let context = context.clone();
        self.active_jobs += 1;
        self.set_status(format!("Building PDF from {} page(s)…", pages.len()), false);
        thread::spawn(move || {
            let result = document::export_pages_with_settings(&pages, &path, &settings)
                .map_err(|error| format!("{error:#}"));
            let _ = sender.send(AppMessage::ExportFinished(result));
            context.request_repaint();
        });
    }
}

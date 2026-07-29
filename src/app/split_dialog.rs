use std::{fs, thread};

use eframe::egui::{self, Color32, RichText};
use pdf_merger::{
    document,
    split::{self, PlannedSplit, SplitMode, SplitReport},
};

use super::{AppMessage, PdfMergerApp};

pub(crate) struct SplitDialogState {
    open: bool,
    mode: SplitMode,
    range_spec: String,
    base_name: String,
    error: Option<String>,
}

impl Default for SplitDialogState {
    fn default() -> Self {
        Self {
            open: false,
            mode: SplitMode::IndividualPages,
            range_spec: "1-3, 4-6".to_owned(),
            base_name: "split".to_owned(),
            error: None,
        }
    }
}

impl PdfMergerApp {
    pub(super) fn open_split_dialog(&mut self) {
        if self.selected.is_empty() {
            self.set_status("Select the pages to split first.", true);
            return;
        }
        self.split_dialog.open = true;
        self.split_dialog.range_spec = if self.selected.len() == 1 {
            "1".to_owned()
        } else {
            format!("1-{}", self.selected.len())
        };
        self.split_dialog.error = None;
    }

    pub(super) fn show_split_dialog(&mut self, context: &egui::Context) {
        if !self.split_dialog.open {
            return;
        }

        let selected_count = self.selected.len();
        let mut dialog = std::mem::take(&mut self.split_dialog);
        let mut open = dialog.open;
        let mut cancel = false;
        let mut request_export = false;

        egui::Window::new("Split selected pages")
            .id(egui::Id::new("split_dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(430.0)
            .show(context, |ui| {
                ui.label(format!(
                    "Create multiple PDFs from {selected_count} selected page(s)."
                ));
                ui.add_space(8.0);
                ui.label(RichText::new("Split mode").strong());
                ui.radio_value(
                    &mut dialog.mode,
                    SplitMode::IndividualPages,
                    "One PDF per page",
                );
                ui.radio_value(
                    &mut dialog.mode,
                    SplitMode::SourceDocuments,
                    "One PDF per original source file",
                );
                ui.radio_value(&mut dialog.mode, SplitMode::Ranges, "One PDF per range");

                if dialog.mode == SplitMode::Ranges {
                    ui.indent("range_options", |ui| {
                        ui.label("Positions within the selected pages:");
                        ui.text_edit_singleline(&mut dialog.range_spec);
                        ui.label(
                            RichText::new("Example: 1-3, 5, 7-9")
                                .small()
                                .color(Color32::from_gray(145)),
                        );
                    });
                }

                ui.add_space(8.0);
                ui.label(RichText::new("Base filename").strong());
                ui.text_edit_singleline(&mut dialog.base_name);
                ui.label(
                    RichText::new("Existing files will never be overwritten.")
                        .small()
                        .color(Color32::from_gray(145)),
                );

                if let Some(error) = &dialog.error {
                    ui.add_space(8.0);
                    ui.label(RichText::new(error).color(Color32::from_rgb(244, 118, 118)));
                }

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    if ui
                        .button(RichText::new("Choose folder and export").strong())
                        .clicked()
                    {
                        request_export = true;
                    }
                });
            });

        dialog.open = open && !cancel;
        if request_export {
            let pages = self
                .workspace
                .pages()
                .iter()
                .filter(|page| self.selected.contains(&page.id))
                .cloned()
                .collect::<Vec<_>>();
            match split::build_groups(dialog.mode, &pages, &dialog.range_spec, &dialog.base_name) {
                Err(error) => dialog.error = Some(error.to_string()),
                Ok(groups) => {
                    if let Some(directory) = rfd::FileDialog::new().pick_folder() {
                        match split::plan_outputs(&directory, groups) {
                            Err(error) => dialog.error = Some(error.to_string()),
                            Ok(planned) => {
                                dialog.open = false;
                                dialog.error = None;
                                self.start_split_export(context, directory, planned);
                            }
                        }
                    }
                }
            }
        }

        self.split_dialog = dialog;
    }

    fn start_split_export(
        &mut self,
        context: &egui::Context,
        directory: std::path::PathBuf,
        planned: Vec<PlannedSplit>,
    ) {
        let output_count = planned.len();
        let settings = self.export_settings.clone();
        let passwords = self.passwords_for_worker();
        let sender = self.sender.clone();
        let context = context.clone();
        self.active_jobs += 1;
        self.set_status(format!("Creating {output_count} split PDF(s)…"), false);

        thread::spawn(move || {
            let mut report = SplitReport {
                directory,
                written: Vec::new(),
                failures: Vec::new(),
                warning_count: 0,
            };
            for output in planned {
                if output.path.exists() {
                    report.failures.push(format!(
                        "{} appeared after validation and was not overwritten",
                        output.path.display()
                    ));
                    continue;
                }
                match document::export_pages_with_settings_and_passwords(
                    &output.pages,
                    &output.path,
                    &settings,
                    &passwords,
                ) {
                    Ok(export) => {
                        report.warning_count += export.warnings.len();
                        report.written.push(output.path);
                    }
                    Err(error) => {
                        let _ = fs::remove_file(&output.path);
                        report
                            .failures
                            .push(format!("{}: {error:#}", output.path.display()));
                    }
                }
            }
            let _ = sender.send(AppMessage::SplitFinished(report));
            context.request_repaint();
        });
    }
}

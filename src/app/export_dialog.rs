use eframe::egui::{self, Color32, RichText};
use pdf_merger::export_settings::{ExportPreset, ExportSettings, ImagePagePolicy};

use super::PdfMergerApp;

#[derive(Clone, Copy, Debug)]
pub(crate) enum ExportTarget {
    AllPages,
    SelectedPages,
}

pub(crate) struct ExportDialogState {
    open: bool,
    target: ExportTarget,
    draft: ExportSettings,
    error: Option<String>,
}

impl ExportDialogState {
    pub(crate) fn new(settings: ExportSettings) -> Self {
        Self {
            open: false,
            target: ExportTarget::AllPages,
            draft: settings,
            error: None,
        }
    }
}

impl PdfMergerApp {
    pub(super) fn open_export_dialog(&mut self, target: ExportTarget) {
        let has_pages = match target {
            ExportTarget::AllPages => !self.workspace.is_empty(),
            ExportTarget::SelectedPages => !self.selected.is_empty(),
        };
        if !has_pages {
            self.set_status("Select or add at least one page before exporting.", true);
            return;
        }
        self.export_dialog.target = target;
        self.export_dialog.draft = self.export_settings.clone();
        self.export_dialog.error = None;
        self.export_dialog.open = true;
    }

    pub(super) fn show_export_dialog(&mut self, context: &egui::Context) {
        if !self.export_dialog.open {
            return;
        }
        let mut dialog = std::mem::replace(
            &mut self.export_dialog,
            ExportDialogState::new(self.export_settings.clone()),
        );
        let mut open = dialog.open;
        let mut cancel = false;
        let mut export = false;

        egui::Window::new("Export PDF")
            .id(egui::Id::new("export_settings_dialog"))
            .open(&mut open)
            .collapsible(false)
            .default_width(480.0)
            .show(context, |ui| {
                ui.label(RichText::new("Optimization preset").strong());
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(dialog.draft.preset == ExportPreset::Lossless, "Lossless")
                        .on_hover_text("No image downsampling; lossless Flate compression")
                        .clicked()
                    {
                        dialog.draft.apply_preset(ExportPreset::Lossless);
                    }
                    if ui
                        .selectable_label(dialog.draft.preset == ExportPreset::Balanced, "Balanced")
                        .on_hover_text("Good visual quality with moderate downsampling")
                        .clicked()
                    {
                        dialog.draft.apply_preset(ExportPreset::Balanced);
                    }
                    if ui
                        .selectable_label(
                            dialog.draft.preset == ExportPreset::SmallerFile,
                            "Smaller file",
                        )
                        .on_hover_text("More downsampling and JPEG compression")
                        .clicked()
                    {
                        dialog.draft.apply_preset(ExportPreset::SmallerFile);
                    }
                });

                ui.add_space(10.0);
                ui.label(RichText::new("Image page layout").strong());
                ui.radio_value(
                    &mut dialog.draft.image_page_policy,
                    ImagePagePolicy::A4Auto,
                    "A4 with automatic orientation",
                );
                ui.radio_value(
                    &mut dialog.draft.image_page_policy,
                    ImagePagePolicy::OriginalAtDpi,
                    "Original dimensions at target DPI",
                );
                if dialog.draft.image_page_policy == ImagePagePolicy::OriginalAtDpi {
                    ui.horizontal(|ui| {
                        ui.label("DPI");
                        ui.add(
                            egui::DragValue::new(&mut dialog.draft.original_dpi)
                                .range(36.0..=1200.0)
                                .speed(1.0),
                        );
                    });
                }
                ui.radio_value(
                    &mut dialog.draft.image_page_policy,
                    ImagePagePolicy::Custom,
                    "Custom page size",
                );
                if dialog.draft.image_page_policy == ImagePagePolicy::Custom {
                    ui.horizontal(|ui| {
                        ui.label("Width");
                        ui.add(
                            egui::DragValue::new(&mut dialog.draft.custom_width_mm)
                                .range(20.0..=2000.0)
                                .suffix(" mm"),
                        );
                        ui.label("Height");
                        ui.add(
                            egui::DragValue::new(&mut dialog.draft.custom_height_mm)
                                .range(20.0..=2000.0)
                                .suffix(" mm"),
                        );
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("Margin");
                    ui.add(
                        egui::DragValue::new(&mut dialog.draft.margin_mm)
                            .range(0.0..=100.0)
                            .suffix(" mm"),
                    );
                });

                ui.add_space(10.0);
                ui.label(RichText::new("Image optimization").strong());
                ui.add_enabled(
                    dialog.draft.preset != ExportPreset::Lossless,
                    egui::Slider::new(&mut dialog.draft.image_quality, 1..=100).text("Quality"),
                );
                let mut downsample = dialog.draft.max_image_dimension.is_some();
                if ui
                    .checkbox(&mut downsample, "Limit maximum image dimension")
                    .changed()
                {
                    dialog.draft.max_image_dimension = downsample.then_some(2400);
                }
                if let Some(maximum) = &mut dialog.draft.max_image_dimension {
                    ui.horizontal(|ui| {
                        ui.label("Maximum width/height");
                        ui.add(
                            egui::DragValue::new(maximum)
                                .range(256..=20_000)
                                .suffix(" px"),
                        );
                    });
                }
                ui.label(
                    RichText::new(
                        "Imported PDF pages remain lossless; these controls affect images.",
                    )
                    .small()
                    .color(Color32::from_gray(145)),
                );

                ui.add_space(10.0);
                ui.collapsing("PDF metadata", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Title");
                        ui.text_edit_singleline(&mut dialog.draft.metadata.title);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Author");
                        ui.text_edit_singleline(&mut dialog.draft.metadata.author);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Subject");
                        ui.text_edit_singleline(&mut dialog.draft.metadata.subject);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Keywords");
                        ui.text_edit_singleline(&mut dialog.draft.metadata.keywords);
                    });
                });

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
                        .button(RichText::new("Choose output and export").strong())
                        .clicked()
                    {
                        export = true;
                    }
                });
            });

        dialog.open = open && !cancel;
        if export {
            match dialog.draft.validate() {
                Err(error) => dialog.error = Some(error.to_string()),
                Ok(()) => {
                    self.export_settings = dialog.draft.clone();
                    let pages = match dialog.target {
                        ExportTarget::AllPages => self.workspace.pages().to_vec(),
                        ExportTarget::SelectedPages => self
                            .workspace
                            .pages()
                            .iter()
                            .filter(|page| self.selected.contains(&page.id))
                            .cloned()
                            .collect(),
                    };
                    let suggested_name = match dialog.target {
                        ExportTarget::AllPages => "merged.pdf",
                        ExportTarget::SelectedPages => "selected-pages.pdf",
                    };
                    dialog.open = false;
                    self.choose_export_pages(context, pages, suggested_name);
                }
            }
        }
        self.export_dialog = dialog;
    }
}

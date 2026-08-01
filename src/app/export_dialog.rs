use eframe::egui::{self, RichText};
use pdf_merger::export_settings::{ExportPreset, ExportSettings, ImagePagePolicy};

use super::{
    PdfMergerApp,
    accessibility::{AnnouncementPriority, mark_live},
    style,
};

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
    focus_requested: bool,
}

impl ExportDialogState {
    pub(crate) fn new(settings: ExportSettings) -> Self {
        Self {
            open: false,
            target: ExportTarget::AllPages,
            draft: settings,
            error: None,
            focus_requested: false,
        }
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open
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
        self.export_dialog.focus_requested = true;
    }

    pub(super) fn show_export_dialog(&mut self, context: &egui::Context) {
        if !self.export_dialog.open {
            return;
        }
        let mut dialog = std::mem::replace(
            &mut self.export_dialog,
            ExportDialogState::new(self.export_settings.clone()),
        );
        let mut cancel = false;
        let mut export = false;

        let modal = egui::Modal::new(egui::Id::new("export_settings_dialog")).show(context, |ui| {
            ui.set_width(style::dialog_width(context, 480.0));
            ui.heading("Export PDF");
            ui.separator();
            ui.label(RichText::new("Optimization preset").strong());
            ui.horizontal(|ui| {
                let lossless = ui
                    .selectable_label(dialog.draft.preset == ExportPreset::Lossless, "Lossless")
                    .on_hover_text("No image downsampling; lossless Flate compression");
                if dialog.focus_requested {
                    lossless.request_focus();
                    dialog.focus_requested = false;
                }
                if lossless.clicked() {
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
                    let label = ui.label("DPI");
                    ui.add(
                        egui::DragValue::new(&mut dialog.draft.original_dpi)
                            .range(36.0..=1200.0)
                            .speed(1.0),
                    )
                    .labelled_by(label.id);
                });
            }
            ui.radio_value(
                &mut dialog.draft.image_page_policy,
                ImagePagePolicy::Custom,
                "Custom page size",
            );
            if dialog.draft.image_page_policy == ImagePagePolicy::Custom {
                ui.horizontal(|ui| {
                    let width_label = ui.label("Width");
                    ui.add(
                        egui::DragValue::new(&mut dialog.draft.custom_width_mm)
                            .range(20.0..=2000.0)
                            .suffix(" mm"),
                    )
                    .labelled_by(width_label.id);
                    let height_label = ui.label("Height");
                    ui.add(
                        egui::DragValue::new(&mut dialog.draft.custom_height_mm)
                            .range(20.0..=2000.0)
                            .suffix(" mm"),
                    )
                    .labelled_by(height_label.id);
                });
            }
            ui.horizontal(|ui| {
                let label = ui.label("Margin");
                ui.add(
                    egui::DragValue::new(&mut dialog.draft.margin_mm)
                        .range(0.0..=100.0)
                        .suffix(" mm"),
                )
                .labelled_by(label.id);
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
                    let label = ui.label("Maximum width/height");
                    ui.add(
                        egui::DragValue::new(maximum)
                            .range(256..=20_000)
                            .suffix(" px"),
                    )
                    .labelled_by(label.id);
                });
            }
            ui.label(
                RichText::new("Imported PDF pages remain lossless; these controls affect images.")
                    .small()
                    .color(style::muted_text(ui)),
            );

            ui.add_space(10.0);
            ui.collapsing("PDF metadata", |ui| {
                ui.horizontal(|ui| {
                    let label = ui.label("Title");
                    ui.add(
                        egui::TextEdit::singleline(&mut dialog.draft.metadata.title)
                            .hint_text("Optional title"),
                    )
                    .labelled_by(label.id);
                });
                ui.horizontal(|ui| {
                    let label = ui.label("Author");
                    ui.add(
                        egui::TextEdit::singleline(&mut dialog.draft.metadata.author)
                            .hint_text("Optional author"),
                    )
                    .labelled_by(label.id);
                });
                ui.horizontal(|ui| {
                    let label = ui.label("Subject");
                    ui.add(
                        egui::TextEdit::singleline(&mut dialog.draft.metadata.subject)
                            .hint_text("Optional subject"),
                    )
                    .labelled_by(label.id);
                });
                ui.horizontal(|ui| {
                    let label = ui.label("Keywords");
                    ui.add(
                        egui::TextEdit::singleline(&mut dialog.draft.metadata.keywords)
                            .hint_text("Optional keywords"),
                    )
                    .labelled_by(label.id);
                });
            });

            if let Some(error) = &dialog.error {
                ui.add_space(8.0);
                let error = ui.label(RichText::new(error).color(style::error_text(ui)));
                mark_live(&error, AnnouncementPriority::Assertive);
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

        if modal.should_close() {
            cancel = true;
        } else if context
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
        {
            export = true;
        }
        dialog.open = !cancel;
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

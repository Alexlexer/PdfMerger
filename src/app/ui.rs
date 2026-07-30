use eframe::egui::{self, Align, Color32, Frame, Id, Layout, Margin, RichText, Stroke, Vec2};

use super::{PdfMergerApp, style::ACCENT};

impl PdfMergerApp {
    pub(super) fn top_bar(&mut self, root_ui: &mut egui::Ui, context: &egui::Context) {
        egui::Panel::top("top_bar")
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(24, 27, 35))
                    .inner_margin(Margin::symmetric(22, 14)),
            )
            .show(root_ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(" P ")
                            .size(22.0)
                            .strong()
                            .color(Color32::WHITE)
                            .background_color(ACCENT),
                    );
                    ui.add_space(4.0);
                    ui.label(RichText::new("PdfMerger").size(21.0).strong());
                    ui.label(
                        RichText::new("native · private · offline")
                            .small()
                            .color(Color32::from_gray(150)),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let export = egui::Button::new(
                            RichText::new("Export PDF").strong().color(Color32::WHITE),
                        )
                        .fill(ACCENT)
                        .corner_radius(8);
                        if ui.add_enabled(!self.workspace.is_empty(), export).clicked() {
                            self.choose_export_path(context);
                        }

                        if ui.button("Add files").clicked() {
                            self.choose_files(context);
                        }

                        self.project_menu(ui, context);

                        if ui
                            .add_enabled(self.workspace.can_redo(), egui::Button::new("Redo"))
                            .on_hover_text("Redo (Ctrl/Cmd+Y or Ctrl/Cmd+Shift+Z)")
                            .clicked()
                        {
                            self.redo();
                        }
                        if ui
                            .add_enabled(self.workspace.can_undo(), egui::Button::new("Undo"))
                            .on_hover_text("Undo (Ctrl/Cmd+Z)")
                            .clicked()
                        {
                            self.undo();
                        }
                    });
                });
            });
    }

    pub(super) fn central_panel(&mut self, root_ui: &mut egui::Ui, context: &egui::Context) {
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(20, 23, 30))
                    .inner_margin(Margin::same(22)),
            )
            .show(root_ui, |ui| {
                if self.workspace.is_empty() {
                    self.empty_state(ui, context);
                } else {
                    ui.horizontal(|ui| {
                        ui.heading("Arrange pages");
                        ui.label(
                            RichText::new(
                                "Drag pages to reorder them or move them between document cards",
                            )
                            .color(Color32::from_gray(145)),
                        );
                    });
                    ui.add_space(10.0);
                    self.selection_toolbar(ui);
                    ui.add_space(10.0);
                    self.page_strip(ui);
                }
            });
    }

    fn selection_toolbar(&mut self, ui: &mut egui::Ui) {
        Frame::new()
            .fill(Color32::from_rgb(28, 31, 40))
            .corner_radius(8)
            .inner_margin(Margin::symmetric(10, 7))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("{} selected", self.selected.len()))
                            .strong()
                            .color(if self.selected.is_empty() {
                                Color32::from_gray(145)
                            } else {
                                ACCENT
                            }),
                    );
                    if ui.small_button("Select all").clicked() {
                        self.select_all();
                    }
                    if ui
                        .add_enabled(!self.selected.is_empty(), egui::Button::new("Deselect"))
                        .clicked()
                    {
                        self.clear_selection();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(!self.selected.is_empty(), egui::Button::new("Split…"))
                        .on_hover_text("Create multiple PDFs from the selection")
                        .clicked()
                    {
                        self.open_split_dialog();
                    }
                    if ui
                        .add_enabled(
                            !self.selected.is_empty(),
                            egui::Button::new("Export selected"),
                        )
                        .on_hover_text("Export selected pages in their visible order")
                        .clicked()
                    {
                        self.choose_selected_export_path(ui.ctx());
                    }
                    if ui
                        .add_enabled(!self.selected.is_empty(), egui::Button::new("Rotate ↻"))
                        .on_hover_text("Rotate selected pages clockwise (R)")
                        .clicked()
                    {
                        self.rotate_selected();
                    }
                    if ui
                        .add_enabled(!self.selected.is_empty(), egui::Button::new("Move first"))
                        .clicked()
                    {
                        self.move_selected_to_start();
                    }
                    if ui
                        .add_enabled(!self.selected.is_empty(), egui::Button::new("Move last"))
                        .clicked()
                    {
                        self.move_selected_to_end();
                    }
                    if ui
                        .add_enabled(!self.selected.is_empty(), egui::Button::new("Delete"))
                        .on_hover_text("Delete selected pages (Delete)")
                        .clicked()
                    {
                        self.remove_selected();
                    }
                });
            });
    }

    fn empty_state(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        ui.vertical_centered(|ui| {
            ui.add_space(70.0);
            ui.label(RichText::new("⇩").size(54.0).color(ACCENT));
            ui.add_space(12.0);
            ui.heading("Drop PDFs and pictures here");
            ui.label(
                RichText::new("Each PDF page and image becomes a draggable page card.")
                    .color(Color32::from_gray(160)),
            );
            ui.add_space(18.0);
            if ui
                .add(
                    egui::Button::new(RichText::new("Choose files").strong())
                        .fill(ACCENT)
                        .corner_radius(8)
                        .min_size(Vec2::new(140.0, 38.0)),
                )
                .clicked()
            {
                self.choose_files(context);
            }
            ui.add_space(12.0);
            ui.label(
                RichText::new("PDF · PNG · JPEG · WebP · BMP · GIF · TIFF")
                    .small()
                    .color(Color32::from_gray(120)),
            );
        });
    }

    pub(super) fn bottom_bar(&mut self, root_ui: &mut egui::Ui) {
        let active_job = self.jobs.primary();
        let active_count = self.jobs.active_count();
        let has_diagnostics = self.jobs.has_diagnostics();
        let mut cancel_job = None;
        let mut open_details = false;
        egui::Panel::bottom("status_bar")
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(24, 27, 35))
                    .inner_margin(Margin::symmetric(20, 9)),
            )
            .show(root_ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(job) = &active_job {
                        ui.spinner();
                        ui.label(
                            RichText::new(format!("{} — {}", job.title, job.phase.label()))
                                .small()
                                .color(Color32::from_gray(190)),
                        );
                        if job.total > 0 {
                            let fraction = job.completed as f32 / job.total as f32;
                            ui.add(
                                egui::ProgressBar::new(fraction)
                                    .desired_width(130.0)
                                    .text(format!("{} / {}", job.completed, job.total)),
                            );
                        }
                        if !job.detail.is_empty() {
                            ui.label(
                                RichText::new(&job.detail)
                                    .small()
                                    .color(Color32::from_gray(145)),
                            );
                        }
                        if ui
                            .add_enabled(!job.cancelling, egui::Button::new("Cancel"))
                            .clicked()
                        {
                            cancel_job = Some(job.id);
                        }
                    } else {
                        let status_color = if self.status_is_error {
                            Color32::from_rgb(244, 118, 118)
                        } else {
                            Color32::from_gray(155)
                        };
                        ui.label(RichText::new(&self.status).small().color(status_color));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{} page(s)", self.workspace.len()))
                                .small()
                                .color(Color32::from_gray(145)),
                        );
                        if has_diagnostics && ui.small_button("Details").clicked() {
                            open_details = true;
                        }
                        if active_count > 1 {
                            ui.label(
                                RichText::new(format!("{active_count} jobs"))
                                    .small()
                                    .color(Color32::from_gray(145)),
                            );
                        }
                    });
                });
            });
        if let Some(job_id) = cancel_job {
            self.jobs.cancel(job_id);
            self.set_status("Cancelling background job…", false);
        }
        if open_details {
            self.jobs.open_details();
        }
    }
    pub(super) fn file_drop_overlay(&self, context: &egui::Context) {
        let hovering_files = context.input(|input| !input.raw.hovered_files.is_empty());
        if !hovering_files {
            return;
        }

        let rect = context.content_rect();
        let painter =
            context.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new("drop")));
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha(180));
        painter.rect_stroke(
            rect.shrink(24.0),
            16.0,
            Stroke::new(3.0, ACCENT),
            egui::StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Drop to add pages",
            egui::FontId::proportional(30.0),
            Color32::WHITE,
        );
    }
}

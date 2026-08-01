use eframe::egui::{self, Color32, Frame, Id, Margin, RichText, Stroke, Vec2};

use super::{
    PdfMergerApp,
    accessibility::{AnnouncementPriority, mark_live},
    style::{self, AppearanceSettings, ColorTheme},
};

impl PdfMergerApp {
    pub(super) fn top_bar(&mut self, root_ui: &mut egui::Ui, context: &egui::Context) {
        egui::Panel::top("top_bar")
            .frame(
                Frame::new()
                    .fill(root_ui.visuals().panel_fill)
                    .inner_margin(Margin::symmetric(22, 14)),
            )
            .show(root_ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(" P ")
                            .size(22.0)
                            .strong()
                            .color(style::accent_text(ui))
                            .background_color(style::accent(ui)),
                    );
                    ui.add_space(4.0);
                    ui.label(RichText::new("PdfMerger").size(21.0).strong());
                    ui.label(
                        RichText::new("native · private · offline")
                            .small()
                            .color(style::muted_text(ui)),
                    );
                    ui.separator();

                    if ui
                        .add_enabled(self.workspace.can_undo(), egui::Button::new("Undo"))
                        .on_hover_text("Undo (Ctrl/Cmd+Z)")
                        .clicked()
                    {
                        self.undo();
                    }
                    if ui
                        .add_enabled(self.workspace.can_redo(), egui::Button::new("Redo"))
                        .on_hover_text("Redo (Ctrl/Cmd+Y or Ctrl/Cmd+Shift+Z)")
                        .clicked()
                    {
                        self.redo();
                    }

                    self.project_menu(ui, context);
                    self.appearance_menu(ui, context);

                    if ui.button("Add files").clicked() {
                        self.choose_files(context);
                    }

                    let export = egui::Button::new(
                        RichText::new("Export PDF")
                            .strong()
                            .color(style::accent_text(ui)),
                    )
                    .fill(style::accent(ui))
                    .corner_radius(8);
                    if ui.add_enabled(!self.workspace.is_empty(), export).clicked() {
                        self.choose_export_path(context);
                    }
                });
            });
    }

    fn appearance_menu(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        let previous = self.appearance;
        ui.menu_button("View", |ui| {
            ui.label(RichText::new("Theme").strong());
            ui.radio_value(&mut self.appearance.theme, ColorTheme::Dark, "Dark");
            ui.radio_value(&mut self.appearance.theme, ColorTheme::Light, "Light");
            ui.separator();
            ui.checkbox(&mut self.appearance.high_contrast, "High contrast");
            ui.separator();
            ui.label(RichText::new("UI scale").strong());
            for zoom in AppearanceSettings::ZOOM_OPTIONS {
                ui.radio_value(&mut self.appearance.zoom_percent, zoom, format!("{zoom}%"));
            }
            ui.separator();
            if ui.button("Reset appearance").clicked() {
                self.appearance = AppearanceSettings::default();
                ui.close();
            }
        });

        if self.appearance != previous {
            self.appearance.apply(context);
            self.set_status(
                format!("Appearance changed to {}.", self.appearance.description()),
                false,
            );
            context.request_repaint();
        }
    }

    pub(super) fn central_panel(&mut self, root_ui: &mut egui::Ui, context: &egui::Context) {
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(root_ui.visuals().extreme_bg_color)
                    .inner_margin(Margin::same(22)),
            )
            .show(root_ui, |ui| {
                if self.workspace.is_empty() {
                    self.empty_state(ui, context);
                } else {
                    ui.horizontal_wrapped(|ui| {
                        ui.heading("Arrange pages");
                        ui.label(
                            RichText::new(
                                "Drag pages or use the labeled controls to reorder and transfer them",
                            )
                            .color(style::muted_text(ui)),
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
            .fill(ui.visuals().faint_bg_color)
            .corner_radius(8)
            .inner_margin(Margin::symmetric(10, 7))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("{} selected", self.selected.len()))
                            .strong()
                            .color(if self.selected.is_empty() {
                                style::muted_text(ui)
                            } else {
                                style::accent(ui)
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
            ui.label(RichText::new("⇩").size(54.0).color(style::accent(ui)));
            ui.add_space(12.0);
            ui.heading("Drop PDFs and pictures here");
            ui.label(
                RichText::new("Each PDF page and image becomes an accessible page card.")
                    .color(style::muted_text(ui)),
            );
            ui.add_space(18.0);
            if ui
                .add(
                    egui::Button::new(RichText::new("Choose files").strong())
                        .fill(style::accent(ui))
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
                    .color(style::muted_text(ui)),
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
                    .fill(root_ui.visuals().panel_fill)
                    .inner_margin(Margin::symmetric(20, 9)),
            )
            .show(root_ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if let Some(job) = &active_job {
                        ui.spinner();
                        let job_status = ui.label(
                            RichText::new(format!("{} — {}", job.title, job.phase.label()))
                                .small()
                                .color(ui.visuals().text_color()),
                        );
                        mark_live(&job_status, AnnouncementPriority::Polite);
                        if job.total > 0 {
                            let fraction = job.completed as f32 / job.total as f32;
                            let progress = ui.add(
                                egui::ProgressBar::new(fraction)
                                    .desired_width(130.0)
                                    .text(format!("{} / {}", job.completed, job.total)),
                            );
                            mark_live(&progress, AnnouncementPriority::Polite);
                        }
                        if !job.detail.is_empty() {
                            ui.label(
                                RichText::new(&job.detail)
                                    .small()
                                    .color(style::muted_text(ui)),
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
                            style::error_text(ui)
                        } else {
                            style::muted_text(ui)
                        };
                        let status =
                            ui.label(RichText::new(&self.status).small().color(status_color));
                        mark_live(
                            &status,
                            if self.status_is_error {
                                AnnouncementPriority::Assertive
                            } else {
                                AnnouncementPriority::Polite
                            },
                        );
                    }
                    ui.separator();
                    if active_count > 1 {
                        ui.label(
                            RichText::new(format!("{active_count} jobs"))
                                .small()
                                .color(style::muted_text(ui)),
                        );
                    }
                    if has_diagnostics && ui.small_button("Details").clicked() {
                        open_details = true;
                    }
                    ui.label(
                        RichText::new(format!("{} page(s)", self.workspace.len()))
                            .small()
                            .color(style::muted_text(ui)),
                    );
                    ui.label(
                        RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                            .small()
                            .color(style::muted_text(ui)),
                    );
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
        let accent = context.style_of(context.theme()).visuals.selection.bg_fill;
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha(180));
        painter.rect_stroke(
            rect.shrink(24.0),
            16.0,
            Stroke::new(3.0, accent),
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

use std::collections::HashMap;

use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Id, Layout, Margin, RichText, ScrollArea, Stroke,
    Vec2,
};
use pdf_merger::model::{PageItem, PageRotation};

use super::{
    PdfMergerApp,
    style::{CARD_MARGIN, CARD_OUTER_WIDTH, CARD_SPACING, CARD_WIDTH, PREVIEW_SIZE},
};

enum CardAction {
    ToggleSelection(u64),
    Remove(usize),
    Rotate(u64),
    MoveLeft(usize),
    MoveRight(usize),
}

impl PdfMergerApp {
    pub(super) fn page_strip(&mut self, ui: &mut egui::Ui) {
        let pages = self.workspace.pages().to_vec();
        let mut card_action = None;
        let mut move_request = None;

        ScrollArea::vertical()
            .id_salt("page_strip")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let available_width = ui.available_width();
                let columns = ((available_width + CARD_SPACING) / (CARD_OUTER_WIDTH + CARD_SPACING))
                    .floor()
                    .max(1.0) as usize;

                egui::Grid::new("page_grid")
                    .min_col_width(CARD_OUTER_WIDTH)
                    .max_col_width(CARD_OUTER_WIDTH)
                    .spacing(Vec2::splat(CARD_SPACING))
                    .show(ui, |ui| {
                        let mut cell = 0;
                        for (index, page) in pages.iter().enumerate() {
                            let frame = Frame::new()
                                .fill(Color32::from_rgb(35, 39, 49))
                                .stroke(Stroke::new(
                                    if self.selected.contains(&page.id) {
                                        2.0
                                    } else {
                                        1.0
                                    },
                                    if self.selected.contains(&page.id) {
                                        super::style::ACCENT
                                    } else {
                                        Color32::from_rgb(57, 63, 78)
                                    },
                                ))
                                .corner_radius(CornerRadius::same(12))
                                .inner_margin(Margin::same(CARD_MARGIN as i8));
                            let (_zone, dropped) = ui.dnd_drop_zone::<usize, _>(frame, |ui| {
                                ui.set_min_width(CARD_WIDTH);
                                ui.set_max_width(CARD_WIDTH);
                                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                                    ui.set_width(CARD_WIDTH);
                                    if let Some(action) = page_card(
                                        ui,
                                        page,
                                        index,
                                        pages.len(),
                                        self.selected.contains(&page.id),
                                        &mut self.preview_textures,
                                    ) {
                                        card_action = Some(action);
                                    }
                                });
                            });
                            if let Some(from) = dropped {
                                move_request = Some((*from, index));
                            }
                            cell += 1;
                            if cell % columns == 0 {
                                ui.end_row();
                            }
                        }

                        let (_end_zone, dropped) = ui.dnd_drop_zone::<usize, _>(
                            Frame::new()
                                .fill(Color32::from_rgb(28, 31, 40))
                                .stroke(Stroke::new(1.0, Color32::from_rgb(61, 66, 80)))
                                .corner_radius(10)
                                .inner_margin(Margin::same(8)),
                            |ui| {
                                ui.set_min_size(Vec2::new(CARD_WIDTH, 64.0));
                                ui.set_max_width(CARD_WIDTH);
                                ui.centered_and_justified(|ui| {
                                    ui.label(
                                        RichText::new("Drop at end").color(Color32::from_gray(130)),
                                    );
                                });
                            },
                        );
                        if let Some(from) = dropped {
                            move_request = Some((*from, pages.len()));
                        }
                        if (cell + 1) % columns != 0 {
                            ui.end_row();
                        }
                    });
            });

        if let Some((from, to)) = move_request {
            self.workspace.move_page(from, to);
            self.set_status("Page order updated.", false);
        }

        if let Some(action) = card_action {
            match action {
                CardAction::ToggleSelection(id) => self.toggle_selection(id),
                CardAction::Remove(index) => {
                    if let Some(page) = pages.get(index) {
                        self.preview_textures.remove(&page.id);
                        self.selected.remove(&page.id);
                    }
                    self.workspace.remove(index);
                    self.set_status("Page removed.", false);
                }
                CardAction::Rotate(id) => self.rotate_page_or_selection(id),
                CardAction::MoveLeft(index) => {
                    self.workspace.move_page(index, index - 1);
                }
                CardAction::MoveRight(index) => {
                    self.workspace.move_page(index, index + 2);
                }
            }
        }
    }
}

fn page_card(
    ui: &mut egui::Ui,
    page: &PageItem,
    index: usize,
    page_count: usize,
    selected: bool,
    preview_textures: &mut HashMap<u64, egui::TextureHandle>,
) -> Option<CardAction> {
    let mut action = None;
    ui.set_min_width(CARD_WIDTH);
    ui.set_max_width(CARD_WIDTH);
    ui.set_width(CARD_WIDTH);
    ui.horizontal(|ui| {
        let mut checked = selected;
        if ui
            .checkbox(&mut checked, "")
            .on_hover_text("Select this page")
            .changed()
        {
            action = Some(CardAction::ToggleSelection(page.id));
        }
        ui.label(
            RichText::new(format!("{:02}", index + 1))
                .strong()
                .color(super::style::ACCENT),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .small_button("×")
                .on_hover_text("Remove this page")
                .clicked()
            {
                action = Some(CardAction::Remove(index));
            }
        });
    });
    ui.add_space(5.0);

    ui.dnd_drag_source(Id::new(("page_drag", page.id)), index, |ui| {
        ui.set_width(CARD_WIDTH);
        let (preview_rect, _) = ui.allocate_exact_size(PREVIEW_SIZE, egui::Sense::hover());
        ui.painter().rect_filled(preview_rect, 5.0, Color32::WHITE);
        ui.painter().rect_stroke(
            preview_rect,
            5.0,
            Stroke::new(1.0, Color32::from_gray(72)),
            egui::StrokeKind::Inside,
        );
        if let Some(preview) = &page.preview {
            let preview_size = match page.rotation {
                PageRotation::Deg90 | PageRotation::Deg270 => [preview.size[1], preview.size[0]],
                PageRotation::Deg0 | PageRotation::Deg180 => preview.size,
            };
            let texture = preview_textures.entry(page.id).or_insert_with(|| {
                let rotated = preview.rotated(page.rotation);
                let image =
                    egui::ColorImage::from_rgba_unmultiplied(rotated.size, rotated.rgba.as_ref());
                ui.ctx().load_texture(
                    format!("page-preview-{}-{}", page.id, page.rotation.degrees()),
                    image,
                    egui::TextureOptions::LINEAR,
                )
            });
            let source_size = Vec2::new(preview_size[0] as f32, preview_size[1] as f32);
            let scale = (PREVIEW_SIZE.x / source_size.x).min(PREVIEW_SIZE.y / source_size.y);
            let image_rect =
                egui::Rect::from_center_size(preview_rect.center(), source_size * scale);
            egui::Image::from_texture(&*texture)
                .fit_to_exact_size(image_rect.size())
                .paint_at(ui, image_rect);
        } else {
            ui.painter().text(
                preview_rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("PDF\nPAGE {}", index + 1),
                egui::FontId::proportional(19.0),
                Color32::from_gray(60),
            );
        }

        ui.add_space(7.0);
        ui.add(
            egui::Label::new(
                RichText::new(&page.title)
                    .strong()
                    .color(Color32::from_gray(225)),
            )
            .truncate(),
        )
        .on_hover_text(page.source.path().display().to_string());
        ui.add(
            egui::Label::new(
                RichText::new(&page.subtitle)
                    .small()
                    .color(Color32::from_gray(135)),
            )
            .truncate(),
        );
    });
    ui.add_space(5.0);
    ui.horizontal(|ui| {
        if ui.button("↻").on_hover_text("Rotate clockwise").clicked() {
            action = Some(CardAction::Rotate(page.id));
        }
        if ui
            .add_enabled(
                index > 0,
                egui::Button::new(RichText::new("Back").strong()).min_size(Vec2::new(48.0, 28.0)),
            )
            .on_hover_text("Move page backward")
            .clicked()
        {
            action = Some(CardAction::MoveLeft(index));
        }
        if ui
            .add_enabled(
                index + 1 < page_count,
                egui::Button::new(RichText::new("Next").strong()).min_size(Vec2::new(48.0, 28.0)),
            )
            .on_hover_text("Move page forward")
            .clicked()
        {
            action = Some(CardAction::MoveRight(index));
        }
    });
    action
}

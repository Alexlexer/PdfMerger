use std::collections::{HashMap, HashSet};

use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Id, Layout, Margin, RichText, ScrollArea, Stroke,
    Vec2,
};
use pdf_merger::model::{PageGroup, PageItem, PageRotation};

use super::{
    PdfMergerApp,
    accessibility::{label_button, label_toggle, mark_expanded},
    export_dialog::ExportTarget,
    previews::PreviewRequest,
    style::{self, CARD_MARGIN, CARD_OUTER_WIDTH, CARD_SPACING, CARD_WIDTH, PREVIEW_SIZE},
};

const DRAG_SCROLL_EDGE: f32 = 72.0;
const DRAG_SCROLL_OVERSHOOT: f32 = 18.0;
const DRAG_SCROLL_MAX_SPEED: f32 = 900.0;

enum CardAction {
    ToggleSelection(u64),
    Remove(usize),
    Rotate(u64),
    MoveLeft(usize),
    MoveRight(usize),
}

enum GroupAction {
    ToggleCollapse(u64),
    ToggleSelection(u64),
    MoveSelectedHere(u64),
    Rotate(u64),
    Remove(u64),
    Export(u64),
    MoveUp(usize),
    MoveDown(usize),
}

#[derive(Clone, Copy)]
struct GroupHeaderState {
    index: usize,
    count: usize,
    collapsed: bool,
    all_selected: bool,
    source_count: usize,
    can_receive_selection: bool,
}

impl PdfMergerApp {
    pub(super) fn page_strip(&mut self, ui: &mut egui::Ui) {
        let pages = self.workspace.pages().to_vec();
        let groups = self.workspace.groups();
        let mut card_action = None;
        let mut group_action = None;
        let mut move_request = None;
        let mut preview_requests = Vec::new();

        let scroll_output = ScrollArea::vertical()
            .id_salt("source_group_strip")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (group_index, group) in groups.iter().enumerate() {
                    let collapsed = self.collapsed_groups.contains(&group.id);
                    let group_ids = pages[group.start..group.end]
                        .iter()
                        .map(|page| page.id)
                        .collect::<HashSet<_>>();
                    let all_selected = group_ids.iter().all(|id| self.selected.contains(id));
                    let source_count = pages[group.start..group.end]
                        .iter()
                        .map(|page| page.source.path())
                        .collect::<HashSet<_>>()
                        .len();
                    let can_receive_selection = pages
                        .iter()
                        .any(|page| self.selected.contains(&page.id) && page.group_id != group.id);

                    let (_group_zone, group_drop) = ui.dnd_drop_zone::<usize, _>(
                        Frame::new()
                            .fill(style::group_fill(ui))
                            .stroke(style::border(ui))
                            .corner_radius(CornerRadius::same(12))
                            .inner_margin(Margin::same(12)),
                        |ui| {
                            if let Some(action) = group_header(
                                ui,
                                group,
                                GroupHeaderState {
                                    index: group_index,
                                    count: groups.len(),
                                    collapsed,
                                    all_selected,
                                    source_count,
                                    can_receive_selection,
                                },
                            ) {
                                group_action = Some(action);
                            }
                            if !collapsed {
                                ui.add_space(10.0);
                                let available_width = ui.available_width();
                                let columns = ((available_width + CARD_SPACING)
                                    / (CARD_OUTER_WIDTH + CARD_SPACING))
                                    .floor()
                                    .max(1.0)
                                    as usize;
                                egui::Grid::new(("page_group_grid", group.id))
                                    .min_col_width(CARD_OUTER_WIDTH)
                                    .max_col_width(CARD_OUTER_WIDTH)
                                    .spacing(Vec2::splat(CARD_SPACING))
                                    .show(ui, |ui| {
                                        let mut cell = 0;
                                        for index in group.start..group.end {
                                            let page = &pages[index];
                                            let frame = Frame::new()
                                                .fill(style::card_fill(ui))
                                                .stroke(style::selection_border(
                                                    ui,
                                                    self.selected.contains(&page.id),
                                                ))
                                                .corner_radius(CornerRadius::same(12))
                                                .inner_margin(Margin::same(CARD_MARGIN as i8));
                                            let (_zone, dropped) =
                                                ui.dnd_drop_zone::<usize, _>(frame, |ui| {
                                                    ui.set_min_width(CARD_WIDTH);
                                                    ui.set_max_width(CARD_WIDTH);
                                                    ui.with_layout(
                                                        Layout::top_down(Align::Min),
                                                        |ui| {
                                                            ui.set_width(CARD_WIDTH);
                                                            if let Some(action) = page_card(
                                                                ui,
                                                                page,
                                                                index,
                                                                pages.len(),
                                                                group.start,
                                                                group.end,
                                                                self.selected.contains(&page.id),
                                                                &mut self.preview_textures,
                                                                &self.pdf_previews,
                                                                &mut preview_requests,
                                                            ) {
                                                                card_action = Some(action);
                                                            }
                                                        },
                                                    );
                                                });
                                            if let Some(from) = dropped {
                                                move_request =
                                                    Some((*from, index, group.id, false));
                                            }
                                            cell += 1;
                                            if cell % columns == 0 {
                                                ui.end_row();
                                            }
                                        }

                                        if cell % columns != 0 {
                                            ui.end_row();
                                        }
                                    });
                            }
                        },
                    );
                    if let Some(from) = group_drop
                        && move_request.is_none()
                    {
                        move_request = Some((*from, group.end, group.id, true));
                    }
                    ui.add_space(12.0);
                }
            });

        self.request_pdf_previews(preview_requests, ui.ctx());

        if egui::DragAndDrop::has_payload_of_type::<usize>(ui.ctx())
            && let Some(pointer) = ui.ctx().pointer_latest_pos()
        {
            let viewport = scroll_output.inner_rect;
            let active_rect = viewport.expand2(Vec2::new(0.0, DRAG_SCROLL_OVERSHOOT));
            if active_rect.contains(pointer) {
                let wheel_delta = ui.input(|input| input.smooth_scroll_delta().y);
                let edge_velocity = drag_edge_scroll_velocity(pointer.y, viewport);
                let frame_time = ui.input(|input| input.stable_dt).min(0.05);
                let scroll_delta = -wheel_delta + edge_velocity * frame_time;

                if scroll_delta.abs() > f32::EPSILON {
                    let maximum_offset =
                        (scroll_output.content_size.y - viewport.height()).max(0.0);
                    let mut state = scroll_output.state;
                    let previous_offset = state.offset.y;
                    state.offset.y = (state.offset.y + scroll_delta).clamp(0.0, maximum_offset);
                    if state.offset.y != previous_offset {
                        state.store(ui.ctx(), scroll_output.id);
                    }
                    if wheel_delta.abs() > f32::EPSILON {
                        ui.input_mut(|input| input.smooth_scroll_delta.y = 0.0);
                    }
                    if edge_velocity.abs() > f32::EPSILON {
                        ui.ctx().request_repaint();
                    }
                }
            }
        }

        if let Some((from, to, target_group_id, dropped_on_group)) = move_request {
            let dragged = pages.get(from);
            let transferred = dragged.is_some_and(|page| page.group_id != target_group_id);
            let move_selection = dropped_on_group
                && transferred
                && dragged.is_some_and(|page| self.selected.contains(&page.id));
            if move_selection {
                let moved = self
                    .workspace
                    .move_ids_to_group(&self.selected, target_group_id);
                if moved > 0 {
                    self.retain_existing_selection();
                    self.set_status(
                        format!("Transferred {moved} selected page(s) into the document card."),
                        false,
                    );
                }
            } else if self.workspace.move_page_to_group(from, to, target_group_id) {
                self.retain_existing_selection();
                self.set_status(
                    if transferred {
                        "Page transferred into the document card."
                    } else {
                        "Page order updated within its document card."
                    },
                    false,
                );
            }
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

        if let Some(action) = group_action {
            match action {
                GroupAction::ToggleCollapse(group_id) => {
                    if !self.collapsed_groups.remove(&group_id) {
                        self.collapsed_groups.insert(group_id);
                    }
                }
                GroupAction::ToggleSelection(group_id) => {
                    let ids = self.workspace.group_page_ids(group_id);
                    if ids.iter().all(|id| self.selected.contains(id)) {
                        self.selected.retain(|id| !ids.contains(id));
                    } else {
                        self.selected.extend(ids);
                    }
                }
                GroupAction::MoveSelectedHere(group_id) => {
                    let moved = self.workspace.move_ids_to_group(&self.selected, group_id);
                    if moved > 0 {
                        self.retain_existing_selection();
                        self.set_status(
                            format!(
                                "Transferred {moved} selected page(s) into the document group."
                            ),
                            false,
                        );
                    }
                }
                GroupAction::Rotate(group_id) => {
                    let ids = self.workspace.group_page_ids(group_id);
                    let rotated = self.workspace.rotate_ids_clockwise(&ids);
                    for id in ids {
                        self.preview_textures.remove(&id);
                    }
                    self.set_status(format!("Rotated {rotated} grouped page(s)."), false);
                }
                GroupAction::Remove(group_id) => {
                    let ids = self.workspace.group_page_ids(group_id);
                    let removed = self.workspace.remove_ids(&ids);
                    for id in ids {
                        self.selected.remove(&id);
                        self.preview_textures.remove(&id);
                    }
                    self.collapsed_groups.remove(&group_id);
                    self.set_status(
                        format!("Removed document group with {removed} page(s)."),
                        false,
                    );
                }
                GroupAction::Export(group_id) => {
                    self.selected = self.workspace.group_page_ids(group_id);
                    self.open_export_dialog(ExportTarget::SelectedPages);
                }

                GroupAction::MoveUp(index) => {
                    if self.workspace.move_group(index, index - 1) {
                        self.set_status("Moved document group earlier.", false);
                    }
                }
                GroupAction::MoveDown(index) => {
                    if self.workspace.move_group(index, index + 2) {
                        self.set_status("Moved document group later.", false);
                    }
                }
            }
        }
    }
}

fn drag_edge_scroll_velocity(pointer_y: f32, viewport: egui::Rect) -> f32 {
    let top_strength =
        ((viewport.top() + DRAG_SCROLL_EDGE - pointer_y) / DRAG_SCROLL_EDGE).clamp(0.0, 1.0);
    let bottom_strength =
        ((pointer_y - (viewport.bottom() - DRAG_SCROLL_EDGE)) / DRAG_SCROLL_EDGE).clamp(0.0, 1.0);
    (bottom_strength - top_strength) * DRAG_SCROLL_MAX_SPEED
}

fn group_header(
    ui: &mut egui::Ui,
    group: &PageGroup,
    state: GroupHeaderState,
) -> Option<GroupAction> {
    let mut action = None;
    let file_name = group
        .source_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Source document");
    ui.horizontal_wrapped(|ui| {
        let collapse = ui
            .small_button(if state.collapsed {
                "Expand"
            } else {
                "Collapse"
            })
            .on_hover_text(if state.collapsed {
                "Expand this document group"
            } else {
                "Collapse this document group"
            });
        label_button(
            &collapse,
            format!(
                "{} document group {file_name}",
                if state.collapsed {
                    "Expand"
                } else {
                    "Collapse"
                }
            ),
        );
        mark_expanded(&collapse, !state.collapsed);
        if collapse.clicked() {
            action = Some(GroupAction::ToggleCollapse(group.id));
        }
        ui.label(RichText::new(file_name).strong().size(16.0));
        ui.label(
            RichText::new(format!("{} page(s)", group.page_count()))
                .small()
                .color(style::muted_text(ui)),
        );
        if state.source_count > 1 {
            ui.label(
                RichText::new(format!("mixed from {} sources", state.source_count))
                    .small()
                    .color(ui.visuals().warn_fg_color),
            );
        }
        let select_group = ui.small_button(if state.all_selected {
            "Deselect group"
        } else {
            "Select group"
        });
        label_toggle(
            &select_group,
            state.all_selected,
            format!("Select document group {file_name}"),
        );
        if select_group.clicked() {
            action = Some(GroupAction::ToggleSelection(group.id));
        }

        let move_selection = ui
            .add_enabled(
                state.can_receive_selection,
                egui::Button::new("Move selection here"),
            )
            .on_hover_text("Transfer selected pages into this document group");
        label_button(
            &move_selection,
            format!("Move selected pages to document group {file_name}"),
        );
        if move_selection.clicked() {
            action = Some(GroupAction::MoveSelectedHere(group.id));
        }
        let export_group = ui.small_button("Export group");
        label_button(&export_group, format!("Export document group {file_name}"));
        if export_group.clicked() {
            action = Some(GroupAction::Export(group.id));
        }
        let rotate_group = ui.small_button("Rotate group ↻");
        label_button(
            &rotate_group,
            format!("Rotate document group {file_name} clockwise"),
        );
        if rotate_group.clicked() {
            action = Some(GroupAction::Rotate(group.id));
        }
        let move_up = ui.add_enabled(state.index > 0, egui::Button::new("Move up"));
        label_button(&move_up, format!("Move document group {file_name} earlier"));
        if move_up.clicked() {
            action = Some(GroupAction::MoveUp(state.index));
        }
        let move_down = ui.add_enabled(
            state.index + 1 < state.count,
            egui::Button::new("Move down"),
        );
        label_button(&move_down, format!("Move document group {file_name} later"));
        if move_down.clicked() {
            action = Some(GroupAction::MoveDown(state.index));
        }
        let remove_group =
            ui.small_button(RichText::new("Remove group").color(style::error_text(ui)));
        label_button(&remove_group, format!("Remove document group {file_name}"));
        if remove_group.clicked() {
            action = Some(GroupAction::Remove(group.id));
        }
    });
    ui.add(
        egui::Label::new(
            RichText::new(group.source_path.display().to_string())
                .small()
                .color(style::muted_text(ui)),
        )
        .truncate(),
    );
    action
}

#[allow(clippy::too_many_arguments)]
fn page_card(
    ui: &mut egui::Ui,
    page: &PageItem,
    index: usize,
    page_count: usize,
    group_start: usize,
    group_end: usize,
    selected: bool,
    preview_textures: &mut HashMap<u64, egui::TextureHandle>,
    pdf_previews: &HashMap<u64, pdf_merger::model::PreviewData>,
    preview_requests: &mut Vec<PreviewRequest>,
) -> Option<CardAction> {
    let mut action = None;
    ui.set_min_width(CARD_WIDTH);
    ui.set_max_width(CARD_WIDTH);
    ui.set_width(CARD_WIDTH);
    ui.horizontal(|ui| {
        let mut checked = selected;
        let page_toggle = ui
            .toggle_value(&mut checked, format!("Page {:02}", index + 1))
            .on_hover_text(format!("Select page {}: {}", index + 1, page.title));
        label_toggle(
            &page_toggle,
            selected,
            format!("Select page {}: {}", index + 1, page.title),
        );
        if page_toggle.changed() {
            action = Some(CardAction::ToggleSelection(page.id));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let remove = ui.small_button("Remove").on_hover_text(format!(
                "Remove page {}: {}",
                index + 1,
                page.title
            ));
            label_button(
                &remove,
                format!("Remove page {}: {}", index + 1, page.title),
            );
            if remove.clicked() {
                action = Some(CardAction::Remove(index));
            }
        });
    });
    ui.add_space(5.0);

    ui.dnd_drag_source(Id::new(("page_drag", page.id)), index, |ui| {
        ui.set_width(CARD_WIDTH);
        let (preview_rect, preview_response) =
            ui.allocate_exact_size(PREVIEW_SIZE, egui::Sense::hover());
        preview_response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Image,
                true,
                format!("Preview of page {}: {}", index + 1, page.title),
            )
        });
        ui.painter().rect_filled(preview_rect, 5.0, Color32::WHITE);
        ui.painter().rect_stroke(
            preview_rect,
            5.0,
            Stroke::new(1.0, Color32::from_gray(72)),
            egui::StrokeKind::Inside,
        );
        let preview = page.preview.as_ref().or_else(|| pdf_previews.get(&page.id));
        if preview.is_none()
            && preview_response.rect.intersects(ui.clip_rect())
            && let pdf_merger::model::PageSource::Pdf { path, page_number } = &page.source
        {
            preview_requests.push(PreviewRequest {
                id: page.id,
                path: path.clone(),
                page_number: *page_number,
            });
        }
        if let Some(preview) = preview {
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
                    .color(ui.visuals().text_color()),
            )
            .truncate(),
        )
        .on_hover_text(page.source.path().display().to_string());
        ui.add(
            egui::Label::new(
                RichText::new(&page.subtitle)
                    .small()
                    .color(style::muted_text(ui)),
            )
            .truncate(),
        );
    });
    ui.add_space(5.0);
    ui.horizontal(|ui| {
        let rotate = ui
            .button("Rotate")
            .on_hover_text(format!("Rotate page {} clockwise", index + 1));
        label_button(
            &rotate,
            format!("Rotate page {} clockwise: {}", index + 1, page.title),
        );
        if rotate.clicked() {
            action = Some(CardAction::Rotate(page.id));
        }
        let earlier = ui
            .add_enabled(
                index > group_start,
                egui::Button::new(RichText::new("Earlier").strong())
                    .min_size(Vec2::new(48.0, 28.0)),
            )
            .on_hover_text("Move page backward within this group");
        label_button(
            &earlier,
            format!("Move page {} earlier: {}", index + 1, page.title),
        );
        if earlier.clicked() {
            action = Some(CardAction::MoveLeft(index));
        }
        let later = ui
            .add_enabled(
                index + 1 < group_end && index + 1 < page_count,
                egui::Button::new(RichText::new("Later").strong()).min_size(Vec2::new(48.0, 28.0)),
            )
            .on_hover_text("Move page forward within this group");
        label_button(
            &later,
            format!("Move page {} later: {}", index + 1, page.title),
        );
        if later.clicked() {
            action = Some(CardAction::MoveRight(index));
        }
    });
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_edge_scroll_accelerates_toward_the_viewport_edges() {
        let viewport = egui::Rect::from_min_max(egui::pos2(0.0, 100.0), egui::pos2(500.0, 700.0));

        assert_eq!(drag_edge_scroll_velocity(400.0, viewport), 0.0);
        assert!(drag_edge_scroll_velocity(120.0, viewport) < -500.0);
        assert!(drag_edge_scroll_velocity(680.0, viewport) > 500.0);
        assert_eq!(
            drag_edge_scroll_velocity(100.0, viewport),
            -DRAG_SCROLL_MAX_SPEED
        );
        assert_eq!(
            drag_edge_scroll_velocity(700.0, viewport),
            DRAG_SCROLL_MAX_SPEED
        );
    }
}

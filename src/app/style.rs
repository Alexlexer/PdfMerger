use eframe::egui::{self, Color32, CornerRadius, Vec2};

pub(super) const ACCENT: Color32 = Color32::from_rgb(94, 106, 210);
pub(super) const CARD_WIDTH: f32 = 166.0;
pub(super) const CARD_MARGIN: f32 = 10.0;
pub(super) const CARD_SPACING: f32 = 10.0;
pub(super) const CARD_OUTER_WIDTH: f32 = CARD_WIDTH + CARD_MARGIN * 2.0;
pub(super) const PREVIEW_SIZE: Vec2 = Vec2::new(CARD_WIDTH, 221.0);

pub(super) fn configure(context: &egui::Context) {
    context.set_theme(egui::Theme::Dark);
    context.set_visuals_of(egui::Theme::Dark, egui::Visuals::dark());
    context.style_mut_of(egui::Theme::Dark, |style| {
        style.spacing.item_spacing = Vec2::new(9.0, 8.0);
        style.visuals.widgets.inactive.corner_radius = CornerRadius::same(7);
        style.visuals.widgets.hovered.corner_radius = CornerRadius::same(7);
        style.visuals.widgets.active.corner_radius = CornerRadius::same(7);
    });
}

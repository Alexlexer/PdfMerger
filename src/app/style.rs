use eframe::egui::{self, Color32, CornerRadius, Stroke, Vec2};

pub(super) const ACCENT: Color32 = Color32::from_rgb(94, 106, 210);
pub(super) const FOCUS: Color32 = Color32::from_rgb(255, 211, 92);
pub(super) const CARD_WIDTH: f32 = 166.0;
pub(super) const CARD_MARGIN: f32 = 10.0;
pub(super) const CARD_SPACING: f32 = 10.0;
pub(super) const CARD_OUTER_WIDTH: f32 = CARD_WIDTH + CARD_MARGIN * 2.0;
pub(super) const PREVIEW_SIZE: Vec2 = Vec2::new(CARD_WIDTH, 221.0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ColorTheme {
    Dark,
    Light,
}

impl ColorTheme {
    fn egui_theme(self) -> egui::Theme {
        match self {
            Self::Dark => egui::Theme::Dark,
            Self::Light => egui::Theme::Light,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct AppearanceSettings {
    pub(super) theme: ColorTheme,
    pub(super) high_contrast: bool,
    pub(super) zoom_percent: u16,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: ColorTheme::Dark,
            high_contrast: false,
            zoom_percent: 100,
        }
    }
}

impl AppearanceSettings {
    pub(super) const ZOOM_OPTIONS: [u16; 4] = [100, 125, 150, 200];

    pub(super) fn apply(self, context: &egui::Context) {
        context.set_visuals_of(
            egui::Theme::Dark,
            theme_visuals(ColorTheme::Dark, self.high_contrast),
        );
        context.set_visuals_of(
            egui::Theme::Light,
            theme_visuals(ColorTheme::Light, self.high_contrast),
        );
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            context.style_mut_of(theme, |style| {
                style.spacing.item_spacing = Vec2::new(9.0, 8.0);
                style.visuals.widgets.inactive.corner_radius = CornerRadius::same(7);
                style.visuals.widgets.hovered.corner_radius = CornerRadius::same(7);
                style.visuals.widgets.active.corner_radius = CornerRadius::same(7);
                style.visuals.widgets.open.corner_radius = CornerRadius::same(7);
                style.visuals.widgets.hovered.bg_stroke = Stroke::new(
                    if self.high_contrast { 2.5 } else { 1.5 },
                    focus_color(self.theme),
                );
                style.visuals.widgets.active.bg_stroke = Stroke::new(
                    if self.high_contrast { 3.0 } else { 2.0 },
                    focus_color(self.theme),
                );
                style.spacing.interact_size.y = style.spacing.interact_size.y.max(30.0);
            });
        }
        context.set_theme(self.theme.egui_theme());
        context.set_zoom_factor(self.zoom_percent as f32 / 100.0);
    }

    pub(super) fn description(self) -> String {
        format!(
            "{}{} at {}% UI scale",
            self.theme.label(),
            if self.high_contrast {
                " high contrast"
            } else {
                ""
            },
            self.zoom_percent
        )
    }
}

pub(super) fn configure(context: &egui::Context) -> AppearanceSettings {
    let appearance = AppearanceSettings::default();
    appearance.apply(context);
    appearance
}

pub(super) fn accent(ui: &egui::Ui) -> Color32 {
    ui.visuals().selection.bg_fill
}

pub(super) fn accent_text(ui: &egui::Ui) -> Color32 {
    ui.visuals().selection.stroke.color
}

pub(super) fn muted_text(ui: &egui::Ui) -> Color32 {
    ui.visuals().weak_text_color()
}

pub(super) fn error_text(ui: &egui::Ui) -> Color32 {
    ui.visuals().error_fg_color
}

pub(super) fn success_text(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(120, 220, 150)
    } else {
        Color32::from_rgb(0, 105, 48)
    }
}

pub(super) fn group_fill(ui: &egui::Ui) -> Color32 {
    ui.visuals().faint_bg_color
}

pub(super) fn card_fill(ui: &egui::Ui) -> Color32 {
    ui.visuals().extreme_bg_color
}

pub(super) fn border(ui: &egui::Ui) -> Stroke {
    ui.visuals().widgets.noninteractive.bg_stroke
}

pub(super) fn dialog_width(context: &egui::Context, preferred: f32) -> f32 {
    dialog_width_for(context.content_rect().width(), preferred)
}

fn dialog_width_for(viewport_width: f32, preferred: f32) -> f32 {
    preferred.min((viewport_width - 32.0).max(160.0))
}

pub(super) fn selection_border(ui: &egui::Ui, selected: bool) -> Stroke {
    if selected {
        Stroke::new(
            2.0,
            focus_color(if ui.visuals().dark_mode {
                ColorTheme::Dark
            } else {
                ColorTheme::Light
            }),
        )
    } else {
        border(ui)
    }
}

fn focus_color(theme: ColorTheme) -> Color32 {
    match theme {
        ColorTheme::Dark => FOCUS,
        ColorTheme::Light => Color32::from_rgb(0, 75, 160),
    }
}

fn theme_visuals(theme: ColorTheme, high_contrast: bool) -> egui::Visuals {
    let mut visuals = match theme {
        ColorTheme::Dark => egui::Visuals::dark(),
        ColorTheme::Light => egui::Visuals::light(),
    };

    let (accent, accent_text, error) = match (theme, high_contrast) {
        (ColorTheme::Dark, false) => (ACCENT, Color32::WHITE, Color32::from_rgb(255, 145, 145)),
        (ColorTheme::Light, false) => (
            Color32::from_rgb(55, 72, 180),
            Color32::WHITE,
            Color32::from_rgb(176, 0, 32),
        ),
        (ColorTheme::Dark, true) => (
            Color32::from_rgb(255, 225, 85),
            Color32::BLACK,
            Color32::from_rgb(255, 128, 128),
        ),
        (ColorTheme::Light, true) => (
            Color32::from_rgb(0, 55, 140),
            Color32::WHITE,
            Color32::from_rgb(150, 0, 0),
        ),
    };
    visuals.selection.bg_fill = accent;
    visuals.selection.stroke = Stroke::new(1.5, accent_text);
    visuals.error_fg_color = error;

    if high_contrast {
        let (background, raised, foreground) = match theme {
            ColorTheme::Dark => (
                Color32::BLACK,
                Color32::from_rgb(18, 18, 18),
                Color32::WHITE,
            ),
            ColorTheme::Light => (
                Color32::WHITE,
                Color32::from_rgb(242, 242, 242),
                Color32::BLACK,
            ),
        };
        visuals.override_text_color = Some(foreground);
        visuals.panel_fill = background;
        visuals.window_fill = background;
        visuals.extreme_bg_color = background;
        visuals.faint_bg_color = raised;
        visuals.code_bg_color = raised;
        visuals.window_stroke = Stroke::new(2.0, foreground);
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.5, foreground);
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.5, foreground);
        visuals.widgets.hovered.bg_stroke = Stroke::new(2.5, focus_color(theme));
        visuals.widgets.active.bg_stroke = Stroke::new(3.0, focus_color(theme));
        visuals.widgets.open.bg_stroke = Stroke::new(2.5, focus_color(theme));
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.5, foreground);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.5, foreground);
        visuals.widgets.hovered.fg_stroke = Stroke::new(2.0, foreground);
        visuals.widgets.active.fg_stroke = Stroke::new(2.0, foreground);
        visuals.widgets.open.fg_stroke = Stroke::new(2.0, foreground);
    }

    visuals
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_descriptions_include_theme_contrast_and_scale() {
        assert_eq!(
            AppearanceSettings::default().description(),
            "Dark at 100% UI scale"
        );
        assert_eq!(
            AppearanceSettings {
                theme: ColorTheme::Light,
                high_contrast: true,
                zoom_percent: 200,
            }
            .description(),
            "Light high contrast at 200% UI scale"
        );
    }

    #[test]
    fn applying_appearance_updates_theme_and_zoom() {
        let context = egui::Context::default();
        AppearanceSettings {
            theme: ColorTheme::Light,
            high_contrast: true,
            zoom_percent: 150,
        }
        .apply(&context);

        assert_eq!(context.theme(), egui::Theme::Light);
        context.begin_pass(egui::RawInput::default());
        let _ = context.end_pass();
        assert_eq!(context.zoom_factor(), 1.5);
    }

    #[test]
    fn dialog_width_stays_inside_a_scaled_viewport() {
        assert_eq!(dialog_width_for(1100.0, 620.0), 620.0);
        assert_eq!(dialog_width_for(380.0, 620.0), 348.0);
        assert_eq!(dialog_width_for(170.0, 620.0), 160.0);
    }

    #[test]
    fn high_contrast_palettes_keep_text_and_selection_distinct() {
        for theme in [ColorTheme::Dark, ColorTheme::Light] {
            let visuals = theme_visuals(theme, true);
            let foreground = visuals
                .override_text_color
                .expect("high contrast must override the foreground");
            assert!(contrast_ratio(foreground, visuals.panel_fill) >= 7.0);
            assert!(
                contrast_ratio(visuals.selection.stroke.color, visuals.selection.bg_fill) >= 7.0
            );
        }
    }

    fn contrast_ratio(first: Color32, second: Color32) -> f32 {
        let light = relative_luminance(first).max(relative_luminance(second));
        let dark = relative_luminance(first).min(relative_luminance(second));
        (light + 0.05) / (dark + 0.05)
    }

    fn relative_luminance(color: Color32) -> f32 {
        let linear = |channel: u8| {
            let channel = channel as f32 / 255.0;
            if channel <= 0.04045 {
                channel / 12.92
            } else {
                ((channel + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(color.r()) + 0.7152 * linear(color.g()) + 0.0722 * linear(color.b())
    }
}

//! The Legaia look: egui's light and dark visuals with the accent and
//! panel tones the game's own launcher draws in (`legaia-engine` with no
//! subcommand), so the two programs in a release folder read as one family.
//!
//! The launcher's palette is the reference: a deep blue-grey ground, a
//! slightly lighter card tone, and the gold of the game's title lettering
//! for the selection and the primary button.

use eframe::egui::{self, Color32, Stroke};

/// The launcher's gold (`GOLD` there), the dark theme's accent.
pub const GOLD: Color32 = Color32::from_rgb(255, 209, 92);
/// The primary-button fill (the launcher's *Play*).
pub const GOLD_FILL: Color32 = Color32::from_rgb(224, 178, 66);
/// Ink on a gold fill.
pub const ON_GOLD: Color32 = Color32::from_rgb(26, 20, 10);
/// The light theme's accent: the same hue, dark enough to read on white.
pub const GOLD_DEEP: Color32 = Color32::from_rgb(150, 100, 12);

/// Install the Legaia visuals for both themes; egui then picks one by the
/// theme preference (`View` menu).
pub fn install(ctx: &egui::Context) {
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        let v = &mut s.visuals;
        v.panel_fill = Color32::from_rgb(20, 22, 34);
        v.window_fill = Color32::from_rgb(26, 29, 44);
        v.extreme_bg_color = Color32::from_rgb(12, 13, 21);
        v.faint_bg_color = Color32::from_rgb(28, 31, 46);
        v.code_bg_color = Color32::from_rgb(30, 33, 48);
        v.hyperlink_color = GOLD;
        v.selection.bg_fill = Color32::from_rgb(120, 92, 30);
        v.selection.stroke = Stroke::new(1.0_f32, GOLD);
        v.widgets.noninteractive.bg_fill = Color32::from_rgb(26, 29, 44);
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(38, 41, 60);
        v.widgets.inactive.bg_fill = Color32::from_rgb(38, 41, 60);
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(52, 57, 84);
        v.widgets.hovered.bg_fill = Color32::from_rgb(52, 57, 84);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(200, 160, 70));
        v.widgets.active.weak_bg_fill = Color32::from_rgb(70, 62, 40);
        v.widgets.active.bg_fill = Color32::from_rgb(70, 62, 40);
        v.widgets.open.weak_bg_fill = Color32::from_rgb(46, 50, 74);
    });
    ctx.style_mut_of(egui::Theme::Light, |s| {
        let v = &mut s.visuals;
        v.panel_fill = Color32::from_rgb(244, 244, 248);
        v.window_fill = Color32::from_rgb(250, 250, 252);
        v.faint_bg_color = Color32::from_rgb(234, 235, 242);
        v.hyperlink_color = GOLD_DEEP;
        v.selection.bg_fill = Color32::from_rgb(246, 214, 140);
        v.selection.stroke = Stroke::new(1.0_f32, Color32::from_rgb(110, 72, 0));
        v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(190, 140, 40));
    });
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |s| {
            s.spacing.button_padding = egui::vec2(8.0, 3.0);
            s.spacing.item_spacing = egui::vec2(8.0, 5.0);
        });
    }
}

/// The accent for the current visuals.
pub fn accent(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        GOLD
    } else {
        GOLD_DEEP
    }
}

/// A primary (gold) button, for the one action a screen is about.
pub fn primary_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.into()).color(ON_GOLD).strong()).fill(GOLD_FILL)
}

/// The tint for a success line.
pub fn good(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(140, 214, 142)
    } else {
        Color32::from_rgb(30, 120, 40)
    }
}

//! The Legaia look: egui's light and dark visuals with the palette and
//! spacing the game's own launcher draws in (`legaia_app_theme`), so the
//! two programs in a release folder read as one family.

use eframe::egui::{self, Color32, Stroke};
use legaia_app_theme::{DARK, LIGHT, Palette, Rgb, spacing};

/// A palette colour as egui's.
pub fn c(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb.0, rgb.1, rgb.2)
}

/// Install the Legaia visuals for both themes; egui then picks one by the
/// theme preference (`View` menu).
pub fn install(ctx: &egui::Context) {
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        let p = &DARK;
        let v = &mut s.visuals;
        surfaces(v, p);
        v.extreme_bg_color = c(p.ground);
        v.code_bg_color = c(p.raised);
        v.widgets.noninteractive.bg_fill = c(p.card);
        v.widgets.inactive.weak_bg_fill = c(p.raised);
        v.widgets.inactive.bg_fill = c(p.raised);
        v.widgets.hovered.weak_bg_fill = c(p.hover);
        v.widgets.hovered.bg_fill = c(p.hover);
        v.widgets.active.weak_bg_fill = c(p.pressed);
        v.widgets.active.bg_fill = c(p.pressed);
        v.widgets.open.weak_bg_fill = c(p.open);
        v.warn_fg_color = c(p.warn);
    });
    // The light theme keeps egui's own widget greys; only the surfaces, the
    // accent and the selection take the palette.
    ctx.style_mut_of(egui::Theme::Light, |s| surfaces(&mut s.visuals, &LIGHT));
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |s| {
            s.spacing.button_padding = spacing::BUTTON_PADDING.into();
            s.spacing.item_spacing = spacing::ITEM_SPACING.into();
        });
    }
}

/// The roles both themes take from the palette.
fn surfaces(v: &mut egui::Visuals, p: &Palette) {
    v.panel_fill = c(p.band);
    v.window_fill = c(p.card);
    v.faint_bg_color = c(p.stripe);
    v.hyperlink_color = c(p.accent);
    v.selection.bg_fill = c(p.selection);
    v.selection.stroke = Stroke::new(1.0_f32, c(p.selection_stroke));
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, c(p.hover_stroke));
}

/// The palette for the current visuals.
pub fn palette(ui: &egui::Ui) -> &'static Palette {
    legaia_app_theme::palette(ui.visuals().dark_mode)
}

/// The accent for the current visuals.
pub fn accent(ui: &egui::Ui) -> Color32 {
    c(palette(ui).accent)
}

/// A primary (gold) button, for the one action a screen is about.
pub fn primary_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(
        egui::RichText::new(text.into())
            .color(c(DARK.on_accent))
            .strong(),
    )
    .fill(c(DARK.accent_fill))
}

/// The tint for a success line.
pub fn good(ui: &egui::Ui) -> Color32 {
    c(palette(ui).good)
}

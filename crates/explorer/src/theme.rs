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
    });
    // The light theme keeps egui's own text and code greys; the surfaces,
    // the buttons' states, the accent, the selection and the control edges
    // take the palette, so a button reads as the launcher's in either theme
    // (a pressed one goes gold, not egui's mid grey).
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
    // A control at rest gets an edge. Without one, a check box is a square
    // 1.1:1 (light) or 1.3:1 (dark) against the panel - an unticked box all
    // but disappears - and a text field is a borderless strip.
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, c(p.outline));
    // egui's own error red is pure #FF0000 in both themes: 3.9:1 on the
    // light panel, and harsher than every other colour on the dark one.
    v.error_fg_color = c(p.error);
    // Weak text (the start page's drop hint and no-game-data line, the
    // model view's orbit hint) is the palette's hint ink. egui's own is the
    // body grey at 60% alpha: 2.7:1 on the dark panel and 2.8:1 on the
    // light one, under the WCAG AA 4.5:1 the palette holds its hints to.
    v.weak_text_color = Some(c(p.ink_faint));
    v.widgets.inactive.weak_bg_fill = c(p.raised);
    v.widgets.inactive.bg_fill = c(p.raised);
    v.widgets.hovered.weak_bg_fill = c(p.hover);
    v.widgets.hovered.bg_fill = c(p.hover);
    v.widgets.active.weak_bg_fill = c(p.pressed);
    v.widgets.active.bg_fill = c(p.pressed);
    v.widgets.open.weak_bg_fill = c(p.open);
    v.warn_fg_color = c(p.warn);
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

/// The tint for a warning line.
pub fn warn(ui: &egui::Ui) -> Color32 {
    c(palette(ui).warn)
}

/// The tint for a success line.
pub fn good(ui: &egui::Ui) -> Color32 {
    c(palette(ui).good)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A button's states are the palette's in both themes, as the
    /// launcher draws its buttons, and its text holds AA on each.
    #[test]
    fn buttons_take_the_palette_in_both_themes() {
        let ctx = egui::Context::default();
        install(&ctx);
        for (theme, p) in [(egui::Theme::Dark, &DARK), (egui::Theme::Light, &LIGHT)] {
            let w = ctx.style_of(theme).visuals.widgets.clone();
            for (what, fill, want) in [
                ("rest", w.inactive.weak_bg_fill, p.raised),
                ("hover", w.hovered.weak_bg_fill, p.hover),
                ("pressed", w.active.weak_bg_fill, p.pressed),
            ] {
                let got = Rgb(fill.r(), fill.g(), fill.b());
                assert_eq!(got, want, "{theme:?}: a button at {what}");
                let text = w.inactive.fg_stroke.color;
                let text = Rgb(text.r(), text.g(), text.b());
                assert!(
                    text.contrast(got) >= 4.5,
                    "{theme:?}: button text at {what} is {:.2}:1",
                    text.contrast(got)
                );
            }
        }
    }

    /// Weak text reads on every surface Explorer draws it over, in both
    /// themes: WCAG AA (4.5:1), the floor `legaia_app_theme` holds its hint
    /// ink to, which egui's own alpha-faded grey missed.
    #[test]
    fn weak_text_meets_the_hint_floor_in_both_themes() {
        let ctx = egui::Context::default();
        install(&ctx);
        for (theme, p) in [(egui::Theme::Dark, &DARK), (egui::Theme::Light, &LIGHT)] {
            let v = ctx.style_of(theme).visuals.clone();
            let weak = v.weak_text_color();
            let weak = Rgb(weak.r(), weak.g(), weak.b());
            assert_eq!(weak, p.ink_faint, "{theme:?}: weak text is the hint ink");
            for (what, surface) in [("panel", v.panel_fill), ("window", v.window_fill)] {
                let s = Rgb(surface.r(), surface.g(), surface.b());
                assert!(
                    weak.contrast(s) >= 4.5,
                    "{theme:?}: weak text on the {what} is {:.2}:1",
                    weak.contrast(s)
                );
            }
        }
    }
}

//! The Legaia look of the desktop programs: one palette, in a dark and a
//! light variant, and the spacing values, shared by `legaia-studio`, the
//! game's launcher (`legaia-engine` with no subcommand) and the play
//! window's own chrome (the key hint, the toggle notices, the `F12` panel).
//!
//! The dark palette is the launcher's: a deep blue-grey ground, lighter card
//! and button tones, and the gold of the game's title lettering for the
//! accent and the primary button. The light palette carries the same hues on
//! a white ground, with the accent darkened until it reads on white.
//!
//! No dependencies: each consumer converts a [`Rgb`] into its toolkit's
//! colour (`egui::Color32::from_rgb`, a `[f32; 4]` for the launcher's
//! software canvas and the play window's text layer).

#![no_std]

/// An sRGB colour, 8 bits per channel, opaque.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// Normalised `[r, g, b, 1.0]`, still sRGB-encoded (what a colour
    /// multiplied into an sRGB texture expects).
    pub const fn to_f32(self) -> [f32; 4] {
        [
            self.0 as f32 / 255.0,
            self.1 as f32 / 255.0,
            self.2 as f32 / 255.0,
            1.0,
        ]
    }

    /// WCAG relative luminance, `0..=1`.
    pub fn luminance(self) -> f32 {
        fn lin(c: u8) -> f32 {
            let c = c as f32 / 255.0;
            if c <= 0.039_28 {
                c / 12.92
            } else {
                // (c + 0.055) / 1.055 raised to 2.4, without `std`'s `powf`.
                pow_2_4((c + 0.055) / 1.055)
            }
        }
        0.2126 * lin(self.0) + 0.7152 * lin(self.1) + 0.0722 * lin(self.2)
    }

    /// WCAG contrast ratio against `other`, `1..=21`.
    pub fn contrast(self, other: Rgb) -> f32 {
        let (a, b) = (self.luminance(), other.luminance());
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }
}

/// `x^2.4` for `x` in `0..=1`: `x^2 * x^0.4`, the fractional power by a few
/// Newton steps on `y^5 = x^2`.
fn pow_2_4(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    let target = x * x;
    let mut y = x;
    for _ in 0..30 {
        let y4 = y * y * y * y;
        y -= (y4 * y - target) / (5.0 * y4);
    }
    x * x * y
}

/// Every colour role the desktop programs draw with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// The deepest surface: the launcher's body, Studio's text fields and
    /// viewports.
    pub ground: Rgb,
    /// Panels and bands (Studio's side and top panels, the launcher's
    /// header band).
    pub band: Rgb,
    /// Cards and windows over a band.
    pub card: Rgb,
    /// The alternate row of a striped grid.
    pub stripe: Rgb,
    /// A button at rest.
    pub raised: Rgb,
    /// A button under the pointer or keyboard focus.
    pub hover: Rgb,
    /// A button held down.
    pub pressed: Rgb,
    /// An open combo box or menu button.
    pub open: Rgb,
    /// Separators and outlines.
    pub rule: Rgb,
    /// Body text.
    pub ink: Rgb,
    /// Secondary text.
    pub ink_dim: Rgb,
    /// Hints and disabled text.
    pub ink_faint: Rgb,
    /// The title gold: headings, links, the selection outline.
    pub accent: Rgb,
    /// The primary button's fill.
    pub accent_fill: Rgb,
    /// The primary button's fill under focus.
    pub accent_fill_focus: Rgb,
    /// Text on an accent fill.
    pub on_accent: Rgb,
    /// A selected row's fill.
    pub selection: Rgb,
    /// A selected row's outline.
    pub selection_stroke: Rgb,
    /// The outline of a widget under the pointer.
    pub hover_stroke: Rgb,
    /// Success lines.
    pub good: Rgb,
    /// Warnings.
    pub warn: Rgb,
    /// Key names and informational lines.
    pub info: Rgb,
}

/// The dark palette: the launcher's.
pub const DARK: Palette = Palette {
    ground: Rgb(14, 15, 24),
    band: Rgb(22, 24, 38),
    card: Rgb(28, 31, 46),
    stripe: Rgb(28, 31, 46),
    raised: Rgb(38, 41, 61),
    hover: Rgb(51, 59, 92),
    pressed: Rgb(70, 62, 40),
    open: Rgb(46, 50, 74),
    rule: Rgb(61, 66, 92),
    ink: Rgb(237, 237, 245),
    ink_dim: Rgb(153, 163, 189),
    ink_faint: Rgb(107, 115, 138),
    accent: Rgb(255, 209, 92),
    accent_fill: Rgb(224, 179, 66),
    accent_fill_focus: Rgb(255, 214, 102),
    on_accent: Rgb(26, 20, 10),
    selection: Rgb(120, 92, 30),
    selection_stroke: Rgb(255, 209, 92),
    hover_stroke: Rgb(200, 160, 70),
    good: Rgb(128, 219, 133),
    warn: Rgb(255, 148, 92),
    info: Rgb(179, 217, 255),
};

/// The light palette: the same hues on white, the accent darkened to read
/// on it.
pub const LIGHT: Palette = Palette {
    ground: Rgb(255, 255, 255),
    band: Rgb(244, 244, 248),
    card: Rgb(250, 250, 252),
    stripe: Rgb(234, 235, 242),
    raised: Rgb(230, 231, 238),
    hover: Rgb(214, 217, 230),
    pressed: Rgb(246, 226, 180),
    open: Rgb(222, 224, 234),
    rule: Rgb(200, 203, 215),
    ink: Rgb(24, 26, 36),
    ink_dim: Rgb(80, 86, 104),
    ink_faint: Rgb(120, 126, 142),
    accent: Rgb(150, 100, 12),
    accent_fill: Rgb(224, 179, 66),
    accent_fill_focus: Rgb(240, 196, 84),
    on_accent: Rgb(26, 20, 10),
    selection: Rgb(246, 214, 140),
    selection_stroke: Rgb(110, 72, 0),
    hover_stroke: Rgb(190, 140, 40),
    good: Rgb(30, 120, 40),
    warn: Rgb(170, 80, 10),
    info: Rgb(30, 90, 170),
};

/// The palette for a dark or a light surface.
pub const fn palette(dark: bool) -> &'static Palette {
    if dark { &DARK } else { &LIGHT }
}

/// Spacing, in logical points (egui) or stage pixels (the launcher and the
/// play window, whose stages are scaled up whole).
pub mod spacing {
    /// Inside a button: horizontal, vertical.
    pub const BUTTON_PADDING: [f32; 2] = [8.0, 3.0];
    /// Between neighbouring widgets: horizontal, vertical.
    pub const ITEM_SPACING: [f32; 2] = [8.0, 5.0];
    /// Between a section's heading and its body, and between sections.
    pub const SECTION_GAP: f32 = 6.0;
    /// The play window's chrome: the inset of its bottom-left lines from the
    /// stage edge, and the gap between stacked lines (stage pixels).
    pub const CHROME_INSET: i32 = 6;
    pub const CHROME_LINE_GAP: i32 = 4;
}

/// The play window's own chrome over the game (dark always: it sits on a
/// scrim over whatever the game draws).
pub mod chrome {
    use super::{DARK, Rgb};
    /// Headings and prompts (*Press Esc again to quit*, the key hint).
    pub const HEADING: Rgb = DARK.accent;
    /// Key names in the help panel, and a toggle's notice line.
    pub const KEY: Rgb = DARK.info;
    /// Body text of the help panel.
    pub const TEXT: Rgb = DARK.ink;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every role an egui or canvas surface draws text or a fill under.
    fn check(p: &Palette, name: &str) {
        let body = [p.ground, p.band, p.card];
        for s in body {
            assert!(p.ink.contrast(s) >= 7.0, "{name}: ink on {s:?}");
            assert!(p.ink_dim.contrast(s) >= 4.5, "{name}: ink_dim on {s:?}");
            assert!(p.accent.contrast(s) >= 4.5, "{name}: accent on {s:?}");
            assert!(p.good.contrast(s) >= 4.5, "{name}: good on {s:?}");
            assert!(p.warn.contrast(s) >= 4.5, "{name}: warn on {s:?}");
            assert!(p.info.contrast(s) >= 4.5, "{name}: info on {s:?}");
            assert!(p.ink_faint.contrast(s) >= 3.0, "{name}: ink_faint on {s:?}");
        }
        assert!(p.ink.contrast(p.raised) >= 7.0, "{name}: button text");
        assert!(
            p.ink.contrast(p.hover) >= 4.5,
            "{name}: hovered button text"
        );
        assert!(
            p.ink.contrast(p.selection) >= 4.5,
            "{name}: selected row text"
        );
        for f in [p.accent_fill, p.accent_fill_focus] {
            assert!(
                p.on_accent.contrast(f) >= 7.0,
                "{name}: primary button text"
            );
        }
    }

    #[test]
    fn both_palettes_meet_text_contrast() {
        check(&DARK, "dark");
        check(&LIGHT, "light");
    }

    #[test]
    fn contrast_matches_the_wcag_reference_points() {
        let black = Rgb(0, 0, 0);
        let white = Rgb(255, 255, 255);
        assert!((black.contrast(white) - 21.0).abs() < 0.05);
        assert!((white.contrast(white) - 1.0).abs() < 1e-4);
        // #777 on white is the textbook 4.48.
        assert!((Rgb(0x77, 0x77, 0x77).contrast(white) - 4.48).abs() < 0.02);
    }

    #[test]
    fn launcher_values_survive_the_round_trip() {
        // The launcher's palette was written as floats; the tokens are those
        // floats to the nearest 8-bit step.
        let close = |c: Rgb, f: [f32; 3]| {
            let g = c.to_f32();
            (0..3).all(|i| (g[i] - f[i]).abs() <= 0.5 / 255.0 + 1e-6)
        };
        assert!(close(DARK.accent, [1.0, 0.82, 0.36]));
        assert!(close(DARK.ground, [0.055, 0.06, 0.095]));
        assert!(close(DARK.card, [0.11, 0.12, 0.18]));
        assert!(close(DARK.ink, [0.93, 0.93, 0.96]));
        assert!(close(DARK.accent_fill, [0.88, 0.70, 0.26]));
        assert_eq!(palette(true), &DARK);
        assert_eq!(palette(false), &LIGHT);
    }
}

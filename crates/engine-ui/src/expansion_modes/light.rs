//! The port's menu screens on a light window chrome.
//!
//! The Multiplayer screen and the game-mode picker are menus the port adds,
//! drawn in one dark palette (`ink`, `pen`). The native window's own chrome
//! follows the desktop's theme, and beside a light `F12` panel a dark
//! Multiplayer screen read as a different program. A host on a light chrome
//! runs the screen's draws through [`light_prims`] / [`light_texts`], which
//! swap each palette colour for its light counterpart and leave every other
//! colour alone - a card face, the felt, a portrait.
//!
//! Only for a screen that covers the stage with its own backdrop: a mode in
//! play draws light text straight over the field and over translucent black
//! strips, where dark text would not read. The browser play page has no
//! window chrome and keeps the dark palette.

use super::{ink, pen};
use crate::TextDraw;
use legaia_render_kernels::screen_prim::ScreenPrim;

/// The light counterparts of the menu palette, as a host's theme names
/// them. Fills are RGB bytes, text is RGB in `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightChrome {
    /// The stage behind every panel ([`ink::BACKDROP`]).
    pub backdrop: [u8; 3],
    /// The title and footer bars ([`ink::BAR`]).
    pub bar: [u8; 3],
    /// A panel's fill ([`ink::PANEL`]).
    pub panel: [u8; 3],
    /// A panel's edge and the bars' rules ([`ink::PANEL_EDGE`]).
    pub edge: [u8; 3],
    /// The focused row's fill ([`ink::SELECT`]).
    pub select: [u8; 3],
    /// The focused row's marker ([`ink::HIGHLIGHT`]).
    pub highlight: [u8; 3],
    /// Body text ([`pen::WHITE`]).
    pub text: [f32; 3],
    /// Secondary text ([`pen::DIM`]).
    pub dim: [f32; 3],
    /// Headings and the focused value ([`pen::GOLD`]).
    pub accent: [f32; 3],
    /// A success line ([`pen::GREEN`]).
    pub good: [f32; 3],
    /// A failure line ([`pen::RED`]).
    pub bad: [f32; 3],
}

/// `c` when it is one of the palette's fills, as its light counterpart
/// with its own alpha; any other colour unchanged.
fn light_fill(c: [u8; 4], l: &LightChrome) -> [u8; 4] {
    let to = |rgb: [u8; 3]| [rgb[0], rgb[1], rgb[2], c[3]];
    match c {
        ink::BACKDROP => to(l.backdrop),
        ink::BAR => to(l.bar),
        ink::PANEL => to(l.panel),
        ink::PANEL_EDGE => to(l.edge),
        ink::SELECT => to(l.select),
        ink::HIGHLIGHT => to(l.highlight),
        other => other,
    }
}

/// `c` when it is one of the palette's text colours, as its light
/// counterpart with its own alpha; any other colour unchanged.
fn light_pen(c: [f32; 4], l: &LightChrome) -> [f32; 4] {
    let is = |p: [f32; 4]| c[0] == p[0] && c[1] == p[1] && c[2] == p[2];
    let to = |rgb: [f32; 3]| [rgb[0], rgb[1], rgb[2], c[3]];
    if is(pen::WHITE) {
        to(l.text)
    } else if is(pen::DIM) {
        to(l.dim)
    } else if is(pen::GOLD) {
        to(l.accent)
    } else if is(pen::GREEN) {
        to(l.good)
    } else if is(pen::RED) {
        to(l.bad)
    } else {
        c
    }
}

/// A menu screen's flat prims in the light palette.
pub fn light_prims(prims: &mut [ScreenPrim], l: &LightChrome) {
    for p in prims {
        if let ScreenPrim::Flat(q) = p {
            q.color = light_fill(q.color, l);
            if let Some(corners) = q.gouraud.as_mut() {
                for c in corners.iter_mut() {
                    *c = light_fill(*c, l);
                }
            }
        }
    }
}

/// A menu screen's labels in the light palette.
pub fn light_texts(texts: &mut [TextDraw], l: &LightChrome) {
    for t in texts {
        t.color = light_pen(t.color, l);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const L: LightChrome = LightChrome {
        backdrop: [244, 244, 248],
        bar: [234, 235, 242],
        panel: [250, 250, 252],
        edge: [128, 132, 148],
        select: [246, 214, 140],
        highlight: [110, 72, 0],
        text: [0.1, 0.1, 0.14],
        dim: [0.31, 0.34, 0.41],
        accent: [0.55, 0.36, 0.04],
        good: [0.12, 0.47, 0.16],
        bad: [0.69, 0.13, 0.13],
    };

    #[test]
    fn every_menu_fill_and_pen_has_a_light_counterpart() {
        for c in [
            ink::BACKDROP,
            ink::BAR,
            ink::PANEL,
            ink::PANEL_EDGE,
            ink::SELECT,
            ink::HIGHLIGHT,
        ] {
            assert_ne!(light_fill(c, &L), c, "{c:?}");
        }
        for c in [pen::WHITE, pen::DIM, pen::GOLD, pen::GREEN, pen::RED] {
            assert_ne!(light_pen(c, &L), c, "{c:?}");
        }
    }

    #[test]
    fn a_colour_off_the_palette_is_left_alone() {
        for c in [ink::FELT, ink::FELT_EDGE, ink::SHADE, [1, 2, 3, 255]] {
            assert_eq!(light_fill(c, &L), c);
        }
        assert_eq!(light_pen(pen::INK, &L), pen::INK);
        assert_eq!(light_pen([0.5, 0.5, 0.5, 1.0], &L), [0.5, 0.5, 0.5, 1.0]);
    }

    #[test]
    fn alpha_rides_through() {
        let faded = [pen::WHITE[0], pen::WHITE[1], pen::WHITE[2], 0.25];
        assert_eq!(light_pen(faded, &L)[3], 0.25);
        let glass = [ink::PANEL[0], ink::PANEL[1], ink::PANEL[2], 120];
        // A translucent panel is not the palette's opaque one.
        assert_eq!(light_fill(glass, &L), glass);
    }

    /// Light text on a light fill, or dark on dark, is the failure this
    /// module exists to avoid: every text colour must stand off every fill
    /// it can sit on.
    #[test]
    fn light_text_stands_off_light_fills() {
        let lum = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let fill = |c: [u8; 3]| {
            lum([
                c[0] as f32 / 255.0,
                c[1] as f32 / 255.0,
                c[2] as f32 / 255.0,
            ])
        };
        for f in [L.backdrop, L.bar, L.panel, L.select] {
            for t in [L.text, L.dim, L.accent, L.good, L.bad] {
                assert!(fill(f) - lum(t) > 0.35, "{t:?} on {f:?}");
            }
        }
    }
}

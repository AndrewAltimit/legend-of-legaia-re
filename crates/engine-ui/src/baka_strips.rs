//! The Baka Fighter HUD's draws both play hosts make: the font digit strips,
//! the widget quads and the duel HUD's VITAL bars.
//!
//! The tally's number drawers - the 8 px right-aligned score field
//! (`FUN_801d6ef4`) and the `0x10` px "GET COIN" numeral strip
//! (`FUN_801d6f44`), plus the one-glyph round digit (`FUN_801d69e4`) on a
//! frame without the HUD art - draw as font glyphs at the ported cell x
//! ([`baka_digit_strip_draws_for`]): the layout is retail's, the glyph source
//! is not. The layout lives in
//! `legaia_engine_minigames::baka_fighter_chrome::hud_digit_placements`; this
//! module is the other half, the glyph quads. A shared *layout* is not a
//! shared draw, and the two hosts drew different things off one layout until
//! the quad emitter sat under both as well.
//!
//! The widget quads ([`baka_hud_prims`], [`baka_hud_prims_at`]) and the VITAL
//! bars ([`baka_bar_prim`]) are screen primitives against the duel VRAM,
//! which carries the PROT 1203 pages; `engine-screens::minigame_hud` composes
//! them into the frame's layer.

use crate::*;

/// Turn placed digits (`(stage x, stage y, digit)`) into glyph quads.
///
/// A digit above `9` is clamped, which is what the widget path does: the
/// descriptor row holds ten cells and the `u` patch is `digit * stride`.
pub fn baka_digit_strip_draws_for(
    font: &legaia_font::Font,
    placed: &[(i32, i32, u8)],
    color: [f32; 4],
) -> Vec<TextDraw> {
    let mut out: Vec<TextDraw> = Vec::new();
    for &(x, y, digit) in placed {
        let byte = [b'0' + digit.min(9)];
        let text = core::str::from_utf8(&byte).unwrap_or("0");
        let layout = font.layout_ascii(text);
        for g in &layout.glyphs {
            out.push(TextDraw {
                dst: (x + g.dst_x, y + g.dst_y, g.width, g.height),
                src: (g.atlas_x, g.atlas_y, g.width, g.height),
                color,
            });
        }
    }
    out
}

/// Centred text labels for the cabinet's widget draws a host has no sprite
/// page for - the round chrome (`baka_fighter_chrome::chrome_labels`) and
/// the "NEXT GAME / PAY OUT" sheet (`baka_cabinet::choice_sheet_labels`).
///
/// Each row is `(centre x, centre y, brightness, text)` in stage pixels.
/// Brightness is the widget's colour scale (`0x80` = the descriptor's own
/// RGB); it maps to alpha as `brightness / 0x80`, capped at 1, so the
/// sheet's `0x40` unpicked side reads at half strength.
pub fn baka_widget_label_draws_for(
    font: &legaia_font::Font,
    labels: &[(i32, i32, i32, String)],
    color: [f32; 4],
) -> Vec<TextDraw> {
    let mut out: Vec<TextDraw> = Vec::new();
    for (x, y, brightness, text) in labels {
        let alpha = (*brightness as f32 / 128.0).clamp(0.0, 1.0);
        let layout = font.layout_ascii(text);
        let w: i32 = layout
            .glyphs
            .iter()
            .map(|g| g.dst_x + g.width as i32)
            .max()
            .unwrap_or(0);
        let h: i32 = layout
            .glyphs
            .iter()
            .map(|g| g.dst_y + g.height as i32)
            .max()
            .unwrap_or(0);
        let (ox, oy) = (x - w / 2, y - h / 2);
        for g in &layout.glyphs {
            out.push(TextDraw {
                dst: (ox + g.dst_x, oy + g.dst_y, g.width, g.height),
                src: (g.atlas_x, g.atlas_y, g.width, g.height),
                color: [color[0], color[1], color[2], color[3] * alpha],
            });
        }
    }
    out
}

/// Ordering-table bucket every Baka Fighter HUD emit links at: retail's
/// emitter `FUN_801d5ed0` links each packet into `_DAT_801DBEBC` and bumps
/// that slot to `3`.
pub const BAKA_HUD_OT: u32 = 3;

/// The cabinet's and the round chrome's widget quads
/// (`legaia_engine_minigames::baka_fighter_chrome::hud_widget_quads`, one per
/// `FUN_801d5ed0` call) as screen-space PSX primitives against the duel
/// VRAM, linked at [`BAKA_HUD_OT`]. The view is the dance HUD's, because the
/// two emitters build the same `POLY_GT4`: packet corners, per-corner UVs,
/// a top / bottom colour pair, CLUT and the ABR-folded texpage.
///
/// REF: FUN_801d5ed0
pub fn baka_hud_prims(
    quads: &[crate::dance::DanceHudQuadView],
) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
    crate::dance::dance_hud_prims(quads, BAKA_HUD_OT)
}

/// [`baka_hud_prims`] at an explicit ordering-table bucket: the duel HUD
/// renderer (`FUN_801d2afc`) links its own cells - bar frames, pips, combo
/// digits, attack icons - one bucket farther than the widget emitter.
pub fn baka_hud_prims_at(
    quads: &[crate::dance::DanceHudQuadView],
    ot_index: u32,
) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
    crate::dance::dance_hud_prims(quads, ot_index)
}

/// One VITAL fill bar of the Baka Fighter duel HUD: packet corners and the
/// colour at each end (`FUN_801d2afc`, `0x801D2B44..0x801D2C38`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BakaBarView {
    /// Corners, `x0 .. x1` by `y0 .. y1`, stage pixels.
    pub rect: (i16, i16, i16, i16),
    /// Gouraud colour of the left edge (vertices 0 and 2).
    pub rgb_left: [u8; 3],
    /// Gouraud colour of the right edge (vertices 1 and 3).
    pub rgb_right: [u8; 3],
}

/// The 5-bit grey every channel of the bars' texel holds: the bar packet is
/// a `POLY_GT4` sampling texpage `0x1F` `uv (0, 0xF0)..(2, 0xF2)` through
/// CLUT `0x7F84`, the resident system patch whose texel is BGR555 grey `14`
/// (`112` in 8-bit; `legaia_ui_formats::title_pak::OVERLAY_SAVE_UI_PROGRESS_TEXEL_GREY`).
/// The duel VRAM does not carry that page, so the bar draws untextured with
/// each vertex colour pre-modulated the way the GPU modulates the texel:
/// `(14 * c) >> 7` in 5 bits. Retail's full bar end `0xBC` comes out `160`,
/// the red a retail duel capture measures.
pub const BAKA_BAR_TEXEL_5BIT: u32 = 14;

/// Modulate an 8-bit vertex channel by the bars' grey texel, as the GPU does.
pub fn baka_bar_channel(c: u8) -> u8 {
    (((BAKA_BAR_TEXEL_5BIT * u32::from(c)) >> 7).min(31) << 3) as u8
}

/// The VITAL bar as a screen primitive: an untextured gouraud quad carrying
/// the texel-modulated colours ([`BAKA_BAR_TEXEL_5BIT`]), at `ot_index`.
pub fn baka_bar_prim(
    bar: &BakaBarView,
    ot_index: u32,
) -> legaia_render_kernels::screen_prim::ScreenPrim {
    let (x0, y0, x1, y1) = bar.rect;
    let m = |c: [u8; 3]| {
        [
            baka_bar_channel(c[0]),
            baka_bar_channel(c[1]),
            baka_bar_channel(c[2]),
            0xFF,
        ]
    };
    let (l, r) = (m(bar.rgb_left), m(bar.rgb_right));
    legaia_render_kernels::screen_prim::ScreenPrim::Flat(
        legaia_render_kernels::screen_prim::FlatQuad {
            xy: [(x0, y0), (x1, y0), (x0, y1), (x1, y1)],
            color: l,
            gouraud: Some([l, r, l, r]),
            semi_transparent: false,
            abr_mode: 0,
            ot_index,
            depth: None,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_placed_digit_becomes_at_least_one_quad_at_its_pen() {
        let font = legaia_font::synthetic_for_tests();
        let out = baka_digit_strip_draws_for(&font, &[(10, 20, 4), (26, 20, 2)], [1.0; 4]);
        assert!(out.len() >= 2);
        let xs: std::collections::BTreeSet<i32> = out.iter().map(|d| d.dst.0).collect();
        assert!(xs.iter().any(|&x| (10..26).contains(&x)));
        assert!(xs.iter().any(|&x| x >= 26));
    }

    #[test]
    fn a_widget_label_is_centred_on_its_pen_and_dimmed_by_brightness() {
        let font = legaia_font::synthetic_for_tests();
        let lit = baka_widget_label_draws_for(&font, &[(100, 50, 0xA0, "AB".into())], [1.0; 4]);
        let dim = baka_widget_label_draws_for(&font, &[(100, 50, 0x40, "AB".into())], [1.0; 4]);
        assert!(!lit.is_empty());
        let left = lit.iter().map(|d| d.dst.0).min().unwrap();
        let right = lit.iter().map(|d| d.dst.0 + d.dst.2 as i32).max().unwrap();
        assert!(left < 100 && right > 100, "straddles its centre");
        assert_eq!(lit[0].color[3], 1.0);
        assert_eq!(dim[0].color[3], 0.5);
    }

    /// The full bar end `0xBC` modulates to the `160` a retail duel capture
    /// measures, and the bar is a gouraud quad with its two edge colours.
    #[test]
    fn the_vital_bar_modulates_by_the_grey_texel() {
        assert_eq!(baka_bar_channel(0xBC), 160);
        assert_eq!(baka_bar_channel(0), 0);
        let p = baka_bar_prim(
            &BakaBarView {
                rect: (0x25, 0x26, 0x89, 0x2B),
                rgb_left: [0xBC, 0x64, 0],
                rgb_right: [0xBC, 0, 0],
            },
            5,
        );
        let legaia_render_kernels::screen_prim::ScreenPrim::Flat(q) = p else {
            panic!("untextured");
        };
        let g = q.gouraud.expect("gouraud");
        assert_eq!(g[0], g[2]);
        assert_eq!(g[1], [160, 0, 0, 0xFF]);
        assert_eq!(q.ot_index, 5);
    }

    #[test]
    fn a_digit_above_nine_clamps_rather_than_running_off_the_row() {
        let font = legaia_font::synthetic_for_tests();
        let nine = baka_digit_strip_draws_for(&font, &[(0, 0, 9)], [1.0; 4]);
        let over = baka_digit_strip_draws_for(&font, &[(0, 0, 200)], [1.0; 4]);
        assert_eq!(nine.len(), over.len());
        assert_eq!(nine[0].src, over[0].src);
    }
}

//! The duel HUD's three digit strips, placed, and the chrome labels.

use super::*;

// ---------------------------------------------------------------------------
// The duel HUD's three digit strips, placed
// ---------------------------------------------------------------------------

/// Stage pen of the one-glyph round digit.
pub const HUD_ROUND_PEN: (i32, i32) = (8, 98);
/// Stage pen of the 8 px right-aligned score field.
pub const HUD_SCORE_PEN: (i32, i32) = (40, 98);
/// Stage pen of the `0x10` px "GET COIN" numeral strip.
pub const HUD_COIN_PEN: (i32, i32) = (140, 98);
/// The round digit is one glyph, so a round past the row's ten cells has
/// nowhere to go; retail's widget row holds ten and the port clamps.
pub const HUD_ROUND_MAX: i32 = 9;

/// Where the duel HUD's three number drawers put their glyphs this frame, as
/// `(stage x, stage y, digit)`.
///
/// The three strips are `single_digit_cell` (the round), the right-aligned
/// score field and the coin strip, each at its own pen with its own cell
/// stride - and the two tally strips draw only while a match tally is up,
/// which is what `tally` carries: `(total, gold_remaining)`.
///
/// This is the *layout*. The glyph quads are
/// `legaia_engine_ui::baka_strips::baka_digit_strip_draws_for`, which both
/// hosts call: a shared layout under two different emitters is exactly the
/// shape that let one host draw a summary line where the other drew the
/// retail cells.
pub fn hud_digit_placements(round: i32, tally: Option<(i32, i32)>) -> Vec<(i32, i32, u8)> {
    use crate::baka_fighter::{coin_digit_cells, right_aligned_number_cells, single_digit_cell};
    let mut out: Vec<(i32, i32, u8)> = Vec::new();
    let r = single_digit_cell((round + 1).clamp(0, HUD_ROUND_MAX) as u8);
    out.push((
        HUD_ROUND_PEN.0 + r.x_offset as i32,
        HUD_ROUND_PEN.1,
        r.digit,
    ));
    if let Some((total, gold_remaining)) = tally {
        for c in right_aligned_number_cells(total) {
            out.push((
                HUD_SCORE_PEN.0 + c.x_offset as i32,
                HUD_SCORE_PEN.1,
                c.digit,
            ));
        }
        for c in coin_digit_cells(gold_remaining) {
            out.push((HUD_COIN_PEN.0 + c.x_offset as i32, HUD_COIN_PEN.1, c.digit));
        }
    }
    out
}

/// The cabinet's widget draws and the round chrome's, as the retail
/// emitter's `POLY_GT4` packets.
///
/// Each `cells` entry is one `FUN_801D5ED0(x, y, widget, brightness, 0x1000)`
/// call of the cabinet (the attract "PRESS START" prompt, the "PLAYER
/// SELECT" banner, the "NEXT GAME / PAY OUT" sheet - `0x1000` is the size
/// literal the attract arm stores at `0x801CF760`); each `draws` entry is one
/// call of the round chrome ([`ChromeDraw`]: the intro title card, the ROUND
/// banner, the countdown). A glyph draw first pages the [`GLYPH_WIDGET`]
/// strip, `u = glyph_u(n)` - the byte store at [`GLYPH_U_VA`] retail makes
/// before it emits - on a copy of the record, so the table stays pristine.
/// Cells come first: the attract arm draws its prompt before it calls the
/// title card. A widget id past the table is skipped.
///
/// The quads sample the PROT 1203 art pages through the widget's own
/// texpage / CLUT words, so a host draws them only while those pages are
/// resident - which the duel VRAM makes them
/// (`legaia_engine_minigame_scenes::baka_duel_scene::BakaDuelSurface::hud_quads`).
///
/// REF: FUN_801d5ed0, FUN_801cf388, FUN_801d59d4
pub fn hud_widget_quads(
    widgets: &[legaia_minigame_formats::baka_opponents::BakaHudWidget],
    draws: &[ChromeDraw],
    cells: &[crate::baka_cabinet::SheetCell],
) -> Vec<crate::baka_fighter::HudWidgetQuad> {
    const CELL_SIZE: i32 = 0x1000;
    let mut out = Vec::with_capacity(cells.len() + draws.len());
    for c in cells {
        if let Some(w) = widgets.get(usize::from(c.widget)) {
            out.push(crate::baka_fighter::hud_widget_quad(
                w,
                c.x as i16,
                c.y as i16,
                c.brightness,
                CELL_SIZE,
                false,
            ));
        }
    }
    for d in draws {
        let Some(w) = widgets.get(usize::from(d.widget)) else {
            continue;
        };
        let mut w = *w;
        if let Some(n) = d.glyph {
            w.u = glyph_u(n);
        }
        out.push(crate::baka_fighter::hud_widget_quad(
            &w,
            d.x,
            d.y,
            d.brightness,
            d.size,
            false,
        ));
    }
    out
}

/// The round chrome's draws as `(centre x, centre y, brightness, text)` rows
/// for a glyph-less host: a glyph draw shows its paged cell's digit, any
/// other widget its id. The play hosts draw [`hud_widget_quads`] instead
/// whenever the duel VRAM carries the HUD pages; these labels are the
/// fallback for a frame without them - one label kernel, so the two hosts
/// cannot drift on what the chrome says.
pub fn chrome_labels(draws: &[ChromeDraw]) -> Vec<(i32, i32, i32, String)> {
    draws
        .iter()
        .map(|d| {
            let text = match d.glyph {
                Some(idx) => format!("{}", idx.rem_euclid(10)),
                None => format!("w{:02x}", d.widget),
            };
            (i32::from(d.x), i32::from(d.y), d.brightness, text)
        })
        .collect()
}

#[cfg(test)]
mod hud_strip_tests {
    use super::*;

    fn widget(u: u8) -> legaia_minigame_formats::baka_opponents::BakaHudWidget {
        legaia_minigame_formats::baka_opponents::BakaHudWidget {
            scale: 0x1000,
            texpage: 5,
            clut: 0x7780,
            u,
            v: 16,
            w: 24,
            h: 16,
            rgb_top: [0x80; 3],
            semi: 1,
            rgb_bottom: [0x80; 3],
            abr: 1,
        }
    }

    /// Cells draw first at the cabinet's fixed size, then the chrome's
    /// draws; a glyph draw pages widget 5's `u` on a copy, and an id past
    /// the table is dropped rather than indexed.
    #[test]
    fn widget_quads_page_glyphs_and_keep_call_order() {
        let table: Vec<_> = (0..8).map(|i| widget(i * 2)).collect();
        let cells = [crate::baka_cabinet::SheetCell {
            widget: 0,
            x: 0xA0,
            y: 0xCC,
            brightness: 0x80,
        }];
        let draws = [
            ChromeDraw {
                widget: GLYPH_WIDGET,
                x: 100,
                y: 50,
                brightness: 0x80,
                size: 0x1000,
                glyph: Some(3),
            },
            ChromeDraw::plain(40, 0, 0, 0x80, 0x1000),
        ];
        let q = hud_widget_quads(&table, &draws, &cells);
        assert_eq!(q.len(), 2, "the out-of-table widget is skipped");
        // Cell: 24x16 centred on (0xA0, 0xCC).
        assert_eq!((q[0].x0, q[0].y0, q[0].x1, q[0].y1), (148, 196, 171, 211));
        assert_eq!(q[0].uv[0], (0, 16));
        // Glyph: u paged to 3 * 24.
        assert_eq!(q[1].uv[0], (glyph_u(3), 16));
        assert_eq!(q[1].tpage_attr, 5 + 0x20, "ABR folds into the texpage");
        assert_eq!(
            table[GLYPH_WIDGET as usize].u, 10,
            "the table stays pristine"
        );
    }

    #[test]
    fn without_a_tally_only_the_round_digit_is_placed() {
        let out = hud_digit_placements(0, None);
        assert_eq!(out, vec![(HUD_ROUND_PEN.0, HUD_ROUND_PEN.1, 1)]);
    }

    #[test]
    fn the_two_tally_strips_take_their_own_pens_and_strides() {
        let out = hud_digit_placements(2, Some((42, 7)));
        // Round 3, then "4" "2" 8 px apart, then the coin strip.
        assert_eq!(out[0], (HUD_ROUND_PEN.0, HUD_ROUND_PEN.1, 3));
        let score: Vec<&(i32, i32, u8)> = out
            .iter()
            .filter(|p| p.0 >= HUD_SCORE_PEN.0 && p.0 < HUD_COIN_PEN.0)
            .collect();
        assert_eq!(score.len(), 2);
        assert_eq!(score[1].0 - score[0].0, 8);
        let coin: Vec<&(i32, i32, u8)> = out.iter().filter(|p| p.0 >= HUD_COIN_PEN.0).collect();
        assert_eq!(coin.len(), 1);
        assert_eq!(coin[0].2, 7);
    }

    #[test]
    fn a_round_past_the_row_clamps_rather_than_indexing_off_it() {
        let out = hud_digit_placements(40, None);
        assert_eq!(out[0].2, HUD_ROUND_MAX as u8);
    }
}

//! The fishing **point-exchange screens** - one composition, both hosts.
//!
//! The venue's prize counter is three screens over the hub menu: the prize
//! list (`FUN_801D0C3C`), the quantity picker (`FUN_801D092C`) and the
//! confirm (`FUN_801D06C8`). Their layout is
//! `legaia_engine_fishing::fishing::PrizeExchange::list_draw` /
//! `dialog_layers`, in retail's 320x240 space; this module turns it into
//! dialog-font [`TextDraw`]s.
//!
//! Each screen sits on its own panel (`FUN_801D74B0`, widget style `0x44`,
//! [`venue_panel_draws_for`]), drawn by the hosts' chrome pass under every
//! text. A
//! later screen's panel covers the text of the screens under it (retail's
//! ordering table draws a routine's text over its own panel and the next
//! routine's panel over both), so [`occlude_text_draws`] crops the lower
//! screens' glyphs against the panels drawn above them.

use crate::*;

/// The list's ink for an available row (ink `7`).
pub const EXCHANGE_INK: [f32; 4] = crate::MENU_TEXT_WHITE;
/// The list's ink for a row [`legaia_engine_fishing`]'s availability refuses
/// (ink `0`).
pub const EXCHANGE_DIM_INK: [f32; 4] = crate::MENU_TEXT_GREY;

/// One prize-list row: the item name at its pen and the price field
/// `(value, places, x, y)`, both in the row's ink.
pub struct ExchangeListRow<'a> {
    pub name: &'a [u8],
    pub name_at: (i32, i32),
    pub price: (i64, i32, i32, i32),
    pub available: bool,
}

/// The prize list's text (`FUN_801D0C3C`): each row's name and price in ink
/// `7` or `0`, and the points label and the point pool in ink `7`. Its
/// cursor icon is a sprite ([`venue_hand_draw_for`]).
pub fn exchange_list_draws_for(
    font: &legaia_font::Font,
    rows: &[ExchangeListRow<'_>],
    points_label: (&[u8], i32, i32),
    points: (i64, i32, i32, i32),
) -> Vec<TextDraw> {
    let mut out = Vec::new();
    for r in rows {
        let ink = if r.available {
            EXCHANGE_INK
        } else {
            EXCHANGE_DIM_INK
        };
        out.extend(text_draws_for(&font.layout(r.name), r.name_at, ink));
        let (value, places, x, y) = r.price;
        out.extend(crate::menu::decimal_field::decimal_field_draws(
            font,
            value,
            (x, y),
            places,
            false,
            ink,
        ));
    }
    let (label, x, y) = points_label;
    out.extend(text_draws_for(&font.layout(label), (x, y), EXCHANGE_INK));
    let (value, places, x, y) = points;
    out.extend(crate::menu::decimal_field::decimal_field_draws(
        font,
        value,
        (x, y),
        places,
        false,
        EXCHANGE_INK,
    ));
    out
}

/// The exchange's quantity picker and confirm over the list, from the
/// layout `legaia_engine_fishing::fishing::PrizeExchange::dialog_layers`
/// puts in retail's 320x240 space: text at its pen (`FUN_80036888`), the
/// decimal fields `(value, places, x, y)` through the menu numerals
/// (`FUN_80034B78`) and separator glyph `6` (`/`, `FUN_8003C1F8`). Their
/// cursor icons are sprites ([`venue_hand_draw_for`]).
pub fn exchange_dialog_draws_for(
    font: &legaia_font::Font,
    texts: &[(&[u8], i32, i32)],
    numbers: &[(i64, i32, i32, i32)],
    slash: Option<(i32, i32)>,
    ink: [f32; 4],
) -> Vec<TextDraw> {
    let mut out = crate::text_lines::text_line_draws_for(
        font,
        texts.iter().map(|&(t, x, y)| (t, x, y, false)),
        ink,
        ink,
    );
    for &(value, places, x, y) in numbers {
        out.extend(crate::menu::decimal_field::decimal_field_draws(
            font,
            value,
            (x, y),
            places,
            false,
            ink,
        ));
    }
    if let Some(pen) = slash {
        out.extend(crate::menu::window_painters::separator_glyph_draws(
            font,
            crate::menu::window_painters::SEPARATOR_GLYPH_SLASH,
            pen,
            ink,
        ));
    }
    out
}

/// The venue screens' cursor icon `FUN_8002C488(x, y, 0x4E)`: widget record
/// `0x4E` is the 16x16 pointing hand (sheet `(0x98, 0x40)`, CLUT row 7) with
/// no pen offset, so its top-left is the pen - the save screen's hand
/// (`SaveMenuAtlasRects::cursor`).
pub fn venue_hand_draw_for(
    rects: &SaveMenuAtlasRects,
    pen: (i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> SpriteDraw {
    let s = stage_scale.max(1);
    let (_, _, w, h) = rects.cursor;
    SpriteDraw {
        dst: (
            stage_origin.0 + pen.0 * s as i32,
            stage_origin.1 + pen.1 * s as i32,
            w * s,
            h * s,
        ),
        src: rects.cursor,
        color: [1.0, 1.0, 1.0, 1.0],
    }
}

/// One venue panel: the box emitter `FUN_8002C69C(x, y, w, h)` in the
/// style `FUN_801D74B0` stages, `0x44`
/// ([`crate::battle_hud_chrome::class0_box_draws_for`]).
pub fn venue_panel_draws_for(
    rects: &SaveMenuAtlasRects,
    rect: (i32, i32, i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    crate::battle_hud_chrome::class0_box_draws_for(rects, rect, stage_origin, stage_scale)
}

/// Crop stage-pixel text draws against a panel's footprint `(x, y, w, h)`
/// drawn over them: a glyph inside it goes, a glyph whose top or bottom
/// band it covers (across the glyph's whole width) keeps the rest, and a
/// glyph it covers only at a corner or not at all stays whole.
///
/// Retail's dialog fill is two half-transparent quads, so text under a panel
/// shows at a quarter of its strength there; over the fill's marble that is
/// not visible on a retail frame, which is what dropping it reproduces.
/// The crop keeps source and destination in step, so it holds for draws at
/// stage scale (one source texel a stage pixel), before the host scales them.
pub fn occlude_text_draws(draws: &mut Vec<TextDraw>, footprint: (i32, i32, i32, i32)) {
    let (fx, fy, fw, fh) = footprint;
    let (fx1, fy1) = (fx + fw, fy + fh);
    draws.retain_mut(|d| {
        let (x, y, w, h) = (d.dst.0, d.dst.1, d.dst.2 as i32, d.dst.3 as i32);
        let (x1, y1) = (x + w, y + h);
        if x1 <= fx || x >= fx1 || y1 <= fy || y >= fy1 {
            return true;
        }
        let spans_x = x >= fx && x1 <= fx1;
        let spans_y = y >= fy && y1 <= fy1;
        if spans_x && spans_y {
            return false;
        }
        let scaled = d.dst.2 == d.src.2 && d.dst.3 == d.src.3;
        if spans_x && scaled {
            if y >= fy {
                // The panel covers the glyph's top band.
                let cut = fy1 - y;
                d.dst.1 += cut;
                d.dst.3 -= cut as u32;
                d.src.1 += cut as u32;
                d.src.3 -= cut as u32;
            } else if y1 <= fy1 {
                let keep = (fy - y) as u32;
                d.dst.3 = keep;
                d.src.3 = keep;
            }
        } else if spans_y && scaled {
            if x >= fx {
                let cut = fx1 - x;
                d.dst.0 += cut;
                d.dst.2 -= cut as u32;
                d.src.0 += cut as u32;
                d.src.2 -= cut as u32;
            } else if x1 <= fx1 {
                let keep = (fx - x) as u32;
                d.dst.2 = keep;
                d.src.2 = keep;
            }
        }
        true
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyph(x: i32, y: i32) -> TextDraw {
        TextDraw {
            dst: (x, y, 8, 12),
            src: (100, 200, 8, 12),
            color: [1.0; 4],
        }
    }

    #[test]
    fn a_glyph_under_a_panel_goes_and_one_beside_it_stays() {
        let mut d = vec![glyph(10, 10), glyph(100, 10)];
        occlude_text_draws(&mut d, (0, 0, 50, 50));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].dst.0, 100);
    }

    #[test]
    fn a_panel_over_a_glyphs_top_band_keeps_its_bottom() {
        let mut d = vec![glyph(10, 40)];
        occlude_text_draws(&mut d, (0, 0, 50, 45));
        assert_eq!(d[0].dst, (10, 45, 8, 7));
        assert_eq!(d[0].src, (100, 205, 8, 7));
        let mut d = vec![glyph(10, 40)];
        occlude_text_draws(&mut d, (0, 46, 50, 45));
        assert_eq!(d[0].dst, (10, 40, 8, 6));
        assert_eq!(d[0].src, (100, 200, 8, 6));
    }

    #[test]
    fn an_unavailable_row_takes_ink_0() {
        let font = legaia_font::synthetic_for_tests();
        let rows = [
            ExchangeListRow {
                name: b"A",
                name_at: (0x52, 0x58),
                price: (5, 6, 0xD4, 0x58),
                available: false,
            },
            ExchangeListRow {
                name: b"B",
                name_at: (0x52, 0x68),
                price: (5, 6, 0xD4, 0x68),
                available: true,
            },
        ];
        let out = exchange_list_draws_for(&font, &rows, (b"p", 0x52, 0xB8), (1640, 6, 0xD4, 0xB8));
        let at = |y: i32| -> Vec<[f32; 4]> {
            out.iter()
                .filter(|d| d.dst.1 == y)
                .map(|d| d.color)
                .collect()
        };
        assert!(at(0x58).iter().all(|&c| c == EXCHANGE_DIM_INK));
        assert!(at(0x68).iter().all(|&c| c == EXCHANGE_INK));
    }
}

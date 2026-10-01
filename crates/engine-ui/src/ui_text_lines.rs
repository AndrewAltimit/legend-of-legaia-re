//! Positioned text lines -> [`TextDraw`]s: the last step of every screen whose
//! layout the engine resolves to `(bytes, x, y, marked)` lines in retail
//! 320x240 space (the fishing venue hub, the field floor window), or to
//! `(bytes, x, y, pen)` lines (the casino coin counter).
//!
//! The bytes are dialog-font text, usually the user's disc;
//! [`legaia_font::Font::layout`] consumes the `0xCE` / `0xCF` escapes without a
//! glyph. A marked line takes `marked_ink` - the screen's highlight.

use crate::*;

/// Compose positioned lines into text draws in stage space.
pub fn text_line_draws_for<'a>(
    font: &legaia_font::Font,
    lines: impl IntoIterator<Item = (&'a [u8], i32, i32, bool)>,
    ink: [f32; 4],
    marked_ink: [f32; 4],
) -> Vec<TextDraw> {
    let mut out = Vec::new();
    for (text, x, y, marked) in lines {
        out.extend(text_draws_for(
            &font.layout(text),
            (x, y),
            if marked { marked_ink } else { ink },
        ));
    }
    out
}

/// Compose positioned lines that each carry their own retail **pen** - the
/// `_DAT_8007B454` staging id the field overlay's panel painters select before
/// a string or number. Pens resolve through [`crate::records_ink`], whose
/// `{5, 6, 7, 9}` rows are pinned against the string CLUT; the field-overlay
/// coin counter stages exactly those four.
pub fn pen_line_draws_for<'a>(
    font: &legaia_font::Font,
    lines: impl IntoIterator<Item = (&'a [u8], i32, i32, u8)>,
) -> Vec<TextDraw> {
    let mut out = Vec::new();
    for (text, x, y, pen) in lines {
        out.extend(text_draws_for(
            &font.layout(text),
            (x, y),
            crate::records_ink(pen),
        ));
    }
    out
}

/// The in-world minigame status rows' bright ink.
pub const STATUS_ROW_INK: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// The in-world minigame status rows' dim ink.
pub const STATUS_ROW_DIM_INK: [f32; 4] = [0.7, 0.85, 1.0, 1.0];

/// Compose the in-world minigame status rows (`engine-core`'s
/// `minigame_status` builders: `(text, pen, bright)` in 320x240 stage space)
/// into text draws in **stage** space. The host applies its one stage
/// transform afterwards, the same as every other stage-space overlay.
pub fn status_row_draws_for<'a>(
    font: &legaia_font::Font,
    rows: impl IntoIterator<Item = (&'a str, (i32, i32), bool)>,
) -> Vec<TextDraw> {
    let mut out = Vec::new();
    for (text, pen, bright) in rows {
        out.extend(text_draws_for(
            &font.layout_ascii(text),
            pen,
            if bright {
                STATUS_ROW_INK
            } else {
                STATUS_ROW_DIM_INK
            },
        ));
    }
    out
}

/// The field floor window's marked ink (the plate set `0x58..=0x60`, the
/// current floor).
pub const FLOOR_WINDOW_MARKED_INK: [f32; 4] = [1.0, 0.92, 0.25, 1.0];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_pen_line_takes_its_pens_ink() {
        let font = legaia_font::synthetic_for_tests();
        let out = pen_line_draws_for(&font, [(&b"A"[..], 10, 20, 6), (&b"B"[..], 30, 40, 9)]);
        assert_eq!(out.len(), 2);
        assert_eq!((out[0].dst.0, out[0].color), (10, crate::MENU_TEXT_GOLD));
        assert_eq!((out[1].dst.1, out[1].color), (40, crate::MENU_TEXT_ORANGE));
    }

    #[test]
    fn each_line_lands_at_its_pen_in_its_ink() {
        let font = legaia_font::synthetic_for_tests();
        let out = text_line_draws_for(
            &font,
            [(&b"A"[..], 10, 20, false), (&b"B"[..], 30, 40, true)],
            [1.0; 4],
            [0.5; 4],
        );
        assert_eq!(out.len(), 2);
        assert_eq!(
            (out[0].dst.0, out[0].dst.1, out[0].color),
            (10, 20, [1.0; 4])
        );
        assert_eq!(
            (out[1].dst.0, out[1].dst.1, out[1].color),
            (30, 40, [0.5; 4])
        );
    }
}

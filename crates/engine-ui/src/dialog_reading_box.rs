//! The field dialog **reading box's text rows** - draw half of the pager's row
//! window, shared by both play hosts.
//!
//! The rows, the scroll word and the typing reveal live in `engine-core`
//! (`legaia_engine_core::dialog_window`, the port of `FUN_801D84D0`'s
//! reading-box arms); a host snapshots the panel into its `|`-joined page, the
//! scroll in whole pixels and the window height, and this module lays the
//! rows out.
//!
//! ## What retail draws
//!
//! Every frame the pager draws each row of its table with `FUN_80036888`
//! at `(ctx+0x12, ctx+0x14 + (scroll >> 4) + i*0xF)` (`0x801D9774..0x801D9800`;
//! the typing state `0x0B` draws without the scroll, which is `0` there). The
//! text is bracketed by two draw-area packets on the same ordering-table
//! slot (`0x801D95A8..0x801D964C` and `0x801D9860..0x801D9934`): the one
//! added **after** the text, which the GPU therefore meets **before** it,
//! narrows the drawing area to the rows band `y = box_y - 1 ..= box_y +
//! rows*0xF - 1`; the one added before the text restores the full screen.
//! So a row scrolling up out of the box is clipped at its top edge and the
//! row scrolling in below the third slot stays hidden until it rises into
//! the band. The box frame is added after both and is not clipped.
//!
//! Everything here is in **320x240 stage pixels**; hosts run the result
//! through their stage transform as for every other builder in this crate.
//!
//! REF: FUN_801D84D0, FUN_80036888

use crate::*;

/// The reading box's row pitch (`0xF`, the draw loop's `addiu s2,s2,0xf`).
pub const DIALOG_ROW_PITCH: i32 = 0xF;

/// First code point of the private-use block [`dialog_page_string`] parks a
/// non-ASCII page byte in.
const PAGE_BYTE_CHAR_BASE: u32 = 0xE000;

/// A panel's typed page bytes as the `|`-joined string both hosts snapshot:
/// printable ASCII as itself, every other byte - a `0xCE` escape and its
/// operand, an accented glyph - parked at `U+E000 + byte`, so
/// [`dialog_reading_box_text_draws_for`] hands the font the original bytes
/// and an escape draws its sprite instead of a `?`.
pub fn dialog_page_string(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| {
            if (0x20..=0x7E).contains(&b) {
                b as char
            } else {
                char::from_u32(PAGE_BYTE_CHAR_BASE + u32::from(b)).unwrap_or('?')
            }
        })
        .collect()
}

/// The page bytes back out of one [`dialog_page_string`] row (any other
/// non-ASCII character becomes `?`).
pub fn dialog_page_bytes(line: &str) -> Vec<u8> {
    line.chars()
        .map(|c| {
            let u = c as u32;
            if u < 0x80 {
                u as u8
            } else if (PAGE_BYTE_CHAR_BASE..PAGE_BYTE_CHAR_BASE + 0x100).contains(&u) {
                (u - PAGE_BYTE_CHAR_BASE) as u8
            } else {
                b'?'
            }
        })
        .collect()
}

/// Rows the reading box is tall for a snapshot: the pager window's height
/// when the panel has one (`box_rows`, three), else the page's own row count
/// (the plain-MES panel's box grows with its page), clamped to `3..=4`.
pub fn dialog_reading_box_lines(page: &str, box_rows: Option<usize>) -> i32 {
    box_rows
        .unwrap_or_else(|| page.split('|').count())
        .clamp(3, 4) as i32
}

/// Text draws for the reading box's rows: row `i` of the `|`-joined `page`
/// at `(box_x, box_y + scroll_px + i*0xF)` in the staged menu white, and -
/// with `clip_rows = Some(n)` - every glyph cropped to the pager's rows band
/// `box_y - 1 ..= box_y + n*0xF - 1`. `clip_rows = None` draws unclipped
/// (the plain-MES panel, which never scrolls).
pub fn dialog_reading_box_text_draws_for(
    font: &legaia_font::Font,
    page: &str,
    box_origin: (i32, i32),
    scroll_px: i32,
    clip_rows: Option<usize>,
) -> Vec<TextDraw> {
    let (bx, by) = box_origin;
    let band = clip_rows.map(|n| (by - 1, by + n as i32 * DIALOG_ROW_PITCH));
    let mut out = Vec::new();
    for (i, line) in page.split('|').enumerate() {
        let pen = (bx, by + scroll_px + i as i32 * DIALOG_ROW_PITCH);
        for d in text_draws_for(&font.layout(&dialog_page_bytes(line)), pen, MENU_TEXT_WHITE) {
            match band {
                None => out.push(d),
                Some((top, bottom)) => out.extend(crop_rows(d, top, bottom)),
            }
        }
    }
    out
}

/// Crop a 1:1 glyph quad to the stage rows `top..bottom` (exclusive end),
/// moving the atlas window with it. `None` when nothing is left.
fn crop_rows(d: TextDraw, top: i32, bottom: i32) -> Option<TextDraw> {
    let (x, y, w, h) = d.dst;
    let y0 = y.max(top);
    let y1 = (y + h as i32).min(bottom);
    if y1 <= y0 {
        return None;
    }
    let cut_top = (y0 - y) as u32;
    let (sx, sy, sw, _) = d.src;
    let nh = (y1 - y0) as u32;
    Some(TextDraw {
        dst: (x, y0, w, nh),
        src: (sx, sy + cut_top, sw, nh),
        color: d.color,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page carrying a `0xCE` escape survives the host snapshot string and
    /// reaches the font as bytes: with the sprite attached, the row draws it
    /// untinted between its neighbours.
    #[test]
    fn an_escape_in_the_page_draws_its_sprite() {
        use legaia_font::escape_icons::{EscapeIcon, EscapeIcons};
        let bytes = [b'A', 0xCE, 0x01, b'B', 0x7C, 0xE9];
        let page = dialog_page_string(&bytes);
        assert_eq!(page.split('|').count(), 2);
        let rows: Vec<Vec<u8>> = page.split('|').map(dialog_page_bytes).collect();
        assert_eq!(rows, vec![vec![b'A', 0xCE, 0x01, b'B'], vec![0xE9]]);
        let icon = EscapeIcon {
            index: 1,
            w: 16,
            h: 16,
            y_offset: -2,
            advance: 18,
            rgba: vec![0xFF; 16 * 16 * 4],
        };
        let font = legaia_font::synthetic_for_tests().with_escape_icons(&EscapeIcons {
            icons: vec![None, Some(icon)],
        });
        let d = dialog_reading_box_text_draws_for(&font, &page, (10, 20), 0, None);
        let sprite = d.iter().find(|t| t.dst.2 == 16).expect("the sprite draws");
        assert_eq!(sprite.dst.1, 18, "y_offset -2 from the row pen");
        assert_eq!(sprite.color, [1.0, 1.0, 1.0, MENU_TEXT_WHITE[3]]);
    }

    fn quad(y: i32, h: u32) -> TextDraw {
        TextDraw {
            dst: (10, y, 6, h),
            src: (100, 200, 6, h),
            color: MENU_TEXT_WHITE,
        }
    }

    #[test]
    fn a_glyph_scrolling_out_of_the_band_loses_its_top() {
        // Band for a 3-row box at y = 0x10: rows 15 ..= 60.
        let top = 0x10 - 1;
        let bottom = 0x10 + 3 * DIALOG_ROW_PITCH;
        let got = crop_rows(quad(12, 10), top, bottom).unwrap();
        assert_eq!(got.dst, (10, 15, 6, 7));
        assert_eq!(got.src, (100, 203, 6, 7));
        // Wholly inside: untouched.
        assert_eq!(
            crop_rows(quad(20, 10), top, bottom).unwrap().dst,
            (10, 20, 6, 10)
        );
        // The row scrolling in below the third slot is hidden until it rises.
        assert!(crop_rows(quad(0x10 + 45, 10), top, bottom).is_none());
        let rising = crop_rows(quad(0x10 + 40, 10), top, bottom).unwrap();
        assert_eq!(rising.dst, (10, 56, 6, 5));
    }

    #[test]
    fn the_box_is_the_window_height_not_the_page_length() {
        assert_eq!(dialog_reading_box_lines("a|b|c|d", Some(3)), 3);
        assert_eq!(dialog_reading_box_lines("a", Some(3)), 3);
        assert_eq!(dialog_reading_box_lines("a|b|c|d", None), 4);
        assert_eq!(dialog_reading_box_lines("a", None), 3);
    }
}

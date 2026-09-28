//! Positioned text lines -> [`TextDraw`]s: the last step of every screen whose
//! layout the engine resolves to `(bytes, x, y, marked)` lines in retail
//! 320x240 space (the fishing venue hub, the field floor window).
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

/// The field floor window's marked ink (the plate set `0x58..=0x60`, the
/// current floor).
pub const FLOOR_WINDOW_MARKED_INK: [f32; 4] = [1.0, 0.92, 0.25, 1.0];

#[cfg(test)]
mod tests {
    use super::*;

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

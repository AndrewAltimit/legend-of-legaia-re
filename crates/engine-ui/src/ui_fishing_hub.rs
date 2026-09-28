//! The fishing venue's **hub menu and help pages** - one draw, both play hosts.
//!
//! The screen's state and layout are `legaia_engine_core::fishing_hub` (the
//! five menu rows, the two help pages, the tackle list, all positioned in
//! retail's 320x240 space off the overlay's own draw calls). What this module
//! owns is the last step: dialog-font text lines into [`TextDraw`]s, with the
//! highlight ink for a marked line. Hosts hand the lines over as
//! `(bytes, x, y, marked)`, the shape both of them already have, and scale the
//! result onto their surface through the stage transform the fishing HUD uses.
//!
//! The text is disc bytes in the dialog-font encoding; [`legaia_font::Font::layout`]
//! consumes the `0xCE` button escapes without drawing them, so the help
//! footer's button glyph is absent on every host rather than wrong on one.

use crate::*;

/// The ink a marked hub line takes (the equipped tackle row - retail's
/// palette word `_DAT_8007B454 = 7`).
pub const FISHING_HUB_MARKED_INK: [f32; 4] = [1.0, 0.85, 0.35, 1.0];

/// Compose hub lines into text draws in stage space, through the shared
/// [`crate::ui_text_lines::text_line_draws_for`].
pub fn fishing_hub_draws_for<'a>(
    font: &legaia_font::Font,
    lines: impl IntoIterator<Item = (&'a [u8], i32, i32, bool)>,
    ink: [f32; 4],
) -> Vec<TextDraw> {
    crate::ui_text_lines::text_line_draws_for(font, lines, ink, FISHING_HUB_MARKED_INK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_line_lands_at_its_pen_and_a_marked_line_takes_the_highlight() {
        let font = legaia_font::synthetic_for_tests();
        let out = fishing_hub_draws_for(
            &font,
            [
                (&b"A"[..], 0x6C, 0x58, false),
                (&b"B"[..], 0x89, 0x50, true),
            ],
            [1.0; 4],
        );
        assert_eq!(out.len(), 2);
        assert_eq!((out[0].dst.0, out[0].dst.1), (0x6C, 0x58));
        assert_eq!(out[0].color, [1.0; 4]);
        assert_eq!(out[1].color, FISHING_HUB_MARKED_INK);
    }
}

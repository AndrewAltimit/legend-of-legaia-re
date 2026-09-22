//! The Baka Fighter HUD's **digit strips**, as a draw both hosts make.
//!
//! The duel's three number drawers - the one-glyph round digit
//! (`FUN_801d69e4`), the 8 px right-aligned score field (`FUN_801d6ef4`) and
//! the `0x10` px "GET COIN" numeral strip (`FUN_801d6f44`) - each patch a HUD
//! widget descriptor's `u` column and draw the widget. No host uploads the
//! PROT 1203 sprite page those descriptors index, so both draw the digit as a
//! font glyph at the ported cell x instead: the layout is retail's, the glyph
//! source is not.
//!
//! The layout lives in `engine_core::baka_fighter_chrome::hud_digit_placements`
//! - which strips draw, and at which pen. This is the other half: the glyph
//! quads. Splitting them is deliberate; a shared *layout* is not a shared
//! draw, and the two hosts drew different things off one layout until the
//! quad emitter sat under both as well.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_placed_digit_becomes_at_least_one_quad_at_its_pen() {
        let font = legaia_font::synthetic_for_tests();
        let out = baka_digit_strip_draws_for(&font, &[(10, 20, 4), (26, 20, 2)], [1.0; 4]);
        assert!(out.len() >= 2);
        let xs: std::collections::BTreeSet<i32> = out.iter().map(|d| d.dst.0).collect();
        assert!(xs.iter().any(|&x| x >= 10 && x < 26));
        assert!(xs.iter().any(|&x| x >= 26));
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

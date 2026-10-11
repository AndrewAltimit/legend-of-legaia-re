//! The fishing **line** as a screen-space primitive.
//!
//! Retail draws the line once a frame from the tail of the lure tick
//! (`FUN_801D26CC`): one `LINE_G2` packet (GP0 `0x50`, a Gouraud two-point
//! line, opaque) from the fish's projected point to the rod tip, clipped by
//! `FUN_801D56E4` and linked at the rod tip's depth bucket. The engine builds
//! that packet (`legaia_engine_core::fishing::PondSession::line_frame`, clip
//! and colours included); this module turns it into the prim set's line kind,
//! [`crate::screen_prim::line_quad`], with the two end colours carried as
//! per-corner Gouraud colours.
//!
//! The native window and the browser play page both draw the line through
//! [`fishing_line_prim`]; the minigames page strokes the same clipped
//! endpoints and colours on its canvas.

use crate::screen_prim::{ScreenPrim, line_quad};

/// The line's quad in display space: `fish` / `rod` are the clipped
/// endpoints in retail 320x240 screen space, each end carrying its packet
/// colour. `None` for a zero-length line.
pub fn fishing_line_prim(
    fish: (i16, i16),
    rod: (i16, i16),
    fish_rgb: [u8; 3],
    rod_rgb: [u8; 3],
    ot: u32,
) -> Option<ScreenPrim> {
    let a = [fish_rgb[0], fish_rgb[1], fish_rgb[2], 0xFF];
    let b = [rod_rgb[0], rod_rgb[1], rod_rgb[2], 0xFF];
    let mut q = line_quad(
        (fish.0 as f32, fish.1 as f32),
        (rod.0 as f32, rod.1 as f32),
        a,
        false,
        0,
        ot,
    )?;
    // `line_quad` lays its corners out `[a0, b0, a1, b1]`: the two `a`
    // corners sit on the fish end, the two `b` corners on the rod end.
    q.gouraud = Some([a, b, a, b]);
    Some(ScreenPrim::Flat(q))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_quad_carries_the_fish_colour_at_the_fish_end() {
        let Some(ScreenPrim::Flat(q)) =
            fishing_line_prim((10, 100), (160, 20), [0x30; 3], [0x80; 3], 7)
        else {
            panic!("a flat quad");
        };
        let g = q.gouraud.expect("per-corner colours");
        for (xy, c) in q.xy.iter().zip(g) {
            let near_fish = (xy.0 - 10).abs() <= 1 && (xy.1 - 100).abs() <= 1;
            let want = if near_fish { 0x30 } else { 0x80 };
            assert_eq!(c[0], want, "corner {xy:?}");
        }
        assert!(!q.semi_transparent, "LINE_G2 0x50 is opaque");
        assert_eq!(q.ot_index, 7);
    }

    #[test]
    fn a_zero_length_line_draws_nothing() {
        assert!(fishing_line_prim((5, 5), (5, 5), [0; 3], [0; 3], 0).is_none());
    }
}

//! PROT 0910's (Swordie, spell `0x88`) own screen-space packets: the two
//! opaque panels that cover the frame and slide apart along a diagonal cut,
//! and the slash streak that runs down the cut first.
//!
//! PORT: FUN_801F7C18 (PROT 0910; the split-panel builder, whole)
//! PORT: FUN_801F7DD0 (PROT 0910; the slash-streak builder, whole)
//!
//! # What the retail builders do
//!
//! Both take no arguments and read only module words; neither touches an
//! actor or the camera.
//!
//! - **`FUN_801F7C18`** allocates two `0x38808080` `POLY_G4` (gouraud,
//!   **opaque**) and copies each one's eight vertex halfwords from the
//!   module tables `0x801F8D6C` (left panel) and `0x801F8D7C` (right panel),
//!   then stores fixed grey ramps: the left panel `0xC0` / `0xB0` across its
//!   top edge and `0x80` / `0x70` across its bottom, the right one `0xB0` /
//!   `0xA0` and `0x70` / `0x60`. It links both at `ot + 0xC` (entry `3`), the
//!   left one first. The tick's arm `0` seeds the tables with the two halves
//!   of the frame cut along `(0x80, 0)..(0xC0, 0xE0)`, and arm `4` rewrites
//!   them from the split counter as the halves fall away.
//! - **`FUN_801F7DD0`** reads the split counter `c` (`0x801F8D8C`). While
//!   `c < 0xE` the cut is still being drawn: two `0x32` `POLY_G3`
//!   (semi-transparent) form the slash's head at row `16 c`, `32 c / 7`
//!   pixels right of `x = 0x80` - on the cut line, whose slope is `2 / 7` -
//!   and two `0x3A` `POLY_G4` trail from that head back up to the top of the
//!   frame at `x = 0x7C..0x84`. From `c = 0xE` the head has reached the
//!   bottom and the two quads become a band along the whole cut that widens
//!   with `c` (`(6 c) >> 6` at the bottom, `c >> 4` at the top). Every
//!   vertex is red `(0x80, 0x40, 0x40)` on the cut and `(0x40, 8, 8)` on
//!   the band's outer edge, and everything links at `ot + 8` (entry `2`):
//!   the triangles first, then the two quads. The colour word the routine
//!   builds from `0x80 - 2 c` survives only in the command byte - every
//!   vertex colour is overwritten.
//!
//! # Engine mapping, disclosed
//!
//! - The streak's packets set no draw mode: their blend is the GPU state the
//!   ordering-table walk left active. Retail frames of the cast show the
//!   streak **brightening** the grey panels to near-white pink, which only
//!   `B + F` does, so the port pins ABR 1, as the Cross Beam and Theeder do.
//! - The overlay draws over the finished scene rather than interleaving
//!   through the software OT. Both entries sit above any 3D bucket, so the
//!   opaque panels hide the scene exactly as retail's do.

use crate::screen_prim::{FlatQuad, ScreenPrim};

/// OT entry of the two panels (`ot + 0xC`).
pub const SWORDIE_SPLIT_OT: u32 = 3;
/// OT entry of the streak (`ot + 8`).
pub const SWORDIE_STREAK_OT: u32 = 2;
/// ABR mode the streak draws under (see the module docs).
pub const SWORDIE_STREAK_ABR_MODE: u8 = 1;
/// The counter below which the streak's head is still running down the cut
/// (`sltiu v0,t0,0xe` at `0x801F7E64`).
pub const SWORDIE_STREAK_HEAD_END: u32 = 0xE;

/// The panels' grey ramps, `[v0, v1, v2, v3]` per panel
/// (`0x801F7D30..0x801F7D9C`).
pub const SWORDIE_PANEL_GREYS: [[u8; 4]; 2] = [[0xC0, 0xB0, 0x80, 0x70], [0xB0, 0xA0, 0x70, 0x60]];

/// The streak's colour on the cut.
const CUT: [u8; 3] = [0x80, 0x40, 0x40];
/// The streak's colour on its outer edge.
const EDGE: [u8; 3] = [0x40, 0x08, 0x08];

fn rgba(c: [u8; 3]) -> [u8; 4] {
    [c[0], c[1], c[2], 0xFF]
}

fn g4(
    xy: [(i16, i16); 4],
    rgb: [[u8; 3]; 4],
    semi_transparent: bool,
    abr: u8,
    ot: u32,
) -> ScreenPrim {
    ScreenPrim::Flat(FlatQuad {
        xy,
        color: rgba(rgb[0]),
        gouraud: Some(rgb.map(rgba)),
        semi_transparent,
        abr_mode: abr,
        ot_index: ot,
        depth: None,
    })
}

/// A `POLY_G3` as the prim set's quad: the fourth corner repeats the third,
/// so the quad's second triangle is degenerate.
fn g3(xy: [(i16, i16); 3], rgb: [[u8; 3]; 3]) -> ScreenPrim {
    g4(
        [xy[0], xy[1], xy[2], xy[2]],
        [rgb[0], rgb[1], rgb[2], rgb[2]],
        true,
        SWORDIE_STREAK_ABR_MODE,
        SWORDIE_STREAK_OT,
    )
}

/// `FUN_801F7C18` over the two panel tables (`x0 y0 x1 y1 x2 y2 x3 y3`
/// each), in `AddPrim` order: left panel, then right.
pub fn swordie_split_prims(panels: &[[i16; 8]; 2]) -> Vec<ScreenPrim> {
    panels
        .iter()
        .zip(SWORDIE_PANEL_GREYS)
        .map(|(t, grey)| {
            g4(
                [(t[0], t[1]), (t[2], t[3]), (t[4], t[5]), (t[6], t[7])],
                grey.map(|g| [g; 3]),
                false,
                0,
                SWORDIE_SPLIT_OT,
            )
        })
        .collect()
}

/// `FUN_801F7DD0` for split counter `c`, in `AddPrim` order.
pub fn swordie_streak_prims(c: u32) -> Vec<ScreenPrim> {
    let quad = |xy: [(i16, i16); 4]| {
        g4(
            xy,
            [CUT, EDGE, CUT, EDGE],
            true,
            SWORDIE_STREAK_ABR_MODE,
            SWORDIE_STREAK_OT,
        )
    };
    let h = |v: u32| v as i16;
    if c < SWORDIE_STREAK_HEAD_END {
        // `(c << 6) >> 1`, divided by 7 through `0x92492493` / `>> 2`.
        let dx = (c << 5) / 7;
        let y = c << 4;
        let x0 = dx + 0x80;
        let tip = (h(x0), h(y));
        let low = (h(dx + 0x82), h(y + 7));
        let left = (h(dx + 0x79), h(y + 2));
        let right = (h(dx + 0x87), h(y.wrapping_sub(2)));
        vec![
            g3([tip, left, low], [CUT, EDGE, EDGE]),
            g3([tip, right, low], [CUT, EDGE, EDGE]),
            quad([tip, left, (0x80, 0), (0x7C, 0)]),
            quad([tip, right, (0x80, 0), (0x84, 0)]),
        ]
    } else {
        let bottom = (6 * c) >> 6;
        let top = c >> 4;
        vec![
            quad([
                (0xC0, 0xE0),
                (h(bottom + 0xBA), 0xE0),
                (0x80, 0),
                (h(top + 0x7C), 0),
            ]),
            quad([
                (0xC0, 0xE0),
                (h(0xC6u32.wrapping_sub(bottom)), 0xE0),
                (0x80, 0),
                (h(0x84u32.wrapping_sub(top)), 0),
            ]),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(p: &ScreenPrim) -> &FlatQuad {
        match p {
            ScreenPrim::Flat(q) => q,
            _ => panic!("flat"),
        }
    }

    /// The panels are opaque grey ramps at entry 3, left first.
    #[test]
    fn panels_are_opaque_grey_ramps_at_entry_three() {
        let p = swordie_split_prims(&[
            [0, 0, 0x80, 0, 0, 0xE0, 0xC0, 0xE0],
            [0x80, 0, 0x140, 0, 0xC0, 0xE0, 0x140, 0xE0],
        ]);
        assert_eq!(p.len(), 2);
        let l = flat(&p[0]);
        assert!(!l.semi_transparent);
        assert_eq!(l.ot_index, SWORDIE_SPLIT_OT);
        assert_eq!(l.xy, [(0, 0), (0x80, 0), (0, 0xE0), (0xC0, 0xE0)]);
        let g = l.gouraud.unwrap();
        assert_eq!(
            [g[0][0], g[1][0], g[2][0], g[3][0]],
            [0xC0, 0xB0, 0x80, 0x70]
        );
        let r = flat(&p[1]);
        assert_eq!(r.xy[1], (0x140, 0));
        assert_eq!(r.gouraud.unwrap()[3], [0x60, 0x60, 0x60, 0xFF]);
    }

    /// While the head runs, it sits on the cut line `(0x80, 0)..(0xC0,
    /// 0xE0)`: row `16 c`, `32 c / 7` right of `0x80`.
    #[test]
    fn the_streak_head_rides_the_cut_line() {
        for c in 0..SWORDIE_STREAK_HEAD_END {
            let p = swordie_streak_prims(c);
            assert_eq!(p.len(), 4);
            let tip = flat(&p[0]).xy[0];
            assert_eq!(i32::from(tip.1), 16 * c as i32);
            // x = 0x80 + y * 0x40 / 0xE0, truncated.
            assert_eq!(i32::from(tip.0), 0x80 + (i32::from(tip.1) * 0x40) / 0xE0);
            assert!(p.iter().all(|q| flat(q).semi_transparent));
            assert!(p.iter().all(|q| flat(q).ot_index == SWORDIE_STREAK_OT));
        }
    }

    /// From `c = 0xE` the streak is a band along the whole cut that widens
    /// with the counter.
    #[test]
    fn the_full_cut_band_widens_with_the_counter() {
        let narrow = swordie_streak_prims(0xE);
        let wide = swordie_streak_prims(0x3F);
        assert_eq!(narrow.len(), 2);
        let (a, b) = (flat(&narrow[0]), flat(&wide[0]));
        assert_eq!(a.xy[0], (0xC0, 0xE0));
        assert_eq!(a.xy[2], (0x80, 0));
        assert!(b.xy[1].0 > a.xy[1].0);
        assert_eq!(b.xy[1].0, ((6 * 0x3F) >> 6) + 0xBA);
        assert_eq!(flat(&wide[1]).xy[3].0, 0x84 - (0x3F >> 4));
    }
}

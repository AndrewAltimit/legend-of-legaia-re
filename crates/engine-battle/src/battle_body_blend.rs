//! A battle body's **whole-mesh semi-transparency**: the colour word's mode
//! byte applied to every prim the body draws.
//!
//! The battle per-actor draw hands each object's prims to the dispatcher
//! `FUN_80043390` with the render node's colour word `+0x74`, and the
//! dispatcher ORs that word's ABE bit into every packet's command
//! (`srl s6,a1,0x1f; sll s6,s6,0x19` at `0x80043510`) and its ABR mode
//! `(a1 >> 24) & 3` into the packets' tpage bits (`0x80043504..0x8004350C`).
//! So a body whose word raises bit 31 - the near-camera ghost pass
//! `FUN_8004DC68` (`0x83000000`, ABR 3 `B + F/4`) or the capture / defeat
//! fade (`0x81000000`, ABR 1 `B + F`) - draws every prim semi-transparent
//! with that equation, and the GPU still honours each texel's STP bit (an
//! STP-free texel draws opaque inside a semi prim).
//!
//! Both hosts already run the PSX per-prim semi path off the TSB word each
//! vertex carries (ABE in bit 15, ABR in bits 5..6), so the port applies the
//! rule where retail does, to the prims: [`apply_body_blend`] rewrites the
//! body's TSB words, and the ordinary opaque / blend passes do the rest.
//!
//! REF: FUN_80043390 (the ABE / ABR OR into each packet), FUN_8004DC68

/// The colour word's semi-transparency: `Some(abr)` when bit 31 (ABE) is
/// set, with the ABR mode from bits 24..25; `None` for an opaque body.
pub fn draw_colour_semi_mode(word: u32) -> Option<u8> {
    (word & 0x8000_0000 != 0).then_some(((word >> 24) & 3) as u8)
}

/// Apply a body's semi-transparency to its per-vertex `[cba, tsb]` words:
/// with `Some(abr)` every TSB gets the ABE enable (bit 15) and `abr` ORed
/// into its ABR field, as `FUN_80043390` ORs the mode into each packet's
/// tpage; `None` leaves the words untouched.
pub fn apply_body_blend(cba_tsb: &mut [[u16; 2]], mode: Option<u8>) {
    let Some(abr) = mode else {
        return;
    };
    for ct in cba_tsb {
        ct[1] = legaia_tmd::mesh::pack_tsb_semi(ct[1] | (u16::from(abr & 3) << 5), true);
    }
}

/// Whether the colour word selects handler bank 2 (bit `0x04000000`,
/// `FUN_80043390`): the near-camera ghost pass and the after-image walk both
/// raise it, and the bank's textured leaves write flat packets, one colour
/// for the primitive (`POLY_FT3` / `POLY_FT4`, codes `0x24` / `0x2C`, at
/// `0x800457C4` / `0x80045988`).
pub fn draw_colour_flat_bank(word: u32) -> bool {
    word & 0x0400_0000 != 0
}

/// Take a bank-2 body's packets for flat ones: drop the Gouraud packet-kind
/// tag its TMD groups carry ([`legaia_tmd::mesh::TSB_GOURAUD_BIT`]), which
/// the PCSX-Redux instrument dithers by.
pub fn apply_flat_bank(cba_tsb: &mut [[u16; 2]]) {
    for ct in cba_tsb {
        ct[1] &= !legaia_tmd::mesh::TSB_GOURAUD_BIT;
    }
}

/// Give a bank-2 body's every corner the colour its packet carries: the
/// bank's textured leaves write one colour word per primitive, the record's
/// first (`lw s7,0x0(a1)` at `0x80045988`, cued by `DPCS` and stored as the
/// `POLY_FT4`'s word at `0x80045B58..0x80045B78`; the triangle leaf
/// `0x800457C4` likewise), so a Gouraud primitive draws flat in its first
/// corner's colour. `colours` holds one entry per vertex of the triangle list
/// `indices`, a quad split `[0,1,2, 1,3,2]` as the mesh builders split it
/// ([`legaia_render_kernels::prim_near_reject::prim_corner_plan`] groups it
/// back); a vertex shared between primitives keeps its own colour.
pub fn apply_flat_bank_colours<T: Copy>(colours: &mut [T], indices: &[u32]) {
    use legaia_render_kernels::prim_near_reject::{PrimPlanScratch, prim_corner_plan_into};
    use std::cell::RefCell;
    // A ghost body flattens every frame it is near the eye; the plan's buffers
    // stay with the thread so the pass allocates nothing once warm.
    thread_local! {
        static SCRATCH: RefCell<(Vec<[u32; 5]>, PrimPlanScratch)> = RefCell::default();
    }
    SCRATCH.with_borrow_mut(|(plan, scratch)| {
        prim_corner_plan_into(colours.len(), indices, plan, scratch);
        flatten(colours, plan);
    });
}

fn flatten<T: Copy>(colours: &mut [T], plan: &[[u32; 5]]) {
    // In place: the plan marks only the corners of primitives that own every
    // corner alone, and such a primitive's first corner plans to itself, so
    // no corner that is read as a first corner is ever written.
    for (v, rec) in plan.iter().enumerate() {
        if rec[4] != 0
            && let Some(&first) = colours.get(rec[0] as usize)
            && let Some(c) = colours.get_mut(v)
        {
            *c = first;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bank_2_word_draws_flat_packets() {
        // The near-camera ghost's `0x87FF2020` and the after-image walk's
        // `0x85..` carry the bank bit; the capture fade's word does not.
        assert!(draw_colour_flat_bank(0x87FF_2020));
        assert!(draw_colour_flat_bank(0x8500_0000));
        assert!(!draw_colour_flat_bank(0x8100_4020));
        let g = legaia_tmd::mesh::TSB_GOURAUD_BIT;
        let mut ct = [[0x7980, 0x8035 | g], [0x7980, 0x0015]];
        apply_flat_bank(&mut ct);
        assert_eq!(ct, [[0x7980, 0x8035], [0x7980, 0x0015]]);
    }

    #[test]
    fn a_bank_2_primitive_draws_in_its_first_corners_colour() {
        // A Gouraud quad `[88, 128, 88, 128]` (Gala's near panel) and a
        // triangle `[96, 128, 104]`: each corner takes corner 0's word.
        let mut c = [
            [88u8; 3], [128; 3], [88; 3], [128; 3], [96; 3], [128; 3], [104; 3],
        ];
        apply_flat_bank_colours(&mut c, &[0, 1, 2, 1, 3, 2, 4, 5, 6]);
        assert_eq!(
            c,
            [
                [88; 3], [88; 3], [88; 3], [88; 3], [96; 3], [96; 3], [96; 3]
            ]
        );
    }

    #[test]
    fn the_ghost_and_fade_words_raise_their_equations() {
        // FUN_8004DC68's ghost bits and the capture fade's top byte.
        assert_eq!(draw_colour_semi_mode(0x8300_0000), Some(3));
        assert_eq!(draw_colour_semi_mode(0x8100_4020), Some(1));
        assert_eq!(draw_colour_semi_mode(0x0300_0000), None, "no ABE");
        assert_eq!(draw_colour_semi_mode(0x2008_0200), None, "neutral");
    }

    #[test]
    fn apply_body_blend_ors_abe_and_abr_into_every_tsb() {
        let mut ct = [[0x7DC0, 0x0015], [0x7DC1, 0x0035]];
        apply_body_blend(&mut ct, None);
        assert_eq!(ct, [[0x7DC0, 0x0015], [0x7DC1, 0x0035]]);
        apply_body_blend(&mut ct, Some(3));
        assert_eq!(ct[0], [0x7DC0, 0x8015 | (3 << 5)]);
        // An ABR already on the page stays ORed, as retail's OR leaves it.
        assert_eq!(ct[1], [0x7DC1, 0x8035 | (3 << 5)]);
    }
}

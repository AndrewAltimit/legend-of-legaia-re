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

#[cfg(test)]
mod tests {
    use super::*;

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

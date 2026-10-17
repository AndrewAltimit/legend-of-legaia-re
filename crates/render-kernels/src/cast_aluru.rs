//! PROT 0916's light bands: the four full-width semi-transparent gradient
//! quads its tick body draws inline while the blast lands.
//!
//! The spread is the band word `ctx[+0x6D8]` over `24`, which the module's
//! director carries (`legaia_engine_cast::cast_module_camera::aluru_direct`);
//! `World::aluru_band_draw` hands it to both hosts' screen layers.
//!
//! PORT: overlay_summon_aluru_0916_801f69f8 (PROT 0916; the inline band packets of arms 0x11..0x13, 0x801F7E74..0x801F8548)
//!
//! # What the retail arms do
//!
//! Arm `0x11` (`0x801F7CB0`) drains `ctx[+0x6D8]` and, on the pass it goes
//! negative, zeroes it and draws the bands once (`0x801F7E74..0x801F8164`)
//! before advancing. Arms `0x12` and `0x13` (`0x801F81C0`) grow it by
//! `rate * speed` a pass and draw them on every pass with the spread
//! `s = (i16)ctx[+0x6D8] / 24` (`mult 0x2AAAAAAB`, `sra 2`, sign fix-up) -
//! arm `0x11`'s draw is the same set at `s = 0`. Each band is a
//! `0x3B808080` `POLY_G4` spanning `x = 0..0x13F`, tag `8`:
//!
//! | Band | Top edge `(y, rgb)` | Bottom edge `(y, rgb)` |
//! |---|---|---|
//! | 1 | `0x58 - s`, `(0x20, 0x40, 0x20)` | `0x70`, `(0xFF, 0xFF, 0xFF)` |
//! | 2 | `0x70`, `(0xFF, 0xFF, 0xFF)` | `0x88 + s`, `(0x20, 0x40, 0x20)` |
//! | 3 | `0x70`, `(0xFF, 0xFF, 0x40)` | `0x48 - s`, `(8, 8, 0)` |
//! | 4 | `0x98 + s`, `(8, 8, 0)` | `0x70`, `(0xFF, 0xFF, 0x40)` |
//!
//! so a white core line at `y = 0x70` with a green-tinted glow and a yellow
//! one either side, widening as the timer grows. All four and then a
//! `SetDrawMode` (tpage `0x3F`: ABR 1, `B + F`) link at ordering-table entry
//! `ctx[+0x6D0] >> 3` (rounded toward zero), the framing depth - so the
//! `SetDrawMode` runs first and the bands draw additively, sorted into the
//! scene at that depth.
//!
//! # Engine mapping, disclosed
//!
//! The overlay draws over the finished scene rather than at the framing
//! depth; the bands are additive, so their order among themselves does not
//! matter.

use crate::screen_prim::{FlatQuad, ScreenPrim};

/// ABR mode the bands draw under (tpage `0x3F`).
pub const ALURU_BAND_ABR_MODE: u8 = 1;
/// Where the overlay links the bands (retail: the framing depth).
pub const ALURU_BAND_OT: u32 = 0x200;

/// The four bands at spread `s`, in `AddPrim` order.
pub fn aluru_band_prims(s: i16) -> Vec<ScreenPrim> {
    const GREEN: [u8; 3] = [0x20, 0x40, 0x20];
    const WHITE: [u8; 3] = [0xFF, 0xFF, 0xFF];
    const YELLOW: [u8; 3] = [0xFF, 0xFF, 0x40];
    const DARK: [u8; 3] = [0x08, 0x08, 0x00];
    let band = |top: i16, top_rgb: [u8; 3], bottom: i16, bottom_rgb: [u8; 3]| {
        let c = |k: [u8; 3]| [k[0], k[1], k[2], 0xFF];
        ScreenPrim::Flat(FlatQuad {
            xy: [(0, top), (0x13F, top), (0, bottom), (0x13F, bottom)],
            color: c(top_rgb),
            gouraud: Some([c(top_rgb), c(top_rgb), c(bottom_rgb), c(bottom_rgb)]),
            semi_transparent: true,
            abr_mode: ALURU_BAND_ABR_MODE,
            ot_index: ALURU_BAND_OT,
            depth: None,
        })
    };
    vec![
        band(0x58 - s, GREEN, 0x70, WHITE),
        band(0x70, WHITE, 0x88 + s, GREEN),
        band(0x70, YELLOW, 0x48 - s, DARK),
        band(0x98 + s, DARK, 0x70, YELLOW),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bands_meet_on_the_core_line_and_widen_with_the_spread() {
        let at = |s| {
            aluru_band_prims(s)
                .into_iter()
                .map(|p| match p {
                    ScreenPrim::Flat(q) => q,
                    _ => panic!("flat"),
                })
                .collect::<Vec<_>>()
        };
        let rest = at(0);
        assert_eq!(rest.len(), 4);
        assert!(rest.iter().all(|q| q.semi_transparent && q.abr_mode == 1));
        assert_eq!(rest[0].xy[0], (0, 0x58));
        assert_eq!(rest[0].xy[3], (0x13F, 0x70));
        let wide = at(64);
        assert_eq!(wide[0].xy[0].1, 0x58 - 64);
        assert_eq!(wide[3].xy[0].1, 0x98 + 64);
    }
}

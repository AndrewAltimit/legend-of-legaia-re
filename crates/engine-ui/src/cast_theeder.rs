//! PROT 0904's (Theeder, spell `0x82`) beam packets: the four packet
//! builders its tick body calls, turned into screen primitives from the
//! projected points of one tick's [`TheederPacket`].
//!
//! PORT: FUN_801F815C (the arm-9 lightning prong, `POLY_FT4`)
//! PORT: FUN_801F83A4 (the arm-11 charge beam, two `POLY_G4`)
//! PORT: FUN_801F8634 (the sweeping beam: trail fan, trail edge, core)
//! PORT: FUN_801F8B84 (the retracting trail fan)
//!
//! # What the retail builders do
//!
//! Each projects its world points through the GTE billboard projector
//! `FUN_800195A8` and allocates packets from the scratchpad pool
//! `0x1F800314 + 0x8C`, linking them into the ordering table at
//! `*(0x1F8003F4)`:
//!
//! - **`FUN_801F815C(a, b)`** - one `0x2F808080` `POLY_FT4` (textured,
//!   semi-transparent, raw texture) from `a` to `b`, `1 + w` pixels above and
//!   below each end, `w = (0x100 - z) >> 5` for a projected depth `z <=
//!   0x100` and `0` beyond. Texture page `0x27` (4bpp at VRAM `(448, 0)`,
//!   ABR 1), CLUT `0x7700` (`(0, 476)`), `u` across the bolt over one of four
//!   32-texel cells picked by `rand() & 3`, `v` `0..0x3F` along it. Linked at
//!   the two ends' average depth.
//! - **`FUN_801F83A4(a, b, level)`** - two `0x3B808080` `POLY_G4` (gouraud,
//!   semi-transparent) glowing out from the line `a..b`: one to the line
//!   offset by `(-4, -4)`, linked at OT entry `2`, one to `(+4, +4)`, linked
//!   at the average depth; the line edge carries `(level >> 5, level >> 5,
//!   level)` and the offset edge black.
//! - **`FUN_801F8634(a, b)`** - the trail fan: for each pair of consecutive
//!   ring tips `hist[i]`, `hist[i + 1]` one `POLY_G4` `(hist[i], hist[i+1],
//!   a, a)` in blue `(16 - i) * 15` / `(15 - i) * 15`, and a one-pixel edge
//!   along the pair in `(4k, 4k, 2k)` for `k = 16 - i` / `15 - i`; then the
//!   beam core from `a` to `b`, two `POLY_G4` two pixels wide each side in
//!   `(0x40, 0x40, 0xC0)` fading to black. Everything at OT entry `0x400`.
//! - **`FUN_801F8B84(a)`** - the same fan over the whole count `n`, without
//!   the core, in `(n - i + 1) * 15` / `(n - i) * 15`.
//!
//! # Engine mapping, disclosed
//!
//! - The hosts project the points with their battle camera; the projector
//!   here returns screen pixels only, so the prong's near-camera widening
//!   `w` is taken as `0` (the battle camera keeps the beam far beyond
//!   `z = 0x100`).
//! - A `POLY_G4` carries no texture page, so its blend is the GPU state the
//!   ordering-table walk left active; every packet here is a black-to-colour
//!   gradient that only reads under `B + F`, and the port pins ABR 1, as the
//!   weapon trail and the Cross Beam do.
//! - The overlay draws over the finished scene rather than interleaving
//!   through the software OT; the fixed entries keep their relative order and
//!   the depth-averaged ones sit at [`THEEDER_DEPTH_OT`].

use crate::screen_prim::{FlatQuad, ScreenPrim, ScreenQuad};
use legaia_engine_vm::cast_seru_ticks_a::TheederPacket;

/// Projected screen point.
pub type Xy = (i16, i16);

/// ABR mode every packet draws under.
pub const THEEDER_ABR_MODE: u8 = 1;
/// The fan / core entry (`ot + 0x1000`).
pub const THEEDER_FAN_OT: u32 = 0x400;
/// The charge beam's fixed entry (`ot + 8`).
pub const THEEDER_CHARGE_NEAR_OT: u32 = 2;
/// Where the depth-averaged packets link in the overlay.
pub const THEEDER_DEPTH_OT: u32 = 0x200;
/// The prong's texture page word (`li v0,0x27` at `0x801F8334`).
pub const THEEDER_PRONG_TPAGE: u16 = 0x27;
/// The prong's CLUT word (`li v0,0x7700` at `0x801F833C`).
pub const THEEDER_PRONG_CLUT: u16 = 0x7700;
/// The beam core's colour (`li s0,0x40` / `li s1,0xc0`).
pub const THEEDER_CORE_RGB: [u8; 3] = [0x40, 0x40, 0xC0];

fn g4(xy: [Xy; 4], rgb: [[u8; 3]; 4], ot: u32) -> ScreenPrim {
    let c = |c: [u8; 3]| [c[0], c[1], c[2], 0xFF];
    ScreenPrim::Flat(FlatQuad {
        xy,
        color: c(rgb[0]),
        gouraud: Some(rgb.map(c)),
        semi_transparent: true,
        abr_mode: THEEDER_ABR_MODE,
        ot_index: ot,
        depth: None,
    })
}

fn off(p: Xy, dx: i16, dy: i16) -> Xy {
    (p.0.wrapping_add(dx), p.1.wrapping_add(dy))
}

/// `FUN_801F815C`: one lightning prong from `a` to `b` on texture cell
/// `cell` (`0..=3`).
pub fn prong_prim(a: Xy, b: Xy, cell: u8) -> ScreenPrim {
    let u = (cell & 3) * 0x20;
    ScreenPrim::Textured(ScreenQuad {
        xy: [off(a, 0, -1), off(b, 0, -1), off(a, 0, 1), off(b, 0, 1)],
        uv: [(u, 0), (u, 0x3F), (u + 0x1F, 0), (u + 0x1F, 0x3F)],
        clut: THEEDER_PRONG_CLUT,
        tpage: THEEDER_PRONG_TPAGE,
        color: 0x80_80_80,
        gouraud: None,
        semi_transparent: true,
        ot_index: THEEDER_DEPTH_OT,
        depth: None,
    })
}

/// `FUN_801F83A4`: the charge beam's two glow quads.
pub fn charge_prims(a: Xy, b: Xy, level: u16) -> [ScreenPrim; 2] {
    let l = (level & 0xFF) as u8;
    let col = [(level >> 5) as u8, (level >> 5) as u8, l];
    let k = [0, 0, 0];
    [
        g4(
            [off(a, -4, -4), a, off(b, -4, -4), b],
            [k, col, k, col],
            THEEDER_CHARGE_NEAR_OT,
        ),
        g4(
            [a, off(a, 4, 4), b, off(b, 4, 4)],
            [col, k, col, k],
            THEEDER_DEPTH_OT,
        ),
    ]
}

/// One fan pair `p0 = hist[i]`, `p1 = hist[i + 1]` to the root `a`, with the
/// near and far intensity steps `k0` / `k1`.
fn fan_pair(out: &mut Vec<ScreenPrim>, a: Xy, p0: Xy, p1: Xy, k0: u32, k1: u32) {
    let blue = |k: u32| [0, 0, (k * 15) as u8];
    let edge = |k: u32| [(k * 4) as u8, (k * 4) as u8, (k * 2) as u8];
    out.push(g4(
        [p0, p1, a, a],
        [blue(k0), blue(k1), blue(k0), blue(k1)],
        THEEDER_FAN_OT,
    ));
    out.push(g4(
        [off(p0, 0, -1), off(p1, 0, -1), p0, p1],
        [edge(k0), edge(k1), edge(k0), edge(k1)],
        THEEDER_FAN_OT,
    ));
}

/// The primitives one tick's packet draws. `project` maps a retail battle
/// point (Y down) to stage pixels, `None` behind the camera; `trail` is the
/// ring the fan packets index (`hist[0]` newest).
pub fn theeder_prims(
    packet: &TheederPacket,
    trail: &[[i16; 3]],
    project: impl Fn([i16; 3]) -> Option<Xy>,
) -> Vec<ScreenPrim> {
    let mut out = Vec::new();
    match *packet {
        TheederPacket::Prongs { mouth, tips, cells } => {
            if let Some(a) = project(mouth) {
                for (tip, cell) in tips.iter().zip(cells) {
                    if let Some(b) = project(*tip) {
                        out.push(prong_prim(a, b, cell));
                    }
                }
            }
        }
        TheederPacket::Charge { mouth, tip, level } => {
            if let (Some(a), Some(b)) = (project(mouth), project(tip)) {
                out.extend(charge_prims(a, b, level));
            }
        }
        TheederPacket::Sweep { mouth, tip, drawn } => {
            let Some(a) = project(mouth) else {
                return out;
            };
            for i in 0..drawn.min(trail.len().saturating_sub(1)) {
                if let (Some(p0), Some(p1)) = (project(trail[i]), project(trail[i + 1])) {
                    fan_pair(&mut out, a, p0, p1, 16 - i as u32, 15 - i as u32);
                }
            }
            if let Some(b) = project(tip) {
                let k = [0, 0, 0];
                let c = THEEDER_CORE_RGB;
                out.push(g4(
                    [off(a, -2, 0), a, off(b, -2, 0), b],
                    [k, c, k, c],
                    THEEDER_FAN_OT,
                ));
                out.push(g4(
                    [a, off(a, 2, 0), b, off(b, 2, 0)],
                    [c, k, c, k],
                    THEEDER_FAN_OT,
                ));
            }
        }
        TheederPacket::Retract { mouth, drawn } => {
            let Some(a) = project(mouth) else {
                return out;
            };
            let n = drawn as u32;
            for i in 0..drawn.min(trail.len().saturating_sub(1)) {
                if let (Some(p0), Some(p1)) = (project(trail[i]), project(trail[i + 1])) {
                    fan_pair(&mut out, a, p0, p1, n - i as u32 + 1, n - i as u32);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(p: &ScreenPrim) -> &FlatQuad {
        match p {
            ScreenPrim::Flat(q) => q,
            ScreenPrim::Textured(_) => panic!("expected a G4"),
        }
    }

    /// Identity-ish projection for the tests: `(x, z)` as the screen point.
    fn proj(p: [i16; 3]) -> Option<Xy> {
        Some((p[0], p[2]))
    }

    #[test]
    fn the_sweep_draws_two_quads_a_pair_and_a_two_quad_core() {
        let trail = [[0, 0, 10], [5, 0, 10], [10, 0, 10], [15, 0, 10]];
        let pk = TheederPacket::Sweep {
            mouth: [0, 0, 0],
            tip: [0, 0, 20],
            drawn: 3,
        };
        let prims = theeder_prims(&pk, &trail, proj);
        assert_eq!(prims.len(), 3 * 2 + 2);
        // The first pair's fan: brightest blue at the newest tip.
        let fan = flat(&prims[0]);
        assert_eq!(fan.xy, [(0, 10), (5, 10), (0, 0), (0, 0)]);
        assert_eq!(fan.gouraud.unwrap()[0], [0, 0, 240, 255]);
        assert_eq!(fan.gouraud.unwrap()[1], [0, 0, 225, 255]);
        // Its edge: (4k, 4k, 2k), one pixel up.
        let edge = flat(&prims[1]);
        assert_eq!(edge.xy[0], (0, 9));
        assert_eq!(edge.gouraud.unwrap()[0], [64, 64, 32, 255]);
        // The core's bright edge sits on the line.
        let core = flat(&prims[6]);
        assert_eq!(core.xy, [(-2, 0), (0, 0), (-2, 20), (0, 20)]);
        assert_eq!(core.gouraud.unwrap()[1], [0x40, 0x40, 0xC0, 255]);
        assert!(prims.iter().all(|p| p.ot_index() == THEEDER_FAN_OT));
    }

    #[test]
    fn the_retract_fades_from_the_count() {
        let trail = [[0, 0, 10], [5, 0, 10], [10, 0, 10]];
        let pk = TheederPacket::Retract {
            mouth: [0, 0, 0],
            drawn: 2,
        };
        let prims = theeder_prims(&pk, &trail, proj);
        assert_eq!(prims.len(), 4);
        assert_eq!(flat(&prims[0]).gouraud.unwrap()[0], [0, 0, 45, 255]);
        assert_eq!(flat(&prims[0]).gouraud.unwrap()[1], [0, 0, 30, 255]);
    }

    #[test]
    fn the_charge_glows_out_four_pixels_each_side() {
        let [near, far] = charge_prims((10, 10), (30, 10), 0xA0);
        assert_eq!(flat(&near).xy, [(6, 6), (10, 10), (26, 6), (30, 10)]);
        assert_eq!(flat(&near).gouraud.unwrap()[1], [5, 5, 0xA0, 255]);
        assert_eq!(near.ot_index(), THEEDER_CHARGE_NEAR_OT);
        assert_eq!(flat(&far).xy[1], (14, 14));
    }

    #[test]
    fn a_prong_samples_its_cell_along_the_bolt() {
        let ScreenPrim::Textured(q) = prong_prim((0, 0), (40, 0), 2) else {
            panic!("expected an FT4");
        };
        assert_eq!(q.uv, [(0x40, 0), (0x40, 0x3F), (0x5F, 0), (0x5F, 0x3F)]);
        assert_eq!(q.xy, [(0, -1), (40, -1), (0, 1), (40, 1)]);
        assert_eq!(q.abr_mode(), 1);
    }
}

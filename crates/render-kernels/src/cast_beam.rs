//! PROT 0948's **Cross Beam** packets: the two sine-swept beams arm 3 of the
//! module's tick body draws in screen space every tick it runs.
//!
//! PORT: FUN_801F726C (PROT 0948; the beam's packet builder, whole)
//!
//! # What the retail builder does
//!
//! `FUN_801F726C` takes no arguments and touches no actor or context state.
//! It advances its own counter `c` (the module word `0x801F8858`, which arm 2
//! zeroes) by `step * 2` (`0x801F72BC..0x801F72D4`, `step` the scratchpad
//! frame step `*(0x1F800393)`), then builds every packet from `c` and the
//! sine table behind `_DAT_8007B81C` alone. Each packet is a `0x3B808080`
//! `POLY_G4` - gouraud, semi-transparent - and the routine ends by linking a
//! `SetDrawMode(.., 0, 0, 0x20, 0)` (`FUN_80059010` at `0x801F8488` /
//! `0x801F84BC`) at the head of each of the two ordering-table entries it
//! used, so both draw under ABR 1, `B + F`.
//!
//! With `X(i) = sin[16 i] / 12` and `Y(i) = sin[12 i] / 32` (both truncated
//! toward zero; the table is indexed unmasked, which the 5120-entry table's
//! extra quarter turn makes the same as masking the angle):
//!
//! - **The left beam** rises from the bottom-left of the frame. Its head is
//!   one quad at OT entry `2` (`ot + 8`) from the tip `(X(c), 0xE0 - Y(c))`
//!   down to the floor line `y = 0xE0`, red at the tip and yellow at the
//!   base. Its body is a trail of segments from `c` back towards `0`, each
//!   linked at OT entry `0x400` (`ot + 0x1000`): a one-pixel yellow core
//!   (`B`), a red glow fading up (`D`) and down (`C`) over a width `w` that
//!   starts at `1`, grows by one per segment and is capped by the core's
//!   height above `y = 0x3C` over 12; the first segment also carries a flare
//!   (`A`). Brightness starts at `0xFF` and drops by `2` per segment, so the
//!   trail is at most 128 segments long.
//! - **The right beam** is the mirror (`x -> 0x140 - x`) of the same curve
//!   run `0x20` steps behind (`c - 0x20`, drawn once `c >= 0x20`), with its
//!   core at half brightness.
//!
//! # Engine mapping, disclosed
//!
//! Retail interleaves the `0x400` entry with the battle scene through the
//! software OT; the screen-prim overlay draws over the finished frame, the
//! same simplification the weapon trail and move-FX streaks take. The two
//! entries keep their relative order.

use crate::gte::psx_sin;
use crate::screen_prim::{FlatQuad, ScreenPrim};

/// ABR blend mode both entries are drawn under (`SetDrawMode` tpage `0x20`).
pub const CROSS_BEAM_ABR_MODE: u8 = 1;
/// OT entry of the two beam heads (`ot + 8`).
pub const CROSS_BEAM_HEAD_OT: u32 = 2;
/// OT entry of the beam bodies (`ot + 0x1000`).
pub const CROSS_BEAM_BODY_OT: u32 = 0x400;
/// How far the right beam runs behind the left (`addiu a0,a0,-0x20` at
/// `0x801F7B6C`, after the `slti 0x20` guard at `0x801F7B4C`).
pub const CROSS_BEAM_RIGHT_LAG: i32 = 0x20;
/// What one call adds to the counter per frame step (`sll v1,v1,1`).
pub const CROSS_BEAM_STEP_SCALE: i32 = 2;

fn sin_at(index: i32) -> i32 {
    psx_sin((index & 0xFFF) as u16)
}

/// `X(i)`: the sine sampled at `16 i`, over 12 (`lhu` at `i * 32` bytes,
/// `mult` by `0x2AAAAAAB`, `sra 1`, sign fix-up).
fn bx(i: i32) -> i32 {
    sin_at(i * 16) / 12
}

/// The head's base offset: the same sample over 24 (`sra 2`).
fn bx24(i: i32) -> i32 {
    sin_at(i * 16) / 24
}

/// `Y(i)`: the sine sampled at `12 i` (`i * 24` bytes), over 32 rounding
/// toward zero (`bgez` / `addiu 0x1f` / `sra 5`).
fn by(i: i32) -> i32 {
    sin_at(i * 12) / 32
}

type Rgb = [u8; 3];

const RED: Rgb = [0xFF, 0, 0];
const YELLOW: Rgb = [0xFF, 0xFF, 0];
const BLACK: Rgb = [0, 0, 0];

fn quad(xy: [(i32, i32); 4], rgb: [Rgb; 4], ot: u32) -> ScreenPrim {
    let c = |k: Rgb| [k[0], k[1], k[2], 0xFF];
    ScreenPrim::Flat(FlatQuad {
        xy: xy.map(|(x, y)| (x as i16, y as i16)),
        color: c(rgb[0]),
        gouraud: Some(rgb.map(c)),
        semi_transparent: true,
        abr_mode: CROSS_BEAM_ABR_MODE,
        ot_index: ot,
        depth: None,
    })
}

/// One beam's trail, `i` from `top` down to `1`, in emission order.
/// `mirror` selects the right beam's x map and vertex order.
fn trail(top: i32, mirror: bool, out: &mut Vec<ScreenPrim>) {
    let mut s1: i32 = 0xFF;
    let mut w: i32 = 1;
    let mut i = top;
    let ot = CROSS_BEAM_BODY_OT;
    while i > 0 && s1 >= 0 {
        let (xa, xb) = (bx(i - 1), bx(i));
        let (ya, yb) = (by(i - 1), by(i));
        let s = s1 as u8;
        let half = (s1 / 2) as u8;
        // Left beam: v0/v2 on sample `i - 1`, v1/v3 on `i`. Right beam: x
        // mirrored about 0x140 and the pair swapped (v0/v2 on `i`).
        let edge = |top_a: i32, top_b: i32, bot_a: i32, bot_b: i32| -> [(i32, i32); 4] {
            if mirror {
                [
                    (0x140 - xb, top_b),
                    (0x140 - xa, top_a),
                    (0x140 - xb, bot_b),
                    (0x140 - xa, bot_a),
                ]
            } else {
                [(xa, top_a), (xb, top_b), (xa, bot_a), (xb, bot_b)]
            }
        };
        let flare = [s, half, 0];
        if i == top {
            // A: the flare at the tip segment (`0xD9` on the `i` sample).
            out.push(quad(
                edge(0xE0 - ya, 0xD9 - yb, 0xE1 - ya, 0xE1 - yb),
                [BLACK, BLACK, flare, flare],
                ot,
            ));
        }
        // B: the one-pixel core, full brightness on the left beam and half
        // on the right (`sb v0` of `s1 / 2` into all four at `0x801F8070`).
        let core = if mirror { [half, half, 0] } else { [s, s, 0] };
        out.push(quad(
            edge(0xE0 - ya, 0xE0 - yb, 0xE1 - ya, 0xE1 - yb),
            [core; 4],
            ot,
        ));
        // The glow width: capped by the core's lower edge on the sample the
        // left beam keeps in v2 (`lh v0,0x1a(s0)`), less 0x3C, over 12.
        let v2y = if mirror { 0xE1 - yb } else { 0xE1 - ya } as i16 as i32;
        let cap = (v2y - 0x3C) / 12;
        if cap < w {
            w = cap;
        }
        let red = [s, 0, 0];
        // C: the glow fading down from the core.
        out.push(quad(
            edge(0xE0 - ya, 0xE0 - yb, w + 0xE0 - ya + 1, w + 0xE0 - yb + 1),
            [red, red, BLACK, BLACK],
            ot,
        ));
        // D: the glow fading up into the core.
        out.push(quad(
            edge(0xE0 - w - ya, 0xE0 - w - yb, 0xE0 - ya, 0xE0 - yb),
            [BLACK, BLACK, red, red],
            ot,
        ));
        i -= 1;
        s1 -= 2;
        w += 1;
    }
}

/// Every packet `FUN_801F726C` emits for counter value `c` (the value after
/// its own increment), in emission order.
pub fn cross_beam_prims(c: i32) -> Vec<ScreenPrim> {
    let mut out = Vec::new();
    // Left head (`0x801F72D8..0x801F7430`).
    out.push(quad(
        [
            (bx(c), 0xE1 - by(c)),
            (bx(c) + 1, 0xE0 - by(c)),
            (bx24(c) + 0x4D, 0xE0),
            (bx24(c) + 0x49, 0xE0),
        ],
        [RED, RED, YELLOW, YELLOW],
        CROSS_BEAM_HEAD_OT,
    ));
    trail(c, false, &mut out);
    if c >= CROSS_BEAM_RIGHT_LAG {
        let d = c - CROSS_BEAM_RIGHT_LAG;
        // Right head (`0x801F7B78..0x801F7D08`).
        out.push(quad(
            [
                (0x141 - bx(d), 0xE0 - by(d)),
                (0x140 - bx(d), 0xE1 - by(d)),
                (0xF7 - bx24(d), 0xE0),
                (0xF3 - bx24(d), 0xE0),
            ],
            [RED, RED, YELLOW, YELLOW],
            CROSS_BEAM_HEAD_OT,
        ));
        trail(d, true, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(p: &ScreenPrim) -> &FlatQuad {
        match p {
            ScreenPrim::Flat(q) => q,
            ScreenPrim::Textured(_) => panic!("textured beam packet"),
        }
    }

    #[test]
    fn a_fresh_counter_draws_the_left_head_alone() {
        let p = cross_beam_prims(0);
        assert_eq!(p.len(), 1);
        let q = flat(&p[0]);
        // sin(0) = 0: the tip sits on the floor line at the frame's left.
        assert_eq!(q.xy, [(0, 0xE1), (1, 0xE0), (0x4D, 0xE0), (0x49, 0xE0)]);
        assert_eq!(q.ot_index, CROSS_BEAM_HEAD_OT);
        assert!(q.semi_transparent);
        assert_eq!(q.abr_mode, 1);
    }

    #[test]
    fn the_trail_is_three_packets_a_segment_plus_the_flare() {
        // c = 10: one head, the flare, and three packets per segment.
        assert_eq!(cross_beam_prims(10).len(), 1 + 1 + 3 * 10);
    }

    #[test]
    fn brightness_bounds_the_trail_at_128_segments() {
        let n = cross_beam_prims(0x1F)
            .iter()
            .filter(|p| p.ot_index() == CROSS_BEAM_BODY_OT)
            .count();
        assert_eq!(n, 1 + 3 * 0x1F);
        // At c = 200 the left trail stops at 128 segments (s1 < 0) and the
        // right one, at d = 168, does too.
        let body = cross_beam_prims(200)
            .iter()
            .filter(|p| p.ot_index() == CROSS_BEAM_BODY_OT)
            .count();
        assert_eq!(body, 2 * (1 + 3 * 128));
    }

    #[test]
    fn the_right_beam_starts_0x20_steps_behind_and_mirrored() {
        let heads: Vec<_> = cross_beam_prims(0x20)
            .iter()
            .filter(|p| p.ot_index() == CROSS_BEAM_HEAD_OT)
            .map(|p| *flat(p))
            .collect();
        assert_eq!(heads.len(), 2);
        assert_eq!(
            heads[1].xy,
            [(0x141, 0xE0), (0x140, 0xE1), (0xF7, 0xE0), (0xF3, 0xE0)]
        );
    }

    #[test]
    fn the_tip_follows_the_sine_sweep() {
        // c = 64: X = sin[1024] / 12 = 4096 / 12 = 341, Y = sin[768] / 32.
        let q = *flat(&cross_beam_prims(64)[0]);
        assert_eq!(q.xy[0].0, 341);
        assert_eq!(q.xy[0].1 as i32, 0xE1 - psx_sin(768) / 32);
    }
}

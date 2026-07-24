//! The field overlay's ledge-hop arc **setup** - the routine that turns the
//! landing-point triple the locomotion controller builds on its stack into a
//! quadratic-Bezier hop clip on a freshly spawned helper actor.
//!
//! PORT: FUN_801d2404
//! REF: FUN_801d1878, FUN_801d2298
//!
//! # Provenance
//!
//! `FUN_801d2404` lives in the field overlay (PROT entry `0897_xxx_dat`, file
//! offset `0x3BEC`, slot-A base `0x801CE818`) and is 122 instructions long.
//! The short standalone dump `ghidra/scripts/funcs/801d2404.txt` is an
//! 8-instruction wrong-base fragment; the base-correct body is
//! `ghidra/scripts/funcs/overlay_0897_door_801d2404.txt`, which agrees
//! instruction-for-instruction with the extracted `0897_xxx_dat.BIN` bytes at
//! that offset.
//!
//! The image holds exactly **one** `jal 0x801D2404` site, at `0x801D1B70`
//! inside the ledge-hop trigger `FUN_801d1878`. Reading the argument set-up in
//! its delay-slot window pins the signature:
//!
//! ```text
//! 801d1b20  addiu a0, sp, 0x18      ; a0 = &stack triple {x, y, z}
//! 801d1b2c  addiu a1, zero, 0x10    ; a1 = apex height (hop up)
//! 801d1b44  addiu a1, zero, 0x18    ;      or 0x18 (hop down)
//! 801d1b60  addiu a2, zero, 0x10    ; a2 = clip length in frames (always 16)
//! 801d1b70  jal   0x801d2404
//! ```
//!
//! so the call is `FUN_801d2404(&landing_xyz, apex, frames)`. The engine side
//! already builds that triple: `engine_core::world::FieldLedgeHop` is posted by
//! `World::try_field_ledge_hop` with exactly `target_x / target_y / target_z`
//! and `kind` = the `0x10` / `0x18` value that lands in `a1`.
//!
//! # What the routine does
//!
//! Reading the disassembly (not the decompiled C - the C renders the apex
//! store as `(min - apex) * 2 - mid`, which is the same value but hides that
//! retail computes it as `mid + 2 * (min - apex - mid)`):
//!
//! 1. Bail out when the scene control block's player slot
//!    (`0x8007C348 + 0x1C` = `_DAT_8007C364`, the player context pointer) is
//!    null.
//! 2. Allocate a helper actor from the pool (`func_0x80020DE0` with the
//!    template pointer `0x801F227C` and the pool handle at `0x8007C34C`);
//!    on failure nothing else happens.
//! 3. Back-link the player into the helper's `+0x90`, and copy the player's
//!    8-byte transform block `+0x14..+0x1B` into the helper - this is the
//!    Bezier start point `P0` (`+0x14` = X, `+0x16` = Y, `+0x18` = Z).
//! 4. Store the landing triple into `+0x24 / +0x26 / +0x28` - the end point
//!    `P2`.
//! 5. Store the three midpoints into `+0x3C / +0x3E / +0x40`, then **replace**
//!    the Y one with the arc's control point `C`.
//! 6. Seed the clip cursor `+0x9C = 0` and the per-frame step
//!    `+0x9E = 0x1000 / frames` (`0x1000` for `frames <= 0`, the retail
//!    divide-by-zero guard).
//! 7. Allocate a second helper from template `0x801F2294` with
//!    `+0x9E = frames`, `+0x9C = 0`, and set the player's movement-lock bit
//!    `0x80000` in `+0x10`. If that second allocation fails, the first helper
//!    gets the tear-down bit `8` in its `+0x10` instead and the hop is
//!    abandoned.
//!
//! # The arc
//!
//! The control point is the only interesting arithmetic, and it has a clean
//! closed form. With `mid = (p0 + p2) / 2` (arithmetic shift, so it rounds
//! toward negative infinity) and `hi = min(p0, p2)`:
//!
//! ```text
//! c = mid + 2 * (hi - apex - mid)
//! ```
//!
//! Evaluating the quadratic Bezier at `t = 0.5` gives `(p0 + 2c + p2) / 4`,
//! which collapses to exactly `hi - apex`. PSX world Y grows downward, so
//! subtracting `apex` raises the point: **the hop peaks `apex` units above
//! whichever endpoint is higher**, independent of how far apart they are.
//! That invariant is what [`hop_apex_height`] and the unit tests below pin.
//!
//! # Not wired
//!
//! The clip this seeds is consumed by the per-frame advance `FUN_801d2298`,
//! which drives a spawned helper actor through the pool - neither the pool nor
//! the helper-actor class exists in `engine-core`'s world model yet, and
//! `World::field_ledge_hop` currently posts the hop and stops. Wiring it means
//! editing `engine-core/src/world/**`, which this change deliberately leaves
//! alone.

/// Fixed-point full-clip extent: retail's cursor runs `0 ..= 0x1000`.
pub const CLIP_FULL: i32 = 0x1000;

/// The landing point retail writes into the helper actor's `+0x24 / +0x26 /
/// +0x28` - the three stack half-words the caller builds at `sp+0x18`.
///
/// Mirrors `engine_core::world::FieldLedgeHop`'s `target_*` fields; kept
/// local so this module stays free of a dependency on the world model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HopTarget {
    /// Landing X (`+0x24`).
    pub x: i16,
    /// Landing Y (`+0x26`) - the floor height sampled one step ahead.
    pub y: i16,
    /// Landing Z (`+0x28`).
    pub z: i16,
}

/// The clip `FUN_801d2404` seeds on the helper actor it spawns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HopArc {
    /// Bezier start point - the player's `+0x14 / +0x16 / +0x18` at the
    /// instant the hop starts, copied verbatim into the helper.
    pub start: (i16, i16, i16),
    /// Bezier end point - the caller's landing triple.
    pub end: (i16, i16, i16),
    /// Bezier control point, stored at `+0x3C / +0x3E / +0x40`. X and Z are
    /// plain midpoints; Y carries the apex correction.
    pub control: (i16, i16, i16),
    /// Clip cursor `+0x9C`, always seeded to `0`.
    pub cursor: i16,
    /// Per-frame cursor step `+0x9E` = `0x1000 / frames`, or `0x1000` when
    /// `frames <= 0`.
    pub step: i16,
    /// The second helper's `+0x9E`: the raw frame count, stored unscaled.
    pub paired_frames: i16,
}

/// Signed midpoint exactly as retail computes it: `addu` then `sra 1`, i.e. a
/// floor-division by two rather than the truncating division Rust's `/` would
/// give for negative sums.
fn mid(a: i16, b: i16) -> i16 {
    ((a as i32 + b as i32) >> 1) as i16
}

/// Build the hop clip.
///
/// `start` is the player's live transform (`+0x14 / +0x16 / +0x18`), `target`
/// the landing triple, `apex` the `a1` height (`0x10` up / `0x18` down) and
/// `frames` the `a2` clip length (retail always passes `0x10`).
///
/// PORT: FUN_801d2404
// NOT WIRED: the helper-actor pool `FUN_801d2404` allocates from, and the
// per-frame advance `FUN_801d2298` that consumes the clip, have no
// counterpart in `engine-core`'s world model; wiring would edit
// `engine-core/src/world/**`, owned elsewhere.
pub fn build_hop_arc(start: (i16, i16, i16), target: HopTarget, apex: i16, frames: i16) -> HopArc {
    let end = (target.x, target.y, target.z);

    // `+0x3C / +0x3E / +0x40` - the plain midpoints, written first.
    let mid_x = mid(start.0, end.0);
    let mid_y = mid(start.1, end.1);
    let mid_z = mid(start.2, end.2);

    // `+0x3E` is then overwritten with the arc control point. Retail selects
    // `min(start.y, end.y)` with an `slt`/`move` pair, then evaluates
    // `mid + 2 * (hi - apex - mid)` with a `subu`/`subu`/`sll 1`/`addu` chain.
    let hi = start.1.min(end.1) as i32;
    let ctrl_y = (mid_y as i32 + (((hi - apex as i32) - mid_y as i32) << 1)) as i16;

    // `+0x9E`: `blez` guard first, so a non-positive frame count skips the
    // divide entirely and stores the full extent.
    let step = if frames > 0 {
        (CLIP_FULL / frames as i32) as i16
    } else {
        CLIP_FULL as i16
    };

    HopArc {
        start,
        end,
        control: (mid_x, ctrl_y, mid_z),
        cursor: 0,
        step,
        paired_frames: frames,
    }
}

/// Evaluate the seeded quadratic Bezier at `t = num/den`, in the same
/// `0x1000`-scaled fixed point the clip cursor uses.
///
/// Provided so the arc's shape is testable without the per-frame advance:
/// retail's `FUN_801d2298` walks `+0x9C` from `0` to `+0x9E` and feeds it
/// through the same basis functions.
pub fn bezier_at(p0: i16, c: i16, p2: i16, cursor: i32) -> i32 {
    let t = cursor.clamp(0, CLIP_FULL) as i64;
    let u = CLIP_FULL as i64 - t;
    let full = CLIP_FULL as i64;
    // (u^2 * p0 + 2*u*t*c + t^2 * p2) / 0x1000^2 - widened because the
    // numerator alone needs 40-odd bits at full i16 range.
    let acc = u * u * p0 as i64 + 2 * u * t * c as i64 + t * t * p2 as i64;
    (acc / (full * full)) as i32
}

/// How far above the higher endpoint the seeded arc peaks, in world units.
///
/// Always equals the `apex` argument by construction - see the module docs.
pub fn hop_apex_height(arc: &HopArc) -> i32 {
    let hi = arc.start.1.min(arc.end.1) as i32;
    hi - bezier_at(arc.start.1, arc.control.1, arc.end.1, CLIP_FULL / 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(x: i16, y: i16, z: i16) -> HopTarget {
        HopTarget { x, y, z }
    }

    #[test]
    fn midpoints_are_plain_for_x_and_z() {
        let arc = build_hop_arc((100, 0, 200), target(300, 0, 400), 0x10, 0x10);
        assert_eq!(arc.control.0, 200);
        assert_eq!(arc.control.2, 300);
    }

    #[test]
    fn step_is_full_extent_over_frame_count() {
        let arc = build_hop_arc((0, 0, 0), target(0, 0, 0), 0x10, 0x10);
        assert_eq!(arc.step, 0x100);
        assert_eq!(arc.cursor, 0);
        assert_eq!(arc.paired_frames, 0x10);
    }

    #[test]
    fn non_positive_frame_count_takes_the_blez_arm() {
        for frames in [0i16, -1, -0x10] {
            let arc = build_hop_arc((0, 0, 0), target(0, 0, 0), 0x10, frames);
            assert_eq!(arc.step, CLIP_FULL as i16, "frames = {frames}");
        }
    }

    #[test]
    fn arc_peaks_apex_units_above_the_higher_endpoint() {
        // PSX Y grows downward, so "higher" is the smaller value.
        for (p0y, p2y) in [(0i16, 0i16), (0, 200), (200, 0), (-64, 96), (96, -64)] {
            for apex in [0x10i16, 0x18] {
                let arc = build_hop_arc((0, p0y, 0), target(0, p2y, 0), apex, 0x10);
                assert_eq!(
                    hop_apex_height(&arc),
                    apex as i32,
                    "p0y={p0y} p2y={p2y} apex={apex}"
                );
            }
        }
    }

    #[test]
    fn endpoints_are_exact() {
        let arc = build_hop_arc((10, 40, 70), target(90, 120, 150), 0x10, 0x10);
        assert_eq!(bezier_at(arc.start.1, arc.control.1, arc.end.1, 0), 40);
        assert_eq!(
            bezier_at(arc.start.1, arc.control.1, arc.end.1, CLIP_FULL),
            120
        );
    }

    #[test]
    fn midpoint_uses_arithmetic_shift_not_truncating_division() {
        // -3 + 0 = -3; `sra 1` floors to -2, whereas `-3 / 2` truncates to -1.
        assert_eq!(mid(-3, 0), -2);
    }
}

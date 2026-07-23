//! Battle-camera fixed-point kernels: the angle-tween step-table builder and
//! the LCG screen-shake jitter.
//!
//! PORT: FUN_801D829C - battle-camera angle-interpolation builder
//! PORT: FUN_801D9D30 - LCG camera-shake jitter
//!
//! Both live in runtime overlays (`overlay_battle_action_801d829c.txt` /
//! `overlay_dialog_801d9d30.txt` dumps) and are pure fixed-point math over the
//! camera globals - the rotation trio at `DAT_8007B790/2/4`, the shake
//! accumulator trio at `0x800840B8/BC/C0`, and the focus trio at
//! `0x80089118/1C/20`. The ports below lift the global reads/writes into
//! caller-owned parameters so the kernels are side-effect-free.
//!
//! REF: `engine-render::window::CutsceneCameraInterp` approximates the *field
//! cutscene* camera ease (`FUN_801DB510`) with float lerps toward op-`0x45`
//! keyframe targets. The retail battle tween here is a different mechanism:
//! table-driven fixed-point stepping - `FUN_801D829C` precomputes a 9-record
//! `{step_count, endpoint}` table (at battle-context `+0x118C` off
//! `_DAT_8007BD24`) that a per-frame walker (`FUN_80021248` arms it) then
//! advances by a constant per-frame increment, with 12-bit wrapped angles.
//! No float easing is involved in retail.

use crate::battle_formulas::psyq_rand_step;

/// The camera rotation/shake/focus trios `FUN_801D829C` tweens. Mirrors the
/// nine 16-bit globals paired against the caller-provided targets:
///
/// - `rotation`: pitch/yaw/roll at `DAT_8007B790/2/4` - 12-bit angles
///   (`0x1000` = full turn).
/// - `shake`: the low halves of the shake accumulator words at
///   `0x800840B8/BC/C0` (the same words [`apply_shake`] jitters).
/// - `focus`: the focus trio at `0x80089118/1C/20`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CameraAngles {
    /// Pitch/yaw/roll, 12-bit angle units.
    pub rotation: [i16; 3],
    /// Shake-offset trio.
    pub shake: [i16; 3],
    /// Focus trio.
    pub focus: [i16; 3],
}

/// One record of the interpolation step table at battle-context `+0x118C`:
/// `{u16 step_count, u16 endpoint}` per tweened value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TweenSlot {
    /// `ceil(|current - target| / speed)` - number of per-frame steps the
    /// walker takes to reach the endpoint. Zero when the value is already at
    /// its target (no steps emitted).
    pub steps: u16,
    /// Raw 16 bits of the (possibly wrap-adjusted) target value the walker
    /// converges on.
    pub target: u16,
}

/// Number of `{steps, endpoint}` records the builder emits - one per
/// (current, target) pair. NB the retail loop counter runs to `0x12` (18) in
/// increments of 2 over the interleaved 18-pointer list, so this is **9
/// slots**, not 18 (an earlier triage note miscounted the pointer list as the
/// slot count).
pub const TWEEN_SLOTS: usize = 9;

/// PORT: FUN_801D829C - build the per-frame camera angle-tween step table.
///
/// Faithful to the dump, in order:
///
/// 1. `target.shake[2]` (retail `param_2 + 4`) is pre-scaled **in place** by
///    `(value << 8) / 0xA0` (signed, truncating toward zero) - the `0xA0`
///    divide applies to this one field only, not the whole table.
/// 2. The **rotation trio only** (first 3 pairs) is normalized to 12 bits
///    (`& 0xFFF` on both current and target, written back in place) with
///    shortest-arc correction: when `current - target > 0x800` the target
///    gains `+0x1000`; when `target - current > 0x800` the current side
///    gains `+0x1000`. Shake and focus pairs are tweened linearly, unwrapped.
/// 3. For all [`TWEEN_SLOTS`] pairs (rotation, shake, focus - in that order)
///    the step count is the ceiling division
///    `(|current - target| + speed - 1) / speed` (retail `divu`), with
///    `speed == 0` coerced to `1`, and the endpoint is the target's raw
///    16 bits.
///
/// Retail mutates both the globals and the caller's target buffer (masking,
/// wrap adjust, the `0xA0` pre-scale), hence `&mut` on both sides here. The
/// retail tail then arms the step-table walker (`FUN_80021248`) on the
/// freshly written table; that walker is out of scope for this kernel.
///
/// NOT WIRED: the builder's only product is a 9-record `{step_count,
/// endpoint}` table for a per-frame walker to advance, and the engine has no
/// walker to hand it to - the battle camera is framed by a per-action snap
/// (`battle_formulas::camera_height_for_frame` through
/// `BattleActionHost::camera_bounds`), not by stepping angles toward a
/// target. The routine that arms retail's walker, `FUN_80021248`, is
/// documented but unported, so nothing exists to consume a step table.
pub fn build_camera_angle_tween(
    current: &mut CameraAngles,
    target: &mut CameraAngles,
    speed: u16,
) -> [TweenSlot; TWEEN_SLOTS] {
    // Step 1: pre-scale the third shake target by 256/160 (signed, trunc).
    target.shake[2] = (((target.shake[2] as i32) << 8) / 0xA0) as i16;

    // Step 2: 12-bit normalize + shortest-arc on the rotation trio.
    for k in 0..3 {
        let mut cur = current.rotation[k] & 0xFFF;
        let mut tgt = target.rotation[k] & 0xFFF;
        if (cur as i32) - (tgt as i32) > 0x800 {
            tgt += 0x1000;
        }
        if (tgt as i32) - (cur as i32) > 0x800 {
            cur += 0x1000;
        }
        current.rotation[k] = cur;
        target.rotation[k] = tgt;
    }

    // Step 3: ceil-divide step counts + endpoints for all nine pairs.
    let speed = u32::from(speed).max(1);
    let pairs: [(i16, i16); TWEEN_SLOTS] = [
        (current.rotation[0], target.rotation[0]),
        (current.rotation[1], target.rotation[1]),
        (current.rotation[2], target.rotation[2]),
        (current.shake[0], target.shake[0]),
        (current.shake[1], target.shake[1]),
        (current.shake[2], target.shake[2]),
        (current.focus[0], target.focus[0]),
        (current.focus[1], target.focus[1]),
        (current.focus[2], target.focus[2]),
    ];
    let mut table = [TweenSlot::default(); TWEEN_SLOTS];
    for (slot, &(cur, tgt)) in table.iter_mut().zip(pairs.iter()) {
        let delta = ((cur as i32) - (tgt as i32)).unsigned_abs();
        // Retail form: `(delta + speed - 1) divu speed` - identical to
        // `div_ceil` for speed >= 1.
        slot.steps = delta.div_ceil(speed) as u16;
        slot.target = tgt as u16;
    }
    table
}

/// PORT: FUN_801D9D30 - re-roll the two-axis LCG camera-shake jitter.
///
/// Retail state: `accum` is the shake accumulator pair at
/// `0x800840B8/0x800840BC`, `offset` is the previously applied jitter pair
/// held in the camera context (`DAT_801C6EA4 + 0x18/+0x1C`), `amplitude` is
/// the shake-amplitude global `_DAT_8007B630`.
///
/// The previous offsets are first subtracted back out of the accumulators and
/// zeroed. When `amplitude != 0`, two LCG samples are drawn (retail RNG
/// `FUN_80056798`, the PsyQ 15-bit `rand()` - reused here via
/// [`psyq_rand_step`]) and masked with
/// `0xFFFFFF >> ((0x15 - amplitude) & 0x1F)` - i.e. `(1 << (amplitude + 3)) - 1`
/// for `1 <= amplitude <= 0x15`. NB the dump uses this **right-shift-of-
/// `0xFFFFFF`** form (`srav` of the `0xFFFFFF` constant); the
/// `(1 << (0x15 - amplitude)) - 1` form quoted in `docs/reference/functions.md`
/// row 662 does not match the disassembly (it would shrink with amplitude).
///
/// - X offset: `(rand & mask) - ((mask + 1) >> 1)` - centered around zero.
/// - Y offset: `-(rand & (mask >> 1))` - half-range, upward only (negated).
///
/// The fresh offsets are then added into the accumulators. `amplitude == 0`
/// therefore clears the jitter contribution entirely.
///
/// REF: this routine is also duplicated verbatim as the tail of
/// `FUN_801DB510` (the camera follow-ease) - one port covers both sites.
pub fn apply_shake(accum: &mut [i32; 2], offset: &mut [i32; 2], amplitude: u32, seed: &mut u32) {
    accum[0] -= offset[0];
    accum[1] -= offset[1];
    offset[0] = 0;
    offset[1] = 0;
    if amplitude != 0 {
        let mask = 0xFF_FFFFu32 >> (0x15u32.wrapping_sub(amplitude) & 0x1F);
        let r0 = u32::from(psyq_rand_step(seed));
        offset[0] = (r0 & mask) as i32 - ((mask as i32 + 1) >> 1);
        let r1 = u32::from(psyq_rand_step(seed));
        offset[1] = -((r1 & (mask >> 1)) as i32);
    }
    accum[0] += offset[0];
    accum[1] += offset[1];
}

#[cfg(test)]
mod tests {
    use super::*;

    fn angles(rotation: [i16; 3], shake: [i16; 3], focus: [i16; 3]) -> CameraAngles {
        CameraAngles {
            rotation,
            shake,
            focus,
        }
    }

    #[test]
    fn shortest_arc_wraps_target_up_across_zero() {
        // current 0xF80, target 0x010: raw delta 0xF70 > 0x800, so the
        // target gains a full turn (0x1010) and the arc becomes 0x90.
        let mut cur = angles([0xF80, 0, 0], [0; 3], [0; 3]);
        let mut tgt = angles([0x010, 0, 0], [0; 3], [0; 3]);
        let table = build_camera_angle_tween(&mut cur, &mut tgt, 1);
        assert_eq!(tgt.rotation[0], 0x1010);
        assert_eq!(cur.rotation[0], 0xF80);
        assert_eq!(table[0].steps, 0x90);
        assert_eq!(table[0].target, 0x1010);
    }

    #[test]
    fn shortest_arc_wraps_current_up_across_zero() {
        // current 0x010, target 0xF80: target - current > 0x800, so the
        // CURRENT side gains the full turn; the endpoint stays 0xF80.
        let mut cur = angles([0x010, 0, 0], [0; 3], [0; 3]);
        let mut tgt = angles([0xF80, 0, 0], [0; 3], [0; 3]);
        let table = build_camera_angle_tween(&mut cur, &mut tgt, 1);
        assert_eq!(cur.rotation[0], 0x1010);
        assert_eq!(tgt.rotation[0], 0xF80);
        assert_eq!(table[0].steps, 0x90);
        assert_eq!(table[0].target, 0xF80);
    }

    #[test]
    fn rotation_masked_to_12_bits_both_sides() {
        let mut cur = angles([0x1234, 0x2FFF, 0], [0; 3], [0; 3]);
        let mut tgt = angles([0x1234, 0x2FFF, 0], [0; 3], [0; 3]);
        let table = build_camera_angle_tween(&mut cur, &mut tgt, 4);
        assert_eq!(cur.rotation[0], 0x234);
        assert_eq!(tgt.rotation[0], 0x234);
        assert_eq!(cur.rotation[1], 0xFFF);
        assert_eq!(tgt.rotation[1], 0xFFF);
        // Equal after masking: zero-delta slots emit no steps.
        assert_eq!(table[0].steps, 0);
        assert_eq!(table[1].steps, 0);
    }

    #[test]
    fn step_count_is_ceiling_division() {
        let mut cur = angles([0; 3], [0x90, 0x80, 0], [0; 3]);
        let mut tgt = angles([0; 3], [0, 0, 0], [0; 3]);
        let table = build_camera_angle_tween(&mut cur, &mut tgt, 0x20);
        // 0x90 / 0x20 = 4.5 -> 5 steps; 0x80 / 0x20 = exact 4.
        assert_eq!(table[3].steps, 5);
        assert_eq!(table[4].steps, 4);
    }

    #[test]
    fn zero_speed_coerced_to_one() {
        let mut cur = angles([0; 3], [0; 3], [7, 0, 0]);
        let mut tgt = angles([0; 3], [0; 3], [0, 0, 0]);
        let table = build_camera_angle_tween(&mut cur, &mut tgt, 0);
        assert_eq!(table[6].steps, 7);
    }

    #[test]
    fn zero_delta_slots_produce_no_steps() {
        let mut cur = angles([0x100, 0x200, 0x300], [1, 2, 0], [4, 5, 6]);
        let mut tgt = cur;
        // shake[2] pre-scale keeps 0 at 0, so every pair stays equal.
        let table = build_camera_angle_tween(&mut cur, &mut tgt, 8);
        for slot in &table {
            assert_eq!(slot.steps, 0);
        }
    }

    #[test]
    fn shake_third_target_prescaled_by_0xa0_divide() {
        // (0xA0 << 8) / 0xA0 = 0x100; negative values truncate toward zero
        // ((-1 << 8) / 0xA0 = -1, not -2).
        let mut cur = angles([0; 3], [0; 3], [0; 3]);
        let mut tgt = angles([0; 3], [0, 0, 0xA0], [0; 3]);
        let table = build_camera_angle_tween(&mut cur, &mut tgt, 1);
        assert_eq!(tgt.shake[2], 0x100);
        assert_eq!(table[5].steps, 0x100);
        assert_eq!(table[5].target, 0x100);

        let mut tgt_neg = angles([0; 3], [0, 0, -1], [0; 3]);
        build_camera_angle_tween(&mut cur, &mut tgt_neg, 1);
        assert_eq!(tgt_neg.shake[2], -1);
    }

    #[test]
    fn negative_deltas_use_absolute_value() {
        let mut cur = angles([0; 3], [0; 3], [-0x40, 0, 0]);
        let mut tgt = angles([0; 3], [0; 3], [0x40, 0, 0]);
        let table = build_camera_angle_tween(&mut cur, &mut tgt, 0x10);
        assert_eq!(table[6].steps, 8); // |(-0x40) - 0x40| = 0x80 -> /0x10
        assert_eq!(table[6].target, 0x40);
    }

    #[test]
    fn shake_zero_amplitude_clears_previous_offset() {
        let mut accum = [100, 200];
        let mut offset = [30, -40];
        let mut seed = 0x1234_5678u32;
        apply_shake(&mut accum, &mut offset, 0, &mut seed);
        assert_eq!(offset, [0, 0]);
        assert_eq!(accum, [70, 240]); // previous jitter backed out, none added
        assert_eq!(seed, 0x1234_5678); // no RNG draw at amp 0
    }

    #[test]
    fn shake_seeded_rng_is_deterministic() {
        let run = || {
            let mut accum = [0, 0];
            let mut offset = [0, 0];
            let mut seed = 0xDEAD_BEEFu32;
            apply_shake(&mut accum, &mut offset, 5, &mut seed);
            (accum, offset, seed)
        };
        assert_eq!(run(), run());
        // Two draws happen: the seed must have advanced twice.
        let (accum, offset, seed) = run();
        let mut check = 0xDEAD_BEEFu32;
        psyq_rand_step(&mut check);
        psyq_rand_step(&mut check);
        assert_eq!(seed, check);
        assert_eq!(accum, offset); // fresh offsets fold straight into accum
    }

    #[test]
    fn shake_offsets_bounded_per_amplitude() {
        for amp in 1u32..=10 {
            let mask = 0xFF_FFFFu32 >> (0x15 - amp); // == (1 << (amp + 3)) - 1
            let half = ((mask + 1) >> 1) as i32;
            let mut seed = 0x0BAD_F00Du32.wrapping_add(amp);
            for _ in 0..64 {
                let mut accum = [0, 0];
                let mut offset = [0, 0];
                apply_shake(&mut accum, &mut offset, amp, &mut seed);
                // X centered: [-half, mask - half]; Y negated half-mask: [-(mask>>1), 0].
                assert!(offset[0] >= -half && offset[0] <= mask as i32 - half);
                assert!(offset[1] <= 0 && offset[1] >= -((mask >> 1) as i32));
                assert_eq!(accum, offset);
            }
        }
    }

    #[test]
    fn shake_reroll_replaces_previous_offset() {
        let mut accum = [500, -500];
        let mut offset = [12, -34];
        let mut seed = 42u32;
        let base = [accum[0] - offset[0], accum[1] - offset[1]];
        apply_shake(&mut accum, &mut offset, 3, &mut seed);
        assert_eq!(accum, [base[0] + offset[0], base[1] + offset[1]]);
        assert_ne!(offset, [12, -34]);
    }
}

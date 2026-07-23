//! Battle-actor separation ("push-apart") kernel: the pairwise overlap-nudge
//! and the all-pairs driver that runs it over the battle actor table.
//!
//! PORT: FUN_80050BB8 - pairwise battle-actor separation nudge
//! PORT: FUN_80051078 - all-pairs separation driver (7x7 over the actor table)
//!
//! Both live in `SCUS_942.54` (`funcs/80050bb8.txt` / `80051078.txt`). The pair
//! kernel projects the between-actor gap onto the angle between the two actors
//! and, when the projected gap is below `(r1 + r2) / 6`, nudges both actors'
//! position accumulators apart. The projection reads the game's sin/cos LUTs at
//! an index derived from an `atan2`-shaped angle (retail `FUN_80019B28`); those
//! table lookups are lifted into caller-provided `sin`/`cos` parameters here so
//! the kernel is side-effect-free and carries no Sony bytes.
//!
//! REF: this is positional physics the engine's own battle loop does not model
//! (see `docs/subsystems/battle.md`). The port exists as a faithful, testable
//! mirror of the retail arithmetic, in the same spirit as the other pure
//! fixed-point battle kernels in this crate (`battle_camera`, `battle_formulas`).
//!
//! # NOT WIRED
//!
//! Three things have to exist before a caller can run this pass. The engine
//! seats battle actors at fixed formation points and never integrates a
//! position, so there is no per-frame slot for an all-pairs nudge. The
//! accumulators the kernel writes are the actor `+0x34` / `+0x38` pair, which
//! `BattleActor` does not carry (the port's positions live on the actor's
//! `move_state`, a different field set with no separate accumulator). And the
//! body radius is `*(actor+0x22C)+0x58`, a field of the monster render record
//! the engine does not load into the battle actor at all.

/// One actor's separation-relevant state, mirroring the retail battle-actor
/// struct fields the kernel reads and writes:
///
/// - `radius`: body radius at `*(i16)(*(actor + 0x22C) + 0x58)`.
/// - `x` / `z`: horizontal position at `+0x3C` / `+0x40` (`i16`).
/// - `acc_x` / `acc_z`: the position accumulators at `+0x34` / `+0x38`, loaded
///   with `lhu` and stored with `sh`, so they wrap in 16 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SepActor {
    /// Body radius (`*(i16)(*(actor+0x22C)+0x58)`).
    pub radius: i16,
    /// X position (`+0x3C`).
    pub x: i16,
    /// Z position (`+0x40`).
    pub z: i16,
    /// X position accumulator (`+0x34`), 16-bit wrapping.
    pub acc_x: u16,
    /// Z position accumulator (`+0x38`), 16-bit wrapping.
    pub acc_z: u16,
}

/// Number of actor slots the driver sweeps (`DAT_801C9370[0..=6]`).
pub const SEPARATION_SLOTS: usize = 7;

/// PORT: FUN_80050BB8 - nudge two overlapping battle actors apart.
///
/// `a` is the first actor (retail `param_1`), `b` the second (`param_2`).
/// `sin` and `cos` are the LUT samples `_DAT_8007B81C[idx]` / `DAT_8007B7F8[idx]`
/// (as signed `i16`) at the index the caller derives from the between-actor
/// angle: `idx = (atan2_units + 0x800) & 0xFFF`, `atan2_units` being retail
/// `FUN_80019B28(b.z, b.x, a.z, a.x)`. Supplying the trig samples keeps this
/// kernel free of the game's angle table.
///
/// Faithful to the disassembly, in order:
///
/// 1. `dx = a.x - b.x`, `dz = a.z - b.z`.
/// 2. `proj = |(|dx| * sin) >> 12| + |(|dz| * cos) >> 12|` (each product
///    arithmetic-shifted right 12, then made absolute).
/// 3. `threshold = (a.radius + b.radius) / 6` (signed, truncating toward zero).
/// 4. The overlap test is retail `sltu`: `(proj as u32) < (threshold as u32)`.
/// 5. When overlapping, with `sin_step = sin >> 10` and `cos_step = cos >> 10`
///    (arithmetic shifts of the signed samples): `a` moves by `-(sin_step,
///    cos_step)` and `b` by `+(sin_step, cos_step)`, each accumulator wrapping
///    in 16 bits (retail `lhu`/`subu`/`addu`/`sh`).
///
/// Returns `true` when the actors were overlapping and both accumulators were
/// nudged, `false` when the pair was left untouched.
pub fn push_apart(a: &mut SepActor, b: &mut SepActor, sin: i16, cos: i16) -> bool {
    let dx = i32::from(a.x) - i32::from(b.x);
    let dz = i32::from(a.z) - i32::from(b.z);

    let proj =
        ((dx.abs() * i32::from(sin)) >> 12).abs() + ((dz.abs() * i32::from(cos)) >> 12).abs();
    let threshold = (i32::from(a.radius) + i32::from(b.radius)) / 6;

    if (proj as u32) < (threshold as u32) {
        let sin_step = i32::from(sin) >> 10;
        let cos_step = i32::from(cos) >> 10;
        a.acc_x = (i32::from(a.acc_x) - sin_step) as u16;
        a.acc_z = (i32::from(a.acc_z) - cos_step) as u16;
        b.acc_x = (i32::from(b.acc_x) + sin_step) as u16;
        b.acc_z = (i32::from(b.acc_z) + cos_step) as u16;
        true
    } else {
        false
    }
}

/// PORT: FUN_80051078 - run the pair nudge over every ordered pair of living
/// actors.
///
/// Retail walks the `SEPARATION_SLOTS`-entry actor table `DAT_801C9370` as a
/// nested loop and, for each ordered pair `(i, j)` with `i != j` and both
/// actors present-and-living (the table slot's pointer non-null and its actor's
/// `+0x4` field non-zero), calls [`push_apart`]`(i, j)`. Every living actor is
/// therefore pushed off every other one exactly once per pass. `alive[k]`
/// abstracts the retail liveness test for slot `k`; `push(i, j)` is invoked in
/// the retail visitation order.
pub fn separation_pass<F: FnMut(usize, usize)>(alive: &[bool; SEPARATION_SLOTS], mut push: F) {
    for i in 0..SEPARATION_SLOTS {
        for j in 0..SEPARATION_SLOTS {
            if i != j && alive[i] && alive[j] {
                push(i, j);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor(radius: i16, x: i16, z: i16) -> SepActor {
        SepActor {
            radius,
            x,
            z,
            acc_x: 0,
            acc_z: 0,
        }
    }

    #[test]
    fn overlapping_pair_nudges_symmetrically() {
        // Actors near-coincident: dx = dz = 0 -> proj = 0. Large radii keep the
        // threshold positive, so the pair overlaps and is pushed.
        let mut a = actor(0x60, 0, 0);
        let mut b = actor(0x60, 0, 0);
        // sin = 0x400 -> step 1; cos = 0x800 -> step 2.
        let pushed = push_apart(&mut a, &mut b, 0x400, 0x800);
        assert!(pushed);
        // a moves by -(step), b by +(step); accumulators wrap in 16 bits.
        assert_eq!(a.acc_x, 0u16.wrapping_sub(1)); // 0xFFFF
        assert_eq!(a.acc_z, 0u16.wrapping_sub(2)); // 0xFFFE
        assert_eq!(b.acc_x, 1);
        assert_eq!(b.acc_z, 2);
    }

    #[test]
    fn threshold_is_signed_divide_by_six() {
        // r1 + r2 = 11 -> 11 / 6 = 1 (truncating). proj must be < 1, i.e. 0,
        // for a push. Coincident actors give proj = 0.
        let mut a = actor(5, 0, 0);
        let mut b = actor(6, 0, 0);
        assert!(push_apart(&mut a, &mut b, 0x100, 0x100));

        // r1 + r2 = 5 -> 5 / 6 = 0. proj = 0 is NOT < 0, so no push.
        let mut c = actor(2, 0, 0);
        let mut d = actor(3, 0, 0);
        assert!(!push_apart(&mut c, &mut d, 0x100, 0x100));
        assert_eq!((c.acc_x, c.acc_z, d.acc_x, d.acc_z), (0, 0, 0, 0));
    }

    #[test]
    fn separated_pair_left_untouched() {
        // Wide horizontal gap: |dx| * sin >> 12 dominates the tiny threshold.
        let mut a = actor(0x10, 0x400, 0);
        let mut b = actor(0x10, 0, 0);
        let pushed = push_apart(&mut a, &mut b, 0x1000, 0x1000);
        assert!(!pushed);
        assert_eq!((a.acc_x, a.acc_z, b.acc_x, b.acc_z), (0, 0, 0, 0));
    }

    #[test]
    fn projection_uses_absolute_values() {
        // Mirror the previous separated pair with the sign of dx flipped and a
        // negative sin: the projection is unchanged (all abs), still separated.
        let mut a = actor(0x10, -0x400, 0);
        let mut b = actor(0x10, 0, 0);
        assert!(!push_apart(&mut a, &mut b, -0x1000, 0x1000));

        // And a coincident pair with a negative sin still pushes; the step is
        // the arithmetic shift of the signed sample (sign preserved).
        let mut c = actor(0x60, 0, 0);
        let mut d = actor(0x60, 0, 0);
        assert!(push_apart(&mut c, &mut d, -0x800, 0));
        // sin_step = -0x800 >> 10 = -2. c.acc_x -= (-2) => +2; d.acc_x += (-2).
        assert_eq!(c.acc_x, 2);
        assert_eq!(d.acc_x, 0u16.wrapping_sub(2));
    }

    #[test]
    fn small_samples_shift_to_zero_step() {
        // |sample| < 0x400 arithmetic-shifts to 0, so an overlapping pair with
        // sub-0x400 samples reports a push but moves nothing.
        let mut a = actor(0x60, 0, 0);
        let mut b = actor(0x60, 0, 0);
        assert!(push_apart(&mut a, &mut b, 0x3FF, 0x1FF));
        assert_eq!((a.acc_x, a.acc_z, b.acc_x, b.acc_z), (0, 0, 0, 0));
    }

    #[test]
    fn driver_visits_every_ordered_living_pair_in_retail_order() {
        // Slots 1, 3, 4 living; 0, 2, 5, 6 dead.
        let mut alive = [false; SEPARATION_SLOTS];
        alive[1] = true;
        alive[3] = true;
        alive[4] = true;
        let mut visited = Vec::new();
        separation_pass(&alive, |i, j| visited.push((i, j)));
        assert_eq!(
            visited,
            vec![(1, 3), (1, 4), (3, 1), (3, 4), (4, 1), (4, 3)]
        );
    }

    #[test]
    fn driver_skips_self_pairs_and_dead_actors() {
        let mut alive = [false; SEPARATION_SLOTS];
        alive[2] = true; // single living actor -> no pairs at all.
        let mut count = 0;
        separation_pass(&alive, |i, j| {
            assert_ne!(i, j);
            count += 1;
        });
        assert_eq!(count, 0);

        // No actors living -> nothing visited.
        let none = [false; SEPARATION_SLOTS];
        let mut any = false;
        separation_pass(&none, |_, _| any = true);
        assert!(!any);
    }

    #[test]
    fn driver_and_kernel_compose_deterministically() {
        // Drive the kernel over a full living table with fixed trig samples and
        // assert the pass is reproducible.
        let run = || {
            let mut actors = [
                actor(0x40, 0, 0),
                actor(0x40, 0, 0),
                actor(0x40, 0x1000, 0),
                actor(0x40, 0, 0),
                actor(0x40, 0, 0),
                actor(0x40, 0, 0),
                actor(0x40, 0, 0),
            ];
            let alive = [true; SEPARATION_SLOTS];
            // Collect the pair order, applying the kernel with constant samples.
            let mut order = Vec::new();
            let mut snapshot = actors;
            separation_pass(&alive, |i, j| {
                order.push((i, j));
                let (lo, hi) = if i < j { (i, j) } else { (j, i) };
                let (left, right) = snapshot.split_at_mut(hi);
                let (ai, aj) = if i < j {
                    (&mut left[lo], &mut right[0])
                } else {
                    (&mut right[0], &mut left[lo])
                };
                push_apart(ai, aj, 0x400, 0x400);
            });
            actors = snapshot;
            (order, actors)
        };
        assert_eq!(run(), run());
    }
}

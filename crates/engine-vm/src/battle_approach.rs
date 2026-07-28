//! The attack-approach distance clamp.
//!
//! PORT: FUN_801DF570
//! REF: FUN_801DEA50 (its one caller, at `0x801DEDC8`)
//!
//! `(slot, requested) -> i16`. Given an acting actor and a requested step, this
//! returns how far the approach is actually allowed to close - the kernel behind
//! the Attack band's "walk toward the target" states. It is a pure function of
//! the two actors' positions plus the requested distance.
//!
//! Transcribed from a disassembly of the mapped `0898` image rather than from a
//! dump: the corpus holds an `overlay_0897` slice at this VA reporting **94**
//! instructions where the battle-action image holds **82**, so the two disagree
//! on the body's own length and only one of them can be based correctly.
//!
//! ```bash
//! scripts/ghidra-analysis/disasm-overlay-fn.py \
//!     extracted/overlays/overlay_battle_action_0898.bin \
//!     --base 0x801CE818 --addr 0x801df570
//! ```
//!
//! ## What it computes
//!
//! ```text
//! actor  = pool[slot]
//! target = pool[actor[+0x1DD]]
//! a      = (bearing_12bit(target[+0x40], target[+0x3C],
//!                         actor[+0x38],  actor[+0x34]) + 0x800) & 0xFFF
//! d      = |(|actor.x - target.x| * sin[a]) >> 12|
//!        + |(|actor.z - target.z| * cos[a]) >> 12|
//! r = requested
//! if d < r { r = d }                  // r = min(requested, d)
//! if r < (d * 3) >> 2 { r = 3d/4 }    // then floored at three quarters of d
//! return r
//! ```
//!
//! So the result is `requested` clamped into `[3d/4, d]`: an approach never
//! overshoots the separation, and never gives up more than a quarter of it
//! either. The `+ 0x800` is a half-turn, so `a` is the bearing measured back
//! from the target toward the attacker.
//!
//! Two details the arithmetic depends on and a C rendering would flatten. Every
//! one of the four magnitude steps is a separate `bgez`/`negu` pair - the
//! coordinate deltas are made absolute *before* the multiply and the two
//! products are made absolute *again* after the `>> 12`, so a negative product
//! never cancels the other axis. And both clamp compares are **`sltu`**, not
//! `slt`: `requested` arrives sign-extended from a halfword, so a negative
//! request compares as a very large unsigned value and takes the `min` arm
//! rather than the `max` arm. [`approach_distance`] keeps that.
//!
//! # NOT WIRED
//!
//! No engine caller. The Attack band's approach states (`0x16` "advance",
//! `0x19` "short-step") are ported in
//! [`battle_action`](crate::battle_action), but they step the actor with the
//! band's own sin/cos drift rather than through a distance request, so there is
//! no site holding a `requested` value for this to clamp. Wiring it means the
//! approach states asking for a step length first - which is also the shape the
//! `0x19` approach-park investigation wants, since a clamp that can only return
//! `[3d/4, d]` can never close the last quarter on its own. See
//! `docs/subsystems/battle-action.md` ("The `0x19` attack-approach park").
//!
//! **The host's seat accessor does not unblock it**, and a triage note that
//! said it would is withdrawn. Positions were never the gap: the retail caller
//! is not the action SM at all. The corpus holds exactly one `jal 0x801df570`,
//! at `0x801DEDC8` inside `FUN_801DEA50` (the staged-value reader), and its
//! `a1` is a sign-extended halfword - the requested step. That is the value the
//! port has no producer for.

/// Half-turn added to the bearing before the LUT lookup (`0x800` of a 12-bit
/// angle).
pub const HALF_TURN: i32 = 0x800;
/// 12-bit angle mask.
pub const ANGLE_MASK: i32 = 0xFFF;
/// Fixed-point shift on each axis product (`sra ..., 0xC`).
pub const PRODUCT_SHIFT: u32 = 12;

/// The two actor positions the clamp reads, named by their record offsets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ApproachPose {
    /// `+0x34` - world X.
    pub x: i16,
    /// `+0x38` - world Z.
    pub z: i16,
    /// `+0x3C` - the target-side X the bearing and the delta are taken against.
    pub ref_x: i16,
    /// `+0x40` - the target-side Z.
    pub ref_z: i16,
}

/// The projected separation `d` between attacker and target.
///
/// `angle` is the already-biased, already-masked 12-bit angle; `sin` and `cos`
/// are the LUT reads at that angle (retail dereferences the pointers at
/// `_DAT_8007B81C` / `_DAT_8007B7F8` and indexes with a halfword stride).
pub fn projected_separation(
    attacker_x: i16,
    attacker_z: i16,
    target_x: i16,
    target_z: i16,
    sin: i16,
    cos: i16,
) -> i32 {
    let dx = (attacker_x as i32 - target_x as i32).abs();
    let dz = (attacker_z as i32 - target_z as i32).abs();
    let px = (dx.wrapping_mul(sin as i32) >> PRODUCT_SHIFT).abs();
    let pz = (dz.wrapping_mul(cos as i32) >> PRODUCT_SHIFT).abs();
    px.wrapping_add(pz)
}

/// The bearing the clamp feeds its LUT lookups, given the raw
/// [`bearing_12bit`](crate::battle_action::bearing_12bit) result.
pub const fn approach_angle(bearing: u16) -> i32 {
    (bearing as i32 + HALF_TURN) & ANGLE_MASK
}

/// Clamp a requested approach step against a projected separation.
///
/// Both compares are unsigned, as retail's are - see the module doc.
pub fn clamp_step(requested: i16, separation: i32) -> i16 {
    let mut r = requested as i32;
    if (separation as u32) < (r as u32) {
        r = separation;
    }
    let floor = (separation.wrapping_mul(3) as u32 >> 2) as i32;
    if ((r as i16) as u32) < (floor as u32) {
        r = floor;
    }
    r as i16
}

/// The whole routine: project the separation, then clamp the request.
///
/// PORT: FUN_801DF570
pub fn approach_distance(pose: ApproachPose, requested: i16, sin: i16, cos: i16) -> i16 {
    let d = projected_separation(pose.x, pose.z, pose.ref_x, pose.ref_z, sin, cos);
    clamp_step(requested, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angle_is_the_bearing_plus_a_half_turn_wrapped() {
        assert_eq!(approach_angle(0), 0x800);
        assert_eq!(approach_angle(0x800), 0);
        assert_eq!(approach_angle(0xFFF), 0x7FF);
        for b in [0u16, 1, 0x400, 0xC00, 0xFFF] {
            assert!((0..=ANGLE_MASK).contains(&approach_angle(b)), "bearing {b}");
        }
    }

    #[test]
    fn separation_is_the_sum_of_two_absolute_projections() {
        // sin = cos = 0x1000 (unit), so each axis contributes its own delta.
        let d = projected_separation(300, 400, 100, 100, 0x1000, 0x1000);
        assert_eq!(d, 200 + 300);
    }

    #[test]
    fn both_axes_are_made_absolute_so_they_cannot_cancel() {
        // A negative sin would subtract the X term if the abs were missing.
        let with_neg = projected_separation(300, 400, 100, 100, -0x1000, 0x1000);
        assert_eq!(with_neg, 200 + 300, "the X term is re-absolutised");
        // And a negative coordinate delta is absolutised before the multiply.
        let mirrored = projected_separation(-100, -200, 100, 100, 0x1000, 0x1000);
        assert_eq!(mirrored, 200 + 300);
    }

    #[test]
    fn a_request_larger_than_the_separation_is_capped_at_it() {
        assert_eq!(clamp_step(1000, 400), 400);
        assert_eq!(clamp_step(401, 400), 400);
    }

    #[test]
    fn a_request_below_three_quarters_is_lifted_to_three_quarters() {
        // d = 400 -> floor = 300.
        assert_eq!(clamp_step(0, 400), 300);
        assert_eq!(clamp_step(100, 400), 300);
        assert_eq!(clamp_step(299, 400), 300);
    }

    #[test]
    fn a_request_inside_the_band_is_returned_untouched() {
        assert_eq!(clamp_step(300, 400), 300);
        assert_eq!(clamp_step(350, 400), 350);
        assert_eq!(clamp_step(400, 400), 400);
    }

    #[test]
    fn the_result_always_lands_in_the_three_quarter_band() {
        for d in [1i32, 7, 64, 400, 4000] {
            let floor = (d * 3) >> 2;
            for req in [0i16, 1, 50, 300, 4000, 16000] {
                let r = clamp_step(req, d) as i32;
                assert!(r >= floor, "d={d} req={req} -> {r} below {floor}");
                assert!(r <= d.max(floor), "d={d} req={req} -> {r} above {d}");
            }
        }
    }

    #[test]
    fn a_negative_request_takes_the_min_arm_because_the_compare_is_unsigned() {
        // Retail's `sltu` sees a sign-extended -1 as 0xFFFFFFFF, so the first
        // clamp fires and the request collapses to the separation - the
        // opposite of what a signed compare would do.
        assert_eq!(clamp_step(-1, 400), 400);
        assert_eq!(clamp_step(-1000, 400), 400);
    }

    #[test]
    fn a_zero_separation_pins_the_result_at_zero() {
        assert_eq!(clamp_step(500, 0), 0);
        assert_eq!(clamp_step(0, 0), 0);
    }

    #[test]
    fn whole_routine_composes_the_two_halves() {
        let pose = ApproachPose {
            x: 300,
            z: 400,
            ref_x: 100,
            ref_z: 100,
        };
        // d = 500; a request of 1000 caps at 500, one of 10 lifts to 375.
        assert_eq!(approach_distance(pose, 1000, 0x1000, 0x1000), 500);
        assert_eq!(approach_distance(pose, 10, 0x1000, 0x1000), 375);
        assert_eq!(approach_distance(pose, 400, 0x1000, 0x1000), 400);
    }
}

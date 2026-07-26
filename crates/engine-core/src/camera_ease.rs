//! Field **camera yaw easing**: the per-frame step that walks the camera's
//! smoothed yaw toward the angle the current camera zone asks for.
//!
//! PORT: FUN_801DA390
//!
//! One call per frame. It owns a single global (`_DAT_8007BCAC`, seeded to
//! `0x3C` by the field initialiser) and moves it toward a target derived from
//! the camera-zone record's angle minus the player's facing. The interesting
//! part is the **step size**, which is not constant: while the camera is
//! settled it moves one unit a frame, and while it is not it moves by a
//! gap-proportional amount capped at twelve. That is what makes a scene
//! transition swing the camera round quickly and then creep the last few units.
//!
//! Provenance: `overlay_cutscene_dialogue_801da390.txt`, cross-checked against
//! `overlay_cutscene_mapview_801da390.txt` and the standalone `801da390.txt`
//! (all 99 instructions, identical).
//!
//! NOT WIRED (whole module). The engine's field camera
//! ([`crate::camera`]) eases in its own float-based controller against a typed
//! camera-zone record, and has no `_DAT_8007BCAC` channel to step - the retail
//! global is a fixed-point yaw the renderer reads directly. Adding one is a
//! **fidelity-mode decision**, not missing plumbing: the two easings would
//! disagree frame by frame, so the retail channel only makes sense alongside a
//! toggle that picks which one drives the camera. That is why this is disclosed
//! rather than wired - wiring it silently would change camera feel.
//!
//! REF: FUN_801D6704 (seeds the global to `0x3C`), FUN_801DBA20 (the zone query
//! that supplies the target angle)

/// Pad-word bit that suspends the easing entirely - the field input lock.
///
/// The same bit the dev-menu warp applier raises while its rise-up plays, so a
/// scripted camera move is not fought by the follow easing.
pub const PAD_INPUT_LOCKED: u32 = 0x0100_0000;

/// Pad-word bit selecting the **fast** arm.
pub const PAD_FAST_ARM: u32 = 0x0002_0000;

/// Mask applied to `_DAT_8007B850` on the fast arm; a non-zero result pins the
/// step at [`STEP_MAX`].
pub const FAST_ARM_MASK: u32 = 0xF000;

/// Step used while the camera is settled (both its angle and its Z already
/// match their targets).
pub const STEP_SETTLED: i16 = 1;

/// Largest step the adaptive arm will take.
pub const STEP_MAX: i16 = 0xC;

/// Shift applied to the gap when deriving the adaptive step (`gap / 16`).
const STEP_SHIFT: u32 = 4;

/// The camera state the easing reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameraEaseInput {
    /// Scratchpad pad word `_DAT_1F800394`.
    pub pad: u32,
    /// Camera-zone record `+0x4A` - the angle the zone wants.
    pub zone_angle: u16,
    /// Player actor `+0x16` - current facing.
    pub player_facing: u16,
    /// Player actor `+0x1E` - the facing's settle target.
    pub facing_target: i16,
    /// Player actor `+0x18` / `+0x20` - Z and its settle target.
    pub z: i16,
    pub z_target: i16,
    /// `_DAT_8007B850`, consulted only on the fast arm.
    pub fast_flags: u32,
    /// `_DAT_8007BCAC` - the smoothed yaw being eased.
    pub current: i32,
}

/// Choose this frame's step size.
///
/// PORT: FUN_801DA390 (`0x801da3c4..0x801da480`).
///
/// Three arms, in the order retail tests them:
///
/// 1. **Fast** - [`PAD_FAST_ARM`] set *and* `fast_flags & 0xF000` non-zero:
///    step [`STEP_MAX`] outright.
/// 2. **Settled** - the player's facing equals its target *and* its Z equals
///    its target: step [`STEP_SETTLED`].
/// 3. **Adaptive** - otherwise derived from the gap, capped at [`STEP_MAX`].
///
/// The adaptive arm is `v = |current - gap| >> 4`, then **`v + 1` when
/// `v + 1 < 2`, else `v`**. Read the branch carefully: `bne v0,zero,0x801da46c`
/// jumps *past* `move a0,v1`, so the `v + 1` value is what survives the taken
/// branch and `v` is the fall-through. The effect is that a gap too small to
/// survive the shift (`v == 0`) still steps `1` - which is what lets the easing
/// converge at all - while any `v >= 1` steps by `v` itself.
///
/// Getting that inversion backwards yields a step of `0` for every gap under
/// 16 and the easing silently never arrives; the convergence test in this
/// module is what pins it.
///
/// NOT WIRED: reached only from [`ease_camera_yaw`], which nothing calls - same
/// missing retail yaw channel as the module note. Exposed separately because
/// the step rule is the part a fidelity-mode camera would want to reuse even if
/// it drove its own accumulator.
pub fn ease_step(input: CameraEaseInput, gap: i16) -> i16 {
    if input.pad & PAD_FAST_ARM != 0 && input.fast_flags & FAST_ARM_MASK != 0 {
        return STEP_MAX;
    }
    let settled = i32::from(input.player_facing as i16) == i32::from(input.facing_target)
        && input.z == input.z_target;
    if settled {
        return STEP_SETTLED;
    }
    let spread = if input.current < i32::from(gap) {
        i32::from(gap) - input.current
    } else {
        input.current - i32::from(gap)
    };
    let v = (spread as i16) >> STEP_SHIFT;
    let stepped = v.wrapping_add(1);
    // `bne` skips the `move a0,v1`, so `v + 1` survives the taken branch.
    let step = if stepped < 2 { stepped } else { v };
    if step < STEP_MAX { step } else { STEP_MAX }
}

/// Advance the smoothed camera yaw by one frame.
///
/// PORT: FUN_801DA390 (`0x801da390..0x801da518`).
///
/// Returns the new value of `_DAT_8007BCAC`. While [`PAD_INPUT_LOCKED`] is set
/// the value is returned unchanged - the routine returns before touching it.
///
/// The target is `zone_angle - player_facing`, both read as `u16` and
/// subtracted as such, so the gap wraps like the 12-bit angle space it comes
/// from. The move is then clamped to [`ease_step`]'s magnitude in whichever
/// direction closes the gap, and a gap of exactly zero leaves the value alone.
///
/// NOT WIRED: the engine's field camera ([`crate::camera`]) eases in its own
/// float-based controller against a typed camera-zone record, and has no
/// `_DAT_8007BCAC` equivalent to step - the retail global is a fixed-point yaw
/// the renderer reads directly. Wiring this needs the camera controller to
/// carry the retail smoothed-yaw channel alongside its own, which is a
/// fidelity-mode decision rather than a missing plumbing detail.
pub fn ease_camera_yaw(input: CameraEaseInput) -> i32 {
    if input.pad & PAD_INPUT_LOCKED != 0 {
        return input.current;
    }
    let gap = input.zone_angle.wrapping_sub(input.player_facing) as i16;
    let step = ease_step(input, gap);
    let cur = input.current;
    let g = i32::from(gap);
    if g < cur {
        // Overshoot: step down, but never past the target.
        let room = (cur - g) as i16;
        let mv = if step < room { step } else { room };
        cur - i32::from(mv)
    } else if cur < g {
        let room = (g - cur) as i16;
        let mv = if step < room { step } else { room };
        cur + i32::from(mv)
    } else {
        cur
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> CameraEaseInput {
        CameraEaseInput {
            pad: 0,
            zone_angle: 0,
            player_facing: 0,
            facing_target: 0,
            z: 0,
            z_target: 0,
            fast_flags: 0,
            current: 0,
        }
    }

    #[test]
    fn the_input_lock_freezes_the_value() {
        let mut i = input();
        i.pad = PAD_INPUT_LOCKED;
        i.zone_angle = 0x400;
        i.current = 0x3C;
        assert_eq!(ease_camera_yaw(i), 0x3C);
    }

    #[test]
    fn a_settled_camera_creeps_one_unit_a_frame() {
        let mut i = input();
        // facing == target and z == z_target -> settled.
        i.zone_angle = 500;
        i.current = 0;
        assert_eq!(ease_step(i, 500), STEP_SETTLED);
        assert_eq!(ease_camera_yaw(i), 1);
    }

    #[test]
    fn an_unsettled_camera_takes_a_gap_proportional_step_capped_at_twelve() {
        let mut i = input();
        i.facing_target = 5; // != player_facing 0 -> not settled
        i.zone_angle = 800;
        i.current = 0;
        // gap 800, |800 - 0| >> 4 = 50, +1 = 51, capped at 12.
        assert_eq!(ease_step(i, 800), STEP_MAX);
        assert_eq!(ease_camera_yaw(i), i32::from(STEP_MAX));
    }

    #[test]
    fn the_fast_arm_pins_the_step_regardless_of_gap() {
        let mut i = input();
        i.pad = PAD_FAST_ARM;
        i.fast_flags = 0x1000;
        assert_eq!(ease_step(i, 1), STEP_MAX);
        // Without the flags word the fast arm does not engage.
        i.fast_flags = 0x0800;
        assert_eq!(ease_step(i, 1), STEP_SETTLED);
    }

    #[test]
    fn the_step_never_overshoots_the_target() {
        // The clamp only bites where the step can exceed the remaining room,
        // and in the adaptive arm it never can (step is the gap shifted right,
        // so it is always <= the gap). The fast arm is where it matters: the
        // step is pinned at 12 whatever the gap.
        let mut i = input();
        i.pad = PAD_FAST_ARM;
        i.fast_flags = 0x1000;
        i.zone_angle = 3;
        i.current = 0;
        assert_eq!(ease_step(i, 3), STEP_MAX);
        assert_eq!(ease_camera_yaw(i), 3, "lands on the target, not past it");
        // Same from above.
        i.zone_angle = 0;
        i.current = 3;
        assert_eq!(ease_camera_yaw(i), 0);
    }

    #[test]
    fn a_small_gap_creeps_one_unit_at_a_time() {
        // `v = gap >> 4` is 0 for any gap under 16, and the `v + 1` arm turns
        // that into a step of 1 rather than 0. A step of 0 would stall the
        // easing forever, which is what an inverted branch here produces.
        let mut i = input();
        i.facing_target = 5; // not settled, so this is the adaptive arm
        i.zone_angle = 3;
        i.current = 0;
        assert_eq!(ease_step(i, 3), 1);
        assert_eq!(ease_camera_yaw(i), 1);
        for gap in 1..16i16 {
            i.zone_angle = gap as u16;
            assert_eq!(ease_step(i, gap), 1, "gap {gap} must still move");
        }
    }

    #[test]
    fn easing_converges_from_either_side_and_then_holds() {
        for start in [-500i32, 500] {
            let mut i = input();
            i.facing_target = 5;
            i.zone_angle = 100;
            i.current = start;
            for _ in 0..400 {
                i.current = ease_camera_yaw(i);
            }
            assert_eq!(i.current, 100, "from {start}");
            // Once there it stops moving.
            assert_eq!(ease_camera_yaw(i), 100);
        }
    }
}

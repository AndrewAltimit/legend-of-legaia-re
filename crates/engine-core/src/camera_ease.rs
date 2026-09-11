//! Field **camera vertical-offset easing**: the per-frame step that walks the
//! scene control block's smoothed camera offset toward the value the current
//! scene asks for.
//!
//! PORT: FUN_801DA390
//!
//! One call per frame. It owns a single global (`_DAT_8007BCAC`, seeded to
//! `0x3C` by the field initialiser) and moves it toward
//! `scene_ctrl[+0x4A] - player[+0x16]`, where `scene_ctrl` is the MAN scene
//! control block `_DAT_801C6EA4` and `player` the player actor `_DAT_8007C364`.
//! The interesting part is the **step size**, which is not constant: while the
//! player is stationary it moves one unit a frame, and while the player is
//! still gliding it moves by a gap-proportional amount capped at twelve. That
//! is what makes a scene transition swing the offset round quickly and then
//! creep the last few units.
//!
//! ### What this channel is **not**
//!
//! It is not a camera **yaw**. The subtrahend is the player actor's `+0x16`,
//! and that slot is the actor's **footing** - the height of the floor it is
//! standing on - not a facing: `FUN_801D1BA0` glides `+0x16` toward the floor
//! sample at a clamped rate before the ledge classifier reads it back
//! (`0x801D1C30..0x801D1C68`), the heading lives at `+0x26`, and two wall-press
//! captures each read `player + 0x16 == -192` on `town0c`'s `-192` floor. The
//! move-VM actor struct maps the same slot as `world_y`. So `_DAT_8007BCAC` is
//! a smoothed **vertical offset** in world units, and both the accumulator and
//! `scene_ctrl[+0x4A]` are denominated in `+0x16`'s units. An earlier reading
//! of this module named `+0x16` "player facing" and `+0x1E` "the facing's
//! settle target", which made the whole channel a yaw; the arithmetic is
//! unchanged by the correction but the units and the consumer are not.
//! See [`field-locomotion.md`](../../../docs/subsystems/field-locomotion.md).
//!
//! The settle test compares `+0x16`/`+0x18` against the parallel slots eight
//! bytes on (`+0x1E`/`+0x20`) - Y and Z only, never X - so what it asks is
//! whether the actor's height and depth have both stopped moving. The engine
//! has no `+0x1E`/`+0x20` pair; [`crate::world::World`] answers the same
//! question from the previous tick's Y and Z, which reproduces the outcome the
//! comparison encodes without asserting what retail keeps in those two slots.
//!
//! Provenance: `overlay_cutscene_dialogue_801da390.txt`, cross-checked against
//! `overlay_cutscene_mapview_801da390.txt` and the standalone `801da390.txt`
//! (all 99 instructions, identical).
//!
//! WIRED: the field VM's op `0x4C` outer-nibble-4 sub-9 is the retail writer of
//! both globals - it sets or ramps `scene_ctrl[+0x4A]`, and on its delta arm
//! writes `_DAT_8007BCAC` in the same breath. `World` now implements all three
//! of that opcode's host hooks
//! (`FieldHost::op4c_n4_sub9_default_write` / `_default_ramp` /
//! `_delta_write_or_ramp`) onto [`crate::world::World::camera_scene_offset`] /
//! [`crate::world::World::camera_offset_ease`], and `World::tick` steps the
//! accumulator once a frame through [`ease_camera_offset`]. The accumulator is
//! observable state, not yet a camera input: [`crate::camera`] still drives the
//! rendered camera from its own float controller, and which of the two owns the
//! view is a fidelity-mode decision, not a wiring one - the two disagree frame
//! by frame and swapping silently changes camera feel.
//!
//! REF: FUN_801D6704 (seeds the global to `0x3C`), FUN_801DBA20 (the zone query
//! that supplies the per-region camera preset), FUN_801D1BA0 (the `+0x16`
//! footing glide)

/// Value the per-scene field initialiser seeds `_DAT_8007BCAC` to
/// (`FUN_801D6704`, `0x801D67B8`: `li v0,0x3c` / `sw v0,-0x4354(v1)`).
pub const CAMERA_OFFSET_EASE_SEED: i32 = 0x3C;

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
    /// Scene control block `+0x4A` (`_DAT_801C6EA4 + 0x4A`) - the offset
    /// the scene wants, in `+0x16` units.
    pub scene_target: u16,
    /// Player actor `+0x16` - current footing height.
    pub player_footing: u16,
    /// Player actor `+0x1E` - the slot the settle test compares `+0x16`
    /// against. Hosts without that slot pass the previous tick's `+0x16`.
    pub footing_settled: i16,
    /// Player actor `+0x18` / `+0x20` - Z and the slot the settle test
    /// compares it against.
    pub z: i16,
    pub z_target: i16,
    /// `_DAT_8007B850`, consulted only on the fast arm.
    pub fast_flags: u32,
    /// `_DAT_8007BCAC` - the smoothed offset being eased.
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
/// 2. **Settled** - the player's `+0x16` footing equals the slot eight bytes
///    on *and* its Z equals its own: step [`STEP_SETTLED`]. Read plainly:
///    the actor has stopped moving in Y and Z.
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
/// WIRED through [`ease_camera_offset`], which `World::tick` runs once a frame.
/// Exposed separately because the step rule is the part a fidelity-mode camera
/// would want to reuse even if it drove its own accumulator.
pub fn ease_step(input: CameraEaseInput, gap: i16) -> i16 {
    if input.pad & PAD_FAST_ARM != 0 && input.fast_flags & FAST_ARM_MASK != 0 {
        return STEP_MAX;
    }
    let settled = i32::from(input.player_footing as i16) == i32::from(input.footing_settled)
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

/// Advance the smoothed camera vertical offset by one frame.
///
/// PORT: FUN_801DA390 (`0x801da390..0x801da518`).
///
/// Returns the new value of `_DAT_8007BCAC`. While [`PAD_INPUT_LOCKED`] is set
/// the value is returned unchanged - the routine returns before touching it.
///
/// The target is `scene_target - player_footing`, both read as `u16` and
/// subtracted as such, then sign-extended, so the gap wraps in 16 bits. The
/// move is then clamped to [`ease_step`]'s magnitude in whichever direction
/// closes the gap, and a gap of exactly zero leaves the value alone.
///
/// WIRED: `World::tick_camera_offset_ease` calls this once a frame, over the
/// two globals the field VM's op `0x4C` n4 sub-9 hooks now write
/// ([`crate::world::World::camera_scene_offset`] and
/// [`crate::world::World::camera_offset_ease`]). What the accumulator does
/// *not* yet do is drive the rendered camera - [`crate::camera`] keeps its own
/// float controller, and choosing between them is a fidelity-mode decision.
pub fn ease_camera_offset(input: CameraEaseInput) -> i32 {
    if input.pad & PAD_INPUT_LOCKED != 0 {
        return input.current;
    }
    let gap = input.scene_target.wrapping_sub(input.player_footing) as i16;
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
            scene_target: 0,
            player_footing: 0,
            footing_settled: 0,
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
        i.scene_target = 0x400;
        i.current = 0x3C;
        assert_eq!(ease_camera_offset(i), 0x3C);
    }

    #[test]
    fn a_settled_camera_creeps_one_unit_a_frame() {
        let mut i = input();
        // facing == target and z == z_target -> settled.
        i.scene_target = 500;
        i.current = 0;
        assert_eq!(ease_step(i, 500), STEP_SETTLED);
        assert_eq!(ease_camera_offset(i), 1);
    }

    #[test]
    fn an_unsettled_camera_takes_a_gap_proportional_step_capped_at_twelve() {
        let mut i = input();
        i.footing_settled = 5; // != player_footing 0 -> not settled
        i.scene_target = 800;
        i.current = 0;
        // gap 800, |800 - 0| >> 4 = 50, +1 = 51, capped at 12.
        assert_eq!(ease_step(i, 800), STEP_MAX);
        assert_eq!(ease_camera_offset(i), i32::from(STEP_MAX));
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
        i.scene_target = 3;
        i.current = 0;
        assert_eq!(ease_step(i, 3), STEP_MAX);
        assert_eq!(ease_camera_offset(i), 3, "lands on the target, not past it");
        // Same from above.
        i.scene_target = 0;
        i.current = 3;
        assert_eq!(ease_camera_offset(i), 0);
    }

    #[test]
    fn a_small_gap_creeps_one_unit_at_a_time() {
        // `v = gap >> 4` is 0 for any gap under 16, and the `v + 1` arm turns
        // that into a step of 1 rather than 0. A step of 0 would stall the
        // easing forever, which is what an inverted branch here produces.
        let mut i = input();
        i.footing_settled = 5; // not settled, so this is the adaptive arm
        i.scene_target = 3;
        i.current = 0;
        assert_eq!(ease_step(i, 3), 1);
        assert_eq!(ease_camera_offset(i), 1);
        for gap in 1..16i16 {
            i.scene_target = gap as u16;
            assert_eq!(ease_step(i, gap), 1, "gap {gap} must still move");
        }
    }

    #[test]
    fn easing_converges_from_either_side_and_then_holds() {
        for start in [-500i32, 500] {
            let mut i = input();
            i.footing_settled = 5;
            i.scene_target = 100;
            i.current = start;
            for _ in 0..400 {
                i.current = ease_camera_offset(i);
            }
            assert_eq!(i.current, 100, "from {start}");
            // Once there it stops moving.
            assert_eq!(ease_camera_offset(i), 100);
        }
    }
}

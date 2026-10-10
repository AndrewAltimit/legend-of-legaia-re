//! A part's translation update, and the camera-locked bit that reroutes it.

use super::*;

/// `+0x52` bit the render dispatcher's camera-relative routine tests first
/// (`FUN_8001CF50`, `0x8001CF7C`): the node is drawn at the eye-space offset
/// `S_b * (+0x14)`, locked to the camera.
pub const CAMERA_LOCKED_BIT: u16 = 0x0400;

/// Whether a part is camera-locked (`+0x52 & 0x400`, written by move-VM op
/// `0x15`).
///
/// Its position is therefore an eye-space offset, and it moves the way
/// retail moves every part - the motion block `crate::part_motion` ports -
/// rather than through [`apply_translation_update`]. The three retail
/// captures that carry such nodes pin the difference: Horn (PROT 0930) at
/// `(0, 0, 2048)`, Cort's Mystic Circle (0938) at `(0, -192, 1536)` and his
/// Ultra Charge (0962) at `(0, 0, 256)`, each the value the part's own
/// op `0x07` WORLD_SET writes; the glide put every one of them at
/// `origin + anim bank` instead.
pub fn is_camera_locked(state: &ActorState) -> bool {
    state.field_52 & CAMERA_LOCKED_BIT != 0
}

/// Render-side translation glide for the stand-in scene-graph: each axis
/// tweens toward `origin + anim bank` over `+0x9C / +0x9E`, snapping when no
/// tween is active (`+0x9E == 0`) and latching exactly on completion.
///
/// Not run for a camera-locked part ([`is_camera_locked`]). Retail's part
/// tick has no such glide at all: its motion block integrates `+0x3C..+0x40`
/// as velocities (`0x800229DC..0x80022AE0`); the glide remains the stand-in
/// for the world-space parts only.
///
/// Provenance note: this glide **reuses the tween shape of `FUN_801F811C`**
/// (advance-clamp-lerp-latch over the `+0x9C/+0x9E` clock with the
/// `FUN_801DE4C8` mode-1 lerp + `FUN_801DE648` sized store), but the retail
/// function is **not** a summon-part position update - it is the screen-mask
/// (iris) widget handler of the PROT-0900 screen-effect family, whose four
/// channels are 2D rect edges and whose quads are the black border bands. The
/// faithful port lives in [`crate::screen_fx::MaskWidget`]. As an engine
/// interpretation this glide also diverges from the retail tween math in one
/// respect: it re-lerps from the **updated** current position each frame
/// (a slightly eased trajectory), where the retail widget re-interpolates
/// from a start value held fixed until the latch.
///
/// The anim banks are summon-local offsets, so the engine adds `origin` (the
/// cast target) to the lerp endpoint to seat the part in world space.
/// `frame_delta` plays the role of retail's per-frame `DAT_1F800393`.
// REF: FUN_801F811C (faithful port: screen_fx::MaskWidget)
// REF: FUN_801DE648 (sized store of the lerp result; here a plain field write)
pub(super) fn apply_translation_update(state: &mut ActorState, origin: [i16; 3], frame_delta: u16) {
    // A draw-kind-4 node (`+0x56 == 4`, the multi-target emitters move-VM ops
    // `0x13` / `0x23` / `0x42` set up) carries the emitter's arguments in
    // `+0x9C` / `+0x9E` - the packed step count and the emitter-select flags
    // `FUN_8001ADA4` reads at `0x8001B0B8..0x8001B124` - not a glide clock.
    // Running the glide over them advanced the count every frame and cleared
    // the `0x2000` ribbon flag at the "latch". Seat the node at its target and
    // leave the words alone.
    if state.move_substate == 4 {
        state.world_x = origin[0].wrapping_add(state.anim_3c);
        state.world_y = origin[1].wrapping_add(state.anim_3e);
        state.world_z = origin[2].wrapping_add(state.anim_40);
        state.world_y_mirror = state.world_y;
        return;
    }
    // Retail reads `*(i16)(actor+0x9C)` (low half of the i32 field_9c) and
    // `*(i16)(actor+0x9E)` (field_9e). Targets are summon-local anim banks +
    // origin (the cast target).
    let target = [
        origin[0].wrapping_add(state.anim_3c),
        origin[1].wrapping_add(state.anim_3e),
        origin[2].wrapping_add(state.anim_40),
    ];

    let duration = state.field_9e as i16;
    if duration == 0 {
        // No active tween → snap to target (the `+0x9E == 0` arm).
        state.world_x = target[0];
        state.world_y = target[1];
        state.world_z = target[2];
        state.world_y_mirror = state.world_y;
        return;
    }

    // Advance the keyframe cursor by the per-frame delta and clamp to duration.
    let mut t = (state.field_9c & 0xFFFF) as i16;
    t = t.wrapping_add(frame_delta as i16);
    if duration < t {
        t = duration;
    }
    state.field_9c = (state.field_9c & !0xFFFF) | (t as u16 as i32);

    // Per-axis LERP toward the (origin + anim-bank) target. Skips an axis whose
    // current value already equals the target (the `target != cur` guard in
    // retail), matching `FUN_801DE4C8`'s early `a == b` return.
    let t_i = t as i32;
    let d_i = duration as i32;
    let cur = [state.world_x, state.world_y, state.world_z];
    for (i, &c) in cur.iter().enumerate() {
        if target[i] != c {
            let v = lerp_axis(target[i] as i32, c as i32, t_i, d_i) as i16;
            match i {
                0 => state.world_x = v,
                1 => state.world_y = v,
                _ => state.world_z = v,
            }
        }
    }
    state.world_y_mirror = state.world_y;

    // Reached the endpoint → latch exactly to the target and clear duration
    // (`+0x9E == 0` means "no active tween" on the next frame).
    if t == duration {
        state.world_x = target[0];
        state.world_y = target[1];
        state.world_z = target[2];
        state.world_y_mirror = state.world_y;
        state.field_9e = 0;
    }
}

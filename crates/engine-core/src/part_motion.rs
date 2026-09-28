//! The part tick's **motion block** - how a move-VM part's position, rotation
//! banks, render scale and depth-cue level move between VM steps.
//!
//! `FUN_80021DF4` (the per-frame tick of every move-VM part) runs, ahead of
//! its `FUN_80023070` call, a block every render mode but `3` (CLUT-cell
//! particle) and `5` (sound emitter) takes (`0x800228A0..0x80022B90`): each
//! rate halfword is scaled by the two scratchpad speed bytes
//! (`DAT_1F800393 * DAT_1F80037D`, the same product the wait timer drains by)
//! and added `>> 6`. The position's X / Z also carry a heading term: the
//! velocity `+0x3C` / `+0x40` is shifted up twelve bits and summed with
//! `sin/cos(+0x96) * +0x98` from the two rotation LUTs before the product is
//! taken `>> 18`, so `+0x98` is a speed along heading `+0x96` (which itself
//! turns at `+0x9A`). After the VM call the tick clamps the depth-cue level
//! `+0x78` and the render scale `+0x72` (`0x80022BC0..0x80022C1C`).
//!
//! The move VM only **sets** these rates (op `0x00` velocity, op `0x04`
//! rotation rates, ops `0x2B` / `0x2D` / `0x35` / `0x37` the scale and level
//! rates, `0x29` / `0x2E` the heading); without this block a part whose record
//! drives itself through them never moves. That is what kept the overworld
//! `map01` puff column still: its travelling puffs set `+0x40 = -4 << 3` and
//! nothing else.
//!
//! PORT: FUN_80021DF4 (`0x800228A0..0x80022B90` the motion block,
//! `0x80022BC0..0x80022C1C` the post-VM clamps)

use legaia_engine_vm::move_vm::ActorState;

use crate::action_effect_script::{RotationLut, retail_rotation_lut};

/// `(rate * delta) >> 6` - the per-channel step, in the 32-bit product retail
/// takes (`mult` twice, `mflo`, `sra 6`).
fn step(rate: i16, delta: u16) -> i16 {
    (i32::from(rate).wrapping_mul(i32::from(delta)) >> 6) as i16
}

/// Whether the tick runs the motion block for this render mode (`+0x5A`):
/// every mode but `3` and `5` (`0x800228A4..0x800228B0`).
pub fn runs_motion_block(st: &ActorState) -> bool {
    st.move_submode != 3 && st.move_submode != 5
}

/// The motion block, `delta = DAT_1F800393 * DAT_1F80037D`.
///
/// `0x800228B8..0x80022B90`: `+0x96 += +0x9A`; the rotation banks
/// `+0x24/+0x26/+0x28 += +0x80/+0x82/+0x84`;
/// `+0x14 += ((sin(+0x96) * +0x98 + (+0x3C << 12)) * delta) >> 18` with the
/// `_DAT_8007B81C` table, `+0x16` and its mirror `+0x2A += +0x3E`,
/// `+0x18 += ((cos(+0x96) * +0x98 + (+0x40 << 12)) * delta) >> 18` with the
/// `_DAT_8007B7F8` table; `+0x72 += +0x92`, `+0x7A += +0x94`,
/// `+0x78 += +0x90`; a negative `+0x7A` is zeroed. Not included: the
/// `+0x86 & 0x2000` call into the field overlay's visibility cull
/// `FUN_801D79E8` (`0x80022900..0x80022914`), which only clears the part's
/// flag bit `2`.
pub fn motion_block(st: &mut ActorState, delta: u16) {
    let d = i32::from(delta);
    st.tween_scale_x = st.tween_scale_x.wrapping_add(step(st.tween_scale_z, delta));
    st.render_24 = st.render_24.wrapping_add(step(st.anim_80, delta));
    st.render_26 = st.render_26.wrapping_add(step(st.anim_82, delta));
    st.render_28 = st.render_28.wrapping_add(step(st.anim_84, delta));
    let lut = retail_rotation_lut();
    let heading = i32::from(st.tween_scale_x) & 0xFFF;
    let speed = i32::from(st.tween_scale_y);
    let along = |trig: i32, vel: i16| -> i16 {
        let v = trig
            .wrapping_mul(speed)
            .wrapping_add(i32::from(vel) << 12)
            .wrapping_mul(d);
        (v >> 18) as i16
    };
    st.world_x = st.world_x.wrapping_add(along(lut.b(heading), st.anim_3c));
    let dy = step(st.anim_3e, delta);
    st.world_y = st.world_y.wrapping_add(dy);
    st.world_y_mirror = st.world_y_mirror.wrapping_add(dy);
    st.world_z = st.world_z.wrapping_add(along(lut.a(heading), st.anim_40));
    st.field_72 = st.field_72.wrapping_add(step(st.tween_src_y, delta) as u16);
    st.field_7a = st.field_7a.wrapping_add(step(st.tween_src_z, delta) as u16);
    st.field_78 = st.field_78.wrapping_add(step(st.tween_src_x, delta) as u16);
    if (st.field_7a as i16) < 0 {
        st.field_7a = 0;
    }
}

/// The post-VM clamps (`0x80022BC0..0x80022C1C`): a depth-cue level past
/// `0x3E80` wraps to `0`, otherwise it saturates at `0x1000`; a render scale
/// past `0x3E80` wraps to `0`, otherwise it saturates at `0x3A98`.
pub fn clamp_levels(st: &mut ActorState) {
    if st.field_78 > 0x3E80 {
        st.field_78 = 0;
    }
    if st.field_78 > 0x1000 {
        st.field_78 = 0x1000;
    }
    if st.field_72 > 0x3E80 {
        st.field_72 = 0;
    }
    if st.field_72 > 0x3A98 {
        st.field_72 = 0x3A98;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Op `0x00`'s velocity moves the part by `(v << 12) * delta >> 18` on
    /// X / Z and `v * delta >> 6` on Y - the same rate both ways.
    #[test]
    fn velocity_moves_the_part() {
        let mut st = ActorState {
            anim_3c: 8,
            anim_3e: -16,
            anim_40: -32,
            ..Default::default()
        };
        motion_block(&mut st, 0x20);
        assert_eq!((st.world_x, st.world_y, st.world_z), (4, -8, -16));
        assert_eq!(st.world_y_mirror, -8);
    }

    /// `+0x98` is a speed along heading `+0x96`.
    #[test]
    fn heading_speed_moves_along_the_heading() {
        let mut st = ActorState {
            tween_scale_x: 0,
            tween_scale_y: 64,
            ..Default::default()
        };
        motion_block(&mut st, 0x40);
        // Heading 0: the `_DAT_8007B81C` table (sine) is 0, the
        // `_DAT_8007B7F8` one (cosine) is 0x1000 -> all of it on Z.
        assert_eq!(st.world_x, 0);
        assert_eq!(st.world_z, 64);
    }

    /// The rates step the rotation banks, scale and level; the clamps hold.
    #[test]
    fn rates_and_clamps() {
        let mut st = ActorState {
            anim_82: 64,
            tween_src_x: 0x40,
            tween_src_y: -0x40,
            field_72: 0x1000,
            field_78: 0xFF0,
            ..Default::default()
        };
        motion_block(&mut st, 0x40);
        assert_eq!(st.render_26, 64);
        assert_eq!(st.field_78, 0x1030);
        assert_eq!(st.field_72, 0x0FC0);
        clamp_levels(&mut st);
        assert_eq!(st.field_78, 0x1000);
        st.field_72 = 0x3F00;
        clamp_levels(&mut st);
        assert_eq!(st.field_72, 0);
    }
}

//! The positional sound-effect emitter: the render-mode-5 arm of the part
//! tick `FUN_80021DF4` (`0x800220D8..0x800226D4`).
//!
//! A part whose `+0x5A` is `5` is a looping sound placed in the world. Move-VM
//! op `0x34` seats it: `+0xAC` the cue id (below `0x200` the static descriptor
//! table, at or above it the scene's runtime bank), `+0xB0` the first SPU
//! voice, `+0x90` / `+0x92` the left / right level (`0..=0x7F`), and the four
//! words `+0x9C` / `+0xA0` / `+0xA4` / `+0xA8` the box it is heard in, as
//! offsets from the part on each side. Every tick the arm
//!
//! 1. ramps the two levels (`+0x98` / `+0x9A` per tick, or toward `+0x94` /
//!    `+0x96` over `+0xC0` ticks while `+0xB8` is set), clamped `0..=0x7F`;
//! 2. tests the camera focus (`_DAT_80089118` / `_DAT_80089120`, stored
//!    negated) against the box, and scales each level by how deep inside it
//!    the focus sits on each axis - `(|edge| - |d|) * 128 / |edge|`, the
//!    nearer edge chosen by the side the focus is on - with a zero `+0x9C`
//!    meaning "everywhere, at full level";
//! 3. turns the level into a raw voice volume
//!    `((voice_volume >> 1) * level * falloff >> 7) * scale >> 7`, clamped
//!    `0x3F80`, `scale` the per-row factor at `0x80091658` (`0x80` in every
//!    captured state), the two sides averaged under libsnd mono;
//! 4. keys the descriptor's voices once (`FUN_80065034` with the volume pair
//!    `>> 7`) and from then on writes the raw volume straight to the voices
//!    every tick (`FUN_800657D0`), or releases them (`FUN_800250D4`) when the
//!    focus leaves the box.
//!
//! The arm only keys in the field (`_DAT_8007B83C == 3`) and only while the
//! side-band request is settled (`_DAT_8007BABC == _DAT_8007BAA0`).
//!
//! Pinned against the library's town states: with the focus and the part as
//! captured, the law reproduces the sounding emitter voices' registers.
//!
//! PORT: FUN_80021DF4 (the render-mode-5 sound arm)

use crate::move_vm::ActorState;

/// The render mode (`+0x5A`) of an emitter part.
pub const EMITTER_RENDER_MODE: i16 = 5;

/// `+0x10` bit a stop request raises; the arm clears it and drops its key-on
/// latch without releasing (`0x80022180..0x8002219C`).
pub const EMITTER_STOP_FLAG: u32 = 0x1_0000;

/// What the arm sees of the world this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmitterListener {
    /// The camera focus as retail stores it: `_DAT_80089118` (X, negated) and
    /// `_DAT_80089120` (Z, negated).
    pub focus_stored: (i32, i32),
    /// The voice / SFX volume word `_DAT_80084580` (cold reset `200`).
    pub voice_volume: i32,
    /// The options screen's Sound word reads Monaural (`_DAT_800846BC != 0`,
    /// `lw v0,0x57c(a1)` off `0x80084140` at `0x800224D4`): the arm writes
    /// both sides at their average, `(l + r) >> 1`. Not libsnd's fold flag
    /// `_DAT_801CE330` - the direct voice write never reads that one, so a
    /// mono emitter sounds at the mean of its two levels, not the larger.
    pub mono: bool,
    /// The per-row scale pair at `0x80091658 + row * 0x10`, clamped
    /// `0..=0x100` by the arm. `(0x80, 0x80)` in every captured state.
    pub scale: (i32, i32),
    /// `_DAT_8007B83C == 3`: the field mode, the only one the arm keys in.
    pub field_mode: bool,
    /// `_DAT_8007BABC == _DAT_8007BAA0`: the side-band bank is settled.
    pub stream_settled: bool,
    /// `_DAT_8007B9EC != 0`: release every emitter and halt it.
    pub release_all: bool,
    /// The frame step `DAT_1F800393`.
    pub frame_step: i32,
    /// The speed scalar `DAT_1F80037D` (`8`, planted by SCUS at boot).
    pub speed: i32,
}

/// What the arm asks of the sound driver this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmitterOp {
    /// Key the cue's voices from `voice` on with the volume pair
    /// (`0..=127` each, the raw volume `>> 7`).
    Key {
        cue: i32,
        voice: i16,
        vol_l: i16,
        vol_r: i16,
    },
    /// Write the raw voice volume (`0..=0x3F80`) to the cue's voices.
    Volume {
        cue: i32,
        voice: i16,
        left: u16,
        right: u16,
    },
    /// Key the cue's voices off (`FUN_800250D4(cue, voice)`).
    Release { cue: i32, voice: i16 },
}

/// The raw voice volume pair an emitter at `pos` (`+0x14`, `+0x18`) with
/// `levels` (`+0x90`, `+0x92`) and box `range` (`+0x9C`, `+0xA0`, `+0xA4`,
/// `+0xA8`) plays at, or `None` when the focus is outside the box.
pub fn emitter_volume(
    pos: (i32, i32),
    levels: (i32, i32),
    range: [i32; 4],
    listener: &EmitterListener,
) -> Option<(u16, u16)> {
    let (x, z) = pos;
    let (fx, fz) = listener.focus_stored;
    let [r9c, ra0, ra4, ra8] = range;
    // The box test (`0x800221A0..0x800221F8`): X strictly inside, Z outside
    // either edge leaves; a zero `+0x9C` is heard everywhere.
    let inside = (-fx + r9c) < x && x < (-fx + ra4) && !(z <= -fz - ra0 || -fz - ra8 <= z);
    if !inside && r9c != 0 {
        return None;
    }
    let falloff = if r9c == 0 {
        0x80
    } else {
        let dx = -x - fx;
        let dz = -z - fz;
        let w = if dx < 1 { ra4 } else { r9c }.abs();
        let h = if dz < 1 { ra8 } else { ra0 }.abs();
        let fw = if w == 0 {
            0
        } else {
            ((w - dx.abs()).abs() << 7) / w
        };
        let fh = if h == 0 {
            0
        } else {
            ((h - dz.abs()).abs() << 7) / h
        };
        let p = fw * fh;
        // `sra 7` with the round-toward-zero bias for a negative product.
        if p < 0 { (p + 0x7F) >> 7 } else { p >> 7 }
    };
    let half = listener.voice_volume >> 1;
    let side = |level: i32, scale: i32| -> i32 {
        let scale = scale.clamp(0, 0x100);
        ((((half * level * falloff) >> 7) * scale) >> 7).clamp(0, 0x3F80)
    };
    let mut l = side(levels.0, listener.scale.0);
    let mut r = side(levels.1, listener.scale.1);
    if listener.mono {
        let m = (l + r) >> 1;
        l = m;
        r = m;
    }
    Some((l as u16, r as u16))
}

/// One tick of the arm over a part. `None` for a part that is not an emitter
/// (`+0x5A != 5`) or one with nothing to do this tick.
pub fn emitter_tick(st: &mut ActorState, listener: &EmitterListener) -> Option<EmitterOp> {
    if st.move_submode != EMITTER_RENDER_MODE {
        return None;
    }
    ramp_levels(st, listener.frame_step, listener.speed);
    let cue = st.actor_u32(0xAC) as i32;
    let voice = st.actor_u16(0xB0) as i16;
    let latch = st.actor_u32(0xB4);
    if st.flags & EMITTER_STOP_FLAG != 0 {
        st.flags &= !EMITTER_STOP_FLAG;
        st.set_actor_u32(0xB4, 0);
        return None;
    }
    let range = [
        st.actor_u32(0x9C) as i32,
        st.actor_u32(0xA0) as i32,
        st.actor_u32(0xA4) as i32,
        st.actor_u32(0xA8) as i32,
    ];
    let pos = (i32::from(st.world_x), i32::from(st.world_z));
    let levels = (i32::from(st.tween_src_x), i32::from(st.tween_src_y));
    let release = |st: &mut ActorState| {
        if st.actor_u32(0xB4) != 0 {
            st.set_actor_u32(0xB4, 0);
            Some(EmitterOp::Release { cue, voice })
        } else {
            None
        }
    };
    let Some((left, right)) = emitter_volume(pos, levels, range, listener) else {
        return release(st);
    };
    if !listener.stream_settled {
        return release(st);
    }
    if listener.release_all {
        st.flags |= 0x8;
        return Some(EmitterOp::Release { cue, voice });
    }
    if !listener.field_mode {
        return None;
    }
    if latch == 0 {
        if voice < 0 {
            return None;
        }
        st.set_actor_u32(0xB4, 1);
        // `(v << 9) >> 16` at the call site: the raw volume `>> 7`.
        let vol = |v: u16| ((u32::from(v) << 9) >> 16) as i16;
        return Some(EmitterOp::Key {
            cue,
            voice,
            vol_l: vol(left),
            vol_r: vol(right),
        });
    }
    Some(EmitterOp::Volume {
        cue,
        voice,
        left,
        right,
    })
}

/// The level ramp (`0x80022260..0x80022344` region): free-running rates
/// `+0x98` / `+0x9A` while `+0xB8` is clear, otherwise a timed glide from
/// `+0x94` / `+0x96` by `+0x98` / `+0x9A` over `+0xC0` ticks counted down in
/// `+0xBC`; then the `0..=0x7F` clamp.
fn ramp_levels(st: &mut ActorState, frame_step: i32, speed: i32) {
    let step_product = frame_step * speed;
    let rate_l = i32::from(st.tween_scale_y);
    let rate_r = i32::from(st.tween_scale_z);
    if st.actor_u32(0xB8) == 0 {
        st.tween_src_x = st
            .tween_src_x
            .wrapping_add(((rate_l * step_product) >> 6) as i16);
        st.tween_src_y = st
            .tween_src_y
            .wrapping_add(((rate_r * step_product) >> 6) as i16);
    } else {
        let left = st.actor_u32(0xBC) as i32 - frame_step;
        if left < 0 {
            st.set_actor_u32(0xBC, 0);
            st.set_actor_u32(0xB8, 0);
            st.tween_scale_y = 0;
            st.tween_scale_z = 0;
            st.tween_src_x = st.tween_src_z.wrapping_add(rate_l as i16);
            st.tween_src_y = st.tween_scale_x.wrapping_add(rate_r as i16);
        } else {
            st.set_actor_u32(0xBC, left as u32);
            let dur = (st.actor_u32(0xC0) as i32).max(1);
            let t = ((dur - left) * 0x100) / dur;
            let lerp = |rate: i32| {
                let v = rate * t;
                (if v < 0 { v + 0xFF } else { v }) >> 8
            };
            st.tween_src_x = st.tween_src_z.wrapping_add(lerp(rate_l) as i16);
            st.tween_src_y = st.tween_scale_x.wrapping_add(lerp(rate_r) as i16);
        }
    }
    st.tween_src_x = st.tween_src_x.clamp(0, 0x7F);
    st.tween_src_y = st.tween_src_y.clamp(0, 0x7F);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listener(focus: (i32, i32)) -> EmitterListener {
        EmitterListener {
            focus_stored: focus,
            voice_volume: 200,
            mono: false,
            scale: (0x80, 0x80),
            field_mode: true,
            stream_settled: true,
            release_all: false,
            frame_step: 1,
            speed: 8,
        }
    }

    /// The `rimelm_wall_press_left` mednafen state's river emitter: part at
    /// `(3968, 1024)`, levels `56 / 56`, box `(-10000, 2500, 10000, -2000)`,
    /// focus stored `(-1838, -2526)`; its three voices sound at `1706`.
    #[test]
    fn the_captured_town_emitter_reproduces_its_voice_volume() {
        let v = emitter_volume(
            (3968, 1024),
            (56, 56),
            [-10000, 2500, 10000, -2000],
            &listener((-1838, -2526)),
        );
        assert_eq!(v, Some((1706, 1706)));
    }

    /// Monaural (`0x800224D4..0x800224E8`): both sides at `(l + r) >> 1`
    /// after the clamp, not at the larger side.
    #[test]
    fn mono_writes_both_sides_at_their_average() {
        let mono = EmitterListener {
            mono: true,
            ..listener((-5000, 0))
        };
        // Zero box (full falloff): 100 * 64 * 128 >> 7 = 6400 and
        // 100 * 16 * 128 >> 7 = 1600 in stereo.
        let range = [0, 0, 0, 0];
        assert_eq!(
            emitter_volume((0, 0), (64, 16), range, &listener((-5000, 0))),
            Some((6400, 1600))
        );
        assert_eq!(
            emitter_volume((0, 0), (64, 16), range, &mono),
            Some((4000, 4000))
        );
    }

    #[test]
    fn a_focus_outside_the_box_is_silent_and_a_zero_box_is_everywhere() {
        let range = [-1000, 500, 1000, -500];
        assert_eq!(
            emitter_volume((0, 0), (64, 64), range, &listener((-5000, 0))),
            None
        );
        // Full falloff (0x80) at a zero `+0x9C`: 100 * 64 * 128 >> 7 = 6400.
        assert_eq!(
            emitter_volume((0, 0), (64, 64), [0, 0, 0, 0], &listener((-5000, 0))),
            Some((6400, 6400))
        );
    }

    #[test]
    fn the_arm_keys_once_then_writes_volume_then_releases() {
        let mut st = ActorState {
            move_submode: EMITTER_RENDER_MODE,
            world_x: 3968,
            world_z: 1024,
            tween_src_x: 56,
            tween_src_y: 56,
            ..Default::default()
        };
        st.set_actor_u32(0xAC, 0x200);
        st.set_actor_u16(0xB0, 16);
        for (off, v) in [
            (0x9C, -10000i32),
            (0xA0, 2500),
            (0xA4, 10000),
            (0xA8, -2000),
        ] {
            st.set_actor_u32(off, v as u32);
        }
        let l = listener((-1838, -2526));
        // 1706 >> 7 = 13.
        assert_eq!(
            emitter_tick(&mut st, &l),
            Some(EmitterOp::Key {
                cue: 0x200,
                voice: 16,
                vol_l: 13,
                vol_r: 13
            })
        );
        assert_eq!(
            emitter_tick(&mut st, &l),
            Some(EmitterOp::Volume {
                cue: 0x200,
                voice: 16,
                left: 1706,
                right: 1706
            })
        );
        let far = listener((-30000, -2526));
        assert_eq!(
            emitter_tick(&mut st, &far),
            Some(EmitterOp::Release {
                cue: 0x200,
                voice: 16
            })
        );
        assert_eq!(emitter_tick(&mut st, &far), None);
    }
}

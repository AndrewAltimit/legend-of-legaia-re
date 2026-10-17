//! The shared pieces of the summon-creature directors that carry their
//! module's whole phase chain ([`ModuleProfile::director_body`]): the
//! countdown gate, a literal shot and a camera drift, in the units the engine
//! ticks in.
//!
//! The modules drain their countdown words by one of three products: the
//! frame delta alone (`*(0x1F800393)`, [`D`] a tick), the scalar times the
//! delta (`*(0x1F80037D) * *(0x1F800393)`, [`P`] a tick), or a multiple of
//! either. Every per-pass camera drift is in the same units.

use super::*;

/// One frame delta per engine tick ([`creature::DELTA_PER_TICK`]).
pub const D: i32 = DELTA_PER_TICK;

/// The scalar times the delta per engine tick ([`MODULE_DRAIN_PER_TICK`]).
pub const P: i32 = MODULE_DRAIN_PER_TICK;

/// The speed scalar `*(0x1F80037D)` itself, the unit of the re-arms.
pub const S: i32 = SPEED_SCALAR;

/// The countdown gate every counted arm opens with: `word -= drain`, and
/// `true` (hold) while the word is still positive (`bgtz` to the busy
/// return). On `false` the caller runs the exit and re-arms.
pub fn counted(st: &mut ModuleCamState, drain: i32) -> bool {
    st.countdown.0 -= drain;
    st.countdown.0 > 0
}

/// The gate shape that drains only a positive word (`if (cd > 0) { cd -=
/// drain; if (cd > 0) busy; }`): an arm entered on a spent word runs its
/// exit at once. Returns `(hold, drained)`.
pub fn counted_if_positive(st: &mut ModuleCamState, drain: i32) -> (bool, bool) {
    if st.countdown.0 > 0 {
        st.countdown.0 -= drain;
        (st.countdown.0 > 0, true)
    } else {
        (false, false)
    }
}

/// A `FUN_801D829C` call with every trio from immediates (or worked out by
/// the caller): `angles` is `(pitch, yaw)`, `focus` the raw stack trio (the
/// negated world point).
pub fn cut(angles: [i32; 2], tr: [i32; 3], focus: [i32; 3], frames: i32) -> ArmDirection {
    ArmDirection::shot(ModuleShot {
        angles: [angles[0] as i16, angles[1] as i16, 0],
        tr: [tr[0] as i16, tr[1] as i16, tr[2] as i16],
        focus: [focus[0] as i16, focus[1] as i16, focus[2] as i16],
        frames: frames as u16,
    })
}

/// The raw focus trio on a seat at floor height.
pub fn on(seat: ModuleSeat) -> [i32; 3] {
    [-i32::from(seat.x), 0, -i32::from(seat.z)]
}

/// Direct per-pass writes into the camera globals: pitch `0x8007B790`, yaw
/// `0x8007B792`, TR x / y / z `0x800840B8..0x800840C0`, as deltas.
pub fn drift(pitch: i32, yaw: i32, tr_x: i32, tr_y: i32, tr_z: i32) -> CaptureDrift {
    CaptureDrift {
        pitch: pitch as i16,
        yaw: yaw as i16,
        tr_y: tr_y as i16,
        tr_z: tr_z as i16,
        tr_x: tr_x as i16,
    }
}

/// A held pass carrying the arm's per-pass drift.
pub fn held(d: Option<CaptureDrift>) -> ArmDirection {
    ArmDirection {
        drift: d,
        ..ArmDirection::HOLD
    }
}

/// `dir` with the arm's per-pass drift folded in (the drift is written on
/// the passing pass as well).
pub fn with_drift(dir: ArmDirection, d: Option<CaptureDrift>) -> ArmDirection {
    ArmDirection { drift: d, ..dir }
}

/// A passing arm that leaves for the done phase `0xFF` - the arm that stores
/// it, or a terminal arm that returns `0` without storing a phase (the port
/// then stands at `0xFF` for it, so the band lets go).
pub fn finish(dir: ArmDirection) -> ArmDirection {
    ArmDirection {
        jump: Some(crate::seru_magic_ticks::SERU_B_DONE_PHASE),
        hold: false,
        ..dir
    }
}

/// A passing arm that stores a literal phase.
pub fn goto(dir: ArmDirection, phase: u8) -> ArmDirection {
    ArmDirection {
        jump: Some(phase),
        hold: false,
        ..dir
    }
}

/// `dir` with a caption.
pub fn caption(dir: ArmDirection, c: ModuleCaption) -> ArmDirection {
    ArmDirection {
        caption: Some(c),
        ..dir
    }
}

/// A clip gate on the creature's playing clip `+0x1D9`: holds while
/// `waiting(clip)`. With no clip fed the gate passes.
pub fn clip_wait(st: &ModuleCamState, waiting: impl Fn(u8) -> bool) -> bool {
    st.creature_clip.is_some_and(waiting)
}

/// The raw TR z a shot must carry for the camera to land on the global TR z
/// `global` (the inverse of the tween builder's prescale `<< 8 / 0xA0`),
/// for an arm that stores the global straight. Clamped to the halfword.
pub fn raw_tr_z(global: i32) -> i32 {
    ((global * 0xA0) >> 8).clamp(-0x8000, 0x7FFF)
}

/// The yaw drift that lands the yaw global on `yaw` from the live yaw (the
/// shortest way round), for an arm that stores the global straight. `0`
/// with no live yaw fed.
pub fn yaw_to(st: &ModuleCamState, yaw: i32) -> i32 {
    st.live_yaw.map_or(0, |live| {
        let d = (yaw - i32::from(live)) & 0xFFF;
        if d >= 0x800 { d - 0x1000 } else { d }
    })
}

/// The most passes a director holds an arm on a host-fed creature state
/// (a clip frame, a loop counter) before it lets the arm through: well past
/// any clip the band plays at the creature's slowest rate, so the bound only
/// ever releases an arm whose state the host stopped advancing.
pub const HOST_WAIT_CAP: u16 = 0x400;

/// Hold arm `phase` while `waiting`, for at most [`HOST_WAIT_CAP`] passes.
/// Returns whether the arm holds this pass.
pub fn host_wait(st: &mut ModuleCamState, phase: u8, waiting: bool) -> bool {
    if st.wait.0 != phase {
        st.wait = (phase, 0);
    }
    if !waiting || st.wait.1 >= HOST_WAIT_CAP {
        return false;
    }
    st.wait.1 += 1;
    true
}

/// A frame wait on the creature's node cursor: holds while it reads below
/// `frame` (sixteenths). With no cursor fed the wait passes.
pub fn frame_wait(st: &mut ModuleCamState, phase: u8, frame: u16) -> bool {
    let waiting = st.creature_frame.is_some_and(|f| f < frame);
    host_wait(st, phase, waiting)
}

/// A wait on the creature's loop counter `+0x21B` still reading the
/// `0xFF` sentinel: its clip has not reached its park frame.
pub fn park_wait(st: &mut ModuleCamState, phase: u8) -> bool {
    let waiting = st.creature_loops == Some(0xFF);
    host_wait(st, phase, waiting)
}

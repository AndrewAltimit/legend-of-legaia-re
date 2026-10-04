//! The **capture-class** cast modules' camera arms (PROT 0935..0966): the
//! shots and drifts a monster's special arms while the capture band's `0x70`
//! re-enters it.
//!
//! `0x70` calls no framing case (`0x801E50C8..0x801E50E8`: the module tick
//! `FUN_801F2160`, then `0x71` once it returns zero), so the camera through
//! the module's run is whatever the module does to it - the last case-6 pose
//! `0x6F` armed, then the module's own `FUN_801D829C` shots and the drifts it
//! adds straight into the camera globals. The module's phase chain (its
//! simulation) is ported in [`crate::cast_module_ticks`]; this is the camera
//! half, run beside it on the phase the body is about to run.
//!
//! A drift is per battle frame `k * delta` with `delta = *(0x1F800393)`, the
//! vsyncs a battle frame spans; the engine ticks once per vsync, so a drift
//! here is `k` per tick.

use super::{ModuleCamSeats, ModuleCamState, ModuleShot, SPEED_SCALAR, focus_on, yaw_from};

/// A capture arm's direct writes into the live camera globals, every pass the
/// arm runs (held or not): pitch / yaw `0x8007B790` / `0x8007B792`, TR y / z
/// `0x800840BC` / `0x800840C0` (the TR z delta in the global's own prescaled
/// units).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CaptureDrift {
    pub pitch: i16,
    pub yaw: i16,
    pub tr_y: i16,
    pub tr_z: i16,
}

/// What a capture module's camera does on one pass of one arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CaptureCamArm {
    /// The `FUN_801D829C` call the arm makes, if any.
    pub shot: Option<ModuleShot>,
    /// The drift the arm adds to the globals, if any.
    pub drift: Option<CaptureDrift>,
    /// The arm's countdown gate holds: the body's phase chain must not run
    /// this pass (retail's arm returns busy before any of its writes).
    pub hold: bool,
}

/// One capture body's camera arms, by the module phase `ctx[+0x279]` the
/// body is about to run.
pub type CaptureCamDirector = fn(&mut ModuleCamState, u8, ModuleCamSeats) -> CaptureCamArm;

/// The camera director of a capture-class body, keyed on `(entry, body)` as
/// the trampoline map is ([`crate::cast_module_ticks::capture_tick_body`]).
/// `None` for a body whose camera arms are not ported: the camera then holds
/// the pose `0x6F` left.
pub fn capture_camera_director(entry: u32, body: u32) -> Option<CaptureCamDirector> {
    match (entry, body) {
        (940, MYSTIC_SHIELD_BODY) => Some(mystic_shield_camera),
        _ => None,
    }
}

/// The module-resident countdown word a directed capture body gates its arms
/// on, by the caster's queued action id `+0x1DF` - what a save state's RAM
/// holds of where in an arm the module is.
pub fn capture_countdown_va(action: u8) -> Option<u32> {
    match action {
        0xAC => Some(MYSTIC_SHIELD_COUNTDOWN),
        _ => None,
    }
}

/// PROT 0940's countdown word (`lui 0x8020` / `-0x79B4`).
pub const MYSTIC_SHIELD_COUNTDOWN: u32 = 0x801F_864C;

/// PROT 0940's `0xAC` body (Cort's Mystic Shield).
pub const MYSTIC_SHIELD_BODY: u32 = 0x801F_7240;

/// Cort's **Mystic Shield** (PROT 0940, body `0x801F7240`, eight arms off the
/// table at the image head):
///
/// | arm | camera |
/// |---|---|
/// | 0 (`0x801F72E0`) | shot: pitch `0x200`, yaw `0x800 - caster[+0x46]`, TR `(0, -0x100, 0x400)`, focus the caster's seat, `0x40` frames |
/// | 2 (`0x801F73C4`) | drift: TR z `+12`, TR y `+6`, pitch `-2` |
/// | 4, 5 (`0x801F7584`, `0x801F76E8`) | drift: TR z `-6`, TR y `+1`, yaw `-16` |
/// | 6 (`0x801F77BC`) | drift: TR z `-8`, TR y `+2` |
///
/// Every arm from 1 on is gated on the module countdown `0x801F864C`
/// (drained `scalar * delta`, holding while it stays positive), and re-arms
/// it as it passes: arm 0 seeds `scalar * 0x60`, arm 1 adds `scalar << 8`,
/// arms 2 / 3 / 5 / 6 `scalar << 6` and arm 4 `scalar * 0xC0`; arm 7 is the
/// last gate before the body finishes. Arms 1, 3 and 7 write no camera. The
/// `cort_mystic_shield_mid_cast` capture sits in arm 1 with `496` of the
/// word left - `34` vsyncs into arm 0's `0x40`-frame shot: pitch `288` of
/// `0x200`, TR y `416` of `-0x100`.
///
/// PORT: FUN_801F7240 (PROT 0940; the camera arms 0/2/4/5/6 and the
/// countdown gates of arms 1..7)
pub fn mystic_shield_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let c = seats.caster;
    // The re-arm each gate makes as it passes, `scalar * n`.
    let rearm = |phase: u8| match phase {
        1 => Some(0x100),
        2 | 3 | 5 | 6 => Some(0x40),
        4 => Some(0xC0),
        _ => None,
    };
    let drift = match phase {
        2 => Some(drift(-2, 0, 6, 12)),
        4 | 5 => Some(drift(0, -16, 1, -6)),
        6 => Some(drift(0, 0, 2, -8)),
        _ => None,
    };
    match phase {
        0 => {
            st.countdown.0 = SPEED_SCALAR * 0x60;
            CaptureCamArm {
                shot: Some(ModuleShot {
                    angles: [0x200, yaw_from(0x800, c.facing), 0],
                    tr: [0, -0x100, 0x400],
                    focus: focus_on(c),
                    frames: 0x40,
                }),
                drift: None,
                hold: false,
            }
        }
        1..=7 => {
            let hold = st.countdown.drain_above(0);
            if !hold && let Some(n) = rearm(phase) {
                st.countdown.add(n);
            }
            CaptureCamArm {
                shot: None,
                drift,
                hold,
            }
        }
        _ => CaptureCamArm::default(),
    }
}

fn drift(pitch: i16, yaw: i16, tr_y: i16, tr_z: i16) -> CaptureDrift {
    CaptureDrift {
        pitch,
        yaw,
        tr_y,
        tr_z,
    }
}

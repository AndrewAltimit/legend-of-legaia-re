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
    /// For a body whose phase chain is **not** ported, the phase a passing
    /// arm moves to (`None` on the arm that finishes). The director then
    /// owns the module phase, the way a camera-only player director does; a
    /// ported body ignores it.
    pub next: Option<u8>,
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
        (944, GUILTY_CROSS_BODY) => Some(guilty_cross_camera),
        (962, ULTRA_CHARGE_BODY) => Some(ultra_charge_camera),
        _ => None,
    }
}

/// The module-resident countdown word a directed capture body gates its arms
/// on, by the caster's queued action id `+0x1DF` - what a save state's RAM
/// holds of where in an arm the module is.
pub fn capture_countdown_va(action: u8) -> Option<u32> {
    match action {
        0xAC => Some(MYSTIC_SHIELD_COUNTDOWN),
        0x37 => Some(GUILTY_CROSS_COUNTDOWN),
        0xA5 => Some(ULTRA_CHARGE_COUNTDOWN),
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
                next: None,
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
                next: None,
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

/// PROT 0944's `0x37` body (Cort's Guilty Cross).
pub const GUILTY_CROSS_BODY: u32 = 0x801F_6A04;
/// PROT 0944's countdown word (`lui 0x8020` / `-0x7CA0`).
pub const GUILTY_CROSS_COUNTDOWN: u32 = 0x801F_8360;

/// Cort's **Guilty Cross** (PROT 0944, body `0x801F6A04`, six arms off the
/// table at the image head), every arm from 1 on gated on the countdown
/// `0x801F8360` (drained `scalar * delta`):
///
/// | arm | camera | countdown as it passes |
/// |---|---|---|
/// | 0 (`0x801F6AC8`) | shot: pitch `0`, yaw `0x800 - caster[+0x46]`, TR `(0, 0x600, 0x800)`, focus the caster, `0x20` frames | `= scalar << 7` |
/// | 1 (`0x801F6CC4`) | drift TR z `+4` | `+= scalar << 7` |
/// | 2 (`0x801F6E04`) | drift TR z `+4`; then a cut: pitch `0x100`, yaw `0x800 - victim[+0x46]`, TR `(0, 0x400, 0xA00)`, focus the victim | `+= scalar << 7` |
/// | 3 (`0x801F6FD4`) | drift TR z `+32` | `+= scalar << 8` |
/// | 4 (`0x801F7238`) | a cut back to arm 0's framing | `+= scalar * 0xC0` |
/// | 5 (`0x801F7378`) | drift TR z `+4` | - (the body finishes) |
///
/// The `cort_guilty_cross_mid_cast` capture sits in arm 1 with `456` of the
/// word left, on arm 0's framing.
///
/// PORT: FUN_801F6A04 (PROT 0944; the camera arms and the countdown gates)
pub fn guilty_cross_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let c = seats.caster;
    let v = seats.victim;
    let behind_caster = |frames: u16| ModuleShot {
        angles: [0, yaw_from(0x800, c.facing), 0],
        tr: [0, 0x600, 0x800],
        focus: focus_on(c),
        frames,
    };
    if phase == 0 {
        st.countdown.0 = SPEED_SCALAR << 7;
        return CaptureCamArm {
            shot: Some(behind_caster(0x20)),
            drift: None,
            hold: false,
            next: None,
        };
    }
    let drift = match phase {
        1 | 2 | 5 => Some(drift(0, 0, 0, 4)),
        3 => Some(drift(0, 0, 0, 32)),
        _ => None,
    };
    if !(1..=5).contains(&phase) {
        return CaptureCamArm::default();
    }
    let hold = st.countdown.drain_above(0);
    let mut shot = None;
    if !hold {
        match phase {
            1 => st.countdown.add(1 << 7),
            2 => {
                st.countdown.add(1 << 7);
                shot = Some(ModuleShot {
                    angles: [0x100, yaw_from(0x800, v.facing), 0],
                    tr: [0, 0x400, 0xA00],
                    focus: focus_on(v),
                    frames: 1,
                });
            }
            3 => st.countdown.add(1 << 8),
            4 => {
                st.countdown.add(0xC0);
                shot = Some(behind_caster(1));
            }
            _ => {}
        }
    }
    CaptureCamArm {
        shot,
        drift,
        hold,
        next: None,
    }
}

/// PROT 0962's `0xA5` body (evolved Cort's Ultra Charge).
pub const ULTRA_CHARGE_BODY: u32 = 0x801F_69D8;
/// PROT 0962's `0xA5` countdown word (`lui 0x8020` / `-0x7654`).
pub const ULTRA_CHARGE_COUNTDOWN: u32 = 0x801F_89AC;

/// **Ultra Charge** (PROT 0962, body `0x801F69D8`, a `beq` chain over
/// `{0, 1, 0xFF}`):
///
/// - arm 0 (`0x801F6A78`) - a shot behind the caster: pitch `0x10`, yaw
///   `0x800 - caster[+0x46]`, TR `(0, h, 0xC00)` over `0xC` frames, `h` being
///   `0xC00` when the formation's first monster is `0xB5` and `0x240`
///   otherwise; the countdown `= scalar * 0x180`;
/// - arm 1 (`0x801F6C1C`) - TR z drifts out by `scalar * delta` a pass while
///   the countdown, drained by the same product, stays positive; then
///   `0xFF`.
///
/// The `cort_evolved_ultra_charge_mid_cast` capture is in arm 1, on the
/// `0xB5` framing (TR y `0xC00`), its TR z `2400` drifted past the shot's.
///
/// The body's phase chain is not ported elsewhere, so this director also
/// owns its phase ([`CaptureCamArm::next`]): `0 -> 1 -> 0xFF`, finishing on
/// `0xFF`.
///
/// PORT: FUN_801F69D8 (PROT 0962 `0xA5`; the camera arms, the countdown and
/// the phase chain)
pub fn ultra_charge_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let c = seats.caster;
    match phase {
        0 => {
            st.countdown.0 = SPEED_SCALAR * 0x180;
            let h = if seats.first_monster == 0xB5 {
                0xC00
            } else {
                0x240
            };
            CaptureCamArm {
                shot: Some(ModuleShot {
                    angles: [0x10, yaw_from(0x800, c.facing), 0],
                    tr: [0, h, 0xC00],
                    focus: focus_on(c),
                    frames: 0xC,
                }),
                drift: None,
                hold: false,
                next: Some(1),
            }
        }
        1 => CaptureCamArm {
            shot: None,
            drift: Some(drift(0, 0, 0, super::MODULE_DRAIN_PER_TICK as i16)),
            hold: st.countdown.drain_above(0),
            next: Some(0xFF),
        },
        _ => CaptureCamArm::default(),
    }
}

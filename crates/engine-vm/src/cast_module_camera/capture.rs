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
    /// A store into the caster's battle-scoped latch word
    /// (`0x801C8FE0 + (ctx[+0x13] + 1) * 4`, the monster AI's ability
    /// cooldown `dat[m + 4]`), when the arm makes one.
    pub latch: Option<i32>,
    /// The arm took a branch that applies no damage: the band's fold owes
    /// this cast nothing.
    pub skips_fold: bool,
}

/// One capture body's camera arms, by the module phase `ctx[+0x279]` the
/// body is about to run.
pub type CaptureCamDirector = fn(&mut ModuleCamState, u8, ModuleCamSeats) -> CaptureCamArm;

/// The camera director of a capture-class body, keyed on `(entry, body)` as
/// the trampoline map is ([`crate::cast_module_ticks::capture_tick_body`]).
/// A module with no trampoline has one tick body and is keyed on
/// [`SINGLE_BODY`]. `None` for a body whose camera arms are not ported: the
/// camera then holds the pose `0x6F` left.
pub fn capture_camera_director(entry: u32, body: u32) -> Option<CaptureCamDirector> {
    match (entry, body) {
        (940, MYSTIC_SHIELD_BODY) => Some(mystic_shield_camera),
        (940, GLARE_BODY) => Some(glare_camera),
        (953, SINGLE_BODY) => Some(terio_punch_camera),
        (944, GUILTY_CROSS_BODY) => Some(guilty_cross_camera),
        (962, ULTRA_CHARGE_BODY) => Some(ultra_charge_camera),
        (938, MYSTIC_CIRCLE_BODY) => Some(mystic_circle_camera),
        (946, SINGLE_BODY) => Some(wave_camera),
        _ => None,
    }
}

/// The module-resident countdown word a directed capture body gates its arms
/// on, by the caster's queued action id `+0x1DF` - what a save state's RAM
/// holds of where in an arm the module is.
pub fn capture_countdown_va(action: u8) -> Option<u32> {
    match action {
        0xAC | 0x3C => Some(MYSTIC_SHIELD_COUNTDOWN),
        0x37 => Some(GUILTY_CROSS_COUNTDOWN),
        0xA5 => Some(ULTRA_CHARGE_COUNTDOWN),
        0xB7 => Some(MYSTIC_CIRCLE_COUNTDOWN),
        0x55 | 0x56 => Some(WAVE_COUNTDOWN),
        _ => None,
    }
}

/// PROT 0940's countdown word (`lui 0x8020` / `-0x79B4`).
pub const MYSTIC_SHIELD_COUNTDOWN: u32 = 0x801F_864C;

/// The key [`capture_camera_director`] takes for a module with no trampoline
/// (one tick body, the module's own head-table dispatch).
pub const SINGLE_BODY: u32 = 0;

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
                ..Default::default()
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
                ..Default::default()
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
            ..Default::default()
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
        ..Default::default()
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
                ..Default::default()
            }
        }
        1 => CaptureCamArm {
            shot: None,
            drift: Some(drift(0, 0, 0, super::MODULE_DRAIN_PER_TICK as i16)),
            hold: st.countdown.drain_above(0),
            next: Some(0xFF),
            ..Default::default()
        },
        _ => CaptureCamArm::default(),
    }
}

/// PROT 0938's `0xB7` body (Cort's Mystic Circle).
pub const MYSTIC_CIRCLE_BODY: u32 = 0x801F_69EC;
/// PROT 0938's `0xB7` countdown word (`lui 0x8020` / `-0x7FC0`).
pub const MYSTIC_CIRCLE_COUNTDOWN: u32 = 0x801F_8040;

/// Cort's **Mystic Circle** (PROT 0938, body `0x801F69EC`, five arms off the
/// table at the image head). Unlike the other directed bodies its countdown
/// `0x801F8040` is absolute, not scalar-scaled: arm 0 stores `0x800`, every
/// later arm drains `8 * delta` (`8` a vsync) and holds while it stays
/// positive, and arms 1 / 2 / 3 re-arm `+0x400` / `+0x400` / `+0x600`.
///
/// | arm | camera |
/// |---|---|
/// | 0 (`0x801F6AA4`) | turns the caster to its target, then a cut: pitch `-0x40`, yaw `0x800 - caster[+0x46]`, TR `(0, 0x600, 0x600)`, focus the caster |
/// | 1, 2 (`0x801F6C00`, `0x801F6D88`) | drift: TR z `+4`, TR y `-1`; arm 2's exit cuts to pitch `0x180`, yaw `-caster[+0x46]`, TR `(0, 0x600, 0xC00)` |
/// | 3 (`0x801F6F3C`) | drift: TR z `+96`, TR y `-29` |
/// | 4 (`0x801F71F8`) | the last gate |
///
/// Arm 0's turn (toward the target, or the party's centre for the
/// all-party target `8`) is the cast's own; the director frames off the
/// caster's facing as the engine left it. The `cort_mystic_circle_mid_cast`
/// capture sits in arm 1 with `96` of the word left: `244` vsyncs of drift,
/// TR y `1292` of `0x600`.
///
/// PORT: FUN_801F69EC (PROT 0938 `0xB7`; the camera arms and the countdown)
pub fn mystic_circle_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    const DRAIN: i32 = 8;
    let c = seats.caster;
    if phase == 0 {
        st.countdown.0 = 0x800;
        return CaptureCamArm {
            shot: Some(ModuleShot {
                angles: [-0x40, yaw_from(0x800, c.facing), 0],
                tr: [0, 0x600, 0x600],
                focus: focus_on(c),
                frames: 1,
            }),
            ..Default::default()
        };
    }
    if !(1..=4).contains(&phase) {
        return CaptureCamArm::default();
    }
    let drift = match phase {
        1 | 2 => Some(drift(0, 0, -1, 4)),
        3 => Some(drift(0, 0, -29, 96)),
        _ => None,
    };
    st.countdown.0 -= DRAIN;
    let hold = st.countdown.0 > 0;
    let mut shot = None;
    if !hold {
        match phase {
            1 => st.countdown.0 += 0x400,
            2 => {
                st.countdown.0 += 0x400;
                shot = Some(ModuleShot {
                    angles: [0x180, yaw_from(0, c.facing), 0],
                    tr: [0, 0x600, 0xC00],
                    focus: focus_on(c),
                    frames: 1,
                });
            }
            3 => st.countdown.0 += 0x600,
            _ => {}
        }
    }
    CaptureCamArm {
        shot,
        drift,
        hold,
        next: None,
        ..Default::default()
    }
}

/// PROT 0946's countdown word (`lui 0x801F` / `+0x7F20`).
pub const WAVE_COUNTDOWN: u32 = 0x801F_7F20;

/// Zeto's **Call Wave** (`0x55`) and **Big Wave** (`0x56`), PROT 0946's one
/// tick body `0x801F69FC` (nine arms off the image-head table). Retail picks
/// the choreography at arm 0 off a per-seat word (`0x801CEFE0 + (seat+1)*4`)
/// it toggles on every cast; the two library captures pin which branch each
/// spell takes, so the port keys on the action id:
///
/// | arm | Call Wave | Big Wave |
/// |---|---|---|
/// | 0 (`0x801F6AD8`) | cut: pitch `0x100`, TR `(0, 0x600, 2z)`; to arm 1 | cut: pitch `0x20`, TR `(0, 0x600, z/2)`; to arm 4 |
/// | 1 (`0x801F6D0C`) | pan: pitch `0`, TR `(0, 0x600, z/2)` over `0x100` frames; countdown `+= scalar << 8` | - |
/// | 2 (`0x801F6D9C`) | gate; then pitch `0`, TR `(0, 0x600, z)` over `0x40`; `+= scalar * 0x60` | - |
/// | 3 (`0x801F6E48`) | gate; the body finishes | - |
/// | 4 (`0x801F6E90`) | - | pan: pitch `0x20`, TR `(0, 0x600, 2z)` over `0x100`; `+= scalar << 6` |
/// | 5 (`0x801F6F30`) | - | gate; the wave's effects; `+= scalar * 0xC0` (`3 << 6`); to arm 6 |
/// | 6 (`0x801F7104`) | - | gate; cut: pitch `0x180`, yaw `0xF00 - caster[+0x46]`, TR `(0, 0x600, 3z/2)`; `+= scalar * 0x60`; to arm 7 |
/// | 7 (`0x801F72DC`) | - | yaw `+= delta * scalar / 4` every pass; gate; the party's hits; `+= scalar * 0xA0`; to arm 8 |
/// | 8 (`0x801F74E4`) | - | the same yaw spin; gate; the body finishes |
///
/// Arms 0..5's shots are behind the caster (yaw `0x800 - caster[+0x46]`,
/// focus the caster) and `z` is `ctx[+0x6D0]`. The body has no other port, so
/// this director owns its phase and finishes the module at arm 8 (retail's
/// own exit also waits on every party member's clip, which the countdown
/// outlasts). The `zeto_call_wave_mid_cast` capture is in arm 2 on arm 1's
/// pan, `zeto_big_wave_mid_cast` in arm 5 on arm 4's.
///
/// PORT: FUN_801F69FC, overlay_cast_call_wave_0946_801f69fc (PROT 0946; the camera arms 0..8, their countdown and
/// phase chain)
pub fn wave_camera(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> CaptureCamArm {
    let c = seats.caster;
    let z = seats.depth_raw;
    let shot = |pitch: i16, tr_z: i32, frames: u16| ModuleShot {
        angles: [pitch, yaw_from(0x800, c.facing), 0],
        tr: [0, 0x600, tr_z as i16],
        focus: focus_on(c),
        frames,
    };
    let pass = |shot: Option<ModuleShot>, next: Option<u8>| CaptureCamArm {
        shot,
        drift: None,
        hold: false,
        next,
        ..Default::default()
    };
    let gate = |st: &mut ModuleCamState| st.countdown.drain_above(0);
    let held = CaptureCamArm {
        hold: true,
        ..Default::default()
    };
    match phase {
        0 => {
            st.countdown.0 = 0;
            if seats.action == 0x56 {
                pass(Some(shot(0x20, z / 2, 1)), Some(4))
            } else {
                pass(Some(shot(0x100, z * 2, 1)), Some(1))
            }
        }
        1 => {
            st.countdown.add(1 << 8);
            pass(Some(shot(0, z / 2, 0x100)), Some(2))
        }
        2 => {
            if gate(st) {
                return held;
            }
            st.countdown.add(0x60);
            pass(Some(shot(0, z, 0x40)), Some(3))
        }
        3 => {
            if gate(st) {
                return held;
            }
            pass(None, None)
        }
        4 => {
            st.countdown.add(1 << 6);
            pass(Some(shot(0x20, z * 2, 0x100)), Some(5))
        }
        5 => {
            if gate(st) {
                return held;
            }
            // `3 * scalar << 6` (`0x801F6F78..0x801F6F94`).
            st.countdown.add(0xC0);
            pass(None, Some(6))
        }
        6 => {
            if gate(st) {
                return held;
            }
            // `3 * scalar << 5` (`0x801F71C8..0x801F71EC`), then the cut at
            // `0x801F7258..0x801F72CC`: `TR.z = 3 * ctx[+0x6D0] / 2`.
            st.countdown.add(0x60);
            let s = ModuleShot {
                angles: [0x180, yaw_from(0xF00, c.facing), 0],
                tr: [0, 0x600, (z * 3 / 2) as i16],
                focus: focus_on(c),
                frames: 1,
            };
            pass(Some(s), Some(7))
        }
        7 | 8 => {
            // `_DAT_8007B792 += (delta * scalar) / 4` ahead of the gate
            // (`0x801F7318..0x801F7368`, `0x801F74E4..0x801F7528`): per
            // vsync, a quarter of the scalar.
            let spin = Some(drift(0, (super::SPEED_SCALAR / 4) as i16, 0, 0));
            if gate(st) {
                return CaptureCamArm {
                    drift: spin,
                    ..held
                };
            }
            let next = if phase == 7 {
                // `5 * scalar << 5` (`0x801F74AC..0x801F74CC`).
                st.countdown.add(0xA0);
                Some(8)
            } else {
                None
            };
            CaptureCamArm {
                drift: spin,
                ..pass(None, next)
            }
        }
        _ => pass(None, None),
    }
}

/// PROT 0940's `0x3C` body (Glare), the trampoline's first arm.
pub const GLARE_BODY: u32 = 0x801F_69F8;

/// **Glare** (PROT 0940, body `0x801F69F8`, a `beq` chain over `0..=3`; its
/// countdown is the module word `0x801F864C` Mystic Shield also uses):
///
/// | arm | camera | countdown as it passes |
/// |---|---|---|
/// | 0 (`0x801F6ACC`) | shot behind the caster: pitch `0`, yaw `0x800 - caster[+0x46]`, TR `(0, 0x600, 0x800)` for monster `0xA9` else `(0, 0x400, 0xA00)`, focus the caster, `0xC` frames | `= scalar << 8` |
/// | 1 (`0x801F6C7C`) | drift TR z `-8`, TR y `+2` a pass; once spent, a cut onto the victim: pitch `0`, yaw `0x800 - victim[+0x46]`, TR `(0, 0x400, 0xA00)` | `+= scalar << 7` |
/// | 2 (`0x801F6EEC`) | drift TR z `-8` | `+= scalar << 7` |
/// | 3 (`0x801F71B4`) | drift TR z `-8`; the last gate - the body returns `0` | - |
///
/// Each arm from 1 on drains the word by `scalar * delta` and holds while it
/// stays positive (`bgtz` to the epilogue with the busy `1`). The body has no
/// damage site: arm 2 raises the element `0x5B` and sets `ctx[+0x18]`, the
/// status half the band's fold owns, which the camera does not touch. The
/// body's phase chain has no other port, so this director owns the phase.
///
/// PORT: overlay_cast_glare_divide_0940_801f69f8 (PROT 0940 `0x3C`; the camera arms, the countdown and the phase chain)
pub fn glare_camera(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> CaptureCamArm {
    let pull = drift(0, 0, 0, -8);
    match phase {
        0 => {
            st.countdown.0 = SPEED_SCALAR << 8;
            let tr = if seats.caster_monster == 0xA9 {
                [0, 0x600, 0x800]
            } else {
                [0, 0x400, 0xA00]
            };
            CaptureCamArm {
                shot: Some(ModuleShot {
                    angles: [0, yaw_from(0x800, seats.caster.facing), 0],
                    tr,
                    focus: focus_on(seats.caster),
                    frames: 0xC,
                }),
                drift: None,
                hold: false,
                next: Some(1),
                ..Default::default()
            }
        }
        1..=3 => {
            let d = if phase == 1 { drift(0, 0, 2, -8) } else { pull };
            let hold = st.countdown.drain_above(0);
            if hold {
                return CaptureCamArm {
                    shot: None,
                    drift: Some(d),
                    hold: true,
                    next: Some(phase),
                    ..Default::default()
                };
            }
            if phase < 3 {
                st.countdown.add(1 << 7);
            }
            let shot = (phase == 1).then(|| ModuleShot {
                angles: [0, yaw_from(0x800, seats.victim.facing), 0],
                tr: [0, 0x400, 0xA00],
                focus: focus_on(seats.victim),
                frames: 1,
            });
            CaptureCamArm {
                shot,
                drift: Some(d),
                hold: false,
                next: (phase < 3).then_some(phase + 1),
                ..Default::default()
            }
        }
        _ => CaptureCamArm::default(),
    }
}

/// PROT 0953 (Terio Punch), a single-body module: its `0x801CF56C` arm
/// points straight at the tick `0x801F69FC`, a nine-arm switch (`sltiu 9`,
/// table at the image head).
///
/// The body is **two casts in one**, forked at arm 0 on the caster's latch
/// word `0x801C8FE0 + (ctx[+0x13] + 1) * 4` - the word the monster AI
/// reads for monster `0x8B` (it picks `0x5D` when it is set, else `0x5E`):
///
/// * latch clear - the **charge** (`0x801F6CEC`): a cut behind the caster -
///   pitch `0`, yaw `0x800 - caster[+0x46]`, TR `(0, 0x600, depth / 2)` -
///   the latch set, and phase 1; arm 1 (`0x801F6E74`) pulls out to pitch
///   `0x200`, TR `(0, 0, depth * 2)` over `0x100` frames and arms
///   `scalar << 8` on the module word `0x801F7E88`; arm 2 holds on it and
///   the body returns `0`. No damage site runs, so the fold owes nothing.
/// * latch set - the **punch** (`0x801F6B0C`): the caster turned to face
///   its victim (`h = heading(victim -> caster) + 0x800`), a cut - pitch
///   `0x20`, yaw `0x400 - h`, TR `(0, 0x600, depth)` - the latch cleared, and
///   phase 4. Arm 4 (`0x801F6FA0`): pitch `-0x40`, yaw `0x600 - h`, TR
///   `(0x200, 0xA00, depth / 3)` over `0x50` frames, `+= scalar * 80`. Arm 5:
///   held, then pitch `0`, yaw `0x800 - h`, TR `(0, 0x550, depth)` over
///   `0x40`, `+= scalar << 6`. Arm 6 drains every pass (spawning the trail
///   parts) and, once spent, pulls out to TR `(0, 0x550, 0x2800)` over
///   `0x80`, `+= scalar * 96`. Arm 7: held, then the **strike** -
///   `FUN_801DD6B4(0x274, caster, seat)` over every hittable party seat
///   (`0x801F7410`), which is the band's fold here - and `+= scalar * 160`.
///   Arm 8 holds out the word and the body returns `0`.
///
/// `depth` is `ctx[+0x6D0]`. Every framing's focus is the caster. Arm 8 also
/// waits for every party seat to leave its reaction clip, which this
/// director does not model.
///
/// PORT: overlay_cast_terio_punch_0953_801f69fc (PROT 0953; the camera arms, the countdown, the charge latch and the phase chain)
pub fn terio_punch_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let c = seats.caster;
    let depth = seats.depth_raw as i16;
    let h = super::heading(seats.victim, c).wrapping_add(0x800) & 0xFFF;
    let shot = |angles: [i16; 3], tr: [i16; 3], frames: u16| {
        Some(ModuleShot {
            angles,
            tr,
            focus: focus_on(c),
            frames,
        })
    };
    let held = CaptureCamArm {
        hold: true,
        next: Some(phase),
        ..Default::default()
    };
    let step = |shot: Option<ModuleShot>, next: Option<u8>| CaptureCamArm {
        shot,
        next,
        ..Default::default()
    };
    match phase {
        0 if seats.caster_latch != 0 => {
            st.countdown.0 = 0;
            CaptureCamArm {
                latch: Some(0),
                ..step(
                    shot([0x20, yaw_from(0x400, h), 0], [0, 0x600, depth], 1),
                    Some(4),
                )
            }
        }
        0 => CaptureCamArm {
            latch: Some(1),
            skips_fold: true,
            ..step(
                shot([0, yaw_from(0x800, c.facing), 0], [0, 0x600, depth / 2], 1),
                Some(1),
            )
        },
        1 => {
            st.countdown.add(1 << 8);
            step(
                shot(
                    [0x200, yaw_from(0x800, c.facing), 0],
                    [0, 0, depth.wrapping_mul(2)],
                    0x100,
                ),
                Some(2),
            )
        }
        2 => {
            if st.countdown.drain_above(0) {
                return held;
            }
            step(None, None)
        }
        4 => {
            st.countdown.add(80);
            step(
                shot(
                    [-0x40, yaw_from(0x600, h), 0],
                    [0x200, 0xA00, depth / 3],
                    0x50,
                ),
                Some(5),
            )
        }
        5 => {
            if st.countdown.drain_above(0) {
                return held;
            }
            st.countdown.add(1 << 6);
            step(
                shot([0, yaw_from(0x800, h), 0], [0, 0x550, depth], 0x40),
                Some(6),
            )
        }
        6 => {
            if st.countdown.drain_above(0) {
                return held;
            }
            st.countdown.add(96);
            step(
                shot([0, yaw_from(0x800, h), 0], [0, 0x550, 0x2800], 0x80),
                Some(7),
            )
        }
        7 => {
            if st.countdown.drain_above(0) {
                return held;
            }
            st.countdown.add(160);
            step(None, Some(8))
        }
        8 => {
            if st.countdown.0 > 0 && st.countdown.drain_above(0) {
                return held;
            }
            step(None, None)
        }
        _ => CaptureCamArm::default(),
    }
}

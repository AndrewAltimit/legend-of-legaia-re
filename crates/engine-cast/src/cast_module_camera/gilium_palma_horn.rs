//! PROT 0923 (Gilium), 0928 (Palma) and 0930 (Horn): each module's whole
//! phase chain as a director ([`ModuleProfile::director_body`]), read off
//! its tick's disassembly. Gilium drains its word by `8 * delta`, Palma by
//! the scalar times the delta, Horn by the delta alone.
//!
//! Not carried, as in [`super::gola_mushura_barra`]: part spawns (the stager
//! seats the record set on its first tick), the writes to the other seats,
//! the stage words `ctx[+0x27A..+0x286]`, pad rumble, and every hit beyond
//! the one the band's fold lands on the hit arm.
//!
//! See `ghidra/scripts/funcs/overlay_summon_gilium_0923_801f69d8.txt`,
//! `overlay_summon_palma_0928_801f69f4.txt` and
//! `overlay_summon_horn_0930_801f6a74.txt`.
//!
//! The arms' fades are not reported here: the presentation hook
//! (`World::fire_cast_presentation`) fires them off the image's own decoded
//! call sites, on the arm timing these directors pace.

use super::body_kit::*;
use super::*;

// ---------------------------------------------------------------------------
// PROT 0923 - Gilium
// ---------------------------------------------------------------------------

/// PROT 0923's hit arm: its pass sweeps the living monsters through
/// `FUN_801DD0AC(0x12, 7, seat)` (`0x801F88C4..0x801F8A48`).
pub const GILIUM_HIT_ARM: u8 = 0x15;

/// Gilium's drain, `8 * delta` a pass.
const GILIUM_DRAIN: i32 = 8 * D;

/// PROT 0923 (Gilium): every arm opens on the gate that drains only a
/// positive word, and its per-pass drift is written only on the passes it
/// drains. Every framing is a literal on the origin.
///
/// | arm | drift a draining pass | exit |
/// |---|---|---|
/// | 0 | - | a cut; word `= 0x200` |
/// | 1..3 | TR z `-= 4` | `+= 0x200` / `0x400`; arm 3 seats the creature at `(0, -0x100, -0x800)` |
/// | 4 | TR z `-= 4` | creature `z = 0x400`; a `0xC`-frame shot; the spell name; clip `1`; `+= 0x200` |
/// | 5, 6 | TR z `+= 4` | `+= 0x3A0`; arm 6: the attack name, clip `2`, a cut, `+= 0x400` |
/// | 7 | TR z, TR y `+= 4` | a cut; `+= 0x400` |
/// | 8 | TR z `-= 1`, TR y `+= 16`, pitch `+= 1` | `+= 0x600` |
/// | 9 | TR z `+= 32`, TR y `+= 4` | a cut; `+= 0x300` |
/// | 0xA | TR z `-= 2`, TR y `+= 4` | a cut; two fades; `+= 0x600` |
/// | 0xB | TR z, TR y `+= 8` | a cut; a white hold; `+= 0x100` |
/// | 0xC | - | the white faded out; `+= 0x800` |
/// | 0xD | TR y `+= 16`, TR z `+= 8`, yaw `+= 4` | `+= 0x200` |
/// | 0xE, 0xF | yaw `+= 4` | a fade; `+= 0x40`; arm 0xF: TR y `-= 0x100`, `+= 0x3C0` |
/// | 0x10, 0x11 | TR y `-= 8`, yaw `+= 4` | a subtractive fade, `+= 0x200`; arm 0x11: a cut, `+= 0x400` |
/// | 0x12 | - | a `0x40`-frame tilt, one drain, no gate |
/// | 0x13 | - | `+= 0x200` |
/// | 0x14, 0x15 | TR z `-= 4` (0x15: pitch `-= 1`) | fades; `+= 0x600` / `0x400`; arm 0x15 is the hit |
/// | 0x16 | TR z `-= 8`, pitch `-= 1` | the monster row settled; then `0xFF` |
/// | 0xFF | - | the caption cleared; the loop counter cleared; done |
///
/// PORT: FUN_801F69D8 (PROT 0923; the whole phase chain's gates, camera writes, captions, creature seat and clips - part spawns and their ring motion, the row's reaction writes and the stage bytes not carried)
// The arm's early exit rides the `Err` side of a local `?` chain.
#[allow(clippy::result_large_err)]
pub fn gilium_direct(st: &mut ModuleCamState, phase: u8, _seats: ModuleCamSeats) -> ArmDirection {
    // `if (cd > 0) { cd -= 8 delta; <drift>; if (cd > 0) busy; }`, then the
    // exit's re-arm.
    let gated = |st: &mut ModuleCamState,
                 d: CaptureDrift,
                 add: i32|
     -> Result<Option<CaptureDrift>, ArmDirection> {
        let (hold, drained) = counted_if_positive(st, GILIUM_DRAIN);
        let d = drained.then_some(d);
        if hold {
            return Err(held(d));
        }
        st.countdown.0 += add;
        Ok(d)
    };
    let z = |n: i32| drift(0, 0, 0, 0, n);
    let pass = |d: Option<CaptureDrift>, dir: ArmDirection| with_drift(dir, d);
    // Arms 9..`0xB` hold the creature's clip on its park every pass, held or
    // not (`sb 0xFF,0x21B` / `sh 0xFF0,0x176` at `0x801F7858`,
    // `0x801F7904`, `0x801F7B58`).
    if (9..=0xB).contains(&phase) {
        st.creature_loop_arm = true;
    }
    let r = (|| -> Result<ArmDirection, ArmDirection> {
        Ok(match phase {
            0 => {
                st.countdown.0 = 0x200;
                cut([0, 0x800], [0, 0x600, 0x1000], [0; 3], 1)
            }
            1 => {
                let (_, drained) = counted_if_positive(st, GILIUM_DRAIN);
                pass(drained.then_some(z(-4)), ArmDirection::PASS)
            }
            2 => pass(gated(st, z(-4), 0x200)?, ArmDirection::PASS),
            3 => {
                let d = gated(st, z(-4), 0x400)?;
                st.creature = Some(ModuleSeat {
                    x: 0,
                    y: -0x100,
                    z: -0x800,
                    facing: 0,
                });
                pass(d, ArmDirection::PASS)
            }
            4 => {
                let d = gated(st, z(-4), 0x200)?;
                if let Some(c) = st.creature.as_mut() {
                    c.z = 0x400;
                }
                st.creature_stage = Some(1);
                pass(
                    d,
                    caption(
                        cut([-0x80, 0x800], [0, 0x500, 0x800], [0, 0, -0x400], 0xC),
                        ModuleCaption::SpellName,
                    ),
                )
            }
            5 => pass(gated(st, z(4), 0x3A0)?, ArmDirection::PASS),
            6 => {
                let d = gated(st, z(4), 0x400)?;
                st.creature_stage = Some(2);
                pass(
                    d,
                    caption(
                        cut([0, 0x800], [0, 0x680, 0xE80], [0; 3], 1),
                        ModuleCaption::AttackName,
                    ),
                )
            }
            7 => pass(
                gated(st, drift(0, 0, 0, 4, 4), 0x400)?,
                cut([0x100, 0], [0, 0x580, 0xA00], [0, 0, -0x400], 1),
            ),
            8 => pass(
                gated(st, drift(1, 0, 0, 16, -1), 0x600)?,
                ArmDirection::PASS,
            ),
            9 => pass(
                gated(st, drift(0, 0, 0, 4, 32), 0x300)?,
                cut([-0x300, 0x800], [0, 0xA00, 0x100], [0, 0, -0x400], 1),
            ),
            0xA => pass(
                gated(st, drift(0, 0, 0, 4, -2), 0x600)?,
                cut([0x100, 0xA00], [0, 0x1000, 0x2000], [0, 0, -0x4E4], 1),
            ),
            0xB => pass(
                gated(st, drift(0, 0, 0, 8, 8), 0x100)?,
                cut([-0x180, 0], [0, -0x1000, 0x600], [0; 3], 1),
            ),
            0xC => {
                gated(st, z(0), 0x800)?;
                ArmDirection::PASS
            }
            0xD => pass(gated(st, drift(0, 4, 0, 16, 8), 0x200)?, ArmDirection::PASS),
            0xE => pass(gated(st, drift(0, 4, 0, 0, 0), 0x40)?, ArmDirection::PASS),
            0xF => {
                gated(st, drift(0, 4, 0, 0, 0), 0x3C0)?;
                pass(Some(drift(0, 4, 0, -0x100, 0)), ArmDirection::PASS)
            }
            0x10 => pass(gated(st, drift(0, 4, 0, -8, 0), 0x200)?, ArmDirection::PASS),
            0x11 => pass(
                gated(st, drift(0, 4, 0, -8, 0), 0x400)?,
                cut([0, 0], [0, 0x2000, 0x2000], [0; 3], 1),
            ),
            0x12 => {
                st.countdown.0 -= GILIUM_DRAIN;
                cut([0x200, 0], [0, 0x400, 0x2000], [0; 3], 0x40)
            }
            0x13 => pass(gated(st, z(0), 0x200)?, ArmDirection::PASS),
            0x14 => pass(gated(st, z(-4), 0x600)?, ArmDirection::PASS),
            GILIUM_HIT_ARM => pass(
                gated(st, drift(-1, 0, 0, 0, -4), 0x400)?,
                ArmDirection::PASS,
            ),
            0x16 => {
                let d = gated(st, drift(-1, 0, 0, 0, -8), 0)?;
                if !st.row_settled {
                    return Err(held(d));
                }
                finish(pass(d, ArmDirection::PASS))
            }
            0xFF => {
                st.creature_loop_release = true;
                caption(ArmDirection::HOLD, ModuleCaption::Cleared)
            }
            _ => ArmDirection::HOLD,
        })
    })();
    r.unwrap_or_else(|held| held)
}

// ---------------------------------------------------------------------------
// PROT 0928 - Palma
// ---------------------------------------------------------------------------

/// PROT 0928's hit arm: its pass sweeps the living monsters through
/// `FUN_801DD0AC(0x10, 7, seat)` with a clamp that can kill
/// (`0x801F89AC..0x801F8B40`). Arm 0xF's earlier sweep cannot kill and is
/// not carried.
pub const PALMA_HIT_ARM: u8 = 0x13;

/// The phase PROT 0928's hit arm stores (`0xC8`), its settle arm.
pub const PALMA_SETTLE_PHASE: u8 = 0xC8;

/// One of arms 3 and 4's per-pass stores, worked out of the word `t`: the
/// globals land straight on these values every pass.
fn palma_sweep(phase: u8, t: i32) -> ArmDirection {
    let sin = |a: i32| i32::from(legaia_engine_system::trig::sin12((a & 0xFFF) as u16));
    let (pitch, yaw, tr_y, tr_z) = if phase == 3 {
        let yaw = sin(3 * t / 8 + 0x800);
        if t >= 160 * S {
            (0x40 - sin(2 * t) / 8, yaw, 0, 0x1800 - sin(t))
        } else {
            // Monster 3 is lifted `2 p` a pass; TR y follows it.
            (
                0x80 - sin(2 * t) / 16,
                yaw,
                0x200 + 4 * 2 * t,
                0x2000 - sin(t),
            )
        }
    } else {
        (
            0x40 - sin(t) / 4,
            sin(3 * t / 4 + 0xB00),
            sin(t) / 2,
            0x1800 - 2 * sin(t),
        )
    };
    cut([pitch, yaw], [0, tr_y, raw_tr_z(tr_z)], [0; 3], 1)
}

/// The stage byte `ctx[+0x278]` PROT 0928's arm 3 stores once its word `T`
/// has climbed to `scalar << 7` (`slt` / `bne` at `0x801F6F04..0x801F6F0C`,
/// `sb a1,0x278(v0)` at `0x801F6F1C`, `a1` the phase byte the switch loaded:
/// `3`). It darkens and drops the stage under the arm's starfield wash, and
/// no later arm touches it before arm `0x11`'s `0`.
pub fn palma_stage_byte(phase: u8, countdown: i32) -> Option<u8> {
    (phase == 3 && countdown >= S << 7).then_some(3)
}

/// PROT 0928 (Palma): the word `T` at `0x801FA0D0` drives arms 3 and 4
/// upwards (`T += p` to `scalar * 256` / `128`), where the module stores the
/// camera globals straight from sines of `T` every pass; the port lands each
/// store as a one-frame cut on the origin. Every other arm is a countdown
/// gate on the same word.
///
/// | arm | gate | camera / writes |
/// |---|---|---|
/// | 0 | - | a cut on the origin |
/// | 1, 2 | the side-band applier stage `ctx[+0x276]`; the CD poll | arm 2: `T = 0`, two fades |
/// | 3, 4 | `T` up to `scalar * 256` / `* 128` | the per-pass stores; arm 4's exit a `0x160`-frame pan, the creature seated on the origin, `T = scalar * 384` |
/// | 5 | `T` | a `0x10`-frame shot, the spell name, clip `1`, `T = scalar * 64` |
/// | 6 | `T`, that shot re-armed each pass | a `0x60`-frame shot, clip `2`, `T = scalar * 180` |
/// | 7 | until clip `2` plays, that shot re-armed | a cut, the attack name, `T = scalar * 128` |
/// | 8 | the creature's park (`+0x21B` off `0xFF`) | TR z `+= 4 p`, TR y `-= p / 2`; a cut; the cursor back on keyframe 15 and the park re-armed |
/// | 9, 0xA | the park | cuts; arm 9 replays the swing like arm 8; arm 0xA `T = scalar * 64` |
/// | 0xB, 0xC | `T`, TR z `+= 32 p`, TR y `-= 4 p` | `T = scalar * 96`; arm 0xC: an all-zero cut, `T = scalar * 256` |
/// | 0xD, 0xE | `T` | `T = scalar * 120`; arm 0xE: a cut, `T = scalar * 128` |
/// | 0xF, 0x10 | `T`, a shot re-armed each pass | arm 0xF: a cut; arm 0x10: a fade in, a `0x80`-frame shot |
/// | 0x11 | `T` | the fade out, a cut |
/// | 0x12, 0x13 | `T`, yaw `+= p / 4`, TR z `+= 4 p` (0x13: pitch `-= p / 4`, TR y `+= 2 p`) | arm 0x12: a fade; arm 0x13: the hit, phase `0xC8`, `T += scalar * 128` |
/// | 0xC8 | `T` spent and the row settled | TR z `+= p` while `T` runs; `0xFF` |
/// | 0xFF | - | `ctx[+0x6DA] = 0`; the loop counter cleared; the caption cleared; done |
///
/// PORT: FUN_801F69F4 (PROT 0928; the whole phase chain's gates, camera writes and stores, captions, creature seat and clips - the roll store, the monster-row lift and seats, part spawns, the arm-0xF sweep and the stage and lighting words not carried)
pub fn palma_direct(st: &mut ModuleCamState, phase: u8, _seats: ModuleCamSeats) -> ArmDirection {
    let gated = |st: &mut ModuleCamState, set: i32| -> bool {
        if counted(st, P) {
            return true;
        }
        st.countdown.0 = set;
        false
    };
    let shot5 = || cut([-0x40, 0x680], [0x240, 0x980, 0x300], [0; 3], 0x10);
    let shot6 = || cut([-0x100, -0x6A0], [0, 0x280, 0x700], [0; 3], 0x60);
    let shot_f = || cut([0x80, 0x800], [0, 0, 0x2000], [0, 0, 0x4000], 0x40);
    let shot_10 = || cut([-0x140, 0xA00], [0x2710, 0x1388, 0x4E20], [0; 3], 0x80);
    match phase {
        0 => cut([0x80, 0], [0, 0x200, 0x2000], [0; 3], 1),
        // Arm 1 returns busy while the side-band applier stage
        // `ctx[+0x276]` is up (`0x801F6D58..0x801F6D60`, [`super::sideband_wait_arm`]):
        // four slots of `summon.dat` for Palma's base `0xCB`, which a fresh
        // cast clears 29 vsyncs into the sustain.
        1 => gate(st.sideband_busy),
        2 => {
            st.countdown.0 = 0;
            ArmDirection::PASS
        }
        3 | 4 => {
            st.countdown.0 += P;
            let end = if phase == 3 { 256 * S } else { 128 * S };
            if st.countdown.0 < end {
                return ArmDirection {
                    hold: true,
                    ..palma_sweep(phase, st.countdown.0)
                };
            }
            if phase == 3 {
                st.countdown.0 = 0;
                return ArmDirection::PASS;
            }
            st.countdown.0 = 384 * S;
            st.creature = Some(ModuleSeat::default());
            cut([0x100, 0x600], [0, 0xC8, 0x1000], [0; 3], 0x160)
        }
        5 => {
            if gated(st, 64 * S) {
                return ArmDirection::HOLD;
            }
            st.creature_stage = Some(1);
            caption(shot5(), ModuleCaption::SpellName)
        }
        6 => {
            if gated(st, 180 * S) {
                return ArmDirection {
                    hold: true,
                    ..shot5()
                };
            }
            st.creature_stage = Some(2);
            shot6()
        }
        7 => {
            st.countdown.0 -= P;
            if clip_wait(st, |k| k != 2) {
                return ArmDirection {
                    hold: true,
                    ..shot6()
                };
            }
            st.countdown.0 = 128 * S;
            caption(
                cut([-0x40, 0x800], [0, 0x300, 0x400], [0; 3], 1),
                ModuleCaption::AttackName,
            )
        }
        // Arms 8..`0xA` hold while the creature's clip has not reached its
        // park (`lbu v0,0x21b(s4); beq v0,0xff` at `0x801F79D4`,
        // `0x801F7A70`, `0x801F7AF8`). Arms 8 and 9 then put the cursor
        // back on keyframe 15 (`sh 0xF0,0x68(node)`) and re-arm the park
        // (`+0x21B = 0xFF`, `+0x176 = 0xFF0`), so the clip replays its
        // swing from there.
        8 => {
            let d = Some(drift(0, 0, 0, -(P / 2), 4 * P));
            st.countdown.0 -= P;
            if park_wait(st, phase) {
                return held(d);
            }
            st.creature_seek = Some(0xF0);
            st.creature_loop_arm = true;
            with_drift(cut([0, 0x800], [0xC00, 0x480, 0x200], [0; 3], 1), d)
        }
        9 => {
            if park_wait(st, phase) {
                return ArmDirection::HOLD;
            }
            st.creature_seek = Some(0xF0);
            st.creature_loop_arm = true;
            cut([0, 0x800], [-0xC00, 0x480, 0x200], [0; 3], 1)
        }
        0xA => {
            if park_wait(st, phase) {
                return ArmDirection::HOLD;
            }
            st.countdown.0 = 64 * S;
            cut([0, 0x800], [0, 0x300, 0xA00], [0; 3], 1)
        }
        0xB | 0xC => {
            let d = Some(drift(0, 0, 0, -4 * P, 32 * P));
            let set = if phase == 0xB { 96 * S } else { 256 * S };
            if gated(st, set) {
                return held(d);
            }
            if phase == 0xB {
                with_drift(ArmDirection::PASS, d)
            } else {
                with_drift(cut([0, 0], [0, 0, 0], [0; 3], 1), d)
            }
        }
        0xD => gate(gated(st, 120 * S)),
        0xE => {
            if gated(st, 128 * S) {
                return ArmDirection::HOLD;
            }
            cut([0x100, 0x100], [0, 0, 0xC00], [0, 0, 0x4000], 1)
        }
        0xF => {
            if gated(st, 128 * S) {
                return ArmDirection {
                    hold: true,
                    ..shot_f()
                };
            }
            cut([0, 0xE00], [0, 0x400, 0x1800], [0; 3], 1)
        }
        0x10 => {
            if gated(st, 128 * S) {
                return ArmDirection {
                    hold: true,
                    ..shot_10()
                };
            }
            cut([-0x280, 0xA00], [0x7FFF, 0x5388, 0x4E20], [0; 3], 0x80)
        }
        0x11 => {
            if gated(st, 128 * S) {
                return ArmDirection::HOLD;
            }
            cut([0x140, 0xE00], [0, 0, 0x1000], [0; 3], 1)
        }
        0x12 => {
            let d = Some(drift(0, P / 4, 0, 0, 4 * P));
            if gated(st, 96 * S) {
                return held(d);
            }
            with_drift(ArmDirection::PASS, d)
        }
        PALMA_HIT_ARM => {
            let d = Some(drift(-(P / 4), P / 4, 0, 2 * P, 4 * P));
            if counted(st, P) {
                return held(d);
            }
            st.countdown.0 += 128 * S;
            goto(with_drift(ArmDirection::PASS, d), PALMA_SETTLE_PHASE)
        }
        PALMA_SETTLE_PHASE => {
            let mut d = None;
            if st.countdown.0 > 0 {
                st.countdown.0 -= P;
                d = Some(drift(0, 0, 0, 0, P));
            }
            if !st.row_settled || st.countdown.0 > 0 {
                return held(d);
            }
            finish(with_drift(ArmDirection::PASS, d))
        }
        0xFF => {
            st.yaw_base = 0;
            st.creature_loop_release = true;
            caption(ArmDirection::HOLD, ModuleCaption::Cleared)
        }
        _ => ArmDirection::HOLD,
    }
}

// ---------------------------------------------------------------------------
// PROT 0930 - Horn
// ---------------------------------------------------------------------------

/// PROT 0930's outcome arm: its pass fully heals every party seat and clears
/// its status (`0x801F771C`); Horn makes no damage call.
pub const HORN_HEAL_ARM: u8 = 0xA;

/// PROT 0930's terminal arm: it returns `0` without storing a phase.
pub const HORN_LAST_ARM: u8 = 0xB;

/// PROT 0930 (Horn): its word `0x801F9380` drains by the frame delta.
///
/// | arm | gate | camera / writes |
/// |---|---|---|
/// | 0, 1 | - | a cut on the origin, then a `0x100`-frame rise |
/// | 2 | the side-band applier stage `ctx[+0x276]` ([`super::sideband_wait_arm`]) | the creature seek |
/// | 3 | - (the stream poll) | a white fade in after `0x100`, word `= 0x180`, the creature seated at `(0, 0x800)` facing `0x800` |
/// | 4 | word | a fade out, the spell name; `+= 0x114` |
/// | 5, 6 | one drain each | - |
/// | 7 | word | a cut, the attack name; `+= 0x100` |
/// | 8 | word, yaw `+= delta`, TR z `+= 2 delta` | a cut, clip `1`; `+= 0xC0` |
/// | 9 | word, TR z `+= 16 delta`, clip `2` staged | two fades; `+= 0x140` |
/// | 0xA | word, TR z `+= 16 delta` | a cut, the heal, three fades; `+= 0x180` |
/// | 0xB | word, TR z `+= 3 delta` | the caption cleared, a fade out, a cut on the caster; done |
///
/// Arm `0x1E`, the creature's clip cycler, is in the jump table but no arm
/// stores it.
///
/// PORT: FUN_801F6A74, overlay_summon_horn_0930_801f6a74 (PROT 0930; the whole phase chain's gates, camera writes, captions, creature seat and clips - the party re-seat, part spawns and the stage words not carried)
pub fn horn_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let gated = |st: &mut ModuleCamState, add: i32| -> bool {
        if counted(st, D) {
            return true;
        }
        st.countdown.0 += add;
        false
    };
    match phase {
        0 => cut([0, 0], [0, 0x400, 0x400], [0; 3], 1),
        1 => cut([0, 0], [0, 0x1500, 0], [0, 0, -0x200], 0x100),
        // Arm 2 returns busy while the side-band applier stage `ctx[+0x276]`
        // is up (`0x801F6D60..0x801F6D68`; `sideband_wait_arm`).
        2 => gate(st.sideband_busy),
        3 => {
            st.countdown.0 = 0x180;
            st.creature = Some(ModuleSeat {
                x: 0,
                y: 0,
                z: 0x800,
                facing: 0x800,
            });
            ArmDirection::PASS
        }
        4 => {
            if gated(st, 0x114) {
                return ArmDirection::HOLD;
            }
            caption(ArmDirection::PASS, ModuleCaption::SpellName)
        }
        5 | 6 => {
            st.countdown.0 -= D;
            ArmDirection::PASS
        }
        7 => {
            if gated(st, 0x100) {
                return ArmDirection::HOLD;
            }
            caption(
                cut([-0x140, -0x80], [0, 0x1080, 0x1200], [0, 0, -0x800], 1),
                ModuleCaption::AttackName,
            )
        }
        8 => {
            let d = Some(drift(0, D, 0, 0, 2 * D));
            if gated(st, 0xC0) {
                return held(d);
            }
            st.creature_stage = Some(1);
            with_drift(cut([0, 0], [0, 0x158C, 0x200], [0, 0, -0x800], 1), d)
        }
        9 => {
            let d = Some(drift(0, 0, 0, 0, 16 * D));
            st.creature_stage = Some(2);
            if gated(st, 0x140) {
                return held(d);
            }
            with_drift(ArmDirection::PASS, d)
        }
        HORN_HEAL_ARM => {
            let d = Some(drift(0, 0, 0, 0, 16 * D));
            if gated(st, 0x180) {
                return held(d);
            }
            with_drift(cut([0, 0x800], [0, 0x3A0, 0xC00], [0, 0, -0x190], 1), d)
        }
        HORN_LAST_ARM => {
            let d = Some(drift(0, 0, 0, 0, 3 * D));
            if gated(st, 0) {
                return held(d);
            }
            let c = seats.caster;
            finish(caption(
                with_drift(
                    cut(
                        [0, 0x800 - i32::from(c.facing)],
                        [0, 0x3A0, 0x400],
                        on(c),
                        1,
                    ),
                    d,
                ),
                ModuleCaption::Cleared,
            ))
        }
        _ => ArmDirection::HOLD,
    }
}

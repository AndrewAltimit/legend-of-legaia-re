//! PROT 0931 (Jedo), 0921 (Iota) and 0929 (Mule): each module's whole phase
//! chain as a director ([`ModuleProfile::director_body`]), read off its
//! tick's disassembly. Jedo drains its word by the frame delta, Iota and
//! Mule by the scalar times the delta.
//!
//! Not carried, as in [`super::gola_mushura_barra`]: part spawns, the
//! writes to the other seats, the stage words `ctx[+0x27A..+0x286]` and the
//! camera block `0x80083FF8`, and every hit beyond the one the band's fold
//! lands on the hit arm. Jedo's instant kill and its shatter loop are the
//! host's (`World::apply_jedo_writes`), keyed on [`JEDO_HIT_ARM`] and
//! [`JEDO_KILL_ARM`].
//!
//! See `ghidra/scripts/funcs/overlay_summon_jedo_0931_801f6a58.txt`,
//! `overlay_summon_iota_0921_801f6a08.txt` and
//! `overlay_summon_mule_0929_801f69fc.txt`.
//!
//! The arms' fades are not reported here: the presentation hook
//! (`World::fire_cast_presentation`) fires them off the image's own decoded
//! call sites, on the arm timing these directors pace.

use super::body_kit::*;
use super::*;

// ---------------------------------------------------------------------------
// PROT 0931 - Jedo
// ---------------------------------------------------------------------------

/// PROT 0931's hit arm on a scripted fight: its exit sweeps the monster row
/// through `FUN_801DD0AC(0x12, 7, seat)` (`0x801F8438`). On any other fight
/// the arm makes no damage call.
pub const JEDO_HIT_ARM: u8 = 29;

/// PROT 0931's kill arm: on a fight that is not scripted, every pass adds
/// `delta << 2` to each living monster's `+0x1BA` (`0x801F86E0..
/// 0x801F874C`) and its exit zeroes their HP (`0x801F8814`).
pub const JEDO_KILL_ARM: u8 = 30;

/// The shatter step of [`JEDO_KILL_ARM`], one vsync's worth per engine tick.
pub const JEDO_SHATTER_STEP_PER_TICK: u16 = (D as u16) << 2;

/// PROT 0931's terminal arm: it returns `0` without storing a phase.
pub const JEDO_LAST_ARM: u8 = 31;

/// PROT 0931 (Jedo): its word `0x801F95FC` drains by the frame delta and
/// re-arms by literals. Every framing aims at the raw focus `(0, 0, 0x600)`
/// through arm 8 and `(0, 0, 0x400)` after it.
///
/// | arm | gate | camera / writes |
/// |---|---|---|
/// | 0, 1 | - | a cut, then a `0xAA`-frame pull-back |
/// | 2, 3 | the side-band applier stage `ctx[+0x276]` ([`super::sideband_wait_arm`]), then the stream poll | word `= 0x80` |
/// | 4 | word | the creature seated at `(0, -0x800)`; `+= 0x80` |
/// | 5, 6 | word; TR z `+= 8 d` (6: `+= 30 d`, TR y `-= 10 d`) | a cut (5); `+= 0x80` / `0xC0` |
/// | 7 | word | a cut, clip `1`; `+= 0x20` |
/// | 8 | one drain | a `0x40`-frame shot, clip `2` |
/// | 9, 10 | word | the spell name; the attack name and a cut; `+= 0x133` / `0xE0` |
/// | 11 | one drain | a `0x100`-frame pan |
/// | 12..21 | word (13 and 20 re-arm a shot each pass; 16 TR z `-= 2 d`, 19 `+= 2 d`) | cuts, clips `3..6`, a red fade (15), the loop counter cleared (14) |
/// | 22 | one drain a pass; the creature's clip cursor on `0x120` | two flash fades |
/// | 23, 24 | word | cuts, the same flashes |
/// | 25 | word | the loop counter cleared, a red hold |
/// | 26 | the word | a cut; `+= 0x40` |
/// | 27, 28 | none / word; TR z `+= 48 d` | `+= 0x40` |
/// | 29 | word | two fades; the scripted fight's hit; `+= 0x100` |
/// | 30 | word | a fade out; the kill (not scripted); `+= 0x80` |
/// | 31 | word; TR z `+= 4 d` | the caption cleared, `ctx[+0x6DA] = 0`, the loop counter cleared; done |
///
/// PORT: FUN_801F6A58, overlay_summon_jedo_0931_801f6a58 (PROT 0931; the whole phase chain's gates, camera writes, captions, creature seat and clips - part spawns and their sizes, the monster-row writes and the stage words not carried; the kill and the shatter loop are the host's)
pub fn jedo_direct(st: &mut ModuleCamState, phase: u8, _seats: ModuleCamSeats) -> ArmDirection {
    let gated = |st: &mut ModuleCamState, add: i32| -> bool {
        if counted(st, D) {
            return true;
        }
        st.countdown.0 += add;
        false
    };
    let at6 = [0, 0, 0x600];
    let at4 = [0, 0, 0x400];
    let shot13 = || cut([-0x80, 0x800], [0, 0x1380, 0xA00], at4, 0x30);
    let shot20 = || cut([-0x4C, 0x778], [0x500, 0x1B10, 0x200], at4, 0x20);
    match phase {
        0 => cut([-0x80, 0xA00], [0, 0x800, 0x1000], at6, 1),
        1 => cut([-0x80, 0xA00], [0, 0xC00, 0x1800], at6, 0xAA),
        // Arm 2 returns busy while the side-band applier stage `ctx[+0x276]`
        // is up (`lbu v0,0x276(v0)` / `bne v0,zero` at `0x801F6D18..0x801F6D20`;
        // `sideband_wait_arm`), then requests the creature stream.
        2 => gate(st.sideband_busy),
        3 => {
            st.countdown.0 = 0x80;
            ArmDirection::PASS
        }
        4 => {
            if gated(st, 0x80) {
                return ArmDirection::HOLD;
            }
            st.creature = Some(ModuleSeat {
                x: 0,
                y: 0,
                z: -0x800,
                facing: 0,
            });
            ArmDirection::PASS
        }
        5 => {
            let d = Some(drift(0, 0, 0, 0, 8 * D));
            if gated(st, 0x80) {
                return held(d);
            }
            with_drift(cut([0x100, 0x800], [0, 0x1000, 0x1000], at6, 1), d)
        }
        6 => {
            let d = Some(drift(0, 0, 0, -10 * D, 30 * D));
            if gated(st, 0xC0) {
                return held(d);
            }
            with_drift(ArmDirection::PASS, d)
        }
        7 => {
            if gated(st, 0x20) {
                return ArmDirection::HOLD;
            }
            st.creature_stage = Some(1);
            cut([0, 0x800], [0, 0x1600, 0xB00], at6, 1)
        }
        8 => {
            st.countdown.0 -= D;
            st.creature_stage = Some(2);
            cut([-0x140, 0x800], [0, 0x1400, 0x900], at6, 0x40)
        }
        9 => {
            if gated(st, 0x133) {
                return ArmDirection::HOLD;
            }
            caption(ArmDirection::PASS, ModuleCaption::SpellName)
        }
        10 => {
            if gated(st, 0xE0) {
                return ArmDirection::HOLD;
            }
            caption(
                cut([-0xC0, 0x900], [0, 0x1180, 0xA0], at4, 1),
                ModuleCaption::AttackName,
            )
        }
        11 | 17 => {
            st.countdown.0 -= D;
            if phase == 11 {
                cut([-0xC0, 0x700], [0, 0x1600, 0xA0], at4, 0x100)
            } else {
                cut([-0x180, 0x800], [0, 0x1480, 0x1080], at4, 0x60)
            }
        }
        12 => {
            if gated(st, 0x80) {
                return ArmDirection::HOLD;
            }
            st.creature_stage = Some(3);
            cut([0, 0x800], [0, 0x1200, 0x180], at4, 1)
        }
        13 => {
            let hold = gated(st, 0x70);
            ArmDirection { hold, ..shot13() }
        }
        14 => {
            if gated(st, 0x40) {
                return ArmDirection::HOLD;
            }
            st.creature_loop_release = true;
            st.creature_stage = Some(4);
            ArmDirection::PASS
        }
        15 => {
            if gated(st, 0xC0) {
                return ArmDirection::HOLD;
            }
            ArmDirection::PASS
        }
        16 => {
            let d = Some(drift(0, 0, 0, 0, -2 * D));
            if gated(st, 0x80) {
                return held(d);
            }
            st.creature_stage = Some(5);
            with_drift(cut([-0xA0, 0x980], [0, 0x1480, 0xD00], at4, 1), d)
        }
        18 | 21 => {
            let add = if phase == 18 { 0x100 } else { 0x40 };
            gate(gated(st, add))
        }
        19 => {
            let d = Some(drift(0, 0, 0, 0, 2 * D));
            if gated(st, 0x68) {
                return held(d);
            }
            st.creature_stage = Some(6);
            with_drift(cut([0x40, 0x7A0], [0, 0x1480, 0x980], at4, 1), d)
        }
        20 => {
            if gated(st, 0x28) {
                return ArmDirection {
                    hold: true,
                    ..shot20()
                };
            }
            cut([0x28, 0x8B8], [0, 0x1110, 0x1416], at4, 0x28)
        }
        22 => {
            // `lh v1,0x68(node); li v0,0x120; bne` at `0x801F7C80`: the arm
            // waits for the creature's cursor to stand on `0x120`. The
            // engine's cursor steps in its own increments, so the port waits
            // for it to reach the frame rather than to equal it.
            st.countdown.0 -= D;
            if frame_wait(st, phase, 0x120) {
                return ArmDirection::HOLD;
            }
            ArmDirection::PASS
        }
        23 | 24 => {
            if gated(st, 0x40) {
                return ArmDirection::HOLD;
            }
            if phase == 23 {
                cut([0x180, 0xF20], [0x400, 0x9A4, 0x732], at4, 1)
            } else {
                cut([0x194, 0x8B8], [0x400, -0x570, 0x1C4A], at4, 1)
            }
        }
        25 => {
            if gated(st, 0x10) {
                return ArmDirection::HOLD;
            }
            st.creature_loop_release = true;
            ArmDirection::PASS
        }
        26 => {
            if gated(st, 0x40) {
                return ArmDirection::HOLD;
            }
            cut([0, 0], [0, 0, 0], [0; 3], 1)
        }
        27 => {
            st.countdown.0 -= D;
            with_drift(ArmDirection::PASS, Some(drift(0, 0, 0, 0, 48 * D)))
        }
        28 => {
            let d = Some(drift(0, 0, 0, 0, 48 * D));
            if gated(st, 0x40) {
                return held(d);
            }
            with_drift(ArmDirection::PASS, d)
        }
        JEDO_HIT_ARM => {
            if gated(st, 0x100) {
                return ArmDirection::HOLD;
            }
            ArmDirection::PASS
        }
        JEDO_KILL_ARM => {
            if gated(st, 0x80) {
                return ArmDirection::HOLD;
            }
            ArmDirection::PASS
        }
        JEDO_LAST_ARM => {
            let d = Some(drift(0, 0, 0, 0, 4 * D));
            if counted(st, D) {
                return held(d);
            }
            st.yaw_base = 0;
            st.creature_loop_release = true;
            finish(caption(
                with_drift(ArmDirection::PASS, d),
                ModuleCaption::Cleared,
            ))
        }
        _ => ArmDirection::HOLD,
    }
}

// ---------------------------------------------------------------------------
// PROT 0921 - Iota
// ---------------------------------------------------------------------------

/// PROT 0921's hit arm: its pass sweeps the living monsters through
/// `FUN_801DD0AC(0x12, 7, seat)` (`0x801F7BE8`).
pub const IOTA_HIT_ARM: u8 = 10;

/// PROT 0921 (Iota): a cut on the stage origin at arm 0 arming the word
/// `0x801F8CB0 = scalar << 6`. Most arms open on the gate that holds on any
/// pass that finds the word positive (it drains it by `p` first), with the
/// arm's drift on those passes.
///
/// | arm | gate | camera / writes |
/// |---|---|---|
/// | 0 | - | a cut; word `= scalar << 6` |
/// | 1 | - | TR z `-= p` while the word drains |
/// | 2, 3 | the word, TR z `-= p` | word `= scalar << 6`; arm 3 seats the creature on the origin, a cut, `+= scalar * 0x118` |
/// | 4 | one drain | a `0x118`-frame push in, three fades |
/// | 5 | the word | a cut, the fades replaced by a fade out, clip `1`; `+= scalar << 6` |
/// | 6 | the word, TR z `+= p` | the spell name; `+= scalar * 180` |
/// | 7 | word (`bgtz`) | a cut, the attack name; `+= scalar * 192` |
/// | 8 | the word, yaw `+= p / 4` | a cut; `+= scalar << 7` |
/// | 9, 10 | the word; TR z `+= p / 2` every pass | a subtractive fade (9); the hit and a fade (10); `+= scalar << 7` |
/// | 11 | the word, then the row settled; TR z `+= p / 2` every pass | the caption cleared; done |
///
/// PORT: overlay_summon_iota_0921_801f6a08 (PROT 0921; the whole phase chain's gates, camera writes, captions, creature seat and clips - the monster-row re-seat and drain, part spawns and their kills not carried)
pub fn iota_direct(st: &mut ModuleCamState, phase: u8, _seats: ModuleCamSeats) -> ArmDirection {
    // `if (cd > 0) { cd -= p; <drift>; busy }`.
    let drained = |st: &mut ModuleCamState| -> bool {
        if st.countdown.0 > 0 {
            st.countdown.drain();
            true
        } else {
            false
        }
    };
    let z = |n: i32| Some(drift(0, 0, 0, 0, n));
    match phase {
        0 => {
            st.countdown.arm(6);
            cut([0, 0x800], [0, 0x400, 0x800], [0; 3], 1)
        }
        1 => {
            let d = drained(st).then_some(drift(0, 0, 0, 0, -P));
            with_drift(ArmDirection::PASS, d)
        }
        2 => {
            if drained(st) {
                return held(z(-P));
            }
            st.countdown.arm(6);
            ArmDirection::PASS
        }
        3 => {
            if drained(st) {
                return held(z(-P));
            }
            st.creature = Some(ModuleSeat::default());
            st.countdown.add(0x118);
            cut([0, 0x800], [0, 0x400, 0x4E20], [0; 3], 1)
        }
        4 => {
            st.countdown.drain();
            cut([-0x118, 0x800], [0, 0x400, 0x200], [0; 3], 0x118)
        }
        5 => {
            if drained(st) {
                return ArmDirection::HOLD;
            }
            st.creature_stage = Some(1);
            st.countdown.add(1 << 6);
            cut([0x80, 0x900], [0, 0x200, 0x600], [0; 3], 1)
        }
        6 => {
            if drained(st) {
                return held(z(P));
            }
            st.countdown.add(180);
            caption(ArmDirection::PASS, ModuleCaption::SpellName)
        }
        7 => {
            if counted(st, P) {
                return ArmDirection::HOLD;
            }
            st.countdown.add(192);
            caption(
                cut([0x80, -0xC0], [0, 0x200, 0x1000], [0, 0, -0x400], 1),
                ModuleCaption::AttackName,
            )
        }
        8 => {
            if drained(st) {
                return held(Some(drift(0, P / 4, 0, 0, 0)));
            }
            st.countdown.add(1 << 7);
            cut([0, 0], [0, 0x400, 0], [0; 3], 1)
        }
        9 | IOTA_HIT_ARM => {
            let d = z(P / 2);
            if drained(st) {
                return held(d);
            }
            st.countdown.add(1 << 7);
            with_drift(ArmDirection::PASS, d)
        }
        11 => {
            let d = z(P / 2);
            if drained(st) || !st.row_settled {
                return held(d);
            }
            finish(caption(
                with_drift(ArmDirection::PASS, d),
                ModuleCaption::Cleared,
            ))
        }
        _ => ArmDirection::HOLD,
    }
}

// ---------------------------------------------------------------------------
// PROT 0929 - Mule
// ---------------------------------------------------------------------------

/// PROT 0929's hit arm: its pass sweeps the living monsters through
/// `FUN_801DD0AC(0x10, 7, seat)` (`0x801F8758`). Arm 16's per-pass hits,
/// which cannot kill, are not carried.
pub const MULE_HIT_ARM: u8 = 18;

/// The parts each PROT 0929 arm halts on its pass (`ori 0x8` into the flag
/// word of a handle the module parked at `0x801F9A88..`), by record:
///
/// | arm | halts | site |
/// |---:|---|---|
/// | 12 | arm 3's bubble volley `0x801F917C` | `0x801F77A8..0x801F77D8` |
/// | 14 | arm 12's three creature-borne parts | `0x801F796C..0x801F79B8` |
/// | 16 | arm 15's three far parts, arm 14's three creature-borne parts | `0x801F8218..0x801F82A4` |
/// | 19 | arm 17's spray `0x801F9738` (handle `0x801F9AAC`) | `0x801F8A48..0x801F8A60` |
pub fn mule_arm_kills(phase: u8) -> &'static [u32] {
    match phase {
        12 => &[0x801F_917C],
        14 => &[0x801F_91C0, 0x801F_9220, 0x801F_9284],
        16 => &[
            0x801F_9344,
            0x801F_9398,
            0x801F_9498,
            0x801F_94E0,
            0x801F_9528,
        ],
        19 => &[0x801F_9738],
        _ => &[],
    }
}

/// The parts PROT 0929's arm 16 lets run out instead of halting: it zeroes
/// their op-`0x18` loop counter `+0x8C` (`0x801F82A8..0x801F82C0`), so each
/// leaves its loop at the next op `0x19` and halts. They are arm 14's two
/// CLUT-cell cyclers, the backdrop CLUT row's `0x801F95A8` and `0x801F9468`.
pub fn mule_arm_loop_ends(phase: u8) -> &'static [u32] {
    match phase {
        16 => &[0x801F_9468, 0x801F_95A8],
        _ => &[],
    }
}

/// PROT 0929 (Mule): its word `0x801F9A80` drains by `p`. From arm 5 on its
/// framings aim at `(0, 0, a)`, where `a` is the world anchor's Z
/// `0x80089120` that arms 1..4 push by `p / 2` a pass; the port models the
/// anchor on [`ModuleCamState::ramp`] from `0`. Arm 4 waits on the stage
/// word `ctx[+0x27A]` climbing `p / 4` a pass past `0x80`, which the port
/// models on the countdown.
///
/// | arm | gate | camera / writes |
/// |---|---|---|
/// | 0 | - | a cut on the origin |
/// | 1, 2 | the side-band applier stage `ctx[+0x276]` (1); the band timer (2) | the anchor `+= p / 2` while the band timer runs, arm 0's cut following it |
/// | 3, 4 | the stage word (4) | the anchor `+= p / 2` a pass, arm 0's cut following it |
/// | 5 | word | a `0x7E`-frame shot, the creature seated at `(0, -a)` facing `0x800`, clip `1`; `+= scalar * 126` |
/// | 6..8 | word | cuts; `+= scalar * 150` / `150` / `240` |
/// | 9 | word | a `0x3C`-frame shot, the spell name, clip `0`; `+= scalar * 60` |
/// | 10 | word | a `0xB4`-frame shot; `+= scalar * 300` |
/// | 11 | word | a cut, the attack name, the creature at `y = -0x100` facing `0`; `+= scalar * 208` |
/// | 12..14 | word; yaw `-= p / 8`, TR z `+= p / 2` (13, 14: TR z `+= p / 4`) | cuts, clip `2` (14); `+= scalar * 208` / `208` / `320` |
/// | 15 | word; TR x `-= p / 4`, TR y `-= p / 8`, TR z `+= p`, yaw `+= p / 4` | a fade in after `0x100`; `+= scalar * 320` |
/// | 16 | word; TR x `+= 2 p`, TR y `-= p / 4` | a cut on the target (a group by its centroid); `+= scalar << 5` |
/// | 17 | word | the fade out; `+= scalar << 7` |
/// | 18 | word; pitch `-= p / 8`, yaw `+= p / 8`, TR z `+= p / 4` | the hit, a fade; `+= scalar << 7` |
/// | 19 | the word, then the row settled; yaw `+= p / 8`, TR z `+= p` while the word runs | `0xFF` |
/// | 0xFF | - | the caption cleared, `ctx[+0x6DA] = 0`, the loop counter cleared; done |
///
/// PORT: overlay_summon_mule_0929_801f69fc (PROT 0929; the whole phase chain's gates, camera writes, captions, creature seat and clips - the monster-row writes, the wave and its per-pass hits, part spawns and the stage bytes not carried)
pub fn mule_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    // Arms 1..4 push the anchor under arm 0's landed cut, and the view
    // builder reads the global every frame, so the shot's focus walks with
    // it: re-armed as a one-frame cut on the moved anchor each pass.
    let anchored = |a: i32, hold: bool| ArmDirection {
        hold,
        ..cut([0, 0], [0, 0x500, 0x800], [0, 0, a], 1)
    };
    let gated = |st: &mut ModuleCamState, add: i32| -> bool {
        if counted(st, P) {
            return true;
        }
        st.countdown.0 += add;
        false
    };
    let a = st.ramp;
    let at = [0, 0, a];
    let creature = st.creature.unwrap_or_default();
    match phase {
        0 => {
            st.ramp = 0;
            cut([0, 0], [0, 0x500, 0x800], [0; 3], 1)
        }
        1 | 2 => {
            if seats.band_timer != 0 {
                st.ramp += P / 2;
            }
            // Arm 1 returns busy while the side-band applier stage
            // `ctx[+0x276]` is up (`0x801F6D94..0x801F6DA8`,
            // [`super::sideband_wait_arm`]), arm 2 while the band timer runs.
            let hold = if phase == 1 {
                st.sideband_busy
            } else {
                seats.band_timer != 0
            };
            anchored(st.ramp, hold)
        }
        3 => {
            st.ramp += P / 2;
            st.countdown.0 = P / 4;
            anchored(st.ramp, false)
        }
        4 => {
            st.ramp += P / 2;
            st.countdown.0 += P / 4;
            if st.countdown.0 < 0x81 {
                return anchored(st.ramp, true);
            }
            st.countdown.0 = 24 * S;
            anchored(st.ramp, false)
        }
        5 => {
            if gated(st, 126 * S) {
                return ArmDirection::HOLD;
            }
            st.creature = Some(ModuleSeat {
                x: 0,
                y: 0,
                z: (-a) as i16,
                facing: 0x800,
            });
            st.creature_stage = Some(1);
            cut([-0x100, 0], [0, 0x460, 0x800], at, 0x7E)
        }
        6 => {
            if gated(st, 150 * S) {
                return ArmDirection::HOLD;
            }
            cut([0, 0x400], [-0xE00, 0xD00, 0x400], at, 1)
        }
        7 => {
            if gated(st, 150 * S) {
                return ArmDirection::HOLD;
            }
            cut([0, 0x800], [0, 0xB00, 0xC00], at, 1)
        }
        8 => {
            if gated(st, 240 * S) {
                return ArmDirection::HOLD;
            }
            cut([-0x180, 0], [0, 0x400, 0x2E0], at, 1)
        }
        9 => {
            if gated(st, 60 * S) {
                return ArmDirection::HOLD;
            }
            st.creature_stage = Some(0);
            caption(
                cut([0, -0x60], [0, 0x600, 0x700], at, 0x3C),
                ModuleCaption::SpellName,
            )
        }
        10 => {
            if gated(st, 300 * S) {
                return ArmDirection::HOLD;
            }
            cut([0, -0x180], [0, 0x400, 0x400], at, 0xB4)
        }
        11 => {
            if gated(st, 208 * S) {
                return ArmDirection::HOLD;
            }
            st.creature = Some(ModuleSeat {
                y: -0x100,
                facing: 0,
                ..creature
            });
            caption(
                cut([0x80, 0x60], [0, 0x500, 0xC00], [0; 3], 1),
                ModuleCaption::AttackName,
            )
        }
        12 => {
            let d = Some(drift(0, -(P / 8), 0, 0, P / 2));
            if gated(st, 208 * S) {
                return held(d);
            }
            with_drift(cut([0, 0x800], [0, 0x800, 0x400], on(creature), 1), d)
        }
        13 => {
            let d = Some(drift(0, 0, 0, 0, P / 4));
            if gated(st, 208 * S) {
                return held(d);
            }
            with_drift(cut([0x40, 0x800], [0, 0x400, 0x800], [0; 3], 1), d)
        }
        14 => {
            let d = Some(drift(0, 0, 0, 0, P / 4));
            if gated(st, 320 * S) {
                return held(d);
            }
            st.creature_stage = Some(2);
            with_drift(cut([0, 0x980], [0x140, 0x880, 0x200], on(creature), 1), d)
        }
        15 => {
            let d = Some(drift(0, P / 4, -(P / 4), -(P / 8), P));
            if gated(st, 320 * S) {
                return held(d);
            }
            with_drift(ArmDirection::PASS, d)
        }
        16 => {
            let d = Some(drift(0, 0, 2 * P, -(P / 4), 0));
            if gated(st, S << 5) {
                return held(d);
            }
            let shot = match (st.target_code, st.target_aim) {
                (Some(code), Some(g)) if code >= 8 => cut(
                    [0x80, 0x700],
                    [0, 0x600, (10 * i32::from(g.extent)).min(0x1800)],
                    [i32::from(g.centroid_x), 0, i32::from(g.centroid_z)],
                    1,
                ),
                // One target: the focus is its `+0x34` / `+0x38` as they
                // stand, not negated (`0x801F8314..0x801F8348`), and the
                // depth ten times its body radius `*(+0x22C) + 0x58`, capped
                // at `0x1800` (`0x801F834C..0x801F8394`).
                _ => cut(
                    [0x80, 0x700],
                    [0, 0x600, (10 * i32::from(seats.victim_radius)).min(0x1800)],
                    [i32::from(seats.victim.x), 0, i32::from(seats.victim.z)],
                    1,
                ),
            };
            with_drift(shot, d)
        }
        17 => {
            if gated(st, S << 7) {
                return ArmDirection::HOLD;
            }
            ArmDirection::PASS
        }
        MULE_HIT_ARM => {
            let d = Some(drift(-(P / 8), P / 8, 0, 0, P / 4));
            if gated(st, S << 7) {
                return held(d);
            }
            with_drift(ArmDirection::PASS, d)
        }
        19 => {
            let mut d = drift(0, P / 8, 0, 0, 0);
            if st.countdown.0 > 0 {
                d.tr_z = P as i16;
                st.countdown.0 -= P;
            }
            if !st.row_settled || st.countdown.0 > 0 {
                return held(Some(d));
            }
            finish(with_drift(ArmDirection::PASS, Some(d)))
        }
        0xFF => {
            st.yaw_base = 0;
            st.creature_loop_release = true;
            caption(ArmDirection::HOLD, ModuleCaption::Cleared)
        }
        _ => ArmDirection::HOLD,
    }
}

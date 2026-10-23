//! The **arm holds** of six summon-band modules whose tick bodies are ported
//! but whose countdowns were not: PROT 0918 (Kemaro), 0919 (Spoon), 0922
//! (Puera), 0924 (Ultimate Rave), 0925 (Spikefish) and 0927 (Juggernaut).
//!
//! Each of these images keeps one countdown word of its own and opens most
//! arms on it: drain the word, store it back, and return busy (`bgtz` to the
//! epilogue) while it stays positive. The tick that sees it at or below zero
//! runs the arm's writes, re-arms the word and advances the phase. Without
//! the gate every arm took one tick and a summon that holds the screen for
//! twenty seconds in retail was over in thirty frames.
//!
//! The director here is that gate: it holds the phase while the arm's wait
//! lasts and applies the arm's re-arm on the tick it lets the tick body
//! run. For PROT 0919 and 0924 it is nothing else and arms no camera (their
//! shot arms are not ported, so the battle camera keeps the band's framing);
//! PROT 0925's adds the body's camera writes ([`spikefish_direct`]). PROT
//! 0918, 0922 and 0927 run full directors over these tables
//! (`super::kemaro_direct`, `super::puera_direct`,
//! `super::juggernaut_direct`) that add their camera, drift, spawn and
//! caption writes. The phase walk,
//! staging and outcome stay in the tick bodies in
//! [`crate::cast_module_ticks`] and [`crate::cast_module_ticks::SPOON_CHAIN`],
//! which own the arms that store a literal phase instead of `phase + 1`.
//!
//! ## Gate shapes
//!
//! | shape | retail form | engine |
//! |---|---|---|
//! | [`Gate::Count`] | `cd -= d; sw; bgtz busy` | drain, hold while `> 0` |
//! | [`Gate::CountPositive`] | `blez skip; cd -= d; sw; bgtz busy` | drain only a positive word |
//! | [`Gate::Drain`] | `blez skip; cd -= d; sw` with no `bgtz` | one drain, never holds |
//! | [`Gate::RampUp`] | a model word counted up to a limit | hold while below it |
//! | [`Gate::RampDown`] | a model word counted down, the countdown drained beside it | hold while above the step |
//! | [`Gate::Settle`] | [`Gate::CountPositive`], then every monster seat settled | hold while either waits |
//!
//! The drain `d` is the retail per-frame expression with the frame step at
//! `1`: the engine ticks once per vsync, so `scalar * step` drains
//! [`SPEED_SCALAR`] a tick and a bare `step` drains `1`.
//!
//! ## Waits the engine has nothing to wait on
//!
//! Every module's first arms also wait on the band's own stream: the
//! `ctx[+0x276]` load flag and `FUN_8003F2B8(1)`. The engine's creature is
//! resident at once, so those arms pass on their first tick.
//!
//! ## Measured against retail
//!
//! The cast-sequence oracle's retail captures (`_t3` rows of spells `0x90`,
//! `0x94`, `0x99`) carry each module phase's length in vsyncs. Every arm
//! retail runs at a frame step of `1` to `3` matches its countdown to within
//! a battle frame (PROT 0918's 128 / 96 / 128 / 180, 0922's 64 / 78 / 18 /
//! 15 / 39 / 60, 0927's 128 / 128 / 64 / 64). Arms retail runs at step `4`
//! read longer than their countdown: a frame that takes more than four
//! vsyncs still drains four vsyncs' worth, so those arms carry the lag of a
//! scene the hardware cannot draw at speed. The engine keeps the countdown.
//!
//! Provenance: the disassembly of each module at slot-B base `0x801F69D8`
//! (`see ghidra/scripts/funcs/overlay_summon_<label>_<entry>_<va>.txt`; PROT
//! 0924 from its extracted image, which has no dump); the per-row comments
//! cite the gate and the re-arm stores.

use super::{
    ArmDirection, CaptureDrift, ModuleCamSeats, ModuleCamState, ModuleProfile, ModuleShot,
    SPEED_SCALAR,
};

/// One arm's wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// No countdown gate: the arm runs on its first tick.
    Open,
    /// Drain by `d`, hold while the word stays positive.
    Count(i32),
    /// Drain by `d` only while the word is positive, hold while it stays so.
    CountPositive(i32),
    /// Drain a positive word by `d` and run on; the arm's other wait (the
    /// stream flag) is the one that holds.
    Drain(i32),
    /// Add `per_tick` to the ramp word, hold while it is below `limit`.
    RampUp { per_tick: i32, limit: i32 },
    /// Pass once the ramp word is below `per_tick`, else take `per_tick` off
    /// it and hold; the countdown drains `drain` on every tick of the arm.
    RampDown { per_tick: i32, drain: i32 },
    /// [`Gate::CountPositive`], then hold until every monster seat has
    /// settled ([`ModuleCamState::row_settled`]).
    Settle(i32),
}

/// What an arm stores into the countdown word on the tick it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rearm {
    /// `sw <value>, countdown`.
    Set(i32),
    /// `countdown += <value>`, on top of the drain's residue.
    Add(i32),
}

/// One arm's hold half.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoldArm {
    /// The module phase `ctx[+0x279]` the arm runs on.
    pub arm: u8,
    /// Its wait.
    pub gate: Gate,
    /// The countdown store it makes as it passes.
    pub rearm: Option<Rearm>,
    /// The ramp word it seeds as it passes, for the next arm's ramp gate.
    pub ramp: Option<i32>,
}

/// Ticks a [`Gate::Settle`] waits on the row before it lets the arm run
/// anyway: an engine seat that never reads idle must not park the band.
pub const SETTLE_TICK_LIMIT: i32 = 600;

const S: i32 = SPEED_SCALAR;
/// Per-tick drain of the `scalar * step` product.
const P: i32 = SPEED_SCALAR;
/// Per-tick drain of the bare `step`.
const T: i32 = 1;

const fn arm(arm: u8, gate: Gate, rearm: Option<Rearm>) -> HoldArm {
    HoldArm {
        arm,
        gate,
        rearm,
        ramp: None,
    }
}

const fn seeds(mut a: HoldArm, ramp: i32) -> HoldArm {
    a.ramp = Some(ramp);
    a
}

const fn set(v: i32) -> Option<Rearm> {
    Some(Rearm::Set(v))
}

const fn add(v: i32) -> Option<Rearm> {
    Some(Rearm::Add(v))
}

/// PROT 0918 (Kemaro, `0x90`), `FUN_801F6C70`, word `0x801F92E4`, product
/// drain. Arm 0 seeds `scalar << 7` (`0x801F6EC0`). Arm 2 waits on
/// `ctx[+0x276]`, arm 3 on the stream and stores `scalar * 0x60`
/// (`0x801F7018`, in the branch's delay slot, so both paths store it). Arm
/// 4's gate (`0x801F719C`) seats the creature and adds `scalar << 7`
/// (`0x801F7220`); it also zeroes the creature record's `+0x72`, which arm 6
/// counts up by `4 * scalar * step` a frame to `0x1000` (`0x801F7338..73A8`)
/// before it stores `scalar * 0xB4` (`0x801F746C`). Arm 7 stores
/// `scalar * 0x1C0` (`0x801F75B0`). Arm 17 has no gate. Arm `0x13` stores
/// `0xFF` (the tick body's [`crate::cast_module_ticks::kemaro_tick`]).
pub const KEMARO: &[HoldArm] = &[
    arm(0, Gate::Open, set(S << 7)),
    arm(1, Gate::Count(P), None),
    arm(3, Gate::Open, set(S * 0x60)),
    seeds(arm(4, Gate::Count(P), add(S << 7)), 0),
    arm(5, Gate::Count(P), None),
    arm(
        6,
        Gate::RampUp {
            per_tick: 4 * S,
            limit: 0x1000,
        },
        set(S * 0xB4),
    ),
    arm(7, Gate::Count(P), set(S * 0x1C0)),
    arm(8, Gate::Count(P), add(S << 6)),
    arm(9, Gate::Count(P), add(S << 8)),
    arm(10, Gate::Count(P), add(S << 6)),
    arm(11, Gate::Count(P), add(S << 7)),
    arm(12, Gate::Count(P), add(S * 0x60)),
    arm(13, Gate::Count(P), add(S * 30)),
    arm(14, Gate::Count(P), add(S << 4)),
    arm(15, Gate::Count(P), add(S << 2)),
    arm(16, Gate::Count(P), None),
    arm(17, Gate::Open, add(S << 7)),
    arm(0x12, Gate::Count(P), add(S * 0x60)),
    arm(0x13, Gate::Count(P), None),
];

/// PROT 0919 (Spoon, `0x91`), `FUN_801F69D8`, word `0x801F8E5C`, product
/// drain. Arm 0 seeds `scalar << 7` (`0x801F6C8C`); arm 1 drains a positive
/// word with no gate of its own and waits on `ctx[+0x276]`; arm 2 drains,
/// waits on the stream, holds (`0x801F6D80`), seats the creature and stores
/// `scalar * 0x50` (`0x801F6DA4`). Gates at `0x801F6EB8` / `6FB4` / `7594` /
/// `7820` / `7C64` / `852C` re-arm `scalar << 7`, `scalar << 5`,
/// `scalar * 0xC0`, `scalar << 7`, `scalar * 0x140` (`0x801F6F74`, `70C0`,
/// `75B8`, `7890`, `8474`); arm 8 stores `0xFF` (`0x801F853C`).
pub const SPOON: &[HoldArm] = &[
    arm(0, Gate::Open, set(S << 7)),
    arm(1, Gate::Drain(P), None),
    arm(2, Gate::CountPositive(P), set(S * 0x50)),
    arm(3, Gate::Count(P), add(S << 7)),
    arm(4, Gate::Count(P), add(S << 5)),
    arm(5, Gate::Count(P), add(S * 0xC0)),
    arm(6, Gate::Count(P), add(S << 7)),
    arm(7, Gate::Count(P), add(S * 0x140)),
    arm(8, Gate::Count(P), None),
];

/// PROT 0922 (Puera, `0x94`), `FUN_801F6A3C`, word `0x801FA0B8`, product
/// drain; every gate drains only a positive word (`blez` ahead of the
/// `subu`). Arm 0 seeds `scalar << 6` (`0x801F6C64`); arm 1 drains with no
/// gate of its own and waits on `ctx[+0x276]`; arm 2 holds
/// (`0x801F6D44`), waits on the stream and stores `scalar * 0x4E`
/// (`0x801F6D9C`). Arm 7 parks the creature's clip frame `+0x68` at `0x1D0`
/// (`0x801F7114`), which arm 8 walks back down by `scalar * step` a frame
/// while it drains the countdown with no gate (`0x801F7118..0x801F7178`).
/// Arm 20 stores `phase + 2` (`0x801F87D8`), so arm 21 is never reached.
/// Arm 24 waits for every monster seat to settle - a living one idle, a dead
/// one faded (`0x801F8F68..0x801F8FF4`).
pub const PUERA: &[HoldArm] = &[
    arm(0, Gate::Open, set(S << 6)),
    arm(1, Gate::Drain(P), None),
    arm(2, Gate::CountPositive(P), set(S * 0x4E)),
    arm(3, Gate::CountPositive(P), add(S << 4)),
    arm(4, Gate::CountPositive(P), add(S << 4)),
    arm(5, Gate::CountPositive(P), add(S * 0x28)),
    arm(6, Gate::CountPositive(P), add(S * 0x3C)),
    seeds(arm(7, Gate::CountPositive(P), add(S * 0x78)), 0x1D0),
    arm(
        8,
        Gate::RampDown {
            per_tick: S,
            drain: P,
        },
        None,
    ),
    arm(9, Gate::CountPositive(P), add(S << 5)),
    arm(10, Gate::CountPositive(P), add(S << 6)),
    arm(11, Gate::CountPositive(P), add(S << 6)),
    arm(12, Gate::CountPositive(P), add(S << 5)),
    arm(13, Gate::CountPositive(P), add(S << 6)),
    arm(14, Gate::CountPositive(P), add(S << 6)),
    arm(15, Gate::CountPositive(P), add(S << 7)),
    arm(16, Gate::CountPositive(P), add(S << 7)),
    arm(17, Gate::CountPositive(P), add(S * 0x50)),
    arm(18, Gate::CountPositive(P), add(S << 6)),
    arm(19, Gate::CountPositive(P), add(S << 6)),
    arm(20, Gate::CountPositive(P), add(S << 5)),
    arm(22, Gate::CountPositive(P), add(S * 0xC0)),
    arm(23, Gate::CountPositive(P), add(S * 0xA0)),
    arm(24, Gate::Settle(P), None),
];

/// PROT 0924 (Ultimate Rave, `0x96`), `FUN_801F6A18`, word `0x801F7C48`,
/// bare-step drain. Arm 0 seeds `0x40` (`0x801F6B08`); arm 1 drains a
/// positive word and waits on `ctx[+0x276]`; arm 2 holds (`0x801F6CE0`),
/// waits on the stream, seats the creature and stores `0x108`
/// (`0x801F6D5C`). Gates at `0x801F6E90` / `6F80` / `709C` / `7280` / `7378`
/// / `7440` / `74D4` / `75C0` add `0x100`, `0x100`, `0x100`, `0x80`, `0x40`,
/// `0xC0`, `0xC0`, `0xC0`; arm 11's gate (`0x801F77C8`) clears the busy
/// register.
pub const ULTIMATE_RAVE: &[HoldArm] = &[
    arm(0, Gate::Open, set(0x40)),
    arm(1, Gate::Drain(T), None),
    arm(2, Gate::CountPositive(T), set(0x108)),
    arm(3, Gate::Count(T), add(0x100)),
    arm(4, Gate::Count(T), add(0x100)),
    arm(5, Gate::Count(T), add(0x100)),
    arm(6, Gate::Count(T), add(0x80)),
    arm(7, Gate::Count(T), add(0x40)),
    arm(8, Gate::Count(T), add(0xC0)),
    arm(9, Gate::Count(T), add(0xC0)),
    arm(10, Gate::Count(T), add(0xC0)),
    arm(11, Gate::Count(T), None),
];

/// PROT 0925 (Spikefish, `0x97`), `FUN_801F6A00`, word `0x801F800C`,
/// bare-step drain. Arm 2 waits on the stream, seats the creature and
/// stores `0x100` (`0x801F6DDC`); gates at `0x801F6E40` / `6F10` / `7044` /
/// `71A0` / `7208` / `7380` add `0xB4`, `0x200`, `0x80`, `0x80`, `0xC0`,
/// `0x80`. Arm 9 has no gate and clears the busy register (`0x801F7AA0`).
pub const SPIKEFISH: &[HoldArm] = &[
    arm(2, Gate::Open, set(0x100)),
    arm(3, Gate::Count(T), add(0xB4)),
    arm(4, Gate::Count(T), add(0x200)),
    arm(5, Gate::Count(T), add(0x80)),
    arm(6, Gate::Count(T), add(0x80)),
    arm(7, Gate::Count(T), add(0xC0)),
    arm(8, Gate::Count(T), add(0x80)),
];

/// PROT 0927 (Juggernaut, `0x99`), `FUN_801F6A84`, word `0x801F9C8C`,
/// bare-step drain. Arm 1 waits on `ctx[+0x276]`; arm 2 waits on the
/// stream, seats the creature, stores `0x80` (`0x801F6DC4`) and the phase
/// `0x0B` (`0x801F6F6C`) - arms 3..10 are never reached. Gates at
/// `0x801F70D4` / `7194` / `7218` / `72D8` / `7304` / `7360` / `738C` /
/// `73F0` / `7568` / `764C` / `76F0` add `0x80`, `0x80`, `0x80`, `0x80`,
/// `0x80`, `0x40`, `0x40`, `0x80`, `0x70`, `0xB4`, `0x100`; arm 21 stores
/// `phase + 2` (`0x801F77C4` and the shared `+1`), so arm 22 is never
/// reached. Gates at `0x801F7858` / `7B48` / `7B74` / `7BF4` / `8000` add
/// `0x180`, `0x80`, `0x80`, `0x100`, `0x80`; arm 28's gate (`0x801F82C4`)
/// clears the busy register.
pub const JUGGERNAUT: &[HoldArm] = &[
    arm(2, Gate::Open, set(0x80)),
    arm(11, Gate::Count(T), add(0x80)),
    arm(12, Gate::Count(T), add(0x80)),
    arm(13, Gate::Count(T), add(0x80)),
    arm(14, Gate::Count(T), add(0x80)),
    arm(15, Gate::Count(T), add(0x80)),
    arm(16, Gate::Count(T), add(0x40)),
    arm(17, Gate::Count(T), add(0x40)),
    arm(18, Gate::Count(T), add(0x80)),
    arm(19, Gate::Count(T), add(0x70)),
    arm(20, Gate::Count(T), add(0xB4)),
    arm(21, Gate::Count(T), add(0x100)),
    arm(23, Gate::Count(T), add(0x180)),
    arm(24, Gate::Count(T), add(0x80)),
    arm(25, Gate::Count(T), add(0x80)),
    arm(26, Gate::Count(T), add(0x100)),
    arm(27, Gate::Count(T), add(0x80)),
    arm(28, Gate::Count(T), None),
];

/// The hold table of a summon module this file covers.
pub fn summon_holds(prot_entry: u32) -> Option<&'static [HoldArm]> {
    Some(match prot_entry {
        918 => KEMARO,
        919 => SPOON,
        922 => PUERA,
        924 => ULTIMATE_RAVE,
        925 => SPIKEFISH,
        927 => JUGGERNAUT,
        _ => return None,
    })
}

/// One tick of `table`'s arm for `phase`: `true` while it holds. On the tick
/// it lets the arm run it applies the arm's re-arm and ramp seed.
pub fn hold_step(table: &[HoldArm], st: &mut ModuleCamState, phase: u8) -> bool {
    let Some(a) = table.iter().find(|a| a.arm == phase) else {
        return false;
    };
    let cd = &mut st.countdown.0;
    let held = match a.gate {
        Gate::Open => false,
        Gate::Count(d) => {
            *cd -= d;
            *cd > 0
        }
        Gate::CountPositive(d) => {
            if *cd > 0 {
                *cd -= d;
            }
            *cd > 0
        }
        Gate::Drain(d) => {
            if *cd > 0 {
                *cd -= d;
            }
            false
        }
        Gate::RampUp { per_tick, limit } => {
            st.ramp += per_tick;
            st.ramp < limit
        }
        Gate::RampDown { per_tick, drain } => {
            *cd -= drain;
            if st.ramp < per_tick {
                false
            } else {
                st.ramp -= per_tick;
                true
            }
        }
        Gate::Settle(d) => {
            if *cd > 0 {
                *cd -= d;
            }
            if *cd > 0 {
                true
            } else {
                // The ramp word counts the ticks spent on the row.
                st.ramp += 1;
                !st.row_settled && st.ramp < SETTLE_TICK_LIMIT
            }
        }
    };
    if held {
        return true;
    }
    match a.rearm {
        Some(Rearm::Set(v)) => st.countdown.0 = v,
        Some(Rearm::Add(v)) => st.countdown.0 += v,
        None => {}
    }
    st.ramp = a.ramp.unwrap_or(0);
    false
}

fn direct(table: &[HoldArm], st: &mut ModuleCamState, phase: u8) -> ArmDirection {
    super::gate(hold_step(table, st, phase))
}

/// PROT 0919's director: [`SPOON`].
pub fn spoon_direct(st: &mut ModuleCamState, phase: u8, _: ModuleCamSeats) -> ArmDirection {
    direct(SPOON, st, phase)
}

/// PROT 0924's director: [`ULTIMATE_RAVE`].
pub fn ultimate_rave_direct(st: &mut ModuleCamState, phase: u8, _: ModuleCamSeats) -> ArmDirection {
    direct(ULTIMATE_RAVE, st, phase)
}

/// PROT 0925's director: the holds of [`SPIKEFISH`] plus the body's camera
/// writes. Every shot is a `FUN_801D829C` call on a zeroed stack trio with
/// immediates over it, so its focus is the stage origin; every drift is
/// the bare frame step `*(0x1F800393)` (the `lbu 0x7F` off `0x1F800314`)
/// times a constant, one vsync's worth a tick.
///
/// | arm | every pass | on the pass |
/// |---|---|---|
/// | 0 (`0x801F6AB0`) | - | cut pitch `0`, yaw `0`, TR `(0, 0x500, 0x300)` |
/// | 3 (`0x801F6E0C..0x801F6E38`) | TR y `-2`, TR z `+8` | - |
/// | 5 (`0x801F6FAC..0x801F7014`) | TR y `+12`, TR z `+8`, yaw `+8`, pitch `-1` | cut pitch `0x80`, yaw `0`, TR `(0, 0x1400, 0x4000)` (`0x801F7078`) |
/// | 6 (`0x801F7180`, before the gate) | a `0x40`-frame shot pitch `0`, yaw `0`, TR `(0, 0x2400, 0x2400)` | - |
/// | 8 (`0x801F7364`, before the gate) | a `0x20`-frame shot pitch `0`, yaw `0`, TR `(0, 0x2400, 0x4E20)` | - |
///
/// Not carried: arm 5's growth of the creature's render scale (`+0x72 +=
/// 10 * step` on its model node, `0x801F7018..0x801F7030`) and arm 8's flee
/// staging, whose reverse-angle tween (`0x801F78C8`) belongs to the run
/// band it hands the action to.
///
/// PORT: overlay_summon_spikefish_0925_801f6a00 (PROT 0925; camera arms 0, 3, 5, 6 and 8)
pub fn spikefish_direct(st: &mut ModuleCamState, phase: u8, _: ModuleCamSeats) -> ArmDirection {
    let held = hold_step(SPIKEFISH, st, phase);
    let mut base = super::gate(held);
    let shot = |pitch: i16, tr: [i16; 3], frames: u16| ModuleShot {
        angles: [pitch, 0, 0],
        tr,
        focus: [0; 3],
        frames,
    };
    let drift = |pitch: i32, yaw: i32, tr_y: i32, tr_z: i32| CaptureDrift {
        pitch: pitch as i16,
        yaw: yaw as i16,
        tr_y: tr_y as i16,
        tr_z: tr_z as i16,
        tr_x: 0,
    };
    match phase {
        0 if !held => base.shot = Some(shot(0, [0, 0x500, 0x300], 1)),
        3 => base.drift = Some(drift(0, 0, -2 * T, 8 * T)),
        5 => {
            base.drift = Some(drift(-T, 8 * T, 12 * T, 8 * T));
            if !held {
                base.shot = Some(shot(0x80, [0, 0x1400, 0x4000], 1));
            }
        }
        6 => base.shot = Some(shot(0, [0, 0x2400, 0x2400], 0x40)),
        8 => base.shot = Some(shot(0, [0, 0x2400, 0x4E20], 0x20)),
        _ => {}
    }
    base
}

/// The directed profile of a module this file covers: its holds pace the
/// band (`0x36` waits on the module's return), the director runs beside the
/// ported tick body and never owns the phase, and the outcome lands once the
/// phase has passed `hit_arm`. `seat_arm` is the phase after the arm that
/// calls `FUN_801F19EC` where that arm seats only on its pass (after its own
/// countdown or stream wait), so the creature appears when retail's does.
pub fn summon_hold_profile(prot_entry: u32) -> Option<ModuleProfile> {
    let (direct, hit_arm, seat_arm): (super::ModuleDirector, u8, u8) = match prot_entry {
        // The damage arm `0x12` (`0x801F87A4`); the seat at arm 4's pass
        // (`0x801F71E0`).
        918 => (
            super::kemaro_direct,
            crate::cast_module_ticks::KEMARO_DAMAGE_PHASE,
            5,
        ),
        // The heal arm; the seat at arm 2's pass (`0x801F6DA8`).
        919 => (spoon_direct, crate::seru_magic_ticks::SPOON_HEAL_ARM, 3),
        // The damage arm 23 (`0x801F8E1C`); the seat at arm 3's pass
        // (`0x801F6DFC`).
        922 => (super::puera_direct, PUERA_DAMAGE_ARM, 4),
        // The finale arm; the seat at arm 2's pass (`0x801F6D24`).
        924 => (
            ultimate_rave_direct,
            crate::cast_module_ticks::ULTIMATE_RAVE_FINALE_ARM,
            3,
        ),
        // The outcome arm; arm 2 seats on its stream poll alone.
        925 => (
            spikefish_direct,
            crate::cast_module_ticks::SPIKEFISH_OUTCOME_ARM,
            2,
        ),
        // The damage arm 26 (`0x801F7E0C`); arm 2 seats on its stream poll.
        927 => (super::juggernaut_direct, JUGGERNAUT_DAMAGE_ARM, 2),
        _ => return None,
    };
    Some(ModuleProfile {
        direct,
        hit_arm: Some(hit_arm),
        walk_arm: None,
        owns_phase: false,
        // A director that reports its arms' spawn calls (PROT 0918's, 0922's, 0927's) seats
        // them per arm; the others leave the stager to seat the record set.
        stages_spawns: matches!(prot_entry, 918 | 922 | 927),
        seat_arm: Some(seat_arm),
        director_body: false,
    })
}

/// The countdown gate's `bgtz` of an arm whose decoded presentation sites
/// the decoder could not split around it
/// (`legaia_overlay_images::cast_presentation::SiteTiming::Every`): a site
/// past this VA runs only on the tick the gate lets through, the way the
/// `bgtz` to the epilogue leaves it. The arms are the ones in these modules
/// with such a site past their gate: PROT 0919 arm 7 (`0x801F7C64`; its
/// three fades, the kills and the CLUT loads), PROT 0922 arms 14
/// (`0x801F7AA0`) and 15 (`0x801F7CAC`; the wash alpha store past the
/// ramp), and PROT 0930's last arm 11 (`0x801F7CAC`): an arm with no phase
/// write reads as `Every`, but its pool flush and fade-out run once the
/// countdown lets the arm finish, and on the first tick they ended the two
/// held whites arm 10 parked a whole arm early.
pub fn gate_va(prot_entry: u32, arm: u8) -> Option<u32> {
    Some(match (prot_entry, arm) {
        (919, 7) => 0x801F_7C64,
        (930, 0x0B) => 0x801F_7CAC,
        (922, 14) => 0x801F_7AA0,
        (922, 15) => 0x801F_7CAC,
        _ => return None,
    })
}

/// PROT 0922's damage arm: the `FUN_801DD0AC` site `0x801F8E1C` is in arm 23.
pub const PUERA_DAMAGE_ARM: u8 = 23;
/// PROT 0927's damage arm: the `FUN_801DD0AC` site `0x801F7E0C` is in arm 26.
pub const JUGGERNAUT_DAMAGE_ARM: u8 = 26;

#[cfg(test)]
mod tests {
    use super::*;

    /// Walk `table` one tick per call, the body advancing one arm per pass
    /// (or along `jumps`), and return each visited arm's dwell in ticks.
    fn dwells(table: &[HoldArm], first: u8, last: u8, jumps: &[(u8, u8)]) -> Vec<(u8, u32)> {
        let mut st = ModuleCamState {
            row_settled: true,
            ..Default::default()
        };
        let mut out = Vec::new();
        let mut phase = first;
        loop {
            let mut ticks = 0;
            while {
                ticks += 1;
                hold_step(table, &mut st, phase)
            } {}
            out.push((phase, ticks));
            if phase == last {
                break;
            }
            phase = jumps
                .iter()
                .find(|(from, _)| *from == phase)
                .map_or(phase + 1, |(_, to)| *to);
        }
        out
    }

    fn dwell_of(d: &[(u8, u32)], arm: u8) -> u32 {
        d.iter().find(|(a, _)| *a == arm).map(|(_, t)| *t).unwrap()
    }

    /// PROT 0918 against the `0x90` capture's step-`1..3` arms: arm 1 128
    /// vsyncs, 4 96, 5 131, 6 129, 7 183, 13 97, 18 128, 19 97.
    #[test]
    fn kemaro_holds_its_captured_arms() {
        let d = dwells(KEMARO, 0, 0x13, &[]);
        assert_eq!(dwell_of(&d, 1), 128);
        assert_eq!(dwell_of(&d, 4), 96);
        assert_eq!(dwell_of(&d, 5), 128);
        assert_eq!(dwell_of(&d, 6), 128);
        assert_eq!(dwell_of(&d, 7), 180);
        assert_eq!(dwell_of(&d, 13), 96);
        assert_eq!(dwell_of(&d, 14), 30);
        assert_eq!(dwell_of(&d, 17), 1);
        assert_eq!(dwell_of(&d, 0x12), 128);
        assert_eq!(dwell_of(&d, 0x13), 96);
    }

    /// PROT 0922 against the `0x94` capture: arms 1 + 2 64 vsyncs, 3 78, 4
    /// 18, 5 15, 6 39, 7 60, 8 60, 9 60, 10 33, 11 63, 12 64, 13 32, 24 161.
    #[test]
    fn puera_holds_its_captured_arms() {
        let d = dwells(PUERA, 0, 24, &[(20, 22)]);
        assert_eq!(dwell_of(&d, 1) + dwell_of(&d, 2), 64);
        assert_eq!(dwell_of(&d, 3), 78);
        assert_eq!(dwell_of(&d, 4), 16);
        assert_eq!(dwell_of(&d, 5), 16);
        assert_eq!(dwell_of(&d, 6), 40);
        assert_eq!(dwell_of(&d, 7), 60);
        assert_eq!(dwell_of(&d, 8), 59);
        assert_eq!(dwell_of(&d, 9), 61);
        assert_eq!(dwell_of(&d, 10), 32);
        assert_eq!(dwell_of(&d, 11), 64);
        assert_eq!(dwell_of(&d, 12), 64);
        assert_eq!(dwell_of(&d, 13), 32);
        assert_eq!(dwell_of(&d, 24), 160);
        assert!(d.iter().all(|(a, _)| *a != 21));
    }

    /// PROT 0927 against the `0x99` capture: arm 11 128 vsyncs, 16 128, 17
    /// 64, 18 64, 21 180, 26 126, 27 259, 28 128.
    #[test]
    fn juggernaut_holds_its_captured_arms() {
        let d = dwells(JUGGERNAUT, 0, 28, &[(2, 11), (21, 23)]);
        assert_eq!(dwell_of(&d, 11), 128);
        assert_eq!(dwell_of(&d, 16), 128);
        assert_eq!(dwell_of(&d, 17), 64);
        assert_eq!(dwell_of(&d, 18), 64);
        assert_eq!(dwell_of(&d, 21), 180);
        assert_eq!(dwell_of(&d, 26), 128);
        assert_eq!(dwell_of(&d, 27), 256);
        assert_eq!(dwell_of(&d, 28), 128);
    }

    /// PROT 0919 against the `0x91` capture: arms 1 + 2 128 vsyncs, 4 130,
    /// 5 37, 6 197.
    #[test]
    fn spoon_holds_its_captured_arms() {
        let d = dwells(SPOON, 0, 8, &[]);
        assert_eq!(dwell_of(&d, 1) + dwell_of(&d, 2), 128);
        assert_eq!(dwell_of(&d, 4), 128);
        assert_eq!(dwell_of(&d, 5), 32);
        assert_eq!(dwell_of(&d, 6), 192);
        assert_eq!(dwell_of(&d, 8), 320);
    }

    /// The two uncaptured modules hold what their re-arms say.
    #[test]
    fn rave_and_spikefish_hold_their_rearms() {
        let d = dwells(ULTIMATE_RAVE, 0, 11, &[]);
        assert_eq!(dwell_of(&d, 1) + dwell_of(&d, 2), 64);
        assert_eq!(dwell_of(&d, 3), 0x108);
        assert_eq!(dwell_of(&d, 11), 0xC0);
        let d = dwells(SPIKEFISH, 0, 9, &[]);
        assert_eq!(dwell_of(&d, 3), 0x100);
        assert_eq!(dwell_of(&d, 5), 0x200);
        assert_eq!(dwell_of(&d, 9), 1);
    }

    /// An unsettled row holds the settle arm, but not past the limit.
    #[test]
    fn the_settle_gate_waits_on_the_row_with_a_cap() {
        let mut st = ModuleCamState::default();
        assert!(hold_step(PUERA, &mut st, 24));
        let mut n = 1;
        while hold_step(PUERA, &mut st, 24) {
            n += 1;
        }
        assert_eq!(n, SETTLE_TICK_LIMIT - 1);
    }

    /// PROT 0925's camera: arm 0's cut on the stage origin, arm 3's and
    /// arm 5's per-pass drifts (held or not), arm 5's cut on its pass, and
    /// the tweens arms 6 and 8 re-arm on every pass ahead of their gates.
    #[test]
    fn the_spikefish_arms_carry_the_body_camera() {
        let seats = ModuleCamSeats::default();
        let mut st = ModuleCamState::default();
        let a0 = spikefish_direct(&mut st, 0, seats);
        let s = a0.shot.expect("arm 0 cuts");
        assert_eq!(
            (s.angles, s.tr, s.focus, s.frames),
            ([0; 3], [0, 0x500, 0x300], [0; 3], 1)
        );

        st.countdown.0 = 5;
        let held = spikefish_direct(&mut st, 3, seats);
        assert!(held.hold && held.shot.is_none());
        let d = held.drift.expect("arm 3 drifts while held");
        assert_eq!((d.tr_y, d.tr_z, d.pitch, d.yaw), (-2, 8, 0, 0));

        st.countdown.0 = 1;
        let pass = spikefish_direct(&mut st, 5, seats);
        assert!(!pass.hold);
        let d = pass.drift.expect("arm 5 drifts on its pass too");
        assert_eq!((d.pitch, d.yaw, d.tr_y, d.tr_z), (-1, 8, 12, 8));
        let s = pass.shot.expect("arm 5 cuts on its pass");
        assert_eq!(
            (s.angles, s.tr, s.frames),
            ([0x80, 0, 0], [0, 0x1400, 0x4000], 1)
        );

        for (arm, tr, frames) in [
            (6, [0, 0x2400, 0x2400], 0x40),
            (8, [0, 0x2400, 0x4E20], 0x20),
        ] {
            st.countdown.0 = 100;
            let held = spikefish_direct(&mut st, arm, seats);
            assert!(held.hold);
            let s = held.shot.expect("re-armed on a held pass");
            assert_eq!(
                (s.angles, s.tr, s.focus, s.frames),
                ([0; 3], tr, [0; 3], frames)
            );
        }
    }

    #[test]
    fn every_covered_module_is_profiled() {
        for e in [918, 919, 922, 924, 925, 927] {
            let p = summon_hold_profile(e).unwrap();
            assert!(p.paces_band() && !p.owns_phase && !p.director_body);
            assert!(summon_holds(e).is_some());
        }
    }
}

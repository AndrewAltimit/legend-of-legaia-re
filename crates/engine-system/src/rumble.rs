//! DualShock rumble: the two motor words the game writes and the per-frame
//! kernel that turns them into the controller's two actuator bytes.
//!
//! Retail keeps the live rumble state in two words, `_DAT_8007B92C`
//! (`gp+0x614`) and `_DAT_8007B930` (`gp+0x618`, with `gp = 0x8007B318`).
//! Field scripts write or ramp them through op `0x4C` nibble-4 subs `6` / `7`
//! (gated by the options screen's Events vibration word `_DAT_800845A8`),
//! and the battle overlay, the cast modules and the summons write them on
//! hits and quakes. Once a frame `FUN_80018DB0` reads both and stores the
//! actuator payload libpad transmits on the next poll: port 0's two bytes
//! at `0x800915DA` / `0x800915DB`, registered by `FUN_80018F94` through
//! `PadSetAct(socket, block + 2, 2)`.
//!
//! In the DualShock layout the first byte is the small motor (on / off) and
//! the second the large motor's level. The small motor is pulsed: the word
//! at `gp+0x614`, biased by `0x20`, clamped to `0xFA` and shifted down four
//! bits, gives a period of `15 - level` frames; a period under `11` fires a
//! one-frame pulse every time the cadence counter `_DAT_8007B8A4` runs out.
//! The large motor takes the low byte of `gp+0x618` verbatim.
//!
//! The other layout (`_DAT_8007B79C`, raised by `FUN_80018F94` for a pad
//! that reports no extended-mode actuator data) parks the first byte at
//! `0x40`, pulses the second, and paces the pulse by the larger of the two
//! words.
//!
//! See `ghidra/scripts/funcs/80018db0.txt` (the kernel), `80018f94.txt`
//! (the registration), `80046a20.txt` (the battle-side decay of both words).

/// The two motor words, as retail stores them (`i32`, `0..=0xFF` in use).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RumbleMotors {
    /// `_DAT_8007B92C` (`gp+0x614`): paces the small motor's pulse.
    pub pulse: i32,
    /// `_DAT_8007B930` (`gp+0x618`): the large motor's level.
    pub level: i32,
}

/// The actuator payload's two layouts (`_DAT_8007B79C`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ActuatorLayout {
    /// `_DAT_8007B79C == 0`: byte 0 is the pulsed small motor, byte 1 the
    /// large motor's level. A DualShock in the mode the game sets.
    #[default]
    DualShock,
    /// `_DAT_8007B79C != 0`: byte 0 parked at `0x40`, byte 1 pulsed.
    Fixed,
}

/// The per-frame actuator kernel's own state: the pulse cadence counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RumbleActuator {
    /// `_DAT_8007B8A4`: frames until the next small-motor pulse; `-1` once
    /// it has run out.
    pub cadence: i32,
}

impl Default for RumbleActuator {
    fn default() -> Self {
        // Every captured field and overworld state holds it at `2`, the
        // value the kernel's idle arm stores.
        Self { cadence: 2 }
    }
}

/// The small-motor pulse period `FUN_80018DB0` derives from a motor word:
/// `15 - (clamp(word + 0x20, 0, 0xFA) >> 4)`.
pub fn pulse_period(word: i32) -> i32 {
    let v = word.saturating_add(0x20).clamp(0, 0xFA);
    15 - (v >> 4)
}

impl RumbleActuator {
    // PORT: FUN_80018DB0
    /// One frame of `FUN_80018DB0`'s actuator half (`0x80018E04..`): the
    /// cadence step and the two actuator bytes for `motors`. The kernel's
    /// leading branch (a CD-pause watchdog on `_DAT_8007BC70`) and the
    /// frame counter `_DAT_8007B8AC` are not rumble and stay out.
    pub fn step(&mut self, motors: RumbleMotors, layout: ActuatorLayout) -> [u8; 2] {
        self.cadence -= 1;
        if self.cadence < 0 {
            self.cadence = -1;
        }
        let pacer = match layout {
            ActuatorLayout::DualShock => motors.pulse,
            ActuatorLayout::Fixed => motors.level.max(motors.pulse),
        };
        let period = pulse_period(pacer);
        let pulse = if period < 11 {
            if self.cadence < 0 {
                self.cadence = period;
                1
            } else {
                0
            }
        } else {
            self.cadence = 2;
            0
        };
        match layout {
            ActuatorLayout::DualShock => [pulse, motors.level as u8],
            ActuatorLayout::Fixed => [0x40, pulse],
        }
    }
}

/// The battle side's hold timer and per-word decay rates: `gp+0xA1C`
/// (`0x8007BD34`), `gp+0xA30` (`0x8007BD48`) and `gp+0xA34` (`0x8007BD4C`).
///
/// Every battle-side writer stores a pulse word, a level word, a rate for
/// each and a zero hold in one block; the battle frame driver then runs the
/// words down. The battle controller's spawn (`FUN_80055B6C`,
/// `0x80055B7C..0x80055BB8`) seeds both rates at `8` and the hold at `0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleRumbleDecay {
    /// `gp+0xA1C`: frames the words hold before they decay. No shipped
    /// writer stores anything but `0` here.
    pub hold: i32,
    /// `gp+0xA30`: what the pulse word loses per frame step.
    pub pulse_rate: i32,
    /// `gp+0xA34`: what the level word loses per frame step.
    pub level_rate: i32,
}

/// The rate the battle controller's spawn seeds (`addiu v0,zero,8` at
/// `0x80055B7C`, stored at `0x80055BA0` / `0x80055BA8`).
pub const BATTLE_RUMBLE_SEED_RATE: i32 = 8;

impl Default for BattleRumbleDecay {
    fn default() -> Self {
        Self {
            hold: 0,
            pulse_rate: BATTLE_RUMBLE_SEED_RATE,
            level_rate: BATTLE_RUMBLE_SEED_RATE,
        }
    }
}

impl BattleRumbleDecay {
    // PORT: FUN_80046A20 (`0x80047164..0x8004725C`, the rumble hold and decay)
    /// One battle frame of the frame driver's rumble block, `step` being the
    /// frame step `DAT_1F800393`:
    ///
    /// - a positive hold drops by `step` and, while still positive, skips the
    ///   decay (`bgtz v0,0x80047260` at `0x80047188`); otherwise it is zeroed
    ///   (`sw zero,0xa1c(gp)` at `0x80047198`);
    /// - each word that is positive loses `step * rate` (`mult` / `mflo` /
    ///   `subu`) and is clamped to `0xFF` from above (`slti v0,v0,0x100`);
    ///   a negative rate therefore grows it, up to `0xFF`;
    /// - each word that is negative afterwards - including one that was not
    ///   positive to begin with - is zeroed (`bgez` / `sw zero`).
    ///
    /// Pulse first (`0x800471A4..0x800471F4`), then level (`0x8004720C..
    /// 0x8004725C`), each with its own rate.
    pub fn step(&mut self, motors: &mut RumbleMotors, step: i32) {
        if self.hold > 0 {
            self.hold -= step;
            if self.hold > 0 {
                return;
            }
        }
        self.hold = 0;
        decay_word(&mut motors.pulse, step, self.pulse_rate);
        decay_word(&mut motors.level, step, self.level_rate);
    }
}

fn decay_word(word: &mut i32, step: i32, rate: i32) {
    if *word > 0 {
        *word = word.wrapping_sub(step.wrapping_mul(rate));
        if *word >= 0x100 {
            *word = 0xFF;
        }
    }
    if *word < 0 {
        *word = 0;
    }
}

/// One battle-side rumble write: the five words every battle overlay, cast
/// module and summon store together, behind the Battles vibration row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleRumbleWrite {
    /// `_DAT_8007B92C`.
    pub pulse: i32,
    /// `_DAT_8007B930`.
    pub level: i32,
    /// `gp+0xA30`.
    pub pulse_rate: i32,
    /// `gp+0xA34`.
    pub level_rate: i32,
}

impl BattleRumbleWrite {
    /// The same strength and rate on both words, the shape most writers use.
    pub const fn even(value: i32, rate: i32) -> Self {
        Self {
            pulse: value,
            level: value,
            pulse_rate: rate,
            level_rate: rate,
        }
    }

    /// Store the write: both words, both rates, and a zero hold.
    pub fn apply(self, motors: &mut RumbleMotors, decay: &mut BattleRumbleDecay) {
        motors.pulse = self.pulse;
        motors.level = self.level;
        decay.pulse_rate = self.pulse_rate;
        decay.level_rate = self.level_rate;
        decay.hold = 0;
    }
}

/// The battle overlay's (PROT 0898) own writers, each behind the Battles
/// vibration word `0x800845C8 == 0`.
pub mod battle_writes {
    use super::BattleRumbleWrite;

    /// `FUN_801DDB30` (the shared damage finisher, magic / special / summon
    /// hits), a **party** defender (`sltiu v0,v0,3` on `s3` at `0x801DE314`):
    /// `0x801DE320..0x801DE338`, `0x801DE354..0x801DE364`.
    pub const FINISHER_PARTY_STRUCK: BattleRumbleWrite = BattleRumbleWrite::even(0xFF, 4);
    /// The same, a monster defender (`0x801DE33C..0x801DE364`).
    pub const FINISHER_MONSTER_STRUCK: BattleRumbleWrite = BattleRumbleWrite::even(0xC0, 3);
    /// `FUN_801DEA50`'s floor part (effect-script codes `0x81..=0x83`),
    /// `0x801DEF24..0x801DEF54`.
    pub const FLOOR_PART: BattleRumbleWrite = BattleRumbleWrite::even(0x80, 0xC);
    /// `FUN_801EC3E4`'s block, a party defender (`sltiu v0,a0,3`):
    /// `0x801EC8B0..0x801EC8D8` (the roll) and `0x801EC994..0x801EC9BC` (the
    /// juggle-window arm).
    pub const BLOCK_PARTY_DEFENDS: BattleRumbleWrite = BattleRumbleWrite::even(0xFF, 0x40);
    /// The same block, a party **attacker** (`andi v0,s6,0xff; sltiu`):
    /// `0x801EC8EC..0x801EC91C` / `0x801EC9D0..0x801ECA00`, stored after the
    /// defender's and so winning when both are party seats.
    pub const BLOCK_PARTY_ATTACKS: BattleRumbleWrite = BattleRumbleWrite {
        pulse: 0xC0,
        level: 0xB0,
        pulse_rate: 0x30,
        level_rate: 0x2C,
    };
    /// `FUN_801EC3E4`'s landed hit, a party defender (`sltiu v0,a3,3` at
    /// `0x801EDCE0`): `0x801EDCEC..0x801EDD14`.
    pub const HIT_PARTY_STRUCK: BattleRumbleWrite = BattleRumbleWrite::even(0xFF, 0x20);
    /// The same hit, a party attacker (`0x801EDD18..0x801EDD58`).
    pub const HIT_PARTY_ATTACKS: BattleRumbleWrite = BattleRumbleWrite {
        pulse: 0xC0,
        level: 0xB0,
        pulse_rate: 0x18,
        level_rate: 0x16,
    };
    /// `FUN_801EC3E4`'s impact kind `5` (the Rot roll) on a party defender,
    /// ahead of the immunity test (`0x801EE514..0x801EE568`; the pulse rate
    /// is `a0 = 4` from `0x801EE44C`).
    pub const ROT_PARTY_STRUCK: BattleRumbleWrite = BattleRumbleWrite {
        pulse: 0xFF,
        level: 0x80,
        pulse_rate: 4,
        level_rate: 2,
    };
}

/// The SCUS anim decoder's selector-`2` body (`FUN_8004998C`,
/// `0x8004A84C..0x8004A8BC`): `0xA0 - rand % 64` and `0x80 - rand % 64`,
/// rates `8`. `r0` / `r1` are the two `rand()` draws, in order.
pub fn burning_body_write(r0: u32, r1: u32) -> BattleRumbleWrite {
    // `rand()` is `0..=0x7FFF`, so the signed `%` the code builds
    // (`addiu v1,v0,0x3f` on a negative draw) is the plain remainder.
    BattleRumbleWrite {
        pulse: 0xA0 - (r0 % 64) as i32,
        level: 0x80 - (r1 % 64) as i32,
        pulse_rate: 8,
        level_rate: 8,
    }
}

/// A host's controller output for one frame, from the DualShock payload:
/// magnitudes in `0.0..=1.0`, as both a gamepad force-feedback API and the
/// browser's `GamepadHapticActuator` take them.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RumbleFrame {
    /// The large (low-frequency) motor.
    pub strong: f32,
    /// The small (high-frequency) motor.
    pub weak: f32,
}

impl RumbleFrame {
    /// The DualShock payload as magnitudes. The small motor is on / off.
    pub fn from_dualshock(act: [u8; 2]) -> Self {
        Self {
            strong: f32::from(act[1]) / 255.0,
            weak: if act[0] != 0 { 1.0 } else { 0.0 },
        }
    }

    /// Whether either motor runs.
    pub fn is_active(&self) -> bool {
        self.strong > 0.0 || self.weak > 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_words_never_pulse_and_park_the_cadence() {
        let mut a = RumbleActuator::default();
        for _ in 0..10 {
            assert_eq!(
                a.step(RumbleMotors::default(), ActuatorLayout::DualShock),
                [0, 0]
            );
            assert_eq!(a.cadence, 2);
        }
    }

    #[test]
    fn the_period_follows_the_disassembly() {
        assert_eq!(pulse_period(0), 13);
        assert_eq!(pulse_period(-100), 15);
        // `amp + 0x20 >= 0x50` is the first level that pulses.
        assert_eq!(pulse_period(0x2F), 11);
        assert_eq!(pulse_period(0x30), 10);
        assert_eq!(pulse_period(0xFF), 0);
        assert_eq!(pulse_period(i32::MAX), 0);
    }

    #[test]
    fn a_strong_word_pulses_on_its_period_and_the_level_passes_through() {
        let mut a = RumbleActuator::default();
        let m = RumbleMotors {
            pulse: 0x60,
            level: 0xC0,
        };
        // period = 15 - ((0x60 + 0x20) >> 4) = 7.
        let mut pulses = Vec::new();
        for _ in 0..24 {
            let act = a.step(m, ActuatorLayout::DualShock);
            assert_eq!(act[1], 0xC0);
            pulses.push(act[0]);
        }
        let on: Vec<usize> = (0..pulses.len()).filter(|&i| pulses[i] == 1).collect();
        // Idle cadence 2 runs out on the third frame, then every period + 1.
        assert_eq!(on, vec![2, 10, 18]);
    }

    #[test]
    fn full_strength_holds_the_small_motor_on() {
        let mut a = RumbleActuator::default();
        let m = RumbleMotors {
            pulse: 0xFF,
            level: 0,
        };
        let on = (0..10)
            .filter(|_| a.step(m, ActuatorLayout::DualShock)[0] == 1)
            .count();
        assert_eq!(on, 8, "every frame once the idle cadence has run out");
    }

    #[test]
    fn the_fixed_layout_parks_byte_zero_and_paces_by_the_larger_word() {
        let mut a = RumbleActuator::default();
        let m = RumbleMotors {
            pulse: 0,
            level: 0xFF,
        };
        let acts: Vec<[u8; 2]> = (0..4).map(|_| a.step(m, ActuatorLayout::Fixed)).collect();
        assert!(acts.iter().all(|x| x[0] == 0x40));
        assert!(acts.iter().any(|x| x[1] == 1));
    }

    #[test]
    fn the_battle_decay_follows_the_disassembly() {
        let mut m = RumbleMotors::default();
        let mut d = BattleRumbleDecay::default();
        battle_writes::HIT_PARTY_STRUCK.apply(&mut m, &mut d);
        // Step 2 a pass: 0xFF - 2 * 0x20 per pass.
        d.step(&mut m, 2);
        assert_eq!((m.pulse, m.level), (0xBF, 0xBF));
        for _ in 0..3 {
            d.step(&mut m, 2);
        }
        assert_eq!((m.pulse, m.level), (0, 0), "clamped at zero, not negative");
        // A hold skips the decay while it lasts, and the pass that runs it
        // out decays.
        m = RumbleMotors {
            pulse: 0x80,
            level: 0x80,
        };
        d.hold = 3;
        d.pulse_rate = 1;
        d.level_rate = 1;
        d.step(&mut m, 2);
        assert_eq!((m.pulse, d.hold), (0x80, 1));
        d.step(&mut m, 2);
        assert_eq!((m.pulse, d.hold), (0x7E, 0));
        // A negative rate grows the word up to the 0xFF clamp; a word at
        // zero is not touched; a negative word is zeroed.
        m = RumbleMotors {
            pulse: 0xF0,
            level: 0,
        };
        d.pulse_rate = -1;
        d.level_rate = -1;
        for _ in 0..10 {
            d.step(&mut m, 2);
        }
        assert_eq!((m.pulse, m.level), (0xFF, 0));
        m.level = -5;
        d.step(&mut m, 2);
        assert_eq!(m.level, 0);
    }

    #[test]
    fn the_burning_body_write_takes_two_draws() {
        let w = burning_body_write(0x3F + 64 * 7, 1);
        assert_eq!((w.pulse, w.level), (0xA0 - 0x3F, 0x7F));
        assert_eq!((w.pulse_rate, w.level_rate), (8, 8));
    }

    #[test]
    fn the_frame_maps_the_payload_to_magnitudes() {
        let f = RumbleFrame::from_dualshock([1, 0xFF]);
        assert_eq!((f.weak, f.strong), (1.0, 1.0));
        assert!(!RumbleFrame::from_dualshock([0, 0]).is_active());
    }
}

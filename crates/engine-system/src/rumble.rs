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
    fn the_frame_maps_the_payload_to_magnitudes() {
        let f = RumbleFrame::from_dualshock([1, 0xFF]);
        assert_eq!((f.weak, f.strong), (1.0, 1.0));
        assert!(!RumbleFrame::from_dualshock([0, 0]).is_active());
    }
}

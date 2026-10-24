//! The world's DualShock rumble: the two motor words, the ramps a field
//! script runs on them, and the per-frame actuator payload both play hosts
//! hand to the controller. The kernel is `legaia_engine_system::rumble`.

use super::*;
use legaia_engine_motion_vm::ambient_motion::{Ramp, RampKind, RampScheduler};
use legaia_engine_system::rumble::{ActuatorLayout, RumbleActuator, RumbleFrame, RumbleMotors};

/// Ramp destination tags: the words' `gp` offsets (`gp = 0x8007B318`).
const DEST_PULSE: u32 = 0x614;
const DEST_LEVEL: u32 = 0x618;

/// The live rumble state.
#[derive(Debug, Clone, Default)]
pub struct WorldRumble {
    /// `_DAT_8007B92C` / `_DAT_8007B930`.
    pub motors: RumbleMotors,
    /// The `FUN_8003C5F0` ramps op `0x4C` nibble-4 subs `6` / `7` install on
    /// the two words (the shared ramp pool in retail; the words' own here).
    ramps: RampScheduler,
    /// `FUN_80018DB0`'s cadence counter.
    pub actuator: RumbleActuator,
    /// The payload the last tick produced (`0x800915DA` / `0x800915DB`).
    pub payload: [u8; 2],
    /// The small motor's last eight pulse bits, newest in bit 0, for
    /// [`Self::frame`].
    pulse_history: u8,
}

impl WorldRumble {
    /// Op `0x4C` nibble-4 sub `6` (`pulse`) / `7` (`level`): write the word
    /// when `ticks == 0`, else ramp it from its live value over `ticks`
    /// frames. `sub` outside `6..=7` is ignored.
    pub fn write_or_ramp(&mut self, sub: u8, target: i32, ticks: u16) {
        let (dest, slot) = match sub {
            6 => (DEST_PULSE, &mut self.motors.pulse),
            7 => (DEST_LEVEL, &mut self.motors.level),
            _ => return,
        };
        // A newer write owns the word: drop a ramp still driving it. Retail
        // leaves the older job running in the shared pool, where the two
        // write the word in slot order each frame; no shipped script
        // overlaps two ramps on one word.
        self.ramps.free_owner(dest);
        if ticks == 0 {
            *slot = target;
        } else {
            self.ramps.install(Ramp {
                dest,
                owner: dest,
                start: *slot,
                end: target,
                total: i32::from(ticks),
                remaining: i32::from(ticks),
                kind: RampKind::U32,
            });
        }
    }

    /// Vibration off: both words to zero, as the gated arm does.
    pub fn clear(&mut self) {
        self.ramps.reset_pool();
        self.motors = RumbleMotors::default();
    }

    /// One frame: the ramps, then the actuator kernel.
    pub fn step(&mut self, speed: u8) {
        for w in self.ramps.tick(speed) {
            match w.dest {
                DEST_PULSE => self.motors.pulse = w.value,
                DEST_LEVEL => self.motors.level = w.value,
                _ => {}
            }
        }
        self.payload = self.actuator.step(self.motors, ActuatorLayout::DualShock);
        self.pulse_history = (self.pulse_history << 1) | u8::from(self.payload[0] != 0);
    }

    /// The controller output for the payload the last tick produced. The
    /// small motor is a one-frame pulse retail repeats on a cadence, which a
    /// DualShock's motor inertia smooths; a host API takes a level instead,
    /// so the weak magnitude is the pulse duty over the last eight ticks.
    pub fn frame(&self) -> RumbleFrame {
        RumbleFrame {
            weak: self.pulse_history.count_ones() as f32 / 8.0,
            ..RumbleFrame::from_dualshock(self.payload)
        }
    }
}

impl World {
    /// Whether the options screen's Events vibration row is Off
    /// (`_DAT_800845A8 != 0`), the gate on the field scripts' rumble writes.
    pub fn event_vibration_off(&self) -> bool {
        self.toggles.retail_options.vibration_events != 0
    }

    /// Advance the rumble state one tick (`FUN_80018DB0` runs once a frame
    /// in every mode).
    pub fn tick_rumble(&mut self) {
        self.rumble.step(1);
    }

    /// The controller output this tick produced, for the host's gamepad.
    pub fn rumble_frame(&self) -> RumbleFrame {
        self.rumble.frame()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_script_write_reaches_the_payload_and_a_ramp_runs_down() {
        let mut r = WorldRumble::default();
        r.write_or_ramp(7, 0xC0, 0);
        r.step(1);
        assert_eq!(r.payload[1], 0xC0);
        assert!(r.frame().strong > 0.7);
        r.write_or_ramp(7, 0, 4);
        let levels: Vec<u8> = (0..5)
            .map(|_| {
                r.step(1);
                r.payload[1]
            })
            .collect();
        assert_eq!(levels[3], 0, "the ramp lands on its endpoint");
        assert!(levels[0] > levels[1] && levels[1] > levels[2]);
        r.write_or_ramp(6, 0xFF, 0);
        let pulses = (0..10)
            .filter(|_| {
                r.step(1);
                r.payload[0] == 1
            })
            .count();
        assert!(pulses > 0);
        assert_eq!(r.frame().weak, 1.0, "full strength holds the motor on");
        r.clear();
        r.step(1);
        assert_eq!(r.payload, [0, 0]);
    }
}

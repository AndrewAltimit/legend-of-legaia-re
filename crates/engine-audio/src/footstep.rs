//! Field footstep + ambient cue cadence.
//!
//! The per-frame ticker that decides **when** the field player's footstep
//! cue fires and when the periodic ambient cue retriggers. It runs every
//! field-mode frame and owns three counters plus the two per-voice trigger
//! bytes the SPU side reads (`0x800915DA` / `0x800915DB`).
//!
//! The interesting part is the step interval, which is derived from the
//! player's movement magnitude rather than from a fixed timer: a faster
//! walk produces a shorter interval and therefore more frequent steps.
//!
//! ```text
//!   biased   = min(speed + 0x20, 0xFA)          (clamped to >= 0 below)
//!   interval = 0xF - (biased >> 4)
//! ```
//!
//! `interval` is then gated against `0xB`. Only `interval < 0xB` counts as
//! "moving fast enough to make noise"; at or above it the player is treated
//! as stationary/creeping, the countdown is parked at `2`, and no step
//! fires. Because `biased` is capped at `0xFA` the interval bottoms out at
//! `0` (a step every frame) and, with `speed == 0`, sits at `0xD` - above
//! the gate, which is why standing still is silent.
//!
//! Which speed feeds the formula depends on the footstep-active flag: when
//! set, the cadence uses `max(primary, secondary)` of the two movement
//! magnitudes; when clear it uses `primary` alone. The two branches also
//! write **different** trigger bytes, which is the part worth not
//! paraphrasing - see [`FootstepCadence::tick`].
//!
//! Clean-room from the decompiled control flow; no Sony bytes. Retail
//! reference `docs/subsystems/audio.md` and the `80018DB0` row of
//! `docs/reference/functions.md`.

/// Frames between periodic ambient-cue retriggers (retail `0x4B0`).
pub const AMBIENT_PERIOD_FRAMES: i32 = 0x4B0;

/// Constant added to the movement magnitude before the interval lookup.
pub const SPEED_BIAS: i32 = 0x20;

/// Upper clamp on the biased movement magnitude.
pub const SPEED_CAP: i32 = 0xFA;

/// The interval is counted **down** from this base as speed rises.
pub const INTERVAL_BASE: i32 = 0xF;

/// Intervals at or above this are treated as "not moving enough to step".
pub const INTERVAL_GATE: i32 = 0xB;

/// Countdown parked here while the player is below the step gate.
pub const STALL_RELOAD: i32 = 2;

/// What a single [`FootstepCadence::tick`] fired.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CadenceTick {
    /// The periodic ambient cue retriggered this frame (retail calls the
    /// voice stop/rewind helper `FUN_8005C034(9, 0)`).
    pub ambient_fired: bool,
    /// A footstep landed this frame.
    pub step_fired: bool,
}

/// Per-frame footstep / ambient cadence state.
///
/// Field names carry the retail global each one mirrors so the state can be
/// diffed against a save-state capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FootstepCadence {
    /// `_DAT_8007BC70` - ambient retrigger countdown. **Zero disarms it
    /// entirely**; retail only decrements when the value is already
    /// non-zero, so a zeroed counter never reloads on its own.
    pub ambient_countdown: i32,
    /// `_DAT_8007B8A4` - frames until the next step may fire. Floors at
    /// `-1` (the "ready to fire" sentinel), never runs further negative.
    pub step_countdown: i32,
    /// `_DAT_8007B8AC` - free-running frame counter, incremented every tick
    /// and never reset here.
    pub uptime: i32,
    /// `DAT_8007B79C` - footstep-active flag, selecting which of the two
    /// branches below runs.
    pub footstep_active: bool,
    /// `DAT_800915DA` - first per-voice trigger byte.
    pub trigger_a: u8,
    /// `DAT_800915DB` - second per-voice trigger byte.
    pub trigger_b: u8,
}

impl Default for FootstepCadence {
    fn default() -> Self {
        Self {
            ambient_countdown: AMBIENT_PERIOD_FRAMES,
            // Retail's "ready to fire" sentinel: a fresh scene may step on
            // its first moving frame rather than waiting out an interval.
            step_countdown: -1,
            uptime: 0,
            footstep_active: false,
            trigger_a: 0,
            trigger_b: 0,
        }
    }
}

/// The speed -> interval curve shared by both branches.
///
/// Separate from [`FootstepCadence::tick`] so the curve can be tested on
/// its own: it is the only arithmetic in the function, and the `< 0` guard
/// on the *biased* value (not the raw speed) is easy to get subtly wrong.
fn step_interval(speed: i32) -> i32 {
    let biased = (speed + SPEED_BIAS).min(SPEED_CAP);
    // Retail clamps the shifted result, not the input: a biased value that
    // went negative yields shift 0, hence the maximum interval.
    let shifted = if biased < 0 { 0 } else { biased >> 4 };
    INTERVAL_BASE - shifted
}

impl FootstepCadence {
    /// Advance one field frame.
    ///
    /// `speed_primary` / `speed_secondary` are the two movement magnitudes
    /// retail reads from `gp+0x614` and `gp+0x618`. This function only
    /// reads them - it does not age them.
    ///
    /// The two branches differ in more than which speed they use, and the
    /// asymmetry is deliberate in retail:
    ///
    /// - **active** (`footstep_active`): interval from
    ///   `max(primary, secondary)`; `trigger_a` is pinned to `0x40` every
    ///   frame and `trigger_b` carries the one-frame step pulse (`1` on the
    ///   firing frame, `0` otherwise).
    /// - **inactive**: interval from `primary` alone; `trigger_a` carries
    ///   the pulse (`1` / `0`) and `trigger_b` is loaded with the low byte
    ///   of `speed_secondary` every frame regardless of whether a step fired.
    ///
    /// A step fires only when the interval is under [`INTERVAL_GATE`] *and*
    /// the countdown has reached the `-1` sentinel; firing reloads the
    /// countdown with the interval. Above the gate the countdown is parked
    /// at [`STALL_RELOAD`], so resuming a walk costs at most two frames.
    // PORT: FUN_80018db0 - field footstep / ambient cadence tick. Ambient
    // countdown reload (0x4B0) + voice retrigger, the step countdown floored
    // at the -1 sentinel, the free-running uptime counter, and the two
    // branches over DAT_8007B79C that derive the step interval from the
    // movement magnitude and arm the trigger bytes DAT_800915DA / DB.
    // The libspu voice retrigger itself (FUN_8005C034) is reported through
    // CadenceTick rather than called: engine-audio owns its own voice pool.
    pub fn tick(&mut self, speed_primary: i32, speed_secondary: i32) -> CadenceTick {
        let mut out = CadenceTick::default();

        // Periodic ambient retrigger. Retail decrements only a non-zero
        // counter, then reloads when the decrement took it below 1.
        if self.ambient_countdown != 0 {
            self.ambient_countdown -= 1;
            if self.ambient_countdown < 1 {
                self.ambient_countdown = AMBIENT_PERIOD_FRAMES;
                out.ambient_fired = true;
            }
        }

        self.step_countdown -= 1;
        self.uptime = self.uptime.wrapping_add(1);
        if self.step_countdown < 0 {
            self.step_countdown = -1;
        }

        if self.footstep_active {
            let interval = step_interval(speed_primary.max(speed_secondary));
            if interval < INTERVAL_GATE {
                if self.step_countdown < 0 {
                    self.step_countdown = interval;
                    self.trigger_a = 0x40;
                    self.trigger_b = 1;
                    out.step_fired = true;
                    return out;
                }
            } else {
                self.step_countdown = STALL_RELOAD;
            }
            self.trigger_a = 0x40;
            self.trigger_b = 0;
            return out;
        }

        let interval = step_interval(speed_primary);
        let mut fired = false;
        if interval < INTERVAL_GATE {
            if self.step_countdown < 0 {
                self.trigger_a = 1;
                self.step_countdown = interval;
                fired = true;
            }
        } else {
            self.step_countdown = STALL_RELOAD;
        }
        if !fired {
            self.trigger_a = 0;
        }
        // Retail writes this on every path through the inactive branch,
        // including the frames where no step fired.
        self.trigger_b = speed_secondary as u8;
        out.step_fired = fired;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Standing still lands above the gate, so no step ever fires.
    #[test]
    fn zero_speed_is_above_the_gate() {
        // (0 + 0x20) >> 4 = 2, interval = 0xF - 2 = 0xD >= 0xB.
        assert_eq!(step_interval(0), 0xD);
        assert!(step_interval(0) >= INTERVAL_GATE);
    }

    /// The speed cap bottoms the interval out at zero, never negative.
    #[test]
    fn interval_saturates_at_zero() {
        assert_eq!(step_interval(SPEED_CAP), 0);
        // Well past the cap clamps to the same place.
        assert_eq!(step_interval(100_000), 0);
    }

    /// A biased value driven negative yields the maximum interval rather
    /// than an arithmetic-shift surprise.
    #[test]
    fn negative_bias_yields_max_interval() {
        assert_eq!(step_interval(-1000), INTERVAL_BASE);
    }

    /// Faster movement is monotonically shorter-intervalled.
    #[test]
    fn interval_is_monotonically_decreasing_in_speed() {
        let mut prev = step_interval(0);
        for speed in (0..=SPEED_CAP).step_by(4) {
            let cur = step_interval(speed);
            assert!(cur <= prev, "interval rose at speed {speed}");
            prev = cur;
        }
    }

    /// Below the gate, steps fire on a cadence equal to the interval: one
    /// firing frame then `interval` silent frames before the countdown is
    /// back at the -1 sentinel.
    #[test]
    fn inactive_branch_fires_on_the_interval_cadence() {
        let mut c = FootstepCadence::default();
        let speed = SPEED_CAP; // interval 0 -> fires every frame
        let t = c.tick(speed, 0);
        assert!(t.step_fired);
        assert_eq!(c.trigger_a, 1, "inactive branch pulses trigger_a");

        // interval 0 reloads the countdown to 0, which the next tick's
        // decrement takes to -1, so the following frame fires again.
        let t = c.tick(speed, 0);
        assert!(t.step_fired);
    }

    /// A slower (but still gated-in) speed spaces the steps out.
    #[test]
    fn slower_speed_spaces_steps_further_apart() {
        fn steps_in(frames: usize, speed: i32) -> usize {
            let mut c = FootstepCadence::default();
            (0..frames).filter(|_| c.tick(speed, 0).step_fired).count()
        }
        let fast = steps_in(120, SPEED_CAP);
        let slow = steps_in(120, 0x60); // (0x60+0x20)>>4 = 8 -> interval 7
        assert!(
            fast > slow,
            "fast={fast} should out-step slow={slow} over the same window"
        );
        assert!(slow > 0, "0x60 is under the gate and must still step");
    }

    /// Above the gate the countdown parks at STALL_RELOAD and nothing fires.
    #[test]
    fn above_the_gate_parks_the_countdown_and_stays_silent() {
        let mut c = FootstepCadence::default();
        for _ in 0..30 {
            let t = c.tick(0, 0);
            assert!(!t.step_fired);
        }
        assert_eq!(c.step_countdown, STALL_RELOAD);
        assert_eq!(c.trigger_a, 0);
    }

    /// The active branch pins trigger_a to 0x40 and pulses trigger_b - the
    /// mirror image of the inactive branch's byte assignment.
    #[test]
    fn active_branch_swaps_which_trigger_byte_pulses() {
        let mut c = FootstepCadence {
            footstep_active: true,
            ..Default::default()
        };
        let t = c.tick(SPEED_CAP, 0);
        assert!(t.step_fired);
        assert_eq!(c.trigger_a, 0x40);
        assert_eq!(c.trigger_b, 1, "active branch pulses trigger_b");

        // A stationary frame keeps trigger_a at 0x40 but drops the pulse.
        let mut c = FootstepCadence {
            footstep_active: true,
            ..Default::default()
        };
        c.tick(0, 0);
        assert_eq!(c.trigger_a, 0x40);
        assert_eq!(c.trigger_b, 0);
    }

    /// The active branch takes the max of the two speeds, so a fast
    /// secondary drives the cadence even when the primary is stationary.
    #[test]
    fn active_branch_uses_the_larger_speed() {
        let mut c = FootstepCadence {
            footstep_active: true,
            ..Default::default()
        };
        assert!(
            c.tick(0, SPEED_CAP).step_fired,
            "secondary speed alone must be able to drive a step"
        );
    }

    /// The inactive branch stamps trigger_b with the low byte of the
    /// secondary speed on every frame, fired or not.
    #[test]
    fn inactive_branch_always_stamps_trigger_b() {
        let mut c = FootstepCadence::default();
        // Stationary: no step, but trigger_b still takes the low byte.
        c.tick(0, 0x1234);
        assert_eq!(c.trigger_b, 0x34);
    }

    /// Ambient fires exactly on the period and reloads.
    #[test]
    fn ambient_fires_on_its_period() {
        let mut c = FootstepCadence::default();
        let mut fired_at = Vec::new();
        for frame in 0..(AMBIENT_PERIOD_FRAMES * 2 + 5) {
            if c.tick(0, 0).ambient_fired {
                fired_at.push(frame);
            }
        }
        assert_eq!(
            fired_at,
            vec![AMBIENT_PERIOD_FRAMES - 1, AMBIENT_PERIOD_FRAMES * 2 - 1],
            "ambient retriggers once per period"
        );
    }

    /// A zeroed ambient counter is disarmed and never self-reloads.
    #[test]
    fn zero_ambient_counter_stays_disarmed() {
        let mut c = FootstepCadence {
            ambient_countdown: 0,
            ..Default::default()
        };
        for _ in 0..(AMBIENT_PERIOD_FRAMES + 10) {
            assert!(!c.tick(0, 0).ambient_fired);
        }
        assert_eq!(c.ambient_countdown, 0);
    }

    /// Uptime advances every frame regardless of branch.
    #[test]
    fn uptime_counts_every_frame() {
        let mut c = FootstepCadence::default();
        for _ in 0..50 {
            c.tick(0, 0);
        }
        assert_eq!(c.uptime, 50);
    }
}

//! The scripted countdown timer the field VM arms with `0x4C 0xD3`
//! (`SCHEDULE_TIMED_FLAGS`) - retail's "escape timer", the collapsing-dungeon
//! clock in `chitei2`.
//!
//! PORT: FUN_801D2EBC - countdown scheduler + HUD decomposition
//!
//! Source: `ghidra/scripts/funcs/overlay_cutscene_dialogue_801d2ebc.txt`.
//!
//! One retail function does three things per frame, and all three are here:
//! it subtracts the play-clock delta from the counter `_DAT_800845A0`, fires
//! a below-threshold flag and an expiry flag through `func_0x8003CE08` as the
//! count crosses each line, and decomposes what is left into the MM:SS.ff
//! readout plus its ink colour. The decomposition is therefore a product of
//! the tick, not of a renderer. The routine is the handler of the HUD actor
//! the timer op spawns, and that actor's own `+0x54` phase machine - which
//! holds a zeroed readout for [`EXPIRED_HOLD`] frames after expiry and then
//! kills the actor, ending the countdown - is [`EscapeTimerHud`].
//!
//! The installer half lives in [`crate::field`] (`FUN_801DE840` case 0xD sub
//! 3, which writes the duration / threshold / packed-flag-word triple);
//! `legaia_engine_core::World` joins the two through its
//! `schedule_timed_flags` host hook and its per-frame `tick_escape_timer`.
//!
//! Split out of [`crate::world_map_overlay`], whose other four addresses are
//! the developer-menu / records-screen leaves and have no engine caller.
//! Written from the disassembly; no Sony bytes live here.

/// Ink colour the escape-timer HUD selects from the remaining count
/// (`_DAT_8007B454`): white while there is time, then a warning colour, then
/// a critical colour below the last minute-and-a-half.
///
/// PORT: FUN_801D2EBC (`_DAT_8007B454` selection)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerInk {
    /// Remaining == 0: neutral / white (`2`).
    Neutral = 2,
    /// `0 < remaining <= 0x707`: warning (`6`).
    Warning = 6,
    /// `remaining > 0x707`: cool/safe (`7`).
    Safe = 7,
}

/// The retail ink logic: `2` at zero, `6` while non-zero and `<= 0x707`,
/// `7` above `0x707`.
///
/// PORT: FUN_801D2EBC
pub fn timer_ink(remaining: i32) -> TimerInk {
    if remaining == 0 {
        TimerInk::Neutral
    } else if remaining > 0x707 {
        TimerInk::Safe
    } else {
        TimerInk::Warning
    }
}

/// Story-flag ids the scheduler fires as the counter drops. Both are the low
/// 12 bits of a packed word (`_DAT_800845C0`): the low half is a warning flag
/// fired once the counter falls below `_DAT_800845BC`, the high half an
/// expiry flag fired when it reaches zero.
///
/// PORT: FUN_801D2EBC (`func_0x8003CE08` calls)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TimerFlagEvents {
    /// Fire `flags[warning_flag & 0xFFF]` (counter dropped below threshold).
    pub warning_flag: Option<u16>,
    /// Fire `flags[expiry_flag & 0xFFF]` and disarm the timer (counter hit 0).
    pub expiry_flag: Option<u16>,
}

/// Live state of the escape-timer scheduler.
///
/// PORT: FUN_801D2EBC
#[derive(Debug, Clone, Copy, Default)]
pub struct EscapeTimer {
    /// Remaining countdown (`_DAT_800845A0`).
    pub remaining: i32,
    /// Below-threshold trigger point (`_DAT_800845BC`).
    pub warn_threshold: i32,
    /// Whether the timer is still armed (`_DAT_800845B8 != 0`).
    pub armed: bool,
}

impl EscapeTimer {
    /// Advance the countdown by the frame-clock delta and report which story
    /// flags the tick fires. `clock_delta = new_clock - prev_clock`
    /// (`_DAT_80084570 - old _DAT_80073ED4`). `flag_word` is `_DAT_800845C0`:
    /// low half = warning flag, high half = expiry flag.
    ///
    /// When `busy` is set (the retail short-circuit for any of the three
    /// pause conditions) the counter is left untouched and no flags fire -
    /// the caller still refreshes the clock latch.
    ///
    /// PORT: FUN_801D2EBC (scheduler head)
    pub fn tick(&mut self, clock_delta: i32, flag_word: u32, busy: bool) -> TimerFlagEvents {
        let mut events = TimerFlagEvents::default();
        if busy {
            return events;
        }
        self.remaining -= clock_delta;
        if self.remaining < 1 {
            self.armed = false;
            events.expiry_flag = Some(((flag_word >> 16) & 0xFFF) as u16);
        }
        if self.remaining < self.warn_threshold {
            events.warning_flag = Some((flag_word & 0xFFF) as u16);
        }
        events
    }

    /// Decompose the remaining count into the MM:SS.ff fields the HUD draws.
    /// `frames = remaining % 60`, `seconds = (remaining/60) % 60`,
    /// `minutes = (remaining/60) / 60`.
    ///
    /// PORT: FUN_801D2EBC (`% 0x3C` decomposition + `(frames*100)/0x3C`)
    pub fn hud_fields(&self) -> (i32, i32, i32) {
        let frames = self.remaining % 60;
        let seconds = (self.remaining / 60) % 60;
        let minutes = (self.remaining / 60) / 60;
        // The hundredths cell is `(frames * 100) / 60`.
        let hundredths = frames * 100 / 60;
        (minutes, seconds, hundredths)
    }
}

/// Frames the expired readout holds `00:00.00` before the HUD actor kills
/// itself: `+0x68 >= 0x79` (`slti v0,v0,0x79` at `0x801D3098`).
pub const EXPIRED_HOLD: i16 = 0x79;

/// The HUD actor's own phase machine, `+0x54` (`0x801D2FE8..0x801D30B4`).
///
/// `FUN_801D2EBC` is the handler of the HUD actor the timer op spawns, so
/// the countdown lives exactly as long as that actor does. After the count
/// runs out (or the timer is disarmed) the actor does not vanish on the spot:
///
/// - **phase 0** - while the timer is armed and the count positive, the
///   readout is the live decomposition. Otherwise the count is zeroed, the
///   digits are drawn as zeros, and the phase advances;
/// - **phase 1** - zero the hold clock `+0x68`, advance, and fall straight
///   into phase 2's body the same frame;
/// - **phase 2** - zero the count and the digits, add the frame step to
///   `+0x68`, and once it reaches [`EXPIRED_HOLD`] set the kill bit
///   (`+0x10 |= 8`).
///
/// The draw that follows runs in every phase, so the expired timer shows a
/// neutral-ink `00:00.00` for the hold.
///
/// PORT: FUN_801D2EBC (the `+0x54` phase machine)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EscapeTimerHud {
    /// `+0x54`.
    pub phase: u8,
    /// `+0x68` - the expired-readout hold clock.
    pub hold: i16,
}

/// What one pass of [`EscapeTimerHud::step`] leaves on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EscapeTimerHudFrame {
    /// `(minutes, seconds, hundredths)` - zeros once the timer has expired.
    pub digits: (i32, i32, i32),
    /// The ink `_DAT_8007B454` the digits draw in.
    pub ink: TimerInk,
    /// The actor set its kill bit this frame.
    pub teardown: bool,
}

impl EscapeTimerHud {
    /// One pass of the phase machine, after [`EscapeTimer::tick`] has drained
    /// the count this frame. `frame_step` is `DAT_1F800393`.
    pub fn step(&mut self, timer: &mut EscapeTimer, frame_step: u8) -> EscapeTimerHudFrame {
        let mut zeroed = false;
        let mut teardown = false;
        let mut phase2 = false;
        match self.phase {
            0 => {
                if !(timer.armed && timer.remaining > 0) {
                    timer.remaining = 0;
                    zeroed = true;
                    self.phase = 1;
                }
            }
            1 => {
                self.hold = 0;
                self.phase = 2;
                phase2 = true;
            }
            2 => phase2 = true,
            _ => {}
        }
        if phase2 {
            timer.remaining = 0;
            zeroed = true;
            self.hold = self.hold.wrapping_add(i16::from(frame_step));
            teardown = self.hold >= EXPIRED_HOLD;
        }
        let digits = if zeroed {
            (0, 0, 0)
        } else {
            timer.hud_fields()
        };
        EscapeTimerHudFrame {
            digits,
            ink: timer_ink(timer.remaining),
            teardown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_timer_ink_thresholds() {
        assert_eq!(timer_ink(0), TimerInk::Neutral);
        assert_eq!(timer_ink(0x708), TimerInk::Safe);
        assert_eq!(timer_ink(0x707), TimerInk::Warning);
        assert_eq!(timer_ink(1), TimerInk::Warning);
    }

    #[test]
    fn escape_timer_fires_flags_on_expiry() {
        let mut t = EscapeTimer {
            remaining: 5,
            warn_threshold: 100,
            armed: true,
        };
        // flag word: low half 0x0C7 (warning), high half 0x123 (expiry).
        let ev = t.tick(10, 0x0123_00C7, false);
        assert_eq!(t.remaining, -5);
        assert!(!t.armed); // disarmed on expiry
        assert_eq!(ev.expiry_flag, Some(0x123));
        assert_eq!(ev.warning_flag, Some(0x0C7)); // also below threshold
    }

    #[test]
    fn escape_timer_warning_only() {
        let mut t = EscapeTimer {
            remaining: 200,
            warn_threshold: 100,
            armed: true,
        };
        // drop to 150 -> above 0, below... no, 150 > 100 -> no warning yet.
        let ev = t.tick(50, 0x0123_00C7, false);
        assert_eq!(t.remaining, 150);
        assert!(t.armed);
        assert_eq!(ev.expiry_flag, None);
        assert_eq!(ev.warning_flag, None);
        // drop below threshold.
        let ev = t.tick(60, 0x0123_00C7, false);
        assert_eq!(t.remaining, 90);
        assert_eq!(ev.warning_flag, Some(0x0C7));
        assert_eq!(ev.expiry_flag, None);
    }

    #[test]
    fn escape_timer_busy_freezes() {
        let mut t = EscapeTimer {
            remaining: 5,
            warn_threshold: 100,
            armed: true,
        };
        let ev = t.tick(10, 0x0123_00C7, true);
        assert_eq!(t.remaining, 5); // untouched
        assert!(t.armed);
        assert_eq!(ev, TimerFlagEvents::default());
    }

    #[test]
    fn escape_timer_hud_fields() {
        let t = EscapeTimer {
            remaining: 60 * 90 + 30, // 1m30s + 30 frames
            warn_threshold: 0,
            armed: true,
        };
        let (m, s, hundredths) = t.hud_fields();
        assert_eq!(m, 1);
        assert_eq!(s, 30);
        assert_eq!(hundredths, 30 * 100 / 60); // 50
    }

    #[test]
    fn expired_hud_holds_zeros_then_tears_down() {
        let mut t = EscapeTimer {
            remaining: 2,
            warn_threshold: 0,
            armed: true,
        };
        let mut hud = EscapeTimerHud::default();
        t.tick(1, 0, false);
        let f = hud.step(&mut t, 1);
        assert_eq!((f.digits, f.teardown, hud.phase), ((0, 0, 1), false, 0));
        t.tick(1, 0, false); // hits zero: disarms
        let f = hud.step(&mut t, 1);
        assert_eq!(
            (f.digits, f.ink, hud.phase),
            ((0, 0, 0), TimerInk::Neutral, 1)
        );
        // Phase 1 falls into phase 2 the same frame: hold = 1.
        t.tick(1, 0, false);
        let f = hud.step(&mut t, 1);
        assert_eq!((hud.phase, hud.hold, f.teardown), (2, 1, false));
        assert_eq!(t.remaining, 0, "phase 2 zeroes the count each frame");
        let mut frames = 1;
        loop {
            t.tick(1, 0, false);
            frames += 1;
            if hud.step(&mut t, 1).teardown {
                break;
            }
        }
        assert_eq!(frames, EXPIRED_HOLD as i32);
    }
}

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
//! the tick, not of a renderer.
//!
//! The installer half lives in [`crate::field`] (`FUN_801DE840` case 0xD sub
//! 3, which writes the duration / threshold / packed-flag-word triple);
//! `legaia_engine_core::World` joins the two through its
//! `schedule_timed_flags` host hook and its per-frame `tick_escape_timer`.
//!
//! Split out of [`crate::world_map_overlay`], whose other four addresses are
//! the developer-menu / records-screen leaves and have no engine caller.
//! Clean-room from the disassembly; no Sony bytes live here.

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
}

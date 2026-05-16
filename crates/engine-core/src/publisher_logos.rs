//! Publisher-logos boot phase.
//!
//! Runs before the title screen. Displays the four TIMs from PROT 0895
//! (`init.pak`) in sequence: PROKION → Contrail → SCEA → WARNING. Each
//! logo fades in, holds, fades out before advancing to the next.
//!
//! The session is renderer-free: it owns timing/state only. Engines
//! query `current_logo()` + `alpha()` each frame to drive the actual
//! blit. When [`PublisherLogosSession::is_done`] returns true, the
//! caller transitions to the title screen.

/// Per-logo timing (in frames @ 60 Hz). Tunable; retail timings TBD —
/// these match a typical PSX boot pacing.
const FADE_IN_FRAMES: u16 = 30;
const HOLD_FRAMES: u16 = 90;
const FADE_OUT_FRAMES: u16 = 30;
const FRAMES_PER_LOGO: u16 = FADE_IN_FRAMES + HOLD_FRAMES + FADE_OUT_FRAMES;

/// Total number of publisher logos shown during boot.
pub const LOGO_COUNT: usize = 4;

/// Phase within a single logo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogoPhase {
    /// Black → full opacity (`FADE_IN_FRAMES` frames).
    FadeIn,
    /// Full opacity hold (`HOLD_FRAMES` frames).
    Hold,
    /// Full opacity → black (`FADE_OUT_FRAMES` frames).
    FadeOut,
}

/// Boot-time publisher logos state machine.
#[derive(Debug, Clone)]
pub struct PublisherLogosSession {
    logo_idx: u8,
    frames_in_logo: u16,
    done: bool,
    /// When true, caller has signalled "skip the rest" (Start pressed).
    /// On the next tick we advance straight to `done`.
    skip_requested: bool,
}

impl Default for PublisherLogosSession {
    fn default() -> Self {
        Self::new()
    }
}

impl PublisherLogosSession {
    pub fn new() -> Self {
        Self {
            logo_idx: 0,
            frames_in_logo: 0,
            done: false,
            skip_requested: false,
        }
    }

    /// Advance one frame. Returns the [`LogoPhase`] that just ticked
    /// (or `None` if the session has finished).
    pub fn tick(&mut self) -> Option<LogoPhase> {
        if self.done {
            return None;
        }
        if self.skip_requested {
            self.done = true;
            return None;
        }
        let phase = self.phase();
        self.frames_in_logo += 1;
        if self.frames_in_logo >= FRAMES_PER_LOGO {
            self.frames_in_logo = 0;
            self.logo_idx += 1;
            if (self.logo_idx as usize) >= LOGO_COUNT {
                self.done = true;
            }
        }
        Some(phase)
    }

    /// Request that the session end on the next tick. Caller hooks this
    /// to Start being pressed during the boot sequence.
    pub fn request_skip(&mut self) {
        self.skip_requested = true;
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Index of the logo currently being displayed (`0..LOGO_COUNT`).
    /// Returns `LOGO_COUNT` when the session is done.
    pub fn current_logo(&self) -> usize {
        if self.done {
            LOGO_COUNT
        } else {
            self.logo_idx as usize
        }
    }

    pub fn phase(&self) -> LogoPhase {
        if self.frames_in_logo < FADE_IN_FRAMES {
            LogoPhase::FadeIn
        } else if self.frames_in_logo < FADE_IN_FRAMES + HOLD_FRAMES {
            LogoPhase::Hold
        } else {
            LogoPhase::FadeOut
        }
    }

    /// Opacity in `[0.0, 1.0]` for the current logo this frame.
    /// `0.0` = fully black, `1.0` = fully visible.
    pub fn alpha(&self) -> f32 {
        if self.done {
            return 0.0;
        }
        match self.phase() {
            LogoPhase::FadeIn => self.frames_in_logo as f32 / FADE_IN_FRAMES as f32,
            LogoPhase::Hold => 1.0,
            LogoPhase::FadeOut => {
                let into_fadeout = self.frames_in_logo - (FADE_IN_FRAMES + HOLD_FRAMES);
                1.0 - (into_fadeout as f32 / FADE_OUT_FRAMES as f32)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advances_through_all_four_logos() {
        let mut s = PublisherLogosSession::new();
        assert_eq!(s.current_logo(), 0);
        // Tick exactly one logo's worth of frames.
        for _ in 0..FRAMES_PER_LOGO {
            assert!(!s.is_done());
            s.tick();
        }
        assert_eq!(s.current_logo(), 1);

        for _ in 0..FRAMES_PER_LOGO {
            s.tick();
        }
        assert_eq!(s.current_logo(), 2);

        for _ in 0..FRAMES_PER_LOGO {
            s.tick();
        }
        assert_eq!(s.current_logo(), 3);

        for _ in 0..FRAMES_PER_LOGO {
            s.tick();
        }
        assert!(s.is_done());
        assert_eq!(s.current_logo(), LOGO_COUNT);
    }

    #[test]
    fn alpha_curves_match_phase_boundaries() {
        let mut s = PublisherLogosSession::new();
        // FadeIn starts at 0, climbs.
        assert_eq!(s.alpha(), 0.0);
        for _ in 0..(FADE_IN_FRAMES - 1) {
            s.tick();
        }
        // Just before Hold begins.
        let alpha_pre_hold = s.alpha();
        assert!(
            (alpha_pre_hold - (FADE_IN_FRAMES - 1) as f32 / FADE_IN_FRAMES as f32).abs() < 1e-6
        );
        s.tick();
        assert_eq!(s.phase(), LogoPhase::Hold);
        assert_eq!(s.alpha(), 1.0);

        // Skip ahead into FadeOut.
        for _ in 0..HOLD_FRAMES {
            s.tick();
        }
        assert_eq!(s.phase(), LogoPhase::FadeOut);
        // First FadeOut frame: alpha = 1.0 (just entered).
        assert!((s.alpha() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn request_skip_ends_on_next_tick() {
        let mut s = PublisherLogosSession::new();
        // Tick into the middle of logo 1.
        for _ in 0..(FRAMES_PER_LOGO + 30) {
            s.tick();
        }
        assert_eq!(s.current_logo(), 1);
        s.request_skip();
        assert!(!s.is_done()); // not done yet
        s.tick();
        assert!(s.is_done());
        assert_eq!(s.current_logo(), LOGO_COUNT);
    }
}

//! The play-time clock: the per-frame advance and its stall guard.

use super::*;

/// The longest gap between two play-clock readings that still counts as
/// play ([`World::tick_play_clock`]). The hosts read once a tick, so anything
/// near this is a stall.
pub const PLAY_CLOCK_STALL_SECS: f64 = 5.0;

impl World {
    /// Advance the wall-clock play-time counter by `delta_seconds`. Engines
    /// drive this from the frame loop's wall-clock delta. Mirrors the
    /// retail "play time" field shown on the save screen.
    pub fn advance_play_time(&mut self, delta_seconds: u32) {
        self.clock.play_time_seconds = self.clock.play_time_seconds.saturating_add(delta_seconds);
    }

    /// Advance the play clock off a host wall-clock reading `now_secs` (any
    /// epoch - only deltas are used). Whole seconds only, and by delta
    /// against a high-water mark rather than absolutely, so a loaded save
    /// keeps its accumulated total.
    ///
    /// The one play-clock law both play hosts call. Each used to keep its own
    /// origin and high-water mark, and they drifted: the native window
    /// measured from window creation, so the title screen's time landed in
    /// the first delta after New Game; the browser page reset its origin on
    /// New Game but not its high-water mark, so after a second New Game play
    /// time stood still until the wall clock passed the old mark. The origin
    /// is set by the first call and dropped by [`Self::begin_new_game`].
    ///
    /// A reading more than [`PLAY_CLOCK_STALL_SECS`] after the previous one is
    /// a stalled host, not play: a paused or backgrounded browser tab, a
    /// minimised window, a suspended machine. Retail's counter is frames the
    /// game ran, so the gap is taken out of the origin and counts nothing;
    /// both hosts read every tick, and before this a page left paused for an
    /// hour came back an hour older on its save screen.
    pub fn tick_play_clock(&mut self, now_secs: f64) {
        if let (Some(origin), Some(last)) = (
            self.clock.play_clock_origin.as_mut(),
            self.clock.play_clock_last,
        ) {
            let gap = now_secs - last;
            if gap > PLAY_CLOCK_STALL_SECS {
                *origin += gap;
            }
        }
        self.clock.play_clock_last = Some(now_secs);
        let origin = *self.clock.play_clock_origin.get_or_insert(now_secs);
        let now = (now_secs - origin).max(0.0) as u32;
        if now > self.clock.play_clock_high_water {
            let delta = now - self.clock.play_clock_high_water;
            self.clock.play_clock_high_water = now;
            self.advance_play_time(delta);
        }
    }
}

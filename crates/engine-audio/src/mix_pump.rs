//! How far ahead of the output the browser's mix runs.
//!
//! The page mixes on its main thread and hands finished blocks to an
//! `AudioWorkletProcessor`, which only plays them ([`crate::WebAudioOut`]). A
//! cue reaches the mix at the next block, so it sounds behind everything
//! already handed over: the depth of that queue **is** the cue's latency, and
//! what it buys is riding out a main-thread stall. [`MixPump`] holds that trade
//! as plain arithmetic with no browser in it: it answers how many frames to mix
//! now, and moves the target to what the page's frames actually need.
//!
//! The measure is the queue's low point, which the output reports itself: the
//! main thread's own view is stale exactly when it matters, because reports stack
//! up behind a stall and are read in order. A page whose longest frame is 30 ms
//! sees the queue fall 30 ms below the target, and that is the depth it needs,
//! whatever the output has or has not starved yet. A low point under
//! [`LOW_WATER_MS`] raises the target at once by the shortfall, plus whatever the
//! output played dry; a whole quiet window whose low point stayed clear gives half
//! the excess back.
//!
//! Time is frames played, not a wall clock: a suspended context plays nothing,
//! so it asks for nothing and nothing here decays.

/// The floor: below about one 60 Hz frame the top-up cadence itself is what the
/// queue would have to ride out.
pub const TARGET_MIN_MS: u32 = 25;
/// Where it starts, so a page's first seconds do not pay for finding its depth.
pub const TARGET_START_MS: u32 = 60;
/// A page that drains past this has stalled outright (a synchronous scene
/// load), and no depth a player would accept rides that out. It is about what
/// the 4096-frame script processor's two buffers hold.
pub const TARGET_MAX_MS: u32 = 190;
/// The margin the low point has to keep.
const LOW_WATER_MS: u32 = 6;
/// The window a low point is taken over before the target comes down.
const WINDOW_SECS: u32 = 4;
/// Below this a top-up is not worth a message: the next report will ask again.
const MIN_BLOCK_FRAMES: u32 = 128;

/// The main thread's view of the output queue.
#[derive(Debug, Clone)]
pub struct MixPump {
    rate: u32,
    target: u32,
    sent: u64,
    played: u64,
    dry_frames: u64,
    window_from: u64,
    low_point: u64,
}

impl MixPump {
    pub fn new(device_rate: u32) -> Self {
        let target = frames(device_rate, TARGET_START_MS);
        Self {
            rate: device_rate,
            target,
            sent: 0,
            played: 0,
            dry_frames: 0,
            window_from: 0,
            low_point: u64::MAX,
        }
    }

    /// The output's report: frames it has played in total, frames it has had to
    /// play as silence because the queue was dry, and the fewest frames it held
    /// since the last report. Dry frames say how far past empty a stall ran,
    /// which the low point, stopped at zero, cannot.
    pub fn report(&mut self, played: u64, dry_frames: u64, low: u64) {
        self.played = played.min(self.sent);
        let dry = dry_frames.saturating_sub(self.dry_frames);
        self.dry_frames = self.dry_frames.max(dry_frames);
        let low_water = u64::from(frames(self.rate, LOW_WATER_MS));
        let (lowest, highest) = (
            u64::from(frames(self.rate, TARGET_MIN_MS)),
            u64::from(frames(self.rate, TARGET_MAX_MS)),
        );
        self.low_point = self.low_point.min(low);
        if low < low_water || dry > 0 {
            let target = u64::from(self.target) + low_water.saturating_sub(low) + dry;
            self.target = target.min(highest) as u32;
        }
        if self.played - self.window_from >= u64::from(self.rate * WINDOW_SECS) {
            if self.low_point > low_water {
                let excess = (self.low_point - low_water) / 2;
                self.target = u64::from(self.target).saturating_sub(excess).max(lowest) as u32;
            }
            self.window_from = self.played;
            self.low_point = u64::MAX;
        }
    }

    /// Frames to mix and hand over now; `0` when the queue is near enough full.
    pub fn take(&mut self) -> u32 {
        let want = u64::from(self.target).saturating_sub(self.depth()) as u32;
        if want < MIN_BLOCK_FRAMES {
            return 0;
        }
        self.sent += u64::from(want);
        want
    }

    /// Frames handed over and not yet played, as of the last report.
    pub fn depth(&self) -> u64 {
        self.sent - self.played
    }

    pub fn target(&self) -> u32 {
        self.target
    }

    pub fn dry_frames(&self) -> u64 {
        self.dry_frames
    }

    pub fn played(&self) -> u64 {
        self.played
    }
}

fn frames(rate: u32, ms: u32) -> u32 {
    rate * ms / 1000
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;
    const START: u32 = RATE * TARGET_START_MS / 1000;
    const FLOOR: u32 = RATE * TARGET_MIN_MS / 1000;
    const LOW: u64 = (RATE * LOW_WATER_MS / 1000) as u64;

    /// A page whose every top-up comes `gap` frames after the last: the queue
    /// is at its low point just before each, `gap` under the target.
    fn run(p: &mut MixPump, played: &mut u64, gap: u64, secs: u32) {
        for _ in 0..u64::from(RATE * secs) / gap {
            *played += gap;
            let low = p.depth().saturating_sub(gap);
            p.report(*played, 0, low);
            p.take();
        }
    }

    #[test]
    fn the_first_take_fills_the_queue_and_a_full_queue_asks_for_nothing() {
        let mut p = MixPump::new(RATE);
        assert_eq!(p.take(), START);
        assert_eq!(p.depth(), u64::from(START));
        assert_eq!(p.take(), 0);
    }

    #[test]
    fn a_top_up_replaces_what_was_played_and_small_gaps_wait() {
        let mut p = MixPump::new(RATE);
        p.take();
        p.report(127, 0, 2000);
        assert_eq!(p.take(), 0, "under one block");
        p.report(384, 0, 2000);
        assert_eq!(p.take(), 384);
        assert_eq!(p.depth(), u64::from(START));
    }

    #[test]
    fn a_suspended_output_asks_for_nothing_however_often_it_is_asked() {
        let mut p = MixPump::new(RATE);
        p.take();
        for _ in 0..100 {
            p.report(0, 0, u64::from(START));
            assert_eq!(p.take(), 0);
        }
        assert_eq!(p.target(), START);
    }

    #[test]
    fn a_page_that_keeps_up_comes_down_to_the_floor_and_stays() {
        let mut p = MixPump::new(RATE);
        p.take();
        let mut played = 0;
        run(&mut p, &mut played, 384, 60);
        assert_eq!(p.target(), FLOOR);
    }

    /// The queue falling under the margin raises the target by the shortfall,
    /// before the output has run dry.
    #[test]
    fn a_low_point_raises_the_target_before_the_output_starves() {
        let mut p = MixPump::new(RATE);
        p.take();
        p.report(100, 0, LOW - 50);
        assert_eq!(p.target(), START + 50);
    }

    /// Frames of a set length settle the target near that length plus the
    /// margin, however it got there.
    #[test]
    fn the_target_settles_on_what_the_pages_frames_need() {
        const FRAME: u64 = (RATE / 20) as u64; // 50 ms between top-ups
        let mut p = MixPump::new(RATE);
        p.take();
        let mut played = 0;
        run(&mut p, &mut played, FRAME, 60);
        let settled = u64::from(p.target());
        assert!(settled >= FRAME + LOW - 1, "{settled}");
        assert!(settled <= FRAME + 3 * LOW, "{settled}");
    }

    #[test]
    fn a_page_that_recovers_gives_the_latency_back_down_to_the_floor() {
        let mut p = MixPump::new(RATE);
        p.take();
        let mut played = 0;
        run(&mut p, &mut played, u64::from(RATE / 10), 10);
        assert!(p.target() > RATE / 10);
        run(&mut p, &mut played, 384, 120);
        assert_eq!(p.target(), FLOOR);
    }

    /// A stall that ran the queue dry for 60 ms asks for those 60 ms at once,
    /// even read off a report the main thread only reaches later.
    #[test]
    fn dry_frames_raise_the_target_by_how_far_past_empty_the_stall_ran() {
        let mut p = MixPump::new(RATE);
        p.take();
        p.report(u64::from(START), 2880, 0);
        assert_eq!(u64::from(p.target()), u64::from(START) + LOW + 2880);
        assert_eq!(p.dry_frames(), 2880);
    }

    #[test]
    fn a_stall_past_the_cap_stops_at_the_cap() {
        let mut p = MixPump::new(RATE);
        p.take();
        p.report(u64::from(START), u64::from(RATE), 0);
        assert_eq!(p.target(), frames(RATE, TARGET_MAX_MS));
    }

    #[test]
    fn a_report_past_what_was_sent_cannot_underflow_the_depth() {
        let mut p = MixPump::new(RATE);
        p.take();
        p.report(9999, 0, 0);
        assert_eq!(p.depth(), 0);
    }
}

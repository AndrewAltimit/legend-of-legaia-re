//! The play page's frame-step exports: the shared wall-clock to sim-tick rule
//! ([`legaia_engine_core::frame_step::SimStepper`]) the native window's redraw
//! drains through too.
//!
//! The page's animation loop fires at the display refresh. It used to keep
//! its own accumulator in JavaScript with its own clamp, beside a native
//! accumulator that clamped differently (a backlog past four ticks carried
//! into the next frame natively and was dropped on the page). One kernel now
//! answers "how many ticks does this frame run" for both.

use crate::runtime::LegaiaRuntime;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl LegaiaRuntime {
    /// How many sim ticks this display frame runs, given the wall time since
    /// the previous call in **milliseconds**. At most
    /// [`legaia_engine_core::frame_step::MAX_BACKLOG_TICKS`]; a longer gap is
    /// dropped, not carried.
    pub fn play_drain_sim_steps(&mut self, elapsed_ms: f64) -> u32 {
        self.sim_stepper.drain(elapsed_ms / 1000.0)
    }

    /// Drop the undrained backlog - the page is paused or single-stepping,
    /// and the gap must not come back as catch-up ticks.
    pub fn play_resync_sim_clock(&mut self) {
        self.sim_stepper.resync();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The export is the kernel: milliseconds in, the same tick counts the
    /// native window's `SimStepper::drain(secs)` returns.
    #[test]
    fn the_export_drains_through_the_shared_stepper() {
        let mut rt = LegaiaRuntime::new();
        let mut native = legaia_engine_core::frame_step::SimStepper::new();
        for ms in [8.0, 8.0, 16.7, 33.4, 250.0, 16.7, 6.9] {
            assert_eq!(rt.play_drain_sim_steps(ms), native.drain(ms / 1000.0));
        }
        rt.play_resync_sim_clock();
        assert_eq!(rt.play_drain_sim_steps(10.0), 0);
    }
}

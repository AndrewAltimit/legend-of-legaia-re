//! Opt-in per-frame timing breakdown for the windowed engine.
//!
//! Off by default and free when off: every entry point short-circuits on a
//! single cached `bool` read, so an instrumented call site costs one
//! well-predicted branch per frame when the profiler is disabled.
//!
//! Enable with `LEGAIA_PROFILE=1`. The profiler banks the wall time between
//! [`mark`] calls under the name passed to the *closing* mark, and prints a
//! rolling one-second summary to stderr:
//!
//! ```text
//! [profile]  143.2 fps over 143 frames | frame avg  6.98ms p50  6.90 p99  8.40 | tick 0.21 pose 1.90 ...
//! ```
//!
//! Companion knobs (each read once, at first use):
//!
//! * `LEGAIA_PROFILE_FRAMES=N` - print a final summary and exit the process
//!   after `N` frames. Turns `play-window` into a repeatable benchmark: same
//!   scene, same frame count, one number out.
//! * `LEGAIA_VSYNC=off` - configure the surface with an uncapped present mode
//!   (see [`Renderer::new`](crate::Renderer)). Without it the frame time is
//!   pinned to the display refresh interval, and a measurement reads the vsync
//!   wait rather than the engine's own cost.

use std::cell::RefCell;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

static ENABLED: OnceLock<bool> = OnceLock::new();
static FRAME_LIMIT: OnceLock<Option<u64>> = OnceLock::new();
static NO_VSYNC: OnceLock<bool> = OnceLock::new();

/// Whether `LEGAIA_PROFILE` is set. Cached after the first call.
#[inline]
pub fn enabled() -> bool {
    *ENABLED.get_or_init(|| std::env::var_os("LEGAIA_PROFILE").is_some())
}

fn frame_limit() -> Option<u64> {
    *FRAME_LIMIT.get_or_init(|| {
        std::env::var("LEGAIA_PROFILE_FRAMES")
            .ok()
            .and_then(|s| s.parse().ok())
    })
}

/// Whether the surface should be configured with an uncapped present mode.
/// Read by the [`Renderer`](crate::Renderer) constructor; it lives here so the
/// benchmark knobs stay in one place.
pub fn no_vsync() -> bool {
    *NO_VSYNC.get_or_init(|| {
        std::env::var("LEGAIA_VSYNC")
            .map(|v| v.eq_ignore_ascii_case("off") || v == "0")
            .unwrap_or(false)
    })
}

struct State {
    /// Start of the current frame (set by [`begin_frame`]).
    frame_start: Instant,
    /// End of the last closed stage.
    mark: Instant,
    /// Accumulated time per stage over the current print window, in first-seen
    /// order (stable output ordering, no map hashing on the hot path).
    stages: Vec<(&'static str, Duration)>,
    /// Per-frame totals over the current print window.
    frames: Vec<Duration>,
    window_start: Instant,
    total_frames: u64,
    /// Last-reported scene draw counts (see [`draw_counts`]).
    draws: (usize, usize),
}

/// Record this frame's scene draw-call counts (textured, untextured) so the
/// summary can attribute the encode cost. No-op when the profiler is off.
pub fn draw_counts(textured: usize, untextured: usize) {
    if !enabled() {
        return;
    }
    STATE.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            st.draws = (textured, untextured);
        }
    });
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Open a frame. Resets the stage cursor; the stage accumulators persist for
/// the whole one-second print window.
pub fn begin_frame() {
    if !enabled() {
        return;
    }
    STATE.with(|s| {
        let now = Instant::now();
        let mut s = s.borrow_mut();
        match s.as_mut() {
            Some(st) => {
                st.frame_start = now;
                st.mark = now;
            }
            None => {
                *s = Some(State {
                    frame_start: now,
                    mark: now,
                    stages: Vec::new(),
                    frames: Vec::new(),
                    window_start: now,
                    total_frames: 0,
                    draws: (0, 0),
                })
            }
        }
    });
}

/// Close the stage that ran since the previous [`begin_frame`] / [`mark`] and
/// bank its duration under `stage`.
pub fn mark(stage: &'static str) {
    if !enabled() {
        return;
    }
    STATE.with(|s| {
        let now = Instant::now();
        let mut s = s.borrow_mut();
        let Some(st) = s.as_mut() else { return };
        let dt = now.saturating_duration_since(st.mark);
        st.mark = now;
        match st.stages.iter_mut().find(|(n, _)| *n == stage) {
            Some((_, acc)) => *acc += dt,
            None => st.stages.push((stage, dt)),
        }
    });
}

/// Close a frame: bank its total and, once a second, print the rolling
/// breakdown. Honours `LEGAIA_PROFILE_FRAMES` by printing a final summary and
/// exiting the process.
pub fn end_frame() {
    if !enabled() {
        return;
    }
    let mut done = false;
    STATE.with(|s| {
        let now = Instant::now();
        let mut s = s.borrow_mut();
        let Some(st) = s.as_mut() else { return };
        st.frames
            .push(now.saturating_duration_since(st.frame_start));
        st.total_frames += 1;
        let window = now.saturating_duration_since(st.window_start);
        if frame_limit().is_some_and(|n| st.total_frames >= n) {
            eprintln!("{}", summary(st, window));
            done = true;
            return;
        }
        if window >= Duration::from_secs(1) {
            eprintln!("{}", summary(st, window));
            st.stages.clear();
            st.frames.clear();
            st.window_start = now;
        }
    });
    if done {
        std::process::exit(0);
    }
}

fn summary(st: &mut State, window: Duration) -> String {
    let n = st.frames.len().max(1);
    st.frames.sort_unstable();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    let pick = |q: f64| ms(st.frames[((n as f64 * q) as usize).min(n - 1)]);
    let total: Duration = st.frames.iter().sum();
    let fps = n as f64 / window.as_secs_f64().max(1e-9);
    let mut out = format!(
        "[profile] {fps:6.1} fps over {n} frames | frame avg {:5.2}ms p50 {:5.2} p99 {:5.2} | draws {}+{} |",
        ms(total) / n as f64,
        pick(0.50),
        pick(0.99),
        st.draws.0,
        st.draws.1,
    );
    for (name, acc) in &st.stages {
        out.push_str(&format!(" {name} {:.2}", ms(*acc) / n as f64));
    }
    out
}

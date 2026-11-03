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
//! * `LEGAIA_PROFILE_SKIP=N` - leave the first `N` frames out of the `[bench]`
//!   line's averages, so scene entry's own frames read as hitches rather than
//!   as the running cost.
//!
//! A run that names a frame limit ends on one `[bench]` line of JSON over every
//! frame past the skip. `scripts/ci/frame-bench.py` is its consumer. In a build
//! with `engine-shell`'s `bench-alloc` feature [`CountingAlloc`] is the global
//! allocator, and each stage carries its allocation count too;
//! `LEGAIA_PROFILE_ALLOC_SITES=1` there also samples allocation call stacks and
//! prints the heaviest at the end.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

static ENABLED: OnceLock<bool> = OnceLock::new();
static FRAME_LIMIT: OnceLock<Option<u64>> = OnceLock::new();
static NO_VSYNC: OnceLock<bool> = OnceLock::new();
static SKIP: OnceLock<u64> = OnceLock::new();
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);

/// A counting wrapper over the system allocator, for measurement builds: the
/// `bench-alloc` feature of `engine-shell` and of `web-viewer` install it. Every
/// thread counts, the audio callback's included.
pub struct CountingAlloc;

// SAFETY: every call forwards to `System` with the caller's own arguments; the
// counters are relaxed atomics that never allocate.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note_alloc(layout.size());
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note_alloc(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note_alloc(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

/// One allocation in every this many has its call stack kept, past the skip
/// (`LEGAIA_PROFILE_ALLOC_SITES=1`).
const SITE_SAMPLE: u64 = 97;
static SITES_ARMED: AtomicBool = AtomicBool::new(false);
static SITES: Mutex<Vec<std::backtrace::Backtrace>> = Mutex::new(Vec::new());
thread_local! {
    /// Set while a stack is being taken, whose own allocations must not sample.
    static IN_SITE: Cell<bool> = const { Cell::new(false) };
}

#[inline]
fn note_alloc(size: usize) {
    let n = ALLOCS.fetch_add(1, Ordering::Relaxed);
    ALLOC_BYTES.fetch_add(size as u64, Ordering::Relaxed);
    if SITES_ARMED.load(Ordering::Relaxed) && n.is_multiple_of(SITE_SAMPLE) {
        take_site();
    }
}

#[cold]
fn take_site() {
    let _ = IN_SITE.try_with(|g| {
        if g.replace(true) {
            return;
        }
        let bt = std::backtrace::Backtrace::force_capture();
        if let Ok(mut v) = SITES.lock() {
            v.push(bt);
        }
        g.set(false);
    });
}

/// Print the sampled allocation sites, heaviest first: for each stack its
/// innermost frames outside the allocator and `std`.
fn print_alloc_sites() {
    SITES_ARMED.store(false, Ordering::Relaxed);
    let Ok(v) = SITES.lock() else { return };
    let mut by_site: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for bt in v.iter() {
        let text = bt.to_string();
        let site: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with("at "))
            .filter_map(|l| l.split_once(": ").map(|(_, f)| f))
            .filter(|f| {
                ![
                    "std::", "core::", "alloc::", "__rust", "<alloc::", "<core::", "<std::",
                ]
                .iter()
                .any(|p| f.starts_with(p))
                    && !f.contains("::profile::")
            })
            .take(4)
            .collect();
        *by_site.entry(site.join(" < ")).or_default() += 1;
    }
    let mut rows: Vec<_> = by_site.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1));
    for (site, n) in rows.into_iter().take(40) {
        eprintln!("[alloc-site] {:7} {site}", n * SITE_SAMPLE);
    }
}

/// `(allocations, bytes)` since the process began: zero unless
/// [`CountingAlloc`] is the global allocator.
pub fn alloc_totals() -> (u64, u64) {
    (
        ALLOCS.load(Ordering::Relaxed),
        ALLOC_BYTES.load(Ordering::Relaxed),
    )
}

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
    /// Allocation count at the last mark.
    alloc_mark: u64,
    /// Allocation count at the frame's open.
    alloc_frame: u64,
    /// Whole-run totals past `LEGAIA_PROFILE_SKIP`, for the `[bench]` line.
    bench: Bench,
}

/// The `[bench]` line's accumulators.
#[derive(Default)]
struct Bench {
    /// `(stage, time, allocations)`, first-seen order.
    stages: Vec<(&'static str, Duration, u64)>,
    frames: Vec<Duration>,
    allocs: u64,
    bytes: u64,
    bytes_mark: u64,
    /// The worst frame inside the skipped span, and the first frame's time.
    entry_worst: Duration,
    first: Option<Duration>,
    /// This frame's stages so far, and the slowest measured frame's.
    current: Vec<(&'static str, Duration)>,
    worst: Option<(u64, Duration, Vec<(&'static str, Duration)>)>,
    draws_max: (usize, usize),
}

/// One step of process startup or scene entry: its time since the previous
/// call and since the first, as a `[startup]` line. No-op when the profiler is
/// off.
pub fn startup(step: &'static str) {
    if !enabled() {
        return;
    }
    static CLOCK: Mutex<Option<(Instant, Instant)>> = Mutex::new(None);
    let now = Instant::now();
    let Ok(mut c) = CLOCK.lock() else { return };
    let (first, last) = c.get_or_insert((now, now));
    eprintln!(
        "[startup] {step} {:.1} ms (at {:.1})",
        now.saturating_duration_since(*last).as_secs_f64() * 1e3,
        now.saturating_duration_since(*first).as_secs_f64() * 1e3,
    );
    *last = now;
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
                let (a, b) = alloc_totals();
                st.alloc_mark = a;
                st.alloc_frame = a;
                st.bench.bytes_mark = b;
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
                    alloc_mark: alloc_totals().0,
                    alloc_frame: alloc_totals().0,
                    bench: Bench {
                        bytes_mark: alloc_totals().1,
                        ..Bench::default()
                    },
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
        let allocs = alloc_totals().0;
        let da = allocs - st.alloc_mark;
        st.alloc_mark = allocs;
        st.bench.current.push((stage, dt));
        if st.total_frames >= skip_frames() {
            match st.bench.stages.iter_mut().find(|(n, ..)| *n == stage) {
                Some((_, t, a)) => {
                    *t += dt;
                    *a += da;
                }
                None => st.bench.stages.push((stage, dt, da)),
            }
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
    let mut first_frame = false;
    STATE.with(|s| {
        let now = Instant::now();
        let mut s = s.borrow_mut();
        let Some(st) = s.as_mut() else { return };
        st.frames
            .push(now.saturating_duration_since(st.frame_start));
        st.total_frames += 1;
        let frame = now.saturating_duration_since(st.frame_start);
        let (allocs, bytes) = alloc_totals();
        if st.bench.first.is_none() {
            st.bench.first = Some(frame);
            first_frame = true;
        }
        if st.total_frames == skip_frames()
            && std::env::var_os("LEGAIA_PROFILE_ALLOC_SITES").is_some()
        {
            SITES_ARMED.store(true, Ordering::Relaxed);
        }
        if st.total_frames > skip_frames() {
            st.bench.frames.push(frame);
            if st.bench.worst.as_ref().is_none_or(|w| frame > w.1) {
                let stages = std::mem::take(&mut st.bench.current);
                st.bench.worst = Some((st.total_frames, frame, stages));
            }
            st.bench.allocs += allocs - st.alloc_frame;
            st.bench.bytes += bytes - st.bench.bytes_mark;
            st.bench.draws_max.0 = st.bench.draws_max.0.max(st.draws.0);
            st.bench.draws_max.1 = st.bench.draws_max.1.max(st.draws.1);
        } else {
            st.bench.entry_worst = st.bench.entry_worst.max(frame);
        }
        st.bench.current.clear();
        let window = now.saturating_duration_since(st.window_start);
        if frame_limit().is_some_and(|n| st.total_frames >= n) {
            eprintln!("{}", summary(st, window));
            eprintln!("[bench] {}", bench_json(&mut st.bench));
            print_alloc_sites();
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
    if first_frame {
        startup("first frame");
    }
    if done {
        std::process::exit(0);
    }
}

fn skip_frames() -> u64 {
    *SKIP.get_or_init(|| {
        std::env::var("LEGAIA_PROFILE_SKIP")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    })
}

/// The whole-run summary as one JSON object.
fn bench_json(b: &mut Bench) -> String {
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    b.frames.sort_unstable();
    let n = b.frames.len().max(1);
    let pick = |q: f64| {
        b.frames
            .get(((n as f64 * q) as usize).min(n - 1))
            .map_or(0.0, |d| ms(*d))
    };
    let total: Duration = b.frames.iter().sum();
    let stages: Vec<String> = b
        .stages
        .iter()
        .map(|(name, t, a)| {
            format!(
                "\"{name}\":{{\"ms\":{:.4},\"allocs\":{:.2}}}",
                ms(*t) / n as f64,
                *a as f64 / n as f64
            )
        })
        .collect();
    let worst = b
        .worst
        .as_ref()
        .map_or("null".to_string(), |(frame, t, stages)| {
            let list: Vec<String> = stages
                .iter()
                .map(|(n, d)| format!("\"{n}\":{:.2}", ms(*d)))
                .collect();
            format!(
                "{{\"frame\":{frame},\"ms\":{:.2},\"stages\":{{{}}}}}",
                ms(*t),
                list.join(",")
            )
        });
    format!(
        "{{\"worst\":{worst},\"frames\":{},\"avg_ms\":{:.4},\"p50_ms\":{:.4},\"p99_ms\":{:.4},\"max_ms\":{:.4},\"first_ms\":{:.2},\"entry_worst_ms\":{:.2},\"allocs_per_frame\":{:.2},\"alloc_bytes_per_frame\":{:.0},\"draws\":[{},{}],\"stages\":{{{}}}}}",
        b.frames.len(),
        ms(total) / n as f64,
        pick(0.50),
        pick(0.99),
        pick(1.0),
        b.first.map_or(0.0, ms),
        ms(b.entry_worst),
        b.allocs as f64 / n as f64,
        b.bytes as f64 / n as f64,
        b.draws_max.0,
        b.draws_max.1,
        stages.join(","),
    )
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

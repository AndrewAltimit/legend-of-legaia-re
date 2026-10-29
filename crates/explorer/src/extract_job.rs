//! The extraction pipeline (`legaia_extract::run`) on a worker thread, with
//! its progress, log and outcome shared with the UI and a cancel flag.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use legaia_extract::{Cancelled, Options, Reporter, STEP_COUNT, Summary};

/// Lines kept in the log pane; older ones are dropped (verbose runs write
/// one line per file).
const LOG_CAP: usize = 20_000;

/// The prefix a warning's log line carries, so the log can be filtered to
/// warnings ([`Progress::warning_lines`]).
pub const WARNING_PREFIX: &str = "warning: ";

#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub step: usize,
    pub step_name: String,
    pub done: usize,
    pub total: usize,
    pub log: Vec<String>,
    pub warnings: usize,
    /// When the current step started (`None` before the first one).
    pub step_started: Option<Instant>,
}

impl Progress {
    /// Overall completion in `0..=1` of a run of `steps` steps: whole steps
    /// plus the current step's fraction.
    pub fn fraction(&self, steps: usize) -> f32 {
        let within = if self.total > 0 {
            self.done as f32 / self.total as f32
        } else {
            0.0
        };
        ((self.step as f32 + within) / steps.max(1) as f32).clamp(0.0, 1.0)
    }

    /// The time left in the current step, extrapolated from its own rate so
    /// far. `None` until the step has run long enough, and done enough, for
    /// the rate to mean something. There is no estimate for the whole run:
    /// the steps differ too much in cost for one to be honest.
    pub fn step_remaining(&self, now: Instant) -> Option<Duration> {
        let started = self.step_started?;
        let spent = now.saturating_duration_since(started);
        if self.total == 0 || self.done == 0 || self.done >= self.total {
            return None;
        }
        if spent < Duration::from_secs(2) || self.done * 20 < self.total {
            return None;
        }
        let left = (self.total - self.done) as f64 / self.done as f64;
        Some(spent.mul_f64(left))
    }

    /// The log lines that are warnings.
    pub fn warning_lines(&self) -> impl Iterator<Item = &String> {
        self.log.iter().filter(|l| l.starts_with(WARNING_PREFIX))
    }
}

/// How many steps a run with `options` reports: the fixed ones, plus the
/// every-texture sweep when it is asked for.
pub fn step_count(options: &Options) -> usize {
    STEP_COUNT + usize::from(options.all_textures)
}

/// A duration the way the progress line words it: `45 s`, `3 min 05 s`,
/// `1 h 02 min`.
pub fn human_duration(d: Duration) -> String {
    let s = d.as_secs();
    if s < 60 {
        format!("{s} s")
    } else if s < 3600 {
        format!("{} min {:02} s", s / 60, s % 60)
    } else {
        format!("{} h {:02} min", s / 3600, (s % 3600) / 60)
    }
}

/// What the chosen output folder holds, for the note under the field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutDir {
    /// Not there yet: the run creates it.
    New,
    /// There and empty.
    Empty,
    /// There with files in it, which a run overwrites where names collide.
    HasFiles,
    /// A file, not a folder.
    NotAFolder,
}

impl OutDir {
    pub fn of(path: &Path) -> Self {
        if !path.exists() {
            OutDir::New
        } else if !path.is_dir() {
            OutDir::NotAFolder
        } else if std::fs::read_dir(path).is_ok_and(|mut d| d.next().is_none()) {
            OutDir::Empty
        } else {
            OutDir::HasFiles
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Done(Summary),
    Cancelled,
    Failed(String),
}

struct Shared {
    progress: Mutex<Progress>,
    cancel: AtomicBool,
    outcome: Mutex<Option<Outcome>>,
    /// How long the run took, set with the outcome.
    took: Mutex<Option<Duration>>,
}

impl Shared {
    fn new() -> Self {
        Self {
            progress: Mutex::new(Progress::default()),
            cancel: AtomicBool::new(false),
            outcome: Mutex::new(None),
            took: Mutex::new(None),
        }
    }

    fn push(&self, line: String) {
        let mut p = self.progress.lock().unwrap_or_else(|e| e.into_inner());
        if p.log.len() >= LOG_CAP {
            p.log.drain(..LOG_CAP / 10);
        }
        p.log.push(line);
    }
}

impl Reporter for Shared {
    fn log(&self, msg: &str) {
        self.push(msg.to_string());
    }
    fn warn(&self, msg: &str) {
        self.progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .warnings += 1;
        self.push(format!("{WARNING_PREFIX}{msg}"));
    }
    fn step(&self, index: usize, name: &str) {
        let mut p = self.progress.lock().unwrap_or_else(|e| e.into_inner());
        p.step = index;
        p.step_name = name.to_string();
        p.done = 0;
        p.total = 0;
        p.step_started = Some(Instant::now());
    }
    fn progress(&self, done: usize, total: usize) {
        let mut p = self.progress.lock().unwrap_or_else(|e| e.into_inner());
        // Parallel steps report out of order; keep the high-water mark.
        if total != p.total || done > p.done {
            p.done = done;
            p.total = total;
        }
    }
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// A running (or finished) extraction.
pub struct ExtractJob {
    pub options: Options,
    shared: Arc<Shared>,
    handle: Option<JoinHandle<()>>,
    started: Instant,
    /// The outcome has been handed out once ([`Self::take_finished`]).
    announced: bool,
}

impl ExtractJob {
    pub fn start(options: Options) -> Self {
        let shared = Arc::new(Shared::new());
        let worker = Arc::clone(&shared);
        let opts = options.clone();
        let started = Instant::now();
        let handle = std::thread::Builder::new()
            .name("legaia-extract".into())
            .spawn(move || {
                let outcome = match legaia_extract::run(&opts, worker.as_ref()) {
                    Ok(s) => Outcome::Done(s),
                    Err(e) if e.is::<Cancelled>() => Outcome::Cancelled,
                    Err(e) => Outcome::Failed(format!("{e:#}")),
                };
                let line = match &outcome {
                    Outcome::Done(_) => "finished".to_string(),
                    Outcome::Cancelled => "cancelled - files written so far are kept".to_string(),
                    Outcome::Failed(e) => format!("FAILED: {e}"),
                };
                worker.push(line);
                *worker.took.lock().unwrap_or_else(|e| e.into_inner()) = Some(started.elapsed());
                *worker.outcome.lock().unwrap_or_else(|e| e.into_inner()) = Some(outcome);
            })
            .expect("spawn extraction thread");
        Self {
            options,
            shared,
            handle: Some(handle),
            started,
            announced: false,
        }
    }

    pub fn cancel(&self) {
        self.shared.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelling(&self) -> bool {
        self.shared.cancel.load(Ordering::Relaxed) && self.outcome().is_none()
    }

    pub fn progress(&self) -> Progress {
        self.shared
            .progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// `Some` once the worker has finished.
    pub fn outcome(&self) -> Option<Outcome> {
        self.shared
            .outcome
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// How long the run has taken: so far while it runs, in total once it
    /// has finished.
    pub fn elapsed(&self) -> Duration {
        self.shared
            .took
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .unwrap_or_else(|| self.started.elapsed())
    }

    /// The outcome, the first time it is asked for once the run has
    /// finished; `None` before that and after. The app announces a finished
    /// run once, on whatever view is open.
    pub fn take_finished(&mut self) -> Option<Outcome> {
        if self.announced {
            return None;
        }
        let o = self.outcome()?;
        self.announced = true;
        Some(o)
    }

    /// Block until the worker exits (tests, and app shutdown).
    pub fn join(&mut self) -> Option<Outcome> {
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        self.outcome()
    }
}

impl Drop for ExtractJob {
    fn drop(&mut self) {
        // Closing the window mid-run stops the worker at its next check.
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_in_the_largest_sensible_unit() {
        assert_eq!(human_duration(Duration::from_secs(0)), "0 s");
        assert_eq!(human_duration(Duration::from_secs(59)), "59 s");
        assert_eq!(human_duration(Duration::from_secs(185)), "3 min 05 s");
        assert_eq!(human_duration(Duration::from_secs(3720)), "1 h 02 min");
    }

    #[test]
    fn step_estimate_waits_for_a_meaningful_rate() {
        let t0 = Instant::now();
        let mut p = Progress {
            total: 100,
            done: 50,
            step_started: Some(t0),
            ..Default::default()
        };
        // Too early in wall time.
        assert_eq!(p.step_remaining(t0 + Duration::from_secs(1)), None);
        // Half done in 10 s: about 10 s left.
        assert_eq!(
            p.step_remaining(t0 + Duration::from_secs(10)),
            Some(Duration::from_secs(10))
        );
        // Too little done for a rate.
        p.done = 2;
        assert_eq!(p.step_remaining(t0 + Duration::from_secs(10)), None);
        // A finished step, or one with no unit count, has no estimate.
        p.done = 100;
        assert_eq!(p.step_remaining(t0 + Duration::from_secs(10)), None);
        p.total = 0;
        assert_eq!(p.step_remaining(t0 + Duration::from_secs(10)), None);
    }

    #[test]
    fn fraction_spans_the_steps_the_run_has() {
        let steps = step_count(&Options::default());
        assert_eq!(steps, STEP_COUNT);
        let last = Progress {
            step: steps - 1,
            done: 1,
            total: 1,
            ..Default::default()
        };
        assert_eq!(last.fraction(steps), 1.0, "the last step's end is 100%");
        let all = Options {
            all_textures: true,
            ..Default::default()
        };
        assert_eq!(step_count(&all), STEP_COUNT + 1);
        assert!(last.fraction(step_count(&all)) < 1.0);
    }

    #[test]
    fn warnings_are_tagged_and_filterable() {
        let shared = Shared::new();
        shared.log("step one");
        shared.warn("channel 3 skipped");
        let p = shared.progress.lock().unwrap().clone();
        assert_eq!(p.warnings, 1);
        let w: Vec<_> = p.warning_lines().collect();
        assert_eq!(w, ["warning: channel 3 skipped"]);
    }

    #[test]
    fn output_folder_states() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(OutDir::of(&root.join("new")), OutDir::New);
        std::fs::create_dir(root.join("out")).unwrap();
        assert_eq!(OutDir::of(&root.join("out")), OutDir::Empty);
        std::fs::write(root.join("out").join("a.png"), b"").unwrap();
        assert_eq!(OutDir::of(&root.join("out")), OutDir::HasFiles);
        assert_eq!(
            OutDir::of(&root.join("out").join("a.png")),
            OutDir::NotAFolder
        );
    }
}

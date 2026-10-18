//! The extraction pipeline (`legaia_extract::run`) on a worker thread, with
//! its progress, log and outcome shared with the UI and a cancel flag.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use legaia_extract::{Cancelled, Options, Reporter, STEP_COUNT, Summary};

/// Lines kept in the log pane; older ones are dropped (verbose runs write
/// one line per file).
const LOG_CAP: usize = 20_000;

#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub step: usize,
    pub step_name: String,
    pub done: usize,
    pub total: usize,
    pub log: Vec<String>,
    pub warnings: usize,
}

impl Progress {
    /// Overall completion in `0..=1`: whole steps plus the current step's
    /// fraction.
    pub fn fraction(&self) -> f32 {
        let within = if self.total > 0 {
            self.done as f32 / self.total as f32
        } else {
            0.0
        };
        ((self.step as f32 + within) / (STEP_COUNT as f32 + 1.0)).clamp(0.0, 1.0)
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
}

impl Shared {
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
        self.push(msg.to_string());
    }
    fn step(&self, index: usize, name: &str) {
        let mut p = self.progress.lock().unwrap_or_else(|e| e.into_inner());
        p.step = index;
        p.step_name = name.to_string();
        p.done = 0;
        p.total = 0;
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
}

impl ExtractJob {
    pub fn start(options: Options) -> Self {
        let shared = Arc::new(Shared {
            progress: Mutex::new(Progress::default()),
            cancel: AtomicBool::new(false),
            outcome: Mutex::new(None),
        });
        let worker = Arc::clone(&shared);
        let opts = options.clone();
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
                *worker.outcome.lock().unwrap_or_else(|e| e.into_inner()) = Some(outcome);
            })
            .expect("spawn extraction thread");
        Self {
            options,
            shared,
            handle: Some(handle),
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

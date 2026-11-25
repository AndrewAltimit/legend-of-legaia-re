//! Branch-flip mutation: invert one conditional branch of one routine image
//! on the retail side, so a differential that still passes shows the arm's
//! effect is not in what it compares.
//!
//! Compiled in by the `branch-flip` feature, and off unless
//! `PSX_RETAIL_EXEC_FLIP` names a branch:
//!
//! ```text
//! PSX_RETAIL_EXEC_FLIP=<entry>:<image-hash>:<branch-pc>:<taken|fell>:<n|all>
//! ```
//!
//! `entry` and `image-hash` are a routine image as the
//! [`branch_cov`](crate::branch_cov) table names it (the hash of the
//! [`ledger::BODY_LEN`](crate::ledger::BODY_LEN) bytes at the entry), so a
//! slot-B VA is flipped only while that overlay is resident. Each time the
//! run executes the branch at `branch-pc` and it would go the named way
//! (`taken`, or `fell` through), that is an execution of the direction; the
//! `n`-th one (1-based), or every one with `all`, goes the other way instead.
//! The interpreter's registers, the delay slot and the link of
//! `bltzal` / `bgezal` are as the hardware leaves them; only the next PC
//! changes. The Rust side of a differential never runs here, so the mutation
//! is retail's alone.
//!
//! With `PSX_RETAIL_EXEC_FLIP_LOG` naming a file, each thread that ran the
//! branch appends one line when it ends (a libtest thread is named after its
//! test):
//!
//! ```text
//! F  test  executions-of-the-direction  flipped  abnormal-stops
//! ```
//!
//! An abnormal stop is a [`Cpu::call`](crate::Cpu::call) or
//! [`Cpu::resume`](crate::Cpu::resume) that flipped and then ended other than
//! by returning (the step budget, a bad access, ...): a mutant that runs away
//! is detected, but not by the comparison. `scripts/ci/checked-arms.py`
//! drives the runs.

use crate::Stop;
use std::cell::RefCell;
use std::io::Write;
use std::sync::{Mutex, OnceLock};

/// Which executions of the direction go the other way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    Nth(u64),
    All,
}

/// The branch a run flips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    pub entry: u32,
    pub image: u64,
    pub pc: u32,
    /// The direction flipped: `true` for taken.
    pub taken: bool,
    pub which: Which,
}

impl Spec {
    /// Parse `entry:image:pc:taken|fell:n|all` (hex addresses and hash).
    pub fn parse(s: &str) -> Option<Spec> {
        let f: Vec<&str> = s.trim().split(':').collect();
        if f.len() != 5 {
            return None;
        }
        let hex = |t: &str| u64::from_str_radix(t.trim_start_matches("0x"), 16).ok();
        let taken = match f[3] {
            "taken" => true,
            "fell" => false,
            _ => return None,
        };
        let which = match f[4] {
            "all" => Which::All,
            n => Which::Nth(n.parse().ok().filter(|&n| n > 0)?),
        };
        Some(Spec {
            entry: u32::try_from(hex(f[0])?).ok()?,
            image: hex(f[1])?,
            pc: u32::try_from(hex(f[2])?).ok()?,
            taken,
            which,
        })
    }
}

fn spec() -> Option<&'static Spec> {
    static SPEC: OnceLock<Option<Spec>> = OnceLock::new();
    SPEC.get_or_init(|| {
        let s = std::env::var("PSX_RETAIL_EXEC_FLIP").ok()?;
        let spec = Spec::parse(&s);
        assert!(spec.is_some(), "PSX_RETAIL_EXEC_FLIP: cannot parse {s:?}");
        spec
    })
    .as_ref()
}

/// The branch to flip: built with the `branch-flip` feature and the variable
/// set. Without the feature this is a constant `None`, so the interpreter's
/// loop carries no trace of the mutation.
pub fn active() -> Option<&'static Spec> {
    if cfg!(feature = "branch-flip") {
        spec()
    } else {
        None
    }
}

fn log() -> Option<&'static Mutex<std::fs::File>> {
    static FILE: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
    FILE.get_or_init(|| {
        let path = std::env::var_os("PSX_RETAIL_EXEC_FLIP_LOG")?;
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
            .map(Mutex::new)
    })
    .as_ref()
}

#[derive(Default)]
struct Count {
    seen: u64,
    flipped: u64,
    abnormal: u64,
    /// Flips since the current call began.
    in_call: u64,
}

impl Drop for Count {
    fn drop(&mut self) {
        if self.seen == 0 {
            return;
        }
        let Some(f) = log() else { return };
        let thread = std::thread::current();
        let line = format!(
            "F\t{}\t{}\t{}\t{}\n",
            thread.name().unwrap_or("?"),
            self.seen,
            self.flipped,
            self.abnormal
        );
        if let Ok(mut f) = f.lock() {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

thread_local! {
    static COUNT: RefCell<Count> = RefCell::new(Count::default());
}

/// Whether this execution of the branch at `spec.pc`, about to go the way
/// `taken` says, goes the other way. `body` is the bytes at `spec.entry`.
pub(crate) fn flips(spec: &Spec, body: &[u8], taken: bool) -> bool {
    if taken != spec.taken || crate::branch_cov::image_hash(body) != spec.image {
        return false;
    }
    COUNT.with(|c| {
        let mut c = c.borrow_mut();
        c.seen += 1;
        let flip = match spec.which {
            Which::All => true,
            Which::Nth(n) => c.seen == n,
        };
        if flip {
            c.flipped += 1;
            c.in_call += 1;
        }
        flip
    })
}

/// A call or resume is about to run: its flips start from zero.
pub(crate) fn begin_call() {
    COUNT.with(|c| c.borrow_mut().in_call = 0);
}

/// A call or resume ended with `stop`: an abnormal end after a flip is noted.
pub(crate) fn end_call(stop: &Stop) {
    COUNT.with(|c| {
        let mut c = c.borrow_mut();
        if c.in_call > 0 && *stop != Stop::Returned {
            c.abnormal += 1;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spec_parses() {
        assert_eq!(
            Spec::parse("801d0748:00ab:801d0760:taken:all"),
            Some(Spec {
                entry: 0x801D_0748,
                image: 0xAB,
                pc: 0x801D_0760,
                taken: true,
                which: Which::All,
            })
        );
        assert_eq!(
            Spec::parse("8001:1:8004:fell:2").map(|s| (s.taken, s.which)),
            Some((false, Which::Nth(2)))
        );
        assert_eq!(Spec::parse("8001:1:8004:fell:0"), None);
        assert_eq!(Spec::parse("8001:1:8004:up:1"), None);
    }
}

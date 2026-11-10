//! Which routines this thread has run, for a test that has to prove it ran
//! the routine it claims to compare.
//!
//! Every [`Cpu::call`](crate::Cpu::call) notes its entry address, the first
//! [`BODY_LEN`] bytes at that address and whether it returned, in a
//! thread-local table keyed by entry. A test harness reads the table back
//! ([`entered_since`]) to assert that a declared routine really was the one
//! called, and that the bytes it ran are the image's it names.
//!
//! With `PSX_RETAIL_EXEC_LEDGER` set to a path, the first call of each entry
//! on each thread is also appended to that file as one tab-separated line
//! `thread  entry  body-hex`. A libtest thread is named after its test, so the
//! file maps tests to the routines they entered. The body bytes are the
//! executable's own: the file belongs under a gitignored directory and is
//! never committed.

use crate::Stop;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::{Mutex, OnceLock};

/// How many bytes at the entry a record keeps: enough to find the routine's
/// image by searching for them.
pub const BODY_LEN: usize = 32;

/// Every [`Cpu::call`](crate::Cpu::call) of one entry on this thread.
#[derive(Debug, Clone)]
pub struct Entered {
    pub entry: u32,
    /// The bytes at the entry on the latest call.
    pub body: Vec<u8>,
    pub calls: u64,
    /// How many of those calls returned to the sentinel.
    pub returned: u64,
    /// The thread's call counter at the latest call, for [`entered_since`].
    last: u64,
}

#[derive(Default)]
struct Table {
    seq: u64,
    by_entry: BTreeMap<u32, Entered>,
}

thread_local! {
    static TABLE: RefCell<Table> = RefCell::new(Table::default());
}

fn file() -> Option<&'static Mutex<std::fs::File>> {
    static FILE: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
    FILE.get_or_init(|| {
        let path = std::env::var_os("PSX_RETAIL_EXEC_LEDGER")?;
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
            .map(Mutex::new)
    })
    .as_ref()
}

pub(crate) fn record(entry: u32, body: Vec<u8>, stop: &Stop) {
    let first = TABLE.with(|t| {
        let mut t = t.borrow_mut();
        t.seq += 1;
        let seq = t.seq;
        let e = t.by_entry.entry(entry).or_insert_with(|| Entered {
            entry,
            body: Vec::new(),
            calls: 0,
            returned: 0,
            last: 0,
        });
        let first = e.calls == 0;
        e.calls += 1;
        e.returned += u64::from(*stop == Stop::Returned);
        e.last = seq;
        e.body.clone_from(&body);
        first
    });
    if first && let Some(f) = file() {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("?");
        let hex: String = body.iter().map(|b| format!("{b:02x}")).collect();
        if let Ok(mut f) = f.lock() {
            let _ = writeln!(f, "{name}\t{entry:08x}\t{hex}");
        }
    }
}

/// This thread's call counter: a mark for [`entered_since`].
pub fn mark() -> u64 {
    TABLE.with(|t| t.borrow().seq)
}

/// Every entry this thread called after `mark`, in address order.
pub fn entered_since(mark: u64) -> Vec<Entered> {
    TABLE.with(|t| {
        t.borrow()
            .by_entry
            .values()
            .filter(|e| e.last > mark)
            .cloned()
            .collect()
    })
}

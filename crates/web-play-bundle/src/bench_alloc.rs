//! The measurement build's counting allocator (`bench-alloc`): the browser twin
//! of `engine-render`'s `profile::CountingAlloc`, which the wasm bundle does not
//! link. Read by `scripts/ci/frame-bench-web.mjs`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};
use wasm_bindgen::prelude::*;

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

struct Counting;

// SAFETY: every call forwards to `System` with the caller's own arguments.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(l.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(l.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(n as u64, Ordering::Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// `[allocations, bytes]` since the module loaded.
#[wasm_bindgen]
pub fn bench_alloc_totals() -> Vec<f64> {
    vec![
        ALLOCS.load(Ordering::Relaxed) as f64,
        BYTES.load(Ordering::Relaxed) as f64,
    ]
}

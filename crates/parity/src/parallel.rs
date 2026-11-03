//! Running one corpus's states on several threads.

/// Stack of a corpus worker: a state's seed runs whole engine sessions, which
/// on a single-threaded run have the main thread's stack to use.
const WORKER_STACK: usize = 64 * 1024 * 1024;

/// `f` over `items` on up to `jobs` threads, results in input order; `jobs`
/// of `0` or `1` runs on this thread. A panic in `f` propagates when the
/// threads join.
pub fn parallel_map<T: Sync, R: Send>(
    items: &[T],
    jobs: usize,
    f: impl Fn(&T) -> R + Sync,
) -> Vec<R> {
    if jobs <= 1 {
        return items.iter().map(f).collect();
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut out: Vec<(usize, R)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..jobs.min(items.len()))
            .map(|_| {
                std::thread::Builder::new()
                    .stack_size(WORKER_STACK)
                    .spawn_scoped(scope, || {
                        let mut got = Vec::new();
                        loop {
                            let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some(item) = items.get(i) else {
                                return got;
                            };
                            got.push((i, f(item)));
                        }
                    })
                    .expect("spawn a corpus worker")
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
            .collect()
    });
    out.sort_by_key(|(i, _)| *i);
    out.into_iter().map(|(_, r)| r).collect()
}

/// The core count a CPU-only corpus pass uses by default.
pub fn default_jobs() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get().min(8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_keep_input_order_at_any_thread_count() {
        let items: Vec<u32> = (0..500).collect();
        let want: Vec<u32> = items.iter().map(|i| i * 3).collect();
        for jobs in [0, 1, 2, 8, 1000] {
            assert_eq!(parallel_map(&items, jobs, |i| i * 3), want);
        }
    }

    #[test]
    #[should_panic(expected = "state 7")]
    fn a_panic_in_a_worker_reaches_the_caller() {
        let items: Vec<u32> = (0..16).collect();
        parallel_map(&items, 4, |i| {
            assert_ne!(*i, 7, "state 7");
        });
    }
}

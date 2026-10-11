//! Bisect helper: given two save states A (good) and B (bad), and a
//! sequence of intermediate states (mc1, mc2, ..., mc_n), find the first
//! state in which a specific PSX address has the "bad" value.
//!
//! The user records states at progressive points during a sequence (area
//! load, level-up, battle action). Bisect tells them which neighbouring
//! pair brackets the write - that's the pair to drill into with a Ghidra
//! function-search on writers of the address in the captured overlay.

use crate::extract::ram_slice;

/// Outcome of a bisect run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BisectOutcome {
    /// The target address held the "good" value in every captured state.
    NeverBecameBad,
    /// The target address held the "bad" value in every captured state.
    AlreadyBadFromStart,
    /// The transition was bracketed between two adjacent states. The
    /// `before` state's value was good; the `after` state's value was bad.
    /// `before_idx` indexes into the input slice.
    BracketedAt { before_idx: usize, after_idx: usize },
}

/// Find when `target_addr` transitioned from a "good" predicate to a "bad"
/// predicate over a sequence of RAM snapshots.
///
/// `predicate_bad` returns `true` if the value at `target_addr` represents
/// the post-transition (post-write) state. The function walks the snapshots
/// linearly and reports the first index at which the predicate flipped to
/// `true`.
pub fn bisect_first_bad(
    snapshots: &[(&str, &[u8])],
    target_addr: u32,
    predicate_bad: impl Fn(u32) -> bool,
) -> BisectOutcome {
    if snapshots.is_empty() {
        return BisectOutcome::NeverBecameBad;
    }
    let read = |ram: &[u8]| -> u32 {
        let bytes = ram_slice(ram, target_addr, target_addr + 4).expect("target_addr in main RAM");
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    };

    let first_val = read(snapshots[0].1);
    if predicate_bad(first_val) {
        return BisectOutcome::AlreadyBadFromStart;
    }

    let mut last_good = 0usize;
    for (i, (_, ram)) in snapshots.iter().enumerate().skip(1) {
        let v = read(ram);
        if predicate_bad(v) {
            return BisectOutcome::BracketedAt {
                before_idx: last_good,
                after_idx: i,
            };
        }
        last_good = i;
    }
    BisectOutcome::NeverBecameBad
}

/// Trace the value at `target_addr` across every snapshot - useful for
/// quickly seeing how a field evolves.
pub fn trace_addr(snapshots: &[(&str, &[u8])], target_addr: u32) -> Vec<(String, u32)> {
    snapshots
        .iter()
        .map(|(label, ram)| {
            let bytes =
                ram_slice(ram, target_addr, target_addr + 4).expect("target_addr in main RAM");
            let v = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            ((*label).to_owned(), v)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::{PSX_RAM_KSEG0, PSX_RAM_SIZE};

    fn ram_with(addr: u32, value: u32) -> Vec<u8> {
        let mut ram = vec![0u8; PSX_RAM_SIZE];
        let off = (addr - PSX_RAM_KSEG0) as usize;
        ram[off..off + 4].copy_from_slice(&value.to_le_bytes());
        ram
    }

    #[test]
    fn never_became_bad_with_zero_snapshots() {
        let outcome = bisect_first_bad(&[], 0x80100000, |_| true);
        assert_eq!(outcome, BisectOutcome::NeverBecameBad);
    }

    #[test]
    fn detects_already_bad() {
        let r = ram_with(0x80100000, 0xFFFFFFFF);
        let outcome = bisect_first_bad(&[("a", &r)], 0x80100000, |v| v != 0);
        assert_eq!(outcome, BisectOutcome::AlreadyBadFromStart);
    }

    #[test]
    fn brackets_transition_correctly() {
        let r0 = ram_with(0x80100000, 0);
        let r1 = ram_with(0x80100000, 0);
        let r2 = ram_with(0x80100000, 0x12345678);
        let r3 = ram_with(0x80100000, 0x12345678);
        let snaps: &[(&str, &[u8])] = &[("mc0", &r0), ("mc1", &r1), ("mc2", &r2), ("mc3", &r3)];
        let outcome = bisect_first_bad(snaps, 0x80100000, |v| v != 0);
        assert_eq!(
            outcome,
            BisectOutcome::BracketedAt {
                before_idx: 1,
                after_idx: 2
            }
        );
    }

    #[test]
    fn never_became_bad_when_all_good() {
        let r0 = ram_with(0x80100000, 0);
        let r1 = ram_with(0x80100000, 0);
        let snaps: &[(&str, &[u8])] = &[("a", &r0), ("b", &r1)];
        let outcome = bisect_first_bad(snaps, 0x80100000, |v| v != 0);
        assert_eq!(outcome, BisectOutcome::NeverBecameBad);
    }

    #[test]
    fn trace_returns_value_per_snapshot() {
        let r0 = ram_with(0x80100000, 0xAA);
        let r1 = ram_with(0x80100000, 0xBB);
        let snaps: &[(&str, &[u8])] = &[("first", &r0), ("second", &r1)];
        let trace = trace_addr(snaps, 0x80100000);
        assert_eq!(trace, vec![("first".into(), 0xAA), ("second".into(), 0xBB)]);
    }
}

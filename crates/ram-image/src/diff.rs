//! Watchpoint-equivalent diff between two save-state RAM snapshots.
//!
//! The "memory breakpoint" workflow in PCSX-Redux/Mednafen is interactive:
//! you set a watchpoint on an address, run the game, and the emulator pauses
//! when something writes there. We don't get that interactivity from
//! mednafen's CLI - but we can do the next best thing: take save states at
//! before/after points and diff their main RAM.
//!
//! Any byte that changed between A and B was written by code that ran in
//! between. Cluster the changed bytes into contiguous regions and you have a
//! ranked candidate list of structures to investigate (with addresses you
//! can hand back to Ghidra to look up writers).
//!
//! See `docs/tooling/mednafen-automation.md` for the workflow.

use crate::extract::{PSX_RAM_KSEG0, PSX_RAM_SIZE};
use serde::{Deserialize, Serialize};

/// Per-byte (or per-aligned-word) diff between two snapshots' main RAM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RamDiff {
    pub left_label: String,
    pub right_label: String,
    pub regions: Vec<RegionDiff>,
    pub total_bytes_changed: usize,
}

/// One contiguous run of changed bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionDiff {
    pub start_addr: u32,
    pub end_addr: u32,
    pub bytes_changed: usize,
    /// First 16 changed bytes for fingerprinting (truncated for very long runs).
    pub left_sample: Vec<u8>,
    pub right_sample: Vec<u8>,
}

impl RegionDiff {
    pub fn len(&self) -> usize {
        (self.end_addr - self.start_addr) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.start_addr == self.end_addr
    }
}

/// Filter knobs for [`diff_ram`].
#[derive(Debug, Clone)]
pub struct DiffOptions {
    /// Only consider this PSX virtual-address window. Defaults to all of
    /// main RAM (`0x80000000..0x80200000`).
    pub window: (u32, u32),
    /// Merge two changed regions if they're closer than this many bytes.
    /// Defaults to 16 (one cache line) - small enough to keep separate
    /// structures distinct, large enough to coalesce a 32-byte struct
    /// where some fields stayed unchanged.
    pub merge_gap: usize,
    /// Drop regions smaller than this many changed bytes. Defaults to 1.
    pub min_bytes_changed: usize,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self {
            window: (PSX_RAM_KSEG0, PSX_RAM_KSEG0 + PSX_RAM_SIZE as u32),
            merge_gap: 16,
            min_bytes_changed: 1,
        }
    }
}

/// Diff two main-RAM snapshots. Both must be exactly [`PSX_RAM_SIZE`] bytes.
pub fn diff_ram(
    left: &[u8],
    right: &[u8],
    left_label: &str,
    right_label: &str,
    opts: &DiffOptions,
) -> RamDiff {
    assert_eq!(left.len(), PSX_RAM_SIZE, "left must be 2 MiB main RAM");
    assert_eq!(right.len(), PSX_RAM_SIZE, "right must be 2 MiB main RAM");

    let win_lo = opts.window.0.saturating_sub(PSX_RAM_KSEG0) as usize;
    let win_hi = opts
        .window
        .1
        .saturating_sub(PSX_RAM_KSEG0)
        .min(PSX_RAM_SIZE as u32) as usize;

    let mut regions: Vec<RegionDiff> = Vec::new();
    let mut total_bytes_changed = 0usize;
    let mut cur: Option<(usize, usize)> = None;

    for i in win_lo..win_hi {
        if left[i] != right[i] {
            total_bytes_changed += 1;
            match cur.as_mut() {
                Some(span) => span.1 = i + 1,
                None => cur = Some((i, i + 1)),
            }
        } else if let Some((lo, hi)) = cur {
            // No diff at i. If the gap to the next diff is small enough
            // we want to coalesce - defer the close until we either run
            // off the end or find another diff far enough out.
            if i - hi >= opts.merge_gap {
                push_region(&mut regions, lo, hi, left, right, opts);
                cur = None;
            }
        }
    }
    if let Some((lo, hi)) = cur {
        push_region(&mut regions, lo, hi, left, right, opts);
    }

    RamDiff {
        left_label: left_label.to_owned(),
        right_label: right_label.to_owned(),
        regions,
        total_bytes_changed,
    }
}

fn push_region(
    out: &mut Vec<RegionDiff>,
    lo_off: usize,
    hi_off: usize,
    left: &[u8],
    right: &[u8],
    opts: &DiffOptions,
) {
    let bytes_changed = (lo_off..hi_off).filter(|&i| left[i] != right[i]).count();
    if bytes_changed < opts.min_bytes_changed {
        return;
    }
    let sample_len = (hi_off - lo_off).min(16);
    out.push(RegionDiff {
        start_addr: PSX_RAM_KSEG0 + lo_off as u32,
        end_addr: PSX_RAM_KSEG0 + hi_off as u32,
        bytes_changed,
        left_sample: left[lo_off..lo_off + sample_len].to_vec(),
        right_sample: right[lo_off..lo_off + sample_len].to_vec(),
    });
}

/// Sort regions by `bytes_changed` descending - useful for "what's the
/// noisiest structure" queries.
pub fn sort_by_size(diff: &mut RamDiff) {
    diff.regions
        .sort_by_key(|r| std::cmp::Reverse(r.bytes_changed));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank_ram() -> Vec<u8> {
        vec![0u8; PSX_RAM_SIZE]
    }

    #[test]
    fn no_diff_when_identical() {
        let a = blank_ram();
        let b = blank_ram();
        let d = diff_ram(&a, &b, "a", "b", &DiffOptions::default());
        assert!(d.regions.is_empty());
        assert_eq!(d.total_bytes_changed, 0);
    }

    #[test]
    fn detects_single_byte_change() {
        let a = blank_ram();
        let mut b = blank_ram();
        b[0x100] = 0xFF;
        let d = diff_ram(&a, &b, "a", "b", &DiffOptions::default());
        assert_eq!(d.regions.len(), 1);
        assert_eq!(d.regions[0].start_addr, 0x80000100);
        assert_eq!(d.regions[0].end_addr, 0x80000101);
        assert_eq!(d.regions[0].bytes_changed, 1);
        assert_eq!(d.total_bytes_changed, 1);
    }

    #[test]
    fn coalesces_within_merge_gap() {
        let a = blank_ram();
        let mut b = blank_ram();
        b[0x100] = 0xFF;
        b[0x108] = 0xFF; // 8-byte gap, well within default merge_gap of 16
        let d = diff_ram(&a, &b, "a", "b", &DiffOptions::default());
        assert_eq!(d.regions.len(), 1);
        assert_eq!(d.regions[0].start_addr, 0x80000100);
        assert_eq!(d.regions[0].end_addr, 0x80000109);
        assert_eq!(d.regions[0].bytes_changed, 2);
    }

    #[test]
    fn splits_when_gap_exceeds_merge() {
        let a = blank_ram();
        let mut b = blank_ram();
        b[0x100] = 0xFF;
        b[0x200] = 0xFF; // 256 bytes apart - split.
        let d = diff_ram(&a, &b, "a", "b", &DiffOptions::default());
        assert_eq!(d.regions.len(), 2);
        assert_eq!(d.regions[0].start_addr, 0x80000100);
        assert_eq!(d.regions[1].start_addr, 0x80000200);
    }

    #[test]
    fn window_filters_out_of_range() {
        let a = blank_ram();
        let mut b = blank_ram();
        b[0x100] = 0xFF;
        b[0x10_0000] = 0xFF;
        let opts = DiffOptions {
            window: (0x80100000, 0x80200000),
            ..Default::default()
        };
        let d = diff_ram(&a, &b, "a", "b", &opts);
        // Only the 0x10_0000 (= 0x80100000) change is in-window.
        assert_eq!(d.regions.len(), 1);
        assert_eq!(d.regions[0].start_addr, 0x80100000);
    }

    #[test]
    fn sorts_regions_by_bytes_changed_desc() {
        let a = blank_ram();
        let mut b = blank_ram();
        // Two regions: one with 4 changes, one with 1.
        for slot in &mut b[0x100..0x104] {
            *slot = 0xFF;
        }
        b[0x300] = 0xFF;
        let mut d = diff_ram(&a, &b, "a", "b", &DiffOptions::default());
        sort_by_size(&mut d);
        assert_eq!(d.regions[0].bytes_changed, 4);
        assert_eq!(d.regions[1].bytes_changed, 1);
    }

    #[test]
    fn min_bytes_changed_drops_tiny_regions() {
        let a = blank_ram();
        let mut b = blank_ram();
        b[0x100] = 0xFF; // 1 byte
        for slot in &mut b[0x200..0x205] {
            *slot = 0xFF; // 5 bytes
        }
        let opts = DiffOptions {
            min_bytes_changed: 2,
            ..Default::default()
        };
        let d = diff_ram(&a, &b, "a", "b", &opts);
        assert_eq!(d.regions.len(), 1);
        assert_eq!(d.regions[0].start_addr, 0x80000200);
    }
}

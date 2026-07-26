//! The property that pins an entry's size: **the entries tile the archive.**
//!
//! An entry's size is the sector gap to the next entry
//! (`toc[p+3] - toc[p+2]`), so the entry extents form a partition of
//! `PROT.DAT`: starts strictly increasing, every entry's end equal to the
//! next entry's start (no gaps, no overlaps), and the extents summing to the
//! contiguous span between the first start LBA and the archive's last sector.
//!
//! That is not a restatement of the definition. "Entry `p` ends where entry
//! `p+1` starts" *is* a tautology under this definition and is deliberately
//! **not** what this module asserts on its own; what is checkable, and what
//! fails for any other reading of the TOC, is that the resulting partition
//! covers the archive exactly - the starts are monotonic with no back-edges,
//! nothing falls outside the image, and the total lands on the file's own
//! last sector rather than short of it or (as the `toc[p+5] - toc[p+3] + 4`
//! expression does) at ~2.5x it. No partition of a file can sum to more than
//! the file.
//!
//! [`check`] returns the measurement; a caller asserts on it. The disc-gated
//! oracle is `crates/prot/tests/archive_tiling_real.rs`; the synthetic case
//! lives in this module's tests.
//!
//! See [`docs/formats/prot.md`](../../../docs/formats/prot.md).

use crate::archive::{Entry, SECTOR};

/// What [`check`] measured about an entry set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tiling {
    /// Number of entries examined.
    pub entries: usize,
    /// First entry's start LBA.
    pub first_start_lba: u32,
    /// Last entry's end LBA (`start + size`).
    pub last_end_lba: u32,
    /// Sum of every entry's `size_sectors`.
    pub total_sectors: u64,
    /// Entry indices whose start LBA is at or before the previous entry's
    /// start (a back-edge in an otherwise monotonic table).
    pub non_monotonic: Vec<u32>,
    /// `(entry, gap_sectors)` for each unclaimed run between one entry's end
    /// and the next entry's start.
    pub gaps: Vec<(u32, u32)>,
    /// `(entry, overlap_sectors)` for each entry that reaches past the next
    /// entry's start.
    pub overlaps: Vec<(u32, u32)>,
}

impl Tiling {
    /// True when the entries partition `[first_start_lba, last_end_lba)`
    /// exactly - monotonic, no gaps, no overlaps, and the sizes summing to
    /// that span.
    pub fn is_exact(&self) -> bool {
        self.non_monotonic.is_empty()
            && self.gaps.is_empty()
            && self.overlaps.is_empty()
            && self.total_sectors == (self.last_end_lba - self.first_start_lba) as u64
    }

    /// The archive's own sector count, for the "the tiling reaches the end of
    /// the file" half of the property. `file_len` in bytes.
    pub fn covers_to_end_of(&self, file_len: u64) -> bool {
        self.last_end_lba as u64 == file_len / (SECTOR as u64)
    }
}

/// Measure the tiling property over `entries` (which must be in TOC order).
pub fn check(entries: &[Entry]) -> Tiling {
    let mut t = Tiling {
        entries: entries.len(),
        first_start_lba: entries.first().map(|e| e.start_lba).unwrap_or(0),
        last_end_lba: entries
            .last()
            .map(|e| e.start_lba + e.size_sectors)
            .unwrap_or(0),
        total_sectors: entries.iter().map(|e| e.size_sectors as u64).sum(),
        non_monotonic: Vec::new(),
        gaps: Vec::new(),
        overlaps: Vec::new(),
    };
    for w in entries.windows(2) {
        let (cur, next) = (&w[0], &w[1]);
        if next.start_lba <= cur.start_lba {
            t.non_monotonic.push(next.index);
            continue;
        }
        let end = cur.start_lba + cur.size_sectors;
        if end < next.start_lba {
            t.gaps.push((cur.index, next.start_lba - end));
        } else if end > next.start_lba {
            t.overlaps.push((cur.index, end - next.start_lba));
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::Archive;

    /// Synthetic PROT.DAT whose TOC start LBAs tile the image: 1, 3, 10, 15,
    /// 19, then the file end at 20 and a zeroed tail.
    fn tiled_prot() -> Vec<u8> {
        let sectors = 20u32;
        let mut img = vec![0u8; (sectors * SECTOR) as usize];
        img[4..8].copy_from_slice(&6u32.to_le_bytes()); // file_num - 1
        img[8..12].copy_from_slice(&1u32.to_le_bytes()); // header_sectors
        for (i, lba) in [1u32, 3, 10, 15, 19, 20].iter().enumerate() {
            let off = 8 + (2 + i) * 4;
            img[off..off + 4].copy_from_slice(&lba.to_le_bytes());
        }
        img
    }

    #[test]
    fn parsed_entries_tile_the_image_exactly() {
        let img = tiled_prot();
        let len = img.len() as u64;
        let arch = Archive::from_bytes(img).expect("synthetic archive parses");
        let t = check(&arch.entries);
        assert_eq!(t.entries, 5);
        assert_eq!(t.first_start_lba, 1);
        assert_eq!(t.last_end_lba, 20);
        assert_eq!(t.total_sectors, 19);
        assert!(t.is_exact(), "{t:?}");
        assert!(t.covers_to_end_of(len));
    }

    /// The historical `toc[p+5] - toc[p+3] + 4` expression measures the two
    /// *following* entries, so it sums to more than the region it claims to
    /// partition - which is the whole argument against it, in one assertion.
    #[test]
    fn the_declared_span_cannot_be_a_partition() {
        let arch = Archive::from_bytes(tiled_prot()).expect("synthetic archive parses");
        let t = check(&arch.entries);
        let declared: u64 = arch
            .entries
            .iter()
            .map(|e| e.declared_span_sectors as u64)
            .sum();
        assert!(
            declared > t.total_sectors,
            "declared spans {declared} vs the {} sectors actually there",
            t.total_sectors
        );
        // ...and each one is its two successors' footprints plus 4.
        for w in arch.entries.windows(3) {
            assert_eq!(
                w[0].declared_span_sectors,
                w[1].size_sectors + w[2].size_sectors + 4,
                "entry {}: declared span is footprint(p+1) + footprint(p+2) + 4",
                w[0].index
            );
        }
    }

    #[test]
    fn a_gap_and_an_overlap_are_both_reported() {
        let mk = |index: u32, start_lba: u32, size_sectors: u32| Entry {
            index,
            start_lba,
            size_sectors,
            byte_offset: (start_lba as u64) * (SECTOR as u64),
            size_bytes: (size_sectors as u64) * (SECTOR as u64),
            declared_span_sectors: size_sectors,
            declared_span_bytes: (size_sectors as u64) * (SECTOR as u64),
        };
        // 0: [10,12) then a 3-sector hole; 1: [15,25) reaching 5 past entry 2.
        let t = check(&[mk(0, 10, 2), mk(1, 15, 10), mk(2, 20, 5)]);
        assert_eq!(t.gaps, vec![(0, 3)]);
        assert_eq!(t.overlaps, vec![(1, 5)]);
        assert!(!t.is_exact());
    }
}

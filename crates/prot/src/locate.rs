//! Reverse lookup: map a PROT.DAT byte offset back to the entry that owns it.
//!
//! PROT.DAT's TOC declares each entry a size (the `toc[p+5] - toc[p+3] + 4`
//! formula) that, for several entries, far exceeds the entry's real on-disc
//! footprint - the sector gap to the *next* entry's start LBA. `prot-extract`
//! writes the full declared window, so those extracted `.BIN` files carry a
//! neighbour's bytes in their tail: the monster archive (entry 867), whose own
//! declared size equals its footprint, is re-covered by the oversized windows of
//! the two player-battle files that precede it (865 / 866). Retail never trips on
//! this because its loader reads a bounded prologue, not the full declared size.
//!
//! This module answers "which entry really owns byte X", and flags when X lands
//! in an over-read tail (a neighbour's bytes) rather than the entry's own data.
//! The footprint span is taken from [`runtime_toc`](crate::runtime_toc) so there
//! is exactly one implementation of the `next_start - start` arithmetic.

use crate::archive::{Entry, SECTOR};
use crate::runtime_toc;

/// True on-disc footprint of an entry in bytes: the sector span to the next
/// entry's start LBA. Falls back to the entry's surfaced size when the TOC span
/// is unavailable (a short table, or an unsorted / tail row that would wrap).
pub fn footprint_bytes(toc: &[u32], entry: &Entry) -> u64 {
    match runtime_toc::entry_sector_span_from_archive_toc(toc, entry.index as usize) {
        Some(sectors) if sectors > 0 => (sectors as u64) * (SECTOR as u64),
        _ => entry.size_bytes,
    }
}

/// An entry over-reads when the window `prot-extract` writes (`size_bytes`)
/// extends past its true footprint - i.e. its `.BIN` tail is a neighbour's data.
pub fn is_over_read(toc: &[u32], entry: &Entry) -> bool {
    entry.size_bytes > footprint_bytes(toc, entry)
}

/// Result of locating an absolute PROT.DAT byte offset.
#[derive(Debug, Clone)]
pub struct Located {
    /// The absolute PROT.DAT byte offset that was located.
    pub abs_offset: u64,
    /// Position (in the `entries` slice) of the entry whose *footprint* owns the
    /// offset - the true source of these bytes. `None` when the offset is past
    /// every entry (tail padding) or before the first.
    pub owner: Option<usize>,
    /// Positions of every entry whose *extracted window* (`size_bytes`) covers
    /// the offset - i.e. every `.BIN` file that contains these bytes. For an
    /// over-read region this is the true owner plus each preceding entry whose
    /// oversized window reaches this far. Sorted by entry order.
    pub covering: Vec<usize>,
}

/// Locate an absolute PROT.DAT byte offset against a set of entries. Footprints
/// partition the archive (`[start_lba, next_start_lba)`), so at most one entry
/// *owns* an offset; the `covering` set can be larger because declared windows
/// overlap forward.
pub fn locate(toc: &[u32], entries: &[Entry], abs_offset: u64) -> Located {
    let mut owner = None;
    let mut covering = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        let footprint = footprint_bytes(toc, e);
        if abs_offset >= e.byte_offset && abs_offset < e.byte_offset + footprint {
            owner = Some(i);
        }
        if abs_offset >= e.byte_offset && abs_offset < e.byte_offset + e.size_bytes {
            covering.push(i);
        }
    }
    Located {
        abs_offset,
        owner,
        covering,
    }
}

/// Translate an offset within entry `entry_index`'s extracted `.BIN` file to an
/// absolute PROT.DAT byte offset. `None` if no entry carries that index.
pub fn abs_from_entry_offset(entries: &[Entry], entry_index: u32, local: u64) -> Option<u64> {
    entries
        .iter()
        .find(|e| e.index == entry_index)
        .map(|e| e.byte_offset + local)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an `Entry` at a given TOC index / start LBA with a declared
    /// (extracted-window) size in sectors. `byte_offset` / `size_bytes` mirror
    /// what `Archive::parse` computes.
    fn entry(index: u32, start_lba: u32, declared_sectors: u32) -> Entry {
        Entry {
            index,
            start_lba,
            size_sectors: declared_sectors,
            byte_offset: (start_lba as u64) * (SECTOR as u64),
            size_bytes: (declared_sectors as u64) * (SECTOR as u64),
            indexed_size_sectors: declared_sectors,
            indexed_size_bytes: (declared_sectors as u64) * (SECTOR as u64),
        }
    }

    /// A three-entry archive modelled on the battle-data cluster: entry 1's
    /// declared window (10 sectors) over-reads far past its 2-sector footprint
    /// into entry 2, which itself declares exactly its footprint.
    ///
    /// TOC layout: two header words, then per-index start LBAs. Index `p` reads
    /// `toc[p+2]` (start) and `toc[p+3]` (next start).
    fn cluster() -> (Vec<u32>, Vec<Entry>) {
        // start LBAs: entry0=100, entry1=140, entry2=142, sentinel next=200.
        let toc = vec![0xAAAA, 0xBBBB, 100, 140, 142, 200];
        let entries = vec![
            entry(0, 100, 40), // footprint 40, declared 40 -> not over-read
            entry(1, 140, 10), // footprint 2,  declared 10 -> over-read x5
            entry(2, 142, 58), // footprint 58, declared 58 -> exact
        ];
        (toc, entries)
    }

    #[test]
    fn footprint_is_the_lba_span_not_the_declared_size() {
        let (toc, e) = cluster();
        assert_eq!(footprint_bytes(&toc, &e[1]), 2 * SECTOR as u64);
        assert_eq!(footprint_bytes(&toc, &e[2]), 58 * SECTOR as u64);
    }

    #[test]
    fn over_read_flag_tracks_declared_vs_footprint() {
        let (toc, e) = cluster();
        assert!(!is_over_read(&toc, &e[0]));
        assert!(is_over_read(&toc, &e[1]));
        assert!(!is_over_read(&toc, &e[2]));
    }

    #[test]
    fn offset_inside_an_entry_footprint_is_owned_by_that_entry() {
        let (toc, e) = cluster();
        // 1 sector into entry 1's own data.
        let abs = 141 * SECTOR as u64;
        let loc = locate(&toc, &e, abs);
        assert_eq!(loc.owner, Some(1));
        // Only entry 1's window covers its own first sector.
        assert_eq!(loc.covering, vec![1]);
    }

    #[test]
    fn over_read_tail_is_owned_by_the_neighbour_but_covered_by_both() {
        let (toc, e) = cluster();
        // Byte 0 of entry 2 (start LBA 142) = the tail of entry 1's over-read
        // window (140 + 10 sectors reaches LBA 150).
        let abs = 142 * SECTOR as u64;
        let loc = locate(&toc, &e, abs);
        // Owned by entry 2 (its footprint starts here)...
        assert_eq!(loc.owner, Some(2));
        // ...but both entry 1's over-read window AND entry 2's window carry it.
        assert_eq!(loc.covering, vec![1, 2]);
    }

    #[test]
    fn offset_past_every_footprint_has_no_owner() {
        let (toc, e) = cluster();
        let abs = 10_000 * SECTOR as u64;
        let loc = locate(&toc, &e, abs);
        assert_eq!(loc.owner, None);
        assert!(loc.covering.is_empty());
    }

    #[test]
    fn entry_offset_translates_to_absolute() {
        let (_toc, e) = cluster();
        assert_eq!(
            abs_from_entry_offset(&e, 1, 0x55),
            Some(140 * SECTOR as u64 + 0x55)
        );
        assert_eq!(abs_from_entry_offset(&e, 999, 0), None);
    }
}

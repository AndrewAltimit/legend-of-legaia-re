//! Definitive catalog of every standard PSX TIM in a PROT.DAT image.
//!
//! jPSXdec indexes PROT.DAT as one flat 2048-byte-sector stream and reports
//! each TIM it finds (1132 items on the retail NA disc). This module
//! reproduces that scan clean-room with the strict TIM validator
//! ([`legaia_tim::parse_strict`]) and maps each hit back to OUR native
//! addressing - the owning PROT TOC entry and the byte offset within it (or
//! the unindexed system-UI gap that precedes the first entry).
//!
//! ## Why a flat scan
//!
//! Our per-entry [`crate::tim_scan`] only sees bytes that belong to a TOC
//! entry; it never touches the ~240 KB unindexed gap between the TOC and the
//! first entry (`init_data`), which holds the boot-time menu-glyph atlas and
//! cursor/icon TIMs. A flat scan over the whole image catches those too, so
//! the catalog covers every TIM regardless of which addressing layer hosts
//! it.
//!
//! ## jPSXdec parity
//!
//! The strict validator's three extra rejections - reserved flag bits must be
//! zero, `pmode` must be 0..=3, and each block length must be *exactly*
//! `12 + w*h*2` - plus the in-VRAM-bounds check are precisely what separate
//! jPSXdec's clean TIM set from the looser candidates a magic-only scan turns
//! up (TIM magic landing inside another TIM's pixel body, padded blocks,
//! `Mixed`-pmode noise). Under strict validation a flat scan recovers the
//! same item set jPSXdec reports, in the same order, with identical
//! dimensions and palette counts. The catalog is keyed by that scan order, so
//! a catalog `id` equals jPSXdec's `PROT.DAT[<id>]` item index.

use legaia_prot::archive::{Archive, Entry};
use serde::Serialize;

/// jPSXdec indexes PROT.DAT with this sector size; absolute offsets are
/// `sector * SECTOR + offset_within_sector`.
pub const SECTOR: u64 = 0x800;

/// One cataloged TIM. All fields are derived metadata (offsets, dimensions,
/// CLUT counts, a content fingerprint) - never any pixel bytes - so the
/// catalog is safe to commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogTim {
    /// Stable id = flat-scan order = jPSXdec's `PROT.DAT[<id>]` item index.
    pub id: u32,
    /// Absolute byte offset of the TIM magic in the flat PROT.DAT image.
    pub abs_offset: u64,
    /// jPSXdec-style start sector (`abs_offset / SECTOR`).
    pub sector: u64,
    /// Owning PROT TOC entry index, or `None` if the TIM lives in the
    /// unindexed gap before the first entry.
    pub entry_index: Option<u32>,
    /// Byte offset of the TIM within its owning entry. For gap hits this is
    /// the absolute offset (there is no owning entry to subtract).
    pub offset_in_entry: u64,
    /// Decoded pixel width.
    pub width: u32,
    /// Decoded pixel height.
    pub height: u32,
    /// Bits per pixel (4, 8, 16, or 24).
    pub bpp: u32,
    /// Number of CLUT palettes (= jPSXdec's `Palettes:` field; 0 for 16/24bpp).
    pub clut_count: usize,
    /// Total bytes the TIM occupies (header + CLUT block + image block).
    pub byte_len: usize,
    /// FNV-1a-64 fingerprint of the TIM's bytes. A hash, not the bytes - lets
    /// the regression detect any drift in the decoded region without
    /// committing Sony pixel data.
    pub fnv1a: u64,
}

/// FNV-1a-64 of a byte slice.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// A TOC entry's `(start, end, index)` footprint, sorted by `start`.
type Span = (u64, u64, u32);

/// Map an absolute PROT.DAT offset to its owning TOC entry. `sorted` is the
/// entry spans sorted ascending by start; the owner is the innermost entry
/// (largest start `<= abs`) whose footprint actually contains `abs`. Returns
/// `None` for offsets in the unindexed system-UI gap that precedes the first
/// entry, or in any hole between entries.
fn owning_entry(sorted: &[Span], abs: u64) -> Option<u32> {
    let i = sorted.partition_point(|&(start, _, _)| start <= abs);
    if i == 0 {
        return None; // before the first entry -> unindexed gap
    }
    let (start, end, idx) = sorted[i - 1];
    if start <= abs && abs < end {
        Some(idx)
    } else {
        None // falls in a hole between entries
    }
}

/// Build the TIM catalog from a flat PROT.DAT image and its parsed TOC.
///
/// Steps every 4-byte boundary (TIMs are word-aligned) and strict-parses
/// each magic match. Strict validation admits no TIM whose magic lands
/// inside an accepted TIM's body (a coincidental `0x10` word inside pixel
/// data fails the exact-length / VRAM-bounds checks), so a plain word-stride
/// scan yields exactly the non-overlapping item set jPSXdec reports - no
/// "consume past the item" bookkeeping needed.
pub fn build(prot: &[u8], entries: &[Entry]) -> Vec<CatalogTim> {
    let mut sorted: Vec<Span> = entries
        .iter()
        .map(|e| (e.byte_offset, e.byte_offset + e.size_bytes, e.index))
        .collect();
    sorted.sort_unstable();
    let entry_off = |idx: u32| {
        entries
            .iter()
            .find(|e| e.index == idx)
            .map(|e| e.byte_offset)
            .unwrap_or(0)
    };

    let mut out = Vec::new();
    let mut off = 0usize;
    let mut id = 0u32;
    while off + 4 <= prot.len() {
        let magic = u32::from_le_bytes(prot[off..off + 4].try_into().unwrap());
        if magic == legaia_tim::TIM_MAGIC
            && let Ok(tim) = legaia_tim::parse_strict(&prot[off..])
        {
            let byte_len = tim.byte_extent();
            let abs = off as u64;
            let entry_index = owning_entry(&sorted, abs);
            let offset_in_entry = match entry_index {
                Some(idx) => abs - entry_off(idx),
                None => abs,
            };
            let bpp = match tim.mode {
                legaia_tim::PixelMode::Bpp4 => 4,
                legaia_tim::PixelMode::Bpp8 => 8,
                legaia_tim::PixelMode::Bpp16 => 16,
                legaia_tim::PixelMode::Bpp24 => 24,
                legaia_tim::PixelMode::Mixed => 0,
            };
            out.push(CatalogTim {
                id,
                abs_offset: abs,
                sector: abs / SECTOR,
                entry_index,
                offset_in_entry,
                width: tim.pixel_width() as u32,
                height: tim.pixel_height() as u32,
                bpp,
                clut_count: tim.palette_count(),
                byte_len,
                fnv1a: fnv1a64(&prot[off..off + byte_len]),
            });
            id += 1;
        }
        off += 4;
    }
    out
}

/// Convenience: open a PROT.DAT file and build its catalog.
pub fn build_from_path(path: &std::path::Path) -> anyhow::Result<Vec<CatalogTim>> {
    let archive = Archive::open(path)?;
    let entries = archive.entries.clone();
    let prot = std::fs::read(path)?;
    Ok(build(&prot, &entries))
}

/// Canonical, diff-friendly serialization of the catalog: a one-line header
/// followed by one tab-separated row per TIM. This is the exact text of the
/// committed reference catalog, so the disc-gated regression can rebuild from
/// the disc and compare byte-for-byte. `entry_index` is `-1` for gap-owned
/// TIMs; `fnv1a` is lowercase 16-hex-digit.
pub fn to_tsv(catalog: &[CatalogTim]) -> String {
    let mut s = String::new();
    s.push_str("id\tabs_offset\tsector\tentry_index\toffset_in_entry\twidth\theight\tbpp\tclut_count\tbyte_len\tfnv1a\n");
    for t in catalog {
        let entry = match t.entry_index {
            Some(i) => i as i64,
            None => -1,
        };
        s.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:016x}\n",
            t.id,
            t.abs_offset,
            t.sector,
            entry,
            t.offset_in_entry,
            t.width,
            t.height,
            t.bpp,
            t.clut_count,
            t.byte_len,
            t.fnv1a,
        ));
    }
    s
}

/// A compact rollup over the catalog: total count plus an order-sensitive
/// FNV-1a-64 fold of every TIM's `(abs_offset, width, height, bpp,
/// clut_count, byte_len, fnv1a)`. The disc-gated regression pins this so a
/// single number guards the whole catalog against drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rollup {
    pub count: usize,
    pub digest: u64,
}

pub fn rollup(catalog: &[CatalogTim]) -> Rollup {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut fold = |v: u64| {
        for b in v.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for t in catalog {
        fold(t.abs_offset);
        fold(t.width as u64);
        fold(t.height as u64);
        fold(t.bpp as u64);
        fold(t.clut_count as u64);
        fold(t.byte_len as u64);
        fold(t.fnv1a);
    }
    Rollup {
        count: catalog.len(),
        digest: h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owning_entry_maps_gap_holes_and_entries() {
        // entry 5 spans [0x1000,0x1800), entry 6 spans [0x2000,0x2800).
        // (start, end, index), sorted by start.
        let sorted: Vec<Span> = vec![(0x1000, 0x1800, 5), (0x2000, 0x2800, 6)];
        assert_eq!(owning_entry(&sorted, 0x800), None); // before first -> gap
        assert_eq!(owning_entry(&sorted, 0x1000), Some(5));
        assert_eq!(owning_entry(&sorted, 0x17FF), Some(5));
        assert_eq!(owning_entry(&sorted, 0x1900), None); // hole between entries
        assert_eq!(owning_entry(&sorted, 0x2000), Some(6));
        assert_eq!(owning_entry(&sorted, 0x9999), None); // past the last entry
    }

    #[test]
    fn fnv1a_is_stable() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        // Distinct inputs hash distinctly.
        assert_ne!(fnv1a64(b"abc"), fnv1a64(b"abd"));
    }

    #[test]
    fn build_finds_a_handcrafted_tim_in_a_gap() {
        // A minimal valid 4bpp TIM with no preceding entry -> gap-owned.
        let mut prot = vec![0u8; 8]; // 8 bytes of leading zeros
        // header
        prot.extend_from_slice(&0x10u32.to_le_bytes());
        prot.extend_from_slice(&0x08u32.to_le_bytes()); // pmode 0 + CLUT
        // CLUT block: 12 + 16*2 = 44
        prot.extend_from_slice(&44u32.to_le_bytes());
        prot.extend_from_slice(&0u16.to_le_bytes()); // fb_x
        prot.extend_from_slice(&0u16.to_le_bytes()); // fb_y
        prot.extend_from_slice(&16u16.to_le_bytes()); // w
        prot.extend_from_slice(&1u16.to_le_bytes()); // h
        prot.extend(std::iter::repeat_n(0u8, 32));
        // image block: 12 + 1*4*2 = 20, fb_w=1, h=4
        prot.extend_from_slice(&20u32.to_le_bytes());
        prot.extend_from_slice(&0u16.to_le_bytes());
        prot.extend_from_slice(&0u16.to_le_bytes());
        prot.extend_from_slice(&1u16.to_le_bytes());
        prot.extend_from_slice(&4u16.to_le_bytes());
        prot.extend(std::iter::repeat_n(0u8, 8));

        let cat = build(&prot, &[]);
        assert_eq!(cat.len(), 1);
        assert_eq!(cat[0].id, 0);
        assert_eq!(cat[0].abs_offset, 8);
        assert_eq!(cat[0].entry_index, None);
        assert_eq!(cat[0].width, 4);
        assert_eq!(cat[0].height, 4);
        assert_eq!(cat[0].bpp, 4);
        assert_eq!(cat[0].clut_count, 1);
    }
}

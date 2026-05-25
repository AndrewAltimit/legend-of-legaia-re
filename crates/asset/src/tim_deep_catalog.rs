//! Deep TIM catalog: standard PSX TIMs recovered from inside LZS-compressed
//! PROT entries.
//!
//! The flat [`crate::tim_catalog`] - like the external reference decoder it
//! reproduces - scans only RAW bytes, so any TIM stored inside an
//! LZS-compressed section is invisible to it. Most character and scene
//! textures are compressed, so a large fraction of the disc's textures never
//! reach the flat catalog. This module recovers them: it walks every PROT
//! entry, LZS-decompresses it into sections, and strict-parses every TIM in
//! each decoded section.
//!
//! ## Why this is a separate tier
//!
//! The flat catalog is pinned byte-for-byte against an independent reference
//! decoder; perturbing it would break that parity guard. The deep tier has no
//! external oracle (the reference decoder doesn't decompress), so it is kept
//! wholly separate, keyed by OUR native addressing - `(entry index, LZS
//! section index, byte offset within the decoded section)` - and validated by
//! a different rule.
//!
//! ## Validity rule: never trust "it decompressed"
//!
//! The LZS ring buffer initializes to zeros, so random input decodes to
//! plausible-looking output - "decompresses without error" is never a
//! validity signal. A deep hit is admitted only when the decoded bytes both
//! (a) pass [`legaia_tim::parse_strict`] (no reserved flag bits, real pmode,
//! exact block lengths, in-VRAM-bounds image) AND (b) decode to RGBA without
//! error. That double gate rejects the coincidental TIM-magic-in-noise hits a
//! magic-only scan of decompressed garbage would otherwise turn up.

use legaia_prot::archive::Archive;
use serde::Serialize;

/// One TIM recovered from inside an LZS-compressed PROT section. All fields
/// are derived metadata (offsets, dimensions, CLUT counts, a content
/// fingerprint) - never any pixel bytes - so the catalog is safe to commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeepCatalogTim {
    /// Stable id = deep-scan order (entries ascending by index, then sections
    /// ascending, then offset ascending).
    pub id: u32,
    /// Owning PROT TOC entry index.
    pub entry_index: u32,
    /// LZS section index within the entry's decompressed container.
    pub lzs_section: u32,
    /// Byte offset of the TIM magic within the decoded section.
    pub offset_in_section: u64,
    /// Decoded pixel width.
    pub width: u32,
    /// Decoded pixel height.
    pub height: u32,
    /// Bits per pixel (4, 8, 16, or 24).
    pub bpp: u32,
    /// Number of CLUT palettes (0 for 16/24bpp).
    pub clut_count: usize,
    /// Total bytes the TIM occupies in the decoded section (header + CLUT
    /// block + image block).
    pub byte_len: usize,
    /// FNV-1a-64 fingerprint of the TIM's DECODED bytes. A hash, not the
    /// bytes - lets the regression detect drift without committing Sony pixel
    /// data (and without committing decompressed Sony bytes).
    pub fnv1a: u64,
    /// Our own curated semantic label for this texture, looked up by content
    /// fingerprint in [`crate::tim_labels`]; `None` when not yet curated. The
    /// table is shared with the raw tier, so a texture that appears in both
    /// (e.g. a raw page also embedded compressed elsewhere) gets the same
    /// label. Not folded into [`rollup`]; serialized in [`to_tsv`].
    pub label: Option<&'static str>,
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

/// Scan one decoded LZS section for strict-valid TIMs, pushing each into
/// `out` with its `(entry, section)` key. `id` is advanced per hit.
///
/// Walks every word-aligned offset (TIMs are word-aligned). Strict validation
/// plus a decode-to-RGBA dry run is the validity gate - a coincidental TIM
/// magic landing in decompressed noise fails one or the other.
fn scan_section(
    section: &[u8],
    entry_index: u32,
    lzs_section: u32,
    id: &mut u32,
    out: &mut Vec<DeepCatalogTim>,
) {
    let mut off = 0usize;
    while off + 4 <= section.len() {
        let magic = u32::from_le_bytes(section[off..off + 4].try_into().unwrap());
        if magic == legaia_tim::TIM_MAGIC
            && let Ok(tim) = legaia_tim::parse_strict(&section[off..])
            // Dimension sanity + decode dry run: a strict-valid header whose
            // body is noise still decodes here (the bytes exist), but this
            // catches any pathological dimension that would panic a consumer.
            && tim.pixel_width() > 0
            && tim.pixel_height() > 0
            && legaia_tim::decode_rgba8(&tim, 0).is_ok()
        {
            let byte_len = tim.byte_extent();
            let bpp = match tim.mode {
                legaia_tim::PixelMode::Bpp4 => 4,
                legaia_tim::PixelMode::Bpp8 => 8,
                legaia_tim::PixelMode::Bpp16 => 16,
                legaia_tim::PixelMode::Bpp24 => 24,
                legaia_tim::PixelMode::Mixed => 0,
            };
            let fnv1a = fnv1a64(&section[off..off + byte_len]);
            out.push(DeepCatalogTim {
                id: *id,
                entry_index,
                lzs_section,
                offset_in_section: off as u64,
                width: tim.pixel_width() as u32,
                height: tim.pixel_height() as u32,
                bpp,
                clut_count: tim.palette_count(),
                byte_len,
                fnv1a,
                label: crate::tim_labels::label_for(fnv1a),
            });
            *id += 1;
        }
        off += 4;
    }
}

/// Build the deep catalog from a flat PROT.DAT image and its TOC entry spans.
///
/// Walks every entry in index order, LZS-decompresses its bytes with
/// [`legaia_lzs::decompress_container`] (the same decode path
/// [`crate::tim_scan::scan_entry`] uses), and strict-parses every TIM in each
/// decoded section. Entries that are not LZS containers (the lenient header
/// heuristic fails to find a section table) contribute no sections and are
/// skipped - the flat catalog already covers their raw TIMs.
///
/// Takes raw `(byte_offset, size_bytes, index)` spans (the same shape
/// [`crate::tim_catalog::build_from_spans`] takes) so the in-browser viewer
/// can build the deep catalog from its own lightweight TOC metadata over the
/// in-memory PROT.DAT, without an [`Archive`].
pub fn build_from_spans(prot: &[u8], entry_spans: &[(u64, u64, u32)]) -> Vec<DeepCatalogTim> {
    // Iterate in index-ascending order so the assigned ids (and thus the
    // rollup digest) are stable regardless of how the caller ordered spans.
    let mut spans: Vec<(u64, u64, u32)> = entry_spans.to_vec();
    spans.sort_unstable_by_key(|&(_, _, idx)| idx);

    let mut out = Vec::new();
    let mut id = 0u32;
    for &(off, size, index) in &spans {
        let start = off as usize;
        let end = start.saturating_add(size as usize);
        if end > prot.len() {
            continue; // span runs off the image (already filtered at TOC parse)
        }
        let Ok(sections) = legaia_lzs::decompress_container(&prot[start..end]) else {
            continue;
        };
        for (sec_idx, section) in sections.iter().enumerate() {
            scan_section(section, index, sec_idx as u32, &mut id, &mut out);
        }
    }
    out
}

/// Build the deep catalog from an open [`Archive`].
pub fn build(archive: &Archive, prot: &[u8]) -> Vec<DeepCatalogTim> {
    let spans: Vec<(u64, u64, u32)> = archive
        .entries
        .iter()
        .map(|e| (e.byte_offset, e.size_bytes, e.index))
        .collect();
    build_from_spans(prot, &spans)
}

/// Convenience: open a PROT.DAT file and build its deep catalog.
pub fn build_from_path(path: &std::path::Path) -> anyhow::Result<Vec<DeepCatalogTim>> {
    let archive = Archive::open(path)?;
    let prot = std::fs::read(path)?;
    Ok(build(&archive, &prot))
}

/// Canonical, diff-friendly serialization: a one-line header followed by one
/// tab-separated row per TIM. This is the exact text of the committed
/// reference deep catalog, so the disc-gated regression can rebuild from the
/// disc and compare byte-for-byte. `fnv1a` is lowercase 16-hex-digit.
pub fn to_tsv(catalog: &[DeepCatalogTim]) -> String {
    let mut s = String::new();
    s.push_str(
        "id\tentry_index\tlzs_section\toffset_in_section\twidth\theight\tbpp\tclut_count\tbyte_len\tfnv1a\tlabel\n",
    );
    for t in catalog {
        s.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:016x}\t{}\n",
            t.id,
            t.entry_index,
            t.lzs_section,
            t.offset_in_section,
            t.width,
            t.height,
            t.bpp,
            t.clut_count,
            t.byte_len,
            t.fnv1a,
            t.label.unwrap_or(""),
        ));
    }
    s
}

/// A compact rollup over the deep catalog: total count plus an order-sensitive
/// FNV-1a-64 fold of every TIM's structural fields. The disc-gated regression
/// pins this so a single number guards the whole catalog against drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rollup {
    pub count: usize,
    pub digest: u64,
}

pub fn rollup(catalog: &[DeepCatalogTim]) -> Rollup {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut fold = |v: u64| {
        for b in v.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for t in catalog {
        fold(t.entry_index as u64);
        fold(t.lzs_section as u64);
        fold(t.offset_in_section);
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

    /// Build a minimal valid 4bpp TIM (16-entry CLUT, 4x4 image).
    fn make_tim() -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(&0x10u32.to_le_bytes());
        t.extend_from_slice(&0x08u32.to_le_bytes()); // pmode 0 + CLUT
        // CLUT block: 12 + 16*2 = 44
        t.extend_from_slice(&44u32.to_le_bytes());
        t.extend_from_slice(&0u16.to_le_bytes()); // fb_x
        t.extend_from_slice(&0u16.to_le_bytes()); // fb_y
        t.extend_from_slice(&16u16.to_le_bytes()); // w
        t.extend_from_slice(&1u16.to_le_bytes()); // h
        t.extend(std::iter::repeat_n(0u8, 32));
        // image block: 12 + 1*4*2 = 20, fb_w=1, h=4
        t.extend_from_slice(&20u32.to_le_bytes());
        t.extend_from_slice(&0u16.to_le_bytes());
        t.extend_from_slice(&0u16.to_le_bytes());
        t.extend_from_slice(&1u16.to_le_bytes());
        t.extend_from_slice(&4u16.to_le_bytes());
        t.extend(std::iter::repeat_n(0u8, 8));
        t
    }

    #[test]
    fn scan_section_finds_a_tim() {
        let mut section = vec![0u8; 16]; // leading padding
        section.extend_from_slice(&make_tim());
        let mut id = 0u32;
        let mut out = Vec::new();
        scan_section(&section, 7, 2, &mut id, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].entry_index, 7);
        assert_eq!(out[0].lzs_section, 2);
        assert_eq!(out[0].offset_in_section, 16);
        assert_eq!(out[0].width, 4);
        assert_eq!(out[0].height, 4);
        assert_eq!(out[0].bpp, 4);
        assert_eq!(out[0].clut_count, 1);
    }

    #[test]
    fn scan_section_rejects_magic_in_noise() {
        // TIM magic word followed by junk that fails strict parse.
        let mut section = vec![0u8; 64];
        section[0..4].copy_from_slice(&0x10u32.to_le_bytes());
        section[8..12].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        let mut id = 0u32;
        let mut out = Vec::new();
        scan_section(&section, 0, 0, &mut id, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn tsv_round_trips_header_and_row() {
        let cat = vec![DeepCatalogTim {
            id: 0,
            entry_index: 12,
            lzs_section: 1,
            offset_in_section: 99,
            width: 64,
            height: 32,
            bpp: 4,
            clut_count: 2,
            byte_len: 1234,
            fnv1a: 0xdead_beef_0000_0001,
            label: Some("environment"),
        }];
        let tsv = to_tsv(&cat);
        let header = tsv.lines().next().unwrap();
        assert!(header.starts_with("id\tentry_index\tlzs_section\toffset_in_section\t"));
        assert!(header.ends_with("\tlabel"));
        assert!(tsv.contains("0\t12\t1\t99\t64\t32\t4\t2\t1234\tdeadbeef00000001\tenvironment\n"));
    }
}

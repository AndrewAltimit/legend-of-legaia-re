//! Find Legaia TMDs embedded in arbitrary buffers.
//!
//! Scans for the Legaia TMD magic (`0x80000002`, little-endian) at every
//! word-aligned offset, attempts a structural parse, and records hits whose
//! object/vert counts look plausible.
//!
//! Used by `asset tmd-scan` to inventory TMDs across all PROT entries
//! (raw + LZS-decompressed) - the per-cutscene `chunk01_TMD/` extracts cover
//! only ~29 unique meshes; the rest of the character/monster roster lives in
//! containers we haven't broken out yet.
//!
//! The scanner is intentionally optimistic: the TMD parser itself enforces
//! the heavy validation (FLIST_BIT set, object count sane, table fits, etc.),
//! so any false positive on the magic check will be rejected at parse time.

use legaia_lzs::decompress_container;
use legaia_tmd as tmd;

/// Legaia TMD identifier - see `docs/formats/tmd.md` and `FUN_80026b4c`.
const LEGAIA_TMD_MAGIC: u32 = 0x80000002;

/// One TMD found inside a source buffer.
#[derive(Debug, Clone)]
pub struct Hit {
    /// Byte offset inside the source buffer where the magic lives.
    pub offset: usize,
    /// Length in bytes of the parsed TMD (header + objects + sections).
    pub byte_len: usize,
    /// Object count from the TMD header.
    pub n_obj: u32,
    /// Sum of `n_vert` across all objects.
    pub total_verts: u32,
    /// Sum of `n_primitive` across all objects.
    pub total_prims: u32,
}

/// Scan one buffer for embedded TMDs. Walks every word-aligned offset; for
/// each magic match, tries the parser and keeps the hit if it survives plus
/// passes plausibility filters (≥1 object, ≥4 total verts).
///
/// Returns hits in source-order. Overlapping hits are not deduplicated.
pub fn scan_buffer(buf: &[u8]) -> Vec<Hit> {
    let mut hits = Vec::new();
    if buf.len() < tmd::HEADER_SIZE {
        return hits;
    }
    // Walk by 4 bytes - TMDs are word-aligned in every container we've seen.
    let mut off = 0usize;
    while off + 4 <= buf.len() {
        let magic = u32::from_le_bytes(buf[off..off + 4].try_into().unwrap());
        if magic == LEGAIA_TMD_MAGIC
            && let Some(hit) = try_parse_at(&buf[off..], off)
        {
            hits.push(hit);
        }
        off += 4;
    }
    hits
}

/// Parse a TMD starting at offset 0 of `slice`; build a [`Hit`] anchored at
/// `source_off` if the parse succeeds and the result looks plausible.
fn try_parse_at(slice: &[u8], source_off: usize) -> Option<Hit> {
    let parsed = tmd::parse(slice).ok()?;
    if parsed.header.nobj == 0 {
        return None;
    }
    let total_verts: u32 = parsed.objects.iter().map(|o| o.header.n_vert).sum();
    let total_prims: u32 = parsed.objects.iter().map(|o| o.header.n_primitive).sum();
    if total_verts < 4 {
        return None;
    }
    let byte_len = tmd_byte_extent(&parsed);
    Some(Hit {
        offset: source_off,
        byte_len,
        n_obj: parsed.header.nobj,
        total_verts,
        total_prims,
    })
}

/// Compute the highest byte offset touched by a parsed TMD: header + object
/// table + max(end-of-vert, end-of-normal, end-of-prim-section). That's the
/// total slab size we should carve out when extracting.
fn tmd_byte_extent(parsed: &tmd::Tmd) -> usize {
    let mut end = tmd::HEADER_SIZE + parsed.objects.len() * tmd::OBJECT_SIZE;
    for o in &parsed.objects {
        let vert_end = tmd::HEADER_SIZE
            + o.header.vert_top as usize
            + o.header.n_vert as usize * tmd::VECTOR_SIZE;
        let norm_end = tmd::HEADER_SIZE
            + o.header.normal_top as usize
            + o.header.n_normal as usize * tmd::VECTOR_SIZE;
        let prim_end = o.primitives_byte_offset + o.primitives_byte_size;
        end = end.max(vert_end).max(norm_end).max(prim_end);
    }
    end
}

/// Where a hit was found relative to the original PROT entry: directly in
/// the raw bytes, or after LZS-decompressing the entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Raw,
    /// Index of the LZS section within the container (some PROT entries are
    /// multi-section LZS bundles).
    Lzs(usize),
}

/// Per-PROT-entry scan result. Holds the decompressed LZS sections (when the
/// entry decompressed) so a downstream extractor can re-slice the right buffer
/// for each `Source::Lzs(i)` hit without re-decoding.
#[derive(Debug, Clone)]
pub struct EntryScan {
    pub hits: Vec<(Source, Hit)>,
    /// LZS-decompressed sections (one per section in the container). Empty if
    /// the entry didn't decompress.
    pub lzs_sections: Vec<Vec<u8>>,
    /// Whether the entry decompressed at all under the LZS strict decoder.
    pub lzs_ok: bool,
}

/// Scan one PROT entry: raw scan first, then LZS-strict scan. Hits from each
/// pass are tagged with their source so the caller can extract bytes from
/// the right buffer (raw or `lzs_sections[i]`).
pub fn scan_entry(buf: &[u8]) -> EntryScan {
    let mut hits: Vec<(Source, Hit)> = scan_buffer(buf)
        .into_iter()
        .map(|h| (Source::Raw, h))
        .collect();

    let (lzs_sections, lzs_ok) = match decompress_container(buf) {
        Ok(sections) => {
            for (idx, section) in sections.iter().enumerate() {
                for h in scan_buffer(section) {
                    hits.push((Source::Lzs(idx), h));
                }
            }
            (sections, true)
        }
        Err(_) => (Vec::new(), false),
    };

    EntryScan {
        hits,
        lzs_sections,
        lzs_ok,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_buffer_returns_no_hits() {
        assert!(scan_buffer(&[]).is_empty());
        assert!(scan_buffer(&[0; 8]).is_empty());
    }

    #[test]
    fn random_bytes_with_magic_but_no_parse_is_rejected() {
        // Magic at offset 0, but everything else is junk - parser must reject.
        let mut buf = vec![0u8; 64];
        buf[0..4].copy_from_slice(&LEGAIA_TMD_MAGIC.to_le_bytes());
        // FLIST_BIT clear, nobj huge → parse rejects.
        buf[8..12].copy_from_slice(&9999u32.to_le_bytes());
        assert!(scan_buffer(&buf).is_empty());
    }
}

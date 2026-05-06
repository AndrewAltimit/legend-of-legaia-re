//! "Scripted scene-asset-table" detector.
//!
//! A composite shape that pairs a `[u16 count][u16 offsets[count]]` script
//! prescript at offset 0 with a canonical 7-asset scene table at the next
//! 2 KB sector boundary past the prescript.
//!
//! ### Layout (empirically verified across 77 PROT entries, 2026-05-05)
//!
//! ```text
//! +0x00              u16  count             ; 1..=4096
//! +0x02              u16  offsets[count]    ; offsets[0] = 2 + count*2,
//!                                          ; monotonically non-decreasing,
//!                                          ; all <= file size
//! +offsets[count-1]  record bytecode        ; per-record opcodes (often
//!                                          ; leading with `0xFFFF 0x0000`
//!                                          ; sentinel — distinct event
//!                                          ; scripts per record)
//! +0x800-aligned     u32  count = 7         ; canonical scene-asset-table lead
//! ...                                       ; 7 descriptors, first data_offset
//!                                          ; = 0x40, all type bytes <= 0x14
//! ```
//!
//! ### Provenance
//!
//! Manual cluster characterisation found 100 PROT entries match the leading
//! `[u16 count][u16 offsets]` prescript shape. Of those, **77 also have a
//! valid scene-asset-table (`u32 count = 7` + first descriptor at +0x40 +
//! known type byte) at the next 0x800-aligned boundary** past the last
//! prescript record offset. These 77 entries were previously split between
//! `unknown_high_entropy` (52 + ones missed by `scene_asset_table` detector
//! at offset 0) and `unknown_other` (depending on entropy).
//!
//! ### Format meaning — partially understood
//!
//! The prescript is plausibly the **scene event-script bytecode** that the
//! field VM (`FUN_801DE840`) executes when the scene loads. Per-record
//! `0xFFFF 0x0000` markers strongly resemble field-VM frame-divider opcodes.
//! The asset table after the boundary is a standard scene asset bundle.
//!
//! Detection is gated on the asset-table shape — that's a much stronger
//! signal than the prescript alone, which can occasionally match arbitrary
//! `[count][offsets]`-shaped data.
//!
//! ### Coverage impact
//!
//! Promotes 77 entries from `unknown_high_entropy` / `unknown_other` to
//! `Class::SceneScriptedAssetTable`. Net coverage moves up by ~6.2 %.

use serde::Serialize;

use crate::AssetType;

/// Sector size used by the PSX disc + the runtime's `read at sector boundary`
/// loader. The asset table always starts on a 0x800-aligned offset past the
/// prescript.
const SECTOR_SIZE: usize = 0x800;

/// Asset-table count literal — same as [`crate::scene_asset_table`]. The
/// scripted variant always carries the canonical 7-asset shape.
const ASSET_TABLE_COUNT: u32 = 7;

/// `8 + 7 * 8` — the byte after the asset-table descriptor block.
const ASSET_TABLE_HEADER_END: u32 = 8 + (ASSET_TABLE_COUNT * 8);

/// Maximum sane prescript record count. Real hits sit in `1..=71`; 4096
/// gives plenty of headroom while rejecting random buffers whose `u16[0]`
/// happens to fall in the 0x0000..=0xFFFF range.
const MAX_PRESCRIPT_COUNT: u16 = 4096;

/// Detection result.
#[derive(Debug, Clone, Serialize)]
pub struct SceneScriptedAssetTable {
    /// Number of records in the prescript offset table at offset 0.
    pub prescript_records: u16,
    /// Byte offset where the prescript ends (= last record offset). The asset
    /// table begins at the next `SECTOR_SIZE`-aligned offset past this.
    pub prescript_end: u16,
    /// Byte offset where the inner scene-asset-table starts. Always
    /// `SECTOR_SIZE`-aligned.
    pub asset_table_offset: usize,
}

/// Try to detect a scripted scene-asset-table. Returns `None` if either the
/// prescript shape or the inner asset-table shape doesn't match.
pub fn detect(buf: &[u8]) -> Option<SceneScriptedAssetTable> {
    let (count, last_off) = detect_prescript(buf)?;

    // The asset table starts at the next 0x800-aligned offset past the
    // prescript's last record offset. Anything past EOF means the asset
    // table can't be present.
    let table_off = (last_off as usize)
        .checked_add(SECTOR_SIZE)?
        .checked_sub(1)?
        & !(SECTOR_SIZE - 1);
    if table_off + ASSET_TABLE_HEADER_END as usize > buf.len() {
        return None;
    }

    // Validate the inner asset table — same checks as
    // `crate::scene_asset_table::detect` but at a non-zero base offset.
    let lead = read_u32_le(buf, table_off)?;
    if lead != ASSET_TABLE_COUNT {
        return None;
    }
    // First descriptor's offset must be the literal 0x40 (= 8 + 7*8).
    let first_data_off = read_u32_le(buf, table_off + 12)?;
    if first_data_off != ASSET_TABLE_HEADER_END {
        return None;
    }
    // All 7 type bytes must be legal asset-type values.
    for i in 0..(ASSET_TABLE_COUNT as usize) {
        let p = table_off + 8 + i * 8;
        let type_size = read_u32_le(buf, p)?;
        let type_byte = ((type_size >> 24) & 0xFF) as u8;
        if matches!(AssetType::from_byte(type_byte), AssetType::Unknown(_)) {
            return None;
        }
    }

    Some(SceneScriptedAssetTable {
        prescript_records: count,
        prescript_end: last_off,
        asset_table_offset: table_off,
    })
}

/// Walk the prescript offset table and produce per-record `(start, end)` byte
/// ranges. The records run up to (but not including) the next record's start;
/// the final record ends at the asset-table sector boundary so callers don't
/// accidentally read into the inner asset bundle's bytes.
///
/// Returns `None` if the prescript is malformed.
pub fn record_ranges(buf: &[u8]) -> Option<Vec<(usize, usize)>> {
    let (count, last_off) = detect_prescript(buf)?;
    // The asset table sits at the next 0x800-aligned offset past the
    // prescript's last record offset. We clamp the final record's `end` at
    // that boundary so callers don't see asset-table bytes inside a script
    // record.
    let table_off = (last_off as usize)
        .checked_add(SECTOR_SIZE)?
        .checked_sub(1)?
        & !(SECTOR_SIZE - 1);
    let final_end = table_off.min(buf.len());

    let mut out = Vec::with_capacity(count as usize);
    let table_end = 2 + (count as usize) * 2;
    let mut offsets = Vec::with_capacity(count as usize);
    for i in 0..(count as usize) {
        offsets.push(read_u16_le(buf, 2 + i * 2)? as usize);
    }
    debug_assert_eq!(offsets[0], table_end);
    for (i, &start) in offsets.iter().enumerate() {
        let end = if i + 1 < offsets.len() {
            offsets[i + 1]
        } else {
            final_end
        };
        if start > end || end > buf.len() {
            return None;
        }
        out.push((start, end));
    }
    Some(out)
}

/// Validate the leading `[u16 count][u16 offsets[count]]` prescript shape.
/// Returns `(count, last_offset)` on success.
fn detect_prescript(buf: &[u8]) -> Option<(u16, u16)> {
    if buf.len() < 4 {
        return None;
    }
    let count = read_u16_le(buf, 0)?;
    if count == 0 || count > MAX_PRESCRIPT_COUNT {
        return None;
    }
    let table_end = 2usize.checked_add((count as usize) * 2)?;
    if table_end > buf.len() {
        return None;
    }
    // First record offset must equal `2 + count * 2` (= `table_end`). This
    // is the algebraic invariant that gives the detector its precision.
    let first = read_u16_le(buf, 2)?;
    if first as usize != table_end {
        return None;
    }
    // All offsets monotonic, in-bounds.
    let mut prev = 0u16;
    let mut last = 0u16;
    for i in 0..(count as usize) {
        let o = read_u16_le(buf, 2 + i * 2)?;
        if (o as usize) > buf.len() || o < prev {
            return None;
        }
        prev = o;
        last = o;
    }
    Some((count, last))
}

fn read_u16_le(buf: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(buf.get(at..at + 2)?.try_into().ok()?))
}

fn read_u32_le(buf: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(buf.get(at..at + 4)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthesize a valid scripted scene-asset-table:
    /// `[u16 count][u16 offsets][record bodies][zero-pad to next sector]
    ///  [u32 count=7][u32 meta1][7 × (type_size, data_offset)]`.
    fn synth(count: u16, record_lens: &[usize], asset_types: [u8; 7]) -> Vec<u8> {
        assert_eq!(count as usize, record_lens.len());

        let mut buf = Vec::new();
        let table_end = 2 + (count as usize) * 2;

        // Compute record offsets.
        let mut record_offs = Vec::with_capacity(count as usize);
        let mut acc = table_end;
        for &rl in record_lens {
            record_offs.push(acc);
            acc += rl;
        }
        let last_off = *record_offs.last().expect("at least one record");

        // Header: count + offsets.
        buf.extend_from_slice(&count.to_le_bytes());
        for &o in &record_offs {
            buf.extend_from_slice(&(o as u16).to_le_bytes());
        }
        // Record bodies — fill with marker bytes to disambiguate from padding.
        for (i, &rl) in record_lens.iter().enumerate() {
            buf.extend(std::iter::repeat_n(0xA0u8 + (i as u8 & 0xF), rl));
        }
        assert_eq!(buf.len(), acc);

        // Pad with zeros to next 0x800 boundary past `last_off`.
        let table_off = (last_off + SECTOR_SIZE - 1) & !(SECTOR_SIZE - 1);
        if table_off > buf.len() {
            buf.resize(table_off, 0);
        }

        // Inner asset table: count = 7, meta1, 7 descriptors.
        buf.extend_from_slice(&ASSET_TABLE_COUNT.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes()); // meta1
        let mut data_off = ASSET_TABLE_HEADER_END;
        for &t in &asset_types {
            // type_size = (t << 24) | size_low24
            let type_size = ((t as u32) << 24) | 0x10;
            buf.extend_from_slice(&type_size.to_le_bytes());
            buf.extend_from_slice(&data_off.to_le_bytes());
            data_off += 0x40;
        }
        // Pad inner asset payload region.
        buf.resize(table_off + ASSET_TABLE_HEADER_END as usize + 0x100, 0);
        buf
    }

    #[test]
    fn detects_canonical_layout() {
        let buf = synth(3, &[16, 32, 48], [1, 2, 3, 4, 5, 6, 7]);
        let r = detect(&buf).expect("should detect");
        assert_eq!(r.prescript_records, 3);
    }

    #[test]
    fn rejects_when_first_offset_doesnt_match_table_end() {
        let mut buf = synth(3, &[16, 32, 48], [1, 2, 3, 4, 5, 6, 7]);
        // Stomp first offset to a wrong value.
        buf[2] = 0xFF;
        buf[3] = 0;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_when_inner_table_count_isnt_7() {
        let mut buf = synth(3, &[16, 32, 48], [1, 2, 3, 4, 5, 6, 7]);
        // Find inner table offset by recomputing.
        let last_off = u16::from_le_bytes([buf[2 + 4], buf[2 + 5]]) as usize;
        let table_off = (last_off + SECTOR_SIZE - 1) & !(SECTOR_SIZE - 1);
        buf[table_off] = 6; // change count from 7 to 6
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_when_first_descriptor_offset_isnt_0x40() {
        let mut buf = synth(3, &[16, 32, 48], [1, 2, 3, 4, 5, 6, 7]);
        let last_off = u16::from_le_bytes([buf[2 + 4], buf[2 + 5]]) as usize;
        let table_off = (last_off + SECTOR_SIZE - 1) & !(SECTOR_SIZE - 1);
        // First descriptor's data_offset is at table_off + 12.
        buf[table_off + 12] = 0x80;
        buf[table_off + 13] = 0;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_when_asset_type_is_unknown() {
        // Use a definitely-unknown type byte (0xC0 — outside 0x00..=0x14).
        let buf = synth(3, &[16, 32, 48], [1, 2, 3, 4, 5, 6, 0xC0]);
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_random_bytes() {
        let buf: Vec<u8> = (0..0x4000u32).map(|i| (i & 0xFF) as u8).collect();
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn record_ranges_walks_prescript_and_clamps_at_sector_boundary() {
        let buf = synth(3, &[16, 32, 48], [1, 2, 3, 4, 5, 6, 7]);
        let ranges = record_ranges(&buf).expect("prescript valid");
        assert_eq!(ranges.len(), 3);
        // Records start at `2 + 3*2 = 8`. Lengths are 16/32/48.
        assert_eq!(ranges[0], (8, 24));
        assert_eq!(ranges[1], (24, 56));
        // Last range ends at the sector boundary, not the asset-table content.
        assert_eq!(ranges[2].0, 56);
        assert!(ranges[2].1 <= 0x800);
    }
}

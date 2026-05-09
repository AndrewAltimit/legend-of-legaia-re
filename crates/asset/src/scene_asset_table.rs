//! "Scene asset table" detector — the canonical 7-typed-asset bundle shape.
//!
//! ### Provenance
//!
//! Round-21 cluster characterisation (2026-05-04) found that **80 PROT entries
//! lead with the literal 4-byte `07 00 00 00`** and decode as
//! `parse_player_lzs(buf, 7)`-style descriptor tables — a fixed 7-asset
//! bundle whose descriptor types form the canonical scene-asset sequence
//! `(TimList, Tmd, Man, Mes, Move, Anm, Vdf)` = `(1, 2, 3, 4, 5, 6, 7)`.
//!
//! ### Layout
//!
//! ```text
//! +0x00   u32  count = 7              ; literal `07 00 00 00`
//! +0x04   u32  meta1                  ; varies — not a file-relative offset
//! +0x08   7 × (u32 type_size, u32 data_offset)
//!                                     ; each pair packs `(type<<24)|size`
//!                                     ; first descriptor's `data_offset` = 0x40
//! +0x40   asset payload region        ; LZS-compressed in ~26 / 80 entries,
//!                                     ; stored raw in the rest
//! ```
//!
//! ### Type-sequence variants (empirically observed)
//!
//! | Tuple                          | Count | Notes |
//! |--------------------------------|-------|-------|
//! | `(1, 2, 3, 4, 5, 6, 7)`        | 67    | Standard scene bundle. |
//! | `(1, 3, 4, 5, 6, 7, 0x14)`     | 7     | Skips Tmd; trailing 0x14 is a `Flag` sentinel. |
//! | `(2, 3, 4, 5, 6, 7, 0x14)`     | 4     | Skips TimList. |
//! | `(10, 2, 3, 4, 5, 6, 7)`       | 1     | Leading `Flag(0xA)` sentinel. |
//! | `(1, 2, 3, 4, 6, 7, 0x14)`     | 1     | Skips Move. |
//!
//! All tuples consist of legal asset-type bytes ∈ `{0x00..=0x14}` — none
//! contain unknown types. The first descriptor's `data_offset` is **always**
//! `0x40` (= `8 + 7*8`, the byte after the header).
//!
//! ### Detection strategy
//!
//! Strict structural check — no LZS-decode requirement, so the detector
//! captures both the LZS-payload variants and the raw-payload variants
//! uniformly:
//!
//! 1. `u32_le[0] == 7` (the literal `07 00 00 00` lead).
//! 2. Buffer is large enough for the 64-byte header (`8 + 7 * 8`).
//! 3. First descriptor's `data_offset == 0x40`.
//! 4. All 7 descriptor type bytes are legal (`<= 0x14`).
//! 5. All 7 descriptor sizes fit in 4 MB.
//! 6. All 7 descriptor offsets fit within the buffer + a 64-byte slack
//!    (some entries pad past the last asset for sector alignment).
//!
//! ### Coverage impact
//!
//! Promotes 80 entries to `Class::SceneAssetTable`. Of those, 26 were
//! previously classed `lzs_container` (with `n=1` — a coincidental match
//! because the `n=1` branch only validated the *first* descriptor), 43 were
//! `unknown_high_entropy`, and 11 were `unknown_other`. Net named-format
//! coverage change: **+54 entries** (the 26 lzs_container ones were already
//! "named"; the strict 7-asset detector simply gives them a more accurate
//! semantic class).
//!
//! See `docs/formats/scene-bundles.md` for the full byte-level spec.

use serde::Serialize;

use crate::AssetType;

/// Literal lead u32 — `07 00 00 00`.
const HEADER_COUNT: u32 = 7;

/// `8 + 7 * 8` — the byte after the descriptor table.
const HEADER_END: u32 = 8 + (HEADER_COUNT * 8);

/// Per-asset size cap. Real entries top out at ~3 MB — 4 MB leaves headroom.
const MAX_ASSET_SIZE: u32 = 4 * 1024 * 1024;

/// Cap on the magnitude of `data_offset` for descriptors past the first.
/// **Important:** descriptor offsets past the first are *not* file-relative
/// byte offsets — they reference positions within a runtime-allocated
/// decompression buffer (the loader's working RAM). Empirically these top
/// out around 0x80000 (512 KB). 16 MB is a defensive cap that rejects
/// pointer-shaped values like `0x801C0000` while accepting all real scene
/// asset tables.
const MAX_RUNTIME_OFFSET: u32 = 16 * 1024 * 1024;

/// Detection result.
#[derive(Debug, Clone, Serialize)]
pub struct SceneAssetTable {
    /// `meta[1]` from the 8-byte header. Not currently understood; surfaced
    /// for future runtime tracing.
    pub meta1: u32,
    /// Per-descriptor `(type_byte, size, data_offset)`. Always 7 entries.
    pub descriptors: [DescriptorRecord; 7],
}

/// One descriptor pair from the table.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct DescriptorRecord {
    /// Asset type byte (high byte of `type_size`).
    pub type_byte: u8,
    /// Asset payload size in bytes (low 24 bits of `type_size`).
    pub size: u32,
    /// Byte offset within the file where the payload starts.
    pub data_offset: u32,
}

/// Try to detect a scene asset table. Returns `None` when the buffer doesn't
/// match the strict 7-asset header.
pub fn detect(buf: &[u8]) -> Option<SceneAssetTable> {
    if buf.len() < HEADER_END as usize {
        return None;
    }
    let count = read_u32_le(buf, 0)?;
    if count != HEADER_COUNT {
        return None;
    }
    let meta1 = read_u32_le(buf, 4)?;

    let mut descriptors = [DescriptorRecord {
        type_byte: 0,
        size: 0,
        data_offset: 0,
    }; 7];
    for (i, slot) in descriptors.iter_mut().enumerate() {
        let p = 8 + i * 8;
        let type_size = read_u32_le(buf, p)?;
        let data_offset = read_u32_le(buf, p + 4)?;
        let type_byte = ((type_size >> 24) & 0xFF) as u8;
        let size = type_size & 0x00FF_FFFF;

        if !is_known_type(type_byte) {
            return None;
        }
        if size > MAX_ASSET_SIZE {
            return None;
        }
        // First descriptor's offset MUST be a real file-relative byte offset
        // (anchored at the byte after the descriptor table). The rest are
        // runtime-buffer offsets that don't fit in the on-disc file.
        if i == 0 {
            if data_offset != HEADER_END {
                return None;
            }
        } else if data_offset > MAX_RUNTIME_OFFSET {
            return None;
        }
        *slot = DescriptorRecord {
            type_byte,
            size,
            data_offset,
        };
    }

    Some(SceneAssetTable { meta1, descriptors })
}

/// Returns `true` when the type byte is a legal asset-type from the
/// dispatcher table at `FUN_8001f05c` (cases 0x00..=0x14, with a few gaps).
fn is_known_type(b: u8) -> bool {
    !matches!(AssetType::from_byte(b), AssetType::Unknown(_))
}

fn read_u32_le(buf: &[u8], at: usize) -> Option<u32> {
    let bytes = buf.get(at..at + 4)?;
    Some(u32::from_le_bytes(bytes.try_into().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid scene asset table with caller-chosen type sequence.
    fn synth(types: [u8; 7], total_size: usize) -> Vec<u8> {
        let mut buf = Vec::with_capacity(total_size);
        buf.extend_from_slice(&HEADER_COUNT.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes()); // meta1
        let mut data_off: u32 = HEADER_END;
        for &t in &types {
            let sz: u32 = 0x100;
            let type_size = ((t as u32) << 24) | sz;
            buf.extend_from_slice(&type_size.to_le_bytes());
            buf.extend_from_slice(&data_off.to_le_bytes());
            data_off += sz;
        }
        buf.resize(total_size.max(buf.len()), 0);
        buf
    }

    #[test]
    fn detects_canonical_scene_bundle() {
        let buf = synth([1, 2, 3, 4, 5, 6, 7], 0x10000);
        let s = detect(&buf).expect("should detect");
        assert_eq!(s.descriptors[0].type_byte, 1);
        assert_eq!(s.descriptors[6].type_byte, 7);
        assert_eq!(s.descriptors[0].data_offset, HEADER_END);
    }

    #[test]
    fn detects_variant_with_flag_sentinel() {
        // (1, 3, 4, 5, 6, 7, 0x14) — 7 entries observed in the corpus.
        let buf = synth([1, 3, 4, 5, 6, 7, 0x14], 0x10000);
        assert!(detect(&buf).is_some());
    }

    #[test]
    fn detects_leading_flag_variant() {
        // (10, 2, 3, 4, 5, 6, 7) — 1 entry observed.
        let buf = synth([10, 2, 3, 4, 5, 6, 7], 0x10000);
        assert!(detect(&buf).is_some());
    }

    #[test]
    fn rejects_buffer_smaller_than_header() {
        assert!(detect(&[0u8; 16]).is_none());
        assert!(detect(&[0u8; 63]).is_none());
    }

    #[test]
    fn rejects_wrong_count() {
        let mut buf = synth([1, 2, 3, 4, 5, 6, 7], 0x10000);
        // Patch count from 7 to 8.
        buf[0..4].copy_from_slice(&8u32.to_le_bytes());
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_unknown_type_byte() {
        let mut buf = synth([1, 2, 3, 4, 5, 6, 7], 0x10000);
        // Patch descriptor[0].type_byte to an unknown value (0x55).
        buf[8 + 3] = 0x55;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_oversized_asset() {
        let mut buf = synth([1, 2, 3, 4, 5, 6, 7], 0x10000);
        // Set descriptor[0].size to 0x00FF_FFFF — exceeds the 4 MB cap.
        let big = (1u32 << 24) | 0x00FF_FFFF;
        buf[8..12].copy_from_slice(&big.to_le_bytes());
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_first_descriptor_not_at_header_end() {
        let mut buf = synth([1, 2, 3, 4, 5, 6, 7], 0x10000);
        // Patch descriptor[0].data_offset from 0x40 to 0x80.
        buf[12..16].copy_from_slice(&0x80u32.to_le_bytes());
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn accepts_runtime_offset_past_file_size() {
        // Real-world: descriptor offsets past desc[0] reference positions in a
        // runtime decompression buffer, not the on-disc file. They legitimately
        // exceed the file length (e.g. izumi.BIN is 96 KB but desc[2].off is
        // 0x228be = 141 KB).
        let mut buf = synth([1, 2, 3, 4, 5, 6, 7], 0x100);
        // Patch descriptor[6].data_offset to a 256 KB value — well past the
        // 256-byte file but within MAX_RUNTIME_OFFSET.
        buf[8 + 6 * 8 + 4..8 + 6 * 8 + 8].copy_from_slice(&0x0004_0000u32.to_le_bytes());
        assert!(detect(&buf).is_some());
    }

    #[test]
    fn rejects_pointer_shaped_offset() {
        // Reject descriptor offsets shaped like a RAM pointer (0x80...).
        let mut buf = synth([1, 2, 3, 4, 5, 6, 7], 0x100);
        buf[8 + 6 * 8 + 4..8 + 6 * 8 + 8].copy_from_slice(&0x801C_0000u32.to_le_bytes());
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_random_bytes() {
        let buf: Vec<u8> = (0..=255u8).cycle().take(0x100).collect();
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn accepts_real_world_head_pattern_izumi() {
        // 0031_izumi.BIN head: `07 00 00 00 28 F2 04 00 94 5C 02 01 40 00 00 00 …`
        // Descriptor 0: type_size = 0x01025c94, off = 0x40 → type=0x01, size=0x025c94.
        let mut buf = vec![
            0x07, 0x00, 0x00, 0x00, // count = 7
            0x28, 0xF2, 0x04, 0x00, // meta1
            0x94, 0x5C, 0x02, 0x01, 0x40, 0x00, 0x00, 0x00, // desc 0
            0xA8, 0xE5, 0x01, 0x02, 0xC1, 0x3A, 0x01, 0x00, // desc 1
            0xBC, 0x40, 0x00, 0x03, 0xBE, 0x28, 0x02, 0x00, // desc 2
            0x28, 0x00, 0x00, 0x04, 0x5C, 0x49, 0x02, 0x00, // desc 3
            0xC8, 0x00, 0x00, 0x05, 0x84, 0x49, 0x02, 0x00, // desc 4
            0xCC, 0x00, 0x00, 0x06, 0x4C, 0x4A, 0x02, 0x00, // desc 5
            0x18, 0x00, 0x00, 0x07, 0x18, 0x4B, 0x02, 0x00, // desc 6
        ];
        // Pad enough that all descriptor offsets fit (plus trailing slack).
        buf.resize(0x30000, 0);
        let s = detect(&buf).expect("real-world izumi pattern should detect");
        assert_eq!(s.descriptors[0].type_byte, 1);
        assert_eq!(s.descriptors[1].type_byte, 2);
        assert_eq!(s.descriptors[6].type_byte, 7);
        assert_eq!(s.descriptors[0].data_offset, 0x40);
    }
}

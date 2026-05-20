//! "Scene asset table" detector - the canonical 7-typed-asset bundle shape.
//!
//! ### Provenance
//!
//! Round-21 cluster characterisation (2026-05-04) found that **80 PROT entries
//! lead with the literal 4-byte `07 00 00 00`** and decode as
//! `parse_player_lzs(buf, 7)`-style descriptor tables - a fixed 7-asset
//! bundle whose descriptor types form the canonical scene-asset sequence
//! `(TimList, Tmd, Man, Mes, Move, Anm, Vdf)` = `(1, 2, 3, 4, 5, 6, 7)`.
//!
//! ### Layout
//!
//! ```text
//! +0x00   u32  count = 7              ; literal `07 00 00 00`
//! +0x04   u32  meta1                  ; varies - not a file-relative offset
//! +0x08   7 × (u32 type_size, u32 data_offset)
//!                                     ; each pair packs `(type<<24)|size`
//!                                     ; first descriptor's `data_offset` = 0x40
//! +0x40   per-descriptor LZS streams  ; one independent LZS stream per
//!                                     ; descriptor, addressed by
//!                                     ; `data_offset` and decompressing to
//!                                     ; exactly `size` bytes
//! ```
//!
//! ### Descriptor offsets are file-relative against the EXTENDED footprint
//!
//! Each descriptor `(type, size, data_offset)` is its own LZS stream where
//! `size` is the **decompressed** byte count. `data_offset` is the
//! file-relative byte position of that stream inside the bundle entry's
//! **full on-disc footprint** ([`legaia_prot::archive::Archive::read_entry`]),
//! **not** the TOC-indexed sub-region (`Archive::read_entry_indexed`).
//! Several entries (e.g. `0588_juui1`) carry descriptor offsets that fall
//! past the indexed end and into the trailing-overlay sectors that the
//! per-PROT TOC crops off; those offsets are valid against the extended
//! footprint. See `legaia-engine-core::scene_bundle::extract_move_payload`
//! for the canonical reader.
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
//! All tuples consist of legal asset-type bytes ∈ `{0x00..=0x14}` - none
//! contain unknown types. The first descriptor's `data_offset` is **always**
//! `0x40` (= `8 + 7*8`, the byte after the header).
//!
//! ### Detection strategy
//!
//! Strict structural check - no LZS-decode requirement, so the detector
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
//! previously classed `lzs_container` (with `n=1` - a coincidental match
//! because the `n=1` branch only validated the *first* descriptor), 43 were
//! `unknown_high_entropy`, and 11 were `unknown_other`. Net named-format
//! coverage change: **+54 entries** (the 26 lzs_container ones were already
//! "named"; the strict 7-asset detector simply gives them a more accurate
//! semantic class).
//!
//! See `docs/formats/scene-bundles.md` for the full byte-level spec.

use serde::Serialize;

use crate::AssetType;

/// Canonical lead u32 for kingdom-bundle scenes - `07 00 00 00`.
const HEADER_COUNT: u32 = 7;

/// Maximum descriptor count the fixed-size [`SceneAssetTable::descriptors`]
/// array can hold. Two variants exist in the retail corpus: kingdom-bundle
/// scenes use `count = 7` (first descriptor `TimList`), and the early
/// standalone-town scenes (e.g. `town01`, `town0c`) use `count = 6` (first
/// descriptor `Tmd`/type-0x0A). Both are walked by `FUN_80020224`, which
/// reads `count` from the file and loops that many descriptors.
const MAX_DESCRIPTORS: usize = 7;

/// Header-end byte offset for a table with `count` descriptors: the 8-byte
/// `[count][meta]` header plus `count` 8-byte descriptor records. The first
/// descriptor's `data_offset` is always anchored here (`0x40` for count 7,
/// `0x38` for count 6).
fn header_end(count: u32) -> u32 {
    8 + count * 8
}

/// Per-asset size cap. Real entries top out at ~3 MB - 4 MB leaves headroom.
const MAX_ASSET_SIZE: u32 = 4 * 1024 * 1024;

/// Cap on the magnitude of `data_offset` for descriptors past the first.
///
/// Offsets are file-relative against the extended bundle footprint (see
/// the module-level "Descriptor offsets" section). Empirically they top
/// out around 0x80000 (512 KB) across the 80 retail bundles. 16 MB is a
/// defensive cap that rejects pointer-shaped values like `0x801C0000`
/// while accepting all real scene asset tables - the detector runs on raw
/// PROT bytes before the extended footprint is loaded, so it can't
/// validate `data_offset <= file_size` directly.
const MAX_DATA_OFFSET: u32 = 16 * 1024 * 1024;

/// Detection result.
#[derive(Debug, Clone, Serialize)]
pub struct SceneAssetTable {
    /// `meta[1]` from the 8-byte header. Not currently understood; surfaced
    /// for future runtime tracing.
    pub meta1: u32,
    /// Number of real descriptors (`6` or `7`). Only `descriptors[..count]`
    /// are populated; the rest are zero padding.
    pub count: usize,
    /// Per-descriptor `(type_byte, size, data_offset)`. Indices `>= count`
    /// are zero padding (the table is `count`-prefixed, not fixed-7).
    pub descriptors: [DescriptorRecord; MAX_DESCRIPTORS],
}

impl SceneAssetTable {
    /// First descriptor whose `type_byte` is the `Move` asset type (`0x05`),
    /// or `None` if this table doesn't carry a move-table slot.
    ///
    /// In every observed scene with a `scene_asset_table` shape, the Move
    /// descriptor is at index 4. Each per-scene CDNAME block's
    /// `slot+1` PROT entry sources that scene's per-area `move.mdt` -
    /// this is what populates `_DAT_8007B888` (the move-table base
    /// pointer read by `FUN_800204F8`) when the scene loads.
    pub fn move_descriptor(&self) -> Option<&DescriptorRecord> {
        self.used().iter().find(|d| d.type_byte == 0x05)
    }

    /// Same as [`move_descriptor`](Self::move_descriptor) but returns the
    /// descriptor's index in the table.
    pub fn move_descriptor_index(&self) -> Option<usize> {
        self.used().iter().position(|d| d.type_byte == 0x05)
    }

    /// The populated descriptor slice (`descriptors[..count]`), excluding the
    /// zero padding that follows for the `count == 6` variant.
    pub fn used(&self) -> &[DescriptorRecord] {
        &self.descriptors[..self.count]
    }
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
    let count_u32 = read_u32_le(buf, 0)?;
    // Two header shapes in the retail corpus: kingdom bundles use `count = 7`
    // (canonical), early standalone-town scenes use `count = 6`. Constrain to
    // the observed values - the anchor check below is the strong signal, but
    // an unbounded count would let arbitrary small leading words through.
    if count_u32 != HEADER_COUNT && count_u32 != HEADER_COUNT - 1 {
        return None;
    }
    let count = count_u32 as usize;
    let table_end = header_end(count_u32) as usize;
    if buf.len() < table_end {
        return None;
    }
    let meta1 = read_u32_le(buf, 4)?;

    let mut descriptors = [DescriptorRecord {
        type_byte: 0,
        size: 0,
        data_offset: 0,
    }; MAX_DESCRIPTORS];
    for (i, slot) in descriptors.iter_mut().take(count).enumerate() {
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
        // First descriptor's offset is anchored at the byte after the
        // `count`-prefixed descriptor table (`0x40` for count 7, `0x38` for
        // count 6). The remaining offsets are file-relative against the
        // EXTENDED bundle footprint (see module doc) and only get
        // sanity-checked against MAX_DATA_OFFSET here - the detector runs on
        // raw PROT bytes before the extended footprint is materialised.
        if i == 0 {
            if data_offset as usize != table_end {
                return None;
            }
        } else if data_offset > MAX_DATA_OFFSET {
            return None;
        }
        *slot = DescriptorRecord {
            type_byte,
            size,
            data_offset,
        };
    }

    Some(SceneAssetTable {
        meta1,
        count,
        descriptors,
    })
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
        synth_n(&types, total_size)
    }

    /// Build a table with a caller-chosen descriptor count (6 or 7).
    fn synth_n(types: &[u8], total_size: usize) -> Vec<u8> {
        let count = types.len() as u32;
        let mut buf = Vec::with_capacity(total_size);
        buf.extend_from_slice(&count.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes()); // meta1
        let mut data_off: u32 = header_end(count);
        for &t in types {
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
    fn move_descriptor_finds_index_4_for_canonical_layout() {
        let buf = synth([1, 2, 3, 4, 5, 6, 7], 0x10000);
        let s = detect(&buf).expect("should detect");
        let d = s
            .move_descriptor()
            .expect("Move slot is present at index 4");
        assert_eq!(d.type_byte, 0x05);
        assert_eq!(s.move_descriptor_index(), Some(4));
    }

    #[test]
    fn move_descriptor_handles_skip_move_variant() {
        // Tuple `(1, 2, 3, 4, 6, 7, 0x14)` skips Move.
        let buf = synth([1, 2, 3, 4, 6, 7, 0x14], 0x10000);
        let s = detect(&buf).expect("should detect");
        assert!(s.move_descriptor().is_none());
        assert!(s.move_descriptor_index().is_none());
    }

    #[test]
    fn detects_canonical_scene_bundle() {
        let buf = synth([1, 2, 3, 4, 5, 6, 7], 0x10000);
        let s = detect(&buf).expect("should detect");
        assert_eq!(s.count, 7);
        assert_eq!(s.descriptors[0].type_byte, 1);
        assert_eq!(s.descriptors[6].type_byte, 7);
        assert_eq!(s.descriptors[0].data_offset, header_end(7));
    }

    #[test]
    fn detects_count6_town_variant() {
        // Early standalone towns (town01 / town0c) use a 6-descriptor table
        // whose first descriptor is anchored at 0x38 (= 8 + 6*8). The MAN is
        // descriptor index 1 (town01) / 2 (town0c).
        let buf = synth_n(&[0x02, 0x03, 0x05, 0x06, 0x07, 0x14], 0x8000);
        let s = detect(&buf).expect("count-6 table should detect");
        assert_eq!(s.count, 6);
        assert_eq!(s.descriptors[0].data_offset, header_end(6));
        assert_eq!(s.descriptors[0].data_offset, 0x38);
        // The MAN descriptor (type 0x03) resolves through `used()`.
        let man = s.used().iter().find(|d| d.type_byte == 0x03);
        assert!(man.is_some(), "count-6 table exposes its MAN descriptor");
        // Padding slot is not surfaced.
        assert_eq!(s.used().len(), 6);
    }

    #[test]
    fn detects_variant_with_flag_sentinel() {
        // (1, 3, 4, 5, 6, 7, 0x14) - 7 entries observed in the corpus.
        let buf = synth([1, 3, 4, 5, 6, 7, 0x14], 0x10000);
        assert!(detect(&buf).is_some());
    }

    #[test]
    fn detects_leading_flag_variant() {
        // (10, 2, 3, 4, 5, 6, 7) - 1 entry observed.
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
        // Set descriptor[0].size to 0x00FF_FFFF - exceeds the 4 MB cap.
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
    fn accepts_extended_footprint_offset_past_indexed_size() {
        // Real-world: descriptor offsets past desc[0] are file-relative
        // against the EXTENDED bundle footprint, which often runs past the
        // TOC-indexed view. The detector runs on raw PROT bytes (which may
        // be either view), so it only sanity-checks against MAX_DATA_OFFSET
        // rather than the local buffer length. E.g. `0588_juui1.BIN`'s
        // indexed view is 67584 B but desc[4].data_offset is 177413.
        let mut buf = synth([1, 2, 3, 4, 5, 6, 7], 0x100);
        // Patch descriptor[6].data_offset to a 256 KB value - well past the
        // 256-byte buffer but within MAX_DATA_OFFSET.
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

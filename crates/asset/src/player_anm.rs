//! Player-character animation bundle - the per-scene ANM blob the retail
//! engine allocates at `DAT_8007B7C8` (see [Ghidra dump of `FUN_8001F05C`
//! case 6](../../../ghidra/scripts/funcs/8001f05c.txt)).
//!
//! Each town scene's first PROT slot ships a multi-section
//! [`parse_player_lzs`](crate::parse_player_lzs)-shaped container; one of
//! the sections (typically section 2) is tagged with asset type byte
//! [`SCENE_ANM_TYPE_BYTE`] (= `0x05`, [`crate::AssetType::Move`]). Despite
//! the "MOVE" label in the dispatcher table, the content is a canonical
//! [`legaia_anm`]-shaped container with `marker_1 = 0x080C` records. The
//! dispatcher's `case 6` (which allocates `DAT_8007B7C8` via the
//! `anm_malloc_err` string) actually routes type-`0x05` data, not
//! type-`0x06` - a quirk between the runtime case index and the asset-type
//! byte.
//!
//! Confirmed corpus (byte-equality against live `DAT_8007B7C8` in the
//! [`v0_1_pre_battle_tetsu`](../../scripts/scenarios.toml) field-mode save):
//!
//! | PROT entry | CDNAME | Section | Records | Decoded bytes |
//! |---|---|---|---|---|
//! | `0004` | town01 | 2 | 69 | 96 448 |
//! | `0013` | town0b | 2 | 69 | 91 784 |
//! | `0183` | balden | 2 | 72 | 71 604 |
//! | `0408` | bubu1  | 2 | 70 | 87 844 |
//! | `1203` | other5 | 2 | 30 | 87 684 (battle variant) |
//!
//! The detector at [`find_in_entry`] walks the player-LZS container shape
//! and reports every type-[`SCENE_ANM_TYPE_BYTE`] section that decompresses
//! cleanly into a valid ANM container.
//!
//! ## Disc-vs-runtime layout
//!
//! Disc form (LZS-decompressed):
//!
//! ```text
//! +0x00  u32  count
//! +0x04  u32  byte_offsets[count]   ; ABSOLUTE byte offsets into the buffer
//!        ...  records start at byte_offsets[i]
//! ```
//!
//! The offsets are absolute (i.e. an offset of `0x118` means record 0 starts
//! at byte `0x118` of the buffer, exactly at the end of the offset table for
//! a 69-record bundle). This matches the existing [`legaia_anm::parse`]
//! convention.
//!
//! Each record's first 8 bytes are the canonical
//! [`legaia_anm::RecordHeader`] (`a`, `b`, `marker_1`, `flag`). Across the
//! corpus, `marker_1` is `0x080C` for every record, and the record-body
//! size obeys:
//!
//! ```text
//!     record_size = 16 + 8 * (a & 0xFF) * b
//! ```
//!
//! where `a & 0xFF` is the **bone count** (number of animated objects in the
//! TMD) and `b` is the **frame count**. The 16 = 8 (header) + 8 (per-anim
//! constants / first-frame reference) precedes a contiguous frame table:
//!
//! ```text
//! +0x00..+0x08    header (a, b, marker_1, flag)
//! +0x08..+0x10    per-anim leading 8 bytes (frame_0 / rest-pose hint -
//!                  exact meaning still TBC, see "Open threads" below)
//! +0x10..+end     frame_count frames; per frame:
//!                    bone_count × 8 bytes
//!                  each 8-byte entry is one bone's pose for that frame
//! ```
//!
//! Each per-bone, per-frame 8-byte entry is read as **4 `i16` values**.
//! The exact semantic layout (rotation/translation packing) is the
//! still-open thread - see the **Open threads** note in
//! [`docs/formats/anm.md`](../../../docs/formats/anm.md) for the working
//! hypothesis and the falsification status.
//!
//! Runtime form (what `FUN_8001F05C` case 6 allocates at `DAT_8007B7C8`):
//!
//! ```text
//! +0x00  u32  zero
//! +0x04  u32  self_ptr - 0x0C
//! +0x08  u32  end_ptr
//! +0x0C  u32  total_size
//! +0x10  -- disc-form starts here, byte-for-byte --
//! ```
//!
//! [`parse`] returns the disc form (no preamble).

use anyhow::{Result, bail};
use serde::Serialize;

use crate::{DecodeMode, decode, parse_player_lzs};

/// Asset type byte the per-scene player ANM bundle ships under. Counter-
/// intuitively this is the [`crate::AssetType::Move`] tag (`0x05`), not
/// [`crate::AssetType::Anm`] (`0x06`) - the dispatcher's `case 6` (which
/// allocates `DAT_8007B7C8`) actually routes type-0x05 data, not type-0x06.
pub const SCENE_ANM_TYPE_BYTE: u8 = 0x05;

/// ANM record `marker_1` - the first halfword of every record header
/// (at byte `+4` of the record, per [`legaia_anm::RecordHeader`]).
pub const ANM_MARKER_1: u16 = 0x080C;

/// Header size in bytes (a, b, marker_1, flag - matches
/// [`legaia_anm::RECORD_HEADER_SIZE`]).
pub const RECORD_HEADER_SIZE: usize = 8;

/// Size of the per-anim leading block that precedes the frame table.
/// Empirically `8` across every record in the corpus.
pub const RECORD_PROLOGUE_SIZE: usize = 8;

/// Bytes per (bone, frame) entry. Empirically `8` across every record in
/// the corpus; size formula `16 + 8 * (a & 0xFF) * b` falls out of this.
pub const BONE_FRAME_BYTES: usize = 8;

/// One decoded player-ANM record (one animation clip).
#[derive(Debug, Clone, Serialize)]
pub struct PlayerAnmRecord {
    /// Header `a` field - the **bone count** lives in `a & 0xFF`. The high
    /// byte appears to be a sub-format flag (set to `0x01` for records 9+
    /// in the field-form bundle and for every record in the battle-form
    /// bundle).
    pub a: u16,
    /// Header `b` field - **frame count** of this animation clip.
    pub b: u16,
    /// Canonical marker (`0x080C` for every record in the corpus).
    pub marker_1: u16,
    /// Sub-format selector (`0x02` / `0x04` in the field corpus, plus
    /// `0x0201` / `0x0401` / `0x0402` in the battle-form bundle).
    pub flag: u16,
    /// Computed bone count = `a & 0xFF`.
    pub bone_count: u16,
    /// Computed frame count = `b`.
    pub frame_count: u16,
    /// Per-anim leading 8 bytes (frame_0 reference / rest-pose hint -
    /// exact semantic still TBC).
    pub prologue: [u8; 8],
}

/// A single decoded player-ANM bundle (one type-0x05 section's worth of
/// records).
#[derive(Debug, Clone, Serialize)]
pub struct PlayerAnmBundle {
    /// `count` from the container header.
    pub record_count: u32,
    /// Absolute byte offsets of each record's start (one entry per record).
    pub record_offsets: Vec<u32>,
    /// Per-record sizes in bytes (`record_offsets[i+1] - record_offsets[i]`,
    /// or `decoded.len() - record_offsets[last]` for the final record).
    pub record_sizes: Vec<u32>,
    /// LZS-decoded bytes of the whole bundle (container header + offset
    /// table + records).
    pub decoded: Vec<u8>,
}

impl PlayerAnmBundle {
    /// Byte slice of record `index` (header + prologue + frames). Empty if
    /// `index` is past `record_count`.
    pub fn record_bytes(&self, index: usize) -> &[u8] {
        if index >= self.record_offsets.len() {
            return &[];
        }
        let start = self.record_offsets[index] as usize;
        let size = self.record_sizes[index] as usize;
        let end = start + size;
        if end > self.decoded.len() {
            return &[];
        }
        &self.decoded[start..end]
    }

    /// Read the `marker_1` halfword (at byte +4 of the record's header).
    pub fn record_marker_1(&self, index: usize) -> Option<u16> {
        let r = self.record_bytes(index);
        if r.len() < RECORD_HEADER_SIZE {
            return None;
        }
        Some(u16::from_le_bytes([r[4], r[5]]))
    }

    /// Decode record `index`'s header + sizes. Errors if the record's
    /// size doesn't satisfy the `16 + 8 * (a & 0xFF) * b` invariant.
    pub fn record(&self, index: usize) -> Result<PlayerAnmRecord> {
        let r = self.record_bytes(index);
        if r.len() < RECORD_HEADER_SIZE + RECORD_PROLOGUE_SIZE {
            bail!(
                "record {index} too small for header + prologue ({} < {})",
                r.len(),
                RECORD_HEADER_SIZE + RECORD_PROLOGUE_SIZE
            );
        }
        let a = u16::from_le_bytes([r[0], r[1]]);
        let b = u16::from_le_bytes([r[2], r[3]]);
        let marker_1 = u16::from_le_bytes([r[4], r[5]]);
        let flag = u16::from_le_bytes([r[6], r[7]]);
        let bone_count = a & 0xFF;
        let frame_count = b;
        let expected = RECORD_HEADER_SIZE
            + RECORD_PROLOGUE_SIZE
            + (bone_count as usize) * (frame_count as usize) * BONE_FRAME_BYTES;
        if r.len() != expected {
            bail!(
                "record {index} size mismatch: a=0x{a:04X} (bone_count={bone_count}), b={frame_count}, \
                 expected size = 16 + 8 * {bone_count} * {frame_count} = {expected}, \
                 actual = {}",
                r.len()
            );
        }
        let mut prologue = [0u8; 8];
        prologue.copy_from_slice(&r[RECORD_HEADER_SIZE..RECORD_HEADER_SIZE + RECORD_PROLOGUE_SIZE]);
        Ok(PlayerAnmRecord {
            a,
            b,
            marker_1,
            flag,
            bone_count,
            frame_count,
            prologue,
        })
    }

    /// Borrow the per-frame slice (`bone_count * 8` bytes) for one frame.
    /// Returns `&[]` on out-of-range record or frame.
    pub fn frame_bytes(&self, record_index: usize, frame_index: usize) -> &[u8] {
        let r = self.record_bytes(record_index);
        if r.len() < RECORD_HEADER_SIZE + RECORD_PROLOGUE_SIZE {
            return &[];
        }
        let bone_count = (u16::from_le_bytes([r[0], r[1]]) & 0xFF) as usize;
        let frame_count = u16::from_le_bytes([r[2], r[3]]) as usize;
        if frame_index >= frame_count {
            return &[];
        }
        let frame_bytes = bone_count * BONE_FRAME_BYTES;
        let off = RECORD_HEADER_SIZE + RECORD_PROLOGUE_SIZE + frame_index * frame_bytes;
        if off + frame_bytes > r.len() {
            return &[];
        }
        &r[off..off + frame_bytes]
    }

    /// Borrow the 8-byte (one bone, one frame) entry from record
    /// `record_index`, frame `frame_index`, bone `bone_index`. Returns `&[]`
    /// if any index is out of range.
    pub fn bone_frame_bytes(
        &self,
        record_index: usize,
        frame_index: usize,
        bone_index: usize,
    ) -> &[u8] {
        let f = self.frame_bytes(record_index, frame_index);
        if f.is_empty() {
            return &[];
        }
        let off = bone_index * BONE_FRAME_BYTES;
        if off + BONE_FRAME_BYTES > f.len() {
            return &[];
        }
        &f[off..off + BONE_FRAME_BYTES]
    }
}

/// Find every player-ANM-shaped section in a single PROT entry.
///
/// Walks `bytes` as a [`parse_player_lzs`]-shaped container with the given
/// `descriptor_count` (most scene bundles use 3, 5, 6, or 7). For each
/// type-[`SCENE_ANM_TYPE_BYTE`] descriptor, LZS-decode the section and
/// validate it parses as a canonical ANM container.
pub fn find_in_entry(bytes: &[u8], descriptor_count: usize) -> Vec<PlayerAnmBundle> {
    let Ok(container) = parse_player_lzs(bytes, descriptor_count) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for d in &container.descriptors {
        if d.type_byte != SCENE_ANM_TYPE_BYTE {
            continue;
        }
        let Ok(decoded) = decode(bytes, d, DecodeMode::Lzs) else {
            continue;
        };
        let Ok(bundle) = parse(&decoded) else {
            continue;
        };
        out.push(bundle);
    }
    out
}

/// Parse one fully-decoded player-ANM bundle (the LZS-decompressed bytes of
/// a type-0x05 section). Returns `Err` if the container header / offset
/// table / first record's marker_1 don't validate.
pub fn parse(decoded: &[u8]) -> Result<PlayerAnmBundle> {
    if decoded.len() < 8 {
        bail!("buffer too small for player-ANM container header");
    }
    let count = u32::from_le_bytes(decoded[..4].try_into().unwrap());
    if count == 0 || count > 256 {
        bail!("implausible record count {count}");
    }
    let count_us = count as usize;
    let table_end = 4 + count_us * 4;
    if table_end > decoded.len() {
        bail!(
            "offset table ({count} entries, ends at {table_end}) overruns buffer ({} bytes)",
            decoded.len()
        );
    }
    let mut offsets: Vec<u32> = Vec::with_capacity(count_us);
    for i in 0..count_us {
        let off = u32::from_le_bytes(decoded[4 + i * 4..8 + i * 4].try_into().unwrap());
        offsets.push(off);
    }
    // Offsets are ABSOLUTE byte offsets into `decoded`. They must be
    // monotonically non-decreasing and >= table_end (i.e. point past the
    // offset table).
    let mut prev = 0u32;
    for (i, off) in offsets.iter().enumerate() {
        let abs = *off as usize;
        if abs < table_end {
            bail!(
                "offset[{i}] 0x{off:X} points into the offset table (table ends at 0x{table_end:X})"
            );
        }
        if abs >= decoded.len() {
            bail!(
                "offset[{i}] 0x{off:X} overruns buffer ({} bytes)",
                decoded.len()
            );
        }
        if i > 0 && *off < prev {
            bail!("offsets not monotonic: offset[{i}] = 0x{off:X} < prev 0x{prev:X}");
        }
        prev = *off;
    }
    // Per-record sizes from consecutive offsets.
    let mut sizes: Vec<u32> = Vec::with_capacity(count_us);
    for i in 0..count_us {
        let end = if i + 1 < count_us {
            offsets[i + 1]
        } else {
            decoded.len() as u32
        };
        sizes.push(end - offsets[i]);
    }
    // First record's marker_1 (at byte +4 of the record) must be 0x080C.
    let r0_start = offsets[0] as usize;
    if r0_start + RECORD_HEADER_SIZE > decoded.len() {
        bail!("first record overruns buffer");
    }
    let m = u16::from_le_bytes([decoded[r0_start + 4], decoded[r0_start + 5]]);
    if m != ANM_MARKER_1 {
        bail!(
            "first record marker_1 mismatch: expected 0x{:04X}, got 0x{m:04X}",
            ANM_MARKER_1
        );
    }
    Ok(PlayerAnmBundle {
        record_count: count,
        record_offsets: offsets,
        record_sizes: sizes,
        decoded: decoded.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic bundle that mirrors the real disc layout:
    /// absolute offsets, marker at byte +4 of each record, and
    /// `size = 16 + 8 * bone_count * frame_count`.
    fn synthetic_two_records() -> Vec<u8> {
        // Two records: (a=2 bones, b=3 frames, size = 16 + 8*2*3 = 64) twice.
        let count: u32 = 2;
        let table_end = 4 + 4 * count as usize;
        let rec_size = 64;
        let mut buf = Vec::new();
        buf.extend_from_slice(&count.to_le_bytes());
        let off0 = table_end as u32;
        let off1 = off0 + rec_size;
        buf.extend_from_slice(&off0.to_le_bytes());
        buf.extend_from_slice(&off1.to_le_bytes());
        for r in 0..2u16 {
            // header: a=2, b=3, marker=0x080C, flag=0x0002
            buf.extend_from_slice(&2u16.to_le_bytes());
            buf.extend_from_slice(&3u16.to_le_bytes());
            buf.extend_from_slice(&ANM_MARKER_1.to_le_bytes());
            buf.extend_from_slice(&0x0002u16.to_le_bytes());
            // prologue (8 bytes)
            buf.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x00, 0x11]);
            // 3 frames × 2 bones × 8 bytes = 48 bytes, tag with record index
            for f in 0..3u8 {
                for bidx in 0..2u8 {
                    // 4 i16s: tag with record/frame/bone for visibility
                    let v: u8 = (r as u8) * 100 + f * 10 + bidx;
                    buf.extend_from_slice(&[v, 0, v + 1, 0, v + 2, 0, v + 3, 0]);
                }
            }
        }
        buf
    }

    #[test]
    fn parses_synthetic_two_records() {
        let buf = synthetic_two_records();
        let bundle = parse(&buf).expect("synthetic should parse");
        assert_eq!(bundle.record_count, 2);
        assert_eq!(bundle.record_offsets.len(), 2);
        assert_eq!(bundle.record_sizes, vec![64, 64]);
        assert_eq!(bundle.record_marker_1(0), Some(ANM_MARKER_1));
        assert_eq!(bundle.record_marker_1(1), Some(ANM_MARKER_1));
    }

    #[test]
    fn record_decodes_header_and_sizes() {
        let buf = synthetic_two_records();
        let bundle = parse(&buf).unwrap();
        let r0 = bundle.record(0).unwrap();
        assert_eq!(r0.a, 2);
        assert_eq!(r0.b, 3);
        assert_eq!(r0.marker_1, ANM_MARKER_1);
        assert_eq!(r0.flag, 0x0002);
        assert_eq!(r0.bone_count, 2);
        assert_eq!(r0.frame_count, 3);
        assert_eq!(
            r0.prologue,
            [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x00, 0x11]
        );
    }

    #[test]
    fn frame_and_bone_indexing() {
        let buf = synthetic_two_records();
        let bundle = parse(&buf).unwrap();
        // Record 0, frame 1, bone 0: tagged with (0*100 + 1*10 + 0) = 10
        let bf = bundle.bone_frame_bytes(0, 1, 0);
        assert_eq!(bf.len(), 8);
        assert_eq!(bf[0], 10);
        // Record 1, frame 2, bone 1: tagged with (1*100 + 2*10 + 1) = 121
        let bf = bundle.bone_frame_bytes(1, 2, 1);
        assert_eq!(bf.len(), 8);
        assert_eq!(bf[0], 121);
        // Out-of-range frame returns empty
        assert!(bundle.bone_frame_bytes(0, 99, 0).is_empty());
        // Out-of-range bone returns empty
        assert!(bundle.bone_frame_bytes(0, 0, 99).is_empty());
    }

    #[test]
    fn rejects_wrong_marker() {
        // Build a buffer where marker at +4 is wrong
        let mut buf = Vec::new();
        buf.extend_from_slice(&1u32.to_le_bytes()); // count
        buf.extend_from_slice(&8u32.to_le_bytes()); // offset (absolute)
        buf.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xAA, 0xBB]); // wrong marker at +4
        buf.extend_from_slice(&[0u8; 64]);
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn rejects_offset_in_table() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&1u32.to_le_bytes()); // count
        buf.extend_from_slice(&4u32.to_le_bytes()); // offset points into table (< 8)
        buf.extend_from_slice(&[0u8; 64]);
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn record_with_bad_size_errors() {
        // Build a single record that claims a=2, b=3 (expected size 64) but
        // actually leaves only 32 bytes.
        let mut buf = Vec::new();
        buf.extend_from_slice(&1u32.to_le_bytes()); // count
        buf.extend_from_slice(&8u32.to_le_bytes()); // offset = 8 (absolute, table is 4..8)
        // header
        buf.extend_from_slice(&2u16.to_le_bytes());
        buf.extend_from_slice(&3u16.to_le_bytes());
        buf.extend_from_slice(&ANM_MARKER_1.to_le_bytes());
        buf.extend_from_slice(&0x0002u16.to_le_bytes());
        // prologue + only 16 bytes (vs the expected 48 = 3 frames * 2 bones * 8)
        buf.extend_from_slice(&[0u8; 8]);
        buf.extend_from_slice(&[0u8; 16]);
        let bundle = parse(&buf).unwrap();
        assert!(bundle.record(0).is_err());
    }
}

//! Player-character animation bundle - the per-scene ANM blob the retail
//! engine allocates at `DAT_8007B7C8` (see [Ghidra dump of `FUN_8001F05C`
//! case 6](../../../ghidra/scripts/funcs/8001f05c.txt)).
//!
//! The data was historically thought to live in a dedicated PROT entry, with
//! [`crate::AssetType::Anm`] (type byte `0x06`) as the dispatcher tag. In
//! practice the player ANM ships **inside the per-scene asset bundle**
//! under the [`crate::AssetType::Move`] tag (type byte `0x05`, label
//! "MOVE" in the dispatcher table but content is a canonical
//! [`legaia_anm`]-shaped container with `marker_1 = 0x080C` records).
//!
//! ## Where it lives
//!
//! Each town scene's first PROT slot ships a multi-section
//! [`parse_player_lzs`](crate::parse_player_lzs)-shaped container; section 2
//! (the third descriptor) is a type-0x05 "MOVE" entry that LZS-decodes to
//! roughly 70-100 KB of animation records. There's also a battle-mode
//! variant at PROT `1203` (`other5` block, alongside the battle character
//! mesh pack at PROT 1204).
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
//! The remaining ~90 scene bundles either share an ANM blob with one of
//! these via run-time caching, or have smaller per-scene ANM sections that
//! the >10 000-byte filter excludes. The detector here doesn't filter on
//! size; it just walks the player-LZS container shape and reports every
//! type-0x05 section that decompresses cleanly into a valid ANM container.
//!
//! ## Disc-vs-runtime layout
//!
//! Disc form (LZS-decompressed):
//!
//! ```text
//! +0x00  u32  count
//! +0x04  u32  byte_offsets[count]   ; relative to +0x04 (table base)
//!        ...  records start at +0x04 + offsets[0]
//! ```
//!
//! Each record begins with the canonical [`legaia_anm`] header — the first
//! two bytes are `0x080C` (`marker_1`) and the next two select the
//! sub-format flag (`0x0002` is the common one seen in the player ANM
//! corpus). Per-bone keyframes follow per the
//! [`docs/formats/anm.md`](../../../docs/formats/anm.md) spec.
//!
//! Runtime form (what `FUN_8001F05C` case 6 allocates at `DAT_8007B7C8`):
//!
//! ```text
//! +0x00  u32  zero
//! +0x04  u32  self_ptr - 0x0C
//! +0x08  u32  end_ptr
//! +0x0C  u32  total_size
//! +0x10  u32  count                ; same as disc +0x00
//! +0x14  u32  byte_offsets[count]  ; same as disc +0x04
//!        ...  records (same as disc)
//! ```
//!
//! So the runtime adds a 16-byte preamble (`zero` / `self` / `end_ptr` /
//! `size`) before the disc-form container starts. The disc form is what
//! [`parse`] returns.

use anyhow::{Result, bail};
use serde::Serialize;

use crate::{DecodeMode, decode, parse_player_lzs};

/// Asset type byte the per-scene player ANM bundle ships under. Counter-
/// intuitively this is the [`crate::AssetType::Move`] tag (`0x05`), not
/// [`crate::AssetType::Anm`] (`0x06`) — the dispatcher's `case 6` (which
/// allocates `DAT_8007B7C8`) actually routes type-0x05 data, not type-0x06.
/// The mismatch between the runtime case index and the asset-type tag is a
/// quirk of the dispatcher; the data itself is canonical
/// [`legaia_anm`]-shaped animation records.
pub const SCENE_ANM_TYPE_BYTE: u8 = 0x05;

/// ANM record `marker_1` — the first halfword of every record body. Matches
/// [`legaia_anm::RECORD_MARKER_1`].
pub const ANM_MARKER_1: u16 = 0x080C;

/// A single decoded player-ANM section.
#[derive(Debug, Clone, Serialize)]
pub struct PlayerAnmBundle {
    /// `count` from the container header (record count).
    pub record_count: u32,
    /// `byte_offsets[count]` — one offset per record, relative to the
    /// offset-table base (i.e. `+0x04` of the disc container).
    pub record_offsets: Vec<u32>,
    /// LZS-decoded bytes of the whole bundle (container header + offset
    /// table + records). Pass to [`legaia_anm::parse`] for the per-record
    /// keyframe walk.
    pub decoded: Vec<u8>,
}

impl PlayerAnmBundle {
    /// Byte slice of record `index`'s body, including the 8-byte header
    /// (`a`, `b`, `marker_1`, `flag`). Empty if `index` is past
    /// `record_count`.
    pub fn record_bytes(&self, index: usize) -> &[u8] {
        if index >= self.record_offsets.len() {
            return &[];
        }
        let start = 4 + self.record_offsets[index] as usize;
        let end = if index + 1 < self.record_offsets.len() {
            4 + self.record_offsets[index + 1] as usize
        } else {
            self.decoded.len()
        };
        if end <= start || end > self.decoded.len() {
            return &[];
        }
        &self.decoded[start..end]
    }

    /// Read the `marker_1` halfword (offset `+0`) of one record. Returns
    /// `None` if the record is too short.
    pub fn record_marker_1(&self, index: usize) -> Option<u16> {
        let r = self.record_bytes(index);
        if r.len() < 2 {
            return None;
        }
        Some(u16::from_le_bytes([r[0], r[1]]))
    }
}

/// Find every player-ANM-shaped section in a single PROT entry.
///
/// Walks `bytes` as a [`parse_player_lzs`]-shaped container with the given
/// `descriptor_count` (most scene bundles use 3, 5, or 7). For each
/// type-[`SCENE_ANM_TYPE_BYTE`] descriptor, LZS-decode the section and
/// validate it parses as a canonical ANM container (small record count, all
/// offsets in-range, first record's marker_1 == 0x080C). Returns one
/// [`PlayerAnmBundle`] per cleanly-decoded section.
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
    // Validate offsets are monotonically non-decreasing and in-range.
    let mut prev = 0u32;
    for (i, off) in offsets.iter().enumerate() {
        if *off < prev {
            bail!("offset[{i}] = 0x{off:X} is less than prev 0x{prev:X}");
        }
        let abs = 4 + *off as usize;
        if abs >= decoded.len() {
            bail!(
                "offset[{i}] 0x{off:X} overruns buffer ({} bytes)",
                decoded.len()
            );
        }
        prev = *off;
    }
    // First record's marker_1 must be 0x080C.
    let r0_start = 4 + offsets[0] as usize;
    if r0_start + 2 > decoded.len() {
        bail!("first record overruns buffer");
    }
    let m = u16::from_le_bytes([decoded[r0_start], decoded[r0_start + 1]]);
    if m != ANM_MARKER_1 {
        bail!(
            "first record marker_1 mismatch: expected 0x{:04X}, got 0x{m:04X}",
            ANM_MARKER_1
        );
    }
    Ok(PlayerAnmBundle {
        record_count: count,
        record_offsets: offsets,
        decoded: decoded.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_synthetic() {
        // 2 records, each 16 bytes after the offset table.
        let mut buf: Vec<u8> = Vec::new();
        let count: u32 = 2;
        buf.extend_from_slice(&count.to_le_bytes());
        // offsets: 8 (record 0 starts 8 bytes after table base), 24 (record 1)
        buf.extend_from_slice(&8u32.to_le_bytes());
        buf.extend_from_slice(&24u32.to_le_bytes());
        // pad to record 0's position
        while buf.len() < 4 + 8 {
            buf.push(0);
        }
        // record 0: marker_1 + flag + zeros (16 bytes)
        buf.extend_from_slice(&ANM_MARKER_1.to_le_bytes());
        buf.extend_from_slice(&0x0002u16.to_le_bytes());
        buf.extend_from_slice(&[0u8; 12]);
        // record 1: marker_1 + flag + zeros
        buf.extend_from_slice(&ANM_MARKER_1.to_le_bytes());
        buf.extend_from_slice(&0x0002u16.to_le_bytes());
        buf.extend_from_slice(&[0u8; 12]);
        let bundle = parse(&buf).expect("synthetic parses");
        assert_eq!(bundle.record_count, 2);
        assert_eq!(bundle.record_offsets, vec![8, 24]);
        assert_eq!(bundle.record_marker_1(0), Some(ANM_MARKER_1));
        assert_eq!(bundle.record_marker_1(1), Some(ANM_MARKER_1));
    }

    #[test]
    fn rejects_wrong_marker() {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&1u32.to_le_bytes()); // count
        buf.extend_from_slice(&4u32.to_le_bytes()); // offset
        buf.extend_from_slice(&[0xAA, 0xBB]); // marker_1 wrong
        buf.extend_from_slice(&[0u8; 6]);
        assert!(parse(&buf).is_err());
    }
}

//! Scene VDF (vertex-deformation) pack - the type-byte `0x07` slot of a
//! scene bundle.
//!
//! Every `scene_asset_table` bundle reserves a VDF slot; on the scenes with
//! animated geometry it is populated with a morph-delta pack (jou - Rim Elm
//! fused with the Juggernaut - carries 17 sub-entries; the `jouina`/`jouind`/
//! `jouine` interiors carry the largest packs on the disc). The battle-side
//! sibling is the standalone `vdf.dat` (extraction 0872, loaded whole by the
//! battle init chain - see `docs/formats/effect.md`).
//!
//! ## Layout
//!
//! ```text
//! +0x00  u32  count               ; sub-entry count
//! +0x04  u32  offsets[count]      ; byte offsets from the pack base,
//!                                 ; strictly ascending
//! per sub-entry (at offsets[i]):
//!   u32  record_count
//!   per record:
//!     u32 group_id               ; TMD group (object) the record morphs
//!     u32 dst_index              ; first destination vertex
//!     u32 delta_count
//!     delta_count x 8 bytes      ; [i16 dx][i16 dy][i16 dz][pad]
//! ```
//!
//! The runtime chain: asset-type dispatcher case 7 stores the decoded pack at
//! `DAT_8007B7DC`, `FUN_8001FBCC` walks the sub-entries into the parallel
//! pointer table at `0x80083E58`, move-VM op `0x0A` arms per-actor morph
//! lanes with sub-entry indices, the ramp envelope (`FUN_80020740`) moves
//! each lane's weight, and the morph stager `FUN_8001C604` applies the
//! weighted deltas to the actor's TMD group vertices per frame. The record
//! walk itself is ported at `legaia_engine_vm::vdf_morph`
//! (`parse_vdf_morph_records`); this module owns the on-disc pack framing.
//!
//! REF: FUN_8001FBCC, FUN_8001C604

/// A parsed scene VDF pack: the decoded slot bytes plus validated sub-entry
/// offsets. Sub-entry payloads are borrowed views into `bytes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneVdf {
    /// The decoded (decompressed) type-7 slot payload.
    pub bytes: Vec<u8>,
    /// Validated sub-entry byte offsets (ascending, in-bounds).
    pub offsets: Vec<usize>,
}

impl SceneVdf {
    /// Number of sub-entries.
    pub fn len(&self) -> usize {
        self.offsets.len()
    }

    /// `true` when the pack has no sub-entries.
    pub fn is_empty(&self) -> bool {
        self.offsets.is_empty()
    }

    /// Sub-entry `i`'s bytes: `[u32 record_count][records...]`, bounded by
    /// the next sub-entry's offset (or the pack end for the last one). This
    /// is the slice the `0x80083E58` pointer table hands to `FUN_8001C604` -
    /// feed it to `legaia_engine_vm::vdf_morph::parse_vdf_morph_records`.
    pub fn sub_entry(&self, i: usize) -> Option<&[u8]> {
        let start = *self.offsets.get(i)?;
        let end = self.offsets.get(i + 1).copied().unwrap_or(self.bytes.len());
        self.bytes.get(start..end)
    }
}

/// Parse a decoded VDF pack buffer (`[u32 count][u32 offsets[count]]` +
/// sub-entry payloads). Structural validation: plausible count, ascending
/// in-bounds offsets that start past the offset table.
pub fn parse(bytes: Vec<u8>) -> Result<SceneVdf, String> {
    if bytes.len() < 4 {
        return Err("truncated header".into());
    }
    let count = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
    if count == 0 || count > 256 {
        return Err(format!("implausible sub-entry count {count}"));
    }
    let table_end = 4 + count * 4;
    if bytes.len() < table_end {
        return Err("offset table truncated".into());
    }
    let mut offsets = Vec::with_capacity(count);
    let mut prev = 0usize;
    for i in 0..count {
        let off = u32::from_le_bytes(bytes[4 + i * 4..8 + i * 4].try_into().unwrap()) as usize;
        if off < table_end || off > bytes.len() {
            return Err(format!("sub-entry {i} offset {off:#x} out of range"));
        }
        if i > 0 && off <= prev {
            return Err(format!("sub-entry {i} offset {off:#x} not ascending"));
        }
        prev = off;
        offsets.push(off);
    }
    Ok(SceneVdf { bytes, offsets })
}

/// Decode a scene bundle PROT entry's type-7 slot and parse it as a VDF
/// pack. `None` when the entry has no asset table or no type-7 slot;
/// `Some(Err)` for the 4-byte placeholder slots (most scenes) and decode
/// failures.
pub fn from_scene_bundle(entry: &[u8]) -> Option<Result<SceneVdf, String>> {
    let decoded = match crate::scene_asset_table::decode_slot_by_type(entry, 7)? {
        Ok(b) => b,
        Err(e) => return Some(Err(e)),
    };
    Some(parse(decoded))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(entries: &[&[u8]]) -> Vec<u8> {
        let count = entries.len() as u32;
        let mut b = count.to_le_bytes().to_vec();
        let mut off = 4 + entries.len() * 4;
        for e in entries {
            b.extend_from_slice(&(off as u32).to_le_bytes());
            off += e.len();
        }
        for e in entries {
            b.extend_from_slice(e);
        }
        b
    }

    #[test]
    fn parses_sub_entries_with_bounds() {
        let a = [1u8, 0, 0, 0, 9, 9, 9, 9];
        let b = [2u8, 0, 0, 0];
        let v = parse(pack(&[&a, &b])).expect("parse");
        assert_eq!(v.len(), 2);
        assert_eq!(v.sub_entry(0).unwrap(), &a);
        assert_eq!(v.sub_entry(1).unwrap(), &b);
        assert_eq!(v.sub_entry(2), None);
    }

    #[test]
    fn rejects_bad_headers() {
        assert!(parse(vec![0, 0, 0, 0]).is_err(), "count 0");
        assert!(parse(vec![1, 0, 0, 0]).is_err(), "table truncated");
        // Offset inside the offset table.
        let mut b = 1u32.to_le_bytes().to_vec();
        b.extend_from_slice(&4u32.to_le_bytes());
        assert!(parse(b).is_err(), "offset inside table");
        // Descending offsets.
        let mut b = 2u32.to_le_bytes().to_vec();
        b.extend_from_slice(&20u32.to_le_bytes());
        b.extend_from_slice(&12u32.to_le_bytes());
        b.extend_from_slice(&[0; 12]);
        assert!(parse(b).is_err(), "not ascending");
    }
}

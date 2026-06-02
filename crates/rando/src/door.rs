//! Scene-transition ("door / exit") randomization.
//!
//! A field scene reaches another scene through the field-VM **`0x3F`
//! named-scene-change op**, which carries its destination inline:
//! `[i16 index][u8 name_len][name][entry_x][entry_z][dir]`. These ops live in
//! **partition-2 MAN records**, addressed at runtime through the partition-2
//! record-offset table (the controller sets the VM bytecode base to
//! `man_base + data_region + partition2[slot]` and runs the record — pinned by a
//! PCSX-Redux dispatch trace). Selection is by stable slot index, so the op's
//! `index` is just the destination-scene id.
//!
//! Because the destination *name* is variable length, re-pointing a door at a
//! differently-named scene changes the record's byte length. The
//! [`legaia_asset::man_edit`] relocation engine makes that safe: it resizes the
//! name and fixes the partition record-offset tables (the door dispatch index),
//! the header section-0 offset, and any intra-record relative-jump deltas that
//! straddle the edit. So — unlike encounters/chests, which are same-size edits —
//! a door's destination can be **any** scene regardless of name length.
//!
//! [`SceneDoors`] locates a scene bundle's MAN, decompresses it, and enumerates
//! its door sites ([`legaia_asset::man_edit::scene_change_sites`], the clean
//! partition walk). [`SceneDoors::rebuild`] applies a set of destination
//! rewrites, recompresses, and returns the new MAN stream + decompressed size
//! when it fits the original compressed footprint and validates — the caller
//! then writes both the recompressed stream and the descriptor's
//! decompressed-size word back to the disc.

use legaia_asset::man_edit::{self, DestEdit, SceneChangeSite};
use legaia_asset::scene_asset_table;

/// MAN asset type byte in a scene bundle's descriptor table.
const MAN_TYPE: u8 = 0x03;

/// A scene bundle's MAN located in a PROT entry, with its door (`0x3F`) sites.
pub struct SceneDoors {
    /// PROT entry index this scene bundle lives in.
    pub entry_idx: usize,
    /// Byte offset of the compressed MAN stream within the entry.
    pub man_offset: usize,
    /// Bytes the recompressed MAN must fit within (the original compressed
    /// length; the data after belongs to the next asset).
    pub compressed_budget: usize,
    /// Byte offset, within the entry, of the MAN descriptor's
    /// `(type<<24)|size` word — rewritten when the decompressed size changes.
    pub man_descriptor_off: usize,
    /// Decompressed MAN buffer.
    pub decoded: Vec<u8>,
    /// The door sites, sorted by op offset.
    pub sites: Vec<SceneChangeSite>,
}

impl SceneDoors {
    /// Locate a scene bundle's MAN and its door sites, or `None` when the entry
    /// isn't a scene bundle, has no MAN, the MAN doesn't decode, or it carries
    /// no clean door site.
    pub fn locate(entry: &[u8], entry_idx: usize) -> Option<Self> {
        let table = scene_asset_table::detect(entry)?;
        let man_idx = table.descriptor_index(MAN_TYPE)?;
        let man = table.used()[man_idx];
        if man.size == 0 || man.data_offset == 0 {
            return None;
        }
        let man_offset = man.data_offset as usize;
        let body = entry.get(man_offset..)?;
        let (decoded, consumed) = legaia_lzs::decompress_tracked(body, man.size as usize).ok()?;
        if decoded.len() != man.size as usize {
            return None;
        }
        let sites = man_edit::scene_change_sites(&decoded);
        if sites.is_empty() {
            return None;
        }
        Some(Self {
            entry_idx,
            man_offset,
            compressed_budget: consumed,
            man_descriptor_off: scene_asset_table::SceneAssetTable::size_word_offset(man_idx),
            decoded,
            sites,
        })
    }

    /// Apply a set of destination rewrites to this scene's MAN and recompress.
    /// Returns `(recompressed_stream, new_decompressed_size)` when the rebuild
    /// validates and the stream fits the original compressed footprint; `None`
    /// otherwise (overflow / validation failure → caller leaves the scene
    /// unchanged). The new size word is `scene_asset_table::encode_size_word(
    /// 0x03, new_size)`, written at [`Self::man_descriptor_off`].
    pub fn rebuild(&self, edits: &[DestEdit]) -> Option<(Vec<u8>, u32)> {
        if edits.is_empty() {
            return None;
        }
        let new_man = man_edit::apply_dest_edits(&self.decoded, edits).ok()?;
        // Validate: each edited op (at its mapped offset) decodes with the
        // intended name in the rebuilt buffer.
        let expected: Vec<(usize, Vec<u8>)> = edits
            .iter()
            .map(|e| {
                (
                    man_edit::map_offset_after(edits, &self.decoded, e.op_pc),
                    e.name.clone(),
                )
            })
            .collect();
        let exp_refs: Vec<(usize, &[u8])> =
            expected.iter().map(|(o, n)| (*o, n.as_slice())).collect();
        if !man_edit::validate(&new_man, &exp_refs) {
            return None;
        }
        let stream = legaia_lzs::compress(&new_man);
        if stream.len() > self.compressed_budget {
            return None;
        }
        Some((stream, new_man.len() as u32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_change_site_fields_round_trip_through_man_edit() {
        // A minimal MAN with one partition-2 destination record, exercised
        // through the public enumerator + editor (no disc).
        // record: [name_len=1]["XY"][c0][c1][c2] + 0x3F op + Nop.
        let mut man = vec![0u8; 0x2B + 3];
        man[0x26] = 1; // N2 = 1
        man[0x28] = 6; // u24_at_28 = record length (set below)
        // partition2[0] = 0 at 0x2B.
        let mut rec = vec![0x01, b'X', b'Y', 0x00, 0x00, 0x00];
        // 0x3F index=5 name="ab" entry (0x10,0x20) dir 0x30
        rec.extend_from_slice(&[0x3F, 0x05, 0x00, 0x02, b'a', b'b', 0x10, 0x20, 0x30, 0x21]);
        man[0x28] = rec.len() as u8;
        man.extend_from_slice(&rec);
        man.extend_from_slice(&[0u8; 18]); // 6 terminator sections

        let sites = man_edit::scene_change_sites(&man);
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].partition, 2);
        assert_eq!(sites[0].index, 5);
        assert_eq!(sites[0].name, "ab");
        assert_eq!(sites[0].entry_x, 0x10);
        assert_eq!(sites[0].entry_z, 0x20);
        assert_eq!(sites[0].dir, 0x30);
    }
}

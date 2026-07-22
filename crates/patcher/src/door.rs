//! Scene-transition ("door / exit") randomization.
//!
//! A field scene reaches another scene through the field-VM **`0x3F`
//! named-scene-change op**, which carries its destination inline:
//! `[i16 index][u8 name_len][name][entry_x][entry_z][dir]`. These ops live in
//! **partition-2 MAN records**, addressed at runtime through the partition-2
//! record-offset table (the controller sets the VM bytecode base to
//! `man_base + data_region + partition2[slot]` and runs the record - pinned by a
//! PCSX-Redux dispatch trace). Selection is by stable slot index, so the op's
//! `index` is just the destination-scene id.
//!
//! Because the destination *name* is variable length, re-pointing a door at a
//! differently-named scene changes the record's byte length. The
//! [`legaia_asset::man_edit`] relocation engine makes that safe: it resizes the
//! name and fixes the partition record-offset tables (the door dispatch index),
//! the header section-0 offset, and any intra-record relative-jump deltas that
//! straddle the edit. So - unlike encounters/chests, which are same-size edits -
//! a door's destination can be **any** scene regardless of name length.
//!
//! [`SceneDoors`] locates a scene bundle's MAN, decompresses it, and enumerates
//! its door sites ([`legaia_asset::man_edit::scene_change_sites`], the clean
//! partition walk). [`SceneDoors::rebuild`] applies a set of destination
//! rewrites, recompresses, and returns the new MAN stream + decompressed size
//! when it fits the original compressed footprint and validates - the caller
//! then writes both the recompressed stream and the descriptor's
//! decompressed-size word back to the disc.
//!
//! ## Shuffle-pool eligibility ([`DoorSiteClass`])
//!
//! Not every `0x3F` site is a door a player walks through. The same op family
//! carries **scripted scene changes**: cutscene continuations (the world-map
//! hubs hold story records like the Rim Elm Genesis-Tree-revival return, run
//! at arrival under story state, not from a doorway tile) and event warps
//! invoked from other records' scripts. Shuffling those replays a cutscene hop
//! at a random door - and hands the cutscene a random destination - so the
//! shuffle pool is gated structurally:
//!
//! - **Walk-trigger evidence.** A genuine door's partition-2 record is spawned
//!   by a `.MAP` kind-1 **gate-1** tile trigger (`[tile_x, tile_z, record,
//!   gate]`, retail `FUN_801D1EC4` → `FUN_8003BDE0`; the same table the engine
//!   port dispatches). A site whose carrying record no gate-1 trigger
//!   references is script/cutscene-invoked - excluded, kept vanilla. Trigger
//!   evidence is the discriminator because the trigger dispatcher is the only
//!   pinned partition-2 spawner (no field-VM op that runs a P2 record by
//!   index is known) and every identified cutscene record on the retail disc
//!   is trigger-less; should a script-spawn path ever be pinned, its records
//!   must demote to `ScriptInvoked` - exclusion wins over shuffle coverage.
//! - **World-map endpoints.** Any site whose home scene **or** destination is
//!   a kingdom-overworld hub ([`WORLD_MAP_SCENES`]) is excluded: the hubs'
//!   own records are dominated by arrival/story scripts, and scene↔overworld
//!   transitions interleave with the world-map controller's own state - both
//!   directions of every town↔overworld connection stay vanilla.
//! - Non-partition-2 sites (the handful of P0/P1 `0x3F` ops) are script
//!   choreography by construction - excluded.

use legaia_asset::man_edit::{self, DestEdit, SceneChangeSite};
use legaia_asset::{man_section, scene_asset_table};

/// MAN asset type byte in a scene bundle's descriptor table.
const MAN_TYPE: u8 = 0x03;

/// CDNAME labels of the kingdom-overworld hub scenes (Drake / Sebucus /
/// Karisto). Door sites touching these - as home or destination - are excluded
/// from the shuffle pool (see the module doc).
pub const WORLD_MAP_SCENES: [&str; 3] = ["map01", "map02", "map03"];

/// Is `name` a kingdom-overworld hub scene label?
pub fn is_world_map_scene(name: &str) -> bool {
    WORLD_MAP_SCENES.contains(&name)
}

/// Shuffle-pool classification of one `0x3F` site (see the module doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoorSiteClass {
    /// A genuine walk-through door: a partition-2 record referenced by a
    /// `.MAP` kind-1 gate-1 walk trigger, with both endpoints field scenes.
    /// The only class the randomizer moves.
    WalkDoor,
    /// Script/cutscene-invoked: no walk trigger spawns the carrying record
    /// (or the site isn't in a partition-2 record at all). Kept vanilla.
    ScriptInvoked,
    /// Home scene or destination is a kingdom-overworld hub. Kept vanilla.
    WorldMap,
}

/// Partition-2 record slots referenced by a **kind-1 gate-1** tile trigger in
/// one `.MAP` trigger block: the records the walk-on dispatch spawns when the
/// player crosses the trigger tile - the structural definition of "a door you
/// walk through". Gate-0 entries bind partition-0 object scripts and are not
/// P2 references. `block` is a trigger block (the `.MAP`'s `+0x10000` primary
/// or the `+0x12000` fallback window - the next PROT entry's first sectors,
/// which retail's lookup scans second and where many scenes keep their door
/// triggers). Header shape shared with the kind-0 teleport table: sub-table
/// offset `s16` at `+4k+2`, count `s16` at `+4k+4`, kind-1 records
/// `[tile_x, tile_z, record, gate]`. Because the fallback window is shared
/// bytes with the sibling entry, rows are sanity-gated: tile coords inside the
/// `0x80`-tile grid and gate exactly `1` (garbage rows from a non-trigger
/// sibling fail these, so they can't launder a cutscene record into the pool).
/// REF: FUN_801D5AE0, FUN_801D1EC4, FUN_8003BDE0
pub fn walk_trigger_p2_slots(block: &[u8]) -> std::collections::BTreeSet<usize> {
    let mut out = std::collections::BTreeSet::new();
    let read_s16 = |off: usize| -> Option<i16> {
        Some(i16::from_le_bytes([*block.get(off)?, *block.get(off + 1)?]))
    };
    let (Some(off), Some(count)) = (read_s16(6), read_s16(8)) else {
        return out;
    };
    if off < 0 || count <= 0 || count as usize > 0x400 {
        return out;
    }
    let (off, count) = (off as usize, count as usize);
    for i in 0..count {
        let Some(r) = block.get(off + i * 4..off + i * 4 + 4) else {
            break;
        };
        if r[0] < 0x80 && r[1] < 0x80 && r[3] == 1 {
            out.insert(r[2] as usize);
        }
    }
    out
}

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
    /// `(type<<24)|size` word - rewritten when the decompressed size changes.
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
        // Compressed footprint the recompressed MAN may grow into: the gap from
        // the MAN stream to the next asset descriptor's data (the bytes between
        // the original compressed stream and the next asset are slack/padding,
        // safe to write into). Falls back to the original `consumed` length.
        let next_off = table
            .used()
            .iter()
            .map(|d| d.data_offset as usize)
            .filter(|&o| o > man_offset)
            .min();
        let compressed_budget = match next_off {
            Some(end) if end <= entry.len() => (end - man_offset).max(consumed),
            _ => consumed,
        };
        Some(Self {
            entry_idx,
            man_offset,
            compressed_budget,
            man_descriptor_off: scene_asset_table::SceneAssetTable::size_word_offset(man_idx),
            decoded,
            sites,
        })
    }

    /// The partition-2 record slot carrying each site, in [`Self::sites`]
    /// order (`None` when the site isn't inside a partition-2 record, or the
    /// containing record can't be attributed unambiguously). The slot is the
    /// index the `.MAP` kind-1 triggers reference.
    pub fn site_p2_slots(&self) -> Vec<Option<usize>> {
        let Ok(mf) = man_section::parse(&self.decoded) else {
            return vec![None; self.sites.len()];
        };
        let dro = mf.data_region_offset;
        // Every record start (all partitions) + section starts bound a record.
        let mut bounds: Vec<usize> = Vec::new();
        for part in &mf.partitions {
            for &off in part {
                bounds.push(dro + off as usize);
            }
        }
        for s in &mf.sections {
            bounds.push(s.offset);
        }
        bounds.sort_unstable();
        bounds.dedup();
        self.sites
            .iter()
            .map(|site| {
                if site.partition != 2 {
                    return None;
                }
                // Containing P2 record: greatest P2 start <= op_pc, with no
                // other record/section start between it and the op.
                let (slot, start) = mf.partitions[2]
                    .iter()
                    .enumerate()
                    .map(|(ri, &off)| (ri, dro + off as usize))
                    .filter(|&(_, st)| st <= site.op_pc)
                    .max_by_key(|&(_, st)| st)?;
                let intruded = bounds
                    .iter()
                    .any(|&b| b > start && b <= site.op_pc && b != start);
                (!intruded).then_some(slot)
            })
            .collect()
    }

    /// Classify each site for shuffle eligibility (see the module doc), in
    /// [`Self::sites`] order. `home_scene` is this scene's CDNAME label;
    /// `walk_slots` is the union of [`walk_trigger_p2_slots`] over the scene's
    /// `.MAP` trigger blocks (`None` when the scene has no located `.MAP` -
    /// then nothing is provably a walk door and every non-world-map site
    /// classifies [`DoorSiteClass::ScriptInvoked`]). A referenced slot only
    /// counts when it indexes a real partition-2 record of this MAN.
    pub fn classify_sites(
        &self,
        home_scene: &str,
        walk_slots: Option<&std::collections::BTreeSet<usize>>,
    ) -> Vec<DoorSiteClass> {
        let p2_count = man_section::parse(&self.decoded)
            .map(|mf| mf.partitions[2].len())
            .unwrap_or(0);
        let slots = self.site_p2_slots();
        self.sites
            .iter()
            .zip(slots)
            .map(|(site, slot)| {
                if is_world_map_scene(home_scene) || is_world_map_scene(&site.name) {
                    DoorSiteClass::WorldMap
                } else if slot
                    .is_some_and(|sl| sl < p2_count && walk_slots.is_some_and(|w| w.contains(&sl)))
                {
                    DoorSiteClass::WalkDoor
                } else {
                    DoorSiteClass::ScriptInvoked
                }
            })
            .collect()
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

    /// A minimal MAN with one partition-2 destination record carrying a single
    /// `0x3F` op with destination `name`.
    fn one_door_man(name: &[u8]) -> Vec<u8> {
        let mut man = vec![0u8; 0x2B + 3];
        man[0x26] = 1; // N2 = 1
        // partition2[0] = 0 at 0x2B.
        let mut rec = vec![0x01, b'X', b'Y', 0x00, 0x00, 0x00];
        // 0x3F index=5 name=<name> entry (0x10,0x20) dir 0x30
        rec.extend_from_slice(&[0x3F, 0x05, 0x00, name.len() as u8]);
        rec.extend_from_slice(name);
        rec.extend_from_slice(&[0x10, 0x20, 0x30, 0x21]);
        man[0x28] = rec.len() as u8; // u24_at_28 = record length
        man.extend_from_slice(&rec);
        man.extend_from_slice(&[0u8; 18]); // 6 terminator sections
        man
    }

    fn scene_doors_for(man: Vec<u8>) -> SceneDoors {
        let sites = man_edit::scene_change_sites(&man);
        SceneDoors {
            entry_idx: 0,
            man_offset: 0,
            compressed_budget: 0,
            man_descriptor_off: 0,
            decoded: man,
            sites,
        }
    }

    #[test]
    fn scene_change_site_fields_round_trip_through_man_edit() {
        // A minimal MAN with one partition-2 destination record, exercised
        // through the public enumerator + editor (no disc).
        let man = one_door_man(b"ab");
        let sites = man_edit::scene_change_sites(&man);
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].partition, 2);
        assert_eq!(sites[0].index, 5);
        assert_eq!(sites[0].name, "ab");
        assert_eq!(sites[0].entry_x, 0x10);
        assert_eq!(sites[0].entry_z, 0x20);
        assert_eq!(sites[0].dir, 0x30);
    }

    #[test]
    fn walk_trigger_slots_parse_and_sanity_gate() {
        // kind-1 header: sub-table offset s16 at +6, count s16 at +8.
        let mut block = vec![0u8; 0x40];
        block[6..8].copy_from_slice(&16i16.to_le_bytes());
        block[8..10].copy_from_slice(&4i16.to_le_bytes());
        block[16..20].copy_from_slice(&[10, 11, 3, 1]); // valid gate-1 spawn
        block[20..24].copy_from_slice(&[10, 12, 4, 0]); // gate-0 object bind
        block[24..28].copy_from_slice(&[0x90, 11, 5, 1]); // tile out of grid
        block[28..32].copy_from_slice(&[12, 13, 3, 1]); // duplicate slot
        let slots = walk_trigger_p2_slots(&block);
        assert_eq!(slots.into_iter().collect::<Vec<_>>(), vec![3]);
        // Garbage / short blocks parse to nothing.
        assert!(walk_trigger_p2_slots(&[0u8; 4]).is_empty());
        assert!(walk_trigger_p2_slots(&[0xFFu8; 0x20]).is_empty());
    }

    #[test]
    fn classify_gates_on_trigger_reference_and_world_map() {
        let sd = scene_doors_for(one_door_man(b"ab"));
        assert_eq!(sd.site_p2_slots(), vec![Some(0)]);
        // No .MAP located → nothing is provably a walk door.
        assert_eq!(
            sd.classify_sites("town01", None),
            vec![DoorSiteClass::ScriptInvoked]
        );
        // A gate-1 trigger references the carrying record → walk door.
        let hit: std::collections::BTreeSet<usize> = [0].into();
        assert_eq!(
            sd.classify_sites("town01", Some(&hit)),
            vec![DoorSiteClass::WalkDoor]
        );
        // Triggers reference some other record → script-invoked.
        let miss: std::collections::BTreeSet<usize> = [1].into();
        assert_eq!(
            sd.classify_sites("town01", Some(&miss)),
            vec![DoorSiteClass::ScriptInvoked]
        );
        // A world-map endpoint dominates: home hub ...
        assert_eq!(
            sd.classify_sites("map01", Some(&hit)),
            vec![DoorSiteClass::WorldMap]
        );
        // ... or hub destination.
        let to_hub = scene_doors_for(one_door_man(b"map02"));
        assert_eq!(
            to_hub.classify_sites("town01", Some(&hit)),
            vec![DoorSiteClass::WorldMap]
        );
    }
}

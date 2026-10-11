//! Object binds: which MAN partition-0 record drives which object-pack slot, and the
//! record lookups (anim id, spawn flags) those binds read.

use super::*;

/// The **object bind** of a placed field object: the MAN partition-0 record
/// retail attaches to it at scene init.
///
/// `FUN_8003A55C` resolves it by the object's footprint-anchor tile
/// (`func_0x801d5630(1, anchor_col, anchor_row)` - the `.MAP` kind-1
/// tile-trigger sub-table, primary block first then the `+0x12000` fallback),
/// takes the trigger's `record` byte, and reads that record out of the MAN's
/// flat record-offset table. Partition 0 comes first in that table, so the
/// index is a partition-0 record index.
///
/// The record's header is `[u8 n][n*2 name bytes][u8 anim_id]`, and the script
/// begins right after it. `FUN_8003A55C` stores the record base into the
/// actor's `+0x90` (script buffer), the post-header offset into `+0x9E` (PC),
/// and the header's last byte into `+0x5C` - the actor's animation id.
///
/// **A placed record whose anchor tile has no trigger is not `FUN_8003A55C`'s** -
/// it is skipped there, and a live Rim Elm capture's init actor list holds exactly
/// the bound ones (37 of `town01`'s 46 placements). It is **not** unspawned,
/// though: the sub-area window sweep `FUN_801D7B50` creates those nine, gated on
/// the complementary anchor-tile bit
/// [`CELL_BIND_OWNED`](legaia_scene_formats::field_objects::CELL_BIND_OWNED) rather than
/// on a bind, so they draw unscripted and unposed.
// REF: FUN_8003A55C, FUN_801D5630, FUN_800204f8, FUN_801D7B50
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectBind {
    /// MAN partition-0 record index the anchor tile's trigger names.
    pub record: u8,
    /// The record header's trailing byte: the actor's animation id
    /// (`0` = none). Indexes the per-scene ANM bundle as `anim_id - 1`.
    pub anim_id: u8,
}

/// Resolve every object bind a scene's `.MAP` + MAN define, keyed by the
/// **anchor tile** (`(anchor_col, anchor_row)`) the placement looks it up by.
///
/// `field_map` is the extended `.MAP` footprint (the trigger blocks live at
/// `+0x10000` and `+0x12000`); `man_file` / `man` are the scene's parsed MAN.
/// Triggers are scanned primary-block-first, so a tile carried by both blocks
/// resolves to the primary entry, as in retail's `FUN_801D5630`. The trigger's
/// dispatch `gate` is **not** consulted: `FUN_8003A55C` binds whatever kind-1
/// entry sits on the tile (retail towns only put gate-0 entries there).
///
/// PORT: FUN_8003A55C (the `func_0x801d5630` bind lookup + record-header decode)
pub fn object_binds(
    field_map: &[u8],
    man_file: &ManFile,
    man: &[u8],
) -> HashMap<(u8, u8), ObjectBind> {
    let mut triggers: Vec<TileTrigger> = Vec::new();
    for base in [
        field_regions::MAP_REGION_BLOCK_OFFSET,
        field_regions::MAP_TRIGGER_FALLBACK_OFFSET,
    ] {
        if let Some(block) = field_map.get(base..) {
            triggers.extend(field_regions::parse_tile_triggers(block));
        }
    }
    let mut out: HashMap<(u8, u8), ObjectBind> = HashMap::new();
    for t in triggers {
        let key = (t.tile_x, t.tile_z);
        if out.contains_key(&key) {
            continue; // primary block wins (retail scans it first)
        }
        let Some(anim_id) = partition0_anim_id(man_file, man, t.record as usize) else {
            continue;
        };
        out.insert(
            key,
            ObjectBind {
                record: t.record,
                anim_id,
            },
        );
    }
    out
}

/// Flat MAN record -> the env-pack slots of every placed object whose
/// footprint-anchor tile binds it: the meshes a bound object's op-`0x4B`
/// morph lanes (`World::arm_field_morph`) reach. One record
/// can bind several placements (`rikuroa`'s Genesis tree is three objects on
/// three records, each its own mesh).
// REF: FUN_8003A55C
pub fn object_record_pack_slots(
    placements: &[Placement],
    binds: &HashMap<(u8, u8), ObjectBind>,
) -> std::collections::BTreeMap<u16, Vec<usize>> {
    let mut out: std::collections::BTreeMap<u16, Vec<usize>> = Default::default();
    for p in placements {
        let (Some(pack), Some(bind)) = (p.pack_index, binds.get(&(p.anchor_col, p.anchor_row)))
        else {
            continue;
        };
        let slots = out.entry(u16::from(bind.record)).or_default();
        if !slots.contains(&(pack as usize)) {
            slots.push(pack as usize);
        }
    }
    out
}

/// The animation id in MAN partition-0 record `index`'s header
/// (`[u8 n][n*2 name bytes][u8 anim_id]`). `None` when the record or its header
/// runs past the buffer.
pub(super) fn partition0_anim_id(man_file: &ManFile, man: &[u8], index: usize) -> Option<u8> {
    let off = man_file
        .data_region_offset
        .checked_add(*man_file.partitions[0].get(index)? as usize)?;
    let n = *man.get(off)? as usize;
    man.get(off.checked_add(1)?.checked_add(2 * n)?).copied()
}

/// The actor `+0x10` bits MAN partition-0 record `index`'s spawn prologue
/// sets ([`PropProgram::spawn_cflags`]) - the collision / interaction class
/// of the placed object the record binds, available without building a bank
/// entry (an `anim_id == 0` bound placement still carries its class: e.g. a
/// `31 00` born-exempt marker object).
pub fn record_spawn_cflags(man_file: &ManFile, man: &[u8], index: usize) -> u32 {
    match partition0_record(man_file, man, index) {
        Some((record, pc0)) => decode_prop_program(record, pc0).spawn_cflags,
        None => 0,
    }
}

/// A partition-0 record's bytes and the PC just past its
/// `[u8 n][n*2 name bytes][u8 anim_id]` header - where `FUN_8003A55C` parks the
/// actor's script cursor (`actor+0x9E`).
pub(super) fn partition0_record<'a>(
    man_file: &ManFile,
    man: &'a [u8],
    index: usize,
) -> Option<(&'a [u8], usize)> {
    let start = man_file
        .data_region_offset
        .checked_add(*man_file.partitions[0].get(index)? as usize)?;
    let n = *man.get(start)? as usize;
    let pc0 = 1 + 2 * n + 1;
    // The record runs to the next record start in the flat table (partitions
    // are laid out back to back), or to the end of the MAN.
    let mut end = man.len();
    for part in &man_file.partitions {
        for &off in part {
            let a = man_file.data_region_offset.checked_add(off as usize)?;
            if a > start && a < end {
                end = a;
            }
        }
    }
    let body = man.get(start..end)?;
    if pc0 >= body.len() {
        return None;
    }
    Some((body, pc0))
}

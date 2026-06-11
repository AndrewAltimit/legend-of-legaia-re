//! Battle character-mesh assembly from a player battle file.
//!
//! Clean-room port of the retail battle-setup chain that builds each party
//! member's in-battle TMD out of their `data\battle\PLAYER<n>` file (see
//! [`crate::battle_data_pack`] for the container and
//! `docs/formats/character-mesh.md` § Battle form for the full chain):
//!
//! 1. **Section selection** ([`select_sections`]): walk the descriptor
//!    table's five sections, matching each entry's id against the
//!    character's equipped item id for that slot; an `id = 0` entry
//!    supplies the section default and advances the slot.
//! 2. **Object splice** ([`assemble_character`]): LZS-decode the five
//!    selected sections and splice their TMD objects into one merged TMD -
//!    object entries relocated into the merged pool, one bone-id byte per
//!    attached object, surplus objects (the equipment's visual meshes)
//!    tagged and sorted to the end, with their attach bones recorded.
//!
//! The output mirrors the retail blob the engine registers into
//! `DAT_8007C018[slot]` (standard relative-offset Legaia TMD + the bone-tag
//! and attach-bone side tables), byte-verified against a full-party battle
//! save (Vahn: `nobj = 17`, tags `[0..14, 200, 201]`, attach `[5, 8]`; the
//! 24-slot object table and the data pool are byte-exact vs the live blob,
//! with the only differences being each primitive's TSB +3 / CBA +0x40
//! rewrite - the separate per-slot runtime-band relocation pass applied at
//! registration, see `docs/formats/character-mesh.md` § Battle render).
//! This module emits the disc-authentic (authoring) TSB/CBA values.

use anyhow::{Context, Result, bail};

use crate::battle_data_pack::{BattleDataPack, Record, decode_record};

/// Number of equipment sections per player file (= equip slots in the
/// character record's `+0x196..+0x19A` byte order).
pub const SECTION_COUNT: usize = 5;

/// Legaia TMD magic.
const TMD_MAGIC: u32 = 0x8000_0002;
/// Bytes per TMD object-table entry (7 u32 words).
const OBJ_ENTRY_BYTES: usize = 0x1C;
/// The retail assembler reserves a fixed 24-entry object table in the
/// merged blob (data cursor pre-positioned at `blob + 0x2C4` = object-table
/// start + `24 * 0x1C`, regardless of the actual object count); the gap
/// after the real entries is zero in the output.
pub const RESERVED_OBJ_SLOTS: usize = 24;

/// One decoded equipment section, framed the way the retail assembler reads
/// it: the loader frame at `decoded + u32[0]` carries the attach list and
/// the embedded TMD (see `docs/formats/battle-data-pack.md`
/// § Decompressed slot layout).
struct Section {
    /// Bone ids for the first `attach_count` objects.
    bone_ids: Vec<u8>,
    /// The section TMD's object count.
    nobj: usize,
    /// The section's 7-word object-table entries (raw bytes).
    obj_table: Vec<u8>,
    /// The section's data words (vertex / normal / primitive pools).
    data: Vec<u8>,
}

/// Select the five equipment sections of `pack` for a character whose
/// equipped item ids are `equipped` (the char record's `+0x196..+0x19A`
/// bytes, in the file's section order). Returns one descriptor [`Record`]
/// per section.
///
/// An entry matching the current slot's equipped id is selected; an
/// `id = 0` entry supplies the section's default when nothing matched and
/// advances to the next slot.
// PORT: FUN_80052770 (case 4) - the equipment-section selector; the match
// reads the live char record's +0x196 equip bytes against each 12-byte
// descriptor entry, with the id=0 separator doubling as the default slot.
pub fn select_sections(
    pack: &BattleDataPack,
    equipped: &[u8; SECTION_COUNT],
) -> Result<[Record; SECTION_COUNT]> {
    let mut selected: [Option<Record>; SECTION_COUNT] = [None; SECTION_COUNT];
    let mut slot = 0usize;
    for record in &pack.records {
        if slot >= SECTION_COUNT {
            break;
        }
        if record.id == equipped[slot] as u32 && selected[slot].is_none() {
            selected[slot] = Some(*record);
        }
        if record.id == 0 {
            // Section default + slot advance. (When the equipped id is 0
            // the match above already took this entry.)
            if selected[slot].is_none() {
                selected[slot] = Some(*record);
            }
            slot += 1;
        }
    }
    if slot < SECTION_COUNT {
        bail!("descriptor table ends after {slot} sections (expected {SECTION_COUNT})");
    }
    Ok(std::array::from_fn(|i| {
        selected[i].expect("every section has at least its id = 0 default")
    }))
}

/// A character battle mesh assembled from its player file.
pub struct AssembledCharacter {
    /// Standard relative-offset Legaia TMD (`[magic][flags = 0][nobj]
    /// [object table][data]`). This is the pre-registration layout - the
    /// retail registrar later rewrites the offsets absolute (`flags` 0->1).
    pub tmd: Vec<u8>,
    /// One tag byte per object, post-sort: skeleton objects carry their
    /// bone id (ascending), surplus equipment meshes sort last with tags
    /// `200+` (first surplus of a section) / `100+` (further surpluses).
    pub bone_tags: Vec<u8>,
    /// Attach bone for each `200+`-tagged equipment mesh, in tag order:
    /// the bone id of the object that preceded it in its section.
    pub attach_bones: Vec<u8>,
    /// The descriptor entries the five sections came from.
    pub sections: [Record; SECTION_COUNT],
}

/// Assemble a character's battle TMD from their player file (`buf` +
/// parsed `pack`) and equipped item ids.
// PORT: FUN_80052FA0 (mesh half) - decodes the five selected sections and
// builds the merged TMD at ctx+0x50 (magic at blob+0x18, nobj accumulated
// by the per-section splice); the palette half of the same function is
// ported as crate::battle_char_palette.
pub fn assemble_character(
    buf: &[u8],
    pack: &BattleDataPack,
    equipped: &[u8; SECTION_COUNT],
) -> Result<AssembledCharacter> {
    let records = select_sections(pack, equipped)?;
    let mut sections = Vec::with_capacity(SECTION_COUNT);
    for (i, rec) in records.iter().enumerate() {
        sections.push(
            decode_section(buf, pack, rec)
                .with_context(|| format!("section {i} (id {:#x})", rec.id))?,
        );
    }

    // The merged object-table region is a fixed 24-slot reservation
    // (retail pre-positions its data cursor at object-table start +
    // 24 * 0x1C before the first splice).
    let total_nobj: usize = sections.iter().map(|s| s.nobj).sum();
    if total_nobj > RESERVED_OBJ_SLOTS {
        bail!(
            "assembled object count {total_nobj} exceeds the {RESERVED_OBJ_SLOTS}-slot reservation"
        );
    }
    let obj_table_bytes = RESERVED_OBJ_SLOTS * OBJ_ENTRY_BYTES;

    let mut obj_table = Vec::with_capacity(obj_table_bytes);
    let mut data = Vec::new();
    let mut tags: Vec<u8> = Vec::with_capacity(total_nobj);
    // PORT: FUN_800536BC - the object splice. Per section: append the
    // 7-word object entries with vert/normal/prim offsets relocated into
    // the merged pool, copy the data words, accumulate nobj, and write one
    // bone-id byte per object from the attach list (surplus objects get
    // the 0xFF-first / 0xFE-rest tags = the equipment visual meshes).
    for s in &sections {
        // Offsets in a section entry are relative to the section's
        // object-table start; its data begins right after that table. In
        // the merged TMD the section's data lands at
        // `obj_table_bytes + data.len()` relative to the merged
        // object-table start.
        let delta = (obj_table_bytes + data.len()) as i64 - (s.nobj * OBJ_ENTRY_BYTES) as i64;
        for k in 0..s.nobj {
            let e = &s.obj_table[k * OBJ_ENTRY_BYTES..(k + 1) * OBJ_ENTRY_BYTES];
            // Words 0 / 2 / 4 are vert_top / normal_top / prim_top;
            // words 1 / 3 / 5 / 6 (counts + scale) copy through.
            for w in 0..7 {
                let v = u32::from_le_bytes(e[w * 4..w * 4 + 4].try_into().unwrap());
                let v = if w % 2 == 0 && w < 6 {
                    (v as i64 + delta) as u32
                } else {
                    v
                };
                obj_table.extend_from_slice(&v.to_le_bytes());
            }
            tags.push(if k < s.bone_ids.len() {
                s.bone_ids[k]
            } else if k == s.bone_ids.len() {
                0xFF
            } else {
                0xFE
            });
        }
        data.extend_from_slice(&s.data);
    }

    // PORT: FUN_80053898 - post-pass. First pass retags the surplus
    // objects (0xFF -> 200,201,..., 0xFE -> 100,101,...) and records each
    // 0xFF extra's attach bone (= the previous object's bone id); second
    // pass selection-sorts the object table by tag so the skeleton bones
    // come first (ascending) and the equipment extras land last. (Retail
    // additionally mirrors the 0xFE count into the battle context at
    // +0x240 + slot - engine-context, not part of the mesh.)
    let mut attach_bones = Vec::new();
    let mut ff_seen = 0u8;
    let mut fe_seen = 0u8;
    for i in 0..tags.len() {
        match tags[i] {
            0xFF => {
                attach_bones.push(tags[i - 1]);
                tags[i] = 200 + ff_seen;
                ff_seen += 1;
            }
            0xFE => {
                tags[i] = 100 + fe_seen;
                fe_seen += 1;
            }
            _ => {}
        }
    }
    // Selection sort ascending by tag, swapping 7-word entries in step.
    for i in 0..tags.len() {
        let mut min = i;
        for j in i + 1..tags.len() {
            if tags[j] < tags[min] {
                min = j;
            }
        }
        if min != i {
            tags.swap(i, min);
            for w in 0..7 {
                let a = i * OBJ_ENTRY_BYTES + w * 4;
                let b = min * OBJ_ENTRY_BYTES + w * 4;
                for k in 0..4 {
                    obj_table.swap(a + k, b + k);
                }
            }
        }
    }

    let mut tmd = Vec::with_capacity(12 + obj_table_bytes + data.len());
    tmd.extend_from_slice(&TMD_MAGIC.to_le_bytes());
    tmd.extend_from_slice(&0u32.to_le_bytes()); // flags = 0 (relative offsets)
    tmd.extend_from_slice(&(total_nobj as u32).to_le_bytes());
    tmd.extend_from_slice(&obj_table);
    // Zero-fill the unused tail of the 24-slot reservation so data lands
    // at the retail offset.
    tmd.resize(12 + obj_table_bytes, 0);
    tmd.extend_from_slice(&data);

    Ok(AssembledCharacter {
        tmd,
        bone_tags: tags,
        attach_bones,
        sections: records,
    })
}

/// Decode one selected section through the loader frame at
/// `decoded + u32[0]` (see `docs/formats/battle-data-pack.md`
/// § Decompressed slot layout).
fn decode_section(buf: &[u8], pack: &BattleDataPack, rec: &Record) -> Result<Section> {
    let entry = decode_record(buf, pack, rec.index)?;
    let d = &entry.bytes;
    let frame = read_u32(d, 0)? as usize;
    let attach_count =
        *d.get(frame)
            .ok_or_else(|| anyhow::anyhow!("loader frame past decoded end"))? as usize;
    let bone_ids = d
        .get(frame + 1..frame + 1 + attach_count)
        .ok_or_else(|| anyhow::anyhow!("attach list past decoded end"))?
        .to_vec();
    let data_size = read_u32(d, frame + 8)? as usize;
    let magic = read_u32(d, frame + 0xC)?;
    if magic != TMD_MAGIC {
        bail!("section TMD magic {magic:#010x}");
    }
    let nobj = read_u32(d, frame + 0x14)? as usize;
    if nobj == 0 || nobj > 64 {
        bail!("implausible section nobj {nobj}");
    }
    if nobj < attach_count {
        bail!("attach list longer than the object table ({attach_count} > {nobj})");
    }
    let table_start = frame + 0x18;
    let table_end = table_start + nobj * OBJ_ENTRY_BYTES;
    let obj_table = d
        .get(table_start..table_end)
        .ok_or_else(|| anyhow::anyhow!("object table past decoded end"))?
        .to_vec();
    // Data span: the retail splice copies `(data_size - nobj*0x1C - 9) >> 2`
    // words starting right after the section object table.
    let data_words = (data_size.saturating_sub(nobj * OBJ_ENTRY_BYTES + 9)) >> 2;
    let data = d
        .get(table_end..table_end + data_words * 4)
        .ok_or_else(|| anyhow::anyhow!("data span past decoded end"))?
        .to_vec();
    Ok(Section {
        bone_ids,
        nobj,
        obj_table,
        data,
    })
}

fn read_u32(buf: &[u8], off: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        buf.get(off..off + 4)
            .ok_or_else(|| anyhow::anyhow!("u32 read at {off:#x} past end"))?
            .try_into()
            .unwrap(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle_data_pack::Record;

    fn rec(index: usize, id: u32) -> Record {
        Record {
            index,
            id,
            data_offset: 0,
            size: 0x800,
        }
    }

    fn pack_with_ids(ids: &[u32]) -> BattleDataPack {
        BattleDataPack {
            table_offset: 0x40,
            records: ids.iter().enumerate().map(|(i, &id)| rec(i, id)).collect(),
            data_base: 0x8000,
        }
    }

    #[test]
    fn select_prefers_equipped_id_over_default() {
        // Two sections: [5, 4, 0] and [9, 0].
        let pack = pack_with_ids(&[5, 4, 0, 9, 0, 0, 0, 0]);
        let sel = select_sections(&pack, &[4, 9, 0, 0, 0]).unwrap();
        assert_eq!(sel[0].id, 4);
        assert_eq!(sel[1].id, 9);
        // Unequipped slots take the id = 0 defaults.
        assert_eq!(sel[2].id, 0);
    }

    #[test]
    fn select_falls_back_to_default() {
        let pack = pack_with_ids(&[5, 4, 0, 9, 0, 0, 0, 0]);
        let sel = select_sections(&pack, &[7, 7, 7, 7, 7]).unwrap();
        assert!(sel.iter().all(|r| r.id == 0), "all defaults");
    }

    #[test]
    fn select_requires_five_sections() {
        let pack = pack_with_ids(&[5, 0]);
        assert!(select_sections(&pack, &[0; 5]).is_err());
    }
}

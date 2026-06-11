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
//! [`assemble_character`] emits the disc-authentic (authoring) TSB/CBA
//! values; [`relocate_tsb_cba`] applies the registration-time pass that
//! moves them into the per-slot runtime VRAM band.

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

/// VRAM CLUT row of party slot 0's runtime palette (rows `481 + slot`,
/// i.e. 481/482/483 for Vahn/Noa/Gala).
pub const RUNTIME_CLUT_ROW_BASE: u16 = 0x1E1;

/// 5-bit TSB texpage index of party slot 0's first runtime page
/// (`0x18` = VRAM `(512, 256)`); slot `s` uses pages `0x18 + 2s` /
/// `0x19 + 2s`, packing the party band into `x ∈ [512, 896), y = 256`.
pub const RUNTIME_TEXPAGE_BASE: u16 = 0x18;

/// The authoring texpage index every player-file section meshes at; the
/// relocation maps it to the slot's **first** runtime page and every other
/// authoring page to the **second** (the player files author exactly two
/// pages, `0x15`/`0x16`, so this is a faithful per-page remap: `+3` on the
/// texpage index for slot 0).
pub const AUTHORING_FIRST_TEXPAGE: u16 = 0x15;

/// Relocate an assembled battle TMD's texture addressing into party slot
/// `slot`'s runtime VRAM band, in place. Retail runs this pass at battle
/// registration, right after installing the blob into
/// `DAT_8007C018[slot]`; the on-disc (authoring) TSB/CBA — texpages
/// `0x15`/`0x16` = `(320, 256)`/`(384, 256)`, CLUT row 480 — are never
/// sampled by a normal battle. Per **textured** primitive (group-header
/// mode byte TME bit set):
///
/// - **CBA**: CLUT row (bits 6..14) ← `481 + slot`; the column
///   (`(cba & 0x3F) * 16`) and the high bit are preserved. For the
///   authoring row 480 this is the `+0x40` CLUT-id rewrite the live
///   runtime blob exhibits.
/// - **TSB**: texpage index (bits 0..4) ← `0x18 + 2*slot` when the
///   authoring page is `0x15`, else `0x19 + 2*slot`; ABR / colour-depth
///   bits are preserved. For the authoring pages `0x15`/`0x16` this is
///   the `+3` texpage rewrite (slot 0).
///
/// Untextured groups (`F*`/`G*`) carry no texture block and are left
/// untouched. Returns the number of primitives rewritten.
// PORT: FUN_80053a28 - the per-slot TSB/CBA relocation loop (walks each
// object's primitive groups, gated on the group mode byte's TME bit;
// CBA word & 0x803fffff | (0x1e1+slot)<<22, TSB word & 0xffe0ffff |
// (slot*2 + (page==0x15 ? 0x18 : 0x19))<<16).
// REF: FUN_800513F0 - the battle scene-loader state that calls it per
// party slot right after registering the assembled blob.
pub fn relocate_tsb_cba(tmd_bytes: &mut [u8], slot: u8) -> Result<usize> {
    let tmd = legaia_tmd::parse(tmd_bytes).context("parse assembled TMD for relocation")?;
    // Collect each textured prim's texture-block start first (immutable
    // walk), then rewrite. The block is `[u0, v0, cba, u1, v1, tsb, ...]`
    // ending at the descriptor's vertex-index offset.
    let mut blocks: Vec<usize> = Vec::new();
    for obj in &tmd.objects {
        let groups = legaia_tmd::legaia_prims::iter_groups(
            tmd_bytes,
            obj.primitives_byte_offset,
            obj.primitives_byte_size,
        )
        .context("walk primitive groups for relocation")?;
        for group in groups {
            // Retail gates on the group mode byte's TME bit (mode & 4).
            if group.header.mode & 0x04 == 0 {
                continue;
            }
            let Some(vert_off) = legaia_tmd::legaia_prims::vertex_offset_bytes(group.header.flags)
            else {
                continue;
            };
            let block_len = 4 + group.header.n_vertices() * 2;
            if vert_off < block_len {
                continue;
            }
            for prim in &group.prims {
                blocks.push(prim.bytes_offset + vert_off - block_len);
            }
        }
    }
    for &bs in &blocks {
        let Some(block) = tmd_bytes.get_mut(bs..bs + 8) else {
            bail!("texture block at {bs:#x} past TMD end");
        };
        let cba = u16::from_le_bytes([block[2], block[3]]);
        let new_cba = (cba & 0x803F) | ((RUNTIME_CLUT_ROW_BASE + slot as u16) << 6);
        block[2..4].copy_from_slice(&new_cba.to_le_bytes());
        let tsb = u16::from_le_bytes([block[6], block[7]]);
        let page = if tsb & 0x1F == AUTHORING_FIRST_TEXPAGE {
            RUNTIME_TEXPAGE_BASE + slot as u16 * 2
        } else {
            RUNTIME_TEXPAGE_BASE + 1 + slot as u16 * 2
        };
        let new_tsb = (tsb & 0xFFE0) | page;
        block[6..8].copy_from_slice(&new_tsb.to_le_bytes());
    }
    Ok(blocks.len())
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

    /// Build a minimal one-object Legaia TMD whose primitive section holds
    /// one textured FT3 group (two prims, authoring texpages `0x15`/`0x16`,
    /// CLUT row 480) followed by one untextured F3 group, for exercising
    /// the relocation rewrite. Returns the buffer plus the byte offsets of
    /// the two textured prims' texture blocks and the untextured prim's
    /// payload start.
    fn synthetic_tmd() -> (Vec<u8>, [usize; 2], usize) {
        let mut buf = Vec::new();
        buf.extend_from_slice(&0x8000_0002u32.to_le_bytes()); // magic
        buf.extend_from_slice(&0u32.to_le_bytes()); // flags = 0 (relative)
        buf.extend_from_slice(&1u32.to_le_bytes()); // nobj
        // Object entry: prim section right after the entry, vertices after
        // the prim section. *_top offsets are relative to the object-table
        // start (byte 12).
        let prim_top = OBJ_ENTRY_BYTES; // abs 0x28
        // prim section: FT3 group (8 + 3*20) + F3 group (8 + 2*20) + term(4)
        let prim_size = 8 + 3 * 20 + 8 + 2 * 20 + 4;
        let vert_top = prim_top + prim_size;
        for w in [
            vert_top as u32,
            3, // n_vert
            0,
            0, // normals
            prim_top as u32,
            3, // n_prim
            0, // scale
        ] {
            buf.extend_from_slice(&w.to_le_bytes());
        }
        // FT3 group: count=2, flags=0x20 (textured triangle, vertex
        // indices at byte 14), ilen=5 (20-byte prims), mode 0x24 (TME).
        let ft3_start = buf.len();
        buf.extend_from_slice(&[0x02, 0x00, 0x20, 0x00, 0x04, 0x05, 0x00, 0x24]);
        // Texture block = bytes 4..14 of each prim:
        // [u0, v0, cba_lo, cba_hi, u1, v1, tsb_lo, tsb_hi, u2, v2].
        let ft3_prim = |cba: u16, tsb: u16, buf: &mut Vec<u8>| {
            buf.extend_from_slice(&[0x80, 0x80, 0x80, 0x24]); // colour word
            buf.extend_from_slice(&[0, 0]);
            buf.extend_from_slice(&cba.to_le_bytes());
            buf.extend_from_slice(&[8, 0]);
            buf.extend_from_slice(&tsb.to_le_bytes());
            buf.extend_from_slice(&[0, 8]);
            for idx in [0u16, 8, 16] {
                buf.extend_from_slice(&idx.to_le_bytes());
            }
        };
        let block_a = buf.len() + 4;
        ft3_prim(0x7804, 0x0035, &mut buf); // row 480 col 64, page 0x15 abr=1
        let block_b = buf.len() + 4;
        ft3_prim(0xF800, 0x0016, &mut buf); // row 480 bit15-set, page 0x16
        buf.resize(buf.len() + 20, 0); // footer slot
        assert_eq!(buf.len() - ft3_start, 8 + 3 * 20);
        // F3 group (untextured): count=1, flags=0x10, ilen=5, mode 0x20
        // (TME clear). Payload bytes mimic a texture block and must stay
        // untouched.
        buf.extend_from_slice(&[0x01, 0x00, 0x10, 0x00, 0x04, 0x05, 0x00, 0x20]);
        let f3_payload = buf.len();
        buf.extend_from_slice(&[0xAA; 20]);
        buf.resize(buf.len() + 20, 0); // footer slot
        buf.extend_from_slice(&0u32.to_le_bytes()); // group terminator
        assert_eq!(buf.len(), 12 + vert_top);
        buf.resize(buf.len() + 3 * 8, 0); // 3 zero vertices
        (buf, [block_a, block_b], f3_payload)
    }

    #[test]
    fn relocate_rewrites_textured_prims_into_the_slot_band() {
        let (mut buf, [block_a, block_b], _) = synthetic_tmd();
        let n = relocate_tsb_cba(&mut buf, 1).expect("relocate slot 1");
        assert_eq!(n, 2, "both textured prims rewritten");
        // Prim A: CLUT row 480 col 64 -> row 482 col 64 (column kept);
        // texpage 0x15 -> slot 1's first page 0x1A, ABR bits kept.
        let cba_a = u16::from_le_bytes([buf[block_a + 2], buf[block_a + 3]]);
        let tsb_a = u16::from_le_bytes([buf[block_a + 6], buf[block_a + 7]]);
        assert_eq!(cba_a, (482 << 6) | 4);
        assert_eq!(tsb_a, 0x0020 | 0x1A);
        // Prim B: bit 15 of the CBA preserved; page 0x16 (non-0x15) ->
        // slot 1's second page 0x1B.
        let cba_b = u16::from_le_bytes([buf[block_b + 2], buf[block_b + 3]]);
        let tsb_b = u16::from_le_bytes([buf[block_b + 6], buf[block_b + 7]]);
        assert_eq!(cba_b, 0x8000 | (482 << 6));
        assert_eq!(tsb_b, 0x001B);
    }

    #[test]
    fn relocate_slot_table_matches_the_documented_band() {
        // docs/formats/character-mesh.md § Battle render: runtime texpages
        // (512,256)+(576,256) / (640,256)+(704,256) / (768,256)+(832,256),
        // CLUT rows 481 / 482 / 483 for slots 0 / 1 / 2.
        for (slot, pages, row) in [
            (0u8, (512u16, 576u16), 481u16),
            (1, (640, 704), 482),
            (2, (768, 832), 483),
        ] {
            let (mut buf, [block_a, block_b], _) = synthetic_tmd();
            relocate_tsb_cba(&mut buf, slot).expect("relocate");
            let tsb_a = u16::from_le_bytes([buf[block_a + 6], buf[block_a + 7]]);
            let tsb_b = u16::from_le_bytes([buf[block_b + 6], buf[block_b + 7]]);
            assert_eq!((tsb_a & 0xF) * 64, pages.0, "slot {slot} first page");
            assert_eq!((tsb_a >> 4) & 1, 1, "slot {slot} page y = 256");
            assert_eq!((tsb_b & 0xF) * 64, pages.1, "slot {slot} second page");
            for block in [block_a, block_b] {
                let cba = u16::from_le_bytes([buf[block + 2], buf[block + 3]]);
                assert_eq!((cba >> 6) & 0x1FF, row, "slot {slot} CLUT row");
            }
        }
    }

    #[test]
    fn relocate_leaves_untextured_groups_alone() {
        let (mut buf, _, f3_payload) = synthetic_tmd();
        relocate_tsb_cba(&mut buf, 0).expect("relocate");
        assert_eq!(
            &buf[f3_payload..f3_payload + 20],
            &[0xAA; 20],
            "untextured prim payload untouched"
        );
    }
}

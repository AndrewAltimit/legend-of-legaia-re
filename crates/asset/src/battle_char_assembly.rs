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
    /// Per-object **animation part index**, post-sort: the index of the
    /// pose channel that drives this object in the character's own battle
    /// animation streams ([`battle_animations`] - the `[parts][frames]
    /// [9-byte TRS]` streams in record[0] of the same player file, whose
    /// `parts` count equals the skeleton bone count).
    ///
    /// Skeleton objects are driven by their own bone channel (`= tag`,
    /// which post-sort is also the object index); equipment extras (tags
    /// `100+`/`200+`, sorted last, past `parts`) ride the channel of the
    /// bone object that precedes them in their section - their attach
    /// piece (for `200+` extras this equals
    /// `attach_bones[tag - 200]`).
    ///
    /// NOTE: the **PROT 1203 ANM bundle is NOT the battle-anim source for
    /// the assembled mesh** - its banks are authored against the
    /// PROT 1204 pack's own object order (which differs from the
    /// assembled tag order per character), so posing the assembled blob
    /// from 1203 mis-sockets the rig. Verified live: a mid-battle capture
    /// has no 1203 record resident, and the party render-node's anim
    /// context points at record[0]'s idle stream (parts = skeleton bone
    /// count) inside the loaded player file.
    pub anm_bones: Vec<u8>,
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
    // Animation part index per object (see
    // [`AssembledCharacter::anm_bones`]): a skeleton object is driven by
    // its own bone channel; a section's surplus (equipment-visual) objects
    // ride the channel of the bone object that precedes them.
    let mut anm_bones: Vec<u8> = Vec::with_capacity(total_nobj);
    let mut last_bone: u8 = 0;
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
            if k < s.bone_ids.len() {
                tags.push(s.bone_ids[k]);
                last_bone = s.bone_ids[k];
                anm_bones.push(last_bone);
            } else {
                tags.push(if k == s.bone_ids.len() { 0xFF } else { 0xFE });
                anm_bones.push(last_bone);
            }
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
            anm_bones.swap(i, min);
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
        anm_bones,
        sections: records,
    })
}

// ---------------------------------------------------------------------------
// Battle animation (record[0] per-action TRS streams)
// ---------------------------------------------------------------------------

/// Number of action slots scanned in record[0]'s head offset table. On disc
/// only `+0x00..+0x2C` (slots 0..0xB) are populated; the runtime table is
/// wider: the battle loader `FUN_80052FA0` rebases the 12 disc words, fills
/// slots `0xC..0xF` (offsets `0x30..0x3C`) with swing records spliced from
/// the equipped-item sections ([`swing_battle_animations`]), and the anim
/// commit `FUN_8004AD80` installs dynamically-materialized art records at
/// slots `0x10`/`0x11` ([`art_animation_bank`]). The word at `+0x58` is the
/// **art-animation record bank** pointer (`[u32 count]` + `0xD0`-stride
/// records the dynamic slots are built from; also the art matcher's table),
/// and `+0x5C` a sibling pointer (rebased at load) - in all four retail
/// files it equals `clut_a_off - 4`, the zero word immediately before
/// record[0]'s first image block (consumer untraced; the "it points at the
/// art ME stream archive" hypothesis is disc-refuted - those archives live
/// in `readef.DAT`, see [`art_me_archive`]). Not texture-block offsets as
/// earlier noted.
pub const ACTION_SLOT_COUNT: usize = 22;

/// Offset of the packed `[u8 parts][u8 frames][9-byte TRS records]` stream
/// inside a record[0] action entry (the monster archive's sibling entries
/// keep theirs at `+0x8C`). The runtime loader points the entry's `+0x88`
/// stream pointer here (`FUN_80047430` / `FUN_8004AD80` consume it). The
/// entry's first byte is its **action tag** (identity with the slot index in
/// the player files: `0` idle, `1` walk/approach, `2`/`3` light flinches,
/// `4` knockdown, `5` get-up, `7`/`8`/`9` ready/recover/defeat poses, `0x0B`
/// block) - the key space of the actor `+0x1EF..+0x1F3` reaction map.
pub const PLAYER_ANIM_STREAM_OFFSET: usize = 0xAC;

/// LZS-decode a player file's `record[0]` (header
/// `[desc_off][clut_a][clut_b][budget]`, LZS stream at `+0x10`). Scans
/// 4-byte-aligned offsets for a plausible header (skipping any `"pochi"`
/// filler prefix on the historical over-read copies) and accepts the first
/// whose stream decompresses to its declared budget. Unlike
/// [`crate::battle_char_palette::find_record0`] this does **not** require
/// the fixed-stride palette-chain assembly to succeed (it overflows for
/// Noa / Gala - see [`crate::battle_char_palette::collect_palette`]).
pub fn decode_record0(file: &[u8]) -> Result<Vec<u8>> {
    let mut o = 0;
    while o + 0x10 <= file.len() {
        let desc_off = read_u32(file, o)? as usize;
        let clut_a = read_u32(file, o + 4)? as usize;
        let clut_b = read_u32(file, o + 8)? as usize;
        let budget = read_u32(file, o + 0xC)? as usize;
        let plausible = (0x100..file.len() - o).contains(&desc_off)
            && (0x1000..=0x4_0000).contains(&budget)
            && (0x10..budget).contains(&clut_a)
            && (0x10..budget).contains(&clut_b);
        if plausible && let Ok(decoded) = legaia_lzs::decompress(&file[o + 0x10..], budget) {
            return Ok(decoded);
        }
        o += 4;
    }
    bail!("no record[0] header found")
}

/// Decode the character's **battle action animations** from `record[0]` of
/// their player file: per populated action slot, the packed
/// `[u8 parts][u8 frames][9-byte TRS records]` stream at entry
/// `+`[`PLAYER_ANIM_STREAM_OFFSET`] - the same rigid-transform keyframe
/// format as the monster archive's per-action streams
/// (`docs/formats/monster-animation.md`), with `parts` = the character's
/// **skeleton bone count** (equipment extras carry no channel of their own
/// and ride their attach bone - see [`AssembledCharacter::anm_bones`]).
/// Slot 0 is the neutral idle loop; its frame 0 is the combat-stance rest
/// pose that sockets the assembled mesh.
///
/// `action_id` on the returned animations is the slot index.
// PORT: FUN_80047430 (anim-context consumer) - the battle party render
// node's +0x4C anim context is a record[0] action entry; its +0x88 stream
// pointer (loader-reconstructed, entry+0xAC) feeds the FUN_8004AD80 /
// FUN_8004998C keyframe decode chain shared with battle monsters.
pub fn battle_animations(file: &[u8]) -> Result<Vec<crate::monster_archive::MonsterAnimation>> {
    let block = decode_record0(file)?;
    let mut out = Vec::new();
    for slot in 0..ACTION_SLOT_COUNT {
        let Ok(entry_off) = read_u32(&block, slot * 4) else {
            break;
        };
        let entry_off = entry_off as usize;
        if entry_off == 0 || entry_off >= block.len() {
            continue;
        }
        let rate = block
            .get(entry_off + crate::monster_archive::ANIM_RATE_OFFSET)
            .copied()
            .unwrap_or(0);
        if let Some(anim) = crate::monster_archive::parse_animation_stream(
            &block,
            slot as u8,
            rate,
            entry_off + PLAYER_ANIM_STREAM_OFFSET,
        ) {
            out.push(anim);
        }
    }
    Ok(out)
}

/// Decode just the **idle** animation (action slot 0) of a player file -
/// the loop the battle engine plays while the character awaits commands.
/// Frame 0 is the rest pose that sockets the assembled battle mesh.
/// `Ok(None)` when slot 0 is absent or its stream doesn't decode.
pub fn idle_battle_animation(
    file: &[u8],
) -> Result<Option<crate::monster_archive::MonsterAnimation>> {
    let block = decode_record0(file)?;
    let entry_off = read_u32(&block, 0)? as usize;
    if entry_off == 0 || entry_off >= block.len() {
        return Ok(None);
    }
    let rate = block
        .get(entry_off + crate::monster_archive::ANIM_RATE_OFFSET)
        .copied()
        .unwrap_or(0);
    Ok(crate::monster_archive::parse_animation_stream(
        &block,
        0,
        rate,
        entry_off + PLAYER_ANIM_STREAM_OFFSET,
    ))
}

// ---------------------------------------------------------------------------
// Weapon-swing animations (equipment-section records -> runtime slots 0xC..0xF)
// ---------------------------------------------------------------------------

/// First runtime action slot filled from the equipment sections: the four
/// direction-command swings live at slots `0xC` (L) / `0xD` (R) / `0xE` (D) /
/// `0xF` (U) - the same byte values the Tactical-Arts command queue stages
/// as anim ids.
pub const SWING_SLOT_BASE: u8 = 0xC;

/// One weapon-swing animation spliced from an equipment section's payload
/// into the runtime action table (see [`swing_battle_animations`]).
#[derive(Debug, Clone)]
pub struct SwingAnimation {
    /// Runtime action-table slot (`0xC..=0xF`).
    pub slot: u8,
    /// Equipment section the record came from (`2..=4`; sections 0/1 carry
    /// no swing records - their `+0x04`/`+0x08` words are zero on disc).
    pub section: usize,
    /// Descriptor id of the section slot (equippable item id; `0` =
    /// section default).
    pub item_id: u32,
    /// The record's first byte - a presentation-class tag in the same id
    /// space as the art entries' (`0x0E..0x1F` observed), **not** the slot.
    pub entry_tag: u8,
    /// The decoded keyframe animation. `action_id` is the runtime slot.
    pub anim: crate::monster_archive::MonsterAnimation,
    /// The entry's facial keyframe tracks (`+0x8C` eyes / `+0x98` mouth),
    /// consumed by the per-frame facial animator while the swing plays
    /// (see [`crate::face_anim`]). `None` only for a truncated header.
    pub face: Option<crate::face_anim::FaceTracks>,
}

/// Parse the standard `0xAC`-byte action entry at `off` in `block`: action
/// tag at `+0x00`, rate byte at `+0x78`
/// ([`crate::monster_archive::ANIM_RATE_OFFSET`]), packed keyframe stream at
/// `+0xAC` ([`PLAYER_ANIM_STREAM_OFFSET`]).
// PORT: FUN_800557b8 - the record copy that pins this shape: 0x2B words
// (= 0xAC bytes) of header, then `(parts * frames * 9 + 5) >> 2` words of
// the packed stream read from the bytes at +0xAC.
fn parse_action_entry(
    block: &[u8],
    off: usize,
    action_id: u8,
) -> Option<crate::monster_archive::MonsterAnimation> {
    let rate = block
        .get(off + crate::monster_archive::ANIM_RATE_OFFSET)
        .copied()?;
    crate::monster_archive::parse_animation_stream(
        block,
        action_id,
        rate,
        off + PLAYER_ANIM_STREAM_OFFSET,
    )
}

/// Decode the **weapon-swing animations** the battle loader splices into the
/// runtime action table from the equipped-item sections: per selected
/// section 2/3/4, the decoded payload's `+0x04` word is a self-relative
/// offset to a standard action-entry record (header + keyframe stream at
/// `+0xAC`), installed at slot `0xC + (section - 2)`; section 4's `+0x08`
/// word carries a **second** record, installed at slot `0xF`. Sections 0/1
/// contribute none (their words are zero on disc).
///
/// `equipped` is the char record's `+0x196..+0x19A` bytes, as for
/// [`assemble_character`]; the returned animations' `action_id` is the
/// runtime slot (`0xC..=0xF`).
// PORT: FUN_80052FA0 (swing-splice half) - the `if (1 < iVar3)` section
// loop: copies the section-base + `+0x04` record via FUN_800557b8 into the
// action-table word at 0x28 + section*4 (= slot 0xC..0xE for sections
// 2..4), and section 4's `+0x08` record into word 0x3C (slot 0xF), pointing
// each installed entry's +0x88 stream pointer at entry+0xAC.
pub fn swing_battle_animations(
    buf: &[u8],
    pack: &BattleDataPack,
    equipped: &[u8; SECTION_COUNT],
) -> Result<Vec<SwingAnimation>> {
    let records = select_sections(pack, equipped)?;
    let mut out = Vec::with_capacity(4);
    for (section, rec) in records.iter().enumerate().take(SECTION_COUNT).skip(2) {
        let entry = decode_record(buf, pack, rec.index)
            .with_context(|| format!("decode section {section} (id {:#x})", rec.id))?;
        let d = &entry.bytes;
        let mut offsets = vec![(SWING_SLOT_BASE + (section as u8 - 2), read_u32(d, 4)?)];
        if section == 4 {
            offsets.push((0xF, read_u32(d, 8)?));
        }
        for (slot, off) in offsets {
            let off = off as usize;
            if off == 0 || off >= d.len() {
                bail!("section {section} swing record offset {off:#x} out of range");
            }
            let entry_tag = d[off];
            let anim = parse_action_entry(d, off, slot).ok_or_else(|| {
                anyhow::anyhow!("section {section} swing record at {off:#x} has no valid stream")
            })?;
            out.push(SwingAnimation {
                slot,
                section,
                item_id: rec.id,
                entry_tag,
                anim,
                face: crate::face_anim::FaceTracks::from_entry(d, off),
            });
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Art-animation bank (record[0] +0x58 -> dynamic slots 0x10/0x11)
// ---------------------------------------------------------------------------

/// Stride of one art-animation bank record.
pub const ART_RECORD_STRIDE: usize = 0xD0;

/// Offset of the embedded `0xAC`-byte action-entry header inside a bank
/// record (`0x24 + 0xAC` = the full `0xD0` stride).
pub const ART_ENTRY_OFFSET: usize = 0x24;

/// First staged anim id that resolves through the bank: id `q >= 0x10`
/// selects bank record `q - 0x10` (`FUN_8004AD80`).
pub const ART_ANIM_ID_BASE: u8 = 0x10;

/// One record of the per-character **art-animation bank** (record[0] image
/// word `+0x58`): `[u32 count]` then `count` `0xD0`-stride records, each a
/// `0x24`-byte arts-matcher head + a standard `0xAC`-byte action entry.
///
/// The anim commit `FUN_8004AD80` materializes a staged anim id
/// `q >= 0x10` from record `q - 0x10`: ids `0x10` and `0x1A` install at
/// runtime slot `0x11`, every other id at slot `0x10`; ids `> 0x1A`
/// additionally drive the HUD art-name display from [`Self::name`] and
/// `FUN_8004C650(char, id - 0x1B)`. The record's keyframe stream is **not
/// inline**: `FUN_8002B28C(_DAT_8007BD74, scratch, stream_source)` pulls it
/// from the `"ME"` archive resident in the side-band streaming buffer - see
/// [`art_me_archive`] / [`crate::me_archive`].
#[derive(Debug, Clone)]
pub struct ArtAnimRecord {
    /// Bank record index.
    pub index: usize,
    /// The staged anim id this record materializes (`0x10 + index`).
    pub anim_id: u8,
    /// Arts-matcher direction-command sequence (record `+0x00`, values
    /// `1..=4`, zero-terminated; empty for the un-named base records). The
    /// same combo bytes the arts matcher reads (the canonical
    /// `record +0` combo of the arts-randomizer corpus).
    pub combo: Vec<u8>,
    /// Index into the character's art `"ME"` stream archive (record
    /// `+0x0A`) - the `FUN_8002B28C` third argument.
    pub stream_source: u8,
    /// Inline art-name string (record `+0x10`, NUL-terminated ASCII, up to
    /// 20 bytes; empty on the base / un-named records).
    pub name: String,
    /// Action tag byte (entry `+0x00`; presentation-class id space
    /// `0x16..0x1F` on named arts, `0` on base records).
    pub entry_tag: u8,
    /// Attach key (entry `+0x77` = record `+0x9B`): the id the equipment
    /// sections' attach-object records match against (`FUN_80052FA0`'s
    /// bank scan compares attach-record `+0x07` with it).
    pub attach_key: u8,
    /// Playback-rate byte (entry `+0x78`, the `FUN_80047430` cursor
    /// multiplier; `1..=7` observed).
    pub rate: u8,
    /// Entry `+0x84` (a secondary anim-rate field - `FUN_8004AD80` copies
    /// it into actor `+0x21B`). `0xFF` marks the eight **base-archive**
    /// records present in every character's bank - see
    /// [`Self::uses_base_archive`].
    pub rate_alt: u8,
    /// The embedded entry's facial keyframe tracks (entry `+0x8C` eyes /
    /// `+0x98` mouth = record `+0xB0` / `+0xBC`). `FUN_8004AD80` installs
    /// the embedded entry (record `+0x24`) as the action-table slot
    /// `0x10`/`0x11` pointer, so while the materialized art clip plays the
    /// render node's `+0x4C` anim context is this entry and the per-frame
    /// facial animator `FUN_8004C7B4` reads these tracks - the mid-battle
    /// art-strike faces. Sibling of
    /// [`SwingAnimation::face`]; `None` only for a truncated record.
    pub face: Option<crate::face_anim::FaceTracks>,
    /// record[0]-image byte offset of the record's action-entry header.
    pub entry_offset: usize,
}

impl ArtAnimRecord {
    /// Whether this record's [`Self::stream_source`] indexes the
    /// character's **base** art archive (readef slot `3*char + 2`) instead
    /// of the main one (slot `3*char + 1`).
    ///
    /// Disc-pinned mapping (the art-path caller of the side-band request
    /// arm `FUN_80055B4C`, which stages the readef slot via the
    /// `ctx+0x26B` byte, is not in the dumped corpus): in all four
    /// retail files the records with
    /// `rate_alt == 0xFF` are exactly eight per character with
    /// `stream_source` `0..=7` = the base archive's exact entry range,
    /// while the remaining records' max `stream_source` equals the main
    /// archive's `count - 1` exactly (17/18/19/1 entries for
    /// Vahn/Noa/Gala/Terra).
    pub fn uses_base_archive(&self) -> bool {
        self.rate_alt == 0xFF
    }
}

/// Parse the art-animation bank out of a decoded record[0] image
/// ([`decode_record0`]): the self-relative word at `+0x58` locates
/// `[u32 count][count x 0xD0-stride records]`.
// PORT: FUN_8004AD80 (bank-record select) - dynamic anim id q >= 0x10 reads
// record q - 0x10 (entry pointer = bank + 4 + (q - 0x10)*0xD0 + 0x24, the
// `q*0xD0 + bank + 4 - 0xCDC` install arithmetic), name at -0xCF0
// (record +0x10), stream-source byte at -0xCF6 (record +0x0A).
// REF: FUN_80052FA0 - rebases the +0x58 word and scans the bank's
// `+0x9B` attach keys for the equipment attach-object records.
pub fn art_animation_bank(record0: &[u8]) -> Result<Vec<ArtAnimRecord>> {
    let bank_off = read_u32(record0, 0x58)? as usize;
    let count = read_u32(record0, bank_off).context("art bank count word")? as usize;
    if count == 0 || count > 0x40 {
        bail!("implausible art bank count {count}");
    }
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let base = bank_off + 4 + index * ART_RECORD_STRIDE;
        let rec = record0
            .get(base..base + ART_RECORD_STRIDE)
            .ok_or_else(|| anyhow::anyhow!("art bank record {index} past record[0] end"))?;
        let combo: Vec<u8> = rec[..0x0A]
            .iter()
            .copied()
            .take_while(|&b| b != 0)
            .collect();
        if combo.iter().any(|&b| !(1..=4).contains(&b)) {
            bail!("art bank record {index} combo byte outside 1..=4");
        }
        let name_raw = &rec[0x10..ART_ENTRY_OFFSET];
        let name_end = name_raw
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(name_raw.len());
        let name_bytes = &name_raw[..name_end];
        if !name_bytes.iter().all(|&b| (0x20..0x7F).contains(&b)) {
            bail!("art bank record {index} name is not printable ASCII");
        }
        let entry = &rec[ART_ENTRY_OFFSET..];
        out.push(ArtAnimRecord {
            index,
            anim_id: ART_ANIM_ID_BASE + index as u8,
            combo,
            stream_source: rec[0x0A],
            name: String::from_utf8_lossy(name_bytes).into_owned(),
            entry_tag: entry[0],
            attach_key: entry[0x77],
            rate: entry[0x78],
            rate_alt: entry[0x84],
            face: crate::face_anim::FaceTracks::from_entry(rec, ART_ENTRY_OFFSET),
            entry_offset: base + ART_ENTRY_OFFSET,
        });
    }
    Ok(out)
}

/// `readef.DAT` slot index of a character's art `"ME"` stream archive:
/// slot `3*char_index + 1` (main archive - the named arts) or
/// `3*char_index + 2` (base archive - the eight `rate_alt == 0xFF`
/// records). `char_index` is 0..=3 for Vahn/Noa/Gala/Terra. Disc-pinned
/// (see [`ArtAnimRecord::uses_base_archive`]); slot `3*char_index` is the
/// character's non-ME texture slot.
pub fn art_me_slot(char_index: usize, base: bool) -> usize {
    char_index * 3 + if base { 2 } else { 1 }
}

/// Slice + parse a character's art `"ME"` archive out of the raw
/// `readef.DAT` bytes (extraction PROT entry 894 - the
/// `crate::summon_readef` side-band file whose `0x10800`-byte slots the
/// battle streamer `FUN_801F17F8` reads into `_DAT_8007BD74`).
pub fn art_me_archive(
    readef: &[u8],
    char_index: usize,
    base: bool,
) -> Result<crate::me_archive::MeArchive<'_>> {
    use crate::summon_readef::SLOT_BYTES;
    let slot = art_me_slot(char_index, base);
    let bytes = readef
        .get(slot * SLOT_BYTES..(slot + 1) * SLOT_BYTES)
        .ok_or_else(|| anyhow::anyhow!("readef slot {slot} past file end"))?;
    crate::me_archive::parse(bytes).with_context(|| format!("art ME archive in readef slot {slot}"))
}

/// Resolve + decode one art record's keyframe animation through its `"ME"`
/// archive (the caller picks the archive per
/// [`ArtAnimRecord::uses_base_archive`]). The returned animation's
/// `action_id` is the record's staged anim id (`0x10 + index`) and `rate`
/// the record's entry `+0x78` byte.
pub fn art_animation(
    record: &ArtAnimRecord,
    archive: &crate::me_archive::MeArchive<'_>,
) -> Result<crate::monster_archive::MonsterAnimation> {
    let stream = archive
        .entry(record.stream_source as usize)
        .with_context(|| format!("art record {} stream", record.index))?;
    crate::monster_archive::parse_animation_stream(&stream, record.anim_id, record.rate, 0)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "art record {} stream (source {}) is not a valid keyframe stream",
                record.index,
                record.stream_source
            )
        })
}

/// Re-index an animation's pose channels **per assembled object**: output
/// part `i` is input part `anm_bones[i]` (each equipment extra duplicates
/// its attach bone's channel), so `frames[f][obj]` can drive TMD object
/// `obj` directly through the engine's posed-mesh builders. Channels
/// referencing a part the stream doesn't carry come out as the identity
/// pose.
pub fn expand_animation_for_objects(
    anim: &crate::monster_archive::MonsterAnimation,
    anm_bones: &[u8],
) -> crate::monster_archive::MonsterAnimation {
    let frames: Vec<Vec<crate::monster_archive::PartPose>> = anim
        .frames
        .iter()
        .map(|frame| {
            anm_bones
                .iter()
                .map(|&b| frame.get(b as usize).copied().unwrap_or_default())
                .collect()
        })
        .collect();
    crate::monster_archive::MonsterAnimation {
        action_id: anim.action_id,
        rate: anim.rate,
        part_count: anm_bones.len(),
        frame_count: anim.frame_count,
        frames,
    }
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
/// `DAT_8007C018[slot]`; the on-disc (authoring) TSB/CBA - texpages
/// `0x15`/`0x16` = `(320, 256)`/`(384, 256)`, CLUT row 480 - are never
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

/// VRAM x base (in halfwords / 16bpp pixels) of party slot 0's battle
/// texture band; slot `s` starts at `BAND_X_BASE + s * BAND_X_STRIDE`.
pub const BAND_X_BASE: u16 = 0x200;

/// Per-party-slot x stride of the battle texture band (= two 64-halfword
/// texpages, the pages `relocate_tsb_cba` retargets).
pub const BAND_X_STRIDE: u16 = 0x80;

/// VRAM y base every battle-character image rect offsets from.
pub const BAND_Y_BASE: u16 = 0x100;

/// One image rect of the battle-texture placement, in the pre-band frame
/// the retail tables author: the upload lands at
/// `(BAND_X_BASE + party_slot * BAND_X_STRIDE + x0, BAND_Y_BASE + y0)`.
/// `w` is in VRAM halfwords (32 halfwords = 128 px at 4bpp).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureRect {
    /// Band-relative x (halfwords).
    pub x0: u16,
    /// Band-relative y (rows).
    pub y0: u16,
    /// Width in halfwords.
    pub w: u16,
    /// Height in rows.
    pub h: u16,
}

impl TextureRect {
    /// Absolute VRAM x of the upload for party slot `party_slot`.
    pub fn fb_x(&self, party_slot: u8) -> u16 {
        self.x0 + BAND_X_BASE + party_slot as u16 * BAND_X_STRIDE
    }

    /// Absolute VRAM y of the upload.
    pub fn fb_y(&self) -> u16 {
        self.y0 + BAND_Y_BASE
    }

    /// Byte size of the rect's pixel payload (`w * h` halfwords).
    pub fn pixel_bytes(&self) -> usize {
        self.w as usize * self.h as usize * 2
    }
}

/// Per-section texture-pool placement rects - mirror of the static
/// `SCUS_942.54` table at `0x800775B8` (4 u16 per equip section, read by
/// `FUN_80052FA0`'s per-section decode loop and handed to `FUN_80053B9C`).
/// Together with [`RECORD0_TEXTURE_RECTS`] the seven rects tile each party
/// slot's 128-halfword x 256-row band exactly.
pub const SECTION_TEXTURE_RECTS: [TextureRect; SECTION_COUNT] = [
    TextureRect {
        x0: 0x00,
        y0: 0x80,
        w: 0x20,
        h: 0x80,
    },
    TextureRect {
        x0: 0x00,
        y0: 0x00,
        w: 0x40,
        h: 0x80,
    },
    TextureRect {
        x0: 0x40,
        y0: 0x00,
        w: 0x20,
        h: 0x80,
    },
    TextureRect {
        x0: 0x40,
        y0: 0x80,
        w: 0x20,
        h: 0x80,
    },
    TextureRect {
        x0: 0x60,
        y0: 0x80,
        w: 0x20,
        h: 0x80,
    },
];

/// Placement rects of the two `record[0]` image blocks (the blocks at the
/// file header's `clut_a_off` / `clut_b_off` within record[0]'s decoded
/// output). Inline constants of `FUN_80052FA0` (`0x800020` / `0x60` +
/// `0x800020` packed `(x0,y0)` / `(w,h)` pairs).
pub const RECORD0_TEXTURE_RECTS: [TextureRect; 2] = [
    TextureRect {
        x0: 0x20,
        y0: 0x80,
        w: 0x20,
        h: 0x80,
    },
    TextureRect {
        x0: 0x60,
        y0: 0x00,
        w: 0x20,
        h: 0x80,
    },
];

/// One decoded battle-texture upload block in the `FUN_80053B9C` frame:
/// `[u16 clut_x][u16 clut_n][clut_n x u16 BGR555][w*h halfwords pixels]`.
/// The CLUT half LoadImages to `(clut_x, 0x1E1 + party_slot, clut_n, 1)`
/// with the STP bit forced on every non-zero entry; the pixel half
/// LoadImages to the rect's banded `(fb_x, fb_y, w, h)`.
#[derive(Debug, Clone)]
pub struct TextureUpload {
    /// Placement rect (pre-band frame; see [`TextureRect`]).
    pub rect: TextureRect,
    /// Party slot the block was decoded for (0..=2).
    pub party_slot: u8,
    /// VRAM x (halfwords) of the CLUT run on row `0x1E1 + party_slot`.
    pub clut_x: u16,
    /// CLUT entries with the retail STP pass applied (`e |= 0x8000` on
    /// every non-zero entry). Empty in both `record[0]` blocks.
    pub clut: Vec<u16>,
    /// Pixel payload (`rect.w * rect.h` halfwords, row-major).
    pub pixels: Vec<u8>,
}

impl TextureUpload {
    /// Absolute VRAM x of the pixel upload.
    pub fn fb_x(&self) -> u16 {
        self.rect.fb_x(self.party_slot)
    }

    /// Absolute VRAM y of the pixel upload.
    pub fn fb_y(&self) -> u16 {
        self.rect.fb_y()
    }

    /// VRAM row of the CLUT run (`0x1E1 + party_slot` - the same rows the
    /// `relocate_tsb_cba` CBA rewrite targets).
    pub fn clut_row(&self) -> u16 {
        RUNTIME_CLUT_ROW_BASE + self.party_slot as u16
    }

    /// CLUT entries as little-endian bytes (ready for a VRAM row write).
    pub fn clut_bytes(&self) -> Vec<u8> {
        self.clut.iter().flat_map(|w| w.to_le_bytes()).collect()
    }
}

/// Decode one battle-texture upload block (the `FUN_80053B9C` source
/// frame) out of `block`, placing it at `rect` for `party_slot`.
// PORT: FUN_80053b9c - the battle-texture upload helper: reads the
// [clut_x, clut_n, entries] prefix, ORs STP onto non-zero entries,
// LoadImages the CLUT run to (clut_x, 0x1E1+slot, clut_n, 1) and the
// pixels to (x0 + 0x200 + slot*0x80, y0 + 0x100, w, h). (The shadow CLUT
// copy into the battle context at +0x894 is engine-context, not VRAM.)
// REF: FUN_800583c8 - the LoadImage wrapper it issues both rects through.
pub fn parse_upload_block(
    block: &[u8],
    rect: TextureRect,
    party_slot: u8,
) -> Result<TextureUpload> {
    if party_slot > 2 {
        bail!("party slot {party_slot} out of the 0..=2 battle band");
    }
    let clut_x = u16::from_le_bytes(
        block
            .get(0..2)
            .ok_or_else(|| anyhow::anyhow!("upload block shorter than its clut_x word"))?
            .try_into()
            .unwrap(),
    );
    let clut_n = u16::from_le_bytes(
        block
            .get(2..4)
            .ok_or_else(|| anyhow::anyhow!("upload block shorter than its clut_n word"))?
            .try_into()
            .unwrap(),
    ) as usize;
    if clut_n > 0x400 {
        bail!("implausible CLUT run length {clut_n}");
    }
    let clut_bytes = block
        .get(4..4 + clut_n * 2)
        .ok_or_else(|| anyhow::anyhow!("CLUT run past block end"))?;
    let clut = clut_bytes
        .chunks_exact(2)
        .map(|c| {
            let e = u16::from_le_bytes([c[0], c[1]]);
            if e != 0 { e | 0x8000 } else { e }
        })
        .collect();
    let pix_off = 4 + clut_n * 2;
    let pixels = block
        .get(pix_off..pix_off + rect.pixel_bytes())
        .ok_or_else(|| anyhow::anyhow!("pixel payload past block end"))?
        .to_vec();
    Ok(TextureUpload {
        rect,
        party_slot,
        clut_x,
        clut,
        pixels,
    })
}

/// The two `record[0]` texture uploads of a player file: LZS-decode
/// `record[0]` (header `budget` at `+0x0C`, stream at `+0x10`) and frame
/// the blocks at the header's `clut_a_off` / `clut_b_off` with the
/// [`RECORD0_TEXTURE_RECTS`] placement.
// PORT: FUN_80052FA0 (record[0] texture half) - the two FUN_80053b9c
// calls right after the record[0] decode, before the per-section loop.
pub fn record0_texture_uploads(file: &[u8], party_slot: u8) -> Result<Vec<TextureUpload>> {
    let clut_a = read_u32(file, 4)? as usize;
    let clut_b = read_u32(file, 8)? as usize;
    let budget = read_u32(file, 0xC)? as usize;
    if budget == 0 || budget > 0x40_0000 || clut_a >= clut_b || clut_b >= budget {
        bail!(
            "implausible player-file header (clut_a {clut_a:#x} clut_b {clut_b:#x} budget {budget:#x})"
        );
    }
    let stream = file
        .get(0x10..)
        .ok_or_else(|| anyhow::anyhow!("file shorter than its record[0] stream"))?;
    let decoded = legaia_lzs::decompress(stream, budget)?;
    let mut out = Vec::with_capacity(2);
    for (off, rect) in [
        (clut_a, RECORD0_TEXTURE_RECTS[0]),
        (clut_b, RECORD0_TEXTURE_RECTS[1]),
    ] {
        let block = decoded
            .get(off..)
            .ok_or_else(|| anyhow::anyhow!("record[0] block at {off:#x} past decoded end"))?;
        out.push(
            parse_upload_block(block, rect, party_slot)
                .with_context(|| format!("record[0] block at {off:#x}"))?,
        );
    }
    Ok(out)
}

/// The texture upload of one decoded equipment section, when the section
/// is flagged for upload (`u16` at `+0x12` non-zero): the block at
/// `decoded + tmd_body_end` placed at [`SECTION_TEXTURE_RECTS`]`[section]`.
/// `Ok(None)` for unflagged sections (their pool bytes are dead - retail
/// overwrites them with the next section's decode without uploading).
// PORT: FUN_80052FA0 (per-section texture half) - the `lh 0x12(s2)` gate
// + the FUN_80053b9c call at decoded+tmd_body_end with the DAT_800775b8
// per-section rect.
pub fn section_texture_upload(
    decoded: &[u8],
    section: usize,
    party_slot: u8,
) -> Result<Option<TextureUpload>> {
    if section >= SECTION_COUNT {
        bail!("section index {section} out of the 5-slot table");
    }
    let flag = u16::from_le_bytes(
        decoded
            .get(0x12..0x14)
            .ok_or_else(|| anyhow::anyhow!("decoded section shorter than its header"))?
            .try_into()
            .unwrap(),
    );
    if flag == 0 {
        return Ok(None);
    }
    let pool = read_u32(decoded, 0xC)? as usize;
    let block = decoded
        .get(pool..)
        .ok_or_else(|| anyhow::anyhow!("texture pool at {pool:#x} past decoded end"))?;
    Ok(Some(
        parse_upload_block(block, SECTION_TEXTURE_RECTS[section], party_slot)
            .with_context(|| format!("section {section} pool at {pool:#x}"))?,
    ))
}

/// Every battle-texture upload of one character, in retail order: the two
/// `record[0]` blocks, then the five equipped sections' flagged pools.
/// `equipped` is the char record's `+0x196..+0x19A` bytes; `party_slot`
/// is the character's 0-based ordinal among the *present* battle party
/// (the band selector - not the character id).
pub fn character_texture_uploads(
    file: &[u8],
    pack: &BattleDataPack,
    equipped: &[u8; SECTION_COUNT],
    party_slot: u8,
) -> Result<Vec<TextureUpload>> {
    let mut out = record0_texture_uploads(file, party_slot)?;
    let records = select_sections(pack, equipped)?;
    for (i, rec) in records.iter().enumerate() {
        let entry = decode_record(file, pack, rec.index)
            .with_context(|| format!("decode section {i} (id {:#x})", rec.id))?;
        if let Some(upload) = section_texture_upload(&entry.bytes, i, party_slot)
            .with_context(|| format!("section {i} (id {:#x})", rec.id))?
        {
            out.push(upload);
        }
    }
    Ok(out)
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
    fn art_bank_parses_synthetic_records() {
        // record0 image with the bank pointer at +0x58 -> 0x100:
        // [u32 count = 2] + two 0xD0-stride records.
        let mut img = vec![0u8; 0x100 + 4 + 2 * ART_RECORD_STRIDE];
        img[0x58..0x5C].copy_from_slice(&0x100u32.to_le_bytes());
        img[0x100..0x104].copy_from_slice(&2u32.to_le_bytes());
        let base0 = 0x104;
        // Record 0: combo [1,2,4], source 3, named, entry fields.
        img[base0..base0 + 3].copy_from_slice(&[1, 2, 4]);
        img[base0 + 0x0A] = 3;
        img[base0 + 0x10..base0 + 0x18].copy_from_slice(b"Test Art");
        img[base0 + ART_ENTRY_OFFSET] = 0x18; // entry tag
        img[base0 + ART_ENTRY_OFFSET + 0x77] = 21; // attach key
        img[base0 + ART_ENTRY_OFFSET + 0x78] = 2; // rate
        img[base0 + ART_ENTRY_OFFSET + 0x84] = 0; // rate_alt (main archive)
        // Embedded-entry face tracks: eye record 0 at record +0xB0
        // (= entry +0x8C), mouth record 1 at record +0xBC + 3.
        img[base0 + 0xB0..base0 + 0xB3].copy_from_slice(&[2, 4, 9]);
        img[base0 + 0xBC + 3..base0 + 0xBC + 6].copy_from_slice(&[1, 10, 14]);
        // Record 1: empty combo/name, source 5, base-archive marker.
        let base1 = base0 + ART_RECORD_STRIDE;
        img[base1 + 0x0A] = 5;
        img[base1 + ART_ENTRY_OFFSET + 0x78] = 1;
        img[base1 + ART_ENTRY_OFFSET + 0x84] = 0xFF;

        let bank = art_animation_bank(&img).expect("bank parses");
        assert_eq!(bank.len(), 2);
        assert_eq!(bank[0].anim_id, 0x10);
        assert_eq!(bank[0].combo, vec![1, 2, 4]);
        assert_eq!(bank[0].stream_source, 3);
        assert_eq!(bank[0].name, "Test Art");
        assert_eq!(bank[0].entry_tag, 0x18);
        assert_eq!(bank[0].attach_key, 21);
        assert_eq!(bank[0].rate, 2);
        assert!(!bank[0].uses_base_archive());
        assert_eq!(bank[0].entry_offset, base0 + ART_ENTRY_OFFSET);
        // The embedded entry's face tracks (record +0xB0 / +0xBC).
        let face = bank[0].face.expect("record 0 face tracks");
        assert_eq!(
            face.eyes[0],
            crate::face_anim::FaceTrackRecord {
                frame: 2,
                start: 4,
                end: 9,
            }
        );
        assert_eq!(
            face.mouth[1],
            crate::face_anim::FaceTrackRecord {
                frame: 1,
                start: 10,
                end: 14,
            }
        );
        assert!(!face.is_empty());
        assert_eq!(bank[1].anim_id, 0x11);
        assert!(bank[1].combo.is_empty());
        assert!(bank[1].name.is_empty());
        assert!(bank[1].uses_base_archive());
        assert!(
            bank[1].face.expect("record 1 face tracks").is_empty(),
            "untouched record parses as empty tracks"
        );

        // A combo byte outside 1..=4 is rejected.
        img[base0] = 9;
        assert!(art_animation_bank(&img).is_err());
        img[base0] = 1;
        // A non-printable name byte is rejected.
        img[base0 + 0x10] = 0x01;
        assert!(art_animation_bank(&img).is_err());
    }

    #[test]
    fn art_me_slot_mapping() {
        // Vahn/Noa/Gala/Terra -> readef slots (3c+1, 3c+2).
        assert_eq!(art_me_slot(0, false), 1);
        assert_eq!(art_me_slot(0, true), 2);
        assert_eq!(art_me_slot(1, false), 4);
        assert_eq!(art_me_slot(2, true), 8);
        assert_eq!(art_me_slot(3, false), 10);
        assert_eq!(art_me_slot(3, true), 11);
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
    fn upload_block_applies_stp_and_band_math() {
        // Block: clut_x=8, clut_n=2, entries [0x1D40, 0x0000], 2x1 pixels.
        let rect = TextureRect {
            x0: 0x40,
            y0: 0x80,
            w: 2,
            h: 1,
        };
        let mut block = Vec::new();
        block.extend_from_slice(&8u16.to_le_bytes());
        block.extend_from_slice(&2u16.to_le_bytes());
        block.extend_from_slice(&0x1D40u16.to_le_bytes());
        block.extend_from_slice(&0u16.to_le_bytes());
        block.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]);
        let u = parse_upload_block(&block, rect, 2).expect("parse");
        // STP forced on non-zero entries only.
        assert_eq!(u.clut, vec![0x9D40, 0x0000]);
        assert_eq!(u.clut_x, 8);
        assert_eq!(u.clut_row(), 0x1E3); // row 483 for party slot 2
        assert_eq!(u.pixels, vec![0xAA, 0xBB, 0xCC, 0xDD]);
        // Band math: x = x0 + 0x200 + slot*0x80, y = y0 + 0x100.
        assert_eq!(u.fb_x(), 0x40 + 0x200 + 2 * 0x80);
        assert_eq!(u.fb_y(), 0x80 + 0x100);
    }

    #[test]
    fn upload_block_rejects_short_payload() {
        let rect = TextureRect {
            x0: 0,
            y0: 0,
            w: 0x20,
            h: 0x80,
        };
        // clut_n = 0 but only 8 pixel bytes follow - far short of w*h*2.
        let mut block = vec![0u8; 4];
        block.extend_from_slice(&[0u8; 8]);
        assert!(parse_upload_block(&block, rect, 0).is_err());
    }

    #[test]
    fn section_upload_gates_on_the_flag_halfword() {
        // Decoded slot: header with tmd_body_end = 0x20, flag clear.
        let mut decoded = vec![0u8; 0x20];
        decoded[0xC..0x10].copy_from_slice(&0x20u32.to_le_bytes());
        // Pool block: clut_n = 0 + 0x20*0x80 halfwords of pixels.
        decoded.extend_from_slice(&[0u8; 4]);
        decoded.extend_from_slice(&vec![0x55u8; 0x20 * 0x80 * 2]);
        assert!(
            section_texture_upload(&decoded, 0, 0)
                .expect("unflagged ok")
                .is_none(),
            "flag @ +0x12 clear: no upload"
        );
        decoded[0x12] = 1;
        let u = section_texture_upload(&decoded, 0, 0)
            .expect("flagged ok")
            .expect("upload present");
        assert_eq!(u.rect, SECTION_TEXTURE_RECTS[0]);
        assert_eq!(u.pixels.len(), 0x20 * 0x80 * 2);
    }

    #[test]
    fn placement_rects_tile_each_band_exactly() {
        // The 5 section rects + 2 record[0] rects cover the 128x256
        // halfword band exactly once (no gaps, no overlaps).
        let mut cover = vec![0u32; 0x80 * 0x100];
        for r in SECTION_TEXTURE_RECTS.iter().chain(&RECORD0_TEXTURE_RECTS) {
            for y in r.y0..r.y0 + r.h {
                for x in r.x0..r.x0 + r.w {
                    cover[y as usize * 0x80 + x as usize] += 1;
                }
            }
        }
        assert!(cover.iter().all(|&c| c == 1), "band tiled exactly once");
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

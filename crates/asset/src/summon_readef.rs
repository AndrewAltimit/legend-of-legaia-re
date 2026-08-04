//! `\data\battle\summon.dat` / `\data\battle\readef.DAT` - the battle
//! side-band streaming files (CDNAME block `bat_back_dat`): per-special-attack
//! VRAM texture pages + summon-creature actor records, streamed mid-battle in
//! fixed `0x10800`-byte slots.
//!
//! ## Which PROT entries these are
//!
//! The battle overlay's streaming handler `FUN_801F17F8` opens both files
//! through the file-open shim `FUN_800558FC(path, 0, 0, prot_index)`. In the
//! retail build the ISO9660 open is a trap stub (`_DAT_8007B8C2 != 0`), so the
//! path string is **ignored** and the fourth argument is consumed directly as
//! a PROT TOC index by `FUN_8003E8A8` (the LBA resolver):
//!
//! - `summon.dat` → TOC index `0x37F` ([`SUMMON_RETAIL_TOC_INDEX`])
//! - `readef.DAT` → TOC index `0x380` ([`READEF_RETAIL_TOC_INDEX`])
//!
//! `FUN_8003E8A8` reads `word[(idx + 2) * 4]` of the **raw** in-RAM TOC copy
//! at `0x801C70F0` (the boot loader copies PROT.DAT's first 3 sectors
//! verbatim, 8-byte header included - `streaming_read_api(3, 0x801c70f0,
//! 0x80)` in `FUN_8003E4E8`). [`legaia_prot::archive::Archive`] strips the
//! header and indexes entries at `toc[p + 2]`, so a retail TOC index maps to
//! the extraction-space entry index **minus 2**:
//!
//! - `summon.dat` = extraction entry **893** ([`SUMMON_PROT_INDEX`])
//! - `readef.DAT` = extraction entry **894** ([`READEF_PROT_INDEX`])
//!
//! Both footprints divide exactly: entry 893 = 103 × `0x10800`, entry 894 =
//! 78 × `0x10800`. Byte-verified live: in a mid-cast battle save state the
//! full 67584-byte stream buffer at `*0x8007BD74` equals entry 894 at slot
//! offset `1 * 0x10800`, and slot 0's CLUT row + texture page match VRAM rows
//! 488 / rect (512,0) byte-for-byte.
//!
//! ## Slot framing
//!
//! Each slot is `0x10800` bytes. The battle-action SM (`FUN_801E295C`, case
//! `0x32` of the cast sequence) computes a **base slot byte** from the actor's
//! action id (`actor + 0x1DF`):
//!
//! ```text
//! id <  0x9A:  base = 3 * (id - 1)        (mod 256)
//! id >= 0x9A:  base = 4 * id + 0x63       (mod 256)
//! ```
//!
//! Bit 7 of the base byte selects the file (set → `summon.dat`, clear →
//! `readef.DAT`); `base & 0x7F` is the starting slot index. The applier
//! (`FUN_801F12D0`) then streams slots `base .. base+3` through
//! `FUN_80055B4C` / `FUN_801F17F8` and consumes them positionally:
//!
//! | seq | slot kind | consumed as |
//! |---|---|---|
//! | 1st | [`SlotKind::Texture`] | CLUT row(s) → VRAM `(0,488)`, texture → `(512,0)` |
//! | 2nd | [`SlotKind::Texture`] | CLUT → `(0,490)`, texture → `(640,0)` (id-banded: skipped unless `base ∈ 0x0C..=0x36` or `base >= 0x42`) |
//! | 3rd | raw (big summons, `base >= 0xCB` only) | 240-entry CLUT (STP forced) → `(0,486)`, 64×256 texture → `(448,256)`, `0x8620`-byte part pool → `*0x8007B85C + 0x44000` |
//! | 4th | [`SlotKind::ActorRecord`] | summon-creature install `FUN_801F19EC`: offsets fixed up to pointers, TMD + texture pool handed to `FUN_80055468` (the monster-archive mesh installer) |
//!
//! `readef.DAT` sequences end after the 2nd slot (the applier resets unless
//! bit 7 is set or `base == 0x36`), so the last streamed slot is the group's
//! actor record / payload. `summon.dat` groups are 3 slots wide for ids
//! `0x81..=0x99` (25 groups × 3 = slots 0..=74) and 4 slots wide for the
//! seven big-summon ids `0x9A..=0xA0` (slots 75..=102) - 103 total.
//! `readef.DAT` is 26 groups × 3 = 78 slots, ids `0x01..=0x1A`.
//!
//! A texture slot is `[u32 mode][CLUT rows][4bpp texture page]`: mode 0 =
//! 1 CLUT row + 64-halfword-wide page at `+0x204`; mode 1 = 2 CLUT rows +
//! 128-halfword page at `+0x404`; mode 2 = 1 CLUT row + 128-halfword page at
//! `+0x204`. All pages are 256 rows tall. An actor-record slot leads with
//! three in-slot byte offsets `[name][TMD][texture pool]` (the TMD offset
//! lands on Legaia TMD magic `0x80000002` for every record in the corpus),
//! a part count at `+0x4A` and a part-offset table from `+0x4C`.
//!
//! Provenance: `ghidra/scripts/funcs/overlay_battle_801f17f8.txt`,
//! `overlay_muscle_dome_801f12d0.txt`, `overlay_muscle_dome_801f19ec.txt`,
//! `800558fc.txt`, `8003e8a8.txt`, `8003e4e8.txt`,
//! `overlay_magic_capture_801e295c.txt` (case `0x32`). See
//! [`docs/formats/summon-readef.md`](../../../docs/formats/summon-readef.md).

use anyhow::{Result, bail};

use crate::monster_archive::{MonsterAnimation, MonsterMesh, PartPose};

/// Fixed streaming-slot size in bytes (`0x10800` = 67584 = 33 CD sectors).
pub const SLOT_BYTES: usize = 0x10800;

/// `summon.dat` PROT entry index in extraction space
/// (`extracted/PROT/0893_*.BIN`; `legaia_prot::archive` numbering).
pub const SUMMON_PROT_INDEX: u16 = 893;
/// `readef.DAT` PROT entry index in extraction space.
pub const READEF_PROT_INDEX: u16 = 894;

/// `summon.dat` retail TOC index - the literal fourth argument the battle
/// overlay passes to `FUN_800558FC` (= extraction index + 2).
pub const SUMMON_RETAIL_TOC_INDEX: u16 = 0x37F;
/// `readef.DAT` retail TOC index.
pub const READEF_RETAIL_TOC_INDEX: u16 = 0x380;

/// Slot count of `summon.dat` (footprint / `0x10800`, exact).
pub const SUMMON_SLOT_COUNT: usize = 103;
/// Slot count of `readef.DAT` (footprint / `0x10800`, exact).
pub const READEF_SLOT_COUNT: usize = 78;

/// Legaia TMD magic, used to recognise actor-record slots.
const TMD_MAGIC: u32 = 0x8000_0002;

/// Which side-band file a cast streams from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamFile {
    /// `\data\battle\summon.dat` - Seru-magic summons (base byte bit 7 set).
    Summon,
    /// `\data\battle\readef.DAT` - non-summon special attacks.
    Readef,
}

/// Base slot byte for an action id - the value `FUN_801E295C` case `0x32`
/// writes into the applier context at `+0x277`.
///
/// REF: FUN_801E295C
pub fn base_byte_for_action(action_id: u8) -> u8 {
    if action_id < 0x9A {
        action_id.wrapping_sub(1).wrapping_mul(3)
    } else {
        action_id.wrapping_mul(4).wrapping_add(0x63)
    }
}

/// Resolve an action id to `(file, starting slot index)` the way the
/// streaming handler does: bit 7 of the base byte selects the file
/// (`FUN_801F17F8`), the low 7 bits are the slot index.
pub fn stream_target(action_id: u8) -> (StreamFile, u8) {
    let base = base_byte_for_action(action_id);
    let file = if base & 0x80 != 0 {
        StreamFile::Summon
    } else {
        StreamFile::Readef
    };
    (file, base & 0x7F)
}

/// A `[u32 mode]`-headed texture slot (the 1st/2nd slot of a group).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureSlot {
    /// Layout selector - `0`, `1` or `2` (see module docs).
    pub mode: u32,
    /// Number of 256-entry CLUT rows at byte `+4` (mode 1 → 2, else 1).
    pub clut_rows: usize,
    /// Byte offset of the texture page inside the slot.
    pub texture_offset: usize,
    /// Texture-page width in 16-bit VRAM halfwords (64 or 128; height 256).
    pub texture_width_halfwords: usize,
}

impl TextureSlot {
    /// CLUT byte length (`clut_rows * 512`).
    pub fn clut_bytes(&self) -> usize {
        self.clut_rows * 512
    }
    /// Texture-page byte length (`width_hw * 2 * 256`).
    pub fn texture_bytes(&self) -> usize {
        self.texture_width_halfwords * 2 * 256
    }
}

/// VRAM upload targets for the 1st vs 2nd texture slot of a group
/// (`FUN_801F12D0` cases 2 and 4): `((clut_x, clut_y), (tex_x, tex_y))`.
pub const TEXTURE_SLOT_VRAM_TARGETS: [((u16, u16), (u16, u16)); 2] =
    [((0, 0x1E8), (0x200, 0)), ((0, 0x1EA), (0x280, 0))];

/// A summon-creature actor record (the last streamed slot of a group),
/// consumed in place by `FUN_801F19EC`.
#[derive(Debug, Clone)]
pub struct ActorRecordSlot {
    /// In-slot byte offset of the attack-name string (`rec[0]`).
    pub name_offset: usize,
    /// In-slot byte offset of the Legaia TMD (`rec[1]`, magic `0x80000002`).
    pub tmd_offset: usize,
    /// In-slot byte offset of the texture pool (`rec[2]`), handed to the
    /// monster-archive mesh installer `FUN_80055468`.
    pub texture_pool_offset: usize,
    /// Part count (byte at `+0x4A`).
    pub part_count: u8,
    /// Per-part in-slot offsets (u32 table from `+0x4C`).
    pub part_offsets: Vec<u32>,
    /// NUL-terminated ASCII attack name at `name_offset`, when printable.
    pub name: Option<String>,
}

/// Classification of one `0x10800` slot.
#[derive(Debug, Clone)]
pub enum SlotKind {
    /// `[u32 mode][CLUT][texture page]` - uploaded to VRAM by `FUN_801F12D0`.
    Texture(TextureSlot),
    /// Summon-creature actor record - TMD + texture pool + part table.
    ActorRecord(ActorRecordSlot),
    /// `"ME"` keyframe-stream archive at the slot head - a player
    /// **art-animation stream source** (`readef.DAT` slots `3*char + 1` /
    /// `3*char + 2`, read by `FUN_8002B28C` out of the `_DAT_8007BD74`
    /// streaming buffer this file fills). See [`crate::me_archive`] and
    /// [`crate::battle_char_assembly::art_me_archive`].
    MeArchive {
        /// Entry count.
        count: usize,
        /// How many entries are compressed (size-word bit 15).
        compressed: usize,
    },
    /// Neither shape (raw payload, e.g. the big-summon 3rd slot's
    /// CLUT+texture+part-pool block, or filler).
    Payload,
}

/// One parsed slot.
#[derive(Debug, Clone)]
pub struct Slot {
    /// Slot index (file offset / `0x10800`).
    pub index: usize,
    pub kind: SlotKind,
}

/// Parsed view of one side-band file (`summon.dat` or `readef.DAT`).
#[derive(Debug, Clone)]
pub struct SidebandFile {
    pub slots: Vec<Slot>,
}

/// Decode the `[u32 mode]` texture-slot header the applier consumes.
///
/// PORT: FUN_801F12D0
fn texture_slot(mode: u32) -> Option<TextureSlot> {
    match mode {
        0 => Some(TextureSlot {
            mode,
            clut_rows: 1,
            texture_offset: 0x204,
            texture_width_halfwords: 0x40,
        }),
        1 => Some(TextureSlot {
            mode,
            clut_rows: 2,
            texture_offset: 0x404,
            texture_width_halfwords: 0x80,
        }),
        2 => Some(TextureSlot {
            mode,
            clut_rows: 1,
            texture_offset: 0x204,
            texture_width_halfwords: 0x80,
        }),
        _ => None,
    }
}

/// Parse an actor-record slot the way the summon installer fixes it up.
///
/// PORT: FUN_801F19EC
fn actor_record_slot(slot: &[u8]) -> Option<ActorRecordSlot> {
    let u32_at = |off: usize| -> Option<u32> {
        slot.get(off..off + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    };
    let name_offset = u32_at(0)? as usize;
    let tmd_offset = u32_at(4)? as usize;
    let texture_pool_offset = u32_at(8)? as usize;
    // The installer adds the slot base to all three; sane records keep them
    // in-slot, ordered name < TMD <= pool, and the TMD offset lands on the
    // Legaia TMD magic.
    if name_offset >= SLOT_BYTES || tmd_offset >= SLOT_BYTES || texture_pool_offset >= SLOT_BYTES {
        return None;
    }
    if u32_at(tmd_offset)? != TMD_MAGIC {
        return None;
    }
    let part_count = *slot.get(0x4A)?;
    let mut part_offsets = Vec::with_capacity(part_count as usize);
    for i in 0..part_count as usize {
        part_offsets.push(u32_at(0x4C + i * 4)?);
    }
    let name = slot.get(name_offset..).and_then(|tail| {
        let end = tail.iter().position(|&b| b == 0)?;
        let s = std::str::from_utf8(&tail[..end]).ok()?;
        (!s.is_empty() && s.bytes().all(|b| (0x20..0x7F).contains(&b))).then(|| s.to_owned())
    });
    Some(ActorRecordSlot {
        name_offset,
        tmd_offset,
        texture_pool_offset,
        part_count,
        part_offsets,
        name,
    })
}

/// Parse a whole side-band file (PROT entry 893 or 894 bytes). The length
/// must be an exact multiple of [`SLOT_BYTES`].
pub fn parse(bytes: &[u8]) -> Result<SidebandFile> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(SLOT_BYTES) {
        bail!(
            "side-band file length {:#x} is not a multiple of slot size {SLOT_BYTES:#x}",
            bytes.len()
        );
    }
    let slots = bytes
        .chunks_exact(SLOT_BYTES)
        .enumerate()
        .map(|(index, slot)| {
            let mode = u32::from_le_bytes(slot[..4].try_into().unwrap());
            let kind = if let Some(t) = texture_slot(mode) {
                SlotKind::Texture(t)
            } else if let Some(r) = actor_record_slot(slot) {
                SlotKind::ActorRecord(r)
            } else if let Ok(me) = crate::me_archive::parse(slot) {
                SlotKind::MeArchive {
                    count: me.len(),
                    compressed: (0..me.len())
                        .filter(|&n| me.is_compressed(n) == Some(true))
                        .count(),
                }
            } else {
                SlotKind::Payload
            };
            Slot { index, kind }
        })
        .collect();
    Ok(SidebandFile { slots })
}

/// Minimum slot count a side-band file must have to be recognised from bytes.
/// Retail's two carriers hold 103 and 78; the largest *other* entry on the
/// disc whose footprint divides by [`SLOT_BYTES`] holds 9.
pub const DETECT_MIN_SLOTS: usize = 16;

/// Share of slots that must classify as something other than
/// [`SlotKind::Payload`] for [`detect`] to accept. Retail's two carriers sit at
/// 93 % and 100 %; every other slot-divisible entry on the disc sits at or
/// below 11 %.
pub const DETECT_MIN_NAMED_NUMER: usize = 3;
/// Denominator of [`DETECT_MIN_NAMED_NUMER`].
pub const DETECT_MIN_NAMED_DENOM: usize = 4;

/// Recognise a battle side-band streaming file (`summon.dat` / `readef.DAT`)
/// from bytes alone.
///
/// The footprint dividing exactly by [`SLOT_BYTES`] is necessary but far from
/// sufficient - a fifth of the corpus happens to divide. The discriminator is
/// that a real side-band file's slots each open with a shape the runtime
/// appliers consume (a texture-slot mode word, an actor record whose TMD offset
/// lands on the TMD magic, or an `"ME"` archive), which filler and unrelated
/// containers do not.
pub fn detect(bytes: &[u8]) -> Option<SidebandFile> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(SLOT_BYTES) {
        return None;
    }
    let slots = bytes.len() / SLOT_BYTES;
    if slots < DETECT_MIN_SLOTS {
        return None;
    }
    let parsed = parse(bytes).ok()?;
    let named = parsed
        .slots
        .iter()
        .filter(|s| !matches!(s.kind, SlotKind::Payload))
        .count();
    (named * DETECT_MIN_NAMED_DENOM >= slots * DETECT_MIN_NAMED_NUMER).then_some(parsed)
}

// ---------------------------------------------------------------------------
// Cast view - one summon spell's mesh, texture pool and keyframe clips
// ---------------------------------------------------------------------------

/// The whole player-summon action-id span carried by `summon.dat`: the base +
/// evolved + flute + Evil-Seru groups (`0x81..=0x99`, three slots each) and the
/// seven big-summon groups (`0x9A..=0xA0`, four slots each). Mirrors
/// `legaia_engine_core::summon::PLAYER_SUMMON_IDS`.
pub const PLAYER_CAST_IDS: std::ops::RangeInclusive<u8> = 0x81..=0xA0;

/// The four-slot **big-summon** band - the Sim-Seru (Palma / Mule / Horn /
/// Jedo) and Ra-Seru (Meta / Terra / Ozma) casts. Their groups carry the extra
/// raw CLUT+texture+part-pool slot, and their meshes are bespoke rather than
/// reused `battle_data` creature bodies (see [`crate::summon_creatures`]).
pub const BIG_SUMMON_IDS: std::ops::RangeInclusive<u8> = 0x9A..=0xA0;

/// Byte length of the big-summon raw slot's CLUT region: 240 BGR555 entries =
/// the same 15 × 16-colour region a monster texture pool leads with
/// ([`crate::monster_archive::CLUT_REGION_BYTES`]).
pub const RAW_SLOT_CLUT_BYTES: usize = 0x1E0;
/// Byte length of the big-summon raw slot's 4bpp page (64 halfwords × 256
/// rows = 128 bytes per row = 256 texels wide).
pub const RAW_SLOT_PAGE_BYTES: usize = 0x8000;
/// Offset of the part pool inside a big-summon raw slot - exactly the end of
/// the CLUT + page regions, which is what makes the raw slot's head a
/// monster-shaped texture pool byte-for-byte.
pub const RAW_SLOT_PART_POOL_OFFSET: usize = RAW_SLOT_CLUT_BYTES + RAW_SLOT_PAGE_BYTES;
/// Byte length of the big-summon raw slot's part pool (`FUN_801F12D0` case 6
/// copies it to `*0x8007B85C + 0x44000`). The three regions tile the slot
/// exactly: `0x1E0 + 0x8000 + 0x8620 = 0x10800`.
pub const RAW_SLOT_PART_POOL_BYTES: usize = SLOT_BYTES - RAW_SLOT_PART_POOL_OFFSET;

/// Battle texture slot the summon creature's pool occupies.
///
/// Not a choice: the big-summon raw slot's own VRAM targets are the CLUT row
/// `486` and page origin `(448, 256)` that `FUN_801F12D0` case 6 hardcodes, and
/// those are exactly [`crate::monster_archive::monster_page_origin`] and CLUT
/// row `484 + slot` for `slot = 2`. So a big summon installs over monster
/// battle slot 2, and relocating its mesh with
/// [`crate::monster_archive::MonsterMesh::battle_render_mesh`] at this slot
/// reproduces the retail placement.
pub const SUMMON_VRAM_SLOT: u8 = 2;

/// Offset of the cast's **element** byte inside an actor record (`0`=earth,
/// `1`=water, `2`=fire, `3`=wind, `4`=thunder, `5`=light, `6`=dark, `7`=none).
/// The damage pipeline reads this record byte directly through the record
/// pointer table `0x801C9348` - a cast's element is never the caster's.
pub const ACTOR_ELEMENT_OFFSET: usize = 0x1D;

/// Offset of a per-part entry's packed keyframe stream (`[u8 parts][u8 frames]`
/// then nine-byte TRS records). Same offset as the monster archive's per-action
/// entries; the record's own `+0x88` self-pointer is what the installer fixes
/// up to it.
pub const PART_STREAM_OFFSET: usize = 0x8C;
/// Offset of a per-part entry's playback-rate byte.
pub const PART_RATE_OFFSET: usize = 0x78;
/// Bytes of a per-part entry's head kept as the effect-script region.
const PART_HEAD_BYTES: usize = 0x54;
/// Bytes per packed part record (six 12-bit fields).
const PART_POSE_STRIDE: usize = 9;

/// 4bpp page byte length for the narrow (64 bytes per row) layout.
const NARROW_PAGE_BYTES: usize = 0x4000;
/// 4bpp page byte length for the wide (128 bytes per row) layout.
const WIDE_PAGE_BYTES: usize = 0x8000;

/// `true` when `spell_id` is one of the seven four-slot big-summon casts.
pub fn is_big_summon(spell_id: u8) -> bool {
    BIG_SUMMON_IDS.contains(&spell_id)
}

/// `(first slot index, slot count)` of a cast's `summon.dat` group, or `None`
/// when the id is outside [`PLAYER_CAST_IDS`]. Three slots for `0x81..=0x99`,
/// four for the big-summon band - the same tiling [`stream_target`] resolves.
pub fn group_slots(spell_id: u8) -> Option<(usize, usize)> {
    if !PLAYER_CAST_IDS.contains(&spell_id) {
        return None;
    }
    let (_, first) = stream_target(spell_id);
    Some((first as usize, if is_big_summon(spell_id) { 4 } else { 3 }))
}

/// One `0x10800` slice of a side-band file.
fn slot_bytes(file: &[u8], index: usize) -> Result<&[u8]> {
    file.get(index * SLOT_BYTES..(index + 1) * SLOT_BYTES)
        .ok_or_else(|| anyhow::anyhow!("slot {index} is past the end of the side-band file"))
}

/// Sign-extend a 12-bit field to `i16` (mirrors the monster keyframe decoder).
fn sx12(v: u16) -> i16 {
    if v & 0x800 != 0 {
        (v | 0xf000) as i16
    } else {
        v as i16
    }
}

/// Unpack one nine-byte part record into its six 12-bit fields - the identical
/// bit layout `FUN_8004998C` decodes for the monster archive (low bytes at
/// `[0,1,3,4,6,7]`, high nibbles packed into `[2,5,8]`). Cross-validated
/// byte-for-byte against [`crate::monster_archive::animations`] by the
/// disc-gated `summon_cast_stream_matches_monster_decoder` oracle.
fn unpack_pose(b: &[u8]) -> PartPose {
    let v0 = b[0] as u16 | ((b[2] as u16 & 0x0f) << 8);
    let v1 = b[1] as u16 | ((b[2] as u16 & 0xf0) << 4);
    let v2 = b[3] as u16 | ((b[5] as u16 & 0x0f) << 8);
    let v3 = b[4] as u16 | ((b[5] as u16 & 0xf0) << 4);
    let v4 = b[6] as u16 | ((b[8] as u16 & 0x0f) << 8);
    let v5 = b[7] as u16 | ((b[8] as u16 & 0xf0) << 4);
    PartPose {
        tx: sx12(v0),
        ty: sx12(v1),
        tz: sx12(v2),
        rx: v3 & 0xfff,
        ry: v4 & 0xfff,
        rz: v5 & 0xfff,
    }
}

/// Decode one per-part entry at `entry_off` inside `pool` into a clip.
///
/// `pool` is the buffer the entry offsets are relative to: the **actor-record
/// slot itself** for the three-slot groups, and the big-summon group's **raw
/// slot part pool** (`+0x81E0`) for the four-slot ones - the "off-band fixup
/// arm" the installer takes when the parts live outside the record.
fn parse_part_entry(pool: &[u8], entry_off: usize) -> Option<MonsterAnimation> {
    let action_id = *pool.get(entry_off)?;
    let rate = pool.get(entry_off + PART_RATE_OFFSET).copied().unwrap_or(0);
    let s = entry_off.checked_add(PART_STREAM_OFFSET)?;
    let part_count = *pool.get(s)? as usize;
    let frame_count = *pool.get(s + 1)? as usize;
    if part_count == 0 || frame_count == 0 {
        return None;
    }
    let data = s + 2;
    let need = frame_count * part_count * PART_POSE_STRIDE;
    if data + need > pool.len() {
        return None;
    }
    let mut frames = Vec::with_capacity(frame_count);
    for f in 0..frame_count {
        let mut parts = Vec::with_capacity(part_count);
        for p in 0..part_count {
            let o = data + (f * part_count + p) * PART_POSE_STRIDE;
            parts.push(unpack_pose(&pool[o..o + PART_POSE_STRIDE]));
        }
        frames.push(parts);
    }
    let head_end = (entry_off + PART_HEAD_BYTES).min(pool.len());
    Some(MonsterAnimation {
        action_id,
        rate,
        part_count,
        frame_count,
        frames,
        effect_script: pool.get(entry_off..head_end).unwrap_or_default().to_vec(),
    })
}

/// Byte length of the texture pool at `pool_off`, clamped to the monster-pool
/// layout (`0x1E0` CLUT region + a 64- or 128-byte-per-row 4bpp page over 256
/// rows). A streaming slot is fixed-size, so the pool's tail is zero padding -
/// taking "everything to the end of the slot" would hand the decoder a
/// nonsense row stride. `0` when the record carries no usable pool.
fn texture_pool_extent(slot: &[u8], pool_off: usize) -> usize {
    let Some(tail) = slot.get(pool_off..) else {
        return 0;
    };
    let used = tail.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
    if used <= RAW_SLOT_CLUT_BYTES {
        return 0;
    }
    let page = if used <= RAW_SLOT_CLUT_BYTES + NARROW_PAGE_BYTES {
        NARROW_PAGE_BYTES
    } else {
        WIDE_PAGE_BYTES
    };
    (RAW_SLOT_CLUT_BYTES + page).min(tail.len())
}

/// One decoded summon cast: everything a renderer needs to draw the creature a
/// Seru-magic spell summons and play the clips it performs.
#[derive(Debug, Clone)]
pub struct SummonCast {
    /// Action id (`actor[+0x1DF]`) = the spell-table id of the cast.
    pub spell_id: u8,
    /// The attack-name string at the actor record's `rec[0]` offset (e.g.
    /// `"Burning Attack"`, `"Inferno"`). This is the on-disc name of the cast.
    pub attack_name: Option<String>,
    /// Element the damage pipeline attributes to the cast (record `+0x1D`).
    pub element: u8,
    /// `true` for the four-slot big-summon band (bespoke mesh, parts in the
    /// group's raw slot).
    pub bespoke: bool,
    /// Slot index of the group's actor record inside `summon.dat`.
    pub actor_slot: usize,
    /// Slot indices of the group's `[u32 mode]`-headed FX texture slots - the
    /// per-cast CLUT rows + 4bpp pages the applier uploads to VRAM while the
    /// cast plays. Decode one with [`decode_texture_slot`].
    pub fx_slots: Vec<(usize, TextureSlot)>,
    /// The creature mesh + its texture pool, shaped as a
    /// [`MonsterMesh`] so the ordinary battle relocation
    /// ([`MonsterMesh::battle_render_mesh`] at [`SUMMON_VRAM_SLOT`]) applies -
    /// which is what the retail installer does too (`FUN_801F19EC` routes the
    /// record's TMD + pool through the monster mesh installer `FUN_80055468`).
    pub mesh: MonsterMesh,
    /// The cast's keyframe clips, in the actor record's `+0x4C` table order.
    /// Clip 0 is the cast's opening pose loop; the rest are its phases.
    pub clips: Vec<MonsterAnimation>,
}

impl SummonCast {
    /// Total keyframes across every decoded clip - a cheap non-vacuity probe.
    pub fn total_frames(&self) -> usize {
        self.clips.iter().map(|c| c.frame_count).sum()
    }
}

/// Decode one summon cast out of raw `summon.dat` (extraction PROT
/// [`SUMMON_PROT_INDEX`]) bytes.
///
/// The group's **last** slot is the actor record (`[u32 name][u32 TMD][u32
/// pool]`, part table at `+0x4C`); for the four-slot big-summon groups the
/// per-part keyframe entries live in the **previous** slot's part pool instead
/// of in the record, and the same slot's head is the creature's texture pool.
pub fn parse_cast(summon_dat: &[u8], spell_id: u8) -> Result<SummonCast> {
    if !PLAYER_CAST_IDS.contains(&spell_id) {
        bail!("{spell_id:#04x} is not a player summon cast id");
    }
    let actor_slot = crate::summon_creatures::actor_record_slot_index(spell_id);
    let actor = slot_bytes(summon_dat, actor_slot)?;
    let rec = actor_record_slot(actor)
        .ok_or_else(|| anyhow::anyhow!("slot {actor_slot} is not a summon actor record"))?;
    let bespoke = is_big_summon(spell_id);

    // Where the per-part entries and the creature texture pool come from.
    let (part_pool, tex_pool): (Vec<u8>, Vec<u8>) = if bespoke {
        let raw = slot_bytes(summon_dat, actor_slot - 1)?;
        (
            raw[RAW_SLOT_PART_POOL_OFFSET..].to_vec(),
            raw[..RAW_SLOT_PART_POOL_OFFSET].to_vec(),
        )
    } else {
        let n = texture_pool_extent(actor, rec.texture_pool_offset);
        let pool = actor
            .get(rec.texture_pool_offset..rec.texture_pool_offset + n)
            .unwrap_or_default()
            .to_vec();
        (actor.to_vec(), pool)
    };

    let clips: Vec<MonsterAnimation> = rec
        .part_offsets
        .iter()
        .filter_map(|&off| parse_part_entry(&part_pool, off as usize))
        .collect();

    // Compose a monster-shaped block: the record verbatim (so `tmd_offset`
    // still points at the TMD) with the trimmed pool appended as the tail the
    // monster texture decoder expects.
    let mut block = actor.to_vec();
    let texture_pool_offset = if tex_pool.is_empty() { 0 } else { block.len() };
    block.extend_from_slice(&tex_pool);

    let mut fx_slots = Vec::new();
    if let Some((first, count)) = group_slots(spell_id) {
        for i in first..first + count {
            if i == actor_slot {
                continue;
            }
            let Ok(s) = slot_bytes(summon_dat, i) else {
                continue;
            };
            let mode = u32::from_le_bytes(s[..4].try_into().unwrap());
            if let Some(t) = texture_slot(mode) {
                fx_slots.push((i, t));
            }
        }
    }

    Ok(SummonCast {
        spell_id,
        attack_name: rec.name.clone(),
        element: actor.get(ACTOR_ELEMENT_OFFSET).copied().unwrap_or(7),
        bespoke,
        actor_slot,
        fx_slots,
        mesh: MonsterMesh {
            id: spell_id as u16,
            block,
            tmd_offset: rec.tmd_offset,
            texture_pool_offset,
        },
        clips,
    })
}

/// A decoded FX texture page: the cast's 4bpp page resolved through one
/// 16-colour window of its CLUT row.
#[derive(Debug, Clone)]
pub struct FxPage {
    /// Page width in texels (`texture_width_halfwords * 4`).
    pub width: usize,
    /// Page height in texels (always 256).
    pub height: usize,
    /// Row-major RGBA8; the all-zero BGR555 texel keeps alpha 0.
    pub rgba: Vec<u8>,
}

/// Decode a `[u32 mode]`-headed FX texture slot into RGBA through the
/// `clut_sub`-th 16-colour window of its first CLUT row (the CLUT row is 256
/// entries = 16 such windows, and a prim picks one with `cba & 0x3F`).
pub fn decode_texture_slot(slot: &[u8], t: &TextureSlot, clut_sub: u8) -> Option<FxPage> {
    let (width, height) = (t.texture_width_halfwords * 4, 256usize);
    let clut_base = 4 + (clut_sub as usize & 0xF) * 32;
    let pal: Vec<[u8; 4]> = (0..16)
        .map(|i| {
            let off = clut_base + i * 2;
            let raw = slot
                .get(off..off + 2)
                .map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
            legaia_tim::bgr555_to_rgba8(raw)
        })
        .collect();
    let page = slot.get(t.texture_offset..t.texture_offset + width * height / 2)?;
    let mut rgba = vec![0u8; width * height * 4];
    for (texel, px) in rgba.chunks_exact_mut(4).enumerate() {
        let byte = page[texel / 2];
        let idx = if texel.is_multiple_of(2) {
            byte & 0xF
        } else {
            byte >> 4
        };
        px.copy_from_slice(&pal[idx as usize]);
    }
    Some(FxPage {
        width,
        height,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_rejects_short_and_undivisible_buffers() {
        assert!(detect(&[]).is_none());
        assert!(detect(&vec![0u8; SLOT_BYTES + 1]).is_none());
        // Divisible, but far too few slots and all of them filler.
        assert!(detect(&vec![0xAAu8; SLOT_BYTES * 4]).is_none());
    }

    #[test]
    fn detect_accepts_a_synthetic_texture_slot_file() {
        // Every slot a mode-1 texture slot: the applier's own header shape.
        let mut buf = vec![0u8; SLOT_BYTES * DETECT_MIN_SLOTS];
        for i in 0..DETECT_MIN_SLOTS {
            buf[i * SLOT_BYTES..i * SLOT_BYTES + 4].copy_from_slice(&1u32.to_le_bytes());
        }
        let f = detect(&buf).expect("synthetic side-band file");
        assert_eq!(f.slots.len(), DETECT_MIN_SLOTS);
    }

    #[test]
    fn detect_rejects_a_mostly_filler_file() {
        let mut buf = vec![0u8; SLOT_BYTES * DETECT_MIN_SLOTS];
        // Only two slots carry a mode word; the rest are unrecognisable.
        for i in 0..DETECT_MIN_SLOTS {
            let base = i * SLOT_BYTES;
            let word: u32 = if i < 2 { 1 } else { 0xDEAD_BEEF };
            buf[base..base + 4].copy_from_slice(&word.to_le_bytes());
        }
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn action_id_to_slot_mapping() {
        // readef band: ids 1..=26 -> slots 0,3,..,75 (bit 7 clear).
        assert_eq!(stream_target(0x01), (StreamFile::Readef, 0));
        assert_eq!(stream_target(0x02), (StreamFile::Readef, 3));
        assert_eq!(stream_target(0x1A), (StreamFile::Readef, 75));
        // summon 3-slot band: ids 0x81..=0x99 -> slots 0,3,..,72.
        assert_eq!(stream_target(0x81), (StreamFile::Summon, 0));
        assert_eq!(stream_target(0x82), (StreamFile::Summon, 3));
        assert_eq!(stream_target(0x99), (StreamFile::Summon, 72));
        // big-summon 4-slot band: ids 0x9A..=0xA0 -> slots 75,79,..,99.
        assert_eq!(base_byte_for_action(0x9A), 0xCB);
        assert_eq!(stream_target(0x9A), (StreamFile::Summon, 75));
        assert_eq!(stream_target(0xA0), (StreamFile::Summon, 99));
    }

    #[test]
    fn texture_slot_layouts() {
        let t0 = texture_slot(0).unwrap();
        assert_eq!((t0.clut_rows, t0.texture_offset), (1, 0x204));
        assert_eq!(t0.texture_bytes(), 0x8000);
        let t1 = texture_slot(1).unwrap();
        assert_eq!((t1.clut_rows, t1.texture_offset), (2, 0x404));
        assert_eq!(t1.texture_bytes(), 0x10000);
        let t2 = texture_slot(2).unwrap();
        assert_eq!((t2.clut_rows, t2.texture_offset), (1, 0x204));
        assert_eq!(t2.texture_bytes(), 0x10000);
        assert!(texture_slot(3).is_none());
        // mode-1 slot fits: 4 + 0x400 + 0x10000 <= 0x10800.
        assert!(4 + t1.clut_bytes() + t1.texture_bytes() <= SLOT_BYTES);
    }

    #[test]
    fn parse_rejects_bad_length() {
        assert!(parse(&[0u8; 0x10801]).is_err());
        assert!(parse(&[]).is_err());
    }

    #[test]
    fn cast_groups_tile_the_summon_file_exactly() {
        // 25 three-slot groups then 7 four-slot ones, back to back, ending on
        // the file's last slot. A gap or an overlap here would mean a cast
        // reads another cast's record.
        let mut cursor = 0usize;
        for id in PLAYER_CAST_IDS {
            let (first, count) = group_slots(id).expect("every cast id has a group");
            assert_eq!(
                first, cursor,
                "group for {id:#04x} starts where the last ended"
            );
            assert_eq!(count, if is_big_summon(id) { 4 } else { 3 });
            // The actor record is always the group's final slot.
            assert_eq!(
                crate::summon_creatures::actor_record_slot_index(id),
                first + count - 1,
                "actor record is the last slot of {id:#04x}'s group"
            );
            cursor += count;
        }
        assert_eq!(cursor, SUMMON_SLOT_COUNT);
        assert!(group_slots(0x80).is_none());
        assert!(group_slots(0xA1).is_none());
    }

    #[test]
    fn big_summon_raw_slot_regions_tile_the_slot() {
        assert_eq!(
            RAW_SLOT_CLUT_BYTES + RAW_SLOT_PAGE_BYTES + RAW_SLOT_PART_POOL_BYTES,
            SLOT_BYTES
        );
        assert_eq!(RAW_SLOT_PART_POOL_BYTES, 0x8620);
        // The raw slot's head is a monster-shaped texture pool byte-for-byte,
        // which is why the ordinary battle relocation applies to it.
        assert_eq!(
            RAW_SLOT_CLUT_BYTES,
            crate::monster_archive::CLUT_REGION_BYTES
        );
        // ... at monster battle slot 2, whose page origin is the (448, 256)
        // FUN_801F12D0 case 6 hardcodes.
        assert_eq!(
            crate::monster_archive::monster_page_origin(SUMMON_VRAM_SLOT),
            (448, 256)
        );
    }

    #[test]
    fn texture_pool_extent_picks_the_page_stride_not_the_slot_tail() {
        // A pool whose used bytes fit the narrow page must report the narrow
        // extent even though the slot runs on for tens of KB of zero padding.
        let mut slot = vec![0u8; SLOT_BYTES];
        let pool_off = 0x4000;
        for b in slot[pool_off..pool_off + RAW_SLOT_CLUT_BYTES + NARROW_PAGE_BYTES].iter_mut() {
            *b = 0x21;
        }
        assert_eq!(
            texture_pool_extent(&slot, pool_off),
            RAW_SLOT_CLUT_BYTES + NARROW_PAGE_BYTES
        );
        // One more used byte tips it into the wide layout.
        slot[pool_off + RAW_SLOT_CLUT_BYTES + NARROW_PAGE_BYTES] = 1;
        assert_eq!(
            texture_pool_extent(&slot, pool_off),
            RAW_SLOT_CLUT_BYTES + WIDE_PAGE_BYTES
        );
        // A pointer landing past the record's data is an empty pool.
        let empty = vec![0u8; SLOT_BYTES];
        assert_eq!(texture_pool_extent(&empty, pool_off), 0);
    }

    #[test]
    fn unpack_pose_reads_the_twelve_bit_layout() {
        // Low bytes at [0,1,3,4,6,7]; high nibbles packed into [2,5,8] - the
        // low nibble of the packing byte belongs to the FIRST field of its
        // pair, the high nibble to the second.
        // b[2] = 0x18 -> tx high nibble 0x8 (sign bit set), ty high nibble 0x1.
        let b = [0x01, 0x02, 0x18, 0x03, 0x04, 0x00, 0x05, 0x06, 0x00];
        let p = unpack_pose(&b);
        assert_eq!(p.tx, -2047, "0x801 sign-extends negative");
        assert_eq!(p.ty, 0x102, "high nibble 1 makes a positive 12-bit value");
        assert_eq!(p.tz, 3);
        // Rotations are unsigned 12-bit angles, never sign-extended.
        assert_eq!((p.rx, p.ry, p.rz), (4, 5, 6));
        let hi = [0x00, 0x00, 0x00, 0x00, 0xFF, 0xF0, 0x00, 0x00, 0x00];
        assert_eq!(unpack_pose(&hi).rx, 0xFFF, "12-bit angle stays unsigned");
    }

    #[test]
    fn parse_part_entry_needs_a_stream_that_fits() {
        // parts=2, frames=2 -> 36 bytes of pose data must be present.
        let mut pool = vec![0u8; PART_STREAM_OFFSET + 2 + 2 * 2 * PART_POSE_STRIDE];
        pool[0] = 0x20; // action tag
        pool[PART_RATE_OFFSET] = 2;
        pool[PART_STREAM_OFFSET] = 2;
        pool[PART_STREAM_OFFSET + 1] = 2;
        let anim = parse_part_entry(&pool, 0).expect("stream fits");
        assert_eq!((anim.action_id, anim.rate), (0x20, 2));
        assert_eq!((anim.part_count, anim.frame_count), (2, 2));
        assert_eq!(anim.frames.len(), 2);
        // Truncate by one byte and the entry must be rejected, not read OOB.
        pool.pop();
        assert!(parse_part_entry(&pool, 0).is_none());
        // A zero part or frame count is an empty entry, not a clip.
        pool.push(0);
        pool[PART_STREAM_OFFSET] = 0;
        assert!(parse_part_entry(&pool, 0).is_none());
    }

    #[test]
    fn parse_cast_rejects_ids_outside_the_summon_span() {
        let file = vec![0u8; SLOT_BYTES * SUMMON_SLOT_COUNT];
        assert!(parse_cast(&file, 0x80).is_err());
        assert!(parse_cast(&file, 0xA1).is_err());
        // In range but the file is filler: the actor record must not parse.
        assert!(parse_cast(&file, 0x81).is_err());
    }
}

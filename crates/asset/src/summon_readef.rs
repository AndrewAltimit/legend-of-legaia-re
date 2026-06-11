//! `\data\battle\summon.dat` / `\data\battle\readef.DAT` — the battle
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
//! verbatim, 8-byte header included — `streaming_read_api(3, 0x801c70f0,
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
//! seven big-summon ids `0x9A..=0xA0` (slots 75..=102) — 103 total.
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

/// Fixed streaming-slot size in bytes (`0x10800` = 67584 = 33 CD sectors).
pub const SLOT_BYTES: usize = 0x10800;

/// `summon.dat` PROT entry index in extraction space
/// (`extracted/PROT/0893_*.BIN`; `legaia_prot::archive` numbering).
pub const SUMMON_PROT_INDEX: u16 = 893;
/// `readef.DAT` PROT entry index in extraction space.
pub const READEF_PROT_INDEX: u16 = 894;

/// `summon.dat` retail TOC index — the literal fourth argument the battle
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
    /// `\data\battle\summon.dat` — Seru-magic summons (base byte bit 7 set).
    Summon,
    /// `\data\battle\readef.DAT` — non-summon special attacks.
    Readef,
}

/// Base slot byte for an action id — the value `FUN_801E295C` case `0x32`
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
    /// Layout selector — `0`, `1` or `2` (see module docs).
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
    /// `[u32 mode][CLUT][texture page]` — uploaded to VRAM by `FUN_801F12D0`.
    Texture(TextureSlot),
    /// Summon-creature actor record — TMD + texture pool + part table.
    ActorRecord(ActorRecordSlot),
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
            } else {
                SlotKind::Payload
            };
            Slot { index, kind }
        })
        .collect();
    Ok(SidebandFile { slots })
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

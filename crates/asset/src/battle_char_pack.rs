//! Battle-form character mesh pack - PROT entries `1204` + `1205` (`other5`):
//! the meshes and their texture atlases, one entry each.
//!
//! This is the party's **in-battle** character mesh set: the higher-detail
//! Vahn / Noa / Gala models (plus two extra fighter slots) the engine installs
//! into `DAT_8007C018[0..=2]` for every turn-based battle. The five fighter
//! TMDs have larger `nobj` counts than the field pack (`nobj` 15/16/15 on disc
//! vs. the field pack's 12/12/12; the runtime patches +2 equipment groups, so
//! a live battle slot reads 17/18/17).
//!
//! The **Baka Fighter** fist-fight minigame reuses this same pack - it lets you
//! play *as* Vahn / Noa / Gala, so it borrows the battle character models
//! (`overlay_baka_fighter` loads `data\field\other5.lzs` + PROT 1205/1206 with
//! the debug string `"OTHER5 %d %d"`). That minigame reuse is why earlier
//! captures pinned this pack during Baka Fighter sessions; it is **not** a
//! minigame-exclusive roster.
//!
//! The field-form pack (PROT 0874 §0, see [`crate::character_pack`]) is the
//! *field-only* low-poly walk/talk models; it is **not** used in battle. The
//! captured battle scene loader `FUN_800520F0` fills the effect/model window
//! `DAT_8007C018[3..]` from `etmd.dat`; the party-mesh load that installs this
//! pack into `[0..=2]` is in an as-yet-uncaptured battle-setup overlay (only
//! `overlay_baka_fighter` references the `other5` family in the current dumps).
//!
//! **Provenance (empirical, decisive).** The party slots' live vertex data,
//! read out of `DAT_8007C018[0..=2]` in real-battle save states, byte-matches
//! this pack and **not** the field pack - across the Tetsu tutorial fight, the
//! Gimard Seru-boss fight (an unambiguous turn-based battle), and the
//! full-party `party_battle_gobu_gobu` capture. See
//! [`battle_char_pack_real`](../../tests/battle_char_pack_real.rs).
//!
//! ## On-disc layout - two entries, one pack
//!
//! The meshes and the textures are **two consecutive PROT entries**, not one.
//! PROT 1204 is a flat streaming-format container (no LZS wrapper) holding
//! exactly five chunks of asset type `0x09` (`Tmd2`, the "battle TMD" tag)
//! plus a terminator, and it ends there - `0x25800` bytes, its own 75 sectors
//! to the byte. PROT 1205 is the sibling streaming container holding the
//! character TIM atlases as type-`0x00` chunks.
//!
//! ```text
//! PROT 1204 (75 sectors = 0x25800) - the meshes
//! Offset      Type    Size      Contents
//! ----------  ------  --------  -------------------------------------------
//! 0x000000    [hdr]   4         streaming chunk0 header: type=0x09 size=33516
//! 0x000004    TMD2    33516     slot 0 - Vahn battle (nobj 15)
//! 0x0082F0    [hdr]   4         chunk1 header: type=0x09 size=33636
//! 0x0082F4    TMD2    33636     slot 1 - Noa battle (nobj 16)
//! 0x010658    [hdr]   4         chunk2 header: type=0x09 size=24780
//! 0x01065C    TMD2    24780     slot 2 - Gala battle (nobj 15)
//! 0x016728    [hdr]   4         chunk3 header: type=0x09 size=27036
//! 0x01672C    TMD2    27036     slot 3 - extra battle character (nobj 20)
//! 0x01D0C8    [hdr]   4         chunk4 header: type=0x09 size=33340
//! 0x01D0CC    TMD2    33340     slot 4 - extra battle character (nobj 15)
//! 0x025308    [hdr]   4         terminator (0x00000000), then zero padding
//!
//! PROT 1205 (131 sectors = 0x41800) - the atlases
//! 0x000000    [hdr]   4         chunk0 header: type=0x00 (TIM) size=33312
//! 0x000004    TIM     33312     atlas[0] - 256x256 4bpp + 256x1 CLUT @ (0,490)
//! 0x008228    TIM     33312     atlas[1] - CLUT @ (0,491)
//! 0x01044C    TIM     33312     atlas[2] - CLUT @ (0,492)
//! 0x018670    TIM     33312     atlas[3] - CLUT @ (0,493)
//! 0x020894    TIM     33312     atlas[4] - CLUT @ (0,494)
//! 0x028AB8    TIM     33312     atlas[5] - CLUT @ (0,495)
//! 0x030CDC    TIM     33312     atlas[6] - CLUT @ (0,497)
//! 0x038F00    TIM     33312     atlas[7] - CLUT @ (0,496)
//! 0x041120    [hdr]   4         terminator (0x00000000), then zero padding
//! ```
//!
//! The chunk stride is exactly `0x8224` (4-byte header + a 33312-byte TIM);
//! each TIM is a 256x256 4bpp image plus a 256-colour (16x16) sub-CLUT row at
//! VRAM `(0, 490..=497)`. The character atlases sit just below the
//! [row-479 NPC CLUT band](crate::npc_palette) but above the dialog-font
//! glyph band - the runtime uploads them via the same targeted-upload pass
//! the field engine uses for scene textures.
//!
//! ### Correction: eight atlases, and row 496 is not skipped
//!
//! This module used to describe **seven** atlases inside PROT 1204 at
//! `0x25804 + k*0x8224`, with "row 496 intentionally skipped". Both halves
//! were artifacts of the pre-correction PROT entry size (`toc[p+5] -
//! toc[p+3] + 4`, which for 1204 read 184 sectors instead of 75 and so ran
//! `0x36800` bytes into entry 1205):
//!
//! - `0x25804` is entry 1205's offset `0x4` seen through that window, so
//!   every atlas offset was really a 1205 offset plus `0x25800`.
//! - the window ended at `0x5C000`, i.e. `0x36800` into 1205, which is past
//!   atlas 6 (`0x30CDC`) and short of atlas 7 (`0x38F00`). The eighth atlas
//!   was outside the window, so it was never seen - and because the CLUT rows
//!   were being read from a hard-coded list rather than from each TIM, its
//!   row (496) looked like a gap in the sequence rather than a missing entry.
//!
//! Entry 1205's own 131 sectors hold exactly eight type-`0x00` chunks
//! (`4 + 8*0x8224 = 0x41124` of `0x41800`, remainder zero), whose CLUT rows
//! read off the TIM headers are `490, 491, 492, 493, 494, 495, 497, 496` in
//! disc order. [`parse_atlases`] now walks the chunk chain and takes each row
//! from the TIM, so the constants below are a pinned expectation rather than
//! the source of the answer. This is the same over-read family as the
//! title-TIM "duplicates" in [`crate::title_pak`].
//!
//! ## Slot identity
//!
//! Slots 0/1/2 are pinned by byte-equality against the live RAM allocations
//! `DAT_8007C018[0..=2]` from a save state where Vahn / Noa / Gala are all
//! active in battle ([`scripts/scenarios.toml`'s `party_battle_gobu_gobu`](
//! ../../../scripts/scenarios.toml) is the catalogued capture):
//!
//! - slot 0 → 12 of 17 live group bodies match → **Vahn**.
//! - slot 1 → 16 of 18 live group bodies match → **Noa**.
//! - slot 2 → 17 of 17 live group bodies match → **Gala**.
//!
//! Slots 3 and 4 carry additional battle-form characters whose runtime
//! identity depends on the active scene; the disc bytes are stable, but the
//! engine only installs them into `DAT_8007C018[3..=4]` during battles where
//! those characters participate.
//!
//! ## Cross-reference to the field-form pack
//!
//! | Pack | PROT entry | Layout | nobj (disc) | When resident |
//! |---|---|---|---|---|
//! | Field-form ([`crate::character_pack`]) | 874 §0 | `parse_player_lzs` -> LZS section -> `pack::extract_pack` | 12 / 12 / 12 / 3 / 2 | every field scene |
//! | Battle-form (this module) | 1204 + 1205 | flat streaming-format: 5 TMD2 chunks, then 8 TIM chunks in the sibling entry | 15 / 16 / 15 / 20 / 15 | every BattleMode session |
//!
//! The same `DAT_8007C018[0..=4]` table is repointed between the two; only
//! one form is resident at a time.
//!
//! ## Asset type 0x09 (TMD2)
//!
//! Streaming chunks of type `0x09` are tagged "TMD2" in [`crate::AssetType`]
//! but parse as standard Legaia TMDs (magic `0x80000002`). The distinction is
//! a dispatcher tag: the field engine routes type-0x02 chunks through one
//! installer chain and type-0x09 chunks (which only appear in this pack)
//! through the battle-form chain. The TMD body shape is otherwise identical
//! to the one documented in [`crate::tmd`].

use anyhow::{Context, Result, bail};

/// PROT entry index that carries the battle-form character **meshes** (the
/// five type-`0x09` TMD2 chunks).
pub const PROT_ENTRY_INDEX: u32 = 1204;

/// PROT entry index that carries the battle-form character **atlases** (the
/// eight type-`0x00` TIM chunks). The sibling entry immediately after
/// [`PROT_ENTRY_INDEX`]; see the module docs for why they used to look like
/// one entry.
pub const ATLAS_PROT_ENTRY_INDEX: u32 = 1205;

/// Number of TMDs in the battle-character pack (chunks 0..=4).
pub const SLOT_COUNT: usize = 5;

/// Asset type byte for the battle-form character TMD chunks. Streaming-format
/// dispatch tag; the body is a standard Legaia TMD (magic `0x80000002`).
pub const BATTLE_TMD_CHUNK_TYPE: u8 = 0x09;

/// Number of 256x256 4bpp character TIM atlases in
/// [`ATLAS_PROT_ENTRY_INDEX`].
pub const ATLAS_COUNT: usize = 8;

/// Asset type byte for the atlas chunks: `0x00` (TIM). Streaming-format
/// dispatch tag, same encoding as [`BATTLE_TMD_CHUNK_TYPE`].
pub const ATLAS_CHUNK_TYPE: u8 = 0x00;

/// Stride between successive atlas chunks in bytes - a 4-byte streaming chunk
/// header plus a 33312-byte TIM (8-byte header, 524-byte CLUT block, 32780-byte
/// image block).
pub const ATLAS_STRIDE_BYTES: usize = 0x8224;

/// Byte offset of the first atlas TIM inside [`ATLAS_PROT_ENTRY_INDEX`]: just
/// past that entry's first streaming chunk header.
pub const FIRST_ATLAS_OFFSET: usize = 4;

/// VRAM CLUT row numbers used by the eight atlases, in disc order. Read off
/// the TIM headers by [`parse_atlases`]; this is the pinned expectation, not
/// the source. Note 496 comes **last**, after 497 - see the module docs.
pub const ATLAS_CLUT_ROWS: [u16; ATLAS_COUNT] = [490, 491, 492, 493, 494, 495, 497, 496];

/// Legaia TMD magic (`0x80000002`).
const TMD_MAGIC: u32 = 0x8000_0002;

/// PSX TIM magic (`0x10`).
const TIM_MAGIC: u32 = 0x10;

/// Short display label for one battle-character pack slot. Slots 0/1/2 are
/// the active-party characters Vahn/Noa/Gala (matched by byte-equality
/// against `DAT_8007C018[0..=2]` in the `party_battle_gobu_gobu` save);
/// slots 3/4 are additional battle-form characters whose runtime identity
/// depends on which battle they're installed for.
pub fn slot_label(slot: usize) -> &'static str {
    match slot {
        0 => "Vahn",
        1 => "Noa",
        2 => "Gala",
        3 => "Extra 0",
        4 => "Extra 1",
        _ => "(out of range)",
    }
}

/// One decoded slot of the battle-form character pack: a Legaia TMD plus its
/// provenance.
#[derive(Debug, Clone)]
pub struct BattleCharSlot {
    /// 0-based slot inside the disc pack (`0..=4`).
    pub slot: usize,
    /// `nobj` from the TMD header on disc.
    pub disc_nobj: u32,
    /// Byte offset of this slot's TMD body inside PROT 1204.
    pub file_offset: usize,
    /// Raw disc-form TMD bytes.
    pub tmd_bytes: Vec<u8>,
}

/// One character texture atlas: a 256x256 4bpp PSX TIM with its own 256x1
/// sub-CLUT, at a fixed VRAM coordinate.
#[derive(Debug, Clone)]
pub struct BattleCharAtlas {
    /// 0-based atlas index inside the atlas entry (`0..=7`).
    pub atlas_index: usize,
    /// VRAM Y coordinate of the atlas's CLUT block (X is `0`), read from the
    /// TIM's own CLUT block header. Expected to equal
    /// [`ATLAS_CLUT_ROWS`]`[atlas_index]`.
    pub clut_fb_y: u16,
    /// Byte offset of the TIM (starting at the `0x10` magic) inside
    /// [`ATLAS_PROT_ENTRY_INDEX`].
    pub file_offset: usize,
    /// Raw TIM bytes - the chunk body, i.e. the declared chunk size.
    pub tim_bytes: Vec<u8>,
}

/// The full parsed battle-form character pack - five TMD slots from
/// [`PROT_ENTRY_INDEX`] + eight TIM atlases from [`ATLAS_PROT_ENTRY_INDEX`],
/// each in disc order.
#[derive(Debug, Clone)]
pub struct BattleCharPack {
    pub slots: [BattleCharSlot; SLOT_COUNT],
    pub atlases: [BattleCharAtlas; ATLAS_COUNT],
}

impl BattleCharPack {
    /// Borrowed view of all five slots.
    pub fn slots(&self) -> &[BattleCharSlot; SLOT_COUNT] {
        &self.slots
    }

    /// Get one slot by its 0-based pack index.
    pub fn slot(&self, idx: usize) -> Option<&BattleCharSlot> {
        self.slots.get(idx)
    }

    /// Get one atlas by its 0-based pack index.
    pub fn atlas(&self, idx: usize) -> Option<&BattleCharAtlas> {
        self.atlases.get(idx)
    }
}

fn read_u32_le(buf: &[u8], off: usize) -> Result<u32> {
    if off + 4 > buf.len() {
        bail!("read past end of buffer at offset 0x{off:X}");
    }
    Ok(u32::from_le_bytes(buf[off..off + 4].try_into().unwrap()))
}

/// Parse the whole battle-form character pack from its two PROT entries.
///
/// `mesh_entry` is [`PROT_ENTRY_INDEX`] (1204), `atlas_entry` is
/// [`ATLAS_PROT_ENTRY_INDEX`] (1205). See [`parse_slots`] / [`parse_atlases`]
/// for the halves; callers that only need geometry should use `parse_slots`
/// and never read the atlas entry at all.
pub fn parse(mesh_entry: &[u8], atlas_entry: &[u8]) -> Result<BattleCharPack> {
    Ok(BattleCharPack {
        slots: parse_slots(mesh_entry)?,
        atlases: parse_atlases(atlas_entry)?,
    })
}

/// Walk the eight type-`0x00` (TIM) streaming chunks of
/// [`ATLAS_PROT_ENTRY_INDEX`].
///
/// The chunk chain is the authority: each header is
/// `(type << 24) | body_size`, the body is the TIM, and the chain ends on a
/// non-`0x00` type (in retail, a zero terminator word). Each atlas's CLUT row
/// is read from the TIM's own CLUT block header (`+0x0E`), then checked
/// against [`ATLAS_CLUT_ROWS`] - so a future disc revision disagreeing with
/// the constant fails loudly instead of being silently relabelled.
pub fn parse_atlases(atlas_entry: &[u8]) -> Result<[BattleCharAtlas; ATLAS_COUNT]> {
    let buf = atlas_entry;
    let mut atlases: Vec<BattleCharAtlas> = Vec::with_capacity(ATLAS_COUNT);
    let mut cursor = 0usize;
    while atlases.len() < ATLAS_COUNT {
        let head = read_u32_le(buf, cursor).with_context(|| {
            format!(
                "battle_char_pack atlas {}: chunk header at 0x{cursor:X} past end of PROT {ATLAS_PROT_ENTRY_INDEX} (len {})",
                atlases.len(),
                buf.len()
            )
        })?;
        let typ = ((head >> 24) & 0xFF) as u8;
        let size = (head & 0x00FF_FFFF) as usize;
        if typ != ATLAS_CHUNK_TYPE {
            bail!(
                "battle_char_pack atlas {}: expected streaming chunk type 0x{ATLAS_CHUNK_TYPE:02X} (TIM), found 0x{typ:02X} at 0x{cursor:X}",
                atlases.len()
            );
        }
        let body_off = cursor + 4;
        if body_off + size > buf.len() {
            bail!(
                "battle_char_pack atlas {}: chunk body (size {size}) at 0x{body_off:X} overruns PROT {ATLAS_PROT_ENTRY_INDEX} (len {})",
                atlases.len(),
                buf.len()
            );
        }
        let body = &buf[body_off..body_off + size];
        let magic = read_u32_le(body, 0)?;
        if magic != TIM_MAGIC {
            bail!(
                "battle_char_pack atlas {}: expected TIM magic 0x{TIM_MAGIC:02X} at 0x{body_off:X}, got 0x{magic:08X}",
                atlases.len()
            );
        }
        // TIM: [magic u32][flag u32][clut bnum u32][clut fb_x u16][clut fb_y
        // u16]... The atlases are all 4bpp-with-CLUT (`flag & 8`), so the CLUT
        // block is present and its fb_y is the row the mesh's CBA samples.
        let clut_fb_y = u16::from_le_bytes(
            body.get(0x0E..0x10)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "battle_char_pack atlas {}: TIM shorter than a CLUT block header",
                        atlases.len()
                    )
                })?
                .try_into()
                .unwrap(),
        );
        let atlas_index = atlases.len();
        atlases.push(BattleCharAtlas {
            atlas_index,
            clut_fb_y,
            file_offset: body_off,
            tim_bytes: body.to_vec(),
        });
        cursor = body_off + size;
    }
    for atlas in &atlases {
        if atlas.clut_fb_y != ATLAS_CLUT_ROWS[atlas.atlas_index] {
            bail!(
                "battle_char_pack atlas {}: CLUT row {} on disc, expected {}",
                atlas.atlas_index,
                atlas.clut_fb_y,
                ATLAS_CLUT_ROWS[atlas.atlas_index]
            );
        }
    }
    atlases
        .try_into()
        .map_err(|v: Vec<_>| anyhow::anyhow!("expected {ATLAS_COUNT} atlases, got {}", v.len()))
}

/// Walk the five [`BATTLE_TMD_CHUNK_TYPE`] streaming chunks of
/// [`PROT_ENTRY_INDEX`], validating each slot's TMD magic.
pub fn parse_slots(mesh_entry: &[u8]) -> Result<[BattleCharSlot; SLOT_COUNT]> {
    let buf = mesh_entry;

    // -- 5 streaming TMD2 chunks --
    let mut slots: Vec<BattleCharSlot> = Vec::with_capacity(SLOT_COUNT);
    let mut cursor = 0usize;
    for slot in 0..SLOT_COUNT {
        let head = read_u32_le(buf, cursor)?;
        let typ = ((head >> 24) & 0xFF) as u8;
        let size = (head & 0x00FF_FFFF) as usize;
        if typ != BATTLE_TMD_CHUNK_TYPE {
            bail!(
                "battle_char_pack slot {slot}: expected streaming chunk type 0x{:02X} (TMD2), found 0x{:02X}",
                BATTLE_TMD_CHUNK_TYPE,
                typ
            );
        }
        let body_off = cursor + 4;
        if body_off + size > buf.len() {
            bail!(
                "battle_char_pack slot {slot}: chunk body (size {size}) overruns buffer (len {})",
                buf.len()
            );
        }
        let body = &buf[body_off..body_off + size];
        if body.len() < 0x0C {
            bail!(
                "battle_char_pack slot {slot}: chunk body too short ({}) for a Legaia TMD header",
                body.len()
            );
        }
        let magic = u32::from_le_bytes(body[..4].try_into().unwrap());
        if magic != TMD_MAGIC {
            bail!(
                "battle_char_pack slot {slot}: expected Legaia TMD magic 0x{:08X}, got 0x{:08X}",
                TMD_MAGIC,
                magic
            );
        }
        let disc_nobj = u32::from_le_bytes(body[0x08..0x0C].try_into().unwrap());
        slots.push(BattleCharSlot {
            slot,
            disc_nobj,
            file_offset: body_off,
            tmd_bytes: body.to_vec(),
        });
        cursor = body_off + size;
    }
    // Terminator at `cursor` - a zero u32. (We don't strictly require it, but
    // bail if a sixth chunk of type=0x09 turned up, which would indicate the
    // SLOT_COUNT is wrong for this corpus.)
    if cursor + 4 <= buf.len() {
        let next = read_u32_le(buf, cursor)?;
        let next_typ = ((next >> 24) & 0xFF) as u8;
        if next_typ == BATTLE_TMD_CHUNK_TYPE {
            bail!("battle_char_pack: unexpected 6th TMD2 chunk after slot {SLOT_COUNT}");
        }
    }
    slots
        .try_into()
        .map_err(|v: Vec<_>| anyhow::anyhow!("expected {SLOT_COUNT} slots, got {}", v.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic minimal pack across the two entries: a mesh entry of 5 chunks
    /// each holding a 12-byte TMD header (magic + flag + nobj) plus a
    /// terminator, and an atlas entry of 8 minimal TIM chunks. Verifies the
    /// parser threads both formats without bailing.
    #[test]
    fn parses_minimal_synthetic() {
        // -- mesh entry --
        let body_size: u32 = 12;
        let nobj_seq = [12u32, 13, 14, 15, 16];
        let mut mesh = vec![0u8; SLOT_COUNT * (4 + body_size as usize) + 4];
        let mut cursor = 0usize;
        for &n in &nobj_seq {
            let head = (BATTLE_TMD_CHUNK_TYPE as u32) << 24 | body_size;
            mesh[cursor..cursor + 4].copy_from_slice(&head.to_le_bytes());
            let body_off = cursor + 4;
            mesh[body_off..body_off + 4].copy_from_slice(&TMD_MAGIC.to_le_bytes());
            mesh[body_off + 4..body_off + 8].copy_from_slice(&0u32.to_le_bytes());
            mesh[body_off + 8..body_off + 12].copy_from_slice(&n.to_le_bytes());
            cursor = body_off + body_size as usize;
        }

        // -- atlas entry: 8 chunks at the real stride, then a terminator --
        let tim_size = ATLAS_STRIDE_BYTES - 4;
        let mut atlas = vec![0u8; ATLAS_COUNT * ATLAS_STRIDE_BYTES + 4];
        for (i, &y) in ATLAS_CLUT_ROWS.iter().enumerate() {
            let head_off = i * ATLAS_STRIDE_BYTES;
            let head = (ATLAS_CHUNK_TYPE as u32) << 24 | tim_size as u32;
            atlas[head_off..head_off + 4].copy_from_slice(&head.to_le_bytes());
            let tim_off = head_off + 4;
            atlas[tim_off..tim_off + 4].copy_from_slice(&TIM_MAGIC.to_le_bytes());
            // flag = 8: 4bpp with a CLUT block, so `+0x0E` is the CLUT fb_y.
            atlas[tim_off + 4..tim_off + 8].copy_from_slice(&0x08u32.to_le_bytes());
            atlas[tim_off + 0x0E..tim_off + 0x10].copy_from_slice(&y.to_le_bytes());
        }

        let pack = parse(&mesh, &atlas).expect("synthetic pack should parse");
        assert_eq!(pack.slots.len(), SLOT_COUNT);
        assert_eq!(pack.atlases.len(), ATLAS_COUNT);
        for (i, slot) in pack.slots.iter().enumerate() {
            assert_eq!(slot.slot, i);
            assert_eq!(slot.disc_nobj, nobj_seq[i]);
        }
        for (i, atlas) in pack.atlases.iter().enumerate() {
            assert_eq!(atlas.atlas_index, i);
            assert_eq!(atlas.clut_fb_y, ATLAS_CLUT_ROWS[i]);
            assert_eq!(
                atlas.file_offset,
                FIRST_ATLAS_OFFSET + i * ATLAS_STRIDE_BYTES
            );
        }
    }

    /// The mesh entry alone must not be accepted as the atlas entry: that is
    /// exactly the over-read this split corrects, and it should fail loudly.
    #[test]
    fn mesh_entry_is_not_an_atlas_entry() {
        let mut mesh = vec![0u8; 64];
        let head = (BATTLE_TMD_CHUNK_TYPE as u32) << 24 | 12u32;
        mesh[..4].copy_from_slice(&head.to_le_bytes());
        let err = parse_atlases(&mesh).expect_err("a TMD2 chunk is not a TIM chunk");
        assert!(err.to_string().contains("found 0x09"), "{err}");
    }
}

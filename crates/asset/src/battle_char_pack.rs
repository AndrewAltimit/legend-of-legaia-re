//! Battle-form character mesh pack — PROT entry `1204` (`other5`).
//!
//! This is the party's **in-battle** character mesh set: the higher-detail
//! Vahn / Noa / Gala models (plus two extra fighter slots) the engine installs
//! into `DAT_8007C018[0..=2]` for every turn-based battle. The five fighter
//! TMDs have larger `nobj` counts than the field pack (`nobj` 15/16/15 on disc
//! vs. the field pack's 12/12/12; the runtime patches +2 equipment groups, so
//! a live battle slot reads 17/18/17).
//!
//! The **Baka Fighter** fist-fight minigame reuses this same pack — it lets you
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
//! this pack and **not** the field pack — across the Tetsu tutorial fight, the
//! Gimard Seru-boss fight (an unambiguous turn-based battle), and the
//! full-party `party_battle_gobu_gobu` capture. See
//! [`battle_char_pack_real`](../../tests/battle_char_pack_real.rs).
//!
//! ## On-disc layout
//!
//! PROT 1204 is a flat streaming-format container (no LZS wrapper) with
//! exactly five chunks of asset type `0x09` (`Tmd2`, the "battle TMD" tag),
//! plus a terminator, plus seven fixed-stride character TIM atlases.
//!
//! ```text
//! Offset      Type    Size      Contents
//! ----------  ------  --------  -------------------------------------------
//! 0x000000    [hdr]   4         streaming chunk0 header: type=0x09 size=33516
//! 0x000004    TMD2    33516     slot 0 — Vahn battle (nobj 15)
//! 0x0082F0    [hdr]   4         chunk1 header: type=0x09 size=33636
//! 0x0082F4    TMD2    33636     slot 1 — Noa battle (nobj 16)
//! 0x010658    [hdr]   4         chunk2 header: type=0x09 size=24780
//! 0x01065C    TMD2    24780     slot 2 — Gala battle (nobj 15)
//! 0x016728    [hdr]   4         chunk3 header: type=0x09 size=27036
//! 0x01672C    TMD2    27036     slot 3 — extra battle character (nobj 20)
//! 0x01D0C8    [hdr]   4         chunk4 header: type=0x09 size=33340
//! 0x01D0CC    TMD2    33340     slot 4 — extra battle character (nobj 15)
//! 0x025308    [hdr]   4         terminator (0x00000000)
//! 0x02530C    -       4         (alignment padding to next sector boundary)
//! 0x025804    TIM     ~33312    atlas[0] — 256x256 4bpp + 256x1 CLUT @ (0,490)
//! 0x02DA28    TIM     ~33312    atlas[1] — CLUT @ (0,491)
//! 0x035C4C    TIM     ~33312    atlas[2] — CLUT @ (0,492)
//! 0x03DE70    TIM     ~33312    atlas[3] — CLUT @ (0,493)
//! 0x046094    TIM     ~33312    atlas[4] — CLUT @ (0,494)
//! 0x04E2B8    TIM     ~33312    atlas[5] — CLUT @ (0,495)
//! 0x0564DC    TIM     ~33312    atlas[6] — CLUT @ (0,497)
//! ```
//!
//! The TIM stride is exactly `0x8224` (33316 bytes); each TIM is a 256x256
//! 4bpp image plus a 256-color (16x16) sub-CLUT row at VRAM `(0, 490..497)`
//! (row 496 is skipped). The character atlases sit just below the
//! [row-479 NPC CLUT band](crate::npc_palette) but above the dialog-font
//! glyph band — the runtime uploads them via the same targeted-upload pass
//! the field engine uses for scene textures.
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
//! | Battle-form (this module) | 1204 | flat streaming-format with 5 TMD2 chunks + 7 TIMs | 15 / 16 / 15 / 20 / 15 | every BattleMode session |
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

use anyhow::{Result, bail};

/// PROT entry index that carries the battle-form character mesh pack.
pub const PROT_ENTRY_INDEX: u32 = 1204;

/// Number of TMDs in the battle-character pack (chunks 0..=4).
pub const SLOT_COUNT: usize = 5;

/// Asset type byte for the battle-form character TMD chunks. Streaming-format
/// dispatch tag; the body is a standard Legaia TMD (magic `0x80000002`).
pub const BATTLE_TMD_CHUNK_TYPE: u8 = 0x09;

/// Number of 256x256 4bpp character TIM atlases that follow the TMD chunks.
pub const ATLAS_COUNT: usize = 7;

/// Stride between successive atlas TIMs in bytes — 32 bytes of TIM header
/// padding, 524 bytes of CLUT block, ~32 KiB of image block; empirically
/// pinned at `0x8224` in the corpus.
pub const ATLAS_STRIDE_BYTES: usize = 0x8224;

/// First atlas-TIM byte offset inside the container (after the streaming
/// terminator and a small alignment gap).
pub const FIRST_ATLAS_OFFSET: usize = 0x25804;

/// VRAM CLUT row numbers used by the seven atlases (row 496 is intentionally
/// skipped).
pub const ATLAS_CLUT_ROWS: [u16; ATLAS_COUNT] = [490, 491, 492, 493, 494, 495, 497];

/// Legaia TMD magic (`0x80000002`).
const TMD_MAGIC: u32 = 0x8000_0002;

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
    /// 0-based atlas index inside the pack (`0..=6`).
    pub atlas_index: usize,
    /// VRAM Y coordinate of the atlas's CLUT block (X is `0`). Mirrors
    /// [`ATLAS_CLUT_ROWS`]`[atlas_index]`.
    pub clut_fb_y: u16,
    /// Byte offset of the TIM (starting at the `0x10` magic) inside PROT 1204.
    pub file_offset: usize,
    /// Raw TIM bytes (length `ATLAS_STRIDE_BYTES` or shorter for the last
    /// atlas; everything past the TIM payload is alignment padding).
    pub tim_bytes: Vec<u8>,
}

/// The full parsed battle-form character pack — five TMD slots + seven TIM
/// atlases in disc order.
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

/// Parse the battle-form character pack from the raw bytes of PROT entry 1204.
///
/// Walks the five [`BATTLE_TMD_CHUNK_TYPE`] streaming chunks, then reads the
/// seven trailing TIM atlases at their fixed stride. Validates each slot's
/// TMD magic and each atlas's TIM magic; bails on the first inconsistency.
pub fn parse(prot_1204_bytes: &[u8]) -> Result<BattleCharPack> {
    let buf = prot_1204_bytes;

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
    // Terminator at `cursor` — a zero u32. (We don't strictly require it, but
    // bail if a sixth chunk of type=0x09 turned up, which would indicate the
    // SLOT_COUNT is wrong for this corpus.)
    if cursor + 4 <= buf.len() {
        let next = read_u32_le(buf, cursor)?;
        let next_typ = ((next >> 24) & 0xFF) as u8;
        if next_typ == BATTLE_TMD_CHUNK_TYPE {
            bail!("battle_char_pack: unexpected 6th TMD2 chunk after slot {SLOT_COUNT}");
        }
    }
    let slots: [BattleCharSlot; SLOT_COUNT] = slots
        .try_into()
        .map_err(|v: Vec<_>| anyhow::anyhow!("expected {SLOT_COUNT} slots, got {}", v.len()))?;

    // -- 7 trailing TIM atlases at fixed stride --
    let mut atlases: Vec<BattleCharAtlas> = Vec::with_capacity(ATLAS_COUNT);
    for (atlas_index, &clut_row) in ATLAS_CLUT_ROWS.iter().enumerate() {
        let tim_off = FIRST_ATLAS_OFFSET + atlas_index * ATLAS_STRIDE_BYTES;
        if tim_off + 8 > buf.len() {
            bail!(
                "battle_char_pack atlas {atlas_index}: offset 0x{tim_off:X} past end of PROT 1204 (len {})",
                buf.len()
            );
        }
        let magic = read_u32_le(buf, tim_off)?;
        if magic != 0x10 {
            bail!(
                "battle_char_pack atlas {atlas_index}: expected TIM magic 0x10 at 0x{tim_off:X}, got 0x{magic:08X}"
            );
        }
        let end = (tim_off + ATLAS_STRIDE_BYTES).min(buf.len());
        atlases.push(BattleCharAtlas {
            atlas_index,
            clut_fb_y: clut_row,
            file_offset: tim_off,
            tim_bytes: buf[tim_off..end].to_vec(),
        });
    }
    let atlases: [BattleCharAtlas; ATLAS_COUNT] = atlases
        .try_into()
        .map_err(|v: Vec<_>| anyhow::anyhow!("expected {ATLAS_COUNT} atlases, got {}", v.len()))?;

    Ok(BattleCharPack { slots, atlases })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic minimal pack: 5 chunks each holding a 12-byte TMD header
    /// (magic + flag + nobj), one terminator, then 7 minimal TIM headers at
    /// stride 0x8224. Verifies the parser threads the format without bailing.
    #[test]
    fn parses_minimal_synthetic() {
        let mut buf = vec![0u8; FIRST_ATLAS_OFFSET + ATLAS_STRIDE_BYTES * ATLAS_COUNT + 4];
        // 5 TMD2 chunks back-to-back. Use a tiny body size of 12 (header only)
        // for each so we can fit five chunks in well under FIRST_ATLAS_OFFSET.
        let body_size: u32 = 12;
        let nobj_seq = [12u32, 13, 14, 15, 16];
        let mut cursor = 0usize;
        for &n in &nobj_seq {
            let head = (BATTLE_TMD_CHUNK_TYPE as u32) << 24 | body_size;
            buf[cursor..cursor + 4].copy_from_slice(&head.to_le_bytes());
            let body_off = cursor + 4;
            buf[body_off..body_off + 4].copy_from_slice(&TMD_MAGIC.to_le_bytes());
            buf[body_off + 4..body_off + 8].copy_from_slice(&0u32.to_le_bytes());
            buf[body_off + 8..body_off + 12].copy_from_slice(&n.to_le_bytes());
            cursor = body_off + body_size as usize;
        }
        // Terminator (zero u32) already in place. Now plant 7 TIM headers.
        for (i, &y) in ATLAS_CLUT_ROWS.iter().enumerate() {
            let tim_off = FIRST_ATLAS_OFFSET + i * ATLAS_STRIDE_BYTES;
            buf[tim_off..tim_off + 4].copy_from_slice(&0x10u32.to_le_bytes());
            // version+pmode=8 (4bpp + clut). Pmode tests just look at low byte.
            buf[tim_off + 4..tim_off + 8].copy_from_slice(&0x08u32.to_le_bytes());
            // Plant CLUT fb_y so the atlas slot validates the row.
            buf[tim_off + 14..tim_off + 16].copy_from_slice(&y.to_le_bytes());
        }

        let pack = parse(&buf).expect("synthetic pack should parse");
        assert_eq!(pack.slots.len(), SLOT_COUNT);
        assert_eq!(pack.atlases.len(), ATLAS_COUNT);
        for (i, slot) in pack.slots.iter().enumerate() {
            assert_eq!(slot.slot, i);
            assert_eq!(slot.disc_nobj, nobj_seq[i]);
        }
        for (i, atlas) in pack.atlases.iter().enumerate() {
            assert_eq!(atlas.atlas_index, i);
            assert_eq!(atlas.clut_fb_y, ATLAS_CLUT_ROWS[i]);
        }
    }
}

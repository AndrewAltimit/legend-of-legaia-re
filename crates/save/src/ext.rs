//! Engine-side global save state that doesn't fit in per-character records.
//!
//! The retail PSX save format encodes story flags, inventory, and other globals
//! in a block format whose exact layout hasn't been reverse-engineered from the
//! save-screen overlay yet. This module defines a self-describing binary format
//! (`LGSF v1`) for the engine's own slot files that bundles the party records
//! with the global state. Once the retail layout is traced it can be added as a
//! separate round-trip path alongside this one.
//!
//! ## Binary layout (`LGSF v1`)
//!
//! ```text
//! Offset  Bytes  Field
//! 0x00    4      Magic: b"LGSF"
//! 0x04    1      Version: 1
//! 0x05    4      story_flags (u32 LE)
//! 0x09    4      money       (i32 LE)
//! 0x0D    1      inventory_count (N)
//! 0x0E    2*N    inventory pairs: (item_id u8, count u8)
//! 0x0E+2N 1      party_count (M)
//! 0x0F+2N M*0x414  party records (M × CHARACTER_RECORD_SIZE bytes)
//! ```
//!
//! ## Binary layout (`LGSF v2` - extends v1)
//!
//! v2 is backward-compatible: the writer (when emitting v2 or higher) emits
//! a v1-shaped prelude and appends a per-character extension block plus a
//! game-level extension block after the v1 party records:
//!
//! ```text
//! ... v1 fields above ...
//!
//! After party records:
//! 4      ext_magic: b"LGX2"  (sentinel - old v1 readers stop here)
//! 4      ext_total_size (u32 LE) - bytes of the v2 extension block
//! 4      play_time_seconds (u32 LE) - total game time
//! 1      active_party_size (P)
//! P      active_party (P × char_slot bytes; 0..=2 main, 3+ guests)
//! 1      ext_char_count (X)
//! X*Y    per-character extension records (Y bytes each):
//!        - 4    learned_arts_mask (u32 LE)
//!        - 1    spell_count (S)
//!        - S    spell ids (u8 each)
//!        - 1    seru_capture_count (T)
//!        - T*4  per-seru: (seru_id u16 LE, points u16 LE)
//!        - 16   tactical_arts_chain bytes (4 chains × 4 directions max,
//!               packed Command bytes; 0 = empty)
//! 1      saved_chain_count (C)
//! C*M    per saved chain:
//!        - 1    char_slot
//!        - 1    name_len
//!        - N    name bytes (UTF-8)
//!        - 1    sequence_len
//!        - K    Command bytes
//! ```
//!
//! ## Binary layout (`LGSF v3` - extends v2)
//!
//! v3 appends a retail-faithful story-flag bitmap (mirroring the 512-byte
//! region at retail SC offset `0x14C0` / live RAM `0x80085600..0x80085800`)
//! after the v2 ext block. Writers always emit v3 once any byte of
//! [`SaveExt::story_flag_bits`] is non-empty; readers accept v1, v2, and v3.
//!
//! ```text
//! ... v2 fields above ...
//! 4      ext3_magic: b"LGX3" (sentinel - v2 readers stop here)
//! 4      ext3_total_size (u32 LE)
//! 2      story_flag_bits_len (u16 LE)
//! N      story_flag_bits (bytes, mirrors retail SC `+0x14C0`)
//! ```

use anyhow::{Context, Result, bail};

use crate::character::{CHARACTER_RECORD_SIZE, Party};

/// Four-byte magic at the start of every `LGSF` save file.
pub const SAVE_FILE_MAGIC: [u8; 4] = *b"LGSF";
/// Current format version stored in byte 4.
pub const SAVE_FILE_VERSION: u8 = 3;
/// V1 sentinel kept for legacy save reads.
pub const SAVE_FILE_VERSION_V1: u8 = 1;
/// V2 sentinel - first version with the LGX2 ext block but no LGX3 story-flag bitmap.
pub const SAVE_FILE_VERSION_V2: u8 = 2;
/// Magic at the start of the v2 extension block.
pub const SAVE_FILE_EXT_MAGIC: [u8; 4] = *b"LGX2";
/// Magic at the start of the v3 extension block (full story-flag bitmap).
pub const SAVE_FILE_EXT3_MAGIC: [u8; 4] = *b"LGX3";

/// Engine-wide global state that is not part of any per-character record.
///
/// Retail SC blocks store these alongside the character records at fixed
/// offsets:
/// - `story_flag_bits` mirrors the 512-byte bitmap at `0x14C0` (RAM
///   `0x80085600..0x80085800`).
/// - `inventory` mirrors the 144-byte 72-slot pair array at `0x1818`
///   (RAM `0x80085958..0x800859E8`).
///
/// [`Self::story_flags`] is a separate 32-bit flag word at
/// `_DAT_1F800394` (PSX scratchpad) that the field VM operates on via
/// opcodes `0x2E` / `0x2F` / `0x30`. It is **not** a cache of the wide
/// retail bitmap: retail seeds it from the game-mode descriptor table
/// (`mode_table[mode_idx].param`, stride `0x18`, byte offset `0x14`) on
/// mode init in `FUN_8001DCF8`, and the SC save/load path never copies
/// between the bitmap and scratchpad. The two fields therefore
/// round-trip independently.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveExt {
    /// Field-VM scratchpad flag word (`_DAT_1F800394` in retail). Read
    /// by op `0x30` (test bit); set by op `0x2E`; cleared by op `0x2F`.
    /// Seeded on mode init from `mode_table[mode_idx].param` low 16
    /// bits; not synced to or from [`Self::story_flag_bits`].
    pub story_flags: u32,
    /// Full 512-byte story-flag bitmap from retail SC offset `0x14C0`,
    /// mirroring live RAM `0x80085600..0x80085800`. Empty (`vec![]`) means
    /// "no retail-shaped data captured"; engines that don't track the wide
    /// bitmap can leave this default. Reads/writes of the bitmap go through
    /// [`crate::card::read_retail_story_flags`] /
    /// [`crate::card::write_retail_story_flags`].
    pub story_flag_bits: Vec<u8>,
    /// Running gold total mirroring `World::money`. Field-VM op `0x3A`
    /// mutates this; clamped to `[0, 9_999_999]` at runtime.
    pub money: i32,
    /// Per-item-ID inventory counts. Pairs are sorted by `item_id`.
    pub inventory: Vec<(u8, u8)>,
}

/// Per-character v2 extension data. Engines populate this from
/// in-memory state at save time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CharSaveExt {
    /// Bitmask of learned Tactical Arts (1 << art_id). The retail engine
    /// stores this in the character record at `+0x13C`; we mirror it here
    /// as a u32 word for convenience.
    pub learned_arts_mask: u32,
    /// Per-character learned spell list (spell ids).
    pub spells: Vec<u8>,
    /// Per-Seru capture totals: `(seru_id, points)` pairs.
    pub seru_captures: Vec<(u16, u16)>,
    /// Up to four "active" tactical-arts chains the player has bound to
    /// quick-call slots. Each chain is up to four packed `Command` bytes
    /// (`0` terminator). Mirrors the retail in-RAM chain table.
    pub active_chains: [[u8; 4]; 4],
}

/// One named saved chain in the v2 ext block.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedChainRecord {
    /// Character index this chain belongs to.
    pub char_slot: u8,
    /// Display name.
    pub name: String,
    /// Packed Command bytes (`0` = empty / terminator).
    pub sequence: Vec<u8>,
}

/// V2-only top-level extension block (engines fill at save time).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveExtV2 {
    /// Total game time in seconds (engine-side counter).
    pub play_time_seconds: u32,
    /// `char_slot` indices of the active battle party. First three are
    /// main characters; later entries are story guests.
    pub active_party: Vec<u8>,
    /// Per-character extension records keyed by `char_slot`.
    pub per_char: Vec<(u8, CharSaveExt)>,
    /// Cross-character saved chain library.
    pub saved_chains: Vec<SavedChainRecord>,
}

/// A complete engine save file: party records plus global state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveFile {
    /// Per-character roster (N records of [`CHARACTER_RECORD_SIZE`] bytes).
    pub party: Party,
    /// Global engine state bundled with the party.
    pub ext: SaveExt,
    /// V2 extension block. Empty / default when reading v1 saves.
    pub ext_v2: SaveExtV2,
}

impl Default for SaveFile {
    fn default() -> Self {
        Self {
            party: Party { members: vec![] },
            ext: SaveExt::default(),
            ext_v2: SaveExtV2::default(),
        }
    }
}

impl SaveFile {
    /// Serialise to `LGSF v2` bytes (v2 contains a v1-compatible
    /// prelude - old readers can still consume the party + globals).
    pub fn write(&self) -> Vec<u8> {
        let party_bytes = self.party.write();
        let inv = &self.ext.inventory;
        let mut out = Vec::with_capacity(15 + inv.len() * 2 + party_bytes.len() + 256);

        out.extend_from_slice(&SAVE_FILE_MAGIC);
        out.push(SAVE_FILE_VERSION);
        out.extend_from_slice(&self.ext.story_flags.to_le_bytes());
        out.extend_from_slice(&self.ext.money.to_le_bytes());
        out.push(inv.len() as u8);
        for &(id, count) in inv {
            out.push(id);
            out.push(count);
        }
        let party_count = self.party.members.len().min(255) as u8;
        out.push(party_count);
        out.extend_from_slice(&party_bytes[..party_count as usize * CHARACTER_RECORD_SIZE]);

        // V2 extension block.
        let mut ext_block = Vec::new();
        ext_block.extend_from_slice(&self.ext_v2.play_time_seconds.to_le_bytes());
        let active_len = self.ext_v2.active_party.len().min(255) as u8;
        ext_block.push(active_len);
        for &cs in self.ext_v2.active_party.iter().take(active_len as usize) {
            ext_block.push(cs);
        }
        let pc_count = self.ext_v2.per_char.len().min(255) as u8;
        ext_block.push(pc_count);
        for (cs, ce) in self.ext_v2.per_char.iter().take(pc_count as usize) {
            ext_block.push(*cs);
            ext_block.extend_from_slice(&ce.learned_arts_mask.to_le_bytes());
            let s_len = ce.spells.len().min(255) as u8;
            ext_block.push(s_len);
            ext_block.extend_from_slice(&ce.spells[..s_len as usize]);
            let t_len = ce.seru_captures.len().min(255) as u8;
            ext_block.push(t_len);
            for &(sid, pts) in ce.seru_captures.iter().take(t_len as usize) {
                ext_block.extend_from_slice(&sid.to_le_bytes());
                ext_block.extend_from_slice(&pts.to_le_bytes());
            }
            for chain in ce.active_chains.iter() {
                ext_block.extend_from_slice(chain);
            }
        }
        let ch_count = self.ext_v2.saved_chains.len().min(255) as u8;
        ext_block.push(ch_count);
        for ch in self.ext_v2.saved_chains.iter().take(ch_count as usize) {
            ext_block.push(ch.char_slot);
            let nb = ch.name.as_bytes();
            let nlen = nb.len().min(63) as u8;
            ext_block.push(nlen);
            ext_block.extend_from_slice(&nb[..nlen as usize]);
            let slen = ch.sequence.len().min(63) as u8;
            ext_block.push(slen);
            ext_block.extend_from_slice(&ch.sequence[..slen as usize]);
        }

        out.extend_from_slice(&SAVE_FILE_EXT_MAGIC);
        let ext_total_size = ext_block.len() as u32;
        out.extend_from_slice(&ext_total_size.to_le_bytes());
        out.extend_from_slice(&ext_block);

        // V3 extension block: retail-faithful 512-byte story-flag bitmap.
        // Emit unconditionally so the magic byte is a stable parse marker -
        // an empty bitmap encodes as `len=0` and costs 10 bytes.
        let mut ext3_block = Vec::new();
        let bits_len = self.ext.story_flag_bits.len().min(u16::MAX as usize) as u16;
        ext3_block.extend_from_slice(&bits_len.to_le_bytes());
        ext3_block.extend_from_slice(&self.ext.story_flag_bits[..bits_len as usize]);
        out.extend_from_slice(&SAVE_FILE_EXT3_MAGIC);
        let ext3_total_size = ext3_block.len() as u32;
        out.extend_from_slice(&ext3_total_size.to_le_bytes());
        out.extend_from_slice(&ext3_block);

        out
    }

    /// Parse `LGSF` bytes (v1 or v2), or fall back to the old party-only
    /// format for save files written before this module existed.
    pub fn parse(buf: &[u8]) -> Result<Self> {
        if buf.starts_with(&SAVE_FILE_MAGIC) {
            Self::parse_versioned(buf)
        } else {
            // Old format: raw party bytes, no ext data.
            let party = Party::parse(buf).context("parse legacy party-only save")?;
            Ok(Self {
                party,
                ext: SaveExt::default(),
                ext_v2: SaveExtV2::default(),
            })
        }
    }

    fn parse_versioned(buf: &[u8]) -> Result<Self> {
        if buf.len() < 15 {
            bail!("LGSF: buffer too short ({} bytes)", buf.len());
        }
        let version = buf[4];
        match version {
            SAVE_FILE_VERSION_V1 | SAVE_FILE_VERSION_V2 | SAVE_FILE_VERSION => {}
            other => bail!("LGSF: unsupported version {other}"),
        }
        let story_flags = u32::from_le_bytes(buf[5..9].try_into().unwrap());
        let money = i32::from_le_bytes(buf[9..13].try_into().unwrap());
        let inv_count = buf[13] as usize;
        let inv_end = 14 + inv_count * 2;
        if buf.len() < inv_end + 1 {
            bail!("LGSF: truncated inventory (need {} bytes)", inv_end + 1);
        }
        let mut inventory = Vec::with_capacity(inv_count);
        for i in 0..inv_count {
            let off = 14 + i * 2;
            inventory.push((buf[off], buf[off + 1]));
        }
        let party_count = buf[inv_end] as usize;
        let party_start = inv_end + 1;
        let party_end = party_start + party_count * CHARACTER_RECORD_SIZE;
        if buf.len() < party_end {
            bail!(
                "LGSF: truncated party ({party_count} records need {}, got {})",
                party_count * CHARACTER_RECORD_SIZE,
                buf.len().saturating_sub(party_start)
            );
        }
        let party = if party_count == 0 {
            Party { members: vec![] }
        } else {
            Party::parse(&buf[party_start..party_end]).context("LGSF: parse party records")?
        };

        let mut ext = SaveExt {
            story_flags,
            money,
            inventory,
            story_flag_bits: Vec::new(),
        };

        // V1 reads stop at party_end. V2/V3 may have one or two extension blocks.
        if version == SAVE_FILE_VERSION_V1 {
            return Ok(Self {
                party,
                ext,
                ext_v2: SaveExtV2::default(),
            });
        }
        let mut cursor = party_end;
        if cursor + 8 > buf.len() {
            // V2/V3 declared but no ext block - treat as empty.
            return Ok(Self {
                party,
                ext,
                ext_v2: SaveExtV2::default(),
            });
        }
        let magic = &buf[cursor..cursor + 4];
        if magic != SAVE_FILE_EXT_MAGIC {
            bail!("LGSF v2: missing ext magic at {cursor:#x}");
        }
        cursor += 4;
        let ext_total_size =
            u32::from_le_bytes(buf[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4;
        let ext_end = cursor + ext_total_size;
        if buf.len() < ext_end {
            bail!("LGSF v2: ext block truncated");
        }
        let ext_buf = &buf[cursor..ext_end];
        let ext_v2 = parse_ext_v2(ext_buf).context("parse LGSF v2 ext block")?;
        cursor = ext_end;

        // V3 LGX3 ext block carries the 512-byte retail story-flag bitmap.
        if version == SAVE_FILE_VERSION {
            if cursor + 8 > buf.len() {
                bail!("LGSF v3: LGX3 ext header missing (cursor {cursor:#x})");
            }
            let magic3 = &buf[cursor..cursor + 4];
            if magic3 != SAVE_FILE_EXT3_MAGIC {
                bail!("LGSF v3: missing LGX3 magic at {cursor:#x}");
            }
            cursor += 4;
            let ext3_total_size =
                u32::from_le_bytes(buf[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;
            let ext3_end = cursor + ext3_total_size;
            if buf.len() < ext3_end {
                bail!("LGSF v3: LGX3 ext block truncated");
            }
            ext.story_flag_bits =
                parse_ext_v3(&buf[cursor..ext3_end]).context("parse LGSF v3 ext block")?;
        }

        Ok(Self { party, ext, ext_v2 })
    }

    /// Build a [`SaveFile`] from a retail SC save block (8 KiB block whose
    /// first two bytes are [`crate::SAVE_BLOCK_MAGIC`]).
    ///
    /// Reads party records, the 512-byte story-flag bitmap, and the 72-slot
    /// inventory at their pinned offsets. `money` defaults to `0` since the
    /// retail gold offset (`game_data+0x025C`) is documented but not yet
    /// exposed as a read helper. Empty inventory slots (`(0, 0)`) are
    /// dropped so the returned [`SaveExt::inventory`] is compact.
    ///
    /// `max_records` caps the party walk - retail saves never hold more
    /// than 4 active records (Vahn / Noa / Gala / Terra).
    pub fn from_retail_sc_block(sc_block: &[u8], max_records: usize) -> Result<Self> {
        let party = Party::from_retail_sc_block(sc_block, max_records)
            .context("LGSF: parse party records from retail SC block")?;
        let story_flag_bits = crate::card::read_retail_story_flags(sc_block)
            .map(<[u8]>::to_vec)
            .unwrap_or_default();
        let inventory: Vec<(u8, u8)> = crate::card::read_retail_inventory(sc_block)
            .map(|raw| {
                raw.chunks_exact(2)
                    .filter(|p| !(p[0] == 0 && p[1] == 0))
                    .map(|p| (p[0], p[1]))
                    .collect()
            })
            .unwrap_or_default();
        let story_flags = story_flag_bits
            .get(..4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .unwrap_or(0);
        Ok(Self {
            party,
            ext: SaveExt {
                story_flags,
                story_flag_bits,
                money: 0,
                inventory,
            },
            ext_v2: SaveExtV2::default(),
        })
    }

    /// Write this save into a retail SC save block in place.
    ///
    /// Stamps the SC magic at offset 0, then dispatches each region through
    /// the corresponding `write_retail_*` helper. The SC block must be at
    /// least the size of the largest region's end (handled by the underlying
    /// helpers). `ext.story_flag_bits` shorter than 512 bytes is right-padded
    /// with zeros; longer is truncated. Inventory slots past the 72-slot
    /// retail cap are dropped.
    ///
    /// `money` is NOT written: the retail gold offset isn't exposed as a
    /// write helper yet. Engines that need to update gold via this path can
    /// write the 4 bytes directly at `RETAIL_GAME_DATA_OFFSET + 0x025C`.
    pub fn write_into_retail_sc_block(&self, sc_block: &mut [u8]) -> Result<()> {
        if sc_block.len() < 2 {
            bail!("sc_block too small to hold SC magic");
        }
        sc_block[..2].copy_from_slice(&crate::card::SAVE_BLOCK_MAGIC);
        let records: Vec<Vec<u8>> = self.party.members.iter().map(|m| m.raw.to_vec()).collect();
        crate::card::write_retail_char_records(sc_block, &records)?;
        crate::card::write_retail_story_flags(sc_block, &self.ext.story_flag_bits)?;
        crate::card::write_retail_inventory(sc_block, &self.ext.inventory)?;
        Ok(())
    }
}

fn parse_ext_v3(buf: &[u8]) -> Result<Vec<u8>> {
    if buf.len() < 2 {
        bail!("LGX3 ext block too short");
    }
    let bits_len = u16::from_le_bytes(buf[..2].try_into().unwrap()) as usize;
    if buf.len() < 2 + bits_len {
        bail!("LGX3 story_flag_bits truncated");
    }
    Ok(buf[2..2 + bits_len].to_vec())
}

fn parse_ext_v2(buf: &[u8]) -> Result<SaveExtV2> {
    if buf.len() < 4 {
        bail!("ext block too short");
    }
    let mut p = 0usize;
    let play_time_seconds = u32::from_le_bytes(buf[p..p + 4].try_into().unwrap());
    p += 4;
    if p >= buf.len() {
        bail!("ext: missing active party len");
    }
    let active_len = buf[p] as usize;
    p += 1;
    if p + active_len > buf.len() {
        bail!("ext: truncated active party");
    }
    let active_party = buf[p..p + active_len].to_vec();
    p += active_len;
    if p >= buf.len() {
        bail!("ext: missing per-char count");
    }
    let pc_count = buf[p] as usize;
    p += 1;
    let mut per_char = Vec::with_capacity(pc_count);
    for _ in 0..pc_count {
        if p + 6 > buf.len() {
            bail!("ext: per-char prelude truncated");
        }
        let cs = buf[p];
        p += 1;
        let learned_arts_mask = u32::from_le_bytes(buf[p..p + 4].try_into().unwrap());
        p += 4;
        let s_len = buf[p] as usize;
        p += 1;
        if p + s_len > buf.len() {
            bail!("ext: spell list truncated");
        }
        let spells = buf[p..p + s_len].to_vec();
        p += s_len;
        if p >= buf.len() {
            bail!("ext: missing seru count");
        }
        let t_len = buf[p] as usize;
        p += 1;
        if p + t_len * 4 > buf.len() {
            bail!("ext: seru list truncated");
        }
        let mut seru_captures = Vec::with_capacity(t_len);
        for _ in 0..t_len {
            let sid = u16::from_le_bytes(buf[p..p + 2].try_into().unwrap());
            let pts = u16::from_le_bytes(buf[p + 2..p + 4].try_into().unwrap());
            seru_captures.push((sid, pts));
            p += 4;
        }
        if p + 16 > buf.len() {
            bail!("ext: chain table truncated");
        }
        let mut active_chains = [[0u8; 4]; 4];
        for chain in active_chains.iter_mut() {
            chain.copy_from_slice(&buf[p..p + 4]);
            p += 4;
        }
        per_char.push((
            cs,
            CharSaveExt {
                learned_arts_mask,
                spells,
                seru_captures,
                active_chains,
            },
        ));
    }
    let ch_count = if p < buf.len() { buf[p] as usize } else { 0 };
    if p < buf.len() {
        p += 1;
    }
    let mut saved_chains = Vec::with_capacity(ch_count);
    for _ in 0..ch_count {
        if p + 1 > buf.len() {
            bail!("saved_chain: char_slot missing");
        }
        let cs = buf[p];
        p += 1;
        let nlen = buf[p] as usize;
        p += 1;
        if p + nlen > buf.len() {
            bail!("saved_chain: name truncated");
        }
        let name = std::str::from_utf8(&buf[p..p + nlen])
            .context("saved_chain: name not UTF-8")?
            .to_string();
        p += nlen;
        if p >= buf.len() {
            bail!("saved_chain: seq_len missing");
        }
        let slen = buf[p] as usize;
        p += 1;
        if p + slen > buf.len() {
            bail!("saved_chain: sequence truncated");
        }
        let sequence = buf[p..p + slen].to_vec();
        p += slen;
        saved_chains.push(SavedChainRecord {
            char_slot: cs,
            name,
            sequence,
        });
    }
    Ok(SaveExtV2 {
        play_time_seconds,
        active_party,
        per_char,
        saved_chains,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::CharacterRecord;

    fn minimal_save() -> SaveFile {
        SaveFile {
            party: Party {
                members: vec![CharacterRecord::zeroed()],
            },
            ext: SaveExt {
                story_flags: 0xDEAD_BEEF,
                money: 12345,
                inventory: vec![(1, 5), (7, 2), (255, 1)],
                story_flag_bits: Vec::new(),
            },
            ext_v2: SaveExtV2::default(),
        }
    }

    #[test]
    fn round_trip_v2_default_ext() {
        let sf = minimal_save();
        let bytes = sf.write();
        assert_eq!(&bytes[..4], b"LGSF");
        assert_eq!(bytes[4], SAVE_FILE_VERSION);
        let parsed = SaveFile::parse(&bytes).unwrap();
        assert_eq!(parsed, sf);
    }

    #[test]
    fn empty_inventory_and_party() {
        let sf = SaveFile {
            party: Party { members: vec![] },
            ext: SaveExt {
                story_flags: 0,
                money: 0,
                inventory: vec![],
                story_flag_bits: Vec::new(),
            },
            ext_v2: SaveExtV2::default(),
        };
        let bytes = sf.write();
        let parsed = SaveFile::parse(&bytes).unwrap();
        assert_eq!(parsed, sf);
    }

    #[test]
    fn legacy_party_only_fallback() {
        // Old format: just raw party bytes with no magic header.
        let party = Party {
            members: vec![CharacterRecord::zeroed()],
        };
        let bytes = party.write();
        // Must NOT start with LGSF.
        assert_ne!(&bytes[..4], b"LGSF");
        let sf = SaveFile::parse(&bytes).unwrap();
        assert_eq!(sf.party.members.len(), 1);
        assert_eq!(sf.ext, SaveExt::default());
        assert_eq!(sf.ext_v2, SaveExtV2::default());
    }

    #[test]
    fn multi_member_party() {
        let sf = SaveFile {
            party: Party {
                members: vec![CharacterRecord::zeroed(), CharacterRecord::zeroed()],
            },
            ext: SaveExt {
                story_flags: 1,
                money: 999,
                inventory: vec![(0, 3)],
                story_flag_bits: Vec::new(),
            },
            ext_v2: SaveExtV2::default(),
        };
        let bytes = sf.write();
        let parsed = SaveFile::parse(&bytes).unwrap();
        assert_eq!(parsed.party.members.len(), 2);
        assert_eq!(parsed.ext.money, 999);
    }

    #[test]
    fn round_trip_v2_full_ext() {
        let sf = SaveFile {
            party: Party {
                members: vec![CharacterRecord::zeroed()],
            },
            ext: SaveExt::default(),
            ext_v2: SaveExtV2 {
                play_time_seconds: 7200,
                active_party: vec![0, 1, 2],
                per_char: vec![
                    (
                        0,
                        CharSaveExt {
                            learned_arts_mask: 0x0000_00FF,
                            spells: vec![0x10, 0x11, 0x20],
                            seru_captures: vec![(1, 50), (2, 100)],
                            active_chains: [[1, 2, 3, 4], [4, 4, 4, 0], [0, 0, 0, 0], [2, 1, 3, 4]],
                        },
                    ),
                    (
                        1,
                        CharSaveExt {
                            learned_arts_mask: 0x0000_0007,
                            spells: vec![],
                            seru_captures: vec![(5, 25)],
                            active_chains: [[0; 4]; 4],
                        },
                    ),
                ],
                saved_chains: vec![
                    SavedChainRecord {
                        char_slot: 0,
                        name: "Combo A".into(),
                        sequence: vec![1, 2, 3, 4, 1],
                    },
                    SavedChainRecord {
                        char_slot: 2,
                        name: "Power".into(),
                        sequence: vec![4, 4, 4, 4],
                    },
                ],
            },
        };
        let bytes = sf.write();
        let parsed = SaveFile::parse(&bytes).unwrap();
        assert_eq!(parsed, sf);
    }

    #[test]
    fn parse_v1_file_into_v2_struct() {
        // Hand-craft a v1 save: just the v1 fields with version=1.
        let mut v1 = Vec::new();
        v1.extend_from_slice(&SAVE_FILE_MAGIC);
        v1.push(SAVE_FILE_VERSION_V1);
        v1.extend_from_slice(&0u32.to_le_bytes()); // story_flags
        v1.extend_from_slice(&500i32.to_le_bytes()); // money
        v1.push(1); // inv_count
        v1.push(7); // item id
        v1.push(3); // item count
        v1.push(0); // party_count
        let parsed = SaveFile::parse(&v1).unwrap();
        assert_eq!(parsed.ext.money, 500);
        assert_eq!(parsed.ext.inventory, vec![(7, 3)]);
        // V2 ext should be default for v1 saves.
        assert_eq!(parsed.ext_v2, SaveExtV2::default());
    }

    #[test]
    fn parse_unsupported_version_errors() {
        let mut bad = vec![b'L', b'G', b'S', b'F', 99]; // version 99
        bad.extend(std::iter::repeat_n(0u8, 20));
        let err = SaveFile::parse(&bad).unwrap_err();
        assert!(format!("{err:#}").contains("unsupported version"));
    }

    #[test]
    fn ext_v2_default_writes_minimum_bytes() {
        let sf = SaveFile::default();
        let bytes = sf.write();
        // Find the LGX2 marker.
        let marker_pos = bytes.windows(4).position(|w| w == b"LGX2").unwrap();
        // After magic + size header (8 bytes): play_time(4) + active_len(1)
        // + per_char_count(1) + saved_chain_count(1) = 7 bytes minimum.
        let ext_size =
            u32::from_le_bytes(bytes[marker_pos + 4..marker_pos + 8].try_into().unwrap());
        assert_eq!(ext_size, 7);
    }

    #[test]
    fn truncated_v2_ext_returns_error() {
        let mut bad = SaveFile::default().write();
        bad.truncate(bad.len() - 3);
        let err = SaveFile::parse(&bad).unwrap_err();
        assert!(format!("{err:#}").contains("ext"));
    }

    #[test]
    fn round_trip_v3_story_flag_bits() {
        let mut bits = vec![0u8; 0x200];
        for (i, b) in bits.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(0x33);
        }
        let sf = SaveFile {
            party: Party {
                members: vec![CharacterRecord::zeroed()],
            },
            ext: SaveExt {
                story_flags: 0xCAFE_F00D,
                money: 42,
                inventory: vec![(0x10, 7)],
                story_flag_bits: bits.clone(),
            },
            ext_v2: SaveExtV2::default(),
        };
        let bytes = sf.write();
        assert_eq!(bytes[4], SAVE_FILE_VERSION);
        let parsed = SaveFile::parse(&bytes).unwrap();
        assert_eq!(parsed, sf);
        assert_eq!(parsed.ext.story_flag_bits, bits);
    }

    #[test]
    fn parse_v2_file_into_v3_struct_keeps_bits_empty() {
        // Hand-craft a v2 save (no LGX3 trailer).
        let mut v2 = Vec::new();
        v2.extend_from_slice(&SAVE_FILE_MAGIC);
        v2.push(SAVE_FILE_VERSION_V2);
        v2.extend_from_slice(&0u32.to_le_bytes()); // story_flags
        v2.extend_from_slice(&500i32.to_le_bytes()); // money
        v2.push(1); // inv_count
        v2.push(7);
        v2.push(3); // (id, count)
        v2.push(0); // party_count
        v2.extend_from_slice(&SAVE_FILE_EXT_MAGIC);
        // 7 bytes minimum LGX2 body (play_time(4) + active_len(1) + per_char_count(1) + saved_chain_count(1)).
        v2.extend_from_slice(&7u32.to_le_bytes());
        v2.extend_from_slice(&0u32.to_le_bytes()); // play_time
        v2.push(0); // active_party len
        v2.push(0); // per_char count
        v2.push(0); // saved_chain count
        let parsed = SaveFile::parse(&v2).unwrap();
        assert_eq!(parsed.ext.money, 500);
        assert!(parsed.ext.story_flag_bits.is_empty());
    }

    #[test]
    fn truncated_v3_ext_returns_error() {
        let mut bits = vec![0u8; 0x200];
        bits[0] = 0xAB;
        let sf = SaveFile {
            party: Party::default(),
            ext: SaveExt {
                story_flags: 0,
                money: 0,
                inventory: vec![],
                story_flag_bits: bits,
            },
            ext_v2: SaveExtV2::default(),
        };
        let mut bytes = sf.write();
        // Lop off the last 16 bytes - well inside the 0x200-byte LGX3 body.
        bytes.truncate(bytes.len() - 16);
        let err = SaveFile::parse(&bytes).unwrap_err();
        assert!(
            format!("{err:#}").to_lowercase().contains("lgx3"),
            "{err:#}"
        );
    }

    #[test]
    fn save_file_round_trips_through_retail_sc_block() {
        use crate::card::{BLOCK_SIZE, SAVE_BLOCK_MAGIC};

        let mut bits = vec![0u8; crate::card::RETAIL_STORY_FLAGS_SIZE];
        for (i, b) in bits.iter_mut().enumerate() {
            *b = (i as u8).wrapping_add(1);
        }
        // Two records with a recognisable pattern in the first byte so the
        // reader doesn't stop at slot 0 thinking it's empty.
        let mut rec0 = CharacterRecord::zeroed();
        rec0.raw[0] = 0x11;
        let mut rec1 = CharacterRecord::zeroed();
        rec1.raw[0] = 0x22;
        let original = SaveFile {
            party: Party {
                members: vec![rec0, rec1],
            },
            ext: SaveExt {
                story_flags: 0,
                money: 0,
                inventory: vec![(0x05, 9), (0x10, 1), (0x33, 64)],
                story_flag_bits: bits.clone(),
            },
            ext_v2: SaveExtV2::default(),
        };

        let mut sc_block = vec![0u8; BLOCK_SIZE];
        original.write_into_retail_sc_block(&mut sc_block).unwrap();
        assert_eq!(&sc_block[..2], &SAVE_BLOCK_MAGIC);

        let parsed = SaveFile::from_retail_sc_block(&sc_block, 4).unwrap();
        assert_eq!(parsed.party.members.len(), 2);
        assert_eq!(parsed.party.members[0].raw[0], 0x11);
        assert_eq!(parsed.party.members[1].raw[0], 0x22);
        assert_eq!(parsed.ext.story_flag_bits, bits);
        assert_eq!(parsed.ext.inventory, vec![(0x05, 9), (0x10, 1), (0x33, 64)]);
        // story_flags scratchpad u32 mirrors the first 4 bits-bytes LE.
        assert_eq!(
            parsed.ext.story_flags,
            u32::from_le_bytes([bits[0], bits[1], bits[2], bits[3]])
        );
    }
}

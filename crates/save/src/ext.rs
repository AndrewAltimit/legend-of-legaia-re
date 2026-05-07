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

use anyhow::{Context, Result, bail};

use crate::character::{CHARACTER_RECORD_SIZE, Party};

/// Four-byte magic at the start of every `LGSF` save file.
pub const SAVE_FILE_MAGIC: [u8; 4] = *b"LGSF";
/// Current format version stored in byte 4.
pub const SAVE_FILE_VERSION: u8 = 1;

/// Engine-wide global state that is not part of any per-character record.
///
/// The retail PSX memory card stores this alongside the character records
/// in a block whose layout is not yet reversed. Until that trace is done,
/// this struct is the engine's own representation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveExt {
    /// Story-flag word mirroring `World::story_flags` (`_DAT_1F800394` in
    /// retail). Read by field-VM op `0x30`; set by ops `0x31` / `0x32`.
    pub story_flags: u32,
    /// Running gold total mirroring `World::money`. Field-VM op `0x3A`
    /// mutates this; clamped to `[0, 9_999_999]` at runtime.
    pub money: i32,
    /// Per-item-ID inventory counts. Pairs are sorted by `item_id`.
    pub inventory: Vec<(u8, u8)>,
}

/// A complete engine save file: party records plus global state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveFile {
    /// Per-character roster (N records of [`CHARACTER_RECORD_SIZE`] bytes).
    pub party: Party,
    /// Global engine state bundled with the party.
    pub ext: SaveExt,
}

impl SaveFile {
    /// Serialise to `LGSF v1` bytes.
    pub fn write(&self) -> Vec<u8> {
        let party_bytes = self.party.write();
        let inv = &self.ext.inventory;
        // capacity: header(15) + 2*inv_count + party_bytes
        let mut out = Vec::with_capacity(15 + inv.len() * 2 + party_bytes.len());

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
        out
    }

    /// Parse `LGSF v1` bytes, or fall back to the old party-only format for
    /// save files written before this module existed.
    pub fn parse(buf: &[u8]) -> Result<Self> {
        if buf.starts_with(&SAVE_FILE_MAGIC) {
            Self::parse_v1(buf)
        } else {
            // Old format: raw party bytes, no ext data.
            let party = Party::parse(buf).context("parse legacy party-only save")?;
            Ok(Self {
                party,
                ext: SaveExt::default(),
            })
        }
    }

    fn parse_v1(buf: &[u8]) -> Result<Self> {
        if buf.len() < 15 {
            bail!("LGSF v1: buffer too short ({} bytes)", buf.len());
        }
        let version = buf[4];
        if version != SAVE_FILE_VERSION {
            bail!("LGSF: unsupported version {version}");
        }
        let story_flags = u32::from_le_bytes(buf[5..9].try_into().unwrap());
        let money = i32::from_le_bytes(buf[9..13].try_into().unwrap());
        let inv_count = buf[13] as usize;
        let inv_end = 14 + inv_count * 2;
        if buf.len() < inv_end + 1 {
            bail!("LGSF v1: truncated inventory (need {} bytes)", inv_end + 1);
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
                "LGSF v1: truncated party ({party_count} records need {}, got {})",
                party_count * CHARACTER_RECORD_SIZE,
                buf.len().saturating_sub(party_start)
            );
        }
        let party = if party_count == 0 {
            Party { members: vec![] }
        } else {
            Party::parse(&buf[party_start..party_end]).context("LGSF v1: parse party records")?
        };
        Ok(Self {
            party,
            ext: SaveExt {
                story_flags,
                money,
                inventory,
            },
        })
    }
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
            },
        }
    }

    #[test]
    fn round_trip_v1() {
        let sf = minimal_save();
        let bytes = sf.write();
        assert_eq!(&bytes[..4], b"LGSF");
        assert_eq!(bytes[4], 1);
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
            },
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
            },
        };
        let bytes = sf.write();
        let parsed = SaveFile::parse(&bytes).unwrap();
        assert_eq!(parsed.party.members.len(), 2);
        assert_eq!(parsed.ext.money, 999);
    }
}

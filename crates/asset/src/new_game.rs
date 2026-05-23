//! New-game starting-party template (`SCUS_942.54`).
//!
//! When the title screen's NEW GAME row is confirmed, the boot chain
//! (`FUN_80025B64` -> `FUN_801D6704`, see `docs/subsystems/boot.md`) launches
//! the field/town overlay with a fresh game state. Part of that fresh state is
//! the starting party: a small static table in the executable holds each
//! roster member's opening stats and display name, which the seed routine
//! expands into the live per-character records at `0x80084708 + n*0x414`.
//!
//! This module parses that static table straight out of a `SCUS_942.54` image,
//! so the engine can seed a faithful New Game from the user's own disc at
//! runtime - the same "decode in-game data from the disc, never commit Sony
//! bytes" pattern as [`crate::item_names`] / [`crate::spell_names`].
//!
//! ## Record layout (26-byte stride)
//!
//! The table sits at [`PARTY_TEMPLATE_VA`]; each record is eight little-endian
//! `u16` stats followed by a fixed 10-byte NUL-padded name:
//!
//! | Offset | Type | Field |
//! |---|---|---|
//! | `+0`  | u16 | `hp_max` |
//! | `+2`  | u16 | `mp_max` |
//! | `+4`  | u16 | `agl` (also seeds the spirit gauge + cap; see below) |
//! | `+6`  | u16 | `atk` |
//! | `+8`  | u16 | `udf` (upper / physical defence) |
//! | `+10` | u16 | `ldf` (lower / magical defence) |
//! | `+12` | u16 | `spd` |
//! | `+14` | u16 | `intel` |
//! | `+16` | u8[10] | display name, NUL-padded |
//!
//! The table holds [`PARTY_RECORDS`] entries in roster order
//! (`Vahn`, `Noa`, `Gala`, `Terra`). At a true New Game only Vahn has joined;
//! the rest are the templates the game uses when each character is introduced.
//!
//! The `+4` stat is a single value the seed routine fans out to several live
//! fields. Cross-validated against an early `town01` save state, Vahn's `+4`
//! (`100`) lands in the live record as `agl`, `cap_constant`, and the initial
//! spirit-gauge value all at once; the per-character archetypes
//! (`Noa = 120`, `Gala = 80`) read as agility, so this module names it `agl`.
//!
//! The starting (interactive) scene a New Game enters is `town01` (Rim Elm) -
//! [`OPENING_SCENE`]; the executable's default map-name buffer holds the
//! literal `"town01"` past the same boot chain.

/// RAM address of the starting-party template (Vahn's record base).
pub const PARTY_TEMPLATE_VA: u32 = 0x8007_8C4C;

/// Per-record stride: eight `u16` stats (16 bytes) + a 10-byte name.
pub const RECORD_STRIDE: usize = 26;

/// Number of `u16` stat fields per record.
pub const STAT_COUNT: usize = 8;

/// Length of the fixed, NUL-padded name field per record.
pub const NAME_LEN: usize = 10;

/// Number of roster records the template carries (Vahn, Noa, Gala, Terra).
pub const PARTY_RECORDS: usize = 4;

/// CDNAME label of the interactive opening scene a New Game enters (Rim Elm).
pub const OPENING_SCENE: &str = "town01";

/// PSX-EXE `t_addr` -> file-offset resolver. `SCUS_942.54` loads its data
/// segment at `t_addr` from file offset `0x800`. (Same shape as the resolver
/// in [`crate::item_names`]; kept local so this module stands alone.)
struct ExeMap {
    t_addr: u32,
    t_size: u32,
}

impl ExeMap {
    fn parse(scus: &[u8]) -> Option<Self> {
        if scus.len() < 0x800 || &scus[0..8] != b"PS-X EXE" {
            return None;
        }
        let t_addr = u32::from_le_bytes(scus[0x18..0x1C].try_into().ok()?);
        let t_size = u32::from_le_bytes(scus[0x1C..0x20].try_into().ok()?);
        Some(Self { t_addr, t_size })
    }

    /// File offset for a virtual address, or `None` if outside the data
    /// segment.
    fn off(&self, va: u32) -> Option<usize> {
        if va < self.t_addr || va >= self.t_addr.checked_add(self.t_size)? {
            return None;
        }
        Some((va - self.t_addr) as usize + 0x800)
    }
}

/// One roster member's opening stats + name, decoded from the template.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StartingChar {
    /// Display name (e.g. `"Vahn"`).
    pub name: String,
    /// Maximum (and starting) HP.
    pub hp_max: u16,
    /// Maximum (and starting) MP.
    pub mp_max: u16,
    /// Agility; also seeds the spirit-gauge value and stat cap at New Game.
    pub agl: u16,
    /// Physical attack.
    pub atk: u16,
    /// Upper / physical defence.
    pub udf: u16,
    /// Lower / magical defence.
    pub ldf: u16,
    /// Speed (turn-order initiative seed).
    pub spd: u16,
    /// Intelligence.
    pub intel: u16,
}

/// The decoded starting-party template: one [`StartingChar`] per roster slot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StartingParty {
    members: Vec<StartingChar>,
}

impl StartingParty {
    /// Parse the starting-party template out of a `SCUS_942.54` image. Returns
    /// `None` if the image isn't a PSX-EXE or the table address is out of
    /// range.
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        let map = ExeMap::parse(scus)?;
        let mut members = Vec::with_capacity(PARTY_RECORDS);
        for rec in 0..PARTY_RECORDS {
            let base = map.off(PARTY_TEMPLATE_VA + (rec * RECORD_STRIDE) as u32)?;
            let stat = |i: usize| -> Option<u16> {
                let o = base + i * 2;
                Some(u16::from_le_bytes(scus.get(o..o + 2)?.try_into().ok()?))
            };
            let name_off = base + STAT_COUNT * 2;
            let name_bytes = scus.get(name_off..name_off + NAME_LEN)?;
            let end = name_bytes.iter().position(|&b| b == 0).unwrap_or(NAME_LEN);
            let name = String::from_utf8_lossy(&name_bytes[..end]).into_owned();
            members.push(StartingChar {
                name,
                hp_max: stat(0)?,
                mp_max: stat(1)?,
                agl: stat(2)?,
                atk: stat(3)?,
                udf: stat(4)?,
                ldf: stat(5)?,
                spd: stat(6)?,
                intel: stat(7)?,
            });
        }
        Some(Self { members })
    }

    /// Build directly from a member list (tests / non-SCUS callers).
    pub fn from_members(members: Vec<StartingChar>) -> Self {
        Self { members }
    }

    /// All roster records in slot order.
    pub fn members(&self) -> &[StartingChar] {
        &self.members
    }

    /// The record at roster slot `idx` (`0` = Vahn), or `None` if out of range.
    pub fn member(&self, idx: usize) -> Option<&StartingChar> {
        self.members.get(idx)
    }

    /// Number of records decoded.
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// `true` when no records were decoded.
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal PSX-EXE image holding the given records at
    /// [`PARTY_TEMPLATE_VA`], so the parser is exercisable without Sony bytes.
    fn synth_scus(records: &[StartingChar]) -> Vec<u8> {
        const T_ADDR: u32 = 0x8001_0000;
        let table_off = (PARTY_TEMPLATE_VA - T_ADDR) as usize + 0x800;
        let total = table_off + records.len() * RECORD_STRIDE + 0x10;
        let mut buf = vec![0u8; total];
        buf[0..8].copy_from_slice(b"PS-X EXE");
        buf[0x18..0x1C].copy_from_slice(&T_ADDR.to_le_bytes());
        let t_size = (total - 0x800) as u32;
        buf[0x1C..0x20].copy_from_slice(&t_size.to_le_bytes());
        for (i, r) in records.iter().enumerate() {
            let base = table_off + i * RECORD_STRIDE;
            for (j, v) in [
                r.hp_max, r.mp_max, r.agl, r.atk, r.udf, r.ldf, r.spd, r.intel,
            ]
            .iter()
            .enumerate()
            {
                buf[base + j * 2..base + j * 2 + 2].copy_from_slice(&v.to_le_bytes());
            }
            let name_off = base + STAT_COUNT * 2;
            let nb = r.name.as_bytes();
            let n = nb.len().min(NAME_LEN - 1);
            buf[name_off..name_off + n].copy_from_slice(&nb[..n]);
        }
        buf
    }

    fn vahn() -> StartingChar {
        StartingChar {
            name: "Vahn".into(),
            hp_max: 180,
            mp_max: 20,
            agl: 100,
            atk: 24,
            udf: 16,
            ldf: 12,
            spd: 19,
            intel: 9,
        }
    }

    #[test]
    fn parses_records_and_names() {
        let noa = StartingChar {
            name: "Noa".into(),
            hp_max: 150,
            mp_max: 10,
            agl: 120,
            atk: 21,
            udf: 13,
            ldf: 11,
            spd: 30,
            intel: 3,
        };
        let scus = synth_scus(&[vahn(), noa.clone(), Default::default(), Default::default()]);
        let party = StartingParty::from_scus(&scus).expect("parse");
        assert_eq!(party.len(), PARTY_RECORDS);
        assert_eq!(party.member(0), Some(&vahn()));
        assert_eq!(party.member(1), Some(&noa));
        // Out-of-range slot.
        assert_eq!(party.member(PARTY_RECORDS), None);
    }

    #[test]
    fn non_psx_exe_returns_none() {
        assert!(StartingParty::from_scus(b"not an exe").is_none());
        assert!(StartingParty::from_scus(&[0u8; 0x900]).is_none());
    }

    #[test]
    fn from_members_round_trips() {
        let p = StartingParty::from_members(vec![vahn()]);
        assert_eq!(p.member(0).unwrap().name, "Vahn");
        assert!(!p.is_empty());
    }
}

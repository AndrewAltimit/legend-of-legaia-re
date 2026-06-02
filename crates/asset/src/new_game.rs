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
//! A New Game's first scene is the prologue cutscene `opdeene`
//! ([`OPENING_CUTSCENE_SCENE`]) - the in-engine 3D "It was the Seru."
//! Genesis-tree narration. The front-end launcher (`init_game` in the title
//! overlay) writes that scene id into the active-scene-name buffer
//! (`0x8007050C`); a `new_game_cutscene_intro_a` save state reads back
//! `"opdeene"` there, and the later Rim Elm states read `"town01"`. The
//! cutscene hands off to the interactive scene `town01` (Rim Elm) -
//! [`OPENING_SCENE`].

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

/// RAM address of the new-game inventory-seed code block inside
/// `FUN_80034A6C`. The retail routine writes the single starting item here
/// (`DAT_80085958 = 0x77` / `DAT_80085959 = 5` = Healing Leaf ×5) with a
/// `li`/`sb` pair, immediately followed by an inline loop that zeroes the 512
/// bytes *below* the inventory. Both callers (`FUN_8001DCF8`'s new-game branch
/// and `FUN_8001FFA4`) memset `SC[0..0x1a18)` — which includes the whole
/// inventory — right before calling, so that inline zero-loop is redundant.
/// The 10 instructions from here on (`0x80034b04..0x80034b2b`, 40 bytes) are
/// therefore reclaimable as the starting-item seed region.
pub const STARTING_INV_SEED_VA: u32 = 0x8003_4B04;

/// Byte length of the reclaimable starting-item seed region (10 MIPS
/// instructions = 4 original seed + 6 redundant zero-loop).
pub const STARTING_INV_SEED_LEN: usize = 40;

/// Byte offset of the consumable inventory base relative to the save-context
/// (`SC`) base (`0x80084140`); the live inventory is `SC + 0x1818`
/// (`0x80085958`). The seed code's `sb`/`sh` stores use `$s0` (= `SC` base)
/// with these offsets, so the decoder reads slots from here.
pub const INVENTORY_SC_OFFSET: u32 = 0x1818;

/// CDNAME label of the prologue cutscene a New Game enters first (the
/// in-engine "It was the Seru." Genesis-tree narration). Written as the
/// opening scene id by the front-end launcher (`init_game`), verified live in
/// the `new_game_cutscene_intro_a` save state. Hands off to [`OPENING_SCENE`].
pub const OPENING_CUTSCENE_SCENE: &str = "opdeene";

/// CDNAME label of the interactive opening scene a New Game reaches after the
/// prologue cutscene ([`OPENING_CUTSCENE_SCENE`]) - Rim Elm.
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

/// File offset of the starting-inventory seed region ([`STARTING_INV_SEED_VA`])
/// within a `SCUS_942.54` image, or `None` if the image isn't a PSX-EXE or the
/// address is out of range. The disc patcher writes the seed patch here.
pub fn starting_inv_seed_file_offset(scus: &[u8]) -> Option<usize> {
    ExeMap::parse(scus)?.off(STARTING_INV_SEED_VA)
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

/// The decoded new-game starting inventory: the `(item_id, count)` slots the
/// seed routine ([`STARTING_INV_SEED_VA`]) writes into the live consumable
/// inventory at New Game, in slot order. Vanilla retail is a single slot
/// `(0x77, 5)` (Healing Leaf ×5); the starting-item randomizer rewrites this
/// region to seed up to five slots.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StartingInventory {
    items: Vec<(u8, u8)>,
}

impl StartingInventory {
    /// Decode the starting inventory by interpreting the seed code region.
    ///
    /// The region writes inventory bytes with one of two idioms, both of which
    /// load a constant into `$v0` then store it relative to `$s0` (= `SC`
    /// base): the vanilla `sb` byte-store pair (`addiu $v0,id; sb …; addiu
    /// $v0,count; sb …`) or the randomizer's packed `sh` halfword-store
    /// (`addiu $v0,(count<<8)|id; sh …`). This walks the 40 bytes, replays
    /// every `sb $v0`/`sh $v0` store into a sparse `SC`-offset → byte map, then
    /// reads `(id, count)` slots from [`INVENTORY_SC_OFFSET`] until the
    /// id-`0` terminator — so it handles either encoding (and any future one)
    /// without special-casing instruction order. Returns `None` if the image
    /// isn't a PSX-EXE or the region is out of range.
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        let map = ExeMap::parse(scus)?;
        let off = map.off(STARTING_INV_SEED_VA)?;
        let region = scus.get(off..off + STARTING_INV_SEED_LEN)?;
        Some(Self::decode_region(region))
    }

    /// Decode a 40-byte seed region (exposed for callers that already hold the
    /// raw bytes, e.g. a patcher reading back its own edit).
    pub fn decode_region(region: &[u8]) -> Self {
        use std::collections::BTreeMap;
        // Top 16 bits of the LE instruction word identify the op + fixed
        // registers ($v0 = rt 2, $s0 = base 16); see the encodings in
        // `docs/formats/new-game-table.md`.
        const ADDIU_V0: u16 = 0x2402; // addiu $v0, $zero, imm16
        const SB_V0_S0: u16 = 0xA202; // sb    $v0, off($s0)
        const SH_V0_S0: u16 = 0xA602; // sh    $v0, off($s0)

        let mut bytes: BTreeMap<u32, u8> = BTreeMap::new();
        let mut v0: u32 = 0;
        for chunk in region.chunks_exact(4) {
            let word = u32::from_le_bytes(chunk.try_into().unwrap());
            let top = (word >> 16) as u16;
            let imm = word & 0xFFFF;
            match top {
                ADDIU_V0 => v0 = imm,
                SB_V0_S0 => {
                    bytes.insert(imm, (v0 & 0xFF) as u8);
                }
                SH_V0_S0 => {
                    bytes.insert(imm, (v0 & 0xFF) as u8);
                    bytes.insert(imm + 1, ((v0 >> 8) & 0xFF) as u8);
                }
                _ => {}
            }
        }
        let mut items = Vec::new();
        let mut slot = 0u32;
        loop {
            let id = bytes
                .get(&(INVENTORY_SC_OFFSET + slot * 2))
                .copied()
                .unwrap_or(0);
            if id == 0 {
                break;
            }
            let count = bytes
                .get(&(INVENTORY_SC_OFFSET + slot * 2 + 1))
                .copied()
                .unwrap_or(0);
            items.push((id, count));
            slot += 1;
        }
        Self { items }
    }

    /// Build directly from `(id, count)` slots (tests / non-SCUS callers).
    pub fn from_items(items: Vec<(u8, u8)>) -> Self {
        Self { items }
    }

    /// The decoded `(item_id, count)` slots in slot order.
    pub fn items(&self) -> &[(u8, u8)] {
        &self.items
    }

    /// Number of seeded slots.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// `true` when the new game seeds no starting items.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
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

    /// Assemble a 40-byte seed region from MIPS instruction words (LE).
    fn region(words: &[u32]) -> Vec<u8> {
        let mut buf = vec![0u8; STARTING_INV_SEED_LEN];
        for (i, w) in words.iter().enumerate() {
            buf[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        buf
    }

    #[test]
    fn decodes_vanilla_sb_seed() {
        // The exact retail instruction stream at 0x80034b04: the `addiu`/`sb`
        // pair seeding Healing Leaf (0x77) ×5, then the redundant zero-loop
        // (which stores via `$zero`/`$v1`, so the decoder ignores it).
        let r = region(&[
            0x240401ff, // addiu $a0, $zero, 0x1ff
            0x02041821, // addu  $v1, $s0, $a0
            0x24020077, // addiu $v0, $zero, 0x77
            0xa2021818, // sb    $v0, 0x1818($s0)   id
            0x24020005, // addiu $v0, $zero, 5
            0xa2021819, // sb    $v0, 0x1819($s0)   count
            0xa0601618, // sb    $zero, 0x1618($v1) (zero-loop body, ignored)
            0x2484ffff, // addiu $a0, $a0, -1
            0x0481fffd, // bgez  $a0, ...
            0x2463ffff, // addiu $v1, $v1, -1
        ]);
        let inv = StartingInventory::decode_region(&r);
        assert_eq!(inv.items(), &[(0x77, 5)]);
    }

    #[test]
    fn decodes_packed_sh_seed() {
        // The randomizer's packed halfword form: `addiu $v0,(count<<8)|id; sh`.
        let r = region(&[
            0x24020280, // addiu $v0, $zero, 0x0280  -> id 0x80, count 2
            0xa6021818, // sh    $v0, 0x1818($s0)
            0x2402017e, // addiu $v0, $zero, 0x017e  -> id 0x7e, count 1
            0xa602181a, // sh    $v0, 0x181a($s0)
            0x24020388, // addiu $v0, $zero, 0x0388  -> id 0x88, count 3
            0xa602181c, // sh    $v0, 0x181c($s0)
            0, 0, 0, 0, // nop padding
        ]);
        let inv = StartingInventory::decode_region(&r);
        assert_eq!(inv.items(), &[(0x80, 2), (0x7e, 1), (0x88, 3)]);
    }

    #[test]
    fn decode_stops_at_id_zero_terminator() {
        // A `sh` that writes id 0 terminates the list even if later slots hold
        // data (matches the game's id-0 sentinel scan).
        let r = region(&[
            0x24020105, // id 5, count 1
            0xa6021818, // sh slot 0
            0x24020000, // id 0 (terminator)
            0xa602181a, // sh slot 1
            0x24020207, // id 7, count 2 (orphaned past the terminator)
            0xa602181c, // sh slot 2
            0, 0, 0, 0,
        ]);
        let inv = StartingInventory::decode_region(&r);
        assert_eq!(inv.items(), &[(5, 1)], "scan stops at the id-0 slot");
    }

    #[test]
    fn empty_region_decodes_to_no_items() {
        assert!(StartingInventory::decode_region(&[0u8; STARTING_INV_SEED_LEN]).is_empty());
    }
}

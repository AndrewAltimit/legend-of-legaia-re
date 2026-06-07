//! Equipment stat-bonus table parser (`DAT_80074F68` + the item property table).
//!
//! Every equippable item (weapon / body armor / head accessory / footwear)
//! grants a fixed block of passive stat bonuses. Like the
//! [item-effect table](crate::item_effect), the lookup is double-indirected:
//! the item id selects an 8-byte bonus record through the shared item table.
//!
//! ## Indexing (Ghidra-traced)
//!
//! From the equip-effect aggregator `FUN_801CF650`
//! (`ghidra/scripts/funcs/overlay_menu_801cf650.txt`), which walks a
//! character's five equipment slots and sums their bonuses:
//!
//! ```text
//! kind        = item_table[id].byte(+0)            // DAT_80074368[id*0xC]; 1 = equipment
//! bonus_index = item_table[id].byte(+1)            // DAT_80074369[id*0xC]
//! record      = (&DAT_80074F68)[bonus_index * 8]   // stride-8 record
//! // only applied when kind == 1; the aggregator sums record[+0..+4]
//! // into five stat accumulators (DAT_801EF09C / 08C / 090 / 094 / 098).
//! ```
//!
//! The same `+1` byte is the [item-effect](crate::item_effect) subtype for
//! consumables (`kind == 2`); it is overloaded per kind.
//!
//! ## Record layout (8 bytes, stride `0x8`)
//!
//! | Offset | Type | Field |
//! |---|---|---|
//! | `+0` | u8 | stat bonus 0 - the head-gear stat (set by head accessories) |
//! | `+1` | u8 | **attack** bonus (weapons' only field; boots also add a small amount) |
//! | `+2` | u8 | **defense-up** (`UDF`) bonus (body armor + head accessories) |
//! | `+3` | u8 | **defense-down** (`LDF`) bonus (body armor + boots) |
//! | `+4` | u8 | stat bonus 4 - the footwear stat (only boots/shoes set it) |
//! | `+5` | u8 | constant `0x40` |
//! | `+6` | u8 | **equip character mask** (`1` Vahn/Meta, `2` Noa/Terra, `4` Gala/Ozma; `7` = anyone) |
//! | `+7` | u8 | **slot type** (`0x00` body, `0x20` head, `0x40` weapon, `0x60` footwear) + bit `0x01` = Ra-Seru |
//!
//! ## What is pinned vs. best-effort
//!
//! The `+1`/`+2`/`+3` fields are **byte-exact** against the curated gamedata:
//! every weapon's `+1` equals its `attack`, and every body armor's `+2`/`+3`
//! equal its `udf`/`ldf`. The `+6` mask matches each item's `equip_best` /
//! `equip_others`. The `+0` and `+4` fields are the remaining two battle-stat
//! bonuses (the agility / speed pair - `+0` appears only on head gear, `+4`
//! only on footwear); the curated tables don't carry those per item, so they
//! are exposed raw rather than named to a guessed stat.
//!
//! ## Provenance + parser
//!
//! Indexing traced from `ghidra/scripts/funcs/overlay_menu_801cf650.txt`
//! (also documented in `docs/subsystems/save-screen.md`). The
//! `legaia_asset::equip_stats` resolver mirrors the same `t_addr -> file-offset`
//! map as [`crate::item_names`] / [`crate::item_effect`]. The disc-gated
//! `equip_stats_real` test pins the attack / defense bytes + equip masks
//! against the real executable and the curated gamedata.

/// RAM address of the stat-bonus table (`DAT_80074F68`).
pub const BONUS_TABLE_VA: u32 = 0x8007_4F68;
/// Per-record stride in bytes.
pub const BONUS_STRIDE: usize = 8;

/// Item property record base (`DAT_80074368`): `+0` kind, `+1` bonus index.
const ITEM_TABLE_BASE_VA: u32 = 0x8007_4368;
/// Item property record stride.
const ITEM_RECORD_STRIDE: u32 = 0x0C;
/// Number of item ids.
const ITEM_COUNT: usize = 256;
/// `kind` byte value marking an equippable item.
pub const KIND_EQUIPMENT: u8 = 1;

/// Equip slot category, from the record's `+7` byte (masked to `0x60`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipSlot {
    /// Body armor (`0x00`).
    Body,
    /// Head accessory - seal / clip / crown / earring / helmet (`0x20`).
    Head,
    /// Weapon (`0x40`).
    Weapon,
    /// Footwear - boots / shoes (`0x60`).
    Footwear,
}

/// One 8-byte equipment stat-bonus record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EquipBonus {
    /// The raw 8 bytes of the record.
    pub raw: [u8; 8],
}

impl EquipBonus {
    /// The five battle-stat bonuses (`+0..+4`). Indices 1/2/3 are
    /// attack/def-up/def-down (pinned); indices 0/4 are the head/footwear stat
    /// bonuses (exposed raw - see module docs).
    pub fn stat_bonus(&self) -> [u8; 5] {
        [
            self.raw[0],
            self.raw[1],
            self.raw[2],
            self.raw[3],
            self.raw[4],
        ]
    }

    /// Attack bonus (`+1`).
    pub fn attack(&self) -> u8 {
        self.raw[1]
    }

    /// Defense-up (`UDF`) bonus (`+2`).
    pub fn def_up(&self) -> u8 {
        self.raw[2]
    }

    /// Defense-down (`LDF`) bonus (`+3`).
    pub fn def_down(&self) -> u8 {
        self.raw[3]
    }

    /// Equip character-mask byte (`+6`): bit `1` Vahn/Meta, `2` Noa/Terra,
    /// `4` Gala/Ozma; `7` = any party member.
    pub fn equip_mask(&self) -> u8 {
        self.raw[6]
    }

    /// `true` if the item is equippable by the character whose mask bit is
    /// `bit` (`1` Vahn, `2` Noa, `4` Gala).
    pub fn equips_mask_bit(&self, bit: u8) -> bool {
        self.equip_mask() & bit != 0
    }

    /// The equip slot category, from `+7 & 0x60`.
    pub fn slot(&self) -> EquipSlot {
        match self.raw[7] & 0x60 {
            0x00 => EquipSlot::Body,
            0x20 => EquipSlot::Head,
            0x40 => EquipSlot::Weapon,
            _ => EquipSlot::Footwear,
        }
    }

    /// `true` if this is a Ra-Seru (story upgrade) equip - bit `0x01` of `+7`.
    pub fn is_ra_seru(&self) -> bool {
        self.raw[7] & 0x01 != 0
    }
}

/// PSX-EXE `t_addr` -> file-offset resolver (same shape as
/// [`crate::item_names`] / [`crate::item_effect`]).
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

    fn off(&self, va: u32) -> Option<usize> {
        if va < self.t_addr || va >= self.t_addr.checked_add(self.t_size)? {
            return None;
        }
        Some((va - self.t_addr) as usize + 0x800)
    }
}

/// The resolved equipment stat-bonus table: per item id the kind + bonus index,
/// plus the bonus records the equipment ids reach.
#[derive(Debug, Clone)]
pub struct EquipStatTable {
    /// `kind[id]` - item property table `+0` byte.
    kind: Vec<u8>,
    /// `bonus_index[id]` - item property table `+1` byte.
    bonus_index: Vec<u8>,
    /// Bonus records `0..=max_equipment_index`.
    bonuses: Vec<EquipBonus>,
}

impl EquipStatTable {
    /// Resolve the property + bonus tables out of a `SCUS_942.54` image.
    /// Returns `None` if the input isn't a PS-X EXE or a table falls outside
    /// the data segment.
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        let map = ExeMap::parse(scus)?;

        let mut kind = Vec::with_capacity(ITEM_COUNT);
        let mut bonus_index = Vec::with_capacity(ITEM_COUNT);
        for id in 0..ITEM_COUNT {
            let base = map.off(ITEM_TABLE_BASE_VA + (id as u32) * ITEM_RECORD_STRIDE)?;
            kind.push(*scus.get(base)?);
            bonus_index.push(*scus.get(base + 1)?);
        }

        // The bonus table only needs rows the equippable ids reach.
        let max_index = kind
            .iter()
            .zip(&bonus_index)
            .filter(|(k, _)| **k == KIND_EQUIPMENT)
            .map(|(_, i)| *i as usize)
            .max()
            .unwrap_or(0);
        let mut bonuses = Vec::with_capacity(max_index + 1);
        for row in 0..=max_index {
            let off = map.off(BONUS_TABLE_VA + (row as u32) * BONUS_STRIDE as u32)?;
            let rec = scus.get(off..off + BONUS_STRIDE)?;
            bonuses.push(EquipBonus {
                raw: rec.try_into().ok()?,
            });
        }

        Some(Self {
            kind,
            bonus_index,
            bonuses,
        })
    }

    /// `true` if the item id is an equippable item (`kind == 1`).
    pub fn is_equipment(&self, id: u8) -> bool {
        self.kind[id as usize] == KIND_EQUIPMENT
    }

    /// The bonus record for an equippable item id, or `None` if the id isn't
    /// equipment or its index is past the parsed rows.
    pub fn bonus(&self, id: u8) -> Option<EquipBonus> {
        if !self.is_equipment(id) {
            return None;
        }
        self.bonuses
            .get(self.bonus_index[id as usize] as usize)
            .copied()
    }

    /// Number of bonus records parsed.
    pub fn record_count(&self) -> usize {
        self.bonuses.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_and_ra_seru_decode_from_byte_7() {
        let mk = |b7| EquipBonus {
            raw: [0, 0, 0, 0, 0, 0x40, 1, b7],
        };
        assert_eq!(mk(0x40).slot(), EquipSlot::Weapon);
        assert_eq!(mk(0x00).slot(), EquipSlot::Body);
        assert_eq!(mk(0x20).slot(), EquipSlot::Head);
        assert_eq!(mk(0x60).slot(), EquipSlot::Footwear);
        assert!(!mk(0x40).is_ra_seru());
        assert!(mk(0x41).is_ra_seru());
        assert_eq!(mk(0x41).slot(), EquipSlot::Weapon);
    }

    #[test]
    fn equip_mask_bits() {
        let any = EquipBonus {
            raw: [0, 6, 0, 0, 0, 0x40, 7, 0x40],
        };
        assert!(any.equips_mask_bit(1));
        assert!(any.equips_mask_bit(2));
        assert!(any.equips_mask_bit(4));
        let vahn = EquipBonus {
            raw: [0, 40, 0, 0, 0, 0x40, 1, 0x40],
        };
        assert!(vahn.equips_mask_bit(1));
        assert!(!vahn.equips_mask_bit(2));
        assert_eq!(vahn.attack(), 40);
    }
}

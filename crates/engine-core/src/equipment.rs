//! Equipment catalog: typed slot model + vanilla item table.
//!
//! Mirrors the [`crate::items::ItemCatalog`] shape but for equippable
//! gear. The retail engine uses 8 equipment slots per character; this
//! module enumerates them, maps each to an [`EquipSlot`] kind, and
//! ships a vanilla catalog covering the early-game roster.
//!
//! Each entry produces a [`crate::battle_stats::ItemModifier`] that the
//! aggregator (`compute_battle_stats`) reads on commit. Engines build
//! the catalog at startup and pass it into [`EquipmentSession`] /
//! [`crate::equip_session::EquipSession`] for the player UI.
//!
//! The vanilla catalog is a clean-room reconstruction approximating the
//! retail values; the actual numeric stats live in the equipment table
//! that the level_up overlay reads (still partially overlay-blocked).
//! Engines that care about exact retail values can override per-id via
//! [`EquipmentCatalog::set`].

use crate::battle_stats::{EquipmentTable, ItemModifier};

/// The 8 equipment slot kinds. Matches the retail `equip[8]` byte array
/// at `+0x196` in the character record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EquipSlot {
    Weapon = 0,
    Helmet = 1,
    BodyArmor = 2,
    HandGuard = 3,
    Boot = 4,
    Ring1 = 5,
    Ring2 = 6,
    Accessory = 7,
}

impl EquipSlot {
    pub fn from_index(idx: u8) -> Option<Self> {
        match idx {
            0 => Some(EquipSlot::Weapon),
            1 => Some(EquipSlot::Helmet),
            2 => Some(EquipSlot::BodyArmor),
            3 => Some(EquipSlot::HandGuard),
            4 => Some(EquipSlot::Boot),
            5 => Some(EquipSlot::Ring1),
            6 => Some(EquipSlot::Ring2),
            7 => Some(EquipSlot::Accessory),
            _ => None,
        }
    }

    pub fn as_index(self) -> u8 {
        self as u8
    }

    pub fn label(self) -> &'static str {
        match self {
            EquipSlot::Weapon => "Weapon",
            EquipSlot::Helmet => "Helmet",
            EquipSlot::BodyArmor => "Body Armor",
            EquipSlot::HandGuard => "Hand Guard",
            EquipSlot::Boot => "Boots",
            EquipSlot::Ring1 => "Ring 1",
            EquipSlot::Ring2 => "Ring 2",
            EquipSlot::Accessory => "Accessory",
        }
    }
}

/// Per-character equip restriction. Some weapons are character-locked
/// (Vahn-only swords, Noa-only knuckles, Gala-only quarterstaves).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharRestriction {
    /// Any character may equip.
    Any,
    /// Only this character (slot index in the active party).
    Only(u8),
}

/// One equipable item.
#[derive(Debug, Clone, Copy)]
pub struct EquipmentEntry {
    pub id: u8,
    pub name: &'static str,
    pub slot: EquipSlot,
    pub restriction: CharRestriction,
    /// Stat modifier applied on commit. Aggregator sums these from
    /// every equipped slot.
    pub modifier: ItemModifier,
    /// Buy price (for shop integration). 0 = unsellable / quest item.
    pub buy_price: u32,
    /// Sell price (50% of buy by retail convention; 0 = unsellable).
    pub sell_price: u32,
}

/// Catalog mapping equipment id → [`EquipmentEntry`].
#[derive(Debug, Default, Clone)]
pub struct EquipmentCatalog {
    by_id: std::collections::HashMap<u8, EquipmentEntry>,
}

impl EquipmentCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, e: EquipmentEntry) {
        self.by_id.insert(e.id, e);
    }

    pub fn get(&self, id: u8) -> Option<&EquipmentEntry> {
        self.by_id.get(&id)
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &EquipmentEntry> {
        self.by_id.values()
    }

    /// Iterator filtered to a slot.
    pub fn for_slot(&self, slot: EquipSlot) -> impl Iterator<Item = &EquipmentEntry> {
        self.by_id.values().filter(move |e| e.slot == slot)
    }

    /// Iterator filtered to entries equippable by `char_slot`.
    pub fn for_char(&self, char_slot: u8) -> impl Iterator<Item = &EquipmentEntry> {
        self.by_id.values().filter(move |e| match e.restriction {
            CharRestriction::Any => true,
            CharRestriction::Only(s) => s == char_slot,
        })
    }

    /// Build an [`EquipmentTable`] from this catalog. Useful for engines
    /// that pass [`compute_battle_stats`] separately from the catalog.
    pub fn to_modifier_table(&self) -> EquipmentTable {
        let mut table = EquipmentTable::new();
        for e in self.iter() {
            table.set(e.id, e.modifier);
        }
        table
    }
}

/// Build a disc-accurate equipment modifier table keyed by **real** retail item
/// ids from the static equipment stat-bonus table
/// ([`legaia_asset::equip_stats`], `DAT_80074F68`). Each equippable id's
/// attack / def-up / def-down bytes (byte-exact vs the curated gamedata) map
/// onto an [`ItemModifier`].
///
/// This is the real-id counterpart to [`vanilla_equipment_catalog`]'s
/// [`EquipmentCatalog::to_modifier_table`]: the vanilla catalog keys by
/// fabricated ids (e.g. `0x20` "Bronze Sword") that collide with the real item
/// id space (`0x20` is the Mace), so a save holding a real equipped id pulls a
/// wrong / empty modifier from it. Built from the user's `SCUS_942.54`, this
/// table indexes by the same ids a character record's `equip[8]` actually
/// carries.
///
/// All five equipment-fed stats are mapped: attack (`+1`), def-up (`+2`),
/// def-down (`+3`), plus the head-gear `+0` byte (INT) and footwear `+4` byte
/// (SPD), pinned via the equip-screen aggregator's record-offset preload
/// (`FUN_801CF5D0`; see `legaia_asset::equip_stats`). Accuracy / evasion are
/// derived from AGL and are not equipment-fed, so they are absent here.
pub fn equip_modifier_table_from_disc(
    table: &legaia_asset::equip_stats::EquipStatTable,
) -> EquipmentTable {
    let mut out = EquipmentTable::new();
    for id in 0u8..=u8::MAX {
        if let Some(b) = table.bonus(id) {
            out.set(
                id,
                ItemModifier {
                    atk: b.attack() as i16,
                    udf: b.def_up() as i16,
                    ldf: b.def_down() as i16,
                    spd: b.spd_up() as i16,
                    int: b.int_up() as i16,
                    ability_bits: [0; 32],
                },
            );
        }
    }
    out
}

/// Disc-pinned per-item equip restrictions, built from the static equipment
/// stat-bonus table (`DAT_80074F68`, [`legaia_asset::equip_stats`]).
///
/// Two facts the equip UI needs that the modifier-only [`EquipmentTable`] view
/// drops: **which characters** may equip an item (the `+6` character mask) and
/// the item's **slot category** (the `+7` byte). The retail equip screen gates
/// each character's item list on the mask (`equip_mask & (1 << char_index)`);
/// this is the disc-accurate replacement for the engine's placeholder
/// `id >> 5` slot rule + previously-missing character gate.
///
/// Only the four disc slot *categories* (weapon / body / head / footwear) are
/// pinned - the `+7` byte does not distinguish helmet vs. ring vs. accessory
/// (all read as "head"), so this table cannot fully drive the engine's 8-slot
/// model. [`DiscEquipInfo::category`] returns the disc category; the equip
/// session maps it onto the four unambiguous UI slots and falls back to the
/// mask-only gate for the ambiguous head/hand slots.
#[derive(Debug, Clone, Default)]
pub struct DiscEquipInfo {
    entries: std::collections::HashMap<u8, DiscEquipEntry>,
}

/// One item's disc-pinned equip restriction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscEquipEntry {
    /// `+6` character mask (`1` Vahn/Meta, `2` Noa/Terra, `4` Gala/Ozma; `7` any).
    pub mask: u8,
    /// `+7 & 0x60` disc slot category.
    pub category: legaia_asset::equip_stats::EquipSlot,
    /// `+7 & 0x01` Ra-Seru (story upgrade) flag.
    pub is_ra_seru: bool,
}

impl DiscEquipInfo {
    /// Build from the parsed equipment stat-bonus table. Indexes every
    /// equippable item id (`kind == 1`) that resolves to a bonus record.
    pub fn from_disc(table: &legaia_asset::equip_stats::EquipStatTable) -> Self {
        let mut entries = std::collections::HashMap::new();
        for id in 0u8..=u8::MAX {
            if let Some(b) = table.bonus(id) {
                entries.insert(
                    id,
                    DiscEquipEntry {
                        mask: b.equip_mask(),
                        category: b.slot(),
                        is_ra_seru: b.is_ra_seru(),
                    },
                );
            }
        }
        Self { entries }
    }

    /// Build from explicit `(id, entry)` pairs. Useful for engines that
    /// source restrictions from somewhere other than the static table, and
    /// for tests.
    pub fn from_entries(entries: impl IntoIterator<Item = (u8, DiscEquipEntry)>) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
    }

    /// `true` if `id` is a known equippable item.
    pub fn is_equipment(&self, id: u8) -> bool {
        self.entries.contains_key(&id)
    }

    /// The disc restriction record for `id`, if it is equipment.
    pub fn entry(&self, id: u8) -> Option<DiscEquipEntry> {
        self.entries.get(&id).copied()
    }

    /// The disc slot category for `id`, if it is equipment.
    pub fn category(&self, id: u8) -> Option<legaia_asset::equip_stats::EquipSlot> {
        self.entries.get(&id).map(|e| e.category)
    }

    /// `true` if the party member in `party_slot` (`0` Vahn, `1` Noa, `2` Gala)
    /// may equip `id`. Returns `false` for non-equipment ids.
    pub fn can_equip(&self, id: u8, party_slot: u8) -> bool {
        match self.entries.get(&id) {
            Some(e) => party_slot < 3 && (e.mask & (1 << party_slot)) != 0,
            None => false,
        }
    }

    /// Number of equippable ids indexed.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` if no equippable ids were indexed.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Map an engine [`EquipSlot`] (8-slot UI model) to the disc slot category it
/// unambiguously corresponds to, or `None` for the slots the disc `+7` byte
/// cannot distinguish (hand guard + rings + accessory all collapse to the
/// "head" category, and helmet shares it). The four mapped slots get
/// disc-category filtering in the equip session; the rest fall back to a
/// mask-only gate. See [`DiscEquipInfo`] for the disambiguation limitation.
pub fn engine_slot_disc_category(slot: EquipSlot) -> Option<legaia_asset::equip_stats::EquipSlot> {
    use legaia_asset::equip_stats::EquipSlot as Disc;
    match slot {
        EquipSlot::Weapon => Some(Disc::Weapon),
        EquipSlot::BodyArmor => Some(Disc::Body),
        EquipSlot::Helmet => Some(Disc::Head),
        EquipSlot::Boot => Some(Disc::Footwear),
        // Hand guard, both rings, and accessory are not separable from the
        // disc "head" category, so they are not category-filtered.
        EquipSlot::HandGuard | EquipSlot::Ring1 | EquipSlot::Ring2 | EquipSlot::Accessory => None,
    }
}

const VAHN: u8 = 0;
const NOA: u8 = 1;
const GALA: u8 = 2;

fn mk_modifier(atk: i16, udf: i16, ldf: i16, spd: i16, int: i16) -> ItemModifier {
    ItemModifier {
        atk,
        udf,
        ldf,
        spd,
        int,
        ability_bits: [0; 32],
    }
}

fn mk_modifier_with_ability(
    atk: i16,
    udf: i16,
    ldf: i16,
    spd: i16,
    int: i16,
    ability_bit: u16,
) -> ItemModifier {
    let mut bits = [0u8; 32];
    let byte = (ability_bit / 8) as usize;
    let bit = (ability_bit & 7) as u8;
    if byte < 32 {
        bits[byte] |= 1 << bit;
    }
    ItemModifier {
        atk,
        udf,
        ldf,
        spd,
        int,
        ability_bits: bits,
    }
}

/// Vanilla equipment catalog. Approximates the retail Legaia roster with
/// **fabricated ids** (e.g. `0x20` "Bronze Sword") and "feels right" stat
/// tiers - kept as a disc-free fallback / UI scaffold. The real per-equip
/// attack / def values are the static `SCUS_942.54` table (`DAT_80074F68`,
/// [`legaia_asset::equip_stats`]); [`equip_modifier_table_from_disc`] builds a
/// disc-accurate modifier table keyed by the real item ids, which the boot path
/// prefers when the executable is readable.
pub fn vanilla_equipment_catalog() -> EquipmentCatalog {
    let mut c = EquipmentCatalog::new();

    // ===== Weapons (slot 0) =====
    // Vahn - swords
    c.insert(EquipmentEntry {
        id: 0x20,
        name: "Bronze Sword",
        slot: EquipSlot::Weapon,
        restriction: CharRestriction::Only(VAHN),
        modifier: mk_modifier(10, 0, 0, 0, 0),
        buy_price: 100,
        sell_price: 50,
    });
    c.insert(EquipmentEntry {
        id: 0x21,
        name: "Iron Sword",
        slot: EquipSlot::Weapon,
        restriction: CharRestriction::Only(VAHN),
        modifier: mk_modifier(20, 0, 0, 1, 0),
        buy_price: 250,
        sell_price: 125,
    });
    c.insert(EquipmentEntry {
        id: 0x22,
        name: "Steel Sword",
        slot: EquipSlot::Weapon,
        restriction: CharRestriction::Only(VAHN),
        modifier: mk_modifier(35, 0, 0, 2, 0),
        buy_price: 600,
        sell_price: 300,
    });
    c.insert(EquipmentEntry {
        id: 0x23,
        name: "Mythril Sword",
        slot: EquipSlot::Weapon,
        restriction: CharRestriction::Only(VAHN),
        modifier: mk_modifier(50, 2, 0, 3, 0),
        buy_price: 1500,
        sell_price: 750,
    });

    // Noa - knuckles
    c.insert(EquipmentEntry {
        id: 0x24,
        name: "Bronze Knuckles",
        slot: EquipSlot::Weapon,
        restriction: CharRestriction::Only(NOA),
        modifier: mk_modifier(8, 0, 0, 1, 1),
        buy_price: 90,
        sell_price: 45,
    });
    c.insert(EquipmentEntry {
        id: 0x25,
        name: "Iron Knuckles",
        slot: EquipSlot::Weapon,
        restriction: CharRestriction::Only(NOA),
        modifier: mk_modifier(18, 0, 0, 2, 1),
        buy_price: 230,
        sell_price: 115,
    });
    c.insert(EquipmentEntry {
        id: 0x26,
        name: "Steel Knuckles",
        slot: EquipSlot::Weapon,
        restriction: CharRestriction::Only(NOA),
        modifier: mk_modifier(32, 0, 0, 3, 2),
        buy_price: 580,
        sell_price: 290,
    });

    // Gala - quarterstaves
    c.insert(EquipmentEntry {
        id: 0x27,
        name: "Wooden Staff",
        slot: EquipSlot::Weapon,
        restriction: CharRestriction::Only(GALA),
        modifier: mk_modifier(12, 1, 0, 0, 0),
        buy_price: 110,
        sell_price: 55,
    });
    c.insert(EquipmentEntry {
        id: 0x28,
        name: "Iron Staff",
        slot: EquipSlot::Weapon,
        restriction: CharRestriction::Only(GALA),
        modifier: mk_modifier(22, 2, 0, 1, 0),
        buy_price: 260,
        sell_price: 130,
    });
    c.insert(EquipmentEntry {
        id: 0x29,
        name: "Steel Staff",
        slot: EquipSlot::Weapon,
        restriction: CharRestriction::Only(GALA),
        modifier: mk_modifier(38, 3, 0, 2, 0),
        buy_price: 620,
        sell_price: 310,
    });

    // ===== Helmets (slot 1) =====
    c.insert(EquipmentEntry {
        id: 0x40,
        name: "Cloth Cap",
        slot: EquipSlot::Helmet,
        restriction: CharRestriction::Any,
        modifier: mk_modifier(0, 2, 0, 0, 0),
        buy_price: 80,
        sell_price: 40,
    });
    c.insert(EquipmentEntry {
        id: 0x41,
        name: "Leather Helm",
        slot: EquipSlot::Helmet,
        restriction: CharRestriction::Any,
        modifier: mk_modifier(0, 5, 0, 0, 0),
        buy_price: 200,
        sell_price: 100,
    });
    c.insert(EquipmentEntry {
        id: 0x42,
        name: "Iron Helm",
        slot: EquipSlot::Helmet,
        restriction: CharRestriction::Any,
        modifier: mk_modifier(0, 10, 0, 0, -1),
        buy_price: 500,
        sell_price: 250,
    });
    c.insert(EquipmentEntry {
        id: 0x43,
        name: "Mythril Helm",
        slot: EquipSlot::Helmet,
        restriction: CharRestriction::Any,
        modifier: mk_modifier(0, 18, 0, 0, 0),
        buy_price: 1200,
        sell_price: 600,
    });

    // ===== Body armor (slot 2) =====
    c.insert(EquipmentEntry {
        id: 0x60,
        name: "Cloth Robe",
        slot: EquipSlot::BodyArmor,
        restriction: CharRestriction::Any,
        modifier: mk_modifier(0, 0, 3, 0, 1),
        buy_price: 100,
        sell_price: 50,
    });
    c.insert(EquipmentEntry {
        id: 0x61,
        name: "Leather Vest",
        slot: EquipSlot::BodyArmor,
        restriction: CharRestriction::Any,
        modifier: mk_modifier(0, 0, 7, 0, 0),
        buy_price: 240,
        sell_price: 120,
    });
    c.insert(EquipmentEntry {
        id: 0x62,
        name: "Chain Mail",
        slot: EquipSlot::BodyArmor,
        restriction: CharRestriction::Any,
        modifier: mk_modifier(0, 0, 15, 0, -2),
        buy_price: 600,
        sell_price: 300,
    });
    c.insert(EquipmentEntry {
        id: 0x63,
        name: "Plate Mail",
        slot: EquipSlot::BodyArmor,
        restriction: CharRestriction::Any,
        modifier: mk_modifier(0, 0, 25, 0, -4),
        buy_price: 1400,
        sell_price: 700,
    });

    // ===== Hand guards (slot 3) =====
    c.insert(EquipmentEntry {
        id: 0x80,
        name: "Cloth Wrap",
        slot: EquipSlot::HandGuard,
        restriction: CharRestriction::Any,
        modifier: mk_modifier(0, 1, 1, 0, 0),
        buy_price: 60,
        sell_price: 30,
    });
    c.insert(EquipmentEntry {
        id: 0x81,
        name: "Leather Bracelet",
        slot: EquipSlot::HandGuard,
        restriction: CharRestriction::Any,
        modifier: mk_modifier(0, 3, 3, 0, 1),
        buy_price: 180,
        sell_price: 90,
    });
    c.insert(EquipmentEntry {
        id: 0x82,
        name: "Iron Gauntlets",
        slot: EquipSlot::HandGuard,
        restriction: CharRestriction::Any,
        modifier: mk_modifier(1, 6, 6, 0, -1),
        buy_price: 460,
        sell_price: 230,
    });

    // ===== Boots (slot 4) =====
    c.insert(EquipmentEntry {
        id: 0xA0,
        name: "Cloth Shoes",
        slot: EquipSlot::Boot,
        restriction: CharRestriction::Any,
        modifier: mk_modifier(0, 0, 1, 2, 0),
        buy_price: 60,
        sell_price: 30,
    });
    c.insert(EquipmentEntry {
        id: 0xA1,
        name: "Leather Boots",
        slot: EquipSlot::Boot,
        restriction: CharRestriction::Any,
        modifier: mk_modifier(0, 0, 3, 4, 0),
        buy_price: 200,
        sell_price: 100,
    });
    c.insert(EquipmentEntry {
        id: 0xA2,
        name: "Iron Boots",
        slot: EquipSlot::Boot,
        restriction: CharRestriction::Any,
        modifier: mk_modifier(0, 0, 7, 2, 0),
        buy_price: 480,
        sell_price: 240,
    });
    c.insert(EquipmentEntry {
        id: 0xA3,
        name: "Wind Boots",
        slot: EquipSlot::Boot,
        restriction: CharRestriction::Any,
        // Ability bit 12 = "evasion bonus" - mirrors retail's hidden flag.
        modifier: mk_modifier_with_ability(0, 0, 5, 2, 0, 12),
        buy_price: 1100,
        sell_price: 550,
    });

    // ===== Rings (slots 5/6) =====
    c.insert(EquipmentEntry {
        id: 0xC0,
        name: "Power Ring",
        slot: EquipSlot::Ring1,
        restriction: CharRestriction::Any,
        modifier: mk_modifier(5, 0, 0, 0, 0),
        buy_price: 800,
        sell_price: 400,
    });
    c.insert(EquipmentEntry {
        id: 0xC1,
        name: "Defense Ring",
        slot: EquipSlot::Ring1,
        restriction: CharRestriction::Any,
        modifier: mk_modifier(0, 4, 4, 0, 0),
        buy_price: 800,
        sell_price: 400,
    });
    c.insert(EquipmentEntry {
        id: 0xC2,
        name: "Speed Ring",
        slot: EquipSlot::Ring2,
        restriction: CharRestriction::Any,
        modifier: mk_modifier(0, 0, 0, 0, 5),
        buy_price: 1000,
        sell_price: 500,
    });
    c.insert(EquipmentEntry {
        id: 0xC3,
        name: "Hit Ring",
        slot: EquipSlot::Ring2,
        restriction: CharRestriction::Any,
        modifier: mk_modifier(0, 0, 0, 5, 0),
        buy_price: 1000,
        sell_price: 500,
    });

    // ===== Accessories (slot 7) =====
    c.insert(EquipmentEntry {
        id: 0xE0,
        name: "Goblin Foot",
        slot: EquipSlot::Accessory,
        restriction: CharRestriction::Any,
        // Ability bit 4 = "encounter rate down" (a synthetic catalog bit;
        // the retail encounter modifiers are the High/Low Encounter passives
        // 0x3B/0x3C consumed by World::encounter_rate_modifiers).
        modifier: mk_modifier_with_ability(0, 0, 0, 0, 0, 4),
        buy_price: 1500,
        sell_price: 750,
    });
    c.insert(EquipmentEntry {
        id: 0xE1,
        name: "Wisdom Ring",
        slot: EquipSlot::Accessory,
        restriction: CharRestriction::Any,
        // Ability bit 7 = "MP cost reduced".
        modifier: mk_modifier_with_ability(0, 2, 2, 0, 0, 7),
        buy_price: 2000,
        sell_price: 1000,
    });
    c.insert(EquipmentEntry {
        id: 0xE2,
        name: "Lucky Charm",
        slot: EquipSlot::Accessory,
        restriction: CharRestriction::Any,
        // Ability bit 9 = "bonus EXP".
        modifier: mk_modifier_with_ability(0, 0, 0, 1, 1, 9),
        buy_price: 2400,
        sell_price: 1200,
    });

    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_index_round_trip() {
        for i in 0..8u8 {
            let s = EquipSlot::from_index(i).unwrap();
            assert_eq!(s.as_index(), i);
        }
        assert!(EquipSlot::from_index(8).is_none());
    }

    #[test]
    fn slot_labels_unique() {
        let labels: Vec<&str> = (0..8u8)
            .map(|i| EquipSlot::from_index(i).unwrap().label())
            .collect();
        let unique: std::collections::HashSet<&&str> = labels.iter().collect();
        assert_eq!(unique.len(), labels.len());
    }

    #[test]
    fn vanilla_catalog_non_empty() {
        let c = vanilla_equipment_catalog();
        assert!(c.len() >= 25);
    }

    #[test]
    fn vanilla_catalog_per_slot_non_empty() {
        let c = vanilla_equipment_catalog();
        for slot in [
            EquipSlot::Weapon,
            EquipSlot::Helmet,
            EquipSlot::BodyArmor,
            EquipSlot::HandGuard,
            EquipSlot::Boot,
            EquipSlot::Ring1,
            EquipSlot::Ring2,
            EquipSlot::Accessory,
        ] {
            let count = c.for_slot(slot).count();
            assert!(count > 0, "no entries for {:?}", slot);
        }
    }

    #[test]
    fn vahn_can_equip_swords_only() {
        let c = vanilla_equipment_catalog();
        let vahn_weapons: Vec<_> = c
            .for_char(VAHN)
            .filter(|e| e.slot == EquipSlot::Weapon)
            .collect();
        // Vahn-only swords + any-character weapons (none in this catalog).
        assert!(vahn_weapons.iter().all(|e| e.name.contains("Sword")));
        assert!(vahn_weapons.len() >= 4);
    }

    #[test]
    fn noa_can_equip_knuckles_only() {
        let c = vanilla_equipment_catalog();
        let noa_weapons: Vec<_> = c
            .for_char(NOA)
            .filter(|e| e.slot == EquipSlot::Weapon)
            .collect();
        assert!(noa_weapons.iter().all(|e| e.name.contains("Knuckles")));
    }

    #[test]
    fn shared_armor_visible_to_all() {
        let c = vanilla_equipment_catalog();
        for char_slot in 0..3u8 {
            let count = c
                .for_char(char_slot)
                .filter(|e| e.slot == EquipSlot::Helmet)
                .count();
            assert!(count >= 4, "{char_slot} sees too few helmets ({count})");
        }
    }

    #[test]
    fn modifier_table_has_every_entry() {
        let c = vanilla_equipment_catalog();
        let table = c.to_modifier_table();
        for entry in c.iter() {
            assert!(
                table.get(entry.id).is_some(),
                "missing in modifier table: {:#04x}",
                entry.id
            );
        }
    }

    #[test]
    fn ability_bit_packed_correctly() {
        let m = mk_modifier_with_ability(0, 0, 0, 0, 0, 12);
        // Bit 12 => byte 1, bit 4 => 0x10.
        assert_eq!(m.ability_bits[1], 0x10);
        assert_eq!(m.ability_bits[0], 0x00);
    }

    #[test]
    fn disc_equip_info_gates_by_mask_and_category() {
        use legaia_asset::equip_stats::EquipSlot as Disc;
        let info = DiscEquipInfo::from_entries([
            (
                0x20,
                DiscEquipEntry {
                    mask: 1,
                    category: Disc::Weapon,
                    is_ra_seru: false,
                },
            ),
            (
                0xC0,
                DiscEquipEntry {
                    mask: 7,
                    category: Disc::Head,
                    is_ra_seru: false,
                },
            ),
        ]);
        assert!(info.is_equipment(0x20));
        assert!(!info.is_equipment(0x21));
        // Vahn-only weapon.
        assert!(info.can_equip(0x20, 0));
        assert!(!info.can_equip(0x20, 1));
        assert!(!info.can_equip(0x20, 2));
        // Universal accessory.
        assert!(info.can_equip(0xC0, 0));
        assert!(info.can_equip(0xC0, 2));
        // Non-equipment id never equips.
        assert!(!info.can_equip(0x21, 0));
        assert_eq!(info.category(0x20), Some(Disc::Weapon));
        assert_eq!(info.category(0xC0), Some(Disc::Head));
        assert_eq!(info.category(0x21), None);
        assert_eq!(info.len(), 2);
    }

    #[test]
    fn engine_slot_disc_category_maps_only_unambiguous_slots() {
        use legaia_asset::equip_stats::EquipSlot as Disc;
        assert_eq!(
            engine_slot_disc_category(EquipSlot::Weapon),
            Some(Disc::Weapon)
        );
        assert_eq!(
            engine_slot_disc_category(EquipSlot::BodyArmor),
            Some(Disc::Body)
        );
        assert_eq!(
            engine_slot_disc_category(EquipSlot::Helmet),
            Some(Disc::Head)
        );
        assert_eq!(
            engine_slot_disc_category(EquipSlot::Boot),
            Some(Disc::Footwear)
        );
        // Ambiguous slots (the disc +7 byte can't separate them).
        assert_eq!(engine_slot_disc_category(EquipSlot::HandGuard), None);
        assert_eq!(engine_slot_disc_category(EquipSlot::Ring1), None);
        assert_eq!(engine_slot_disc_category(EquipSlot::Ring2), None);
        assert_eq!(engine_slot_disc_category(EquipSlot::Accessory), None);
    }

    #[test]
    fn buy_sell_consistent() {
        let c = vanilla_equipment_catalog();
        for entry in c.iter() {
            // Sell is exactly 50% of buy by retail convention.
            assert_eq!(
                entry.sell_price * 2,
                entry.buy_price,
                "{} buy/sell mismatch",
                entry.name
            );
        }
    }

    #[test]
    fn power_ring_atk_bonus_resolves() {
        let c = vanilla_equipment_catalog();
        let ring = c.get(0xC0).unwrap();
        assert_eq!(ring.modifier.atk, 5);
    }

    #[test]
    fn wind_boots_grant_speed_and_evasion_bit() {
        let c = vanilla_equipment_catalog();
        let boots = c.get(0xA3).unwrap();
        // Footwear boosts SPD; the evasion flavour is the hidden ability bit.
        assert!(boots.modifier.spd > 0);
        // Ability bit 12 set (evasion flag).
        assert_ne!(boots.modifier.ability_bits[1] & 0x10, 0);
    }
}

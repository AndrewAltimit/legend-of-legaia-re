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

const VAHN: u8 = 0;
const NOA: u8 = 1;
const GALA: u8 = 2;

fn mk_modifier(atk: i16, udf: i16, ldf: i16, acc: i16, eva: i16) -> ItemModifier {
    ItemModifier {
        atk,
        udf,
        ldf,
        acc,
        eva,
        ability_bits: [0; 32],
    }
}

fn mk_modifier_with_ability(
    atk: i16,
    udf: i16,
    ldf: i16,
    acc: i16,
    eva: i16,
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
        acc,
        eva,
        ability_bits: bits,
    }
}

/// Vanilla equipment catalog. Approximates the retail Legaia roster.
/// Numeric values are rounded to the nearest "feels right" tier; the
/// actual retail equipment table still requires the level_up overlay
/// trace for exact values (see `docs/subsystems/levelup.md`).
pub fn vanilla_equipment_catalog() -> EquipmentCatalog {
    let mut c = EquipmentCatalog::new();

    // ===== Weapons (slot 0) =====
    // Vahn — swords
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

    // Noa — knuckles
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

    // Gala — quarterstaves
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
        modifier: mk_modifier(0, 0, 1, 0, 2),
        buy_price: 60,
        sell_price: 30,
    });
    c.insert(EquipmentEntry {
        id: 0xA1,
        name: "Leather Boots",
        slot: EquipSlot::Boot,
        restriction: CharRestriction::Any,
        modifier: mk_modifier(0, 0, 3, 0, 4),
        buy_price: 200,
        sell_price: 100,
    });
    c.insert(EquipmentEntry {
        id: 0xA2,
        name: "Iron Boots",
        slot: EquipSlot::Boot,
        restriction: CharRestriction::Any,
        modifier: mk_modifier(0, 0, 7, 0, 2),
        buy_price: 480,
        sell_price: 240,
    });
    c.insert(EquipmentEntry {
        id: 0xA3,
        name: "Wind Boots",
        slot: EquipSlot::Boot,
        restriction: CharRestriction::Any,
        // Ability bit 12 = "evasion bonus" — mirrors retail's hidden flag.
        modifier: mk_modifier_with_ability(0, 0, 5, 2, 8, 12),
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
        // Ability bit 4 = "encounter rate down" (engine reads this in
        // EncounterTracker::add_rate_bias).
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
    fn wind_boots_grant_evasion_bit() {
        let c = vanilla_equipment_catalog();
        let boots = c.get(0xA3).unwrap();
        assert!(boots.modifier.eva > 0);
        // Ability bit 12 set.
        assert_ne!(boots.modifier.ability_bits[1] & 0x10, 0);
    }
}

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

/// PORT: FUN_800430AC
///
/// Party-wide **accessory unequip-by-id**. Walks the active party in
/// roster order (retail: member ids at `0x80084598+`, count at
/// `DAT_80084594`, records at `0x80084708 + id * 0x414`) and scans each
/// record's three accessory ("Goods") slots - equipment indices `5..=7`,
/// record offsets `+0x19B..0x19D` (Ring 1 / Ring 2 / Accessory). On the
/// **first** slot whose byte equals `item_id` it zeroes the slot and
/// stops. Returns `true` when a slot was cleared (retail return `0`) and
/// `false` when no member carries the id (retail `0x100`).
///
/// Faithful edge: retail compares the raw slot byte, so `item_id == 0`
/// matches the first *empty* accessory slot and still reports success.
///
/// NOT WIRED: no live engine path strips an accessory party-wide yet
/// (retail reaches this from overlay-resident event/menu flows); the
/// kernel is exercised by this module's tests.
pub fn party_unequip_accessory_by_id(party: &mut legaia_save::Party, item_id: u8) -> bool {
    for member in &mut party.members {
        let mut eq = member.equipment();
        for slot in 5..8usize {
            if eq.slots[slot] == item_id {
                eq.slots[slot] = 0;
                member.set_equipment(eq);
                return true;
            }
        }
    }
    false
}

/// PORT: FUN_800302E4
///
/// Equipment **stat-field accessor**. Given an item `id` (whose high nibble
/// `id & 0xF000` tags its id space) and a `field` selector, resolves the id
/// to an 8-byte equip stat-bonus record (`DAT_80074F68`,
/// [`legaia_asset::equip_stats`]) and returns the requested stat combination.
///
/// Id-space tags (`id & 0xF000`):
/// - `0x1000` / `0x6000` / `0x9000`: `id & 0x3FF` is an **inventory-slot
///   index**; the real item id is read from the live inventory-slot block
///   (retail `0x80085958 + (id & 0x3FF) * 2`), supplied by `inventory_slot`.
/// - `0x7000`: `id & 0x3FF` is a **direct** item id.
/// - any other tag: returns `0`.
///
/// The resolved item id selects the equip record via `equip_bonus` (retail's
/// un-gated `item_table[id].byte(+1)` -> `DAT_80074F68` double-indirect; a
/// caller typically passes `|id| table.bonus(id as u8)`). `field` picks the
/// value returned:
/// - `0`: def-up (`+2`) + def-down (`+3`) — total defence
/// - `1`: def-up (`+2`)
/// - `2`: attack (`+1`) + `agility_term(resolved_id)` — where `agility_term`
///   is retail's overlay-resident `FUN_801DD0C0(_, id, 0)`, injected by the
///   caller because it needs runtime context absent from this kernel
/// - `3`: attack (`+1`) + def-down (`+3`)
/// - any other field: `0`
///
/// Returns `0` when the tag is unrecognised, the resolved id has no equip
/// record, or the field is out of range (retail falls through to the shared
/// epilogue returning `0` in each case). Note `param_1` (the retail first
/// arg) is dead except as the passthrough to `FUN_801DD0C0`, so it is folded
/// into the caller's `agility_term` closure rather than taken here.
///
/// NOT WIRED: its `id` argument is a **class-tagged row-entry word**
/// (`0x1000` / `0x6000` / `0x7000` / `0x9000`), the same id space
/// [`crate::menu_list_rows`] decodes - and nothing in the engine produces
/// those words. The Equip screen's stat-compare columns are fed instead
/// from [`crate::battle_stats::compute_battle_stats`] over a trial-equipped
/// record ([`crate::equip_session::EquipSession::preview_candidate`]), which
/// needs no row word. Wiring this needs the retail list-node row model
/// disclosed on `menu_list_rows`.
pub fn equip_stat_field(
    id: u16,
    field: u8,
    inventory_slot: impl Fn(u16) -> u8,
    equip_bonus: impl Fn(u16) -> Option<legaia_asset::equip_stats::EquipBonus>,
    agility_term: impl Fn(u16) -> u32,
) -> u32 {
    let resolved = match id & 0xf000 {
        // Inventory-slot indirection: the low bits index the live slot block,
        // whose byte is the real item id (read unmasked in retail).
        0x1000 | 0x6000 | 0x9000 => inventory_slot(id & 0x3ff) as u16,
        // Direct item id.
        0x7000 => id & 0x3ff,
        _ => return 0,
    };
    let Some(b) = equip_bonus(resolved) else {
        return 0;
    };
    match field {
        0 => b.def_up() as u32 + b.def_down() as u32,
        1 => b.def_up() as u32,
        2 => b.attack() as u32 + agility_term(resolved),
        3 => b.attack() as u32 + b.def_down() as u32,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- party_unequip_accessory_by_id (FUN_800430AC) ------------------

    fn party_with_goods(goods: &[[u8; 3]]) -> legaia_save::Party {
        let mut p = legaia_save::Party::zeroed(goods.len());
        for (m, g) in p.members.iter_mut().zip(goods) {
            let mut eq = m.equipment();
            eq.slots[5..8].copy_from_slice(g);
            m.set_equipment(eq);
        }
        p
    }

    #[test]
    fn unequip_clears_first_matching_goods_slot_only() {
        // Member 0 carries 0x42 twice; only the first match clears.
        let mut p = party_with_goods(&[[0x42, 0x42, 0x11], [0x42, 0, 0]]);
        assert!(party_unequip_accessory_by_id(&mut p, 0x42));
        assert_eq!(p.members[0].equipment().slots[5..8], [0, 0x42, 0x11]);
        // Member 1 untouched (the scan stopped at member 0).
        assert_eq!(p.members[1].equipment().slots[5], 0x42);
    }

    #[test]
    fn unequip_scans_later_members_when_earlier_lack_the_id() {
        let mut p = party_with_goods(&[[0x11, 0x12, 0x13], [0, 0x99, 0]]);
        assert!(party_unequip_accessory_by_id(&mut p, 0x99));
        assert_eq!(p.members[1].equipment().slots[5..8], [0, 0, 0]);
        // Member 0 untouched.
        assert_eq!(p.members[0].equipment().slots[5..8], [0x11, 0x12, 0x13]);
    }

    #[test]
    fn unequip_misses_report_the_retail_0x100_case() {
        let mut p = party_with_goods(&[[0x11, 0x12, 0x13]]);
        assert!(!party_unequip_accessory_by_id(&mut p, 0x99));
        // Weapon/armor slots (0..5) are never scanned - an id living
        // there does not count.
        let mut eq = p.members[0].equipment();
        eq.slots[0] = 0x99;
        p.members[0].set_equipment(eq);
        assert!(!party_unequip_accessory_by_id(&mut p, 0x99));
        assert!(!party_unequip_accessory_by_id(
            &mut legaia_save::Party::zeroed(0),
            0x11
        ));
    }

    #[test]
    fn unequip_id_zero_matches_an_empty_slot_faithfully() {
        // Retail compares the raw byte, so id 0 "clears" the first empty
        // goods slot and reports success.
        let mut p = party_with_goods(&[[0x11, 0, 0x13]]);
        assert!(party_unequip_accessory_by_id(&mut p, 0));
        assert_eq!(p.members[0].equipment().slots[5..8], [0x11, 0, 0x13]);
    }

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

    // -- equip_stat_field (FUN_800302E4) -------------------------------

    use legaia_asset::equip_stats::EquipBonus;

    // raw = [INT, ATK, UDF, LDF, SPD, passive, mask, slot]
    const REC_10: EquipBonus = EquipBonus {
        raw: [0, 10, 5, 3, 0, 0x40, 7, 0x40],
    };

    // A closure resolving item id 0x30 -> REC_10, everything else missing.
    fn bonus_30(id: u16) -> Option<EquipBonus> {
        (id == 0x30).then_some(REC_10)
    }

    #[test]
    fn direct_id_selects_each_field() {
        // 0x7000-tagged direct id 0x30, no agility term.
        let zero = |_: u16| 0u32;
        let no_inv = |_: u16| 0u8;
        let f = |field| equip_stat_field(0x7030, field, no_inv, bonus_30, zero);
        assert_eq!(f(0), 5 + 3); // def-up + def-down
        assert_eq!(f(1), 5); // def-up
        assert_eq!(f(2), 10); // attack + agility(0)
        assert_eq!(f(3), 10 + 3); // attack + def-down
    }

    #[test]
    fn field_two_adds_agility_term_keyed_on_resolved_id() {
        // agility_term is applied only for field 2, and receives the resolved
        // id (0x30), not the tagged input.
        let agl = |resolved: u16| if resolved == 0x30 { 7 } else { 999 };
        let got = equip_stat_field(0x7030, 2, |_| 0, bonus_30, agl);
        assert_eq!(got, 10 + 7);
    }

    #[test]
    fn inventory_tag_resolves_through_slot_block() {
        // 0x1000/0x6000/0x9000 read the item id out of the inventory slot.
        // Slot 5 holds item id 0x30.
        let inv = |slot: u16| if slot == 5 { 0x30 } else { 0 };
        for tag in [0x1000u16, 0x6000, 0x9000] {
            let got = equip_stat_field(tag | 5, 1, inv, bonus_30, |_| 0);
            assert_eq!(got, 5, "tag {tag:#06x}");
        }
        // A slot holding an unequippable id yields 0 (no record).
        let got = equip_stat_field(0x1000 | 6, 1, inv, bonus_30, |_| 0);
        assert_eq!(got, 0);
    }

    #[test]
    fn unknown_tag_returns_zero() {
        // 0x2000 is not a recognised id space.
        let got = equip_stat_field(0x2030, 0, |_| 0x30u8, bonus_30, |_| 0);
        assert_eq!(got, 0);
    }

    #[test]
    fn missing_record_and_bad_field_return_zero() {
        // Direct id with no equip record.
        assert_eq!(equip_stat_field(0x7099, 0, |_| 0, bonus_30, |_| 0), 0);
        // Valid record but out-of-range field.
        assert_eq!(equip_stat_field(0x7030, 4, |_| 0, bonus_30, |_| 0), 0);
        assert_eq!(equip_stat_field(0x7030, 200, |_| 0, bonus_30, |_| 0), 0);
    }
}

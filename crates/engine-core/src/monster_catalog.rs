//! Monster catalog + formation tables for engine-driven encounters.
//!
//! The retail engine resolves a battle scene's monster set in two stages:
//! the encounter table picks a `formation_id`; the battle scene loader
//! reads the `battle_data` PROT entries to populate per-formation slot
//! lists with monster definitions. The retail definitions live in
//! still-uncaptured battle overlays — until they're traced, this module
//! ships a vanilla in-engine catalog so the encounter → battle path can
//! be exercised end-to-end without disc data.
//!
//! Vanilla coverage targets the early-game roster the player encounters
//! between Drake Castle and Vidna's outskirts: Goblin, Bandit, Wolf,
//! Sluggers, Skeleton, etc. Stats are scaled to give the level-1
//! starting party a 5-10 turn fight.
//!
//! ## Components
//!
//! - [`MonsterDef`] — one monster row (HP, MP, ATK, UDF, LDF, accuracy,
//!   evasion, EXP yield, gold drop, optional drop-item id).
//! - [`MonsterCatalog`] — id → [`MonsterDef`] table.
//! - [`FormationSlot`] — one occupied slot in a formation: monster id +
//!   optional level offset.
//! - [`FormationDef`] — a formation row: 1..=4 slots (battles support up
//!   to 5 enemy slots; we cap at 4 so the player slot stays distinct).
//! - [`FormationTable`] — formation_id → [`FormationDef`] map plus
//!   reverse lookup helpers.
//!
//! Pure data — no Vfs / disc / world coupling. Engines call
//! [`FormationTable::formation`] with the `formation_id` from
//! [`crate::encounter::EncounterRoll`] and feed the resulting
//! [`FormationDef`] into their battle scene loader.

use std::collections::HashMap;

/// One monster's definition (clean-room, vanilla values).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonsterDef {
    pub id: u16,
    /// Display name shown in the battle HUD.
    pub name: String,
    pub hp: u16,
    pub mp: u16,
    pub attack: u16,
    /// Upper-defense stat (used against high-power-target strikes).
    pub udf: u16,
    /// Lower-defense stat (used against low-power-target strikes).
    pub ldf: u16,
    pub accuracy: u8,
    pub evasion: u8,
    /// Experience awarded to the party on defeat.
    pub exp: u16,
    /// Gold dropped on defeat.
    pub gold: u16,
    /// Optional drop item id (`None` = no drop).
    pub drop_item: Option<u8>,
    /// `1/256` drop-rate. Engines roll one byte; if it falls below this the
    /// drop fires. `0` means never; `255` means always.
    pub drop_rate_q8: u8,
}

impl MonsterDef {
    pub fn new(id: u16, name: impl Into<String>, hp: u16, attack: u16) -> Self {
        Self {
            id,
            name: name.into(),
            hp,
            mp: 0,
            attack,
            udf: attack / 2,
            ldf: attack / 2,
            accuracy: 70,
            evasion: 10,
            exp: hp / 2,
            gold: hp / 4,
            drop_item: None,
            drop_rate_q8: 0,
        }
    }
}

/// Monster id → definition map.
#[derive(Debug, Default, Clone)]
pub struct MonsterCatalog {
    pub by_id: HashMap<u16, MonsterDef>,
}

impl MonsterCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, def: MonsterDef) {
        self.by_id.insert(def.id, def);
    }

    pub fn get(&self, id: u16) -> Option<&MonsterDef> {
        self.by_id.get(&id)
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

/// One slot in a formation row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormationSlot {
    pub monster_id: u16,
    /// Level offset applied to the monster's base stats. `0` keeps them
    /// at catalog values; positive ramps stats for late-game variants.
    pub level_offset: i8,
}

impl FormationSlot {
    pub const fn new(monster_id: u16) -> Self {
        Self {
            monster_id,
            level_offset: 0,
        }
    }

    pub const fn with_offset(monster_id: u16, level_offset: i8) -> Self {
        Self {
            monster_id,
            level_offset,
        }
    }
}

/// One formation row.
#[derive(Debug, Clone, Default)]
pub struct FormationDef {
    pub formation_id: u16,
    /// Up to 4 occupied slots. Fewer means the trailing battle slots are
    /// empty for this formation. The retail max is 5 monsters but we cap
    /// at 4 to leave one slot for a guest character or boss summon.
    pub slots: Vec<FormationSlot>,
    /// Display label for the formation (used by the encounter banner).
    /// Engines fall back to `"Encounter #N"` when this is empty.
    pub label: String,
}

impl FormationDef {
    pub fn new(formation_id: u16, slots: Vec<FormationSlot>) -> Self {
        Self {
            formation_id,
            slots,
            label: String::new(),
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }
}

/// Formation id → definition map.
#[derive(Debug, Default, Clone)]
pub struct FormationTable {
    pub by_id: HashMap<u16, FormationDef>,
}

impl FormationTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, def: FormationDef) {
        self.by_id.insert(def.formation_id, def);
    }

    pub fn formation(&self, formation_id: u16) -> Option<&FormationDef> {
        self.by_id.get(&formation_id)
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

/// Vanilla monster catalog. ~20 early-game entries scaled for level-1 to
/// level-10 parties. Stats follow a "small / medium / large" tier pattern
/// so engines can quickly verify the encounter → battle pipeline.
/// Tuple shape used by [`vanilla_monster_catalog`] for compactness.
/// `(id, name, hp, mp, attack, defense, accuracy, evasion, exp, gold)`.
type VanillaMonsterRow = (u16, &'static str, u16, u16, u16, u16, u8, u8, u16, u16);

pub fn vanilla_monster_catalog() -> MonsterCatalog {
    let mut cat = MonsterCatalog::new();
    let entries: &[VanillaMonsterRow] = &[
        // (id, name, hp, mp, attack, defense, accuracy, evasion, exp, gold)
        (1, "Goblin", 30, 0, 10, 5, 70, 10, 8, 6),
        (2, "Big Goblin", 50, 0, 14, 8, 70, 8, 14, 12),
        (3, "Wolf", 35, 0, 12, 6, 80, 18, 10, 4),
        (4, "Bandit", 60, 5, 16, 10, 75, 15, 18, 24),
        (5, "Bandit Boss", 120, 10, 24, 18, 78, 12, 60, 80),
        (6, "Skeleton", 45, 0, 13, 8, 65, 8, 12, 5),
        (7, "Killer Bee", 25, 0, 9, 4, 88, 25, 7, 3),
        (8, "Slime", 40, 5, 8, 12, 60, 5, 8, 4),
        (9, "Big Slime", 80, 10, 14, 18, 65, 5, 22, 12),
        (10, "Frog", 28, 0, 8, 5, 72, 14, 6, 4),
        (11, "Lizard Man", 55, 5, 17, 11, 76, 12, 18, 14),
        (12, "Mole", 70, 0, 19, 14, 60, 8, 22, 18),
        (13, "Spike Mole", 100, 0, 24, 20, 65, 9, 38, 30),
        (14, "Dark Crab", 90, 0, 18, 22, 64, 6, 28, 25),
        (15, "Crystal Bat", 38, 8, 11, 6, 90, 28, 12, 8),
        (16, "Berserker", 140, 0, 28, 16, 78, 14, 70, 60),
        (17, "Stone Golem", 200, 0, 22, 30, 60, 4, 100, 90),
        (18, "Sea Slug", 50, 5, 12, 14, 65, 8, 14, 9),
        (19, "Drake Wyrm", 250, 30, 32, 25, 85, 14, 180, 200),
        (20, "Goblin King", 180, 0, 26, 18, 80, 10, 90, 120),
    ];
    for &(id, name, hp, mp, atk, def, acc, eva, exp, gold) in entries {
        let def_struct = MonsterDef {
            id,
            name: name.into(),
            hp,
            mp,
            attack: atk,
            udf: def,
            ldf: def,
            accuracy: acc,
            evasion: eva,
            exp,
            gold,
            drop_item: None,
            drop_rate_q8: 0,
        };
        cat.insert(def_struct);
    }
    cat
}

/// Vanilla formation table. Maps the encounter-system `formation_id` rows
/// to monster groups, providing a default playable set for the early-game
/// scenes (`town01` outskirts, `cave01`, `road01`, `wood01`, etc.).
pub fn vanilla_formation_table() -> FormationTable {
    let mut t = FormationTable::new();
    // Single-monster encounters (early game).
    t.insert(
        FormationDef::new(1, vec![FormationSlot::new(1)]) // Goblin
            .with_label("Goblin"),
    );
    t.insert(
        FormationDef::new(2, vec![FormationSlot::new(3)]) // Wolf
            .with_label("Wolf"),
    );
    t.insert(
        FormationDef::new(3, vec![FormationSlot::new(8)]) // Slime
            .with_label("Slime"),
    );
    // Pair encounters.
    t.insert(
        FormationDef::new(
            10,
            vec![FormationSlot::new(1), FormationSlot::new(1)], // 2x Goblin
        )
        .with_label("Goblin x2"),
    );
    t.insert(
        FormationDef::new(
            11,
            vec![FormationSlot::new(7), FormationSlot::new(7)], // 2x Killer Bee
        )
        .with_label("Killer Bee x2"),
    );
    t.insert(
        FormationDef::new(
            12,
            vec![FormationSlot::new(3), FormationSlot::new(6)], // Wolf + Skeleton
        )
        .with_label("Wolf + Skeleton"),
    );
    // Triple encounters (mid-route).
    t.insert(
        FormationDef::new(
            20,
            vec![
                FormationSlot::new(1),
                FormationSlot::new(2),
                FormationSlot::new(1),
            ],
        )
        .with_label("Goblin pack"),
    );
    t.insert(
        FormationDef::new(
            21,
            vec![
                FormationSlot::new(4),
                FormationSlot::new(4),
                FormationSlot::new(11),
            ],
        )
        .with_label("Bandit ambush"),
    );
    // Cave / dungeon encounters.
    t.insert(FormationDef::new(30, vec![FormationSlot::new(12)]).with_label("Mole"));
    t.insert(
        FormationDef::new(31, vec![FormationSlot::new(13), FormationSlot::new(12)])
            .with_label("Spike Mole + Mole"),
    );
    t.insert(FormationDef::new(32, vec![FormationSlot::new(14)]).with_label("Dark Crab"));
    // Boss encounters.
    t.insert(FormationDef::new(100, vec![FormationSlot::new(5)]).with_label("Bandit Boss"));
    t.insert(FormationDef::new(101, vec![FormationSlot::new(20)]).with_label("Goblin King"));
    t.insert(FormationDef::new(102, vec![FormationSlot::new(19)]).with_label("Drake Wyrm"));
    t.insert(FormationDef::new(103, vec![FormationSlot::new(17)]).with_label("Stone Golem"));
    t
}

/// Convenience constructor: a default early-game encounter table the
/// engine can install at boot to make `town01`-area scenes triggerable
/// without disc data. Mirrors retail's "outskirts of Rim Elm" mix.
pub fn default_early_encounter_table(
    scene_label: impl Into<String>,
) -> crate::encounter::EncounterTable {
    use crate::encounter::{EncounterEntry, EncounterTable};
    let mut t = EncounterTable::new(scene_label);
    // Retail "outskirts of Rim Elm" is approximately 1 in 50-60 steps;
    // 5/256 ≈ 1 in 51, which matches without being annoying.
    t.set_trigger_rate(5);
    t.push(EncounterEntry::new(1, 50)); // Goblin (common)
    t.push(EncounterEntry::new(3, 30)); // Slime
    t.push(EncounterEntry::new(2, 15)); // Wolf
    t.push(EncounterEntry::new(10, 10)); // Goblin x2
    t.push(EncounterEntry::new(11, 5)); // Killer Bee x2
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vanilla_catalog_has_expected_entries() {
        let cat = vanilla_monster_catalog();
        assert!(cat.len() >= 20);
        let g = cat.get(1).expect("Goblin");
        assert_eq!(g.name, "Goblin");
        assert!(g.hp > 0 && g.attack > 0);
        let king = cat.get(20).expect("Goblin King");
        assert!(king.hp >= 100); // boss tier
    }

    #[test]
    fn vanilla_formation_table_covers_basics() {
        let t = vanilla_formation_table();
        let f1 = t.formation(1).expect("formation 1");
        assert_eq!(f1.slots.len(), 1);
        assert_eq!(f1.slots[0].monster_id, 1);
        let f10 = t.formation(10).expect("formation 10");
        assert_eq!(f10.slots.len(), 2);
        let boss = t.formation(100).expect("boss");
        assert_eq!(boss.slots.len(), 1);
        assert_eq!(boss.slots[0].monster_id, 5); // Bandit Boss
    }

    #[test]
    fn formation_label_fallback() {
        let f = FormationDef::new(99, vec![FormationSlot::new(1)]);
        assert!(f.label.is_empty());
        let f = f.with_label("Test");
        assert_eq!(f.label, "Test");
    }

    #[test]
    fn formation_slot_with_offset() {
        let s = FormationSlot::with_offset(5, 3);
        assert_eq!(s.monster_id, 5);
        assert_eq!(s.level_offset, 3);
        let s2 = FormationSlot::new(5);
        assert_eq!(s2.level_offset, 0);
    }

    #[test]
    fn empty_catalog_lookups() {
        let cat = MonsterCatalog::new();
        assert!(cat.is_empty());
        assert!(cat.get(1).is_none());
    }

    #[test]
    fn default_early_table_has_goblin_majority() {
        let t = default_early_encounter_table("test");
        // Goblin (formation 1) should be the heaviest weighted row.
        let goblin_w = t
            .entries
            .iter()
            .find(|e| e.formation_id == 1)
            .unwrap()
            .weight;
        let max_other = t
            .entries
            .iter()
            .filter(|e| e.formation_id != 1)
            .map(|e| e.weight)
            .max()
            .unwrap_or(0);
        assert!(goblin_w >= max_other);
    }
}

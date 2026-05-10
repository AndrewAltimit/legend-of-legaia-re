//! Inventory item-effect catalog.
//!
//! Maps item IDs to typed [`ItemEffect`] descriptions that the battle
//! and field menus consume. The retail engine reads these effects from
//! the SCUS data section table starting at `_DAT_8006F198` in the
//! action validator (cf. `ghidra/scripts/funcs/8003fb10.txt` arm 6),
//! plus the per-spell consumer table at `+0x9C0` for healing magic.
//!
//! ## Format
//!
//! Each entry is a typed [`ItemEffect`] describing the side-effect
//! applied when the item is used:
//!
//! - [`ItemEffect::Heal`]: restores `amount` HP, capped at `hp_max`.
//! - [`ItemEffect::HealAll`]: full HP restore.
//! - [`ItemEffect::HealMp`]: restores MP.
//! - [`ItemEffect::Cure`]: clears one [`StatusKind`].
//! - [`ItemEffect::CureAll`]: clears every status.
//! - [`ItemEffect::Revive`]: restores HP to `(hp_max * factor) / 256`
//!   from zero (default factor is `128` = 50%).
//! - [`ItemEffect::StatBoost`]: permanently raises a base stat by
//!   `delta` (capped by [`crate::battle_stats::StatRecord`] limits).
//! - [`ItemEffect::Spirit`]: refunds `amount` AP into the active
//!   character's [`crate::ap_gauge::ApGauge`].
//! - [`ItemEffect::Capture`]: marks the target monster slot with the
//!   capture flag - battle's monster-wipe handler reads this.
//! - [`ItemEffect::Escape`]: forces a Run / Escape outcome.
//! - [`ItemEffect::Damage`]: deals `amount` HP damage (offensive
//!   items like Bombs).
//!
//! ## Application
//!
//! [`ItemEffect::apply`] returns an [`ItemOutcome`] enum that engines
//! fold into their world / battle event stream. Pure data - no I/O.

use legaia_engine_vm::status_effects::StatusKind;

/// Which stat an [`ItemEffect::StatBoost`] modifies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatBoostTarget {
    HpMax,
    MpMax,
    Attack,
    Udf,
    Ldf,
    Accuracy,
    Evasion,
}

/// Typed item effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemEffect {
    Heal {
        amount: u16,
    },
    HealAll,
    HealMp {
        amount: u16,
    },
    HealMpAll,
    Cure {
        kind: StatusKind,
    },
    CureAll,
    /// Revive a fallen actor. `factor` is a 0..=255 fixed-point fraction
    /// of `hp_max`; 128 = 50%, 255 = 100%.
    Revive {
        factor: u8,
    },
    StatBoost {
        target: StatBoostTarget,
        delta: u16,
    },
    Spirit {
        amount: u8,
    },
    Capture {
        strength: u16,
    },
    Escape,
    Damage {
        amount: u16,
    },
    /// Item exists in inventory but has no battle effect (key items).
    KeyItem,
}

/// One item entry. Engines populate the catalog at startup.
#[derive(Debug, Clone, Copy)]
pub struct ItemEntry {
    pub id: u8,
    pub name: &'static str,
    pub effect: ItemEffect,
    /// `true` if usable mid-battle. Key items / equipment-only items
    /// are `false`; the item menu filters by this.
    pub usable_in_battle: bool,
    /// `true` if usable on the field menu.
    pub usable_in_field: bool,
}

/// Catalog of item entries.
#[derive(Debug, Default, Clone)]
pub struct ItemCatalog {
    by_id: std::collections::HashMap<u8, ItemEntry>,
}

impl ItemCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build the default catalog from the catalogued vanilla items.
    /// Ids match the order in the retail item table at
    /// `_DAT_8007AB14` (item display strings reference these).
    pub fn vanilla() -> Self {
        let mut c = Self::new();
        // Healing
        c.insert(ItemEntry {
            id: 0x01,
            name: "Healing Leaf",
            effect: ItemEffect::Heal { amount: 100 },
            usable_in_battle: true,
            usable_in_field: true,
        });
        c.insert(ItemEntry {
            id: 0x02,
            name: "Healing Flower",
            effect: ItemEffect::Heal { amount: 300 },
            usable_in_battle: true,
            usable_in_field: true,
        });
        c.insert(ItemEntry {
            id: 0x03,
            name: "Healing Fruit",
            effect: ItemEffect::Heal { amount: 600 },
            usable_in_battle: true,
            usable_in_field: true,
        });
        c.insert(ItemEntry {
            id: 0x04,
            name: "Heal All Leaf",
            effect: ItemEffect::HealAll,
            usable_in_battle: true,
            usable_in_field: true,
        });
        // MP
        c.insert(ItemEntry {
            id: 0x05,
            name: "Magic Leaf",
            effect: ItemEffect::HealMp { amount: 30 },
            usable_in_battle: true,
            usable_in_field: true,
        });
        c.insert(ItemEntry {
            id: 0x06,
            name: "Magic Flower",
            effect: ItemEffect::HealMp { amount: 80 },
            usable_in_battle: true,
            usable_in_field: true,
        });
        c.insert(ItemEntry {
            id: 0x07,
            name: "Magic Fruit",
            effect: ItemEffect::HealMpAll,
            usable_in_battle: true,
            usable_in_field: true,
        });
        // Cure
        c.insert(ItemEntry {
            id: 0x08,
            name: "Antidote Leaf",
            effect: ItemEffect::Cure {
                kind: StatusKind::Poisoned,
            },
            usable_in_battle: true,
            usable_in_field: true,
        });
        c.insert(ItemEntry {
            id: 0x09,
            name: "Antidote Flower",
            effect: ItemEffect::CureAll,
            usable_in_battle: true,
            usable_in_field: true,
        });
        c.insert(ItemEntry {
            id: 0x0A,
            name: "Awake Leaf",
            effect: ItemEffect::Cure {
                kind: StatusKind::Asleep,
            },
            usable_in_battle: true,
            usable_in_field: true,
        });
        c.insert(ItemEntry {
            id: 0x0B,
            name: "Wake-Up Leaf",
            effect: ItemEffect::Cure {
                kind: StatusKind::Confused,
            },
            usable_in_battle: true,
            usable_in_field: true,
        });
        // Revive
        c.insert(ItemEntry {
            id: 0x0C,
            name: "Resurrection Leaf",
            effect: ItemEffect::Revive { factor: 128 }, // 50%
            usable_in_battle: true,
            usable_in_field: true,
        });
        c.insert(ItemEntry {
            id: 0x0D,
            name: "Resurrection Flower",
            effect: ItemEffect::Revive { factor: 255 }, // 100%
            usable_in_battle: true,
            usable_in_field: true,
        });
        // Stat boosts
        c.insert(ItemEntry {
            id: 0x0E,
            name: "Power Tonic",
            effect: ItemEffect::StatBoost {
                target: StatBoostTarget::Attack,
                delta: 1,
            },
            usable_in_battle: false,
            usable_in_field: true,
        });
        c.insert(ItemEntry {
            id: 0x0F,
            name: "Vital Tonic",
            effect: ItemEffect::StatBoost {
                target: StatBoostTarget::HpMax,
                delta: 10,
            },
            usable_in_battle: false,
            usable_in_field: true,
        });
        // AP / Spirit
        c.insert(ItemEntry {
            id: 0x10,
            name: "Spirit Sphere",
            effect: ItemEffect::Spirit { amount: 5 },
            usable_in_battle: true,
            usable_in_field: false,
        });
        // Capture
        c.insert(ItemEntry {
            id: 0x11,
            name: "Genocide Crystal",
            effect: ItemEffect::Capture { strength: 100 },
            usable_in_battle: true,
            usable_in_field: false,
        });
        // Escape
        c.insert(ItemEntry {
            id: 0x12,
            name: "Goblin Foot",
            effect: ItemEffect::Escape,
            usable_in_battle: true,
            usable_in_field: false,
        });
        // Damage
        c.insert(ItemEntry {
            id: 0x13,
            name: "Bomb",
            effect: ItemEffect::Damage { amount: 200 },
            usable_in_battle: true,
            usable_in_field: false,
        });
        c
    }

    pub fn insert(&mut self, entry: ItemEntry) {
        self.by_id.insert(entry.id, entry);
    }

    pub fn get(&self, id: u8) -> Option<&ItemEntry> {
        self.by_id.get(&id)
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Iterate over all items in the catalog. Order is unspecified.
    pub fn iter(&self) -> impl Iterator<Item = &ItemEntry> {
        self.by_id.values()
    }
}

/// Outcome of applying an item to a target. Engines fold these into
/// world state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemOutcome {
    HealedHp { amount: u16 },
    HealedMp { amount: u16 },
    Cured { kind: StatusKind },
    CuredAll,
    Revived { hp_after: u16 },
    StatRaised { target: StatBoostTarget, delta: u16 },
    SpiritGained { amount: u8 },
    CaptureRolled { strength: u16 },
    EscapeRequested,
    DamageDealt { amount: u16 },
    NoEffect,
}

/// Per-target snapshot the apply pass reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct TargetSnapshot {
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
    pub is_dead: bool,
}

/// Apply an [`ItemEffect`] against a [`TargetSnapshot`]. Pure function;
/// the caller is responsible for writing back the deltas.
pub fn apply_effect(effect: ItemEffect, target: &TargetSnapshot) -> ItemOutcome {
    match effect {
        ItemEffect::Heal { amount } => {
            if target.is_dead {
                return ItemOutcome::NoEffect;
            }
            let cap = target.hp_max.saturating_sub(target.hp);
            let healed = amount.min(cap);
            ItemOutcome::HealedHp { amount: healed }
        }
        ItemEffect::HealAll => {
            if target.is_dead {
                return ItemOutcome::NoEffect;
            }
            let healed = target.hp_max.saturating_sub(target.hp);
            ItemOutcome::HealedHp { amount: healed }
        }
        ItemEffect::HealMp { amount } => {
            let cap = target.mp_max.saturating_sub(target.mp);
            let healed = amount.min(cap);
            ItemOutcome::HealedMp { amount: healed }
        }
        ItemEffect::HealMpAll => {
            let healed = target.mp_max.saturating_sub(target.mp);
            ItemOutcome::HealedMp { amount: healed }
        }
        ItemEffect::Cure { kind } => ItemOutcome::Cured { kind },
        ItemEffect::CureAll => ItemOutcome::CuredAll,
        ItemEffect::Revive { factor } => {
            if !target.is_dead {
                return ItemOutcome::NoEffect;
            }
            // Fixed-point: hp_max * factor / 256.
            let raw = (target.hp_max as u32 * factor as u32) / 256;
            ItemOutcome::Revived {
                hp_after: raw as u16,
            }
        }
        ItemEffect::StatBoost { target: t, delta } => ItemOutcome::StatRaised { target: t, delta },
        ItemEffect::Spirit { amount } => ItemOutcome::SpiritGained { amount },
        ItemEffect::Capture { strength } => ItemOutcome::CaptureRolled { strength },
        ItemEffect::Escape => ItemOutcome::EscapeRequested,
        ItemEffect::Damage { amount } => {
            let dealt = amount.min(target.hp);
            ItemOutcome::DamageDealt { amount: dealt }
        }
        ItemEffect::KeyItem => ItemOutcome::NoEffect,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alive(hp: u16, hp_max: u16, mp: u16, mp_max: u16) -> TargetSnapshot {
        TargetSnapshot {
            hp,
            hp_max,
            mp,
            mp_max,
            is_dead: false,
        }
    }

    fn dead(hp_max: u16) -> TargetSnapshot {
        TargetSnapshot {
            hp: 0,
            hp_max,
            mp: 0,
            mp_max: 0,
            is_dead: true,
        }
    }

    #[test]
    fn vanilla_catalog_has_basic_items() {
        let c = ItemCatalog::vanilla();
        assert!(c.len() >= 18);
        assert!(c.get(0x01).is_some()); // healing leaf
        assert!(c.get(0x12).is_some()); // goblin foot
    }

    #[test]
    fn heal_clamps_to_hp_max() {
        let t = alive(50, 100, 0, 0);
        let r = apply_effect(ItemEffect::Heal { amount: 200 }, &t);
        assert_eq!(r, ItemOutcome::HealedHp { amount: 50 });
    }

    #[test]
    fn heal_all_full_recovery() {
        let t = alive(10, 100, 0, 0);
        let r = apply_effect(ItemEffect::HealAll, &t);
        assert_eq!(r, ItemOutcome::HealedHp { amount: 90 });
    }

    #[test]
    fn heal_on_dead_is_noop() {
        let t = dead(100);
        let r = apply_effect(ItemEffect::Heal { amount: 100 }, &t);
        assert_eq!(r, ItemOutcome::NoEffect);
    }

    #[test]
    fn revive_at_50pct() {
        let t = dead(200);
        let r = apply_effect(ItemEffect::Revive { factor: 128 }, &t);
        assert_eq!(r, ItemOutcome::Revived { hp_after: 100 });
    }

    #[test]
    fn revive_at_100pct() {
        let t = dead(200);
        let r = apply_effect(ItemEffect::Revive { factor: 255 }, &t);
        // 200 * 255 / 256 = 199 (floored).
        assert_eq!(r, ItemOutcome::Revived { hp_after: 199 });
    }

    #[test]
    fn revive_on_alive_is_noop() {
        let t = alive(50, 100, 0, 0);
        let r = apply_effect(ItemEffect::Revive { factor: 128 }, &t);
        assert_eq!(r, ItemOutcome::NoEffect);
    }

    #[test]
    fn mp_heal_clamps() {
        let t = alive(0, 0, 90, 100);
        let r = apply_effect(ItemEffect::HealMp { amount: 50 }, &t);
        assert_eq!(r, ItemOutcome::HealedMp { amount: 10 });
    }

    #[test]
    fn cure_just_passes_through_kind() {
        let t = alive(50, 100, 0, 0);
        let r = apply_effect(
            ItemEffect::Cure {
                kind: StatusKind::Poisoned,
            },
            &t,
        );
        assert_eq!(
            r,
            ItemOutcome::Cured {
                kind: StatusKind::Poisoned
            }
        );
    }

    #[test]
    fn damage_clamps_at_current_hp() {
        let t = alive(50, 100, 0, 0);
        let r = apply_effect(ItemEffect::Damage { amount: 200 }, &t);
        assert_eq!(r, ItemOutcome::DamageDealt { amount: 50 });
    }

    #[test]
    fn key_item_has_no_effect() {
        let t = alive(50, 100, 0, 0);
        let r = apply_effect(ItemEffect::KeyItem, &t);
        assert_eq!(r, ItemOutcome::NoEffect);
    }

    #[test]
    fn spirit_grants_ap() {
        let t = alive(50, 100, 0, 0);
        let r = apply_effect(ItemEffect::Spirit { amount: 5 }, &t);
        assert_eq!(r, ItemOutcome::SpiritGained { amount: 5 });
    }

    #[test]
    fn escape_emits_escape_requested() {
        let t = alive(50, 100, 0, 0);
        let r = apply_effect(ItemEffect::Escape, &t);
        assert_eq!(r, ItemOutcome::EscapeRequested);
    }

    #[test]
    fn capture_rolls_with_strength() {
        let t = alive(10, 100, 0, 0);
        let r = apply_effect(ItemEffect::Capture { strength: 200 }, &t);
        assert_eq!(r, ItemOutcome::CaptureRolled { strength: 200 });
    }

    #[test]
    fn stat_boost_produces_raised_outcome() {
        let t = alive(50, 100, 0, 0);
        let r = apply_effect(
            ItemEffect::StatBoost {
                target: StatBoostTarget::Attack,
                delta: 5,
            },
            &t,
        );
        assert_eq!(
            r,
            ItemOutcome::StatRaised {
                target: StatBoostTarget::Attack,
                delta: 5
            }
        );
    }

    #[test]
    fn catalog_iteration_covers_all_entries() {
        let c = ItemCatalog::vanilla();
        let count = c.iter().count();
        assert_eq!(count, c.len());
    }

    #[test]
    fn unknown_id_returns_none() {
        let c = ItemCatalog::vanilla();
        assert!(c.get(0xFE).is_none());
    }
}

//! Inventory item-effect catalog.
//!
//! Maps **real** retail item ids ([`legaia_asset::item_names`], the
//! `SCUS_942.54` item table) to typed [`ItemEffect`] descriptions the battle
//! and field menus consume.
//!
//! The per-item *effect-value* table the retail engine reads at use time is
//! **not yet pinned**. (An earlier note here placed it at `_DAT_8006F198` via
//! the "action validator" `FUN_8003fb10`; that is a misattribution -
//! `_DAT_8006F198`'s only consumers are the SFX-cue functions `FUN_800250D4` /
//! `FUN_80016B6C`, i.e. it is the [SFX descriptor table](../../docs/formats/sfx-table.md),
//! and `FUN_8003fb10` reads battle-actor HP fields, not an item table.) Until
//! that table is found, the amounts here are the curated walkthrough values
//! (`data/gamedata/items.toml`).
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

    /// Build the default catalog of consumable items keyed by their **real**
    /// retail item ids - the `SCUS_942.54` item table ([`legaia_asset::item_names`]),
    /// the same id space a granted / shop / dropped item carries. (The prior
    /// catalog keyed effects by fabricated sequential ids `0x01..` that collide
    /// with the table's internal `Ra-Seru Meta $N` placeholders, so live
    /// item-use never matched a real granted id; e.g. the real Healing Leaf is
    /// `0x77`, not `0x01`.)
    ///
    /// Only the consumables the current effect taxonomy models faithfully are
    /// included: single-target HP/MP restore, full restore, single + all status
    /// cure, revive, and field escape. Items that need infra this engine doesn't
    /// have yet are intentionally **omitted** (a held one just isn't offered)
    /// rather than shown as a no-op:
    /// - party-wide fixed-amount heals (Healing Bloom `0x7A`, Healing Fruit
    ///   `0x7B`) need all-target application;
    /// - temporary battle stat buffs (Power/Shield/Speed/Wonder Elixir
    ///   `0x8B..=0x8E`, the *Water* line `0x82..=0x87`, Fury Boost `0x81`) need
    ///   a battle-buff taxonomy;
    /// - utility (Door of Wind warp `0x89`, Incense encounter-rate `0x8A`, the
    ///   summon flutes `0x98`/`0x99`) have no engine consumer yet.
    ///
    /// Amounts are the curated walkthrough values
    /// (`data/gamedata/items.toml`); the on-disc per-item effect-value table is
    /// not yet pinned (see the module docs).
    pub fn vanilla() -> Self {
        let mut c = Self::new();
        let heal = |id, name, amount| ItemEntry {
            id,
            name,
            effect: ItemEffect::Heal { amount },
            usable_in_battle: true,
            usable_in_field: true,
        };
        // Single-target HP restore.
        c.insert(heal(0x77, "Healing Leaf", 200));
        c.insert(heal(0x78, "Healing Flower", 800));
        c.insert(heal(0xA3, "Healing Shroom", 60));
        // Full HP restore ("Restores maximum HP").
        c.insert(ItemEntry {
            id: 0x79,
            name: "Healing Berry",
            effect: ItemEffect::HealAll,
            usable_in_battle: true,
            usable_in_field: true,
        });
        c.insert(ItemEntry {
            id: 0xB2,
            name: "Soru Bread",
            effect: ItemEffect::HealAll,
            usable_in_battle: true,
            usable_in_field: true,
        });
        // MP restore.
        c.insert(ItemEntry {
            id: 0x7C,
            name: "Magic Leaf",
            effect: ItemEffect::HealMp { amount: 50 },
            usable_in_battle: true,
            usable_in_field: true,
        });
        c.insert(ItemEntry {
            id: 0x7D,
            name: "Magic Fruit",
            effect: ItemEffect::HealMp { amount: 200 },
            usable_in_battle: true,
            usable_in_field: true,
        });
        // Status cure.
        c.insert(ItemEntry {
            id: 0x7E,
            name: "Antidote",
            effect: ItemEffect::Cure {
                kind: StatusKind::Poisoned,
            },
            usable_in_battle: true,
            usable_in_field: true,
        });
        c.insert(ItemEntry {
            id: 0x7F,
            name: "Medicine",
            effect: ItemEffect::CureAll,
            usable_in_battle: true,
            usable_in_field: true,
        });
        // Revive a fallen ally with a small amount of HP (~25%).
        c.insert(ItemEntry {
            id: 0x80,
            name: "Phoenix",
            effect: ItemEffect::Revive { factor: 64 },
            usable_in_battle: true,
            usable_in_field: true,
        });
        // Field escape ("Escape from a dungeon").
        c.insert(ItemEntry {
            id: 0x88,
            name: "Door of Light",
            effect: ItemEffect::Escape,
            usable_in_battle: false,
            usable_in_field: true,
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
    fn vanilla_catalog_uses_real_item_ids() {
        let c = ItemCatalog::vanilla();
        assert!(c.len() >= 10);
        // Real retail ids (not the old fabricated 0x01.. sequence).
        let leaf = c.get(0x77).expect("Healing Leaf is item id 0x77");
        assert_eq!(leaf.name, "Healing Leaf");
        assert_eq!(leaf.effect, ItemEffect::Heal { amount: 200 });
        assert_eq!(c.get(0x7E).map(|e| e.name), Some("Antidote")); // id 0x7E
        // The internal placeholder id 0x01 ("Ra-Seru Meta $1") is not a usable
        // consumable, so the catalog must not claim it.
        assert!(c.get(0x01).is_none());
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

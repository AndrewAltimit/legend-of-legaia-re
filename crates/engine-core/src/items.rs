//! Inventory item-effect catalog.
//!
//! Maps **real** retail item ids ([`legaia_asset::item_names`], the
//! `SCUS_942.54` item table) to typed [`ItemEffect`] descriptions the battle
//! and field menus consume.
//!
//! The retail engine's per-item *effect class / targeting / usability* is the
//! on-disc [item-effect descriptor table](../../docs/formats/item-effect-table.md)
//! (`DAT_800752C0`, parser [`legaia_asset::item_effect`]): keyed by item id ->
//! subtype -> `[class, tier, flags]`. [`ItemCatalog::apply_effect_flags`]
//! installs its field/battle usability gates over the curated entries (which is
//! how cure/revive items end up correctly battle-only). The literal restore
//! *amount* lives in a **separate, also static** heal-amount table
//! (`0x8007655C`) the apply handler `FUN_800402F4` reads: HP tiers
//! `[200, 800, 9999]`, MP tiers `[50, 200, 20]` (parser
//! [`legaia_asset::item_effect::ItemEffectTable::heal_amounts`] /
//! `restore_amount`). The curated walkthrough values here are **byte-confirmed**
//! against that table by the disc-gated `item_effect_real` test, so they stay as
//! the engine's source (no behaviour change). (This corrects an earlier note
//! that the amounts were an *overlay-resident* immediate switch with no disc
//! table - they are static `SCUS_942.54` data.)
//!
//! (An earlier note placed the effect table at `_DAT_8006F198` via the "action
//! validator" `FUN_8003fb10`; that was a misattribution - `_DAT_8006F198` is the
//! [SFX descriptor table](../../docs/formats/sfx-table.md), and `FUN_8003fb10`
//! reads battle-actor HP fields, not an item table.)
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
//!
//! Target shape is orthogonal to the effect: the descriptor's `0x20`
//! all-party flag is tracked separately ([`ItemCatalog::is_all_party`], set by
//! [`ItemCatalog::apply_effect_flags`]). The item-use session fans a flagged
//! restorative item out across every living ally rather than asking for a
//! single target.

use legaia_engine_vm::status_effects::StatusKind;

/// The permanent stat-up consumables (class 6) by their **real** retail item
/// ids + names (the `SCUS_942.54` item table). The *Water* line raises one
/// record stat each; Honey / Miracle Water raise every stat. Seeded only when
/// the on-disc effect table is installed ([`ItemCatalog::apply_stat_items`]);
/// the actual per-stat changes are resolved from that table at use time. Names
/// are the on-disc item-table strings so they match the real id space.
const PERMANENT_STAT_ITEMS: &[(u8, &str)] = &[
    (0x82, "Life Water"),     // tier 0: Max HP +16
    (0x83, "Power Water"),    // tier 1: ATK +4
    (0x84, "Guardian Water"), // tier 2: DEF +4 (both facets)
    (0x85, "Swift Water"),    // tier 3: SPD +4
    (0x86, "Wisdom Water"),   // tier 4: INT +4
    (0x87, "Magic Water"),    // tier 5: Max MP +8
    (0x65, "Honey"),          // tier 6: all stats +4
    (0x6D, "Miracle Water"),  // tier 6: all stats +4
];

/// The one-battle stat-buff consumables (class 7, the Elixirs) by their real
/// retail ids + names. Each ramps the targeted battle-actor stat by ×6/5 for
/// the rest of the battle. Seeded only when the on-disc effect table is
/// installed ([`ItemCatalog::apply_buff_items`]); the buffed stats are resolved
/// from that table at use time ([`crate::World::use_item`]).
const BATTLE_BUFF_ITEMS: &[(u8, &str)] = &[
    (0x8B, "Power Elixir"),  // ATK
    (0x8C, "Shield Elixir"), // DEF (both facets)
    (0x8D, "Speed Elixir"),  // SPD
    (0x8E, "Wonder Elixir"), // all (SPD + DEF + ATK + AGL)
];

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
    /// Agility (`+0x110` live block). The permanent stat-up *Water* line raises
    /// it directly (vs. the Accuracy/Evasion aliases, which also derive from AGL).
    Agility,
    /// Speed (`+0x118`) — turn-order initiative stat.
    Speed,
    /// Intelligence (`+0x11A`).
    Intelligence,
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
    /// Permanent multi-stat increase (the class-6 *Water* line + the all-stats
    /// Honey / Miracle Water). A marker: the actual per-stat changes are
    /// resolved from the on-disc effect table at use time
    /// ([`crate::World::use_item`]), so this is only ever installed when a real
    /// item-effect table is present (see [`ItemCatalog::apply_stat_items`]).
    StatUp,
    /// One-battle stat buff (the class-7 Elixirs). A marker: the buffed stats are
    /// resolved from the on-disc effect table at use time
    /// ([`crate::World::use_item`]), which ramps each by ×6/5 for the battle.
    /// Only ever installed when a real table is present (see
    /// [`ItemCatalog::apply_buff_items`]).
    BattleBuff,
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
    /// Item ids whose effect applies to the **whole party** (the item-effect
    /// descriptor's `0x20` all-party flag, e.g. Healing Bloom / Healing Fruit).
    /// Seeded for the vanilla party-heal items and refreshed authoritatively
    /// from disc by [`ItemCatalog::apply_effect_flags`]. The item-use session
    /// fans a flagged item out across every valid ally instead of asking for a
    /// single target.
    all_party: std::collections::HashSet<u8>,
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
    /// cure, revive, field escape, and the party-wide HP heals (Healing Bloom
    /// `0x7A`, Healing Fruit `0x7B`), which fan out across the party via the
    /// item-effect descriptor's all-party flag (see [`Self::is_all_party`]).
    ///
    /// The stat-affecting consumables are **not** in this static set — they are
    /// seeded from the on-disc effect table (so they only appear when the disc
    /// is present) and resolve their per-stat changes from that table at use
    /// time: the permanent stat-up *Water* line (`0x82..=0x87` + the all-stats
    /// Honey `0x65` / Miracle Water `0x6D`) via [`Self::apply_stat_items`], and
    /// the one-battle buff Elixirs (Power/Shield/Speed/Wonder Elixir
    /// `0x8B..=0x8E`) via [`Self::apply_buff_items`]. Items that still need infra
    /// this engine doesn't have are intentionally **omitted** (a held one just
    /// isn't offered) rather than shown as a no-op:
    /// - Fury Boost (`0x81`) needs the action-gauge-extend consumer;
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
        // Single-target HP restore. Healing Shroom (0xA3) shares the item
        // table's subtype 0 with Healing Leaf (0x77) and its on-disc
        // description reads "Recover 200HP. Ally." - so it heals 200, not 60
        // (the curated gamedata conflated its 60-gold price with the amount).
        c.insert(heal(0x77, "Healing Leaf", 200));
        c.insert(heal(0x78, "Healing Flower", 800));
        c.insert(heal(0xA3, "Healing Shroom", 200));
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
                kind: StatusKind::Venom,
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
        // Party-wide HP restore ("Restores NHP to each character"). Each entry
        // carries a single-target per-member [`ItemEffect::Heal`]; the all-party
        // flag (below) fans the effect across every living ally. Disc tiers:
        // Bloom = HealHpAllParty tier 0 (200), Fruit = tier 1 (800).
        c.insert(ItemEntry {
            id: 0x7A,
            name: "Healing Bloom",
            effect: ItemEffect::Heal { amount: 200 },
            usable_in_battle: true,
            usable_in_field: true,
        });
        c.insert(ItemEntry {
            id: 0x7B,
            name: "Healing Fruit",
            effect: ItemEffect::Heal { amount: 800 },
            usable_in_battle: true,
            usable_in_field: true,
        });
        // Seed the all-party flag for the vanilla party-heal items so the
        // item-use session fans them out even on disc-free builds;
        // `apply_effect_flags` refreshes this from the real descriptor table.
        c.all_party.insert(0x7A);
        c.all_party.insert(0x7B);
        c
    }

    pub fn insert(&mut self, entry: ItemEntry) {
        self.by_id.insert(entry.id, entry);
    }

    /// Override each entry's field/battle usability from the real on-disc
    /// item-effect descriptor table ([`legaia_asset::item_effect`],
    /// `DAT_800752C0`). The on-disc flags are authoritative: e.g. the cure /
    /// revive items (Antidote `0x7E`, Medicine `0x7F`, Phoenix `0x80`) carry
    /// the battle-only flag byte `0x84` - they are usable in battle but NOT
    /// from the field menu, even though hand-curated data marked them
    /// field-usable. Healers carry `0x86` (both menus); the field-utility items
    /// carry `0x02` (field only). Entries whose id doesn't resolve to a usable
    /// on-disc consumable are left untouched (the curated amount/effect kind
    /// stays - this only corrects the usability gates).
    pub fn apply_effect_flags(&mut self, table: &legaia_asset::item_effect::ItemEffectTable) {
        // Collect ids first to avoid borrowing `self.by_id` while mutating
        // `self.all_party`.
        let ids: Vec<u8> = self.by_id.keys().copied().collect();
        for id in ids {
            let Some(eff) = table.effect(id) else {
                continue;
            };
            if eff.is_usable_consumable()
                && let Some(entry) = self.by_id.get_mut(&id)
            {
                entry.usable_in_field = eff.field_usable();
                entry.usable_in_battle = eff.battle_usable();
            }
            // The all-party flag is authoritative from disc for every item in
            // the catalog (even non-usable rows keep a consistent flag).
            if eff.all_party() {
                self.all_party.insert(id);
            } else {
                self.all_party.remove(&id);
            }
        }
    }

    /// Seed the permanent stat-up consumables (class 6, the *Water* line + the
    /// all-stats Honey / Miracle Water) into the catalog from the real on-disc
    /// item-effect table. Each is installed as an [`ItemEffect::StatUp`] marker;
    /// the per-stat changes are resolved from the same table at use time
    /// ([`crate::World::use_item`]).
    ///
    /// These are seeded **only** when the disc table is present (called from
    /// [`crate::World::set_item_effects`] / [`crate::World::set_item_catalog`]),
    /// so a disc-free build never offers an item that would resolve to a no-op.
    /// An item is added only if the installed table actually classifies it as a
    /// permanent stat-up (defensive against an edited table).
    pub fn apply_stat_items(&mut self, table: &legaia_asset::item_effect::ItemEffectTable) {
        use legaia_asset::item_effect::StatItemEffect;
        for &(id, name) in PERMANENT_STAT_ITEMS {
            if matches!(table.stat_effect(id), Some(StatItemEffect::Permanent(_))) {
                let field_usable = table.effect(id).map(|e| e.field_usable()).unwrap_or(true);
                self.insert(ItemEntry {
                    id,
                    name,
                    effect: ItemEffect::StatUp,
                    usable_in_battle: false,
                    usable_in_field: field_usable,
                });
            }
        }
    }

    /// Seed the one-battle stat-buff Elixirs (class 7) into the catalog from the
    /// real on-disc item-effect table. Each is installed as an
    /// [`ItemEffect::BattleBuff`] marker (battle-only); the buffed stats are
    /// resolved from the same table at use time ([`crate::World::use_item`]),
    /// which ramps each by ×6/5 for the rest of the battle.
    ///
    /// Like [`Self::apply_stat_items`], seeded **only** when the disc table is
    /// present, and only for ids the table actually classifies as a one-battle
    /// buff (defensive against an edited table).
    pub fn apply_buff_items(&mut self, table: &legaia_asset::item_effect::ItemEffectTable) {
        use legaia_asset::item_effect::StatItemEffect;
        for &(id, name) in BATTLE_BUFF_ITEMS {
            if matches!(
                table.stat_effect(id),
                Some(StatItemEffect::BuffOneBattle(_))
            ) {
                let battle_usable = table.effect(id).map(|e| e.battle_usable()).unwrap_or(true);
                self.insert(ItemEntry {
                    id,
                    name,
                    effect: ItemEffect::BattleBuff,
                    usable_in_battle: battle_usable,
                    usable_in_field: false,
                });
            }
        }
    }

    /// `true` if the item's effect applies to the whole party (the descriptor's
    /// `0x20` all-party flag). The item-use session fans a flagged item out
    /// across every valid ally instead of asking for a single target.
    pub fn is_all_party(&self, id: u8) -> bool {
        self.all_party.contains(&id)
    }

    /// Set or clear the all-party flag for an item id. Engines that source the
    /// flag from somewhere other than [`Self::apply_effect_flags`] (and tests)
    /// use this directly.
    pub fn set_all_party(&mut self, id: u8, on: bool) {
        if on {
            self.all_party.insert(id);
        } else {
            self.all_party.remove(&id);
        }
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
    HealedHp {
        amount: u16,
    },
    HealedMp {
        amount: u16,
    },
    Cured {
        kind: StatusKind,
    },
    CuredAll,
    Revived {
        hp_after: u16,
    },
    StatRaised {
        target: StatBoostTarget,
        delta: u16,
    },
    /// A permanent multi-stat boost ([`ItemEffect::StatUp`]) was applied;
    /// `count` is the number of individual stat raises performed (a single
    /// *Water* raises one, the all-stats items raise several).
    StatsRaised {
        count: u8,
    },
    /// A one-battle stat buff ([`ItemEffect::BattleBuff`], the class-7 Elixirs)
    /// was applied; `count` is the number of stats ramped (Power/Shield/Speed
    /// Elixir buff one, Wonder Elixir buffs four).
    Buffed {
        count: u8,
    },
    SpiritGained {
        amount: u8,
    },
    CaptureRolled {
        strength: u16,
    },
    EscapeRequested,
    DamageDealt {
        amount: u16,
    },
    NoEffect,
}

/// Bit position of a [`StatusKind`] in a status bitset (the `StatusKind` enum
/// is fieldless, so its discriminant is the bit index). Shared by
/// [`TargetSnapshot::status_mask`] and the item-menu usability gate.
pub(crate) fn status_bit(kind: StatusKind) -> u8 {
    1u8 << (kind as u8)
}

/// Per-target snapshot the apply pass reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct TargetSnapshot {
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
    pub is_dead: bool,
    /// Bitset of [`StatusKind`]s afflicting the target (bit `status_bit(kind)`).
    /// Cure effects read it to no-op when the relevant affliction is absent.
    pub status_mask: u8,
}

impl TargetSnapshot {
    /// `true` if `kind` currently afflicts the target.
    pub fn has_status(&self, kind: StatusKind) -> bool {
        self.status_mask & status_bit(kind) != 0
    }
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
        ItemEffect::Cure { kind } => {
            // Curing an unafflicted target does nothing - report it honestly
            // rather than a phantom "Cured" (matches the retail relevance
            // predicate, which only treats a cure as applicable when the
            // matching status is present).
            if target.has_status(kind) {
                ItemOutcome::Cured { kind }
            } else {
                ItemOutcome::NoEffect
            }
        }
        ItemEffect::CureAll => {
            if target.status_mask != 0 {
                ItemOutcome::CuredAll
            } else {
                ItemOutcome::NoEffect
            }
        }
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
        // The multi-stat permanent boost and the one-battle buff are both
        // resolved from the on-disc table in `World::use_item` (which has the
        // table + the target actor), so the pure, table-less path is a no-op.
        ItemEffect::StatUp | ItemEffect::BattleBuff => ItemOutcome::NoEffect,
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
            status_mask: 0,
        }
    }

    fn dead(hp_max: u16) -> TargetSnapshot {
        TargetSnapshot {
            hp: 0,
            hp_max,
            mp: 0,
            mp_max: 0,
            is_dead: true,
            status_mask: 0,
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
    fn cure_passes_through_kind_when_the_status_is_present() {
        let mut t = alive(50, 100, 0, 0);
        t.status_mask = status_bit(StatusKind::Venom);
        let r = apply_effect(
            ItemEffect::Cure {
                kind: StatusKind::Venom,
            },
            &t,
        );
        assert_eq!(
            r,
            ItemOutcome::Cured {
                kind: StatusKind::Venom
            }
        );
    }

    #[test]
    fn cure_is_a_no_op_when_the_status_is_absent() {
        // Venom cure on a target afflicted only by Toxic (and on a clean
        // target) does nothing.
        let mut t = alive(50, 100, 0, 0);
        t.status_mask = status_bit(StatusKind::Toxic);
        let r = apply_effect(
            ItemEffect::Cure {
                kind: StatusKind::Venom,
            },
            &t,
        );
        assert_eq!(r, ItemOutcome::NoEffect);

        let clean = alive(50, 100, 0, 0);
        assert_eq!(
            apply_effect(ItemEffect::CureAll, &clean),
            ItemOutcome::NoEffect
        );
        let mut afflicted = alive(50, 100, 0, 0);
        afflicted.status_mask = status_bit(StatusKind::Curse);
        assert_eq!(
            apply_effect(ItemEffect::CureAll, &afflicted),
            ItemOutcome::CuredAll
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
    fn stat_up_marker_is_a_noop_in_the_table_less_path() {
        // The multi-stat StatUp boost and the one-battle BattleBuff are both
        // resolved from the on-disc table in `World::use_item`; the pure
        // `apply_effect` has no table, so they no-op.
        let t = alive(50, 100, 0, 0);
        assert_eq!(apply_effect(ItemEffect::StatUp, &t), ItemOutcome::NoEffect);
        assert_eq!(
            apply_effect(ItemEffect::BattleBuff, &t),
            ItemOutcome::NoEffect
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

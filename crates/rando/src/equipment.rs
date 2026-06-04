//! Equipment-as-enemy-drops support: classify which item ids are equippable
//! gear (weapons / armor / accessories), tier each by power, and turn each
//! monster's single drop slot into a *rare* random equipment drop.
//!
//! ## Classification (no committed Sony bytes)
//!
//! The retail item id space is one flat 256-entry table shared by consumables,
//! key items, and equipment (see [`legaia_asset::item_names`]). Nothing on the
//! disc cleanly flags "this id is a weapon" in a single byte, so we classify by
//! **name**: every weapon / armor / accessory in the curated, public
//! [`legaia_gamedata`] tables is matched (case-insensitively) against the
//! disc's own item-name table to recover its id. The names come from public
//! walkthroughs and ship in the repo; the ids come from the *user's* disc at
//! runtime — no Sony bytes are embedded, and the join double-checks the curated
//! tables against the real executable.
//!
//! ## Tiering + drop rate
//!
//! Each equipment piece carries its curated gamedata gold price; each monster
//! carries its base EXP reward ([`legaia_asset::monster_archive`] `+0x46`).
//! Both are bucketed into early / mid / late tiers, and the drop rate is the
//! *lower* of the two tiers' rates ("both combined" — a powerful weapon from a
//! weak early enemy is as rare as the rarest of the two). Rates: early 3 %,
//! mid 2 %, late 1 %.
//!
//! The retail drop roll is integer `rand() % 100 < chance`
//! ([`crate::monster::DROP_CHANCE_OFFSET`], pinned in `FUN_8004E568`), so the
//! finest representable nonzero rate is **1 %** — the late-game "0.5 %" target
//! is floored to 1 % because the engine cannot express a sub-percent chance.

use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};

use legaia_asset::item_names::ItemNameTable;
use legaia_gamedata::Database;

use crate::drops::DropAssignment;
use crate::rng::SplitMix64;

/// gamedata gold price at or below which an equipment piece counts as
/// early-game (the cheap starter gear). ~33rd percentile of priced equipment.
pub const EARLY_PRICE_MAX: u32 = 3_700;
/// gamedata gold price at or below which an equipment piece counts as mid-game.
/// ~66th percentile of priced equipment; above this (or unpriced quest gear) is
/// late-game.
pub const MID_PRICE_MAX: u32 = 17_000;

/// Monster base-EXP at or below which an enemy counts as early-game.
pub const EARLY_EXP_MAX: u16 = 600;
/// Monster base-EXP at or below which an enemy counts as mid-game; above this is
/// late-game.
pub const MID_EXP_MAX: u16 = 3_000;

/// Early-tier drop rate (percent).
pub const RATE_EARLY: u8 = 3;
/// Mid-tier drop rate (percent).
pub const RATE_MID: u8 = 2;
/// Late-tier drop rate (percent). The requested 0.5 % floors to 1 % because the
/// retail drop roll is integer `rand() % 100` (a sub-percent chance is
/// unrepresentable).
pub const RATE_LATE: u8 = 1;

/// One equippable item eligible to be assigned as a rare enemy drop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EquipmentItem {
    /// Item id in the shared 256-entry id space (the value written to a
    /// monster record's `+0x48` drop slot).
    pub id: u8,
    /// Curated gamedata gold price, or `None` for quest-only gear (treated as
    /// late-tier). Used only to tier the drop rate.
    pub price: Option<u32>,
}

/// A monster's id paired with its base EXP reward — the planner's per-enemy
/// input for tiering the drop rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonsterExp {
    pub monster_id: u16,
    pub exp: u16,
}

/// Tier index `0` = early, `1` = mid, `2` = late (higher = rarer).
fn price_tier(price: Option<u32>) -> u8 {
    match price {
        Some(p) if p <= EARLY_PRICE_MAX => 0,
        Some(p) if p <= MID_PRICE_MAX => 1,
        // Expensive endgame gear, or unpriced quest gear (`None`) — late.
        _ => 2,
    }
}

fn exp_tier(exp: u16) -> u8 {
    if exp <= EARLY_EXP_MAX {
        0
    } else if exp <= MID_EXP_MAX {
        1
    } else {
        2
    }
}

fn rate_for_tier(tier: u8) -> u8 {
    match tier {
        0 => RATE_EARLY,
        1 => RATE_MID,
        _ => RATE_LATE,
    }
}

/// Combined equipment drop chance (percent): the *lower* of the item-tier rate
/// and the enemy-tier rate, i.e. the rate of the rarer (higher) of the two
/// tiers. So a late-game weapon is rare even on an early enemy, and an early
/// trinket is rare on a late boss.
pub fn equipment_drop_chance(item_price: Option<u32>, enemy_exp: u16) -> u8 {
    rate_for_tier(price_tier(item_price).max(exp_tier(enemy_exp)))
}

/// Build the equipment pool from a `SCUS_942.54` image: every weapon, armor and
/// accessory in the curated gamedata tables whose name resolves to a real id in
/// the disc's item-name table, deduplicated and sorted by id. Each entry keeps
/// its gamedata price for tiering.
///
/// Returns an error only if `scus` isn't a PSX-EXE / the item table is absent.
/// Items the gamedata names but the disc doesn't (or vice versa) are simply
/// absent from the pool — a few character-default weapons and quest items don't
/// match by name, which is harmless for a drop pool.
pub fn equipment_pool(scus: &[u8]) -> Result<Vec<EquipmentItem>> {
    let table = ItemNameTable::from_scus(scus)
        .context("SCUS_942.54 is not a PSX-EXE / item table absent")?;

    // Disc name (lowercased) -> id. Item names are unique, so first-wins is
    // safe; `or_insert` just guards against any accidental duplicate.
    let mut by_name: HashMap<String, u8> = HashMap::new();
    for id in 1..=u8::MAX {
        if let Some(name) = table.name(id) {
            by_name.entry(name.to_ascii_lowercase()).or_insert(id);
        }
    }

    let gd = Database::load();
    let named_prices = gd
        .weapons()
        .iter()
        .map(|w| (w.name.as_str(), w.price))
        .chain(gd.armor().iter().map(|a| (a.name.as_str(), a.price)))
        .chain(gd.accessories().iter().map(|a| (a.name.as_str(), a.price)));

    let mut seen: HashSet<u8> = HashSet::new();
    let mut pool: Vec<EquipmentItem> = Vec::new();
    for (name, price) in named_prices {
        if let Some(&id) = by_name.get(&name.to_ascii_lowercase())
            && seen.insert(id)
        {
            pool.push(EquipmentItem { id, price });
        }
    }
    pool.sort_by_key(|e| e.id);
    Ok(pool)
}

/// Plan an equipment drop for **every** monster: each monster's drop slot
/// becomes a uniformly-random equipment piece from `pool`, with a chance tiered
/// by [`equipment_drop_chance`]. Deterministic in `(monsters, pool, seed)`; the
/// monsters are visited in the given order so a published seed reproduces the
/// table. Returns an empty plan if `pool` is empty.
pub fn plan_equipment_drops(
    monsters: &[MonsterExp],
    pool: &[EquipmentItem],
    seed: u64,
) -> Vec<DropAssignment> {
    if pool.is_empty() {
        return Vec::new();
    }
    let mut rng = SplitMix64::new(seed);
    monsters
        .iter()
        .map(|m| {
            let item = pool[rng.below(pool.len())];
            DropAssignment {
                monster_id: m.monster_id,
                item: item.id,
                chance: equipment_drop_chance(item.price, m.exp),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_split_at_the_thresholds() {
        assert_eq!(price_tier(Some(0)), 0);
        assert_eq!(price_tier(Some(EARLY_PRICE_MAX)), 0);
        assert_eq!(price_tier(Some(EARLY_PRICE_MAX + 1)), 1);
        assert_eq!(price_tier(Some(MID_PRICE_MAX)), 1);
        assert_eq!(price_tier(Some(MID_PRICE_MAX + 1)), 2);
        assert_eq!(price_tier(None), 2, "quest/unpriced gear is late-tier");

        assert_eq!(exp_tier(0), 0);
        assert_eq!(exp_tier(EARLY_EXP_MAX), 0);
        assert_eq!(exp_tier(EARLY_EXP_MAX + 1), 1);
        assert_eq!(exp_tier(MID_EXP_MAX), 1);
        assert_eq!(exp_tier(MID_EXP_MAX + 1), 2);
    }

    #[test]
    fn combined_chance_takes_the_rarer_tier() {
        // Early item + early enemy -> 3%.
        assert_eq!(equipment_drop_chance(Some(100), 50), RATE_EARLY);
        // Early item + late enemy -> late rate (1%): the enemy tier wins.
        assert_eq!(equipment_drop_chance(Some(100), 60000), RATE_LATE);
        // Late (quest) item + early enemy -> late rate: the item tier wins.
        assert_eq!(equipment_drop_chance(None, 10), RATE_LATE);
        // Mid item + mid enemy -> mid (2%).
        assert_eq!(equipment_drop_chance(Some(10_000), 2_000), RATE_MID);
        // Mid item + early enemy -> mid (the item tier is rarer).
        assert_eq!(equipment_drop_chance(Some(10_000), 10), RATE_MID);
        // Every rate is a representable integer percent in 1..=3.
        for &c in &[RATE_EARLY, RATE_MID, RATE_LATE] {
            assert!((1..=3).contains(&c));
        }
    }

    #[test]
    fn plan_is_deterministic_and_covers_every_monster() {
        let pool = vec![
            EquipmentItem {
                id: 0x22,
                price: Some(1200),
            },
            EquipmentItem {
                id: 0x46,
                price: Some(15000),
            },
            EquipmentItem {
                id: 0xc2,
                price: None,
            },
        ];
        let monsters = vec![
            MonsterExp {
                monster_id: 1,
                exp: 50,
            },
            MonsterExp {
                monster_id: 2,
                exp: 5000,
            },
            MonsterExp {
                monster_id: 3,
                exp: 800,
            },
        ];
        let a = plan_equipment_drops(&monsters, &pool, 0xABCD);
        let b = plan_equipment_drops(&monsters, &pool, 0xABCD);
        assert_eq!(a, b, "same seed reproduces the plan");
        assert_eq!(a.len(), monsters.len(), "every monster gets a drop");
        for asn in &a {
            assert!(
                pool.iter().any(|e| e.id == asn.item),
                "assigned item {} is from the pool",
                asn.item
            );
            assert!(
                (1..=3).contains(&asn.chance),
                "chance {} is a tiered rate",
                asn.chance
            );
        }
    }

    #[test]
    fn empty_pool_plans_nothing() {
        let monsters = vec![MonsterExp {
            monster_id: 1,
            exp: 50,
        }];
        assert!(plan_equipment_drops(&monsters, &[], 1).is_empty());
    }
}

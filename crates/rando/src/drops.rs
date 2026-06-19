//! Drop-table randomization: reassign monster item drops from a seed.
//!
//! Pure planning logic - it decides *what* each monster should drop; applying
//! the plan to the disc is [`crate::monster::set_drop`]. Deterministic in
//! `(current drops, item pool, seed, mode)` so a published seed always
//! reproduces the same drop table.

use crate::rng::SplitMix64;

/// How drops are reassigned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropMode {
    /// Redistribute the existing set of drops among the monsters that have one:
    /// same items and chances, new owners. Preserves the overall drop economy.
    Shuffle,
    /// Give each dropping monster a uniformly random item from the pool (its
    /// drop *chance* is kept). Wilder; the drop economy changes.
    Random,
}

/// A monster's current drop (item id + chance percent).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurrentDrop {
    pub monster_id: u16,
    pub item: u8,
    pub chance: u8,
}

/// A planned new drop for a monster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropAssignment {
    pub monster_id: u16,
    pub item: u8,
    pub chance: u8,
}

/// Plan new drops for the monsters that currently have one (`item != 0`).
///
/// Monsters without a drop are left untouched (no assignment is returned for
/// them). The result is in the same order as the dropping monsters appear in
/// `current`, which keeps the plan stable for a given seed.
pub fn plan_drops(
    current: &[CurrentDrop],
    item_pool: &[u8],
    seed: u64,
    mode: DropMode,
) -> Vec<DropAssignment> {
    let droppers: Vec<&CurrentDrop> = current.iter().filter(|d| d.item != 0).collect();
    let mut rng = SplitMix64::new(seed);
    match mode {
        DropMode::Random => {
            if item_pool.is_empty() {
                return Vec::new();
            }
            droppers
                .iter()
                .map(|d| DropAssignment {
                    monster_id: d.monster_id,
                    item: item_pool[rng.below(item_pool.len())],
                    chance: d.chance,
                })
                .collect()
        }
        DropMode::Shuffle => {
            let mut pairs: Vec<(u8, u8)> = droppers.iter().map(|d| (d.item, d.chance)).collect();
            rng.shuffle(&mut pairs);
            droppers
                .iter()
                .zip(pairs)
                .map(|(d, (item, chance))| DropAssignment {
                    monster_id: d.monster_id,
                    item,
                    chance,
                })
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<CurrentDrop> {
        vec![
            CurrentDrop {
                monster_id: 1,
                item: 10,
                chance: 25,
            },
            CurrentDrop {
                monster_id: 2,
                item: 0,
                chance: 0,
            }, // no drop
            CurrentDrop {
                monster_id: 3,
                item: 20,
                chance: 50,
            },
            CurrentDrop {
                monster_id: 4,
                item: 30,
                chance: 100,
            },
            CurrentDrop {
                monster_id: 5,
                item: 0,
                chance: 0,
            }, // no drop
        ]
    }

    #[test]
    fn deterministic_for_seed() {
        let cur = sample();
        let pool = vec![1, 2, 3, 4, 5, 99, 100];
        for mode in [DropMode::Shuffle, DropMode::Random] {
            let a = plan_drops(&cur, &pool, 0x1234, mode);
            let b = plan_drops(&cur, &pool, 0x1234, mode);
            assert_eq!(a, b, "same seed must reproduce the plan ({mode:?})");
        }
    }

    #[test]
    fn only_dropping_monsters_are_planned() {
        let cur = sample();
        let pool = vec![7, 8, 9];
        for mode in [DropMode::Shuffle, DropMode::Random] {
            let plan = plan_drops(&cur, &pool, 1, mode);
            let ids: Vec<u16> = plan.iter().map(|a| a.monster_id).collect();
            assert_eq!(ids, vec![1, 3, 4], "only item!=0 monsters are reassigned");
        }
    }

    #[test]
    fn random_draws_only_from_pool() {
        let cur = sample();
        let pool = vec![42, 77, 123];
        let plan = plan_drops(&cur, &pool, 9, DropMode::Random);
        for a in &plan {
            assert!(
                pool.contains(&a.item),
                "assigned item {} not in pool",
                a.item
            );
        }
        // Chance is preserved in Random mode.
        assert_eq!(plan[0].chance, 25);
        assert_eq!(plan[2].chance, 100);
    }

    #[test]
    fn shuffle_preserves_the_drop_multiset() {
        let cur = sample();
        let plan = plan_drops(&cur, &[], 5, DropMode::Shuffle);
        let mut got: Vec<(u8, u8)> = plan.iter().map(|a| (a.item, a.chance)).collect();
        let mut want: Vec<(u8, u8)> = vec![(10, 25), (20, 50), (30, 100)];
        got.sort_unstable();
        want.sort_unstable();
        assert_eq!(
            got, want,
            "shuffle keeps the same set of (item, chance) drops"
        );
    }

    #[test]
    fn random_with_empty_pool_plans_nothing() {
        let cur = sample();
        assert!(plan_drops(&cur, &[], 1, DropMode::Random).is_empty());
    }
}

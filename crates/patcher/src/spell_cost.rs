//! Spell MP-cost randomizer: redistribute the MP costs of the named, costed
//! spells in the static `SCUS_942.54` spell table.
//!
//! The spell table (`DAT_800754C8`, 12-byte stride) carries each spell's MP cost
//! at record `+3` (see [`legaia_asset::spell_names`]). The randomizable
//! population is the spells that are both **named** and carry a **non-zero** MP
//! cost - i.e. real castable magic, not the unnamed internal enemy-attack tiers
//! (which read cost `0`). [`plan_costs`] either permutes that cost column
//! ([`StatMode::Shuffle`], so the multiset of MP costs is preserved - every cost
//! still exists, just on a different spell) or draws each from the pool
//! ([`StatMode::Random`]).
//!
//! Only the `+3` cost byte moves; names, target shapes, and capture classes are
//! untouched, and only costed/named spells participate, so a free or
//! internal-tier entry never gains a cost. The table lives in `SCUS_942.54`, so
//! the edit is a same-size in-place SCUS patch (like the steal randomizer).

use crate::rng::SplitMix64;

/// Re-exported [`crate::drops::DropMode`] so the MP-cost pass shares the CLI's
/// `shuffle` / `random` vocabulary.
pub use crate::drops::DropMode as StatMode;

/// Plan an MP-cost randomization over `current` (the costs of the randomizable
/// spells, in id order). Deterministic in `(current, seed, mode)`.
///
/// [`StatMode::Shuffle`] permutes the column (a 1:1 reassignment, so the cost
/// multiset is preserved); [`StatMode::Random`] draws each cost from the pool.
pub fn plan_costs(current: &[u8], seed: u64, mode: StatMode) -> Vec<u8> {
    if current.is_empty() {
        return Vec::new();
    }
    let mut rng = SplitMix64::new(seed);
    match mode {
        StatMode::Shuffle => {
            let mut bag = current.to_vec();
            rng.shuffle(&mut bag);
            bag
        }
        StatMode::Random => (0..current.len())
            .map(|_| current[rng.below(current.len())])
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shuffle_preserves_multiset() {
        let current: Vec<u8> = vec![2, 4, 6, 8, 10, 12, 30];
        let plan = plan_costs(&current, 0x77, StatMode::Shuffle);
        let mut a = current.clone();
        let mut b = plan.clone();
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(a, b, "shuffle must preserve the MP-cost multiset");
    }

    #[test]
    fn random_draws_from_pool() {
        let current: Vec<u8> = vec![3, 9, 18, 36];
        let plan = plan_costs(&current, 4, StatMode::Random);
        let pool: std::collections::HashSet<u8> = current.iter().copied().collect();
        assert_eq!(plan.len(), current.len());
        assert!(plan.iter().all(|c| pool.contains(c)));
    }

    #[test]
    fn deterministic_and_empty_noop() {
        let current: Vec<u8> = vec![1, 2, 3, 4, 5];
        assert_eq!(
            plan_costs(&current, 9, StatMode::Shuffle),
            plan_costs(&current, 9, StatMode::Shuffle)
        );
        assert!(plan_costs(&[], 1, StatMode::Shuffle).is_empty());
    }
}

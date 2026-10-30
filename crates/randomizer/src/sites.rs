//! Order-independent picks: one value per named site, from a seed.
//!
//! The patcher sees the whole disc at once, so it can shuffle a population
//! (every chest, every shop slot) in one pass. A runtime randomizer meets the
//! sites one at a time, in whatever order the player reaches them, so its
//! picks must not depend on that order: each site's draw is seeded by the run
//! seed mixed with the site's own key (`domain` plus the bytes that name the
//! site). The same seed and key give the same pick on every visit, in every
//! run, on every machine.

use crate::rng::SplitMix64;

/// The generator for one site: the run `seed` mixed with an FNV-1a-64 hash
/// of `domain` and `key`.
pub fn site_rng(seed: u64, domain: &str, key: &[u8]) -> SplitMix64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in domain.as_bytes().iter().chain([0u8].iter()).chain(key) {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    // One splitmix step decorrelates neighbouring keys before the mix.
    let mut mix = SplitMix64::new(h);
    SplitMix64::new(seed ^ mix.next_u64())
}

/// A uniform pick from `pool` for the site `(domain, key)`; `None` when the
/// pool is empty.
pub fn pick<T: Copy>(seed: u64, domain: &str, key: &[u8], pool: &[T]) -> Option<T> {
    if pool.is_empty() {
        return None;
    }
    Some(pool[site_rng(seed, domain, key).below(pool.len())])
}

/// A fresh item for the item-giving site `(domain, key)` that held
/// `original`: `original` itself when it is in `keep` (a quest or key item -
/// the patcher's keep-static rule, which is what keeps a randomized run
/// completable) or the pool is empty, otherwise a pick from `pool`.
pub fn reassign_item(
    seed: u64,
    domain: &str,
    key: &[u8],
    original: u8,
    keep: &[u8],
    pool: &[u8],
) -> u8 {
    if original == 0 || keep.contains(&original) {
        return original;
    }
    pick(seed, domain, key, pool).unwrap_or(original)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_are_stable_per_key_and_spread_across_keys() {
        let pool: Vec<u8> = (1..=50).collect();
        let a = pick(7, "chest", b"town01:12", &pool);
        assert_eq!(a, pick(7, "chest", b"town01:12", &pool));
        let distinct: std::collections::BTreeSet<u8> = (0u32..64)
            .filter_map(|k| pick(7, "chest", &k.to_le_bytes(), &pool))
            .collect();
        assert!(distinct.len() > 20, "{distinct:?}");
        assert_ne!(
            (0u32..16)
                .map(|k| pick(7, "chest", &k.to_le_bytes(), &pool))
                .collect::<Vec<_>>(),
            (0u32..16)
                .map(|k| pick(8, "chest", &k.to_le_bytes(), &pool))
                .collect::<Vec<_>>(),
            "another seed is another run"
        );
        assert_eq!(pick::<u8>(7, "chest", b"x", &[]), None);
    }

    #[test]
    fn kept_items_and_the_empty_sentinel_never_move() {
        let pool = [5u8, 6, 7];
        assert_eq!(reassign_item(1, "chest", b"a", 0x9a, &[0x9a], &pool), 0x9a);
        assert_eq!(reassign_item(1, "chest", b"a", 0, &[], &pool), 0);
        assert!(pool.contains(&reassign_item(1, "chest", b"a", 9, &[], &pool)));
        assert_eq!(reassign_item(1, "chest", b"a", 9, &[], &[]), 9);
    }
}

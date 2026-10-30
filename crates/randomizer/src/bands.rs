//! Stat bands: every value of a record moved by its own random factor
//! inside a permille band, seeded per record.
//!
//! The patcher's stat pass shuffles each stat column across the whole
//! roster, which needs every record at once. The run-rule randomizer meets a
//! monster when a fight seats it, so it moves each record inside a band
//! around its own values instead: a weak monster stays weak-ish and a strong
//! one strong-ish, so the game's difficulty curve keeps its shape. The
//! protected ids of [`crate::monsters`] keep their own stats.

use crate::monsters::PROTECTED_MONSTER_IDS;
use crate::sites::site_rng;

/// A band of multipliers in permille, `lo..=hi`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Band {
    pub lo: u16,
    pub hi: u16,
}

impl Band {
    /// The default band: `0.8x..=1.25x`.
    pub const DEFAULT: Band = Band { lo: 800, hi: 1250 };
}

/// `stats` of monster `monster_id` moved inside `band`, each value by its own
/// draw: round half up, a zero stays zero, otherwise held to `1..=65535`.
/// A protected monster comes back unchanged.
pub fn band_stats<const N: usize>(
    seed: u64,
    monster_id: u16,
    stats: [u16; N],
    band: Band,
) -> [u16; N] {
    if PROTECTED_MONSTER_IDS.contains(&monster_id) {
        return stats;
    }
    let (lo, hi) = (band.lo.min(band.hi), band.lo.max(band.hi));
    let mut rng = site_rng(seed, "stat-band", &monster_id.to_le_bytes());
    let span = usize::from(hi - lo) + 1;
    stats.map(|v| {
        let f = u32::from(lo) + rng.below(span) as u32;
        if v == 0 {
            return 0;
        }
        let scaled = (u32::from(v) * f + 500) / 1000;
        scaled.clamp(1, 65535) as u16
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_hold_and_protected_monsters_keep_their_stats() {
        let s = [100u16, 0, 50, 40, 30, 20, 10];
        let b = band_stats(42, 33, s, Band::DEFAULT);
        assert_eq!(b, band_stats(42, 33, s, Band::DEFAULT), "deterministic");
        for (o, n) in s.iter().zip(b) {
            if *o == 0 {
                assert_eq!(n, 0);
            } else {
                assert!(n >= o * 8 / 10 && u32::from(n) <= u32::from(*o) * 125 / 100 + 1);
            }
        }
        for &id in PROTECTED_MONSTER_IDS {
            assert_eq!(band_stats(42, id, s, Band::DEFAULT), s);
        }
        assert_eq!(
            band_stats(1, 33, [7u16; 3], Band { lo: 1000, hi: 1000 }),
            [7; 3]
        );
    }
}

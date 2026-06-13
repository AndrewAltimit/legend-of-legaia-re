//! Special-attack power randomizer: redistribute the per-move power values in
//! the battle-action overlay's move-power table.
//!
//! The table (`0x801F4F5C`, PROT entry 898 at file offset
//! [`legaia_asset::move_power::MOVE_POWER_TABLE_FILE_OFFSET`]) holds 44
//! 26-byte records; the damage kernel reads each record's `+0x00` halfword as
//! the move's **power** (the `rand % ((power >> shift) + 1)` roll modulus). This
//! is the *special-attack* power space — enemy specials and Seru-magic, NOT
//! party Tactical Arts (those take power from the per-strike art-record byte).
//!
//! Only the `+0x00` power halfword moves: [`StatMode::Shuffle`] permutes it
//! across the 44 records (the multiset of move powers is preserved), while
//! [`StatMode::Random`] draws each from that pool. The other 24 bytes of each
//! record — strike geometry, phase timing, impact-effect / trail-texpage / sound
//! cue, contact / launch effect lists — are left untouched, so every move keeps
//! its own animation and effects; only how hard it hits changes. PROT 0898 is
//! stored raw (no LZS), so the write is strictly same-size and in place.

use crate::rng::SplitMix64;

/// Re-exported [`crate::drops::DropMode`] so the move-power pass shares the
/// CLI's `shuffle` / `random` vocabulary.
pub use crate::drops::DropMode as StatMode;

/// Plan a power-column randomization. `current` is the per-record `+0x00` power
/// halfword in table order; the returned vec is the reassigned powers in the
/// same order. Deterministic in `(current, seed, mode)`.
///
/// [`StatMode::Shuffle`] permutes the column (a 1:1 reassignment, so the
/// multiset of powers is preserved); [`StatMode::Random`] draws each cell from
/// the column pool with replacement.
pub fn plan_powers(current: &[i16], seed: u64, mode: StatMode) -> Vec<i16> {
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
        let current: Vec<i16> = vec![10, 20, 30, 40, 50, 60, 70, 80];
        let plan = plan_powers(&current, 0x1234, StatMode::Shuffle);
        let mut before = current.clone();
        let mut after = plan.clone();
        before.sort_unstable();
        after.sort_unstable();
        assert_eq!(before, after, "shuffle must preserve the power multiset");
    }

    #[test]
    fn random_draws_from_pool() {
        let current: Vec<i16> = vec![5, 15, 25, 35];
        let plan = plan_powers(&current, 9, StatMode::Random);
        let pool: std::collections::HashSet<i16> = current.iter().copied().collect();
        assert_eq!(plan.len(), current.len());
        assert!(plan.iter().all(|p| pool.contains(p)));
    }

    #[test]
    fn deterministic() {
        let current: Vec<i16> = (0..44).collect();
        assert_eq!(
            plan_powers(&current, 77, StatMode::Shuffle),
            plan_powers(&current, 77, StatMode::Shuffle)
        );
    }

    #[test]
    fn empty_is_noop() {
        assert!(plan_powers(&[], 1, StatMode::Shuffle).is_empty());
    }
}

//! Element-affinity matrix randomizer: scramble which element pairs are strong
//! or weak against each other.
//!
//! The battle-action overlay carries an 8×8 affinity matrix
//! (`matrix[attacker_element][defender_element]`, PROT entry 898 at file offset
//! [`legaia_asset::element_affinity::AFFINITY_MATRIX_FILE_OFFSET`]); each cell is
//! a damage-scale percentage (`damage = roll * pct / 100`, so `100` = neutral,
//! `> 100` = weak, `< 100` = resist, `0` = immune). See
//! [`legaia_asset::element_affinity`].
//!
//! [`plan_matrix`] randomizes the 64 cells: [`StatMode::Shuffle`] permutes them
//! (the multiset of scale percentages is preserved — the same number of
//! weaknesses / resistances exists, just between different element pairs), while
//! [`StatMode::Random`] draws each cell from that pool. Only the matrix moves;
//! the per-character element assignment and the summon-power rows are left
//! untouched, so the change is purely *which element beats which*. PROT 0898 is
//! stored raw (no LZS), so the write is strictly same-size and in place.

use crate::rng::SplitMix64;
use legaia_asset::element_affinity::ELEMENT_COUNT;

/// Re-exported [`crate::drops::DropMode`] so the affinity pass shares the CLI's
/// `shuffle` / `random` vocabulary.
pub use crate::drops::DropMode as StatMode;

/// Number of cells in the affinity matrix (`8 × 8`).
pub const MATRIX_CELLS: usize = ELEMENT_COUNT * ELEMENT_COUNT;

/// Plan an affinity-matrix randomization over the flattened `8×8` cells (row
/// major, `attacker * 8 + defender`). Deterministic in `(current, seed, mode)`.
///
/// [`StatMode::Shuffle`] permutes the cells (a 1:1 reassignment, so the multiset
/// of scale percentages is preserved); [`StatMode::Random`] draws each cell from
/// the pool with replacement.
pub fn plan_matrix(current: &[u8; MATRIX_CELLS], seed: u64, mode: StatMode) -> [u8; MATRIX_CELLS] {
    let mut rng = SplitMix64::new(seed);
    let mut out = *current;
    match mode {
        StatMode::Shuffle => {
            rng.shuffle(&mut out);
        }
        StatMode::Random => {
            for cell in out.iter_mut() {
                *cell = current[rng.below(MATRIX_CELLS)];
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> [u8; MATRIX_CELLS] {
        let mut m = [100u8; MATRIX_CELLS];
        // A few non-neutral cells, like the real sparse matrix (row-major
        // `attacker * 8 + defender`).
        m[17] = 104; // fire(2) vs water(1)
        m[10] = 104; // water(1) vs fire(2)
        m[32] = 200; // thunder(4) vs earth(0)
        m[4] = 50; // earth(0) vs thunder(4)
        m
    }

    #[test]
    fn shuffle_preserves_multiset() {
        let cur = sample();
        let plan = plan_matrix(&cur, 0xBEEF, StatMode::Shuffle);
        let mut a = cur.to_vec();
        let mut b = plan.to_vec();
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(a, b, "shuffle must preserve the scale-percentage multiset");
    }

    #[test]
    fn random_draws_from_pool() {
        let cur = sample();
        let plan = plan_matrix(&cur, 3, StatMode::Random);
        let pool: std::collections::HashSet<u8> = cur.iter().copied().collect();
        assert!(plan.iter().all(|c| pool.contains(c)));
    }

    #[test]
    fn deterministic() {
        let cur = sample();
        assert_eq!(
            plan_matrix(&cur, 5, StatMode::Shuffle),
            plan_matrix(&cur, 5, StatMode::Shuffle)
        );
    }
}

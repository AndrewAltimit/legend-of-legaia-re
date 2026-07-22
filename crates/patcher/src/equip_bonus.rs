//! Equipment stat-bonus randomizer: redistribute the passive stat tuples on the
//! static `SCUS_942.54` equipment bonus table (`DAT_80074F68`).
//!
//! Every equippable item resolves through the shared item table to an 8-byte
//! bonus record (see [`legaia_asset::equip_stats`]): `+0..+4` are the five stat
//! bonuses `[INT, ATK, UDF, LDF, SPD]`, `+5` the accessory-passive slot, `+6`
//! the equip-character mask, and `+7` the slot type (`0x00` body / `0x20` head /
//! `0x40` weapon / `0x60` footwear, plus a Ra-Seru bit). This pass moves only
//! the `+0..+4` stat tuple **within a slot category**, so a weapon's stats only
//! ever land on another weapon, armor on armor, and so on - the equip mask,
//! passive, and slot type stay welded to their record.
//!
//! It operates on bonus **rows**, not item ids: several items can share one
//! record, and editing per-id would rewrite a shared record more than once and
//! corrupt its bonuses. [`plan_bonus_shuffle`] groups the rows it is handed by
//! their `+7` slot category and, under [`StatMode::Shuffle`], permutes the stat
//! tuples within each category (the per-category multiset of bonuses is
//! preserved - the same gear power exists, just on different equipment); under
//! [`StatMode::Random`] it draws each row's tuple from its category pool. The
//! table lives in `SCUS_942.54`, so the edit is a same-size in-place SCUS patch.

use crate::rng::SplitMix64;

/// Re-exported [`crate::drops::DropMode`] so the bonus pass shares the CLI's
/// `shuffle` / `random` vocabulary.
pub use crate::drops::DropMode as StatMode;

/// Number of leading stat bytes that move (`+0..+4` = INT, ATK, UDF, LDF, SPD).
pub const STAT_LEN: usize = 5;

/// `+7` mask selecting the slot category (body / head / weapon / footwear); the
/// low bits (Ra-Seru flag) stay with the row, so they don't split the pool.
const SLOT_CATEGORY_MASK: u8 = 0x60;

/// Plan a stat-bonus randomization over the given 8-byte bonus rows. The rows
/// are grouped by their `+7` slot category and only the `+0..+4` stat tuple is
/// reassigned within a group; bytes `+5..+7` are copied through untouched. The
/// returned vec has the same length and order as `rows`. Deterministic in
/// `(rows, seed, mode)`.
///
/// [`StatMode::Shuffle`] permutes the tuples within each category (a 1:1
/// reassignment, so the per-category multiset of bonuses is preserved);
/// [`StatMode::Random`] draws each row's tuple from its category pool with
/// replacement.
pub fn plan_bonus_shuffle(rows: &[[u8; 8]], seed: u64, mode: StatMode) -> Vec<[u8; 8]> {
    use std::collections::BTreeMap;

    let mut out = rows.to_vec();
    if rows.is_empty() {
        return out;
    }

    // Row indices grouped by slot category, in sorted-key order so the RNG draw
    // sequence is deterministic regardless of how the rows are laid out.
    let mut groups: BTreeMap<u8, Vec<usize>> = BTreeMap::new();
    for (i, r) in rows.iter().enumerate() {
        groups.entry(r[7] & SLOT_CATEGORY_MASK).or_default().push(i);
    }

    let mut rng = SplitMix64::new(seed);
    for members in groups.values() {
        let tuples: Vec<[u8; STAT_LEN]> = members
            .iter()
            .map(|&i| {
                let mut t = [0u8; STAT_LEN];
                t.copy_from_slice(&rows[i][..STAT_LEN]);
                t
            })
            .collect();
        match mode {
            StatMode::Shuffle => {
                let mut bag = tuples.clone();
                rng.shuffle(&mut bag);
                for (k, &i) in members.iter().enumerate() {
                    out[i][..STAT_LEN].copy_from_slice(&bag[k]);
                }
            }
            StatMode::Random => {
                for &i in members {
                    let pick = tuples[rng.below(tuples.len())];
                    out[i][..STAT_LEN].copy_from_slice(&pick);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Two weapons, two body armors, one head, in mixed order (slot byte in +7).
    fn fixture() -> Vec<[u8; 8]> {
        vec![
            [0, 50, 0, 0, 0, 0x40, 1, 0x40],  // weapon A (atk 50)
            [0, 0, 30, 20, 0, 0x40, 1, 0x00], // body A (udf 30 / ldf 20)
            [0, 20, 0, 0, 0, 0x40, 2, 0x40],  // weapon B (atk 20)
            [0, 0, 10, 5, 0, 0x40, 4, 0x00],  // body B (udf 10 / ldf 5)
            [9, 0, 4, 0, 0, 0x40, 7, 0x20],   // head (int 9 / udf 4)
        ]
    }

    #[test]
    fn shuffle_preserves_per_category_multiset_and_keeps_tail() {
        let rows = fixture();
        let plan = plan_bonus_shuffle(&rows, 0xC0FFEE, StatMode::Shuffle);
        // +5/+6/+7 never move.
        for (a, b) in rows.iter().zip(&plan) {
            assert_eq!(a[5..], b[5..], "passive/mask/slot bytes must stay put");
        }
        // The weapon stat tuples are exactly the original weapon tuples, permuted.
        let mut before_wpn = vec![rows[0][..STAT_LEN].to_vec(), rows[2][..STAT_LEN].to_vec()];
        let mut after_wpn = vec![plan[0][..STAT_LEN].to_vec(), plan[2][..STAT_LEN].to_vec()];
        before_wpn.sort();
        after_wpn.sort();
        assert_eq!(before_wpn, after_wpn, "weapon multiset preserved");
        // A weapon tuple never leaks into the body or head rows.
        assert_eq!(plan[4], rows[4], "the lone head row can only map to itself");
    }

    #[test]
    fn random_draws_within_category() {
        let rows = fixture();
        let plan = plan_bonus_shuffle(&rows, 7, StatMode::Random);
        let wpn_tuples = [rows[0][..STAT_LEN].to_vec(), rows[2][..STAT_LEN].to_vec()];
        for &i in &[0usize, 2] {
            assert!(
                wpn_tuples.contains(&plan[i][..STAT_LEN].to_vec()),
                "weapon row {i} drew a non-weapon tuple"
            );
            assert_eq!(plan[i][5..], rows[i][5..], "tail must stay put");
        }
    }

    #[test]
    fn deterministic_and_empty_noop() {
        let rows = fixture();
        assert_eq!(
            plan_bonus_shuffle(&rows, 1, StatMode::Shuffle),
            plan_bonus_shuffle(&rows, 1, StatMode::Shuffle)
        );
        assert!(plan_bonus_shuffle(&[], 1, StatMode::Shuffle).is_empty());
    }
}

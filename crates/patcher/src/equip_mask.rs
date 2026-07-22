//! Equip-character-mask randomizer: redistribute the `+6` equip-character mask
//! on the static `SCUS_942.54` equipment bonus table (`DAT_80074F68`).
//!
//! Every equippable item resolves through the shared item table to an 8-byte
//! bonus record (see [`legaia_asset::equip_stats`]). Byte `+6` is the
//! **equip-character mask** - bit `1` Vahn/Meta, `2` Noa/Terra, `4` Gala/Ozma
//! (`7` = anyone) - and it is the field the [`crate::equip_bonus`] stat pass
//! deliberately leaves welded to its row. This pass moves **only** that byte, so
//! it changes *who* can wear each piece of gear while every stat bonus, passive
//! slot, and slot type stays put. The two passes touch disjoint bytes, so they
//! compose: run both and a shuffled-stat sword also lands on a shuffled owner.
//!
//! The engine consumes the same `+6` byte (`legaia_engine_core::equipment::
//! DiscEquipInfo::can_equip`), so a patched disc genuinely re-gates each
//! character's equip picker.
//!
//! Like the stat pass it operates on bonus **rows**, not item ids (several items
//! can share one record; a per-id rewrite would double-edit a shared record and
//! could give two items conflicting masks). [`plan_mask_shuffle`] groups the
//! rows it is handed by their `+7` slot category and, under [`MaskMode::Shuffle`],
//! permutes the masks **within each category** - the per-category multiset of
//! masks is preserved, so each character keeps exactly the same *count* of
//! equippable weapons / body / head / footwear it had in retail (a
//! character can never be left with zero equippable gear in a slot); under
//! [`MaskMode::Random`] it draws each row's mask from its category pool. The
//! table lives in `SCUS_942.54`, so the edit is a same-size in-place SCUS patch.

use crate::rng::SplitMix64;

/// Re-exported [`crate::drops::DropMode`] so the mask pass shares the CLI's
/// `shuffle` / `random` vocabulary.
pub use crate::drops::DropMode as MaskMode;

/// Byte offset of the equip-character mask inside an 8-byte bonus record.
pub const MASK_OFFSET: usize = 6;

/// `+7` mask selecting the slot category (body / head / weapon / footwear); the
/// low bits (Ra-Seru flag) stay with the row, so they don't split the pool.
const SLOT_CATEGORY_MASK: u8 = 0x60;

/// Plan an equip-character-mask randomization over the given 8-byte bonus rows.
/// The rows are grouped by their `+7` slot category and only the `+6` mask byte
/// is reassigned within a group; every other byte is copied through untouched.
/// The returned vec has the same length and order as `rows`. Deterministic in
/// `(rows, seed, mode)`.
///
/// [`MaskMode::Shuffle`] permutes the masks within each category (a 1:1
/// reassignment, so the per-category multiset of masks is preserved - each
/// character keeps the same count of equippable gear per slot);
/// [`MaskMode::Random`] draws each row's mask from its category pool with
/// replacement.
pub fn plan_mask_shuffle(rows: &[[u8; 8]], seed: u64, mode: MaskMode) -> Vec<[u8; 8]> {
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
        let masks: Vec<u8> = members.iter().map(|&i| rows[i][MASK_OFFSET]).collect();
        match mode {
            MaskMode::Shuffle => {
                let mut bag = masks.clone();
                rng.shuffle(&mut bag);
                for (k, &i) in members.iter().enumerate() {
                    out[i][MASK_OFFSET] = bag[k];
                }
            }
            MaskMode::Random => {
                for &i in members {
                    out[i][MASK_OFFSET] = masks[rng.below(masks.len())];
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Three weapons (Vahn/Noa/Gala only), two body armors (any + Gala), one head,
    // in mixed order (mask in +6, slot byte in +7).
    fn fixture() -> Vec<[u8; 8]> {
        vec![
            [0, 50, 0, 0, 0, 0x40, 1, 0x40],  // weapon A, Vahn-only
            [0, 0, 30, 20, 0, 0x40, 7, 0x00], // body A, anyone
            [0, 20, 0, 0, 0, 0x40, 2, 0x40],  // weapon B, Noa-only
            [0, 0, 10, 5, 0, 0x40, 4, 0x00],  // body B, Gala-only
            [0, 30, 0, 0, 0, 0x40, 4, 0x40],  // weapon C, Gala-only
            [9, 0, 4, 0, 0, 0x40, 1, 0x20],   // head, Vahn-only
        ]
    }

    /// Per slot category, the sorted multiset of `+6` masks.
    fn category_masks(rows: &[[u8; 8]]) -> std::collections::BTreeMap<u8, Vec<u8>> {
        let mut m: std::collections::BTreeMap<u8, Vec<u8>> = std::collections::BTreeMap::new();
        for r in rows {
            m.entry(r[7] & 0x60).or_default().push(r[MASK_OFFSET]);
        }
        for v in m.values_mut() {
            v.sort_unstable();
        }
        m
    }

    #[test]
    fn shuffle_preserves_per_category_mask_multiset_and_keeps_rest() {
        let rows = fixture();
        let plan = plan_mask_shuffle(&rows, 0xC0FFEE, MaskMode::Shuffle);
        // Every byte except +6 stays put.
        for (a, b) in rows.iter().zip(&plan) {
            for j in 0..8 {
                if j != MASK_OFFSET {
                    assert_eq!(a[j], b[j], "only the +6 mask byte may change");
                }
            }
        }
        // Per-category mask multiset preserved (no mask crosses categories).
        assert_eq!(category_masks(&plan), category_masks(&rows));
        // The lone head row can only map to itself.
        assert_eq!(plan[5][MASK_OFFSET], rows[5][MASK_OFFSET]);
    }

    #[test]
    fn random_draws_within_category_pool() {
        let rows = fixture();
        let plan = plan_mask_shuffle(&rows, 7, MaskMode::Random);
        // Weapon rows can only draw a mask that exists among the weapons.
        let weapon_masks = [1u8, 2, 4];
        for &i in &[0usize, 2, 4] {
            assert!(
                weapon_masks.contains(&plan[i][MASK_OFFSET]),
                "weapon row {i} drew a non-weapon mask {}",
                plan[i][MASK_OFFSET]
            );
        }
        // A drawn mask is never zero (retail masks are all non-zero, so the pool
        // can't produce an unequippable row).
        for r in &plan {
            assert_ne!(r[MASK_OFFSET], 0, "no row should become unequippable");
        }
    }

    #[test]
    fn deterministic_and_empty_noop() {
        let rows = fixture();
        assert_eq!(
            plan_mask_shuffle(&rows, 1, MaskMode::Shuffle),
            plan_mask_shuffle(&rows, 1, MaskMode::Shuffle)
        );
        assert!(plan_mask_shuffle(&[], 1, MaskMode::Shuffle).is_empty());
    }
}

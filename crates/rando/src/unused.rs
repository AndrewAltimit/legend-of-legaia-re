//! Curated "unused content" the game ships but never surfaces in normal play,
//! and which the randomizer can optionally bring back.
//!
//! These are *opt-in* expansions of the randomizer's pools: a normal run never
//! places them (they stay invisible, exactly as in retail), but the
//! `--unused-enemies` / `--unused-items` toggles widen the candidate pools to
//! include them. The ids here are pinned from the disc, not guessed — see each
//! constant's note for the provenance.
//!
//! ## Why "unused"
//!
//! Both the monster archive (`battle_data`, PROT 867) and the item-name table
//! (`SCUS_942.54`) carry more populated records than the shipped game ever
//! references. The encounter formations never name these monster ids and no
//! shop / chest / drop / steal ever hands out these items, so a vanilla player
//! can't meet or hold them. They are nevertheless *complete* records — the
//! battle loader streams a monster's `0x14000` slot on demand keyed by its id
//! (there is no separate per-scene preload list, so any in-archive id loads and
//! renders), and the item table has valid metadata for the item slots — so
//! re-introducing them via the randomizer is safe.

/// Unused enemy monster ids the `--unused-enemies` toggle can inject into random
/// encounters.
///
/// **"Comm" — id 78.** A complete, standalone monster record (HP 2520, AGL/etc.
/// stats, casts magic `0x23`, exp 945, no drop) that **no** scene formation
/// references — a genuine cut/unused enemy, not a duplicate. Spawnable like any
/// other (the loader streams its slot by id).
///
/// **Evil Bat duplicates — ids 176, 177, 178.** The monster archive holds 186
/// populated 1-based records (`slot = (id - 1) * 0x14000`; the id is the global
/// battle-loader index, derived from the slot, not stored). Ids 176, 177, 178
/// are byte-identical clones of each other *and* of the in-use Evil Bat at id
/// 140 (FNV `0xc400ee45d9c2b252` for all four; HP 390, no drop). No formation in
/// any scene's MAN references 176/177/178, so they never spawn in retail.
///
/// Because monster assets load per-formation by id (confirmed: the battle loader
/// `FUN_800542C8` streams `(id - 1) * 0x14000` from PROT 867 for whatever id is
/// in the formation cell — there is no per-scene monster preload array in the
/// MAN), injecting one of these ids into a formation id byte is sufficient to
/// make it appear; nothing else needs patching.
pub const UNUSED_ENEMY_IDS: &[u8] = &[78, 176, 177, 178];

/// Unused item ids the `--unused-items` toggle adds to the valid item pool (so
/// the `random` drop / chest / steal fills can hand them out).
///
/// - **`0x6B` "Something Good".** Named in the item table but never sold,
///   dropped, stolen, or placed in a chest in retail — its only behaviour is a
///   50,000 G sell value. (It *is* named, so [`crate::items::valid_item_pool`]
///   already includes it; it is listed here so the toggle's intent — "bring back
///   the unused items" — is explicit and the pair is documented in one place.)
/// - **`0xFD` the unnamed accessory.** An accessory-class slot
///   (metadata class byte `0x02`, per-class index `0x7e`, continuing the
///   accessory sequence unbroken) whose name string is **empty**, so
///   [`crate::items::valid_item_pool`] excludes it. The toggle is what makes it
///   obtainable. Its documented effect is to make only Seru-class enemies appear
///   in random encounters; because it is unobtainable in retail that effect is
///   never exercised, so treat it as experimental.
pub const UNUSED_ITEM_IDS: &[u8] = &[0x6B, 0xFD];

/// Append `extra` to `pool`, skipping ids already present, preserving order
/// (existing pool first, then the new ids in their listed order). Used to widen
/// the item / monster pools by the curated unused sets without disturbing the
/// determinism of the base pool's ordering.
pub fn extend_pool(pool: &mut Vec<u8>, extra: &[u8]) {
    for &id in extra {
        if !pool.contains(&id) {
            pool.push(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unused_enemy_ids_are_comm_and_the_three_evil_bat_clones() {
        assert_eq!(UNUSED_ENEMY_IDS, &[78, 176, 177, 178]);
    }

    #[test]
    fn unused_items_carry_something_good_and_the_unnamed_accessory() {
        assert!(UNUSED_ITEM_IDS.contains(&0x6B), "Something Good");
        assert!(UNUSED_ITEM_IDS.contains(&0xFD), "unnamed accessory");
    }

    #[test]
    fn extend_pool_dedups_and_appends_in_order() {
        let mut pool = vec![1, 2, 0x6B];
        extend_pool(&mut pool, UNUSED_ITEM_IDS);
        // 0x6B already present (not re-added); 0xFD appended.
        assert_eq!(pool, vec![1, 2, 0x6B, 0xFD]);
        // Idempotent.
        let snapshot = pool.clone();
        extend_pool(&mut pool, UNUSED_ITEM_IDS);
        assert_eq!(pool, snapshot);
    }
}

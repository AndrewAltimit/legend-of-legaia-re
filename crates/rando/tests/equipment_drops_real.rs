//! End-to-end disc-gated test for the equipment-as-enemy-drops randomizer:
//! build the equipment pool from the real `SCUS_942.54`, plan a full
//! every-monster equipment drop, apply it to a scratch copy of the disc, and
//! re-decode every monster's drop off the patched image to confirm it now
//! drops a pool equipment id at a tiered (1..=3 %) chance — and that a fixed
//! seed is byte-deterministic.
//!
//! Gates on `LEGAIA_DISC_BIN`; skips+passes when unset. The patched image lives
//! only in memory (never written to disk).

use legaia_iso::iso9660::read_file_in_image;
use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::equipment::{self, equipment_pool};
use std::collections::HashSet;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

#[test]
fn equipment_pool_classifies_weapons_armor_accessories() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS in image");
    let pool = equipment_pool(&scus).expect("build equipment pool");

    // The retail corpus matches ~150 of the 155 curated equipment names
    // (a few character-default weapons + quest items don't match by name).
    assert!(
        pool.len() >= 120,
        "equipment pool unexpectedly small: {}",
        pool.len()
    );
    // Ids are unique and sorted.
    let ids: Vec<u8> = pool.iter().map(|e| e.id).collect();
    let unique: HashSet<u8> = ids.iter().copied().collect();
    assert_eq!(ids.len(), unique.len(), "pool ids must be unique");
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted, "pool is sorted by id");

    // Spot-check a few known ids land in the pool with the expected price tier.
    // Survival Knife (0x22) is cheap early gear; Magic Ring (0xC2) is pricey.
    let by_id = |id: u8| pool.iter().find(|e| e.id == id).copied();
    let knife = by_id(0x22).expect("Survival Knife (0x22) in pool");
    assert!(
        knife.price.is_some_and(|p| p <= equipment::EARLY_PRICE_MAX),
        "Survival Knife should tier early"
    );
    // It must NOT contain the stray in-range consumable Honey (0x65) — name
    // matching against the equipment tables excludes it.
    assert!(
        by_id(0x65).is_none(),
        "Honey (0x65) is a consumable, not gear"
    );
}

#[test]
fn every_monster_drops_pool_equipment_at_a_tiered_rate() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS in image");
    let pool = equipment_pool(&scus).expect("pool");
    let pool_ids: HashSet<u8> = pool.iter().map(|e| e.id).collect();

    let seed = 0xC0FFEE_u64;
    let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let (plan, report) = apply::randomize_equipment_drops(&mut patcher, &pool, seed).unwrap();

    assert!(!plan.is_empty(), "plan covers monsters");
    assert_eq!(
        report.changed + report.skipped.len(),
        plan.len(),
        "every planned monster is either written or skipped"
    );

    // Re-decode every monster's drop straight off the patched image; the ones
    // that were written must now carry a pool equipment id at a 1..=3 % chance.
    let skipped: HashSet<u16> = report.skipped.iter().copied().collect();
    let entry = patcher
        .read_entry(legaia_rando::disc::MONSTER_ARCHIVE_ENTRY)
        .unwrap();
    let records = legaia_asset::monster_archive::records(&entry).unwrap();
    let mut checked = 0;
    for r in &records {
        if skipped.contains(&r.id) {
            continue;
        }
        assert!(
            pool_ids.contains(&r.drop_item),
            "monster {} drops {} which is not pool equipment",
            r.id,
            r.drop_item
        );
        assert!(
            (1..=3).contains(&r.drop_chance_pct),
            "monster {} chance {} not a tiered rate",
            r.id,
            r.drop_chance_pct
        );
        checked += 1;
    }
    assert!(checked > 100, "checked a meaningful number of monsters");

    // Same seed => identical plan (byte-determinism of the published seed).
    let mut p2 = DiscPatcher::open(disc).expect("reopen disc");
    let (plan2, _) = apply::randomize_equipment_drops(&mut p2, &pool, seed).unwrap();
    assert_eq!(plan, plan2, "fixed seed is deterministic");
}

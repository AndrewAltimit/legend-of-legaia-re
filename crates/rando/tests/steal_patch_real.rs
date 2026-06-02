//! Disc-gated end-to-end test for the steal-item randomizer: shuffle the
//! per-monster steal items in the static `SCUS_942.54` table on a scratch copy
//! of the disc, then re-read the patched table straight off the patched image
//! and confirm the edit is faithful — item multiset preserved (shuffle), every
//! steal chance byte untouched, the touched `SCUS_942.54` sectors EDC/ECC-valid,
//! and a fixed seed byte-deterministic. Skips + passes without `LEGAIA_DISC_BIN`.

use legaia_iso::iso9660::find_file_in_image;
use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_SIZE};
use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::drops::DropMode;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// `(sorted (monster_id, chance) pairs, sorted item multiset)` for every
/// stealable monster — the invariants a shuffle must preserve.
fn snapshot(patcher: &DiscPatcher) -> (Vec<(u16, u8)>, Vec<u8>) {
    let steals = apply::current_steals(patcher).expect("read steal table");
    let mut chances: Vec<(u16, u8)> = steals.iter().map(|s| (s.monster_id, s.chance)).collect();
    let mut items: Vec<u8> = steals.iter().map(|s| s.item).collect();
    chances.sort_unstable();
    items.sort_unstable();
    (chances, items)
}

#[test]
fn shuffle_steals_round_trips_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let seed = 0x5EA1_F00D_1234_ABCD;

    let base = DiscPatcher::open(original.clone()).expect("open");
    let (before_chances, before_items) = snapshot(&base);
    assert!(
        before_items.len() > 150,
        "expected many stealable monsters, found {}",
        before_items.len()
    );

    // Shuffle the steal items on a scratch copy.
    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    let (plan, report) =
        apply::randomize_steals(&mut patcher, &[], seed, DropMode::Shuffle).expect("randomize");
    assert!(report.items_changed > 0, "should change at least one steal");
    assert_eq!(plan.len(), before_items.len(), "all stealable planned");

    // Re-read the steal table off the PATCHED image.
    let (after_chances, after_items) = snapshot(&patcher);
    // Shuffle preserves the exact item multiset...
    assert_eq!(
        after_items, before_items,
        "shuffle must preserve the steal-item multiset"
    );
    // ...and never touches any monster's steal chance (item-only edit).
    assert_eq!(
        after_chances, before_chances,
        "steal chances must be untouched by an item shuffle"
    );

    // The patched SCUS_942.54 sectors stay EDC/ECC-valid.
    let img = patcher.image();
    let (scus_lba, _scus_size) = find_file_in_image(img, "SCUS_942.54").unwrap();
    // The steal table sits ~0x68028 into SCUS; that byte's sector must be valid.
    let table_sector = scus_lba as usize + 0x68028 / USER_DATA_SIZE;
    let sb = table_sector * SECTOR_SIZE;
    assert!(
        legaia_iso::write::mode2_form1_sector_is_valid(&img[sb..sb + SECTOR_SIZE]),
        "patched steal-table sector must be EDC/ECC-valid"
    );

    // Determinism: same seed -> byte-identical patched image.
    let mut patcher2 = DiscPatcher::open(original).expect("open");
    let (_p2, report2) =
        apply::randomize_steals(&mut patcher2, &[], seed, DropMode::Shuffle).expect("randomize");
    assert_eq!(report2.items_changed, report.items_changed);
    assert!(
        patcher2.image() == patcher.image(),
        "same seed must reproduce the patched image"
    );

    eprintln!(
        "steals shuffle seed {seed:#x}: {} of {} stealable monsters changed; multiset + chances preserved",
        report.items_changed,
        plan.len()
    );
}

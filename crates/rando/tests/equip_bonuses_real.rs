//! Disc-gated end-to-end test for the equipment stat-bonus randomizer: shuffle
//! the passive stat tuples in the `SCUS_942.54` bonus table (`DAT_80074F68`) on
//! a scratch copy of the disc, re-read the patched table off the patched image,
//! and confirm:
//!
//! - the `+5/+6/+7` tail bytes (accessory passive / equip mask / slot type) of
//!   every row are untouched - only the `+0..+4` stat tuple moves;
//! - per slot category, the multiset of stat tuples is preserved (a shuffle is a
//!   1:1 reassignment within a category, so the gear-power budget is kept and no
//!   tuple crosses a category boundary);
//! - the touched SCUS sectors stay EDC/ECC-valid;
//! - a fixed seed reproduces the patched image byte-for-byte.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use std::collections::BTreeMap;

use legaia_asset::equip_stats::{EquipStatTable, bonus_table_file_offset};
use legaia_iso::iso9660::find_file_in_image;
use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_SIZE};
use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::drops::DropMode;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// The raw 8-byte bonus rows, read off a patcher's `SCUS_942.54`.
fn raw_rows(patcher: &DiscPatcher) -> Vec<[u8; 8]> {
    let scus = patcher
        .read_named_file("SCUS_942.54")
        .expect("SCUS present");
    let table = EquipStatTable::from_scus(&scus).expect("bonus table parses");
    table.rows().iter().map(|b| b.raw).collect()
}

/// Per slot category (`+7 & 0x60`), the sorted multiset of `+0..+4` stat tuples.
fn category_multisets(rows: &[[u8; 8]]) -> BTreeMap<u8, Vec<[u8; 5]>> {
    let mut m: BTreeMap<u8, Vec<[u8; 5]>> = BTreeMap::new();
    for r in rows {
        let mut t = [0u8; 5];
        t.copy_from_slice(&r[..5]);
        m.entry(r[7] & 0x60).or_default().push(t);
    }
    for v in m.values_mut() {
        v.sort_unstable();
    }
    m
}

#[test]
fn shuffle_equip_bonuses_round_trips_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let seed = 0x5E1F_B045_DE00_0001;

    let base = DiscPatcher::open(original.clone()).expect("open");
    let before = raw_rows(&base);
    assert!(
        before.len() > 50,
        "expected many bonus rows, found {}",
        before.len()
    );
    let before_cats = category_multisets(&before);
    assert!(
        before_cats.len() >= 3,
        "expected several slot categories, found {}",
        before_cats.len()
    );

    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    let changed =
        apply::randomize_equip_bonuses(&mut patcher, seed, DropMode::Shuffle).expect("randomize");
    assert!(changed > 0, "a shuffle should move at least one bonus row");

    // Re-read off the PATCHED image.
    let after = raw_rows(&patcher);
    assert_eq!(after.len(), before.len(), "row count must be unchanged");

    // Tail bytes (passive / mask / slot) never move.
    for (i, (a, b)) in before.iter().zip(&after).enumerate() {
        assert_eq!(
            a[5..],
            b[5..],
            "row {i}: +5/+6/+7 (passive/mask/slot) must stay put"
        );
    }

    // Per-category stat-tuple multiset preserved (no tuple crosses categories).
    assert_eq!(
        category_multisets(&after),
        before_cats,
        "shuffle must preserve each slot category's stat-tuple multiset"
    );

    // Every SCUS sector the bonus table spans stays EDC/ECC-valid.
    let img = patcher.image();
    let (scus_lba, _) = find_file_in_image(img, "SCUS_942.54").unwrap();
    let scus =
        legaia_iso::iso9660::read_file_in_image(&original, "SCUS_942.54").expect("read SCUS");
    let off = bonus_table_file_offset(&scus).expect("bonus table offset");
    let span = before.len() * 8;
    let first = off / USER_DATA_SIZE;
    let last = (off + span - 1) / USER_DATA_SIZE;
    for s in first..=last {
        let sector = scus_lba as usize + s;
        let sb = sector * SECTOR_SIZE;
        assert!(
            legaia_iso::write::mode2_form1_sector_is_valid(&img[sb..sb + SECTOR_SIZE]),
            "patched bonus-table sector {sector} must be EDC/ECC-valid"
        );
    }

    // Determinism.
    let mut patcher2 = DiscPatcher::open(original).expect("open");
    let changed2 =
        apply::randomize_equip_bonuses(&mut patcher2, seed, DropMode::Shuffle).expect("randomize");
    assert_eq!(changed2, changed);
    assert!(
        patcher2.image() == patcher.image(),
        "same seed must reproduce the patched image"
    );

    eprintln!(
        "equip-bonus shuffle seed {seed:#x}: {changed} of {} rows changed; \
         per-category multiset + tail bytes preserved across {} categories",
        before.len(),
        before_cats.len()
    );
}

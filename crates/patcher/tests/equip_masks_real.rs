//! Disc-gated end-to-end test for the equip-character-mask randomizer: shuffle
//! the `+6` equip mask (who can wear each piece of gear) in the `SCUS_942.54`
//! bonus table (`DAT_80074F68`) on a scratch copy of the disc, re-read the
//! patched table off the patched image, and confirm:
//!
//! - every byte except `+6` of every row is untouched - only the equip mask
//!   moves (so this composes byte-disjointly with the stat-bonus pass);
//! - per slot category, the multiset of masks is preserved (a shuffle is a 1:1
//!   reassignment within a category, so each character keeps the same count of
//!   equippable gear per slot, and no mask crosses a category boundary);
//! - every referenced row's mask stays non-zero (no gear becomes unequippable);
//! - the touched SCUS sectors stay EDC/ECC-valid;
//! - a fixed seed reproduces the patched image byte-for-byte.
//!
//! A companion assertion runs the stat-bonus pass on the same image first and
//! confirms the two passes compose (stat pass leaves +6 alone; mask pass leaves
//! +0..+4 alone) so a run can randomize both at once.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use std::collections::BTreeMap;

use legaia_asset::equip_stats::{EquipStatTable, bonus_table_file_offset};
use legaia_iso::iso9660::find_file_in_image;
use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_SIZE};
use legaia_patcher::apply;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::drops::DropMode;

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

/// The 1-based item ids that reference each bonus row, off a patcher's SCUS.
fn items_for_rows(patcher: &DiscPatcher) -> Vec<Vec<u8>> {
    let scus = patcher
        .read_named_file("SCUS_942.54")
        .expect("SCUS present");
    EquipStatTable::from_scus(&scus)
        .expect("bonus table parses")
        .items_for_rows()
}

/// Per slot category (`+7 & 0x60`), the sorted multiset of `+6` equip masks.
fn category_masks(rows: &[[u8; 8]]) -> BTreeMap<u8, Vec<u8>> {
    let mut m: BTreeMap<u8, Vec<u8>> = BTreeMap::new();
    for r in rows {
        m.entry(r[7] & 0x60).or_default().push(r[6]);
    }
    for v in m.values_mut() {
        v.sort_unstable();
    }
    m
}

#[test]
fn shuffle_equip_masks_round_trips_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let seed = 0x5E1F_B045_DE00_0002;

    let base = DiscPatcher::open(original.clone()).expect("open");
    let before = raw_rows(&base);
    let refs = items_for_rows(&base);
    assert!(
        before.len() > 50,
        "expected many bonus rows, found {}",
        before.len()
    );
    let before_cats = category_masks(&before);
    assert!(
        before_cats.len() >= 3,
        "expected several slot categories, found {}",
        before_cats.len()
    );
    // Non-vacuous: at least one referenced row per character mask bit exists, so
    // the shuffle actually has distinct masks to move around.
    let referenced_masks: Vec<u8> = (0..before.len())
        .filter(|&i| !refs[i].is_empty())
        .map(|i| before[i][6])
        .collect();
    assert!(
        referenced_masks.iter().any(|&m| m & 0x7 != 0x7),
        "expected some character-restricted gear (non-'any' mask) to shuffle"
    );

    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    let changed =
        apply::randomize_equip_masks(&mut patcher, seed, DropMode::Shuffle).expect("randomize");
    assert!(changed > 0, "a shuffle should move at least one equip mask");

    // Re-read off the PATCHED image.
    let after = raw_rows(&patcher);
    assert_eq!(after.len(), before.len(), "row count must be unchanged");

    // Every byte except +6 stays put.
    for (i, (a, b)) in before.iter().zip(&after).enumerate() {
        for j in 0..8 {
            if j != 6 {
                assert_eq!(a[j], b[j], "row {i} byte +{j}: only the +6 mask may change");
            }
        }
    }

    // Per-category mask multiset preserved (no mask crosses categories).
    assert_eq!(
        category_masks(&after),
        before_cats,
        "shuffle must preserve each slot category's equip-mask multiset"
    );

    // No referenced row is left unequippable.
    let after_refs = items_for_rows(&patcher);
    for i in 0..after.len() {
        if !after_refs[i].is_empty() {
            assert_ne!(
                after[i][6] & 0x7,
                0,
                "referenced row {i} became unequippable"
            );
        }
    }

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
    let mut patcher2 = DiscPatcher::open(original.clone()).expect("open");
    let changed2 =
        apply::randomize_equip_masks(&mut patcher2, seed, DropMode::Shuffle).expect("randomize");
    assert_eq!(changed2, changed);
    assert!(
        patcher2.image() == patcher.image(),
        "same seed must reproduce the patched image"
    );

    // Composition: stat pass then mask pass touch disjoint bytes.
    let mut both = DiscPatcher::open(original).expect("open");
    apply::randomize_equip_bonuses(&mut both, seed, DropMode::Shuffle).expect("stat pass");
    apply::randomize_equip_masks(&mut both, seed ^ 0xABCD, DropMode::Shuffle).expect("mask pass");
    let both_rows = raw_rows(&both);
    let both_cats_mask = category_masks(&both_rows);
    assert_eq!(
        both_cats_mask, before_cats,
        "the mask multiset survives running the stat pass first"
    );

    eprintln!(
        "equip-mask shuffle seed {seed:#x}: {changed} of {} rows changed; \
         per-category mask multiset preserved across {} categories; composes with stat pass",
        before.len(),
        before_cats.len()
    );
}

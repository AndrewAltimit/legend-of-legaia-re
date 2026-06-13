//! Disc-gated end-to-end test for the spell MP-cost randomizer: shuffle the
//! named, costed spells' MP costs in the `SCUS_942.54` spell table on a scratch
//! copy of the disc, then re-read the patched table off the patched image and
//! confirm:
//!
//! - the MP-cost multiset is preserved (a shuffle is a 1:1 reassignment);
//! - the set of named, costed spells is unchanged (only the +3 byte moves, so
//!   no spell loses its name or gains/loses its costed status);
//! - the touched SCUS sectors stay EDC/ECC-valid;
//! - a fixed seed reproduces the patched image byte-for-byte.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_iso::iso9660::find_file_in_image;
use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_SIZE};
use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::drops::DropMode;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// `(sorted (id) list, sorted MP multiset)` — the invariants a shuffle keeps.
fn snapshot(patcher: &DiscPatcher) -> (Vec<u8>, Vec<u8>) {
    let spells = apply::current_spell_costs(patcher)
        .expect("read spell costs")
        .expect("spell table present");
    let mut ids: Vec<u8> = spells.iter().map(|s| s.id).collect();
    let mut mp: Vec<u8> = spells.iter().map(|s| s.mp).collect();
    ids.sort_unstable();
    mp.sort_unstable();
    (ids, mp)
}

#[test]
fn shuffle_spell_costs_round_trips_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let seed = 0x05EA_1F00_5DE1_0001;

    let base = DiscPatcher::open(original.clone()).expect("open");
    let (before_ids, before_mp) = snapshot(&base);
    assert!(
        before_mp.len() > 20,
        "expected many costed spells, found {}",
        before_mp.len()
    );

    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    let changed =
        apply::randomize_spell_costs(&mut patcher, seed, DropMode::Shuffle).expect("randomize");
    assert!(changed > 0, "a shuffle should move at least one MP cost");

    // Re-read off the PATCHED image.
    let (after_ids, after_mp) = snapshot(&patcher);
    assert_eq!(
        after_ids, before_ids,
        "the costed-spell set must be unchanged"
    );
    assert_eq!(
        after_mp, before_mp,
        "shuffle must preserve the MP-cost multiset"
    );

    // The patched SCUS spell-table sector stays EDC/ECC-valid.
    let img = patcher.image();
    let (scus_lba, _) = find_file_in_image(img, "SCUS_942.54").unwrap();
    let scus =
        legaia_iso::iso9660::read_file_in_image(&original, "SCUS_942.54").expect("read SCUS");
    let table_off = legaia_asset::spell_names::stats_file_offset(&scus).expect("stats offset");
    let table_sector = scus_lba as usize + table_off / USER_DATA_SIZE;
    let sb = table_sector * SECTOR_SIZE;
    assert!(
        legaia_iso::write::mode2_form1_sector_is_valid(&img[sb..sb + SECTOR_SIZE]),
        "patched spell-table sector must be EDC/ECC-valid"
    );

    // Determinism.
    let mut patcher2 = DiscPatcher::open(original).expect("open");
    let changed2 =
        apply::randomize_spell_costs(&mut patcher2, seed, DropMode::Shuffle).expect("randomize");
    assert_eq!(changed2, changed);
    assert!(
        patcher2.image() == patcher.image(),
        "same seed must reproduce the patched image"
    );

    eprintln!(
        "spell-cost shuffle seed {seed:#x}: {changed} of {} costed spells changed; multiset + id set preserved",
        before_mp.len()
    );
}

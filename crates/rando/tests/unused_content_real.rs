//! Disc-gated validation of the "unused content" curated sets and their opt-in
//! re-introduction.
//!
//! Pins, against the real disc, the facts the toggles rely on:
//! - the unused Evil Bat monster ids ([`legaia_rando::unused::UNUSED_ENEMY_IDS`])
//!   exist and are byte-identical clones of each other and of the in-use Evil
//!   Bat (id 140);
//! - the unused items ([`legaia_rando::unused::UNUSED_ITEM_IDS`]): "Something
//!   Good" (`0x6B`) is named (so already in the valid pool) while the unnamed
//!   accessory (`0xFD`) has no name (so excluded — the toggle is what adds it);
//! - turning the toggle on actually injects an unused enemy into the
//!   random-encounter result, and leaving it off injects none.
//!
//! Skips + passes without `LEGAIA_DISC_BIN`.

use legaia_asset::item_names::ItemNameTable;
use legaia_asset::monster_archive::SLOT_STRIDE;
use legaia_iso::iso9660::find_file_in_image;
use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_SIZE};
use legaia_rando::apply;
use legaia_rando::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};
use legaia_rando::drops::DropMode;
use legaia_rando::encounter::SceneEncounters;
use legaia_rando::item_name::{SERU_BELL_ID, SERU_BELL_NAME};
use legaia_rando::items::valid_item_pool;
use legaia_rando::unused::{self, UNUSED_ENEMY_IDS, UNUSED_ITEM_IDS};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// The in-use Evil Bat id every unused clone duplicates (1-based archive index).
const EVIL_BAT_INUSE_ID: u16 = 140;

#[test]
fn unused_evil_bat_ids_are_byte_identical_clones() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open disc");
    let archive = patcher
        .read_entry(MONSTER_ARCHIVE_ENTRY)
        .expect("read monster archive");

    let slot = |id: u16| -> &[u8] {
        let start = (id as usize - 1) * SLOT_STRIDE;
        &archive[start..start + SLOT_STRIDE]
    };
    let reference = slot(EVIL_BAT_INUSE_ID);
    for &id in UNUSED_ENEMY_IDS {
        assert_eq!(
            slot(id as u16),
            reference,
            "unused enemy id {id} must be a byte-identical clone of the in-use Evil Bat (id {EVIL_BAT_INUSE_ID})"
        );
    }

    // The reference really is the Evil Bat (name lives at a block-relative
    // pointer; the parser resolves it).
    let rec = legaia_asset::monster_archive::record(&archive, EVIL_BAT_INUSE_ID)
        .expect("decode record")
        .expect("record present");
    assert!(
        rec.name.contains("Evil Bat"),
        "id {EVIL_BAT_INUSE_ID} name is {:?}, expected to contain \"Evil Bat\"",
        rec.name
    );
}

#[test]
fn unused_items_split_named_vs_unnamed() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open disc");
    let scus = patcher
        .read_named_file("SCUS_942.54")
        .expect("read SCUS_942.54");
    let table = ItemNameTable::from_scus(&scus).expect("parse item table");

    // "Something Good" is named (so the valid pool already carries it).
    let something_good = table.name(0x6B);
    assert!(
        something_good.is_some_and(|n| !n.is_empty()),
        "item 0x6B must be a named item, got {something_good:?}"
    );
    // The unnamed accessory has no name string (so the valid pool excludes it).
    assert!(
        table.name(0xFD).is_none(),
        "item 0xFD must be the unnamed accessory (no name), got {:?}",
        table.name(0xFD)
    );

    let base_pool = valid_item_pool(&scus).expect("build pool");
    assert!(
        base_pool.contains(&0x6B),
        "0x6B is named -> already in pool"
    );
    assert!(
        !base_pool.contains(&0xFD),
        "0xFD is unnamed -> excluded from the base pool"
    );

    // The toggle's pool extension adds exactly the missing unused id (0xFD) and
    // is a no-op for the already-present one (0x6B).
    let mut widened = base_pool.clone();
    unused::extend_pool(&mut widened, UNUSED_ITEM_IDS);
    assert!(widened.contains(&0x6B) && widened.contains(&0xFD));
    assert_eq!(
        widened.len(),
        base_pool.len() + 1,
        "only the unnamed accessory is genuinely new"
    );
}

/// Count, across every scene, how many formation id slots hold an unused-enemy
/// id — re-decoding each scene MAN straight off the (possibly patched) image.
fn unused_spawns_on_disc(patcher: &DiscPatcher) -> usize {
    let mut n = 0;
    for idx in 0..patcher.entry_count() {
        let Ok(entry) = patcher.read_entry(idx) else {
            continue;
        };
        if let Some(scene) = SceneEncounters::locate(&entry, idx) {
            n += scene.count_ids_in(UNUSED_ENEMY_IDS);
        }
    }
    n
}

#[test]
fn unused_enemy_toggle_injects_only_when_enabled() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let seed = 0x00E7_1BA7u64;

    // Vanilla: no formation references an unused enemy.
    let base = DiscPatcher::open(disc.clone()).expect("open disc");
    assert_eq!(
        unused_spawns_on_disc(&base),
        0,
        "vanilla scenes must reference no unused enemy"
    );

    // Toggle OFF: a random encounter pass draws only from each scene's own pool,
    // so it still places zero unused enemies.
    let mut off = DiscPatcher::open(disc.clone()).expect("open disc");
    apply::randomize_encounters(&mut off, seed, DropMode::Random, &[]).expect("encounters off");
    assert_eq!(
        unused_spawns_on_disc(&off),
        0,
        "with the toggle off the unused enemies never enter the pool"
    );

    // Toggle ON: the unused ids join the Random pool and get placed.
    let mut on = DiscPatcher::open(disc).expect("open disc");
    let report = apply::randomize_encounters(&mut on, seed, DropMode::Random, UNUSED_ENEMY_IDS)
        .expect("encounters on");
    assert!(
        report.unused_placed > 0,
        "the toggle must inject at least one unused enemy (got {})",
        report.unused_placed
    );
    // Re-decode off the patched image: the placements are real disc bytes, and
    // the report count matches the written-back scenes.
    let on_disk = unused_spawns_on_disc(&on);
    assert!(
        on_disk >= report.unused_placed,
        "patched image must carry the injected unused enemies (disk {on_disk} >= reported {})",
        report.unused_placed
    );

    // Deterministic for a fixed seed.
    let mut on2 =
        DiscPatcher::open(std::fs::read(std::env::var_os("LEGAIA_DISC_BIN").unwrap()).unwrap())
            .expect("reopen disc");
    let report2 = apply::randomize_encounters(&mut on2, seed, DropMode::Random, UNUSED_ENEMY_IDS)
        .expect("encounters on (2)");
    assert_eq!(
        report.unused_placed, report2.unused_placed,
        "same seed must reproduce the injection count"
    );
}

#[test]
fn seru_bell_name_injection_names_only_the_accessory() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let original_len = disc.len();

    // Baseline: the accessory is unnamed; the other ids share the empty slot.
    let base = DiscPatcher::open(disc.clone()).expect("open disc");
    let base_scus = base.read_named_file("SCUS_942.54").expect("read SCUS");
    let base_table = ItemNameTable::from_scus(&base_scus).expect("parse table");
    assert!(
        base_table.name(SERU_BELL_ID).is_none(),
        "0xFD starts unnamed"
    );
    let shared_unnamed = [0x12u8, 0x1A, 0x52, 0xB9];
    for &id in &shared_unnamed {
        assert!(base_table.name(id).is_none(), "id {id:#x} starts unnamed");
    }

    // Inject the name on a scratch copy.
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    let set = apply::inject_seru_bell_name(&mut patcher)
        .expect("inject")
        .expect("a fresh disc must get the name set");
    assert_eq!(set, SERU_BELL_NAME);

    // Re-read the table off the PATCHED image.
    let scus = patcher
        .read_named_file("SCUS_942.54")
        .expect("read patched SCUS");
    let table = ItemNameTable::from_scus(&scus).expect("parse patched table");
    assert_eq!(
        table.name(SERU_BELL_ID),
        Some(SERU_BELL_NAME),
        "the accessory now resolves to its injected name"
    );
    // The other ids that shared the empty-string slot are untouched.
    for &id in &shared_unnamed {
        assert!(
            table.name(id).is_none(),
            "id {id:#x} must stay unnamed (only 0xFD's pointer was repointed)"
        );
    }

    // Same-size patch + the touched SCUS sectors stay EDC/ECC-valid.
    assert_eq!(patcher.image().len(), original_len, "image size unchanged");
    let img = patcher.image();
    let (scus_lba, _) = find_file_in_image(img, "SCUS_942.54").unwrap();
    let (ptr_off, _) = legaia_asset::item_names::name_ptr_slot(&scus, SERU_BELL_ID).unwrap();
    let str_off = legaia_asset::item_names::file_offset_for_va(
        &base_scus,
        legaia_rando::item_name::SERU_BELL_STRING_VA,
    )
    .unwrap();
    for byte_off in [ptr_off, str_off] {
        let sector = scus_lba as usize + byte_off / USER_DATA_SIZE;
        let sb = sector * SECTOR_SIZE;
        assert!(
            legaia_iso::write::mode2_form1_sector_is_valid(&img[sb..sb + SECTOR_SIZE]),
            "patched SCUS sector at byte {byte_off:#x} must be EDC/ECC-valid"
        );
    }

    // Idempotent: re-running on the patched image makes no further change.
    assert!(
        apply::inject_seru_bell_name(&mut patcher)
            .expect("re-inject")
            .is_none(),
        "already-named accessory must not be re-injected"
    );
}

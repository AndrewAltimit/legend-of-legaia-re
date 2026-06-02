//! Disc-gated end-to-end test for the starting-item randomizer: rewrite the
//! new-game inventory-seed code in `SCUS_942.54` on a scratch copy of the disc,
//! then re-decode the seed straight off the patched image and confirm the edit
//! is faithful — the seeded items match the plan, every id is a valid consumable
//! from the pool, the surrounding `FUN_80034A6C` prologue/epilogue bytes are
//! untouched, the image is the same size, the touched SCUS sector stays
//! EDC/ECC-valid, and a fixed seed is byte-deterministic. Skips + passes without
//! `LEGAIA_DISC_BIN`.

use legaia_asset::new_game::{
    STARTING_INV_SEED_LEN, StartingInventory, starting_inv_seed_file_offset,
};
use legaia_iso::iso9660::{find_file_in_image, read_file_in_image};
use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_SIZE};
use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::starting_items::{MAX_STARTING_ITEMS, STARTING_ITEM_POOL, plan_starting_items};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

#[test]
fn randomize_starting_items_round_trips_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let seed = 0x57A4_7117_E115_0001;
    let n = 4;

    // Vanilla baseline: the new game starts with exactly Healing Leaf (0x77) x5.
    let base = DiscPatcher::open(original.clone()).expect("open");
    let before = apply::current_starting_items(&base).expect("read seed");
    assert_eq!(before, vec![(0x77, 5)], "retail new game = Healing Leaf x5");

    // The seed-region file offset + the surrounding bytes we must not disturb.
    let scus = read_file_in_image(&original, "SCUS_942.54").expect("SCUS present");
    let off = starting_inv_seed_file_offset(&scus).expect("seed offset");
    let before_prologue = scus[off - 16..off].to_vec();
    let after_region_orig =
        scus[off + STARTING_INV_SEED_LEN..off + STARTING_INV_SEED_LEN + 16].to_vec();

    // Randomize on a scratch copy.
    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    let report = apply::randomize_starting_items(&mut patcher, seed, n).expect("randomize");
    assert_eq!(report.items_set, n.min(MAX_STARTING_ITEMS));
    assert_eq!(
        report.items,
        plan_starting_items(seed, n),
        "report mirrors the plan"
    );

    // Re-decode the seed off the PATCHED image: it must equal the plan.
    let after = apply::current_starting_items(&patcher).expect("read patched seed");
    assert_eq!(
        after, report.items,
        "patched seed decodes to the planned items"
    );
    assert_ne!(after, before, "the seed actually changed");

    // Every seeded id is a valid consumable from the pool, with a sane count.
    let mut ids = Vec::new();
    for (id, count) in &after {
        assert!(
            STARTING_ITEM_POOL.contains(id),
            "id {id:#x} not in consumable pool"
        );
        assert!(*count >= 1, "seeded count must be >= 1");
        ids.push(*id);
    }
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), after.len(), "seeded ids are distinct");

    // The edit is confined to the 40-byte seed region: the function prologue
    // just before it and the code just after it are byte-identical.
    let patched_scus = read_file_in_image(patcher.image(), "SCUS_942.54").expect("SCUS");
    assert_eq!(
        &patched_scus[off - 16..off],
        &before_prologue[..],
        "prologue untouched"
    );
    assert_eq!(
        &patched_scus[off + STARTING_INV_SEED_LEN..off + STARTING_INV_SEED_LEN + 16],
        &after_region_orig[..],
        "code after the seed region untouched"
    );
    // Cross-check the decoder against the raw region bytes.
    let region = &patched_scus[off..off + STARTING_INV_SEED_LEN];
    assert_eq!(StartingInventory::decode_region(region).items(), &after[..]);

    // Same image size; the touched SCUS sector stays EDC/ECC-valid.
    assert_eq!(
        patcher.image().len(),
        original.len(),
        "image size unchanged"
    );
    let (scus_lba, _) = find_file_in_image(patcher.image(), "SCUS_942.54").unwrap();
    let seed_sector = scus_lba as usize + off / USER_DATA_SIZE;
    let sb = seed_sector * SECTOR_SIZE;
    assert!(
        legaia_iso::write::mode2_form1_sector_is_valid(&patcher.image()[sb..sb + SECTOR_SIZE]),
        "patched seed-region sector must be EDC/ECC-valid"
    );

    // Determinism: same seed -> byte-identical patched image.
    let mut patcher2 = DiscPatcher::open(original).expect("open");
    apply::randomize_starting_items(&mut patcher2, seed, n).expect("randomize");
    assert!(
        patcher2.image() == patcher.image(),
        "same seed must reproduce the patched image"
    );

    eprintln!(
        "starting-items seed {seed:#x}: {} random consumables seeded {after:?}",
        report.items_set
    );
}

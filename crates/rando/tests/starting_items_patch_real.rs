//! Disc-gated end-to-end test for the starting-item randomizer: rewrite the
//! new-game inventory-seed code in `SCUS_942.54` on a scratch copy of the disc,
//! then re-decode the seed straight off the patched image and confirm the edit
//! is faithful — the seeded items match the plan, every id is a valid consumable
//! from the pool, the surrounding `FUN_80034A6C` prologue/epilogue bytes are
//! untouched, the image is the same size, the touched SCUS sector stays
//! EDC/ECC-valid, and a fixed seed is byte-deterministic. Skips + passes without
//! `LEGAIA_DISC_BIN`.

use legaia_asset::new_game::{
    DOOR_OF_WIND_ITEM, STARTING_INV_SEED_LEN, StartingInventory, WARP_SEED_LEN,
    region_unlocks_all_warps, scus_unlocks_all_warps, starting_inv_seed_file_offset,
    warp_seed_file_offset,
};
use legaia_iso::iso9660::{find_file_in_image, read_file_in_image};
use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_SIZE};
use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::starting_items::{
    MAX_STARTING_ITEMS, STARTING_ITEM_POOL, StartingSeedOptions, plan_seed,
};

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
    let opts = StartingSeedOptions {
        random_items: n,
        door_of_wind: 0,
        incense: 0,
        speed_chain: 0,
        chicken_heart: 0,
        good_luck_bell: 0,
        all_warps: false,
    };

    // Vanilla baseline: the new game starts with exactly Healing Leaf (0x77) x5,
    // and no Door-of-Wind warp preset.
    let base = DiscPatcher::open(original.clone()).expect("open");
    let before = apply::current_starting_items(&base).expect("read seed");
    assert_eq!(before, vec![(0x77, 5)], "retail new game = Healing Leaf x5");
    assert!(
        !apply::current_all_warps(&base).expect("read warp flag"),
        "retail new game does not preset the all-warps bitmask"
    );

    // The seed-region file offset + the surrounding bytes we must not disturb.
    let scus = read_file_in_image(&original, "SCUS_942.54").expect("SCUS present");
    let off = starting_inv_seed_file_offset(&scus).expect("seed offset");
    let before_prologue = scus[off - 16..off].to_vec();
    let after_region_orig =
        scus[off + STARTING_INV_SEED_LEN..off + STARTING_INV_SEED_LEN + 16].to_vec();

    // Randomize on a scratch copy.
    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    let report = apply::randomize_starting_items(&mut patcher, seed, &opts).expect("randomize");
    assert_eq!(report.items_set, n.min(MAX_STARTING_ITEMS));
    assert_eq!(
        report.items,
        plan_seed(seed, &opts).items,
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
    apply::randomize_starting_items(&mut patcher2, seed, &opts).expect("randomize");
    assert!(
        patcher2.image() == patcher.image(),
        "same seed must reproduce the patched image"
    );

    eprintln!(
        "starting-items seed {seed:#x}: {} random consumables seeded {after:?}",
        report.items_set
    );
}

/// The Door-of-Wind convenience toggles: forcing the warp consumable into the
/// starting bag and presetting the all-towns warp bitmask. The warp preset lives
/// in its OWN reclaimable region, so it must not reduce the item capacity, must
/// not disturb the inventory region, and must not clobber the live instruction
/// just after it (which carries `$v0 = 0x2dc0` into `DAT_80073ef8`).
#[test]
fn door_of_wind_and_all_warps_round_trip_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let seed = 0xD00D_0FE1_1500_0089_u64;
    // 5 random + Door of Wind + warps: the budget would have capped items at 3
    // when the warp preset shared the inventory region. It no longer does.
    let opts = StartingSeedOptions {
        random_items: 5,
        door_of_wind: legaia_rando::starting_items::DOOR_OF_WIND_COUNT,
        incense: 0,
        speed_chain: 0,
        chicken_heart: 0,
        good_luck_bell: 0,
        all_warps: true,
    };

    let scus = read_file_in_image(&original, "SCUS_942.54").expect("SCUS present");
    let inv_off = starting_inv_seed_file_offset(&scus).expect("inv seed offset");
    let warp_off = warp_seed_file_offset(&scus).expect("warp seed offset");
    let before_prologue = scus[inv_off - 16..inv_off].to_vec();
    let after_inv_region =
        scus[inv_off + STARTING_INV_SEED_LEN..inv_off + STARTING_INV_SEED_LEN + 16].to_vec();
    // The live constant load (addiu $v0,0x2dc0 at 0x80034ad8) just before the warp
    // region, and the live consumer (sw $v0,0x3ef8 at 0x80034aec) just after it.
    let before_warp = scus[warp_off - 4..warp_off].to_vec();
    let after_warp = scus[warp_off + WARP_SEED_LEN..warp_off + WARP_SEED_LEN + 8].to_vec();
    assert!(
        !scus_unlocks_all_warps(&scus).unwrap_or(true),
        "vanilla disc does not preset the warp bitmask"
    );

    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    let report = apply::randomize_starting_items(&mut patcher, seed, &opts).expect("randomize");
    assert!(report.all_warps, "report records the warp preset");
    assert_eq!(report.items, plan_seed(seed, &opts).items);

    // Warps no longer steal item budget: all five slots fill, Door of Wind first.
    assert_eq!(
        report.items_set, 5,
        "all five item slots fill with warps on"
    );
    assert_eq!(
        report.items[0],
        (
            DOOR_OF_WIND_ITEM,
            legaia_rando::starting_items::DOOR_OF_WIND_COUNT
        ),
        "Door of Wind is seeded first"
    );

    // Re-decode off the patched image: inventory + warp flag both reflect the plan.
    let after = apply::current_starting_items(&patcher).expect("read patched seed");
    assert_eq!(after, report.items, "inventory decodes to the plan");
    assert!(
        apply::current_all_warps(&patcher).expect("read warp flag"),
        "the all-warps bitmask is preset on the patched disc"
    );
    let patched_scus = read_file_in_image(patcher.image(), "SCUS_942.54").expect("SCUS");

    // The warp preset is in the WARP region, not the inventory region.
    assert!(
        region_unlocks_all_warps(&patched_scus[warp_off..warp_off + WARP_SEED_LEN]),
        "the warp region writes the all-towns bitmask"
    );
    assert!(
        !region_unlocks_all_warps(&patched_scus[inv_off..inv_off + STARTING_INV_SEED_LEN]),
        "the inventory region carries no warp stores"
    );

    // Neither edit disturbs the surrounding live code.
    assert_eq!(
        &patched_scus[inv_off - 16..inv_off],
        &before_prologue[..],
        "inventory prologue untouched"
    );
    assert_eq!(
        &patched_scus[inv_off + STARTING_INV_SEED_LEN..inv_off + STARTING_INV_SEED_LEN + 16],
        &after_inv_region[..],
        "code after the inventory region untouched"
    );
    assert_eq!(
        &patched_scus[warp_off - 4..warp_off],
        &before_warp[..],
        "the addiu $v0,0x2dc0 before the warp region is untouched"
    );
    assert_eq!(
        &patched_scus[warp_off + WARP_SEED_LEN..warp_off + WARP_SEED_LEN + 8],
        &after_warp[..],
        "the sw $v0,0x3ef8 after the warp region is untouched ($v0 preserved)"
    );

    // Same image size; both touched SCUS sectors stay EDC/ECC-valid.
    assert_eq!(
        patcher.image().len(),
        original.len(),
        "image size unchanged"
    );
    let (scus_lba, _) = find_file_in_image(patcher.image(), "SCUS_942.54").unwrap();
    for region_off in [inv_off, warp_off] {
        let sector = scus_lba as usize + region_off / USER_DATA_SIZE;
        let sb = sector * SECTOR_SIZE;
        assert!(
            legaia_iso::write::mode2_form1_sector_is_valid(&patcher.image()[sb..sb + SECTOR_SIZE]),
            "patched sector must be EDC/ECC-valid"
        );
    }

    // Determinism.
    let mut patcher2 = DiscPatcher::open(original).expect("open");
    apply::randomize_starting_items(&mut patcher2, seed, &opts).expect("randomize");
    assert!(
        patcher2.image() == patcher.image(),
        "deterministic for a fixed seed"
    );

    eprintln!("door-of-wind + all-warps seed {seed:#x}: bag {after:?}, warps unlocked");
}

/// Seeding Incense (the encounter-rate consumable) into the starting bag is
/// additive to the vanilla Healing Leaf and round-trips off the patched disc,
/// mirroring the Door-of-Wind path.
#[test]
fn incense_round_trips_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let seed = 0x14CE_45E0_0000_0001_u64;
    let opts = StartingSeedOptions {
        incense: legaia_rando::starting_items::INCENSE_COUNT,
        ..Default::default()
    };

    // Vanilla baseline: exactly Healing Leaf (0x77) x5.
    let base = DiscPatcher::open(original.clone()).expect("open");
    assert_eq!(
        apply::current_starting_items(&base).expect("read seed"),
        vec![(0x77, 5)],
        "vanilla starts with Healing Leaf x5"
    );

    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    let report = apply::randomize_starting_items(&mut patcher, seed, &opts).expect("randomize");
    assert_eq!(report.items, plan_seed(seed, &opts).items);

    // Re-decode off the patched image: Incense first, then the kept Healing Leaf.
    let after = apply::current_starting_items(&patcher).expect("read patched seed");
    assert_eq!(
        after,
        vec![
            (
                legaia_rando::starting_items::INCENSE_ID,
                legaia_rando::starting_items::INCENSE_COUNT
            ),
            (0x77, 5),
        ],
        "Incense is seeded additively to the vanilla Healing Leaf"
    );

    // Same image size; the touched SCUS sector stays EDC/ECC-valid.
    assert_eq!(
        patcher.image().len(),
        original.len(),
        "image size unchanged"
    );
    let scus = read_file_in_image(patcher.image(), "SCUS_942.54").expect("SCUS");
    let inv_off = starting_inv_seed_file_offset(&scus).expect("inv seed offset");
    let (scus_lba, _) = find_file_in_image(patcher.image(), "SCUS_942.54").unwrap();
    let sector = scus_lba as usize + inv_off / USER_DATA_SIZE;
    let sb = sector * SECTOR_SIZE;
    assert!(
        legaia_iso::write::mode2_form1_sector_is_valid(&patcher.image()[sb..sb + SECTOR_SIZE]),
        "patched sector must be EDC/ECC-valid"
    );

    // Determinism.
    let mut patcher2 = DiscPatcher::open(original).expect("open");
    apply::randomize_starting_items(&mut patcher2, seed, &opts).expect("randomize");
    assert!(
        patcher2.image() == patcher.image(),
        "deterministic for a fixed seed"
    );
    eprintln!("incense seed {seed:#x}: bag {after:?}");
}

/// Seeding the convenience accessories (Speed Chain / Chicken Heart / Good Luck
/// Bell) into the starting bag round-trips off the patched disc. Accessories are
/// "Goods", but the owned-item list is a single ordered `(id, count)` array, so
/// they seed exactly like a consumable.
#[test]
fn accessories_round_trip_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    use legaia_rando::starting_items::{
        ACCESSORY_SEED_COUNT, CHICKEN_HEART_ID, GOOD_LUCK_BELL_ID, SPEED_CHAIN_ID,
    };
    let seed = 0xACCE_5500_0000_0001_u64;
    let opts = StartingSeedOptions {
        speed_chain: ACCESSORY_SEED_COUNT,
        chicken_heart: ACCESSORY_SEED_COUNT,
        good_luck_bell: ACCESSORY_SEED_COUNT,
        ..Default::default()
    };

    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    let report = apply::randomize_starting_items(&mut patcher, seed, &opts).expect("randomize");
    assert_eq!(report.items, plan_seed(seed, &opts).items);

    // Re-decode off the patched image: the three accessories first, then the
    // kept vanilla Healing Leaf.
    let after = apply::current_starting_items(&patcher).expect("read patched seed");
    assert_eq!(
        after,
        vec![
            (SPEED_CHAIN_ID, ACCESSORY_SEED_COUNT),
            (CHICKEN_HEART_ID, ACCESSORY_SEED_COUNT),
            (GOOD_LUCK_BELL_ID, ACCESSORY_SEED_COUNT),
            (0x77, 5),
        ],
        "accessories seeded additively to the vanilla Healing Leaf"
    );

    // Same image size; the touched SCUS sector stays EDC/ECC-valid.
    assert_eq!(
        patcher.image().len(),
        original.len(),
        "image size unchanged"
    );
    let scus = read_file_in_image(patcher.image(), "SCUS_942.54").expect("SCUS");
    let inv_off = starting_inv_seed_file_offset(&scus).expect("inv seed offset");
    let (scus_lba, _) = find_file_in_image(patcher.image(), "SCUS_942.54").unwrap();
    let sb = (scus_lba as usize + inv_off / USER_DATA_SIZE) * SECTOR_SIZE;
    assert!(
        legaia_iso::write::mode2_form1_sector_is_valid(&patcher.image()[sb..sb + SECTOR_SIZE]),
        "patched sector must be EDC/ECC-valid"
    );

    // Determinism.
    let mut patcher2 = DiscPatcher::open(original).expect("open");
    apply::randomize_starting_items(&mut patcher2, seed, &opts).expect("randomize");
    assert!(
        patcher2.image() == patcher.image(),
        "deterministic for a fixed seed"
    );
    eprintln!("accessories seed {seed:#x}: bag {after:?}");
}

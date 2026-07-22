//! Disc-gated end-to-end test for the starting-item randomizer: rewrite the
//! new-game inventory-seed code in `SCUS_942.54` on a scratch copy of the disc,
//! then re-decode the seed straight off the patched image and confirm the edit
//! is faithful - the seeded items match the plan, every id is a valid consumable
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
use legaia_patcher::apply;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::starting_items::{
    GOOD_LUCK_BELL_ID, INV_REGION_SLOTS, MAX_STARTING_ITEMS, SPEED_CHAIN_ID, STARTING_ITEM_POOL,
    StartingSeedOptions, plan_seed,
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
        ..Default::default()
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
        door_of_wind: legaia_patcher::starting_items::DOOR_OF_WIND_COUNT,
        all_warps: true,
        ..Default::default()
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
            legaia_patcher::starting_items::DOOR_OF_WIND_COUNT
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
        incense: legaia_patcher::starting_items::INCENSE_COUNT,
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
                legaia_patcher::starting_items::INCENSE_ID,
                legaia_patcher::starting_items::INCENSE_COUNT
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
    use legaia_patcher::starting_items::{
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

/// The additive fix: convenience items + a full random fill, beyond the
/// inventory region's five slots, with all-warps off. The last slots overflow
/// into the warp-preset region; decoding both regions off the patched disc must
/// recover the whole seven-item bag - so the random items are NOT crowded out by
/// the convenience picks. Also pins that the overflow write leaves the live code
/// bracketing the warp region intact (the `$v0 = 0x2dc0` it must preserve).
#[test]
fn convenience_plus_random_overflow_round_trips_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    use legaia_patcher::starting_items::{DOOR_OF_WIND_COUNT, INCENSE_COUNT};
    let seed = 0x0FF1_0AD0_0000_0007_u64;
    // 2 convenience (Door of Wind + Incense) + 5 random = 7 = the full capacity
    // with all-warps off. All five random must survive on top of the two forced.
    let opts = StartingSeedOptions {
        random_items: 5,
        door_of_wind: DOOR_OF_WIND_COUNT,
        incense: INCENSE_COUNT,
        all_warps: false,
        ..Default::default()
    };

    let scus = read_file_in_image(&original, "SCUS_942.54").expect("SCUS present");
    let inv_off = starting_inv_seed_file_offset(&scus).expect("inv seed offset");
    let warp_off = warp_seed_file_offset(&scus).expect("warp seed offset");
    // The live constant load (addiu $v0,0x2dc0) before and consumer (sw $v0,0x3ef8)
    // after the warp region, which the overflow write must not disturb.
    let before_warp = scus[warp_off - 4..warp_off].to_vec();
    let after_warp = scus[warp_off + WARP_SEED_LEN..warp_off + WARP_SEED_LEN + 8].to_vec();

    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    let report = apply::randomize_starting_items(&mut patcher, seed, &opts).expect("randomize");
    assert!(!report.all_warps);
    assert_eq!(
        report.items_set, MAX_STARTING_ITEMS,
        "all seven slots fill: 2 convenience + 5 random"
    );
    assert_eq!(report.items, plan_seed(seed, &opts).items);

    // Re-decode off the patched image (replays BOTH regions): the whole bag.
    let after = apply::current_starting_items(&patcher).expect("read patched seed");
    assert_eq!(after, report.items, "patched seed decodes to the full plan");
    assert_eq!(after.len(), 7);
    // Both convenience items survived, plus five random consumables on top of
    // them (Door of Wind 0x89 / Incense 0x8A are themselves in the consumable
    // pool, so the random fill is everything that ISN'T one of the two forced).
    use legaia_patcher::starting_items::{DOOR_OF_WIND_ID, INCENSE_ID};
    assert!(after.iter().any(|&(id, _)| id == DOOR_OF_WIND_ID));
    assert!(after.iter().any(|&(id, _)| id == INCENSE_ID));
    let randoms = after
        .iter()
        .filter(|(id, _)| {
            STARTING_ITEM_POOL.contains(id) && *id != DOOR_OF_WIND_ID && *id != INCENSE_ID
        })
        .count();
    assert_eq!(
        randoms, 5,
        "all five random items survive the convenience picks"
    );

    let patched_scus = read_file_in_image(patcher.image(), "SCUS_942.54").expect("SCUS");
    // The warp region now carries item stores, not the bitmask.
    assert!(
        !scus_unlocks_all_warps(&patched_scus).unwrap_or(true),
        "no all-warps preset when the warp region holds overflow items"
    );
    // The inventory region holds the first five slots; the warp region the rest.
    let inv_only =
        StartingInventory::decode_region(&patched_scus[inv_off..inv_off + STARTING_INV_SEED_LEN]);
    assert_eq!(
        inv_only.items(),
        &after[..INV_REGION_SLOTS],
        "inventory region holds the first five slots"
    );
    // The live code bracketing the warp region is intact ($v0 preserved).
    assert_eq!(&patched_scus[warp_off - 4..warp_off], &before_warp[..]);
    assert_eq!(
        &patched_scus[warp_off + WARP_SEED_LEN..warp_off + WARP_SEED_LEN + 8],
        &after_warp[..]
    );

    // Same image size; both touched SCUS sectors stay EDC/ECC-valid.
    assert_eq!(
        patcher.image().len(),
        original.len(),
        "image size unchanged"
    );
    let (scus_lba, _) = find_file_in_image(patcher.image(), "SCUS_942.54").unwrap();
    for region_off in [inv_off, warp_off] {
        let sb = (scus_lba as usize + region_off / USER_DATA_SIZE) * SECTOR_SIZE;
        assert!(
            legaia_iso::write::mode2_form1_sector_is_valid(&patcher.image()[sb..sb + SECTOR_SIZE]),
            "patched sector must be EDC/ECC-valid"
        );
    }

    // Determinism.
    let mut patcher2 = DiscPatcher::open(original).expect("open");
    apply::randomize_starting_items(&mut patcher2, seed, &opts).expect("randomize");
    assert!(patcher2.image() == patcher.image(), "deterministic");
    eprintln!("overflow seed {seed:#x}: full bag {after:?}");
}

/// Explicit `--start-with` items: the user names exact `(id, count)` slots to seed
/// into the starting bag. Unlike the random fill (consumable-pool only), the
/// explicit path takes ANY item id - including accessories ("Goods"), since the
/// owned-item list is one unified array shared by every menu category. This patches
/// two accessory ids directly and confirms they decode back verbatim, additively to
/// the vanilla Healing Leaf base, with the surrounding code untouched.
#[test]
fn explicit_start_with_items_round_trip_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // Two accessories requested explicitly - neither is a consumable-pool id, so a
    // pass proves the explicit path bypasses the random fill's pool restriction.
    let opts = StartingSeedOptions {
        extra_items: vec![(SPEED_CHAIN_ID, 1), (GOOD_LUCK_BELL_ID, 1)],
        ..Default::default()
    };
    assert!(
        !STARTING_ITEM_POOL.contains(&SPEED_CHAIN_ID)
            && !STARTING_ITEM_POOL.contains(&GOOD_LUCK_BELL_ID),
        "the requested ids are accessories, outside the random consumable pool"
    );
    let expected = plan_seed(0, &opts).items;
    assert_eq!(
        expected,
        vec![(SPEED_CHAIN_ID, 1), (GOOD_LUCK_BELL_ID, 1), (0x77, 5)],
        "explicit items seeded first, then the vanilla Healing Leaf base"
    );

    let scus = read_file_in_image(&original, "SCUS_942.54").expect("SCUS present");
    let off = starting_inv_seed_file_offset(&scus).expect("seed offset");
    let before_prologue = scus[off - 16..off].to_vec();
    let after_region_orig =
        scus[off + STARTING_INV_SEED_LEN..off + STARTING_INV_SEED_LEN + 16].to_vec();

    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    let report = apply::randomize_starting_items(&mut patcher, 0, &opts).expect("randomize");
    assert_eq!(report.items, expected, "report mirrors the plan");

    // Re-decode the seed off the PATCHED image: the explicit accessories land
    // verbatim, including their counts.
    let after = apply::current_starting_items(&patcher).expect("read patched seed");
    assert_eq!(after, expected, "patched seed decodes to the explicit bag");

    // The edit stays inside the 40-byte seed region.
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

    // Determinism.
    let mut patcher2 = DiscPatcher::open(original).expect("open");
    apply::randomize_starting_items(&mut patcher2, 0, &opts).expect("randomize");
    assert!(patcher2.image() == patcher.image(), "deterministic");
    eprintln!("explicit start-with bag: {after:?}");
}

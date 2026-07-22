//! Disc-gated end-to-end oracle for the starting-item randomizer **at runtime**
//! - the sixth member of the randomizer runtime-oracle set (chest, monster-drop,
//!   encounter, steal, door, and now starting items).
//!
//! The randomizer's own disc-gated test (`crates/patcher/tests/starting_items_patch_real`)
//! proves a patched starting inventory is *written* faithfully: the seed code in
//! `SCUS_942.54` (`FUN_80034A6C`) is rewritten in place, the surrounding function
//! bytes are untouched, and the touched sector stays EDC/ECC-valid. What it does
//! **not** prove is that a runtime actually *reads the patched seed and grants
//! those items* when a New Game begins.
//!
//! A savestate can't answer that cleanly (the same trap the other oracles
//! document): the seed is executable code that runs once at New Game and lands
//! in RAM, so any state captured after that point reflects whatever inventory
//! the (possibly unpatched) executable seeded. The patched seed is only observed
//! by a fresh New Game off the patched disc.
//!
//! The clean-room engine sidesteps that: it decodes the starting inventory
//! straight from the disc's `SCUS_942.54` bytes
//! ([`StartingInventory::from_scus`]) and seeds the bag via
//! [`World::seed_starting_inventory`] - the same path `BootSession::begin_new_game`
//! drives. So this test:
//!   1. confirms a New Game off the *unpatched* disc seeds the vanilla Healing
//!      Leaf ×5 (baseline, so the patched assertion can't pass vacuously),
//!   2. randomizes the starting items on a scratch copy of the real disc,
//!   3. re-decodes the seed off the patched image (the bytes a fresh New Game
//!      would run),
//!   4. seeds a fresh `World` and asserts the bag holds exactly the patched
//!      items and never the vanilla Healing Leaf ×5.
//!
//! Skips without `LEGAIA_DISC_BIN` (CLAUDE.md convention).

use legaia_asset::new_game::StartingInventory;
use legaia_engine_core::world::World;
use legaia_patcher::apply;
use legaia_patcher::disc::DiscPatcher;
use std::collections::HashMap;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Seed a fresh New Game world's bag from a starting-inventory seed and return
/// the resulting `id -> count` map (the engine path `begin_new_game` drives).
fn seed_bag(inv: &StartingInventory) -> HashMap<u8, u8> {
    let mut world = World::default();
    world.begin_new_game(); // clears the bag, like the retail SC memset
    world.seed_starting_inventory(inv);
    world.inventory.clone()
}

#[test]
fn patched_starting_items_seed_the_bag_at_runtime() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let seed = 0x57A4_7117_E115_BEEFu64;
    let n = 4;

    // --- Baseline: a New Game off the UNPATCHED disc seeds Healing Leaf x5. ---
    let scus = legaia_iso::iso9660::read_file_in_image(&disc, "SCUS_942.54").expect("SCUS");
    let vanilla = StartingInventory::from_scus(&scus).expect("decode vanilla seed");
    assert_eq!(
        vanilla.items(),
        &[(0x77, 5)],
        "baseline: retail new game = Healing Leaf x5"
    );
    let vanilla_bag = seed_bag(&vanilla);
    assert_eq!(
        vanilla_bag.get(&0x77).copied(),
        Some(5),
        "baseline: the engine seeds Healing Leaf x5 (non-vacuous)"
    );
    assert_eq!(vanilla_bag.len(), 1, "baseline bag holds only Healing Leaf");

    // --- Patch the starting items on a scratch copy of the disc. ---
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    let opts = legaia_patcher::starting_items::StartingSeedOptions {
        random_items: n,
        ..Default::default()
    };
    let report = apply::randomize_starting_items(&mut patcher, seed, &opts).expect("randomize");
    assert!(report.items_set >= 2, "expected several seeded items");

    // --- Re-decode the seed off the PATCHED image (what a fresh New Game runs)
    //     and seed a fresh world's bag from it. ---
    let patched_scus =
        legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54").expect("SCUS");
    let patched = StartingInventory::from_scus(&patched_scus).expect("decode patched seed");
    assert_eq!(
        patched.items(),
        report.items.as_slice(),
        "patched seed decodes to the plan"
    );
    assert_ne!(
        patched.items(),
        vanilla.items(),
        "the patched seed differs from vanilla"
    );

    let bag = seed_bag(&patched);

    // The runtime bag holds EXACTLY the patched items (id -> count), not the
    // vanilla Healing Leaf x5.
    let expected: HashMap<u8, u8> = patched.items().iter().copied().collect();
    assert_eq!(
        bag, expected,
        "the New Game bag is exactly the patched seed"
    );
    assert_ne!(
        bag, vanilla_bag,
        "the bag is not the vanilla Healing Leaf x5"
    );

    eprintln!(
        "starting-items runtime oracle: New Game bag = {:?} (vanilla was {:?})",
        {
            let mut v: Vec<_> = bag.iter().map(|(k, c)| (*k, *c)).collect();
            v.sort_unstable();
            v
        },
        vanilla.items()
    );
}

/// Runtime oracle for the **Door of Wind** convenience toggle: forcing the warp
/// consumable into the new game's starting bag. Same structure as above - seed a
/// fresh world from the patched seed and assert the bag holds Door of Wind. The
/// all-warps toggle is a story-flag preset the clean-room engine has no consumer
/// for yet (there is no Door-of-Wind warp menu), so its runtime check stays at
/// the disc-round-trip level (`crates/patcher/tests/starting_items_patch_real`);
/// here we cover the half that the engine *does* run - the item grant.
#[test]
fn forced_door_of_wind_seeds_the_bag_at_runtime() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    use legaia_asset::new_game::DOOR_OF_WIND_ITEM;
    use legaia_patcher::starting_items::{DOOR_OF_WIND_COUNT, StartingSeedOptions};

    // Baseline: a vanilla New Game has no Door of Wind in the bag.
    let scus = legaia_iso::iso9660::read_file_in_image(&disc, "SCUS_942.54").expect("SCUS");
    let vanilla = StartingInventory::from_scus(&scus).expect("decode vanilla seed");
    let vanilla_bag = seed_bag(&vanilla);
    assert_eq!(
        vanilla_bag.get(&DOOR_OF_WIND_ITEM),
        None,
        "baseline: vanilla bag has no Door of Wind (non-vacuous)"
    );

    // Patch with the Door-of-Wind toggle (no reroll, no warps): additive to vanilla.
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    let opts = StartingSeedOptions {
        door_of_wind: DOOR_OF_WIND_COUNT,
        ..Default::default()
    };
    let report = apply::randomize_starting_items(&mut patcher, 1, &opts).expect("randomize");
    assert!(!report.all_warps);

    let patched_scus =
        legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54").expect("SCUS");
    let patched = StartingInventory::from_scus(&patched_scus).expect("decode patched seed");
    let bag = seed_bag(&patched);

    // The runtime bag holds Door of Wind ×DOOR_OF_WIND_COUNT, and the vanilla
    // Healing Leaf base is preserved (additive toggle).
    assert_eq!(
        bag.get(&DOOR_OF_WIND_ITEM).copied(),
        Some(DOOR_OF_WIND_COUNT),
        "the New Game bag holds the forced Door of Wind"
    );
    assert_eq!(
        bag.get(&0x77).copied(),
        Some(5),
        "the vanilla Healing Leaf base is preserved (additive)"
    );

    eprintln!("door-of-wind runtime oracle: New Game bag = {:?}", {
        let mut v: Vec<_> = bag.iter().map(|(k, c)| (*k, *c)).collect();
        v.sort_unstable();
        v
    });
}

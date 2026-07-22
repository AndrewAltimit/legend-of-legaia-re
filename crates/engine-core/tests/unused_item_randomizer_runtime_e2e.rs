//! Disc-gated end-to-end oracle for the `--unused-items` toggle at runtime.
//!
//! The toggle does two things, and this proves both at runtime:
//!   1. it makes the unused accessory (item `0xFD`) **obtainable** - it adds it
//!      to the random-fill pool so a drop / chest / steal can hand it out; and
//!   2. it names that otherwise-blank accessory **"Seru Bell"** so it shows a
//!      real name when it lands.
//!
//! The randomizer's own disc-gated test (`crates/patcher/tests/unused_content_real`)
//! proves the *bytes*: the pool widens to include `0xFD`, and the name injection
//! repoints only `0xFD`'s pointer to a "Seru Bell" string in reclaimable SCUS
//! space. What it does **not** prove is that a runtime actually *grants `0xFD`
//! into the bag and resolves it to the injected name* - the runtime-grant and
//! name-resolution paths the player sees.
//!
//! So this oracle, on a scratch copy of the real disc:
//!   1. applies the toggle's name injection (`apply::inject_seru_bell_name`) and
//!      re-reads the item-name table off the patched SCUS, asserting `0xFD`
//!      resolves to "Seru Bell" while the other empty-name ids stay blank (the
//!      display side), and confirms "Something Good" (`0x6B`) is named;
//!   2. patches one monster's drop to the unused accessory `0xFD` (the value the
//!      widened pool can now place), re-decodes the record off the patched
//!      `battle_data`, and drives the victory-spoils path
//!      ([`World::apply_battle_loot`]), asserting the bag receives `0xFD` (the
//!      grant side).
//!
//! A baseline pass over the unpatched record first confirms the engine grants the
//! monster's original drop, so the patched assertion can't pass vacuously. The
//! drop roll is seeded to land. Skips without `LEGAIA_DISC_BIN`.

use legaia_engine_core::monster_catalog::{
    FormationDef, FormationSlot, catalog_from_monster_archive,
};
use legaia_engine_core::world::World;
use legaia_patcher::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};
use legaia_patcher::item_name::{SERU_BELL_ID, SERU_BELL_NAME};

/// World RNG seed for which the first `apply_battle_loot` drop roll is `0` (so
/// the drop lands for any positive rate) - same value the drop oracle uses.
const ROLL_LANDS_SEED: u32 = 229;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Decode `monster_id` off `archive`, run the engine victory-spoils path, and
/// return the dropped item ids that landed in the bag. Fresh `World` per call.
fn drops_for_monster(archive: &[u8], monster_id: u16) -> Vec<u8> {
    let catalog = catalog_from_monster_archive(archive, &[monster_id]);
    let formation = FormationDef::new(0, vec![FormationSlot::new(monster_id)]);
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    world.rng_state = ROLL_LANDS_SEED;
    let rewards = world.apply_battle_loot(&formation, &catalog);
    for &item in &rewards.drops {
        assert!(
            world.inventory.get(&item).copied().unwrap_or(0) >= 1,
            "dropped item 0x{item:02x} must be in the bag"
        );
    }
    rewards.drops
}

#[test]
fn enabling_unused_items_grants_and_names_the_accessory_at_runtime() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };

    // ===== Display side: the name injection names only the accessory =====
    let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let set = legaia_patcher::apply::inject_seru_bell_name(&mut patcher)
        .expect("inject")
        .expect("fresh disc must get the name set");
    assert_eq!(set, SERU_BELL_NAME);
    let scus = patcher
        .read_named_file("SCUS_942.54")
        .expect("read patched SCUS");
    let table = legaia_asset::item_names::ItemNameTable::from_scus(&scus).expect("parse table");
    assert_eq!(
        table.name(SERU_BELL_ID),
        Some(SERU_BELL_NAME),
        "the runtime item-name lookup resolves 0xFD to the injected name"
    );
    // The other ids that shared the empty-string slot stay blank.
    for id in [0x12u8, 0x1A, 0x52, 0xB9] {
        assert!(table.name(id).is_none(), "id {id:#x} must stay unnamed");
    }
    // "Something Good" (the other unused item) is a real named item.
    assert!(
        table.name(0x6B).is_some_and(|n| !n.is_empty()),
        "Something Good (0x6B) must be named"
    );

    // ===== Grant side: the engine grants 0xFD as a drop =====
    // Pick a monster with a drop whose slot re-packs once set to 0xFD.
    let base = DiscPatcher::open(disc.clone()).expect("open disc");
    let archive = base
        .read_entry(MONSTER_ARCHIVE_ENTRY)
        .expect("read archive");
    let records = legaia_asset::monster_archive::records(&archive).expect("decode archive");
    let (monster_id, original_item, chance) = records
        .iter()
        .find_map(|r| {
            if r.drop_item == 0 || r.drop_chance_pct == 0 || r.drop_item == SERU_BELL_ID {
                return None;
            }
            let slot = base.monster_slot(r.id).ok()?;
            legaia_patcher::monster::set_drop(&slot, SERU_BELL_ID, r.drop_chance_pct).ok()?;
            Some((r.id, r.drop_item, r.drop_chance_pct))
        })
        .expect("archive must hold a droppable, re-packable monster");

    // Baseline: original drop is granted (non-vacuous).
    let baseline = drops_for_monster(&archive, monster_id);
    assert!(
        baseline.contains(&original_item) && !baseline.contains(&SERU_BELL_ID),
        "baseline must grant 0x{original_item:02x}, not the unused accessory, got {baseline:02x?}"
    );

    // Patch the drop to the unused accessory, re-decode off the patched image.
    let mut patcher = DiscPatcher::open(disc).expect("reopen disc");
    let slot = patcher.monster_slot(monster_id).unwrap();
    let repacked = legaia_patcher::monster::set_drop(&slot, SERU_BELL_ID, chance).unwrap();
    patcher.patch_monster_slot(monster_id, &repacked).unwrap();
    let patched_archive = patcher.read_entry(MONSTER_ARCHIVE_ENTRY).unwrap();
    let patched_rec = legaia_asset::monster_archive::record(&patched_archive, monster_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        patched_rec.drop_item, SERU_BELL_ID,
        "patched bytes carry 0xFD"
    );

    // Runtime: the engine grants the unused accessory.
    let runtime = drops_for_monster(&patched_archive, monster_id);
    assert!(
        runtime.contains(&SERU_BELL_ID),
        "runtime must grant the unused accessory 0xFD ({SERU_BELL_NAME}), got {runtime:02x?}"
    );
    assert!(
        !runtime.contains(&original_item),
        "runtime must not still grant the original drop after the patch, got {runtime:02x?}"
    );

    eprintln!(
        "unused-item runtime E2E: 0xFD resolves to {SERU_BELL_NAME:?} and is granted by monster \
         {monster_id} (was 0x{original_item:02x})"
    );
}

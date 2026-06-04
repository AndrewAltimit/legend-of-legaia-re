//! Disc-gated runtime oracle for the **equipment-as-drops** randomizer.
//!
//! The drop randomizer's runtime oracle (`monster_drop_randomizer_runtime_e2e`)
//! already proves a patched `+0x48` drop id is read off the disc and granted by
//! the victory-spoils path. This test pins the equipment-drops *feature*
//! end-to-end: the full pool → plan → apply pipeline
//! ([`legaia_rando::equipment`] + [`legaia_rando::apply::randomize_equipment_drops`])
//! turns a monster's drop into a piece of equipment, and the clean-room engine,
//! decoding that monster's record off the patched `battle_data`, grants exactly
//! that equipment id through [`World::apply_battle_loot`].
//!
//! Equipment-drop chances are tiered to 1..=3 % (early/mid/late, see
//! [`legaia_rando::equipment`]); the world RNG is seeded so the drop roll is `0`,
//! which lands for any positive rate, so the grant is observed deterministically.
//! A baseline pass over the unpatched record confirms the original (consumable)
//! drop, so the patched assertion can't pass vacuously. Skips without
//! `LEGAIA_DISC_BIN`.

use legaia_engine_core::monster_catalog::{
    FormationDef, FormationSlot, catalog_from_monster_archive,
};
use legaia_engine_core::world::World;
use legaia_rando::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};

/// Seed for which the first `apply_battle_loot` drop roll is `0` (lands for any
/// positive rate). Same value the monster-drop oracle pins.
const ROLL_LANDS_SEED: u32 = 229;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Drive one monster through the victory-spoils path on `archive`, returning the
/// granted drop ids (roll seeded to land).
fn drops_for_monster(archive: &[u8], monster_id: u16) -> Vec<u8> {
    let catalog = catalog_from_monster_archive(archive, &[monster_id]);
    let formation = FormationDef::new(0, vec![FormationSlot::new(monster_id)]);
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    world.rng_state = ROLL_LANDS_SEED;
    world.apply_battle_loot(&formation, &catalog).drops
}

#[test]
fn equipment_drop_grants_equipment_at_runtime() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    let scus =
        legaia_iso::iso9660::read_file_in_image(&disc, "SCUS_942.54").expect("SCUS in image");
    let pool = legaia_rando::equipment::equipment_pool(&scus).expect("equipment pool");
    let pool_ids: std::collections::HashSet<u8> = pool.iter().map(|e| e.id).collect();
    assert!(!pool.is_empty(), "equipment pool is non-empty");

    // Apply the equipment-drop randomizer to a scratch disc.
    let seed = 0xE0_u64;
    let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let (plan, report) =
        legaia_rando::apply::randomize_equipment_drops(&mut patcher, &pool, seed).unwrap();
    let skipped: std::collections::HashSet<u16> = report.skipped.iter().copied().collect();

    // Pick the first planned monster whose slot was written (not skipped).
    let target = plan
        .iter()
        .find(|a| !skipped.contains(&a.monster_id))
        .expect("at least one monster got an equipment drop");
    assert!(
        pool_ids.contains(&target.item),
        "planned drop 0x{:02x} is a pool equipment id",
        target.item
    );
    assert!((1..=3).contains(&target.chance), "tiered chance");

    // Baseline: on the UNPATCHED disc this monster drops its original
    // (non-equipment) item, never the planned equipment id.
    let base_patcher = DiscPatcher::open(disc).expect("reopen disc");
    let orig_archive = base_patcher.read_entry(MONSTER_ARCHIVE_ENTRY).unwrap();
    let orig_rec = legaia_asset::monster_archive::record(&orig_archive, target.monster_id)
        .unwrap()
        .unwrap();
    // Only meaningful if the original differs from the planned equipment id.
    if orig_rec.drop_item != target.item && orig_rec.drop_chance_pct > 0 {
        let baseline = drops_for_monster(&orig_archive, target.monster_id);
        assert!(
            !baseline.contains(&target.item),
            "baseline must not already grant the equipment id (vacuous otherwise)"
        );
    }

    // Runtime: decode the patched record off the patched image and grant.
    let patched_archive = patcher.read_entry(MONSTER_ARCHIVE_ENTRY).unwrap();
    let patched_rec = legaia_asset::monster_archive::record(&patched_archive, target.monster_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        patched_rec.drop_item, target.item,
        "patched disc carries the planned equipment id"
    );

    let runtime = drops_for_monster(&patched_archive, target.monster_id);
    assert!(
        runtime.contains(&target.item),
        "runtime grants the planned equipment id 0x{:02x} (monster {}), got {runtime:02x?}",
        target.item,
        target.monster_id
    );
    assert!(
        pool_ids.contains(&runtime[0]),
        "the granted drop is equipment from the pool"
    );
}

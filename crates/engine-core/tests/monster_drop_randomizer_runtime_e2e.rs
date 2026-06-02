//! Disc-gated end-to-end oracle for the monster **drop** randomizer at runtime.
//!
//! The randomizer's own disc-gated test (`crates/rando/tests/disc_patch_real`)
//! proves a patched monster drop is *written* faithfully: the `+0x48` drop item
//! byte changes inside the re-packed `0x14000` `battle_data` slot, neighbouring
//! records stay intact, and the touched PROT.DAT sectors stay EDC/ECC-valid.
//! What it does **not** prove is that a runtime actually *reads the patched byte
//! and grants the new item on victory* — the question behind "is it truly
//! randomizing, or is something serving a stale value?".
//!
//! A savestate can't answer that cleanly (the same trap the chest oracle
//! documents): a mednafen state snapshots all of RAM, and the `battle_data`
//! archive is resident in RAM the moment a battle is loaded, so loading such a
//! state on a patched disc still grants the *original* drop — the value is read
//! from the already-loaded RAM copy, never re-fetched from the (patched) disc.
//! A patched drop is only observed after a *fresh battle load* re-reads the
//! monster record off the (patched) disc.
//!
//! The clean-room engine sidesteps that cache entirely: it decodes the monster
//! record straight from disc bytes (`legaia_asset::monster_archive`) and runs
//! the very victory-spoils path the randomizer's edit feeds — the drop roll in
//! [`World::apply_battle_loot`] (ported from the reward resolver `FUN_8004E568`
//! → `FUN_80054CB0` record→actor copy). So this test:
//!   1. picks one monster that has a drop, on a scratch copy of the real disc,
//!   2. patches only its `+0x48` drop item to a distinct id (chance unchanged,
//!      exactly the surgical single-operand edit the chest oracle makes),
//!   3. re-decodes the patched monster record off the patched image (the same
//!      bytes a fresh battle load would stream),
//!   4. builds the engine [`MonsterCatalog`] from that record and drives a
//!      one-monster formation through `apply_battle_loot`,
//!   5. asserts the runtime grants the **patched** id into the bag and never the
//!      original.
//!
//! A baseline pass over the *unpatched* record first confirms the engine grants
//! that monster's original drop at all, so the patched assertion can't pass
//! vacuously. The drop roll is made deterministic by seeding the world RNG so
//! the roll lands for any non-zero rate (`roll == 0`), keeping both passes
//! comparable. Skips without `LEGAIA_DISC_BIN` (CLAUDE.md convention).

use legaia_engine_core::monster_catalog::{
    FormationDef, FormationSlot, catalog_from_monster_archive,
};
use legaia_engine_core::world::World;
use legaia_rando::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};

/// World RNG seed for which the first `apply_battle_loot` drop roll is `0`
/// (`(next_rng() & 0xFF) == 0`), so the drop lands for any positive rate. The
/// LCG is `state*1_664_525 + 1_013_904_223`; `229` solves
/// `(13*state + 95) mod 256 == 0`. A fresh `World` per pass reuses it, so the
/// baseline and patched rolls are identical and the only variable is the disc
/// byte.
const ROLL_LANDS_SEED: u32 = 229;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Build a one-monster formation, decode that monster's record off `archive`,
/// and run the engine victory-spoils path. Returns the dropped item ids (the
/// `apply_battle_loot` result, which mirrors what landed in the bag). A fresh
/// `World` keeps the inventory clean and the RNG seeded so the roll lands.
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
    // Cross-check the bag actually received what the rewards report claims.
    for &item in &rewards.drops {
        assert!(
            world.inventory.get(&item).copied().unwrap_or(0) >= 1,
            "dropped item 0x{item:02x} must be in the bag"
        );
    }
    rewards.drops
}

#[test]
fn patched_monster_drop_grants_new_item_at_runtime() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };

    // Item names purely for legible output (no Sony bytes are asserted).
    let names = legaia_iso::iso9660::read_file_in_image(&disc, "SCUS_942.54")
        .and_then(|scus| legaia_asset::item_names::ItemNameTable::from_scus(&scus));
    let name_of = |id: u8| {
        names
            .as_ref()
            .and_then(|t| t.name(id))
            .unwrap_or("?")
            .to_string()
    };

    // --- Pick a monster that has a drop, and whose slot re-packs within its
    //     footprint (so the patch is same-size in place). ---
    let patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let archive = patcher
        .read_entry(MONSTER_ARCHIVE_ENTRY)
        .expect("read archive");
    let records = legaia_asset::monster_archive::records(&archive).expect("decode archive");

    let (monster_id, original_item, original_chance) = records
        .iter()
        .find_map(|r| {
            // Needs a real drop, a non-zero chance (so the roll can land), and a
            // slot the re-packer fits.
            if r.drop_item == 0 || r.drop_chance_pct == 0 {
                return None;
            }
            let replacement = pick_replacement(r.drop_item);
            let slot = patcher.monster_slot(r.id).ok()?;
            legaia_rando::monster::set_drop(&slot, replacement, r.drop_chance_pct).ok()?;
            Some((r.id, r.drop_item, r.drop_chance_pct))
        })
        .expect("archive must hold at least one droppable, re-packable monster");
    let replacement_item = pick_replacement(original_item);

    // --- Baseline: the engine grants this monster's ORIGINAL drop on the
    //     UNPATCHED disc (proves the path is reachable, non-vacuous). ---
    let baseline = drops_for_monster(&archive, monster_id);
    assert!(
        baseline.contains(&original_item),
        "baseline: monster {monster_id} must drop its original 0x{original_item:02x}, got {baseline:02x?}"
    );
    assert!(
        !baseline.contains(&replacement_item),
        "baseline must NOT already drop the replacement id (test would be vacuous)"
    );

    // --- Patch only the drop item (chance unchanged) on a scratch disc, then
    //     re-decode the record off the PATCHED image. ---
    let mut patcher = DiscPatcher::open(disc).expect("reopen disc");
    let slot = patcher.monster_slot(monster_id).unwrap();
    let repacked =
        legaia_rando::monster::set_drop(&slot, replacement_item, original_chance).unwrap();
    patcher.patch_monster_slot(monster_id, &repacked).unwrap();

    let patched_archive = patcher.read_entry(MONSTER_ARCHIVE_ENTRY).unwrap();
    let patched_rec = legaia_asset::monster_archive::record(&patched_archive, monster_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        patched_rec.drop_item, replacement_item,
        "patched disc bytes must carry the new drop id"
    );
    assert_eq!(
        patched_rec.drop_chance_pct, original_chance,
        "drop chance must be untouched by the item patch"
    );

    // --- Runtime: drive the SAME monster through the victory-spoils path on the
    //     patched record. It must grant the replacement and never the original. ---
    let runtime = drops_for_monster(&patched_archive, monster_id);
    assert!(
        runtime.contains(&replacement_item),
        "runtime grants the patched id 0x{replacement_item:02x} (monster {monster_id}), got {runtime:02x?}"
    );
    assert!(
        !runtime.contains(&original_item),
        "runtime must NOT grant the original 0x{original_item:02x} after the patch \
         (got {runtime:02x?}) — a stale value here is the caching failure this test guards"
    );

    eprintln!(
        "monster drop runtime E2E: monster {monster_id} baseline drops {baseline:02x?} \
         ({}), patched drops {runtime:02x?} ({}) — 0x{original_item:02x} {} -> 0x{replacement_item:02x} {}",
        name_of(original_item),
        name_of(replacement_item),
        name_of(original_item),
        name_of(replacement_item),
    );
}

/// An arbitrary but distinct replacement id (validity is irrelevant to the
/// runtime-grant proof: the grant hook adds whatever id the record carries).
/// Avoids `0` so the replacement is a real drop.
fn pick_replacement(original: u8) -> u8 {
    let candidate = original.wrapping_add(0x40);
    if candidate == 0 || candidate == original {
        0x42
    } else {
        candidate
    }
}

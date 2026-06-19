//! Disc-gated end-to-end oracle for the `--unused-enemies` toggle at runtime.
//!
//! The randomizer's own disc-gated test (`crates/rando/tests/unused_content_real`)
//! proves the toggle *writes* an unused enemy id into a scene's encounter
//! formations: with the toggle on, the curated Evil Bat ids join each scene's
//! Random pool and get placed, and the patched scene MAN re-packs in its
//! footprint. What it does **not** prove is that a runtime actually *reads that
//! injected id and spawns the Evil Bat into battle* - the question behind "does
//! enabling the unused enemy really make it appear?".
//!
//! This is the runtime counterpart, mirroring the encounter oracle
//! ([`encounter_randomizer_runtime_e2e`]) but driven by the toggle's own code:
//!   1. run the real toggle path
//!      ([`SceneEncounters::randomize_with_extra`] with
//!      [`legaia_rando::unused::UNUSED_ENEMY_IDS`], the same call
//!      `apply::randomize_encounters` makes) on a scene of the real disc until
//!      it places an unused id at some formation row's slot 0,
//!   2. write the re-packed MAN onto a scratch disc and re-decode it off the
//!      patched image (the bytes a fresh scene load would stream),
//!   3. force that formation row into a battle through the live-loop encounter
//!      path and read the spawned enemy actor's `battle_monster_id`,
//!   4. assert the runtime spawns an **unused-enemy id** at that slot.
//!
//! A baseline pass over the *unpatched* MAN row first confirms the engine spawns
//! that row's original (vanilla, non-unused) monster, so the patched assertion
//! can't pass vacuously. Skips without `LEGAIA_DISC_BIN`.

use legaia_engine_core::encounter_man::scene_encounter_from_man;
use legaia_engine_core::world::{SceneMode, World};
use legaia_rando::disc::DiscPatcher;
use legaia_rando::encounter::SceneEncounters;
use legaia_rando::unused::UNUSED_ENEMY_IDS;

/// Fixed seed for the toggle's per-scene Random pass (mixed with the entry idx
/// inside `randomize_with_extra`, so deterministic per scene).
const SEED: u64 = 0x5E_7A_BA_70;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Drive formation row `formation_id` of a decoded scene MAN into a battle and
/// return the monster id spawned into enemy slot 0. Same path as the encounter
/// oracle: build the table + per-row defs from the MAN, force the row, let the
/// live loop flip `Field -> Battle`. `None` if the row never reaches battle.
fn spawn_enemy0_for_formation(decoded_man: &[u8], formation_id: u16) -> Option<u16> {
    let (table, defs) = scene_encounter_from_man("oracle", decoded_man)?;
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Field;
    world.live_gameplay_loop = true;
    world.actors[0].active = true;
    world.actors[0].battle.hp = 400;
    world.actors[0].battle.max_hp = 400;
    world.actors[0].battle.liveness = 1;
    world.set_battle_attack(0, 80);
    world.install_man_encounter(table, defs);
    world.install_man_formation(formation_id)?;
    world.on_field_step();
    for _ in 0..256 {
        world.tick();
        if world.mode == SceneMode::Battle {
            return world
                .actors
                .get(world.party_count as usize)
                .and_then(|a| a.battle_monster_id);
        }
    }
    None
}

#[test]
fn enabling_unused_enemies_spawns_the_evil_bat_at_runtime() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let is_unused = |id: u8| UNUSED_ENEMY_IDS.contains(&id);

    // --- Find a scene where the toggle's Random pass places an unused enemy at a
    //     formation row's slot 0, the MAN re-packs in its footprint, and the
    //     engine actually drives that row into battle (so the proof isn't
    //     vacuous). ---
    let patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let mut chosen: Option<(usize, u16, u8, Vec<u8>, usize)> = None; // idx, row, original0, stream, man_off
    'scan: for idx in 0..patcher.entry_count() {
        let Ok(entry) = patcher.read_entry(idx) else {
            continue;
        };
        let Some(orig) = SceneEncounters::locate(&entry, idx) else {
            continue;
        };
        // Run the real toggle path on a fresh copy of this scene.
        let mut patched = SceneEncounters::locate(&entry, idx).unwrap();
        let changed = patched.randomize_with_extra(
            SEED,
            legaia_rando::drops::DropMode::Random,
            UNUSED_ENEMY_IDS,
        );
        if changed == 0 {
            continue;
        }
        let Some(stream) = patched.repack() else {
            continue; // overflow - the randomizer would skip this scene too
        };
        for row in 0..patched.formation_count() as u16 {
            let patched0 = patched.formation_ids(row as usize);
            let orig0 = orig.formation_ids(row as usize);
            let (Some(&p0), Some(&o0)) = (patched0.first(), orig0.first()) else {
                continue;
            };
            if is_unused(p0) && !is_unused(o0) {
                // Confirm the engine drives this row to battle on the unpatched
                // MAN before committing to it (keeps the baseline non-None).
                if spawn_enemy0_for_formation(&orig.decoded, row).is_some() {
                    chosen = Some((idx, row, o0, stream.clone(), patched.man_offset));
                    break 'scan;
                }
            }
        }
    }
    let (entry_idx, row, original0, stream, man_off) =
        chosen.expect("the toggle must place an unused enemy at a drivable formation slot 0");
    assert!(
        !is_unused(original0),
        "the row's vanilla slot 0 must be a normal monster"
    );

    // --- Baseline: the engine spawns the row's ORIGINAL (vanilla) monster on the
    //     unpatched MAN. ---
    let entry = patcher.read_entry(entry_idx).unwrap();
    let orig = SceneEncounters::locate(&entry, entry_idx).unwrap();
    let baseline = spawn_enemy0_for_formation(&orig.decoded, row)
        .expect("baseline: scene must drive the chosen formation row into battle");
    assert_eq!(
        baseline, original0 as u16,
        "baseline must spawn the vanilla monster 0x{original0:02x}, got 0x{baseline:04x}"
    );
    assert!(
        !is_unused(baseline as u8),
        "baseline must not already be an unused enemy"
    );

    // --- Write the toggle's re-packed MAN onto a scratch disc; re-decode off the
    //     patched image (disc-truth, not the in-memory copy). ---
    let mut patcher = DiscPatcher::open(disc).expect("reopen disc");
    patcher
        .patch_prot_entry(entry_idx, man_off as u64, &stream)
        .expect("write patched scene MAN");
    let patched_entry = patcher.read_entry(entry_idx).unwrap();
    let patched = SceneEncounters::locate(&patched_entry, entry_idx).unwrap();
    assert!(
        is_unused(patched.formation_ids(row as usize)[0]),
        "patched disc bytes must carry the unused enemy at the chosen slot"
    );

    // --- Runtime: drive the SAME row on the patched MAN - it must spawn an
    //     unused enemy. ---
    let spawned = spawn_enemy0_for_formation(&patched.decoded, row)
        .expect("patched scene must drive the formation row into battle");
    assert!(
        is_unused(spawned as u8),
        "runtime must spawn an unused enemy id (one of {UNUSED_ENEMY_IDS:?}) at the patched slot, got 0x{spawned:04x}"
    );
    assert_ne!(
        spawned as u8, original0,
        "the patched spawn must differ from the vanilla one"
    );

    eprintln!(
        "unused-enemy runtime E2E: scene entry {entry_idx} formation {row} \
         spawned vanilla 0x{original0:02x} baseline -> unused 0x{spawned:02x} patched (Evil Bat)"
    );
}

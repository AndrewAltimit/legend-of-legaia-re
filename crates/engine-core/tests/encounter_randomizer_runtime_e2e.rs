//! Disc-gated end-to-end oracle for the random-**encounter** randomizer at
//! runtime — the third member of the randomizer runtime-oracle set, alongside
//! the chest ([`chest_randomizer_runtime_e2e`]) and monster-drop
//! ([`monster_drop_randomizer_runtime_e2e`]) oracles.
//!
//! The randomizer's own disc-gated test (`crates/rando/tests/encounter_patch_real`)
//! proves a patched formation is *written* faithfully: the formation's monster-id
//! bytes change inside the re-packed scene MAN, formation counts + the id multiset
//! are preserved, every id stays in the scene's pool, and the touched PROT.DAT
//! sectors stay EDC/ECC-valid. What it does **not** prove is that a runtime
//! actually *reads the patched id and spawns that monster into battle* — the
//! question behind "is it truly randomizing, or is something serving a stale
//! value?".
//!
//! A savestate can't answer that cleanly (the same trap the chest + drop oracles
//! document): a mednafen state snapshots all of RAM, and a scene's MAN — every
//! formation row included — is resident in RAM the moment you stand in the scene,
//! so loading such a state on a patched disc still spawns the *original* monster.
//! A patched formation is only observed after a *fresh scene load* re-reads the
//! MAN off the (patched) disc.
//!
//! The clean-room engine sidesteps that cache entirely: it builds the encounter
//! table + per-row formation defs straight from the MAN bytes
//! ([`scene_encounter_from_man`]) and spawns the rolled formation through the very
//! battle-entry path the randomizer's edit feeds (the `FUN_801DA51C` formation
//! select-by-index → `enter_battle_from_formation`, which tags each enemy actor
//! with its `battle_monster_id`). So this test:
//!   1. picks a scene whose encounter formation re-packs within its footprint on a
//!      scratch copy of the real disc,
//!   2. patches only one formation row's slot-0 monster id to a distinct value
//!      (exactly the surgical single-id edit the chest + drop oracles make),
//!   3. re-decodes the patched scene MAN off the patched image (the same bytes a
//!      fresh scene load would stream),
//!   4. forces that formation row into a battle through the live-loop encounter
//!      path and reads the spawned enemy actor's `battle_monster_id`,
//!   5. asserts the runtime spawns the **patched** id at that slot and never the
//!      original.
//!
//! A baseline pass over the *unpatched* MAN first confirms the engine spawns that
//! row's original monster at all, so the patched assertion can't pass vacuously.
//! Skips without `LEGAIA_DISC_BIN` (CLAUDE.md convention).

use legaia_engine_core::encounter_man::scene_encounter_from_man;
use legaia_engine_core::world::{SceneMode, World};
use legaia_rando::disc::DiscPatcher;
use legaia_rando::encounter::SceneEncounters;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Drive formation row `formation_id` of a decoded scene MAN into a battle and
/// return the monster id spawned into enemy slot 0 (battle actor
/// `party_count + 0`). This is the random-encounter spawn path: build the
/// table + per-row formation defs from the MAN, force that row at full rate (the
/// `FUN_801DA51C` carrier-by-index `install_man_formation`), then let the live
/// loop resolve the per-step roll + transition and flip `Field -> Battle`. A
/// fresh `World` keeps the actor table clean. Returns `None` if the MAN has no
/// rollable formations, the row isn't registered, or the battle never starts.
fn spawn_enemy0_for_formation(decoded_man: &[u8], formation_id: u16) -> Option<u16> {
    let (table, defs) = scene_encounter_from_man("oracle", decoded_man)?;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Field;
    world.live_gameplay_loop = true;
    // A capable lone party member so `enter_battle_from_formation` has a side to
    // place (the monster slot is what we read; the party slot just needs to exist).
    world.actors[0].active = true;
    world.actors[0].battle.hp = 400;
    world.actors[0].battle.max_hp = 400;
    world.actors[0].battle.liveness = 1;
    world.set_battle_attack(0, 80);

    // Register every row's formation def (built from THESE MAN bytes), then force
    // the target row so the next field step rolls it deterministically.
    world.install_man_encounter(table, defs);
    world.install_man_formation(formation_id)?;
    world.on_field_step();

    // Drive the live loop: the encounter session advances through its transition
    // window and spawns the battle. A handful of frames is plenty (transition is
    // ~32 frames); cap well above that.
    for _ in 0..256 {
        world.tick();
        if world.mode == SceneMode::Battle {
            // Enemy slot 0 is battle actor `party_count + 0`.
            return world
                .actors
                .get(world.party_count as usize)
                .and_then(|a| a.battle_monster_id);
        }
    }
    None
}

/// An arbitrary but distinct replacement id (validity is irrelevant to the
/// runtime-spawn proof: `enter_battle_from_formation` tags the enemy actor with
/// whatever id the formation row carries, catalog entry or not). Avoids `0`.
fn pick_replacement(original: u8) -> u8 {
    let candidate = original.wrapping_add(0x40);
    if candidate == 0 || candidate == original {
        0x42
    } else {
        candidate
    }
}

#[test]
fn patched_encounter_grants_new_monster_at_runtime() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };

    // --- Pick a scene with an encounter formation whose slot-0 single-id patch
    //     re-packs within the original compressed footprint (so the write is
    //     same-size in place, exactly like the encounter randomizer requires). ---
    let patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let mut chosen: Option<(usize, u16, u8, u8)> = None;
    'scan: for idx in 0..patcher.entry_count() {
        let Ok(entry) = patcher.read_entry(idx) else {
            continue;
        };
        let Some(scene) = SceneEncounters::locate(&entry, idx) else {
            continue;
        };
        for row in 0..scene.formation_count() {
            let ids = scene.formation_ids(row);
            let Some(&original) = ids.first() else {
                continue; // zero-monster row
            };
            let replacement = pick_replacement(original);
            let Some(off) = scene.formation_id_offset(row, 0) else {
                continue;
            };
            // Probe the same-size edit + re-pack fit without disturbing `scene`.
            let mut probe = SceneEncounters::locate(&entry, idx).unwrap();
            probe.decoded[off] = replacement;
            if probe.repack().is_some() {
                chosen = Some((idx, row as u16, original, replacement));
                break 'scan;
            }
        }
    }
    let (entry_idx, row, original, replacement) =
        chosen.expect("disc must hold a scene whose encounter formation re-packs in place");
    assert_ne!(
        original, replacement,
        "replacement must differ from original"
    );

    // --- Baseline: the engine spawns this row's ORIGINAL monster on the
    //     UNPATCHED disc (proves the path is reachable, non-vacuous). ---
    let entry = patcher.read_entry(entry_idx).unwrap();
    let scene = SceneEncounters::locate(&entry, entry_idx).unwrap();
    let baseline = spawn_enemy0_for_formation(&scene.decoded, row).unwrap_or_else(|| {
        panic!("engine must spawn scene {entry_idx} formation {row} into battle (baseline)")
    });
    assert_eq!(
        baseline, original as u16,
        "baseline: scene {entry_idx} formation {row} slot 0 must spawn the original 0x{original:02x}, got 0x{baseline:04x}"
    );

    // --- Patch only that one formation id on a scratch disc, then re-decode the
    //     MAN off the PATCHED image (what a fresh scene load reads). ---
    let mut patcher = DiscPatcher::open(disc).expect("reopen disc");
    let mut sc =
        SceneEncounters::locate(&patcher.read_entry(entry_idx).unwrap(), entry_idx).unwrap();
    let off = sc.formation_id_offset(row as usize, 0).unwrap();
    assert_eq!(
        sc.decoded[off], original,
        "site must still hold the original"
    );
    sc.decoded[off] = replacement;
    let stream = sc
        .repack()
        .expect("chosen scene MAN re-packs within its footprint");
    patcher
        .patch_prot_entry(entry_idx, sc.man_offset as u64, &stream)
        .expect("write patched scene MAN");

    // Re-decode off the patched image — disc-truth, not the in-memory copy.
    let patched_entry = patcher.read_entry(entry_idx).unwrap();
    let patched = SceneEncounters::locate(&patched_entry, entry_idx).unwrap();
    assert_eq!(
        patched.formation_ids(row as usize).first().copied(),
        Some(replacement),
        "patched disc bytes must carry the new monster id at the same formation slot"
    );

    // --- Runtime: force the SAME formation row into battle on the patched MAN.
    //     It must spawn the replacement and never the original. ---
    let runtime = spawn_enemy0_for_formation(&patched.decoded, row).unwrap_or_else(|| {
        panic!("engine must spawn scene {entry_idx} formation {row} into battle (patched)")
    });
    assert_eq!(
        runtime, replacement as u16,
        "runtime must spawn the patched id 0x{replacement:02x} (scene {entry_idx} formation {row}), \
         got 0x{runtime:04x} — a stale value here is the caching failure this test guards"
    );
    assert_ne!(
        runtime, original as u16,
        "runtime must NOT spawn the original 0x{original:02x} after the patch"
    );

    eprintln!(
        "encounter runtime E2E: scene {entry_idx} formation {row} slot 0 \
         baseline spawns 0x{baseline:04x}, patched spawns 0x{runtime:04x} \
         — 0x{original:02x} -> 0x{replacement:02x}"
    );
}

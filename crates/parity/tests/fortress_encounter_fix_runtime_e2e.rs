//! Disc-gated runtime oracle for the **Fortress encounter fix**
//! (`legaia-patcher --fortress-encounter-fix`).
//!
//! The patcher's own disc test (`crates/patcher/tests/fortress_encounter_fix_real`)
//! proves the rewritten condition list is *written* faithfully and reads the
//! same rows under every flag state. This one proves a runtime that walks the
//! list **acts** on it: with the Floating Castle's stale flag `0x15` set in the
//! flag bank, the engine's per-step region roll - the port of `FUN_801D9E1C`,
//! condition walk included - is silent on the retail tables and rolls on the
//! patched ones.
//!
//! A savestate cannot answer this (the scene MAN is resident in RAM while you
//! stand in the scene, so a state loaded on a patched disc still walks the
//! retail list); the engine builds its region table straight from the MAN
//! bytes a fresh scene load would stream, so it reads the patched disc.
//!
//! Per Fortress scene, on a walkable rate-18 tile:
//!   1. **baseline, healthy file** - retail MAN, flag clear: the step counter
//!      drains 18 a step and a battle rolls, the Door of Light is allowed
//!      (the path works at all - non-vacuous);
//!   2. **the defect** - retail MAN, flag `0x15` set: the counter never moves,
//!      no battle in 400 steps, the Door of Light is blocked;
//!   3. **the fix** - patched MAN, flag `0x15` set: identical to (1).
//!
//! Skips without `LEGAIA_DISC_BIN` (CLAUDE.md convention).

use legaia_disc_patch::disc::DiscPatcher;
use legaia_engine_battle::region_encounter::region_encounter_table_from_man;
use legaia_engine_core::world::World;
use legaia_engine_mode::mode::SceneMode;
use legaia_patcher::apply;
use legaia_patcher::encounter::SceneEncounters;
use legaia_patcher::fortress_encounter_fix::{FORTRESS_SCENES, STALE_FLAG};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// A walkable tile inside a rate-18 region of each scene's default group:
/// `deroa`'s north-west hall, `chitei2`'s main hall (the save-point room; the
/// tile the retail capture of this defect walks).
const TILES: [(i16, i16); 2] = [(10, 10), (49, 37)];

/// What `STEPS` walked steps on one tile produce.
#[derive(Debug, PartialEq, Eq)]
struct Walk {
    /// Step-counter drain on the first step (the scaled region rate).
    first_step_drain: i32,
    /// The step (1-based) a battle was rolled on, if any.
    battle_at: Option<usize>,
    /// `(Door of Light blocked, Door of Wind blocked)` after the first step.
    door_gates: (bool, bool),
}

const STEPS: usize = 400;

/// Seat a player on `tile` of the scene `man` describes and walk `STEPS`
/// field steps through `World::on_field_step`, with `stale_flag` deciding
/// whether system flag `0x15` is set in the flag bank.
fn walk(name: &str, man: &[u8], tile: (i16, i16), stale_flag: bool) -> Walk {
    let table = region_encounter_table_from_man(name, man).expect("region table from MAN");
    let mut world = World {
        mode: SceneMode::Field,
        player_actor_slot: Some(0),
        ..World::default()
    };
    world.actors[0].active = true;
    world.actors[0].move_state.world_x = tile.0 * 128 + 64;
    world.actors[0].move_state.world_z = tile.1 * 128 + 64;
    if stale_flag {
        world.system_flag_set(STALE_FLAG);
    }
    assert_eq!(world.system_flag_test(STALE_FLAG), stale_flag);
    world.set_field_regions(Some(table));
    // The cold-load step counter (the `SCUS_942.54` file value of
    // `_DAT_8007B5FC`), so every run starts from the same place.
    world.set_encounter_step_counter(974);

    let mut out = Walk {
        first_step_drain: 0,
        battle_at: None,
        door_gates: (false, false),
    };
    for step in 1..=STEPS {
        let before = world.encounter_step_counter();
        let rolled = world.on_field_step();
        if step == 1 {
            out.first_step_drain = before - world.encounter_step_counter();
            out.door_gates = world.region_door_gates();
        }
        if rolled {
            out.battle_at = Some(step);
            break;
        }
    }
    out
}

#[test]
fn stale_flag_silences_retail_fortress_and_not_the_patched_one() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let retail = DiscPatcher::open(disc.clone()).expect("open retail");
    let mut patched = DiscPatcher::open(disc).expect("open scratch");
    let report = apply::apply_fortress_encounter_fix(&mut patched).expect("apply fix");
    assert_eq!(report.scenes_changed, 2);

    let man = |p: &DiscPatcher, idx: usize| {
        let entry = p.read_entry(idx).expect("read entry");
        SceneEncounters::locate(&entry, idx)
            .expect("scene bundle MAN")
            .decoded
    };

    for ((idx, name), tile) in FORTRESS_SCENES.into_iter().zip(TILES) {
        let retail_man = man(&retail, idx);
        let patched_man = man(&patched, idx);

        // 1. Baseline: a healthy retail file rolls here.
        let healthy = walk(name, &retail_man, tile, false);
        assert_eq!(healthy.first_step_drain, 18, "{name}: rate-18 room");
        // 974 / 18 rounded up: the first battle after a cold load.
        assert_eq!(healthy.battle_at, Some(55), "{name}: healthy file rolls");
        assert_eq!(
            healthy.door_gates,
            (false, true),
            "{name}: Door of Light allowed, Door of Wind blocked"
        );

        // 2. The defect: the stale flag selects the castle's group.
        let affected = walk(name, &retail_man, tile, true);
        assert_eq!(
            affected,
            Walk {
                first_step_drain: 0,
                battle_at: None,
                door_gates: (true, true),
            },
            "{name}: retail is silent with flag 0x15 set, Door of Light greyed out"
        );

        // 3. The fix: the same save rolls like the healthy one.
        let healed = walk(name, &patched_man, tile, true);
        assert_eq!(healed, healthy, "{name}: patched disc ignores flag 0x15");
        // And a healthy file on the patched disc is unchanged.
        assert_eq!(walk(name, &patched_man, tile, false), healthy, "{name}");
    }
}

//! Disc-gated: the encounter step counter `_DAT_8007B5FC` is one global that
//! crosses doors, topped up at scene entry and rerolled by the op-`0x3E`
//! scripted-formation arm.
//!
//! Retail writes the counter from four places, every one through the reroll
//! leaf `FUN_801DDF48` (`r1 % 487 - r2 % 487 + 0x3CE`, `488..=1460`):
//!
//! 1. the region roll's trigger reset (`FUN_801D9E1C`, inlined);
//! 2. the scene-entry top-up in the SCUS system-script installer
//!    `FUN_8003AB2C` (`0x8003AC78..0x8003ACAC`): only a counter below `487`
//!    gains **half** a reroll, anything else crosses the door untouched;
//! 3. the op-`0x3E` scripted-formation arm (`0x801E076C`);
//! 4. field-VM op `4C EC` (`0x801E34F8`) - which no shipped script issues.
//!
//! Pinned here against real scenes: the carry and the top-up on field entry
//! (a scene with an encounter-region section, so the installed tracker is
//! seen to start from the carried value), and the reroll on a real scene's
//! registered scripted row.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_engine_core::region_encounter::region_encounter_table_from_man;
use legaia_engine_core::scene::{DefaultMapIdResolver, SceneHost, is_world_map_scene};

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn step_counter_carries_tops_up_and_rerolls_on_real_scenes() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse cdname");
    let mut scene_names: Vec<String> = cdname.values().cloned().collect();
    scene_names.sort();
    scene_names.dedup();

    // A field scene whose MAN carries an encounter-region section, so the
    // entry installs a region tracker we can read the seeded counter off.
    let scene = scene_names
        .iter()
        .find(|name| {
            if is_world_map_scene(name) || host.enter_field_scene(name, 0).is_err() {
                return false;
            }
            host.world.terrain.region_tracker.is_some()
                && host
                    .scene
                    .as_ref()
                    .and_then(|s| s.field_man_payload(&host.index).ok().flatten())
                    .and_then(|man| region_encounter_table_from_man(name, &man))
                    .is_some()
        })
        .cloned()
        .expect("some field scene installs a region tracker");

    // 1. Carry: a counter at or above 487 crosses the door untouched, and the
    //    new tracker starts from it (not from a per-scene 974 re-seed).
    host.world.set_encounter_step_counter(600);
    host.enter_field_scene(&scene, 0).expect("re-enter");
    assert_eq!(
        host.world.encounter_step_counter(),
        600,
        "{scene}: counter carried"
    );
    assert_eq!(
        host.world
            .terrain
            .region_tracker
            .as_ref()
            .map(|t| t.counter()),
        Some(600),
        "{scene}: the installed tracker is seeded from the carried counter"
    );

    // 2. Top-up: below 487 the entry adds half a reroll, 244..=730.
    host.world.set_encounter_step_counter(100);
    host.enter_field_scene(&scene, 0).expect("re-enter");
    let topped = host.world.encounter_step_counter();
    assert!(
        (100 + 244..=100 + 730).contains(&topped),
        "{scene}: topped-up counter {topped} outside 344..=830"
    );
    assert_eq!(
        host.world
            .terrain
            .region_tracker
            .as_ref()
            .map(|t| t.counter()),
        Some(topped)
    );

    // 3. The op-0x3E arm rerolls on a real scene's registered scripted row.
    let row = (0u8..=0x20)
        .find(|&r| {
            host.world
                .tables
                .formation_table
                .formation(u16::from(r))
                .is_some_and(|d| !d.slots.is_empty())
        })
        .expect("the scene registers at least one formation row");
    host.world.set_encounter_step_counter(3);
    assert!(host.world.trigger_scripted_battle(row));
    let rerolled = host.world.encounter_step_counter();
    assert!(
        (488..=1460).contains(&rerolled),
        "{scene}: rerolled counter {rerolled} outside 488..=1460"
    );
    eprintln!("[ok] {scene}: carry 600, top-up 100 -> {topped}, reroll row {row} -> {rerolled}");
}

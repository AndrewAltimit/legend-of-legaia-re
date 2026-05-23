//! Disc-gated: `BootSession::enter_world_map_live` loads a real world-map
//! scene, routes its region-keyed encounter table onto the overworld, installs
//! the player, and reaches `SceneMode::WorldMap` ready to roll encounters.
//!
//! This is the reachability test for the playable overworld: it asserts the
//! window's `--world-map` path (which now calls `enter_world_map_live`) ends up
//! with a region tracker installed and a player actor, then walks the player
//! across tiles and confirms a region encounter eventually flips into a battle.
//!
//! Skip-passes without disc data / extracted assets (the `LEGAIA_DISC_BIN`
//! convention) so CI works without Sony bytes.

use std::path::PathBuf;

use legaia_engine_core::input::PadButton;
use legaia_engine_core::world::SceneMode;
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn world_map_live_installs_regions_and_player() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing — run `legaia-extract` first");
        return;
    };

    // map03 is a kingdom-bundle world-map scene with a populated region table.
    let cfg = BootConfig {
        scene: "map03".into(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("open boot session");
    let opts = FieldLiveOpts::default();
    let mode = session
        .enter_world_map_live("map03", &opts)
        .expect("enter_world_map_live");

    assert_eq!(mode, SceneMode::WorldMap, "ends in world-map mode");
    let world = &session.host.world;
    assert!(
        world.world_map_region_tracker.is_some(),
        "the scene's region table is routed onto the overworld"
    );
    assert!(
        world.player_actor_slot.is_some(),
        "an overworld player actor is installed"
    );
    let regions = world
        .world_map_region_tracker
        .as_ref()
        .map(|t| t.table().regions.len())
        .unwrap_or(0);
    assert!(regions > 0, "map03 routes encounter regions ({regions})");
    eprintln!("[map03] world-map live: {regions} regions, player installed");
}

#[test]
fn world_map_live_walk_reaches_battle() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let cfg = BootConfig {
        scene: "map03".into(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("open boot session");
    let opts = FieldLiveOpts::default();
    session
        .enter_world_map_live("map03", &opts)
        .expect("enter_world_map_live");

    let world = &mut session.host.world;
    // Place the player at the centre of the first rollable region so a walk
    // stays in-region and the roll has a real formation to pick.
    let pos = world.world_map_region_tracker.as_ref().and_then(|t| {
        t.table()
            .regions
            .iter()
            .find(|r| r.formation_count > 0 && r.rate_increment > 0)
            .map(|r| {
                (
                    ((r.tile_x_min as i32 + r.tile_x_max as i32) / 2 * 128) as i16,
                    ((r.tile_z_min as i32 + r.tile_z_max as i32) / 2 * 128) as i16,
                )
            })
    });
    let Some((px, pz)) = pos else {
        eprintln!("[skip] map03 has no rollable region");
        return;
    };
    if let Some(slot) = world.player_actor_slot {
        let a = &mut world.actors[slot as usize];
        a.move_state.world_x = px;
        a.move_state.world_z = pz;
    }
    // Re-seed the step latch so the move above isn't counted as a crossing.
    world.world_map_last_tile = None;

    // Walk +X; each 128-unit tile crossing rolls the region. A real region's
    // rate is modest, so allow a generous budget.
    world.set_pad(PadButton::Right.mask());
    let mut reached = false;
    for _ in 0..20_000 {
        let _ = world.tick();
        if world.mode == SceneMode::Battle {
            reached = true;
            break;
        }
        // Stay in-region: if the player walked out of every region, recentre.
        if world
            .world_map_region_tracker
            .as_ref()
            .and_then(|t| {
                let a = &world.actors[world.player_actor_slot.unwrap() as usize];
                t.table()
                    .region_at_world(a.move_state.world_x, a.move_state.world_z)
            })
            .is_none()
        {
            let slot = world.player_actor_slot.unwrap() as usize;
            world.actors[slot].move_state.world_x = px;
            world.actors[slot].move_state.world_z = pz;
        }
    }
    assert!(
        reached,
        "walking the overworld in-region reaches a battle within the budget"
    );
    assert_eq!(world.battle_return_mode, SceneMode::WorldMap);
}

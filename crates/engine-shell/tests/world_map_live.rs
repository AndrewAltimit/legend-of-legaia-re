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
    // Report whether the overworld scene actually carries a walkability grid
    // (the same `_DAT_1f8003ec + 0x4000` source the field uses). A non-empty
    // grid means the overworld collision path is genuinely exercised; an empty
    // grid means the scene has no MAP block and the player roams unbounded.
    let wall_nibbles: usize = world
        .field_collision_grid
        .iter()
        .map(|b| (b >> 4).count_ones() as usize)
        .sum();
    eprintln!(
        "[map03] world-map live: {regions} regions, player installed, \
         collision grid {} bytes ({wall_nibbles} wall sub-cells)",
        world.field_collision_grid.len()
    );
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

/// Every kingdom overworld scene loads a **non-empty** walkability grid into
/// the live world after `enter_world_map_live`.
///
/// The overworld walk overlay's locomotion is the same `FUN_801d01b0` +
/// `FUN_801cfe4c` as the field, colliding against the same
/// `_DAT_1f8003ec + 0x4000` grid — and the three kingdom maps carry thousands
/// of wall sub-cells in that grid. This guards the regression where a redundant
/// `install_field_player` after scene load reset the grid to zeros and left the
/// overworld player roaming unbounded.
#[test]
fn overworld_scenes_load_nonempty_walkability_grid() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    for scene in ["map01", "map02", "map03"] {
        let cfg = BootConfig {
            scene: scene.into(),
            enable_audio: false,
        };
        let mut session = BootSession::open(&extracted, &cfg).expect("open boot session");
        let opts = FieldLiveOpts::default();
        session
            .enter_world_map_live(scene, &opts)
            .expect("enter_world_map_live");
        let walls: usize = session
            .host
            .world
            .field_collision_grid
            .iter()
            .map(|b| (b >> 4).count_ones() as usize)
            .sum();
        eprintln!("[{scene}] live walkability grid: {walls} wall sub-cells");
        assert!(
            walls > 0,
            "{scene}: overworld must keep its walkability grid (got {walls} walls — \
             a re-install likely zeroed it)"
        );
    }
}

/// A field-VM `scene_transition(map_id)` that resolves to an overworld scene is
/// auto-routed into world-map mode with its region table seeded — the
/// boot/transition path, not the explicit `--world-map` entry. This is the
/// regression guard for "the overworld seeds itself when the game walks onto
/// it", driving the real `SceneHost::tick` transition handler.
#[test]
fn world_map_scene_transition_auto_enters_world_map() {
    use legaia_engine_core::scene::{SceneTickEvent, VecMapIdResolver};
    use legaia_engine_core::world::SceneMode;

    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };

    // Boot on a town, then transition to the overworld the way the field VM
    // would: map_id 0 -> "map03" via the resolver, latched as a pending
    // scene transition that `SceneHost::tick` processes.
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("open boot session");
    session
        .host
        .set_map_resolver(Box::new(VecMapIdResolver::new(vec!["map03".into()])));
    session.host.world.pending_scene_transition = Some(0);

    let event = session.host.tick().expect("tick processes the transition");
    assert!(
        matches!(&event, SceneTickEvent::SceneEntered { name } if name == "map03"),
        "transition entered map03, got {event:?}"
    );

    let world = &session.host.world;
    assert_eq!(
        world.mode,
        SceneMode::WorldMap,
        "an overworld transition lands in world-map mode, not plain Field"
    );
    assert!(
        world.world_map_region_tracker.is_some(),
        "the overworld region table is seeded on the transition path"
    );
    assert!(
        world.world_map_ctrl.is_some(),
        "the world-map camera controller is installed"
    );
    let walls: usize = world
        .field_collision_grid
        .iter()
        .map(|b| (b >> 4).count_ones() as usize)
        .sum();
    assert!(
        walls > 0,
        "the overworld walkability grid is loaded ({walls} walls)"
    );
}

/// A real overworld portal auto-engages when the player walks onto its tile:
/// teleport the player onto a classified `Portal` entity's tile and confirm the
/// next world-map ticks surface a `WorldMapTransition` to the portal's target
/// map — no host `engage_world_map_entity` call.
#[test]
fn world_map_walking_onto_real_portal_transitions() {
    use legaia_engine_core::field_events::FieldEvent;
    use legaia_engine_core::world::WorldMapEntityConfig;

    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };

    // map02 / map03 each classify one warp portal from their placement scripts.
    let mut tested_any = false;
    for scene in ["map02", "map03"] {
        let cfg = BootConfig {
            scene: scene.into(),
            enable_audio: false,
        };
        let mut session = BootSession::open(&extracted, &cfg).expect("open boot session");
        let opts = FieldLiveOpts::default();
        session
            .enter_world_map_live(scene, &opts)
            .expect("enter_world_map_live");

        let world = &mut session.host.world;
        // Find the first installed Portal entity and its placement position.
        let portal = world
            .world_map_entity_configs
            .iter()
            .enumerate()
            .find_map(|(i, c)| match c {
                WorldMapEntityConfig::Portal { target_map } => Some((i, *target_map)),
                _ => None,
            });
        let Some((idx, target_map)) = portal else {
            eprintln!("[{scene}] no portal entity installed");
            continue;
        };
        let (px, pz) = world.world_map_entity_positions[idx];
        // Teleport the player onto the portal tile, then tick: auto-engage
        // should fire the transition within a couple of frames.
        if let Some(slot) = world.player_actor_slot {
            let a = &mut world.actors[slot as usize];
            a.move_state.world_x = px;
            a.move_state.world_z = pz;
        }
        let mut transitioned = false;
        for _ in 0..4 {
            let _ = world.tick();
            if world.drain_field_events().into_iter().any(|e| {
                matches!(e, FieldEvent::WorldMapTransition { target_map: t, .. } if t == target_map)
            }) {
                transitioned = true;
                break;
            }
        }
        eprintln!("[{scene}] portal #{idx} -> map {target_map}: transitioned={transitioned}");
        assert!(
            transitioned,
            "[{scene}] standing on the portal tile auto-fires its transition"
        );
        tested_any = true;
    }
    assert!(
        tested_any,
        "at least one overworld portal must be exercised"
    );
}

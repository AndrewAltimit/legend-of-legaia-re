//! Disc-gated: the Rebirth pack's `rebirth_tower_1f` map entered as a live
//! field scene over its retail base scene (`tower`): walked, blocked by a
//! wall, its triggers run through the pack's quest and battle runtimes, its
//! encounter zone rolling the pack's table, a save made on it carrying the
//! map, and its stair exit into Rim Elm.
//!
//! The pack is the committed `packs/rebirth`, read the way the host reads it
//! (`read_files`). Skips when `LEGAIA_DISC_BIN` is unset.

use std::sync::Arc;

use legaia_engine_mode::mode::SceneMode;
use legaia_engine_save_state::WorldSaveState;
use legaia_engine_scene::expansion_map::tile_centre_world;
use legaia_engine_scene_host::scene_host::{SceneHost, SceneTickEvent};
use legaia_engine_system::input::PadButton;
use legaia_expansion_format::ExpansionPack;
use legaia_expansion_format::map::MapDef;

const MAP_ID: &str = "rebirth_tower_1f";

fn rebirth_pack() -> ExpansionPack {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/rebirth");
    let files = legaia_expansion_format::read_files(std::path::Path::new(dir)).expect("read");
    ExpansionPack::from_files(&files).expect("the Rebirth pack parses")
}

fn host() -> Option<SceneHost> {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    };
    let mut host = SceneHost::open_disc(disc).expect("open the disc");
    host.activate_expansion_pack(Arc::new(rebirth_pack()))
        .expect("the Rebirth pack activates");
    Some(host)
}

fn def(host: &SceneHost) -> MapDef {
    host.world.expansion.pack.as_ref().unwrap().maps[MAP_ID].clone()
}

fn seat(host: &mut SceneHost, col: u16, row: u16) {
    let (x, z) = tile_centre_world(&def(host), col, row);
    host.world.actors[0].move_state.world_x = x;
    host.world.actors[0].move_state.world_z = z;
}

fn flag(host: &SceneHost, f: &str) -> bool {
    host.world.expansion.quests.progress.flags.is_set(f)
}

fn player_xz(host: &SceneHost) -> (i16, i16) {
    let ms = &host.world.actors[0].move_state;
    (ms.world_x, ms.world_z)
}

/// Hold `btn` for `frames` host ticks; returns the first non-step event.
fn hold(host: &mut SceneHost, btn: PadButton, frames: usize) -> Option<SceneTickEvent> {
    for _ in 0..frames {
        host.world.set_pad(btn.mask());
        match host.tick().expect("tick") {
            SceneTickEvent::Stepped => {}
            other => return Some(other),
        }
    }
    None
}

#[test]
fn the_tower_floor_enters_walks_blocks_and_exits_to_rim_elm() {
    let Some(mut host) = host() else {
        return;
    };
    host.enter_expansion_map(MAP_ID, "entrance")
        .expect("enter the pack map");
    assert_eq!(host.world.mode, SceneMode::Field);
    assert_eq!(host.active_expansion_map(), Some(MAP_ID));
    assert_eq!(host.scene.as_ref().map(|s| s.name.as_str()), Some("tower"));
    let def = def(&host);

    // Seated on the entrance spawn's tile centre, on its floor.
    assert_eq!(player_xz(&host), tile_centre_world(&def, 7, 10));
    assert_eq!(host.world.actors[0].move_state.world_y, 0);

    // The base scene's script layer is off: no entry script, no MAN.
    assert!(host.world.field_bytecode.is_empty());
    // The map's name stands as the place-name banner.
    assert_eq!(
        host.world
            .cutscene
            .text_balloon
            .as_ref()
            .map(|b| b.text.clone()),
        Some(def.name.as_bytes().to_vec())
    );
    let scene = host.scene.as_ref().unwrap();
    assert!(scene.field_man_payload(&host.index).unwrap().is_none());
    // The base scene's VRAM and geometry pack still load: they draw the map.
    assert!(
        host.resources.as_ref().is_some_and(|r| !r.tmds.is_empty()),
        "the base scene's meshes are resident"
    );
    // The map's props come back as the scene's placements.
    let placed = scene.field_object_placements(&host.index).unwrap().unwrap();
    assert_eq!(placed.len(), def.props.len(), "{placed:?}");

    // Walk: N steps west along row 10 move the player.
    let start = player_xz(&host);
    assert!(hold(&mut host, PadButton::Left, 20).is_none());
    let moved = player_xz(&host);
    assert!(
        moved.0 < start.0 - 40,
        "walked west: {start:?} -> {moved:?}"
    );
    assert_eq!(moved.1, start.1, "a straight walk");

    // Blocked: keep walking west into the arena wall (column 0).
    assert!(hold(&mut host, PadButton::Left, 200).is_none());
    let (wx, _) = player_xz(&host);
    eprintln!("[{MAP_ID}] walked {start:?} -> {moved:?}, blocked at x = {wx}");
    let wall_edge = tile_centre_world(&def, 0, 10).0 + 64;
    assert!(
        wx > wall_edge && wx < wall_edge + 128,
        "stopped against the west wall: x = {wx}, wall face at {wall_edge}"
    );

    // The entrance trigger counts as talking to the tower guide, and its
    // once-flag goes up with the pack's progress.
    assert!(!flag(&host, "map.rebirth_tower_1f.entrance_guide"));
    seat(&mut host, 7, 9);
    let _ = host.tick().expect("tick");
    assert!(flag(&host, "map.rebirth_tower_1f.entrance_guide"));
    assert!(
        host.world
            .expansion
            .map
            .as_ref()
            .unwrap()
            .pending_events()
            .is_empty()
    );

    // Exit: stand on the landing and walk south onto the stair tile.
    seat(&mut host, 7, 11);
    let _ = host.tick().expect("tick");
    let event = hold(&mut host, PadButton::Up, 400);
    match event {
        Some(SceneTickEvent::SceneEntered { name }) => assert_eq!(name, "town01"),
        other => panic!("expected the stair exit into town01, got {other:?}"),
    }
    assert!(host.active_expansion_map().is_none(), "left the pack map");
    assert!(host.scene.as_ref().unwrap().overrides.is_none());
    // Arrived on the exit's tile in Rim Elm, on the retail field layer.
    let (x, z) = player_xz(&host);
    eprintln!("[{MAP_ID}] stair exit landed in town01 at ({x}, {z})");
    assert_eq!((x >> 7, z >> 7), (28, 91));
    assert!(
        host.scene
            .as_ref()
            .unwrap()
            .field_man_payload(&host.index)
            .unwrap()
            .is_some(),
        "Rim Elm's own MAN is back"
    );
}

#[test]
fn a_map_to_unknown_spawn_or_map_fails_without_leaving_the_scene() {
    let Some(mut host) = host() else {
        return;
    };
    assert!(host.enter_expansion_map(MAP_ID, "nowhere").is_err());
    assert!(host.enter_expansion_map("no_such_map", "").is_err());
    assert!(host.scene.is_none());
    assert!(host.active_expansion_map().is_none());
}

#[test]
fn the_dais_wakes_its_guardian_once_ever() {
    let Some(mut host) = host() else {
        return;
    };
    host.enter_expansion_map(MAP_ID, "entrance").unwrap();
    seat(&mut host, 7, 6);
    let _ = host.tick().expect("tick");
    assert!(flag(&host, "map.rebirth_tower_1f.dais_guardian"));
    let forge = host.world.expansion_formation_id("forge_guard").unwrap();
    let mut fought = None;
    for _ in 0..400 {
        let _ = host.tick().expect("tick");
        if host.world.mode == SceneMode::Battle {
            fought = host
                .world
                .battle
                .active_formation
                .as_ref()
                .map(|f| f.formation_id);
            break;
        }
    }
    assert_eq!(
        fought,
        Some(forge),
        "the dais trigger starts the pack fight"
    );
    // Re-entering the map: the trigger's saved flag keeps it quiet.
    host.world.abandon_live_battle();
    host.enter_expansion_map(MAP_ID, "entrance").unwrap();
    seat(&mut host, 7, 6);
    for _ in 0..60 {
        let _ = host.tick().expect("tick");
    }
    assert_ne!(host.world.mode, SceneMode::Battle);
}

#[test]
fn the_outer_ring_rolls_the_pack_table_and_the_floor_rolls_nothing_else() {
    let Some(mut host) = host() else {
        return;
    };
    host.enter_expansion_map(MAP_ID, "entrance").unwrap();
    host.world.toggles.live_gameplay_loop = true;
    let ids = [
        host.world.expansion_formation_id("ogre_pack").unwrap(),
        host.world.expansion_formation_id("forge_guard").unwrap(),
    ];
    // Pace the open floor below the ring (rows 9..10): no zone, no fight.
    for _ in 0..6 {
        assert!(hold(&mut host, PadButton::Left, 80).is_none());
        assert!(hold(&mut host, PadButton::Right, 80).is_none());
        assert_ne!(
            host.world.mode,
            SceneMode::Battle,
            "no fight outside every zone"
        );
    }
    // Pace the outer ring (row 1) until the pack's step counter fires.
    seat(&mut host, 4, 1);
    let mut fought = None;
    'walk: for _ in 0..40 {
        for btn in [PadButton::Right, PadButton::Left] {
            for _ in 0..80 {
                host.world.set_pad(btn.mask());
                let _ = host.tick().expect("tick");
                if host.world.mode == SceneMode::Battle {
                    fought = host
                        .world
                        .battle
                        .active_formation
                        .as_ref()
                        .map(|f| f.formation_id);
                    break 'walk;
                }
            }
        }
    }
    let fought = fought.expect("the ring rolls an encounter");
    assert!(
        ids.contains(&fought),
        "a `tower_1f` formation, got {fought:#06X}"
    );
}

#[test]
fn a_save_on_the_map_carries_the_map_and_where_on_it() {
    let Some(mut host) = host() else {
        return;
    };
    host.enter_expansion_map(MAP_ID, "entrance").unwrap();
    seat(&mut host, 3, 5);
    let at = player_xz(&host);
    let save = host.world.save_full();
    let block = save
        .ext_v2
        .expansion
        .clone()
        .expect("a pack save carries the block");
    assert!(!block.is_empty());

    // Loaded into a fresh world with the pack active, the map location reads
    // back; entering it seated puts the player where the save was made.
    let Some(mut other) = self::host() else {
        return;
    };
    other.world.load_full(save);
    let loc = other
        .world
        .expansion_saved_map_location()
        .cloned()
        .expect("the save names the map");
    assert_eq!((loc.map_id.as_str(), loc.x, loc.z), (MAP_ID, at.0, at.1));
    other
        .enter_expansion_map_seated(&loc.map_id, "", Some((loc.x, loc.z, loc.facing)))
        .unwrap();
    assert_eq!(player_xz(&other), at);

    // Off every map the next save carries no location.
    other.enter_field_scene("town01", 0).unwrap();
    let save = other.world.save_full();
    other.world.load_full(save);
    assert!(other.world.expansion_saved_map_location().is_none());
}

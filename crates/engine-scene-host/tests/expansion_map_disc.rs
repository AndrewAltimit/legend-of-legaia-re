//! Disc-gated: the Rebirth pack's `rebirth_tower_1f` map entered as a live
//! field scene over its retail base scene (`tower`), walked, blocked by a
//! wall, and left through its stair exit into Rim Elm.
//!
//! The pack's manifest is synthesised here (the map file is the committed
//! one), so the test does not depend on the rest of the pack tree. Skips
//! when `LEGAIA_DISC_BIN` is unset.

use std::sync::Arc;

use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene::expansion_map::tile_centre_world;
use legaia_engine_scene_host::scene_host::{SceneHost, SceneTickEvent};
use legaia_engine_system::input::PadButton;
use legaia_expansion_format::map::MapEvent;
use legaia_expansion_format::{ExpansionPack, PackFiles};

const MAP_ID: &str = "rebirth_tower_1f";

fn rebirth_pack() -> ExpansionPack {
    let map = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../packs/rebirth/maps/rebirth_tower_1f.toml"
    ))
    .expect("the sample map is committed");
    let mut files = PackFiles::new();
    files.insert(
        "pack.toml".into(),
        b"id = \"rebirth\"\nname = \"Rebirth\"\nversion = \"0.0.0\"\n".to_vec(),
    );
    files.insert(format!("maps/{MAP_ID}.toml"), map);
    ExpansionPack::from_files(&files).expect("pack parses")
}

fn host() -> Option<SceneHost> {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    };
    let mut host = SceneHost::open_disc(disc).expect("open the disc");
    host.set_expansion_pack(Arc::new(rebirth_pack()));
    Some(host)
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
    let def = host.world.expansion.pack.as_ref().unwrap().maps[MAP_ID].clone();

    // Seated on the entrance spawn's tile centre, on its floor.
    assert_eq!(player_xz(&host), tile_centre_world(&def, 7, 10));
    assert_eq!(host.world.actors[0].move_state.world_y, 0);

    // The base scene's script layer is off: no entry script, no MAN.
    assert!(host.world.field_bytecode.is_empty());
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

    // The entrance trigger row raises its dialog event on entry.
    let (ex, ez) = tile_centre_world(&def, 7, 9);
    host.world.actors[0].move_state.world_x = ex;
    host.world.actors[0].move_state.world_z = ez;
    let _ = host.tick().expect("tick");
    let events = host.world.expansion.map.as_mut().unwrap().take_events();
    assert_eq!(
        events,
        vec![MapEvent::Dialog {
            line: "tower_1f_welcome".into()
        }]
    );

    // Exit: stand on the landing and walk south onto the stair tile.
    let (lx, lz) = tile_centre_world(&def, 7, 11);
    host.world.actors[0].move_state.world_x = lx;
    host.world.actors[0].move_state.world_z = lz;
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

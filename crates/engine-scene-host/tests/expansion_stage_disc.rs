//! Disc-gated: golf and racing played in the world. The `links` fixture
//! pack's course and track (`crates/engine-modes/tests/fixtures/links`) are
//! entered as modes; the scene host enters each one's pack map under the
//! running mode seat (`SceneHost::sync_pack_mode_map`), and the in-world
//! stage then maps the session onto the live field - the leader on the
//! tee, on the floor the map's field image gives. Skips when
//! `LEGAIA_DISC_BIN` is unset.

use std::sync::Arc;

use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene_host::scene_host::{SceneHost, SceneTickEvent};
use legaia_expansion_format::ExpansionPack;

fn links_pack() -> ExpansionPack {
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../engine-modes/tests/fixtures/links"
    );
    let files = legaia_expansion_format::read_files(std::path::Path::new(dir)).expect("read");
    ExpansionPack::from_files(&files).expect("the links pack parses")
}

fn host() -> Option<SceneHost> {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    };
    let mut host = SceneHost::open_disc(disc).expect("open the disc");
    host.activate_expansion_pack(Arc::new(links_pack()))
        .expect("the links pack activates");
    host.enter_field_scene("town01", 0).expect("enter town01");
    Some(host)
}

/// Enter mode `id` from Rim Elm and tick until its map is up.
fn enter(host: &mut SceneHost, id: &str, map: &str) {
    host.world.enter_pack_mode(id).expect("the mode builds");
    assert_eq!(
        host.world.pack_mode_wants_map().map(|m| m.0),
        Some(map.to_string())
    );
    let mut entered = false;
    for _ in 0..4 {
        host.world.set_pad(0);
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            assert_eq!(name, "tower");
            entered = true;
        }
    }
    assert!(entered, "the map entered under the seat");
    assert_eq!(host.active_expansion_map(), Some(map));
    assert_eq!(host.world.mode, SceneMode::Expansion, "the seat survives");
    assert!(host.world.pack_mode_wants_map().is_none());
}

#[test]
fn an_in_world_golf_hole_on_a_pack_map() {
    let Some(mut host) = host() else {
        return;
    };
    enter(&mut host, "links-golf", "links_green");
    let stage = host
        .world
        .pack_mode_stage()
        .expect("the stage runs")
        .clone();
    let def = host.world.expansion.pack.as_ref().unwrap().maps["links_green"].clone();
    let grid = def.tile_grid().unwrap();
    let ball = stage.balls.iter().find(|b| b.seat == 0).unwrap();
    assert_eq!(
        def.tile_at_world(&grid, ball.pos[0] as i32, ball.pos[1] as i32),
        Some((2, 2))
    );
    // The leader stands at the tee on the live floor.
    let a = &host.world.actors[0].move_state;
    assert_eq!(
        i32::from(a.world_y),
        host.world
            .sample_field_floor_height(a.world_x.into(), a.world_z.into())
    );
    // The raised plateau is the map's tier 1 on the live field.
    let centre = |c: i32| (16 + c) * 128 + 64;
    assert_eq!(
        host.world.sample_field_floor_height(centre(6), centre(7)),
        -48
    );
    // Quitting from the prompt returns to the field of the map.
    host.world.exit_pack_mode();
    assert_eq!(host.world.mode, SceneMode::Field);
    assert_eq!(host.active_expansion_map(), Some("links_green"));
}

#[test]
fn an_in_world_race_on_a_pack_map() {
    let Some(mut host) = host() else {
        return;
    };
    enter(&mut host, "links-race", "links_ring");
    let stage = host
        .world
        .pack_mode_stage()
        .expect("the stage runs")
        .clone();
    assert_eq!(stage.bodies.len(), 2);
    assert_eq!(stage.banner.as_deref(), Some("3"), "the countdown");
}

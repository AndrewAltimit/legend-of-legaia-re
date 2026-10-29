//! Disc-gated: the Rebirth pack's way in from the retail game. The
//! `rim_elm_mist_stair` entrance (`maps/rebirth_tower_1f.toml`) stands in
//! Rim Elm's `town01`, behind the woodpile by the south gate: stepping onto
//! it enters the tower map once the party has reached level 30, the arrival
//! back from the tower does not bounce into it, and the pause menu's Travel
//! request enters the map from anywhere.
//!
//! The pack is the committed `packs/rebirth`. Skips when `LEGAIA_DISC_BIN`
//! is unset.

use std::sync::Arc;

use crate::scene_host::{SceneHost, SceneTickEvent};
use legaia_expansion_format::ExpansionPack;

const MAP_ID: &str = "rebirth_tower_1f";

fn host() -> Option<SceneHost> {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    };
    let mut host = SceneHost::open_disc(disc).expect("open the disc");
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/rebirth");
    let files = legaia_expansion_format::read_files(std::path::Path::new(dir)).expect("read");
    let pack = ExpansionPack::from_files(&files).expect("the Rebirth pack parses");
    host.activate_expansion_pack(Arc::new(pack))
        .expect("the Rebirth pack activates");
    Some(host)
}

fn set_level(host: &mut SceneHost, level: u8) {
    let w = &mut host.world;
    if w.party.roster.members.is_empty() {
        w.party
            .roster
            .members
            .push(legaia_save::CharacterRecord::zeroed());
        w.party.party_count = 1;
    }
    for m in &mut w.party.roster.members {
        m.set_level(level);
    }
}

/// Seat the player on field-grid tile `(x, z)` and run one host tick.
fn step_onto(host: &mut SceneHost, x: u8, z: u8) -> SceneTickEvent {
    host.world.seat_player_at_tile(x, z);
    host.tick().expect("tick")
}

#[test]
fn the_rim_elm_entrance_opens_at_level_30_and_enters_the_tower() {
    let Some(mut host) = host() else {
        return;
    };
    host.enter_field_scene("town01", 0).expect("enter Rim Elm");
    // The entrance tiles are open floor the room's walk reaches.
    for (x, z) in [(17, 43), (18, 43)] {
        let (wx, wz) = (x * 128 + 64, z * 128 + 64);
        assert!(
            !host.world.field_tile_is_wall(wx, wz),
            "({x}, {z}) is floor"
        );
        assert!(host.world.field_collision_reaches_floor(wx, wz, 4096));
    }
    // Below level 30 the entrance is shut.
    set_level(&mut host, 29);
    assert!(matches!(
        step_onto(&mut host, 16, 43),
        SceneTickEvent::Stepped
    ));
    assert!(matches!(
        step_onto(&mut host, 17, 43),
        SceneTickEvent::Stepped
    ));
    assert!(host.active_expansion_map().is_none());
    // At level 30, stepping off and back on enters the tower.
    set_level(&mut host, 30);
    assert!(matches!(
        step_onto(&mut host, 16, 43),
        SceneTickEvent::Stepped
    ));
    match step_onto(&mut host, 17, 43) {
        SceneTickEvent::SceneEntered { name } => assert_eq!(name, "tower"),
        other => panic!("expected the tower map, got {other:?}"),
    }
    assert_eq!(host.active_expansion_map(), Some(MAP_ID));

    // The tower's stair lands on (20, 44), outside the entrance: a few free
    // ticks there stay in Rim Elm.
    host.enter_field_scene("town01", 0)
        .expect("back to Rim Elm");
    host.world.seat_player_at_tile(20, 44);
    for _ in 0..4 {
        assert!(matches!(
            host.tick().expect("tick"),
            SceneTickEvent::Stepped
        ));
    }
    assert!(host.active_expansion_map().is_none());

    // The pause menu's Travel page enters the map from anywhere.
    assert!(host.world.request_expansion_travel(MAP_ID, "dais"));
    match host.tick().expect("tick") {
        SceneTickEvent::SceneEntered { .. } => {}
        other => panic!("expected the Travel entry, got {other:?}"),
    }
    assert_eq!(host.active_expansion_map(), Some(MAP_ID));
}

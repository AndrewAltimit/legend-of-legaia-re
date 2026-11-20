//! Disc-gated: the village Genesis Tree stands in Rim Elm's plaza on every
//! scene-picker route back into the village.
//!
//! Each Rim Elm scene places its tree as a partition-1 actor whose header
//! tile is the off-map hide box `(16320, 16320)`; the spawn prologue runs it
//! into the plaza (`4C 51 20 5D`, world `(4160, 11968)`) behind a
//! `SysFlag.Test 0x141`, the tree-revived flag:
//!
//! - `town01` `P1[50]`, the withered tree: runs in while `0x141` is clear;
//! - `town0b` `P1[55]` (withered) while clear, `P1[56]` (revived) while set;
//! - `town0c` `P1[41]`, the revived tree: runs in only while `0x141` is set.
//!
//! The overworld's door back into the village (`map01` `P2[0]`) is a
//! `SceneChange` into `town0c` with no flag test, and retail can only reach
//! `map01` after the elder's talk (`town0b` `P1[55]`) has raised `0x141`.
//! A picker visit anywhere but the village used to clear the flag (the
//! free-roam baseline's managed reset) and an overworld pick left it as the
//! previous pick had, so walking back into Rim Elm drew it treeless - unless
//! the last pick happened to be `town0c`. `World::seed_picker_story_flags`
//! raises Rim Elm's departure flags for every pick past it.
//!
//! Skip-passes without `LEGAIA_DISC_BIN`.

use legaia_engine_scene_host::scene_host::{SceneHost, SceneTickEvent};
use legaia_engine_system::input::PadButton;

/// The plaza square the trees run to (`4C 51 20 5D`: tile `(32, 93)`).
const PLAZA: (i16, i16) = (32 * 128 + 0x40, 93 * 128 + 0x40);

fn open_host() -> Option<SceneHost> {
    let disc = std::env::var("LEGAIA_DISC_BIN")
        .ok()
        .filter(|p| std::path::Path::new(p).exists())?;
    Some(SceneHost::open_disc(&disc).expect("open disc"))
}

/// The placement `slot`'s header tile, as both hosts' NPC passes read it.
fn spawn_tile(h: &SceneHost, slot: u8) -> (i16, i16) {
    let man = h
        .scene
        .as_ref()
        .and_then(|s| s.field_man_payload(&h.index).ok().flatten())
        .expect("scene MAN");
    let mf = legaia_scene_formats::man_section::parse(&man).expect("parse MAN");
    let p = legaia_engine_field::man_field_scripts::classify_placements(&mf, &man)
        .into_iter()
        .map(|(p, _)| p)
        .find(|p| p.index == usize::from(slot))
        .expect("tree placement");
    (p.world_x, p.world_z)
}

/// Whether placement `slot` draws in the plaza: run there off its hide-box
/// header, with a draw pose (`World::field_npc_draw_pose`, which both play
/// hosts' NPC passes take and which is `None` in the hide box).
fn tree_in_plaza(h: &SceneHost, slot: u8) -> bool {
    let spawn = spawn_tile(h, slot);
    h.world.npcs.positions.get(&slot) == Some(&PLAZA)
        && h.world.field_npc_draw_pose(slot, spawn).is_some()
}

/// The tree `scene` should draw with the flags it was entered under.
fn assert_tree(h: &SceneHost, scene: &str, route: &str) {
    let revived = h.world.system_flag_test(0x141);
    let (drawn, parked): (u8, &[u8]) = match (scene, revived) {
        ("town01", false) => (50, &[]),
        ("town0b", false) => (55, &[56]),
        ("town0b", true) => (56, &[55]),
        ("town0c", true) => (41, &[]),
        _ => panic!("[{route}] {scene} entered with 0x141 = {revived}: no retail visit does"),
    };
    assert!(
        tree_in_plaza(h, drawn),
        "[{route}] {scene} P1[{drawn}] stands in the plaza (pos {:?})",
        h.world.npcs.positions.get(&drawn)
    );
    for &p in parked {
        assert!(
            !tree_in_plaza(h, p),
            "[{route}] {scene} P1[{p}] stays in the hide box"
        );
    }
}

fn settle(h: &mut SceneHost, n: usize) {
    for _ in 0..n {
        h.tick().expect("tick");
    }
}

fn seat(h: &mut SceneHost, tile_x: i16, tile_z: i16) {
    let slot = h.world.player_actor_slot.expect("player installed") as usize;
    h.world.actors[slot].move_state.world_x = tile_x * 128 + 0x40;
    h.world.actors[slot].move_state.world_z = tile_z * 128 + 0x40;
    settle(h, 2);
}

fn walk_until_scene(h: &mut SceneHost, pad: u16) -> Option<String> {
    let mut entered = None;
    for _ in 0..600 {
        h.world.set_pad(pad);
        if let SceneTickEvent::SceneEntered { name } = h.tick().expect("tick") {
            entered = Some(name);
            break;
        }
    }
    h.world.set_pad(0);
    entered
}

/// A picker entry as both play hosts make it (`World::stage_picker_entry`
/// plus the seat arm), field or overworld.
fn pick(h: &mut SceneHost, scene: &str) {
    h.world.stage_picker_entry(scene, false);
    if legaia_engine_scene::scene::is_world_map_scene(scene) {
        h.arm_picker_entry_seat(scene);
        h.enter_world_map_scene(scene).expect("enter overworld");
    } else {
        h.arm_picker_story_entry(scene);
        h.enter_field_scene(scene, 0).expect("enter field");
    }
    settle(h, 30);
}

/// From `map01`, walk onto the Rim Elm entrance and settle in the village.
fn walk_into_rim_elm(h: &mut SceneHost, route: &str) {
    seat(h, 0x60, 0x19);
    let entered = walk_until_scene(h, PadButton::Down.mask());
    assert_eq!(
        entered.as_deref(),
        Some("town0c"),
        "[{route}] map01 entrance"
    );
    settle(h, 60);
    assert_tree(h, "town0c", route);
}

#[test]
fn every_picker_route_back_into_rim_elm_finds_the_tree() {
    let Some(_) = open_host() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };

    // The overworld picked straight away, and after a pick of each
    // pre-departure village (whose staging clears the departure flags).
    for before in [None, Some("town01"), Some("town0b"), Some("dolk")] {
        let route = format!("{before:?} then map01");
        let mut h = open_host().unwrap();
        if let Some(s) = before {
            pick(&mut h, s);
        }
        pick(&mut h, "map01");
        walk_into_rim_elm(&mut h, &route);
    }

    // A field pick past the village, walked out to the overworld.
    let mut h = open_host().unwrap();
    pick(&mut h, "cave01");
    seat(&mut h, 93, 99);
    assert_eq!(
        walk_until_scene(&mut h, PadButton::Down.mask()).as_deref(),
        Some("map01"),
        "cave01 exit"
    );
    settle(&mut h, 3);
    walk_into_rim_elm(&mut h, "cave01 then walk");

    // The village itself picked, out through the south gate and back.
    let mut h = open_host().unwrap();
    pick(&mut h, "town0c");
    assert_tree(&h, "town0c", "town0c pick");
    seat(&mut h, 25, 42);
    assert_eq!(
        walk_until_scene(&mut h, PadButton::Up.mask()).as_deref(),
        Some("map01"),
        "town0c south gate"
    );
    settle(&mut h, 3);
    walk_into_rim_elm(&mut h, "town0c gate round trip");

    // The pre-departure villages keep their own trees.
    for scene in ["town01", "town0b"] {
        let mut h = open_host().unwrap();
        pick(&mut h, "dolk");
        pick(&mut h, scene);
        settle(&mut h, 30);
        assert_tree(&h, scene, &format!("dolk then {scene}"));
    }
}

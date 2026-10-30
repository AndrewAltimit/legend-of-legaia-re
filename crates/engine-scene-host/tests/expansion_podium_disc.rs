//! Disc-gated: a party night's podium in the world. Rebirth's Tower Game
//! Night is entered from the 5F floor and played out at once
//! (`World::debug_play_out_party_night`); the night's podium then stands on
//! the floor: three blocks as standable tops, the leader on the winner's
//! block, the rivals' body NPCs on theirs, confetti, the shot. Closing the
//! night takes it down. Skips when `LEGAIA_DISC_BIN` is unset.

use std::sync::Arc;

use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::pad::PadButton;
use legaia_expansion_format::ExpansionPack;

#[test]
fn the_game_night_ends_on_a_podium_in_the_rest() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/rebirth");
    let pack = ExpansionPack::from_dir(&root).expect("Rebirth loads");
    let mut host = SceneHost::open_disc(disc).expect("open the disc");
    host.activate_expansion_pack(Arc::new(pack))
        .expect("Rebirth activates");
    host.enter_expansion_map("rebirth_tower_5f", "")
        .expect("enter 5F");
    let before = {
        let m = &host.world.actors[0].move_state;
        (m.world_x, m.world_y, m.world_z)
    };
    host.world
        .enter_pack_mode("tower-game-night")
        .expect("the night opens");
    assert!(host.world.debug_play_out_party_night());
    let mut staged = None;
    for _ in 0..30 {
        host.world.set_pad(0);
        host.tick().expect("tick");
        if let Some(s) = host.world.pack_mode_stage() {
            staged = Some(s.clone());
        }
    }
    let stage = staged.expect("the podium stands");
    let pod = stage.podium.as_ref().expect("a podium stage");
    assert_eq!(pod.blocks.len(), 3);
    assert_eq!(pod.figures[0].key, 0, "the player won the played-out night");
    // Every rival on the podium stands through a body NPC of the map.
    assert!(
        stage.bodies.iter().all(|b| b.actor.is_some()),
        "{:?}",
        stage.bodies
    );
    // Once risen, the leader stands on the winner's block.
    for _ in 0..90 {
        host.world.set_pad(0);
        host.tick().expect("tick");
    }
    let m = &host.world.actors[0].move_state;
    let block = &host
        .world
        .pack_mode_stage()
        .unwrap()
        .podium
        .as_ref()
        .unwrap()
        .blocks[0];
    assert!((f32::from(m.world_x) - block.centre[0]).abs() < 1.0);
    assert!((f32::from(m.world_z) - block.centre[1]).abs() < 1.0);
    let floor_off = i32::from(m.world_y);
    assert!(
        i32::from(before.1) - floor_off >= 100,
        "lifted onto the block: {} -> {floor_off}",
        before.1
    );
    // Cross closes the finished night; the podium comes down.
    let mut closed = false;
    for f in 0..2000 {
        host.world.set_pad(if f % 2 == 0 {
            PadButton::Cross as u16
        } else {
            0
        });
        host.tick().expect("tick");
        if host.world.local_party().is_none() {
            closed = true;
            break;
        }
    }
    assert!(closed, "the night closed");
    assert_eq!(host.world.mode, SceneMode::Field);
    assert!(host.world.pack_mode_stage().is_none());
    let m = &host.world.actors[0].move_state;
    assert_eq!((m.world_x, m.world_y, m.world_z), before, "put back");
}

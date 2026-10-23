//! The field camera re-seats on the player when a battle returns to the
//! field, on the return tick itself.
//!
//! Retail leaves a battle through MAIN INIT back into field mode 3, whose
//! init `FUN_801D6704` runs the camera reset `FUN_80025C24` unconditionally
//! (`0x801D698C`) and re-centres the window on the saved player tile on the
//! back-from-battle word (`0x801D6E0C..0x801D6ECC`). The port keeps the scene
//! resident and reports no scene entry, so the follow camera kept the focus
//! it pinned onto the party's battle seat through the fight, and a standing
//! player's ease never moved it off.

use super::*;
use legaia_engine_battle::monster_catalog::{FormationDef, FormationSlot};
use legaia_engine_battle_vm::battle_action::BattleEndCause;
use legaia_engine_field::camera::Camera;
use legaia_engine_field::frame_step::camera_after_world_tick;
use legaia_engine_mode::mode::SceneMode;

const FIELD_POS: (i16, i16) = (0x1040, 0x2040);

/// A field world with terrain (a zone table that misses, so the zone camera
/// owns the walk camera) and a settled follow camera on the player.
fn field_world() -> (World, Camera) {
    let mut w = World {
        mode: SceneMode::Field,
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    w.spawn_actor(0);
    w.player_actor_slot = Some(0);
    w.actors[0].battle.hp = 100;
    w.actors[0].move_state.world_x = FIELD_POS.0;
    w.actors[0].move_state.world_z = FIELD_POS.1;
    w.load_field_region_tables(&[], &[0u8]);
    let mut c = Camera::default();
    c.reset_globals_for_scene_entry();
    camera_after_world_tick(&mut c, &mut w, false);
    assert_eq!(player_focus(&c), field_focus(), "settled on the player");
    (w, c)
}

fn field_focus() -> [i32; 2] {
    [i32::from(FIELD_POS.0), i32::from(FIELD_POS.1)]
}

fn player_focus(c: &Camera) -> [i32; 2] {
    let f = c.globals.focus_world();
    [f[0], f[2]]
}

/// Enter a battle, tick the camera through it with the party on its battle
/// seat, and end it with `cause`.
fn fight(w: &mut World, c: &mut Camera, cause: BattleEndCause) {
    w.field_return = Some(FieldReturnState {
        actors: w.actors.clone(),
        player_actor_slot: w.player_actor_slot,
        party_count: w.party.party_count,
    });
    w.battle.return_mode = SceneMode::Field;
    w.enter_battle_from_formation(&FormationDef::new(7, vec![FormationSlot::new(1)]));
    w.actors[0].move_state.world_x = 0;
    w.actors[0].move_state.world_z = -0x300;
    for _ in 0..4 {
        camera_after_world_tick(c, w, false);
    }
    assert_ne!(player_focus(c), field_focus(), "the fight moved the focus");
    w.battle.end = Some(cause);
    w.begin_battle_end_sequence();
    let mut frames = 0;
    while w.battle.victory.is_some() && frames < 2000 {
        w.tick_battle_end_sequence();
        frames += 1;
    }
    assert_eq!(w.mode, SceneMode::Field, "back on the field");
}

#[test]
fn an_escape_reseats_the_camera_on_the_return_tick() {
    let (mut w, mut c) = field_world();
    fight(&mut w, &mut c, BattleEndCause::Escaped);
    camera_after_world_tick(&mut c, &mut w, false);
    assert_eq!(player_focus(&c), field_focus());
    assert!(!w.take_field_camera_reseat(), "the latch is taken once");
}

#[test]
fn a_win_reseats_the_camera_on_the_return_tick() {
    let (mut w, mut c) = field_world();
    fight(&mut w, &mut c, BattleEndCause::MonsterWipe);
    camera_after_world_tick(&mut c, &mut w, false);
    assert_eq!(player_focus(&c), field_focus());
}

/// The latch is the fix: a standing player's ease alone leaves the focus on
/// the battle seat.
#[test]
fn without_the_reseat_the_standing_player_keeps_the_battle_focus() {
    let (mut w, mut c) = field_world();
    fight(&mut w, &mut c, BattleEndCause::Escaped);
    assert!(w.take_field_camera_reseat());
    for _ in 0..4 {
        camera_after_world_tick(&mut c, &mut w, false);
    }
    assert_ne!(player_focus(&c), field_focus());
}

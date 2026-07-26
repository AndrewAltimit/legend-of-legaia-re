//! Output-level checks for two battle-overlay kernels wired onto the live
//! frame path, driven through the same `World` entry points the play window
//! calls - not through the kernels directly.
//!
//! 1. **The attack-target ring** (`FUN_801D8A88` -> `FUN_801D8D00`, with
//!    `FUN_80019B28` supplying the headings). Retail's enemy cursor steps to
//!    the *angularly* nearest live monster, so on the authored four-monster
//!    row the cursor from the left-of-centre seat goes to right-of-centre one
//!    way and to the far left flank the other - neither of which is the next
//!    slot index. Both a plain slot-order walk and the retail ring would agree
//!    on a formation seated in slot order, which is why the assertion is
//!    written against a formation where they differ.
//!
//! 2. **Learn-on-use** (`FUN_801EFBFC`). Performing an art emits exactly one
//!    `TacticalArtLearned`, on the first performance, and nothing on repeats.
//!
//! Disc-free.

use super::*;
use crate::battle_events::BattleEvent;
use crate::input::PadButton;
use crate::target_picker::{CursorRow, PickerState};

/// One party member facing the authored four-monster row
/// (`battle_seats::MONSTER_SEATS[3]`, the pincer-free normal family), seated
/// by `World::enter_battle` exactly as the retail setup `FUN_800513F0` does.
fn seated_battle() -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.enter_battle(1, 4);
    w.battle_player_driven = true;
    w.mode = SceneMode::Battle;
    for i in 0..5 {
        w.actors[i].active = true;
        w.actors[i].battle.max_hp = 400;
        w.actors[i].battle.hp = 400;
        w.actors[i].battle.liveness = 1;
    }
    w.set_battle_attack(0, 40);
    for m in 1..5u8 {
        w.set_battle_defense(m, 10);
    }
    w.load_party(legaia_save::Party::zeroed(1));
    w
}

/// Open the arts menu on slot 0 with one saved chain, then step to the target
/// cursor. Returns the world with the picker live.
fn open_target_cursor(w: &mut World) {
    w.saved_chains.push(legaia_save::SavedChainRecord {
        char_slot: 0,
        name: "Combo".into(),
        sequence: vec![1, 2, 3],
    });
    w.battle_ctx.active_actor = 0;
    w.battle_arts_menu = Some(crate::battle_arts::BattleArtsSession::new(
        0,
        0,
        w.build_battle_arts_rows(0),
    ));
    // Cross opens the single-enemy target picker.
    w.set_pad(0);
    w.set_pad(PadButton::Cross.mask());
    w.tick_battle_arts_menu();
}

fn cursor_slot(w: &World) -> u8 {
    let picker = w
        .battle_arts_menu
        .as_ref()
        .and_then(|m| m.picker())
        .expect("target picker live");
    match picker.state() {
        PickerState::Cursor {
            row: CursorRow::Enemy,
            slot,
        } => slot,
        other => panic!("expected an enemy cursor, got {other:?}"),
    }
}

fn press(w: &mut World, button: PadButton) {
    w.set_pad(0);
    w.set_pad(button.mask());
    w.tick_battle_arts_menu();
}

/// Re-seat the four monsters so **slot order and screen order disagree**:
/// picker slots `0,1,2,3` sit at x `-300, +900, -900, +300`, all at the same
/// depth. A slot-order cursor walks `0,1,2,3`; a bearing-ordered one sweeps
/// the row left to right, `0,3,1,2`.
fn scramble_seats(w: &mut World) {
    for (slot, x) in [(1usize, -300i16), (2, 900), (3, -900), (4, 300)] {
        w.actors[slot].move_state.world_x = x;
        w.actors[slot].move_state.world_z = 800;
    }
}

#[test]
fn enemy_cursor_steps_by_angle_not_by_slot_index() {
    let mut w = seated_battle();
    scramble_seats(&mut w);
    open_target_cursor(&mut w);
    assert_eq!(cursor_slot(&w), 0, "picker opens on the first live row");

    // Right from picker slot 0 (x = -300): the nearest alternate by bearing is
    // slot 3 (x = +300). A slot-order walk would have gone to slot 1.
    press(&mut w, PadButton::Right);
    assert_eq!(cursor_slot(&w), 3, "angular neighbour, not the next index");
    // The sweep continues left-to-right across the row and wraps.
    press(&mut w, PadButton::Right);
    assert_eq!(cursor_slot(&w), 1);
    press(&mut w, PadButton::Right);
    assert_eq!(cursor_slot(&w), 2);
    press(&mut w, PadButton::Right);
    assert_eq!(cursor_slot(&w), 0, "the ring closes");

    // And Left runs the sweep backwards: from slot 0 straight to the far left
    // of the row (slot 2), where a slot-order walk would have gone to slot 3.
    press(&mut w, PadButton::Left);
    assert_eq!(cursor_slot(&w), 2);
    press(&mut w, PadButton::Left);
    assert_eq!(cursor_slot(&w), 1);
}

#[test]
fn a_dead_monster_leaves_the_ring() {
    let mut w = seated_battle();
    // Kill the left-of-centre monster before the cursor opens.
    w.actors[2].battle.hp = 0;
    w.actors[2].battle.liveness = 0;
    open_target_cursor(&mut w);
    let mut seen = vec![cursor_slot(&w)];
    for _ in 0..4 {
        press(&mut w, PadButton::Right);
        seen.push(cursor_slot(&w));
    }
    assert!(
        !seen.contains(&1),
        "the dead monster must never be reachable: {seen:?}"
    );
    assert!(seen.contains(&0) && seen.contains(&2) && seen.contains(&3));
}

#[test]
fn the_cursor_falls_back_to_slot_order_without_seats() {
    // A host that never seated its actors leaves every seat at the origin;
    // the ring has nothing to order and the plain scan takes over.
    let mut w = seated_battle();
    for i in 0..5 {
        w.actors[i].move_state.world_x = 0;
        w.actors[i].move_state.world_z = 0;
    }
    open_target_cursor(&mut w);
    assert_eq!(cursor_slot(&w), 0);
    press(&mut w, PadButton::Right);
    assert_eq!(cursor_slot(&w), 1);
    press(&mut w, PadButton::Right);
    assert_eq!(cursor_slot(&w), 2);
}

/// Stage Vahn's Somersault (`0x27`) so the saved chain `[Up]` resolves to a
/// real art constant. A synthetic row carries none, and the learn check has
/// no id to insert - so without this the test would pass vacuously.
fn stage_somersault(w: &mut World) {
    let action = legaia_art::ActionConstant::from_byte(0x27).unwrap();
    let rec = legaia_art::ArtRecord {
        action,
        commands: vec![legaia_art::Command::Up],
        anim_index: 0,
        anim_extra: vec![],
        name: None,
        power: vec![legaia_art::power::PowerByte::from_byte(0x16); 2],
        dmg_timing: vec![],
        effect_cues: Default::default(),
        hit_cues: vec![],
        identifier: 0,
        anim_speed: 0,
        enemy_effect: legaia_art::EnemyEffect::None,
        repeat_frames: Default::default(),
        background: 0,
        runtime_address: None,
    };
    w.set_art_record(legaia_art::Character::Vahn, action, rec);
}

fn open_somersault_menu(w: &mut World) {
    w.battle_ctx.active_actor = 0;
    let rows = w.build_battle_arts_rows(0);
    w.battle_arts_menu = Some(crate::battle_arts::BattleArtsSession::new(0, 0, rows));
}

#[test]
fn performing_an_art_learns_it_once() {
    let mut w = seated_battle();
    stage_somersault(&mut w);
    w.saved_chains.push(legaia_save::SavedChainRecord {
        char_slot: 0,
        name: "Som".into(),
        sequence: vec![4], // Up
    });
    let art_id = w
        .build_battle_arts_rows(0)
        .first()
        .and_then(|r| r.action)
        .expect("the staged record must resolve a real art constant")
        .as_byte();
    assert_eq!(art_id, 0x27);
    open_somersault_menu(&mut w);

    press(&mut w, PadButton::Cross); // open the target cursor
    press(&mut w, PadButton::Cross); // confirm - the art runs
    assert!(w.battle_arts_menu.is_none(), "arts menu closed");

    let learned: Vec<_> = w
        .drain_battle_events()
        .into_iter()
        .filter_map(|e| match e {
            BattleEvent::TacticalArtLearned { char_id, art_id } => Some((char_id, art_id)),
            _ => None,
        })
        .collect();
    assert_eq!(
        learned,
        vec![(0u8, art_id)],
        "exactly one learn event, for the art actually performed"
    );
    assert!(w.tactical_arts.is_learned(0, art_id));
    assert_eq!(w.tactical_arts.learned_ids(0), vec![art_id]);
    assert!(w.current_art_banner.is_some(), "HUD banner armed");

    // Perform it again: retail's membership scan hits, so nothing fires.
    w.current_art_banner = None;
    open_somersault_menu(&mut w);
    press(&mut w, PadButton::Cross);
    press(&mut w, PadButton::Cross);
    let again = w
        .drain_battle_events()
        .into_iter()
        .filter(|e| matches!(e, BattleEvent::TacticalArtLearned { .. }))
        .count();
    assert_eq!(again, 0, "a known art re-fires nothing");
    assert!(w.current_art_banner.is_none());
}

#[test]
fn an_art_inside_the_innate_band_is_never_learned() {
    // The retail insert gate is `art_id > innate_cap`. With the cap above
    // Somersault's id, performing it changes nothing - which is what keeps a
    // character's starting arts out of the learn banner.
    let mut w = seated_battle();
    stage_somersault(&mut w);
    w.tactical_arts.set_innate_cap(0, 0x30);
    w.saved_chains.push(legaia_save::SavedChainRecord {
        char_slot: 0,
        name: "Som".into(),
        sequence: vec![4],
    });
    open_somersault_menu(&mut w);
    press(&mut w, PadButton::Cross);
    press(&mut w, PadButton::Cross);
    assert!(
        !w.drain_battle_events()
            .iter()
            .any(|e| matches!(e, BattleEvent::TacticalArtLearned { .. }))
    );
    assert!(w.tactical_arts.learned_ids(0).is_empty());
}

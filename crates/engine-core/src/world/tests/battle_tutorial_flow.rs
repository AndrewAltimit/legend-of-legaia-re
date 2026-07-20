//! The sparring tutorial firing inside a live player-driven battle: the
//! `CommandPhase -> ctx[+0x06]` bridge, the box queue, the wrong-lesson rewind,
//! and the lesson walk.
//!
//! Uses a **synthetic** prompt corpus - the real strings are Sony bytes read off
//! the user's disc at runtime. `battle_tutorial_disc.rs` is the disc-gated half
//! that checks the real text resolves.

use super::*;

use crate::battle_flow::BattleFlowState;
use crate::battle_input::{BattleCommandSession, CommandPhase};
use crate::battle_tutorial::{BattleTutorialScript, TutorialLesson, msg};

/// A stand-in overlay blob: every message VA the machine can emit gets a short
/// ASCII marker naming its own address, so a queued box is traceable back to the
/// hook that produced it without shipping any retail text.
fn synthetic_script() -> BattleTutorialScript {
    let base = crate::battle_tutorial::OVERLAY_967_BASE_VA;
    let mut ids: Vec<u32> = BattleTutorialScript::MESSAGE_IDS.to_vec();
    ids.push(msg::ENTER_HIGH_LOW_HIGH);
    ids.push(msg::WRONG_COMMANDS);
    ids.push(msg::PRACTICE_OVER);
    let span = ids.iter().map(|v| v - base).max().unwrap() as usize + 16;
    let mut bytes = vec![0u8; span];
    for va in ids {
        let off = (va - base) as usize;
        let marker = format!("m{va:08X}");
        bytes[off..off + marker.len()].copy_from_slice(marker.as_bytes());
    }
    BattleTutorialScript::from_overlay(&bytes, base)
}

fn marker(va: u32) -> String {
    format!("m{va:08X}")
}

fn tutorial_battle_world() -> World {
    let mut world = World::new();
    world.live_gameplay_loop = true;
    world.battle_player_driven = true;
    world.prime_battle_tutorial(synthetic_script());
    world.enter_battle(3, 2);
    for i in 0..5 {
        world.actors[i].battle.hp = 100;
        world.actors[i].battle.max_hp = 100;
    }
    world
}

/// Texts of the boxes currently queued, front first.
fn queued(world: &World) -> Vec<String> {
    world
        .battle_tutorial_boxes
        .iter()
        .map(|b| b.text.clone())
        .collect()
}

#[test]
fn priming_arms_the_machine_at_battle_entry() {
    let world = tutorial_battle_world();
    assert!(world.battle_tutorial.is_some(), "tutorial armed");
    assert_eq!(
        world.battle_tutorial_lesson(),
        Some(TutorialLesson::Attacks)
    );
    assert_eq!(world.battle_flow, BattleFlowState::Idle);
    // A battle entered without priming stays clean.
    let mut plain = World::new();
    plain.enter_battle(3, 2);
    assert!(plain.battle_tutorial.is_none());
}

#[test]
fn opening_a_turn_raises_the_turn_prompt_and_queues_the_lesson_intro() {
    let mut world = tutorial_battle_world();
    world.open_battle_command(0);
    assert_eq!(world.battle_flow, BattleFlowState::TurnPrompt);
    // Retail state 30 / lesson 0: the intro plus the first-visit directional
    // explainer.
    assert_eq!(
        queued(&world),
        vec![marker(msg::LESSON0_INTRO), marker(msg::HOWTO_DIRECTIONAL)]
    );
    assert!(world.battle_tutorial_box_up());
}

#[test]
fn the_command_menu_raises_the_category_prompt_after_the_intro_clears() {
    let mut world = tutorial_battle_world();
    world.open_battle_command(0);
    // Drain the two intro boxes (the second waits for input).
    world.battle_tutorial_boxes.clear();
    world.tick_battle_command();
    assert_eq!(world.battle_flow, BattleFlowState::CategoryMenu);
    // Lesson 0 names [Attack] as the category to pick.
    assert_eq!(queued(&world), vec![marker(msg::PICK_ATTACK)]);
}

#[test]
fn a_hook_fires_once_per_entry_into_its_flow_state() {
    let mut world = tutorial_battle_world();
    world.open_battle_command(0);
    world.battle_tutorial_boxes.clear();
    world.tick_battle_command();
    assert_eq!(queued(&world).len(), 1);
    world.battle_tutorial_boxes.clear();
    // Still in the category menu: the one-shot latch swallows the re-dispatch.
    world.tick_battle_command();
    assert!(queued(&world).is_empty(), "latched - no second emission");
}

#[test]
fn picking_the_item_window_during_the_attack_lesson_rewinds() {
    let mut world = tutorial_battle_world();
    world.open_battle_command(0);
    world.battle_tutorial_boxes.clear();
    world.battle_command = Some(BattleCommandSession {
        actor: 0,
        party_slot: 0,
        phase: CommandPhase::OpenItemMenu,
    });
    world.tick_battle_command();
    // The item submenu never opens; the rewind box names the taught lesson and
    // the command menu is back up.
    assert!(world.battle_item_menu.is_none(), "item window rejected");
    assert_eq!(queued(&world), vec![marker(msg::WRONG_ATTACKS)]);
    assert!(world.battle_command.is_some(), "command menu reopened");
}

#[test]
fn the_item_window_is_allowed_once_the_item_lesson_is_running() {
    let mut world = tutorial_battle_world();
    world.battle_tutorial.as_mut().unwrap().lesson = TutorialLesson::Items.raw();
    world.open_battle_command(0);
    world.battle_tutorial_boxes.clear();
    world.battle_command = Some(BattleCommandSession {
        actor: 0,
        party_slot: 0,
        phase: CommandPhase::OpenItemMenu,
    });
    world.tick_battle_command();
    assert!(world.battle_item_menu.is_some(), "item window opens");
    assert_eq!(
        queued(&world),
        vec![marker(msg::SELECT_ITEM), marker(msg::ITEM_WINDOW_EXPLAIN)]
    );
}

#[test]
fn run_is_rejected_for_the_whole_sparring_fight() {
    let mut world = tutorial_battle_world();
    for lesson in [
        TutorialLesson::Attacks,
        TutorialLesson::Items,
        TutorialLesson::Spirit,
        TutorialLesson::HyperArts,
    ] {
        let mut world = std::mem::replace(&mut world, tutorial_battle_world());
        world.battle_tutorial.as_mut().unwrap().lesson = lesson.raw();
        world.open_battle_command(0);
        world.battle_tutorial_boxes.clear();
        world.battle_command = Some(BattleCommandSession {
            actor: 0,
            party_slot: 0,
            phase: CommandPhase::RunAway,
        });
        world.tick_battle_command();
        assert_eq!(
            queued(&world),
            vec![marker(msg::NO_RUNNING)],
            "lesson {lesson:?} should refuse to flee"
        );
        assert!(world.battle_command.is_some(), "back at the command menu");
    }
}

#[test]
fn committing_the_taught_category_is_accepted_and_advances_the_lesson() {
    let mut world = tutorial_battle_world();
    world.open_battle_command(0);
    world.battle_tutorial_boxes.clear();
    // Attack confirmed on a monster - retail category 3, which lesson 0 teaches.
    world.battle_command = Some(BattleCommandSession {
        actor: 0,
        party_slot: 0,
        phase: CommandPhase::Confirmed {
            command: crate::battle_input::BattleCommand::Attack,
            target_row: crate::target_picker::CursorRow::Enemy,
            target_slot: 0,
        },
    });
    world.tick_battle_command();
    assert_eq!(queued(&world), vec![marker(msg::NOW_BEGIN)]);
    assert!(world.battle_command.is_none(), "the strike commits");
    assert!(world.battle_tutorial.as_ref().unwrap().pending_advance);

    // The bump lands at the next turn start, so lesson 1's intro is what the
    // following turn opens with.
    world.battle_tutorial_boxes.clear();
    world.battle_flow = BattleFlowState::Idle;
    world.open_battle_command(0);
    assert_eq!(world.battle_tutorial_lesson(), Some(TutorialLesson::Items));
    assert_eq!(queued(&world), vec![marker(msg::LESSON1_INTRO)]);
}

#[test]
fn a_box_on_screen_parks_the_battle_loop() {
    let mut world = tutorial_battle_world();
    world.open_battle_command(0);
    assert!(world.battle_tutorial_box_up());
    let before = world.battle_ctx.action_state;
    // The live tick must not advance the action SM while a box waits.
    world.live_battle_tick();
    assert_eq!(world.battle_ctx.action_state, before);
    assert!(world.battle_tutorial_box_up(), "box still waiting");
}

#[test]
fn a_waiting_box_dismisses_on_cross_and_a_plain_one_times_out() {
    use crate::input::PadButton;
    let mut world = tutorial_battle_world();
    world.open_battle_command(0);
    // Box 0 is style 0 (no wait); box 1 is style 3 (waits).
    assert!(!world.battle_tutorial_boxes[0].waits_for_input);
    assert!(world.battle_tutorial_boxes[1].waits_for_input);

    // The plain box ages out on its own.
    let frames = world.battle_tutorial_boxes[0].frames_remaining;
    for _ in 0..frames {
        world.tick_battle_tutorial_boxes();
    }
    assert_eq!(world.battle_tutorial_boxes.len(), 1, "plain box expired");

    // The waiting box sits there until Cross.
    for _ in 0..600 {
        world.tick_battle_tutorial_boxes();
    }
    assert_eq!(world.battle_tutorial_boxes.len(), 1, "still waiting");
    world.input.set_pad(PadButton::Cross.mask());
    world.tick_battle_tutorial_boxes();
    assert!(world.battle_tutorial_boxes.is_empty(), "acknowledged");
}

#[test]
fn every_queued_box_carries_a_decodable_retail_placement() {
    let mut world = tutorial_battle_world();
    world.open_battle_command(0);
    for b in &world.battle_tutorial_boxes {
        let pos = b.position(96).expect("style inside the retail 0..=9 table");
        assert!(pos.0 >= 0 && pos.1 >= 0, "box {b:?} placed off-screen");
    }
}

#[test]
fn the_fourth_completed_lesson_closes_the_fight_out() {
    let mut world = tutorial_battle_world();
    // Lesson 3 done: the counter reaches 4 and the completion tail runs.
    world.battle_tutorial.as_mut().unwrap().lesson = TutorialLesson::HyperArts.raw();
    world.battle_tutorial.as_mut().unwrap().pending_advance = true;
    world.open_battle_command(0);
    assert!(
        queued(&world).contains(&marker(msg::PRACTICE_OVER)),
        "sign-off box shown, got {:?}",
        queued(&world)
    );
    assert!(
        world.battle_tutorial.is_none(),
        "the machine disarms once the fight is closed"
    );
}

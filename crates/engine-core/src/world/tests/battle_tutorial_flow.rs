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
    world.toggles.live_gameplay_loop = true;
    world.battle.player_driven = true;
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
        .battle
        .tutorial_boxes
        .iter()
        .map(|b| b.text.clone())
        .collect()
}

#[test]
fn priming_arms_the_machine_at_battle_entry() {
    let world = tutorial_battle_world();
    assert!(world.battle.tutorial.is_some(), "tutorial armed");
    assert_eq!(
        world.battle_tutorial_lesson(),
        Some(TutorialLesson::Attacks)
    );
    assert_eq!(world.battle.flow, BattleFlowState::Idle);
    // A battle entered without priming stays clean.
    let mut plain = World::new();
    plain.enter_battle(3, 2);
    assert!(plain.battle.tutorial.is_none());
}

/// Retail's own condition - the one-shot system-flag arm the entity SM's
/// battle-entry tail tests and clears (`FUN_801DA51C`,
/// `0x801DA698..0x801DA6B0`) - drives the machine with no host priming at all.
///
/// This is the shared model both hosts sit on: the native window and the
/// browser play page each reach it through `World::enter_battle` and neither
/// needs to know the tutorial exists.
#[test]
fn the_disc_arm_flag_runs_the_tutorial_for_exactly_one_battle() {
    use crate::battle_tutorial::TUTORIAL_ARM_FLAG;

    let mut world = World::new();
    world.set_battle_tutorial_script(synthetic_script());

    // Baseline: the corpus alone arms nothing. Having the text is not the
    // condition, which is the confusion the old host gate encoded.
    world.enter_battle(3, 2);
    assert!(
        world.battle.tutorial.is_none(),
        "an unarmed battle must not run the tutorial"
    );

    // The field VM raising the flag is the whole trigger (town01's Tetsu
    // record does it with `50 19`, two ops before its battle-entry op).
    world.system_flag_set(TUTORIAL_ARM_FLAG);
    world.enter_battle(3, 2);
    assert!(
        world.battle.tutorial.is_some(),
        "the disc arm must run the tutorial in the very next battle"
    );
    assert_eq!(
        world.battle_tutorial_lesson(),
        Some(TutorialLesson::Attacks)
    );
    assert!(
        !world.system_flag_test(TUTORIAL_ARM_FLAG),
        "battle entry must CONSUME the arm (retail clears it in the same breath)"
    );

    // ...and only that battle. A second fight is an ordinary one.
    world.enter_battle(3, 2);
    assert!(
        world.battle.tutorial.is_none(),
        "the arm is one-shot; the fight after the spar is ordinary"
    );
}

/// A forced (debug) tutorial still consumes a raised arm, so it cannot leave
/// the flag behind to fire a second time on the next fight.
#[test]
fn a_forced_tutorial_consumes_the_disc_arm_too() {
    use crate::battle_tutorial::TUTORIAL_ARM_FLAG;

    let mut world = World::new();
    world.prime_battle_tutorial(synthetic_script());
    world.system_flag_set(TUTORIAL_ARM_FLAG);
    world.enter_battle(3, 2);
    assert!(world.battle.tutorial.is_some());
    assert!(!world.system_flag_test(TUTORIAL_ARM_FLAG), "arm consumed");
    world.enter_battle(3, 2);
    assert!(
        world.battle.tutorial.is_none(),
        "neither the force nor the arm may survive into the next battle"
    );
}

/// The stage-id resolver is the retail arithmetic, not a boolean rename: the
/// unarmed value is the "no stage overlay" id `0`, and the armed one is the
/// id `overlay_loader::battle_stage_overlay_entry` maps to PROT 0967.
#[test]
fn the_stage_id_resolver_matches_the_overlay_dispatch() {
    use crate::battle_tutorial::{TUTORIAL_STAGE_ID, stage_id_at_battle_entry};

    assert_eq!(stage_id_at_battle_entry(false), 0);
    assert_eq!(stage_id_at_battle_entry(true), TUTORIAL_STAGE_ID);
    assert_eq!(
        crate::overlay_loader::battle_stage_overlay_entry(stage_id_at_battle_entry(false)),
        None,
        "stage id 0 pages no overlay"
    );
    assert_eq!(
        crate::overlay_loader::battle_stage_overlay_entry(stage_id_at_battle_entry(true)),
        Some(crate::battle_tutorial::OVERLAY_967_PROT_INDEX),
        "the armed stage id must resolve to the prompt overlay's PROT entry"
    );
}

#[test]
fn opening_a_turn_raises_the_turn_prompt_and_queues_the_lesson_intro() {
    let mut world = tutorial_battle_world();
    world.open_battle_command(0);
    assert_eq!(world.battle.flow, BattleFlowState::TurnPrompt);
    // Retail state 30 / lesson 0: the intro plus the first-visit directional
    // explainer.
    assert_eq!(
        queued(&world),
        vec![marker(msg::LESSON0_INTRO), marker(msg::HOWTO_DIRECTIONAL)]
    );
    assert!(world.battle_tutorial_box_up());
}

/// Take `Begin` on the round prompt - retail's `0x1E -> 0x28` (the confirm arm
/// at `0x801D108C`). A turn opens on the prompt, not on the ring, so the ring's
/// hooks are one press away rather than one tick away.
fn take_begin(world: &mut World) {
    world.set_pad(0);
    world.set_pad(crate::input::PadButton::Cross.mask());
    world.tick_battle_command();
    world.set_pad(0);
}

#[test]
fn the_command_menu_raises_the_category_prompt_after_begin_is_taken() {
    let mut world = tutorial_battle_world();
    world.open_battle_command(0);
    // Drain the two intro boxes (the second waits for input).
    world.battle.tutorial_boxes.clear();
    // The turn opens on `Begin | Run`; the ring is behind it.
    assert_eq!(world.battle.flow, BattleFlowState::TurnPrompt);
    take_begin(&mut world);
    assert_eq!(world.battle.flow, BattleFlowState::CategoryMenu);
    // Lesson 0 names [Attack] as the category to pick.
    assert_eq!(queued(&world), vec![marker(msg::PICK_ATTACK)]);
}

#[test]
fn a_hook_fires_once_per_entry_into_its_flow_state() {
    let mut world = tutorial_battle_world();
    world.open_battle_command(0);
    world.battle.tutorial_boxes.clear();
    take_begin(&mut world);
    assert_eq!(queued(&world).len(), 1);
    world.battle.tutorial_boxes.clear();
    // Still in the category menu: the one-shot latch swallows the re-dispatch.
    world.tick_battle_command();
    assert!(queued(&world).is_empty(), "latched - no second emission");
}

#[test]
fn picking_the_item_window_during_the_attack_lesson_rewinds() {
    let mut world = tutorial_battle_world();
    world.open_battle_command(0);
    world.battle.tutorial_boxes.clear();
    world.battle.command = Some(BattleCommandSession {
        actor: 0,
        party_slot: 0,
        no_escape: false,
        phase: CommandPhase::OpenItemMenu,
    });
    world.tick_battle_command();
    // The item submenu never opens; the rewind box names the taught lesson and
    // the command menu is back up.
    assert!(world.battle.item_menu.is_none(), "item window rejected");
    assert_eq!(queued(&world), vec![marker(msg::WRONG_ATTACKS)]);
    assert!(world.battle.command.is_some(), "command menu reopened");
}

#[test]
fn the_item_window_is_allowed_once_the_item_lesson_is_running() {
    let mut world = tutorial_battle_world();
    world.battle.tutorial.as_mut().unwrap().lesson = TutorialLesson::Items.raw();
    world.open_battle_command(0);
    world.battle.tutorial_boxes.clear();
    world.battle.command = Some(BattleCommandSession {
        actor: 0,
        party_slot: 0,
        no_escape: false,
        phase: CommandPhase::OpenItemMenu,
    });
    world.tick_battle_command();
    assert!(world.battle.item_menu.is_some(), "item window opens");
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
        world.battle.tutorial.as_mut().unwrap().lesson = lesson.raw();
        world.open_battle_command(0);
        world.battle.tutorial_boxes.clear();
        world.battle.command = Some(BattleCommandSession {
            actor: 0,
            party_slot: 0,
            no_escape: false,
            phase: CommandPhase::RunAway,
        });
        world.tick_battle_command();
        assert_eq!(
            queued(&world),
            vec![marker(msg::NO_RUNNING)],
            "lesson {lesson:?} should refuse to flee"
        );
        assert!(world.battle.command.is_some(), "back at the command menu");
    }
}

#[test]
fn committing_the_taught_category_is_accepted_and_advances_the_lesson() {
    let mut world = tutorial_battle_world();
    world.open_battle_command(0);
    world.battle.tutorial_boxes.clear();
    // Attack confirmed on a monster - retail category 3, which lesson 0 teaches.
    world.battle.command = Some(BattleCommandSession {
        actor: 0,
        party_slot: 0,
        no_escape: false,
        phase: CommandPhase::Confirmed {
            command: crate::battle_input::BattleCommand::Attack,
            target_row: crate::target_picker::CursorRow::Enemy,
            target_slot: 0,
        },
    });
    world.tick_battle_command();
    assert_eq!(queued(&world), vec![marker(msg::NOW_BEGIN)]);
    // The strike commits and the ring walks on to the next member that owes
    // a command (three are seated here), so slot 0's session is gone.
    assert!(
        world.battle.command.as_ref().is_none_or(|s| s.actor != 0),
        "the strike commits"
    );
    assert!(world.battle.tutorial.as_ref().unwrap().pending_advance);

    // The bump lands at the next turn start, so lesson 1's intro is what the
    // following turn opens with.
    world.battle.tutorial_boxes.clear();
    world.battle.flow = BattleFlowState::Idle;
    world.open_battle_command(0);
    assert_eq!(world.battle_tutorial_lesson(), Some(TutorialLesson::Items));
    assert_eq!(queued(&world), vec![marker(msg::LESSON1_INTRO)]);
}

/// Press `button` for one arts-entry frame, then release.
fn arts_press(world: &mut World, button: crate::input::PadButton) {
    world.set_pad(0);
    world.set_pad(button.mask());
    world.tick_battle_arts_input();
    world.set_pad(0);
}

/// The hyper-arts lesson's arts entry, opened as the `Command` chip opens it.
fn drill_world() -> World {
    let mut world = tutorial_battle_world();
    world.battle.tutorial.as_mut().unwrap().lesson = TutorialLesson::HyperArts.raw();
    world.open_battle_command(0);
    world.battle.command = Some(BattleCommandSession {
        actor: 0,
        party_slot: 0,
        no_escape: false,
        phase: CommandPhase::OpenArtsMenu,
    });
    world.tick_battle_command();
    assert!(world.battle.arts_input.is_some(), "the arts entry opens");
    assert_eq!(world.battle.flow, BattleFlowState::ArtsCommandEntry);
    assert!(queued(&world).contains(&marker(msg::ENTER_HIGH_LOW_HIGH)));
    world.battle.tutorial_boxes.clear();
    world
}

/// The user-reported spar defect: the Somersault (`Up Down Up`, the
/// `[High] [Low] [High]` drill) entered through the arts entry never met the
/// `90` drill check or the `110` validator, so the last lesson was never
/// accepted and the fight ran on.
#[test]
fn the_somersault_entry_passes_the_drill_and_commits_the_last_lesson() {
    use crate::input::PadButton;
    let mut world = drill_world();
    for b in [PadButton::Up, PadButton::Down, PadButton::Up] {
        arts_press(&mut world, b);
    }
    // Confirm ends the entry: `0x50 -> 0x5A`, the drill check.
    arts_press(&mut world, PadButton::Cross);
    assert_eq!(world.battle.flow, BattleFlowState::TargetSelect);
    assert_eq!(
        queued(&world),
        vec![marker(msg::SELECT_TARGET), marker(msg::TARGET_EXPLAIN)],
        "the drill is accepted"
    );
    assert_eq!(
        world
            .battle
            .tutorial
            .as_ref()
            .unwrap()
            .inputs
            .command_buffer[..3],
        [0x0F, 0x0E, 0x0F],
        "the hook sees the gauge's swing bytes"
    );
    world.battle.tutorial_boxes.clear();
    // Any press on the review picks the lone target and commits.
    arts_press(&mut world, PadButton::Cross);
    assert!(world.battle.arts_input.is_none(), "the art commits");
    assert_eq!(queued(&world), vec![marker(msg::NOW_BEGIN)]);
    assert!(world.battle.tutorial.as_ref().unwrap().pending_advance);
}

#[test]
fn a_wrong_drill_string_is_refused_back_to_the_command_menu() {
    use crate::input::PadButton;
    let mut world = drill_world();
    for b in [PadButton::Down, PadButton::Down, PadButton::Down] {
        arts_press(&mut world, b);
    }
    arts_press(&mut world, PadButton::Cross);
    assert_eq!(queued(&world), vec![marker(msg::WRONG_COMMANDS)]);
    assert!(world.battle.arts_input.is_none(), "the entry is discarded");
    assert!(world.battle.command.is_some(), "command menu reopened");
    assert!(!world.battle.tutorial.as_ref().unwrap().pending_advance);
}

#[test]
fn the_drill_matches_one_leading_arrow() {
    use crate::input::PadButton;
    let mut world = drill_world();
    // Four arrows at the favoured cost need more than the default pool.
    let entry = world.battle.arts_input.as_mut().unwrap();
    entry.pool = 200;
    entry.pool_max = 200;
    for b in [
        PadButton::Left,
        PadButton::Up,
        PadButton::Down,
        PadButton::Up,
    ] {
        arts_press(&mut world, b);
    }
    arts_press(&mut world, PadButton::Cross);
    assert_eq!(
        queued(&world),
        vec![marker(msg::SELECT_TARGET), marker(msg::TARGET_EXPLAIN)]
    );
}

/// `ctx[+0x266]` is seat 0's Auto flag: an Auto attack in the drill lesson
/// is the wrong-lesson rewind at the target cursor (`0x801F6F98`).
#[test]
fn an_auto_attack_in_the_drill_lesson_rewinds() {
    use crate::battle_input::BattleCommand;
    let mut world = tutorial_battle_world();
    world.battle.tutorial.as_mut().unwrap().lesson = TutorialLesson::HyperArts.raw();
    world.open_battle_command(0);
    world.battle.tutorial_boxes.clear();
    world.battle.command = Some(BattleCommandSession {
        actor: 0,
        party_slot: 0,
        no_escape: false,
        phase: CommandPhase::AttackMode { cursor: 0 },
    });
    world.set_pad(0);
    world.set_pad(crate::input::PadButton::Left.mask());
    world.tick_battle_command();
    world.set_pad(0);
    assert_eq!(queued(&world), vec![marker(msg::WRONG_HYPER_ARTS)]);
    assert!(
        !matches!(
            world.battle.command.as_ref().map(|c| &c.phase),
            Some(CommandPhase::Targeting {
                command: BattleCommand::Attack,
                ..
            })
        ),
        "the target cursor is backed out of"
    );
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
    assert!(!world.battle.tutorial_boxes[0].waits_for_input);
    assert!(world.battle.tutorial_boxes[1].waits_for_input);

    // The plain box ages out on its own.
    let frames = world.battle.tutorial_boxes[0].frames_remaining;
    for _ in 0..frames {
        world.tick_battle_tutorial_boxes();
    }
    assert_eq!(world.battle.tutorial_boxes.len(), 1, "plain box expired");

    // The waiting box sits there until Cross.
    for _ in 0..600 {
        world.tick_battle_tutorial_boxes();
    }
    assert_eq!(world.battle.tutorial_boxes.len(), 1, "still waiting");
    world.input.set_pad(PadButton::Cross.mask());
    world.tick_battle_tutorial_boxes();
    assert!(world.battle.tutorial_boxes.is_empty(), "acknowledged");
}

#[test]
fn every_queued_box_carries_a_decodable_retail_placement() {
    let mut world = tutorial_battle_world();
    world.open_battle_command(0);
    for b in &world.battle.tutorial_boxes {
        let pos = b.position(96).expect("style inside the retail 0..=9 table");
        assert!(pos.0 >= 0 && pos.1 >= 0, "box {b:?} placed off-screen");
    }
}

#[test]
fn the_fourth_completed_lesson_closes_the_fight_out() {
    let mut world = tutorial_battle_world();
    // Lesson 3 done: the counter reaches 4 and the completion tail runs.
    world.battle.tutorial.as_mut().unwrap().lesson = TutorialLesson::HyperArts.raw();
    world.battle.tutorial.as_mut().unwrap().pending_advance = true;
    world.open_battle_command(0);
    assert!(
        queued(&world).contains(&marker(msg::PRACTICE_OVER)),
        "sign-off box shown, got {:?}",
        queued(&world)
    );
    // The machine stays armed after the tail: its `ctx[+0x6B4]` countdown
    // (armed by `FUN_801F7628` at `0x801F7460`) is what ends the fight. This
    // test used to assert `tutorial.is_none()` - the disarm that left the
    // sparring fight with nothing to end it.
    let tut = world.battle.tutorial.as_ref().expect("still armed");
    assert!(tut.finished);
    assert_eq!(
        tut.countdown,
        crate::battle_tutorial::COMPLETION_COUNTDOWN_VSYNCS
    );
}

/// Close a sparring fight out (lesson 3 done, completion tail run) with the
/// side-band past the caption, as a live round start leaves it.
fn closed_sparring_world() -> World {
    let mut world = tutorial_battle_world();
    world.battle.tutorial.as_mut().unwrap().lesson = TutorialLesson::HyperArts.raw();
    world.battle.tutorial.as_mut().unwrap().pending_advance = true;
    // The caption hold drained at the round start (`FUN_80056208` phase
    // 1 -> 2), so the per-frame hook call is live.
    world.battle.sideband.phase = 2;
    world.open_battle_command(0);
    assert!(world.battle.tutorial.as_ref().unwrap().finished);
    world
}

#[test]
fn the_completion_countdown_takes_the_sparring_fight_back_to_the_field() {
    let mut world = closed_sparring_world();
    world.system_flag_clear(1);
    assert_eq!(world.mode, SceneMode::Battle);
    let mut frames = 0u32;
    let mut vsyncs = 0u32;
    let mut saw_phase3 = false;
    while world.mode == SceneMode::Battle && frames < 2000 {
        world.set_pad(0);
        let _ = world.tick();
        // Both counters drain by the frame step `DAT_1F800393` per battle
        // pass, and a pass spans that many vsyncs: the world ticks once a
        // vsync, so each tick is one.
        vsyncs += 1;
        saw_phase3 |= world.battle.sideband.phase == 3;
        frames += 1;
    }
    println!(
        "sparring fight left battle after {frames} frames / {vsyncs} vsyncs \
         (countdown 360 + exit gate 0x43)"
    );
    assert!(
        saw_phase3,
        "the countdown's expiry raises side-band phase 3"
    );
    assert_eq!(
        world.mode,
        SceneMode::Field,
        "the sparring fight never exited"
    );
    // 360 vsyncs of countdown, then `ctx[+0x6CE]` counts to 0x43.
    assert!(
        (360 + 0x43..=360 + 0x43 + 8).contains(&vsyncs),
        "exit after {vsyncs} vsyncs"
    );
    assert_eq!(
        world.battle.stage_id, 0,
        "the stage id dies with the battle"
    );
    assert!(world.battle.tutorial.is_none());
    assert!(
        world.system_flag_test(1),
        "the 967 exit arm raises the survived bit - story flag 1 on return"
    );
    assert!(!world.game_over);
}

#[test]
fn a_press_skips_the_completion_countdown_once_the_sign_off_box_is_gone() {
    let mut world = closed_sparring_world();
    // A press with the sign-off box up only dismisses the box
    // (`ctx[+0x6B2] != 0` guards the zeroing at `0x801F7224`) ...
    world.set_pad(0);
    let _ = world.tick();
    world.set_pad(crate::input::PadButton::Cross.mask());
    let _ = world.tick();
    world.set_pad(0);
    let _ = world.tick();
    assert!(
        world.battle.tutorial_boxes.is_empty(),
        "Cross dismisses the box"
    );
    assert_eq!(
        world.battle.sideband.phase, 2,
        "... and leaves the countdown running"
    );
    // ... and the next press ends the wait.
    world.set_pad(crate::input::PadButton::Circle.mask());
    let _ = world.tick();
    world.set_pad(0);
    assert_eq!(
        world.battle.sideband.phase, 3,
        "a press with no box up skips the wait"
    );
    let mut frames = 0u32;
    while world.mode == SceneMode::Battle && frames < 200 {
        let _ = world.tick();
        frames += 1;
    }
    assert_eq!(world.mode, SceneMode::Field);
    assert!(
        frames <= 0x43 + 4,
        "phase 3 exits at ctx[+0x6CE] = 0x43, took {frames} frames"
    );
}

#[test]
fn a_closed_sparring_fight_runs_no_more_battle() {
    let mut world = closed_sparring_world();
    world.battle.tutorial_boxes.clear();
    let action_state = world.battle_ctx.action_state;
    let flow = world.battle.flow;
    for _ in 0..100 {
        world.set_pad(0);
        let _ = world.tick();
    }
    // Flow byte 0xC8 is no `FUN_801D0748` case and the hook holds it.
    assert_eq!(world.battle_ctx.action_state, action_state);
    assert_eq!(world.battle.flow, flow);
    assert_eq!(world.mode, SceneMode::Battle, "still counting down");
}

/// A direct entry into a scripted carrier's row (`--battle <row>`) replays
/// the arm the row's own record raises - and only for the row the record's
/// `3E FF <row>` names.
#[test]
fn replay_scripted_battle_arm_keys_on_the_records_entry_row() {
    use crate::battle_tutorial::TUTORIAL_ARM_FLAG;
    use legaia_asset::man_section::{ManFile, ManHeader};

    // One controller record + one placement whose script is the retail
    // sparring shape: dialogue, wait, `50 19 · 50 00 · 52 3C · 3E FF 04`.
    let data_region_offset = 0x40usize;
    let rec0: &[u8] = &[0x00, 0, 0, 0, 0, 0x21];
    let mut rec1 = vec![0x00, 0x05, 0x00, 0x03, 0x04, 0x1F];
    rec1.extend_from_slice(b"Come at me!");
    rec1.extend_from_slice(&[
        0x00, 0x4A, 0x10, 0x00, 0x50, 0x19, 0x50, 0x00, 0x52, 0x3C, 0x3E, 0xFF, 0x04, 0x21,
    ]);
    let mut man = vec![0u8; data_region_offset];
    man.extend_from_slice(rec0);
    let off1 = rec0.len() as u32;
    man.extend_from_slice(&rec1);
    let man_file = ManFile {
        header: ManHeader {
            status_flags: 0,
            low_flag: false,
            depth_lut: [0; 16],
            partition_counts: [0, 2, 0],
            u24_at_28: 0,
        },
        partitions: [vec![], vec![0, off1], vec![]],
        data_region_offset,
        sections: std::array::from_fn(|_| legaia_asset::man_section::SectionRef {
            offset: man.len(),
            length: 0,
        }),
    };

    let mut world = World::new();
    world.install_field_carriers_from_man(&man_file, &man);
    assert!(
        !world.system_flag_test(TUTORIAL_ARM_FLAG),
        "nothing armed before the replay"
    );
    assert!(
        !world.replay_scripted_battle_arm(3),
        "row 3 is not the record's entry row"
    );
    assert!(!world.system_flag_test(TUTORIAL_ARM_FLAG));
    assert!(
        world.replay_scripted_battle_arm(4),
        "row 4 is the record's entry row"
    );
    assert!(
        world.system_flag_test(TUTORIAL_ARM_FLAG),
        "the replay raises the one-shot arm the entry consumes"
    );
}

/// Retail's flow reaches the round start `0x14` - which is what the
/// side-band arms the sparring caption on - only past the enemy-name hold
/// of flow `0x0A` / `0x0B` (`FUN_801D0748`, `0x801D0DE0..0x801D0E58`). The
/// open holds the round, the caption and the action SM until the names have
/// gone, then starts the round on its own.
#[test]
fn the_sparring_caption_waits_for_the_enemy_names_to_go() {
    let mut world = tutorial_battle_world();
    world.battle.intro_names_frames = crate::battle_open::PLAIN_OPEN_FRAMES;
    assert!(world.sparring_open_held());
    world.begin_battle_round();
    assert!(world.battle.sparring_round_pending, "round start held");
    assert_eq!(world.battle.sideband.phase, 0, "caption not armed");
    assert!(!world.battle_tutorial_box_up());
    let state = world.battle_ctx.action_state;
    // The names drain one a tick; nothing else moves under them.
    for _ in 0..crate::battle_open::PLAIN_OPEN_FRAMES {
        assert!(world.battle.sparring_round_pending);
        world.live_battle_tick();
        assert_eq!(world.battle_ctx.action_state, state);
        assert_eq!(world.battle.flow, BattleFlowState::Idle, "no prompt yet");
    }
    assert_eq!(world.battle.intro_names_frames, 0);
    assert!(!world.sparring_open_held());
    // The next side-band pass sees the open done and starts the round: the
    // caption arm runs (with no caption text installed it passes straight
    // on to phase 2 and the first lesson's prompt).
    world.live_battle_tick();
    assert!(!world.battle.sparring_round_pending);
    assert_ne!(world.battle.sideband.phase, 0, "side-band past its arm");
}

/// Every fight but the spar opens its round at the flip, names or no names.
#[test]
fn an_ordinary_fight_is_not_held_by_its_enemy_names() {
    let mut world = World::new();
    world.toggles.live_gameplay_loop = true;
    world.battle.player_driven = true;
    world.enter_battle(3, 2);
    world.battle.intro_names_frames = crate::battle_open::PLAIN_OPEN_FRAMES;
    assert!(!world.sparring_open_held());
    world.begin_battle_round();
    assert!(!world.battle.sparring_round_pending);
}

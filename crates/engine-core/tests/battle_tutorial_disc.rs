//! Disc-gated: the battle sparring-tutorial prompt corpus resolves off a real
//! disc, and every message the ported hook machine can emit is present.
//!
//! The prompt text lives inside the battle-stage slot-B overlay (extraction
//! PROT 967, base `0x801F69D8`), so it can only be checked against a user's
//! own disc - no prompt text is committed. Skips and passes when
//! `LEGAIA_DISC_BIN` / `extracted/` is absent, per the repo convention.

use std::path::PathBuf;

use legaia_engine_core::battle_tutorial::{
    self as tut, BattleTutorial, BattleTutorialScript, TutorialInputs, TutorialLesson,
};
use legaia_prot::archive::Archive;

fn extracted_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for d in ["extracted", "../../extracted"] {
        let p = PathBuf::from(d);
        if p.join("PROT.DAT").exists() {
            return Some(p);
        }
    }
    None
}

/// Overlay 967's as-loaded bytes, straight out of `PROT.DAT`.
fn overlay_967() -> Option<Vec<u8>> {
    let extracted = extracted_dir()?;
    let rec = legaia_asset::static_overlay::overlay_map()
        .by_label("battle_tutorial")
        .expect("battle_tutorial row in static-overlays.toml");
    assert_eq!(rec.prot_index, tut::OVERLAY_967_PROT_INDEX);
    assert_eq!(rec.base_va, tut::OVERLAY_967_BASE_VA);

    let mut archive = Archive::open(&extracted.join("PROT.DAT")).expect("open PROT.DAT");
    let entry = archive.entries[rec.prot_index as usize].clone();
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .expect("read PROT 967");
    Some(legaia_asset::static_overlay::as_loaded(&bytes, rec).expect("as-loaded slice"))
}

#[test]
fn every_tutorial_prompt_resolves_off_the_disc() {
    let Some(bytes) = overlay_967() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let script = BattleTutorialScript::from_overlay(&bytes, tut::OVERLAY_967_BASE_VA);

    // All 25 cross-product messages plus the three drill/completion ones.
    assert_eq!(
        script.len(),
        BattleTutorialScript::MESSAGE_IDS.len() + 3,
        "every committed message VA should land on a string"
    );

    for id in BattleTutorialScript::MESSAGE_IDS {
        let text = script
            .text(id)
            .unwrap_or_else(|| panic!("message {id:#010X} missing from overlay 967"));
        assert!(!text.is_empty(), "message {id:#010X} is empty");
        assert!(
            text.is_ascii(),
            "message {id:#010X} should be plain ASCII, got {text:?}"
        );
        // Retail's '|' hard break must have been folded to a newline.
        assert!(
            !text.contains('|'),
            "message {id:#010X} still carries a raw pipe"
        );
    }
}

#[test]
fn lesson_intro_prompts_are_distinct_and_ordered() {
    let Some(bytes) = overlay_967() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let script = BattleTutorialScript::from_overlay(&bytes, tut::OVERLAY_967_BASE_VA);

    let intros = [
        tut::msg::LESSON0_INTRO,
        tut::msg::LESSON1_INTRO,
        tut::msg::LESSON2_INTRO,
        tut::msg::LESSON3_INTRO,
    ];
    let texts: Vec<&str> = intros.iter().map(|&i| script.text(i).unwrap()).collect();
    for (a, b) in intros.iter().zip(intros.iter().skip(1)) {
        assert!(a < b, "intro VAs should ascend with the lesson index");
    }
    for i in 0..texts.len() {
        for j in (i + 1)..texts.len() {
            assert_ne!(texts[i], texts[j], "lesson intros should be distinct");
        }
    }
    // Each intro is a one-liner prompting the player to select [Begin].
    for t in &texts {
        assert!(
            t.contains("[Begin]"),
            "lesson intro should prompt [Begin], got {t:?}"
        );
    }
}

#[test]
fn a_full_four_lesson_run_emits_a_prompt_at_every_hook() {
    let Some(bytes) = overlay_967() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let script = BattleTutorialScript::from_overlay(&bytes, tut::OVERLAY_967_BASE_VA);

    // Walk each lesson through the hook states the player actually reaches
    // when they follow the instructions, and assert the boxes resolve.
    let mut emitted = 0usize;
    for lesson in [
        TutorialLesson::Attacks,
        TutorialLesson::Items,
        TutorialLesson::Spirit,
        TutorialLesson::HyperArts,
    ] {
        let mut t = BattleTutorial::new();
        t.lesson = lesson.raw();
        t.inputs = TutorialInputs {
            // Player commits the category this lesson is teaching.
            action_category: lesson.expected_action_category().unwrap(),
            // ...and enters the drill correctly.
            command_buffer: tut::DRILL_AUTOFILL,
            ..Default::default()
        };
        for state in tut::HOOK_STATES {
            t.enter_flow_state();
            let tick = t.tick(state);
            for b in &tick.emission.boxes {
                assert!(
                    script.text(b.message).is_some(),
                    "lesson {lesson:?} state {state} emitted unresolvable {:#010X}",
                    b.message
                );
                assert!(
                    b.placement().is_some(),
                    "style {} out of the retail 0..=9 table",
                    b.style
                );
                emitted += 1;
            }
        }
    }
    assert!(
        emitted >= 20,
        "a four-lesson walk should emit a substantial prompt set, got {emitted}"
    );
}

/// The host-facing loader reads the same corpus the raw-archive path does, so
/// `play-window` arms the machine with real text and not an empty script.
#[test]
fn the_prot_index_loader_returns_the_same_corpus() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let index = legaia_engine_core::scene::ProtIndex::open_extracted(&extracted)
        .expect("open extracted PROT index");
    let via_host = BattleTutorialScript::from_prot(&index);
    let via_archive =
        BattleTutorialScript::from_overlay(&overlay_967().unwrap(), tut::OVERLAY_967_BASE_VA);
    assert_eq!(via_host.len(), via_archive.len());
    for id in BattleTutorialScript::MESSAGE_IDS {
        assert_eq!(via_host.text(id), via_archive.text(id), "{id:#010X}");
    }
}

/// End-to-end: a real disc's prompt corpus driving a real player-driven battle
/// through `World::live_battle_tick`. Asserts the boxes actually reach the
/// screen queue in retail order, that they park the loop, and that following the
/// lesson walks the machine forward.
#[test]
fn a_live_battle_shows_the_real_prompts_in_order() {
    use legaia_engine_core::battle_flow::BattleFlowState;
    use legaia_engine_core::input::PadButton;
    use legaia_engine_core::world::World;

    let Some(bytes) = overlay_967() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let script = BattleTutorialScript::from_overlay(&bytes, tut::OVERLAY_967_BASE_VA);
    let intro = script.text(tut::msg::LESSON0_INTRO).unwrap().to_string();
    let pick_attack = script.text(tut::msg::PICK_ATTACK).unwrap().to_string();
    let no_running = script.text(tut::msg::NO_RUNNING).unwrap().to_string();

    let mut world = World::new();
    world.live_gameplay_loop = true;
    world.battle_player_driven = true;
    world.prime_battle_tutorial(script);
    world.enter_battle(3, 2);
    for i in 0..5 {
        world.actors[i].battle.hp = 100;
        world.actors[i].battle.max_hp = 100;
    }
    assert!(world.battle_tutorial.is_some(), "armed at battle entry");

    // Drive the public per-frame tick until the loop reaches the first party
    // turn and the tutorial puts its lesson-0 intro up.
    for _ in 0..600 {
        world.tick();
        if world.battle_tutorial_box_up() {
            break;
        }
    }
    assert_eq!(world.battle_flow, BattleFlowState::TurnPrompt);
    assert_eq!(
        world.battle_tutorial_box().map(|b| b.text.as_str()),
        Some(intro.as_str()),
        "the lesson-0 intro opens the fight"
    );
    let parked = world.battle_ctx.action_state;
    world.tick();
    assert_eq!(
        world.battle_ctx.action_state, parked,
        "a box on screen parks the action SM"
    );

    // Acknowledge every queued box; the loop stays parked until the last one
    // clears, then the category prompt for this lesson comes up.
    for _ in 0..600 {
        if world.battle_flow != BattleFlowState::TurnPrompt {
            break;
        }
        world.input.set_pad(PadButton::Cross.mask());
        world.tick();
        world.input.set_pad(0);
        world.tick();
    }
    assert_eq!(world.battle_flow, BattleFlowState::CategoryMenu);
    assert_eq!(
        world.battle_tutorial_box().map(|b| b.text.as_str()),
        Some(pick_attack.as_str()),
        "lesson 0 names [Attack] next"
    );

    // Running is refused for the whole sparring fight.
    world.battle_tutorial_boxes.clear();
    world.battle_command = Some(legaia_engine_core::battle_input::BattleCommandSession {
        actor: 0,
        party_slot: 0,
        phase: legaia_engine_core::battle_input::CommandPhase::RunAway,
    });
    world.tick();
    assert_eq!(
        world.battle_tutorial_box().map(|b| b.text.as_str()),
        Some(no_running.as_str()),
        "Run is rejected"
    );
    assert!(
        world.battle_command.is_some(),
        "and the command menu comes back"
    );
}

#[test]
fn following_the_lesson_never_rewinds() {
    // Pure-logic companion: when the player does exactly what the current
    // lesson asks, the category validator (state 110) never rewinds.
    for lesson in [
        TutorialLesson::Attacks,
        TutorialLesson::Items,
        TutorialLesson::Spirit,
        TutorialLesson::HyperArts,
    ] {
        let inputs = TutorialInputs {
            action_category: lesson.expected_action_category().unwrap(),
            ..Default::default()
        };
        let e = tut::dispatch(110, lesson, &inputs);
        assert!(
            !e.rewind,
            "lesson {lesson:?} should accept its own category"
        );
        assert_eq!(e.boxes[0].message, tut::msg::NOW_BEGIN);
    }
}

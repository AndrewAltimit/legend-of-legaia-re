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

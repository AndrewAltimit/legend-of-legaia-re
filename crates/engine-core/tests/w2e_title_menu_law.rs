//! The title menu's retail law, as both hosts get it.
//!
//! `TitleSession` is what the native window and the browser play page both
//! tick, so a property asserted here is a property of both. Each one below is
//! read off `FUN_801DD35C`'s `AttractIdle` block, not off the port.

use legaia_engine_core::title::{TitleEvent, TitleInput, TitleOutcome, TitlePhase, TitleSession};
use legaia_engine_vm::title_overlay::{
    ATTRACT_INPUT_FREEZE_BELOW, COUNTDOWN_RESET_VALUE, TITLE_MENU_ROWS, TITLE_SFX_CONFIRM,
    TITLE_SFX_CURSOR_MOVE,
};

fn at_menu() -> TitleSession {
    let mut s = TitleSession::new();
    s.skip_fade_in();
    s.tick(TitleInput {
        start: true,
        ..Default::default()
    });
    assert!(matches!(s.phase(), TitlePhase::MainMenu { .. }));
    s
}

fn cursor(s: &TitleSession) -> u8 {
    match s.phase() {
        TitlePhase::MainMenu { cursor } => cursor,
        other => panic!("expected MainMenu, got {other:?}"),
    }
}

/// Retail confirms on `pad & 0x844` - Start, L1 **or** Cross. The port used
/// to take Cross alone once the menu was open.
#[test]
fn start_confirms_the_highlighted_row() {
    let mut s = at_menu();
    // Land on NEW GAME.
    s.tick(TitleInput {
        up: true,
        ..Default::default()
    });
    assert_eq!(cursor(&s), 0);
    let events = s.tick(TitleInput {
        start: true,
        ..Default::default()
    });
    assert!(events.contains(&TitleEvent::MenuConfirmed { row: 0 }));
    assert_eq!(s.outcome(), Some(TitleOutcome::NewGame));
    assert_eq!(s.last_sfx_cue(), Some(TITLE_SFX_CONFIRM));
}

/// Down steps forward, Up steps back, and the two-row space wraps -
/// retail's `andi v1,v1,0x1`.
#[test]
fn the_cursor_wraps_over_two_rows_and_cues_on_every_move() {
    assert_eq!(TITLE_MENU_ROWS, 2);
    let mut s = at_menu();
    let start = cursor(&s);
    let mut seen = vec![start];
    for _ in 0..TITLE_MENU_ROWS * 2 {
        let events = s.tick(TitleInput {
            down: true,
            ..Default::default()
        });
        assert!(
            events
                .iter()
                .any(|e| matches!(e, TitleEvent::CursorMoved { .. })),
            "every step moves in a two-row space"
        );
        assert_eq!(s.last_sfx_cue(), Some(TITLE_SFX_CURSOR_MOVE));
        seen.push(cursor(&s));
    }
    // Four steps in a two-row space returns to where it started.
    assert_eq!(seen.first(), seen.last());
    assert!(seen.iter().all(|c| *c < TITLE_MENU_ROWS));
}

/// The countdown re-arms on any pad bit and is spent by an idle frame.
#[test]
fn the_attract_countdown_runs_and_re_arms() {
    let mut s = at_menu();
    let armed = COUNTDOWN_RESET_VALUE as i32;
    s.tick(TitleInput::default());
    assert!(
        s.attract_countdown() < armed,
        "an idle frame spends the countdown"
    );
    s.tick(TitleInput {
        down: true,
        ..Default::default()
    });
    assert_eq!(
        s.attract_countdown(),
        armed - 1,
        "any pad bit re-arms it, then the frame's own tick is spent"
    );
}

/// The last sixteen frames before the attract fires read no pad at all.
#[test]
fn input_is_frozen_under_the_attract_band() {
    let mut s = at_menu();
    let before = cursor(&s);
    // Idle until the countdown drops into the freeze band.
    while s.attract_countdown() >= ATTRACT_INPUT_FREEZE_BELOW {
        s.tick(TitleInput::default());
    }
    let events = s.tick(TitleInput {
        down: true,
        cross: true,
        ..Default::default()
    });
    assert!(
        events.is_empty(),
        "the freeze band must swallow the whole input block: {events:?}"
    );
    assert_eq!(cursor(&s), before);
    assert!(s.outcome().is_none());
}

/// The attract fire arm is off by default, so a title left alone keeps
/// working instead of parking under the freeze band forever.
#[test]
fn the_attract_never_fires_unless_the_host_asks_for_it() {
    let mut s = at_menu();
    let mut fired = false;
    for _ in 0..(COUNTDOWN_RESET_VALUE as usize + 64) {
        if s.tick(TitleInput::default())
            .contains(&TitleEvent::AttractTimeout)
        {
            fired = true;
        }
    }
    assert!(!fired, "attract_enabled is false by default");
    // And the screen is still usable after the countdown lapped.
    let mut s2 = s.clone();
    s2.tick(TitleInput {
        down: true,
        ..Default::default()
    });
    // Either the cursor moved or we caught the one freeze window; in both
    // cases the session is still live.
    assert!(s2.outcome().is_none());

    let mut s = at_menu();
    s.attract_enabled = true;
    let mut fired = false;
    for _ in 0..(COUNTDOWN_RESET_VALUE as usize + 8) {
        if s.tick(TitleInput::default())
            .contains(&TitleEvent::AttractTimeout)
        {
            fired = true;
            break;
        }
    }
    assert!(fired, "with the knob on, the countdown fires");
}

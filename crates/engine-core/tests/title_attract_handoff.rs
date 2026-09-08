//! The title screen's attract hand-off, as both shipped hosts drive it.
//!
//! Retail's `AttractIdle` (`0x10`) arm zeroes the FMV index and writes master
//! game mode `0x1A` (`0x801DDCE8` / `0x801DDCF0` inside `FUN_801DD35C`), i.e.
//! hands the screen to `fmv_id 0` and returns to the title afterwards. The
//! session models that as [`TitlePhase::Attract`], and both hosts walk it with
//! the same three calls - `attract_pending` -> `mark_attract_started` ->
//! `finish_attract` - which is what
//! `scripts/ci/check-ui-host-drift.py`'s paired-injection row pins.
//!
//! What each host does *between* the second and third call is its own: the
//! native window decodes and plays the movie, the browser play page has no
//! STR/MDEC playback on the play path and finishes on the same frame. Neither
//! may leave the session parked in `Attract`, which is the failure this file
//! exists to catch.

use legaia_engine_core::title::{TitleEvent, TitleInput, TitlePhase, TitleSession};
use legaia_engine_vm::title_overlay::{ATTRACT_FMV_ID, COUNTDOWN_RESET_VALUE};

/// A session sitting on the live menu with the attract armed.
fn armed() -> TitleSession {
    let mut s = TitleSession::new();
    s.attract_enabled = true;
    s.skip_fade_in();
    s.tick(TitleInput {
        start: true,
        ..Default::default()
    });
    assert!(matches!(s.phase(), TitlePhase::MainMenu { .. }));
    s
}

/// Spend the countdown with no input at all.
fn idle_until_attract(s: &mut TitleSession) -> bool {
    for _ in 0..(COUNTDOWN_RESET_VALUE as i32 + 4) {
        for ev in s.tick(TitleInput::default()) {
            if ev == TitleEvent::AttractTimeout {
                return true;
            }
        }
    }
    false
}

#[test]
fn the_countdown_hands_the_screen_to_fmv_zero() {
    let mut s = armed();
    assert!(idle_until_attract(&mut s), "the countdown never fired");
    assert_eq!(s.attract_pending(), Some(ATTRACT_FMV_ID));
    assert_eq!(ATTRACT_FMV_ID, 0, "retail's arm hardcodes the intro movie");
}

#[test]
fn a_host_that_claims_the_movie_stops_being_offered_it() {
    let mut s = armed();
    assert!(idle_until_attract(&mut s));
    s.mark_attract_started();
    assert_eq!(s.attract_pending(), None);
    assert!(s.attract_playing());
    // The session freezes while the movie is up - no phase drift, no outcome.
    for _ in 0..120 {
        assert!(s.tick(TitleInput::default()).is_empty());
    }
    assert!(s.attract_playing());
    assert_eq!(s.outcome(), None);
}

#[test]
fn finishing_the_movie_returns_to_the_menu_with_the_countdown_re_armed() {
    let mut s = armed();
    assert!(idle_until_attract(&mut s));
    s.mark_attract_started();
    s.finish_attract();
    assert!(!s.attract_playing());
    assert_eq!(s.attract_pending(), None);
    match s.phase() {
        TitlePhase::MainMenu { cursor } => assert_eq!(cursor, 0),
        other => panic!("expected the menu, got {other:?}"),
    }
    assert_eq!(s.attract_countdown(), COUNTDOWN_RESET_VALUE as i32);
    // And the menu still works afterwards.
    let ev = s.tick(TitleInput {
        cross: true,
        ..Default::default()
    });
    assert!(ev.contains(&TitleEvent::MenuConfirmed { row: 0 }));
}

#[test]
fn the_browser_shape_finishes_on_the_same_frame_and_loops() {
    // What `web-viewer::boot_title_step` does: claim it, finish it, count the
    // skip. Two full cycles, to prove the countdown really re-arms.
    let mut s = armed();
    for cycle in 0..2 {
        assert!(idle_until_attract(&mut s), "cycle {cycle} never fired");
        let fmv = s
            .attract_pending()
            .unwrap_or_else(|| panic!("cycle {cycle} offered nothing"));
        assert_eq!(fmv, ATTRACT_FMV_ID);
        s.mark_attract_started();
        s.finish_attract();
        assert!(matches!(s.phase(), TitlePhase::MainMenu { .. }));
    }
}

#[test]
fn a_disarmed_session_never_enters_the_attract_state() {
    // The flag is the whole guard: with it off the countdown still runs and
    // re-arms, but nothing is handed anywhere.
    let mut s = TitleSession::new();
    s.skip_fade_in();
    s.tick(TitleInput {
        start: true,
        ..Default::default()
    });
    assert!(!s.attract_enabled);
    for _ in 0..(COUNTDOWN_RESET_VALUE as i32 + 4) {
        for ev in s.tick(TitleInput::default()) {
            assert_ne!(ev, TitleEvent::AttractTimeout);
        }
        assert!(!matches!(s.phase(), TitlePhase::Attract { .. }));
    }
    assert_eq!(s.attract_pending(), None);
}

#[test]
fn a_held_pad_bit_keeps_the_attract_away() {
    // Retail re-arms the countdown from the held word every frame
    // (`_DAT_8007B850` at `0x801DDC78`), so a player resting on a button
    // never sees the movie. `circle` is the port's own cancel arm and would
    // leave the menu, so hold a d-pad bit instead.
    let mut s = armed();
    for _ in 0..(COUNTDOWN_RESET_VALUE as i32 + 4) {
        for ev in s.tick(TitleInput {
            down: true,
            ..Default::default()
        }) {
            assert_ne!(ev, TitleEvent::AttractTimeout);
        }
    }
    assert_eq!(s.attract_pending(), None);
    assert!(matches!(s.phase(), TitlePhase::MainMenu { .. }));
}

#[test]
fn a_cold_boot_session_reports_the_retail_sub_mode() {
    // The session derives its entry sub-mode by running retail's `Init`
    // with the boot entry word raised, so it lands on `0x10` - never the
    // `0x02` text menu, whose handler `init.pak` makes unreachable.
    use legaia_engine_vm::title_overlay::TitleOverlaySubMode;
    let s = TitleSession::new();
    assert_eq!(s.retail_submode(), TitleOverlaySubMode::AttractIdle as u8);
    assert_ne!(s.retail_submode(), TitleOverlaySubMode::TextMenu as u8);
}

#[test]
fn each_confirmed_row_moves_the_retail_sub_mode_where_retail_moves_it() {
    use legaia_engine_vm::title_overlay::TitleOverlaySubMode;
    // Row 0 (NEW GAME) -> 0x16 LaunchFade.
    let mut s = armed();
    s.tick(TitleInput {
        up: true,
        ..Default::default()
    });
    s.tick(TitleInput {
        cross: true,
        ..Default::default()
    });
    assert_eq!(
        s.outcome(),
        Some(legaia_engine_core::title::TitleOutcome::NewGame)
    );
    assert_eq!(s.retail_submode(), TitleOverlaySubMode::LaunchFade as u8);

    // Row 1 (CONTINUE) -> 0x18 ContinueFadeIn.
    let mut s = armed();
    s.tick(TitleInput {
        cross: true,
        ..Default::default()
    });
    assert_eq!(
        s.outcome(),
        Some(legaia_engine_core::title::TitleOutcome::Continue)
    );
    assert_eq!(
        s.retail_submode(),
        TitleOverlaySubMode::ContinueFadeIn as u8
    );
}

#[test]
fn returning_from_the_attract_re_enters_through_init() {
    use legaia_engine_vm::title_overlay::TitleOverlaySubMode;
    let mut s = armed();
    assert!(idle_until_attract(&mut s));
    s.mark_attract_started();
    s.finish_attract();
    assert_eq!(s.retail_submode(), TitleOverlaySubMode::AttractIdle as u8);
}

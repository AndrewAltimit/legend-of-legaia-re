//! End-to-end smoke test for the new menu sessions.
//!
//! Drives a synthetic world through field-menu / status / spell / options /
//! game-over flows, asserting state machines compose into a single playable
//! shell.

use legaia_engine_core::field_menu::{
    FieldMenuInput, FieldMenuOutcome, FieldMenuRow, FieldMenuRowMask, FieldMenuSession,
};
use legaia_engine_core::game_over::{GameOverOutcome, GameOverPhase, GameOverSession};
use legaia_engine_core::input::{Mapping, PadButton};
use legaia_engine_core::key_rebind::{KeyRebindInput, KeyRebindOutcome, KeyRebindSession};
use legaia_engine_core::options::{OptionsInput, OptionsOutcome, OptionsSession, OptionsState};
use legaia_engine_core::spell_menu::{
    CasterSlot, SpellMenuInput, SpellMenuOutcome, SpellMenuSession, TargetRow,
};
use legaia_engine_core::spells::{SpellCatalog, SpellOutcome};
use legaia_engine_core::status_screen::{StatusScreenSession, StatusSnapshot};

fn empty_input() -> FieldMenuInput {
    FieldMenuInput::default()
}

#[test]
fn field_menu_full_cancel_path_emits_closed_outcome() {
    let mut s = FieldMenuSession::new();
    let _ = s.tick(FieldMenuInput {
        circle: true,
        ..empty_input()
    });
    assert_eq!(s.outcome(), Some(FieldMenuOutcome::Closed));
}

#[test]
fn field_menu_pick_save_routes_through_resume() {
    let mut mask = FieldMenuRowMask::ALL_ENABLED;
    mask.disable(FieldMenuRow::Save);
    // Build a mask that has Save enabled but Items disabled to exercise
    // the row-skip cursor.
    let mut mask = FieldMenuRowMask::ALL_ENABLED;
    mask.disable(FieldMenuRow::Items);
    let mut s = FieldMenuSession::with_mask(mask);
    // Cursor lands on Magic (idx 1, the first enabled row). Move down to
    // Save (idx 6 in retail order).
    for _ in 0..(FieldMenuRow::Save.index() - FieldMenuRow::Magic.index()) as usize {
        let _ = s.tick(FieldMenuInput {
            down: true,
            ..empty_input()
        });
    }
    let _ = s.tick(FieldMenuInput {
        cross: true,
        ..empty_input()
    });
    assert!(s.is_suspended());
    let _ = s.resume(true);
    assert_eq!(
        s.outcome(),
        Some(FieldMenuOutcome::Confirmed(FieldMenuRow::Save))
    );
}

#[test]
fn status_screen_cycles_party() {
    let mut s = StatusScreenSession::new(vec![
        StatusSnapshot::placeholder(0, "Vahn"),
        StatusSnapshot::placeholder(1, "Noa"),
        StatusSnapshot::placeholder(2, "Gala"),
    ]);
    use legaia_engine_core::status_screen::StatusInput;
    let _ = s.tick(StatusInput {
        r1: true,
        ..Default::default()
    });
    assert_eq!(s.cursor(), 1);
    let _ = s.tick(StatusInput {
        r1: true,
        ..Default::default()
    });
    let _ = s.tick(StatusInput {
        r1: true,
        ..Default::default()
    });
    assert_eq!(s.cursor(), 0);
    let _ = s.tick(StatusInput {
        circle: true,
        ..Default::default()
    });
    assert!(s.is_done());
}

#[test]
fn spell_menu_completes_heal_cast() {
    let party = vec![
        CasterSlot {
            slot: 0,
            name: "Vahn".into(),
            hp: 60,
            mp: 30,
            spells: vec![],
            ..Default::default()
        },
        CasterSlot {
            slot: 1,
            name: "Noa".into(),
            hp: 50,
            mp: 30,
            spells: vec![0x10],
            ..Default::default()
        },
    ];
    let targets = vec![TargetRow {
        slot: 0,
        name: "Vahn".into(),
        hp: 30,
        hp_max: 60,
    }];
    let mut s = SpellMenuSession::new(party, targets, SpellCatalog::vanilla());
    let _ = s.tick(SpellMenuInput {
        down: true,
        ..Default::default()
    });
    let _ = s.tick(SpellMenuInput {
        cross: true,
        ..Default::default()
    });
    let _ = s.tick(SpellMenuInput {
        cross: true,
        ..Default::default()
    });
    let _ = s.tick(SpellMenuInput {
        cross: true,
        ..Default::default()
    });
    match s.outcome() {
        Some(SpellMenuOutcome::Cast {
            outcome: SpellOutcome::Heal { .. },
            ..
        }) => {}
        other => panic!("expected Heal cast, got {other:?}"),
    }
}

#[test]
fn options_session_round_trip_persists_changes() {
    // Retail flow: Cross opens the value popup on Battle Camera, Down
    // picks "Normal", Cross commits, Circle leaves the screen. The
    // committed value survives the close (retail writes the config word
    // at popup confirm and never reverts).
    let mut s = OptionsSession::new(OptionsState::default());
    for input in [
        OptionsInput {
            cross: true,
            ..Default::default()
        },
        OptionsInput {
            down: true,
            ..Default::default()
        },
        OptionsInput {
            cross: true,
            ..Default::default()
        },
        OptionsInput {
            circle: true,
            ..Default::default()
        },
    ] {
        let _ = s.tick(input);
    }
    assert_eq!(s.outcome(), Some(OptionsOutcome::Closed));
    assert_eq!(
        s.state().battle_camera,
        legaia_engine_core::options::BattleCameraOpt::Normal
    );
}

/// The Key Config row is engine-only and **opt-in**: a session built without
/// a binding table shows the retail ten rows and nothing else, so an oracle,
/// a replay driver or a headless host cannot pick up a row retail has no
/// config word for.
#[test]
fn the_key_config_row_appears_only_when_a_host_arms_a_binding_table() {
    let plain = OptionsSession::new(OptionsState::default());
    assert!(!plain.key_config_armed());
    assert_eq!(plain.display_rows().len(), 10);
    assert!(plain.state().rows_for(false).len() == 10);

    let armed = OptionsSession::with_key_rebind(OptionsState::default(), Mapping::default());
    assert!(armed.key_config_armed());
    assert_eq!(armed.display_rows().len(), 11);
    let rows = armed.state().rows_for(true);
    assert_eq!(rows[10].label, "Key Config");
    // A value string, not an empty column: an empty one reads as the
    // "Dual Shock" group header directly above it.
    assert!(rows[10].value.is_some());
}

/// The whole menu route the key-rebind screen used to lack, end to end: walk
/// the browse cursor onto the engine-only row, Cross into the sub-screen,
/// Cross onto a button row, press a key, and read the committed table back
/// out of the session the way both hosts do (native persists it to
/// `legaia-input.toml`, the browser page to `localStorage`).
#[test]
fn the_options_screen_reaches_the_key_rebind_sub_screen_and_commits_a_bind() {
    let mut s = OptionsSession::with_key_rebind(OptionsState::default(), Mapping::default());
    // Ten Downs from row 0 lands on the eleventh row: the Dual Shock header
    // is skipped as unselectable, so the walk wraps once through row 0.
    let mut guard = 0;
    while s.cursor() as usize != 10 {
        let _ = s.tick(OptionsInput {
            down: true,
            ..Default::default()
        });
        guard += 1;
        assert!(guard < 64, "cursor never reached the Key Config row");
    }
    // Cross opens the sub-screen rather than a value popup.
    let events = s.tick(OptionsInput {
        cross: true,
        ..Default::default()
    });
    assert!(events.iter().any(|e| matches!(
        e,
        legaia_engine_core::options::OptionsEvent::KeyRebindOpened { row: 10 }
    )));
    assert!(s.popup().is_none(), "this row has no value popup");
    let sub = s.key_rebind().expect("sub-screen open");
    assert_eq!(sub.rows().len(), 16, "one row per pad button");
    assert_eq!(sub.rows()[0].button, PadButton::Cross);
    assert_eq!(sub.rows()[0].key, "Z", "default Cross binding");

    // Cross arms the await, then the host hands over one key name.
    let _ = s.tick(OptionsInput {
        cross: true,
        ..Default::default()
    });
    assert!(matches!(
        s.key_rebind().unwrap().phase(),
        legaia_engine_core::key_rebind::KeyRebindPhase::AwaitingKey { cursor: 0 }
    ));
    let events = s.tick_with_key(OptionsInput::default(), Some("K"));
    assert!(events.iter().any(|e| matches!(
        e,
        legaia_engine_core::options::OptionsEvent::BindingsChanged
    )));

    // The committed table is on the session, and the dirty flag reports
    // once - which is exactly what each host persists on.
    let m = s.mapping().expect("armed");
    assert_eq!(m.pad_button_for_key("K"), Some(PadButton::Cross));
    assert_eq!(m.pad_button_for_key("Z"), None, "old binding evicted");
    assert!(s.take_bindings_dirty());
    assert!(!s.take_bindings_dirty(), "dirty is report-and-clear");

    // Leaving the sub-screen drops back to browsing on the same row, and the
    // settings screen still closes normally.
    let _ = s.tick(OptionsInput {
        start: true,
        ..Default::default()
    });
    assert!(s.key_rebind().is_none());
    assert_eq!(s.cursor(), 10);
    assert!(!s.is_done());
    let _ = s.tick(OptionsInput {
        circle: true,
        ..Default::default()
    });
    assert_eq!(s.outcome(), Some(OptionsOutcome::Closed));
}

/// A key handed over while no rebind screen is open must not be bound: both
/// hosts latch one key name per physical press and pass it every tick, so the
/// settings screen sees keys it has to ignore.
#[test]
fn a_key_outside_the_rebind_screen_changes_nothing() {
    let mut s = OptionsSession::with_key_rebind(OptionsState::default(), Mapping::default());
    let before = s.mapping().cloned().unwrap();
    let _ = s.tick_with_key(OptionsInput::default(), Some("K"));
    let _ = s.tick_with_key(
        OptionsInput {
            down: true,
            ..Default::default()
        },
        Some("Q"),
    );
    assert_eq!(s.mapping().unwrap().bindings, before.bindings);
    assert!(!s.take_bindings_dirty());
}

#[test]
fn options_session_popup_cancel_leaves_value() {
    let mut s = OptionsSession::new(OptionsState::default());
    // Open the popup, move the popup cursor, back out with Circle -
    // nothing commits.
    let _ = s.tick(OptionsInput {
        cross: true,
        ..Default::default()
    });
    let _ = s.tick(OptionsInput {
        down: true,
        ..Default::default()
    });
    let _ = s.tick(OptionsInput {
        circle: true,
        ..Default::default()
    });
    assert!(!s.is_done());
    assert_eq!(
        s.state().battle_camera,
        legaia_engine_core::options::BattleCameraOpt::Close
    );
}

/// The wipe hand-off holds, then resolves to the title - the port of
/// `FUN_8003AEB0`'s `game_mode = 0x16` / `_DAT_8007BB00 = 1` store pair.
///
/// These two assertions replace a pair that asserted the defect: they drove a
/// cursor across Continue / Retry / Quit rows and checked that a disabled
/// Continue was skipped. Retail has no rows, so the old tests could only ever
/// hold the invention in place.
#[test]
fn game_over_holds_then_hands_to_the_title() {
    let mut s = GameOverSession::with_hold(2);
    s.tick();
    assert_eq!(
        s.phase(),
        GameOverPhase::Hold {
            frames_remaining: 1
        }
    );
    assert_eq!(s.outcome(), None, "the hold must not resolve early");
    s.tick();
    assert_eq!(s.outcome(), Some(GameOverOutcome::ReturnToTitle));
}

#[test]
fn game_over_default_hold_matches_the_traced_title_fade() {
    // 0xFF (the clamped screen-fade level) / 8 (the per-frame drain in title
    // sub-mode 0x11 at 0x801DDAEC).
    assert_eq!(
        GameOverSession::new().frames_remaining(),
        legaia_engine_core::game_over::TITLE_HANDOFF_FRAMES
    );
}

#[test]
fn key_rebind_round_trip_evicts_old_binding() {
    let mut s = KeyRebindSession::new(Mapping::default());
    // Rebind Cross (cursor 0) from Z to K.
    let _ = s.tick(KeyRebindInput {
        cross: true,
        ..Default::default()
    });
    let _ = s.tick(KeyRebindInput {
        key_pressed: Some("K".into()),
        ..Default::default()
    });
    assert_eq!(s.mapping().pad_button_for_key("K"), Some(PadButton::Cross));
    assert_eq!(s.mapping().pad_button_for_key("Z"), None);
    // Confirm with Start.
    let _ = s.tick(KeyRebindInput {
        start: true,
        ..Default::default()
    });
    assert_eq!(s.outcome(), Some(KeyRebindOutcome::Confirmed));
}

#[test]
fn full_menu_loop_field_menu_to_options_to_save_back_to_scene() {
    // Open field menu.
    let mut fm = FieldMenuSession::new();
    fm.money = 5000;
    fm.play_time_seconds = 600;
    // Confirm row 0 (Items) - would push InventoryUseSession in shell.
    let _ = fm.tick(FieldMenuInput {
        cross: true,
        ..Default::default()
    });
    assert!(fm.is_suspended());
    // Sim sub-session finished, return to browsing.
    let _ = fm.resume(false);
    // Move to Config (last row).
    for _ in 0..(FieldMenuRow::Options.index() as usize) {
        let _ = fm.tick(FieldMenuInput {
            down: true,
            ..Default::default()
        });
    }
    let _ = fm.tick(FieldMenuInput {
        cross: true,
        ..Default::default()
    });
    // Drop into options; Circle leaves the screen (retail commits value
    // edits inside the popup, so exit is a plain close).
    let mut opt = OptionsSession::new(OptionsState::default());
    let _ = opt.tick(OptionsInput {
        circle: true,
        ..Default::default()
    });
    assert_eq!(opt.outcome(), Some(OptionsOutcome::Closed));
    // Return; shell would call resume(true) to close menu.
    let _ = fm.resume(true);
    assert_eq!(
        fm.outcome(),
        Some(FieldMenuOutcome::Confirmed(FieldMenuRow::Options))
    );
}

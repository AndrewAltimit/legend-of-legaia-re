//! End-to-end smoke test for the new menu sessions.
//!
//! Drives a synthetic world through field-menu / status / spell / options /
//! game-over flows, asserting state machines compose into a single playable
//! shell.

use legaia_engine_core::field_menu::{
    FieldMenuInput, FieldMenuOutcome, FieldMenuRow, FieldMenuRowMask, FieldMenuSession,
};
use legaia_engine_core::game_over::{
    GameOverInput, GameOverOutcome, GameOverPhase, GameOverSession,
};
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
    // Cursor lands on Equip (idx 1). Move down to Save (idx 5 in retail order).
    for _ in 0..(FieldMenuRow::Save.index() - FieldMenuRow::Equip.index()) as usize {
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
        },
        CasterSlot {
            slot: 1,
            name: "Noa".into(),
            hp: 50,
            mp: 30,
            spells: vec![0x10],
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
    let mut s = OptionsSession::new(OptionsState::default());
    // Decrement BGM volume once (8 → 7) and confirm.
    let _ = s.tick(OptionsInput {
        left: true,
        ..Default::default()
    });
    let _ = s.tick(OptionsInput {
        cross: true,
        ..Default::default()
    });
    assert_eq!(s.outcome(), Some(OptionsOutcome::Confirmed));
    assert_eq!(s.state().bgm_volume, 7);
}

#[test]
fn options_session_cancel_reverts() {
    let mut s = OptionsSession::new(OptionsState::default());
    let _ = s.tick(OptionsInput {
        right: true,
        ..Default::default()
    });
    let _ = s.tick(OptionsInput {
        circle: true,
        ..Default::default()
    });
    s.revert_if_cancelled();
    assert_eq!(s.state().bgm_volume, 8);
}

#[test]
fn game_over_continue_outcome() {
    let mut s = GameOverSession::new();
    s.fade_in_frames = 1;
    s.phase = GameOverPhase::FadeIn {
        frames_remaining: 1,
    };
    let _ = s.tick(GameOverInput::default());
    let _ = s.tick(GameOverInput {
        cross: true,
        ..Default::default()
    });
    assert_eq!(s.outcome(), Some(GameOverOutcome::Continue));
}

#[test]
fn game_over_skips_continue_when_no_save() {
    let mut s = GameOverSession::with_no_save();
    s.phase = GameOverPhase::Choosing { cursor: 1 }; // Retry
    let _ = s.tick(GameOverInput {
        up: true,
        ..Default::default()
    });
    // Wraps around, skipping disabled Continue, lands on Quit.
    assert_eq!(
        s.cursor(),
        legaia_engine_core::game_over::GameOverRow::Quit.index()
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
    for _ in 0..(FieldMenuRow::Config.index() as usize) {
        let _ = fm.tick(FieldMenuInput {
            down: true,
            ..Default::default()
        });
    }
    let _ = fm.tick(FieldMenuInput {
        cross: true,
        ..Default::default()
    });
    // Drop into options.
    let mut opt = OptionsSession::new(OptionsState::default());
    let _ = opt.tick(OptionsInput {
        cross: true,
        ..Default::default()
    });
    assert_eq!(opt.outcome(), Some(OptionsOutcome::Confirmed));
    // Return; shell would call resume(true) to close menu.
    let _ = fm.resume(true);
    assert_eq!(
        fm.outcome(),
        Some(FieldMenuOutcome::Confirmed(FieldMenuRow::Config))
    );
}

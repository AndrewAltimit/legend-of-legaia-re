//! Op `0x49`'s `-1` rows are a scripted menu-button press.
//!
//! Retail's save point is `49 01 00` followed by the interaction terminator
//! `0x21`. A PCSX-Redux capture at the `town01` save point
//! (`scripts/pcsx-redux/autorun_save_point_press.lua`) logs the chain one hit
//! each: the Idle arm's park store `0x801E09A8`, the enter half
//! `FUN_801F1278`, the state pick `FUN_801F1F4C` with `+0x50 = 7`, the
//! pause-menu session `FUN_801ED308` with `+0x50 = 0x30`, game mode `23` with
//! no Start press, and - after the menu closes - one Done store at
//! `0x801F16AC` from the dispatcher's retire arm, after which the park stays
//! clear.

use super::*;

use crate::field_menu::{FieldMenuGate, FieldMenuPhase, FieldMenuRow, FieldMenuSession};
use crate::field_submode_screen::OP49_PARK_PRESERVING_SUB_OPS;

/// The save-point record's interaction section: `49 01 00`, then the raw
/// `0x21` the dialog SM stops on.
const SAVE_POINT: [u8; 4] = [0x49, 0x01, 0x00, 0x21];

fn armed_world(script: Vec<u8>) -> World {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.load_field_script(script);
    let _ = world.tick();
    world
}

#[test]
fn a_save_point_park_is_a_pending_menu_press_and_opens_no_submode_screen() {
    let world = armed_world(SAVE_POINT.to_vec());
    assert_eq!(world.menu_entry_context_kind(), Some(1));
    assert!(
        !world.field_vm.submode_screen.open,
        "a -1 row runs the state pick, not a submode screen"
    );
    assert!(world.scripted_menu_open_pending());
}

#[test]
fn a_handler_row_is_not_a_menu_press() {
    // Sub-op 9 names slot 0x28 (the prompt), so it opens its own screen.
    let world = armed_world(vec![0x49, 0x09, 0x00, 0x21]);
    assert_eq!(world.menu_entry_context_kind(), Some(9));
    assert!(!world.scripted_menu_open_pending());
}

#[test]
fn the_press_is_answered_once_per_arm() {
    let mut world = armed_world(SAVE_POINT.to_vec());
    assert!(world.scripted_menu_open_pending());
    world.note_scripted_menu_opened();
    assert!(!world.scripted_menu_open_pending());
    // A re-arm is a fresh press.
    world.record_op49_park(1);
    assert!(world.scripted_menu_open_pending());
}

#[test]
fn the_press_waits_for_a_field_run_mode() {
    let mut world = armed_world(SAVE_POINT.to_vec());
    world.mode = SceneMode::Menu;
    assert!(
        !world.scripted_menu_open_pending(),
        "an open menu is not a field-run mode"
    );
    world.mode = SceneMode::Field;
    assert!(world.scripted_menu_open_pending());
}

#[test]
fn kind_one_opens_the_menu_on_the_save_screen_and_its_exit_ends_the_menu() {
    let mut session = FieldMenuSession::new();
    session.set_gate(FieldMenuGate {
        entry_context_kind: Some(1),
        // A field scene: the root Save row is greyed, and the save point
        // must not go through it.
        save_allowed: false,
    });
    session.open_entry_screen();
    assert_eq!(
        session.phase(),
        FieldMenuPhase::Suspended {
            row: FieldMenuRow::Save
        }
    );
    // Every host's root-list step builds the sub-session from this state.
    assert_eq!(
        crate::field_menu_dispatch::tick_root_list(&mut session, 0),
        Some(FieldMenuRow::Save)
    );
    // The hosts resume with `close = false`; there is no picker to return to.
    let _ = session.resume(false);
    assert!(
        session.outcome().is_some(),
        "the save screen's exit ends the menu"
    );

    // Contrast: the same row reached from the picker returns to the picker.
    let mut session = FieldMenuSession::new();
    session.set_gate(FieldMenuGate {
        entry_context_kind: None,
        save_allowed: true,
    });
    session.open_entry_screen();
    assert!(matches!(session.phase(), FieldMenuPhase::Browsing { .. }));
}

#[test]
fn closing_the_menu_resumes_the_parked_op_once() {
    let mut world = armed_world(SAVE_POINT.to_vec());
    let pc_armed = world.field_pc;
    world.note_scripted_menu_opened();
    for _ in 0..30 {
        let _ = world.tick();
    }
    assert_eq!(
        world.field_pc, pc_armed,
        "the op stays parked while the menu is up"
    );
    assert!(world.release_menu_entry_context_park());
    let _ = world.tick();
    assert_ne!(world.field_pc, pc_armed, "the close must resume the op");
    assert_eq!(world.menu_entry_context_kind(), None);
    for _ in 0..30 {
        let _ = world.tick();
    }
    assert!(
        !world.scripted_menu_open_pending(),
        "the resumed op must not press the menu again"
    );
}

#[test]
fn every_scripted_press_row_is_a_minus_one_table_row() {
    for &k in &OP49_PARK_PRESERVING_SUB_OPS {
        assert_eq!(
            legaia_engine_vm::baka_hub_actors::slot_for_sub_op(k),
            None,
            "sub-op {k:#x} names a handler, so the enter half overwrites +0x50"
        );
    }
}

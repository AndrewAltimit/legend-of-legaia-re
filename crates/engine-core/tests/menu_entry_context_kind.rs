//! The op-`0x49` park's **kind byte** reaches the pause menu, and the two
//! screens that hang off kind `0x0D` open.
//!
//! Retail keeps one global entry-context pointer (`_DAT_8007B450`) that the
//! field VM's op `0x49` fills with the *operand pointer*, and every consumer
//! dereferences its first byte - the sub-op. Four values route somewhere in
//! the menu overlay's outer dispatcher (`FUN_801DC6B4`,
//! `0x801dc88c..0x801dc8e4`): `0` opens sub-screen `0x1A`, `1` opens `0x19`,
//! `7` opens `0x20` (the casino prize exchange) and `0x0D` opens `4` - the
//! notice panel whose root-menu cancel is the ready check.
//!
//! The port used to be able to answer that question only for the two sub-ops
//! it happened to resolve through dedicated host paths (an armed inline shop
//! = `0`, an installed tile board = `5`), so nothing could ever select `7` or
//! `0x0D`. These tests are the non-vacuity proof that the arm now records the
//! byte and the pause menu acts on it.
//!
//! Disc-free: the field VM is driven over synthetic bytecode.

use legaia_engine_core::field_menu::{
    FieldMenuGate, FieldMenuInput, FieldMenuPhase, FieldMenuSession,
};
use legaia_engine_core::pause_screens::{
    ROOT_MENU_CONTEXT_LOCKED, RootMenuRoute, root_menu_cancel_route, root_menu_confirm_route,
};
use legaia_engine_core::world::World;

/// Step one op-`0x49` instruction with `sub_op` through the world's own
/// field-VM host, the way a scene script would.
fn arm(world: &mut World, sub_op: u8) {
    world.field_bytecode = vec![0x49, sub_op, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    world.field_pc = 0;
    let _ = world.step_field();
}

#[test]
fn the_arm_publishes_every_sub_op_as_the_entry_context_kind() {
    for sub_op in 0..=0x0Du8 {
        let mut w = World::new();
        assert_eq!(
            w.menu_entry_context_kind(),
            None,
            "a world with nothing parked has a null entry context"
        );
        arm(&mut w, sub_op);
        assert_eq!(
            w.menu_entry_context_kind(),
            Some(sub_op),
            "sub-op {sub_op:#x} must reach the menu as its own kind byte"
        );
    }
}

/// A sub-op the Idle arm refuses (`sltiu v0,v0,0xe` at `0x801e098c`) parks
/// nothing, so the context stays null. Without this the test above would
/// pass for a `record_op49_park` that ran unconditionally.
#[test]
fn an_out_of_range_sub_op_parks_nothing() {
    let mut w = World::new();
    arm(&mut w, 0x0E);
    assert_eq!(w.menu_entry_context_kind(), None);
}

#[test]
fn the_locked_kind_blocks_load_and_reroutes_cancel() {
    let mut w = World::new();
    arm(&mut w, ROOT_MENU_CONTEXT_LOCKED);
    let kind = w.menu_entry_context_kind();
    assert_eq!(kind, Some(ROOT_MENU_CONTEXT_LOCKED));

    // Row 5 is Load: retail buzzes it under this kind and offers it under
    // every other one. Both arms asserted, so a route that buzzed
    // unconditionally would fail.
    assert_eq!(root_menu_confirm_route(5, kind, true), RootMenuRoute::Buzz);
    assert!(matches!(
        root_menu_confirm_route(5, Some(0), true),
        RootMenuRoute::Sub(_)
    ));
    assert_eq!(root_menu_cancel_route(kind), 3);
    assert_eq!(root_menu_cancel_route(Some(0)), 0);
}

/// The whole kind-`0x0D` flow through the shared picker: it opens on the
/// notice panel, a press hands to the root list, cancel raises the ready
/// check seeded to No, No returns, and Yes ends the menu.
#[test]
fn the_locked_kind_opens_the_notice_then_gates_the_exit() {
    let mut s = FieldMenuSession::new();
    s.set_gate(FieldMenuGate {
        entry_context_kind: Some(ROOT_MENU_CONTEXT_LOCKED),
        save_allowed: false,
    });
    s.open_entry_screen();
    assert!(s.notice_is_up(), "sub-screen 4 is the entry screen");

    // Either button dismisses it; there is no cursor on this panel.
    let press = FieldMenuInput {
        cross: true,
        ..Default::default()
    };
    s.tick(press);
    assert!(!s.notice_is_up());
    assert!(matches!(s.phase(), FieldMenuPhase::Browsing { .. }));

    // Move off row 0 first, so "return to the picker" can be told apart from
    // "reset the picker" - retail's ready check never touches the picker's
    // own cursor global.
    s.tick(FieldMenuInput {
        down: true,
        ..Default::default()
    });
    let picked_row = s.cursor();
    assert_ne!(picked_row, 0, "the picker moved off its first row");

    // Cancel out of the root list raises the ready check instead of closing.
    let cancel = FieldMenuInput {
        circle: true,
        ..Default::default()
    };
    s.tick(cancel);
    assert_eq!(
        s.ready_confirm_cursor(),
        Some(1),
        "retail seeds DAT_801E46D0 to 1 = No"
    );
    assert!(!s.is_done());

    // Confirming the default (No) returns to the list, nothing closed - and
    // lands on the row the player cancelled from, not on row 0.
    s.tick(press);
    assert!(matches!(s.phase(), FieldMenuPhase::Browsing { .. }));
    assert_eq!(s.cursor(), picked_row, "the picker keeps its own cursor");

    // Cancel again, step left to Yes, confirm: the menu ends.
    s.tick(cancel);
    let left = FieldMenuInput {
        left: true,
        ..Default::default()
    };
    s.tick(left);
    assert_eq!(s.ready_confirm_cursor(), Some(0));
    s.tick(press);
    assert!(s.is_done(), "Yes routes to sub-screen 0 - the menu ends");
}

/// The control: without the locked kind neither screen exists, so cancel
/// closes immediately and the entry screen is the root list.
#[test]
fn an_unlocked_context_keeps_the_old_behaviour() {
    for kind in [None, Some(0u8), Some(5), Some(7)] {
        let mut s = FieldMenuSession::new();
        s.set_gate(FieldMenuGate {
            entry_context_kind: kind,
            save_allowed: true,
        });
        s.open_entry_screen();
        assert!(!s.notice_is_up(), "kind {kind:?} opens on the root list");
        s.tick(FieldMenuInput {
            circle: true,
            ..Default::default()
        });
        assert!(s.is_done(), "kind {kind:?} closes on cancel");
        assert_eq!(s.ready_confirm_cursor(), None);
    }
}

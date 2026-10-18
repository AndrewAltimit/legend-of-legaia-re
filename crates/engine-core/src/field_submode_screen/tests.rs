use super::*;

fn world_with_driver() -> World {
    let mut w = World::new();
    w.man_load_actor_reset();
    assert!(
        w.find_actor_by_handler(ActorHandler::SubmodeDriver)
            .is_some(),
        "the MAN-load reset spawns the driver actor"
    );
    w
}

#[test]
fn a_screen_only_ticks_while_the_driver_actor_is_alive() {
    let mut w = world_with_driver();
    w.open_coin_counter();
    assert!(!w.tick_submode_screen(1));
    assert!(!w.field_vm.submode_screen.frame.actions.is_empty());

    // Retire the driver: the dispatcher has nothing to run.
    w.retire_actors_by_handler(ActorHandler::SubmodeDriver);
    w.retire_yielded_actors();
    w.field_vm.submode_screen.frame = HubFrame::default();
    assert!(!w.tick_submode_screen(1));
    assert!(w.field_vm.submode_screen.frame.actions.is_empty());
}

/// The coin counter resolves to lines both hosts draw: the entry panel's
/// four labels (read by VA), its numbers as fixed 8-px cells, and - once
/// the confirm is up - the three-line panel at record 11's origin with
/// its cursor, all while the entry panel stays up underneath.
#[test]
fn the_coin_counter_resolves_to_positioned_lines() {
    use legaia_engine_minigames::slot_machine::{COIN_ENTRY_LABEL_VAS, COIN_ENTRY_ORIGIN};
    let mut w = world_with_driver();
    w.party.money = 5_000;
    w.minigames.casino_coins = 7;
    w.open_coin_counter();
    w.tick_submode_screen(1);
    let read = |va: u32| format!("<{va:08X}>").into_bytes();
    let lines = coin_counter_lines(&w.field_vm.submode_screen, read);
    let (x, y) = COIN_ENTRY_ORIGIN;
    let label0 = format!("<{:08X}>", COIN_ENTRY_LABEL_VAS[0]).into_bytes();
    assert!(
        lines
            .iter()
            .any(|l| l.text == label0 && (l.x, l.y) == (x, y + 2))
    );
    // The bank `7` right-aligned in an 8-cell field: the last cell.
    assert!(
        lines
            .iter()
            .any(|l| l.text == b"7" && (l.x, l.y) == (x + 0x78 + 7 * 8, y + 2))
    );

    w.field_vm.submode_screen.counter.set_entered(3);
    w.input.set_pad(legaia_engine_system::pad::retail_packed(
        SUBMODE_ACCEPT_MASK as u16,
    ));
    w.tick_submode_screen(1);
    assert_eq!(w.field_vm.submode_screen.actor.sub, 2);
    let lines = coin_counter_lines(&w.field_vm.submode_screen, read);
    let (tx, ty) = panel_window_origin(hub::window::THREE_LINE);
    let three = format!("<{:08X}>", hub::STR_THREE_LINE[0]).into_bytes();
    assert!(
        lines
            .iter()
            .any(|l| l.text == three && (l.x, l.y) == (tx + 0x24, ty)),
        "the confirm draws at record 11's origin"
    );
    assert!(lines.iter().any(|l| l.text == label0), "entry panel stays");
    assert!(lines.iter().any(|l| l.text == SUBMODE_CURSOR_STAND_IN));

    // Another screen: nothing.
    w.open_field_submode_screen(slot::CLOSE_TICK, None);
    assert!(coin_counter_lines(&w.field_vm.submode_screen, read).is_empty());
}

#[test]
fn the_coin_counter_moves_coins_and_gold_through_a_world_tick() {
    let mut w = world_with_driver();
    w.party.money = 5_000;
    w.minigames.casino_coins = 7;
    w.open_coin_counter();

    // Frame 1: state 0 seeds the screen.
    w.tick_submode_screen(1);
    // Type 12 coins.
    w.field_vm.submode_screen.counter.set_entered(12);
    // Frame 2: accept.
    w.input.set_pad(legaia_engine_system::pad::retail_packed(
        SUBMODE_ACCEPT_MASK as u16,
    ));
    w.tick_submode_screen(1);
    assert_eq!(
        w.field_vm.submode_screen.actor.sub, 2,
        "the confirm panel is up"
    );
    // Frame 3: pick Yes on the confirm panel.
    w.input.set_pad(0);
    w.field_vm.submode_screen.counter.yes_no = 0;
    w.field_vm.submode_screen.picker_result = hub::PICK_ACCEPT;
    w.tick_submode_screen(1);
    // Frame 4: the commit.
    w.field_vm.submode_screen.picker_result = 0;
    w.tick_submode_screen(1);

    assert_eq!(
        w.minigames.casino_coins,
        7 + 12,
        "coins land in the casino bank"
    );
    assert_eq!(w.party.money, 5_000 - 1_200, "gold pays 100 per coin");
}

#[test]
fn the_close_tick_default_unparks_instead_of_hanging() {
    let mut w = world_with_driver();
    // Slot 0 is what a fresh driver carries.
    w.open_field_submode_screen(slot::CLOSE_TICK, None);
    assert!(w.field_vm.submode_screen.is_open());
    let mut retired = false;
    for _ in 0..8 {
        if w.tick_submode_screen(1) {
            retired = true;
            break;
        }
    }
    assert!(retired, "the close tick clears the gate and retires");
    assert!(w.field_vm.submode_screen.is_done());
    assert!(!w.field_vm.submode_screen.is_open());
}

/// `49 0C` (slot `0x33`, `FUN_801EDF00`): the records slide in, a face
/// press fades to white, and the reload raises the title hand-off. The
/// screen never hands the frame back, so the op stays parked.
#[test]
fn the_soft_reset_slides_records_in_then_hands_off_to_the_title() {
    use legaia_engine_field::world_map_panel_actors::{
        SOFT_RESET_SLIDE_REST, SOFT_RESET_SLIDE_START,
    };
    use legaia_engine_system::input::PadButton;
    assert_eq!(slot_for_op49_sub_op(0x0C), Some(SOFT_RESET_SLOT));
    let mut w = world_with_driver();
    w.open_field_submode_screen(SOFT_RESET_SLOT, None);
    assert!(w.soft_reset_records_pen().is_none(), "nothing drawn yet");
    w.tick_submode_screen(1);
    w.tick_submode_screen(1);
    let (x, y0) = w.soft_reset_records_pen().expect("records drawn");
    assert_eq!(x, SOFT_RESET_RECORDS_X);
    assert!(y0 < SOFT_RESET_SLIDE_START && y0 > SOFT_RESET_SLIDE_REST);
    // A press before the slide lands is ignored.
    w.input.set_pad(PadButton::Cross.mask());
    assert!(!w.tick_submode_screen(1));
    w.input.set_pad(0);
    for _ in 0..0x100 {
        assert!(!w.tick_submode_screen(1), "the soft reset never retires");
    }
    assert_eq!(
        w.soft_reset_records_pen(),
        Some((SOFT_RESET_RECORDS_X, SOFT_RESET_SLIDE_REST))
    );
    assert!(!w.game_over);
    w.input.set_pad(PadButton::Cross.mask());
    w.tick_submode_screen(1);
    w.input.set_pad(0);
    assert_eq!(
        w.field_vm.submode_screen.soft_reset.phase, 2,
        "white fade armed"
    );
    for _ in 0..0x78 {
        w.tick_submode_screen(1);
    }
    assert!(w.game_over, "the reload raises the title hand-off");
    assert!(w.field_vm.submode_screen.is_open(), "the op stays parked");
}

#[test]
fn a_painted_screen_emits_draws() {
    let mut w = world_with_driver();
    w.party.money = 100_000;
    w.open_coin_counter();
    // The entry arm installs the counter's idle panel (record 10, painted
    // by `slot_machine::coin_entry_panel` into `coin_panel`); the confirm
    // adds the three-line one, whose painter is a hub painter. So the hub
    // draw appears when the descriptor names it, not when the screen
    // opens, and the entry panel's record stays installed under it.
    w.tick_submode_screen(1);
    assert_eq!(w.field_vm.submode_screen.installed_windows, vec![0, 10, 10]);
    w.field_vm.submode_screen.counter.set_entered(2);
    w.input.set_pad(legaia_engine_system::pad::retail_packed(
        SUBMODE_ACCEPT_MASK as u16,
    ));
    w.tick_submode_screen(1);
    assert_eq!(
        w.field_vm.submode_screen.installed_windows,
        vec![0, 10, 10, hub::window::THREE_LINE]
    );
    assert!(
        !w.field_vm.submode_screen.draws().is_empty(),
        "the installed panel's painter runs alongside the state machine"
    );
}

/// The engine asks the installer kernel which op-`0x49` rows are menu
/// presses; `engine-field`, which sits below `engine-core` and cannot call it,
/// keeps the answer as a constant for its MAN scanners. The two must agree on
/// every byte the operand can hold.
#[test]
fn the_menu_press_rows_are_the_installer_kernels_answer() {
    use legaia_engine_field::man_field_scripts::OP49_PARK_PRESERVING_SUB_OPS;
    for k in 0..=u8::MAX {
        assert_eq!(
            op49_sub_op_is_menu_press(k),
            OP49_PARK_PRESERVING_SUB_OPS.contains(&k),
            "sub-op {k:#04x}"
        );
        // A menu press opens no submode screen, and nothing else is both.
        if op49_sub_op_is_menu_press(k) {
            assert_eq!(slot_for_op49_sub_op(k), None);
        }
    }
}

#[test]
fn dedicated_sub_ops_keep_their_own_paths() {
    for s in OP49_DEDICATED_SUB_OPS {
        assert_eq!(slot_for_op49_sub_op(s), None);
    }
    // Everything else takes the handler retail's `0x801F33A4` table names.
    assert_eq!(slot_for_op49_sub_op(9), Some(slot::PROMPT));
    assert_eq!(slot_for_op49_sub_op(6), Some(slot::COIN_COUNTER));
    // A `-1` row with no dedicated path is a scripted menu press: the
    // enter half stores handler `7` before the table read
    // (`0x801F140C`), so no submode screen opens and the park stands for
    // the pause menu (`World::scripted_menu_open_pending`).
    assert_eq!(slot_for_op49_sub_op(1), None);
    assert_eq!(slot_for_op49_sub_op(0x0D), None);
}

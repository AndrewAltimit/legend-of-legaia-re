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
    // The opening script's operand names the sale flag (`49 03 25 <flag>`).
    w.field_vm.submode_screen.op49_operand = [0x03, 0x25, 0x40, 0x00, 0, 0, 0];
    w.system_flag_set(0x40);

    // Frame 1: state 0 seeds the screen, raising the coin-bank flag and
    // clearing the sale flag.
    w.tick_submode_screen(1);
    assert!(w.system_flag_test(8));
    assert!(!w.system_flag_test(0x40), "the counter opens with no sale");
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
    assert!(w.system_flag_test(0x40), "a sale raises the script's flag");
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
    // The reload is the executable's: the title's New Game (and its card
    // load, `resume_card_load`) starts with no screen up, so the next
    // credits' `49 0C` opens a fresh one instead of reading this one, stuck
    // past its reload, as its own.
    w.begin_new_game();
    assert!(!w.field_vm.submode_screen.is_open());
    assert!(
        w.soft_reset_records_pen().is_none(),
        "no records over the field"
    );
    assert_eq!(w.field_vm.submode_screen.soft_reset, Default::default());
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

/// A record the anti-hang cap ends takes the screen it opened with it, and
/// only its own: the next record to reach an op `0x49` finds Idle and opens
/// a fresh screen instead of parking on the stale one.
#[test]
fn a_capped_record_takes_its_op49_screen_with_it() {
    let mut w = world_with_driver();
    w.open_coin_counter();
    w.field_vm.submode_screen.owner = Op49ParkOwner::CutsceneTimeline;
    w.field_vm.submode_screen.park_sub_op = Some(6);
    w.drop_capped_op49_screen(Op49ParkOwner::HelperContext);
    assert!(
        w.field_vm.submode_screen.is_open(),
        "another owner's screen stays"
    );
    w.drop_capped_op49_screen(Op49ParkOwner::CutsceneTimeline);
    let s = &w.field_vm.submode_screen;
    assert!(!s.is_open() && !s.is_done() && s.park_sub_op.is_none());
    assert!(!s.is_open_for(Op49ParkOwner::CutsceneTimeline));
}

/// Issue #603: `49 09 00 <char> <art>` (Drake Castle's book, every master's
/// grant) opens the prompt screen, whose window record `9` names the
/// character and the art (`FUN_801E733C`). The record had no painter, so the
/// screen drew nothing; the notice now resolves its two lines from the
/// parked payload, the `0xC1` operand patched to the character and the
/// `0xC5` operand to `art + char * 0x40`.
#[test]
fn the_art_learned_prompt_names_the_character_and_the_art() {
    use legaia_art::Character;
    let mut w = world_with_driver();
    w.party.party_names = vec!["Vahn".into(), "Noa".into(), "Gala".into()];
    w.menu.text = Some(legaia_engine_menus::pause_screens::MenuTextTables {
        arts: Some(vec![
            art_row(Character::Vahn, 9, b"Wrong Row"),
            art_row(Character::Noa, 9, b"Test Dive"),
        ]),
        ..Default::default()
    });
    assert_eq!(w.art_notice_shown(), None);
    let instr = [0x49, 0x09, 0x00, 0x01, 0x09];
    w.open_field_submode_screen(slot::PROMPT, None);
    w.set_submode_board_entries(&instr);
    w.tick_submode_screen(1);
    assert_eq!(w.art_notice_shown(), Some((1, 9)));
    // Synthetic stand-ins for the two overlay strings, in their shape: a
    // name token, then an inked arts-name token.
    let read = |va: u32| match va {
        0x801F_2B5C => vec![0xC1, 0x00, b' ', b'u', b's', b'e', b's', 0x00, b'x'],
        0x801F_2B6C => vec![b'[', 0xCF, 0x06, 0xC5, 0x00, 0xCF, 0x07, b']', 0x00, b'x'],
        _ => Vec::new(),
    };
    let [first, second] = w.art_notice_lines(read).expect("notice lines");
    assert_eq!(first, b"Noa uses".to_vec());
    assert_eq!(second, b"[\xCF\x06Test Dive\xCF\x07]".to_vec());

    // The confirm hands the screen back and the notice goes with it.
    w.input.set_pad(legaia_engine_system::pad::retail_packed(
        SUBMODE_ACCEPT_MASK as u16,
    ));
    for _ in 0..4 {
        w.tick_submode_screen(1);
    }
    assert_eq!(w.art_notice_shown(), None);
}

/// The token patch steps over each `0xCx` pair's operand (`FUN_8003CBF8`),
/// so an operand byte equal to the token is not mistaken for it.
#[test]
fn the_notice_token_patch_skips_escape_operands() {
    let mut s = vec![0xCF, 0xC5, 0xC5, 0x00, 0x00];
    patch_token_operand(&mut s, 0xC5, 0x49);
    assert_eq!(s, vec![0xCF, 0xC5, 0xC5, 0x49, 0x00]);
    let mut none = vec![b'a', 0x00];
    patch_token_operand(&mut none, 0xC1, 1);
    assert_eq!(none, vec![b'a', 0x00]);
    // A zero operand is not the terminator.
    assert_eq!(
        token_cstr(&[0xC1, 0x00, b'a', 0x00, b'b']),
        vec![0xC1, 0x00, b'a']
    );
}

/// A synthetic arts-name table row (not disc text).
fn art_row(
    character: legaia_art::Character,
    index: u8,
    name: &[u8],
) -> legaia_art::arts_table::ArtTableEntry {
    legaia_art::arts_table::ArtTableEntry {
        character,
        index,
        name: String::from_utf8_lossy(name).into_owned(),
        ap: 24,
        commands: Vec::new(),
        is_miracle: false,
        name_bytes: name.to_vec(),
        glyphs: Vec::new(),
    }
}

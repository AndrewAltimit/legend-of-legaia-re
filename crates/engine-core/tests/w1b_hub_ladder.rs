//! Hub ladder: the op-`0x49` **submode screens**, opened by a field script and
//! played by pad through `World::tick`.
//!
//! ## Why this instrument exists
//!
//! `field_submode_screen_wiring.rs` proves the dispatcher runs inside the frame
//! loop, and `hub_entry_sub_panel_*.rs` prove one painter's rows. Both open
//! their screen by calling `World::open_field_submode_screen` - the engine's
//! own entry, which no script ever reaches. Under that denominator "the hub
//! screens work" and "no script can open one" are the same green suite, and
//! for most of the family the second was true: the engine had no mapping from
//! a sub-op to a handler slot, so `slot_for_op49_sub_op` answered
//! `CLOSE_TICK` for all eleven non-dedicated sub-ops. Every screen a script
//! asked for closed itself on its first frame. The park cleared, so nothing
//! hung - and nothing drew either.
//!
//! This ladder is denominated in the **script**: every rung starts from a
//! field-VM instruction, steps the ported VM through `World::tick`, and reads
//! only what the frame loop published.
//!
//! | # | rung | what it proves |
//! |---|---|---|
//! | 1 | a `49 <sub_op>` opens the handler retail's own table names | the sub-op -> slot mapping is wired, not a placeholder |
//! | 2 | the opened handler's state machine runs inside `World::tick` | the dispatcher reaches `PTR_FUN_801F33B4[+0x50]` |
//! | 3 | the panel a state machine installs selects the painter | the descriptor names the window, not the caller |
//! | 4 | a pad confirm hands the screen back and unparks the script | the op-`0x49` Done writer is the dispatcher |
//! | 5 | the coin counter, entered digit by digit on the pad, moves gold | the screen is a transaction, not a picture |
//!
//! A rung that does not clear is reported with the sub-op and the reason,
//! following `critical_path_replay`'s contract.
//!
//! ## What this ladder cannot reach, and why
//!
//! Five painters are installed by handler slots with no ported body -
//! `0x20` (`FUN_801EE90C`), `0x21` (`FUN_801EED58`), `0x31` (`FUN_801ED590`)
//! and one of `FUN_801E9B3C`'s own descriptor-op handlers. Their screens open
//! (the slot is named and the driver runs) and produce nothing, which is
//! exactly what an unported slot should do. They are driven here through the
//! host-pinned window instead, and that is marked as such rather than counted
//! as a script-reached rung.
//!
//! Disc-free: every table this needs is committed as a constant, and
//! `w1b_hub_tables_disc.rs` is what pins those constants to the disc.

use legaia_engine_core::actor_handler::ActorHandler;
use legaia_engine_core::field_submode_screen::{
    SUBMODE_ACCEPT_MASK, SUBMODE_BACK_MASK, slot_for_op49_sub_op,
};
use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::baka_hub_actors::{
    self as hub, GOLD_PER_COIN, HubAction, HubDraw, HubPainter, PAD_CURSOR_UP, PICK_ACCEPT, slot,
    window,
};

/// A field world with the submode driver the MAN loader spawns, in the mode
/// whose arm steps the field VM.
fn field_world() -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.man_load_actor_reset();
    assert!(
        w.find_actor_by_handler(ActorHandler::SubmodeDriver)
            .is_some(),
        "every MAN load spawns the op-0x49 submode driver (FUN_801D9C3C)"
    );
    w
}

/// The retail instruction shape: `[0x49][sub_op][payload x3]`, then a second
/// copy so the resume has somewhere to land.
fn op49_script(sub_op: u8, entries: [u8; 3]) -> Vec<u8> {
    let mut out = vec![0x49, sub_op, entries[0], entries[1], entries[2]];
    out.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00]);
    out
}

/// One world frame with `mask` held.
fn step(w: &mut World, mask: u16) {
    w.input.set_pad(mask);
    let _ = w.tick();
}

/// Press-then-release: two frames, one edge.
fn press(w: &mut World, mask: u16) {
    step(w, mask);
    step(w, 0);
}

/// Tick idle until `done`, or give up.
fn tick_until(w: &mut World, limit: usize, mut done: impl FnMut(&World) -> bool) -> bool {
    for _ in 0..limit {
        step(w, 0);
        if done(w) {
            return true;
        }
    }
    false
}

/// Open a screen the way a script does: load the instruction, let the VM arm,
/// and let the dispatcher run its first pass.
///
/// The two are separate frames and the order is retail's: the actor pool runs
/// before the field step inside one tick, so a screen armed at tick `n` is
/// first dispatched at `n + 1` (or later - the pool is on the game-tick clock,
/// which is every second vsync under the field cadence floor).
fn script_opens(sub_op: u8, entries: [u8; 3]) -> World {
    let mut w = field_world();
    w.load_field_script(op49_script(sub_op, entries));
    assert!(
        tick_until(&mut w, 8, |w| w.submode_screen.is_open()),
        "sub-op {sub_op:#x}: the field VM never armed a submode screen"
    );
    assert!(
        tick_until(&mut w, 8, |w| !w.submode_screen.frame.actions.is_empty()),
        "sub-op {sub_op:#x}: the dispatcher never ran the opened handler"
    );
    w
}

// ---------------------------------------------------------------------------
// Rung 1 - the sub-op selects retail's handler
// ---------------------------------------------------------------------------

/// Every sub-op the retail table names a handler for opens **that** handler on
/// the driver actor. The pre-wiring engine opened slot `0` for all of them.
#[test]
fn rung1_a_script_sub_op_opens_the_handler_retail_names() {
    let cases = [
        (6u8, slot::COIN_COUNTER),
        (8, slot::START_MENU),
        (9, slot::PROMPT),
        (0xB, slot::SUBMENU),
    ];
    for (sub_op, want) in cases {
        let w = script_opens(sub_op, [0, 0, 0]);
        assert_eq!(
            w.submode_screen.actor.state, want,
            "sub-op {sub_op:#x} opened handler {:#x}, not {want:#x}",
            w.submode_screen.actor.state
        );
        assert_eq!(
            slot_for_op49_sub_op(sub_op),
            Some(want),
            "the mapping and the opened screen must agree"
        );
    }
}

// ---------------------------------------------------------------------------
// Rung 2 - the state machine runs inside the frame loop
// ---------------------------------------------------------------------------

/// Each opened handler's own state machine produces its retail first-frame
/// output. This is the rung that separates "the slot was named" from "the
/// body ran": a slot with no ported arm reaches `run_slot`'s fall-through and
/// emits nothing but the dispatcher's own re-arm.
#[test]
fn rung2_the_named_state_machine_runs_and_emits_its_first_frame() {
    // The start menu sizes and installs its panel; the prompt fires the entry
    // sting; the sub-menu clears the cursor row; the coin counter publishes
    // the clamped buyable amount.
    let w = script_opens(8, [1, 1, 0]);
    let acts = || w.submode_screen.frame.actions.clone();
    assert!(
        acts()
            .iter()
            .any(|a| matches!(a, HubAction::SizePanel { .. })),
        "the start menu never sized its panel: {:?}",
        acts()
    );
    // Three active entries plus the base row: `n * 14 - 2` tall.
    let sized = acts()
        .into_iter()
        .find_map(|a| match a {
            HubAction::SizePanel { height, top } => Some((height, top)),
            _ => None,
        })
        .expect("sized");
    assert_eq!(
        sized,
        (3 * 14 - 2, 0x2C - ((3 * 14 - 2) >> 1)),
        "the panel height counts the operand's payload bytes"
    );

    let w = script_opens(9, [0, 0, 0]);
    assert!(
        w.submode_screen
            .frame
            .actions
            .iter()
            .any(|a| matches!(a, HubAction::EntryCue(_))),
        "the prompt never played its entry sting"
    );

    let w = script_opens(0xB, [0, 0, 0]);
    assert!(
        w.submode_screen
            .frame
            .actions
            .contains(&HubAction::ClearCursorRow),
        "the sub-menu never cleared the cursor row"
    );

    let mut w = field_world();
    w.money = 12_345;
    w.load_field_script(op49_script(6, [0, 0, 0]));
    assert!(tick_until(&mut w, 8, |w| w.submode_screen.is_open()));
    assert!(tick_until(&mut w, 8, |w| !w
        .submode_screen
        .frame
        .actions
        .is_empty()));
    assert!(
        w.submode_screen
            .frame
            .actions
            .contains(&HubAction::SetCoinAmount(12_345 / GOLD_PER_COIN)),
        "the coin counter never published its clamped amount: {:?}",
        w.submode_screen.frame.actions
    );
}

// ---------------------------------------------------------------------------
// Rung 3 - the descriptor names the painter
// ---------------------------------------------------------------------------

/// The painter follows the panel the state machine installs. Two of the seven
/// ported painters are reachable this way, and both are reached from a script.
#[test]
fn rung3_the_installed_descriptor_selects_the_painter() {
    // The sub-menu's idle panel is the single-label record.
    let w = script_opens(0xB, [0, 0, 0]);
    assert_eq!(
        w.submode_screen.installed_windows,
        vec![window::SINGLE_LABEL],
        "the sub-menu installed {:?}",
        w.submode_screen.installed_windows
    );
    assert!(
        w.submode_screen
            .draws()
            .iter()
            .any(|d| matches!(d, HubDraw::Text { .. } | HubDraw::ShortText { .. })),
        "the single-label painter drew nothing: {:?}",
        w.submode_screen.draws()
    );

    // The coin counter's confirm panel is the three-line record. Reached with
    // the pad alone: Up bumps the selected digit, Cross accepts.
    let mut w = field_world();
    w.money = 100_000;
    w.load_field_script(op49_script(6, [0, 0, 0]));
    assert!(tick_until(&mut w, 8, |w| w.submode_screen.actor.sub == 1));
    press(&mut w, PAD_CURSOR_UP as u16);
    assert_eq!(
        w.submode_screen.counter.entered(),
        1,
        "a pad Up edge never reached the digit cells"
    );
    press(&mut w, SUBMODE_ACCEPT_MASK as u16);
    assert_eq!(
        w.submode_screen.actor.sub, 2,
        "the accept never opened the confirm"
    );
    assert_eq!(
        w.submode_screen.installed_windows,
        vec![window::THREE_LINE],
        "the coin confirm installed {:?}",
        w.submode_screen.installed_windows
    );
    assert!(
        !w.submode_screen.draws().is_empty(),
        "the three-line painter drew nothing"
    );
}

// ---------------------------------------------------------------------------
// Rung 4 - the hand-back unparks the script
// ---------------------------------------------------------------------------

/// A confirm hands the screen back, the dispatcher retires the driver, and the
/// parked op-`0x49` resumes - the whole point of the family. The PC has to
/// move: a screen that never hands back leaves the script on the same
/// instruction for the rest of the scene.
#[test]
fn rung4_a_confirm_hands_back_and_the_script_resumes() {
    for sub_op in [8u8, 9] {
        let mut w = script_opens(sub_op, [0, 0, 0]);
        let parked_pc = w.field_pc;
        assert_eq!(parked_pc, 0, "the script parks on its own instruction");

        // The confirm edge; then the hand-back's draw tick clears the gate.
        press(&mut w, SUBMODE_ACCEPT_MASK as u16);
        assert!(
            tick_until(&mut w, 16, |w| !w.submode_screen.is_open()),
            "sub-op {sub_op:#x}: the screen never handed back"
        );
        assert_eq!(
            w.submode_screen.actor.state,
            slot::DRAW_TICK,
            "a hand-back re-arms `+0x50` to the draw tick"
        );
        assert!(
            w.find_actor_by_handler(ActorHandler::SubmodeDriver)
                .is_none(),
            "the dispatcher retires the driver on hand-back"
        );
        assert!(
            tick_until(&mut w, 8, |w| w.field_pc != parked_pc),
            "sub-op {sub_op:#x}: the script never resumed past its op-0x49"
        );
    }
}

// ---------------------------------------------------------------------------
// Rung 5 - the coin counter is a transaction
// ---------------------------------------------------------------------------

/// Digits entered on the pad, confirmed, committed: gold leaves the party and
/// coins land in the casino bank at [`GOLD_PER_COIN`] each.
#[test]
fn rung5_pad_entered_digits_buy_coins_through_the_script_opened_screen() {
    let mut w = field_world();
    w.money = 5_000;
    w.casino_coins = 7;
    w.load_field_script(op49_script(6, [0, 0, 0]));
    assert!(tick_until(&mut w, 8, |w| w.submode_screen.actor.sub == 1));

    // Three Up edges on the units cell: three coins.
    for _ in 0..3 {
        press(&mut w, PAD_CURSOR_UP as u16);
    }
    assert_eq!(w.submode_screen.counter.entered(), 3);

    press(&mut w, SUBMODE_ACCEPT_MASK as u16);
    assert_eq!(w.submode_screen.actor.sub, 2, "the Yes/No panel is up");

    // `FUN_801E9DC8`'s return is the one value the port has no body for; the
    // host supplies it, as the module documents.
    w.submode_screen.picker_result = PICK_ACCEPT;
    w.submode_screen.counter.yes_no = 0;
    assert!(tick_until(&mut w, 16, |w| w.casino_coins != 7));

    assert_eq!(w.casino_coins, 7 + 3, "coins land in the casino bank");
    assert_eq!(
        w.money,
        5_000 - 3 * GOLD_PER_COIN,
        "gold pays {GOLD_PER_COIN} per coin"
    );

    // And the screen still closes itself afterwards, so the script resumes.
    // `is_done` is one-shot - the resuming op-`0x49` consumes it - so the
    // durable observation is that the screen closed and the PC moved.
    assert!(
        tick_until(&mut w, 256, |w| !w.submode_screen.is_open()),
        "the committed counter never handed back"
    );
    assert!(
        tick_until(&mut w, 16, |w| w.field_pc != 0),
        "the script never resumed past its op-0x49"
    );
}

// ---------------------------------------------------------------------------
// The painters no ported handler slot installs
// ---------------------------------------------------------------------------

/// The five painters retail reaches through handler slots `0x20` / `0x21` /
/// `0x31` and the installer's own descriptor op. Nothing here claims a script
/// reaches them - the window is pinned by the host, which is the seam the
/// engine keeps for exactly this case. What it does assert is that each one
/// draws when the frame loop paints it, so the bodies are live rather than
/// merely compiled.
#[test]
fn the_host_pinned_windows_paint_inside_the_frame_loop() {
    let cases = [
        (window::TWO_OPTION, HubPainter::TwoOption),
        (window::COUNT_GATED_LABEL, HubPainter::CountGatedLabel),
        (window::ENTRY_LIST, HubPainter::EntryList),
        (window::COLUMN_ROW, HubPainter::ColumnRow),
        (window::TWO_LINE, HubPainter::TwoLine),
    ];
    for (index, painter) in cases {
        assert_eq!(
            HubPainter::for_window(index),
            Some(painter),
            "record {index} selects the wrong painter"
        );
        let mut w = field_world();
        w.active_party = vec![0, 1];
        w.open_field_submode_screen(slot::DRAW_TICK, Some(index));
        assert!(
            tick_until(&mut w, 16, |w| !w.submode_screen.draws().is_empty()),
            "record {index} ({painter:?}) drew nothing in a live frame loop"
        );
    }
}

/// The deactivate handler (`FUN_801F1D90`, slot `0x13`) has **no arm at all**
/// in the field overlay: no immediate `0x13` is ever stored to `+0x50`, and
/// the sub-op table names slots `0x21..=0x33` only. It is reached by restoring
/// a stashed handler id, so the host opens it directly here. Both of its arms
/// are asserted, since the branch is the whole of what it decides.
#[test]
fn the_deactivate_handler_picks_its_re_arm_state_from_the_progress_flags() {
    // Progress clear -> the deactivate state.
    let mut w = field_world();
    w.open_field_submode_screen(slot::DEACTIVATE, None);
    assert!(tick_until(&mut w, 16, |w| w.submode_screen.actor.state
        != slot::DEACTIVATE));
    assert_eq!(w.submode_screen.actor.state, hub::HUB_DEACTIVATE_STATE);
    assert_eq!(w.submode_screen.cursor.handback, -1, "the hand-back is set");

    // The skip arm needs a progress flag, which the world's env does not
    // publish; the kernel's own arm is asserted directly so the branch is
    // covered rather than assumed.
    let mut actor = hub::HubActor {
        state: slot::DEACTIVATE,
        ..Default::default()
    };
    let mut grid = hub::HubGrid::default();
    let env = hub::HubEnv {
        progress_a: 1,
        ..Default::default()
    };
    let f = hub::deactivate(&mut actor, &env, &mut grid);
    assert_eq!(actor.state, hub::HUB_SKIP_STATE);
    assert!(f.actions.contains(&HubAction::CloseCue));
}

/// A back-out is a confirm for the screens whose confirm mask is the OR of
/// both buttons - retail reads `_DAT_800846D0 | _DAT_800846D4` there, so
/// Circle closes the prompt just as Cross does.
#[test]
fn the_back_button_also_hands_a_prompt_screen_back() {
    let mut w = script_opens(9, [0, 0, 0]);
    press(&mut w, SUBMODE_BACK_MASK as u16);
    assert!(
        tick_until(&mut w, 16, |w| !w.submode_screen.is_open()),
        "Circle never handed the prompt back"
    );
    assert!(
        tick_until(&mut w, 16, |w| w.field_pc != 0),
        "the script never resumed after the Circle hand-back"
    );
}

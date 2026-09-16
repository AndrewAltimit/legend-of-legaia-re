//! The five save-UI sub-screen bodies no existing ladder opens:
//! `tick_final_exit`, `tick_pad_release_wait`, `tick_party_picker`,
//! `tick_shop_mode_select` and `tick_quantity_spinner`
//! (`FUN_801DD12C`, `FUN_801DD26C`, `FUN_801D98F0`, `FUN_801DAFD4`,
//! `FUN_801DBC5C`).
//!
//! The union's only driver for `SaveScreenMachine` is the card rack
//! (`save_screen::SaveScreenFlow`), which constructs the machine on the one
//! entry context a card transfer uses and therefore enters three of the eight
//! bodies. The other five sit behind a different **entry context** or behind a
//! transition whose predecessor another module owns - so the gap is the screen
//! the flow opens on, not a pad stream.
//!
//! Three rungs, one per way in, and each asserts the body's own observable
//! product (a screen transition, an exit code, or an effect the host must
//! perform) rather than "the call returned":
//!
//! 1. **The shop chain, from its retail entry context.**
//!    `SaveEntryContext::ShopEntry` is the op-`0x49` record's own kind byte
//!    (`0x00`, `0x801DC89C`), and it opens the machine on
//!    `SaveSubScreen::ShopModeSelect`. From there the Sell row walks to the
//!    quantity spinner and back, and the Quit row walks to the terminal
//!    screen, which exits the flow with `EXIT_CODE_NORMAL`. Three of the five
//!    bodies, reached by pad through the dispatcher.
//! 2. **The pad-release wait**, whose predecessor is a screen this module does
//!    not port. The ladder writes the screen id - retail's own transition is a
//!    screen-id store - and then drives the body by pad: held buttons park it,
//!    a release moves it on.
//! 3. **The party picker**, the same shape, with both of its exits taken.
//!
//! What this does *not* establish: that a host opens a shop through this
//! machine. `SaveScreenFlow` constructs only the card contexts, so the shop
//! and post-save entry contexts stay host-owed; this ladder measures reach,
//! and the wiring question is separate.
//!
//! Disc-free: the machine is a pure state machine over its own input struct.

use legaia_engine_core::save_subscreen::{
    EXIT_CODE_NORMAL, SaveEntryContext, SavePhase, SaveScreenMachine, SaveSubScreen,
    SubScreenEffect, SubScreenInput,
};

/// Fade step per frame - the flow fades in from opaque before the dispatcher
/// runs at all, so every rung pays this first.
const FADE_DELTA: u8 = 0x10;

/// Idle input: no script running, no button, no nav.
fn idle() -> SubScreenInput {
    SubScreenInput {
        script_busy: false,
        ..Default::default()
    }
}

/// Run the machine until its dispatcher is live.
fn fade_in(m: &mut SaveScreenMachine) {
    for _ in 0..64 {
        if m.phase() == SavePhase::Dispatch {
            return;
        }
        m.tick(idle(), FADE_DELTA);
    }
    panic!("the flow never reached its dispatch phase");
}

/// One dispatch frame with `nav` on `cursor`.
fn nav(m: &mut SaveScreenMachine, nav: u8, cursor: u16) -> Vec<SubScreenEffect> {
    m.tick(
        SubScreenInput {
            nav,
            cursor,
            ..idle()
        },
        FADE_DELTA,
    )
}

#[test]
fn shop_entry_context_walks_mode_select_spinner_and_terminal_screen() {
    let mut m = SaveScreenMachine::new(SaveEntryContext::ShopEntry);
    fade_in(&mut m);
    assert_eq!(
        m.screen(),
        SaveSubScreen::ShopModeSelect,
        "the shop entry context opens on the mode select"
    );

    // Step 0 of the mode select clears the staging cells and runs its display
    // script - the two effects retail's `FUN_801DAFD4` raises before it looks
    // at the pad at all.
    let first = m.tick(idle(), FADE_DELTA);
    assert!(first.contains(&SubScreenEffect::ClearStaging));
    assert!(first.contains(&SubScreenEffect::RunScript));

    // Sell (row 1) with nothing sellable buzzes and stays put: retail does not
    // fall through to a transition when the bag walk finds no entry.
    let buzz = m.tick(
        SubScreenInput {
            nav: 1,
            cursor: 1,
            sellable_items_available: false,
            ..idle()
        },
        FADE_DELTA,
    );
    assert_eq!(buzz, vec![SubScreenEffect::Sfx(0x23)]);
    assert_eq!(m.screen(), SaveSubScreen::ShopModeSelect);

    // Sell with a sellable bag advances the screen's own step, then settles on
    // the quantity spinner.
    m.tick(
        SubScreenInput {
            nav: 1,
            cursor: 1,
            sellable_items_available: true,
            ..idle()
        },
        FADE_DELTA,
    );
    m.tick(idle(), FADE_DELTA);
    assert_eq!(
        m.screen(),
        SaveSubScreen::QuantitySpinner,
        "the Sell row is the mode select's only proceeding exit"
    );

    // The spinner: script frame, then the staging read it performs on the
    // frame it settles and on every frame it re-runs.
    let spin0 = m.tick(idle(), FADE_DELTA);
    assert!(spin0.contains(&SubScreenEffect::RunScript));
    let settle = m.tick(idle(), FADE_DELTA);
    assert!(
        settle.contains(&SubScreenEffect::ReadInventoryEntry),
        "the spinner reads the focused inventory entry as it settles"
    );
    // Outcome 3 re-runs the second display script and parks on step 3, which
    // returns to the mode select once the script clears.
    let rerun = m.tick(
        SubScreenInput {
            spinner_result: 3,
            ..idle()
        },
        FADE_DELTA,
    );
    assert!(rerun.contains(&SubScreenEffect::RunScript));
    m.tick(idle(), FADE_DELTA);
    assert_eq!(
        m.screen(),
        SaveSubScreen::ShopModeSelect,
        "the spinner returns to the mode select"
    );

    // Quit (cancel) leaves for the terminal screen, which runs its script and
    // then exits the whole flow with the normal exit code.
    m.tick(idle(), FADE_DELTA);
    let quit = nav(&mut m, 2, 2);
    assert!(quit.is_empty(), "the cancel exit raises no cue of its own");
    assert_eq!(m.screen(), SaveSubScreen::FinalExit);
    let term = m.tick(idle(), FADE_DELTA);
    assert_eq!(term, vec![SubScreenEffect::RunScript]);
    m.tick(idle(), FADE_DELTA);
    assert_eq!(
        m.exit_code(),
        Some(EXIT_CODE_NORMAL),
        "the terminal screen wrote the normal exit code"
    );
    // And the exit code survives the fade-out into the terminal phase.
    for _ in 0..64 {
        if m.is_done() {
            break;
        }
        m.tick(idle(), FADE_DELTA);
    }
    assert!(m.is_done());
    assert_eq!(m.exit_code(), Some(EXIT_CODE_NORMAL));
}

#[test]
fn pad_release_wait_parks_while_held_and_moves_on_when_released() {
    let mut m = SaveScreenMachine::new(SaveEntryContext::ScriptSave);
    fade_in(&mut m);
    m.goto(SaveSubScreen::PadReleaseWait);

    let first = m.tick(idle(), FADE_DELTA);
    assert_eq!(first, vec![SubScreenEffect::RunScript]);

    // Held: the wait is for a release, so nothing moves however many frames
    // the button stays down.
    for _ in 0..8 {
        m.tick(
            SubScreenInput {
                any_button_held: true,
                ..idle()
            },
            FADE_DELTA,
        );
        assert_eq!(m.screen(), SaveSubScreen::PadReleaseWait);
    }
    // Released: on to the screen the transition names.
    m.tick(idle(), FADE_DELTA);
    assert_eq!(m.screen(), SaveSubScreen::Routed(0x05));
    assert_eq!(m.step(), 0, "a screen change resets the step counter");
}

#[test]
fn party_picker_takes_both_of_its_exits() {
    // Confirm -> the routed screen, with the confirm cue.
    let mut m = SaveScreenMachine::new(SaveEntryContext::ScriptSave);
    fade_in(&mut m);
    m.goto(SaveSubScreen::PartyPicker);
    assert_eq!(m.tick(idle(), FADE_DELTA), vec![SubScreenEffect::RunScript]);
    let confirm = nav(&mut m, 1, 0);
    assert_eq!(confirm, vec![SubScreenEffect::Sfx(0x20)]);
    assert_eq!(m.screen(), SaveSubScreen::Routed(0x13));

    // Cancel -> the slot selector, silently.
    let mut m = SaveScreenMachine::new(SaveEntryContext::ScriptSave);
    fade_in(&mut m);
    m.goto(SaveSubScreen::PartyPicker);
    m.tick(idle(), FADE_DELTA);
    let cancel = nav(&mut m, 2, 0);
    assert!(cancel.is_empty());
    assert_eq!(m.screen(), SaveSubScreen::SlotSelect);
}

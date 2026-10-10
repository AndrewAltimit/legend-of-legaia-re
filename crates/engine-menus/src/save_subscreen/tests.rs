use super::*;

fn idle() -> SubScreenInput {
    SubScreenInput::default()
}

/// Every id the table indexes round-trips, including the ones with no
/// step machine - the enum has to keep the space total or a retail
/// transition into an unpinned screen would be inexpressible.
#[test]
fn subscreen_ids_round_trip() {
    for id in 0u8..=0x20 {
        assert_eq!(SaveSubScreen::from_id(id).id(), id, "id {id:#04x}");
    }
    assert!(!SaveSubScreen::from_id(0x05).is_pinned());
    assert!(SaveSubScreen::from_id(0x18).is_pinned());
}

/// The entry context picks the opening screen, which is what makes
/// the same overlay serve Load, Save and auto-save.
#[test]
fn entry_context_picks_the_start_screen() {
    assert_eq!(
        SaveEntryContext::DebugParamEditor.start_screen(),
        SaveSubScreen::CharParamEditor
    );
    assert_eq!(
        SaveEntryContext::ScriptSave.start_screen(),
        SaveSubScreen::CardSave
    );
    assert_eq!(
        SaveEntryContext::CasinoPrizeCounter.start_screen(),
        SaveSubScreen::CasinoPrizeConfirm
    );
    assert_eq!(
        SaveEntryContext::PostSave.start_screen(),
        SaveSubScreen::PostSaveReturn
    );
}

/// Input is gated on the fade, so a machine that has only just opened
/// must not dispatch pad reads.
#[test]
fn fade_gates_input_then_dispatch_starts() {
    let mut m = SaveScreenMachine::new(SaveEntryContext::DebugParamEditor);
    m.tick(idle(), 0x10); // Init
    assert_eq!(m.phase(), SavePhase::FadeIn);
    assert!(!m.input_active());
    // Fade down past the threshold.
    while m.phase() == SavePhase::FadeIn {
        m.tick(idle(), 0x10);
    }
    assert_eq!(m.phase(), SavePhase::Dispatch);
    assert!(m.input_active());
}

fn dispatching(entry: SaveEntryContext) -> SaveScreenMachine {
    let mut m = SaveScreenMachine::new(entry);
    while m.phase() != SavePhase::Dispatch {
        m.tick(idle(), 0x40);
    }
    m
}

/// The terminal screen runs its script, waits for it, then ends the
/// flow with the normal exit code and a re-opaqued fade.
#[test]
fn final_exit_runs_script_then_exits() {
    let mut m = dispatching(SaveEntryContext::DebugParamEditor);
    m.goto(SaveSubScreen::FinalExit);

    let fx = m.tick(idle(), 0);
    assert_eq!(fx, vec![SubScreenEffect::RunScript]);
    assert_eq!(m.step(), 1);

    // Still busy: nothing happens.
    m.tick(
        SubScreenInput {
            script_busy: true,
            ..idle()
        },
        0,
    );
    assert!(m.exit_code().is_none());

    m.tick(idle(), 0);
    assert_eq!(m.exit_code(), Some(EXIT_CODE_NORMAL));
    // The screen writes the fade *delta*, so the level is unchanged
    // at the point the exit lands; the fade-out phase is what walks
    // it back up to opaque.
    assert!(m.fade() < FADE_DISPATCH_THRESHOLD);
    assert_eq!(m.phase(), SavePhase::FadeOut);
}

/// Confirming on the default row returns to the slot selector;
/// confirming on the other row falls through to the exit screen.
/// Retail writes the exit first and overwrites it, so getting this
/// backwards is the easy mistake.
#[test]
fn confirm_yes_no_default_row_returns_to_the_selector() {
    for (cursor, expected) in [
        (1u16, SaveSubScreen::SlotSelect),
        (0, SaveSubScreen::FinalExit),
    ] {
        let mut m = dispatching(SaveEntryContext::DebugParamEditor);
        m.goto(SaveSubScreen::ConfirmYesNo);
        m.tick(idle(), 0);
        let fx = m.tick(
            SubScreenInput {
                nav: 1,
                cursor,
                ..idle()
            },
            0,
        );
        assert_eq!(m.screen(), expected, "cursor {cursor}");
        assert_eq!(fx, vec![SubScreenEffect::Sfx(0x20)]);
    }
}

/// Cancelling the confirm goes back to the selector, the same place
/// the default row goes.
#[test]
fn confirm_yes_no_cancel_returns_to_the_selector() {
    let mut m = dispatching(SaveEntryContext::DebugParamEditor);
    m.goto(SaveSubScreen::ConfirmYesNo);
    m.tick(idle(), 0);
    m.tick(SubScreenInput { nav: 2, ..idle() }, 0);
    assert_eq!(m.screen(), SaveSubScreen::SlotSelect);
}

/// The two pad-wait screens are mirror images: one needs a button
/// down, the other needs the pad clear.
#[test]
fn pad_wait_screens_mirror_each_other() {
    let mut press = dispatching(SaveEntryContext::PostSave);
    press.tick(idle(), 0);
    // Pad clear: the press-wait screen stays put.
    press.tick(idle(), 0);
    assert_eq!(press.screen(), SaveSubScreen::PostSaveReturn);
    press.tick(
        SubScreenInput {
            any_button_held: true,
            ..idle()
        },
        0,
    );
    assert_eq!(press.screen(), SaveSubScreen::SlotSelect);

    let mut release = dispatching(SaveEntryContext::DebugParamEditor);
    release.goto(SaveSubScreen::PadReleaseWait);
    release.tick(idle(), 0);
    // Button held: the release-wait screen stays put.
    release.tick(
        SubScreenInput {
            any_button_held: true,
            ..idle()
        },
        0,
    );
    assert_eq!(release.screen(), SaveSubScreen::PadReleaseWait);
    release.tick(idle(), 0);
    assert_eq!(release.screen(), SaveSubScreen::Routed(0x05));
}

/// The Yes branch of the confirm-exit screen does not leave: it runs
/// a second script and exits with the confirmed code once that
/// settles.
#[test]
fn confirm_exit_yes_branch_runs_a_second_script() {
    let mut m = dispatching(SaveEntryContext::DebugParamEditor);
    m.goto(SaveSubScreen::ConfirmExit);
    m.tick(idle(), 0);

    let fx = m.tick(
        SubScreenInput {
            nav: 1,
            cursor: 0,
            ..idle()
        },
        0,
    );
    assert!(fx.contains(&SubScreenEffect::RunScript));
    assert!(fx.contains(&SubScreenEffect::Sfx(0x88)));
    assert_eq!(m.step(), 2);
    assert_eq!(m.screen(), SaveSubScreen::ConfirmExit);

    m.tick(idle(), 0);
    assert_eq!(m.exit_code(), Some(EXIT_CODE_CONFIRMED));
}

/// The No branch leaves for a different screen than the Yes branch's
/// exit, and cancel goes the same way No does.
#[test]
fn confirm_exit_no_and_cancel_leave() {
    for nav in [1u8, 2] {
        let mut m = dispatching(SaveEntryContext::DebugParamEditor);
        m.goto(SaveSubScreen::ConfirmExit);
        m.tick(idle(), 0);
        m.tick(
            SubScreenInput {
                nav,
                cursor: 1,
                ..idle()
            },
            0,
        );
        assert_eq!(m.screen(), SaveSubScreen::Routed(0x06), "nav {nav}");
        assert!(m.exit_code().is_none());
    }
}

/// Both card drivers are the same machine; only the op differs, and
/// both land back on the slot selector.
#[test]
fn card_drivers_differ_only_in_direction() {
    for (screen, op) in [
        (SaveSubScreen::CardSave, CardOp::Save),
        (SaveSubScreen::CardLoad, CardOp::Load),
    ] {
        let mut m = dispatching(SaveEntryContext::DebugParamEditor);
        m.goto(screen);

        let fx = m.tick(idle(), 0);
        assert!(fx.contains(&SubScreenEffect::InstallCardHandle));
        assert!(fx.contains(&SubScreenEffect::RunScript));

        m.tick(idle(), 0); // step 1 -> 2 (script idle)
        assert_eq!(m.step(), 2);

        // Transfer in flight: the op keeps being driven.
        let fx = m.tick(idle(), 0);
        assert_eq!(fx, vec![SubScreenEffect::CardOp(op)]);
        assert_eq!(m.step(), 2);

        m.tick(
            SubScreenInput {
                card_done: true,
                ..idle()
            },
            0,
        );
        assert_eq!(m.step(), 3);
        m.tick(idle(), 0);
        assert_eq!(m.screen(), SaveSubScreen::SlotSelect);
    }
}

/// The shop mode select's three rows do not share an exit: only row 1
/// (Sell) can proceed, row 0 (Buy) opens the buy list `0x1B`, row 2
/// (Quit) leaves.
#[test]
fn shop_mode_select_rows_have_distinct_exits() {
    let cases = [
        (0u16, SaveSubScreen::Routed(0x1B)),
        (2, SaveSubScreen::FinalExit),
    ];
    for (cursor, expected) in cases {
        let mut m = dispatching(SaveEntryContext::DebugParamEditor);
        m.goto(SaveSubScreen::ShopModeSelect);
        m.tick(idle(), 0);
        m.tick(
            SubScreenInput {
                nav: 1,
                cursor,
                ..idle()
            },
            0,
        );
        assert_eq!(m.screen(), expected, "cursor {cursor}");
    }
}

/// Row 1 proceeds only when the scan found a usable save block;
/// without one it plays the error cue and stays put rather than
/// falling through to a transition.
#[test]
fn shop_mode_select_row_one_needs_a_sellable_bag() {
    let mut blocked = dispatching(SaveEntryContext::DebugParamEditor);
    blocked.goto(SaveSubScreen::ShopModeSelect);
    blocked.tick(idle(), 0);
    let fx = blocked.tick(
        SubScreenInput {
            nav: 1,
            cursor: 1,
            sellable_items_available: false,
            ..idle()
        },
        0,
    );
    assert_eq!(fx, vec![SubScreenEffect::Sfx(0x23)]);
    assert_eq!(blocked.screen(), SaveSubScreen::ShopModeSelect);
    assert_eq!(blocked.step(), 1);

    let mut ok = dispatching(SaveEntryContext::DebugParamEditor);
    ok.goto(SaveSubScreen::ShopModeSelect);
    ok.tick(idle(), 0);
    ok.tick(
        SubScreenInput {
            nav: 1,
            cursor: 1,
            sellable_items_available: true,
            ..idle()
        },
        0,
    );
    assert_eq!(ok.step(), 2);
    ok.tick(idle(), 0);
    assert_eq!(ok.screen(), SaveSubScreen::QuantitySpinner);
}

/// The spinner reads the focused inventory entry once it settles, and
/// its result selector picks between committing and re-running.
#[test]
fn quantity_spinner_commits_on_result_two() {
    let mut m = dispatching(SaveEntryContext::DebugParamEditor);
    m.goto(SaveSubScreen::QuantitySpinner);
    m.tick(idle(), 0);

    let fx = m.tick(
        SubScreenInput {
            spinner_result: 2,
            ..idle()
        },
        0,
    );
    assert!(fx.contains(&SubScreenEffect::ReadInventoryEntry));
    assert_eq!(m.screen(), SaveSubScreen::Routed(0x1F));
}

/// Result `3` re-runs the second display script and parks on the
/// wait step rather than leaving the screen.
#[test]
fn quantity_spinner_rerun_parks_on_the_wait_step() {
    let mut m = dispatching(SaveEntryContext::DebugParamEditor);
    m.goto(SaveSubScreen::QuantitySpinner);
    m.tick(idle(), 0);

    let fx = m.tick(
        SubScreenInput {
            spinner_result: 3,
            ..idle()
        },
        0,
    );
    assert!(fx.contains(&SubScreenEffect::RunScript));
    assert_eq!(m.step(), 3);
    assert_eq!(m.screen(), SaveSubScreen::QuantitySpinner);

    m.tick(idle(), 0);
    assert_eq!(m.screen(), SaveSubScreen::ShopModeSelect);
}

/// A full script-save flow: the card driver runs, lands on the
/// selector, and the confirm's default row keeps the player there.
#[test]
fn script_save_flow_walks_card_driver_into_the_selector() {
    let mut m = dispatching(SaveEntryContext::ScriptSave);
    assert_eq!(m.screen(), SaveSubScreen::CardSave);

    m.tick(idle(), 0); // install + script
    m.tick(idle(), 0); // script idle
    m.tick(
        SubScreenInput {
            card_done: true,
            ..idle()
        },
        0,
    );
    m.tick(idle(), 0);
    assert_eq!(m.screen(), SaveSubScreen::SlotSelect);
    assert!(!m.is_done());
}

/// The flow only terminates after the fade-out finishes, so an exit
/// code alone does not mean the UI is gone.
#[test]
fn exit_code_still_waits_for_the_fade_out() {
    let mut m = dispatching(SaveEntryContext::DebugParamEditor);
    m.goto(SaveSubScreen::FinalExit);
    m.tick(idle(), 0);
    m.tick(idle(), 0);
    assert_eq!(m.phase(), SavePhase::FadeOut);
    assert!(!m.is_done());

    while !m.is_done() {
        m.tick(idle(), 0x20);
    }
    // The fade-out ends opaque, not transparent - it is the reverse
    // of the fade-in, and retail's terminal state is reached by the
    // level climbing back to `0xF2`.
    assert_eq!(m.fade(), FADE_OPAQUE);
}

/// A shop entry context opens on the shop's mode select, and that
/// screen must be the *dispatchable* variant. Naming it as an
/// unpinned id would round-trip the number while comparing unequal
/// to `ShopModeSelect`, so the dispatcher would park on a screen this
/// module implements.
#[test]
fn shop_context_opens_a_dispatchable_mode_select() {
    let start = SaveEntryContext::ShopEntry.start_screen();
    assert_eq!(start, SaveSubScreen::ShopModeSelect);
    assert_eq!(start.id(), 0x1A);
    assert_eq!(start, SaveSubScreen::from_id(0x1A));
    assert!(start.is_pinned());

    // It really dispatches: step 0 stages and runs the display
    // script rather than returning nothing.
    let mut m = dispatching(SaveEntryContext::ShopEntry);
    let effects = m.tick(idle(), 0);
    assert!(effects.contains(&SubScreenEffect::RunScript), "{effects:?}");
}

/// Retail uses two distinct fade constants: pad input is unmasked
/// from `0x79` down, but the fade-in only hands over to dispatch
/// below `0x79`. Collapsing them into one would let dispatch start a
/// frame early.
#[test]
fn input_unmasks_one_level_before_dispatch_begins() {
    assert_eq!(FADE_INPUT_THRESHOLD, 0x7A);
    assert_eq!(FADE_DISPATCH_THRESHOLD, 0x79);

    let mut m = SaveScreenMachine::new(SaveEntryContext::DebugParamEditor);
    m.tick(idle(), 0); // Init
    // Land exactly on 0x79: input is already live, dispatch is not.
    while m.fade() > 0x79 {
        m.tick(idle(), 1);
    }
    assert_eq!(m.fade(), 0x79);
    assert!(m.input_active());
    assert_eq!(m.phase(), SavePhase::FadeIn);

    m.tick(idle(), 1);
    assert_eq!(m.phase(), SavePhase::Dispatch);
}

/// The fade-out climbs to opaque and stops there - it neither
/// overshoots nor completes on a transparent screen.
#[test]
fn fade_out_ramps_up_to_opaque_and_clamps() {
    let mut m = dispatching(SaveEntryContext::DebugParamEditor);
    m.goto(SaveSubScreen::FinalExit);
    m.tick(idle(), 0);
    m.tick(idle(), 0);
    assert_eq!(m.phase(), SavePhase::FadeOut);
    // The exiting screen writes a delta, so the level is still the
    // transparent one the dispatch phase ran at.
    assert!(m.fade() < FADE_DISPATCH_THRESHOLD);

    let mut seen = vec![m.fade()];
    while !m.is_done() {
        m.tick(idle(), 0x30);
        seen.push(m.fade());
    }
    assert!(
        seen.windows(2).all(|w| w[1] >= w[0]),
        "not monotonic: {seen:?}"
    );
    assert_eq!(m.fade(), FADE_OPAQUE);
}

// -----------------------------------------------------------------
// The routed half of the id space
// -----------------------------------------------------------------

/// The retail sub-screen pointer table is `0x00..=0x20`, and after the
/// `Routed` split every one of those 33 ids resolves to a screen this
/// crate can name. `Unpinned` is left holding nothing, which is the
/// whole point: the enum no longer claims a live table slot is unknown.
#[test]
fn no_live_table_id_is_unpinned() {
    for id in 0x00u8..=0x20 {
        let screen = SaveSubScreen::from_id(id);
        assert!(
            !matches!(screen, SaveSubScreen::Unpinned(_)),
            "id {id:#04x} still reads as Unpinned"
        );
        assert_eq!(screen.id(), id, "id {id:#04x} does not round-trip");
    }
    // ...and the variant is still reachable for a byte past the table,
    // which is what keeps the id space total.
    assert_eq!(SaveSubScreen::from_id(0x21), SaveSubScreen::Unpinned(0x21));
    assert_eq!(SaveSubScreen::from_id(0xFF), SaveSubScreen::Unpinned(0xFF));
}

/// Exactly 14 named + 1 flush + 18 routed = 33. Asserting the split
/// rather than only the total: a routed id silently promoted to
/// `Unpinned`, or a named one demoted to `Routed`, keeps the sum right
/// and breaks the claim. "Named" is not "stepped" - four of the fourteen
/// (`0x01`, `0x02`, `0x17`, `0x20`) are decoded here and driven
/// elsewhere; see the dispatch arm that lists them.
#[test]
fn the_table_splits_fourteen_named_one_flush_eighteen_routed() {
    let mut stepped = 0;
    let mut flush = 0;
    let mut routed = 0;
    for id in 0x00u8..=0x20 {
        match SaveSubScreen::from_id(id) {
            SaveSubScreen::FrameFlushTick => flush += 1,
            SaveSubScreen::Routed(_) => routed += 1,
            SaveSubScreen::Unpinned(_) => unreachable!(),
            _ => stepped += 1,
        }
    }
    assert_eq!((stepped, flush, routed), (14, 1, 18));
}

/// Every routed id names a port, each row names a distinct retail
/// routine, and the routine name is the `FUN_801D....` form the
/// `// PORT:` tags use - so a row that drifts from the code it points at
/// is findable by grepping the address.
#[test]
fn every_routed_id_names_a_distinct_port() {
    let mut seen_fns = std::collections::BTreeSet::new();
    let mut seen_ids = std::collections::BTreeSet::new();
    for (id, port) in ROUTED_PORTS {
        assert!(seen_ids.insert(*id), "duplicate routed id {id:#04x}");
        assert!(
            seen_fns.insert(port.retail_fn),
            "two ids claim {}",
            port.retail_fn
        );
        assert!(
            port.retail_fn.starts_with("FUN_801D") && port.retail_fn.len() == 12,
            "{} is not a FUN_801Dxxxx address",
            port.retail_fn
        );
        assert!(port.engine_module.starts_with("crate::"));
        assert!(!port.engine_item.is_empty());
        assert_eq!(SaveSubScreen::from_id(*id).routed_port(), Some(*port));
    }
    // A pinned screen has no routed port, and neither does an id past
    // the table.
    assert_eq!(SaveSubScreen::SlotSelect.routed_port(), None);
    assert_eq!(SaveSubScreen::FrameFlushTick.routed_port(), None);
    assert_eq!(SaveSubScreen::from_id(0x21).routed_port(), None);
}

/// Dispatching a routed screen emits the route rather than an empty
/// list. The empty list was the defect: it is what "this screen had
/// nothing to do this frame" looks like too, so a parked flow and a
/// handed-off one were indistinguishable to a host.
#[test]
fn a_routed_screen_dispatches_its_route() {
    let mut m = dispatching(SaveEntryContext::ShopEntry);
    m.goto(SaveSubScreen::Routed(0x1B));
    let effects = m.tick(SubScreenInput::default(), 0);
    assert_eq!(
        effects,
        vec![SubScreenEffect::Route(RoutedPort {
            retail_fn: "FUN_801DB21C",
            engine_module: "crate::shop",
            engine_item: "shop::buy_list_confirm_route",
        })]
    );
    // The flush slot really does nothing, and an out-of-table id parks.
    m.goto(SaveSubScreen::FrameFlushTick);
    assert!(m.tick(SubScreenInput::default(), 0).is_empty());
    m.goto(SaveSubScreen::Unpinned(0x30));
    assert!(m.tick(SubScreenInput::default(), 0).is_empty());
}

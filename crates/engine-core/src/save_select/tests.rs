use super::*;

fn slots(present_set: &[bool]) -> Vec<SlotSnapshot> {
    present_set
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if *p {
                SlotSnapshot {
                    slot: i as u8,
                    present: true,
                    content: SlotContent::LegaiaSave,
                    label: format!("Slot {i}"),
                    play_time_seconds: 1234,
                    party_lv: 5,
                    location: "Town01".into(),
                    money: 100,
                    leader_char_id: 0,
                    leader_name: "Vahn".into(),
                    leader_hp: (100, 100),
                    leader_mp: (20, 20),
                }
            } else {
                SlotSnapshot::empty(i as u8)
            }
        })
        .collect()
}

#[test]
fn empty_slots_session_done_immediately() {
    let s = SaveSelectSession::new(SaveSelectMode::Load, vec![]);
    assert!(s.is_done());
    assert_eq!(s.outcome(), Some(SelectOutcome::Cancelled));
}

#[test]
fn slot_info_mode_follows_what_occupies_the_block() {
    let mut snap = SlotSnapshot::empty(0);
    // A free block, which is what `empty` means.
    assert_eq!(SlotInfoMode::for_slot(&snap), SlotInfoMode::FreeBlock);

    snap.content = SlotContent::Foreign;
    assert_eq!(SlotInfoMode::for_slot(&snap), SlotInfoMode::NotLegaiaSave);

    snap.content = SlotContent::LegaiaSave;
    assert_eq!(SlotInfoMode::for_slot(&snap), SlotInfoMode::Preview);
}

#[test]
fn return_cell_captions_return_whatever_the_block_holds() {
    // `FUN_801E3F74` tests the cell index before it touches either
    // per-slot array, so the Return caption wins over every content
    // class - a readable save in cell 0xF would still caption Return.
    for content in [
        SlotContent::LegaiaSave,
        SlotContent::Foreign,
        SlotContent::Free,
    ] {
        let mut snap = SlotSnapshot::empty(SLOT_INFO_RETURN_CELL);
        snap.content = content;
        snap.present = content == SlotContent::LegaiaSave;
        assert_eq!(
            SlotInfoMode::for_grid_cell(SLOT_INFO_RETURN_CELL, &snap),
            SlotInfoMode::Return
        );
    }
    for m in [SaveSelectMode::Load, SaveSelectMode::Save] {
        assert_eq!(SlotInfoMode::Return.caption(m), Some("Return"));
    }
}

#[test]
fn every_other_cell_falls_through_to_the_content_selector() {
    let mut snap = SlotSnapshot::empty(0);
    snap.content = SlotContent::Free;
    for cell in 0..SLOT_INFO_RETURN_CELL {
        assert_eq!(
            SlotInfoMode::for_grid_cell(cell, &snap),
            SlotInfoMode::for_slot(&snap),
            "cell {cell} must not shortcut to Return"
        );
    }
}

#[test]
fn only_a_free_block_captions_differently_per_mode() {
    // A readable save fills the panel with stats, not a caption.
    assert_eq!(SlotInfoMode::Preview.caption(SaveSelectMode::Load), None);
    assert_eq!(SlotInfoMode::Preview.caption(SaveSelectMode::Save), None);

    // A foreign save reads the same either way.
    for m in [SaveSelectMode::Load, SaveSelectMode::Save] {
        assert_eq!(
            SlotInfoMode::NotLegaiaSave.caption(m),
            Some("Not a Legend of Legaia save.")
        );
    }

    // A free block is the one case that depends on why we're here.
    assert_eq!(
        SlotInfoMode::FreeBlock.caption(SaveSelectMode::Save),
        Some("Able to save.")
    );
    assert_eq!(
        SlotInfoMode::FreeBlock.caption(SaveSelectMode::Load),
        Some("No data")
    );
}

#[test]
fn every_unloadable_slot_gets_a_caption() {
    // The bug this guards: an unreadable slot drew an empty panel.
    // Whatever a slot holds, if it has no preview it must have words.
    for content in [SlotContent::Free, SlotContent::Foreign] {
        let snap = SlotSnapshot {
            content,
            ..SlotSnapshot::empty(3)
        };
        for m in [SaveSelectMode::Load, SaveSelectMode::Save] {
            let caption = SlotInfoMode::for_slot(&snap).caption(m);
            assert!(
                caption.is_some_and(|c| !c.is_empty()),
                "{content:?} in {m:?} mode left the panel blank"
            );
        }
    }
}

#[test]
fn load_empty_slot_invalid_confirm() {
    let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[false; 3]));
    let events = s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    assert!(events.contains(&SelectEvent::InvalidConfirm));
    assert!(matches!(s.phase(), SelectPhase::Browsing { .. }));
}

#[test]
fn load_full_slot_runs_now_checking_then_preview_then_load() {
    let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
    // Use a short timer so the test doesn't loop 120 times.
    s.set_now_checking_frames(2);
    let events = s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    // Should enter NowChecking with the configured frame count.
    match s.phase() {
        SelectPhase::NowChecking {
            slot: 0,
            frames_remaining: 2,
        } => {}
        other => panic!("expected NowChecking, got {other:?}"),
    }
    assert!(events.contains(&SelectEvent::EnteredNowChecking { slot: 0 }));
    // Tick 3 times: counts down 2→1→0→SlotPreview.
    s.tick(SelectInput::default());
    s.tick(SelectInput::default());
    let events = s.tick(SelectInput::default());
    match s.phase() {
        SelectPhase::SlotPreview { slot: 0 } => {}
        other => panic!("expected SlotPreview, got {other:?}"),
    }
    assert!(events.contains(&SelectEvent::EnteredSlotPreview { slot: 0 }));
    // X on preview confirms load.
    let events = s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    assert_eq!(s.outcome(), Some(SelectOutcome::Loaded(0)));
    assert!(events.contains(&SelectEvent::LoadConfirmed { slot: 0 }));
}

#[test]
fn now_checking_ignores_input() {
    let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
    s.set_now_checking_frames(5);
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    // Pressing X again while in NowChecking is a no-op (just ticks down).
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    match s.phase() {
        SelectPhase::NowChecking {
            frames_remaining: 4,
            ..
        } => {}
        other => panic!("input must not skip NowChecking; got {other:?}"),
    }
}

/// Enter the NowChecking beat on slot 0 of a three-slot list.
fn enter_now_checking(frames: u16) -> SaveSelectSession {
    let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
    s.set_now_checking_frames(frames);
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    s
}

/// Retail's beat is a per-frame card poll, not a timer: a card that
/// answers on the first frame ends the beat there. Without
/// `card_status_poll` driving `tick_now_checking` this sits in
/// NowChecking for all 120 frames.
#[test]
fn card_ready_event_ends_the_beat_early() {
    let mut s = enter_now_checking(120);
    s.set_card_events([true, false, false, false]);
    let events = s.tick(SelectInput::default());
    match s.phase() {
        SelectPhase::SlotPreview { slot: 0 } => {}
        other => panic!("a ready card must end the beat at once; got {other:?}"),
    }
    assert!(events.contains(&SelectEvent::EnteredSlotPreview { slot: 0 }));
}

/// Handle 3 (`Complete`) ends the beat the same way handle 0 does.
#[test]
fn card_complete_event_ends_the_beat_early() {
    let mut s = enter_now_checking(120);
    s.set_card_events([false, false, false, true]);
    s.tick(SelectInput::default());
    assert!(matches!(s.phase(), SelectPhase::SlotPreview { slot: 0 }));
}

/// Retail status `3` prints "NOT CARD" and abandons the read. The
/// session must fail back to browsing rather than opening a preview
/// of a card that is not there.
#[test]
fn missing_card_fails_the_beat_instead_of_succeeding() {
    let mut s = enter_now_checking(120);
    s.set_card_events([false, false, true, false]);
    let events = s.tick(SelectInput::default());
    match s.phase() {
        SelectPhase::Browsing { cursor: 0 } => {}
        other => panic!("a missing card must not reach SlotPreview; got {other:?}"),
    }
    assert!(events.contains(&SelectEvent::CardReadFailed { slot: 0 }));
    assert!(!events.contains(&SelectEvent::EnteredSlotPreview { slot: 0 }));
}

/// The default - no card hardware reporting - must be byte-identical
/// to the plain frame countdown, so a disk-backed host sees no
/// change from the poll being wired in.
#[test]
fn no_card_events_leaves_the_frame_countdown_alone() {
    let mut s = enter_now_checking(5);
    for expected in (0..5).rev() {
        s.tick(SelectInput::default());
        match s.phase() {
            SelectPhase::NowChecking {
                frames_remaining, ..
            } => assert_eq!(frames_remaining, expected),
            other => panic!("beat ended early with no card events; got {other:?}"),
        }
    }
    s.tick(SelectInput::default());
    assert!(matches!(s.phase(), SelectPhase::SlotPreview { slot: 0 }));
}

/// The poll counter is retail's `DAT_801EF17C`, which state 0 clears
/// on the way into the poll. A second visit must therefore start its
/// timeout window again.
///
/// The beat is stretched past the 120-frame timeout so the first
/// visit leaves the counter well over the limit. Without the reset
/// the second visit's poll is forced to `Aborted` before it can look
/// at the events, and a card reporting Complete would be ignored.
fn drive_io(
    io: &mut CardIoMachine,
    counter: &mut u16,
    statuses: &[CardStatus],
) -> (i32, Vec<CardIoEffect>) {
    let mut effects = Vec::new();
    let mut result = 0;
    for &st in statuses {
        let (r, e) = io.tick(st, counter);
        result = r;
        if let Some(e) = e {
            effects.push(e);
        }
    }
    (result, effects)
}

#[test]
fn card_io_happy_path_publishes_1_after_both_acks() {
    let mut io = CardIoMachine::new();
    let mut counter = 5u16;
    // Arm (resets the poll counter), first ack, arm second, second
    // ack, publish.
    let (result, effects) = drive_io(
        &mut io,
        &mut counter,
        &[
            CardStatus::Pending, // state 0: arm
            CardStatus::Ready,   // state 1: first ack -> state 2
            CardStatus::Pending, // state 2: arm second op
            CardStatus::Ready,   // state 3: second ack -> state 4
            CardStatus::Pending, // state 4: publish
        ],
    );
    assert_eq!(result, 1, "success publishes the pending 1");
    assert_eq!(effects, vec![CardIoEffect::StartOp, CardIoEffect::SecondOp]);
    assert_eq!(counter, 0, "arm states reset the poll backstop");

    // The both-acked latch short-circuits the next cycle: one ack
    // completes (state 1 Ready goes straight to publish).
    let (result, effects) = drive_io(
        &mut io,
        &mut counter,
        &[CardStatus::Pending, CardStatus::Ready, CardStatus::Pending],
    );
    assert_eq!(result, 1);
    assert_eq!(effects, vec![CardIoEffect::StartOp]);
}

#[test]
fn card_io_no_card_retries_five_times_then_fails_minus_1() {
    let mut io = CardIoMachine::new();
    let mut counter = 0u16;
    // Each NoCard in the first wait burns one retry and re-arms the
    // whole cycle with result 0.
    for _ in 0..CARD_IO_RETRIES {
        let (r, _) = io.tick(CardStatus::Pending, &mut counter); // arm
        assert_eq!(r, 0);
        io.tick(CardStatus::NoCard, &mut counter); // retry -> state 4
        let (r, _) = io.tick(CardStatus::Pending, &mut counter); // publish 0
        assert_eq!(r, 0, "a retry publishes 0, not an error");
    }
    // Budget spent: the sixth NoCard commits -1.
    io.tick(CardStatus::Pending, &mut counter);
    io.tick(CardStatus::NoCard, &mut counter);
    let (r, _) = io.tick(CardStatus::Pending, &mut counter);
    assert_eq!(r, -1);
}

#[test]
fn card_io_abort_exhaustion_publishes_minus_3() {
    let mut io = CardIoMachine::new();
    let mut counter = 0u16;
    for _ in 0..CARD_IO_RETRIES {
        io.tick(CardStatus::Pending, &mut counter);
        io.tick(CardStatus::Aborted, &mut counter);
        let (r, _) = io.tick(CardStatus::Pending, &mut counter);
        assert_eq!(r, 0);
    }
    io.tick(CardStatus::Pending, &mut counter);
    io.tick(CardStatus::Aborted, &mut counter);
    let (r, _) = io.tick(CardStatus::Pending, &mut counter);
    assert_eq!(r, -3);
}

#[test]
fn card_events_drain_clears_all_four() {
    let mut ev = [true, false, true, true];
    card_events_drain(&mut ev);
    assert_eq!(ev, [false; CARD_STATUS_EVENTS]);
}

#[test]
fn card_frame_tick_rebuilds_the_directory_on_the_commit_beat() {
    let mut io = CardIoMachine::new();
    let mut counter = 0u16;
    let mut frame = vec![0u8; CARD_DIRENTRY_STRIDE];
    // Built through the writer rather than respelled: this was the one
    // fixture in the file that named the literal itself, and it named
    // the wrong one (`PRO_`), which is how a matcher that matched no
    // real card kept a green test.
    let name = legaia_save::card::legaia_save_filename(3);
    frame[..name.len()].copy_from_slice(name.as_bytes());
    frame[CARD_DIRENTRY_SIZE_OFFSET..CARD_DIRENTRY_SIZE_OFFSET + 4]
        .copy_from_slice(&0x2000u32.to_le_bytes());
    let entries = vec![CardDirEntry::from_frame(&frame).unwrap()];

    // Off the commit beat: no rebuild, request stays up.
    let mut req = true;
    let (_, _, rebuilt) = card_frame_tick(
        &mut io,
        CardStatus::Pending,
        &mut counter,
        true,
        0,
        &mut req,
        &entries,
    );
    assert!(rebuilt.is_none());
    assert!(req);

    // Commit beat: scan -> cost -> classify runs once and clears the
    // request.
    let (_, _, rebuilt) = card_frame_tick(
        &mut io,
        CardStatus::Pending,
        &mut counter,
        false,
        3,
        &mut req,
        &entries,
    );
    let slots = rebuilt.expect("commit beat rebuilds");
    assert!(!req);
    // Slot 3 carries the save; every other slot is Free (14 free
    // blocks affordable out of 15 - one block spent on the save).
    assert!(slots[3].present);
    assert!(!slots[0].present);
}

#[test]
fn poll_counter_resets_between_visits() {
    let mut s = enter_now_checking(200);
    // Run the first visit right through to the preview, well past
    // CARD_STATUS_TIMEOUT_FRAMES, then back out to browsing.
    for _ in 0..201 {
        s.tick(SelectInput::default());
    }
    assert!(matches!(s.phase(), SelectPhase::SlotPreview { slot: 0 }));
    s.tick(SelectInput {
        circle: true,
        ..Default::default()
    });
    assert!(matches!(s.phase(), SelectPhase::Browsing { cursor: 0 }));

    // Second visit: a card that answers must still be heard.
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    s.set_card_events([false, false, false, true]);
    s.tick(SelectInput::default());
    match s.phase() {
        SelectPhase::SlotPreview { slot: 0 } => {}
        other => panic!("stale poll counter swallowed the card event; got {other:?}"),
    }
}

#[test]
fn slot_preview_circle_returns_to_browsing() {
    let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
    s.set_now_checking_frames(0);
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    // Frames=0 → first NowChecking tick advances to SlotPreview.
    s.tick(SelectInput::default());
    match s.phase() {
        SelectPhase::SlotPreview { slot: 0 } => {}
        other => panic!("expected SlotPreview, got {other:?}"),
    }
    let events = s.tick(SelectInput {
        circle: true,
        ..Default::default()
    });
    match s.phase() {
        SelectPhase::Browsing { cursor: 0 } => {}
        other => panic!("expected Browsing, got {other:?}"),
    }
    assert!(events.contains(&SelectEvent::SlotPreviewCancelled { slot: 0 }));
}

#[test]
fn save_overwrite_default_cursor_is_no() {
    let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[true, false, false]));
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    match s.phase() {
        SelectPhase::ConfirmOverwrite { cursor: 1, .. } => {}
        _ => panic!("default cursor should be 'No'"),
    }
}

#[test]
fn overwrite_cursor_toggles_with_directions() {
    let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[true, false, false]));
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    // cursor at 1 (No)
    s.tick(SelectInput {
        left: true,
        ..Default::default()
    });
    match s.phase() {
        SelectPhase::ConfirmOverwrite { cursor: 0, .. } => {}
        _ => panic!(),
    }
    // Confirm Yes.
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    assert_eq!(s.outcome(), Some(SelectOutcome::Saved(0)));
}

#[test]
fn save_into_empty_slot_goes_directly_to_saved() {
    let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[false, false, false]));
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    assert_eq!(s.outcome(), Some(SelectOutcome::Saved(0)));
}

#[test]
fn cursor_wraps() {
    let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[false, false, false]));
    s.tick(SelectInput {
        up: true,
        ..Default::default()
    });
    match s.phase() {
        SelectPhase::Browsing { cursor: 2 } => {}
        other => panic!("up from 0 should wrap to 2; got {other:?}"),
    }
}

#[test]
fn circle_cancels_session() {
    let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
    s.tick(SelectInput {
        circle: true,
        ..Default::default()
    });
    assert_eq!(s.outcome(), Some(SelectOutcome::Cancelled));
}

#[test]
fn circle_in_save_confirm_returns_to_browse() {
    // Save-mode ConfirmOverwrite still keeps the back-on-Circle
    // behavior (NowChecking only fires in Load mode).
    let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[true, false, false]));
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    s.tick(SelectInput {
        circle: true,
        ..Default::default()
    });
    match s.phase() {
        SelectPhase::Browsing { cursor: 0 } => {}
        _ => panic!(),
    }
}

#[test]
fn delete_shortcut_in_save_mode() {
    let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[true, false, false]));
    s.tick(SelectInput {
        triangle: true,
        ..Default::default()
    });
    match s.phase() {
        SelectPhase::ConfirmDelete { .. } => {}
        other => panic!("expected ConfirmDelete, got {other:?}"),
    }
}

#[test]
fn delete_yes_emits_deleted_outcome() {
    let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[true, false, false]));
    s.tick(SelectInput {
        triangle: true,
        ..Default::default()
    });
    // cursor = 1 (No) - switch to Yes.
    s.tick(SelectInput {
        left: true,
        ..Default::default()
    });
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    assert_eq!(s.outcome(), Some(SelectOutcome::Deleted(0)));
}

// --- card-slots mode (opt-in retail two-stage flow) ---

#[test]
fn card_slots_mode_is_off_by_default() {
    let s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[true]));
    assert!(
        !s.card_slots_mode(),
        "the flat block-list model must stay the default so the native \
         shell's save flow is unchanged"
    );
}

#[test]
fn card_slots_save_crosses_now_checking_then_previews() {
    let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[true, false]));
    s.set_card_slots_mode(true);
    s.set_now_checking_frames(1);
    // X on a slot holding a card reads it, exactly like Load mode -
    // NOT the flat model's straight-to-ConfirmOverwrite.
    let events = s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    assert!(events.contains(&SelectEvent::EnteredNowChecking { slot: 0 }));
    s.tick(SelectInput::default());
    let events = s.tick(SelectInput::default());
    assert!(matches!(s.phase(), SelectPhase::SlotPreview { slot: 0 }));
    assert!(events.contains(&SelectEvent::EnteredSlotPreview { slot: 0 }));
}

#[test]
fn card_slots_save_on_empty_slot_is_an_invalid_blip() {
    // `present == false` means "no card in this slot" here - there is
    // nothing to save into, so it must blip rather than report Saved
    // (the flat model's empty-slot behaviour).
    let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[false, false]));
    s.set_card_slots_mode(true);
    let events = s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    assert!(events.contains(&SelectEvent::InvalidConfirm));
    assert!(matches!(s.phase(), SelectPhase::Browsing { .. }));
    assert!(s.outcome().is_none(), "must not commit a save");
}

#[test]
fn card_slots_save_confirms_overwrite_from_the_preview() {
    let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[true]));
    s.set_card_slots_mode(true);
    s.set_now_checking_frames(0);
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    s.tick(SelectInput::default());
    assert!(matches!(s.phase(), SelectPhase::SlotPreview { .. }));
    // X on the preview raises the destructive prompt (defaulting to
    // "No"), it does not commit.
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    match s.phase() {
        SelectPhase::ConfirmOverwrite { slot: 0, cursor: 1 } => {}
        other => panic!("expected ConfirmOverwrite defaulting to No, got {other:?}"),
    }
    // "No" returns to the grid, not to the pill row.
    s.tick(SelectInput {
        circle: true,
        ..Default::default()
    });
    assert!(
        matches!(s.phase(), SelectPhase::SlotPreview { slot: 0 }),
        "cancelling the overwrite must return to the card's block grid"
    );
    // Yes commits.
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    s.tick(SelectInput {
        left: true,
        ..Default::default()
    });
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    // The card write beat ("Saving to MEMORY CARD") and the result line
    // ("Save successful.") run before the outcome lands.
    assert_eq!(s.committing_work(), Some(true));
    assert_eq!(s.outcome(), None);
    run_commit_beat(&mut s);
    assert_eq!(s.outcome(), Some(SelectOutcome::Saved(0)));
}

/// Tick a [`SelectPhase::Committing`] session through its whole beat with
/// no input; the result line flips on for the last stretch.
fn run_commit_beat(s: &mut SaveSelectSession) {
    let mut saw_result = false;
    for _ in 0..=(COMMIT_WORK_FRAMES + COMMIT_RESULT_FRAMES) {
        if s.committing_work() == Some(false) {
            saw_result = true;
        }
        // The host's write, answered at once.
        if s.awaiting_commit_report() {
            s.report_commit(true);
        }
        s.tick(SelectInput::default());
    }
    assert!(saw_result, "the result line showed before the outcome");
}

/// Retail asks before a card Load replaces the running game ("Do you wish
/// to load?", defaulting to No), then runs the "Now Loading" / "Load
/// successful." beat. Committing straight off the preview was the port's
/// shortcut.
#[test]
fn card_slots_load_asks_then_loads() {
    let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true]));
    s.set_card_slots_mode(true);
    s.set_now_checking_frames(0);
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    s.tick(SelectInput::default());
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    assert!(matches!(
        s.phase(),
        SelectPhase::ConfirmOverwrite { slot: 0, cursor: 1 }
    ));
    assert_eq!(s.outcome(), None, "the confirm does not load");
    s.tick(SelectInput {
        left: true,
        ..Default::default()
    });
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    assert_eq!(s.committing_work(), Some(true));
    // A press during the write beat does not skip it...
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    assert_eq!(s.outcome(), None);
    // ...but one on the result line does.
    while s.committing_work() == Some(true) {
        s.tick(SelectInput::default());
    }
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    assert_eq!(s.outcome(), Some(SelectOutcome::Loaded(0)));
}

#[test]
fn flat_save_mode_unchanged_when_card_slots_mode_is_off() {
    // Regression guard for the native shell: with the flag off, Save
    // mode must still go straight from the pill row to the overwrite
    // prompt / Saved outcome.
    let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[true, false]));
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    assert!(matches!(s.phase(), SelectPhase::ConfirmOverwrite { .. }));
    let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[false]));
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    assert_eq!(s.outcome(), Some(SelectOutcome::Saved(0)));
}

#[test]
fn play_time_string_format() {
    let mut snap = SlotSnapshot::empty(0);
    snap.play_time_seconds = 3 * 3600 + 25 * 60 + 7;
    assert_eq!(snap.play_time_string(), "03:25:07");
}

#[test]
fn empty_snapshot_label_includes_empty() {
    let s = SlotSnapshot::empty(2);
    assert!(s.label.contains("empty"));
    assert!(!s.present);
}

#[test]
fn slide_anim_holds_at_zero_while_browsing() {
    let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
    assert_eq!(s.slide_anim_t(), 0);
    for _ in 0..32 {
        s.tick(SelectInput::default());
        assert_eq!(s.slide_anim_t(), 0, "should stay 0 while Browsing");
    }
}

#[test]
fn slide_anim_ramps_in_now_checking_then_clamps() {
    let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
    s.set_now_checking_frames(120);
    // Enter NowChecking.
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    // The cross-press tick already entered NowChecking and
    // advance_slide_anim ran once -> t=256.
    assert_eq!(s.slide_anim_t(), SLIDE_ANIM_RATE);
    // Tick 15 more times: t goes 256 -> 512 -> ... -> 4096.
    for i in 2..=16 {
        s.tick(SelectInput::default());
        let expected = (SLIDE_ANIM_RATE as u32 * i).min(SLIDE_ANIM_FULL as u32) as u16;
        assert_eq!(
            s.slide_anim_t(),
            expected,
            "frame {i}: expected {expected}, got {}",
            s.slide_anim_t()
        );
    }
    // Should now be clamped at 4096.
    assert_eq!(s.slide_anim_t(), SLIDE_ANIM_FULL);
    s.tick(SelectInput::default());
    assert_eq!(s.slide_anim_t(), SLIDE_ANIM_FULL, "stays clamped");
}

#[test]
fn slide_anim_resets_on_cancel_back_to_browsing() {
    let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
    s.set_now_checking_frames(0);
    // Enter NowChecking (cross), then NowChecking auto-advances
    // to SlotPreview on the next tick because frames_remaining=0.
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    s.tick(SelectInput::default());
    assert!(matches!(s.phase(), SelectPhase::SlotPreview { .. }));
    // Slide should have advanced.
    assert!(s.slide_anim_t() > 0);
    // Cancel back to Browsing.
    s.tick(SelectInput {
        circle: true,
        ..Default::default()
    });
    assert!(matches!(s.phase(), SelectPhase::Browsing { .. }));
    assert_eq!(s.slide_anim_t(), 0, "must reset on cancel");
}

#[test]
fn interpolate_endpoints_match_start_and_target() {
    let start = (160, 96);
    let target = (48, 40);
    assert_eq!(interpolate_anim(start, target, 0), start);
    assert_eq!(interpolate_anim(start, target, SLIDE_ANIM_FULL), target);
    // Midpoint: half-way between (160, 96) and (48, 40) is
    // (104, 68). Integer division truncates toward zero, so
    // verify the exact rounded value.
    let mid = interpolate_anim(start, target, SLIDE_ANIM_FULL / 2);
    assert_eq!(mid, (104, 68));
}

#[test]
fn info_panel_slide_holds_at_zero_in_browsing_and_now_checking() {
    let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
    s.set_now_checking_frames(120);
    // Browsing.
    for _ in 0..8 {
        s.tick(SelectInput::default());
        assert_eq!(s.info_panel_slide_anim_t(), 0);
    }
    // Enter NowChecking; the info panel still holds at 0 here so
    // the panel stays hidden while the dialog plays.
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    for _ in 0..8 {
        s.tick(SelectInput::default());
        assert_eq!(
            s.info_panel_slide_anim_t(),
            0,
            "info panel must stay hidden during NowChecking"
        );
    }
}

#[test]
fn info_panel_slide_ramps_in_slot_preview_then_clamps() {
    let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
    s.set_now_checking_frames(0);
    // Enter NowChecking, auto-advance to SlotPreview because
    // frames_remaining = 0.
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    s.tick(SelectInput::default());
    assert!(matches!(s.phase(), SelectPhase::SlotPreview { .. }));
    // First SlotPreview tick already advanced once.
    let mut last = s.info_panel_slide_anim_t();
    assert_eq!(last, SLIDE_ANIM_RATE);
    // Ramp to clamp.
    for _ in 0..32 {
        s.tick(SelectInput::default());
        let now = s.info_panel_slide_anim_t();
        assert!(now >= last, "monotonic non-decreasing");
        assert!(now <= SLIDE_ANIM_FULL, "never exceeds clamp");
        last = now;
    }
    assert_eq!(last, SLIDE_ANIM_FULL);
}

#[test]
fn info_panel_slide_resets_on_cancel_back_to_browsing() {
    let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
    s.set_now_checking_frames(0);
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    s.tick(SelectInput::default());
    // SlotPreview reached; ramp a few frames.
    for _ in 0..4 {
        s.tick(SelectInput::default());
    }
    assert!(s.info_panel_slide_anim_t() > 0);
    // Cancel back to Browsing.
    s.tick(SelectInput {
        circle: true,
        ..Default::default()
    });
    assert!(matches!(s.phase(), SelectPhase::Browsing { .. }));
    assert_eq!(s.info_panel_slide_anim_t(), 0);
}

#[test]
fn info_panel_offscreen_to_parked_interpolation() {
    // Endpoint check: anim_t=0 -> off-screen y=394; t=4096 -> parked y=138.
    let off = (0, INFO_PANEL_OFFSCREEN_Y);
    let park = (0, INFO_PANEL_PARKED_Y);
    assert_eq!(interpolate_anim(off, park, 0).1, INFO_PANEL_OFFSCREEN_Y);
    assert_eq!(
        interpolate_anim(off, park, SLIDE_ANIM_FULL).1,
        INFO_PANEL_PARKED_Y
    );
}

#[test]
fn interpolate_method_uses_session_anim_t() {
    let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
    s.set_now_checking_frames(120);
    // Browsing: t=0 -> returns start.
    assert_eq!(s.interpolate((100, 50), (200, 80)), (100, 50));
    // Enter NowChecking, then tick 16 frames to reach t=4096.
    s.tick(SelectInput {
        cross: true,
        ..Default::default()
    });
    for _ in 0..16 {
        s.tick(SelectInput::default());
    }
    assert_eq!(s.slide_anim_t(), SLIDE_ANIM_FULL);
    assert_eq!(s.interpolate((100, 50), (200, 80)), (200, 80));
}

use super::*;

fn full_party() -> [SlotState; 3] {
    [SlotState::alive(true, true); 3]
}

fn full_monsters() -> [SlotState; 5] {
    [SlotState::alive(true, true); 5]
}

#[test]
fn single_enemy_initial_cursor_at_first_alive() {
    let mut monsters = full_monsters();
    monsters[0].alive = false;
    let p = TargetPickerSession::new(TargetKind::SingleEnemy, 0, full_party(), monsters);
    match p.state() {
        PickerState::Cursor {
            row: CursorRow::Enemy,
            slot,
        } => assert_eq!(slot, 1),
        _ => panic!("expected enemy cursor"),
    }
}

#[test]
fn single_enemy_skips_dead_when_stepping() {
    let mut monsters = full_monsters();
    monsters[1].alive = false;
    monsters[2].alive = false;
    let mut p = TargetPickerSession::new(TargetKind::SingleEnemy, 0, full_party(), monsters);
    // Cursor starts at 0; step right → should jump to 3.
    p.input(PickerInput {
        right: true,
        ..Default::default()
    });
    match p.state() {
        PickerState::Cursor { slot: 3, .. } => {}
        other => panic!("expected slot 3, got {other:?}"),
    }
}

#[test]
fn single_ally_excludes_self() {
    let mut p = TargetPickerSession::new(TargetKind::SingleAlly, 1, full_party(), full_monsters());
    // Cursor starts at first valid slot != 1.
    match p.state() {
        PickerState::Cursor {
            row: CursorRow::Ally,
            slot,
        } => assert_eq!(slot, 0),
        _ => panic!("expected ally cursor at 0"),
    }
    // Step right → 2 (skipping self at 1).
    p.input(PickerInput {
        right: true,
        ..Default::default()
    });
    match p.state() {
        PickerState::Cursor { slot, .. } => assert_eq!(slot, 2),
        _ => panic!(),
    }
}

#[test]
fn single_ally_or_self_includes_actor() {
    let mut p = TargetPickerSession::new(
        TargetKind::SingleAllyOrSelf,
        1,
        full_party(),
        full_monsters(),
    );
    // Step from 0 → 1, includes self.
    p.input(PickerInput {
        right: true,
        ..Default::default()
    });
    match p.state() {
        PickerState::Cursor { slot, .. } => assert_eq!(slot, 1),
        _ => panic!(),
    }
}

#[test]
fn dead_ally_only_picks_dead_slots() {
    let mut party = full_party();
    party[0].alive = false;
    party[2].alive = false;
    let mut p = TargetPickerSession::new(TargetKind::DeadAlly, 1, party, full_monsters());
    // Cursor starts at first dead slot.
    match p.state() {
        PickerState::Cursor { slot, .. } => assert_eq!(slot, 0),
        _ => panic!(),
    }
    // Step right → 2 (skipping the live slot at 1).
    p.input(PickerInput {
        right: true,
        ..Default::default()
    });
    match p.state() {
        PickerState::Cursor { slot, .. } => assert_eq!(slot, 2),
        _ => panic!(),
    }
}

#[test]
fn no_candidates_when_all_dead() {
    let p = TargetPickerSession::new(
        TargetKind::SingleEnemy,
        0,
        full_party(),
        [SlotState::dead(true); 5],
    );
    assert_eq!(p.outcome(), Some(PickerOutcome::NoCandidates));
}

#[test]
fn confirm_emits_single_outcome() {
    let mut p = TargetPickerSession::new(TargetKind::SingleEnemy, 0, full_party(), full_monsters());
    p.input(PickerInput {
        cross: true,
        ..Default::default()
    });
    match p.outcome().unwrap() {
        PickerOutcome::Single { slot, row } => {
            assert_eq!(slot, 0);
            assert_eq!(row, CursorRow::Enemy);
        }
        _ => panic!(),
    }
}

#[test]
fn cancel_emits_cancelled_outcome() {
    let mut p = TargetPickerSession::new(TargetKind::SingleEnemy, 0, full_party(), full_monsters());
    p.input(PickerInput {
        circle: true,
        ..Default::default()
    });
    assert_eq!(p.outcome(), Some(PickerOutcome::Cancelled));
}

#[test]
fn all_enemies_immediate_sweep() {
    let p = TargetPickerSession::new(TargetKind::AllEnemies, 0, full_party(), full_monsters());
    match p.outcome().unwrap() {
        PickerOutcome::Sweep {
            row: CursorRow::Enemy,
        } => {}
        _ => panic!(),
    }
}

#[test]
fn all_allies_immediate_sweep() {
    let p = TargetPickerSession::new(TargetKind::AllAllies, 0, full_party(), full_monsters());
    match p.outcome().unwrap() {
        PickerOutcome::Sweep {
            row: CursorRow::Ally,
        } => {}
        _ => panic!(),
    }
}

#[test]
fn self_target_immediate() {
    let p = TargetPickerSession::new(TargetKind::Self_, 0, full_party(), full_monsters());
    match p.outcome().unwrap() {
        PickerOutcome::Sweep {
            row: CursorRow::Ally,
        } => {}
        _ => panic!(),
    }
}

#[test]
fn cursor_emits_event_on_move() {
    let mut p = TargetPickerSession::new(TargetKind::SingleEnemy, 0, full_party(), full_monsters());
    let _ = p.drain_events();
    p.input(PickerInput {
        right: true,
        ..Default::default()
    });
    let evs = p.drain_events();
    assert_eq!(evs.len(), 1);
    match evs[0] {
        PickerEvent::CursorMoved { slot: 1, .. } => {}
        _ => panic!(),
    }
}

#[test]
fn cursor_wraps_around_within_row() {
    let mut p = TargetPickerSession::new(TargetKind::SingleEnemy, 0, full_party(), full_monsters());
    // 0 → left → wraps to 4.
    p.input(PickerInput {
        left: true,
        ..Default::default()
    });
    match p.state() {
        PickerState::Cursor { slot, .. } => assert_eq!(slot, 4),
        _ => panic!(),
    }
}

#[test]
fn invalid_confirm_when_initial_state_already_done() {
    let p = TargetPickerSession::new(
        TargetKind::SingleEnemy,
        0,
        full_party(),
        [SlotState::dead(true); 5],
    );
    // Already NoCandidates.
    assert!(p.is_done());
    assert_eq!(p.outcome(), Some(PickerOutcome::NoCandidates));
}

#[test]
fn input_after_done_is_noop() {
    let mut p = TargetPickerSession::new(TargetKind::Self_, 0, full_party(), full_monsters());
    let evs_before = p.drain_events();
    assert!(!evs_before.is_empty());
    p.input(PickerInput {
        cross: true,
        ..Default::default()
    });
    // No new events.
    assert!(p.drain_events().is_empty());
}

#[test]
fn target_kind_immediacy() {
    assert!(TargetKind::AllEnemies.is_immediate());
    assert!(TargetKind::AllAllies.is_immediate());
    assert!(TargetKind::Self_.is_immediate());
    assert!(!TargetKind::SingleEnemy.is_immediate());
}

/// Fixed-pitch stand-in for the retail proportional measurer.
fn width(s: &str) -> i16 {
    s.chars().count() as i16 * 8
}

fn rows_for(formation: [u8; 4], names: &[&str; 4], px: [i16; 4]) -> Vec<EnemyMenuRow> {
    let names: Vec<String> = names.iter().map(|s| s.to_string()).collect();
    enemy_menu_rows(
        formation,
        DEDUP_SUFFIX,
        |slot| names[slot as usize].clone(),
        |slot| px[slot as usize],
    )
}

#[test]
fn distinct_monsters_each_get_their_own_row() {
    let r = rows_for(
        [1, 2, 3, 4],
        &["Bee", "Wasp", "Slug", "Bat"],
        [100, 200, 300, 400],
    );
    assert_eq!(r.len(), 4);
    assert_eq!(
        r.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
        ["Bee", "Wasp", "Slug", "Bat"]
    );
    assert!(r.iter().all(|x| x.members == 1));
    assert_eq!(
        r.iter().map(|x| x.first_slot).collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
}

#[test]
fn a_run_drops_the_instance_letter_appends_the_suffix_then_increments() {
    let r = rows_for(
        [7, 7, 7, 0],
        &["Killer Bee A", "Killer Bee B", "Killer Bee C", ""],
        [80, 96, 112, 0],
    );
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].members, 3);
    // Member 2 drops the first member's letter and appends `* 2`; member
    // 3 increments the digit - the capture's `Killer Bee * 3`.
    assert_eq!(r[0].label, "Killer Bee * 3");
    // The accumulator sums the members' projected positions.
    assert_eq!(r[0].x, 80 + 96 + 112);
    assert_eq!(r[0].first_slot, 0);
}

#[test]
fn the_dedup_is_positional_so_a_b_a_makes_three_rows() {
    let r = rows_for([5, 5, 9, 5], &["Bee", "Bee", "Bat", "Bee"], [0; 4]);
    assert_eq!(r.len(), 3);
    assert_eq!(r[0].members, 2);
    assert_eq!(r[1].members, 1);
    assert_eq!(r[2].members, 1);
    // The trailing repeat starts a fresh row with the plain name.
    assert_eq!(r[2].label, "Bee");
}

#[test]
fn empty_formation_slots_are_skipped_without_ending_the_walk() {
    let r = rows_for([1, 0, 2, 0], &["Bee", "", "Bat", ""], [0; 4]);
    assert_eq!(r.len(), 2);
    assert_eq!(r[1].first_slot, 2);
    assert!(rows_for([0; 4], &["", "", "", ""], [0; 4]).is_empty());
}

#[test]
fn layout_centres_a_single_row_and_never_clamps_it() {
    let mut r = rows_for([1, 0, 0, 0], &["Bee", "", "", ""], [8 * 160, 0, 0, 0]);
    layout_enemy_menu_rows(&mut r, width);
    // (8*160 / 1) >> 3 == 160, minus half the 24px label, plus 0xA0.
    assert_eq!(r[0].x, 160 - 12 + 0xA0);
}

#[test]
fn layout_averages_a_runs_members() {
    let mut r = rows_for([1, 1, 0, 0], &["Bee A", "Bee B", "", ""], [800, 1600, 0, 0]);
    layout_enemy_menu_rows(&mut r, width);
    // avg 1200 >> 3 = 150, label "Bee * 2" is 56px wide.
    assert_eq!(r[0].label, "Bee * 2");
    assert_eq!(r[0].x, 150 - 28 + 0xA0);
}

#[test]
fn layout_pushes_overlapping_rows_apart_symmetrically() {
    // Two rows whose raw centres coincide.
    let mut r = rows_for([1, 2, 0, 0], &["Bee", "Bat", "", ""], [800, 800, 0, 0]);
    layout_enemy_menu_rows(&mut r, width);
    let gap = (r[1].x - r[0].x).abs();
    assert!(
        gap >= width("Bee") + MENU_ROW_GAP - 1,
        "rows still overlap: {:?}",
        r.iter().map(|x| x.x).collect::<Vec<_>>()
    );
    // The split is even, so the pair stays centred on where it started.
    let raw = 100 - 12 + 0xA0;
    assert_eq!((r[0].x + r[1].x) / 2, raw);
}

#[test]
fn layout_clamps_rows_inside_the_screen() {
    // Four rows all crowded to the far right; relaxation plus clamping has
    // to fit them inside [6, 0x13A - width].
    let mut r = rows_for(
        [1, 2, 3, 4],
        &["Aaaaaa", "Bbbbbb", "Cccccc", "Dddddd"],
        [8 * 300; 4],
    );
    layout_enemy_menu_rows(&mut r, width);
    for row in &r {
        assert!(row.x >= MENU_MIN_X, "{row:?}");
        assert!(
            row.x + width(&row.label) <= MENU_MAX_RIGHT,
            "{row:?} runs past the right edge"
        );
    }
}

#[test]
fn seat_cursor_takes_only_a_valid_target() {
    let mut monsters = full_monsters();
    monsters[2].alive = false;
    let mut p = TargetPickerSession::new(TargetKind::SingleEnemy, 0, full_party(), monsters);
    // A dead slot and the ally row are refused; the cursor stays put.
    assert!(!p.seat_cursor(CursorRow::Enemy, 2));
    assert!(!p.seat_cursor(CursorRow::Ally, 1));
    assert_eq!(
        p.state(),
        PickerState::Cursor {
            row: CursorRow::Enemy,
            slot: 0
        }
    );
    // A live monster - the fifth seat included - is taken.
    assert!(p.seat_cursor(CursorRow::Enemy, 4));
    assert_eq!(
        p.state(),
        PickerState::Cursor {
            row: CursorRow::Enemy,
            slot: 4
        }
    );
}

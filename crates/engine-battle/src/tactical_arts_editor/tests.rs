use super::*;

fn lib() -> ChainLibrary {
    ChainLibrary::new()
}

#[test]
fn library_starts_empty() {
    let lib = lib();
    assert_eq!(lib.total_count(), 0);
    assert!(lib.saved(0).is_empty());
}

#[test]
fn save_chain_validates_length() {
    let mut lib = lib();
    let too_short = SavedChain::new("ab", vec![Command::Left, Command::Right]);
    assert_eq!(lib.save(0, too_short), Err(SaveError::InvalidLength(2)));
    let ok = SavedChain::new("ok", vec![Command::Left, Command::Right, Command::Up]);
    assert!(lib.save(0, ok).is_ok());
    // Too long
    let too_long = SavedChain::new(
        "long",
        vec![
            Command::Left,
            Command::Right,
            Command::Up,
            Command::Down,
            Command::Left,
            Command::Right,
            Command::Up,
            Command::Down,
        ],
    );
    assert_eq!(lib.save(0, too_long), Err(SaveError::InvalidLength(8)));
}

#[test]
fn library_caps_at_max_slots() {
    let mut lib = lib();
    for i in 0..ChainLibrary::MAX_SLOTS {
        lib.save(0, SavedChain::new(format!("c{i}"), vec![Command::Left; 4]))
            .unwrap();
    }
    assert!(lib.is_full(0));
    let overflow = lib.save(0, SavedChain::new("over", vec![Command::Left; 4]));
    assert_eq!(overflow, Err(SaveError::LibraryFull));
}

#[test]
fn replace_chain_in_place() {
    let mut lib = lib();
    lib.save(0, SavedChain::new("a", vec![Command::Left; 4]))
        .unwrap();
    lib.replace(0, 0, SavedChain::new("b", vec![Command::Right; 5]))
        .unwrap();
    assert_eq!(lib.saved(0)[0].name, "b");
}

#[test]
fn remove_returns_entry() {
    let mut lib = lib();
    lib.save(0, SavedChain::new("a", vec![Command::Left; 4]))
        .unwrap();
    let removed = lib.remove(0, 0).unwrap();
    assert_eq!(removed.name, "a");
    assert!(lib.saved(0).is_empty());
}

#[test]
fn pretty_sequence_roundtrip() {
    let c = SavedChain::new(
        "x",
        vec![Command::Left, Command::Right, Command::Up, Command::Down],
    );
    assert_eq!(c.pretty_sequence(), "L R U D");
}

#[test]
fn editor_starts_in_browsing() {
    let lib = lib();
    let ed = ChainEditor::new(0, &lib);
    match ed.phase() {
        EditorPhase::Browsing { cursor: 0 } => {}
        _ => panic!(),
    }
    // browse_rows = 0 saved + 1 "new" row.
    assert_eq!(ed.browse_rows(), 1);
}

#[test]
fn editor_cancel_emits_cancelled() {
    let lib = lib();
    let mut ed = ChainEditor::new(0, &lib);
    let events = ed.tick(EditInput {
        circle: true,
        ..Default::default()
    });
    assert!(events.contains(&EditEvent::Cancelled));
    assert!(matches!(ed.outcome(), Some(EditOutcome::Cancelled)));
}

#[test]
fn editor_creates_new_chain_via_naming() {
    let lib = lib();
    let mut ed = ChainEditor::new(0, &lib);
    // Open new-chain editor.
    ed.tick(EditInput {
        cross: true,
        ..Default::default()
    });
    match ed.phase() {
        EditorPhase::Editing { .. } => {}
        _ => panic!(),
    }
    // Push 4 directions.
    ed.tick(EditInput {
        left: true,
        ..Default::default()
    });
    ed.tick(EditInput {
        right: true,
        ..Default::default()
    });
    ed.tick(EditInput {
        up: true,
        ..Default::default()
    });
    ed.tick(EditInput {
        down: true,
        ..Default::default()
    });
    // Confirm → naming.
    ed.tick(EditInput {
        cross: true,
        ..Default::default()
    });
    match ed.phase() {
        EditorPhase::Naming { .. } => {}
        _ => panic!(),
    }
    // Confirm naming → done.
    let events = ed.tick(EditInput {
        cross: true,
        ..Default::default()
    });
    assert!(events.iter().any(|e| matches!(e, EditEvent::Saved { .. })));
    match ed.outcome().unwrap() {
        EditOutcome::Saved { slot: 0, chain } => {
            assert_eq!(chain.len(), 4);
        }
        _ => panic!(),
    }
}

#[test]
fn editor_reject_too_short_commit() {
    let lib = lib();
    let mut ed = ChainEditor::new(0, &lib);
    ed.tick(EditInput {
        cross: true,
        ..Default::default()
    });
    ed.tick(EditInput {
        left: true,
        ..Default::default()
    });
    ed.tick(EditInput {
        left: true,
        ..Default::default()
    });
    let events = ed.tick(EditInput {
        cross: true,
        ..Default::default()
    });
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EditEvent::InvalidCommit { len: 2 }))
    );
    match ed.phase() {
        EditorPhase::Editing { .. } => {}
        _ => panic!(),
    }
}

#[test]
fn editor_pop_with_triangle() {
    let lib = lib();
    let mut ed = ChainEditor::new(0, &lib);
    ed.tick(EditInput {
        cross: true,
        ..Default::default()
    });
    ed.tick(EditInput {
        left: true,
        ..Default::default()
    });
    ed.tick(EditInput {
        right: true,
        ..Default::default()
    });
    let events = ed.tick(EditInput {
        triangle: true,
        ..Default::default()
    });
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EditEvent::SequencePopped { len: 1 }))
    );
}

#[test]
fn editor_replace_existing() {
    let mut lib = lib();
    lib.save(0, SavedChain::new("a", vec![Command::Left; 4]))
        .unwrap();
    let mut ed = ChainEditor::new(0, &lib);
    // Cursor is at row 0 - Cross opens edit.
    ed.tick(EditInput {
        cross: true,
        ..Default::default()
    });
    // Pop the seed sequence and push new one.
    for _ in 0..4 {
        ed.tick(EditInput {
            triangle: true,
            ..Default::default()
        });
    }
    for _ in 0..3 {
        ed.tick(EditInput {
            up: true,
            ..Default::default()
        });
    }
    ed.tick(EditInput {
        cross: true,
        ..Default::default()
    });
    ed.tick(EditInput {
        cross: true,
        ..Default::default()
    });
    match ed.outcome().unwrap() {
        EditOutcome::Replaced { slot: 0, chain } => {
            assert_eq!(chain.len(), 3);
            assert!(chain.sequence.iter().all(|c| matches!(c, Command::Up)));
        }
        other => panic!("expected Replaced, got {other:?}"),
    }
}

#[test]
fn editor_delete_existing_via_triangle_in_browse() {
    let mut lib = lib();
    lib.save(0, SavedChain::new("a", vec![Command::Left; 4]))
        .unwrap();
    lib.save(0, SavedChain::new("b", vec![Command::Right; 4]))
        .unwrap();
    let mut ed = ChainEditor::new(0, &lib);
    // Move cursor to slot 1.
    ed.tick(EditInput {
        down: true,
        ..Default::default()
    });
    let events = ed.tick(EditInput {
        triangle: true,
        ..Default::default()
    });
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EditEvent::Deleted { slot: 1 }))
    );
    match ed.outcome().unwrap() {
        EditOutcome::Deleted { slot: 1 } => {}
        _ => panic!(),
    }
}

#[test]
fn apply_outcome_updates_library() {
    let mut lib = lib();
    lib.save(0, SavedChain::new("seed", vec![Command::Left; 4]))
        .unwrap();
    let mut ed = ChainEditor::new(0, &lib);
    ed.tick(EditInput {
        down: true,
        ..Default::default()
    });
    ed.tick(EditInput {
        cross: true,
        ..Default::default()
    });
    for _ in 0..3 {
        ed.tick(EditInput {
            up: true,
            ..Default::default()
        });
    }
    ed.tick(EditInput {
        cross: true,
        ..Default::default()
    });
    ed.tick(EditInput {
        cross: true,
        ..Default::default()
    });
    ed.apply_outcome(&mut lib).unwrap();
    assert_eq!(lib.saved(0).len(), 2);
}

#[test]
fn next_name_cycles_presets() {
    let n0 = next_name("");
    assert_eq!(n0, NAME_PRESETS[0]);
    let n1 = next_name(NAME_PRESETS[0]);
    assert_eq!(n1, NAME_PRESETS[1]);
    let last = next_name(NAME_PRESETS[7]);
    assert_eq!(last, NAME_PRESETS[0]);
}

#[test]
fn editor_max_length_enforced() {
    let lib = lib();
    let mut ed = ChainEditor::new(0, &lib);
    ed.tick(EditInput {
        cross: true,
        ..Default::default()
    });
    for _ in 0..(ChainLibrary::MAX_LEN + 3) {
        ed.tick(EditInput {
            left: true,
            ..Default::default()
        });
    }
    match ed.phase() {
        EditorPhase::Editing { working } => assert_eq!(working.len(), ChainLibrary::MAX_LEN),
        _ => panic!(),
    }
}

#[test]
fn saved_chain_record_roundtrip_preserves_name_and_directions() {
    let chain = SavedChain::new(
        "Combo",
        vec![Command::Left, Command::Up, Command::Up, Command::Down],
    );
    let rec = chain.to_record(2);
    assert_eq!(rec.char_slot, 2);
    assert_eq!(rec.name, "Combo");
    // 1=Left, 4=Up, 4=Up, 3=Down.
    assert_eq!(rec.sequence, vec![1, 4, 4, 3]);
    assert_eq!(SavedChain::from_record(&rec), chain);
}

#[test]
fn from_record_drops_terminator_and_nondirectional_bytes() {
    let rec = legaia_save::SavedChainRecord {
        char_slot: 0,
        name: "x".into(),
        // 2=Right, 0=terminator, 9=garbage, 3=Down.
        sequence: vec![2, 0, 9, 3],
    };
    let chain = SavedChain::from_record(&rec);
    assert_eq!(chain.sequence, vec![Command::Right, Command::Down]);
}

#[test]
fn library_records_roundtrip_groups_by_slot_in_order() {
    let mut lib = lib();
    lib.save(0, SavedChain::new("a", vec![Command::Left; 4]))
        .unwrap();
    lib.save(2, SavedChain::new("b", vec![Command::Right; 3]))
        .unwrap();
    lib.save(0, SavedChain::new("c", vec![Command::Up; 5]))
        .unwrap();

    let records = lib.to_records();
    // Slot 0's two chains come first (saved order), then slot 2's one.
    let by_slot: Vec<(u8, &str)> = records
        .iter()
        .map(|r| (r.char_slot, r.name.as_str()))
        .collect();
    assert_eq!(by_slot, vec![(0, "a"), (0, "c"), (2, "b")]);

    let rebuilt = ChainLibrary::from_records(&records);
    assert_eq!(rebuilt.saved(0), lib.saved(0));
    assert_eq!(rebuilt.saved(2), lib.saved(2));
    assert_eq!(rebuilt.total_count(), 3);
}

#[test]
fn from_records_caps_each_slot_at_max() {
    let records: Vec<legaia_save::SavedChainRecord> = (0..ChainLibrary::MAX_SLOTS + 4)
        .map(|i| legaia_save::SavedChainRecord {
            char_slot: 1,
            name: format!("c{i}"),
            sequence: vec![1, 2, 3],
        })
        .collect();
    let lib = ChainLibrary::from_records(&records);
    assert_eq!(lib.saved(1).len(), ChainLibrary::MAX_SLOTS);
}

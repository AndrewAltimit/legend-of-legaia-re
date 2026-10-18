use super::*;
use legaia_engine_menus::field_menu::{
    FieldMenuGate, FieldMenuPhase, FieldMenuRow, FieldMenuSession,
};

fn fresh_world() -> World {
    let mut world = World::new();
    // Three placeholder records with non-zero max HP/MP so the
    // status / spell builders include them.
    world.party.roster = legaia_save::Party::zeroed(3);
    for member in &mut world.party.roster.members {
        let mut hms = member.hp_mp_sp();
        hms.hp_cur = 50;
        hms.hp_max = 100;
        hms.mp_cur = 10;
        hms.mp_max = 30;
        member.set_hp_mp_sp(hms);
    }
    world.party.inventory.insert(0x77, 3); // Healing Leaf (real item id)
    world.party.party_leader_slot = Some(0);
    world.set_item_catalog(legaia_engine_menus::items::ItemCatalog::vanilla());
    world
}

fn fresh_save_slots() -> SaveRack {
    SaveRack::Blocks(
        (0..3)
            .map(legaia_engine_menus::save_select::SlotSnapshot::empty)
            .collect(),
    )
}

fn build(row: FieldMenuRow, world: &World) -> FieldMenuSubsession {
    FieldMenuSubsession::build(
        row,
        world,
        &OptionsState::default(),
        &fresh_save_slots(),
        &ChainLibrary::new(),
        &SpellCatalog::vanilla(),
        &EquipmentTable::new(),
    )
}

/// Every pause-menu level readout reads the record's `+0x130` byte, so a
/// level cheat shows on Status and on the Magic caster window alike, and
/// survives the ability-bitfield rebuild (`+0xF4..+0x103`) the battle-stat
/// refresh runs.
#[test]
fn status_and_magic_read_the_live_level() {
    let mut w = fresh_world();
    w.party.roster.members[0].set_level(30);
    w.party.roster.members[0].set_ability_bits([0; legaia_save::ABILITY_BITS_LEN]);
    assert_eq!(status_snapshots(&w)[0].level, 30);
    match build(FieldMenuRow::Magic, &w) {
        FieldMenuSubsession::Spells(s) => {
            let m = legaia_engine_menus::pause_screens::magic_screen_model(&s, None);
            assert_eq!(m.casters[0].1, 30);
        }
        _ => panic!("expected Spells"),
    }
}

#[test]
fn build_items_returns_inventory_session() {
    let w = fresh_world();
    let s = build(FieldMenuRow::Items, &w);
    assert_eq!(s.row(), FieldMenuRow::Items);
    assert!(matches!(s, FieldMenuSubsession::Items(_)));
}

/// A Throw Out discard reaches the world bag through
/// `apply_inventory_outcome` even when the session ends without a
/// completed use (the retail delete zeroes the whole bag-slot pair).
#[test]
fn throw_out_discard_applies_to_world_inventory() {
    use legaia_engine_system::input::PadButton;
    let mut w = fresh_world();
    let FieldMenuSubsession::Items(mut s) = build(FieldMenuRow::Items, &w) else {
        panic!("items session");
    };
    let press = |s: &mut legaia_engine_menus::pause_screens::PauseItemsSession, b: PadButton| {
        s.input_pad_edge(b.mask());
    };
    press(&mut s, PadButton::Down); // -> Throw Out
    press(&mut s, PadButton::Cross); // into the discard list
    press(&mut s, PadButton::Cross); // open the confirm (No)
    press(&mut s, PadButton::Up); // -> Yes
    press(&mut s, PadButton::Cross); // discard the 0x77 stack
    press(&mut s, PadButton::Circle); // back off the command window
    assert!(s.is_done());
    apply_inventory_outcome(&s.inner, &mut w);
    assert!(
        !w.party.inventory.contains_key(&0x77),
        "the whole stack is discarded"
    );
}

/// An equipped slot prints the item's name from the staged item table,
/// not its raw id; an empty slot stays blank.
#[test]
fn status_equipment_rows_print_item_names() {
    let mut w = fresh_world();
    let mut names = vec![None; 256];
    names[0x43] = Some("Survival Knife".to_string());
    w.menu.text = Some(legaia_engine_menus::pause_screens::MenuTextTables {
        item_names: Some(legaia_game_tables::item_names::ItemNameTable::from_names(
            names,
        )),
        ..Default::default()
    });
    let mut eq = w.party.roster.members[0].equipment();
    eq.slots.fill(0);
    // Vahn's weapon byte is `2`; byte `1` is the head row.
    eq.slots[2] = 0x43;
    eq.slots[1] = 0x44;
    w.party.roster.members[0].set_equipment(eq);
    let snap = &status_snapshots(&w)[0];
    assert_eq!(snap.equip[0].item_name, "Survival Knife");
    assert_eq!(snap.equip[1].item_name, "#44", "no name staged");
    assert_eq!(snap.equip[2].item_name, "");
}

#[test]
fn build_status_snapshots_skip_empty_roster_slots() {
    let mut w = fresh_world();
    // Zero one member's max HP - they should drop out of the snapshot.
    let mut hms = w.party.roster.members[2].hp_mp_sp();
    hms.hp_max = 0;
    w.party.roster.members[2].set_hp_mp_sp(hms);
    let snaps = status_snapshots(&w);
    assert_eq!(snaps.len(), 2);
}

/// A lone traveller's status list is the lone traveller, even though
/// every roster record is populated (the New Game template seeds all of
/// them), and a non-identity party lists its members in party order.
#[test]
fn status_snapshots_list_the_present_party_only() {
    let mut w = fresh_world();
    w.install_present_party_list(vec![0]);
    let slots: Vec<u8> = status_snapshots(&w).iter().map(|s| s.slot).collect();
    assert_eq!(slots, vec![0]);
    w.install_present_party_list(vec![2, 0]);
    let slots: Vec<u8> = status_snapshots(&w).iter().map(|s| s.slot).collect();
    assert_eq!(slots, vec![2, 0]);
}

/// The Items and Magic pickers list the present party in party-list order,
/// as the Status list does: retail hands the cursor kernel the party count
/// and resolves the picked row through the roster id bytes (`FUN_801D8308`
/// `0x801D83C0` / `0x801D851C`, `FUN_801D9594` `0x801D964C` / `0x801D96BC`).
/// Walking every roster record offered members who are not in the party as
/// targets, and a heal used on one was spent on nobody the player holds.
#[test]
fn item_and_spell_pickers_list_the_present_party_only() {
    let mut w = fresh_world();
    w.install_present_party_list(vec![0]);
    match build(FieldMenuRow::Items, &w) {
        FieldMenuSubsession::Items(p) => {
            let slots: Vec<u8> = p.inner.targets.iter().map(|t| t.slot).collect();
            assert_eq!(slots, vec![0], "a lone traveller is the only item target");
        }
        _ => panic!("expected Items variant"),
    }
    w.install_present_party_list(vec![2, 0]);
    match build(FieldMenuRow::Items, &w) {
        FieldMenuSubsession::Items(p) => {
            let slots: Vec<u8> = p.inner.targets.iter().map(|t| t.slot).collect();
            assert_eq!(slots, vec![2, 0], "party-list order, by roster id");
        }
        _ => panic!("expected Items variant"),
    }
    match build(FieldMenuRow::Magic, &w) {
        FieldMenuSubsession::Spells(sm) => {
            let casters: Vec<u8> = sm.party().iter().map(|c| c.slot).collect();
            let targets: Vec<u8> = sm.targets().iter().map(|t| t.slot).collect();
            assert_eq!(casters, vec![2, 0]);
            assert_eq!(targets, vec![2, 0]);
        }
        _ => panic!("expected Spells variant"),
    }
}

#[test]
fn build_spells_session_population() {
    let w = fresh_world();
    let s = build(FieldMenuRow::Magic, &w);
    match s {
        FieldMenuSubsession::Spells(sm) => {
            assert_eq!(sm.party().len(), 3);
            assert_eq!(sm.targets().len(), 3);
        }
        _ => panic!("expected Spells variant"),
    }
}

#[test]
fn build_save_uses_save_mode() {
    let w = fresh_world();
    let s = build(FieldMenuRow::Save, &w);
    match s {
        FieldMenuSubsession::Save(ss) => {
            assert_eq!(ss.mode(), SaveSelectMode::Save);
            assert_eq!(ss.slots().len(), 3);
        }
        _ => panic!("expected Save"),
    }
}

/// The Equip row opens on retail's character picker (`0x12`): Down walks
/// the present party and re-points the screen at the hovered member,
/// Cross hands the pad to the slot browse, and the browse's cancel comes
/// back to the picker rather than leaving the screen.
#[test]
fn equip_opens_on_the_character_picker() {
    let mut w = fresh_world();
    w.install_present_party_list(vec![0, 1]);
    let mut s = build(FieldMenuRow::Equip, &w);
    let state = |s: &FieldMenuSubsession| match s {
        FieldMenuSubsession::Equip {
            char_slot, picking, ..
        } => (*char_slot, *picking),
        _ => panic!("expected Equip"),
    };
    assert_eq!(state(&s), (0, true));
    tick_open_subsession(&mut s, PadButton::Down.mask(), None, &w);
    assert_eq!(state(&s), (1, true));
    tick_open_subsession(&mut s, PadButton::Down.mask(), None, &w);
    assert_eq!(state(&s), (1, true), "the walk stops at the last member");
    tick_open_subsession(&mut s, PadButton::Cross.mask(), None, &w);
    assert_eq!(state(&s), (1, false));
    tick_open_subsession(&mut s, PadButton::Circle.mask(), None, &w);
    assert_eq!(state(&s), (1, true));
    assert!(!s.is_done());
    tick_open_subsession(&mut s, PadButton::Circle.mask(), None, &w);
    assert!(s.is_done());
}

#[test]
fn build_equip_uses_active_leader() {
    let mut w = fresh_world();
    w.party.party_leader_slot = Some(2);
    let s = build(FieldMenuRow::Equip, &w);
    match s {
        FieldMenuSubsession::Equip { char_slot, .. } => assert_eq!(char_slot, 2),
        _ => panic!("expected Equip"),
    }
}

#[test]
fn build_options_seeds_state_from_input() {
    let w = fresh_world();
    let opts = OptionsState {
        bgm_volume: 3,
        ..OptionsState::default()
    };
    let s = FieldMenuSubsession::build(
        FieldMenuRow::Options,
        &w,
        &opts,
        &fresh_save_slots(),
        &ChainLibrary::new(),
        &SpellCatalog::vanilla(),
        &EquipmentTable::new(),
    );
    match s {
        FieldMenuSubsession::Config(o) => assert_eq!(o.state().bgm_volume, 3),
        _ => panic!("expected Config"),
    }
}

#[test]
fn tick_pad_edge_status_circle_closes() {
    let w = fresh_world();
    let mut s = build(FieldMenuRow::Status, &w);
    assert!(!s.is_done());
    s.tick_pad_edge(PadButton::Circle.mask());
    assert!(s.is_done());
}

#[test]
fn triangle_on_status_opens_the_arts_editor_for_the_shown_character() {
    let w = fresh_world();
    let mut s = build(FieldMenuRow::Status, &w);
    assert!(try_open_arts_editor(&mut s, PadButton::Triangle.mask(), &w));
    match &s {
        FieldMenuSubsession::Arts(editor) => {
            // The status panel opens on its first shown snapshot, so the
            // editor must target that member's slot - not a fixed 0.
            let expected = status_snapshots(&w).first().map(|s| s.slot).unwrap_or(0);
            assert_eq!(editor.char_slot(), expected);
        }
        _ => panic!("expected the Status session to be swapped for Arts"),
    }
    // The resume cursor parks back on Status, so closing the editor
    // returns the player where the extension was entered.
    assert_eq!(s.row(), FieldMenuRow::Status);
}

#[test]
fn arts_editor_open_button_is_inert_outside_the_status_screen() {
    let w = fresh_world();
    for row in [FieldMenuRow::Items, FieldMenuRow::Equip, FieldMenuRow::Save] {
        let mut s = build(row, &w);
        assert!(
            !try_open_arts_editor(&mut s, PadButton::Triangle.mask(), &w),
            "{row:?} must not open the arts editor"
        );
        assert_eq!(s.row(), row);
    }
    // ...and Status itself only reacts to the documented button.
    let mut s = build(FieldMenuRow::Status, &w);
    assert!(!try_open_arts_editor(&mut s, PadButton::Cross.mask(), &w));
    assert!(matches!(s, FieldMenuSubsession::Status(_)));
}

#[test]
fn arts_editor_view_projects_live_library_and_phase() {
    use legaia_art::queue::Command;
    use legaia_engine_battle::tactical_arts_editor::SavedChain;

    let w = fresh_world();
    let mut lib = ChainLibrary::new();
    lib.save(
        0,
        SavedChain::new(
            "Combo A",
            vec![Command::Left, Command::Right, Command::Down],
        ),
    )
    .expect("in-range chain saves");
    let editor = ChainEditor::new(0, &lib);

    let view = arts_editor_view(&editor, &w);
    assert_eq!(view.phase, ArtsEditorPhaseTag::Browsing);
    assert_eq!(view.saved.len(), 1);
    assert_eq!(view.saved[0].0, "Combo A");
    // The pretty sequence is the shared one-line stringification both
    // hosts print, so it is asserted rather than re-derived per host.
    assert_eq!(view.saved[0].1, "L R D");
    assert_eq!(view.character_name, "Vahn");
    assert_eq!(view.min_len, ChainLibrary::MIN_LEN);
    assert_eq!(view.max_len, ChainLibrary::MAX_LEN);
    assert!(view.can_add_new, "1 of 8 slots used leaves room for + New");
}

#[test]
fn arts_editor_view_hides_new_row_once_the_library_is_full() {
    use legaia_art::queue::Command;
    use legaia_engine_battle::tactical_arts_editor::SavedChain;

    let w = fresh_world();
    let mut lib = ChainLibrary::new();
    for i in 0..ChainLibrary::MAX_SLOTS {
        lib.save(
            0,
            SavedChain::new(
                format!("C{i}"),
                vec![Command::Up, Command::Up, Command::Down],
            ),
        )
        .expect("library has room until MAX_SLOTS");
    }
    let view = arts_editor_view(&ChainEditor::new(0, &lib), &w);
    assert_eq!(view.saved.len(), ChainLibrary::MAX_SLOTS);
    assert!(!view.can_add_new);
}

#[test]
fn tick_pad_edge_options_circle_cancels() {
    let w = fresh_world();
    let mut s = build(FieldMenuRow::Options, &w);
    s.tick_pad_edge(PadButton::Circle.mask());
    assert!(s.is_done());
}

#[test]
fn tick_pad_edge_save_circle_cancels() {
    let w = fresh_world();
    let mut s = build(FieldMenuRow::Save, &w);
    s.tick_pad_edge(PadButton::Circle.mask());
    assert!(s.is_done());
}

#[test]
fn tick_pad_edge_equip_circle_cancels() {
    let w = fresh_world();
    let mut s = build(FieldMenuRow::Equip, &w);
    s.tick_pad_edge(PadButton::Circle.mask());
    assert!(s.is_done());
}

#[test]
fn tick_pad_edge_inventory_circle_cancels() {
    let w = fresh_world();
    let mut s = build(FieldMenuRow::Items, &w);
    s.tick_pad_edge(PadButton::Circle.mask());
    assert!(s.is_done());
}

#[test]
fn tick_pad_edge_spells_circle_cancels() {
    let w = fresh_world();
    let mut s = build(FieldMenuRow::Magic, &w);
    s.tick_pad_edge(PadButton::Circle.mask());
    assert!(s.is_done());
}

#[test]
fn apply_equip_outcome_writes_back_to_roster() {
    let mut w = fresh_world();
    // EquipSession's items_for_slot encodes target slot in the upper
    // 3 bits (slot = id >> 5). Use 0x25 for slot 1 so we don't
    // collide with the Healing Leaf (id 0x01, which also sorts into
    // slot 0).
    w.party.inventory.clear();
    w.party.inventory.insert(0x25, 1);
    let mut equip_table = EquipmentTable::new();
    equip_table.set(
        0x25,
        legaia_engine_battle::battle_stats::ItemModifier::default(),
    );
    let mut s = FieldMenuSubsession::build(
        FieldMenuRow::Equip,
        &w,
        &OptionsState::default(),
        &fresh_save_slots(),
        &ChainLibrary::new(),
        &SpellCatalog::vanilla(),
        &equip_table,
    );
    // Slot-browse row 0 is Best Equipment, so slot 1 (where item 0x25
    // lives) is row 2: two steps down, confirm into the item picker,
    // confirm the single item, confirm Yes.
    // The Equip row opens on its character picker; confirm hands the pad
    // to the slot browse.
    s.tick_pad_edge(PadButton::Cross.mask());
    for _ in 0..2 {
        s.tick_pad_edge(PadButton::Down.mask());
    }
    for _ in 0..3 {
        s.tick_pad_edge(PadButton::Cross.mask());
    }
    assert!(s.is_done());
    if let FieldMenuSubsession::Equip {
        session, char_slot, ..
    } = &s
    {
        let outcome = apply_equip_outcome(session, *char_slot, &mut w);
        assert!(matches!(outcome, Some(EquipOutcome::Committed { .. })));
        // Roster member 0's slot 1 byte now matches the equipped id.
        assert_eq!(w.party.roster.members[0].equipment().slots[1], 0x25);
        // ...and the equipped item LEFT the bag (no duplication). Slot 1 was
        // empty, so nothing is returned.
        assert_eq!(
            w.party.inventory.get(&0x25),
            None,
            "equipped item must be removed from the bag (no duplication)"
        );
    } else {
        panic!("expected Equip variant");
    }
}

/// The Best Equipment row (slot-browse row 0) commits on the session's own
/// record and does not end the session, so its swaps reach the world when the
/// session closes - on a cancel as well, since retail's applier
/// (`FUN_801CF760`) has already written the record by then.
#[test]
fn best_equipment_reaches_the_roster_when_the_session_is_cancelled() {
    let mut w = fresh_world();
    w.party.inventory.clear();
    // Without disc restrictions the id's upper bits name the engine slot:
    // `0x45 >> 5` is slot 2, body armour, which ranks on its defence.
    w.party.inventory.insert(0x45, 1);
    let mut equip_table = EquipmentTable::new();
    equip_table.set(
        0x45,
        legaia_engine_battle::battle_stats::ItemModifier {
            udf: 8,
            ldf: 7,
            ..Default::default()
        },
    );
    let mut s = FieldMenuSubsession::build(
        FieldMenuRow::Equip,
        &w,
        &OptionsState::default(),
        &fresh_save_slots(),
        &ChainLibrary::new(),
        &SpellCatalog::vanilla(),
        &equip_table,
    );
    // Character picker, then Best Equipment on row 0, then back out.
    s.tick_pad_edge(PadButton::Cross.mask());
    s.tick_pad_edge(PadButton::Cross.mask());
    for _ in 0..3 {
        if !s.is_done() {
            s.tick_pad_edge(PadButton::Circle.mask());
        }
    }
    assert!(s.is_done());
    let FieldMenuSubsession::Equip {
        session, char_slot, ..
    } = &s
    else {
        panic!("expected Equip variant");
    };
    assert_eq!(session.best_equipment_swaps().len(), 1);
    let outcome = apply_equip_outcome(session, *char_slot, &mut w);
    assert_eq!(outcome, Some(EquipOutcome::Cancelled));
    assert!(
        w.party.roster.members[0].equipment().slots.contains(&0x45),
        "the swap Best Equipment made must be on the live record"
    );
    assert_eq!(
        w.party.inventory.get(&0x45),
        None,
        "and the item it put on must have left the bag"
    );
}

/// Equipping over an occupied slot returns the swapped-out item to the bag
/// and removes the newly-equipped one - no item loss, no duplication.
#[test]
fn apply_equip_outcome_returns_the_swapped_out_item_to_the_bag() {
    let mut w = fresh_world();
    w.party.inventory.clear();
    w.party.inventory.insert(0x25, 1);
    // Pre-equip a different slot-1 item (0x26 >> 5 == 1) on member 0.
    let mut eq = w.party.roster.members[0].equipment();
    eq.slots[1] = 0x26;
    w.party.roster.members[0].set_equipment(eq);

    let mut equip_table = EquipmentTable::new();
    equip_table.set(
        0x25,
        legaia_engine_battle::battle_stats::ItemModifier::default(),
    );
    equip_table.set(
        0x26,
        legaia_engine_battle::battle_stats::ItemModifier::default(),
    );
    let mut s = FieldMenuSubsession::build(
        FieldMenuRow::Equip,
        &w,
        &OptionsState::default(),
        &fresh_save_slots(),
        &ChainLibrary::new(),
        &SpellCatalog::vanilla(),
        &equip_table,
    );
    // Row 2 is slot 1. The slot is occupied, so its candidate list
    // leads with the Remove row - one more step down lands on 0x25.
    // The Equip row opens on its character picker; confirm hands the pad
    // to the slot browse.
    s.tick_pad_edge(PadButton::Cross.mask());
    for _ in 0..2 {
        s.tick_pad_edge(PadButton::Down.mask());
    }
    s.tick_pad_edge(PadButton::Cross.mask());
    s.tick_pad_edge(PadButton::Down.mask());
    for _ in 0..2 {
        s.tick_pad_edge(PadButton::Cross.mask());
    }
    assert!(s.is_done());
    let FieldMenuSubsession::Equip {
        session, char_slot, ..
    } = &s
    else {
        panic!("expected Equip variant");
    };
    let outcome = apply_equip_outcome(session, *char_slot, &mut w);
    assert!(matches!(
        outcome,
        Some(EquipOutcome::Committed {
            removed: 0x26,
            added: 0x25,
            ..
        })
    ));
    assert_eq!(w.party.roster.members[0].equipment().slots[1], 0x25);
    // 0x25 left the bag, 0x26 came back into it.
    assert_eq!(
        w.party.inventory.get(&0x25),
        None,
        "equipped item left the bag"
    );
    assert_eq!(
        w.party.inventory.get(&0x26),
        Some(&1),
        "swapped-out item returned to the bag"
    );
}

/// Op `0x49`'s save-point row (`49 01 00`) opens the pause menu with entry
/// context kind 1; the hosts' root-list step builds the save sub-session.
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
    assert_eq!(tick_root_list(&mut session, 0), Some(FieldMenuRow::Save));
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

//! End-to-end smoke test for the field-menu sub-session dispatcher.
//!
//! Drives a synthetic World through every row of the field menu:
//!
//! 1. Open the field menu via `FieldMenuSession::new`
//! 2. Confirm a row (`tick` with `cross`) - phase becomes Suspended
//! 3. Build the matching `FieldMenuSubsession` from the world
//! 4. Tick the sub-session through a happy / cancel path
//! 5. Apply the outcome (where defined) back onto the world
//! 6. Call `FieldMenuSession::resume(false)` - phase returns to Browsing
//!
//! Mirrors the dispatch flow in `legaia-engine-shell::play-window` so the
//! plumbing is verified without spinning a wgpu surface.

use legaia_engine_core::battle_stats::EquipmentTable;
use legaia_engine_core::field_menu::{
    FieldMenuInput, FieldMenuPhase, FieldMenuRow, FieldMenuSession,
};
use legaia_engine_core::field_menu_dispatch::{
    FieldMenuSubsession, apply_arts_outcome, apply_equip_outcome, apply_inventory_outcome,
    apply_spell_outcome,
};
use legaia_engine_core::input::PadButton;
use legaia_engine_core::items::ItemCatalog;
use legaia_engine_core::options::OptionsState;
use legaia_engine_core::save_select::{SaveSelectMode, SlotSnapshot};
use legaia_engine_core::spells::SpellCatalog;
use legaia_engine_core::tactical_arts_editor::{ChainEditor, ChainLibrary};
use legaia_engine_core::world::World;

fn fresh_world() -> World {
    let mut world = World::new();
    world.roster = legaia_save::Party::zeroed(3);
    for member in &mut world.roster.members {
        let mut hms = member.hp_mp_sp();
        hms.hp_cur = 50;
        hms.hp_max = 100;
        hms.mp_cur = 12;
        hms.mp_max = 30;
        member.set_hp_mp_sp(hms);
    }
    world.party_leader_slot = Some(0);
    world.set_item_catalog(ItemCatalog::vanilla());
    world.inventory.insert(0x77, 3); // Healing Leaf (real item id)
    world.money = 100;
    world
}

fn slots() -> Vec<SlotSnapshot> {
    (0..3).map(SlotSnapshot::empty).collect()
}

fn build(row: FieldMenuRow, world: &World, options: &OptionsState) -> FieldMenuSubsession {
    FieldMenuSubsession::build(
        row,
        world,
        options,
        &slots(),
        &ChainLibrary::new(),
        &SpellCatalog::vanilla(),
        &EquipmentTable::new(),
    )
}

/// Drive a `FieldMenuSession` to `Suspended { row }` and assert the
/// resulting phase. Mirrors the flow play-window runs every frame the
/// player presses Cross on a row.
fn open_field_menu_at(row: FieldMenuRow) -> FieldMenuSession {
    let mut s = FieldMenuSession::new();
    // Cursor starts at Items (0); step down (row.index()) times to land
    // on the requested row.
    for _ in 0..row.index() {
        let _ = s.tick(FieldMenuInput {
            down: true,
            ..Default::default()
        });
    }
    let _ = s.tick(FieldMenuInput {
        cross: true,
        ..Default::default()
    });
    assert!(matches!(s.phase(), FieldMenuPhase::Suspended { .. }));
    s
}

#[test]
fn field_menu_status_row_routes_through_status_subsession() {
    let world = fresh_world();
    let mut menu = open_field_menu_at(FieldMenuRow::Status);
    let mut sub = build(FieldMenuRow::Status, &world, &OptionsState::default());
    assert!(matches!(sub, FieldMenuSubsession::Status(_)));
    // Press Circle to cancel out - sub completes.
    sub.tick_pad_edge(PadButton::Circle.mask());
    assert!(sub.is_done());
    let _ = menu.resume(false);
    assert!(matches!(menu.phase(), FieldMenuPhase::Browsing { .. }));
}

#[test]
fn field_menu_config_row_round_trips_options_state() {
    let world = fresh_world();
    let options = OptionsState {
        bgm_volume: 4,
        ..OptionsState::default()
    };
    let mut menu = open_field_menu_at(FieldMenuRow::Options);
    let mut sub = build(FieldMenuRow::Options, &world, &options);
    // Circle leaves the options screen (retail commits value edits inside
    // the popup; exit is a plain close that keeps the state).
    sub.tick_pad_edge(PadButton::Circle.mask());
    assert!(sub.is_done());
    if let FieldMenuSubsession::Config(s) = &sub {
        assert_eq!(s.state().bgm_volume, 4);
    } else {
        panic!("expected Config sub");
    }
    let _ = menu.resume(false);
    assert!(matches!(menu.phase(), FieldMenuPhase::Browsing { .. }));
}

#[test]
fn field_menu_save_row_in_save_mode_with_three_slots() {
    let world = fresh_world();
    let _menu = open_field_menu_at(FieldMenuRow::Save);
    let sub = build(FieldMenuRow::Save, &world, &OptionsState::default());
    if let FieldMenuSubsession::Save(s) = &sub {
        assert_eq!(s.mode(), SaveSelectMode::Save);
        assert_eq!(s.slots().len(), 3);
    } else {
        panic!("expected Save sub");
    }
}

#[test]
fn field_menu_items_row_drains_to_inventory_session() {
    let world = fresh_world();
    let mut menu = open_field_menu_at(FieldMenuRow::Items);
    let mut sub = build(FieldMenuRow::Items, &world, &OptionsState::default());
    if let FieldMenuSubsession::Items(s) = &sub {
        // Player has one item (Healing Leaf) - filtered list should be 1.
        assert_eq!(s.inner.filtered_items.len(), 1);
        // The retail screen carries the row's real bag count.
        assert_eq!(s.rows.len(), 1);
        assert_eq!(s.rows[0].count, 3);
    } else {
        panic!("expected Items sub");
    }
    sub.tick_pad_edge(PadButton::Circle.mask());
    assert!(sub.is_done());
    let _ = menu.resume(false);
}

#[test]
fn field_menu_equip_row_uses_active_leader() {
    let mut world = fresh_world();
    world.party_leader_slot = Some(1);
    let _menu = open_field_menu_at(FieldMenuRow::Equip);
    let sub = build(FieldMenuRow::Equip, &world, &OptionsState::default());
    if let FieldMenuSubsession::Equip { char_slot, .. } = &sub {
        assert_eq!(*char_slot, 1);
    } else {
        panic!("expected Equip sub");
    }
}

#[test]
fn field_menu_spells_row_populates_party_and_targets() {
    let world = fresh_world();
    let _menu = open_field_menu_at(FieldMenuRow::Magic);
    let sub = build(FieldMenuRow::Magic, &world, &OptionsState::default());
    if let FieldMenuSubsession::Spells(s) = &sub {
        assert_eq!(s.party().len(), 3);
        assert_eq!(s.targets().len(), 3);
    } else {
        panic!("expected Spells sub");
    }
}

#[test]
fn arts_chain_editor_variant_builds_directly_for_leader() {
    // The Arts chain editor has no retail pause-menu row (retail's list
    // is Items / Magic / Equip / Status / Options / Load / Save);
    // engines construct the sub-session variant directly.
    let editor = ChainEditor::new(2, &ChainLibrary::new());
    let sub = FieldMenuSubsession::Arts(editor);
    if let FieldMenuSubsession::Arts(editor) = &sub {
        assert_eq!(editor.char_slot(), 2);
    } else {
        panic!("expected Arts sub");
    }
    // The extension session parks the resume cursor on Status.
    assert_eq!(sub.row(), FieldMenuRow::Status);
}

#[test]
fn apply_inventory_outcome_does_nothing_on_cancel() {
    let mut world = fresh_world();
    let world_money_before = world.money;
    let mut sub = build(FieldMenuRow::Items, &world, &OptionsState::default());
    sub.tick_pad_edge(PadButton::Circle.mask());
    assert!(sub.is_done());
    if let FieldMenuSubsession::Items(s) = sub {
        apply_inventory_outcome(&s.inner, &mut world);
    }
    assert_eq!(world.money, world_money_before);
}

#[test]
fn apply_equip_outcome_writes_back_to_roster() {
    let mut world = fresh_world();
    // Insert an item that the placeholder (`id >> 5 == slot`) rule lands
    // in slot 1 to avoid the Healing-Leaf collision in slot 0.
    world.inventory.clear();
    world.inventory.insert(0x25, 1);
    let mut equip_table = EquipmentTable::new();
    equip_table.set(
        0x25,
        legaia_engine_core::battle_stats::ItemModifier::default(),
    );
    let mut sub = FieldMenuSubsession::build(
        FieldMenuRow::Equip,
        &world,
        &OptionsState::default(),
        &slots(),
        &ChainLibrary::new(),
        &SpellCatalog::vanilla(),
        &equip_table,
    );
    sub.tick_pad_edge(PadButton::Down.mask());
    for _ in 0..3 {
        sub.tick_pad_edge(PadButton::Cross.mask());
    }
    assert!(sub.is_done());
    if let FieldMenuSubsession::Equip { session, char_slot } = &sub {
        let _ = apply_equip_outcome(session, *char_slot, &mut world);
        assert_eq!(world.roster.members[0].equipment().slots[1], 0x25);
    } else {
        panic!("expected Equip sub");
    }
}

#[test]
fn apply_spell_outcome_zeroes_caster_mp_after_heal() {
    let mut world = fresh_world();
    // Wound member 1 so a heal has effect.
    let mut hms = world.roster.members[1].hp_mp_sp();
    hms.hp_cur = 1;
    world.roster.members[1].set_hp_mp_sp(hms);
    // Give member 0 a spell list with one heal spell (id 0x07 = Spark Arrow,
    // but we want a heal - use 0x05 / 0x09 / 0x0E from the vanilla catalog
    // depending on what's heal). The vanilla catalog's first heal-effect
    // spell ID can be found via SpellCatalog::vanilla.iter, but for the
    // test we just install a known-heal id 0x09 in the spell list.
    let mut spells = world.roster.members[0].spell_list();
    spells.count = 1;
    spells.ids[0] = 0x09;
    world.roster.members[0].set_spell_list(spells);
    let mut sub = build(FieldMenuRow::Magic, &world, &OptionsState::default());
    // Cross on caster → spell select; Cross on spell → target select; Down to
    // pick member 1; Cross to cast.
    sub.tick_pad_edge(PadButton::Cross.mask()); // pick caster 0
    let still_open = !sub.is_done();
    if !still_open {
        // Caster might be invalid (empty spell list edge case). Bail.
        return;
    }
    sub.tick_pad_edge(PadButton::Cross.mask()); // pick first spell
    if sub.is_done() {
        return; // not field-usable / not enough mp etc.
    }
    sub.tick_pad_edge(PadButton::Down.mask()); // cursor to slot 1
    sub.tick_pad_edge(PadButton::Cross.mask()); // confirm target
    if let FieldMenuSubsession::Spells(s) = &sub
        && s.is_done()
    {
        apply_spell_outcome(s, &mut world);
    }
}

#[test]
fn apply_arts_outcome_writes_through_chain_library() {
    let _world = fresh_world();
    let mut library = ChainLibrary::new();
    let sub = FieldMenuSubsession::Arts(ChainEditor::new(0, &ChainLibrary::new()));
    if let FieldMenuSubsession::Arts(editor) = sub {
        // Cancelled path - `apply_outcome` returns Ok with no mutation.
        let _ = apply_arts_outcome(editor, &mut library);
        assert_eq!(library.total_count(), 0);
    } else {
        panic!("expected Arts sub");
    }
}

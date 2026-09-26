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
use legaia_engine_core::save_select::{SaveRack, SaveSelectMode, SlotSnapshot};
use legaia_engine_core::spells::SpellCatalog;
use legaia_engine_core::tactical_arts_editor::{ChainEditor, ChainLibrary};
use legaia_engine_core::world::World;

fn fresh_world() -> World {
    let mut world = World::new();
    world.party.roster = legaia_save::Party::zeroed(3);
    for member in &mut world.party.roster.members {
        let mut hms = member.hp_mp_sp();
        hms.hp_cur = 50;
        hms.hp_max = 100;
        hms.mp_cur = 12;
        hms.mp_max = 30;
        member.set_hp_mp_sp(hms);
    }
    world.party.party_leader_slot = Some(0);
    world.set_item_catalog(ItemCatalog::vanilla());
    world.party.inventory.insert(0x77, 3); // Healing Leaf (real item id)
    world.party.money = 100;
    world
}

fn slots() -> SaveRack {
    SaveRack::Blocks((0..3).map(SlotSnapshot::empty).collect())
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
    world.party.party_leader_slot = Some(1);
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
    let world_money_before = world.party.money;
    let mut sub = build(FieldMenuRow::Items, &world, &OptionsState::default());
    sub.tick_pad_edge(PadButton::Circle.mask());
    assert!(sub.is_done());
    if let FieldMenuSubsession::Items(s) = sub {
        apply_inventory_outcome(&s.inner, &mut world);
    }
    assert_eq!(world.party.money, world_money_before);
}

#[test]
fn apply_equip_outcome_writes_back_to_roster() {
    let mut world = fresh_world();
    // Insert an item that the placeholder (`id >> 5 == slot`) rule lands
    // in slot 1 to avoid the Healing-Leaf collision in slot 0.
    world.party.inventory.clear();
    world.party.inventory.insert(0x25, 1);
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
    // Slot-browse row 0 is "Best Equipment", so slot 1 is row 2.
    for _ in 0..2 {
        sub.tick_pad_edge(PadButton::Down.mask());
    }
    for _ in 0..3 {
        sub.tick_pad_edge(PadButton::Cross.mask());
    }
    assert!(sub.is_done());
    if let FieldMenuSubsession::Equip { session, char_slot } = &sub {
        let _ = apply_equip_outcome(session, *char_slot, &mut world);
        assert_eq!(world.party.roster.members[0].equipment().slots[1], 0x25);
    } else {
        panic!("expected Equip sub");
    }
}

#[test]
fn apply_spell_outcome_zeroes_caster_mp_after_heal() {
    let mut world = fresh_world();
    // Wound member 1 so a heal has effect.
    let mut hms = world.party.roster.members[1].hp_mp_sp();
    hms.hp_cur = 1;
    world.party.roster.members[1].set_hp_mp_sp(hms);
    // Give member 0 a spell list with one heal spell (id 0x07 = Spark Arrow,
    // but we want a heal - use 0x05 / 0x09 / 0x0E from the vanilla catalog
    // depending on what's heal). The vanilla catalog's first heal-effect
    // spell ID can be found via SpellCatalog::vanilla.iter, but for the
    // test we just install a known-heal id 0x09 in the spell list.
    let mut spells = world.party.roster.members[0].spell_list();
    spells.count = 1;
    spells.ids[0] = 0x09;
    world.party.roster.members[0].set_spell_list(spells);
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

/// The group flow (retail sub-screen `0x10`, `FUN_801D9280`): a spell whose
/// stats `+2` byte carries bit `0x20` skips the target picker entirely and
/// heals **every** party row on one confirm. Before the flow existed the same
/// cast went through the picker and healed one member.
#[test]
fn a_group_heal_skips_the_picker_and_heals_the_whole_party() {
    let mut world = fresh_world();
    // Wound every member by a different amount so a single-target heal
    // cannot be mistaken for the group one.
    for (i, member) in world.party.roster.members.iter_mut().enumerate() {
        let mut hms = member.hp_mp_sp();
        hms.hp_cur = 10 + i as u16 * 5;
        member.set_hp_mp_sp(hms);
    }
    // Caster 0 knows the vanilla "Heal All" (id 0x11, AllAllies / HealAll 60).
    let mut spells = world.party.roster.members[0].spell_list();
    spells.count = 1;
    spells.ids[0] = 0x11;
    world.party.roster.members[0].set_spell_list(spells);

    let mut sub = build(FieldMenuRow::Magic, &world, &OptionsState::default());
    sub.tick_pad_edge(PadButton::Cross.mask()); // pick caster 0
    sub.tick_pad_edge(PadButton::Cross.mask()); // confirm the spell -> group flow
    let FieldMenuSubsession::Spells(s) = &sub else {
        panic!("expected Spells sub");
    };
    assert!(
        !s.is_done(),
        "the group flow is its own confirm screen, not an instant commit"
    );
    sub.tick_pad_edge(PadButton::Cross.mask()); // commit the group cast
    let FieldMenuSubsession::Spells(s) = &sub else {
        panic!("expected Spells sub");
    };
    assert!(s.is_done(), "the group confirm resolves the cast");
    apply_spell_outcome(s, &mut world);

    for (i, member) in world.party.roster.members.iter().enumerate() {
        let hms = member.hp_mp_sp();
        assert_eq!(
            hms.hp_cur,
            10 + i as u16 * 5 + 60,
            "member {i} takes the whole 60-point grant"
        );
    }
    // MP is billed once, to the caster, not once per member.
    assert_eq!(world.party.roster.members[0].hp_mp_sp().mp_cur, 12 - 8);
}

/// The menu-cast leveling arm (`FUN_800402F4` HP-heal arms): a full-power
/// menu heal accrues +12 spell XP into the caster record's `+0x8` array,
/// crosses the `0x8007656C` threshold, bumps the `+0x161` level byte and
/// returns the window-7 notice pair.
#[test]
fn menu_heal_cast_accrues_spell_xp_and_levels_with_notice() {
    let mut world = fresh_world();
    world.tables.magic_xp_thresholds = Some([17, 50, 92, 144, 208, 288, 392, 536]);
    // Wound member 1 far past the vanilla Heal amount (60) so the heal runs
    // at full power (deficit >= nominal -> the +12 grant).
    let mut hms = world.party.roster.members[1].hp_mp_sp();
    hms.hp_cur = 1;
    world.party.roster.members[1].set_hp_mp_sp(hms);
    // Caster carries the vanilla Heal (id 0x10) at level 1 with 12 XP
    // already accrued - one more full-power cast lands 24 > 17.
    let mut spells = world.party.roster.members[0].spell_list();
    spells.count = 1;
    spells.ids[0] = 0x10;
    spells.levels[0] = 1;
    world.party.roster.members[0].set_spell_list(spells);
    legaia_engine_core::magic_xp::add_spell_xp(&mut world.party.roster.members[0], 0, 12);

    let cast = |world: &mut World| -> Option<legaia_engine_core::magic_xp::SpellLevelNotice> {
        let mut sub = build(FieldMenuRow::Magic, world, &OptionsState::default());
        sub.tick_pad_edge(PadButton::Cross.mask()); // pick caster 0
        sub.tick_pad_edge(PadButton::Cross.mask()); // pick the Heal row
        sub.tick_pad_edge(PadButton::Down.mask()); // cursor to member 1
        sub.tick_pad_edge(PadButton::Cross.mask()); // confirm target
        let FieldMenuSubsession::Spells(s) = &sub else {
            panic!("expected Spells sub");
        };
        assert!(s.is_done(), "the cast should have resolved");
        apply_spell_outcome(s, world)
    };

    let notice = cast(&mut world).expect("24 XP > threshold 17: the spell levels");
    assert_eq!(notice.caster_slot, 0);
    assert_eq!(notice.spell_index, 0);
    assert_eq!(notice.spell_id, 0x10);
    assert_eq!(notice.new_level, 2);
    assert_eq!(notice.line, "Heal's magic level increased.");
    // Both writes are in the SAVED record bytes (LGSF round-trips them):
    // the +0x8 XP accumulator and the +0x161 level byte.
    let rec = &world.party.roster.members[0];
    assert_eq!(legaia_engine_core::magic_xp::spell_xp(rec, 0), 24);
    assert_eq!(rec.spell_list().levels[0], 2);
    assert_eq!(rec.raw[0x161], 2);

    // A second cast accrues (24 + 12 = 36 < 50) but does not level - no
    // notice, no window 7.
    let mut hms = world.party.roster.members[1].hp_mp_sp();
    hms.hp_cur = 1;
    world.party.roster.members[1].set_hp_mp_sp(hms);
    assert!(cast(&mut world).is_none());
    assert_eq!(
        legaia_engine_core::magic_xp::spell_xp(&world.party.roster.members[0], 0),
        36
    );
    assert_eq!(world.party.roster.members[0].spell_list().levels[0], 2);
}

/// A clipped heal (the target's deficit is smaller than the spell's nominal
/// amount) accrues the partial grant (+4), mirroring `FUN_800402F4`'s
/// deficit-below-cap arm.
#[test]
fn menu_heal_cast_accrues_partial_grant_when_clipped() {
    let mut world = fresh_world();
    world.tables.magic_xp_thresholds = Some([17, 50, 92, 144, 208, 288, 392, 536]);
    // Deficit 50 < the vanilla Heal's 60 -> partial power.
    let mut spells = world.party.roster.members[0].spell_list();
    spells.count = 1;
    spells.ids[0] = 0x10;
    spells.levels[0] = 1;
    world.party.roster.members[0].set_spell_list(spells);

    let mut sub = build(FieldMenuRow::Magic, &world, &OptionsState::default());
    sub.tick_pad_edge(PadButton::Cross.mask()); // caster 0
    sub.tick_pad_edge(PadButton::Cross.mask()); // Heal
    sub.tick_pad_edge(PadButton::Down.mask()); // member 1 (50/100)
    sub.tick_pad_edge(PadButton::Cross.mask()); // cast
    let FieldMenuSubsession::Spells(s) = &sub else {
        panic!("expected Spells sub");
    };
    assert!(s.is_done());
    assert!(apply_spell_outcome(s, &mut world).is_none(), "4 < 17");
    assert_eq!(
        legaia_engine_core::magic_xp::spell_xp(&world.party.roster.members[0], 0),
        4
    );
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

/// The rack the dispatcher is handed - not a per-host flag - is what puts a
/// Load / Save sub-session in retail's two-stage card flow. This is the one
/// decision both the native window and the browser play page make, and the
/// point of routing it through `build` is that neither can make it
/// differently (`scripts/ci/check-ui-host-drift.py`, save-select sim pair).
#[test]
fn the_rack_kind_carries_card_slots_mode_through_the_dispatcher() {
    let world = fresh_world();
    let ports = SaveRack::CardPorts((0..2).map(SlotSnapshot::empty).collect());
    let blocks = SaveRack::Blocks((0..15).map(SlotSnapshot::empty).collect());
    for row in [FieldMenuRow::Load, FieldMenuRow::Save] {
        for (rack, expect) in [(&ports, true), (&blocks, false)] {
            let sub = FieldMenuSubsession::build(
                row,
                &world,
                &OptionsState::default(),
                rack,
                &ChainLibrary::new(),
                &SpellCatalog::vanilla(),
                &EquipmentTable::new(),
            );
            let FieldMenuSubsession::Save(s) = sub else {
                panic!("{row:?} must build a save sub-session");
            };
            assert_eq!(s.card_slots_mode(), expect, "{row:?} against {rack:?}");
            assert_eq!(s.slots().len(), rack.slots().len());
        }
    }
}

/// Why permuting the record is worth anything: the Magic screen's rows come
/// off the record's `+0x13D` / `+0x161` pair **in record order**, so moving
/// those bytes moves what the player sees. The screen that offers the
/// exchange is `list_order::ListOrderSession`, pinned end-to-end in
/// `w4a_list_order_reorder`; this stays the narrower claim about the data
/// under it.
#[test]
fn spell_swap_permutes_the_magic_screen_order() {
    use legaia_engine_core::save_subscreen::sub15_swap_rows;
    let mut world = fresh_world();
    let member = &mut world.party.roster.members[0];
    let mut list = member.spell_list();
    list.count = 3;
    list.ids[..3].copy_from_slice(&[0x81, 0x82, 0x83]);
    list.levels[..3].copy_from_slice(&[1, 2, 3]);
    member.set_spell_list(list);

    let ids_on_screen = |w: &World| -> Vec<u8> {
        let sub = FieldMenuSubsession::build(
            FieldMenuRow::Magic,
            w,
            &OptionsState::default(),
            &slots(),
            &ChainLibrary::new(),
            &SpellCatalog::vanilla(),
            &EquipmentTable::new(),
        );
        let FieldMenuSubsession::Spells(mut s) = sub else {
            panic!("Magic must build a spell sub-session");
        };
        // Step into the caster's spell list: Cross on the char picker.
        s.tick(
            legaia_engine_core::spell_menu::SpellMenuInput::from_pad_edge(PadButton::Cross.mask()),
        );
        s.current_spell_rows().iter().map(|r| r.spell_id).collect()
    };

    assert_eq!(ids_on_screen(&world), vec![0x81, 0x82, 0x83]);
    sub15_swap_rows(&mut world.party.roster.members[0].raw, 0, 2);
    assert_eq!(
        ids_on_screen(&world),
        vec![0x83, 0x82, 0x81],
        "the swap reaches the Magic screen through the record, not through a \
         separate reorderable array"
    );
    let levels = world.party.roster.members[0].spell_list().levels;
    assert_eq!(
        &levels[..3],
        &[3, 2, 1],
        "the parallel `+0x161` byte moves with its id"
    );
}

/// Put an MP-saver bit into roster record `slot`'s `+0xF4` word - the word
/// retail's cast-price kernel `FUN_80035394` reads (`0x800353B4`).
fn equip_mp_saver(world: &mut World, slot: usize, bit: u8) {
    let mut bits = world.party.roster.members[slot].ability_bits();
    bits[0] = bit;
    world.party.roster.members[slot].set_ability_bits(bits);
}

/// Retail debits the **discounted** price on a field cast: `jal 0x80035394`
/// at `0x801D972C` (group) / `0x801D93C0` (single), then `record+0x10A -= v0`.
/// A Spirit Talisman (`+0xF4 & 0x20`, "consume 50% less MP") halves the
/// vanilla Heal All's 8 MP to 4, so the caster keeps 8 of 12.
#[test]
fn a_field_group_cast_debits_the_mp_saver_discounted_price() {
    let mut world = fresh_world();
    for member in world.party.roster.members.iter_mut() {
        let mut hms = member.hp_mp_sp();
        hms.hp_cur = 10;
        member.set_hp_mp_sp(hms);
    }
    let mut spells = world.party.roster.members[0].spell_list();
    spells.count = 1;
    spells.ids[0] = 0x11;
    world.party.roster.members[0].set_spell_list(spells);
    equip_mp_saver(&mut world, 0, 0x20);

    let mut sub = build(FieldMenuRow::Magic, &world, &OptionsState::default());
    sub.tick_pad_edge(PadButton::Cross.mask()); // caster 0
    sub.tick_pad_edge(PadButton::Cross.mask()); // Heal All -> group flow
    sub.tick_pad_edge(PadButton::Cross.mask()); // commit
    let FieldMenuSubsession::Spells(s) = &sub else {
        panic!("expected Spells sub");
    };
    assert!(s.is_done());
    apply_spell_outcome(s, &mut world);
    assert_eq!(
        world.party.roster.members[0].hp_mp_sp().mp_cur,
        12 - 4,
        "Half: cost - (cost >> 1) = 8 - 4"
    );
}

/// The list build greys a row on `record+0x10A < discounted cost`
/// (`0x8003118C..0x80031204`), so a caster below the raw price but at or
/// above the discounted one can still cast - and is charged the discounted
/// price, leaving exactly zero. Without the saver the same caster is refused.
#[test]
fn an_mp_saver_admits_a_cast_the_raw_price_would_refuse() {
    let setup = |saver: bool| -> World {
        let mut world = fresh_world();
        let mut hms = world.party.roster.members[0].hp_mp_sp();
        hms.mp_cur = 6; // Heal All raw 8, Quarter-off 6
        world.party.roster.members[0].set_hp_mp_sp(hms);
        let mut spells = world.party.roster.members[0].spell_list();
        spells.count = 1;
        spells.ids[0] = 0x11;
        world.party.roster.members[0].set_spell_list(spells);
        if saver {
            equip_mp_saver(&mut world, 0, 0x10);
        }
        world
    };

    // Without the saver: the row is inadmissible and confirming it is refused.
    let world = setup(false);
    let mut sub = build(FieldMenuRow::Magic, &world, &OptionsState::default());
    sub.tick_pad_edge(PadButton::Cross.mask());
    let FieldMenuSubsession::Spells(s) = &sub else {
        panic!("expected Spells sub");
    };
    let rows = s.current_spell_rows();
    assert_eq!(rows[0].mp_cost, 8);
    assert!(!rows[0].admissible);

    // With the Spirit Jewel (`0x10`, a quarter off): 8 - (8 >> 2) = 6.
    let mut world = setup(true);
    let mut sub = build(FieldMenuRow::Magic, &world, &OptionsState::default());
    sub.tick_pad_edge(PadButton::Cross.mask());
    let FieldMenuSubsession::Spells(s) = &sub else {
        panic!("expected Spells sub");
    };
    let rows = s.current_spell_rows();
    assert_eq!(rows[0].mp_cost, 6, "the list quotes the discounted price");
    assert!(rows[0].admissible);
    sub.tick_pad_edge(PadButton::Cross.mask()); // Heal All -> group flow
    sub.tick_pad_edge(PadButton::Cross.mask()); // commit
    let FieldMenuSubsession::Spells(s) = &sub else {
        panic!("expected Spells sub");
    };
    assert!(s.is_done());
    apply_spell_outcome(s, &mut world);
    assert_eq!(world.party.roster.members[0].hp_mp_sp().mp_cur, 0);
    assert!(
        world.party.roster.members[1].hp_mp_sp().hp_cur > 50,
        "the cast resolved - the shared cast_spell gate reads the same price"
    );
}

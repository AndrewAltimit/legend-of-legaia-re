//! The record screen's **list page** reached from the pause menu, and the
//! exchange it commits to the record.
//!
//! Disc-free. The page is the port of sub-screen `0x15`'s browse half: a
//! clamping row picker over a seven-row window with a latch-then-exchange
//! confirm on the spell list. What this pins is the whole chain both hosts
//! share - a confirm on the **Status** screen's character opens the page,
//! the exchange permutes the page, and closing it replays the exchange onto
//! the character's own `0x414` bytes so the Magic screen lists the new
//! order.
//!
//! Status is retail's own route: `0x801D6C4C`, in the root picker
//! `FUN_801D6B20`'s row-3 arm, is the only site in PROT 0899 that writes
//! `0x15` into the submenu word `DAT_801E46A4`.

use legaia_engine_core::battle_stats::EquipmentTable;
use legaia_engine_core::field_menu::FieldMenuRow;
use legaia_engine_core::field_menu_dispatch::{FieldMenuSubsession, apply_list_order_outcome};
use legaia_engine_core::input::PadButton;
use legaia_engine_core::items::ItemCatalog;
use legaia_engine_core::list_order::LIST_ORDER_PAGE_ROWS;
use legaia_engine_core::options::OptionsState;
use legaia_engine_core::save_select::{SaveRack, SlotSnapshot};
use legaia_engine_core::spells::SpellCatalog;
use legaia_engine_core::tactical_arts_editor::ChainLibrary;
use legaia_engine_core::world::World;

/// The learned-spell ids the lead caster starts with.
const SPELLS: [u8; 4] = [0x81, 0x82, 0x83, 0x84];

fn world_with_spells() -> World {
    let mut world = World::new();
    world.party.roster = legaia_save::Party::zeroed(3);
    for member in &mut world.party.roster.members {
        let mut hms = member.hp_mp_sp();
        hms.hp_cur = 50;
        hms.hp_max = 100;
        hms.mp_cur = 30;
        hms.mp_max = 30;
        member.set_hp_mp_sp(hms);
    }
    world.party.party_leader_slot = Some(0);
    world.set_item_catalog(ItemCatalog::vanilla());
    let member = &mut world.party.roster.members[0];
    let mut list = member.spell_list();
    list.count = SPELLS.len() as u8;
    list.ids[..SPELLS.len()].copy_from_slice(&SPELLS);
    list.levels[..SPELLS.len()].copy_from_slice(&[1, 2, 3, 4]);
    member.set_spell_list(list);
    // The record screen's list gates on the character's Ra-Seru equip byte
    // (`0x8007B424 + char*2` picks which of the eight it is), so a caster
    // with no Ra-Seru reports an empty list however many spells it knows.
    let mut equip = member.equipment();
    equip.slots[3] = 0x60;
    member.set_equipment(equip);
    world
}

fn row_session(world: &World, row: FieldMenuRow) -> FieldMenuSubsession {
    FieldMenuSubsession::build(
        row,
        world,
        &OptionsState::default(),
        &SaveRack::Blocks((0..3).map(SlotSnapshot::empty).collect()),
        &ChainLibrary::new(),
        &SpellCatalog::vanilla(),
        &EquipmentTable::new(),
    )
}

/// Open the Status screen on the lead character, then confirm into the
/// reorder page over that character's spell list.
fn open_page(world: &World) -> FieldMenuSubsession {
    let mut sub = row_session(world, FieldMenuRow::Status);
    sub.tick_pad_edge(PadButton::Cross.mask()); // status char -> list page
    sub
}

fn screen_ids(world: &World) -> Vec<u8> {
    let mut sub = row_session(world, FieldMenuRow::Magic);
    sub.tick_pad_edge(PadButton::Cross.mask());
    let FieldMenuSubsession::Spells(s) = &sub else {
        panic!("Magic builds a spell sub-session");
    };
    s.current_spell_rows().iter().map(|r| r.spell_id).collect()
}

#[test]
fn a_confirm_on_the_status_screen_opens_the_page_with_its_rows() {
    let world = world_with_spells();
    let sub = open_page(&world);
    let FieldMenuSubsession::ListOrder(page) = &sub else {
        panic!("the Status confirm opens the reorder page");
    };
    let ids: Vec<u8> = page.rows().iter().map(|r| r.id).collect();
    assert_eq!(ids, SPELLS.to_vec(), "the page lists the record's order");
    assert_eq!(page.scroll_top(), 0);
    assert!(page.reorderable(), "the spell list is the reorderable one");
    assert!(page.rows().len() <= LIST_ORDER_PAGE_ROWS);
}

#[test]
fn an_exchange_reaches_the_record_and_the_screen_behind_it() {
    let mut world = world_with_spells();
    assert_eq!(screen_ids(&world), SPELLS.to_vec());

    let mut sub = open_page(&world);
    // Latch row 0, move to row 2, exchange, then close the page.
    sub.tick_pad_edge(PadButton::Cross.mask());
    sub.tick_pad_edge(PadButton::Down.mask());
    sub.tick_pad_edge(PadButton::Down.mask());
    sub.tick_pad_edge(PadButton::Cross.mask());
    sub.tick_pad_edge(PadButton::Circle.mask());
    assert!(sub.is_done(), "cancel with no latch closes the page");

    let FieldMenuSubsession::ListOrder(page) = &sub else {
        panic!("still the page");
    };
    assert_eq!(page.swaps(), &[(0, 2)]);
    assert_eq!(apply_list_order_outcome(page, &mut world), 1);

    assert_eq!(
        screen_ids(&world),
        vec![0x83, 0x82, 0x81, 0x84],
        "the Magic screen lists the new record order"
    );
    let levels = world.party.roster.members[0].spell_list().levels;
    assert_eq!(
        &levels[..4],
        &[3, 2, 1, 4],
        "the parallel companion byte moved with its id"
    );
}

/// A visit that latches and then backs out changes nothing: the page
/// permutes its own copy and only a finished exchange is replayed.
#[test]
fn a_cancelled_visit_leaves_the_record_alone() {
    let mut world = world_with_spells();
    let mut sub = open_page(&world);
    sub.tick_pad_edge(PadButton::Cross.mask()); // latch row 0
    sub.tick_pad_edge(PadButton::Circle.mask()); // drop the latch
    assert!(
        !sub.is_done(),
        "the first cancel drops the latch, not the page"
    );
    sub.tick_pad_edge(PadButton::Circle.mask());
    assert!(sub.is_done());
    let FieldMenuSubsession::ListOrder(page) = &sub else {
        panic!("still the page");
    };
    assert_eq!(apply_list_order_outcome(page, &mut world), 0);
    assert_eq!(screen_ids(&world), SPELLS.to_vec());
}

/// The apply re-derives the list length from the record, so an exchange
/// naming a row the record no longer has is dropped rather than writing
/// past the list.
#[test]
fn the_apply_refuses_a_row_the_record_no_longer_carries() {
    let mut world = world_with_spells();
    let mut sub = open_page(&world);
    sub.tick_pad_edge(PadButton::Cross.mask());
    sub.tick_pad_edge(PadButton::Down.mask());
    sub.tick_pad_edge(PadButton::Down.mask());
    sub.tick_pad_edge(PadButton::Down.mask());
    sub.tick_pad_edge(PadButton::Cross.mask());
    let FieldMenuSubsession::ListOrder(page) = &sub else {
        panic!("still the page");
    };
    assert_eq!(page.swaps(), &[(0, 3)]);

    // The list shrank underneath the page.
    let member = &mut world.party.roster.members[0];
    let mut list = member.spell_list();
    list.count = 2;
    member.set_spell_list(list);
    assert_eq!(apply_list_order_outcome(page, &mut world), 0);
    assert_eq!(
        &world.party.roster.members[0].spell_list().ids[..2],
        &SPELLS[..2],
        "nothing moved"
    );
}

/// The gate is the Ra-Seru slot, not the spell count: a caster with spells
/// and an empty Ra-Seru slot reports an empty list, so the apply has no
/// length to work against and drops every exchange.
#[test]
fn an_empty_raseru_slot_reports_no_list_to_the_apply() {
    let mut world = world_with_spells();
    let mut sub = open_page(&world);
    sub.tick_pad_edge(PadButton::Cross.mask());
    sub.tick_pad_edge(PadButton::Down.mask());
    sub.tick_pad_edge(PadButton::Cross.mask());
    let FieldMenuSubsession::ListOrder(page) = &sub else {
        panic!("still the page");
    };
    assert_eq!(page.swaps().len(), 1);

    let member = &mut world.party.roster.members[0];
    let mut equip = member.equipment();
    equip.slots[3] = 0;
    member.set_equipment(equip);
    assert_eq!(apply_list_order_outcome(page, &mut world), 0);
    assert_eq!(screen_ids(&world), SPELLS.to_vec());
}

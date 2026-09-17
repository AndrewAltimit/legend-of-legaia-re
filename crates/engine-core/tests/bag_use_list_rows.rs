//! The Items screen's row order is the SCUS content-id-3 builder's, not an id
//! sort: `World::bag_use_rows` walks the bag's active window and splits the
//! slots into retail's three buffers.
//!
//! Disc-free - the item-effect table is a synthetic `PS-X EXE` image, which is
//! what lets the test put a chosen `(kind, class, flags)` behind a chosen id.

use legaia_asset::item_effect::{
    HEAL_AMOUNT_TABLE_VA, ItemEffectTable, RECORD_COUNT, RECORD_STRIDE, TABLE_VA,
};
use legaia_engine_core::world::{ItemBag, World};

const ITEM_TABLE_VA: u32 = 0x8007_4368;
const ITEM_RECORD_STRIDE: u32 = 0x0C;
const SEG_BASE: u32 = ITEM_TABLE_VA;
const SEG_LEN: u32 = 0x8007_6570 - ITEM_TABLE_VA;

/// `(item id, record kind, descriptor flags)`. The Use-list builder reads only
/// `kind` and the descriptor's flag byte, so the class/tier stay `0`.
type Row = (u8, u8, u8);

fn table(rows: &[Row]) -> ItemEffectTable {
    let mut buf = vec![0u8; 0x800 + SEG_LEN as usize];
    buf[0..8].copy_from_slice(b"PS-X EXE");
    buf[0x18..0x1C].copy_from_slice(&SEG_BASE.to_le_bytes());
    buf[0x1C..0x20].copy_from_slice(&SEG_LEN.to_le_bytes());
    let off = |va: u32| (va - SEG_BASE) as usize + 0x800;
    for (i, &(id, kind, flags)) in rows.iter().enumerate() {
        let subtype = (i + 1) as u8;
        assert!((subtype as usize) < RECORD_COUNT);
        let rec = off(ITEM_TABLE_VA + u32::from(id) * ITEM_RECORD_STRIDE);
        buf[rec] = kind;
        buf[rec + 1] = subtype;
        let d = off(TABLE_VA + subtype as u32 * RECORD_STRIDE as u32);
        buf[d + 2] = flags;
    }
    for tier in 0..3u32 {
        let hp = off(HEAL_AMOUNT_TABLE_VA + tier * 2);
        buf[hp..hp + 2].copy_from_slice(&200u16.to_le_bytes());
    }
    ItemEffectTable::from_scus(&buf).expect("synthetic SCUS parses")
}

/// Field-usable consumable, equipment, and the flag-`0x8` tail group.
const CONSUMABLE: u8 = 0x02;
const TAIL_GROUP: u8 = 0x08;

fn world(rows: &[Row], slots: &[(u8, u8)]) -> World {
    let mut w = World::new();
    w.load_party(legaia_save::Party::zeroed(3));
    w.set_item_effects(table(rows));
    w.party.inventory = ItemBag::from_slots(slots);
    w
}

/// Retail hoists the kind-1 (equipment) rows out of the in-place buffer, so a
/// low-id piece of gear sorts *after* a high-id consumable - the exact
/// inversion an id sort cannot produce.
#[test]
fn equipment_sorts_after_consumables_regardless_of_id() {
    let rows = [(0x20u8, 1u8, 0u8), (0x77, 2, CONSUMABLE)];
    let w = world(&rows, &[(0x20, 1), (0x77, 3)]);
    let got: Vec<u8> = w
        .bag_use_rows(false)
        .expect("effect table installed")
        .iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(got, vec![0x77, 0x20]);
}

/// The three buffers concatenate in order: in place, then kind-1, then the
/// effect-flag-`0x8` tail.
#[test]
fn the_three_buffers_concatenate_in_retail_order() {
    let rows = [
        (0x10u8, 2u8, TAIL_GROUP),
        (0x20, 1, 0),
        (0x30, 2, CONSUMABLE),
    ];
    let w = world(&rows, &[(0x10, 1), (0x20, 1), (0x30, 1)]);
    let got: Vec<u8> = w
        .bag_use_rows(false)
        .unwrap()
        .iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(got, vec![0x30, 0x20, 0x10]);
}

/// Holes in the physical array emit no row, and a row's `slot` is its physical
/// index - not its position in the list.
#[test]
fn holes_emit_no_row_and_slots_stay_physical() {
    let rows = [(0x30u8, 2u8, CONSUMABLE), (0x31, 2, CONSUMABLE)];
    let w = world(&rows, &[(0, 0), (0x30, 2), (0, 0), (0x31, 5)]);
    let got = w.bag_use_rows(false).unwrap();
    assert_eq!(got.len(), 2);
    assert_eq!((got[0].slot, got[0].id, got[0].count), (1, 0x30, 2));
    assert_eq!((got[1].slot, got[1].id, got[1].count), (3, 0x31, 5));
}

/// The two contexts differ in **order**, not only in ink: a battle-unusable
/// row sorts to the tail in battle and stays in place on the field.
#[test]
fn the_battle_context_sorts_unusable_rows_to_the_tail() {
    // 0x30 is field-only, 0x31 is battle-usable.
    let rows = [(0x30u8, 2u8, CONSUMABLE), (0x31, 2, 0x04)];
    let w = world(&rows, &[(0x30, 1), (0x31, 1)]);

    let field = w.bag_use_rows(false).unwrap();
    assert_eq!(
        field.iter().map(|r| r.id).collect::<Vec<_>>(),
        vec![0x30, 0x31]
    );

    let battle = w.bag_use_rows(true).unwrap();
    assert_eq!(
        battle.iter().map(|r| r.id).collect::<Vec<_>>(),
        vec![0x31, 0x30]
    );
    assert!(battle[1].dim, "the battle-unusable row is dimmed");
    assert!(!battle[0].dim);
}

/// A disc-free host has no descriptors to group by, so the seat answers
/// `None` and the caller keeps its own fallback rather than inventing an
/// order.
#[test]
fn no_effect_table_means_no_retail_row_order() {
    let mut w = World::new();
    w.party.inventory = ItemBag::from_slots(&[(0x30, 1)]);
    assert!(w.bag_use_rows(false).is_none());
}

/// The walk is bounded by the bag's **active window**, which is retail's
/// `gp+0x2D2..gp+0x2D4` pair: a slot outside it builds no row.
#[test]
fn the_walk_is_bounded_by_the_active_window() {
    let rows = [(0x30u8, 2u8, CONSUMABLE)];
    let mut slots = vec![(0u8, 0u8); 256];
    slots[0] = (0x30, 1);
    slots[200] = (0x30, 1);
    let mut w = world(&rows, &slots);
    w.party
        .inventory
        .set_window(legaia_save::retail_inventory::ItemWindow::Low);
    let got = w.bag_use_rows(false).unwrap();
    assert_eq!(got.len(), 1, "only the low half is walked");
    assert_eq!(got[0].slot, 0);
}

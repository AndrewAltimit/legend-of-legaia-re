//! A list row's payload is a **bag slot**, and Use / Throw Out / Sell remove
//! from that slot.
//!
//! Retail's selected payload `_DAT_8007BB88` is a slot index, the Throw Out
//! confirm zeroes `bag[cursor*2]`, and the pause item list **hides empty
//! slots** while the bag itself is never compacted on menu open - the
//! normalize helper `FUN_800423E0` has one reference disc-wide and it sits
//! behind a field-VM scene gate. So with the bag holed the row ordinal and the
//! slot diverge above the selection, and removing by row ordinal or by item id
//! takes the wrong stack.
//!
//! Disc-free: the row order needs an item-effect table, assembled here as a
//! synthetic `PS-X EXE` image.

use legaia_asset::item_effect::{
    HEAL_AMOUNT_TABLE_VA, ItemEffectTable, RECORD_COUNT, RECORD_STRIDE, TABLE_VA,
};
use legaia_engine_core::world::{ItemBag, World};

const ITEM_TABLE_VA: u32 = 0x8007_4368;
const ITEM_RECORD_STRIDE: u32 = 0x0C;
const SEG_BASE: u32 = ITEM_TABLE_VA;
const SEG_LEN: u32 = 0x8007_6570 - ITEM_TABLE_VA;
/// `0x80` base | `0x04` battle-usable | `0x02` field-usable.
const CONSUMABLE: u8 = 0x86;

/// Every id in `ids` becomes a field- and battle-usable kind-2 consumable.
fn table(ids: &[u8]) -> ItemEffectTable {
    let mut buf = vec![0u8; 0x800 + SEG_LEN as usize];
    buf[0..8].copy_from_slice(b"PS-X EXE");
    buf[0x18..0x1C].copy_from_slice(&SEG_BASE.to_le_bytes());
    buf[0x1C..0x20].copy_from_slice(&SEG_LEN.to_le_bytes());
    let off = |va: u32| (va - SEG_BASE) as usize + 0x800;
    for (i, &id) in ids.iter().enumerate() {
        let subtype = (i + 1) as u8;
        assert!((subtype as usize) < RECORD_COUNT);
        let rec = off(ITEM_TABLE_VA + u32::from(id) * ITEM_RECORD_STRIDE);
        buf[rec] = 2;
        buf[rec + 1] = subtype;
        let d = off(TABLE_VA + subtype as u32 * RECORD_STRIDE as u32);
        buf[d + 2] = CONSUMABLE;
    }
    for tier in 0..3u32 {
        let hp = off(HEAL_AMOUNT_TABLE_VA + tier * 2);
        buf[hp..hp + 2].copy_from_slice(&200u16.to_le_bytes());
    }
    ItemEffectTable::from_scus(&buf).expect("synthetic SCUS parses")
}

/// A bag holed at slots 1 / 3 / 6, exactly the shape the capture measured: six
/// occupied slots (0, 2, 4, 5, 7, 8) behind six displayed rows.
fn holed_bag() -> ItemBag {
    let mut slots = vec![(0u8, 0u8); 256];
    // Real retail ids, so the vanilla catalog resolves an effect for each
    // (a Use bounces on an id the catalog does not know).
    slots[0] = (0x77, 1); // Healing Leaf
    slots[2] = (0x78, 1); // Healing Flower
    slots[4] = (0x79, 3); // Healing Berry
    slots[5] = (0x7C, 1);
    slots[7] = (0x7D, 1);
    // Slot 8 repeats slot 4's id: a second stack of one id, which an
    // id-addressed removal cannot tell apart from the first.
    slots[8] = (0x79, 9);
    ItemBag::from_slots(&slots)
}

const BAG_IDS: [u8; 5] = [0x77, 0x78, 0x79, 0x7C, 0x7D];

fn world() -> World {
    let mut w = World::new();
    let mut party = legaia_save::Party::zeroed(3);
    // A wounded lead member, so a heal has somewhere to land.
    let mut hms = party.members[0].hp_mp_sp();
    hms.hp_cur = 10;
    hms.hp_max = 100;
    party.members[0].set_hp_mp_sp(hms);
    w.load_party(party);
    w.set_item_effects(table(&BAG_IDS));
    w.set_item_catalog(legaia_engine_core::items::ItemCatalog::vanilla());
    w.party.inventory = holed_bag();
    w
}

/// The rows skip the holes and carry the physical slot, so the sixth row is
/// slot 8 - not slot 5.
#[test]
fn displayed_rows_skip_holes_and_carry_the_physical_slot() {
    let w = world();
    let rows = w.bag_use_rows(false).expect("effect table installed");
    let got: Vec<(u8, u8)> = rows.iter().map(|r| (r.slot, r.id)).collect();
    assert_eq!(
        got,
        vec![
            (0, 0x77),
            (2, 0x78),
            (4, 0x79),
            (5, 0x7C),
            (7, 0x7D),
            (8, 0x79),
        ]
    );
}

/// Throwing out the sixth **row** zeroes slot 8 - the payload's slot - and
/// leaves slot 4's stack of the same id untouched.
#[test]
fn throw_out_zeroes_the_payloads_slot_not_the_first_stack_of_that_id() {
    let mut w = world();
    let mut session = legaia_engine_core::field_menu_dispatch::build_pause_items_session(&w);
    assert_eq!(session.rows.len(), 6);
    assert_eq!(session.rows[5].slot, 8);

    // Drive the screen: Throw Out, step to the last row, confirm Yes.
    use legaia_engine_core::input::PadButton as P;
    session.input_pad_edge(P::Down.mask()); // command window: Use -> Throw Out
    session.input_pad_edge(P::Cross.mask());
    for _ in 0..5 {
        session.input_pad_edge(P::Down.mask());
    }
    session.input_pad_edge(P::Cross.mask()); // opens the Yes / No window
    session.input_pad_edge(P::Up.mask()); // default is No; step to Yes
    session.input_pad_edge(P::Cross.mask());

    assert_eq!(session.inner.thrown_items, vec![0x79]);
    assert_eq!(session.inner.thrown_slots, vec![8]);

    legaia_engine_core::field_menu_dispatch::apply_pause_items_outcome(&session, &mut w);
    assert_eq!(w.party.inventory.slot(8), (0, 0), "the payload's slot");
    assert_eq!(
        w.party.inventory.slot(4),
        (0x79, 3),
        "the other stack of the same id is untouched"
    );
}

/// The same law for a **Use**: the unit comes off the slot the row named.
#[test]
fn a_use_spends_the_payloads_slot() {
    let mut w = world();
    let before_low = w.party.inventory.slot(4);
    let mut session = legaia_engine_core::field_menu_dispatch::build_pause_items_session(&w);

    use legaia_engine_core::input::PadButton as P;
    session.input_pad_edge(P::Cross.mask()); // command window: Use
    for _ in 0..5 {
        session.input_pad_edge(P::Down.mask()); // row 5 = slot 8, the 2nd 0x79
    }
    session.input_pad_edge(P::Cross.mask()); // enter target select
    session.input_pad_edge(P::Cross.mask()); // use on the first ally

    assert_eq!(session.inner.used_item, Some(0x79));
    assert_eq!(session.inner.used_slot, Some(8));

    legaia_engine_core::field_menu_dispatch::apply_pause_items_outcome(&session, &mut w);
    assert_eq!(w.party.inventory.slot(8), (0x79, 8), "one off slot 8");
    assert_eq!(
        w.party.inventory.slot(4),
        before_low,
        "the lower stack of the same id is untouched"
    );
}

/// A session built without a slot-indexed row list keeps the id-addressed
/// removal, so a disc-free host is not silently changed.
#[test]
fn without_an_effect_table_the_session_carries_no_slots() {
    let mut w = World::new();
    w.load_party(legaia_save::Party::zeroed(1));
    w.set_item_catalog(legaia_engine_core::items::ItemCatalog::vanilla());
    w.party.inventory = holed_bag();
    let session = legaia_engine_core::field_menu_dispatch::build_pause_items_session(&w);
    assert!(session.inner.bag_slots.is_empty());
    assert!(session.rows.iter().all(|r| r.slot == 0));
}

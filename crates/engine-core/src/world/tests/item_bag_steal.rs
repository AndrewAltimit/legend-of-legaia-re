//! The party bag as retail's 256-slot array, and the one consumer that
//! indexes it by slot: PROT 0941's Steal.
//!
//! Four properties, each of which the map-shaped bag could not express at
//! all - occupancy with holes, the squeeze, the shop-price acceptance leg,
//! and the window asymmetry between the draw (whole array) and the removal
//! (`[gp[+0x2D2], gp[+0x2D4])`).

use super::*;
use legaia_engine_cast::cast_arm_ticks::StealOutcome;
use legaia_save::retail_inventory::{ITEM_SLOTS_TOTAL, ItemWindow, NOT_IN_WINDOW};

/// A party-seat world whose bag holds exactly the given `(slot, id, count)`
/// cells, every id priced, and whose RNG is seeded to draw `first_slot` on
/// its first roll.
fn steal_world(cells: &[(usize, u8, u8)], rng_seed: u32) -> World {
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 3,
            ..Default::default()
        },
        ..World::default()
    };
    world.rng_state = rng_seed;
    let mut slots = vec![(0u8, 0u8); ITEM_SLOTS_TOTAL];
    for &(slot, id, count) in cells {
        slots[slot] = (id, count);
    }
    world.party.inventory = legaia_engine_menus::item_bag::ItemBag::from_slots(&slots);
    // Every id priced: the acceptance leg then depends only on the slot's
    // own two bytes, which is what the hole tests want to isolate.
    world.shops.item_shop_data =
        Some(legaia_engine_menus::shop_catalog::ShopItemData::from_prices([10; 256]));
    world
}

/// The world seed whose first retail draw (`World::next_rand`, the BIOS
/// `rand()` shape) `% 0x100` is `slot`. Derived off the stream by hand:
/// `s' = s * 0x41C64E6D + 0x3039`, draw = `(s' >> 16) & 0x7FFF`, and the
/// slot is the draw's low byte - seed 0 -> draw `0x0000`, seed 316 ->
/// `0x30C8`, seed 31 -> `0x7703`.
const SEED_DRAWS_SLOT_0: u32 = 0;

const SEED_DRAWS_SLOT_200: u32 = 316;

const SEED_DRAWS_SLOT_3: u32 = 31;

#[test]
fn the_bag_is_slot_indexed_with_holes() {
    let world = steal_world(&[(3, 0x77, 5), (200, 0x78, 4)], 1);
    assert_eq!(world.party.inventory.slots().len(), ITEM_SLOTS_TOTAL);
    assert_eq!(world.party.inventory.slot(3), (0x77, 5));
    assert_eq!(world.party.inventory.slot(200), (0x78, 4));
    assert_eq!(world.party.inventory.slot(0), (0, 0), "slot 0 is a hole");
    // The map-shaped view is unchanged by the holes.
    assert_eq!(world.party.inventory.get(&0x77), Some(&5));
    assert_eq!(world.party.inventory.len(), 2);
    // Slot order, not id order: the adapter's iteration is the walk retail's
    // pages make.
    let ids: Vec<u8> = world.party.inventory.keys().copied().collect();
    assert_eq!(ids, vec![0x77, 0x78]);
}

#[test]
fn normalize_merges_and_squeezes_into_slot_order() {
    let mut world = steal_world(&[(3, 0x77, 5), (9, 0x77, 4), (200, 0x78, 1)], 1);
    world.party.inventory.normalize();
    assert_eq!(
        world.party.inventory.slot(0),
        (0x77, 9),
        "the duplicate stacks merged into the first slot"
    );
    assert_eq!(world.party.inventory.slot(1), (0x78, 1));
    assert_eq!(world.party.inventory.slot(3), (0, 0), "the hole moved up");
}

#[test]
fn the_draw_reaches_a_slot_no_projection_would_have() {
    // One item, at slot 200. A dense projection of the occupied ids would
    // have put it at slot 0 and accepted the first draw; retail's sampler
    // has to land on 200 itself.
    let mut world = steal_world(&[(200, 0x78, 4)], SEED_DRAWS_SLOT_200);
    assert_eq!(
        world.roll_cast_steal(0),
        Some(StealOutcome::FromBag { item: 0x78 })
    );
}

#[test]
fn a_draw_onto_a_hole_is_rejected_not_accepted() {
    // The same bag, but the first draw lands on slot 0 - a hole. With the
    // budget cut to one draw's worth of luck this would be `BagEmpty`; with
    // the full budget the sampler rejects its way to slot 200 eventually.
    // What is asserted here is the rejection itself: the outcome is never
    // "the item at slot 0", because there is no item at slot 0.
    let mut world = steal_world(&[(200, 0x78, 4)], SEED_DRAWS_SLOT_0);
    match world.roll_cast_steal(0) {
        Some(StealOutcome::FromBag { item }) => assert_eq!(item, 0x78),
        Some(StealOutcome::BagEmpty) => {}
        other => panic!("unexpected steal outcome {other:?}"),
    }
}

#[test]
fn a_zero_price_item_is_unstealable() {
    let mut world = steal_world(&[(3, 0x77, 5)], SEED_DRAWS_SLOT_3);
    // Priced: the draw takes it.
    assert_eq!(
        world.clone_for_price_test().roll_cast_steal(0),
        Some(StealOutcome::FromBag { item: 0x77 })
    );
    // Quest / found-only: the item record's `+2` shop price is zero, which
    // is retail's third acceptance leg (`0x801F789C`), so the whole budget
    // rejects and nothing is taken.
    let mut prices = [10u16; 256];
    prices[0x77] = 0;
    world.shops.item_shop_data =
        Some(legaia_engine_menus::shop_catalog::ShopItemData::from_prices(prices));
    assert_eq!(world.roll_cast_steal(0), Some(StealOutcome::BagEmpty));
}

#[test]
fn the_removal_is_window_bounded_and_the_draw_is_not() {
    // The item sits in the high half; the low-half window is installed (a
    // lone Vahn). The draw is over the whole array, so it can pick it - and
    // the removal, which scans only the window, declines.
    let mut world = steal_world(&[(200, 0x78, 4)], SEED_DRAWS_SLOT_200);
    world.party.inventory.set_window(ItemWindow::Low);
    assert_eq!(
        world.roll_cast_steal(0),
        Some(StealOutcome::FromBag { item: 0x78 }),
        "the draw sees the whole array"
    );
    assert!(
        !world.take_one_from_bag(0x78),
        "the removal reports the out-of-window sentinel"
    );
    assert_eq!(
        world.party.inventory.slot(200),
        (0x78, 4),
        "and the party keeps the item it was told it lost"
    );
    // The same removal inside the window does take one.
    world.party.inventory.set_window(ItemWindow::Full);
    assert!(world.take_one_from_bag(0x78));
    assert_eq!(world.party.inventory.slot(200), (0x78, 3));
}

#[test]
fn the_consume_sentinel_is_the_retail_value() {
    let mut bag = legaia_engine_menus::item_bag::ItemBag::new();
    bag.insert(0x77, 2);
    assert_eq!(bag.consume_returning_slot(0x99, 1), NOT_IN_WINDOW);
    assert_ne!(bag.consume_returning_slot(0x77, 1), NOT_IN_WINDOW);
    assert_eq!(bag.get(&0x77), Some(&1));
}

impl World {
    /// A shallow copy for the priced/unpriced A/B above: only the bag, the
    /// price table and the RNG matter to `roll_cast_steal` on a party seat.
    fn clone_for_price_test(&self) -> World {
        let mut w = World {
            party: legaia_engine_menus::party_state::PartyState {
                party_count: self.party.party_count,
                ..Default::default()
            },
            ..World::default()
        };
        w.rng_state = self.rng_state;
        w.party.inventory = self.party.inventory.clone();
        w.shops.item_shop_data = self.shops.item_shop_data.clone();
        w
    }
}

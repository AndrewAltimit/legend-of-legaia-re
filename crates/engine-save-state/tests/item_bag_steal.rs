//! The party bag as retail's 256-slot array, and the one consumer that
//! indexes it by slot: PROT 0941's Steal.
//!
//! Four properties, each of which the map-shaped bag could not express at
//! all - occupancy with holes, the squeeze, the shop-price acceptance leg,
//! and the window asymmetry between the draw (whole array) and the removal
//! (`[gp[+0x2D2], gp[+0x2D4])`).

use legaia_engine_core::world::*;
use legaia_engine_save_state::WorldSaveState;
use legaia_save::retail_inventory::ITEM_SLOTS_TOTAL;

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

#[test]
fn the_slot_array_survives_a_world_save_and_load() {
    // A bag with holes and a high slot: the two shapes a compacted save
    // silently normalises away.
    let mut w = steal_world(&[(3, 0x77, 5), (200, 0x78, 4)], 1);
    let before: Vec<(u8, u8)> = w.party.inventory.slots().to_vec();
    let sf = w.save_full();
    assert_eq!(sf.ext.item_slots, before, "the save carries the array");
    // Through the file bytes, not just the struct.
    let bytes = sf.write();
    let back = legaia_save::SaveFile::parse(&bytes).expect("parse LGSF");
    let mut w2 = World::new();
    w2.load_full(back);
    assert_eq!(
        w2.party.inventory.slots(),
        before.as_slice(),
        "slot order and holes came back"
    );
    assert_eq!(w2.party.inventory.slot(200), (0x78, 4));
    assert_eq!(w2.party.inventory.slot(0), (0, 0));
}

#[test]
fn a_save_with_no_slot_array_still_loads_densely() {
    // The pre-`LGX6` shape: the compact list alone. A loader that required
    // the array would drop every legacy save's bag.
    let mut sf = World::new().save_full();
    sf.ext.item_slots.clear();
    sf.ext.inventory = vec![(0x77, 5), (0x78, 4)];
    let mut w = World::new();
    w.load_full(sf);
    assert_eq!(w.party.inventory.slot(0), (0x77, 5));
    assert_eq!(w.party.inventory.slot(1), (0x78, 4));
    assert_eq!(w.party.inventory.len(), 2);
}

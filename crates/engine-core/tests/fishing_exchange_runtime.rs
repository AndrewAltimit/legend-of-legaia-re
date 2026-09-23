//! Disc-free runtime oracle for the fishing point-exchange grant kernel:
//! [`World::fishing_exchange_buy`] against synthetic venue rows shaped like
//! the retail tables (a one-time top prize + repeatable stock; see
//! `legaia_asset::fishing_exchange` for the on-disc layout the shapes mirror).

use legaia_engine_core::fishing::{FishingTables, PrizeExchange};
use legaia_engine_core::world::World;

fn venue(page: usize) -> PrizeExchange {
    let rows: Vec<_> = [
        (1u32, 20_000u32, 0x6Fu32),
        (1, 4_000, 0xC5),
        (99, 200, 0x98),
    ]
    .iter()
    .enumerate()
    .map(
        |(row, &(limit, price, item_id))| legaia_asset::fishing_exchange::ExchangeRow {
            row,
            limit,
            price,
            item_id,
        },
    )
    .collect();
    PrizeExchange::from_asset(page, &rows, None)
}

#[test]
fn buy_commits_points_mask_and_inventory() {
    let mut world = World::new();
    world.minigames.fishing_points = 5_000;
    world.open_fishing_exchange(venue(1));

    // The 20k top prize is hidden/unavailable at 5k points; the cursor
    // floored past it on open.
    assert_eq!(world.minigames.fishing_exchange.as_ref().unwrap().cursor, 1);
    assert!(world.fishing_exchange_buy(0, 1).is_none());

    // One-time prize: grants, deducts, latches the venue-1 bit block.
    let p = world.fishing_exchange_buy(1, 1).expect("one-time buys");
    assert_eq!((p.item_id, p.cost), (0xC5, 4_000));
    assert_eq!(world.minigames.fishing_points, 1_000);
    assert_eq!(world.minigames.fishing_prizes_purchased, 1 << 9);
    assert_eq!(world.party.inventory.get(&0xC5), Some(&1));
    // Re-buying the latched row refuses even though it's affordable.
    assert!(world.fishing_exchange_buy(1, 1).is_none());

    // Repeatable stock: buys up to the point pool.
    let p = world.fishing_exchange_buy(2, 4).expect("repeatable buys");
    assert_eq!((p.qty, p.cost), (4, 800));
    assert_eq!(world.minigames.fishing_points, 200);
    assert_eq!(world.party.inventory.get(&0x98), Some(&4));
    // Over-quantity refuses (only 1 more affordable).
    assert!(world.fishing_exchange_buy(2, 2).is_none());
}

#[test]
fn exchange_syncs_live_session_and_exit_banks_points() {
    let mut world = World::new();
    world.minigames.fishing_points = 1_000;
    // An empty table set: the session never hooks, which this test does not
    // need - it is about the point pool and the one-time mask.
    let tables = FishingTables {
        species: Vec::new(),
        spawn: [Vec::new(), Vec::new()],
        cadence: Vec::new(),
    };
    world.enter_fishing_session(&tables, 0, None);
    world.open_fishing_exchange(venue(0));
    world.fishing_exchange_buy(2, 3).expect("buys");
    // The live session's on-screen total follows the pool.
    assert_eq!(world.minigames.fishing.as_ref().unwrap().record.points, 400);
    // Leaving fishing banks the session total back and closes the list.
    let s = world.exit_fishing().expect("session");
    assert_eq!(s.record.points, 400);
    assert_eq!(world.minigames.fishing_points, 400);
    assert!(world.minigames.fishing_exchange.is_none());
}

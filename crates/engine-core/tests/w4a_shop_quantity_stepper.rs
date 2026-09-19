//! The shop **quantity screen** as retail runs it: one number stepping in
//! place, bounded, committing straight into the transaction.
//!
//! Disc-free. What it pins is the wiring, not the arithmetic (the two
//! sessions' own unit tests already cover the decode and the clamps): that
//! `MenuRuntime` installs the retail stepper when a list stages a stack,
//! that the pad reaches it instead of the menu VM's list cursor, that the
//! confirm lands the purchase or sale with no Yes/No screen in between, and
//! that a cancel hands the pad back to the list it came from.

use legaia_engine_core::menu_runtime::{MenuInput, MenuRuntime, MenuState};
use legaia_engine_core::shop::{POINT_CARD_ITEM_ID, ShopInventory, ShopItem, ShopSession};
use legaia_engine_core::world::World;
use legaia_save::{CharacterRecord, Party};

const POTION: u8 = 0x77;
const PRICE: u32 = 50;

fn world_with_gold(gold: i32) -> World {
    let mut world = World::default();
    world.load_party(Party {
        members: vec![CharacterRecord::zeroed()],
    });
    world.party.money = gold;
    world
}

fn shop() -> ShopSession {
    ShopSession::new(ShopInventory::new(
        1,
        vec![ShopItem {
            item_id: POTION,
            price: PRICE,
        }],
    ))
}

fn press(b: fn(&mut MenuInput)) -> MenuInput {
    let mut i = MenuInput::default();
    b(&mut i);
    i
}

fn cross() -> MenuInput {
    press(|i| i.cross = true)
}
fn circle() -> MenuInput {
    press(|i| i.circle = true)
}
fn right() -> MenuInput {
    press(|i| i.right = true)
}
fn down() -> MenuInput {
    press(|i| i.down = true)
}

/// Drive the buy list's first row to the quantity screen.
fn open_buy_quantity(world: &mut World, runtime: &mut MenuRuntime) {
    runtime.open_shop_menu(shop());
    // Rows = [Buy, Sell, Exit]; row 0 is Buy.
    runtime.tick(world, cross());
    assert_eq!(runtime.ctx.state, MenuState::ShopBuy.as_byte());
    runtime.tick(world, cross());
    assert_eq!(
        runtime.ctx.state,
        MenuState::ShopQuantity.as_byte(),
        "a stackable buy row routes to the quantity screen"
    );
}

#[test]
fn the_buy_list_installs_the_retail_stepper_at_quantity_one() {
    let mut world = world_with_gold(1_000);
    let mut runtime = MenuRuntime::new("/tmp/legaia-w4a");
    open_buy_quantity(&mut world, &mut runtime);
    // One tick of the picker runs retail's phase 0 (seed + window script).
    runtime.tick(&mut world, MenuInput::default());
    let view = runtime
        .quantity_view()
        .expect("the stepper owns the screen");
    assert!(view.buying);
    assert_eq!(view.item_id, POTION);
    assert_eq!(view.quantity, 1, "retail seeds the number at one");
    assert_eq!(view.price as u32, PRICE);
    // `min(gold / price, 99, 99 - held)` = 20 at 1000 gold and price 50.
    assert_eq!(view.max, 20, "the bound is the purse, not a flat nine");
}

#[test]
fn the_pad_steps_the_number_in_place_and_the_list_takes_no_cursor() {
    let mut world = world_with_gold(1_000);
    let mut runtime = MenuRuntime::new("/tmp/legaia-w4a");
    open_buy_quantity(&mut world, &mut runtime);
    runtime.tick(&mut world, MenuInput::default());
    let cursor_before = runtime.ctx.cursor;
    for _ in 0..3 {
        runtime.tick(&mut world, right());
    }
    assert_eq!(runtime.quantity_view().unwrap().quantity, 4);
    // Down is retail's ten-step, clamped to the bound.
    runtime.tick(&mut world, down());
    assert_eq!(runtime.quantity_view().unwrap().quantity, 14);
    for _ in 0..3 {
        runtime.tick(&mut world, down());
    }
    assert_eq!(
        runtime.quantity_view().unwrap().quantity,
        20,
        "a step past the bound clamps instead of wrapping"
    );
    assert_eq!(
        runtime.ctx.cursor, cursor_before,
        "the list cursor is not what the pad is moving"
    );
}

#[test]
fn the_confirm_buys_the_stepped_stack_with_no_confirm_screen() {
    let mut world = world_with_gold(1_000);
    let mut runtime = MenuRuntime::new("/tmp/legaia-w4a");
    open_buy_quantity(&mut world, &mut runtime);
    runtime.tick(&mut world, MenuInput::default());
    for _ in 0..2 {
        runtime.tick(&mut world, right());
    }
    assert_eq!(runtime.quantity_view().unwrap().quantity, 3);
    runtime.tick(&mut world, cross());
    // The commit and the return happen without passing through ShopConfirm.
    // Retail splits the commit across its own phases (accrual, then the bag
    // add and purse store, then the return), so spin frames rather than
    // assuming one.
    for _ in 0..4 {
        if runtime.quantity_view().is_none() {
            break;
        }
        assert_ne!(
            runtime.ctx.state,
            MenuState::ShopConfirm.as_byte(),
            "the stepper never routes through the Yes/No screen"
        );
        runtime.tick(&mut world, MenuInput::default());
    }
    assert!(
        runtime.quantity_view().is_none(),
        "the stepper closed after its commit"
    );
    assert_eq!(
        runtime.ctx.state,
        MenuState::ShopBuy.as_byte(),
        "the flow returns to the list, not to a Yes/No screen"
    );
    assert_eq!(world.party.inventory.get(&POTION).copied(), Some(3));
    assert_eq!(world.party.money, 1_000 - 3 * PRICE as i32);
}

/// A Point Card buy runs retail's toast beat inside the picker: the session
/// holds the press itself, so the flag the host paints window 31 from must
/// go out with the session - otherwise the buy list stalls for a second
/// press nothing is waiting on.
#[test]
fn the_point_card_toast_ends_with_the_picker() {
    let mut world = world_with_gold(1_000);
    world.party.inventory.insert(POINT_CARD_ITEM_ID, 1);
    let mut runtime = MenuRuntime::new("/tmp/legaia-w4a");
    open_buy_quantity(&mut world, &mut runtime);
    runtime.tick(&mut world, MenuInput::default());
    runtime.tick(&mut world, cross());
    // Commit frame: the accrual lands and the toast arms.
    runtime.tick(&mut world, MenuInput::default());
    assert!(runtime.point_card_toast().is_some(), "the toast armed");
    assert!(world.minigames.point_card > 0, "the bank took the credit");
    // The picker consumes the dismissing press itself.
    for _ in 0..4 {
        if runtime.quantity_view().is_none() {
            break;
        }
        runtime.tick(&mut world, cross());
    }
    assert!(runtime.quantity_view().is_none());
    assert!(
        runtime.point_card_toast().is_none(),
        "the toast does not outlive the picker"
    );
    assert_eq!(runtime.ctx.state, MenuState::ShopBuy.as_byte());
}

#[test]
fn a_cancel_hands_the_pad_back_to_the_list_and_buys_nothing() {
    let mut world = world_with_gold(1_000);
    let mut runtime = MenuRuntime::new("/tmp/legaia-w4a");
    open_buy_quantity(&mut world, &mut runtime);
    runtime.tick(&mut world, MenuInput::default());
    runtime.tick(&mut world, right());
    runtime.tick(&mut world, circle());
    assert!(runtime.quantity_view().is_none());
    assert_eq!(runtime.ctx.state, MenuState::ShopBuy.as_byte());
    assert_eq!(world.party.inventory.get(&POTION).copied(), None);
    assert_eq!(world.party.money, 1_000);
}

/// A purse that cannot afford one copy leaves the screen with no stepper -
/// retail's bound is zero there, and a zero-bound stepper would offer a
/// purchase the commit would then refuse.
#[test]
fn a_short_purse_opens_no_stepper() {
    let mut world = world_with_gold(10);
    let mut runtime = MenuRuntime::new("/tmp/legaia-w4a");
    runtime.open_shop_menu(shop());
    runtime.tick(&mut world, cross());
    runtime.tick(&mut world, cross());
    assert!(runtime.quantity_view().is_none());
    assert_eq!(world.party.money, 10);
}

#[test]
fn the_sell_list_installs_the_stepper_bounded_by_the_staged_stack() {
    let mut world = world_with_gold(0);
    world.party.inventory.insert(POTION, 4);
    let mut runtime = MenuRuntime::new("/tmp/legaia-w4a");
    runtime.open_shop_menu(shop());
    // Rows = [Buy, Sell, Exit]; row 1 is Sell.
    runtime.tick(&mut world, down());
    runtime.tick(&mut world, cross());
    assert_eq!(runtime.ctx.state, MenuState::ShopSell.as_byte());
    runtime.tick(&mut world, cross());
    assert_eq!(runtime.ctx.state, MenuState::ShopQuantity.as_byte());
    runtime.tick(&mut world, MenuInput::default());
    let view = runtime.quantity_view().expect("the sell stepper is up");
    assert!(!view.buying);
    assert_eq!(view.max, 4, "the bound is the staged stack's count");

    // Sell two: the purse gains half the list price per copy.
    runtime.tick(&mut world, right());
    runtime.tick(&mut world, cross());
    runtime.tick(&mut world, MenuInput::default());
    assert_eq!(world.party.money, PRICE as i32);
    assert_eq!(world.party.inventory.get(&POTION).copied(), Some(2));
    assert_eq!(runtime.ctx.state, MenuState::ShopSell.as_byte());
}

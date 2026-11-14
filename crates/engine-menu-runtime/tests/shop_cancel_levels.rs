//! Circle backs out of every level of the shop, one level a press, and the
//! cancel cue sounds once for each.
//!
//! Retail's menus test the pad edge against the cancel mask word
//! `0x800846D4`, which the new-game init seeds to `0x21` - Circle | L2
//! (`FUN_80034A6C`, `0x80034AA4..0x80034AA8`). The shop's two lists read it in
//! the list kernel (`FUN_80032A44`, `0x80032D5C..0x80032D74`: cue `0x37`), and
//! its mode select, recipient picker and steppers through `FUN_801D688C` and
//! their own decodes. So Circle is the shop's cancel at every level.
//!
//! The defect this file pins was two readers of one press: the cue path
//! sounded the cancel on Circle while the screen's router only moved on
//! Triangle, so the shop blipped and stayed put. Both now read
//! `MenuInput::cancel`. Triangle, the engine's second cancel, is driven
//! through the same levels so the alias cannot drift from the real button.
//!
//! Disc-free.

use legaia_engine_core::world::World;
use legaia_engine_menu_runtime::menu_runtime::MenuRuntime;
use legaia_engine_menus::equipment::{DiscEquipEntry, DiscEquipInfo};
use legaia_engine_menus::field_menu::{FieldMenuInput, FieldMenuPhase, FieldMenuSession};
use legaia_engine_menus::menu::{MenuInput, MenuState};
use legaia_engine_menus::shop::{POINT_CARD_ITEM_ID, ShopInventory, ShopItem, ShopSession};
use legaia_game_tables::equip_stats::EquipSlot as Disc;
use legaia_save::{CharacterRecord, Party};

/// Retail's cancel cue (`FUN_80032A44` at `0x80032D74`).
const CANCEL_CUE: u8 = 0x37;

const POTION: u8 = 0x77;
const ARMOUR: u8 = 0x44;

fn world() -> World {
    let mut world = World::default();
    world.load_party(Party {
        members: vec![CharacterRecord::zeroed(), CharacterRecord::zeroed()],
    });
    world.party.money = 10_000;
    world.party.inventory.insert(POTION, 5);
    world
}

/// A counter selling one stackable item and one piece of body armour.
fn runtime() -> MenuRuntime {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut rt = MenuRuntime::new(tmp.path().to_path_buf());
    rt.retail_equipment_buy = true;
    rt.install_equip_info(DiscEquipInfo::from_entries([(
        ARMOUR,
        DiscEquipEntry {
            mask: 0b111,
            category: Disc::Body,
            is_ra_seru: false,
            passive_index: None,
        },
    )]));
    rt.open_shop_menu(ShopSession::new(ShopInventory::new(
        1,
        vec![
            ShopItem {
                item_id: POTION,
                price: 50,
            },
            ShopItem {
                item_id: ARMOUR,
                price: 400,
            },
        ],
    )));
    rt
}

fn cross() -> MenuInput {
    MenuInput {
        cross: true,
        ..MenuInput::default()
    }
}
fn down() -> MenuInput {
    MenuInput {
        down: true,
        ..MenuInput::default()
    }
}
fn circle() -> MenuInput {
    MenuInput {
        circle: true,
        ..MenuInput::default()
    }
}
fn triangle() -> MenuInput {
    MenuInput {
        triangle: true,
        ..MenuInput::default()
    }
}

fn state(rt: &MenuRuntime) -> Option<MenuState> {
    MenuState::from_byte(rt.ctx.state)
}

/// One cancel press, then an idle frame: the cue the press sounded, with the
/// idle frame proving it did not sound again.
fn cancel_once(rt: &mut MenuRuntime, w: &mut World, cancel: MenuInput) -> Option<u8> {
    rt.tick(w, cancel);
    let cue = rt.take_ui_cue();
    assert_eq!(rt.take_ui_cue(), None, "a cue is taken once");
    rt.tick(w, MenuInput::default());
    assert_eq!(
        rt.take_ui_cue(),
        None,
        "the cancel sounds on its own frame only"
    );
    cue
}

/// Back out from the buy or sell list to the field: list -> mode select ->
/// the exit beat -> closed. Two presses, two cues.
fn back_out_from_list(rt: &mut MenuRuntime, w: &mut World, cancel: MenuInput) {
    assert_eq!(cancel_once(rt, w, cancel), Some(CANCEL_CUE));
    assert_eq!(state(rt), Some(MenuState::ShopMenu), "list -> mode select");
    rt.tick(w, cancel);
    assert_eq!(rt.take_ui_cue(), Some(CANCEL_CUE));
    assert_eq!(state(rt), Some(MenuState::ShopExit), "mode select -> exit");
    // The exit beat takes no input and sounds nothing.
    for _ in 0..200 {
        if !rt.is_open() {
            break;
        }
        rt.tick(w, MenuInput::default());
        assert_eq!(rt.take_ui_cue(), None, "the exit beat is silent");
    }
    assert!(!rt.is_open(), "the shop closed");
    assert!(rt.shop_session.is_none());
}

fn both_cancels(run: fn(MenuInput)) {
    run(circle());
    run(triangle());
}

/// The mode select (Buy / Sell / Quit): a cancel leaves the shop.
#[test]
fn cancel_on_the_mode_select_leaves_the_shop() {
    both_cancels(|cancel| {
        let (mut w, mut rt) = (world(), runtime());
        assert_eq!(state(&rt), Some(MenuState::ShopMenu));
        rt.tick(&mut w, cancel);
        assert_eq!(rt.take_ui_cue(), Some(CANCEL_CUE));
        assert_eq!(state(&rt), Some(MenuState::ShopExit));
    });
}

/// The buy list: a cancel returns to the mode select, a second leaves.
#[test]
fn cancel_on_the_buy_list_backs_out_one_level() {
    both_cancels(|cancel| {
        let (mut w, mut rt) = (world(), runtime());
        rt.tick(&mut w, cross());
        assert_eq!(state(&rt), Some(MenuState::ShopBuy));
        let _ = rt.take_ui_cue();
        back_out_from_list(&mut rt, &mut w, cancel);
    });
}

/// The sell list: a cancel returns to the mode select, a second leaves.
#[test]
fn cancel_on_the_sell_list_backs_out_one_level() {
    both_cancels(|cancel| {
        let (mut w, mut rt) = (world(), runtime());
        rt.tick(&mut w, down());
        rt.tick(&mut w, cross());
        assert_eq!(state(&rt), Some(MenuState::ShopSell));
        let _ = rt.take_ui_cue();
        back_out_from_list(&mut rt, &mut w, cancel);
    });
}

/// The buy quantity stepper: a cancel returns to the buy list and buys
/// nothing.
#[test]
fn cancel_on_the_buy_quantity_returns_to_the_buy_list() {
    both_cancels(|cancel| {
        let (mut w, mut rt) = (world(), runtime());
        rt.tick(&mut w, cross());
        rt.tick(&mut w, cross());
        assert!(rt.quantity_view().is_some_and(|q| q.buying));
        rt.tick(&mut w, MenuInput::default());
        let _ = rt.take_ui_cue();
        assert_eq!(cancel_once(&mut rt, &mut w, cancel), Some(CANCEL_CUE));
        assert!(rt.quantity_view().is_none());
        assert_eq!(state(&rt), Some(MenuState::ShopBuy));
        assert_eq!(w.party.money, 10_000);
        assert_eq!(w.party.inventory.get(&POTION).copied(), Some(5));
        back_out_from_list(&mut rt, &mut w, cancel);
    });
}

/// The sell quantity stepper: a cancel returns to the sell list and sells
/// nothing.
#[test]
fn cancel_on_the_sell_quantity_returns_to_the_sell_list() {
    both_cancels(|cancel| {
        let (mut w, mut rt) = (world(), runtime());
        rt.tick(&mut w, down());
        rt.tick(&mut w, cross());
        rt.tick(&mut w, cross());
        assert!(rt.quantity_view().is_some_and(|q| !q.buying));
        rt.tick(&mut w, MenuInput::default());
        let _ = rt.take_ui_cue();
        assert_eq!(cancel_once(&mut rt, &mut w, cancel), Some(CANCEL_CUE));
        assert!(rt.quantity_view().is_none());
        assert_eq!(state(&rt), Some(MenuState::ShopSell));
        assert_eq!(w.party.money, 10_000);
        assert_eq!(w.party.inventory.get(&POTION).copied(), Some(5));
        back_out_from_list(&mut rt, &mut w, cancel);
    });
}

/// The equip prompt (the recipient picker over an equipment row): a cancel
/// returns to the buy list and buys nothing.
#[test]
fn cancel_on_the_equip_prompt_returns_to_the_buy_list() {
    both_cancels(|cancel| {
        let (mut w, mut rt) = (world(), runtime());
        rt.tick(&mut w, cross());
        rt.tick(&mut w, down());
        rt.tick(&mut w, cross());
        assert!(rt.recipient_session.is_some(), "the equip prompt is open");
        rt.tick(&mut w, MenuInput::default());
        let _ = rt.take_ui_cue();
        assert_eq!(cancel_once(&mut rt, &mut w, cancel), Some(CANCEL_CUE));
        assert!(rt.recipient_session.is_none());
        assert_eq!(state(&rt), Some(MenuState::ShopBuy));
        assert_eq!(w.party.money, 10_000);
        assert_eq!(w.party.inventory.get(&ARMOUR), None);
        back_out_from_list(&mut rt, &mut w, cancel);
    });
}

/// The Point Card notice after a buy: a cancel dismisses it, as a confirm
/// does (`FUN_801DB7F4` case 4 reads confirm | cancel), and it is not a
/// cancel of anything - the purchase stands, no cancel cue sounds, and the
/// buy list has the pad back.
#[test]
fn cancel_dismisses_the_point_card_notice() {
    both_cancels(|cancel| {
        let (mut w, mut rt) = (world(), runtime());
        w.party.inventory.insert(POINT_CARD_ITEM_ID, 1);
        rt.tick(&mut w, cross());
        rt.tick(&mut w, cross());
        rt.tick(&mut w, MenuInput::default());
        rt.tick(&mut w, cross());
        // Commit frame: the purchase lands and the notice arms.
        rt.tick(&mut w, MenuInput::default());
        assert_eq!(w.party.inventory.get(&POTION).copied(), Some(6), "bought");
        assert!(rt.point_card_toast().is_some(), "the notice is up");
        let _ = rt.take_ui_cue();
        assert_ne!(cancel_once(&mut rt, &mut w, cancel), Some(CANCEL_CUE));
        assert!(rt.point_card_toast().is_none(), "the notice is gone");
        assert!(rt.quantity_view().is_none());
        assert_eq!(w.party.inventory.get(&POTION).copied(), Some(6));
        assert_eq!(state(&rt), Some(MenuState::ShopBuy));
        back_out_from_list(&mut rt, &mut w, cancel);
    });
}

/// The guard on the other side of the report: the pause menu's root closes
/// on Circle.
#[test]
fn circle_closes_the_pause_menu_root() {
    let mut menu = FieldMenuSession::new();
    let _ = menu.tick(FieldMenuInput::default());
    assert!(matches!(menu.phase(), FieldMenuPhase::Browsing { .. }));
    let _ = menu.tick(FieldMenuInput {
        circle: true,
        ..FieldMenuInput::default()
    });
    assert!(
        !matches!(menu.phase(), FieldMenuPhase::Browsing { .. }),
        "Circle leaves the root picker: {:?}",
        menu.phase()
    );
}

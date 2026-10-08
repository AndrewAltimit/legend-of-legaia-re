use super::*;
use legaia_save::{CharacterRecord, EquipmentSlots, SpellList};

#[test]
fn an_empty_bag_greys_every_shop_root_row_below_buy() {
    use crate::shop::{SHOP_INK_GREY, SHOP_INK_NORMAL};
    let full = shop_root_labels(false, true);
    assert_eq!(
        full,
        vec![
            ("Buy", SHOP_INK_NORMAL),
            ("Sell", SHOP_INK_NORMAL),
            ("Quit", SHOP_INK_NORMAL)
        ]
    );
    let empty = shop_root_labels(true, false);
    let labels: Vec<_> = empty.iter().map(|r| r.0).collect();
    assert_eq!(labels, ["Buy", "Sell", "Trade Seru", "Quit"]);
    assert_eq!(empty[0].1, SHOP_INK_NORMAL);
    assert!(empty[1..].iter().all(|r| r.1 == SHOP_INK_GREY));
}

fn world_with_party(n: usize) -> World {
    let members = (0..n).map(|_| CharacterRecord::zeroed()).collect();
    let mut world = World::default();
    world.load_party(Party { members });
    world
}

/// A shop is a menu-overlay session (the field is swapped out under it);
/// an inn prompt is a field dialogue and keeps the field running.
#[test]
fn a_shop_suspends_the_field_and_an_inn_does_not() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut rt = MenuRuntime::new(tmp.path().to_path_buf());
    assert!(!rt.suspends_field(), "nothing open");
    rt.open_shop_menu(crate::shop::ShopSession::new(
        crate::shop::ShopInventory::new(0, Vec::new()),
    ));
    assert!(rt.is_open());
    assert!(rt.suspends_field(), "a shop freezes the field");

    // The inn prompt as `open_scene_inn` enters it: the session plus
    // the `InnConfirm` state - open, but the field keeps ticking.
    let mut inn = MenuRuntime::new(tmp.path().to_path_buf());
    inn.open_inn(100);
    inn.ctx.state = MenuState::InnConfirm.as_byte();
    assert!(inn.is_open());
    assert!(!inn.suspends_field(), "an inn is a field dialogue");
}

#[test]
fn menu_input_from_pad_edges_inverts_the_pad_word_fold() {
    // Every single button and a chord survive the round trip, so the two
    // decodes cannot drift apart on a bit.
    for bit in 0..16u16 {
        let word = 1u16 << bit;
        let back = menu_input_pad_word(menu_input_from_pad_edges(word));
        assert_eq!(
            back,
            word & menu_input_pad_word(menu_input_from_pad_edges(0xFFFF))
        );
    }
    let chord = 0x4000 | 0x0040;
    assert_eq!(menu_input_pad_word(menu_input_from_pad_edges(chord)), chord);
    assert_eq!(menu_input_from_pad_edges(0), MenuInput::default());
}

#[test]
fn save_then_load_round_trips_through_disk() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let runtime = MenuRuntime::new(tmp.path().to_path_buf());

    let mut world = world_with_party(3);
    // Mutate one HP value so we can detect round-trip drift.
    world.actors[0].battle.hp = 0x1234;
    let _ = runtime.save_to_slot(&mut world, 1).expect("save_to_slot");
    let path = runtime.slot_path(1);
    assert!(path.exists());

    // Load into a fresh world; HP should match.
    let mut fresh = world_with_party(3);
    runtime
        .load_from_slot(&mut fresh, 1)
        .expect("load_from_slot");
    // The mirrored HP propagates through the BattleActor.
    assert_eq!(fresh.actors[0].battle.hp, 0x1234);
}

#[test]
fn current_label_changes_with_state() {
    let mut runtime = MenuRuntime::new("/tmp/legaia-doesnt-need-this-dir");
    runtime.ctx.state = MenuState::SavePickSlot.as_byte();
    assert_eq!(runtime.current_label(), "SAVE - PICK SLOT");
    runtime.ctx.state = MenuState::Closed.as_byte();
    assert_eq!(runtime.current_label(), "");
}

#[test]
fn slot_path_uses_save_ext() {
    let runtime = MenuRuntime::new("/tmp/legaia-test-save");
    let p = runtime.slot_path(7);
    assert!(p.to_string_lossy().ends_with("slot_07.bin"));
}

#[test]
fn status_character_commit_sets_selected_char() {
    let mut world = world_with_party(3);
    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    runtime.ctx.state = MenuState::StatusCharacter.as_byte();
    runtime.ctx.cursor = 2;
    runtime.tick(
        &mut world,
        MenuInput {
            cross: true,
            ..Default::default()
        },
    );
    assert_eq!(runtime.selected_char, 2);
}

#[test]
fn equipment_commit_unequips_slot() {
    let mut world = world_with_party(1);
    let equip = EquipmentSlots {
        slots: [1, 2, 3, 4, 5, 6, 7, 8],
    };
    world.party.roster.members[0].set_equipment(equip);

    // Slot 2 holds item id 3; it must come back to the bag on unequip.
    let before = world.party.inventory.get(&3).copied().unwrap_or(0);

    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    runtime.selected_char = 0;
    runtime.ctx.state = MenuState::StatusEquipment.as_byte();
    runtime.ctx.cursor = 2;
    runtime.tick(
        &mut world,
        MenuInput {
            cross: true,
            ..Default::default()
        },
    );

    let updated = world.party.roster.members[0].equipment();
    assert_eq!(updated.slots[2], 0, "slot 2 unequipped");
    assert_eq!(updated.slots[0], 1, "other slots unchanged");
    assert_eq!(updated.slots[7], 8, "other slots unchanged");
    // The unequipped item returned to the bag (not destroyed).
    assert_eq!(
        world.party.inventory.get(&3).copied().unwrap_or(0),
        before + 1,
        "unequipped item 3 returned to inventory"
    );
}

#[test]
fn equipment_commit_out_of_bounds_char_is_noop() {
    let mut world = world_with_party(1);
    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    runtime.selected_char = 99; // no such char
    runtime.ctx.state = MenuState::StatusEquipment.as_byte();
    runtime.ctx.cursor = 0;
    // Should not panic.
    runtime.tick(
        &mut world,
        MenuInput {
            cross: true,
            ..Default::default()
        },
    );
}

#[test]
fn inventory_commit_decrements_item_count() {
    let mut world = World::default();
    world.party.inventory.insert(5, 3);

    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    runtime.ctx.state = MenuState::StatusInventory.as_byte();
    runtime.ctx.cursor = 0;
    runtime.tick(
        &mut world,
        MenuInput {
            cross: true,
            ..Default::default()
        },
    );

    assert_eq!(world.party.inventory.get(&5), Some(&2));
}

#[test]
fn inventory_commit_removes_last_item() {
    let mut world = World::default();
    world.party.inventory.insert(10, 1);

    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    runtime.ctx.state = MenuState::StatusInventory.as_byte();
    runtime.ctx.cursor = 0;
    runtime.tick(
        &mut world,
        MenuInput {
            cross: true,
            ..Default::default()
        },
    );

    assert!(!world.party.inventory.contains_key(&10));
}

#[test]
fn inventory_commit_empty_inventory_is_noop() {
    let mut world = World::default();
    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    runtime.ctx.state = MenuState::StatusInventory.as_byte();
    runtime.ctx.cursor = 0;
    // Should not panic on empty inventory.
    runtime.tick(
        &mut world,
        MenuInput {
            cross: true,
            ..Default::default()
        },
    );
}

#[test]
fn spell_view_returns_selected_char_spells() {
    let mut world = world_with_party(2);
    let mut list = SpellList {
        count: 2,
        ..SpellList::default()
    };
    list.ids[0] = 7;
    list.ids[1] = 14;
    world.party.roster.members[1].set_spell_list(list);

    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    runtime.selected_char = 1;

    let view = runtime.spell_view(&world).expect("char 1 exists");
    assert_eq!(view.count, 2);
    assert_eq!(view.ids[0], 7);
    assert_eq!(view.ids[1], 14);
}

#[test]
fn spell_view_out_of_bounds_returns_none() {
    let world = world_with_party(1);
    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    runtime.selected_char = 5;
    assert!(runtime.spell_view(&world).is_none());
}

#[test]
fn equipment_view_returns_selected_char_equipment() {
    let mut world = world_with_party(2);
    let equip = EquipmentSlots {
        slots: [9, 8, 7, 6, 5, 4, 3, 2],
    };
    world.party.roster.members[0].set_equipment(equip);

    let runtime = MenuRuntime::new("/tmp/legaia-test");
    let view = runtime.equipment_view(&world).expect("char 0 exists");
    assert_eq!(view.slots, [9, 8, 7, 6, 5, 4, 3, 2]);
}

#[test]
fn inventory_items_sorted_by_id_filters_zeros() {
    let mut world = World::default();
    world.party.inventory.insert(30, 5);
    world.party.inventory.insert(2, 1);
    world.party.inventory.insert(15, 3);

    let items = MenuRuntime::inventory_items(&world);
    assert_eq!(items, vec![(2, 1), (15, 3), (30, 5)]);
}

#[test]
fn screen_item_count_for_character_clamps_cursor_to_party_size() {
    let mut world = world_with_party(2);
    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    runtime.ctx.state = MenuState::StatusCharacter.as_byte();
    runtime.ctx.cursor = 0;
    // Down 3 times with 2 members: 0 -> 1 -> 0 -> 1
    for _ in 0..3 {
        runtime.tick(
            &mut world,
            MenuInput {
                down: true,
                ..Default::default()
            },
        );
    }
    assert_eq!(runtime.ctx.cursor, 1);
}

fn cross() -> MenuInput {
    MenuInput {
        cross: true,
        ..Default::default()
    }
}

fn triangle() -> MenuInput {
    MenuInput {
        triangle: true,
        ..Default::default()
    }
}

fn down() -> MenuInput {
    MenuInput {
        down: true,
        ..Default::default()
    }
}

#[test]
fn shop_menu_trade_row_drives_a_seru_swap() {
    use crate::shop::{ShopInventory, ShopSession};

    // A shop on a disc with seru trading enabled; the lead owns the seru
    // the seed's bucket-0 offer wants (the bucket model trades a type the
    // party holds) plus an unrelated one.
    let seed = 0xABCDu64;
    let bucket0 =
        legaia_asset::seru_trade::bucket_offer(seed, 0, &legaia_asset::seru_trade::default_pool());
    let mut world = World::new();
    world.tables.seru_trade_config = Some(legaia_asset::seru_trade::SeruTradeConfig {
        enabled: true,
        seed,
        max_offers: 4,
    });
    let mut lead = CharacterRecord::zeroed();
    let mut list = SpellList::default();
    list.ids[0] = bucket0.want_id;
    list.count = 1;
    lead.set_spell_list(list);
    world.load_party(Party {
        members: vec![lead],
    });

    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    let mut shop = ShopSession::new(ShopInventory::new(0, vec![]));
    shop.vendor_id = 7;
    runtime.open_shop_menu(shop);
    assert_eq!(runtime.ctx.state, MenuState::ShopMenu.as_byte());

    // Rows = [Buy, Sell, Trade, Exit]; move to Trade (idx 2) and commit.
    runtime.tick(&mut world, down());
    runtime.tick(&mut world, down());
    assert_eq!(runtime.ctx.cursor, 2);
    runtime.tick(&mut world, cross());
    assert_eq!(runtime.ctx.state, MenuState::ShopTrade.as_byte());
    let offer = runtime
        .trade_session
        .as_ref()
        .and_then(|t| t.offers.first().copied())
        .expect("the vendor offers a trade");

    // Pick the first offer, then confirm Yes.
    runtime.tick(&mut world, cross());
    assert_eq!(runtime.ctx.state, MenuState::ShopTradeConfirm.as_byte());
    runtime.tick(&mut world, cross());
    assert_eq!(
        runtime.ctx.state,
        MenuState::ShopTrade.as_byte(),
        "after a trade the menu returns to the offer list"
    );

    // The owner's spell list now holds the received seru - at the offered
    // level - and no longer the given one.
    let list = world.party.roster.members[offer.owner_slot as usize].spell_list();
    let ids = &list.ids[..list.count as usize];
    let pos = ids
        .iter()
        .position(|&id| id == offer.received_id)
        .expect("received seru added");
    assert_eq!(
        list.levels[pos], offer.received_level,
        "received seru arrives at the offered level"
    );
    assert!(!ids.contains(&offer.given_id), "given seru removed");
}

#[test]
fn shop_menu_hides_trade_row_when_trading_disabled() {
    use crate::shop::{ShopInventory, ShopSession};

    let mut world = World::default(); // no seru_trade_config -> disabled
    world.load_party(Party {
        members: vec![CharacterRecord::zeroed()],
    });
    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    runtime.open_shop_menu(ShopSession::new(ShopInventory::new(0, vec![])));

    // Rows = [Buy, Sell, Exit] (no Trade). Slot 2 routes to ShopExit.
    runtime.tick(&mut world, down());
    runtime.tick(&mut world, down());
    assert_eq!(runtime.ctx.cursor, 2);
    runtime.tick(&mut world, cross());
    assert_eq!(runtime.ctx.state, MenuState::ShopExit.as_byte());
}

#[test]
fn shop_buy_flow_drives_through_tick_and_grants_item() {
    use crate::shop::{ShopInventory, ShopItem, ShopSession};

    let mut world = world_with_party(1);
    world.party.money = 500;
    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    runtime.open_shop(ShopSession::new(ShopInventory::new(
        1,
        vec![ShopItem {
            item_id: 10,
            price: 100,
        }],
    )));
    runtime.ctx.state = MenuState::ShopBuy.as_byte();

    // ShopBuy (cursor 0 = item 10) -> ShopQuantity, where the retail
    // stepper takes the pad; confirming it commits with no Yes/No
    // screen in between, over the phases the picker carries.
    runtime.tick(&mut world, cross());
    assert_eq!(runtime.ctx.state, MenuState::ShopQuantity.as_byte());
    runtime.tick(&mut world, MenuInput::default());
    assert_eq!(runtime.quantity_view().map(|v| v.quantity), Some(1));
    runtime.tick(&mut world, cross());
    for _ in 0..4 {
        if runtime.quantity_view().is_none() {
            break;
        }
        runtime.tick(&mut world, MenuInput::default());
    }
    assert_eq!(runtime.ctx.state, MenuState::ShopBuy.as_byte());

    assert_eq!(world.party.money, 400, "100 gold deducted");
    assert_eq!(
        world.party.inventory.get(&10),
        Some(&1),
        "one item 10 granted"
    );
    assert!(runtime.shop_session.is_some(), "still shopping");
}

/// The buy commit's Point Card accrual (`FUN_801DB7F4` case 2) and the
/// window-31 beat that follows it (cases 3 + 4): a party carrying item
/// `0xFE` banks 5% of the gold spent and the runtime holds the toast
/// until a press, which the menu VM never sees.
#[test]
fn shop_buy_credits_the_point_card_and_holds_the_window_31_toast() {
    use crate::shop::{POINT_CARD_ITEM_ID, ShopInventory, ShopItem, ShopSession};

    let mut world = world_with_party(1);
    world.party.money = 500;
    world.party.inventory.insert(POINT_CARD_ITEM_ID, 1);
    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    runtime.open_shop(ShopSession::new(ShopInventory::new(
        1,
        vec![ShopItem {
            item_id: 10,
            price: 100,
        }],
    )));
    runtime.ctx.state = MenuState::ShopBuy.as_byte();

    runtime.tick(&mut world, cross()); // ShopBuy -> ShopQuantity
    runtime.tick(&mut world, MenuInput::default()); // picker phase 0
    runtime.tick(&mut world, cross()); // confirm the stepped quantity
    runtime.tick(&mut world, MenuInput::default()); // the commit frame

    assert_eq!(world.party.money, 400, "the gold debit still runs");
    assert_eq!(world.minigames.point_card, 5, "100 / 20 * 1 banked");
    assert_eq!(
        runtime.point_card_toast(),
        Some(5),
        "the toast is up with this purchase's credit"
    );

    // While the toast is up the screen is frozen: a d-pad frame moves
    // nothing, because retail's toast phase only tests the confirm /
    // cancel masks.
    let state_before = runtime.ctx.state;
    runtime.tick(&mut world, down());
    assert!(runtime.point_card_toast().is_some(), "d-pad does not clear");
    assert_eq!(runtime.ctx.state, state_before);

    // The press the picker consumes is the one that dismisses it, and
    // the flag goes out with the session rather than outliving it.
    runtime.tick(&mut world, cross());
    for _ in 0..4 {
        if runtime.quantity_view().is_none() {
            break;
        }
        runtime.tick(&mut world, MenuInput::default());
    }
    assert_eq!(runtime.point_card_toast(), None, "a press dismisses it");
    assert_eq!(runtime.ctx.state, MenuState::ShopBuy.as_byte());
}

/// The window-7 beat: an armed spell level-up notice freezes the menu
/// VM (a d-pad frame moves nothing) and holds until a confirm / cancel
/// press - the same stall the retail cast sub-screens run after the
/// widget-VM `[open window 7]` script.
#[test]
fn spell_level_notice_holds_until_a_press() {
    let mut world = world_with_party(1);
    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    runtime.open();
    runtime.arm_spell_level_notice(crate::magic_xp::SpellLevelNotice {
        caster_slot: 0,
        spell_index: 0,
        spell_id: 0x83,
        new_level: 2,
        line: "Gimard's magic level increased.".into(),
    });
    assert!(runtime.spell_level_notice().is_some());

    let state_before = runtime.ctx.state;
    runtime.tick(&mut world, down());
    assert!(
        runtime.spell_level_notice().is_some(),
        "d-pad does not clear"
    );
    assert_eq!(runtime.ctx.state, state_before, "the VM is frozen");

    runtime.tick(&mut world, cross());
    assert_eq!(runtime.spell_level_notice(), None, "a press dismisses it");
    assert_eq!(runtime.ctx.state, state_before, "the press is consumed");
}

/// Without the card in the bag the accrual short-circuits and so does
/// the beat - retail's case 3 returns straight to sub-screen `0x1B`.
#[test]
fn shop_buy_without_the_point_card_neither_credits_nor_toasts() {
    use crate::shop::{ShopInventory, ShopItem, ShopSession};

    let mut world = world_with_party(1);
    world.party.money = 500;
    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    runtime.open_shop(ShopSession::new(ShopInventory::new(
        1,
        vec![ShopItem {
            item_id: 10,
            price: 100,
        }],
    )));
    runtime.ctx.state = MenuState::ShopBuy.as_byte();
    runtime.tick(&mut world, cross()); // ShopBuy -> ShopQuantity
    runtime.tick(&mut world, MenuInput::default()); // picker phase 0
    runtime.tick(&mut world, cross()); // confirm
    for _ in 0..4 {
        if runtime.quantity_view().is_none() {
            break;
        }
        runtime.tick(&mut world, MenuInput::default());
    }

    assert_eq!(
        world.party.inventory.get(&10),
        Some(&1),
        "the buy still lands"
    );
    assert_eq!(world.minigames.point_card, 0);
    assert_eq!(runtime.point_card_toast(), None);
    assert_eq!(
        runtime.ctx.state,
        MenuState::ShopBuy.as_byte(),
        "and the list is reachable on the very next frame"
    );
}

/// A refused buy (short purse) must not bank points: retail's case 2
/// is only reached from the quantity screen a affordable row opened.
#[test]
fn a_refused_buy_banks_no_points() {
    use crate::shop::{POINT_CARD_ITEM_ID, ShopInventory, ShopItem, ShopSession};

    let mut world = world_with_party(1);
    world.party.money = 50;
    world.party.inventory.insert(POINT_CARD_ITEM_ID, 1);
    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    runtime.open_shop(ShopSession::new(ShopInventory::new(
        1,
        vec![ShopItem {
            item_id: 10,
            price: 100,
        }],
    )));
    runtime.ctx.state = MenuState::ShopBuy.as_byte();
    runtime.tick(&mut world, cross());

    assert_eq!(world.minigames.point_card, 0);
    assert_eq!(runtime.point_card_toast(), None);
}

#[test]
fn shop_buy_refusal_beat_stays_on_the_list_row() {
    use crate::shop::{ShopInventory, ShopItem, ShopSession};

    // 50 gold against a 100-gold row: the retail state-2 dispatch
    // refuses at the list (`slt gold, price` + buzz) - no pending
    // item, no quantity screen, hand still on the confirmed row.
    let mut world = world_with_party(1);
    world.party.money = 50;
    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    runtime.open_shop(ShopSession::new(ShopInventory::new(
        1,
        vec![
            ShopItem {
                item_id: 9,
                price: 10,
            },
            ShopItem {
                item_id: 10,
                price: 100,
            },
        ],
    )));
    runtime.ctx.state = MenuState::ShopBuy.as_byte();
    runtime.tick(&mut world, down());
    assert_eq!(runtime.ctx.cursor, 1);
    runtime.tick(&mut world, cross());
    assert_eq!(
        runtime.ctx.state,
        MenuState::ShopBuy.as_byte(),
        "refused buy stays on the list"
    );
    assert_eq!(runtime.ctx.cursor, 1, "hand stays on the refused row");
    assert!(
        runtime
            .shop_session
            .as_ref()
            .is_some_and(|s| s.pending_item_id.is_none()),
        "no pending item was staged"
    );
    // An affordable row still routes into the quantity picker.
    world.party.money = 500;
    runtime.tick(&mut world, cross());
    assert_eq!(runtime.ctx.state, MenuState::ShopQuantity.as_byte());
}

#[test]
fn shop_buy_list_pages_seven_rows_like_the_kernel() {
    use crate::shop::{ShopInventory, ShopItem, ShopSession};

    let mut world = world_with_party(1);
    world.party.money = 100_000;
    let items = (0..10u8)
        .map(|i| ShopItem {
            item_id: 0x40 + i,
            price: 10,
        })
        .collect();
    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    runtime.open_shop(ShopSession::new(ShopInventory::new(1, items)));
    runtime.ctx.state = MenuState::ShopBuy.as_byte();
    let right = MenuInput {
        right: true,
        ..Default::default()
    };
    let up = MenuInput {
        up: true,
        ..Default::default()
    };
    // Up at the page top wraps to the page's last row, not the list's.
    runtime.tick(&mut world, up);
    assert_eq!(runtime.ctx.cursor, 6);
    // Right flips to page 2, clamped to the last row.
    runtime.tick(&mut world, right);
    assert_eq!(runtime.ctx.cursor, 9);
    // Down past the last row wraps to the page top.
    runtime.tick(&mut world, down());
    assert_eq!(runtime.ctx.cursor, 7);
}

#[test]
fn shop_buy_full_stack_row_is_refused_at_the_list() {
    use crate::shop::{ShopInventory, ShopItem, ShopSession};

    // A stack already at 99 is a disabled row (`0x800`): the list
    // kernel buzzes it before the state-2 dispatch - a purse that can
    // afford it changes nothing, and no quantity / confirm screen opens.
    let mut world = world_with_party(1);
    world.party.money = 100_000;
    world.party.inventory.insert(9, 99);
    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    runtime.open_shop(ShopSession::new(ShopInventory::new(
        1,
        vec![ShopItem {
            item_id: 9,
            price: 10,
        }],
    )));
    runtime.ctx.state = MenuState::ShopBuy.as_byte();
    runtime.tick(&mut world, cross());
    assert_eq!(runtime.ctx.state, MenuState::ShopBuy.as_byte());
    assert!(runtime.quantity_view().is_none());
    assert_eq!(world.party.inventory.get(&9), Some(&99));
}

/// The shop keys retail's list-kernel cues: a step only when the hand
/// moved, the dim-row buzz for a refused buy, a confirm for an accepted
/// one, then the quantity stepper's own step / cancel.
#[test]
fn shop_ticks_raise_the_list_kernel_cues() {
    use crate::shop::{ShopInventory, ShopItem, ShopSession};

    let mut world = world_with_party(1);
    world.party.money = 50;
    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    runtime.open_shop(ShopSession::new(ShopInventory::new(
        1,
        vec![
            ShopItem {
                item_id: 9,
                price: 10,
            },
            ShopItem {
                item_id: 10,
                price: 100,
            },
        ],
    )));
    runtime.ctx.state = MenuState::ShopBuy.as_byte();
    runtime.tick(&mut world, down());
    assert_eq!(runtime.take_ui_cue(), Some(0x21), "the hand moved");
    assert_eq!(runtime.take_ui_cue(), None, "a cue is taken once");
    assert_eq!(runtime.ctx.cursor, 1);
    runtime.tick(&mut world, MenuInput::default());
    assert_eq!(runtime.take_ui_cue(), None, "an idle tick is silent");
    runtime.tick(&mut world, cross());
    assert_eq!(runtime.take_ui_cue(), Some(SHOP_REFUSAL_CUE));
    world.party.money = 500;
    runtime.tick(&mut world, cross());
    assert_eq!(runtime.ctx.state, MenuState::ShopQuantity.as_byte());
    assert_eq!(runtime.take_ui_cue(), Some(0x20));
    // The stepper: its first frame stages, then a step moves the number.
    runtime.tick(&mut world, MenuInput::default());
    runtime.tick(
        &mut world,
        MenuInput {
            right: true,
            ..MenuInput::default()
        },
    );
    assert_eq!(runtime.take_ui_cue(), Some(0x21));
    runtime.tick(
        &mut world,
        MenuInput {
            circle: true,
            ..MenuInput::default()
        },
    );
    assert_eq!(runtime.take_ui_cue(), Some(0x37));
}

#[test]
fn equipment_buy_opens_recipient_picker_and_equips_now() {
    use crate::equipment::{DiscEquipEntry, DiscEquipInfo};
    use crate::shop::{ShopInventory, ShopItem, ShopSession};
    use legaia_asset::equip_stats::EquipSlot as Disc;

    let mut world = world_with_party(2);
    world.party.money = 300;
    // Party member 1 already wears item 7 in the weapon slot.
    let mut eq = world.party.roster.members[1].equipment();
    eq.slots[crate::equipment::EquipSlot::Weapon.as_index() as usize] = 7;
    world.party.roster.members[1].set_equipment(eq);

    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    runtime.retail_equipment_buy = true;
    runtime.install_equip_info(DiscEquipInfo::from_entries([(
        0x30,
        DiscEquipEntry {
            mask: 0b010, // second party member only
            category: Disc::Weapon,
            is_ra_seru: false,
            passive_index: None,
        },
    )]));
    runtime.open_shop(ShopSession::new(ShopInventory::new(
        1,
        vec![ShopItem {
            item_id: 0x30,
            price: 120,
        }],
    )));
    runtime.ctx.state = MenuState::ShopBuy.as_byte();

    // Confirming the equipment row parks the list under the picker.
    runtime.tick(&mut world, cross());
    assert_eq!(runtime.ctx.state, MenuState::ShopBuy.as_byte());
    let session = runtime.recipient_session.as_ref().expect("picker open");
    assert_eq!(session.can_equip, vec![false, true]);

    // Row 2 = party member 1: buy and equip now. The displaced weapon
    // returns to the bag, the purse debits, the piece never enters it.
    runtime.tick(&mut world, MenuInput::default()); // Init frame
    runtime.tick(&mut world, down());
    runtime.tick(&mut world, down());
    // A confirm on row 1 (member 0, mask-rejected) would buzz and stay;
    // row 2 is the equippable member.
    runtime.tick(&mut world, cross());
    // One exit-beat frame drops back to the buy list (retail's
    // post-commit return).
    runtime.tick(&mut world, MenuInput::default());
    assert!(runtime.recipient_session.is_none(), "picker closed");
    assert_eq!(world.party.money, 180, "120 gold debited");
    assert_eq!(
        world.party.roster.members[1].equipment().slots
            [crate::equipment::EquipSlot::Weapon.as_index() as usize],
        0x30,
        "bought piece equipped directly"
    );
    assert_eq!(
        world.party.inventory.get(&7).copied(),
        Some(1),
        "displaced weapon returned to the bag"
    );
    assert_eq!(
        world.party.inventory.get(&0x30),
        None,
        "the purchase never entered the bag"
    );
}

#[test]
fn recipient_row_zero_buys_one_copy_into_the_bag() {
    use crate::equipment::{DiscEquipEntry, DiscEquipInfo};
    use crate::shop::{ShopInventory, ShopItem, ShopSession};
    use legaia_asset::equip_stats::EquipSlot as Disc;

    let mut world = world_with_party(1);
    world.party.money = 200;
    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    runtime.retail_equipment_buy = true;
    runtime.install_equip_info(DiscEquipInfo::from_entries([(
        0x31,
        DiscEquipEntry {
            mask: 0b111,
            category: Disc::Body,
            is_ra_seru: false,
            passive_index: None,
        },
    )]));
    runtime.open_shop(ShopSession::new(ShopInventory::new(
        1,
        vec![ShopItem {
            item_id: 0x31,
            price: 60,
        }],
    )));
    runtime.ctx.state = MenuState::ShopBuy.as_byte();
    runtime.tick(&mut world, cross());
    assert!(runtime.recipient_session.is_some());
    // Row 0 (the bag) is the seeded cursor; confirm buys one copy.
    runtime.tick(&mut world, MenuInput::default()); // Init frame
    runtime.tick(&mut world, cross());
    assert_eq!(world.party.money, 140);
    assert_eq!(world.party.inventory.get(&0x31).copied(), Some(1));
    runtime.tick(&mut world, MenuInput::default()); // exit beat
    assert!(runtime.recipient_session.is_none());
}

#[test]
fn equipment_buy_keeps_quantity_flow_while_retail_flow_is_off() {
    use crate::equipment::{DiscEquipEntry, DiscEquipInfo};
    use crate::shop::{ShopInventory, ShopItem, ShopSession};
    use legaia_asset::equip_stats::EquipSlot as Disc;

    let mut world = world_with_party(1);
    world.party.money = 500;
    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    // Equip info installed, but the retail flow not opted into: the
    // legacy quantity route must survive (the native window has no
    // picker surface yet).
    runtime.install_equip_info(DiscEquipInfo::from_entries([(
        0x30,
        DiscEquipEntry {
            mask: 0b111,
            category: Disc::Weapon,
            is_ra_seru: false,
            passive_index: None,
        },
    )]));
    runtime.open_shop(ShopSession::new(ShopInventory::new(
        1,
        vec![ShopItem {
            item_id: 0x30,
            price: 100,
        }],
    )));
    runtime.ctx.state = MenuState::ShopBuy.as_byte();
    runtime.tick(&mut world, cross());
    assert_eq!(runtime.ctx.state, MenuState::ShopQuantity.as_byte());
    assert!(runtime.recipient_session.is_none());
}

#[test]
fn shop_triangle_from_list_tears_down_session_and_closes() {
    use crate::shop::{ShopInventory, ShopSession};

    let mut world = world_with_party(1);
    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    runtime.open_shop(ShopSession::new(ShopInventory::new(1, vec![])));
    runtime.ctx.state = MenuState::ShopBuy.as_byte();

    // Triangle from the buy list backs up to the top shop menu, and a second
    // Triangle leaves it via the ShopExit teardown screen.
    runtime.tick(&mut world, triangle());
    assert_eq!(runtime.ctx.state, MenuState::ShopMenu.as_byte());
    runtime.tick(&mut world, triangle());
    assert_eq!(runtime.ctx.state, MenuState::ShopExit.as_byte());
    // ShopExit fires its one-shot commit (clears the session) then holds.
    runtime.tick(&mut world, MenuInput::default());
    assert!(
        runtime.shop_session.is_none(),
        "session cleared on teardown"
    );
    // Holds, then closes.
    for _ in 0..8 {
        runtime.tick(&mut world, MenuInput::default());
    }
    assert_eq!(runtime.ctx.state, MenuState::Closing.as_byte());
}

#[test]
fn inn_rest_drives_through_tick_restores_hp_and_charges_gold() {
    let mut world = world_with_party(1);
    world.party.money = 50;
    world.party.party_count = 1;
    world.actors[0].active = true;
    let mut hms = world.party.roster.members[0].hp_mp_sp();
    hms.hp_max = 100;
    hms.hp_cur = 10;
    world.party.roster.members[0].set_hp_mp_sp(hms);
    world.mirror_roster_hp_mp(0);

    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    runtime.open_inn(10);
    runtime.ctx.state = MenuState::InnConfirm.as_byte();

    // InnConfirm (cursor 0 = yes) -> InnSleep, rest applied.
    runtime.tick(&mut world, cross());
    assert_eq!(runtime.ctx.state, MenuState::InnSleep.as_byte());
    assert_eq!(world.party.money, 40, "10 gold charged");
    assert_eq!(world.actors[0].battle.hp, 100, "HP restored to max");
    assert_eq!(
        world.party.roster.members[0].hp_mp_sp().hp_cur,
        100,
        "the record holds the restored pool"
    );
    assert!(runtime.inn_session.is_none(), "inn session cleared");

    // Sleep fade holds, then closes.
    for _ in 0..8 {
        runtime.tick(&mut world, MenuInput::default());
    }
    assert_eq!(runtime.ctx.state, MenuState::Closing.as_byte());
}

#[test]
fn inn_decline_closes_without_charging() {
    let mut world = world_with_party(1);
    world.party.money = 50;
    let mut runtime = MenuRuntime::new("/tmp/legaia-test");
    runtime.open_inn(10);
    runtime.ctx.state = MenuState::InnConfirm.as_byte();
    runtime.ctx.cursor = 1; // slot 1 = no

    runtime.tick(&mut world, cross());
    assert_eq!(runtime.ctx.state, MenuState::Closing.as_byte());
    assert_eq!(world.party.money, 50, "no gold charged on decline");
    assert!(runtime.inn_session.is_none(), "inn session cleared");
}

#[test]
fn load_from_missing_slot_returns_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let runtime = MenuRuntime::new(tmp.path().to_path_buf());
    let mut world = world_with_party(3);
    let err = runtime.load_from_slot(&mut world, 99).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("read save slot") || msg.contains("No such file"),
        "unexpected error: {msg}"
    );
}

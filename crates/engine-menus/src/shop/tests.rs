use super::*;

#[test]
fn shop_slides_open_together_and_close_out() {
    let mut s = ShopSlides::default();
    let root = shop_screen_windows(ShopScreenPhase::Root, false);
    s.tick(&root);
    assert!(s.draw_list().iter().all(|(_, p)| *p == 1));
    for _ in 0..SHOP_SLIDE_FRAMES {
        s.tick(&root);
    }
    assert!(s.settled());
    // Buy: the picker leaves (drawn first, sliding out), 41 slides in,
    // the plate and purse stay home.
    let buy = shop_screen_windows(ShopScreenPhase::BuyList, false);
    s.tick(&buy);
    let l = s.draw_list();
    assert_eq!(l[0], (42, SHOP_SLIDE_FRAMES - 1));
    assert!(l.contains(&(33, SHOP_SLIDE_FRAMES)) && l.contains(&(41, 1)));
    for _ in 0..SHOP_SLIDE_FRAMES {
        s.tick(&buy);
    }
    assert!(s.settled());
    assert!(!s.draw_list().iter().any(|(id, _)| *id == 42));
}

#[test]
fn shop_screen_window_sets_follow_the_widget_scripts() {
    use ShopScreenPhase as P;
    assert_eq!(
        shop_screen_windows(P::Root, false),
        vec![33, 42, 32, 40, 34]
    );
    // The buy flow keeps window 41 up on every screen; the list leaves
    // for the steppers.
    for p in [P::BuyList, P::BuyQuantity, P::BuyRecipient] {
        let w = shop_screen_windows(p, false);
        assert!(w.contains(&41) && w.contains(&34), "{p:?}");
        assert!(!w.contains(&42), "{p:?}: window 41 covers the picker");
    }
    assert!(!shop_screen_windows(P::BuyQuantity, false).contains(&40));
    // The sell flow swaps 40 / 34 for 38 / 39.
    let sell = shop_screen_windows(P::SellList, false);
    assert!(sell.contains(&38) && sell.contains(&39) && !sell.contains(&40));
    assert_eq!(shop_screen_windows(P::SellQuantity, true).last(), Some(&31));
}

fn session_with_items() -> ShopSession {
    ShopSession::new(ShopInventory::new(
        1,
        vec![
            ShopItem {
                item_id: 10,
                price: 100,
            },
            ShopItem {
                item_id: 20,
                price: 200,
            },
        ],
    ))
}

#[test]
fn sell_price_half_buy_rounded_down() {
    let inv = ShopInventory::new(
        0,
        vec![ShopItem {
            item_id: 1,
            price: 101,
        }],
    );
    assert_eq!(inv.sell_price(1), 50);
}

#[test]
fn sell_price_unknown_item_is_one() {
    let inv = ShopInventory::new(0, vec![]);
    assert_eq!(inv.sell_price(99), 1);
}

#[test]
fn select_buy_item_sets_pending() {
    let mut s = session_with_items();
    s.select_buy_item(1);
    assert_eq!(s.pending_item_id, Some(20));
    assert!(s.pending_is_buying);
}

#[test]
fn select_buy_item_out_of_range_is_noop() {
    let mut s = session_with_items();
    s.select_buy_item(99);
    assert!(s.pending_item_id.is_none());
}

#[test]
fn select_sell_item_sets_pending() {
    let mut s = session_with_items();
    let inventory = vec![(10u8, 3u8), (20, 1)];
    s.select_sell_item(0, &inventory);
    assert_eq!(s.pending_item_id, Some(10));
    assert!(!s.pending_is_buying);
}

#[test]
fn set_quantity_slot_plus_one() {
    let mut s = session_with_items();
    s.set_quantity(2); // cursor 2 → 3 items
    assert_eq!(s.pending_quantity, 3);
}

#[test]
fn try_buy_deducts_correct_amount() {
    let mut s = session_with_items();
    s.select_buy_item(0); // item 10, price 100
    s.set_quantity(2); // 3 items
    let result = s.try_buy(500);
    assert_eq!(result, Some((10, 3, -300)));
}

#[test]
fn try_buy_fails_when_insufficient_gold() {
    let mut s = session_with_items();
    s.select_buy_item(0); // price 100
    s.set_quantity(4); // 5 × 100 = 500
    assert!(s.try_buy(499).is_none());
}

#[test]
fn try_buy_fails_when_in_sell_mode() {
    let mut s = session_with_items();
    s.select_sell_item(0, &[(10, 5)]);
    assert!(s.try_buy(9999).is_none());
}

#[test]
fn try_sell_returns_proceeds() {
    let mut s = session_with_items();
    s.select_sell_item(0, &[(10, 5)]);
    s.set_quantity(1); // 2 items (slot 1 → qty 2)
    let result = s.try_sell(5);
    // sell price of item 10 (buy 100) = 50; qty 2 → 100
    assert_eq!(result, Some((10, 2, 100)));
}

#[test]
fn try_sell_clamps_to_held_count() {
    let mut s = session_with_items();
    s.select_sell_item(0, &[(10, 1)]);
    s.set_quantity(8); // want 9 but only hold 1
    let result = s.try_sell(1);
    assert_eq!(result.map(|(_, q, _)| q), Some(1));
}

#[test]
fn try_sell_fails_when_hold_zero() {
    let mut s = session_with_items();
    s.select_sell_item(0, &[(10, 1)]);
    assert!(s.try_sell(0).is_none());
}

#[test]
fn buy_item_count_reflects_inventory_length() {
    let s = session_with_items();
    assert_eq!(s.buy_item_count(), 2);
}

use crate::menu_list_rows::ListSelection;
use legaia_engine_system::input::PadButton;

#[test]
fn sell_credit_is_half_price_floored() {
    assert_eq!(sell_credit(100, 2), 100);
    // Odd product floors: 15 * 3 = 45 -> 22.
    assert_eq!(sell_credit(15, 3), 22);
    assert_eq!(sell_credit(1, 1), 0);
}

#[test]
fn apply_sale_gold_clamps_at_cap() {
    assert_eq!(apply_sale_gold(100, 50), 150);
    assert_eq!(apply_sale_gold(GOLD_CAP - 10, 50), GOLD_CAP);
}

#[test]
fn sell_quantity_pad_steps_and_clamps() {
    let mut s = SellQuantitySession::new(0x40, 100, 25);
    assert_eq!(s.tick(0, 1), SellQtyEvent::None); // init frame
    let right = PadButton::Right.mask();
    let left = PadButton::Left.mask();
    let down = PadButton::Down.mask();
    let up = PadButton::Up.mask();
    assert_eq!(s.tick(right, 1), SellQtyEvent::Moved);
    assert_eq!(s.qty, 2);
    assert_eq!(s.tick(down, 1), SellQtyEvent::Moved);
    assert_eq!(s.qty, 12);
    // +10 clamps to max.
    assert_eq!(s.tick(down, 1), SellQtyEvent::Moved);
    assert_eq!(s.tick(down, 1), SellQtyEvent::Moved);
    assert_eq!(s.qty, 25);
    // At max, +1 is a silent no-op.
    assert_eq!(s.tick(right, 1), SellQtyEvent::None);
    // -10 floors at 1.
    assert_eq!(s.tick(up, 1), SellQtyEvent::Moved);
    assert_eq!(s.tick(up, 1), SellQtyEvent::Moved);
    assert_eq!(s.tick(up, 1), SellQtyEvent::Moved);
    assert_eq!(s.qty, 1);
    // At 1, -1 and -10 are silent no-ops (retail gates on qty >= 2).
    assert_eq!(s.tick(left, 1), SellQtyEvent::None);
    assert_eq!(s.tick(up, 1), SellQtyEvent::None);
}

#[test]
fn sell_quantity_partial_sale_returns_to_sell_list() {
    let mut s = SellQuantitySession::new(0x40, 100, 5);
    s.tick(0, 1);
    s.tick(PadButton::Right.mask(), 1); // qty 2
    let ev = s.tick(PadButton::Cross.mask(), 1);
    assert_eq!(
        ev,
        SellQtyEvent::Sold {
            item_id: 0x40,
            qty: 2,
            credit: 100
        }
    );
    assert_eq!(s.tick(0, 1), SellQtyEvent::ExitToSellList);
    assert!(s.is_done());
}

#[test]
fn sell_quantity_whole_stack_empty_bag_delays_to_shop_root() {
    let mut s = SellQuantitySession::new(0x40, 100, 1);
    s.tick(0, 1);
    let ev = s.tick(PadButton::Cross.mask(), 1);
    assert_eq!(
        ev,
        SellQtyEvent::Sold {
            item_id: 0x40,
            qty: 1,
            credit: 50
        }
    );
    s.finish_whole_stack(true);
    // 17 frame-delta units accumulate before the exit fires.
    for _ in 0..(SELL_QTY_EXIT_DELAY - 1) {
        assert_eq!(s.tick(0, 1), SellQtyEvent::None);
    }
    assert_eq!(s.tick(0, 1), SellQtyEvent::ExitToShopRoot);
}

#[test]
fn sell_quantity_whole_stack_with_items_left_returns_to_list() {
    let mut s = SellQuantitySession::new(0x40, 100, 1);
    s.tick(0, 1);
    s.tick(PadButton::Cross.mask(), 1);
    s.finish_whole_stack(false);
    assert_eq!(s.tick(0, 1), SellQtyEvent::ExitToSellList);
}

#[test]
fn sell_quantity_cancel() {
    let mut s = SellQuantitySession::new(0x40, 100, 5);
    s.tick(0, 1);
    assert_eq!(s.tick(PadButton::Circle.mask(), 1), SellQtyEvent::Cancelled);
    assert!(s.is_done());
}

use legaia_engine_system::menu_input::NavButtons;

fn nav(confirm: bool, cancel: bool, left: bool, right: bool) -> NavButtons {
    NavButtons {
        confirm,
        cancel,
        left,
        right,
    }
}

#[test]
fn buy_recipient_row0_buys_into_bag() {
    let mut s = BuyRecipientSession::new(0x30, 200, vec![true, false], false);
    assert_eq!(
        s.tick(nav(false, false, false, false)),
        BuyRecipientEvent::None
    );
    assert_eq!(
        s.tick(nav(true, false, false, false)),
        BuyRecipientEvent::BoughtToBag {
            item_id: 0x30,
            cost: 200,
            point_credit: 0,
        }
    );
    assert_eq!(
        s.tick(nav(false, false, false, false)),
        BuyRecipientEvent::ExitToBuyList
    );
    assert!(s.is_done());
}

#[test]
fn buy_recipient_party_row_equips_or_buzzes() {
    let mut s = BuyRecipientSession::new(0x30, 200, vec![true, false], true);
    s.tick(nav(false, false, false, false));
    assert_eq!(
        s.tick(nav(false, false, false, true)),
        BuyRecipientEvent::Moved
    );
    assert_eq!(
        s.tick(nav(false, false, false, true)),
        BuyRecipientEvent::Moved
    );
    // Row 2 = party member 1, cannot equip -> buzz, stays.
    assert_eq!(
        s.tick(nav(true, false, false, false)),
        BuyRecipientEvent::Buzz
    );
    assert!(!s.is_done());
    // Back to row 1 = party member 0 (can equip).
    s.tick(nav(false, false, true, false));
    assert_eq!(
        s.tick(nav(true, false, false, false)),
        BuyRecipientEvent::BoughtAndEquipped {
            party_index: 0,
            item_id: 0x30,
            cost: 200,
            point_credit: 10,
        }
    );
    // Point Card toast holds for a press.
    assert_eq!(
        s.tick(nav(false, false, false, false)),
        BuyRecipientEvent::None
    );
    assert_eq!(
        s.tick(nav(true, false, false, false)),
        BuyRecipientEvent::ExitToBuyList
    );
}

#[test]
fn buy_recipient_cancel_returns_to_list() {
    let mut s = BuyRecipientSession::new(0x30, 200, vec![true], false);
    s.tick(nav(false, false, false, false));
    assert_eq!(
        s.tick(nav(false, true, false, false)),
        BuyRecipientEvent::Cancelled
    );
    assert!(s.is_done());
}

#[test]
fn point_card_credit_is_five_percent_per_unit() {
    // floor(price / 20) per unit, times qty.
    assert_eq!(point_card_credit(100, 3), 15);
    assert_eq!(point_card_credit(19, 5), 0); // < 20 gold earns nothing
    assert_eq!(point_card_credit(59, 2), 4); // floor(59/20) = 2 per unit
    assert_eq!(apply_point_card(POINT_CARD_CAP - 3, 10), POINT_CARD_CAP);
}

#[test]
fn buy_qty_max_laws() {
    // gold / price, capped at 99.
    assert_eq!(buy_qty_max(1000, 100, None), 10);
    assert_eq!(buy_qty_max(100_000, 100, None), 99);
    // Held stack tightens the cap to 99 - held.
    assert_eq!(buy_qty_max(100_000, 100, Some(95)), 4);
    // Poorer than one copy -> 0.
    assert_eq!(buy_qty_max(50, 100, None), 0);
}

#[test]
fn buy_list_confirm_routes_by_item_kind() {
    // FUN_801DB21C state-2 dispatch: affordability first (buzz +
    // stay), then kind 1 -> recipient picker, kind 2 -> quantity
    // picker, anything else -> the mode-select fallback.
    assert_eq!(buy_list_confirm_route(2, 99, 100), BuyListRoute::Refused);
    assert_eq!(
        buy_list_confirm_route(1, 100, 100),
        BuyListRoute::RecipientPicker
    );
    assert_eq!(
        buy_list_confirm_route(2, 100, 100),
        BuyListRoute::QuantityPicker
    );
    assert_eq!(
        buy_list_confirm_route(0, 100, 100),
        BuyListRoute::ModeSelect
    );
}

#[test]
fn buy_quantity_commit_with_point_card_waits_for_toast() {
    let mut s = BuyQuantitySession::new(0x40, 100, 10_000, None, true);
    assert_eq!(s.tick(0), BuyQtyEvent::None); // init
    assert_eq!(s.tick(PadButton::Right.mask()), BuyQtyEvent::Moved);
    assert_eq!(s.qty, 2);
    assert_eq!(s.tick(PadButton::Cross.mask()), BuyQtyEvent::None);
    assert_eq!(
        s.tick(0),
        BuyQtyEvent::Bought {
            item_id: 0x40,
            qty: 2,
            cost: 200,
            point_credit: 10,
        }
    );
    // Point Card toast holds until a button press.
    assert_eq!(s.tick(0), BuyQtyEvent::None);
    assert_eq!(s.tick(PadButton::Cross.mask()), BuyQtyEvent::ExitToBuyList);
    assert!(s.is_done());
}

#[test]
fn buy_quantity_commit_without_point_card_exits_directly() {
    let mut s = BuyQuantitySession::new(0x40, 100, 10_000, None, false);
    s.tick(0);
    s.tick(PadButton::Cross.mask());
    assert_eq!(
        s.tick(0),
        BuyQtyEvent::Bought {
            item_id: 0x40,
            qty: 1,
            cost: 100,
            point_credit: 0,
        }
    );
    assert_eq!(s.tick(0), BuyQtyEvent::ExitToBuyList);
}

#[test]
fn sell_list_fixup_last_row_alone_on_final_page() {
    // Selling with the hand on the last row, alone at the top of the
    // final page: selection steps back one, scroll steps back a page.
    let mut sel = ListSelection {
        scroll_top: 12,
        selected: 12,
    };
    sell_list_fixup(&mut sel, 13, 12);
    assert_eq!((sel.scroll_top, sel.selected), (0, 11));
    // Not the last row -> untouched.
    let mut sel = ListSelection {
        scroll_top: 12,
        selected: 12,
    };
    sell_list_fixup(&mut sel, 14, 12);
    assert_eq!((sel.scroll_top, sel.selected), (12, 12));
    // Last row but not at the page top -> untouched.
    let mut sel = ListSelection {
        scroll_top: 12,
        selected: 13,
    };
    sell_list_fixup(&mut sel, 14, 12);
    assert_eq!((sel.scroll_top, sel.selected), (12, 13));
    // Row 0 never steps back.
    let mut sel = ListSelection {
        scroll_top: 0,
        selected: 0,
    };
    sell_list_fixup(&mut sel, 1, 12);
    assert_eq!((sel.scroll_top, sel.selected), (0, 0));
}

#[test]
fn cursor_word_suppress_park_and_select() {
    // bit 0x4000 kills the hand on every row.
    assert_eq!(shop_cursor_mode(0x4000 | 1, 1), None);
    // bit 0x2000 parks it: every row draws, mode by the blink bit.
    assert_eq!(shop_cursor_mode(0x2000, 0), Some(4));
    assert_eq!(shop_cursor_mode(0x2000, 2), Some(4));
    assert_eq!(shop_cursor_mode(0x2000 | 0x1000, 2), Some(0));
    // Focused: only the selected row, mode = inverted blink bit.
    assert_eq!(shop_cursor_mode(1, 0), None);
    assert_eq!(shop_cursor_mode(1, 1), Some(1));
    assert_eq!(shop_cursor_mode(0x1000 | 1, 1), Some(0));
}

#[test]
fn root_command_rows_grey_sell_and_quit_together() {
    let rows = shop_root_command_rows((40, 50), 0, false);
    assert_eq!(rows[0].text, (40 + 0x14, 50));
    assert_eq!(rows[1].text, (40 + 0x14, 50 + 0x0E));
    assert_eq!(rows[2].text, (40 + 0x14, 50 + 0x1C));
    // The ink is cleared after the Buy row draws, so Buy stays white and
    // both later rows grey.
    assert_eq!(rows[0].ink, SHOP_INK_NORMAL);
    assert_eq!(rows[1].ink, SHOP_INK_GREY);
    assert_eq!(rows[2].ink, SHOP_INK_GREY);

    let rows = shop_root_command_rows((40, 50), 0, true);
    assert!(rows.iter().all(|r| r.ink == SHOP_INK_NORMAL));
}

#[test]
fn root_command_cursor_sits_at_the_window_origin() {
    let rows = shop_root_command_rows((40, 50), 1, true);
    assert_eq!(rows[0].cursor, None);
    assert_eq!(rows[1].cursor, Some((1, (40, 50 + 0x0E))));
    assert_eq!(rows[2].cursor, None);
}

#[test]
fn passive_index_picks_its_table_off_the_class_byte() {
    let equip = |i: u8| i + 1;
    let effect = |i: u8| i + 2;
    // class 1 goes through the equipment record.
    assert_eq!(item_passive_index(1, 3, equip, effect), Some(4));
    // anything else goes through the item-effect record.
    assert_eq!(item_passive_index(2, 3, equip, effect), Some(5));
    assert_eq!(item_passive_index(0, 3, equip, effect), Some(5));
    // 0x40 is the sentinel, and so is anything above it.
    assert_eq!(item_passive_index(1, 0, |_| PASSIVE_NONE, effect), None);
    assert_eq!(item_passive_index(2, 0, equip, |_| 0xFF), None);
    assert_eq!(item_passive_index(2, 0, equip, |_| 0x3F), Some(0x3F));
}

#[test]
fn sell_detail_panel_halves_the_price_and_falls_back_to_cannot_sell() {
    let p = shop_sell_detail_panel((6, 8), 0x1A, 101, Some(3));
    assert_eq!(p.name_pen, (6, 8));
    assert_eq!(p.desc_pen, (6, 8 + 0x0E));
    assert_eq!(p.price_row_y, 8 + 0x2B);
    // Exactly half, rounded down by the shift.
    assert_eq!(
        p.sell,
        Some(SellPriceRow {
            price: 50,
            label_pen: (6 + 0x24, 8 + 0x2B),
            icon_pen: (6 + 0x54, 8 + 0x2D),
            value_pen: (6 + 0x64, 8 + 0x2B),
        })
    );
    assert_eq!(p.passive, Some(3));
    assert_eq!(p.passive_name_pen, (6, 8 + 0x45));
    assert_eq!(p.passive_desc_pen, (6, 8 + 0x55));

    // Price 0 -> the "Cannot sell" arm.
    let p = shop_sell_detail_panel((6, 8), 0x1A, 0, None);
    assert_eq!(p.sell, None);
    assert_eq!(p.cannot_sell_pen, (6 + 0x50, 8 + 0x2B));
}

#[test]
fn sell_detail_panel_with_no_staged_item_still_draws_the_shade_box() {
    for staged in [0, -1] {
        let p = shop_sell_detail_panel((6, 8), staged, 500, Some(1));
        assert_eq!(p.sell, None);
        assert_eq!(p.passive, None);
        assert_eq!(p.shade_box, ((6, 8 + 0x45), (0x90, 0x28)));
    }
}

/// Window 41's trial equip writes the staged piece into the **record byte**
/// its class names: the weapon into the per-character halfword's byte
/// (`2` Vahn, `3` Noa), never byte `0`, which is the body armour. The Retock
/// Arms Shop capture pins the visible result: hovering a weapon raises ATK by
/// the weapon difference and leaves UDF / LDF alone.
#[test]
fn the_compare_trial_replaces_the_weapon_not_the_armour() {
    use crate::equipment::{DiscEquipEntry, DiscEquipInfo};
    use legaia_engine_battle::battle_stats::{EquipmentTable, ItemModifier};
    use legaia_game_tables::equip_stats::EquipSlot as Disc;
    use legaia_save::{CharacterRecord, EquipmentSlots};
    let (armour, vahn_blade, noa_glove, axe) = (0x50, 0x40, 0x42, 0x41);
    let mut table = EquipmentTable::new();
    let m = |atk, udf| ItemModifier {
        atk,
        udf,
        ..Default::default()
    };
    table.set(armour, m(0, 10));
    table.set(vahn_blade, m(40, 0));
    table.set(noa_glove, m(30, 0));
    table.set(axe, m(53, 0));
    let info = DiscEquipInfo::from_entries([(
        axe,
        DiscEquipEntry {
            mask: 3,
            category: Disc::Weapon,
            is_ra_seru: false,
            passive_index: None,
        },
    )]);
    let mut vahn = CharacterRecord::zeroed();
    vahn.set_name("V");
    vahn.set_equipment(EquipmentSlots {
        slots: [armour, 0, vahn_blade, 0, 0, 0, 0, 0],
    });
    let mut noa = CharacterRecord::zeroed();
    noa.set_name("N");
    noa.set_equipment(EquipmentSlots {
        slots: [armour, 0, 0, noa_glove, 0, 0, 0, 0],
    });
    let gala = CharacterRecord::zeroed();
    let members = [vahn, noa, gala];
    // The present party in party-list order: Noa leads, Gala is absent.
    let rows = party_compare_members_of(&members, &[1, 0], &table, Some(&info), axe);
    let names: Vec<_> = rows.iter().map(|r| (r.member, r.name.as_str())).collect();
    assert_eq!(names, [(1, "N"), (0, "V")]);
    const ATK: usize = 3;
    const UDF: usize = 4;
    for (row, old_weapon_atk) in rows.iter().zip([30, 40]) {
        let cand = row.candidate.expect("a weapon has a trial block");
        assert_eq!(cand[ATK] - row.current[ATK], 53 - old_weapon_atk);
        assert_eq!(cand[UDF], row.current[UDF], "the armour stays on");
        assert!(row.equippable);
    }
}

/// A member the mask leaves out reads as not equippable; ids past the
/// three playable characters skip the test, as retail's chain does.
#[test]
fn the_compare_mask_covers_three_characters() {
    use crate::equipment::{DiscEquipEntry, DiscEquipInfo};
    use legaia_engine_battle::battle_stats::EquipmentTable;
    use legaia_game_tables::equip_stats::EquipSlot as Disc;
    let info = DiscEquipInfo::from_entries([(
        0x41,
        DiscEquipEntry {
            mask: 1,
            category: Disc::Head,
            is_ra_seru: false,
            passive_index: None,
        },
    )]);
    let members: Vec<_> = (0..4)
        .map(|_| legaia_save::CharacterRecord::zeroed())
        .collect();
    let rows = party_compare_members_of(
        &members,
        &[0, 1, 2, 3],
        &EquipmentTable::new(),
        Some(&info),
        0x41,
    );
    let ok: Vec<_> = rows.iter().map(|r| r.equippable).collect();
    assert_eq!(ok, [true, false, false, true]);
    assert!(rows.iter().all(|r| r.name.is_empty()), "no invented names");
}

/// The quantity commit closes the stepper under the Point Card toast.
#[test]
fn the_point_card_toast_rides_over_a_closed_stepper() {
    let w = shop_screen_windows(ShopScreenPhase::BuyQuantity, true);
    assert_eq!(w, vec![33, 32, 34, 41, 31]);
    assert!(shop_screen_windows(ShopScreenPhase::BuyQuantity, false).contains(&35));
}

use super::*;

fn env() -> HubEnv {
    HubEnv {
        submode: 1,
        confirm_mask: 0x0060,
        cancel_mask: 0x0010,
        accept_mask: 0x0040,
        back_mask: 0x0020,
        frame_delta: 1,
        ..HubEnv::default()
    }
}

/// Walk the counter from a fresh open to the commit with `coins` typed in,
/// returning every action the run emitted.
fn run_purchase(env0: &HubEnv, coins: i32) -> (Vec<HubAction>, CoinCounter, HubActor) {
    let mut a = HubActor {
        state: 0x25,
        ..HubActor::default()
    };
    let mut c = CoinCounter::default();
    let mut g = HubGrid::default();
    let mut acts = Vec::new();
    let mut e = env0.clone();

    // State 0 -> 1.
    acts.extend(coin_exchange(&mut a, &e, &mut c, &mut g).actions);
    // Type the amount straight into the cells the way the clamp does.
    c.set_entered(coins);
    // Accept -> confirm panel.
    e.pad_edge = e.accept_mask;
    acts.extend(coin_exchange(&mut a, &e, &mut c, &mut g).actions);
    // Pick "Yes" (row 0) and accept.
    e.pad_edge = 0;
    c.yes_no = 0;
    e.picker_result = PICK_ACCEPT;
    acts.extend(coin_exchange(&mut a, &e, &mut c, &mut g).actions);
    // Commit.
    e.picker_result = 0;
    acts.extend(coin_exchange(&mut a, &e, &mut c, &mut g).actions);
    (acts, c, a)
}

#[test]
fn coin_counter_credits_coins_and_debits_gold() {
    // The regression this test exists for: the credit must land in the
    // casino coin bank, never in gold, and the debit is 100x the coins.
    let mut e = env();
    e.gold = 12_345;
    let (acts, _, a) = run_purchase(&e, 100);
    assert!(acts.contains(&HubAction::BuyCoins {
        coins: 100,
        gold_cost: 10_000,
    }));
    // The commit arms the close hold rather than handing back at once.
    assert_eq!(a.sub, 4);
}

#[test]
fn coin_counter_hold_runs_out_before_the_hand_back() {
    let mut e = env();
    e.gold = 1_000;
    let (_, mut c, mut a) = run_purchase(&e, 5);
    let mut g = HubGrid::default();
    assert_eq!(c.hold, COIN_CLOSE_HOLD);
    for _ in 0..COIN_CLOSE_HOLD - 1 {
        coin_exchange(&mut a, &e, &mut c, &mut g);
        assert_eq!(a.state, 0x25, "still on its own slot while the hold runs");
    }
    coin_exchange(&mut a, &e, &mut c, &mut g);
    assert_eq!(a.state, HUB_RETURN_STATE);
    assert_eq!(g.handback, -1);
    assert_eq!(g.stashed_state, 0x25);
}

#[test]
fn coin_counter_digit_cells_are_little_endian() {
    let mut c = CoinCounter::default();
    c.set_entered(1_203);
    assert_eq!(c.digits, [3, 0, 2, 1, 0, 0, 0, 0]);
    assert_eq!(c.entered(), 1_203);
    c.set_entered(9_999_999);
    assert_eq!(c.entered(), 9_999_999);
}

#[test]
fn coin_counter_cursor_wraps_over_six_cells_not_eight() {
    let mut e = env();
    e.gold = 10_000_000;
    let mut a = HubActor {
        sub: 1,
        ..HubActor::default()
    };
    let mut c = CoinCounter::default();
    let mut g = HubGrid::default();
    e.pad_repeat = PAD_CURSOR_RIGHT;
    coin_exchange(&mut a, &e, &mut c, &mut g);
    assert_eq!(
        c.cursor,
        COIN_CURSOR_CELLS - 1,
        "right off the units cell wraps to the top cell"
    );
    e.pad_repeat = PAD_CURSOR_LEFT;
    coin_exchange(&mut a, &e, &mut c, &mut g);
    assert_eq!(c.cursor, 0);
}

#[test]
fn coin_counter_rewrites_an_unaffordable_amount() {
    // 250 gold buys two coins; typing nine into the tens cell asks for 90.
    let mut e = env();
    e.gold = 250;
    let mut a = HubActor {
        sub: 1,
        ..HubActor::default()
    };
    let mut c = CoinCounter {
        cursor: 1,
        ..CoinCounter::default()
    };
    let mut g = HubGrid::default();
    e.pad_repeat = PAD_CURSOR_DOWN; // 0 -> 9 in cell 1
    coin_exchange(&mut a, &e, &mut c, &mut g);
    assert_eq!(c.entered(), 2, "clamped to what 250 gold can pay for");
    assert_eq!(a.sub, 1, "the clamp keeps the screen open");
}

#[test]
fn coin_counter_refuses_an_over_ceiling_accept_without_leaving_entry() {
    let mut e = env();
    e.gold = 9_999_999;
    e.coin_bank = COIN_BANK_MAX; // no headroom at all
    let mut a = HubActor {
        sub: 1,
        ..HubActor::default()
    };
    let mut c = CoinCounter::default();
    c.set_entered(1);
    let mut g = HubGrid::default();
    e.pad_edge = e.accept_mask;
    let f = coin_exchange(&mut a, &e, &mut c, &mut g);
    assert!(f.actions.contains(&HubAction::ConfirmCue(CUE_REFUSE)));
    assert_eq!(a.sub, 1);
    assert!(
        !f.actions
            .iter()
            .any(|a| matches!(a, HubAction::BuyCoins { .. }))
    );
}

#[test]
fn coin_counter_back_edge_hands_back_without_buying() {
    let mut e = env();
    e.gold = 5_000;
    let mut a = HubActor {
        state: 0x25,
        sub: 1,
        ..HubActor::default()
    };
    let mut c = CoinCounter::default();
    c.set_entered(10);
    let mut g = HubGrid::default();
    e.pad_edge = e.back_mask;
    let f = coin_exchange(&mut a, &e, &mut c, &mut g);
    assert!(
        !f.actions
            .iter()
            .any(|a| matches!(a, HubAction::BuyCoins { .. }))
    );
    assert_eq!(a.state, HUB_RETURN_STATE);
}

#[test]
fn coin_counter_zero_accept_is_a_back_out() {
    let mut e = env();
    e.gold = 5_000;
    let mut a = HubActor {
        state: 0x25,
        sub: 1,
        ..HubActor::default()
    };
    let mut c = CoinCounter::default();
    let mut g = HubGrid::default();
    e.pad_edge = e.accept_mask;
    let f = coin_exchange(&mut a, &e, &mut c, &mut g);
    assert!(f.actions.contains(&HubAction::EntryCue(CUE_BACK)));
    assert_eq!(a.state, HUB_RETURN_STATE);
}

#[test]
fn coin_counter_confirm_panel_no_returns_to_entry() {
    let mut e = env();
    e.gold = 5_000;
    let mut a = HubActor {
        sub: 2,
        ..HubActor::default()
    };
    let mut c = CoinCounter {
        yes_no: 1, // the seeded row is "No"
        ..CoinCounter::default()
    };
    let mut g = HubGrid::default();
    e.picker_result = PICK_ACCEPT;
    let f = coin_exchange(&mut a, &e, &mut c, &mut g);
    assert!(
        f.actions
            .contains(&HubAction::InstallPanel(PANEL_COIN_IDLE))
    );
    assert_eq!(a.sub, 1);
}

#[test]
fn coin_exchange_buys_at_a_hundred_gold_each() {
    assert_eq!(coin_exchange_amount(0, 0), 0);
    assert_eq!(coin_exchange_amount(99, 0), 0);
    assert_eq!(coin_exchange_amount(100, 0), 1);
    assert_eq!(coin_exchange_amount(12_345, 0), 123);
}

#[test]
fn coin_exchange_clamps_against_the_bank_ceiling() {
    // Buying would overflow the bank: the amount becomes the headroom.
    let bank = COIN_BANK_MAX - 5;
    assert_eq!(coin_exchange_amount(1_000_000, bank), 5);
    assert_eq!(coin_exchange_amount(1_000_000, COIN_BANK_MAX), 0);
}

#[test]
fn coin_exchange_divide_truncates_toward_zero() {
    // Retail's reciprocal multiply subtracts the sign, so a debt rounds
    // toward zero rather than down.
    assert_eq!(coin_exchange_amount(-150, 0), -1);
}

#[test]
fn coin_exchange_slot_guard_is_unsigned() {
    assert_eq!(coin_exchange_slot(0), Some(0));
    assert_eq!(coin_exchange_slot(4), Some(4));
    assert_eq!(coin_exchange_slot(5), None);
    assert_eq!(coin_exchange_slot(-1), None);
}

#[test]
fn dispatcher_is_inert_outside_the_three_submodes() {
    let mut a = HubActor::default();
    let mut grid = HubGrid::default();
    let mut e = env();
    e.submode = 2;
    let f = hub_dispatch(&mut a, &e, &mut grid, |_, _| HubFrame::default());
    assert!(f.draws.is_empty() && f.actions.is_empty());
    assert_eq!(a.flags, 0);
}

#[test]
fn dispatcher_retires_on_the_pad_latch_without_running_the_handler() {
    let mut a = HubActor::default();
    let mut grid = HubGrid::default();
    let mut e = env();
    e.pad_latch = PAD_LATCH_SUSPEND;
    let mut ran = false;
    let f = hub_dispatch(&mut a, &e, &mut grid, |_, _| {
        ran = true;
        HubFrame::default()
    });
    assert!(!ran);
    assert_eq!(a.flags & ACTOR_RETIRE, ACTOR_RETIRE);
    assert!(f.actions.is_empty());
}

#[test]
fn dispatcher_release_arm_picks_by_the_board_flag() {
    let mut a = HubActor::default();
    let mut grid = HubGrid::default();
    let mut e = env();
    e.board_flag = 0;
    let f = hub_dispatch(&mut a, &e, &mut grid, |_, _| HubFrame::default());
    assert!(f.actions.contains(&HubAction::ClearBusyBit));
    e.board_flag = 1;
    let mut a = HubActor::default();
    let f = hub_dispatch(&mut a, &e, &mut grid, |_, _| HubFrame::default());
    assert!(f.actions.contains(&HubAction::SetBoardFlag));
}

#[test]
fn start_menu_sizes_and_centres_the_panel() {
    let mut a = HubActor::default();
    let mut g = HubGrid::default();
    let mut e = env();
    e.board_entries = [1, 1, 0];
    let f = start_menu(&mut a, &e, &mut g);
    // one base row plus two active entries = 3 rows.
    assert!(f.actions.contains(&HubAction::SizePanel {
        height: 3 * 14 - 2,
        top: 0x2C - ((3 * 14 - 2) >> 1),
    }));
    assert_eq!(a.sub, 1);
}

#[test]
fn start_menu_confirm_hands_the_actor_back() {
    let mut a = HubActor {
        state: 0x11,
        sub: 1,
        ..HubActor::default()
    };
    let mut g = HubGrid::default();
    let mut e = env();
    e.pad_edge = e.confirm_mask;
    let f = start_menu(&mut a, &e, &mut g);
    assert!(f.actions.contains(&HubAction::ConfirmCue(CUE_CONFIRM)));
    assert_eq!(g.stashed_state, 0x11);
    assert_eq!(g.handback, -1);
    assert_eq!(a.state, HUB_RETURN_STATE);
    assert_eq!(a.sub, 0);
}

#[test]
fn a_blocked_frame_swallows_the_confirm() {
    let mut a = HubActor {
        sub: 1,
        ..HubActor::default()
    };
    let mut g = HubGrid::default();
    let mut e = env();
    e.pad_edge = e.confirm_mask;
    e.input_blocked = 1;
    let f = hub_prompt(&mut a, &e, &mut g);
    assert!(!f.actions.contains(&HubAction::ConfirmCue(CUE_CONFIRM)));
    assert_eq!(a.sub, 1);
}

#[test]
fn submenu_walks_its_three_states() {
    let mut a = HubActor::default();
    let mut g = HubGrid::default();
    let mut e = env();
    let f = submenu(&mut a, &e, &mut g);
    assert!(
        f.actions
            .contains(&HubAction::InstallPanel(PANEL_SUBMENU_IDLE))
    );
    e.pad_edge = e.confirm_mask;
    let f = submenu(&mut a, &e, &mut g);
    assert!(
        f.actions
            .contains(&HubAction::InstallPanel(PANEL_SUBMENU_CONFIRM))
    );
    assert_eq!(a.sub, 2);
    let f = submenu(&mut a, &e, &mut g);
    assert!(f.actions.contains(&HubAction::ClearBoardFlag));
    assert_eq!(a.state, HUB_RETURN_STATE);
}

#[test]
fn draw_tick_clears_the_completion_gate_only_while_unblocked() {
    let mut a = HubActor {
        sub: 1,
        ..HubActor::default()
    };
    let mut g = HubGrid {
        done_gate: 5,
        ..HubGrid::default()
    };
    let mut e = env();
    e.input_blocked = 1;
    draw_tick(&mut a, &e, &mut g);
    assert_eq!(g.done_gate, 5);
    e.input_blocked = 0;
    draw_tick(&mut a, &e, &mut g);
    assert_eq!(g.done_gate, 0);
}

#[test]
fn draw_tick_returns_before_the_pump_past_state_one() {
    let mut a = HubActor {
        sub: 2,
        ..HubActor::default()
    };
    let mut g = HubGrid {
        done_gate: 5,
        ..HubGrid::default()
    };
    let f = draw_tick(&mut a, &env(), &mut g);
    assert!(f.actions.is_empty());
    assert_eq!(g.done_gate, 5);
}

#[test]
fn close_tick_differs_from_draw_tick_only_in_state_zero() {
    let mut a = HubActor::default();
    let mut g = HubGrid::default();
    let f = close_tick(&mut a, &env(), &mut g);
    assert_eq!(f.actions[0], HubAction::CloseAllPanels);
}

#[test]
fn deactivate_picks_the_skip_state_from_the_progress_flags() {
    let mut g = HubGrid::default();
    let mut e = env();
    let mut a = HubActor::default();
    deactivate(&mut a, &e, &mut g);
    assert_eq!(a.state, HUB_DEACTIVATE_STATE);

    e.progress_a = 1;
    let mut a = HubActor::default();
    deactivate(&mut a, &e, &mut g);
    assert_eq!(a.state, HUB_SKIP_STATE);

    e.progress_a = 0;
    e.progress_b = 1;
    e.pad_held = e.cancel_mask;
    let mut a = HubActor::default();
    deactivate(&mut a, &e, &mut g);
    assert_eq!(a.state, HUB_SKIP_STATE);

    // The second flag alone is not enough - the cancel button must be held.
    e.pad_held = 0;
    let mut a = HubActor::default();
    deactivate(&mut a, &e, &mut g);
    assert_eq!(a.state, HUB_DEACTIVATE_STATE);
}

#[test]
fn entry_list_skips_codes_at_or_above_three_and_restores_y() {
    let mut a = HubActor {
        x: 10,
        y: 20,
        ..HubActor::default()
    };
    let mut e = env();
    e.entry_count = 3;
    e.entry_codes = vec![0, 5, 2];
    let f = entry_list(&mut a, &e);
    assert_eq!(a.y, 20);
    let labels: Vec<_> = f
        .draws
        .iter()
        .filter_map(|d| match d {
            HubDraw::Text { text, y, .. } => Some((*text, *y)),
            _ => None,
        })
        .collect();
    assert_eq!(
        labels,
        vec![
            (HubString::EntryLabel(0), 20),
            (HubString::EntryLabel(2), 20 + 0x0D + 0x2A),
        ]
    );
    // Every entry publishes its code, drawn or not.
    assert_eq!(
        f.actions,
        vec![
            HubAction::SetEntryCode(0),
            HubAction::SetEntryCode(5),
            HubAction::SetEntryCode(2),
        ]
    );
}

#[test]
fn column_row_steps_thirty_two_pixels_and_biases_the_cell() {
    let a = HubActor {
        x: 0,
        y: 0,
        ..HubActor::default()
    };
    let mut e = env();
    e.cursor_row = 2;
    let g = HubGrid {
        columns: vec![1, 4],
        ..HubGrid::default()
    };
    let f = column_row(&a, &e, &g);
    assert_eq!(
        f.draws[1..],
        [
            HubDraw::Cell {
                x: 0x10,
                y: 0x10,
                cell: 0x38
            },
            HubDraw::Cell {
                x: 0x30,
                y: 0x10,
                cell: 0x3B
            },
        ]
    );
}

#[test]
fn two_option_panel_draws_the_cursor_before_its_own_label() {
    let a = HubActor::default();
    let mut e = env();
    e.cursor_row = 1;
    let f = two_option_panel(&a, &e);
    assert!(matches!(f.draws[0], HubDraw::Text { .. }));
    assert!(matches!(f.draws[1], HubDraw::Sprite { .. }));
    assert!(matches!(f.draws[2], HubDraw::Text { .. }));
}

#[test]
fn count_gated_label_picks_the_alternate_below_two() {
    let a = HubActor::default();
    let mut e = env();
    e.entry_count = 1;
    let f = count_gated_label(&a, &e);
    assert!(matches!(
        f.draws[0],
        HubDraw::Text { text: HubString::Literal(v), .. } if v == STR_COUNT_GATED[1]
    ));
    e.entry_count = 2;
    let f = count_gated_label(&a, &e);
    assert!(matches!(
        f.draws[0],
        HubDraw::Text { text: HubString::Literal(v), .. } if v == STR_COUNT_GATED[0]
    ));
}

#[test]
fn edge_cursor_anchors_on_the_panel_right_edge() {
    let a = HubActor {
        x: 0x20,
        y: 0x30,
        width: 0x60,
        ..HubActor::default()
    };
    assert_eq!(
        single_label(&a).draws[1],
        HubDraw::Sprite {
            a: 1,
            b: 1,
            x: 0x20 + 0x60 - 0x10,
            y: 0x2E
        }
    );
}

#[test]
fn acquisition_caption_adds_the_amount_only_for_the_point_card_id() {
    let a = HubActor::default();
    let mut e = env();
    e.caption_item = 3;
    assert_eq!(acquisition_caption(&a, &e).draws.len(), 2);
    e.caption_item = CAPTION_POINT_CARD_ID;
    e.caption_amount = 1234;
    let f = acquisition_caption(&a, &e);
    assert_eq!(f.draws.len(), 4);
    assert_eq!(
        f.draws[3],
        HubDraw::Number {
            value: 1234,
            digits: 8,
            x: 0x38,
            y: 0x4E
        }
    );
}

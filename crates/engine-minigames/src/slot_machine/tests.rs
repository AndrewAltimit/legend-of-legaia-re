use super::*;
use legaia_engine_system::rand::BiosRand;
use legaia_minigame_formats::minigame_slot_scene::LANDING_LINE_BY_JITTER;
use legaia_minigame_formats::minigame_slot_scene::PAYLINE_ROW_OFFSETS;
use legaia_minigame_formats::slot_payout::SlotPayoutTable;

#[test]
fn payline_prims_light_exactly_the_winning_line() {
    use legaia_minigame_formats::minigame_slot_scene::{PayLine, Pos3};
    let p = |y: i16| PayLine {
        a: Pos3 {
            x: -640,
            y,
            z: -768,
        },
        b: Pos3 { x: 640, y, z: -768 },
    };
    let table = [p(-192), p(0), p(192), p(320), p(-320)];

    let prims = payline_prims(&table, 2);
    assert_eq!(prims.len(), 5);
    assert_eq!(
        prims.iter().map(|q| q.lit).collect::<Vec<_>>(),
        [false, false, true, false, false]
    );
    assert_eq!(prims[2].color, PAYLINE_COLOR_LIT);
    assert_eq!(prims[0].color, PAYLINE_COLOR_IDLE);
    // The code byte is the same on both - retail patches only the
    // colour bytes, so the line stays semi-transparent when lit.
    assert!(prims.iter().all(|q| q.code == PAYLINE_GP0_CODE));
    // Geometry passes through untouched.
    assert_eq!(prims[3].a, table[3].a);
    assert_eq!(prims[3].b, table[3].b);
}

#[test]
fn payline_index_zero_lights_when_the_winner_word_is_zero() {
    use legaia_minigame_formats::minigame_slot_scene::{PayLine, Pos3};
    let z = Pos3 { x: 0, y: 0, z: 0 };
    let table = [PayLine { a: z, b: z }; 5];
    assert!(payline_prims(&table, 0)[0].lit);
    // Anything outside 0..5 leaves the whole rack dark.
    assert!(payline_prims(&table, -1).iter().all(|q| !q.lit));
    assert!(payline_prims(&table, 9).iter().all(|q| !q.lit));
}

#[test]
fn payline_ot_depth_rounds_toward_zero_then_shifts() {
    assert_eq!(payline_ot_depth(16, 0), 4);
    assert_eq!(payline_ot_depth(16, 2), 1);
    // Negative depths take the +3 bias so the >>2 truncates toward zero.
    assert_eq!(payline_ot_depth(-1, 0), 0);
    assert_eq!(payline_ot_depth(-4, 0), -1);
    assert_eq!(payline_ot_depth(-5, 0), -1);
}

#[test]
fn coin_entry_field_is_least_significant_first() {
    // Units in slot 0: 1234 = 4,3,2,1 then blanks.
    assert_eq!(coin_entry_value(&[4, 3, 2, 1, 0, 0, 0, 0]), 1234);
    assert_eq!(coin_entry_value(&[0; 8]), 0);
    // Every slot filled with 9 = the widest enterable count.
    assert_eq!(coin_entry_value(&[9; 8]), 99_999_999);
}

#[test]
fn coin_exchange_charges_a_hundred_gold_each() {
    let q = coin_exchange_quote(&[5, 0, 0, 0, 0, 0, 0, 0], 1000, 100);
    assert_eq!(q.coins, 5);
    assert_eq!(q.cost, 500);
    assert!(q.is_valid());
}

#[test]
fn coin_exchange_gates_on_gold_and_on_stock_independently() {
    // Affordable but the counter is short: stock gate alone fails.
    let q = coin_exchange_quote(&[9, 0, 0, 0, 0, 0, 0, 0], 100_000, 5);
    assert!(q.affordable, "gold covers 900");
    assert!(!q.in_stock, "counter only holds 5");
    assert!(!q.is_valid());

    // In stock but the party is short: gold gate alone fails.
    let q = coin_exchange_quote(&[9, 0, 0, 0, 0, 0, 0, 0], 100, 100);
    assert!(!q.affordable);
    assert!(q.in_stock);
    assert!(!q.is_valid());
}

fn panel(digits: &[i8], cursor: i32, gold: i32, stock: i32, clock: u32) -> Vec<CoinPanelDraw> {
    coin_entry_panel(&CoinPanelInput {
        origin: COIN_ENTRY_ORIGIN,
        digits,
        cursor,
        bank: 42,
        gold,
        stock,
        clock,
    })
}

#[test]
fn the_entry_panel_lays_out_four_rows_off_the_window_origin() {
    let (x, y) = COIN_ENTRY_ORIGIN;
    let d = panel(&[4, 3, 2, 1, 0, 0, 0, 0], 0, 5_000, 50, 4);
    let labels: Vec<_> = d
        .iter()
        .filter_map(|e| match *e {
            CoinPanelDraw::Label { va, x, y, pen } => Some((va, x, y, pen)),
            _ => None,
        })
        .collect();
    assert_eq!(
        labels,
        vec![
            (COIN_ENTRY_LABEL_VAS[0], x, y + 2, 7),
            (COIN_ENTRY_LABEL_VAS[1], x, y + 0x12, 6),
            (COIN_ENTRY_LABEL_VAS[2], x, y + 0x30, 7),
            (COIN_ENTRY_LABEL_VAS[3], x, y + 0x40, 7),
        ]
    );
    let numbers: Vec<_> = d
        .iter()
        .filter_map(|e| match *e {
            CoinPanelDraw::Number {
                value,
                digits,
                x,
                y,
                pen,
            } => Some((value, digits, x, y, pen)),
            _ => None,
        })
        .collect();
    // Bank, six single cells (cell 5 leftmost), gold, total.
    assert_eq!(numbers[0], (42, 8, x + 0x78, y + 2, 7));
    let cells: Vec<_> = numbers[1..7].iter().map(|n| (n.0, n.2)).collect();
    assert_eq!(
        cells,
        vec![
            (0, x + 0x88),
            (0, x + 0x90),
            (1, x + 0x98),
            (2, x + 0xA0),
            (3, x + 0xA8),
            (4, x + 0xB0),
        ]
    );
    assert_eq!(numbers[7], (5_000, 8, x + 0x78, y + 0x30, 7));
    assert_eq!(
        numbers[8],
        (123_400, 8, x + 0x78, y + 0x40, COIN_PEN_REFUSED)
    );
}

#[test]
fn the_caret_sits_under_the_edited_cell_and_blinks_on_the_clock() {
    let (x, y) = COIN_ENTRY_ORIGIN;
    let caret = |d: &[CoinPanelDraw]| {
        d.iter().find_map(|e| match *e {
            CoinPanelDraw::Cell { x, y, cell } => Some((x, y, cell)),
            _ => None,
        })
    };
    let d = panel(&[0; 8], 2, 0, 0, 4);
    assert_eq!(caret(&d), Some((x + 0xA0, y + 0x1D, COIN_ENTRY_CARET_CELL)));
    // The clock's 0xC bits clear: the caret is off this frame.
    assert_eq!(caret(&panel(&[0; 8], 2, 0, 0, 0x10)), None);
    assert_eq!(caret(&panel(&[0; 8], 2, 0, 0, 3)), None);
}

#[test]
fn the_total_takes_the_normal_pen_only_when_both_gates_pass() {
    let pen_of = |d: Vec<CoinPanelDraw>| match d.last() {
        Some(CoinPanelDraw::Number { pen, value, .. }) => (*pen, *value),
        _ => panic!("the total is the last draw"),
    };
    assert_eq!(
        pen_of(panel(&[5, 0, 0, 0, 0, 0, 0, 0], 0, 500, 5, 4)),
        (COIN_PEN_TOTAL, 500)
    );
    assert_eq!(
        pen_of(panel(&[5, 0, 0, 0, 0, 0, 0, 0], 0, 499, 5, 4)).0,
        COIN_PEN_REFUSED
    );
    assert_eq!(
        pen_of(panel(&[5, 0, 0, 0, 0, 0, 0, 0], 0, 500, 4, 4)).0,
        COIN_PEN_REFUSED
    );
}

#[test]
fn coin_exchange_allows_exactly_affordable_and_exact_stock() {
    // Both gates are `>=`, so an exact match still sells.
    let q = coin_exchange_quote(&[3, 0, 0, 0, 0, 0, 0, 0], 300, 3);
    assert_eq!(q.cost, 300);
    assert!(q.is_valid());
}

fn payouts() -> SlotPayoutTable {
    // Synthetic table: symbol id i pays (i+1)*2 coins.
    let mut payouts = [0u8; SYMBOL_COUNT];
    for (i, p) in payouts.iter_mut().enumerate() {
        *p = ((i + 1) * 2) as u8;
    }
    SlotPayoutTable { payouts }
}

#[test]
fn slot_lcg_folds_the_halves() {
    // seed 0: x = 0*5+1 = 1; folded = (1<<16) + 0 = 0x10000.
    let mut r = SlotRng::new(0);
    assert_eq!(r.next_u32(), 0x10000);
    // next: x = 0x10000*5+1 = 0x50001; folded = (0x50001<<16)+(0x50001>>16)
    //       = 0x00010000 + 5 = 0x10005.
    assert_eq!(r.next_u32(), 0x10005);
    // Deterministic per seed.
    let mut a = SlotRng::new(0xDEAD_BEEF);
    let mut b = SlotRng::new(0xDEAD_BEEF);
    assert_eq!(a.next_u32(), b.next_u32());
}

#[test]
fn strip_is_a_two_of_each_permutation_for_both_probe_steps() {
    let mut rng = SlotRng::new(12345);
    let (symbols, bonus) = build_reel(&mut rng);
    // The symbol half probes by STRIP_PROBE_PRIMARY over base 0; the bonus
    // half probes by STRIP_PROBE_SECONDARY over BONUS_VALUE_BASE. Both are
    // two-of-each permutations of the ten values.
    for (strip, base, probe) in [
        (symbols, 0u8, STRIP_PROBE_PRIMARY),
        (bonus, slot_payout::BONUS_VALUE_BASE, STRIP_PROBE_SECONDARY),
    ] {
        let mut counts = [0usize; SYMBOL_COUNT];
        for &s in &strip {
            let id = s.wrapping_sub(base) as usize;
            assert!(id < SYMBOL_COUNT, "symbol id in range");
            counts[id] += 1;
        }
        assert_eq!(
            counts, [2; SYMBOL_COUNT],
            "each symbol twice (probe {probe})"
        );
    }
}

/// The bonus strip is the same permutation over a rebased value space: the
/// numerals `1..=10` as values `0x10..=0x19`, two rows each.
#[test]
fn the_bonus_strip_carries_the_numerals_as_values_0x10_to_0x19() {
    let mut rng = SlotRng::new(0xBEEF);
    let (symbols, bonus) = build_reel(&mut rng);
    let mut counts = [0usize; SYMBOL_COUNT];
    for &v in &bonus {
        let n = slot_payout::bonus_number_for_value(v);
        assert!(
            (slot_payout::BONUS_VALUE_BASE..=0x19).contains(&v),
            "bonus row {v:#x} is a bonus-strip value"
        );
        assert!((1..=10).contains(&n), "and shows a numeral 1..=10");
        counts[(n - 1) as usize] += 1;
    }
    assert_eq!(counts, [2; SYMBOL_COUNT], "each numeral twice");
    // The two strips are independent permutations - not the same order.
    assert!(
        symbols
            .iter()
            .zip(bonus.iter())
            .any(|(&s, &b)| s + slot_payout::BONUS_VALUE_BASE != b),
        "the two strips are shuffled independently"
    );
}

#[test]
fn land_row_finds_target_within_depth_else_next_row() {
    let mut strip = [0u8; STRIP_LEN];
    strip[9] = 7;
    // From payline row 2 retail searches rows 7.. (raw cur+1..): depth 4
    // reaches row 9, and the middle line puts the target on the payline.
    assert_eq!(land_row(&strip, 2, 4, Some(7), 0), 9);
    // The top line stops one row short, so the target shows one row up
    // (`centre + 1`), which is what line 0's `+1` offset reads.
    assert_eq!(land_row(&strip, 2, 4, Some(7), 1), 8);
    // The bottom line stops one row past it.
    assert_eq!(land_row(&strip, 2, 4, Some(7), -1), 10);
    // Depth too shallow (rows 7, 8) -> next natural row.
    assert_eq!(land_row(&strip, 2, 2, Some(7), 0), 3);
    // The current payline row is never a candidate.
    strip[2] = 7;
    assert_eq!(land_row(&strip, 2, 2, Some(7), 0), 3);
    // Wraps around the strip end: from 14 the window is 19, 0, 1.
    strip[1] = 9;
    assert_eq!(land_row(&strip, 14, 3, Some(9), -1), 2);
    // The bonus round's plan (depth 0 / no target) never searches: the reel
    // lands on the next row, wherever that is.
    assert_eq!(land_row(&strip, 5, 0, None, 0), 6);
    assert_eq!(land_row(&strip, 19, 0, None, 0), 0);
}

/// The landing line follows the spin's jitter: with the same strip and the
/// same search, each `rand % 5` row lands the target on its own payline,
/// and the win the evaluator finds is on that line.
#[test]
fn landing_line_follows_the_jitter_row() {
    for (jitter, &line) in LANDING_LINE_BY_JITTER.iter().enumerate() {
        let mut strip = [0u8; STRIP_LEN];
        strip[9] = 7;
        let rows: Vec<usize> = (0..REEL_COUNT)
            .map(|r| land_row(&strip, 2, 4, Some(7), PAYLINE_ROW_OFFSETS[line][r]))
            .collect();
        for (r, &row) in rows.iter().enumerate() {
            let shown = (row as i32 + PAYLINE_ROW_OFFSETS[line][r]).rem_euclid(STRIP_LEN as i32);
            assert_eq!(strip[shown as usize], 7, "jitter {jitter} reel {r}");
        }
    }
}

#[test]
fn feature_roll_shapes() {
    let mut rand = BiosRand::new(7);
    let roll = feature_roll(&mut rand, 500, 0, false);
    assert!(roll.jitter < 5);
    assert!((2..8).contains(&roll.normal_target));
    // With a feature already active the entry rolls are skipped.
    let mut rand = BiosRand::new(7);
    let roll = feature_roll(&mut rand, 500, 6, false);
    assert_eq!(roll.entered_mode, None);
}

#[test]
fn stop_plan_by_mode() {
    let mut rng = SlotRng::new(3);
    let (d, t) = stop_plan(&mut rng, 0, 4, None);
    assert!((2..5).contains(&d), "mode 0 depth = rand%3 + 2");
    assert_eq!(t, Some(4));
    let (d, t) = stop_plan(&mut rng, 1, 4, None);
    assert!((6..10).contains(&d), "reach depth = (rand&3) + 6");
    assert_eq!(t, Some(9));
    let (_, t) = stop_plan(&mut rng, 2, 4, None);
    assert_eq!(t, Some(8));
    // Guaranteed mode drives to the already-landed symbol.
    let (d, t) = stop_plan(&mut rng, 4, 4, Some(6));
    assert_eq!((d, t), (STRIP_LEN, Some(6)));
    // The bonus round steers nothing: no target, no search.
    assert_eq!(
        stop_plan(&mut rng, FEATURE_MODE_BONUS, 4, Some(6)),
        (0, None)
    );
}

/// `FUN_801d1af4`: two landed reels showing a punch pair on a payline
/// raise the latch to `1`, a kick pair to `2`; one landed reel, or a pair
/// on a reel still spinning, raises nothing; the third stop clears it.
#[test]
fn two_landed_bonus_symbols_raise_the_anticipation_latch() {
    let mut m = SlotMachine::new(payouts(), 42, 50);
    assert!(m.spin());
    for _ in 0..SPIN_UP_FRAMES {
        m.tick();
    }
    assert_eq!(m.phase(), SlotPhase::Stopping);
    let plant = |m: &mut SlotMachine, sym: u8| {
        for r in 0..REEL_COUNT {
            m.reel_pos[r] = 0;
            m.strips[r] = [r as u8; STRIP_LEN];
        }
        // Top row (line 0, `+1`) of reels 0 and 2 - the `(0,2)` pair.
        m.strips[0][1] = sym;
        m.strips[2][1] = sym;
    };
    plant(
        &mut m,
        legaia_minigame_formats::slot_payout::PUNCH_SYMBOL_ID,
    );
    m.stopped = [Some(0), None, None];
    m.tick();
    assert_eq!(m.anticipation(), 0, "one landed reel is not a pair");
    m.stopped = [Some(0), None, Some(0)];
    m.tick();
    assert_eq!(m.anticipation(), 1, "a punch pair");
    plant(&mut m, legaia_minigame_formats::slot_payout::KICK_SYMBOL_ID);
    m.tick();
    assert_eq!(m.anticipation(), 2, "a kick pair");
    assert!(m.stop_reel(1));
    assert_eq!(m.anticipation(), 0, "the third stop clears it");
}

#[test]
fn spin_charges_the_bet_and_sequences_phases() {
    let mut m = SlotMachine::new(payouts(), 42, 50);
    assert_eq!(m.phase(), SlotPhase::Idle);
    assert!(m.spin());
    assert_eq!(m.balance(), 50 - SPIN_COST_NORMAL);
    assert_eq!(m.net_take(), NET_TAKE_NORMAL_SPIN);
    assert_eq!(m.phase(), SlotPhase::Spinning);
    // Reels advance while spinning; timer runs down into Stopping.
    for _ in 0..SPIN_UP_FRAMES {
        m.tick();
    }
    assert_eq!(m.phase(), SlotPhase::Stopping);
    assert!(m.can_stop());
    // Stop all three reels; the spin evaluates into Payout.
    assert!(m.stop_next_reel());
    m.tick();
    assert!(m.stop_next_reel());
    m.tick();
    assert!(m.stop_next_reel());
    assert_eq!(m.phase(), SlotPhase::Payout);
    assert_eq!(m.reels_stopped(), REEL_COUNT);
    let result = m.last_result().expect("evaluated");
    // Collect returns to idle, crediting exactly the evaluated payout.
    let before = m.balance();
    let credited = m.collect();
    assert_eq!(credited, result.payout);
    assert_eq!(m.balance(), before + credited);
    assert_eq!(m.phase(), SlotPhase::Idle);
}

#[test]
fn a_spin_up_press_latches_for_exactly_one_roll() {
    let mut m = SlotMachine::new(payouts(), 42, 200);
    // Idle: the latch is state 2's alone.
    m.latch_spin_up(true);
    assert!(!m.spin_up_pressed());
    assert!(m.spin());
    m.tick();
    assert_eq!(m.phase(), SlotPhase::Spinning);
    m.latch_spin_up(false);
    assert!(!m.spin_up_pressed(), "no edge, no latch");
    m.latch_spin_up(true);
    assert!(m.spin_up_pressed(), "a face edge mid spin-up latches");
    // Run the spin out and start the next: its roll reads the latch, and
    // state 1 clears it straight after.
    while m.phase() == SlotPhase::Spinning {
        m.tick();
    }
    for r in 0..REEL_COUNT {
        m.stop_reel(r);
        for _ in 0..0x40 {
            m.tick();
        }
    }
    if m.phase() == SlotPhase::Payout {
        m.collect();
    }
    assert!(m.spin_up_pressed(), "survives until the next roll");
    assert!(m.spin());
    assert!(!m.spin_up_pressed(), "the next spin's roll consumed it");
}

#[test]
fn spin_gate_blocks_a_thin_balance() {
    let mut m = SlotMachine::new(payouts(), 42, MIN_SPIN_BALANCE - 1);
    assert!(!m.can_spin());
    assert!(!m.spin());
    assert_eq!(m.phase(), SlotPhase::Idle);
}

#[test]
fn winning_line_pays_the_table_value() {
    let mut m = SlotMachine::new(payouts(), 9, 100);
    assert!(m.spin());
    for _ in 0..SPIN_UP_FRAMES {
        m.tick();
    }
    // Force a known middle-line win: overwrite the display strips so the
    // payline rows all read symbol 5, then stop with rigged positions.
    for reel in 0..REEL_COUNT {
        m.strips[reel] = [5; STRIP_LEN];
    }
    m.stop_reel(0);
    m.stop_reel(1);
    m.stop_reel(2);
    let r = m.last_result().expect("evaluated");
    assert_eq!(r.symbol, Some(5));
    assert_eq!(r.payout, (5 + 1) * 2);
    assert!(!r.bonus_triggered);
}

/// Rig a matching line of `sym` on the middle row and stop out of it, so the
/// spin resolves as a win on that symbol.
fn win_on(m: &mut SlotMachine, sym: u8) {
    assert!(m.spin());
    while m.phase() != SlotPhase::Stopping {
        m.tick();
    }
    for reel in 0..REEL_COUNT {
        m.strips[reel] = [sym; STRIP_LEN];
    }
    m.stop_reel(0);
    m.stop_reel(1);
    m.stop_reel(2);
}

/// Play one whole bonus round: spin, run the (long) spin-up out, stop the
/// three reels.
fn play_bonus_spin(m: &mut SlotMachine) {
    assert!(m.spin());
    while m.phase() != SlotPhase::Stopping {
        m.tick();
    }
    for reel in 0..REEL_COUNT {
        // A few frames between stops, as a player's fingers would.
        m.tick();
        assert!(m.stop_reel(reel));
    }
}

#[test]
fn jackpot_symbols_trigger_the_bonus_round_and_product_payout() {
    let mut m = SlotMachine::new(payouts(), 9, 100);
    win_on(&mut m, 9);
    let r = m.last_result().expect("evaluated");
    assert!(r.bonus_triggered);
    assert!(m.in_bonus_round());
    assert_eq!(m.bonus_spins(), BONUS_SPINS_JACKPOT);
    m.collect();

    // A bonus "free" spin still costs 1 coin (the mode-4..6 charge).
    let before = m.balance();
    let take_before = m.net_take();
    play_bonus_spin(&mut m);
    assert_eq!(
        m.balance(),
        before - SPIN_COST_FEATURE,
        "feature spin costs 1 coin"
    );
    let r = m.last_result().expect("evaluated");
    assert!(r.bonus_spin);
    assert_eq!(r.line, Some(1), "a bonus round pays the centre line");

    // The reels carry numbers now, and the payout is their product.
    let numbers = m.tally();
    assert!(
        numbers.iter().all(|&n| (1..=10).contains(&n)),
        "three numerals 1..=10, got {numbers:?}"
    );
    assert_eq!(
        r.payout,
        numbers.iter().product::<u32>() as i32,
        "the payout is the product of the three numbers the tally shows"
    );
    assert!((1..=1000).contains(&r.payout), "bounded 1..=1000");

    assert_eq!(m.bonus_spins(), BONUS_SPINS_JACKPOT - 1);
    assert_eq!(
        m.net_take(),
        take_before + NET_TAKE_FEATURE_SPIN - r.payout,
        "bonus payout is subtracted from the net take"
    );
}

/// The reels really do swap: after a bonus round opens, the value under
/// every payline is a bonus-strip value, so the renderer draws the numeral
/// art - and after the round ends they swap back to symbols.
#[test]
fn the_bonus_round_rotates_the_numbers_onto_the_reels_and_back_off() {
    let mut m = SlotMachine::new(payouts(), 0x51075, 200);
    // One kick = exactly one bonus round, so the round boundary is crisp.
    win_on(&mut m, slot_payout::KICK_SYMBOL_ID);
    assert!(m.in_bonus_round());
    assert_eq!(m.bonus_spins(), BONUS_SPINS_BONUS);
    m.collect();

    play_bonus_spin(&mut m);
    for r in 0..REEL_COUNT {
        let v = m.payline_symbol(r);
        assert!(
            v >= slot_payout::BONUS_VALUE_BASE,
            "reel {r} pays on a bonus value, got {v:#x}"
        );
    }
    // The single round is spent: the machine is back in the normal game.
    assert!(!m.in_bonus_round());
    assert_eq!(m.bonus_spins(), 0);
    m.collect();

    // ...and the next spin's payline is a symbol again - the strip rotated
    // back on its own, which is what the long post-bonus spin-up buys.
    assert!(m.spin());
    while m.phase() != SlotPhase::Stopping {
        m.tick();
    }
    for r in 0..REEL_COUNT {
        m.tick();
        m.stop_reel(r);
        let v = m.payline_symbol(r);
        assert!(
            v < slot_payout::BONUS_VALUE_BASE,
            "reel {r} is back on a symbol id, got {v:#x}"
        );
    }
}

/// The falsifiable half of the tally: it is not a display copy that could
/// drift from the result. Each column fills only as its reel is claimed, and
/// the finished tally's product **is** the payout the evaluator computed.
#[test]
fn the_tally_fills_per_claimed_reel_and_its_product_is_the_payout() {
    let mut m = SlotMachine::new(payouts(), 7, 200);
    win_on(&mut m, slot_payout::PUNCH_SYMBOL_ID);
    assert!(m.in_bonus_round());
    m.collect();

    assert!(m.spin());
    // The bet charge clears the tally: `0 x 0 x 0`.
    assert_eq!(m.tally(), [0, 0, 0]);
    assert!(!m.tally_complete());
    assert_eq!(m.tally_product(), 0);
    while m.phase() != SlotPhase::Stopping {
        m.tick();
    }
    for reel in 0..REEL_COUNT {
        m.tick();
        assert!(m.stop_reel(reel));
        let tally = m.tally();
        // Exactly the claimed columns are filled; the rest still read 0.
        for (r, &n) in tally.iter().enumerate() {
            if r <= reel {
                assert!((1..=10).contains(&n), "claimed column {r} shows its number");
                assert_eq!(
                    n,
                    slot_payout::bonus_number_for_value(m.payline_symbol(r)),
                    "and it is the number that reel actually stopped on"
                );
            } else {
                assert_eq!(n, 0, "unclaimed column {r} still reads 0");
            }
        }
    }
    let r = m.last_result().expect("evaluated");
    assert!(m.tally_complete());
    assert_eq!(
        m.tally_product() as i32,
        r.payout,
        "the tally's product is the coins the round pays"
    );
    // And the balance takes exactly that.
    let before = m.balance();
    assert_eq!(m.collect(), r.payout);
    assert_eq!(m.balance(), before + r.payout);
}

#[test]
fn balance_caps_in_the_tally_and_cash_out_returns_it() {
    let mut m = SlotMachine::new(payouts(), 9, BALANCE_CAP - 1);
    assert!(m.spin());
    for _ in 0..SPIN_UP_FRAMES {
        m.tick();
    }
    for reel in 0..REEL_COUNT {
        m.strips[reel] = [7; STRIP_LEN];
    }
    m.stop_reel(0);
    m.stop_reel(1);
    m.stop_reel(2);
    m.collect();
    assert!(m.balance() <= BALANCE_CAP);
    let committed = m.cash_out();
    assert_eq!(committed, m.balance());
    assert_eq!(m.phase(), SlotPhase::CashedOut);
}

/// Rig a machine whose three reels are stopped on row 0 with the given
/// per-reel row offsets carrying symbol `sym`, and every other cell distinct.
fn rigged(sym: u8, offsets: [i32; REEL_COUNT]) -> SpinResult {
    let mut m = SlotMachine::new(payouts(), 9, 100);
    assert!(m.spin());
    for _ in 0..SPIN_UP_FRAMES {
        m.tick();
    }
    for (reel, &off) in offsets.iter().enumerate() {
        // Fill with symbols that can never line up across all three reels.
        let mut s = [0u8; STRIP_LEN];
        for (row, cell) in s.iter_mut().enumerate() {
            *cell = ((reel * 3 + row) % 5) as u8;
        }
        s[off.rem_euclid(STRIP_LEN as i32) as usize] = sym;
        m.strips[reel] = s;
        m.reel_pos[reel] = 0;
        m.reel_vel[reel] = 0;
        m.stopped[reel] = Some(0);
    }
    m.evaluate_spin()
}

#[test]
fn all_five_paylines_always_play() {
    // There is no bet-line selection: a match on ANY of the five lines pays,
    // and the line index is the medallion the machine lights.
    //
    // Retail's per-reel row offsets (relative to the payline row):
    //   0 = top   (+1 +1 +1)      3 = diagonal (-1  0 +1)
    //   1 = middle ( 0  0  0)     4 = diagonal (+1  0 -1)
    //   2 = bottom (-1 -1 -1)
    for (line, offsets) in legaia_minigame_formats::minigame_slot_scene::PAYLINE_ROW_OFFSETS
        .iter()
        .enumerate()
    {
        let r = rigged(6, *offsets);
        assert_eq!(r.symbol, Some(6), "line {line} pays");
        assert_eq!(r.payout, (6 + 1) * 2);
        assert_eq!(r.line, Some(line), "the winning line index is {line}");
    }
}

#[test]
fn the_two_diagonals_are_real_lines_and_not_the_straights() {
    // The falsifiable half: a diagonal match must NOT be reported as a
    // straight line, and a straight must not be reported as a diagonal.
    let diag = rigged(6, [-1, 0, 1]);
    assert_eq!(diag.line, Some(3), "bottom-left to top-right is line 3");
    let straight = rigged(6, [0, 0, 0]);
    assert_eq!(straight.line, Some(1), "the middle row is line 1");
}

#[test]
fn feature_odds_bracket_on_the_net_take() {
    // Low net take -> large denominators (rare); high -> small
    // (frequent). The exact edges 1000 / 2000 fall in no bracket.
    assert_eq!(feature_denominators(0), Some((700, 500)));
    assert_eq!(feature_denominators(999), Some((700, 500)));
    assert_eq!(feature_denominators(1000), None);
    assert_eq!(feature_denominators(1001), Some((350, 250)));
    assert_eq!(feature_denominators(1999), Some((350, 250)));
    assert_eq!(feature_denominators(2000), None);
    assert_eq!(feature_denominators(2001), Some((175, 125)));
    // Empirically the high bracket enters features far more often than
    // the low one over the same stream length.
    let hits = |take: i32| -> usize {
        let mut rand = BiosRand::new(0x1234_5678);
        (0..4000)
            .filter(|_| {
                feature_roll(&mut rand, take, 0, false)
                    .entered_mode
                    .is_some()
            })
            .count()
    };
    assert!(
        hits(2500) > hits(500) * 2,
        "high net take is far more generous"
    );
}

/// The marquee is driven by the machine, not decoration beside it: a paying
/// spin has to put that spin's own figure on the dot matrix.
///
/// Symbol 6 pays `(6+1)*2 = 14` in the synthetic table, so the caption is a
/// two-digit figure - which pins the leading-zero suppression too: the
/// thousands and hundreds places must be absent, and the tens and units
/// present at their own columns.
#[test]
fn a_paying_spin_puts_its_own_figure_on_the_marquee() {
    use legaia_minigame_formats::minigame_slot_scene as scene;

    let mut m = SlotMachine::new(payouts(), 42, 200);
    // Nothing is captioned before a spin resolves.
    assert!(
        m.marquee_placements().is_empty(),
        "an idle machine in the normal game captions nothing"
    );

    win_on(&mut m, 6);
    let payout = m.last_result().unwrap().payout;
    assert_eq!(payout, 14, "synthetic table pays (6+1)*2");
    // The payout state starts its caption timer at 0 and advances it on
    // its first frame (`0x801CF850..0x801CF868`, then `0x801CF898`).
    assert!(
        m.marquee_placements().is_empty(),
        "no caption on the eval frame"
    );
    m.tick();

    let p = m.marquee_placements();
    let at = |col: usize| {
        p.iter()
            .find(|q| q.col == col as i32)
            .map(|q| q.msg)
            .unwrap_or_else(|| panic!("nothing placed at dot column {col}: {p:?}"))
    };
    // "14 coin": tens then units then the word, at the retail columns.
    assert_eq!(at(scene::PAYOUT_DIGIT_COLS[2]), scene::MSG_NUMBER_BASE + 1);
    assert_eq!(at(scene::PAYOUT_DIGIT_COLS[3]), scene::MSG_NUMBER_BASE + 4);
    assert_eq!(at(scene::PAYOUT_COINS_COL), scene::MSG_COINS);
    // Leading zeros are suppressed, not drawn as "0".
    for place in [0usize, 1] {
        assert!(
            !p.iter()
                .any(|q| q.col == scene::PAYOUT_DIGIT_COLS[place] as i32),
            "place {place} is above the figure and must not draw"
        );
    }

    // The caption comes down with the tally it captioned.
    m.collect();
    assert!(
        m.marquee_placements().is_empty(),
        "collecting the payout clears the caption"
    );
}

/// The machine's sound writes, in retail's order: the bet keys the reel
/// motor, each stop stores the click into ring slot 0, the evaluation
/// releases the motor, and the timed tally ticks once per transfer into
/// slot 0 - `11` coins while more than `20` are owed, else `1`, on odd
/// frames - before the state drops back to idle on its own.
#[test]
fn a_paying_spin_sounds_and_tallies_like_the_overlay() {
    let mut m = SlotMachine::new(payouts(), 42, 200);
    win_on(&mut m, 6);
    let s = m.take_sounds();
    assert_eq!(
        s.voice_on.first().map(|v| v.voice),
        Some(u32::from(SPIN_VOICE))
    );
    assert_eq!(s.voice_on[0].vab_program_tone, (2, 1, 0));
    assert_eq!(
        s.ring.iter().filter(|r| **r == (0, CUE_REEL_STOP)).count(),
        3,
        "one click per stop: {:?}",
        s.ring
    );
    assert_eq!(s.voice_off, vec![SPIN_VOICE]);
    let before = m.balance();
    let owed = m.payout_left();
    assert_eq!(owed, 14);
    let mut ticks = 0;
    for _ in 0..PAYOUT_HOLD_FRAMES + 40 {
        m.tick();
        ticks += m
            .take_sounds()
            .ring
            .iter()
            .filter(|r| **r == (0, CUE_PAYOUT_TICK))
            .count();
        if m.phase() == SlotPhase::Idle {
            break;
        }
    }
    assert_eq!(m.balance(), before + owed, "the tally pays the whole win");
    assert_eq!(ticks, 14, "14 coins under 20 owed: one-coin transfers");
    assert_eq!(m.phase(), SlotPhase::Idle, "the payout state ends itself");
    assert_eq!(m.winning_line_word(), -1, "and clears the lit line");
}

/// The caption slides in over its first frames, and the clock that moves it
/// is the machine's own tick. A caption that never advances is the failure
/// this pins: every row would stay clipped above the matrix.
#[test]
fn the_payout_caption_slides_down_one_row_per_tick() {
    use legaia_minigame_formats::minigame_slot_scene as scene;

    let mut m = SlotMachine::new(payouts(), 42, 200);
    win_on(&mut m, 6);
    m.tick();

    // Frame 1 of the caption: 12 rows above the matrix.
    let first = m.marquee_placements()[0].row;
    assert_eq!(first, 1 - scene::PAYOUT_SLIDE_ROWS);
    assert!(first < 0, "the caption starts above the matrix");

    // It descends exactly one row per tick...
    for expected in (first + 1)..=0 {
        m.tick();
        assert_eq!(
            m.marquee_placements()[0].row,
            expected,
            "the caption advances one row per tick"
        );
    }
    // ...and then holds at row 0 rather than running off the bottom.
    for _ in 0..5 {
        m.tick();
        assert_eq!(m.marquee_placements()[0].row, 0, "the caption holds");
    }
}

/// The bonus tally's marquee columns come off the same `claimed` array the
/// payout multiplies, so the strip cannot show a different spin than it paid.
#[test]
fn the_bonus_tally_marquee_reads_the_claimed_reels() {
    use legaia_minigame_formats::minigame_slot_scene as scene;

    let mut m = SlotMachine::new(payouts(), 0x51075, 200);
    win_on(&mut m, slot_payout::KICK_SYMBOL_ID);
    m.collect();

    // Mid-bonus-spin, with the reels stopping: the tally strip is up.
    assert!(m.spin());
    while m.phase() != SlotPhase::Stopping {
        m.tick();
    }
    m.stop_reel(0);
    let p = m.marquee_placements();
    // Three numerals and two multiplication signs, at the retail columns.
    for &col in scene::TALLY_TIMES_COLS.iter() {
        assert!(
            p.iter()
                .any(|q| q.col == col as i32 && q.msg == scene::MSG_TIMES),
            "a multiplication sign belongs at column {col}"
        );
    }
    // Reel 0 is claimed, so its column shows that reel's landed number;
    // the unclaimed reels read "0".
    let claimed = m.claimed(0);
    let want = scene::MSG_NUMBER_BASE + (claimed - 0x10).max(0) as usize;
    let got = p
        .iter()
        .find(|q| q.col == scene::TALLY_NUMBER_COLS[0] as i32)
        .unwrap()
        .msg;
    assert_eq!(got, want, "the tally's first column is reel 0's claim");
    for reel in 1..REEL_COUNT {
        let got = p
            .iter()
            .find(|q| q.col == scene::TALLY_NUMBER_COLS[reel] as i32)
            .unwrap()
            .msg;
        assert_eq!(
            got,
            scene::MSG_NUMBER_BASE,
            "reel {reel} is unclaimed and reads 0"
        );
    }
}

fn ring(m: &mut SlotMachine) -> Vec<i16> {
    m.take_sounds().ring.into_iter().map(|(_, id)| id).collect()
}

/// Triangle opens the picker (`0x20`); Up on row 0 stays on row 0 (the
/// unsigned `% 3`), Down walks and wraps; Circle backs out (`0x37`).
#[test]
fn the_cash_out_picker_moves_and_cancels_like_retail() {
    let mut m = SlotMachine::new(payouts(), 1, 50);
    assert!(!m.cash_out_input(0), "no edge: the machine keeps the pad");
    assert!(m.cash_out_input(menu_pad::OPEN & 0x10));
    assert_eq!(m.screen(), SlotScreen::Picker { row: 0 });
    assert_eq!(ring(&mut m), vec![CUE_MENU_CONFIRM]);
    m.cash_out_input(menu_pad::UP);
    assert_eq!(m.screen(), SlotScreen::Picker { row: 0 });
    assert_eq!(ring(&mut m), vec![CUE_MENU_CURSOR]);
    m.cash_out_input(menu_pad::DOWN);
    m.cash_out_input(menu_pad::DOWN);
    assert_eq!(m.screen(), SlotScreen::Picker { row: 2 });
    m.cash_out_input(menu_pad::DOWN);
    assert_eq!(m.screen(), SlotScreen::Picker { row: 0 });
    ring(&mut m);
    m.cash_out_input(0x20);
    assert_eq!(m.phase(), SlotPhase::Idle);
    assert_eq!(ring(&mut m), vec![CUE_MENU_CANCEL]);
    assert_eq!(m.balance(), 50, "the picker charges nothing");
}

/// The quit row fades out over sixteen frames and only then commits.
#[test]
fn the_quit_row_fades_then_commits() {
    let mut m = SlotMachine::new(payouts(), 1, 50);
    m.cash_out_input(0x100);
    m.cash_out_input(menu_pad::DOWN);
    m.cash_out_input(0x40);
    assert_eq!(m.phase(), SlotPhase::Leaving);
    let mut frames = 0;
    while m.phase() == SlotPhase::Leaving {
        m.cash_out_input(0);
        frames += 1;
    }
    assert_eq!(frames, 16);
    assert_eq!(m.phase(), SlotPhase::CashedOut);
    assert_eq!(m.fade_level(), 0xFF);
}

/// The rules row: fade to black, page 0 (taken once faded in), page 1,
/// fade out, fade back in on the machine, idle.
#[test]
fn the_rules_row_walks_both_pages_and_returns() {
    let mut m = SlotMachine::new(payouts(), 1, 50);
    m.cash_out_input(0x10);
    m.cash_out_input(menu_pad::DOWN);
    m.cash_out_input(menu_pad::DOWN);
    m.cash_out_input(0x04);
    assert_eq!(m.retail_state(), 0x33);
    for _ in 0..40 {
        m.cash_out_input(0);
    }
    assert_eq!(m.screen(), SlotScreen::Instructions { page: 0 });
    ring(&mut m);
    m.cash_out_input(0x40);
    assert_eq!(m.screen(), SlotScreen::Instructions { page: 1 });
    assert_eq!(ring(&mut m), vec![CUE_MENU_CURSOR]);
    m.cash_out_input(0x40);
    assert_eq!(ring(&mut m), vec![CUE_MENU_CANCEL]);
    let mut frames = 0;
    while m.phase() != SlotPhase::Idle {
        m.cash_out_input(0);
        frames += 1;
        assert!(frames < 64);
    }
    assert_eq!(m.fade_level(), 0);
}

/// Under three coins the idle state raises the prompt; any face button
/// leaves through the fade.
#[test]
fn an_empty_machine_raises_the_prompt_and_leaves() {
    let mut m = SlotMachine::new(payouts(), 1, 2);
    assert!(m.cash_out_input(0));
    assert_eq!(m.screen(), SlotScreen::NoCoins);
    m.cash_out_input(0x80);
    assert_eq!(ring(&mut m), vec![CUE_MENU_CONFIRM]);
    assert_eq!(m.phase(), SlotPhase::Leaving);
    // The submenu edge still wins over the gate on an empty machine.
    let mut m = SlotMachine::new(payouts(), 1, 0);
    m.cash_out_input(0x10);
    assert_eq!(m.screen(), SlotScreen::Picker { row: 0 });
}

#[test]
fn packed_edges_swap_the_raw_layout() {
    use legaia_engine_system::pad::PadButton;
    let raw = PadButton::Triangle.mask() | PadButton::Up.mask();
    assert_eq!(packed_edges(raw, 0), 0x1010);
    assert_eq!(packed_edges(raw, raw), 0);
}

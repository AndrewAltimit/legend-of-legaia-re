use super::*;
use legaia_minigame_formats::baka_opponents::BakaActionSet;

#[test]
fn every_dispatcher_case_is_a_distinct_ascending_state() {
    for pair in CABINET_STATES.windows(2) {
        assert!(pair[0] < pair[1], "{pair:?} out of order");
    }
    assert!(is_cabinet_state(ST_DUEL));
    assert!(is_cabinet_state(ST_EXIT));
    // The editor band `FUN_801D4FC8` gates on is exactly two states.
    assert!(is_cabinet_state(0x190) && is_cabinet_state(0x191));
    assert!(!is_cabinet_state(0x192));
    assert!(!is_cabinet_state(3));
}

#[test]
fn secret_gate_is_stage_and_score_keyed_and_strict() {
    assert_eq!(secret_opponent_gate(5, 250_001), SecretOpponent::First);
    // Strict: exactly on the threshold does not qualify.
    assert_eq!(secret_opponent_gate(5, 250_000), SecretOpponent::None);
    assert_eq!(secret_opponent_gate(4, 999_999), SecretOpponent::None);
    assert_eq!(secret_opponent_gate(13, 700_001), SecretOpponent::Second);
    assert_eq!(secret_opponent_gate(13, 700_000), SecretOpponent::None);
}

#[test]
fn a_pending_secret_rung_suppresses_the_stage_advance() {
    assert_eq!(
        advance_stage(5, SecretOpponent::None),
        StageAdvance {
            stage: 6,
            all_clear: false
        }
    );
    assert_eq!(
        advance_stage(5, SecretOpponent::First),
        StageAdvance {
            stage: 5,
            all_clear: false
        }
    );
    // Reaching the wrap raises the all-clear and restarts at 0, so the
    // second lap serves roster ids 3 and 4 from stages 0 and 1.
    assert_eq!(
        advance_stage(13, SecretOpponent::None),
        StageAdvance {
            stage: 0,
            all_clear: true
        }
    );
    assert_eq!(rung_fold(0, SecretOpponent::None), (3, 5));
    assert_eq!(rung_fold(2, SecretOpponent::None), (5, 7));
    assert_eq!(rung_fold(9, SecretOpponent::First), (3, 5));
    assert_eq!(rung_fold(9, SecretOpponent::Second), (4, 6));
}

#[test]
fn the_bonus_row_pays_more_for_a_shutout() {
    assert_eq!(match_bonus(0), PERFECT_BONUS);
    assert_eq!(match_bonus(1), MATCH_BONUS);
    const { assert!(PERFECT_BONUS > MATCH_BONUS) };
}

#[test]
fn cursor_wraps_in_retails_clamp_order() {
    assert_eq!(
        cursor_step(0, 3, CABINET_LEFT, CABINET_LEFT, CABINET_RIGHT).0,
        2
    );
    assert_eq!(
        cursor_step(2, 3, CABINET_RIGHT, CABINET_LEFT, CABINET_RIGHT).0,
        0
    );
    assert_eq!(
        cursor_step(1, 3, 0, CABINET_LEFT, CABINET_RIGHT),
        (1, false)
    );
    // Both edges in one frame cancel out and still count as a move.
    assert_eq!(
        cursor_step(
            1,
            3,
            CABINET_LEFT | CABINET_RIGHT,
            CABINET_LEFT,
            CABINET_RIGHT
        ),
        (1, true)
    );
    // A span of 2 is what retail's `& 1` fold does.
    assert_eq!(
        cursor_step(0, 2, CABINET_LEFT, CABINET_LEFT, CABINET_RIGHT).0,
        1
    );
    assert_eq!(
        cursor_step(1, 2, CABINET_RIGHT, CABINET_LEFT, CABINET_RIGHT).0,
        0
    );
}

#[test]
fn the_modulo_cursors_are_sticky_at_the_top() {
    // Down wraps at the bottom, the same as the clamped fold.
    assert_eq!(
        cursor_step_mod(2, 3, CABINET_DOWN, CABINET_UP, CABINET_DOWN).0,
        0
    );
    assert_eq!(
        cursor_step_mod(4, 5, CABINET_DOWN, CABINET_UP, CABINET_DOWN).0,
        0
    );
    // Up off row 0 does *not* wrap: `-1` as a u32 is divisible by both 3
    // and 5, so it folds back to 0.
    assert_eq!(
        cursor_step_mod(0, 3, CABINET_UP, CABINET_UP, CABINET_DOWN).0,
        0
    );
    assert_eq!(
        cursor_step_mod(0, 5, CABINET_UP, CABINET_UP, CABINET_DOWN).0,
        0
    );
    // Which is exactly where it differs from the player-select fold.
    assert_eq!(cursor_step(0, 3, CABINET_UP, CABINET_UP, CABINET_DOWN).0, 2);
    // Ordinary steps behave.
    assert_eq!(
        cursor_step_mod(1, 5, CABINET_UP, CABINET_UP, CABINET_DOWN).0,
        0
    );
    assert_eq!(
        cursor_step_mod(1, 5, CABINET_DOWN, CABINET_UP, CABINET_DOWN).0,
        2
    );
}

#[test]
fn shake_always_terminates() {
    let mut a = 0x400;
    for _ in 0..200 {
        a = shake_decay(a, 1);
    }
    assert_eq!(a, 0);
    let mut b = -0x400;
    for _ in 0..200 {
        b = shake_decay(b, 1);
    }
    assert_eq!(b, 0);
    // A single step never crosses zero.
    assert_eq!(shake_decay(1, 1), 0);
    assert_eq!(shake_decay(-1, 1), 0);
}

#[test]
fn vital_bar_is_one_pixel_per_32_hp() {
    let full = vital_bar(0, FULL_HP);
    assert_eq!(full.x1 - full.x0, 0x64);
    assert_eq!(full.x1, VITAL_ANCHOR_LEFT);
    assert_eq!(full.rgb_far, [VITAL_RED, 0x64, 0]);
    let empty = vital_bar(0, 0);
    assert_eq!(empty.x0, empty.x1);
    // The opponent fills the other way from its own anchor.
    let opp = vital_bar(1, FULL_HP);
    assert_eq!(opp.x0, VITAL_ANCHOR_RIGHT);
    assert_eq!(opp.x1, VITAL_ANCHOR_RIGHT + 0x64);
}

#[test]
fn vital_frames_tile_from_each_side_anchor() {
    use legaia_minigame_formats::baka_opponents::BakaBarFrameCell;
    let cell = |width, u: u8| BakaBarFrameCell {
        width,
        uv: [(u, 0), (u + 7, 0), (u, 15), (u + 7, 15)],
    };
    let cells = [cell(8, 0x18), cell(100, 0x20), cell(8, 0x28)];
    let left = vital_frame_cells(0, &cells);
    assert_eq!(
        left.iter().map(|c| (c.x0, c.x1)).collect::<Vec<_>>(),
        vec![(0x1C, 0x24), (0x24, 0x88), (0x88, 0x90)]
    );
    assert!(left.iter().all(|c| (c.y0, c.y1) == (0x20, 0x30)));
    // The frames hold the fill bars' full-HP extent and its scanlines.
    let full = vital_bar(0, FULL_HP);
    assert!(left[0].x0 <= full.x0 && full.x1 <= left[2].x1);
    assert!(left.iter().all(|c| c.y0 < full.y0 && full.y1 < c.y1));
    let right = vital_frame_cells(1, &cells);
    assert_eq!((right[0].x0, right[2].x1), (0xB0, 0xB0 + 116));
    let opp = vital_bar(1, FULL_HP);
    assert!(right[0].x0 <= opp.x0 && opp.x1 <= right[2].x1);
}

#[test]
fn pip_rows_grow_toward_each_other() {
    let p = round_win_pips(0, 1, 2);
    assert_eq!(
        p,
        vec![(0x70, PIP_Y, PIP_U_FILLED), (0x80, PIP_Y, PIP_U_EMPTY)]
    );
    let o = round_win_pips(1, 0, 2);
    assert_eq!(
        o,
        vec![(0xC0, PIP_Y, PIP_U_EMPTY), (0xB0, PIP_Y, PIP_U_EMPTY)]
    );
}

#[test]
fn combo_level_dips_then_snaps_back() {
    assert_eq!(combo_counter_level(0), 0xF0);
    assert_eq!(combo_counter_level(3), 0xF0);
    assert_eq!(combo_counter_level(4), 0xE0);
    assert_eq!(combo_counter_level(5), 0xD0);
    assert_eq!(combo_counter_level(6), 0xC0);
    // From 7 the `10 - combo < 4` test fires and it is full again.
    assert_eq!(combo_counter_level(7), 0xF0);
    assert_eq!(combo_counter_level(50), 0xF0);
}

#[test]
fn stage_digits_split_at_ten() {
    assert_eq!(stage_digits(1), vec![(0x48, 0x1E, 1)]);
    assert_eq!(stage_digits(9), vec![(0x48, 0x1E, 9)]);
    assert_eq!(stage_digits(14), vec![(0x40, 0x1E, 1), (0x48, 0x1E, 4)]);
}

#[test]
fn high_score_place_count_carries_retails_off_by_one() {
    assert_eq!(high_score_x(0), None);
    // 9 measures as two places because the test is against `10^k - 1`.
    assert_eq!(high_score_places(9), 2);
    assert_eq!(high_score_places(10), 2);
    assert_eq!(high_score_places(99), 3);
    assert_eq!(high_score_places(1), 1);
    assert_eq!(high_score_x(1), Some(0x28 - 8));
}

#[test]
fn max_streak_latch_never_falls() {
    let mut l = 0;
    for s in [3, 7, 2, 5, 1] {
        l = max_streak_latch(l, s);
    }
    assert_eq!(l, 7);
}

fn run(cab: &mut BakaCabinet, frames: usize, input: &CabinetInput) -> Vec<u32> {
    (0..frames).map(|_| cab.tick(input).state).collect()
}

#[test]
fn boot_walks_to_the_player_select_screen() {
    let mut cab = BakaCabinet::new();
    let idle = CabinetInput {
        frame_step: 1,
        win_target: 2,
        ..Default::default()
    };
    assert_eq!(cab.tick(&idle).state, ST_ATTRACT);
    // The attract card waits for the start edge indefinitely.
    run(&mut cab, 500, &idle);
    assert_eq!(cab.state(), ST_ATTRACT);
    let start = CabinetInput {
        pad_edge: CABINET_START,
        ..idle
    };
    let f = cab.tick(&start);
    assert_eq!(f.state, ST_ATTRACT_OUT);
    assert_eq!(f.cues, vec![crate::baka_fighter::BAKA_CUE_CONFIRM]);
    run(&mut cab, 0x40, &idle);
    assert_eq!(cab.state(), ST_SELECT);
}

#[test]
fn the_front_end_draws_its_prompt_banner_and_seats_the_pick() {
    let mut cab = BakaCabinet::new();
    let idle = CabinetInput {
        frame_step: 1,
        win_target: 2,
        ..Default::default()
    };
    cab.tick(&idle); // boot -> attract
    // The prompt blinks on bit 4 of the post-increment phase, and the
    // card is called with the clock before its increment.
    let levels: Vec<i32> = (0..0x20)
        .map(|_| {
            let f = cab.tick(&idle);
            assert_eq!(f.widgets.len(), 1);
            assert_eq!(f.widgets[0].widget, WIDGET_PRESS_START);
            f.widgets[0].brightness
        })
        .collect();
    assert_eq!(levels[0], 0x80); // phase 1
    assert_eq!(levels[15], 0x40); // phase 16
    assert_eq!(cab.intro_clock(), 0x20);
    let start = CabinetInput {
        pad_edge: CABINET_START,
        ..idle
    };
    assert_eq!(cab.tick(&start).title_card, Some(0x20));
    // The fade-out flashes the prompt at 0x100 for 0x1E steps, then
    // stops calling the card; its clock no longer advances.
    let flashed = (0..0x3D)
        .map(|_| cab.tick(&idle))
        .take_while(|f| f.title_card.is_some())
        .inspect(|f| assert_eq!(f.widgets[0].brightness, 0x100))
        .count();
    assert_eq!(flashed, 0x1E);
    assert_eq!(cab.intro_clock(), 0x21);
    while cab.state() != ST_SELECT {
        cab.tick(&idle);
    }
    let right = CabinetInput {
        pad_edge: CABINET_RIGHT,
        ..idle
    };
    let f = cab.tick(&right);
    assert!(
        f.widgets
            .iter()
            .any(|c| c.widget == WIDGET_PLAYER_SELECT && (c.x, c.y) == PLAYER_SELECT_PEN)
    );
    assert_eq!(cab.select_cursor(), 1);
    let confirm = CabinetInput {
        pad_edge: CABINET_CONFIRM,
        ..idle
    };
    cab.tick(&confirm);
    let seated = (0..0x40)
        .find_map(|_| cab.tick(&idle).install_player)
        .expect("the pose state seats the pick");
    assert_eq!(seated, 1);
    assert!(!cab.front_end());
}

#[test]
fn a_won_match_banks_the_prize_and_a_lost_one_forfeits_the_pot() {
    let mut cab = BakaCabinet::new();
    cab.enter_duel();
    let won = CabinetInput {
        frame_step: 1,
        player_round_wins: 2,
        round_clock: true,
        win_target: 2,
        rung_prize: 40,
        ..Default::default()
    };
    // The round bracket has to elapse before the win is read.
    for _ in 0..0xB6 {
        cab.tick(&won);
    }
    // The win arm records the prize for the tally; the pot itself is
    // the tally drain's to fill, never this arm's.
    assert_eq!(cab.prize, 40);
    assert_eq!(cab.pot(), 0);
    assert_eq!(cab.stage(), 3);
    assert_eq!(cab.state(), ST_PERFECT);
    assert_eq!(cab.rounds_dropped(), 0, "a shutout drops no rounds");

    // A match won 2-1 still reaches the flourish - the `state = 0x65`
    // store is in the delay slot of the shutout branch, so only the
    // dropped-round tally is gated.
    let mut messy = BakaCabinet::new();
    messy.enter_duel();
    let scrappy = CabinetInput {
        opponent_round_wins: 1,
        round_clock: true,
        ..won
    };
    for _ in 0..0xB6 {
        messy.tick(&scrappy);
    }
    assert_eq!(messy.state(), ST_PERFECT);
    assert_eq!(messy.rounds_dropped(), 1);

    // Now lose one with a pot on the table (the host mirrors the live
    // accumulator in, as the tally would have filled it).
    cab.enter_duel();
    cab.set_pot(40);
    let lost = CabinetInput {
        frame_step: 1,
        opponent_round_wins: 2,
        round_clock: true,
        win_target: 2,
        ..Default::default()
    };
    for _ in 0..0xB6 {
        cab.tick(&lost);
    }
    assert_eq!(cab.state(), ST_LOSE);
    let mut forfeit = None;
    for _ in 0..0x200 {
        let f = cab.tick(&lost);
        if f.forfeit.is_some() {
            forfeit = f.forfeit;
        }
    }
    assert_eq!(forfeit, Some(40));
    assert_eq!(cab.pot(), 0);
    assert_eq!(cab.state(), ST_EXIT);
}

#[test]
fn the_choice_sheet_lights_the_picked_side_and_swaps_the_arrows() {
    let s = choice_sheet(0, 0);
    assert_eq!(
        s[0],
        SheetCell {
            widget: WIDGET_ARROW_LIT,
            x: 0x94,
            y: 0x20,
            brightness: 0xA0
        }
    );
    assert_eq!(s[1].widget, WIDGET_ARROW_DIM);
    assert_eq!((s[2].widget, s[2].brightness), (WIDGET_NEXT_GAME, 0xA0));
    assert_eq!((s[3].widget, s[3].brightness), (WIDGET_PAY_OUT, 0x40));
    assert_eq!((s[4].x, s[4].y), (0x70, 0xCC));
    let s = choice_sheet(1, 0x10);
    assert_eq!(
        s[1],
        SheetCell {
            widget: WIDGET_ARROW_LIT,
            x: 0xAC,
            y: 0x20,
            brightness: 0x80
        }
    );
    assert_eq!((s[2].brightness, s[3].brightness), (0x40, 0xA0));

    let mut cab = BakaCabinet::new();
    assert!(cab.choice_sheet().is_none());
    cab.state = ST_CHOICE;
    assert!(cab.choice_sheet().is_some());
    cab.state_timer = 1;
    assert!(
        cab.choice_sheet().is_none(),
        "NEXT GAME taken: the sheet is gone"
    );
}

#[test]
fn pay_out_banks_the_pot_and_next_game_keeps_it_at_risk() {
    let mut cab = BakaCabinet::new();
    cab.state = ST_CHOICE;
    cab.pot = 120;
    let idle = CabinetInput {
        frame_step: 1,
        win_target: 2,
        ..Default::default()
    };
    // Cursor right onto PAY OUT, then confirm.
    cab.tick(&CabinetInput {
        pad_edge: CABINET_RIGHT,
        ..idle
    });
    assert_eq!(cab.menu_cursor, 1);
    cab.tick(&CabinetInput {
        pad_edge: CABINET_CONFIRM,
        ..idle
    });
    assert_eq!(cab.state(), ST_EXIT);
    let mut payout = None;
    for _ in 0..0x80 {
        if let Some(p) = cab.tick(&idle).payout {
            payout = Some(p);
        }
    }
    assert_eq!(payout, Some(120));

    // NEXT GAME instead: the pot survives into the following rung.
    let mut cab = BakaCabinet::new();
    cab.state = ST_CHOICE;
    cab.pot = 120;
    cab.tick(&CabinetInput {
        pad_edge: CABINET_CONFIRM,
        ..idle
    });
    run(&mut cab, 0xD4, &idle);
    assert_eq!(cab.pot(), 120);
    assert_eq!(cab.state(), ST_ROUND_SETUP);
}

#[test]
fn the_secret_rung_is_served_instead_of_advancing_the_stage() {
    let mut cab = BakaCabinet::new();
    cab.stage = SECRET_A_STAGE;
    cab.set_high_score(SECRET_A_SCORE + 1);
    cab.enter_duel();
    let won = CabinetInput {
        frame_step: 1,
        player_round_wins: 2,
        round_clock: true,
        opponent_round_wins: 1,
        win_target: 2,
        rung_prize: 10,
        ..Default::default()
    };
    for _ in 0..0xB6 {
        cab.tick(&won);
    }
    assert_eq!(cab.secret_opponent(), SecretOpponent::First);
    assert_eq!(cab.stage(), SECRET_A_STAGE, "stage must not advance");
    assert_eq!(rung_fold(cab.stage(), cab.secret_opponent()), (3, 5));
}

#[test]
fn the_duel_menu_edge_routes_to_the_dev_menu_only_when_enabled() {
    let mut cab = BakaCabinet::new();
    cab.enter_duel();
    let menu = CabinetInput {
        frame_step: 1,
        pad_edge: CABINET_DUEL_MENU,
        win_target: 2,
        ..Default::default()
    };
    cab.tick(&menu);
    assert_eq!(cab.state(), ST_PAUSE_3);

    let mut cab = BakaCabinet::new();
    cab.enter_duel();
    cab.tick(&CabinetInput {
        dev_menu_enabled: true,
        ..menu
    });
    assert_eq!(cab.state(), ST_DEV_MENU);
}

#[test]
fn the_editor_state_dumps_the_installed_action_tables() {
    let tables = vec![BakaActionSet {
        index: 0,
        power: [1, 2, 3, 4, 5, 6, 7, 8, 9],
        keyframes: [0, 0, 1, 2, 3, 4, 0, 0, 0],
        speed: [0; 9],
        sub_keyframes: Default::default(),
    }];
    let mut cab = BakaCabinet::new().with_action_tables(tables);
    cab.state = ST_DEV_EDITOR_RUN;
    let idle = CabinetInput {
        frame_step: 1,
        ..Default::default()
    };
    assert!(cab.tick(&idle).action_dump.is_none());
    let dump = cab
        .tick(&CabinetInput {
            pad_edge: CABINET_DEV_DUMP,
            ..idle
        })
        .action_dump
        .expect("L1 dumps the table");
    assert!(dump.contains("ACT_TBL 0"));
    assert_eq!(dump.lines().filter(|l| l.contains("act ")).count(), 9);
    // Select leaves the editor.
    cab.tick(&CabinetInput {
        pad_edge: CABINET_DEV_EXIT,
        ..idle
    });
    assert_eq!(cab.state(), ST_SELECT_POSE);
}

#[test]
fn the_hud_pass_runs_only_in_the_duel_band() {
    let mut cab = BakaCabinet::new();
    cab.enter_duel();
    let input = CabinetInput {
        frame_step: 1,
        hp: [FULL_HP, FULL_HP / 2],
        combo_taken: [1, 4],
        win_target: 2,
        ..Default::default()
    };
    let f = cab.tick(&input);
    assert!(f.draw_hud && f.draw_arena);
    let hud = f.hud.expect("the duel draws the HUD");
    assert_eq!(hud.bars[0].x1 - hud.bars[0].x0, 0x64);
    // Crossed sides: index 0 shows slot 1's streak.
    assert_eq!(hud.combo_level[0], combo_counter_level(4));
    assert_eq!(hud.max_streak, 4);

    // The attract card draws neither.
    let mut cab = BakaCabinet::new();
    let f = cab.tick(&input);
    assert!(f.hud.is_none());
    assert!(!f.draw_hud && !f.draw_arena);
}

#[test]
fn the_epilogue_draw_gates_are_narrower_than_the_state_list() {
    // The arena pass skips the attract pair, the tally, three all-clear
    // beats and the exit; the HUD pass is the round / duel / result band
    // only, and it excludes the tally screen.
    for s in [ST_BOOT, ST_ATTRACT, ST_ATTRACT_OUT, ST_TALLY, ST_EXIT] {
        assert!(!draws_arena(s), "{s:#x} should not draw the arena");
    }
    for s in [ST_CLEAR_B, ST_CLEAR_D, ST_CLEAR_E] {
        assert!(!draws_arena(s), "{s:#x} should not draw the arena");
    }
    for s in [ST_DUEL, ST_CHOICE, ST_DEV_MENU, ST_CLEAR_A, ST_CLEAR_C] {
        assert!(draws_arena(s), "{s:#x} should draw the arena");
    }
    assert!(!draws_arena(0x1234), "an unlisted state draws nothing");
    assert!(draws_hud(ST_DUEL) && draws_hud(ST_GAME_OVER));
    assert!(!draws_hud(ST_TALLY) && !draws_hud(ST_CHOICE));
    // Every HUD state also draws the arena.
    for s in CABINET_STATES {
        if draws_hud(s) {
            assert!(draws_arena(s), "{s:#x} draws a HUD over no arena");
        }
    }
}

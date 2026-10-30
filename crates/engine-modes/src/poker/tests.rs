use super::*;
use crate::session::AiTier;

fn c(s: &str) -> Card {
    Card::parse(s).unwrap()
}

/// A deck that deals `holes[seat]` to each seat (dealing starts at `sb`)
/// and `board` as flop / turn / river.
#[allow(clippy::needless_range_loop)]
fn deck(holes: &[[&str; 2]], sb: usize, board: [&str; 5]) -> Vec<Card> {
    let n = holes.len();
    let mut out = Vec::new();
    for round in 0..2 {
        for k in 0..n {
            out.push(c(holes[(sb + k) % n][round]));
        }
    }
    out.extend(board.iter().map(|s| c(s)));
    for card in Card::deck() {
        if !out.contains(&card) {
            out.push(card);
        }
    }
    assert_eq!(out.len(), 52);
    out
}

fn humans(n: usize) -> PokerConfig {
    PokerConfig {
        seats: vec![SeatKind::Human; n],
        ..PokerConfig::default()
    }
}

fn total_chips(s: &PokerSession) -> u32 {
    (0..s.seats.len()).map(|i| s.chips(i)).sum::<u32>() + s.pot()
}

#[test]
fn heads_up_button_posts_small_blind_and_acts_first() {
    let s = PokerSession::new(humans(2));
    assert_eq!(s.button, 0);
    assert_eq!(s.seats[0].street_bet, 10);
    assert_eq!(s.seats[1].street_bet, 20);
    assert_eq!(s.to_act(), Some(0));
}

#[test]
fn three_handed_blinds_and_utg() {
    let s = PokerSession::new(humans(3));
    // Button 0, small blind 1, big blind 2, the button is first to act.
    assert_eq!(s.seats[1].street_bet, 10);
    assert_eq!(s.seats[2].street_bet, 20);
    assert_eq!(s.to_act(), Some(0));
}

#[test]
fn everyone_folding_hands_the_pot_over() {
    let mut s = PokerSession::new(humans(3));
    s.act(0, Action::Fold).unwrap();
    let ev = s.act(1, Action::Fold).unwrap();
    assert!(ev.iter().any(|e| matches!(
        e,
        PokerEvent::PotAwarded { winners, amount: 30, hand: None, .. } if winners == &vec![2]
    )));
    assert_eq!(s.chips(2), 1010);
    assert_eq!(s.chips(1), 990);
    assert!(matches!(s.phase(), Phase::Showdown { .. }));
}

#[test]
fn big_blind_keeps_its_option() {
    let mut s = PokerSession::new(humans(3));
    s.act(0, Action::Call).unwrap();
    s.act(1, Action::Call).unwrap();
    // Everyone has matched 20, but the big blind still acts.
    assert_eq!(s.to_act(), Some(2));
    assert_eq!(s.act(2, Action::Call), Err(ActionError::NothingToCall));
    s.act(2, Action::RaiseTo(60)).unwrap();
    assert_eq!(s.to_act(), Some(0));
}

#[test]
fn raises_must_be_at_least_the_last_full_raise() {
    let mut s = PokerSession::new(humans(3));
    assert_eq!(
        s.act(0, Action::RaiseTo(30)),
        Err(ActionError::RaiseTooSmall)
    );
    s.act(0, Action::RaiseTo(60)).unwrap(); // a raise of 40
    assert_eq!(s.min_raise_to(), 100);
    assert_eq!(
        s.act(1, Action::RaiseTo(90)),
        Err(ActionError::RaiseTooSmall)
    );
    assert_eq!(
        s.act(1, Action::RaiseTo(5000)),
        Err(ActionError::RaiseTooLarge)
    );
    s.act(1, Action::RaiseTo(100)).unwrap();
    assert_eq!(s.act(2, Action::Check), Err(ActionError::CannotCheck));
    assert_eq!(s.act(0, Action::Fold), Err(ActionError::NotYourTurn));
}

#[test]
fn short_all_in_does_not_reopen_raising_for_seats_that_acted() {
    let mut s = PokerSession::new(humans(3));
    s.set_chips(2, 50); // the big blind has 50 behind its posted 20
    s.act(0, Action::RaiseTo(60)).unwrap(); // full raise of 40
    s.act(1, Action::Call).unwrap();
    // Big blind shoves to 70: a raise of 10, short of 40.
    s.act(2, Action::AllIn).unwrap();
    assert_eq!(s.current_bet, 70);
    assert_eq!(s.to_act(), Some(0));
    // Seat 0 already acted: it may call or fold, not raise.
    assert_eq!(
        s.act(0, Action::RaiseTo(200)),
        Err(ActionError::RaiseClosed)
    );
    s.act(0, Action::Call).unwrap();
    assert_eq!(
        s.act(1, Action::RaiseTo(200)),
        Err(ActionError::RaiseClosed)
    );
    s.act(1, Action::Call).unwrap();
    assert_eq!(s.phase(), Phase::Betting(Street::Flop));
}

#[test]
fn check_down_best_hand_wins() {
    let d = deck(
        &[["As", "Ad"], ["Kc", "Kd"]],
        0,
        ["2c", "7h", "9s", "Jd", "3c"],
    );
    let mut s = PokerSession::with_decks(humans(2), vec![d]);
    s.act(0, Action::Call).unwrap();
    s.act(1, Action::Check).unwrap();
    for _ in 0..3 {
        // Postflop the big blind (seat 1) acts first heads-up.
        assert_eq!(s.to_act(), Some(1));
        s.act(1, Action::Check).unwrap();
        s.act(0, Action::Check).unwrap();
    }
    assert!(matches!(s.phase(), Phase::Showdown { .. }));
    assert_eq!(s.chips(0), 1020);
    assert_eq!(s.chips(1), 980);
    assert_eq!(s.board().len(), 5);
}

#[test]
fn split_pot_odd_chip_goes_left_of_the_button() {
    // Both play the board's broadway straight.
    let d = deck(
        &[["2c", "3d"], ["2h", "3s"], ["4c", "5d"]],
        1,
        ["Ah", "Kd", "Qs", "Jc", "Th"],
    );
    let mut cfg = humans(3);
    cfg.small_blind = 5;
    let mut s = PokerSession::with_decks(cfg, vec![d]);
    // Button 0 calls, small blind 1 folds (5 dead), big blind checks.
    s.act(0, Action::Call).unwrap();
    s.act(1, Action::Fold).unwrap();
    s.act(2, Action::Check).unwrap();
    for _ in 0..3 {
        s.act(2, Action::Check).unwrap();
        s.act(0, Action::Check).unwrap();
    }
    // Pot 45 split two ways: 22 each and the odd chip to seat 2, the first
    // winner left of the button.
    assert_eq!(s.chips(0), 1000 - 20 + 22);
    assert_eq!(s.chips(2), 1000 - 20 + 23);
    assert_eq!(s.chips(1), 995);
}

#[test]
fn side_pots_pay_the_short_stack_only_the_main_pot() {
    // Seat 0 (button) has 100, seat 1 (sb) 300, seat 2 (bb) 1000.
    // Seat 0 holds the best hand, seat 1 the second best.
    let d = deck(
        &[["As", "Ad"], ["Ks", "Kd"], ["7c", "2d"]],
        1,
        ["3c", "8h", "9s", "Jd", "4h"],
    );
    let mut cfg = humans(3);
    cfg.buy_in = 1000;
    let mut s = PokerSession::with_decks(cfg, vec![d]);
    s.set_chips(0, 100);
    s.set_chips(1, 290); // 300 less the posted small blind
    s.act(0, Action::AllIn).unwrap();
    s.act(1, Action::AllIn).unwrap();
    s.act(2, Action::Call).unwrap();
    // Board runs out; seat 0 wins main (300), seat 1 wins side (400).
    assert!(matches!(s.phase(), Phase::Showdown { .. }));
    assert_eq!(s.chips(0), 300);
    assert_eq!(s.chips(1), 400);
    assert_eq!(s.chips(2), 1000 - 20 - 280);
}

#[test]
fn busting_everyone_finishes_with_ranked_results() {
    let d = deck(
        &[["As", "Ad"], ["Kc", "Kd"]],
        0,
        ["2c", "7h", "9s", "Jd", "3c"],
    );
    let mut s = PokerSession::with_decks(humans(2), vec![d]);
    s.act(0, Action::AllIn).unwrap();
    let ev = s.act(1, Action::Call).unwrap();
    assert!(ev.contains(&PokerEvent::SeatBusted { seat: 1 }));
    assert!(s.is_finished());
    let r = s.results().unwrap();
    assert_eq!(r.placements[0].seat, 0);
    assert_eq!(r.placements[1].seat, 1);
    let net: i64 = r
        .rewards
        .iter()
        .map(|r| match r {
            Reward::Currency { amount, .. } => *amount,
            _ => 0,
        })
        .sum();
    assert_eq!(net, 0);
}

#[test]
fn pad_menu_commits_an_action() {
    let mut s = PokerSession::new(humans(2));
    let cross = PadButton::Cross.mask();
    let right = PadButton::Right.mask();
    let up = PadButton::Up.mask();
    // Cursor starts on Check/Call; Right to Raise, Up adds a big blind.
    s.tick(&[right]);
    s.tick(&[0]);
    s.tick(&[up]);
    s.tick(&[0]);
    assert_eq!(s.view().menus[0].raise_to, 60);
    let ev = s.tick(&[cross]);
    assert!(ev.contains(&PokerEvent::Acted {
        seat: 0,
        action: Action::RaiseTo(60)
    }));
    assert_eq!(s.to_act(), Some(1));
}

#[test]
fn ai_table_conserves_chips_and_finishes() {
    for seed in 0..6 {
        let cfg = PokerConfig {
            seats: vec![
                SeatKind::Ai(AiTier::Novice),
                SeatKind::Ai(AiTier::Regular),
                SeatKind::Ai(AiTier::Expert),
                SeatKind::Ai(AiTier::Regular),
            ],
            buy_in: 300,
            ai_think_frames: 0,
            showdown_frames: 0,
            blind_double_every: Some(10),
            seed,
            ..PokerConfig::default()
        };
        let mut s = PokerSession::new(cfg);
        let mut frames = 0;
        while !s.is_finished() {
            s.tick(&[]);
            assert_eq!(total_chips(&s), 1200, "seed {seed} frame {frames}");
            frames += 1;
            assert!(frames < 200_000, "seed {seed} never finished");
        }
        let r = s.results().unwrap();
        assert_eq!(r.placements.len(), 4);
        assert_eq!(r.placements[0].score, 1200);
    }
}

#[test]
fn from_table_maps_the_pack_instance() {
    let t = legaia_expansion_format::modes::PokerTable {
        seats: 9,
        currency: legaia_expansion_format::modes::Currency::Coins,
        chip_value: 5,
        ..Default::default()
    };
    let cfg = PokerConfig::from_table(&t, 7);
    assert_eq!(cfg.seats.len(), MAX_TABLE_SEATS);
    assert_eq!(cfg.seats[0], SeatKind::Human);
    assert_eq!(cfg.currency, Currency::Coins);
    assert_eq!(cfg.chip_value, 5);
}

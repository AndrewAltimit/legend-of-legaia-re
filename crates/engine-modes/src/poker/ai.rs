//! Poker AI seats.
//!
//! Every tier reduces the spot to a win equity in per-mille and compares it
//! with the price of continuing. [`AiTier::Novice`] guesses that equity
//! from its hand category alone and calls far too often;
//! [`AiTier::Regular`] and [`AiTier::Expert`] sample it by Monte Carlo
//! against random opponent holdings (more samples and less noise for the
//! expert, plus the occasional bluff). Every draw comes from the session's
//! RNG, so an AI decision is as deterministic as a human's pad word.

use super::Action;
use super::hand::{Card, Category, best_hand};
use crate::session::{AiTier, ModeRng};

/// What an AI seat sees when it is its turn.
#[derive(Debug, Clone)]
pub struct AiSpot<'a> {
    pub hole: [Card; 2],
    pub board: &'a [Card],
    pub to_call: u32,
    pub pot: u32,
    pub chips: u32,
    pub current_bet: u32,
    pub street_bet: u32,
    pub min_raise_to: u32,
    pub can_raise: bool,
    pub opponents: usize,
    pub big_blind: u32,
}

/// Win equity in per-mille against `opponents` random hands, from
/// `samples` deals (ties count half).
pub fn sample_equity(spot: &AiSpot, samples: u32, rng: &mut ModeRng) -> u32 {
    let mut known = vec![spot.hole[0], spot.hole[1]];
    known.extend_from_slice(spot.board);
    let rest: Vec<Card> = Card::deck()
        .into_iter()
        .filter(|c| !known.contains(c))
        .collect();
    let need_board = 5 - spot.board.len();
    let opp = spot.opponents.max(1);
    let mut score = 0u32;
    let mut deck = rest.clone();
    for _ in 0..samples {
        // Partial shuffle: draw only what this sample needs.
        let draw = need_board + opp * 2;
        for i in 0..draw {
            let j = i + rng.below((deck.len() - i) as u32) as usize;
            deck.swap(i, j);
        }
        let mut board: Vec<Card> = spot.board.to_vec();
        board.extend_from_slice(&deck[..need_board]);
        let mut mine = board.clone();
        mine.extend_from_slice(&spot.hole);
        let me = best_hand(&mine);
        let mut best_opp = super::hand::HandValue(0);
        for o in 0..opp {
            let k = need_board + o * 2;
            let mut theirs = board.clone();
            theirs.extend_from_slice(&deck[k..k + 2]);
            best_opp = best_opp.max(best_hand(&theirs));
        }
        score += match me.cmp(&best_opp) {
            std::cmp::Ordering::Greater => 2,
            std::cmp::Ordering::Equal => 1,
            std::cmp::Ordering::Less => 0,
        };
    }
    score * 500 / samples.max(1)
}

/// A rough equity from the visible hand alone: preflop from high card,
/// pair and suitedness, postflop from the made category.
fn guess_equity(spot: &AiSpot) -> u32 {
    let (a, b) = (spot.hole[0], spot.hole[1]);
    if spot.board.is_empty() {
        let hi = a.rank().max(b.rank()) as u32;
        let mut e = 250 + hi * 20;
        if a.rank() == b.rank() {
            e += 200;
        }
        if a.suit() == b.suit() {
            e += 40;
        }
        return e.min(900);
    }
    let mut seen = spot.board.to_vec();
    seen.extend_from_slice(&spot.hole);
    match best_hand(&seen).category() {
        Category::HighCard => 250,
        Category::Pair => 500,
        Category::TwoPair => 680,
        Category::Trips => 780,
        _ => 900,
    }
}

/// Choose an action.
pub fn decide(spot: &AiSpot, tier: AiTier, rng: &mut ModeRng) -> Action {
    let (samples, noise, bluff_pct) = match tier {
        AiTier::Novice => (0, 150, 0),
        AiTier::Regular => (60, 80, 3),
        AiTier::Expert => (200, 30, 8),
    };
    let raw = if samples == 0 {
        guess_equity(spot)
    } else {
        sample_equity(spot, samples, rng)
    };
    let eq = (raw as i32 + rng.range(-noise, noise)).clamp(0, 1000) as u32;
    // The equity a seat needs for a fair share of the pot.
    let fair = 1000 / (spot.opponents as u32 + 1);
    let strength = eq * 100 / fair.max(1);
    let pot_odds = spot.to_call * 1000 / (spot.pot + spot.to_call).max(1);

    let raise_size = |frac4: u32| -> Action {
        let target = spot.current_bet + (spot.pot * frac4 / 4).max(spot.big_blind);
        let target = target.max(spot.min_raise_to);
        let max_to = spot.street_bet + spot.chips;
        if target >= max_to {
            Action::AllIn
        } else {
            Action::RaiseTo(target)
        }
    };

    if spot.to_call == 0 {
        let bluff = !spot.board.is_empty() && rng.chance(bluff_pct);
        if spot.can_raise && (strength > 140 || bluff) {
            return raise_size(if strength > 200 { 4 } else { 2 });
        }
        return Action::Check;
    }
    let margin: i32 = match tier {
        AiTier::Novice => -150,
        AiTier::Regular => 30,
        AiTier::Expert => 50,
    };
    if spot.can_raise && strength > 165 && spot.chips > spot.to_call {
        return raise_size(3);
    }
    if eq as i32 > pot_odds as i32 + margin {
        return Action::Call;
    }
    Action::Fold
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spot<'a>(hole: [&str; 2], board: &'a [Card]) -> AiSpot<'a> {
        AiSpot {
            hole: [Card::parse(hole[0]).unwrap(), Card::parse(hole[1]).unwrap()],
            board,
            to_call: 20,
            pot: 30,
            chips: 1000,
            current_bet: 20,
            street_bet: 0,
            min_raise_to: 40,
            can_raise: true,
            opponents: 1,
            big_blind: 20,
        }
    }

    #[test]
    fn aces_are_a_big_favourite_and_raise() {
        let s = spot(["As", "Ad"], &[]);
        let mut r = ModeRng::new(9);
        let eq = sample_equity(&s, 400, &mut r);
        assert!((780..=900).contains(&eq), "AA heads-up equity {eq}");
        assert!(matches!(
            decide(&s, AiTier::Expert, &mut r),
            Action::RaiseTo(_) | Action::AllIn
        ));
    }

    #[test]
    fn expert_folds_air_to_a_big_bet() {
        let board: Vec<Card> = ["Kc", "Qd", "9h", "4s", "3c"]
            .iter()
            .map(|c| Card::parse(c).unwrap())
            .collect();
        let mut s = spot(["7d", "2h"], &board);
        s.to_call = 500;
        s.pot = 600;
        let mut r = ModeRng::new(3);
        assert_eq!(decide(&s, AiTier::Expert, &mut r), Action::Fold);
    }
}

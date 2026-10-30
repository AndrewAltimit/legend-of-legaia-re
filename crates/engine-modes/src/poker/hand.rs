//! Playing cards and the Hold'em hand evaluator.
//!
//! [`HandValue`] packs a hand's category and its five tie-break ranks into
//! one `u32` so two hands compare with `<`: category in bits 20..24, then
//! each rank (2..=14) in a 4-bit nibble, most significant first. The
//! evaluator scores every 5-card subset of up to 7 cards and keeps the
//! best.

use serde::{Deserialize, Serialize};

/// One card of the 52: `rank_index * 4 + suit`, rank index `0` = deuce
/// through `12` = ace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Card(pub u8);

impl Card {
    /// `rank` is `2..=14` (ace high), `suit` `0..4`.
    pub fn new(rank: u8, suit: u8) -> Self {
        debug_assert!((2..=14).contains(&rank) && suit < 4);
        Self((rank - 2) * 4 + suit)
    }

    /// `2..=14`.
    pub fn rank(self) -> u8 {
        self.0 / 4 + 2
    }

    /// `0..4`.
    pub fn suit(self) -> u8 {
        self.0 % 4
    }

    /// Parse `"As"`, `"Td"`, `"7c"`, `"2h"`.
    pub fn parse(s: &str) -> Option<Self> {
        let b = s.as_bytes();
        if b.len() != 2 {
            return None;
        }
        let rank = match b[0] {
            b'2'..=b'9' => b[0] - b'0',
            b'T' => 10,
            b'J' => 11,
            b'Q' => 12,
            b'K' => 13,
            b'A' => 14,
            _ => return None,
        };
        let suit = match b[1] {
            b'c' => 0,
            b'd' => 1,
            b'h' => 2,
            b's' => 3,
            _ => return None,
        };
        Some(Self::new(rank, suit))
    }

    /// The full 52-card deck in order.
    pub fn deck() -> Vec<Card> {
        (0..52).map(Card).collect()
    }
}

/// Hand categories, weakest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Category {
    HighCard,
    Pair,
    TwoPair,
    Trips,
    Straight,
    Flush,
    FullHouse,
    Quads,
    StraightFlush,
}

impl Category {
    fn from_index(i: u32) -> Self {
        [
            Self::HighCard,
            Self::Pair,
            Self::TwoPair,
            Self::Trips,
            Self::Straight,
            Self::Flush,
            Self::FullHouse,
            Self::Quads,
            Self::StraightFlush,
        ][i as usize]
    }
}

/// A comparable hand strength (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct HandValue(pub u32);

impl HandValue {
    fn pack(cat: Category, ranks: &[u8]) -> Self {
        let mut v = (cat as u32) << 20;
        for (i, &r) in ranks.iter().take(5).enumerate() {
            v |= (r as u32) << (16 - 4 * i);
        }
        Self(v)
    }

    pub fn category(self) -> Category {
        Category::from_index(self.0 >> 20)
    }
}

/// Score exactly five cards.
pub fn eval5(cards: [Card; 5]) -> HandValue {
    let mut counts = [0u8; 15];
    for c in cards {
        counts[c.rank() as usize] += 1;
    }
    let flush = cards.iter().all(|c| c.suit() == cards[0].suit());
    // Straight high card, with the wheel (A-2-3-4-5) playing five-high.
    let distinct = counts.iter().filter(|&&n| n > 0).count();
    let mut straight_high = None;
    if distinct == 5 {
        let hi = (2..=14).rev().find(|&r| counts[r] > 0).unwrap() as u8;
        let lo = (2..=14).find(|&r| counts[r] > 0).unwrap() as u8;
        if hi - lo == 4 {
            straight_high = Some(hi);
        } else if counts[14] == 1 && counts[2..=5].iter().all(|&n| n == 1) {
            straight_high = Some(5);
        }
    }
    if let Some(h) = straight_high {
        let cat = if flush {
            Category::StraightFlush
        } else {
            Category::Straight
        };
        return HandValue::pack(cat, &[h]);
    }
    // Ranks grouped by (count desc, rank desc).
    let mut groups: Vec<(u8, u8)> = (2..=14u8)
        .filter(|&r| counts[r as usize] > 0)
        .map(|r| (counts[r as usize], r))
        .collect();
    groups.sort_by(|a, b| b.cmp(a));
    let ranks: Vec<u8> = groups.iter().map(|g| g.1).collect();
    let cat = match (groups[0].0, groups.get(1).map(|g| g.0)) {
        (4, _) => Category::Quads,
        (3, Some(2)) => Category::FullHouse,
        _ if flush => Category::Flush,
        (3, _) => Category::Trips,
        (2, Some(2)) => Category::TwoPair,
        (2, _) => Category::Pair,
        _ => Category::HighCard,
    };
    if cat == Category::Flush {
        let mut all: Vec<u8> = cards.iter().map(|c| c.rank()).collect();
        all.sort_unstable_by(|a, b| b.cmp(a));
        return HandValue::pack(cat, &all);
    }
    HandValue::pack(cat, &ranks)
}

/// The best five-card hand among `cards` (`5..=7` of them).
pub fn best_hand(cards: &[Card]) -> HandValue {
    let n = cards.len();
    assert!((5..=7).contains(&n), "best_hand takes 5..=7 cards");
    let mut best = HandValue(0);
    // Every 5-subset = drop (n - 5) cards.
    for skip_a in 0..n {
        for skip_b in skip_a..n {
            if n == 5 && (skip_a, skip_b) != (0, 0) {
                continue;
            }
            if n == 6 && skip_a != skip_b {
                continue;
            }
            if n == 7 && skip_a == skip_b {
                continue;
            }
            let mut five = [Card(0); 5];
            let mut k = 0;
            for (i, &c) in cards.iter().enumerate() {
                let dropped = match n {
                    5 => false,
                    6 => i == skip_a,
                    _ => i == skip_a || i == skip_b,
                };
                if !dropped {
                    five[k] = c;
                    k += 1;
                }
            }
            best = best.max(eval5(five));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cards(s: &str) -> Vec<Card> {
        s.split_whitespace()
            .map(|c| Card::parse(c).unwrap())
            .collect()
    }

    fn v(s: &str) -> HandValue {
        best_hand(&cards(s))
    }

    #[test]
    fn categories_are_recognised() {
        assert_eq!(v("As Ks Qs Js Ts").category(), Category::StraightFlush);
        assert_eq!(v("9c 9d 9h 9s 2c").category(), Category::Quads);
        assert_eq!(v("9c 9d 9h 2s 2c").category(), Category::FullHouse);
        assert_eq!(v("2h 7h 9h Jh Kh").category(), Category::Flush);
        assert_eq!(v("5c 6d 7h 8s 9c").category(), Category::Straight);
        assert_eq!(v("5c 5d 5h 8s 9c").category(), Category::Trips);
        assert_eq!(v("5c 5d 8h 8s 9c").category(), Category::TwoPair);
        assert_eq!(v("5c 5d 7h 8s 9c").category(), Category::Pair);
        assert_eq!(v("2c 5d 7h 8s 9c").category(), Category::HighCard);
    }

    #[test]
    fn category_order_beats_ranks() {
        let order = [
            "2c 3d 4h 5s 7c",
            "2c 2d 4h 5s 7c",
            "2c 2d 4h 4s 7c",
            "2c 2d 2h 5s 7c",
            "2c 3d 4h 5s 6c",
            "2c 3c 4c 5c 7c",
            "2c 2d 2h 3s 3c",
            "2c 2d 2h 2s 3c",
            "2c 3c 4c 5c 6c",
        ];
        for w in order.windows(2) {
            assert!(v(w[0]) < v(w[1]), "{} < {}", w[0], w[1]);
        }
        // A weak category still beats the best hand of the one below.
        assert!(v("2c 2d 3h 4s 5c") > v("As Kd Qh Js 9c"));
    }

    #[test]
    fn wheel_is_five_high() {
        let wheel = v("Ac 2d 3h 4s 5c");
        assert_eq!(wheel.category(), Category::Straight);
        assert!(wheel < v("2c 3d 4h 5s 6c"));
        assert!(wheel > v("Ac Ad Ah Ks Qc"));
        // Steel wheel is the lowest straight flush.
        let steel = v("Ah 2h 3h 4h 5h");
        assert_eq!(steel.category(), Category::StraightFlush);
        assert!(steel < v("2h 3h 4h 5h 6h"));
        // A-K-Q-J-T is broadway, not a wrap-around: K-A-2-3-4 is no straight.
        assert_eq!(v("Kc Ad 2h 3s 4c").category(), Category::HighCard);
    }

    #[test]
    fn kickers_break_ties() {
        assert!(v("Ac Ad Kh 7s 2c") > v("Ac Ad Qh Js Tc"));
        assert!(v("Ac Ad Kh 7s 3c") > v("Ah As Kc 7d 2c"));
        assert!(v("9c 9d 4h 4s Ac") > v("9h 9s 4c 4d Kc"));
        assert!(v("9c 9d 5h 5s 2c") > v("9h 9s 4c 4d Ac"));
        assert!(v("7c 7d 7h As 2c") > v("7c 7d 7h Ks Qc"));
        assert!(v("8c 8d 8h 2s 2c") > v("7c 7d 7h As Ac"));
        assert!(v("Ac Kc 9c 7c 3c") > v("Ad Kd 9d 7d 2d"));
        assert!(v("Qc Qd Qh Qs 3c") > v("Qc Qd Qh Qs 2d"));
        assert!(v("Kc Kd Kh Ks 3c") > v("Qc Qd Qh Qs Ac"));
    }

    #[test]
    fn exact_ties_compare_equal() {
        assert_eq!(v("Ac Kd Qh Js 9c"), v("Ad Kh Qs Jc 9d"));
        assert_eq!(v("5c 6d 7h 8s 9c"), v("5d 6h 7s 8c 9h"));
        assert_eq!(v("2h 7h 9h Jh Kh"), v("2s 7s 9s Js Ks"));
    }

    #[test]
    fn seven_card_picks_the_best_five() {
        // Board plays: the straight on board ties both hole pairs below it.
        let board = "5c 6d 7h 8s 9c";
        assert_eq!(v(&format!("{board} 2c 2d")), v(&format!("{board} 3c 3d")));
        // A higher straight from hole cards wins.
        assert!(v(&format!("{board} Tc 2d")) > v(&format!("{board} 2c 3d")));
        // Flush over a straight on the same seven.
        assert_eq!(v("5h 6h 7h 8s 9c Ah 2h").category(), Category::Flush);
        // Two trips make a full house with the higher trips on top.
        let fh = v("9c 9d 9h 4s 4c 4d 2h");
        assert_eq!(fh, v("9c 9d 9h 4s 4c"));
        // Three pairs: best two plus the best kicker.
        assert_eq!(v("Ac Ad Kc Kd Qc Qd 2h"), v("Ac Ad Kc Kd Qh"));
        // Straight flush hidden in seven with a bigger plain straight.
        assert_eq!(
            v("4h 5h 6h 7h 8h 9c Tc").category(),
            Category::StraightFlush
        );
        // Six-card form.
        assert_eq!(v("Ac Ad Ah Kc Kd 2s").category(), Category::FullHouse);
    }

    #[test]
    fn exhaustive_five_card_categories_have_the_known_counts() {
        // The textbook distribution over all C(52, 5) = 2,598,960 hands.
        let deck = Card::deck();
        let mut counts = [0u32; 9];
        for a in 0..52 {
            for b in a + 1..52 {
                for c in b + 1..52 {
                    for d in c + 1..52 {
                        for e in d + 1..52 {
                            let h = eval5([deck[a], deck[b], deck[c], deck[d], deck[e]]);
                            counts[h.category() as usize] += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(
            counts,
            [
                1_302_540, 1_098_240, 123_552, 54_912, 10_200, 5_108, 3_744, 624, 40
            ]
        );
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(Card::parse("1s").is_none());
        assert!(Card::parse("Ax").is_none());
        assert!(Card::parse("A").is_none());
        assert_eq!(Card::parse("Ts").unwrap().rank(), 10);
    }
}

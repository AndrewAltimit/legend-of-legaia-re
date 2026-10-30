//! Main and side pots from each seat's total contribution to a hand.

use serde::{Deserialize, Serialize};

/// One pot and the seats that may win it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pot {
    pub amount: u32,
    /// Seat indices, ascending.
    pub eligible: Vec<usize>,
}

/// Split `contributed` (one total per seat) into pots. `live[i]` is false
/// for a seat that folded (its chips play, it cannot win).
///
/// Each distinct contribution level of a live seat closes a pot: the pot
/// takes, from every seat, what it put in between the previous level and
/// this one, and every live seat that reached the level is eligible. Chips
/// a folded seat put in above the top live level join the last pot. A bet
/// nobody called comes back as a one-seat pot.
pub fn build_pots(contributed: &[u32], live: &[bool]) -> Vec<Pot> {
    let mut levels: Vec<u32> = contributed
        .iter()
        .zip(live)
        .filter(|&(&c, &l)| l && c > 0)
        .map(|(&c, _)| c)
        .collect();
    levels.sort_unstable();
    levels.dedup();
    let mut pots: Vec<Pot> = Vec::new();
    let mut prev = 0u32;
    for &level in &levels {
        let amount: u32 = contributed
            .iter()
            .map(|&c| c.min(level).saturating_sub(prev))
            .sum();
        let eligible: Vec<usize> = (0..contributed.len())
            .filter(|&i| live[i] && contributed[i] >= level)
            .collect();
        // Two levels with the same eligible set are one pot.
        match pots.last_mut() {
            Some(last) if last.eligible == eligible => last.amount += amount,
            _ => pots.push(Pot { amount, eligible }),
        }
        prev = level;
    }
    let dead: u32 = contributed.iter().map(|&c| c.saturating_sub(prev)).sum();
    if dead > 0 {
        match pots.last_mut() {
            Some(last) => last.amount += dead,
            None => pots.push(Pot {
                amount: dead,
                eligible: Vec::new(),
            }),
        }
    }
    pots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_stacks_make_one_pot() {
        let p = build_pots(&[100, 100, 100], &[true, true, true]);
        assert_eq!(
            p,
            vec![Pot {
                amount: 300,
                eligible: vec![0, 1, 2]
            }]
        );
    }

    #[test]
    fn short_all_in_splits_a_side_pot() {
        // Seat 0 all-in for 50; 1 and 2 play on to 200.
        let p = build_pots(&[50, 200, 200], &[true, true, true]);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].amount, 150);
        assert_eq!(p[0].eligible, vec![0, 1, 2]);
        assert_eq!(p[1].amount, 300);
        assert_eq!(p[1].eligible, vec![1, 2]);
    }

    #[test]
    fn three_levels_and_a_folder() {
        // 0 all-in 30, 1 all-in 80, 2 and 3 to 150; 3 folds at the end.
        let p = build_pots(&[30, 80, 150, 150], &[true, true, true, false]);
        assert_eq!(p[0].amount, 120);
        assert_eq!(p[0].eligible, vec![0, 1, 2]);
        assert_eq!(p[1].amount, 150);
        assert_eq!(p[1].eligible, vec![1, 2]);
        assert_eq!(p[2].amount, 140);
        assert_eq!(p[2].eligible, vec![2]);
        let total: u32 = p.iter().map(|p| p.amount).sum();
        assert_eq!(total, 410);
    }

    #[test]
    fn uncalled_bet_returns_to_its_owner() {
        let p = build_pots(&[300, 100], &[true, true]);
        assert_eq!(p[0].amount, 200);
        assert_eq!(
            p[1],
            Pot {
                amount: 200,
                eligible: vec![0]
            }
        );
    }

    #[test]
    fn folded_overage_joins_the_last_pot() {
        // Seat 2 folded after putting in more than the live all-in seat.
        let p = build_pots(&[40, 40, 60], &[true, true, false]);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].amount, 140);
    }
}

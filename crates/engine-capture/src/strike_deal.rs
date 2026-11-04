//! Dealing a recorded monster strike the hand retail was dealt
//! (`LEGAIA_STRIKE_DEAL`).
//!
//! Past a strike capture's own instant nothing holds the two rounds together:
//! the rest of the swing stream was rolled at the pick, and each hit's block
//! roll, damage and impact cue draw on a stream each side reached another way.
//! So the cast-sequence oracle's sound channel reads a deal as a wrong cue.
//!
//! This stands the engine's fight on what retail held when the capture's gate
//! opens: the capture's queue `+0x1DF..` (the drive's own `played` bytes), its
//! approach terms `ctx[+0x6D2]` / `ctx[+0x6D4]` (the replay walked in from elsewhere,
//! to a target facing elsewhere) and the seed word `0x80009010` retail held going into each hit, from the
//! recording's per-vsync `rand` column. What still differs is the rules. A play
//! host never sets it; unset, the engine runs its own stream.

use std::collections::VecDeque;

use crate::battle_drive::{StrikeHand, engine_seat};
use legaia_engine_core::world::World;

/// The words in order, one per melee hit past the gate.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StrikeDeal {
    pub hit_words: Vec<u32>,
}

/// `w1,w2,..`, each decimal. An empty list is a deal of no hits.
pub fn strike_deal_from_env(v: &str) -> Option<StrikeDeal> {
    let hit_words = v
        .split(',')
        .filter(|f| !f.trim().is_empty())
        .map(|f| f.trim().parse().ok())
        .collect::<Option<Vec<u32>>>()?;
    Some(StrikeDeal { hit_words })
}

/// Stand the striker on the capture's queue, the fight on its approach terms,
/// and arm the hit words.
pub fn apply_strike_deal(world: &mut World, hand: &StrikeHand, deal: &StrikeDeal) {
    let actor = usize::from(engine_seat(hand.seat, world.party.party_count));
    if let Some(a) = world.actors.get_mut(actor) {
        let n = hand.queue.len().min(a.battle.params.len());
        a.battle.params[..n].copy_from_slice(&hand.queue[..n]);
    }
    if let Some((attack, guard)) = hand.ramps {
        world.battle.attack_ramp = attack;
        world.battle.guard_ramp = guard;
    }
    world.battle.capture_hit_rand = VecDeque::from(deal.hit_words.clone());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_env_form_reads_decimal_words_in_order() {
        let d = strike_deal_from_env("1,4294967295").unwrap();
        assert_eq!(d.hit_words, vec![1, u32::MAX]);
        assert_eq!(
            strike_deal_from_env("").unwrap().hit_words,
            Vec::<u32>::new()
        );
        assert!(strike_deal_from_env("x").is_none());
    }
}

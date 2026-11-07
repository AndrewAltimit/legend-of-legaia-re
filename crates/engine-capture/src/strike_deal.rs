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
//! to a target facing elsewhere), the striker's working ATK and the seed word `0x80009010` retail held going into each hit, from the
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
        restage_dealt_byte(&mut a.battle, &hand.queue);
        let n = hand.queue.len().min(a.battle.params.len());
        a.battle.params[..n].copy_from_slice(&hand.queue[..n]);
    }
    if let Some((attack, guard, atk)) = hand.ramps {
        world.battle.attack_ramp = attack;
        world.battle.guard_ramp = guard;
        if let Some(slot) = world.battle.attack.get_mut(actor) {
            *slot = atk;
        }
    }
    world.battle.capture_hit_rand = VecDeque::from(deal.hit_words.clone());
}

/// The byte the replay's strike loop already staged from its own stream,
/// re-staged from the dealt queue.
///
/// The gate opens on the strike loop's phase, not on its stream: a walking
/// monster's loop stages its first byte over the idle on the pass it opens
/// (`battle_rugi_caruban_3_strike` holds clip `0` with byte `8` staged), and
/// the replay's own pick may have staged another (its stream read `07 07 06`).
/// Copying the queue alone leaves that byte to commit, so the replay played
/// its own first swing and skipped retail's. A byte staged and not yet
/// committed is the queue's byte behind the cursor: it takes the dealt one.
fn restage_dealt_byte(b: &mut legaia_engine_battle_vm::battle_action::BattleActor, queue: &[u8]) {
    let Some(behind) = usize::from(b.strike_index).checked_sub(1) else {
        return;
    };
    let (Some(&own), Some(&dealt)) = (b.params.get(behind), queue.get(behind)) else {
        return;
    };
    if b.queued_anim == own && b.queued_anim != b.current_anim && dealt != 0 {
        b.queued_anim = dealt;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_staged_byte_the_replay_picked_takes_the_dealt_one() {
        let mut b = legaia_engine_battle_vm::battle_action::BattleActor::default();
        b.params[..3].copy_from_slice(&[7, 7, 6]);
        b.strike_index = 1;
        b.queued_anim = 7;
        restage_dealt_byte(&mut b, &[8, 7, 6]);
        assert_eq!(b.queued_anim, 8);
        // A committed byte is the clip playing: left alone.
        b.queued_anim = 7;
        b.current_anim = 7;
        restage_dealt_byte(&mut b, &[8, 7, 6]);
        assert_eq!(b.queued_anim, 7);
    }

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

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

/// The words in order, one per melee hit past the gate, each with the
/// recording's vsync it was read on when the deal names one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StrikeDeal {
    pub hit_words: Vec<(u32, Option<u32>)>,
}

/// `w1,w2,..`, each decimal, or `w@vsync` with the recording's vsync of the
/// hit the word went into. An empty list is a deal of no hits.
pub fn strike_deal_from_env(v: &str) -> Option<StrikeDeal> {
    let hit_words = v
        .split(',')
        .filter(|f| !f.trim().is_empty())
        .map(|f| {
            let mut it = f.trim().splitn(2, '@');
            let w = it.next()?.parse().ok()?;
            let at = match it.next() {
                Some(t) => Some(t.parse().ok()?),
                None => None,
            };
            Some((w, at))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(StrikeDeal { hit_words })
}

/// Stand the striker on the capture's queue, the fight on its approach terms,
/// and arm the hit words.
pub fn apply_strike_deal(world: &mut World, hand: &StrikeHand, deal: &StrikeDeal) {
    let actor = usize::from(engine_seat(hand.seat, world.party.party_count));
    if let (Some(a), Some(queue)) = (world.actors.get_mut(actor), hand.queue.as_ref()) {
        restage_dealt_byte(&mut a.battle, queue);
        let n = queue.len().min(a.battle.params.len());
        a.battle.params[..n].copy_from_slice(&queue[..n]);
    }
    if let Some((attack, guard, atk)) = hand.ramps {
        world.battle.attack_ramp = attack;
        world.battle.guard_ramp = guard;
        if let Some(slot) = world.battle.attack.get_mut(actor) {
            *slot = atk;
        }
    }
    deal_hit_words(world, deal);
}

/// Arm the hit words alone: the deal of a cast whose hits are the melee
/// kernel's (PROT 0952's Astral Slash lands Gaza's clip `0x0A` through
/// `FUN_801EC3E4`), where the capture's drive holds no strike hand to stand
/// the queue or the approach terms on.
pub fn deal_hit_words(world: &mut World, deal: &StrikeDeal) {
    // A timed word lands on the hit nearest its vsync: the gate stands on the
    // recording's first row, so the recording's vsync counts from it.
    let gate = world.clock.display_frames;
    world.battle.capture_hit_rand = deal
        .hit_words
        .iter()
        .map(|&(w, at)| (w, at.map(|v| gate + u64::from(v.saturating_sub(1)))))
        .collect::<VecDeque<_>>();
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
        assert_eq!(d.hit_words, vec![(1, None), (u32::MAX, None)]);
        let d = strike_deal_from_env("7@147,9@230").unwrap();
        assert_eq!(d.hit_words, vec![(7, Some(147)), (9, Some(230))]);
        assert!(strike_deal_from_env("7@x").is_none());
        assert_eq!(
            strike_deal_from_env("").unwrap().hit_words,
            Vec::<(u32, Option<u32>)>::new()
        );
        assert!(strike_deal_from_env("x").is_none());
    }
}

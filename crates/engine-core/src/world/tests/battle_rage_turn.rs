//! A Rage (Evil Medallion) seat's turn: the round's state-`0x00` pass fills
//! its stream with learned arts (`FUN_801F0450`'s delegated arm) and the
//! seat's dispatch plays that stream as it stands - the seed's builder call
//! (`FUN_801EED1C` from `0x801E2C7C`) leaves a stream of action constants
//! untouched. A write-watched retail round (every party seat poked to Rage)
//! shows exactly that: the stream's only writers are the fill at `0x801F068C`
//! / `0x801F06B8` and the round-boundary clear, and the seat's latched clip
//! `+0x1DB` takes the stream's art bytes in order.

use super::*;
use legaia_engine_mode::mode::SceneMode;

const RAGE_BIT: u32 = 0x2000;

fn rage_battle(rage_seats: &[usize], learned: &[u8]) -> World {
    let mut w = World::new();
    w.party.party_count = 3;
    w.battle.player_driven = true;
    w.toggles.live_gameplay_loop = true;
    w.mode = SceneMode::Battle;
    // The zeroed records seed HP 0, so they go in first.
    w.load_party(legaia_save::Party::zeroed(3));
    for i in 0..5 {
        w.actors[i].active = true;
        w.actors[i].battle.liveness = 1;
        w.actors[i].battle.hp = 100;
        w.actors[i].battle.max_hp = 100;
    }
    for &seat in rage_seats {
        let rslot = w.party_roster_slot(seat);
        let m = &mut w.party.roster.members[rslot];
        let mut bits = m.ability_bits();
        bits[4..8].copy_from_slice(&RAGE_BIT.to_le_bytes());
        m.set_ability_bits(bits);
        let mut list = m.displayed_skills();
        list.count = learned.len() as u8;
        list.ids[..learned.len()].copy_from_slice(learned);
        m.set_displayed_skills(list);
        // The anim tick's Rage arm stamps the delegation group every frame.
        w.actors[seat].battle.field_flags |= 0x380;
    }
    w
}

fn stream(w: &World, seat: usize) -> Vec<u8> {
    w.actors[seat]
        .battle
        .params
        .iter()
        .take_while(|&&b| b != 0)
        .copied()
        .collect()
}

/// The dispatch plays the fill's stream byte for byte - an arts combo, not
/// the two-swing basic attack a plain auto-strike would seed.
#[test]
fn a_rage_seat_plays_the_learned_arts_stream_the_round_start_filled() {
    let mut filled_runs = 0;
    for seed in 1..40u32 {
        let mut w = rage_battle(&[0], &[7, 11, 10, 6]);
        w.rng_state = seed;
        w.run_round_state_zero();
        assert_eq!(w.battle_ctx.delegated_seats, 0b001, "only the Rage seat");
        let filled = w.actors[0].battle.params;
        assert_eq!(w.actors[0].battle.action_category, 3);
        let arts = stream(&w, 0);
        // Learned ids at or above roster id 1's floor (4), biased by 0x1B.
        assert!(
            arts.iter().all(|b| [7u8, 11, 10, 6].contains(&(b - 0x1B))),
            "{arts:02X?}"
        );
        if !arts.is_empty() {
            filled_runs += 1;
        }
        w.battle_ctx.super_trigger = true;
        w.dispatch_battle_turn(0);
        assert_eq!(
            w.actors[0].battle.params, filled,
            "seed {seed}: not re-seeded"
        );
        assert_eq!(w.actors[0].battle.action_category, 3);
        assert_eq!(w.battle_ctx.queued_action, 3);
        assert_eq!(w.actors[0].battle.strike_index, 0);
        assert!(
            !w.battle_ctx.super_trigger,
            "the builder's head clears 0x801F696C"
        );
    }
    assert!(filled_runs > 0, "some seed draws at least one art");
}

/// A seat without the passive still takes the plain strike the engine arms
/// for a member with no commit, and is not marked delegated.
#[test]
fn a_seat_without_rage_is_not_delegated() {
    let mut w = rage_battle(&[0], &[7, 11, 10, 6]);
    w.run_round_state_zero();
    assert_eq!(w.battle_ctx.delegated_seats & 0b010, 0);
    w.dispatch_battle_turn(1);
    let s = stream(&w, 1);
    assert!(
        !s.is_empty() && s.iter().all(|b| (0x0C..=0x0F).contains(b)),
        "{s:02X?}"
    );
}

/// An empty learned list still commits the Attack (category and target are
/// written before the count gate at `0x801F05AC`); the turn is a walk-up
/// with no swing, as the retail seat whose first stop roll ended its fill.
#[test]
fn a_rage_seat_with_no_learned_arts_walks_up_with_an_empty_stream() {
    let mut w = rage_battle(&[0], &[]);
    w.run_round_state_zero();
    assert_eq!(w.battle_ctx.delegated_seats, 0b001);
    w.dispatch_battle_turn(0);
    assert_eq!(w.actors[0].battle.action_category, 3);
    assert!(stream(&w, 0).is_empty());
}

/// The whole party under Rage: every seat is filled, and each plays its own
/// stream.
#[test]
fn a_whole_rage_party_is_filled_seat_by_seat() {
    let mut w = rage_battle(&[0, 1, 2], &[7, 11, 10, 6]);
    w.rng_state = 3;
    w.run_round_state_zero();
    assert_eq!(w.battle_ctx.delegated_seats, 0b111);
    for seat in 0..3 {
        let filled = w.actors[seat].battle.params;
        w.dispatch_battle_turn(seat as u8);
        assert_eq!(w.actors[seat].battle.params, filled, "seat {seat}");
    }
}

/// The turn picker's dead-target redirect reaches the fill's target: a
/// monster that fell earlier in the round is swapped for a living one.
#[test]
fn a_rage_seats_dead_target_is_redirected() {
    let mut w = rage_battle(&[0], &[7, 11, 10, 6]);
    w.run_round_state_zero();
    let rolled = w.actors[0].battle.active_target;
    assert!((3..5).contains(&rolled), "a monster seat: {rolled}");
    w.actors[usize::from(rolled)].battle.hp = 0;
    w.dispatch_battle_turn(0);
    let other = if rolled == 3 { 4 } else { 3 };
    assert_eq!(w.actors[0].battle.active_target, other);
}

/// The fill runs at state `0x00`, after the round's commits, and overwrites
/// the seat's category and stream (`0x801F0528`, `0x801F06B8`): a commit the
/// engine still holds for a Rage seat is dropped, not played.
#[test]
fn the_delegated_fill_wins_over_a_held_commit() {
    use crate::battle_round::PendingPartyAction;
    let mut w = rage_battle(&[0], &[7, 11, 10, 6]);
    w.rng_state = 5;
    w.run_round_state_zero();
    let filled = w.actors[0].battle.params;
    w.battle.round_flow.pending[0] = Some(PendingPartyAction::Spirit);
    w.dispatch_battle_turn(0);
    assert_eq!(w.actors[0].battle.params, filled);
    assert_eq!(w.actors[0].battle.action_category, 3);
    assert!(w.battle.round_flow.pending[0].is_none());
}

/// Every party dispatch runs the builder, whose head clears the
/// special-trigger flag `0x801F696C` (`0x801EED88`).
#[test]
fn every_party_dispatch_clears_the_special_trigger_flag() {
    use crate::battle_round::PendingPartyAction;
    let mut w = rage_battle(&[], &[]);
    w.battle.round_flow.pending[1] = Some(PendingPartyAction::Spirit);
    w.battle_ctx.super_trigger = true;
    w.dispatch_battle_turn(1);
    assert!(!w.battle_ctx.super_trigger, "a Spirit commit");
    w.battle_ctx.super_trigger = true;
    w.dispatch_battle_turn(2);
    assert!(!w.battle_ctx.super_trigger, "a member with no commit");
}

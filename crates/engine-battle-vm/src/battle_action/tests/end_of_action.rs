//! End-of-action wipe checks, victory staging and the round boundary.

use super::*;

#[test]
fn end_of_action_party_wipe_signals_battle_end() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    // Kill all party.
    host.actors[0].liveness = 0;
    host.actors[1].liveness = 0;
    host.actors[2].liveness = 0;
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::BattleComplete);
    assert!(
        host.take()
            .contains(&Event::BattleEnd(BattleEndCause::PartyWipe))
    );
}

/// A petrified party is a wipe even when the Stone bit lives only in the
/// host's packed `+0x16E` (`status_word`), not in the actor view's
/// `field_flags`: retail masks the whole word with `0x4`, Stone's own bit.
#[test]
fn end_of_action_counts_a_packed_stone_party_as_wiped() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    for s in 0..3u8 {
        host.packed_status.insert(s, 0x4);
    }
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::BattleComplete);
    assert!(
        host.take()
            .contains(&Event::BattleEnd(BattleEndCause::PartyWipe))
    );
}

/// "No party seated" is not "party dead". A battle whose party slots were
/// never seated (`slot_seated == false`, the port-only state retail cannot
/// represent - see `BattleActionHost::slot_seated`) must NOT resolve as a
/// party wipe when those hollow slots read dead; the round continues.
#[test]
fn end_of_action_unseated_party_is_not_reported_as_a_party_wipe() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    for i in 0..3u8 {
        host.actors[i as usize].liveness = 0;
        host.unseated.insert(i);
    }
    let out = step(&mut host, &mut ctx);
    assert_ne!(
        out,
        StepOutcome::BattleComplete,
        "an unseeded battle must not end on the party-wipe arm"
    );
    assert!(
        !host
            .take()
            .contains(&Event::BattleEnd(BattleEndCause::PartyWipe)),
        "no PartyWipe may be signalled for a party that was never seated"
    );
}

/// The unseeded state must still be able to TERMINATE: with no seated party
/// and the monsters down, the monster-wipe arm still fires, so a host that
/// reaches this port-only state tears down to victory rather than spinning.
#[test]
fn end_of_action_unseated_party_still_resolves_a_monster_wipe() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    for i in 0..3u8 {
        host.actors[i as usize].liveness = 0;
        host.unseated.insert(i);
    }
    for i in 3..ACTOR_SLOTS {
        host.actors[i].liveness = 0;
    }
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::BattleComplete);
    let events = host.take();
    assert!(events.contains(&Event::BattleEnd(BattleEndCause::MonsterWipe)));
    assert!(!events.contains(&Event::BattleEnd(BattleEndCause::PartyWipe)));
}

#[test]
fn end_of_action_monster_wipe_signals_battle_end() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    // Kill all monsters.
    for i in 3..ACTOR_SLOTS {
        host.actors[i].liveness = 0;
    }
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::BattleComplete);
    assert!(
        host.take()
            .contains(&Event::BattleEnd(BattleEndCause::MonsterWipe))
    );
}

#[test]
fn end_of_action_monster_wipe_stages_victory_for_acting_party_slot() {
    // Retail baseline: a living party member dealt the kill - the victory
    // arm keeps the acting slot (0x801E6690 alive-skip) and stages the win
    // pose for it (0x801E6770..0x801E6790).
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    for i in 3..ACTOR_SLOTS {
        host.actors[i].liveness = 0;
    }
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::BattleComplete);
    let events = host.take();
    assert!(events.contains(&Event::BattleEnd(BattleEndCause::MonsterWipe)));
    assert!(events.contains(&Event::VictoryStage(1)));
    assert_eq!(ctx.active_actor, 1);
}

#[test]
fn end_of_action_monster_wipe_repicks_pose_actor_when_acting_actor_dead() {
    // Retail re-pick (0x801E66A4..0x801E6724): the acting actor died during
    // its own action; the pose actor re-rolls onto a living, non-0x404
    // party slot. Slot 0 is dead and slot 1 carries 0x404, so slot 2 is the
    // only eligible pick.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    for i in 3..ACTOR_SLOTS {
        host.actors[i].liveness = 0;
    }
    host.actors[0].liveness = 0;
    host.actors[1].field_flags = 0x404;
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::BattleComplete);
    assert!(host.take().contains(&Event::VictoryStage(2)));
    assert_eq!(ctx.active_actor, 2);
}

#[test]
fn end_of_action_retail_mask_keeps_living_charmed_ally_blocking_victory() {
    // Without the widen (retail mask 0x4), a living charmed monster
    // (`+0x16E & 0x380`) still counts as standing - the battle continues.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 3);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    for i in 4..ACTOR_SLOTS {
        host.actors[i].liveness = 0;
    }
    host.actors[3].field_flags = 0x380;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition { to, .. } if to == ActionState::PreActionWait.as_byte()
    ));
    assert!(!host.take().iter().any(|e| matches!(e, Event::BattleEnd(_))));
}

#[test]
fn charm_widen_victory_with_living_charmed_acting_ally_repicks_party_slot() {
    // The charm-softlock wedge condition: the charmed ally (slot 3, alive,
    // `0x380`) dealt the killing blow to the last real enemy, and the
    // randomizer's widened wipe mask (0x384) counts the ally itself as
    // down - victory fires with a living MONSTER as the acting actor.
    // Retail's alive-skip would then index the 3-byte party roster
    // `DAT_8007BD10` with slot 3 and arm a garbage win-pose stream (the
    // pinned softlock). The port must re-pick a living party slot and
    // terminate.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 3);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    ctx.charm_widen = true;
    for i in 4..ACTOR_SLOTS {
        host.actors[i].liveness = 0;
    }
    host.actors[3].field_flags = 0x380;
    host.rng_seq = vec![1];
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::BattleComplete);
    let events = host.take();
    assert!(events.contains(&Event::BattleEnd(BattleEndCause::MonsterWipe)));
    // Pose slot re-picked into the party band - never the monster slot.
    let staged = events.iter().find_map(|e| match e {
        Event::VictoryStage(s) => Some(*s),
        _ => None,
    });
    assert_eq!(staged, Some(1), "rng picks uniformly among 3 living slots");
    assert!(ctx.active_actor < 3);
}

#[test]
fn charm_widen_victory_terminates_even_when_no_party_slot_is_eligible() {
    // Where retail's rejection loop is unbounded: every living party member
    // carries 0x404. The port falls back to the first living party slot
    // instead of spinning.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 3);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    ctx.charm_widen = true;
    for i in 4..ACTOR_SLOTS {
        host.actors[i].liveness = 0;
    }
    host.actors[3].field_flags = 0x380;
    host.actors[0].liveness = 0;
    host.actors[1].field_flags = 0x404;
    host.actors[2].field_flags = 0x400;
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::BattleComplete);
    // Slot 1 is alive but 0x404 (non-targetable): the party-alive scan
    // masks 0x4, so slot 2 (0x400 - targetable) kept the party standing,
    // and the fallback picks the first living slot (1).
    assert!(host.take().contains(&Event::VictoryStage(1)));
}

#[test]
fn a_special_battle_wipes_on_a_leader_with_every_limb_rotted() {
    // `0x801E6578..0x801E65AC`: the party is standing, but the special word
    // is up and slot 0's `+0x16E` carries all of `0x38` - the wipe fires.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    host.actors[0].field_flags = 0x38;
    host.special_word = 0x100;
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::BattleComplete);
    assert!(
        host.take()
            .contains(&Event::BattleEnd(BattleEndCause::PartyWipe))
    );

    // The same leader in an ordinary battle fights on.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    host.actors[0].field_flags = 0x38;
    assert_ne!(step(&mut host, &mut ctx), StepOutcome::BattleComplete);

    // Two rotted limbs are not enough, and a rotted non-leader never is.
    for (slot, flags) in [(0usize, 0x30u16), (1, 0x38)] {
        let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
        ctx.action_state = ActionState::EndOfAction.as_byte();
        host.actors[slot].field_flags = flags;
        host.special_word = 0x100;
        assert_ne!(step(&mut host, &mut ctx), StepOutcome::BattleComplete);
    }
}

#[test]
fn victory_pose_formation_override_forces_songi_slot() {
    // Retail 0x801E6728..0x801E676C: first monster id 0xB3 forces the
    // victory pose onto party slot 2 regardless of who acted.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    host.first_monster_id = 0xB3;
    for i in 3..ACTOR_SLOTS {
        host.actors[i].liveness = 0;
    }
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::BattleComplete);
    assert!(host.take().contains(&Event::VictoryStage(2)));
    assert_eq!(ctx.active_actor, 2);
}

#[test]
fn end_of_action_captured_monster_counts_as_down_under_retail_mask() {
    // Retail mask 0x4: an alive but non-targetable monster (captured) does
    // not block the monster-wipe victory.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    for i in 4..ACTOR_SLOTS {
        host.actors[i].liveness = 0;
    }
    host.actors[3].field_flags = 0x4;
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::BattleComplete);
    assert!(
        host.take()
            .contains(&Event::BattleEnd(BattleEndCause::MonsterWipe))
    );
}

#[test]
fn end_of_action_continues_when_both_sides_alive() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    ctx.turn_cursor = 0;
    let out = step(&mut host, &mut ctx);
    // 8 alive total → bumped cursor (1) < 8 → restart at PreActionWait.
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::PreActionWait.as_byte()
    ));
}

#[test]
fn round_end_is_a_round_boundary_not_a_battle_end() {
    // Retail state 0xFF is written only by the NON-wipe arm of 0x5A (both
    // sides standing, everyone has acted); wipes signal through
    // DAT_8007BD71 = 0xFE without writing a state byte. Reaching 0xFF must
    // therefore never end the battle - the earlier port mapped it to
    // battle_end(MonsterWipe), a spurious victory.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    // Leave a stale turn cursor; the boundary rewinds it.
    ctx.turn_cursor = 7;
    ctx.action_state = ActionState::RoundEnd.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(
        matches!(
            out,
            StepOutcome::Transition { to, .. } if to == ActionState::EndOfAction.as_byte()
        ),
        "round boundary hands control back through EndOfAction, got {out:?}"
    );
    assert!(
        !host.take().iter().any(|e| matches!(e, Event::BattleEnd(_))),
        "the round boundary must not signal battle end"
    );
    assert_eq!(
        ctx.turn_cursor, 0,
        "the turn cursor rewinds for the new round"
    );
}

/// Regression for the spurious-victory defect: drive a full attack action to
/// its end with BOTH sides alive and keep stepping without re-arming, the way
/// a driver that parks the SM at `EndOfAction` (e.g. a folded monster cast)
/// does. The turn cursor - seeded at `0` by `Begin` and bumped once per
/// end-of-action pass - reaches `alive_total` and routes the `0x5A` gate to
/// `0xFF`. Pre-fix that produced
/// `battle_end(BattleEndCause::MonsterWipe)` + `StepOutcome::BattleComplete`
/// after one round; post-fix it is a round boundary and the battle continues.
#[test]
fn full_round_with_both_sides_alive_does_not_end_the_battle() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    // 3 party + 1 monster alive = alive_total 4, the smallest retail-shaped
    // battle and the easiest for the stamped counter (3 + 1 bump) to reach.
    for i in 4..ACTOR_SLOTS {
        host.actors[i].liveness = 0;
    }
    // Arm exactly the way engine-core does (queued_action = 3, state Begin).
    ctx.queued_action = 3;
    ctx.action_state = ActionState::Begin.as_byte();
    // One swing then the terminator, so the attack chain resolves.
    host.actors[0].params[0] = 0x10;
    host.actors[0].params[1] = 0xFF;

    let mut saw_round_boundary = false;
    for _ in 0..0x400 {
        let out = step(&mut host, &mut ctx);
        // Stand in for the anim system: signal the staged swing finished so
        // the chain's 0x801E370C read gate opens (same edge the existing
        // full-flow test drives by hand).
        host.actors[0].flag_bits.clear(ActorFlags::ADVANCE_DONE);
        assert_ne!(
            out,
            StepOutcome::BattleComplete,
            "both sides alive: no step may complete the battle (state {:#04X})",
            ctx.action_state
        );
        if let StepOutcome::Transition { from, .. } = out
            && from == ActionState::RoundEnd.as_byte()
        {
            saw_round_boundary = true;
            break;
        }
    }
    assert!(
        saw_round_boundary,
        "the drive must cross the 0xFF round boundary to be non-vacuous"
    );
    assert!(
        !host.take().iter().any(|e| matches!(e, Event::BattleEnd(_))),
        "no battle-end cause may be signalled with both sides standing"
    );

    // The same gate still ends the battle on a GENUINE wipe: kill the last
    // monster and let the next EndOfAction pass resolve it.
    host.actors[3].liveness = 0;
    ctx.action_state = ActionState::EndOfAction.as_byte();
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::BattleComplete);
    assert!(
        host.take()
            .contains(&Event::BattleEnd(BattleEndCause::MonsterWipe)),
        "a genuine monster wipe still ends the battle"
    );
}

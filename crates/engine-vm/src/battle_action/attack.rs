//! Physical-attack band of the battle-action state machine (face / short-step / windup / chain).

use super::*;

// --- attack band ------------------------------------------------------------

/// Per-frame facing recompute the attack band's states share (`0x14` at
/// `0x801E32EC..0x801E3318`, `0x15`/`0x16`/`0x19` siblings): `facing =
/// (bearing(target_live -> attacker_live) + 0x800) & 0xFFF`, stored into
/// `actor[+0x46]`. The half-turn flips the target-to-attacker bearing into
/// the attacker-to-target heading the trig consumers (root motion, arrival
/// shove, effect placement) walk along. Skipped when the host tracks no
/// positions - the facing is left alone, the pre-accessor behaviour.
fn update_attack_facing<H: BattleActionHost + ?Sized>(host: &mut H, slot: u8, target: u8) {
    let (Some(a), Some(t)) = (host.actor_position(slot), host.actor_position(target)) else {
        return;
    };
    let bearing = bearing_12bit_approx(t.1, t.0, a.1, a.0);
    let facing = bearing.wrapping_add(0x800) & 0xFFF;
    if let Some(actor) = host.actor_mut(slot) {
        actor.facing_angle = facing;
    }
}

pub(super) fn attack_face<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let actor_slot = ctx.active_actor;
    let target_slot = host.actor(actor_slot).map(|a| a.active_target).unwrap_or(0);
    host.pose(actor_slot, Pose::Idle);
    update_attack_facing(host, actor_slot, target_slot);
    let range = host.range_check(actor_slot, target_slot);
    let party_count = host.party_count();
    let next = if range == 0 {
        ActionState::AttackChain
    } else if actor_slot < party_count {
        // Retail stages the approach anim for the party short-step: literal
        // anim id 1 (record[0] entry 1, the walk clip) into `+0x1DA`
        // (overlay_battle_action_801e295c, the state-0x14 party arm).
        if let Some(actor) = host.actor_mut(actor_slot) {
            actor.queued_anim = 1;
        }
        ActionState::AttackShortStep
    } else {
        // Monster arm: retail scans the record's action table for the
        // tag-`0x20` walk (`FUN_80050E2C`) and stages the found entry index
        // (fallback: the tag-`1` Move clip, which routes to `0x19` instead).
        // The engine stages entry 1 - the walk/approach slot of the action
        // tag space (`MonsterAnimation::action_id` 1) - and keeps the
        // windup/advance chain for every monster; the routing difference is
        // disclosed in `docs/subsystems/battle-action.md` (engine port note).
        if let Some(actor) = host.actor_mut(actor_slot) {
            actor.queued_anim = 1;
        }
        ActionState::AttackWindup
    };
    transition(ctx, next)
}

pub(super) fn attack_windup<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    let target = host.actor(slot).map(|a| a.active_target).unwrap_or(0);
    host.pose(slot, Pose::Idle);
    update_attack_facing(host, slot, target);
    if let Some(actor) = host.actor_mut(slot) {
        // Advance anim cursor toward queued.
        if actor.queued_anim != actor.current_anim {
            return stay(ctx);
        }
    } else {
        return stay(ctx);
    }
    transition(ctx, ActionState::AttackAdvance)
}

pub(super) fn attack_advance<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    let target = host.actor(slot).map(|a| a.active_target).unwrap_or(0);
    host.pose(slot, Pose::Idle);
    update_attack_facing(host, slot, target);
    let range = host.range_check(slot, target);
    if range != 0 {
        // Out of range: stay. The movement is NOT here - the walk clip's
        // root-motion term in the anim tick drives the attacker
        // (`FUN_80047430` `0x80047D20..0x80047E18`; engine
        // `World::tick_battle_locomotion`), gated on this same range check.
        return stay(ctx);
    }
    // Arrival shove (retail `0x801E33EC..0x801E3490`): after staging the
    // close-in, the SM steps the *target's* live and seat pairs along the
    // attacker's facing by `trig >> 9` per iteration, while the pair still
    // measures in range - the target is pushed back out to the range
    // boundary before the strikes run. The iteration guard is an engine
    // safety bound the retail loop doesn't need (its trig steps always
    // terminate); it never binds on real geometry.
    let facing = host.actor(slot).map(|a| a.facing_angle).unwrap_or(0);
    let (sin, cos) = motion::trig12(facing);
    let (dx, dz) = motion::arrival_shove_step(sin, cos);
    if (dx, dz) != (0, 0) {
        let mut guard = 0u32;
        while guard < 0x400 && host.range_check(slot, target) == 0 {
            let Some((x, z)) = host.actor_position(target) else {
                break;
            };
            host.set_actor_position(target, x.wrapping_add(dx), z.wrapping_add(dz));
            if let Some((ax, az)) = host.actor_anchor(target) {
                host.set_actor_anchor(target, ax.wrapping_add(dx), az.wrapping_add(dz));
            }
            guard += 1;
        }
    }
    transition(ctx, ActionState::AttackCloseRange)
}

pub(super) fn attack_close_range<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.pose(slot, Pose::Idle);
    let matched = host
        .actor(slot)
        .map(|a| a.queued_anim == a.current_anim)
        .unwrap_or(false);
    if !matched {
        return stay(ctx);
    }
    transition(ctx, ActionState::AttackStrike)
}

pub(super) fn attack_strike<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    let matched = host
        .actor(slot)
        .map(|a| a.queued_anim == a.current_anim)
        .unwrap_or(false);
    if !matched {
        return stay(ctx);
    }
    transition(ctx, ActionState::AttackChain)
}

pub(super) fn attack_short_step<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    let target = host.actor(slot).map(|a| a.active_target).unwrap_or(0);
    host.pose(slot, Pose::Idle);
    update_attack_facing(host, slot, target);
    let range = host.range_check(slot, target);
    if range != 0 {
        // No movement code and no timeout in this state (retail `0x19`
        // stalls at `0x801E35D0`): the staged walk clip's root motion is
        // the drive (engine `World::tick_battle_locomotion`).
        return stay(ctx);
    }
    if let Some(actor) = host.actor_mut(slot) {
        actor.flag_bits.set(ActorFlags::WINDUP_DONE);
        actor.combo_bit = 0;
        // Retail clears the queued approach anim on arrival (`+0x1DA = 0`).
        actor.queued_anim = 0;
    }
    transition(ctx, ActionState::AttackChain)
}

pub(super) fn attack_chain<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    // Walk the per-actor strike-script byte stream. The retail attack band
    // terminates on a `0x00` byte (the magic band is the one that uses `-1`;
    // overlay_battle_action_801e295c, strike-loop arm); `0xFF` additionally
    // terminates as this port's out-of-range sentinel. Otherwise stage the
    // byte as the queued anim.
    //
    // **This state stages; it never resolves damage.** `jal 0x801ec3e4`
    // does not occur anywhere in `FUN_801E295C`: the damage kernel is called
    // from the anim tick `FUN_80047430` (`0x800478A0` / `0x80047BF0`) on
    // every frame the committed clip plays, and its own head decides which
    // frames are that clip's hit events (`crate::battle_action::hit_event`).
    // The engine's driver for that call sits beside its anim tick
    // (`engine-core`'s `World::tick_battle_hit_events`); a host that plays
    // no clips resolves the staged byte's hits as a zero-length clip
    // instead. Either way nothing in this arm touches HP.
    let slot = ctx.active_actor;
    // Strike pacing gate: while ADVANCE_DONE is still set the previous
    // staged swing is in flight - skip the byte read and hold (the anim
    // system clears the bit when the staged clip's event-path commit fires
    // or the clip ends; for the engine that's `World::tick_battle_hit_events`
    // / `World::tick_battle_animations`, or an immediate clear when the
    // actor carries no clips).
    // PORT: overlay_battle_action_801e295c (strike-pacing gate, interior).
    // The retail gate (battle-action overlay, file +0x370C) reads `lbu +0x1DC;
    // andi 0x2; bne -> skip` to guard the next-byte read at `+0x1DF + +0x15`.
    let in_flight = host
        .actor(slot)
        .map(|a| a.flag_bits.has(ActorFlags::ADVANCE_DONE))
        .unwrap_or(false);
    if in_flight {
        return stay(ctx);
    }
    let next_byte = host.actor(slot).map(|a| a.read_param(0)).unwrap_or(0xFF);
    if next_byte == 0x00 || next_byte == 0xFF {
        // An empty / exhausted stream: retail stages the zero it read and
        // falls to `0x1F` on the same step; nothing plays and nothing is
        // owed, so the gate is released here. The cursor is left where it
        // is - retail never rewinds it in this band (ActionSeed zeroes it,
        // `0x801E2CF0`; the `0x1F -> 0x20` edge parks it at `0xFF`).
        if let Some(actor) = host.actor_mut(slot) {
            actor.flag_bits.clear(ActorFlags::ADVANCE_DONE);
        }
        return transition(ctx, ActionState::AttackRecovery);
    }
    // The stage site (`0x801E3734..0x801E3764`): cursor post-increment by
    // exactly one, `+0x1DC |= 2` (the one-per-clip latch, bit 1 - the
    // "commit at the event frame" request the anim tick consumes), and the
    // byte into `+0x1DA`. Retail's stream alphabet for a party attack is
    // direction swings `0x0C..0x0F`, the art starters `0x19`/`0x1A` and the
    // art action constants `0x1B+` (`FUN_801EED1C` writes them inline, see
    // `docs/subsystems/battle-action.md` § A Tactical Art is an ordinary
    // attack-band action); a monster's bytes are archive entry indices. The
    // byte itself names the clip, and the anim commit latches it into
    // `+0x1DB` for the per-art attack camera - and the hit-event driver -
    // to key on.
    if let Some(actor) = host.actor_mut(slot) {
        actor.queued_anim = next_byte;
        actor.flag_bits.set(ActorFlags::ADVANCE_DONE);
        actor.strike_index = actor.strike_index.saturating_add(1);
    }
    // The terminator is tested at the **new** cursor on the same step
    // (`0x801E3998..0x801E39AC`, `0x00` routing to `0x1F` at `0x801E3A7C`):
    // the last byte is staged and the band leaves the loop together, so the
    // recovery wait below is what holds until that last clip commits. (The
    // `0x19` Miracle-continuation refill at `0x801E3A20..0x801E3A64` is not
    // ported.)
    let exhausted = host.actor(slot).map(|a| a.read_param(0)).unwrap_or(0) == 0;
    if exhausted {
        return transition(ctx, ActionState::AttackRecovery);
    }
    stay(ctx)
}

pub(super) fn attack_recovery<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.pose(slot, Pose::Recover);
    // `0x1F` waits for `+0x1DC` bit 1 to clear (`0x801E3AEC..0x801E3AF8`) -
    // i.e. for the last staged byte's clip to commit - then stages idle over
    // it (`sb zero,0x1da` at `0x801E3B04`; the anim tick commits that at
    // the clip's own boundary) and parks the cursor at `0xFF`
    // (`0x801E3B14..0x801E3B1C`) on its way to `0x20`.
    let advance_done = host
        .actor(slot)
        .map(|a| a.flag_bits.has(ActorFlags::ADVANCE_DONE))
        .unwrap_or(false);
    if advance_done {
        return stay(ctx);
    }
    if let Some(actor) = host.actor_mut(slot) {
        actor.queued_anim = 0;
        actor.strike_index = STRIKE_CURSOR_PARKED;
    }
    transition(ctx, ActionState::AttackReturn)
}

pub(super) fn attack_return<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.pose(slot, Pose::Recover);
    // Counter-attack window is gated by both context flags.
    if ctx.counter_attack_a != 0 && ctx.counter_attack_b != 0 {
        // Counter-attack swap: advance the turn cursor past the counterer and
        // route back into AttackChain (retail `0x801E36D0`). Engines drive the
        // actual swap.
        ctx.turn_cursor = ctx.turn_cursor.saturating_add(1);
        return transition(ctx, ActionState::AttackChain);
    }
    transition(ctx, ActionState::DoneCleanup)
}

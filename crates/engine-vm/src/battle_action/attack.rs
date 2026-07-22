//! Physical-attack band of the battle-action state machine (face / short-step / windup / chain).

use super::*;
use crate::battle_formulas::{arms_resolver_admits, arms_weapon_atk_fold};

// --- attack band ------------------------------------------------------------

pub(super) fn attack_face<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let actor_slot = ctx.active_actor;
    let target_slot = host.actor(actor_slot).map(|a| a.active_target).unwrap_or(0);
    host.pose(actor_slot, Pose::Idle);
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
        ActionState::AttackWindup
    };
    transition(ctx, next)
}

pub(super) fn attack_windup<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.pose(slot, Pose::Idle);
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
    let range = host.range_check(slot, target);
    if range != 0 {
        return stay(ctx);
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
    let range = host.range_check(slot, target);
    if range != 0 {
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
    // byte as the queued anim and fire damage.
    let slot = ctx.active_actor;
    // Strike pacing gate: while ADVANCE_DONE is still set the previous
    // staged swing is in flight - skip the byte read and hold (the anim
    // system clears the bit when the staged clip finishes; for the engine
    // that's `World::tick_battle_animations`' staged-clip end handling, or
    // an immediate clear when the actor carries no clips).
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
        if let Some(actor) = host.actor_mut(slot) {
            actor.strike_index = 0;
            actor.flag_bits.clear(ActorFlags::ADVANCE_DONE);
        }
        return transition(ctx, ActionState::AttackRecovery);
    }
    let (target, strike_index_pre, character, chosen_art) = host
        .actor(slot)
        .map(|a| (a.active_target, a.strike_index, a.character, a.chosen_art))
        .unwrap_or((0, 0, legaia_art::Character::default(), None));
    if let Some(actor) = host.actor_mut(slot) {
        actor.queued_anim = next_byte;
        actor.flag_bits.set(ActorFlags::ADVANCE_DONE);
        actor.strike_index = actor.strike_index.saturating_add(1);
    }
    // Arms execution-time weapon fold. Retail runs this in FUN_801EC3E4,
    // which SCUS calls at 0x800478A0 once per committed arms command - a
    // separate call edge from FUN_801E295C, not a subroutine of it. The port
    // drives it from here because this is the engine's equivalent point: the
    // strike loop is where one recorded command byte is consumed and staged.
    // The head guards are evaluated against the same state retail reads
    // (ctx[7], the command byte, the actor's +0x1F4 cursor, the slot), with
    // this strike as the record's last step.
    // PORT: FUN_801EC3E4 (call site for the ATK-working weapon fold)
    // REF: FUN_801E295C (the state machine this call site sits in)
    let (input_cursor, current_command) = host
        .actor(slot)
        .map(|a| (a.input_cursor, a.current_anim))
        .unwrap_or((0, 0));
    if arms_resolver_admits(ctx.action_state, next_byte, 0, 1, input_cursor, slot) {
        let bonuses = host.equip_attack_bonuses(slot);
        if let Some(delta) = arms_weapon_atk_fold(current_command, &bonuses)
            && let Some(actor) = host.actor_mut(slot)
        {
            actor.atk_working = actor.atk_working.wrapping_add(delta);
        }
    }
    // Fire swing-apex damage for this strike. (Retail seeds this byte stream
    // at action start via FUN_801eed1c - the party action-stream setup hook
    // that copies the entered direction commands, strips status-sealed
    // directions, and rewrites matched arts into action constants; the
    // stream bytes here are direction swings `0x0C..0x0F`, art starters
    // `0x19`/`0x1A`, and art constants `0x1B+`.)
    //
    // When the actor has a `chosen_art` set and the host returns an
    // [`legaia_art::ArtRecord`] for it, also dispatch
    // [`BattleActionHost::apply_art_strike`] with the per-strike
    // power/timing/effect/hit-cue values. Generic-attack callers ignore
    // this hook (default no-op); callers wired up to art data drive HP
    // deduction, status application, and SFX timing from it.
    if let Some(art) = chosen_art {
        let info = host.art_record(character, art).map(|rec| {
            let idx = strike_index_pre as usize;
            ArtStrikeInfo {
                strike_index: strike_index_pre,
                anim_byte: next_byte,
                actor_slot: slot,
                target_slot: target,
                character,
                art,
                power: rec.power.get(idx).copied(),
                dmg_timing: rec.dmg_timing.get(idx).copied(),
                enemy_effect: rec.enemy_effect,
                hit_cue: rec.hit_cues.get(idx).copied(),
            }
        });
        if let Some(info) = info {
            host.apply_art_strike(info);
        }
    }
    host.apply_damage(next_byte, 0, target, slot);
    stay(ctx)
}

pub(super) fn attack_recovery<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.pose(slot, Pose::Recover);
    let advance_done = host
        .actor(slot)
        .map(|a| a.flag_bits.has(ActorFlags::ADVANCE_DONE))
        .unwrap_or(false);
    if advance_done {
        return stay(ctx);
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
        // Counter-attack swap: bump the active actor's queue counter and
        // route back into AttackChain. Engines drive the actual swap.
        if let Some(actor) = host.actor_mut(slot) {
            actor.action_queue_counter = actor.action_queue_counter.saturating_add(1);
        }
        return transition(ctx, ActionState::AttackChain);
    }
    transition(ctx, ActionState::DoneCleanup)
}

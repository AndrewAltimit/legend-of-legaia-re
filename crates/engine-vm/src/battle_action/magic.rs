//! Magic / item cast band of the battle-action state machine.

use super::*;

// --- magic / item band ------------------------------------------------------

pub(super) fn magic_cast_begin<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    // Item-target re-route checks. Categories 8 and 9 are intermediate
    // routing categories.
    let category = host
        .actor(slot)
        .map(|a| ActionCategory::from_byte(a.action_category))
        .unwrap_or(ActionCategory::Magic);
    if let Some(actor) = host.actor_mut(slot) {
        match category {
            ActionCategory::ItemRetargetA => {
                actor.active_target = ctx.item_target_a.saturating_sub(1);
            }
            ActionCategory::ItemRetargetB => {
                actor.active_target = ctx.item_target_b;
            }
            _ => {}
        }
    }
    // Stage frame timer for pre-cast wait.
    ctx.frame_timer = 0x14;

    // For party, fire spell-name HUD label.
    let party_count = host.party_count();
    if slot < party_count {
        host.ui_element(0x4C, 0);
    }

    // Capture-spell route?
    let spell_id = host.actor(slot).map(|a| a.params[0]).unwrap_or(0);
    if host.is_capture_spell(spell_id) {
        host.load_capture_archive(spell_id);
        return transition(ctx, ActionState::MagicCaptureBranch);
    }

    // Compute MP cost with the character ability-bit modifier (Half 0x20 takes
    // priority over Quarter 0x10; see battle_formulas + the state-0x28 dump).
    let mp_cost = host.spell_mp_cost(spell_id);
    let bits = host.character_ability_bits(slot);
    let modifier = crate::battle_formulas::MpCostModifier::from_ability_flags(bits);
    let cost = crate::battle_formulas::mp_cost_after_ability_bits(mp_cost as u16, modifier);
    if let Some(actor) = host.actor_mut(slot) {
        actor.mp = actor.mp.saturating_sub(cost);
        actor.last_mp_cost = cost;
    }

    transition(ctx, ActionState::MagicPreCastWait)
}

pub(super) fn magic_pre_cast_wait<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    if !tick_frame_timer(host, ctx) {
        return stay(ctx);
    }
    let slot = ctx.active_actor;
    let party_count = host.party_count();
    let spell_id = host.actor(slot).map(|a| a.params[0]).unwrap_or(0);
    if slot < party_count {
        host.spell_anim_trigger(slot, spell_id);
    }

    // Summon-route check.
    let sub_route = host.actor(slot).map(|a| a.sub_route).unwrap_or(0);
    if sub_route == 9 {
        return transition(ctx, ActionState::SummonInvoke);
    }

    // Pull next anim from params.
    let next_byte = host.actor(slot).map(|a| a.read_param(0)).unwrap_or(0xFF);
    if next_byte == 0xFF {
        return transition(ctx, ActionState::DoneCleanup);
    }
    transition(ctx, ActionState::MagicAnimChain)
}

pub(super) fn magic_anim_chain<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    let next_byte = host.actor(slot).map(|a| a.read_param(0)).unwrap_or(0xFF);
    if next_byte != 0xFF {
        if let Some(actor) = host.actor_mut(slot) {
            actor.queued_anim = next_byte;
            actor.spell_iter = 1;
            actor.strike_index = actor.strike_index.saturating_add(1);
        }
        host.spell_anim_sustain(slot, next_byte);
        return stay(ctx);
    }
    // Terminator hit.
    if let Some(actor) = host.actor_mut(slot) {
        if actor.strike_index == 2 {
            actor.spell_iter = 1;
        }
        actor.flag_bits.set(ActorFlags::EXIT);
    }
    transition(ctx, ActionState::MagicSustain)
}

pub(super) fn magic_sustain<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    let queued = host.actor(slot).map(|a| a.queued_anim).unwrap_or(0);
    host.spell_anim_sustain(slot, queued);
    let iter_done = host.actor(slot).map(|a| a.spell_iter == 0).unwrap_or(false);
    if !iter_done {
        return stay(ctx);
    }
    if let Some(actor) = host.actor_mut(slot) {
        actor.flag_bits.set(ActorFlags::EXIT);
    }
    transition(ctx, ActionState::MagicHitLoop)
}

pub(super) fn magic_hit_loop<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    let queued = host.actor(slot).map(|a| a.queued_anim).unwrap_or(0);
    host.spell_anim_sustain(slot, queued);
    // Exit when current anim is 0 OR hit_counter >= bound (and bound != 0).
    let (current, bound) = host
        .actor(slot)
        .map(|a| (a.current_anim, a.hit_count_bound))
        .unwrap_or((0, 0));
    let exit = current == 0 || (bound != 0 && ctx.hit_counter >= bound);
    if !exit {
        return stay(ctx);
    }
    transition(ctx, ActionState::MagicRecovery)
}

pub(super) fn magic_recovery<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    if ctx.magic_recovery_gate != 0 {
        return stay(ctx);
    }
    let slot = ctx.active_actor;
    if let Some(actor) = host.actor_mut(slot) {
        // Clear actor[+0x176] - modeled as resetting hit_count_bound + a
        // dummy field. Engines that need finer modeling can override the
        // host trait.
        actor.hit_count_bound = 0;
    }
    transition(ctx, ActionState::MagicExit)
}

pub(super) fn magic_exit<H: BattleActionHost + ?Sized>(
    host: &mut H,
    _ctx: &mut BattleActionCtx,
) -> StepOutcome {
    if _ctx.magic_exit_gate != 0 {
        return stay(_ctx);
    }
    host.screen_shake(0);
    transition(_ctx, ActionState::DoneCleanup)
}

// --- magic-capture branch ---------------------------------------------------

pub(super) fn magic_capture_branch<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.pose(slot, Pose::Idle);
    if !host.sound_bank_ready(1) {
        return stay(ctx);
    }
    let capture_idx = host.actor(slot).map(|a| a.params[0]).unwrap_or(0);
    host.load_capture_archive(capture_idx);
    transition(ctx, ActionState::MagicCaptureFade)
}

pub(super) fn magic_capture_fade<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    if ctx.counter_attack_a != 0 {
        host.ramp_brightness(75);
    }
    if !host.previous_action_cleared(1) {
        return stay(ctx);
    }
    transition(ctx, ActionState::MagicCapturePhase2)
}

pub(super) fn magic_capture_phase2<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    host.ramp_brightness(75);
    transition(ctx, ActionState::MagicCaptureFinalize)
}

pub(super) fn magic_capture_finalize<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.pose(slot, Pose::Idle);
    // Ensure all 8 slots are settled - alive with non-zero "+0x4" or non-`8`
    // current_anim. We model as: every alive actor has current_anim != 8.
    let total = host.slot_count();
    let stable = (0..total).all(|s| {
        host.actor(s)
            .map(|a| a.liveness == 0 || a.current_anim != 8)
            .unwrap_or(true)
    });
    if !stable {
        return stay(ctx);
    }
    // Reset per-actor render flag.
    for s in 0..total {
        if let Some(a) = host.actor_mut(s) {
            a.render_flag = 0;
        }
    }
    transition(ctx, ActionState::DoneCleanup)
}

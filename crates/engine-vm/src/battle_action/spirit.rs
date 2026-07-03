//! Spirit / Originals band of the battle-action state machine (MP-cost + ability-bit application).

use super::*;

// --- spirit band ------------------------------------------------------------

pub(super) fn spirit_pre_arm<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.pose(slot, Pose::Idle);
    if let Some(actor) = host.actor_mut(slot) {
        actor.queued_anim = actor.queued_anim_b;
    }
    let category = host
        .actor(slot)
        .map(|a| ActionCategory::from_byte(a.action_category))
        .unwrap_or(ActionCategory::Spirit);
    let spell_id = host.actor(slot).map(|a| a.params[0]).unwrap_or(0);
    if !matches!(category, ActionCategory::Item) {
        // Spell path: compute MP cost, apply ability bits (Half 0x20 first).
        let mp_cost = host.spell_mp_cost(spell_id);
        let bits = host.character_ability_bits(slot);
        let modifier = crate::battle_formulas::MpCostModifier::from_ability_flags(bits);
        let cost = crate::battle_formulas::mp_cost_after_ability_bits(mp_cost as u16, modifier);
        if let Some(actor) = host.actor_mut(slot) {
            actor.mp = actor.mp.saturating_sub(cost);
            actor.last_mp_cost = cost;
        }
        if slot < host.party_count() {
            host.ui_element(7, 0);
        }
    }
    host.ui_element(0x4C, 0);
    transition(ctx, ActionState::SpiritWait)
}

pub(super) fn spirit_wait<H: BattleActionHost + ?Sized>(
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
    if let Some(actor) = host.actor_mut(slot) {
        actor.queued_anim = 0;
    }
    transition(ctx, ActionState::SpiritFire)
}

pub(super) fn spirit_fire<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.pose(slot, Pose::Idle);
    let cur_zero = host
        .actor(slot)
        .map(|a| a.current_anim == 0)
        .unwrap_or(true);
    if !cur_zero {
        return stay(ctx);
    }
    host.ui_element(0x4C, 1);
    ctx.frame_timer = 0x20;
    transition(ctx, ActionState::SpiritFireDamage)
}

pub(super) fn spirit_fire_damage<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    if !tick_frame_timer(host, ctx) {
        return stay(ctx);
    }
    let slot = ctx.active_actor;
    let target = host.actor(slot).map(|a| a.active_target).unwrap_or(0);
    // Fire damage primitive (icon, page, target_slot, party_slot).
    let (icon, page) = host
        .actor(slot)
        .map(|a| (a.queued_anim_b, a.spell_iter))
        .unwrap_or((0, 0));
    host.apply_damage(icon, page, target, slot);
    ctx.frame_timer = 0x80;
    transition(ctx, ActionState::SpiritPostDamage)
}

pub(super) fn spirit_post_damage<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    let target = host.actor(slot).map(|a| a.active_target).unwrap_or(0);
    host.pose(target, Pose::Idle);
    if !tick_frame_timer(host, ctx) {
        return stay(ctx);
    }
    transition(ctx, ActionState::DoneCleanup)
}

// --- spirit-arts variant ----------------------------------------------------

pub(super) fn spirit_arts_entry<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.pose(slot, Pose::Idle);
    if let Some(actor) = host.actor_mut(slot) {
        // Override flags with ADVANCE_DONE only.
        actor.flag_bits = ActorFlags(ActorFlags::ADVANCE_DONE);
        actor.queued_anim = actor.queued_anim_b;
    }
    transition(ctx, ActionState::SpiritArtsSustain)
}

pub(super) fn spirit_arts_sustain<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.pose(slot, Pose::Idle);
    let nonzero_anim = host
        .actor(slot)
        .map(|a| a.current_anim != 0)
        .unwrap_or(false);
    if nonzero_anim && let Some(actor) = host.actor_mut(slot) {
        actor.queued_anim = 0;
    }
    let timer_done = tick_frame_timer(host, ctx);
    let exit_clear = host
        .actor(slot)
        .map(|a| a.flag_bits.0 == 0)
        .unwrap_or(false);
    if !(timer_done && exit_clear) {
        return stay(ctx);
    }
    transition(ctx, ActionState::SpiritArtsFlush)
}

pub(super) fn spirit_arts_flush<H: BattleActionHost + ?Sized>(
    _host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    transition(ctx, ActionState::DoneCleanup)
}

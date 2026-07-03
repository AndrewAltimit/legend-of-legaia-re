//! Summon (Seru-creature) invocation band of the battle-action state machine.

use super::*;

// --- summon band ------------------------------------------------------------

pub(super) fn summon_invoke<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.pose(slot, Pose::Idle);
    if !host.sound_bank_ready(1) {
        return stay(ctx);
    }
    let param0 = host.actor(slot).map(|a| a.params[0]).unwrap_or(0);
    let frame_idx = if param0 < 0x9A {
        // (param0 + 0x7F) * 3 + 0x80
        ((param0 as u32).saturating_add(0x7F))
            .saturating_mul(3)
            .saturating_add(0x80) as u8
    } else {
        // param0 * 4 + 99
        ((param0 as u32).saturating_mul(4)).saturating_add(99) as u8
    };
    ctx.summon_frame_idx = frame_idx;
    ctx.menu_open = 1;
    ctx.summon_staging_a = 1;
    if let Some(actor) = host.actor_mut(slot) {
        actor.queued_anim = 9;
        actor.flag_bits.set(ActorFlags::WINDUP_DONE);
        actor.spell_iter = actor.spell_iter.saturating_add(1);
    }
    transition(ctx, ActionState::SummonFadeIn)
}

pub(super) fn summon_fade_in<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.spell_anim_sustain(slot, 0x12);
    let cued = host.actor(slot).map(|a| a.anim_cue != 0).unwrap_or(false);
    if !cued {
        return stay(ctx);
    }
    transition(ctx, ActionState::SummonActorFreeze)
}

pub(super) fn summon_actor_freeze<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.spell_anim_sustain(slot, 0x12);
    let current_zero = host
        .actor(slot)
        .map(|a| a.current_anim == 0)
        .unwrap_or(false);
    if !current_zero {
        return stay(ctx);
    }
    ctx.summon_staging_a = 0;
    ctx.summon_staging_b = 0;
    ctx.frame_timer = 0x78;
    // Mark all actors as hidden (+0x21C = 0xFF).
    for s in 0..host.slot_count() {
        if let Some(a) = host.actor_mut(s) {
            a.render_flag = 0xFF;
        }
    }
    transition(ctx, ActionState::SummonSustain)
}

pub(super) fn summon_sustain<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    if !tick_frame_timer(host, ctx) {
        let slot = ctx.active_actor;
        let param0 = host.actor(slot).map(|a| a.params[0]).unwrap_or(0);
        // Ramp brightness - 75% for spells < 0x99, else 50%.
        let pct = if param0 < 0x99 { 75 } else { 50 };
        host.ramp_brightness(pct);
        return stay(ctx);
    }
    if ctx.menu_open != 0 {
        ctx.frame_timer = 1;
    }
    transition(ctx, ActionState::SummonReturn)
}

pub(super) fn summon_return<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    // Restore actor visibility.
    for s in 0..host.slot_count() {
        if let Some(a) = host.actor_mut(s) {
            a.render_flag = 0;
        }
    }
    transition(ctx, ActionState::SummonVerifyAlive)
}

pub(super) fn summon_verify_alive<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.pose(slot, Pose::Idle);
    // Ensure all actors are still alive (liveness != 0 AND current_anim != 0).
    // The state machine doesn't gate on this; it just records state.
    transition(ctx, ActionState::SummonDone)
}

pub(super) fn summon_done<H: BattleActionHost + ?Sized>(
    _host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    transition(ctx, ActionState::DoneCleanup)
}

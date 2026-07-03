use super::*;

// --- run / capture band -----------------------------------------------------

pub(super) fn run_begin<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    ctx.frame_timer = 0x3C;
    host.ui_element(0x43, 0);
    // PORT: FUN_801E295C case 0x64 (successful-escape branch). When the run
    // roll succeeded (retail tests `_DAT_8007726C != ctx + 0x189`; the port
    // carries the outcome on `multi_cast_gate`, consumed later by RunWait),
    // retail walks the party slots (`i < ctx[+0]`) and floors every actor's
    // live HP `+0x14C` at 1 - downed (and, per the published behaviour,
    // petrified) members leave the battle alive. `+0x14C` maps to
    // `BattleActor::liveness` here. Engines additionally clear the Stone
    // status via `status_effects::StatusEffectTracker::cure_stone_on_escape`
    // when the battle ends `Escaped`.
    if ctx.multi_cast_gate != 0 {
        for slot in 0..host.party_count() {
            if let Some(actor) = host.actor_mut(slot)
                && actor.liveness == 0
            {
                actor.liveness = 1;
            }
        }
    }
    transition(ctx, ActionState::RunWait)
}

pub(super) fn run_wait<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    if !tick_frame_timer(host, ctx) {
        return stay(ctx);
    }
    // Retail 0x65 branches on the run outcome: a successful escape routes to
    // the 0x66 teardown (fade out + battle-end signal), a failed run routes
    // back to the Done band - the action is consumed and the battle
    // continues. The retail driver decides via a global the run roll set;
    // the port carries the outcome on `multi_cast_gate` (non-zero = the
    // escape succeeded).
    if ctx.multi_cast_gate != 0 {
        ctx.multi_cast_gate = 0;
        return transition(ctx, ActionState::RunEscape);
    }
    transition(ctx, ActionState::DoneCleanup)
}

pub(super) fn run_escape<H: BattleActionHost + ?Sized>(
    host: &mut H,
    _ctx: &mut BattleActionCtx,
) -> StepOutcome {
    host.battle_end(BattleEndCause::Escaped);
    StepOutcome::BattleComplete
}

/// Captured-monster takedown - PORT: FUN_801E7824 (battle overlay 0898,
/// `ghidra/scripts/funcs/overlay_battle_action_801e7824.txt`).
///
/// Applied to the captured monster's slot (the state-0x68 arm calls it with
/// `ctx[+0x13]`, the active actor). Per the dump:
///
/// - queued anim (`+0x1DA`) = the monster-record action-table pick
///   (`FUN_80050E2C(rec + 0x4C, 1, rec[0x4A])` - surfaced as
///   [`BattleActionHost::capture_anim`]);
/// - the per-actor flag byte `+0x1DC` is **incremented** (raw `+1`, not a
///   bit set);
/// - HP-bar display (`+0x172`) and live HP (`+0x14C`) both zeroed - the
///   monster leaves the battle;
/// - facing angle (`+0x46`) zeroed; target byte (`+0x1DD`) set to `8`
///   ("all");
/// - the run-side banner takes over: retail bumps the capture counter
///   `+0x227` (unmodeled), points `_DAT_8007726C` at the ctx name buffer,
///   copies the monster's name into it (`FUN_8003CA78` / `FUN_8003CAC4` -
///   host-side rendering concerns), and opens the run UI banner
///   (`FUN_801D8DE8(0x43, 0)` - surfaced as `ui_element(0x43, 0)`).
pub(super) fn capture_takedown<H: BattleActionHost + ?Sized>(host: &mut H, slot: u8) {
    let anim = host.capture_anim(slot);
    if let Some(actor) = host.actor_mut(slot) {
        if let Some(anim) = anim {
            actor.queued_anim = anim;
        }
        actor.flag_bits.0 = actor.flag_bits.0.wrapping_add(1);
        actor.hp = 0;
        actor.hp_display = Some(0);
        actor.liveness = 0;
        actor.facing_angle = 0;
        actor.active_target = 8;
    }
    host.ui_element(0x43, 0);
}

pub(super) fn capture_start<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    let r = host.rng();
    ctx.combo_timer = ctx.combo_timer.wrapping_add(0x780 + (r % 2) as i16 * 0x80);
    host.pose(slot, Pose::Idle);
    // REF: FUN_801E7824 - the state-0x68 arm removes the captured monster
    // between the pose write and the battle-order recompute.
    capture_takedown(host, slot);
    host.recompute_battle_order();
    ctx.frame_timer = 0x1E;
    transition(ctx, ActionState::CaptureWait)
}

pub(super) fn capture_wait<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.pose(slot, Pose::Idle);
    if !tick_frame_timer(host, ctx) {
        return stay(ctx);
    }
    ctx.frame_timer = 0x5A;
    if let Some(actor) = host.actor_mut(slot) {
        actor.capture_state = 2;
        actor.render_flag = 2;
    }
    transition(ctx, ActionState::CaptureSustain)
}

pub(super) fn capture_sustain<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    if ctx.menu_open != 0 && ctx.frame_timer > 1 {
        ctx.frame_timer = 1;
    }
    if !tick_frame_timer(host, ctx) {
        return stay(ctx);
    }
    ctx.frame_timer = 0x3C;
    host.ui_element(0x43, 1);
    let slot = ctx.active_actor;
    if let Some(actor) = host.actor_mut(slot) {
        actor.action_queue_counter = 0;
    }
    host.pose(0, Pose::Defeat);
    transition(ctx, ActionState::CaptureEnd)
}

pub(super) fn capture_end<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    host.pose(0, Pose::Defeat);
    if !tick_frame_timer(host, ctx) {
        return stay(ctx);
    }
    transition(ctx, ActionState::EndOfAction)
}

// --- terminal -----------------------------------------------------------------

pub(super) fn idle_hold<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    host.pose(ctx.active_actor, Pose::Recover);
    stay(ctx)
}

pub(super) fn battle_complete<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let _ = ctx;
    // The retail handler increments a battle-count and calls
    // `func_0x801F45A4` (battle teardown). For the port we surface this as
    // BattleComplete and let the caller drive overlay unload.
    host.battle_end(BattleEndCause::MonsterWipe);
    StepOutcome::BattleComplete
}

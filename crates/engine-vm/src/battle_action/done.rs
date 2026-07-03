//! Action-teardown ("done") band of the battle-action state machine (recoil reset, exit pose, fade-down timer).

use super::*;

// --- done band --------------------------------------------------------------

pub(super) fn done_cleanup<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.recompute_battle_order();

    // Reset action_recoil based on category.
    let category = host
        .actor(slot)
        .map(|a| ActionCategory::from_byte(a.action_category))
        .unwrap_or(ActionCategory::Attack);
    let recoil = if matches!(category, ActionCategory::Spirit) {
        0x20
    } else {
        8
    };
    if let Some(actor) = host.actor_mut(slot) {
        actor.action_recoil = recoil;
        actor.flag_bits.set(ActorFlags::EXIT);
    }
    // Set frame timer for fade-down (0x3C default; 0x96 if shake).
    ctx.frame_timer = 0x3C;

    // Per-category pose: run → screen-shake; attack → pose 8; otherwise idle.
    match category {
        ActionCategory::Run => host.screen_shake(0x500),
        ActionCategory::Attack => host.pose(slot, Pose::Recover),
        _ => host.pose(slot, Pose::Idle),
    }

    transition(ctx, ActionState::DoneFadeDown)
}

/// "Any HP-bar drain still animating?" settle check - PORT: FUN_801E7250
/// (battle overlay 0898, `ghidra/scripts/funcs/overlay_battle_action_801e7250.txt`).
///
/// Retail dispatches on the active actor's target byte (`+0x1DD`):
///
/// - target `0..=2` (party slot): pending while that actor's live HP
///   (`+0x14C`) differs from its HP-bar display value (`+0x172`);
/// - target `3..=7` (monster slot): never pending (returns 0 immediately -
///   the `2 < bVar1` early-out);
/// - target `8` ("all"): scans every actor slot up to the battle actor
///   count, pending if any pair differs;
/// - target `> 8`: never pending.
///
/// The engine models the retail `+0x14C`-vs-`+0x172` pair as
/// [`BattleActor::hp`] vs [`BattleActor::hp_display`] (`None` = settled).
pub(super) fn hp_bar_drain_pending<H: BattleActionHost + ?Sized>(
    host: &H,
    ctx: &BattleActionCtx,
) -> bool {
    let pending = |slot: u8| -> bool {
        host.actor(slot)
            .map(|a| a.hp_display.is_some_and(|shown| shown != a.hp))
            .unwrap_or(false)
    };
    let target = host
        .actor(ctx.active_actor)
        .map(|a| a.active_target)
        .unwrap_or(8);
    match target {
        0..=2 => pending(target),
        8 => (0..host.slot_count()).any(pending),
        _ => false,
    }
}

pub(super) fn done_fade_down<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    // REF: FUN_801E7250 - retail's state-0x51 arm freezes the `+0x6D8`
    // countdown while the HP-bar drain check reports a mismatch
    // (`if (iVar11 == 0) { decrement }` at the `0x801E6044` callsite).
    if hp_bar_drain_pending(host, ctx) {
        return stay(ctx);
    }
    if !tick_frame_timer(host, ctx) {
        return stay(ctx);
    }
    if ctx.menu_open != 0 {
        return stay(ctx);
    }
    if ctx.multi_cast_gate == 0 {
        return transition(ctx, ActionState::EndOfAction);
    }
    transition(ctx, ActionState::DoneMultiCast)
}

pub(super) fn done_multi_cast<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.pose(slot, Pose::Recover);
    if !tick_frame_timer(host, ctx) {
        return stay(ctx);
    }
    ctx.multi_cast_gate = 0;
    transition(ctx, ActionState::EndOfAction)
}

pub(super) fn end_of_action<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let party_count = host.party_count();
    let total = host.slot_count();

    // Count alive party + monsters.
    let mut party_alive = 0u8;
    let mut monsters_alive = 0u8;
    for s in 0..total {
        let alive = host.actor(s).map(|a| a.liveness != 0).unwrap_or(false);
        if !alive {
            continue;
        }
        if s < party_count {
            party_alive += 1;
        } else {
            monsters_alive += 1;
        }
    }

    if party_alive == 0 {
        host.battle_end(BattleEndCause::PartyWipe);
        return StepOutcome::BattleComplete;
    }
    if monsters_alive == 0 {
        host.battle_end(BattleEndCause::MonsterWipe);
        return StepOutcome::BattleComplete;
    }

    // Pick next active actor: bump active actor's queue counter; if still
    // less than (alive_count), restart at PreActionWait. Otherwise → battle
    // complete (BattleComplete state which then calls battle_end).
    let bumped = if let Some(actor) = host.actor_mut(ctx.active_actor) {
        actor.action_queue_counter = actor.action_queue_counter.saturating_add(1);
        actor.action_queue_counter
    } else {
        0
    };
    let alive_total = party_alive + monsters_alive;
    if bumped < alive_total {
        return transition(ctx, ActionState::PreActionWait);
    }
    transition(ctx, ActionState::BattleComplete)
}

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

    rearm_action_gauge(host, ctx);

    transition(ctx, ActionState::DoneFadeDown)
}

/// Re-arm the per-actor command-gauge slots at the tail of `DoneCleanup`.
///
/// Retail's state-`0x50` body falls through all three `+0x1DE` arms into a
/// shared tail that stamps the next state, clears `ctx[+0x6]` and then
/// `jal`s the re-arm (`overlay_battle_action_801e295c.txt` `0x801E5F64`,
/// unconditional on that path). The kernel itself is
/// [`crate::battle_gauge_rearm::rearm_gauge`]; this is the call site plus the
/// two bridges retail reads through globals:
///
/// * the **gate** input - a party slot (`< 3`) is gated on the actor's
///   `+0x1D9` staged id being below `0x10`, a monster slot on the `+0x87`
///   flag of the art record its staged id resolves to
///   ([`BattleActionHost::staged_art_record_flag`]);
/// * the **slot array** - retail walks the seven actor-pointer-table entries
///   writing `+0x21C` / `+0x21D`, which the port carries as
///   [`BattleActor::render_flag`] / [`BattleActor::impact_step`].
///
/// The context byte `+0x243` ([`BattleActionCtx::gauge_rearm_latch`]) is
/// cleared only when the gate passed - retail's store sits past the two
/// early-outs.
///
/// PORT: FUN_801E93C8 (call site; kernel in `battle_gauge_rearm`)
fn rearm_action_gauge<H: BattleActionHost + ?Sized>(host: &mut H, ctx: &mut BattleActionCtx) {
    use crate::battle_gauge_rearm::{GAUGE_SLOTS, GaugeSlots, StagedAction, rearm_gauge};

    let slot = ctx.active_actor;
    let staged_id = host.actor(slot).map(|a| a.current_anim).unwrap_or(0);
    let staged = if slot < 3 {
        StagedAction::Party {
            action_id: staged_id,
        }
    } else {
        StagedAction::Monster {
            record_flag: host.staged_art_record_flag(slot, staged_id),
        }
    };

    let mut slots = GaugeSlots::default();
    for i in 0..GAUGE_SLOTS {
        if let Some(a) = host.actor(i as u8) {
            slots.latch[i] = a.render_flag;
            slots.arm_width[i] = a.impact_step;
        }
    }
    if !rearm_gauge(staged, &mut slots) {
        return;
    }
    for i in 0..GAUGE_SLOTS {
        if let Some(a) = host.actor_mut(i as u8) {
            a.render_flag = slots.latch[i];
            a.impact_step = slots.arm_width[i];
        }
    }
    ctx.gauge_rearm_latch = 0;
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

/// Monster-wipe victory arm of the end-of-action gate.
///
/// Retail (`overlay_battle_action_801e295c.txt`, `0x801E6688..0x801E6790`)
/// fixes up the acting slot before staging the win pose:
///
/// - `0x801E6688/0x801E6690`: `lhu a0,0x14c(s3)` / `bne a0,zero,0x801E6728` -
///   only a **dead** acting actor triggers the re-pick;
/// - `0x801E66A4..0x801E6724`: rejection-sample `rand % party_count` until a
///   slot with `+0x14C != 0` and `(+0x16E & 0x404) == 0` comes up (the loop
///   back-edges at `0x801E670C` / `0x801E6720` are unbounded);
/// - `0x801E6728..0x801E676C`: formation override - first monster id `0xB3`
///   forces slot `2`, `0xB4` forces slot `1` (the Songi battles);
/// - `0x801E6770..0x801E6790`: read `DAT_8007BD10[slot]` (the 3-byte party
///   roster) and arm the win-pose stream `FUN_80055B4C(char*3 - 1)`.
///
/// Retail's alive-skip is sound only because the scheduler (`FUN_801DABA4`)
/// and the wipe scan agree on the same living predicate (`+0x14C != 0 &&
/// !(+0x16E & 0x4)`) - so an *alive* acting actor at monster-wipe victory is
/// always a party slot. The enemy-ally charm widen (mask `0x384`) breaks
/// that agreement: a living charmed monster can be the acting actor here,
/// and retail then indexes the roster out of bounds (the charm battle
/// softlock - see `docs/subsystems/battle.md`). The port therefore triggers
/// the re-pick whenever the acting slot is **not a living party slot**, and
/// picks uniformly among eligible slots instead of rejection-sampling, so it
/// cannot spin.
fn victory_pose_fixup<H: BattleActionHost + ?Sized>(host: &mut H, ctx: &mut BattleActionCtx) -> u8 {
    let party_count = host.party_count();
    let acting = ctx.active_actor;
    // Retail keeps an alive acting actor unconditionally; the port also
    // requires it to be a party slot (the corrected invariant).
    let keep = acting < party_count && host.actor(acting).is_some_and(|a| a.liveness != 0);
    let mut slot = if keep {
        acting
    } else {
        // Uniform pick over living, non-0x404 party slots - the same
        // distribution retail's rejection loop converges to, but bounded.
        let eligible: Vec<u8> = (0..party_count)
            .filter(|&s| {
                host.actor(s)
                    .is_some_and(|a| a.liveness != 0 && a.field_flags & 0x404 == 0)
            })
            .collect();
        if eligible.is_empty() {
            // Retail's rejection loop would spin forever here (every living
            // party member 0x404-flagged). Bounded fallback: first living
            // party slot - one exists, the party-wipe branch already ran.
            (0..party_count)
                .find(|&s| host.actor(s).is_some_and(|a| a.liveness != 0))
                .unwrap_or(0)
        } else {
            eligible[host.rng() as usize % eligible.len()]
        }
    };
    // Formation override (retail 0x801E6728..0x801E676C).
    match host.first_monster_id() {
        0xB3 => slot = 2,
        0xB4 => slot = 1,
        _ => {}
    }
    ctx.active_actor = slot;
    slot
}

pub(super) fn end_of_action<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let party_count = host.party_count();
    let total = host.slot_count();

    // Wipe scans (retail 0x801E6510..0x801E664C). A combatant counts as
    // standing while alive (`+0x14C != 0`) and not carrying the down-mask
    // bits of `+0x16E`: retail masks both sides with `0x4`
    // (non-targetable, e.g. a captured monster); the enemy-ally charm
    // widen turns the monster-side mask into `0x384` (the one-word edit at
    // `0x801E6638`) so a living charmed ally counts as down.
    let party_alive = (0..party_count)
        .filter(|&s| {
            host.actor(s)
                .is_some_and(|a| a.liveness != 0 && a.field_flags & 0x4 == 0)
        })
        .count();
    let monster_mask: u16 = if ctx.charm_widen { 0x384 } else { 0x4 };
    let monsters_alive = (party_count..total)
        .filter(|&s| {
            host.actor(s)
                .is_some_and(|a| a.liveness != 0 && a.field_flags & monster_mask == 0)
        })
        .count();

    if party_alive == 0 {
        host.battle_end(BattleEndCause::PartyWipe);
        return StepOutcome::BattleComplete;
    }
    if monsters_alive == 0 {
        // Retail order: end signal (0x801E6670..0x801E6680), then the
        // victory-pose fix-up, then the win-pose stream request.
        host.battle_end(BattleEndCause::MonsterWipe);
        let pose_slot = victory_pose_fixup(host, ctx);
        host.victory_stage(pose_slot);
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
    let alive_total = (party_alive + monsters_alive) as u8;
    if bumped < alive_total {
        return transition(ctx, ActionState::PreActionWait);
    }
    transition(ctx, ActionState::BattleComplete)
}

//! Physical-attack band of the battle-action state machine (face / short-step / windup / chain).

use super::*;
use crate::battle_formulas::{arms_resolver_admits, arms_weapon_atk_fold};

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
    let (target, strike_index_pre, character, chosen_art, staged_power, staged_effect) = host
        .actor(slot)
        .map(|a| {
            (
                a.active_target,
                a.strike_index,
                a.character,
                a.chosen_art,
                a.art_power.get(a.strike_index as usize).copied().flatten(),
                a.art_enemy_effect,
            )
        })
        .unwrap_or((
            0,
            0,
            legaia_art::Character::default(),
            None,
            None,
            legaia_art::EnemyEffect::None,
        ));
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
    // The strike loop resolves damage through `FUN_801EC3E4` (the fold
    // above and the host's art-strike hook), NOT through the item / restore
    // applier: `jal 0x800402f4` occurs exactly once in `FUN_801E295C`, at
    // `0x801E4134` in the spirit band, and it is passed an effect class where
    // this loop would have passed an animation byte. The port used to call
    // `apply_damage(next_byte, 0, target, slot)` from here; that call site
    // does not exist in retail and is gone.
    //
    // Which art - if any - this staged byte belongs to.
    //
    // Retail's stream alphabet for a party attack is direction swings
    // `0x0C..0x0F`, the art starters `0x19`/`0x1A` and art action constants
    // `0x1B+` (`docs/subsystems/battle-action.md` § Attack chain - strike
    // loop), so a Tactical-Arts turn stages its constants **inline**: the
    // byte itself names the art, and it is the byte the anim commit latches
    // into `+0x1DB` for the per-art attack camera to dispatch on. That
    // inline id is therefore authoritative here; `chosen_art` stays the
    // fallback for a caller that stages plain anim ids instead.
    //
    // A **direction swing** is a plain committed arms command and never an
    // art hit - it resolves through the host's own melee seam - so no art
    // strike is dispatched for one even while `chosen_art` is set. Without
    // that guard an arts turn's unmatched directions would be charged twice,
    // once as a swing and once as an art strike.
    //
    // The inline read is **party-only**. A monster's stream carries archive
    // entry indices over the whole byte range, so the same `0x1B+` value that
    // names an art on a party slot names a plain clip on a monster - and the
    // slot's `character` key is meaningless there. Retail draws the same line:
    // `FUN_801EED1C` is the party setup hook, and the per-art camera's own
    // gate requires a party seat.
    let party_slot = slot < host.party_count();
    let inline_art = legaia_art::ActionConstant::from_byte(next_byte)
        .filter(|_| party_slot)
        .filter(|a| a.is_art());
    let strike_art = if is_swing_command(next_byte) {
        None
    } else {
        inline_art.or(chosen_art)
    };
    // Dispatch [`BattleActionHost::apply_art_strike`] with the per-strike
    // power/timing/effect/hit-cue values. Generic-attack callers ignore this
    // hook (default no-op); callers wired up to art data drive HP deduction,
    // status application, and SFX timing from it.
    //
    // The power comes from the staged profile when the engine put one on the
    // actor ([`BattleActor::art_power`]) and from the art record otherwise,
    // so a Miracle / Super finisher - whose per-strike profile is a property
    // of its replacement queue, not of any single record - resolves through
    // the same seam as a plain named art.
    if let Some(art) = strike_art {
        let info = {
            let rec = host.art_record(character, art);
            let idx = strike_index_pre as usize;
            let power = staged_power.or_else(|| rec.and_then(|r| r.power.get(idx).copied()));
            let effect = if staged_effect != legaia_art::EnemyEffect::None {
                staged_effect
            } else {
                rec.map(|r| r.enemy_effect).unwrap_or_default()
            };
            // Nothing to resolve at all - no staged profile and no record -
            // is the pre-carrier "art data unavailable" case, which stays a
            // no-op. A source that *exists* but runs out of power bytes at
            // this index still dispatches with `power: None`, which is the
            // documented "this anim plays but does no damage" strike.
            (staged_power.is_some() || rec.is_some()).then_some(ArtStrikeInfo {
                strike_index: strike_index_pre,
                anim_byte: next_byte,
                actor_slot: slot,
                target_slot: target,
                character,
                art,
                power,
                dmg_timing: rec.and_then(|r| r.dmg_timing.get(idx).copied()),
                enemy_effect: effect,
                hit_cue: rec.and_then(|r| r.hit_cues.get(idx).copied()),
            })
        };
        if let Some(info) = info {
            host.apply_art_strike(info);
        }
    }
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
        // Counter-attack swap: advance the turn cursor past the counterer and
        // route back into AttackChain (retail `0x801E36D0`). Engines drive the
        // actual swap.
        ctx.turn_cursor = ctx.turn_cursor.saturating_add(1);
        return transition(ctx, ActionState::AttackChain);
    }
    transition(ctx, ActionState::DoneCleanup)
}

//! Magic / item cast band of the battle-action state machine.

use super::*;

// --- magic / item band ------------------------------------------------------

/// Face the acting actor at whatever its target byte `+0x1DD` names - retail's
/// `0x801E4334..0x801E43A4`, the tail of the cast-begin arm.
///
/// Two arms, split on whether the byte is a slot or a group code:
///
/// * **`code < 8`** (`sltiu v0, t2, 0x8` at `0x801E433C`): the bearing is taken
///   straight from the target actor's seat. Retail skips the whole store when
///   the actor is its own target (`beq v0, t2` at `0x801E4350`).
/// * **`code >= 8`**: [`target_group_aim`](crate::battle_target_group::target_group_aim)
///   folds the group's live seats into a centroid, and retail negates both
///   components back into a world position (`subu a0, zero, a0` /
///   `subu a1, zero, a1` at `0x801E438C`) before the same bearing call.
///
/// Either way the bearing is `FUN_80019B28(p1z, p1x, p2z, p2x)`, which
/// differences `p2 - p1` - so passing the *target* as `p1` measures target ->
/// actor, and the `+ 0x800` half-turn at `0x801E439C` is what turns it back
/// into actor -> target. The result is masked to 12 bits and stored at `+0x46`.
///
/// The group walk is assembled in **retail slot numbering** (party `0..3`,
/// monsters `3..7`), because that is the numbering the group codes index; the
/// engine seats monsters at `party_count` instead, so the two are mapped here
/// the same way [`super::dispatch`]'s target banner maps them.
///
/// A host with no [`BattleActionHost::actor_position`] leaves the facing alone.
///
/// REF: FUN_801E295C (`0x801E4334..0x801E43A4`)
fn face_cast_target<H: BattleActionHost + ?Sized>(host: &mut H, ctx: &BattleActionCtx) {
    use crate::battle_cue_group::MONSTER_SLOT_FIRST;
    use crate::battle_target_group::{GroupSlot, RENDER_FLAG_HIDDEN, target_group_aim};

    let slot = ctx.active_actor;
    let Some((actor_x, actor_z)) = host.actor_position(slot) else {
        return;
    };
    let code = host.actor(slot).map_or(0, |a| a.active_target);
    let party_count = host.party_count();

    let (aim_z, aim_x) = if (code as usize) < ACTOR_SLOTS {
        if code == slot {
            return;
        }
        let Some((target_x, target_z)) = host.actor_position(code) else {
            return;
        };
        (target_z, target_x)
    } else {
        let mut slots = [GroupSlot {
            live: false,
            x: 0,
            z: 0,
        }; ACTOR_SLOTS];
        for (retail_slot, out) in slots.iter_mut().enumerate() {
            let retail_slot = retail_slot as u8;
            // Retail numbering -> the engine's compact seating.
            let engine_slot = if retail_slot < MONSTER_SLOT_FIRST {
                if retail_slot >= party_count {
                    continue;
                }
                retail_slot
            } else {
                party_count + (retail_slot - MONSTER_SLOT_FIRST)
            };
            let Some((x, z)) = host.actor_position(engine_slot) else {
                continue;
            };
            let live = host.actor(engine_slot).is_some_and(|a| {
                // Party arm: the roster byte, i.e. seat occupancy. Monster arm:
                // retail's `+0x4` prim word, read through its `+0x21C` twin.
                retail_slot < MONSTER_SLOT_FIRST || a.render_flag != RENDER_FLAG_HIDDEN
            });
            *out = GroupSlot { live, x, z };
        }
        let Some(aim) = target_group_aim(code, &slots) else {
            return;
        };
        (-aim.centroid_z, -aim.centroid_x)
    };

    let bearing = bearing_12bit_approx(aim_z, aim_x, actor_z, actor_x);
    let facing = bearing.wrapping_add(0x800) & 0xFFF;
    if let Some(actor) = host.actor_mut(slot) {
        actor.facing_angle = facing;
    }
}

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
    // Turn to face the target (or the group's centroid). Retail runs this on
    // the retargeted byte, before it picks the next state.
    face_cast_target(host, ctx);
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
        host.duck_audio_level(75);
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
    host.duck_audio_level(75);
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

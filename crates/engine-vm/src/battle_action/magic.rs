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
        // Retail's `subu`, which wraps rather than trapping.
        (aim.centroid_z.wrapping_neg(), aim.centroid_x.wrapping_neg())
    };

    let bearing = bearing_12bit_approx(aim_z, aim_x, actor_z, actor_x);
    let facing = bearing.wrapping_add(0x800) & 0xFFF;
    if let Some(actor) = host.actor_mut(slot) {
        actor.facing_angle = facing;
    }
}

/// The item-target re-route at the head of the cast-begin arm
/// (`0x801E4298..0x801E4334`).
///
/// Retail keys this on the acting actor's **target byte** `+0x1DD`, not on its
/// action category: `lw t2, 0x20(sp)` at `0x801E4298` reloads the byte the
/// prologue read out of `+0x1DD`. Target code `9` takes the override in
/// `ctx[+0x24B]` and code `8` takes `ctx[+0x24A] - 1`, each only when that ctx
/// byte is non-zero; a zero leaves the code alone, which is what sends it on to
/// the group arm of [`face_cast_target`]. The two checks are **sequential** on
/// the rewritten value (`0x801E42E8` reloads it before the `== 8` compare), so
/// a `9` that resolves to `8` falls into the second arm.
///
/// The earlier port read `actor.action_category` instead, mapped `8` and `9` to
/// the opposite ctx bytes, and rewrote unconditionally. Nothing in the engine
/// writes either ctx byte, so all three were invisible in behaviour - but the
/// wrong key made the arm unreachable, because it is `active_target` that
/// carries `8` / `9` in this port (the monster-AI resolver `FUN_801E7320`
/// writes them).
///
/// REF: FUN_801E295C (`0x801E4298..0x801E4334`)
fn retarget_item_codes<H: BattleActionHost + ?Sized>(host: &mut H, ctx: &BattleActionCtx) {
    let slot = ctx.active_actor;
    let Some(mut code) = host.actor(slot).map(|a| a.active_target) else {
        return;
    };
    let mut rewritten = false;
    if code == 9 && ctx.item_target_b != 0 {
        code = ctx.item_target_b;
        rewritten = true;
    }
    if code == 8 && ctx.item_target_a != 0 {
        code = ctx.item_target_a.wrapping_sub(1);
        rewritten = true;
    }
    if rewritten && let Some(actor) = host.actor_mut(slot) {
        actor.active_target = code;
    }
}

pub(super) fn magic_cast_begin<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    retarget_item_codes(host, ctx);
    // Turn to face the target (or the group's centroid). Retail runs this on
    // the retargeted byte, before it picks the next state.
    face_cast_target(host, ctx);
    // Stage frame timer for pre-cast wait.
    ctx.frame_timer = 0x14;

    // The spell-name HUD label (`FUN_801D8DE8(0x4C, 0)`) is **monster-only**:
    // `lbu v0,0x2(s5); sltiu v0,v0,0x3; bne v0,zero,0x801e4460` at
    // `0x801E43D0..0x801E43DC` branches PAST the name lookup + the element
    // fire for an acting id `< 3` (a party seat). A party cast raises no
    // `0x4C` label anywhere - the retail mid-cast states carry an empty
    // descriptor (`0x80077344 == 0`) for every player summon and a live
    // string pointer only for the monster Tail Fire cast. The earlier port
    // had the branch sense inverted.
    let party_count = host.party_count();
    if slot >= party_count {
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

/// State `0x29` - the pre-cast wait and the cast's first anim pull
/// (`0x801E4598..0x801E4758`).
///
/// On the timer's expiry: a party caster runs the trigger `FUN_801DBF9C`,
/// which writes the cast's anim stream behind the spell id (`params[1..]`)
/// and, for a Seru id, the summon sub-route (`actor[+0x1E0] = 9`, read back
/// here as the `0x32` route). Then the stream cursor is **bumped before the
/// read** (`lbu v0,0x4(s5); addiu v0,v0,0x1; sb v0,0x4(s5)` at
/// `0x801E4644..0x801E4650`), so the first anim byte is `params[1]`, and
/// that byte is staged into `+0x1DA` at once (`sb v0,0x1da(s3)` at
/// `0x801E4664`); a `0xFF` there clears the stage and ends the action
/// (`sb zero,0x1da; ctx[7] = 0x50`). A non-Seru id (`< 0x81`) then bumps
/// the cursor again and hands the next byte to the cast-effect driver
/// `FUN_801DC0A0` ([`BattleActionHost::spell_anim_sustain`]) with the
/// `+0x1FA` / `+0x1DC` bookkeeping and the three id-keyed one-shot cues
/// (`0x3F -> 0x14C`, `0x2C -> 0x144`, `0x6A -> 0x15E`, `0x801E46DC..0x801E4740`).
///
/// The earlier port read `params[0]` - the spell id itself - as the first
/// anim byte and never staged it, which is why no cast could carry a clip
/// through this band.
///
/// PORT: FUN_801E295C (`0x801E4598..0x801E4758`)
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

    // Summon-route check (`lbu v1,0x1e0(s3); li v0,0x9; bne` at
    // `0x801E45EC`).
    let sub_route = host.actor(slot).map(|a| a.sub_route).unwrap_or(0);
    if sub_route == 9 {
        return transition(ctx, ActionState::SummonInvoke);
    }

    // Bump, then read + stage (`0x801E4644..0x801E4664`).
    let next_byte = match host.actor_mut(slot) {
        Some(actor) => {
            actor.strike_index = actor.strike_index.saturating_add(1);
            let b = actor.read_param(0);
            actor.queued_anim = b;
            b
        }
        None => 0xFF,
    };
    if next_byte == 0xFF {
        if let Some(actor) = host.actor_mut(slot) {
            actor.queued_anim = 0;
        }
        host.pose(slot, Pose::Idle);
        return transition(ctx, ActionState::DoneCleanup);
    }
    if spell_id < 0x81 {
        // `0x801E469C..0x801E46D8`: second bump, the effect driver on the
        // byte it lands on, then the bookkeeping.
        let effect_byte = match host.actor_mut(slot) {
            Some(actor) => {
                actor.strike_index = actor.strike_index.saturating_add(1);
                actor.read_param(0)
            }
            None => 0xFF,
        };
        host.spell_anim_sustain(slot, effect_byte);
        if let Some(actor) = host.actor_mut(slot) {
            actor.spell_iter = actor.spell_iter.saturating_add(1);
            actor.flag_bits.set(ActorFlags::WINDUP_DONE);
        }
        let cue = match spell_id {
            0x3F => Some(0x14C),
            0x2C => Some(0x144),
            0x6A => Some(0x15E),
            _ => None,
        };
        if let Some(cue) = cue {
            host.one_shot_sfx(cue);
        }
    } else if let Some(actor) = host.actor_mut(slot) {
        // `0x801E4744`: a Seru id that did not take the summon route stages
        // nothing.
        actor.queued_anim = 0;
    }
    host.pose(slot, Pose::Idle);
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

/// The framing style (`ctx[+0xD]`) the capture band pins for the whole cast:
/// `li v0,0x1` at `0x801E50C4`, stored in the `jal`'s delay slot at
/// `0x801E50CC`. Style `1` is the half-turn arm of the shared `ctx[+0xD]`
/// fork in `FUN_801D5854` (see [`crate::battle_cam_script::ActionFraming`]).
pub const CAPTURE_CAMERA_VARIANT: u8 = 1;

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

/// Per-frame decrement of the capture band's camera framing, in `+0x6D0`
/// units. `lbu v1,0x393(0x1F80)` / `sll v1,v1,0x4` at
/// `0x801E5000..0x801E500C`: the scratchpad frame scalar times sixteen.
pub const CAPTURE_FADE_CAMERA_STEP: i16 = 16;

/// State `0x6F` - the capture band's **fade-in hold**
/// (`0x801E4F88..0x801E5048`).
///
/// Four things, in retail's order:
///
/// * the audio duck, gated on `ctx[+0x287]` (`lbu v0,0x287(v0)` /
///   `beq v0,zero,0x801E4FF8` at `0x801E4F94..0x801E4F9C`) - the same 75%
///   ramp and the same gate state `0x70` runs;
/// * the camera pull-in: `ctx[+0x6D0] -= frame_scalar * 16`, an unsigned
///   halfword subtract with no floor (`0x801E4FFC..0x801E5014`);
/// * `FUN_801D5854(ctx[+0x13], 6)` (`0x801E5018..0x801E5020`) - the framing
///   program re-armed **every frame** of the hold, not once on entry. Pose
///   `6` is [`Pose::Idle`], which is also what the band's neighbours stage,
///   so what this call carries here is the camera half: `FUN_801D5854`'s
///   prologue advances the attack-camera ramp by `8 * frame_step` on every
///   call, so dropping it does not merely skip a pose - it stalls the ramp
///   for the length of the fade;
/// * the exit. `FUN_8003F2B8(1)` non-zero holds; a zero return stores state
///   `0x70` and clears the module phase `ctx[+0x279]` (`sb zero,0x279(v0)`
///   at `0x801E5048`), which is what [`arm_capture_cast_module`] mirrors.
///
/// PORT: FUN_801E295C (`0x801E4F88..0x801E5048`)
///
/// [`arm_capture_cast_module`]: crate::battle_action::BattleActionHost::capture_stager_tick
pub(super) fn magic_capture_fade<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    if ctx.counter_attack_a != 0 {
        host.duck_audio_level(75);
    }
    // The camera pull-in. Unsigned in retail, so a long hold wraps rather
    // than clamping; `wrapping_sub` on the same bits reproduces it.
    let step = CAPTURE_FADE_CAMERA_STEP.wrapping_mul(host.frame_dt().max(0));
    ctx.camera_frame_height = ctx.camera_frame_height.wrapping_sub(step);
    host.camera_frame_height(ctx.camera_frame_height);
    // Re-armed every frame, ahead of the exit test.
    host.pose(ctx.active_actor, Pose::Idle);
    if !host.previous_action_cleared(1) {
        return stay(ctx);
    }
    transition(ctx, ActionState::MagicCapturePhase2)
}

/// State `0x70` - the capture band's **per-frame module tick**
/// (`0x801E504C..0x801E50E4`).
///
/// Three things happen here, in retail's order:
///
/// * the audio duck, gated on `ctx[+0x287]` (`lbu v0,0x287(v0)` /
///   `beq v0,zero,0x801E50BC` at `0x801E5058..0x801E5060`) - the same 75%
///   ramp state `0x6F` runs, and the same gate. The port used to duck
///   unconditionally;
/// * `ctx[+0xD] = 1` (`li v0,0x1` / `sb v0,0xd(v1)` at
///   `0x801E50C4..0x801E50CC`, the `jal`'s delay slot): the framing style is
///   pinned to the half-turn variant for the whole capture, so a capture is
///   always framed from the mirrored side whatever the action seed rolled;
/// * the hold. `jal 0x801f2160` at `0x801E50C8` re-enters the resident
///   slot-B cast module every frame and `bne v0,zero,0x801e6814` at
///   `0x801E50D0` **stays in `0x70`** while it reports busy. Only a zero
///   return advances to `0x71`.
///
/// The port used to transition on the first frame, which collapsed every
/// capture-class cast's staging to a single tick. The tick itself is the
/// host's ([`BattleActionHost::capture_stager_tick`]); a host with none is
/// never busy and still passes straight through.
///
/// The depth re-seed `jal 0x801f0348` at `0x801E50DC` runs in the same breath
/// as the `0x71` store - its delay slot *is* `sb v0,0x7(v1)` - so it is part
/// of this transition and not of the next state. It is ported here.
///
/// Re-seeding matters because `0x6F` spent the whole fade ramping
/// `ctx[+0x6D0]` down ([`CAPTURE_FADE_CAMERA_STEP`]): without it the camera
/// would stay wherever the pull-in left it for the rest of the action. The
/// earlier note here said the port "re-derives `ctx[+0x6D0]` at the action
/// seed instead", which was true and beside the point - the seed ran long
/// before the ramp did.
///
/// PORT: FUN_801E295C (`0x801E504C..0x801E50E4`)
/// PORT: FUN_801f0348 (the `0x801E50DC` re-seed call site)
pub(super) fn magic_capture_phase2<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    if ctx.counter_attack_a != 0 {
        host.duck_audio_level(75);
    }
    ctx.camera_variant = CAPTURE_CAMERA_VARIANT;
    // PORT: FUN_801F2160 (call site; the tick body is the host's resident
    // cast module)
    if host.capture_stager_tick() {
        return stay(ctx);
    }
    // The re-seed, in the transition's own breath. Retail reads the live
    // `+0x1DD` here exactly as the action seed did, so the port asks the host
    // for the same two inputs rather than reusing the seed's answer.
    let target_slot = host.actor(ctx.active_actor).map_or(8, |a| a.active_target);
    let party_count = host.party_count();
    let frame_height = crate::battle_formulas::camera_height_for_frame(
        ctx.active_actor,
        target_slot,
        party_count,
        |slot| host.monster_size_class(slot),
    );
    ctx.camera_frame_height = frame_height;
    host.camera_frame_height(frame_height);
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

//! Spirit / Originals band of the battle-action state machine (MP-cost + ability-bit application).

use super::*;
use crate::battle_cast_cue::{CastCueOutcome, cast_audio_cue};
use crate::battle_cue_group::{CueTables, cue_group_for, expand_cue_group};

// --- spirit band ------------------------------------------------------------

/// Seed the committed action's `(class, tier)` pair into `+0x1E8` / `+0x1E9`:
/// the whole of retail's `0x801E3B70..0x801E3CB0`, the branch state `0x3C`
/// takes on the category byte before it prices the cast.
///
/// The Item leg resolves the item's property record `+1` into the item-effect
/// descriptor table (`0x800752C0`, 4-byte stride) and copies the descriptor's
/// `+0` / `+1`; every other category copies `+0` / `+1` of the spell record
/// (`0x800754C8 + id*0xC`) instead. Both are disc tables the host owns, so a
/// host that installs neither leaves the pair at zero.
///
/// PORT: FUN_801E295C (`0x801E3B70..0x801E3CB0`, the `+0x1E8`/`+0x1E9` seed)
fn seed_cast_class<H: BattleActionHost + ?Sized>(
    host: &mut H,
    slot: u8,
    category: ActionCategory,
    action_id: u8,
) {
    let pair = if matches!(category, ActionCategory::Item) {
        host.item_effect_class_pair(action_id)
    } else {
        host.spell_class_byte(action_id)
            .map(|class| (class, host.spell_sub_class_byte(action_id).unwrap_or(0)))
    };
    let Some((class, tier)) = pair else {
        return;
    };
    if let Some(actor) = host.actor_mut(slot) {
        actor.cast_class = class;
        actor.cast_sub_class = tier;
    }
}

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
    seed_cast_class(host, slot, category, spell_id);
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
    // Cast-start audio cue. Retail's `jal 0x801f3990` sits in this arm, in
    // the delay slot of the `ctx[7] = 0x3E` store (`0x801E3E04`), so the cue
    // fires exactly once per cast, on the frame the queued anim settles.
    // PORT: FUN_801F3990 (call site; the resolver is
    // `crate::battle_cast_cue::cast_audio_cue`)
    let (cast_class, sub_class, queue_head) = host
        .actor(slot)
        .map(|a| (a.cast_class, a.cast_sub_class, a.params[0]))
        .unwrap_or((0, 0, 0));
    let char_kind = host.roster_character_id(slot);
    match cast_audio_cue(slot, char_kind, cast_class, queue_head, sub_class) {
        CastCueOutcome::None => {}
        CastCueOutcome::Sfx(id) => host.one_shot_sfx(id),
        CastCueOutcome::ItemGive { voice_arg } => host.cast_item_give(voice_arg),
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
    // The applier call, with retail's own four arguments (`0x801E4108`..
    // `0x801E4138`): the committed `(class, tier)` pair, the stacked target
    // byte `+0x1DD`, and the acting slot's roster character **index** - the
    // `DAT_8007BD10[slot] - 1` the record base is built from, not the battle
    // slot. An earlier port passed `+0x1E7` / `+0x1FA` here, which are the
    // staged anim and the cast-iteration counter, so the class and tier that
    // select the applier's branch never reached the hook.
    // REF: FUN_800402F4 (the primitive `apply_damage` stands in for)
    let (class, tier) = host
        .actor(slot)
        .map(|a| (a.cast_class, a.cast_sub_class))
        .unwrap_or((0, 0));
    let party_index = host.roster_character_id(slot).saturating_sub(1);
    host.apply_damage(class, tier, target, party_index);
    place_cue_group(host, class, tier, target);
    ctx.frame_timer = 0x80;
    transition(ctx, ActionState::SpiritPostDamage)
}

/// Expand and place the applier arm's cue group - the port's stand-in for the
/// eleven `jal 0x801e22c8` branches inside `FUN_800402F4`.
///
/// [`cue_group_for`] picks the site the committed `(class, tier)` reaches and
/// [`expand_cue_group`] turns that site's group record into its spawn list;
/// each spawn goes to
/// [`BattleActionHost::spawn_cue`](super::BattleActionHost::spawn_cue).
///
/// The site carries its own `a0` tint and `a1` actor-state literals, so the
/// recolour and the two actor writes (`+0x04`, and `+0x0C` except on the
/// revive arm) are retail's own words rather than stand-ins.
///
/// The class-`1` arm's `jal` sits inside the applier's per-slot loop, so its
/// group is placed once per **occupied** slot on one side of the field; every
/// other arm places once, on the action's target. Which side is the target
/// byte's: `param_3 == 9` walks monster slots `3..7` (`s1 = 3, s7 = 7` at
/// `0x80040918`), anything else walks party slots `0..3` (`s7 = 3` at
/// `0x80040924`). The per-slot gate is retail's **roster byte**
/// (`DAT_8007BD10[slot]` below 3, `DAT_8007BD09[slot]` above), i.e. "is this
/// seat filled" - not liveness, so a downed member still gets the cue.
///
/// REF: FUN_800402F4 (the eleven branch sites), FUN_801E22C8 (the expander)
fn place_cue_group<H: BattleActionHost + ?Sized>(host: &mut H, class: u8, tier: u8, target: u8) {
    let Some(site) = cue_group_for(class, tier) else {
        return;
    };
    let slots: Vec<u8> = if site.per_target {
        let range = if target == crate::battle_cue_group::TARGET_ALL_ENEMIES {
            crate::battle_cue_group::MONSTER_SLOT_FIRST..crate::battle_cue_group::MONSTER_SLOT_END
        } else {
            0..host.party_count()
        };
        // `actor(slot).is_some()` is the engine's reading of the roster-byte
        // occupancy gate - the actor table holds exactly the seated
        // combatants (see `BattleActionHost::actor_position`).
        range.filter(|&s| host.actor(s).is_some()).collect()
    } else {
        vec![target]
    };
    for s in slots {
        let yaw = host.actor(s).map(|a| a.facing_angle as i16).unwrap_or(0);
        // The table borrow ends with the statement, so the plan is owned by
        // the time the spawn sink needs `&mut host`.
        let Some(plan) = host.cue_tables().map(|(groups, sfx_map)| {
            expand_cue_group(
                site.tint,
                site.actor_state,
                yaw,
                site.group,
                &CueTables { groups, sfx_map },
            )
        }) else {
            return;
        };
        if let Some(actor) = host.actor_mut(s) {
            actor.render_color = plan.actor_state;
            if let Some(flags) = plan.actor_flags {
                actor.render_scale = flags;
            }
        }
        for spawn in plan.spawns {
            host.spawn_cue(s, spawn);
        }
    }
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

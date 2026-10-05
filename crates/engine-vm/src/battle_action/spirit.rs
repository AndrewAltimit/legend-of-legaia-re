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
    let class = host.actor(slot).map(|a| a.cast_class).unwrap_or(0);
    if class == GAUGE_EXTEND_CLASS {
        gauge_extend_fire(host, ctx, slot);
    }
    ctx.frame_timer = 0x20;
    transition(ctx, ActionState::SpiritFireDamage)
}

/// The committed effect class whose `0x3E` arm this is: `5`, the item-effect
/// table's "extend action gauge for one battle" class (Fury Boost) -
/// `lbu v1,0x1e8(s3)` / `li v0,0x5` / `bne` at `0x801E3E80..0x801E3E88`.
pub const GAUGE_EXTEND_CLASS: u8 = 5;

/// Ceiling of the extended gauge the arm stages (`slti v0,v0,0x121` /
/// `li v0,0x120` at `0x801E3F90..0x801E3F98`).
pub const GAUGE_EXTEND_CEILING: i16 = 0x120;

/// The **extended action gauge** `min(base * 7 / 5 + 8, 0x120)`, the one
/// formula behind every "the AP gauge grows" retail draws: the Spirit
/// dispatch arm's charged-bar seed (`0x801E2F88..0x801E2FCC`), the Spirit
/// band's bar target (`0x801E52C0..0x801E5314`), the gauge-extension item's
/// target (`0x801E3F60..0x801E3F98`) and the round-boundary restore of a
/// Spirit-charged actor (`battle_formulas::round_reset_agility`). `base` is
/// the actor's AGL base `+0x156`; the `0x66666667` reciprocal is `/ 5`
/// applied to `base * 7`.
pub fn extended_gauge(base: u16) -> i16 {
    let v = i32::from(base) * 7 / 5 + 8;
    (v as i16).min(GAUGE_EXTEND_CEILING)
}

/// Placement record of the AP bar the Spirit arm raises (`li a0,0xf` at
/// `0x801E2FD0`).
pub const SPIRIT_AP_BAR_ELEMENT: u8 = 0x0F;
/// Placement record of the AP plate the Spirit arm raises (`li a0,0x52` at
/// `0x801E2FDC`).
pub const SPIRIT_AP_PLATE_ELEMENT: u8 = 0x52;
/// The AP bar's drawn width is its gauge value less this (`addiu v0,v0,-0x6`
/// at `0x801E2F70`, and the `-6` the sustain's bar target takes at
/// `0x801E547C`) - the same bias the arts-entry gauge sizes its records with.
pub const SPIRIT_BAR_WIDTH_BIAS: i16 = 6;
/// The camera depth state `0x46` writes before anything else
/// (`li v0,0x800` / `sh v0,0x6d0(v1)` at `0x801E52AC..0x801E52B0`): the
/// Spirit close-up. Without it the in-fight framing keeps the seed's
/// size-class depth, which sets the camera on the far side of the monsters.
pub const SPIRIT_CAMERA_DEPTH: i16 = 0x800;
/// The Spirit gauge a Spirit turn stages on top of the actor's `+0x170`
/// (`addiu v0,a1,0x20` at `0x801E5320`), and the two passive overrides:
/// `+0x28` with record `+0xF8 & 0x200`, `+0x23` with `& 0x100`
/// (`0x801E5364..0x801E537C`). The same `+0x20` is what the Done band
/// actually adds (`actor[+0x224] = 0x20` for category `4`).
pub const SPIRIT_TURN_GAIN: u16 = 0x20;
pub const SPIRIT_TURN_GAIN_BIT_200: u16 = 0x28;
pub const SPIRIT_TURN_GAIN_BIT_100: u16 = 0x23;

/// Ceiling of the spirit-gauge target the arm stages (`slti v0,v0,0x65` /
/// `li v0,0x64` at `0x801E4004..0x801E4010`).
pub const SPIRIT_TARGET_CEILING: i16 = 100;

/// Ability-word `+0xF8` bit that makes the arm's spirit bump `+10` instead of
/// `+8` (`andi v0,v0,0x200` at `0x801E3FEC`).
pub const SPIRIT_BUMP_PLUS_BIT: u32 = 0x200;

/// State `0x3E`'s class-5 arm (`0x801E3E90..0x801E4018`), run once the cast
/// clip has settled: the gauge-extension item's presentation.
///
/// - raises the two gauge HUD elements `0x0F` and `0x52` (`a1 = 0` at
///   `0x801E3F04` / `0x801E3F10`) and bumps their teardown latch
///   [`BattleActionCtx::spirit_action_count`] (`0x801E3F20..0x801E3F30`);
/// - draws one `rand()` for the camera variant, `(rand % 2) * 2`
///   (`jal 0x80056798` at `0x801E3F2C`, stored at `0x801E3F58`) - the draw is
///   unconditional on this arm, so skipping it desynchronises every later
///   battle draw;
/// - stages the extended gauge `min(0x120, target base * 7 / 5 + 8)` into
///   `ctx[+0x6DC]` (the `0x66666667` reciprocal at `0x801E3F60..0x801E3F80`
///   is `/ 5`, applied to `base * 7`);
/// - stages the acting actor's spirit gauge `+ 8` - `+ 10` with the `0x200`
///   passive - capped at `100`, into `ctx[+0x6DE]`.
///
/// The arm also writes a HUD scratch halfword (`0x80076D7E`) off the target's
/// `+0x1F9` / `+0x16C` / `+0x1DE` bytes, which the port has no slot for.
///
/// PORT: FUN_801E295C (`0x801E3E80..0x801E4018`, the class-5 arm of state
/// `0x3E`)
fn gauge_extend_fire<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
    slot: u8,
) {
    host.ui_element(0x0F, 0);
    host.ui_element(0x52, 0);
    ctx.spirit_action_count = ctx.spirit_action_count.wrapping_add(1);
    ctx.camera_variant = ((host.rng() % 2) * 2) as u8;
    let target = host.actor(slot).map(|a| a.active_target).unwrap_or(0);
    let base = host.actor(target).map(|t| t.agl_base).unwrap_or(0);
    ctx.damage_target = extended_gauge(base);
    let spirit = host.actor(slot).map(|a| a.spirit_gauge).unwrap_or(0) as i16;
    let bump = if host.character_ability_bits_high(slot) & SPIRIT_BUMP_PLUS_BIT != 0 {
        10
    } else {
        8
    };
    ctx.hp_bar_target = spirit.wrapping_add(bump).min(SPIRIT_TARGET_CEILING);
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
    // Retail's applier is a jump table with 116 dead slots
    // ([`super::effect_selector`]): a committed class outside `0x00..=0x0E`
    // (or `0x82`) restores the frame and returns, so the port does not stand
    // in for an arm that does not exist. Every class byte from the spell
    // table's routing band - `0x14` plain cast, `0x32` summon, `0x63`
    // capture - is one of those.
    if super::effect_selector_dispatches(class) {
        host.apply_damage(class, tier, target, party_index);
    }
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
        let Some(plan) = host.cue_tables().map(|(groups, clut_map)| {
            expand_cue_group(
                site.tint,
                site.actor_state,
                yaw,
                site.group,
                &CueTables { groups, clut_map },
            )
        }) else {
            return;
        };
        if let Some(actor) = host.actor_mut(s) {
            actor.render_color = plan.actor_state;
            if let Some(flags) = plan.actor_blend {
                actor.render_blend = flags;
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

// --- spirit band (category 4, `0x46..=0x48`) ---------------------------------
//
// Retail's action seed sends category `4` here unconditionally (`li v0,0x46`
// / `sb v0,0x7(v1)` at `0x801E2F5C`). The band stages the spirit clip the
// commit left at `+0x1E7`, ramps the spirit gauge HUD element toward its
// target and holds until both the clip and a timer have run out. The two HUD
// ramps - the AP plate's value (`*0x801F6968` `+0x10`,
// [`BattleActionCtx::spirit_plate_value`]) and the AP bar's width
// (`ctx[+0x1074]` `+0x0E`, [`BattleActionCtx::spirit_bar_width`]) - are part
// of the band's own exit conditions, so they are stepped here and drawn by
// the hosts off the context.

/// `0x46`'s hold before the sustain reads its exit (`li v0,0x20` /
/// `sh v0,0x2(s7)` at `0x801E539C..0x801E53A0`, `s7 = ctx + 0x6D6`).
pub const SPIRIT_BAND_HOLD: i16 = 0x20;

/// `0x47`'s exit re-arms the timer for the flush (`li v0,0x300` at
/// `0x801E54E0`), which `0x48` drains eight units a frame step.
pub const SPIRIT_FLUSH_HOLD: i16 = 0x300;
/// `0x48`'s per-frame drain multiplier (`sll v1,v0,0x3` at `0x801E5748`).
pub const SPIRIT_FLUSH_STEP: i16 = 8;

/// State `0x46`: pull the camera in to the Spirit close-up (`ctx[+0x6D0] =
/// 0x800`), stage the committed spirit clip (`+0x1DA = +0x1E7`) with the
/// clip-running flag `+0x1DC = 2`, stage the two HUD ramp targets and arm the
/// `0x20` hold.
///
/// The targets: the AP bar grows to the extended gauge
/// [`extended_gauge`]`(+0x156)` into `ctx[+0x6DC]`, and the AP plate climbs to
/// the actor's Spirit `+0x170` plus [`SPIRIT_TURN_GAIN`] (or one of its two
/// passive overrides) capped at `100` into `ctx[+0x6DE]`.
///
/// PORT: FUN_801E295C (state `0x46`, `0x801E52A4..0x801E53B4`)
pub(super) fn spirit_arts_entry<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    // `li v0,0x800` / `sh v0,0x6d0(v1)` at `0x801E52AC..0x801E52B0`, ahead of
    // the arm's case-6 call: the Spirit band frames its member at the near
    // depth, whatever `FUN_801F0348` sized the seed to, and the Done band
    // that follows keeps it (`nivora_duel_pre_megaton_press` reads
    // `ctx[+0x6D0] = 0x800`, eye depth `prescale(0x800)`, in `0x51`).
    ctx.camera_frame_height = SPIRIT_CAMERA_DEPTH;
    host.camera_frame_height(SPIRIT_CAMERA_DEPTH);
    host.pose(slot, Pose::Idle);
    let (base, spirit) = host
        .actor(slot)
        .map(|a| (a.agl_base, a.spirit_gauge))
        .unwrap_or((0, 0));
    if let Some(actor) = host.actor_mut(slot) {
        actor.flag_bits = ActorFlags(ActorFlags::ADVANCE_DONE);
        actor.queued_anim = actor.queued_anim_b;
    }
    ctx.damage_target = extended_gauge(base);
    let bits = host.character_ability_bits_high(slot);
    let gain = if bits & 0x200 != 0 {
        SPIRIT_TURN_GAIN_BIT_200
    } else if bits & 0x100 != 0 {
        SPIRIT_TURN_GAIN_BIT_100
    } else {
        SPIRIT_TURN_GAIN
    };
    ctx.hp_bar_target = (spirit.wrapping_add(gain) as i16).min(SPIRIT_TARGET_CEILING);
    ctx.frame_timer = SPIRIT_BAND_HOLD;
    transition(ctx, ActionState::SpiritArtsSustain)
}

/// State `0x47`: once the clip has committed (`+0x1D9 != 0`) the queued id is
/// cleared so it plays once; the hold drains **level-triggered** (`lh` /
/// `blez` at `0x801E53D8..0x801E53E0` - a positive timer steps and returns);
/// then the AP plate steps toward its target, the AP bar grows toward the
/// extended gauge (holding the band while it moves, unless the `+0x1F9`
/// charge byte already sized it), and the band waits on the clip-running flag
/// `+0x1DC` and leaves for the flush with the timer re-armed at `0x300`.
///
/// PORT: FUN_801E295C (state `0x47`, `0x801E53B8..0x801E54E8`)
pub(super) fn spirit_arts_sustain<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.pose(slot, Pose::Idle);
    let committed = host
        .actor(slot)
        .map(|a| a.current_anim != 0)
        .unwrap_or(false);
    if committed && let Some(actor) = host.actor_mut(slot) {
        actor.queued_anim = 0;
    }
    if ctx.frame_timer > 0 {
        ctx.frame_timer = ctx.frame_timer.saturating_sub(host.frame_dt());
        return stay(ctx);
    }
    let step = host.frame_dt();
    step_spirit_plate(ctx, step);
    // `0x801E5458..0x801E54A8`: with no charge byte, grow the bar one frame
    // step toward `ctx[+0x6DC] - 6` and return while it moves.
    let charged = host.actor(slot).is_some_and(|a| a.spirit_shield != 0);
    if !charged {
        let goal = ctx.damage_target.wrapping_sub(SPIRIT_BAR_WIDTH_BIAS);
        if ctx.spirit_bar_width < goal {
            ctx.spirit_bar_width = ctx.spirit_bar_width.wrapping_add(step);
            return stay(ctx);
        }
    }
    let clip_running = host.actor(slot).is_some_and(|a| a.flag_bits.0 != 0);
    if clip_running {
        return stay(ctx);
    }
    ctx.frame_timer = SPIRIT_FLUSH_HOLD;
    transition(ctx, ActionState::SpiritArtsFlush)
}

/// Step the AP plate's value one frame step toward `ctx[+0x6DE]`, landing
/// exactly on it - the two-compare body states `0x47` and `0x48` share
/// (`0x801E5400..0x801E5454` / `0x801E56D0..0x801E5728`).
fn step_spirit_plate(ctx: &mut BattleActionCtx, step: i16) {
    if ctx.spirit_plate_value < ctx.hp_bar_target {
        ctx.spirit_plate_value = ctx.spirit_plate_value.wrapping_add(step);
    }
    if ctx.hp_bar_target < ctx.spirit_plate_value {
        ctx.spirit_plate_value = ctx.hp_bar_target;
    }
}

/// State `0x48`: finish the AP plate's climb, drain the `0x300` hold eight
/// units a frame step (clamped at zero), and hand the action to the Done band
/// once the hold is out, the actor's anim pair has settled on idle
/// (`+0x1DA == +0x1D9 == 0`) and the plate sits on its target.
///
/// PORT: FUN_801E295C (state `0x48`, `0x801E56D0..0x801E57C4`)
pub(super) fn spirit_arts_flush<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    step_spirit_plate(ctx, host.frame_dt());
    if ctx.frame_timer > 0 {
        let step = host.frame_dt().saturating_mul(SPIRIT_FLUSH_STEP);
        ctx.frame_timer = if ctx.frame_timer < step {
            0
        } else {
            ctx.frame_timer - step
        };
    }
    host.pose(slot, Pose::Idle);
    let settled = host
        .actor(slot)
        .is_none_or(|a| a.queued_anim == a.current_anim && a.queued_anim == 0);
    if !settled || ctx.frame_timer != 0 || ctx.spirit_plate_value != ctx.hp_bar_target {
        return stay(ctx);
    }
    transition(ctx, ActionState::DoneCleanup)
}

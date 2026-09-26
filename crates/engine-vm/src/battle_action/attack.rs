//! Physical-attack band of the battle-action state machine (face / short-step / windup / chain).

use super::*;

// --- attack band ------------------------------------------------------------

/// What survives of the camera variant `ctx[+0xD]` when state `0x14` takes
/// its in-range shortcut into the strike loop: `andi v0,v0,0x1` at
/// `0x801E3224`.
pub const STRIKE_CAMERA_VARIANT_MASK: u8 = 1;

/// Per-frame facing recompute the attack band's states share (`0x15` at
/// `0x801E32EC..0x801E3318`, with `0x14` / `0x16` / `0x19` siblings at
/// `0x801E3068`, `0x801E336C` and `0x801E3568`; the outer `switch`'s entries
/// for `0x14` and `0x15` are `0x801E305C` and `0x801E32E0`): `facing =
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
        // The in-range shortcut is the **only** one of the three
        // `ctx[7] = 0x1E` stores that narrows the camera variant:
        // `lbu v0,0xd(v1)` / `andi v0,v0,0x1` / `sb v0,0xd(v1)` at
        // `0x801E321C..0x801E322C`, in state `0x14`'s own body (jump-table
        // entry `0x14` is `0x801E305C`; the `0x18` and `0x19` stores at
        // `0x801E3550` / `0x801E35AC` carry no such write). Only bit 0 - the
        // half-turn - survives, so a swing entered this way is never framed
        // with the style-2 pitch tilt. The mask is `& 1`, not `= 0`: variant
        // `3` enters the loop as `1` and keeps its mirrored side.
        ctx.camera_variant &= STRIKE_CAMERA_VARIANT_MASK;
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
        // Monster arm (`0x801E3244..0x801E32D0`): stage the record's
        // tag-`0x20` pre-approach entry and take the windup chain; a monster
        // with none stages its tag-`1` walk instead and takes the party's
        // `0x19` short step. Both stores are unconditional - a miss stages
        // the `0xFF` sentinel - and a host with no action table leaves the
        // queued anim alone.
        let pre = monster_action_by_tag(host, actor_slot, APPROACH_TRANSITION_TAG);
        let (anim, next) = match pre {
            Some(legaia_asset::monster_archive::NO_ACTION_ENTRY) | None => (
                monster_action_by_tag(host, actor_slot, WALK_TAG),
                ActionState::AttackShortStep,
            ),
            Some(idx) => (Some(idx), ActionState::AttackWindup),
        };
        if let (Some(anim), Some(actor)) = (anim, host.actor_mut(actor_slot)) {
            actor.queued_anim = anim;
        }
        next
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
    // The pre-approach clip has committed: stage the walk
    // (`0x801E331C..0x801E3348`) before `0x16` polls the range.
    if let Some(walk) = monster_action_by_tag(host, slot, WALK_TAG)
        && let Some(actor) = host.actor_mut(slot)
    {
        actor.queued_anim = walk;
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
    // Arrived: stage the close-in clip (`0x801E33AC..0x801E33D8`) that
    // `0x17` then waits on.
    if let Some(close_in) = monster_action_by_tag(host, slot, CLOSE_IN_TAG)
        && let Some(actor) = host.actor_mut(slot)
    {
        actor.queued_anim = close_in;
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
    // byte as the queued anim.
    //
    // **This state stages; it never resolves damage.** `jal 0x801ec3e4`
    // does not occur anywhere in `FUN_801E295C`: the damage kernel is called
    // from the anim tick `FUN_80047430` (`0x800478A0` / `0x80047BF0`) on
    // every frame the committed clip plays, and its own head decides which
    // frames are that clip's hit events (`crate::battle_action::hit_event`).
    // The engine's driver for that call sits beside its anim tick
    // (`engine-core`'s `World::tick_battle_hit_events`); a host that plays
    // no clips resolves the staged byte's hits as a zero-length clip
    // instead. Either way nothing in this arm touches HP.
    let slot = ctx.active_actor;
    // Strike pacing gate: while ADVANCE_DONE is still set the previous
    // staged swing is in flight - skip the byte read and hold (the anim
    // system clears the bit when the staged clip's event-path commit fires
    // or the clip ends; for the engine that's `World::tick_battle_hit_events`
    // / `World::tick_battle_animations`, or an immediate clear when the
    // actor carries no clips).
    // PORT: overlay_battle_action_801e295c (strike-pacing gate, interior).
    // The retail gate (battle-action overlay, file +0x370C) reads `lbu +0x1DC;
    // andi 0x2; bne -> skip` to guard the next-byte read at `+0x1DF + +0x15`.
    let in_flight = host
        .actor(slot)
        .map(|a| a.flag_bits.has(ActorFlags::ADVANCE_DONE))
        .unwrap_or(false);
    if in_flight {
        // Retail's in-flight branch (`0x801E3718`) lands on the drift block
        // at `0x801E37C0`, which the stage path also falls into - so the
        // drift runs on every frame of the loop, staged or holding.
        swing_drift(host, ctx);
        return stay(ctx);
    }
    let next_byte = host.actor(slot).map(|a| a.read_param(0)).unwrap_or(0xFF);
    if next_byte == 0x00 || next_byte == 0xFF {
        // An empty / exhausted stream: retail stages the zero it read and
        // falls to `0x1F` on the same step; nothing plays and nothing is
        // owed, so the gate is released here. The cursor is left where it
        // is - retail never rewinds it in this band (ActionSeed zeroes it,
        // `0x801E2CF0`; the `0x1F -> 0x20` edge parks it at `0xFF`).
        if let Some(actor) = host.actor_mut(slot) {
            actor.flag_bits.clear(ActorFlags::ADVANCE_DONE);
        }
        return transition(ctx, ActionState::AttackRecovery);
    }
    // The stage site (`0x801E3734..0x801E3764`): cursor post-increment by
    // exactly one, `+0x1DC |= 2` (the one-per-clip latch, bit 1 - the
    // "commit at the event frame" request the anim tick consumes), and the
    // byte into `+0x1DA`. Retail's stream alphabet for a party attack is
    // direction swings `0x0C..0x0F`, the art starters `0x19`/`0x1A` and the
    // art action constants `0x1B+` (`FUN_801EED1C` writes them inline, see
    // `docs/subsystems/battle-action.md` § A Tactical Art is an ordinary
    // attack-band action); a monster's bytes are archive entry indices. The
    // byte itself names the clip, and the anim commit latches it into
    // `+0x1DB` for the per-art attack camera - and the hit-event driver -
    // to key on.
    if let Some(actor) = host.actor_mut(slot) {
        actor.queued_anim = next_byte;
        actor.flag_bits.set(ActorFlags::ADVANCE_DONE);
        actor.strike_index = actor.strike_index.saturating_add(1);
    }
    attack_x2_stage_bump(host, ctx);
    swing_drift(host, ctx);
    // The terminator is tested at the **new** cursor on the same step
    // (`0x801E3998..0x801E39AC`, `0x00` routing to `0x1F` at `0x801E3A7C`):
    // the last byte is staged and the band leaves the loop together, so the
    // recovery wait below is what holds until that last clip commits.
    let exhausted = host.actor(slot).map(|a| a.read_param(0)).unwrap_or(0) == 0;
    if exhausted {
        if attack_x2_refill(host, ctx) {
            return stay(ctx);
        }
        return transition(ctx, ActionState::AttackRecovery);
    }
    stay(ctx)
}

/// The War God Icon's **per-stage pass bump** - `FUN_801E295C`
/// `0x801E3768..0x801E37BC`, the tail of the stage site.
///
/// ```text
/// 801e3798  lw    v0,0x6bc(v0)     ; char record +0xF4 (roster DAT_8007BD10[ctx+0x13])
/// 801e37a0  andi  v0,v0,0x2000     ; War God Icon "Attack x2"
/// 801e37ac  lbu   v0,0x5(s5)       ; ctx[+0x16]
/// 801e37b4  beq   v0,zero,801e37c0 ; zero: the pair has not started - no bump
/// 801e37b8  _addiu v0,v0,0x1
/// 801e37bc  sb    v0,0x5(s5)       ; ctx[+0x16] += 1
/// ```
///
/// Runs on every **staged** byte (only the stage path reaches it - the
/// in-flight hold branches straight to `0x801E37C0`). On the first pass the
/// counter reads `0` and nothing moves; the refill
/// ([`attack_x2_refill`]) raises it to `1` on the step the terminator is
/// read, while the first pass's last clip is still in flight - so that clip's
/// hits read `1` and still carry. The second pass's first stage then lifts it
/// to `2`, which is what ends the damage kernel's carry arm (`s2 = 0xFF`
/// only while `ctx[+0x16] < 2`, [`crate::battle_action::apply_mode`]): the
/// whole second pass applies. Without this bump the counter sat at `1`
/// through the second pass and the carry arm never released.
///
/// Retail's record read indexes the roster by `ctx[+0x13]` with no party
/// test in front of it; the engine asks the host's ability word only for a
/// seated party slot (the same narrowing [`attack_x2_refill`] makes), since a
/// monster ordinal indexes past the three roster bytes.
///
/// PORT: FUN_801E295C (`0x801E37AC..0x801E37BC`, the ctx[+0x16] stage bump)
fn attack_x2_stage_bump<H: BattleActionHost + ?Sized>(host: &mut H, ctx: &mut BattleActionCtx) {
    let slot = ctx.active_actor;
    if usize::from(slot) >= usize::from(host.party_count()) {
        return;
    }
    if host.character_ability_bits(slot) & WAR_GOD_ATTACK_X2_BIT == 0 {
        return;
    }
    if ctx.attack_x2_pass != 0 {
        ctx.attack_x2_pass = ctx.attack_x2_pass.wrapping_add(1);
    }
}

/// Whether the strike loop's **per-frame drift** runs this frame -
/// `FUN_801E295C` `0x801E37C0..0x801E3868`.
///
/// Party actors only (`ctx[+0x13] < 3`, `0x801E37C8`). Two arms, keyed on
/// the committed clip's header byte `ctx[+0x243]`
/// ([`BattleActionCtx::gauge_rearm_latch`]):
///
/// * `+0x243 == 0`: the character's ability word `+0x6BC` (record `+0xF4`)
///   carries the War God Icon bit `0x2000` (`0x801E37EC..0x801E3830`);
/// * `+0x243 != 0`: the builder's special-trigger flag `0x801F696C`
///   ([`BattleActionCtx::super_trigger`]) is set **and** the latched clip id
///   `+0x1DB` is outside `0x10..=0x1A` (`addiu -0x10; sltiu 0xB` at
///   `0x801E3858..0x801E385C`) - the `0x801E3840` read.
///
/// PORT: FUN_801E295C (`0x801E37C0..0x801E3868`, the `0x801F696C` reader at `0x801E3840`)
pub fn swing_drift_armed(
    acting_party: bool,
    clip_header: u8,
    super_trigger: bool,
    latched_anim: u8,
    ability_bits: u32,
) -> bool {
    if !acting_party {
        return false;
    }
    if clip_header != 0 {
        super_trigger && !(0x10..=0x1A).contains(&latched_anim)
    } else {
        ability_bits & WAR_GOD_ATTACK_X2_BIT != 0
    }
}

/// Signed drift speed per frame the arm applies, before `frame_dt` and the
/// acting actor's rate: the acting actor backs off along its facing at `-3`,
/// the target moves along *its* facing at `+3` (`negu` only on the acting
/// side, `0x801E3890`).
pub const SWING_DRIFT_SPEED: i16 = 3;

/// The drift itself (`0x801E386C..0x801E3994`): the acting actor's live pair
/// steps `trig(facing) * -3 * frame_dt * rate >> 15`, and the target's live
/// pair `trig(target facing) * 3 * frame_dt * rate >> 15` - both scaled by
/// the **acting** actor's rate byte `+0x21D`. The same arithmetic as the anim
/// tick's root-motion step ([`motion::root_motion_step`]).
fn swing_drift<H: BattleActionHost + ?Sized>(host: &mut H, ctx: &BattleActionCtx) {
    let slot = ctx.active_actor;
    let acting_party = usize::from(slot) < usize::from(host.party_count());
    let Some(actor) = host.actor(slot) else {
        return;
    };
    let (latched, facing, rate, target) = (
        actor.latched_anim,
        actor.facing_angle,
        actor.anim_rate.0,
        actor.active_target,
    );
    let bits = if acting_party {
        host.character_ability_bits(slot)
    } else {
        0
    };
    if !swing_drift_armed(
        acting_party,
        ctx.gauge_rearm_latch,
        ctx.super_trigger,
        latched,
        bits,
    ) {
        return;
    }
    let dt = host.frame_dt().clamp(0, 0xFF) as u8;
    let nudge = |host: &mut H, who: u8, angle: u16, speed: i16| {
        let Some((x, z)) = host.actor_position(who) else {
            return;
        };
        let (sin, cos) = motion::trig12(angle);
        let (dx, dz) = motion::root_motion_step(sin, cos, speed, dt, rate);
        host.set_actor_position(who, x.wrapping_add(dx as i16), z.wrapping_add(dz as i16));
    };
    nudge(host, slot, facing, -SWING_DRIFT_SPEED);
    let target_facing = host.actor(target).map(|a| a.facing_angle);
    if let Some(tf) = target_facing {
        nudge(host, target, tf, SWING_DRIFT_SPEED);
    }
}

/// PORT: FUN_801E295C (`0x801E39B4..0x801E3A64`) - the strike loop's
/// end-of-stream **Attack x2 refill**.
///
/// Reached on the step the stream terminator is read. Retail's guard chain,
/// in order (`s5` is `ctx + 0x11`, so `0x2(s5)` is `ctx[+0x13]` and `0x5(s5)`
/// is `ctx[+0x16]`):
///
/// ```text
/// 801e39bc  sltiu v0,v0,0x3        ; ctx[+0x13] < 3   - a party actor
/// 801e39fc  lw    v0,0x6bc(v0)     ; char record +0xF4
/// 801e3a04  andi  v0,v0,0x2000     ;   War God Icon "Attack x2"
/// 801e3a18  bne   v0,zero,801e3a74 ; ctx[+0x16] != 0  - the pair already ran
/// 801e3a38  sb    zero,0x4(s5)     ; strike cursor = 0
/// 801e3a40  sb    v0,0x5(s5)       ; ctx[+0x16] += 1
/// 801e3a4c  bne   v0,a1,801e3a58   ; marks[i] == 1
/// 801e3a54  _sb   a0,0x1df(v0)     ;   queue[i] = 0x19
/// ```
///
/// So the *whole* action stream replays once, with every **build-loop** marked
/// starter demoted from the newly-learned `0x1A` to the plain `0x19` - the
/// learn verdict fires on the first pass only.
///
/// The compare at `0x801E3A4C` is against `1`, not against "non-zero", and
/// that is the whole point: the builder's own accept loop writes
/// [`BUILD_STARTER_MARK`] at `0x801EF788`, while the Super tail-replace
/// (`FUN_801EF9E4`) writes [`SUPER_STARTER_MARK`] at every `0x1A` it stamps
/// (`0x801EFBA8`), *after* the reorder. So a Super Art's starter survives the
/// refill at `0x1A` and the second pass performs the Super, not a plain swing.
///
/// The marks come off the acting actor ([`BattleActor::starter_marks`], the
/// engine's carrier for retail's `0x801F6990`). A queue no builder produced -
/// a monster, a synthetic host - carries none, and then the build-loop marks
/// are reconstructed from the queue bytes by
/// [`crate::battle_action::build_starter_marks`] exactly as the reorder pass
/// reads them; a reconstruction can only ever yield `BUILD_STARTER_MARK`, so
/// that fallback is the pre-carrier behaviour and nothing else.
///
/// Returns `true` when the refill ran, in which case the band stays in
/// `AttackChain` and walks the stream again from byte 0.
///
/// This arm was previously read as a "Miracle continuation"; the guard chain
/// above is what settles it.
fn attack_x2_refill<H: BattleActionHost + ?Sized>(host: &mut H, ctx: &mut BattleActionCtx) -> bool {
    // Retail's literal is `ctx[+0x13] < 3`; the engine asks the host for the
    // seated party width instead, which is the same set of ordinals for a
    // full party and does not admit a monster seated at ordinal 2 in a
    // short one.
    let slot = ctx.active_actor;
    if usize::from(slot) >= usize::from(host.party_count()) {
        return false;
    }
    if host.character_ability_bits(slot) & WAR_GOD_ATTACK_X2_BIT == 0 {
        return false;
    }
    if ctx.attack_x2_pass != 0 {
        return false;
    }
    let Some(actor) = host.actor_mut(slot) else {
        return false;
    };
    let marks = match actor.starter_marks {
        Some(marks) => marks,
        None => {
            let mut queue = [0u8; ACTION_QUEUE_CAP];
            let n = actor.params.len().min(queue.len());
            queue[..n].copy_from_slice(&actor.params[..n]);
            build_starter_marks(&queue)
        }
    };
    let n = actor.params.len().min(marks.len());
    for (i, m) in marks.iter().enumerate().take(n) {
        // `bne v0,a1,0x801E3A58` with `a1 = 1`: an exact compare, so the
        // Super applier's `4` is skipped and its `0x1A` starter stands.
        if *m == BUILD_STARTER_MARK {
            actor.params[i] = REGULAR_STARTER;
        }
    }
    actor.strike_index = 0;
    ctx.attack_x2_pass = ctx.attack_x2_pass.saturating_add(1);
    true
}

pub(super) fn attack_recovery<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.pose(slot, Pose::Recover);
    // `0x1F` waits for `+0x1DC` bit 1 to clear (`0x801E3AEC..0x801E3AF8`) -
    // i.e. for the last staged byte's clip to commit - then stages idle over
    // it (`sb zero,0x1da` at `0x801E3B04`; the anim tick commits that at
    // the clip's own boundary) and parks the cursor at `0xFF`
    // (`0x801E3B14..0x801E3B1C`) on its way to `0x20`.
    let advance_done = host
        .actor(slot)
        .map(|a| a.flag_bits.has(ActorFlags::ADVANCE_DONE))
        .unwrap_or(false);
    if advance_done {
        return stay(ctx);
    }
    if let Some(actor) = host.actor_mut(slot) {
        actor.queued_anim = 0;
        actor.strike_index = STRIKE_CURSOR_PARKED;
    }
    transition(ctx, ActionState::AttackReturn)
}

pub(super) fn attack_return<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.pose(slot, Pose::Recover);
    // The counter window's gate, retail's order (`0x801E5554..0x801E557C`):
    // the scripted-fight flag `+0x287` first, then the counter byte `+0x288`.
    // (Retail's middle term, `DAT_8007BD0D`, has no port field.)
    if ctx.scripted_fight != 0 && ctx.counter_attack != 0 {
        // Counter-attack swap: advance the turn cursor past the counterer and
        // route back into AttackChain (retail `0x801E36D0`). Engines drive the
        // actual swap.
        ctx.turn_cursor = ctx.turn_cursor.saturating_add(1);
        return transition(ctx, ActionState::AttackChain);
    }
    stage_ko_taunt(host, slot);
    transition(ctx, ActionState::DoneCleanup)
}

/// The monster's **KO taunt** (`0x801E5594..0x801E5658`), run on the way into
/// the Done band: when a monster's attack has left its target at zero HP,
/// it stages its tag-[`KO_TAUNT_TAG`] entry with the stage latch (`+0x1DC |=
/// 2`) - but only while at least one party member still stands, so the
/// wiping blow plays no taunt. A monster carrying no such entry (most of the
/// roster) stages nothing, and a party attacker never reaches the lookup
/// (`sltiu v0,v0,0x3` at `0x801E559C`).
///
/// Retail's loop re-stages once per standing party slot; the writes are
/// identical, so one is the same state.
fn stage_ko_taunt<H: BattleActionHost + ?Sized>(host: &mut H, slot: u8) {
    let party_count = host.party_count();
    if slot < party_count {
        return;
    }
    let Some(target) = host.actor(slot).map(|a| a.active_target) else {
        return;
    };
    // `lhu v0,0x14c(s8)` - the target's HP halfword, whose zero the engine
    // carries as `liveness` (the capture / petrify paths clear it without
    // zeroing HP).
    if host.actor(target).is_none_or(|t| t.liveness != 0) {
        return;
    }
    let Some(taunt) = monster_action_by_tag(host, slot, KO_TAUNT_TAG)
        .filter(|&t| t != legaia_asset::monster_archive::NO_ACTION_ENTRY)
    else {
        return;
    };
    let party_standing = (0..party_count).any(|i| host.actor(i).is_some_and(|a| a.liveness != 0));
    if !party_standing {
        return;
    }
    if let Some(actor) = host.actor_mut(slot) {
        actor.queued_anim = taunt;
        actor.flag_bits.set(ActorFlags::ADVANCE_DONE);
    }
}

//! Run/escape and capture band of the battle-action state machine.

use super::*;

// --- run / capture band -----------------------------------------------------

/// Screen element `0x43`: the run band's outcome line (and the capture
/// band's caption). A top-of-screen framed line, seat A `(16, -24)` gliding
/// to seat B `(16, 14)`, width 288 (placement record `0x80076C10 + 0x43 *
/// 0x18`).
pub const RUN_MESSAGE_ELEMENT: u8 = 0x43;

pub(super) fn run_begin<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    ctx.frame_timer = 0x3C;
    // The outcome line: record `0x43`, whose content word `0x8007726C` the
    // roll pointed at the escaped (`ctx + 0x159`) or could-not-escape
    // (`ctx + 0x189`) buffer. The arm names it as the action's own element
    // (`sb 0x43,0x7(s5)` at `0x801E57F4`, `ctx[+0x18]`), so the Done band's
    // teardown glides it out after a failed run (`0x801E6188`).
    host.ui_element(RUN_MESSAGE_ELEMENT, 0);
    ctx.action_ui_element = RUN_MESSAGE_ELEMENT;
    // One Run is one roll for the whole party: the arm walks the party seats
    // (`i < ctx[+0]`, `0x801E5810..0x801E58A4`) and takes every other
    // member still owed a turn this round out of it - not the acting seat
    // (`ctx[+0x13]`), a key still live (`+0x16C`), alive (`+0x14C`), and not
    // the AI companion (`DAT_8007BD10[i] != 4`) - stepping the turn cursor
    // past it (`ctx[+0x1A]`) and zeroing its key. A failed run so spends the
    // party's round; the monsters still act.
    let active = ctx.active_actor;
    for slot in 0..host.party_count() {
        if slot == active || host.roster_character_id(slot) == 4 {
            continue;
        }
        if let Some(actor) = host.actor_mut(slot)
            && actor.init_key != 0
            && actor.hp != 0
        {
            actor.init_key = 0;
            ctx.turn_cursor = ctx.turn_cursor.wrapping_add(1);
        }
    }
    // PORT: FUN_801E295C case 0x64 (successful-escape branch). When the run
    // roll succeeded (retail tests `_DAT_8007726C != ctx + 0x189`; the port
    // carries the outcome on `absorbed_seru`, consumed later by RunWait),
    // retail walks the party slots (`i < ctx[+0]`) and floors every actor's
    // live HP `+0x14C` at 1 - downed (and, per the published behaviour,
    // petrified) members leave the battle alive. `+0x14C` maps to
    // `BattleActor::liveness` here. Engines additionally clear the Stone
    // status via `status_effects::StatusEffectTracker::cure_stone_on_escape`
    // when the battle ends `Escaped`.
    if ctx.absorbed_seru != 0 {
        for slot in 0..host.party_count() {
            if let Some(actor) = host.actor_mut(slot)
                && actor.liveness == 0
            {
                // Retail's store is `+0x14C = 1` - ONE halfword, which this
                // port models as both `liveness` (the flag-shaped reads) and
                // `hp` (the arithmetic reads). Flooring only the flag is
                // reverted by any driver that re-derives liveness from HP
                // (the live loop's dead-marking pass does, every tick), so
                // the member left the battle dead - the shape the published
                // "escape revives at 1 HP" behaviour contradicts.
                actor.liveness = 1;
                if actor.hp == 0 {
                    actor.hp = 1;
                    actor.hp_display = Some(1);
                }
            }
        }
    }
    transition(ctx, ActionState::RunWait)
}

pub(super) fn run_wait<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    // A failed run's message is skippable: on that arm only, a newly-pressed
    // or long-held button (`_DAT_8007B874 | _DAT_8007B938`,
    // `0x801E5978..0x801E5990`)
    // zeroes the countdown (`sh zero,0x2(s7)` at `0x801E59A0`) ahead of the
    // pass's decrement, so the press ends the wait on the same pass. The
    // successful escape's arm (`0x801E59A4`) reads no pad.
    if ctx.absorbed_seru == 0 && host.pad_word() | host.pad_held_32() != 0 && ctx.frame_timer > 0 {
        ctx.frame_timer = 0;
    }
    if !tick_frame_timer(host, ctx) {
        return stay(ctx);
    }
    // Retail 0x65 branches on the run outcome: a successful escape routes to
    // the 0x66 teardown (fade out + battle-end signal), a failed run routes
    // back to the Done band - the action is consumed and the battle
    // continues. The retail driver decides via a global the run roll set;
    // the port carries the outcome on `absorbed_seru` (non-zero = the
    // escape succeeded).
    if ctx.absorbed_seru != 0 {
        ctx.absorbed_seru = 0;
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

/// Captured-monster takedown (battle overlay 0898,
/// `ghidra/scripts/funcs/overlay_battle_action_801e7824.txt`).
///
/// PORT: FUN_801E7824
///
/// Applied to the captured monster's slot (the state-0x68 arm calls it with
/// `ctx[+0x13]`, the active actor). Per the dump:
///
/// - queued anim (`+0x1DA`) = the monster's tag-`1` walk entry
///   (`FUN_80050E2C(rec + 0x4C, 1, rec[0x4A])` at `0x801E7858`, through
///   [`monster_action_by_tag`]; `0xFF` when the record carries none);
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
pub fn capture_takedown<H: BattleActionHost + ?Sized>(host: &mut H, slot: u8) {
    let anim = monster_action_by_tag(host, slot, WALK_TAG);
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
    // `sb zero,0xd(v0)` in the delay slot of the coin's `jal 0x80056798`
    // (`0x801E5A9C..0x801E5AA4`): the flight is framed from the default
    // side, whatever variant the seed rolled; the case-6 call below reads it.
    ctx.camera_variant = 0;
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

/// State `0x6A` - the fleeing monster's hold (`0x801E5B5C..0x801E5C04`).
///
/// The step comes first (`lhu; subu; sh` at `0x801E5B64..0x801E5B74`), and
/// only a count that has gone negative meets the streaming clamp: while the
/// side-band applier stage `ctx[+0x276]` is up the word is pinned back at
/// `1` (`0x801E5B84..0x801E5B9C`) and the band holds, so it never leaves
/// while a stream is in flight, whatever the frame step.
///
/// PORT: FUN_801E295C (`0x801E5B5C..0x801E5C04`)
pub(super) fn capture_sustain<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    ctx.frame_timer = ctx.frame_timer.wrapping_sub(host.frame_dt());
    if ctx.frame_timer >= 0 {
        return stay(ctx);
    }
    if ctx.menu_open != 0 {
        ctx.frame_timer = 1;
        return stay(ctx);
    }
    ctx.frame_timer = 0x3C;
    host.ui_element(0x43, 1);
    // `sw zero,0x4(s3)` in the delay slot of the framing call at
    // `0x801E5BDC..0x801E5BE0`: the fled monster's prim word is cleared.
    if let Some(a) = host.actor_mut(ctx.active_actor) {
        a.render_color = 0;
    }
    // No `+0x1A` write here: the dispatcher's only four accesses to the turn
    // cursor are `Begin`, the counter-attack swap, the run arm's skip loop and
    // the end-of-action gate (see `BattleActionCtx::turn_cursor`). The zero
    // this arm used to perform came with the superseded reading that put the
    // counter on the actor record.
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

/// Round boundary - the `0xFF` arm of `FUN_801E295C` (`801e67e8`).
///
/// Retail reaches this state only through the **non-wipe** arm of the `0x5A`
/// end-of-action gate: every living actor has acted and BOTH sides still have
/// someone standing. Its body parks the flow byte at `0x14` (`ctx[+0x6]`, the
/// round driver), bumps the round counter `ctx[+0x28A]` and calls
/// `func_0x801F45A4` (the per-round status-`0x400` settle); the next round's
/// actor selection follows. It never signals battle end - the wipe arms and
/// the escape teardown do that through `DAT_8007BD71 = 0xFE`, which the port
/// surfaces as the `battle_end(..)` + `StepOutcome::BattleComplete` pair on
/// those paths. See `docs/subsystems/battle-action.md`
/// § "`0xFF` is the round boundary, not the battle's end".
///
/// The port hosts the retail body on the driver side (`engine-core`'s live
/// loop runs `advance_battle_mode` - the `ctx[+0x28A]` bump - and
/// `tick_status_0x400_wakes` - `FUN_801F45A4` - at its round boundary), so
/// this handler does the SM-side bookkeeping only: rewind the turn cursor
/// (`+0x1A`) for the new round and hand control back through `EndOfAction`,
/// the state the arming driver keys the next turn on.
///
/// The rewind is the **engine's**, not retail's: retail reseeds the cursor by
/// re-entering `Begin`, which the battle flow SM `FUN_801D0748` arms and the
/// port does not drive from here. Retail's own `0xFF` body touches `+0x1A` not
/// at all.
///
/// PORT: FUN_801E295C case 0xFF (round boundary, `801e67e8`)
/// REF: FUN_801D0748 (the battle flow SM that re-arms `Begin` in retail)
pub(super) fn round_end<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let _ = host;
    ctx.turn_cursor = 0;
    transition(ctx, ActionState::EndOfAction)
}

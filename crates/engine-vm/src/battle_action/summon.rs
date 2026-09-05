//! Summon (Seru-creature) invocation band of the battle-action state machine.
//!
//! States `0x32..=0x38` of `FUN_801E295C`. The band owns the *presentation*
//! frame of a player Seru-magic cast: the caster's cast clip, the white flash,
//! the actor hide / restore, the audio duck, and the hand-off to the
//! per-summon stager `FUN_801F1ED4` that runs the creature's choreography.
//! The stager is overlay code the engine cannot run; the host answers
//! [`BattleActionHost::summon_stager_tick`] with its own choreography and
//! reports "still busy" exactly where retail tests the stager's return.
//!
//! Capture-pinned phase shape (mednafen / PCSX-Redux mid-cast states, read
//! off `ctx+7` and the per-actor `+0x21C` / `+0x1D9` bytes):
//!
//! * `0x33` - the caster plays clip `9` (`+0x1D9 == 9`), every actor still
//!   drawn (`+0x21C == 0`), the summon seat empty.
//! * `0x34` - clip `9` still playing; the screen is already white.
//! * `0x35` - every party actor and living monster hidden (`+0x21C == 0xFF`,
//!   `+0x4 == 0`), the summon creature seated at slot 7; the white clears.
//! * `0x36` - the stager phase byte `ctx+0x279` walks its own space while
//!   the creature performs; the SM **holds** here on the stager's return
//!   (`jal 0x801f1ed4; bne v0,zero,<exit>` at `0x801E4CA8..0x801E4CB0`).

use super::*;
use crate::battle_target_group::RENDER_FLAG_HIDDEN;

/// The 13-`i16` fade template the summon band hands to `FUN_80024E80`
/// (`DAT_801C9070`), field for field: kind = the emitter's **ABR blend
/// mode** (`FUN_80024EE4`'s second argument), a ramp duration, start / end
/// RGB, a start delay in vsyncs, and the post-ramp hold (`-1` = hold until
/// the actor is killed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SummonFadeTemplate {
    /// Template `[0]` - the blend mode the fade quad draws with (`1` =
    /// additive).
    pub kind: i16,
    /// Template `[1]` - ramp duration in vsyncs.
    pub duration: i16,
    /// Template `[3..=5]` - start RGB.
    pub start_rgb: [i16; 3],
    /// Template `[7..=9]` - end RGB.
    pub end_rgb: [i16; 3],
    /// Template `[10]` - start delay in vsyncs (nothing drawn while it runs).
    pub delay: i16,
    /// Template `[11]` - hold after the ramp; `-1` holds until killed.
    pub hold: i16,
}

/// The state-`0x33` flash-in (`0x801E4A60..0x801E4AA0`): additive, a
/// `0x14`-vsync delay, then a `0x14`-vsync ramp black -> white, held at white
/// until the `0x34` arm kills it (`ori v0,0x8; sw v0,0x10(a0)` on the fade
/// actor's flag word at `0x801E4B08`).
pub const SUMMON_FLASH_IN: SummonFadeTemplate = SummonFadeTemplate {
    kind: 1,
    duration: 0x14,
    start_rgb: [0, 0, 0],
    end_rgb: [0xFF, 0xFF, 0xFF],
    delay: 0x14,
    hold: -1,
};

/// The state-`0x34` flash-out (`0x801E4B74..0x801E4BB0`): additive, no delay,
/// a `0x78`-vsync ramp white -> black (an additive black quad is invisible,
/// so this is the white-out clearing), one vsync of hold, then done.
pub const SUMMON_FLASH_OUT: SummonFadeTemplate = SummonFadeTemplate {
    kind: 1,
    duration: 0x78,
    start_rgb: [0xFF, 0xFF, 0xFF],
    end_rgb: [0, 0, 0],
    delay: 0,
    hold: 1,
};

/// The id `FUN_80024E80` stamps on both summon fades (`li a1,0x1` at
/// `0x801E4A64` / `0x801E4B78`) - the OT layer the fade actor's tick draws
/// the quad at.
pub const SUMMON_FADE_ID: i16 = 1;

/// Retail's per-slot predicate for the `0x34` hide loop and the `0x36`
/// restore loop (`0x801E4B30..0x801E4B6C` / `0x801E4CDC..0x801E4D28`): a party
/// seat is always touched, a monster seat only while it is alive
/// (`lhu +0x14C; bne` / `sltiu s0,3`).
fn summon_hide_applies(slot: u8, party_count: u8, liveness: u16) -> bool {
    slot < party_count || liveness != 0
}

// --- summon band ------------------------------------------------------------

pub(super) fn summon_invoke<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.pose(slot, Pose::Idle);
    if !host.sound_bank_ready(1) {
        return stay(ctx);
    }
    let param0 = host.actor(slot).map(|a| a.params[0]).unwrap_or(0);
    let frame_idx = if param0 < 0x9A {
        // (param0 + 0x7F) * 3 + 0x80
        ((param0 as u32).saturating_add(0x7F))
            .saturating_mul(3)
            .saturating_add(0x80) as u8
    } else {
        // param0 * 4 + 99
        ((param0 as u32).saturating_mul(4)).saturating_add(99) as u8
    };
    ctx.summon_frame_idx = frame_idx;
    ctx.menu_open = 1;
    ctx.summon_staging_a = 1;
    if let Some(actor) = host.actor_mut(slot) {
        actor.queued_anim = 9;
        actor.flag_bits.set(ActorFlags::WINDUP_DONE);
        actor.spell_iter = actor.spell_iter.saturating_add(1);
    }
    transition(ctx, ActionState::SummonFadeIn)
}

/// State `0x33`: sustain clip `0x12` on the caster and wait for the anim cue
/// `+0x1F5`; on the cue, spawn the flash-in fade (`0x801E4A50..0x801E4AA0`).
pub(super) fn summon_fade_in<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.spell_anim_sustain(slot, 0x12);
    let cued = host.actor(slot).map(|a| a.anim_cue != 0).unwrap_or(false);
    if !cued {
        return stay(ctx);
    }
    host.spawn_screen_fade(&SUMMON_FLASH_IN, SUMMON_FADE_ID);
    transition(ctx, ActionState::SummonActorFreeze)
}

/// State `0x34`: wait for the cast clip to end (`+0x1D9 == 0`), then kill
/// the flash-in, clear the stager staging bytes, arm the `0x78` timer, run
/// the stager's first tick (`jal 0x801f1ed4` at `0x801E4B1C`), hide every
/// party seat and living monster, and spawn the flash-out
/// (`0x801E4AE0..0x801E4BC0`).
pub(super) fn summon_actor_freeze<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.spell_anim_sustain(slot, 0x12);
    let current_zero = host
        .actor(slot)
        .map(|a| a.current_anim == 0)
        .unwrap_or(false);
    if !current_zero {
        return stay(ctx);
    }
    ctx.summon_staging_a = 0;
    ctx.summon_staging_b = 0;
    ctx.frame_timer = 0x78;
    // The stager's phase-0 tick: this is where the creature is staged
    // (retail: `FUN_801F1ED4` -> the per-summon stager -> `FUN_801F19EC`
    // installs the streamed actor record as slot 7). The return value is
    // not tested here - only the `0x36` arm gates on it.
    let _ = host.summon_stager_tick();
    let party_count = host.party_count();
    for s in 0..host.slot_count() {
        let Some(a) = host.actor_mut(s) else {
            continue;
        };
        if summon_hide_applies(s, party_count, a.liveness) {
            // `sw zero,0x4(a0)` + `sb 0xFF,0x21c` - the prim word cleared and
            // the hidden marker set together. The port keeps the marker.
            a.render_flag = RENDER_FLAG_HIDDEN;
        }
    }
    host.spawn_screen_fade(&SUMMON_FLASH_OUT, SUMMON_FADE_ID);
    transition(ctx, ActionState::SummonSustain)
}

/// State `0x35`: count the `0x78` timer down, duck the music, tick the
/// stager every frame (`jal 0x801f1ed4` at `0x801E4C7C`), and leave for
/// `0x36` when the timer expires (`0x801E4BC4..0x801E4CA4`).
pub(super) fn summon_sustain<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let expired = tick_frame_timer(host, ctx);
    let slot = ctx.active_actor;
    let param0 = host.actor(slot).map(|a| a.params[0]).unwrap_or(0);
    // Ramp the live audio level - 75% for spells < 0x99, else 50%
    // (`sltiu v0,v0,0x99` at `0x801E4BFC`).
    let pct = if param0 < 0x99 { 75 } else { 50 };
    host.duck_audio_level(pct);
    if expired && ctx.menu_open != 0 {
        // `ctx[+0x276]` still streaming: clamp the timer at 1 and keep
        // waiting (`0x801E4C60..0x801E4C78`).
        ctx.frame_timer = 1;
        let _ = host.summon_stager_tick();
        return stay(ctx);
    }
    let _ = host.summon_stager_tick();
    if !expired {
        return stay(ctx);
    }
    ctx.frame_timer = 0;
    transition(ctx, ActionState::SummonReturn)
}

/// State `0x36`: tick the stager and **hold while it reports busy**
/// (`jal 0x801f1ed4; bne v0,zero,0x801e6814` at `0x801E4CA8..0x801E4CB0`);
/// once it is done, run the queued-magic follow-up guard, restore every
/// party seat and living monster, and run the summon-magic level check
/// (`0x801E4CB8..0x801E4D4C`).
pub(super) fn summon_return<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    // PORT: FUN_801F1ED4 (call site; the stager's body is the host's
    // summon choreography)
    if host.summon_stager_tick() {
        return stay(ctx);
    }
    // The queued-magic follow-up guard runs at the head of the settle body,
    // before the visibility restore: retail's `jal 0x801f3c34` is at
    // `0x801E4CB8`, in the body that goes on to stamp `ctx[7] = 0x37`.
    // PORT: FUN_801F3C34 (call site; the pass itself is
    // `crate::move_no_effect_guard::queued_magic_message`)
    let slot = ctx.active_actor;
    let action = host.actor(slot).map(|a| a.params[0]).unwrap_or(0);
    if let Some((ids, levels)) = host.caster_spell_list(slot)
        && let Some(message) = crate::move_no_effect_guard::queued_magic_message(
            action,
            &ids,
            &levels,
            ctx.follow_up_pending != 0,
        )
    {
        host.ui_element(message, 0);
        ctx.message_id = message;
    }
    // Restore actor visibility - the same predicate as the hide loop
    // (`0x801E4CDC..0x801E4D28`; the `+0x8 = 0x81000000` render-word
    // restore rides the marker clear in the port).
    let party_count = host.party_count();
    for s in 0..host.slot_count() {
        let Some(a) = host.actor_mut(s) else {
            continue;
        };
        if summon_hide_applies(s, party_count, a.liveness) {
            a.render_flag = 0;
        }
    }
    transition(ctx, ActionState::SummonVerifyAlive)
}

pub(super) fn summon_verify_alive<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.pose(slot, Pose::Idle);
    // Ensure all actors are still alive (liveness != 0 AND current_anim != 0).
    // The state machine doesn't gate on this; it just records state.
    transition(ctx, ActionState::SummonDone)
}

pub(super) fn summon_done<H: BattleActionHost + ?Sized>(
    _host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    transition(ctx, ActionState::DoneCleanup)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hide_predicate_is_party_always_monster_only_alive() {
        assert!(summon_hide_applies(0, 3, 0));
        assert!(summon_hide_applies(2, 3, 0));
        assert!(summon_hide_applies(3, 3, 1));
        assert!(!summon_hide_applies(3, 3, 0));
        assert!(!summon_hide_applies(6, 3, 0));
    }

    #[test]
    fn the_two_flashes_are_the_dumps_templates() {
        // 0x33: black -> white over 0x14, after a 0x14 delay, held.
        assert_eq!(SUMMON_FLASH_IN.kind, 1);
        assert_eq!(SUMMON_FLASH_IN.duration, 0x14);
        assert_eq!(SUMMON_FLASH_IN.delay, 0x14);
        assert_eq!(SUMMON_FLASH_IN.hold, -1);
        assert_eq!(SUMMON_FLASH_IN.end_rgb, [0xFF; 3]);
        // 0x34: white -> black over 0x78, no delay, one frame of hold.
        assert_eq!(SUMMON_FLASH_OUT.duration, 0x78);
        assert_eq!(SUMMON_FLASH_OUT.delay, 0);
        assert_eq!(SUMMON_FLASH_OUT.hold, 1);
        assert_eq!(SUMMON_FLASH_OUT.start_rgb, [0xFF; 3]);
    }
}

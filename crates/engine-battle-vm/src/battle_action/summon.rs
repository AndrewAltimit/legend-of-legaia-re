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

/// The fade actor's [`legaia_engine_system::fade::FadeTemplate`] off the summon band's 13-word template layout
/// (`DAT_801C9070`).
pub fn summon_fade_template(t: &SummonFadeTemplate) -> legaia_engine_system::fade::FadeTemplate {
    legaia_engine_system::fade::FadeTemplate {
        kind: t.kind,
        duration: t.duration,
        start_rgb: t.start_rgb,
        end_rgb: t.end_rgb,
        mode: [t.delay, t.hold, 0],
    }
}

/// The sound cue the flash-in arm fires the frame it spawns the fade
/// (`li a0,0x63; jal 0x8004fcc8` at `0x801E4AA8..0x801E4AB8`, right after
/// `ctx[7] = 0x34`).
pub const SUMMON_FLASH_CUE: u16 = 0x63;

/// The spell-id split inside the party cast trigger `FUN_801DBF9C`
/// (`sltiu v0,a1,0x25; beq v0,zero,0x801dc064` at `0x801DBFA0`): an id at
/// or above this takes the summon arm - `actor[+0x1E0] = 9` (the sub-route
/// byte the pre-cast wait tests to reach [`ActionState::SummonInvoke`]),
/// `+0x1E1 = 0x12`, `+0x1E2 = 0xFF` - so **every** player Seru id
/// (`0x81..=0xA0`) is a summon in retail's eyes, healing casts included. An
/// id below it copies a per-spell anim-pair list out of the overlay table
/// at `0x801F4E64` / `0x801F4EDC` into `params[1..]` instead.
///
/// REF: FUN_801DBF9C
pub const SPELL_TRIGGER_SUMMON_MIN_ID: u8 = 0x25;

/// The anim byte the trigger's summon arm stages at `params[2]` (`+0x1E1`)
/// (`li v1,0x12; sb v1,0x1e1(a0)` at `0x801DC088`): the argument the band
/// then hands `FUN_801DC0A0` every frame of `0x33` / `0x34` while the caster
/// stays on clip `9` - a cast-effect id, not a clip id.
///
/// REF: FUN_801DBF9C
pub const SUMMON_CAST_EFFECT_ID: u8 = 0x12;

/// Retail's per-slot predicate for the `0x34` hide loop and the `0x36`
/// restore loop (`0x801E4B30..0x801E4B6C` / `0x801E4CDC..0x801E4D28`): a party
/// seat is always touched, a monster seat only while it is alive
/// (`lhu +0x14C; bne` / `sltiu s0,3`).
fn summon_hide_applies(slot: u8, party_count: u8, liveness: u16) -> bool {
    slot < party_count || liveness != 0
}

/// `FUN_801DC0A0(caster, 0x12)` - the summon close-up the `0x33` / `0x34`
/// passes call every frame. Besides arming its camera (the host's), case
/// `0x12` stores `ctx[+0x243] = 1` (`li v0,0x1; sb v0,0x243(v1)` at
/// `0x801DCCF8..0x801DCCFC`) before it reads the ramp - the latch the
/// battle ambient's per-frame ramp dims on
/// ([`crate::battle_ground_grid::ambient_base_step`]).
///
/// REF: FUN_801DC0A0 (case `0x12`'s `+0x243` store)
fn summon_close_up<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
    slot: u8,
) {
    ctx.gauge_rearm_latch = 1;
    host.spell_anim_sustain(slot, SUMMON_CAST_EFFECT_ID);
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

/// Vsyncs the transfer SM `FUN_801F17F8` spends on one `0x10800`-byte slot
/// of `summon.dat` / `readef.DAT` (open, seek, read, close), as the applier
/// stage `ctx[+0x276]` measures it on a PCSX-Redux capture: the four-slot
/// Mule seed lands `119` vsyncs after `0x32` stores it, a two-slot `readef`
/// turn seed `62`. The engine's streams are resident, so this is the CD
/// cost the applier's token stands in for, not a read.
pub const SIDEBAND_SLOT_READ_VSYNCS: u16 = 30;

/// How many slots the side-band applier `FUN_801F12D0` streams from base
/// byte `ctx[+0x277]` before it drops its stage `ctx[+0x276]` to `0`: two
/// (`base`, `base + 1`) for a `readef` group, a third for a `summon.dat`
/// base (bit 7 set, or `0x36`; the stage-4 tests at `0x801F163C..`), and a
/// fourth when that base is `0xCB` or above (stage 6, `sltiu 0xcb`).
///
/// PORT: FUN_801F12D0 (the stage chain's slot count only - partial; the
/// uploads are `legaia_actor_formats::summon_readef`'s)
pub fn sideband_slot_count(base: u8) -> u8 {
    if base & 0x80 == 0 && base != 0x36 {
        2
    } else if base < 0xCB {
        3
    } else {
        4
    }
}

/// The vsyncs the applier's stage byte stays up after a seed of `base`:
/// [`sideband_slot_count`] reads of [`SIDEBAND_SLOT_READ_VSYNCS`] each.
pub fn sideband_token_vsyncs(base: u8) -> u16 {
    u16::from(sideband_slot_count(base)) * SIDEBAND_SLOT_READ_VSYNCS
}

/// State `0x33`: sustain clip `0x12` on the caster and wait for the anim cue
/// `+0x1F5`; on the cue, spawn the flash-in fade (`0x801E4A50..0x801E4AA0`).
pub(super) fn summon_fade_in<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    // Every pass stores framing style `1` ahead of the close-up call
    // (`li v1,0x1; sb v1,0xd(v0)` at `0x801E4A3C..0x801E4A40`). Nothing in
    // `0x34` / `0x35` / `0x36` rewrites it, so it is the style a module's
    // case-6 walk-in (PROT 0903 arm 11) frames with: half a turn on the yaw,
    // the creature seen from in front.
    ctx.camera_variant = 1;
    summon_close_up(host, ctx, slot);
    let cued = host.actor(slot).map(|a| a.anim_cue != 0).unwrap_or(false);
    if !cued {
        return stay(ctx);
    }
    host.spawn_screen_fade(&SUMMON_FLASH_IN, SUMMON_FADE_ID);
    // `0x801E4AA8..0x801E4AB8`: the flash cue goes out on the same frame,
    // after the state byte is stamped. The `sb zero,0x1da(s3)` in the call's
    // delay slot clears retail's *staged* byte, whose "nothing staged" zero
    // is not the port's "go idle" zero (`commit_staged_battle_anim` would
    // cut clip 9 short) - so the port leaves the staged id alone and lets
    // the clip run to its end, which is what `0x34` waits on.
    host.one_shot_sfx(SUMMON_FLASH_CUE);
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
    summon_close_up(host, ctx, slot);
    let current_zero = host
        .actor(slot)
        .map(|a| a.current_anim == 0)
        .unwrap_or(false);
    if !current_zero {
        return stay(ctx);
    }
    ctx.summon_staging_a = 0;
    ctx.summon_staging_b = 0;
    // `li v0,0x78` / `sh v0,0x2(s7)` in the delay slot of the stager call
    // (`0x801E4B10..0x801E4B20`): the word is armed before the module's
    // first tick sees it.
    ctx.frame_timer = 0x78;
    host.summon_band_word_in(ctx.frame_timer);
    // The stager's phase-0 tick: this is where the creature is staged
    // (retail: `FUN_801F1ED4` -> the per-summon stager -> `FUN_801F19EC`
    // installs the streamed actor record as slot 7). The return value is
    // not tested here - only the `0x36` arm gates on it.
    let _ = host.summon_stager_tick();
    if let Some(word) = host.summon_band_word_out() {
        ctx.frame_timer = word;
    }
    let party_count = host.party_count();
    for s in 0..host.slot_count() {
        let Some(a) = host.actor_mut(s) else {
            continue;
        };
        if summon_hide_applies(s, party_count, a.liveness) {
            // `sw zero,0x4(a0)` + `sb 0xFF,0x21c` - the prim word cleared and
            // the hidden marker set together. The zero word is what hides
            // the body; a module that raises the marker over a lit word
            // (PROT 0912's arms 7 and `0x10`) keeps it drawn.
            a.render_color = 0;
            a.render_flag = RENDER_FLAG_HIDDEN;
        }
    }
    host.spawn_screen_fade(&SUMMON_FLASH_OUT, SUMMON_FADE_ID);
    transition(ctx, ActionState::SummonSustain)
}

/// State `0x35`: count the band word `ctx[+0x6D8]` down, duck the music,
/// tick the stager every frame (`jal 0x801f1ed4` at `0x801E4C7C`), and leave
/// for `0x36` once the word reads negative **after** the stager call
/// (`0x801E4BC4..0x801E4CA4`).
///
/// The order is the point. The drain (`lhu; subu; sh` at
/// `0x801E4BCC..0x801E4BDC`) and the streaming clamp (`ctx[+0x276]` set:
/// the word is pinned at `1`, `0x801E4C50..0x801E4C78`) come first, then
/// the stager, then `lh v0,0x2(s7); bgez` re-reads the word. A module whose
/// arm countdown is that word (PROT 0904 / 0907 / 0908) re-seeds it from
/// inside the call, so the band stays in `0x35` across those arms, with the
/// SM's own drain added to the module's.
pub(super) fn summon_sustain<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let dt = host.frame_dt();
    ctx.frame_timer = ctx.frame_timer.wrapping_sub(dt);
    let slot = ctx.active_actor;
    let param0 = host.actor(slot).map(|a| a.params[0]).unwrap_or(0);
    // Ramp the live audio level - 75% for spells < 0x99, else 50%
    // (`sltiu v0,v0,0x99` at `0x801E4BFC`).
    let pct = if param0 < 0x99 { 75 } else { 50 };
    host.duck_audio_level(pct);
    if ctx.frame_timer < 0 && ctx.menu_open != 0 {
        // `ctx[+0x276]` still streaming: clamp the word at 1 and keep
        // waiting (`0x801E4C60..0x801E4C78`).
        ctx.frame_timer = 1;
    }
    host.summon_band_word_in(ctx.frame_timer);
    let _ = host.summon_stager_tick();
    if let Some(word) = host.summon_band_word_out() {
        ctx.frame_timer = word;
    }
    if ctx.frame_timer >= 0 {
        return stay(ctx);
    }
    // `sh zero,0x2(s7)` beside the `0x36` store (`0x801E4C9C`).
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

/// The framing depth `ctx[+0x6D0]` the summon band's `0x37` exit stores
/// (`li v0,0x800` at `0x801E4E6C`).
pub const SUMMON_EXIT_FRAME_HEIGHT: i16 = 0x800;

/// The framing depth the pre-cast wait stores when it routes a cast to the
/// summon band (`li v0,0x600` / `sh v0,0x6d0(v1)` at `0x801E45F8..0x801E4604`).
pub const SUMMON_ROUTE_FRAME_HEIGHT: i16 = 0x600;

pub(super) fn summon_verify_alive<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.pose(slot, Pose::Idle);
    // Ensure all actors are still alive (liveness != 0 AND current_anim != 0).
    // The state machine doesn't gate on this; it just records state.
    // The exit (`0x801E4E60..0x801E4EA4`) drops the close-up latch
    // `ctx[+0x243]` and re-seeds the ambient base one step above the
    // floor, so the battle ambient climbs back from dark.
    ctx.gauge_rearm_latch = 0;
    ctx.summon_staging_a = 0;
    host.summon_band_exit();
    ctx.ambient_base = crate::battle_ground_grid::AMBIENT_BASE_CAST_EXIT;
    // Every pass stores style `1` before its case-6 call (`sb v1,0xd(v0)` at
    // `0x801E4D5C`), and the exit sets the framing depth back to the near
    // `0x800` (`0x801E4E6C..0x801E4E70`): the Done band after a summon frames
    // the caster there (`shiny_refactor_gimard_levelup` reads
    // `ctx[+0x6D0] = 0x800` in `0x51`).
    ctx.camera_variant = 1;
    ctx.camera_frame_height = SUMMON_EXIT_FRAME_HEIGHT;
    host.camera_frame_height(SUMMON_EXIT_FRAME_HEIGHT);
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

    /// The applier's slot count off its base byte: two for a `readef`
    /// group, three for a `summon.dat` base below `0xCB`, four above.
    #[test]
    fn sideband_slot_count_follows_the_applier_stage_tests() {
        assert_eq!(sideband_slot_count(0x12), 2);
        assert_eq!(sideband_slot_count(0x36), 3);
        assert_eq!(sideband_slot_count(0x80), 3);
        assert_eq!(sideband_slot_count(0xCA), 3);
        // Mule, spell 0x9B: 0x9B * 4 + 99 = 0x2CF, byte 0xCF.
        assert_eq!(sideband_slot_count(0xCF), 4);
        assert_eq!(sideband_token_vsyncs(0xCF), 4 * SIDEBAND_SLOT_READ_VSYNCS);
    }

    #[test]
    fn hide_predicate_is_party_always_monster_only_alive() {
        assert!(summon_hide_applies(0, 3, 0));
        assert!(summon_hide_applies(2, 3, 0));
        assert!(summon_hide_applies(3, 3, 1));
        assert!(!summon_hide_applies(3, 3, 0));
        assert!(!summon_hide_applies(6, 3, 0));
    }

    /// A module that keeps its countdown in the band word: it reads the
    /// SM's value, and on the pass the word reaches zero it re-seeds it.
    struct SharedWord {
        word: i16,
        reseed: Option<i16>,
        shared: bool,
    }
    impl BattleActionHost for SharedWord {
        fn actor(&self, _: u8) -> Option<&BattleActor> {
            None
        }
        fn actor_mut(&mut self, _: u8) -> Option<&mut BattleActor> {
            None
        }
        fn summon_band_word_in(&mut self, word: i16) {
            self.word = word;
        }
        fn summon_stager_tick(&mut self) -> bool {
            if self.word == 0
                && let Some(n) = self.reseed.take()
            {
                self.word = n;
            }
            true
        }
        fn summon_band_word_out(&mut self) -> Option<i16> {
            self.shared.then_some(self.word)
        }
    }

    fn sustain_passes(host: &mut SharedWord) -> u32 {
        let mut ctx = BattleActionCtx {
            action_state: ActionState::SummonSustain.as_byte(),
            frame_timer: 0x78,
            ..Default::default()
        };
        let mut passes = 0;
        while ctx.action_state == ActionState::SummonSustain.as_byte() {
            summon_sustain(host, &mut ctx);
            passes += 1;
            assert!(passes < 1000, "0x35 never left");
        }
        assert_eq!(ctx.frame_timer, 0, "the exit zeroes the word");
        passes
    }

    /// `0x35` leaves on the word read back after the stager call
    /// (`0x801E4C84`): a module re-seeding the shared word holds the band,
    /// and the SM's own drain keeps running beside the module's.
    #[test]
    fn sustain_exit_reads_the_word_after_the_stager_call() {
        let own = sustain_passes(&mut SharedWord {
            word: 0,
            reseed: None,
            shared: true,
        });
        assert_eq!(own, 0x79, "0x78 drains to -1 on the 0x79th pass");
        let held = sustain_passes(&mut SharedWord {
            word: 0,
            reseed: Some(40),
            shared: true,
        });
        assert_eq!(held, 0x78 + 41, "the re-seed of 40 adds 41 passes");
        let apart = sustain_passes(&mut SharedWord {
            word: 0,
            reseed: Some(40),
            shared: false,
        });
        assert_eq!(apart, 0x79, "a module with its own word does not hold");
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

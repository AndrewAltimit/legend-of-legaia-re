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
    // The Done band's own countdown. All three of retail's `0x50` paths
    // converge on `li v0,0x3c` / `sh v0,0x2(s7)` (`0x801E5EE8` / `0x801E5EFC`
    // / `0x801E5F24` into `0x801E5F28`), so `0x3C` is the seed regardless of
    // which arm the action category took. The one override is
    // `lbu v0,0x15(s5)` / `beq v0,zero` / `li v0,0x96` / `sh v0,0x2(s7)` at
    // `0x801E5F2C..0x801E5F3C`: a non-zero `ctx[+0x26]` - the level-up banner
    // element ([`BattleActionCtx::levelup_banner_element`]) - re-seeds `0x96`
    // instead, so the "magic level increased" banner the same action raised
    // stays up long enough to read.
    ctx.frame_timer = if ctx.levelup_banner_element != 0 {
        DONE_LEVELUP_BANNER_FRAMES
    } else {
        DONE_SEED_FRAMES
    };
    // `sb zero,0x6(s5)` at `0x801E5F60` - the band's UI-teardown latch is
    // armed here, so the `0x51` tail's unload block runs once for this
    // action and not again.
    ctx.done_ui_torn_down = 0;

    // Per-category pose: run → screen-shake; attack → pose 8; otherwise idle.
    match category {
        ActionCategory::Run => host.screen_shake(0x500),
        ActionCategory::Attack => host.pose(slot, Pose::Recover),
        _ => host.pose(slot, Pose::Idle),
    }

    rearm_action_gauge(host, ctx);

    // The `0x51` arm ramps the live audio level `_DAT_8007B910` back up to
    // its reference every frame (`docs/subsystems/battle-action.md` § the
    // `_DAT_8007B910` ramps are an audio duck); the host owns the ramp, so
    // the target is handed over once, on entry.
    host.duck_audio_level(100);
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
            slots.arm_width[i] = a.anim_rate.get();
        }
    }
    if !rearm_gauge(staged, &mut slots) {
        return;
    }
    for i in 0..GAUGE_SLOTS {
        if let Some(a) = host.actor_mut(i as u8) {
            a.render_flag = slots.latch[i];
            a.anim_rate = crate::battle_anim_rate::AnimRate(slots.arm_width[i]);
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
/// - target `8` ("all"): scans slots `0 .. ctx[+0x00] - 1`, pending if any
///   pair differs. `ctx[+0x00]` is the **party member count** (measured `3`),
///   not the total actor count - so even the all-target arm inspects the party
///   side only, which is the same conclusion the `3..=7` early-out reaches by a
///   different route: a monster's readout can never hold this gate;
/// - target `> 8`: never pending.
///
/// The engine models the retail `+0x14C`-vs-`+0x172` pair as
/// [`BattleActor::hp`] vs [`BattleActor::hp_display`] (`None` = settled), and
/// the third field the pair converges through as
/// [`BattleActor::hp_bar_pending`] - see [`tick_hp_bars`].
pub fn hp_bar_drain_pending<H: BattleActionHost + ?Sized>(host: &H, ctx: &BattleActionCtx) -> bool {
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
        8 => (0..host.party_count()).any(pending),
        _ => false,
    }
}

/// Run one frame of HP-bar ramp across every battle slot - the per-actor tick
/// that makes [`hp_bar_drain_pending`] a *gate* rather than a constant.
///
/// Retail spreads this over per-actor calls to `FUN_80047430`; the caller that
/// makes them is **not in the dumped corpus** (no dump references
/// `0x80047430` as a `jal` target), so the cadence here is the port's own
/// choice: once per battle frame, every slot, before the action SM steps. What
/// *is* disassembly-grounded is the arithmetic and the slot split - see
/// [`crate::battle_hp_bar`].
///
/// REF: FUN_80047430 (the HP-bar ramp arm; kernel in `battle_hp_bar`)
pub fn tick_hp_bars<H: BattleActionHost + ?Sized>(host: &mut H) {
    for slot in 0..host.slot_count() {
        if let Some(actor) = host.actor_mut(slot) {
            actor.tick_hp_bar(slot);
        }
    }
}

/// Rebuild the four cast-census bytes on `ctx` from the actor pool and the
/// host's effect-child arrays - one frame of `FUN_801E09F8`'s head.
///
/// This is what turns [`BattleActionCtx::magic_exit_gate`] (`+0x249`),
/// [`BattleActionCtx::magic_recovery_gate`] (`+0x24D`) and the two retarget
/// latches [`BattleActionCtx::item_target_a`] / [`BattleActionCtx::item_target_b`]
/// (`+0x24A` / `+0x24B`) into live measurements. Retail recomputes them from
/// zero every frame; so does this.
///
/// The arithmetic lives in [`crate::battle_cast_census`], which carries the
/// `// PORT:` tag.
///
/// REF: FUN_801E09F8 (census head)
pub fn tick_cast_census<H: BattleActionHost + ?Sized>(host: &H, ctx: &mut BattleActionCtx) {
    use crate::battle_cast_census::{CENSUS_SLOTS, CensusSlot, cast_census};

    let mut slots = [CensusSlot::default(); CENSUS_SLOTS];
    for (i, s) in slots.iter_mut().enumerate() {
        if let Some(a) = host.actor(i as u8) {
            *s = CensusSlot {
                render_word: a.render_color,
                current_anim: a.current_anim,
                liveness: a.liveness,
            };
        }
    }
    let (kinds, children) = host.effect_child_slots();
    let census = cast_census(&slots, kinds, children);
    ctx.magic_exit_gate = census.anim_outstanding;
    ctx.magic_recovery_gate = census.effect_children;
    ctx.item_target_a = census.sole_party_target;
    ctx.item_target_b = census.sole_monster_target;
}

/// The timer floor retail holds the Done band at while the menu flag
/// `ctx[+0x276]` is up (`slti v0,v0,0xc` / `li v0,0xc` / `sh v0,0x2(s7)` at
/// `0x801E60C0..0x801E60E4`).
///
/// It is a *floor*, not a gate: the countdown is allowed to sink to `0xC` and
/// is then pinned there for as long as the flag is set, so the band's tail
/// cue block (which runs below `0xC`) never starts and the exit below never
/// fires. When the flag clears, the last twelve frames still have to run.
pub const DONE_MENU_HOLD_FRAMES: i16 = 0xC;

/// Countdown value the `0x52` band's teardown arm opens below (`slti
/// v0,v0,0x14` at `0x801E63F0`); the pad clamp in that band pins the
/// countdown one below it (`li v0,0x13` at `0x801E63B4`).
pub const DONE_MULTI_CAST_TEARDOWN_BELOW: i16 = 0x14;

/// The timer retail re-seeds when the Done band routes to
/// [`ActionState::DoneMultiCast`] instead of ending the action
/// (`li v0,0xb4` / `sh v0,0x2(s7)` at `0x801E6134`/`0x801E6138`, the arm
/// `ctx[+0x269] != 0` selects).
///
/// Without it the multi-cast state inherits an already-negative timer and can
/// never satisfy a countdown, which is a park rather than a wait.
pub const DONE_MULTI_CAST_FRAMES: i16 = 0xB4;

/// The `0x50` seed on every ordinary path: `li v0,0x3c` at `0x801E5EE8` /
/// `0x801E5EFC` / `0x801E5F24`, stored at `0x801E5F28`.
pub const DONE_SEED_FRAMES: i16 = 0x3C;

/// The `0x50` seed when a level-up banner is up (`ctx[+0x26] != 0`):
/// `lbu v0,0x15(s5)` / `li v0,0x96` / `sh v0,0x2(s7)` at
/// `0x801E5F2C..0x801E5F3C`, which overwrites the `0x3C` just stored.
pub const DONE_LEVELUP_BANNER_FRAMES: i16 = 0x96;

/// While the banner tail runs, a pad press cuts it - but only once the
/// countdown has fallen below this (`slti v0,v0,0x5b` at `0x801E60A8`), so
/// the banner is guaranteed `0x96 - 0x5B` frames on screen before any input
/// can skip it.
pub const DONE_BANNER_SKIP_BELOW: i16 = 0x5B;

/// `FUN_801D8DE8`'s second argument for the **terminate / unload** half of
/// the element scheduler (`li a1,0x1` at every teardown call site of the
/// `0x51` tail). `0` is the spawn / reset half.
pub const UI_UNLOAD: u8 = 1;

/// The mode byte the element scheduler's **raise** half takes (`a1 = 0`), the
/// sibling of [`UI_UNLOAD`].
pub const UI_RAISE: u8 = 0;

/// The one action element whose teardown drags two more with it
/// (`li v0,0x6` / `bne v1,v0,0x801E61B4` at `0x801E6194..0x801E6198`).
pub const DONE_PAIRED_ELEMENT: u8 = 6;
/// First of the pair (`li a0,0x4e` at `0x801E619C`).
pub const DONE_PAIRED_ELEMENT_A: u8 = 0x4E;
/// Second of the pair (`li a0,0x4f` at `0x801E61A8`).
pub const DONE_PAIRED_ELEMENT_B: u8 = 0x4F;
/// First of the Spirit pair the `ctx[+0x19]` latch drops (`li a0,0xf` at
/// `0x801E61D8`).
pub const DONE_SPIRIT_ELEMENT_A: u8 = 0x0F;
/// Second of the Spirit pair (`li a0,0x52` at `0x801E61E4`).
pub const DONE_SPIRIT_ELEMENT_B: u8 = 0x52;
/// Dropped for every action whose category is not Run (`li a0,0x44` at
/// `0x801E61FC`).
pub const DONE_ACTION_ELEMENT: u8 = 0x44;

/// The element the capture grant **raises** (`li a0,0x59` /
/// `jal 0x801d8de8` / `_clear a1` at `0x801E623C..0x801E6244`).
///
/// `a1 = 0` is the raise half, not the unload half, and the argument order
/// is what settles it: every unload in this block passes `1`. The
/// battle-action table row that read this as "unloads ... `+0x59` (queue
/// marker)" had the direction backwards - nothing in the `0x51` band closes
/// `0x59`; the `0x52` continuation band does, at `0x801E6418`, and those two
/// are the only sites in the whole routine that name the id.
pub const DONE_CAPTURE_BANNER_ELEMENT: u8 = 0x59;

/// The single-target banner the sweep closes (`li a0,0x51` at `0x801E6340`).
pub const DONE_TARGET_BANNER_ELEMENT: u8 = 0x51;

/// Target-slot band the `0x51` close is gated on: `addiu v0,t2,-0x3` /
/// `sltiu v0,v0,0x5` at `0x801E631C`, i.e. `target - 3 < 5`.
pub const DONE_TARGET_BANNER_SLOTS: std::ops::RangeInclusive<u8> = 3..=7;

pub(super) fn done_fade_down<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    // Retail's `0x51` arm, in its own order (`0x801E6044..0x801E6148`):
    //
    // 1. `jal FUN_801E7250` - the HP-bar settle check. A pending drain
    //    branches **past the decrement** (`bne v0,zero,0x801E60B8`), so the
    //    countdown freezes while a party readout is still ramping; it does
    //    not restart it and it does not skip the exit test.
    // 2. decrement, but only while the timer is still non-negative
    //    (`bltz v0,0x801E60C4` at `0x801E605C`).
    // 3. the menu-flag floor at `0xC` ([`DONE_MENU_HOLD_FRAMES`]).
    // 4. leave when the timer is **negative and** the menu flag is clear
    //    (`bgez v0,...` at `0x801E60F0`, `bne v0,zero,...` at `0x801E610C`).
    //
    // Step 4 is a test on the *value*, re-run every pass - not on the frame
    // the countdown happens to cross zero. Gating the exit on the crossing
    // means any pass where the menu flag is up on that one frame loses the
    // exit for good, which is the shape of an unbounded park rather than of
    // retail's bounded tail.
    //
    // REF: FUN_801E7250 (the settle check this arm calls)
    // PORT: FUN_801E295C (`0x801E6044..0x801E6148`)
    if !hp_bar_drain_pending(host, ctx) && ctx.frame_timer >= 0 {
        ctx.frame_timer = ctx.frame_timer.saturating_sub(host.frame_dt());
        // Banner skip (`0x801E6078..0x801E60B4`), reached only through the
        // decrement: while a level-up banner is up, any pad activity
        // (`_DAT_8007B874 | _DAT_8007B938`, tested for non-zero only) snaps
        // the countdown straight to `-1` - but not until it has already sunk
        // below `0x5B`, so the banner always gets its first
        // `DONE_LEVELUP_BANNER_FRAMES - DONE_BANNER_SKIP_BELOW` frames.
        if ctx.levelup_banner_element != 0
            && host.pad_word() != 0
            && ctx.frame_timer < DONE_BANNER_SKIP_BELOW
        {
            ctx.frame_timer = -1;
        }
    }
    if ctx.frame_timer < DONE_MENU_HOLD_FRAMES && ctx.menu_open != 0 {
        ctx.frame_timer = DONE_MENU_HOLD_FRAMES;
    }
    let outcome = if ctx.frame_timer >= 0 || ctx.menu_open != 0 {
        stay(ctx)
    } else {
        // `sb zero,0x288(v1)` at `0x801E6114` - the second counter-attack
        // trigger flag is cleared on the way out, so a counter armed during
        // this action cannot leak into the next one.
        ctx.counter_attack_b = 0;
        if ctx.multi_cast_gate == 0 {
            transition(ctx, ActionState::EndOfAction)
        } else {
            ctx.frame_timer = DONE_MULTI_CAST_FRAMES;
            transition(ctx, ActionState::DoneMultiCast)
        }
    };
    // The band's UI teardown runs on the way out of *every* pass, after the
    // state store and whether or not one happened: retail's exit branch at
    // `0x801E610C` jumps to `0x801E614C`, the head of the tail below, not to
    // the epilogue.
    done_band_ui_teardown(host, ctx);
    outcome
}

/// PORT: FUN_801E295C (`0x801E614C..0x801E6214`) - the Done band's **UI
/// teardown**, the tail every pass of state `0x51` falls into.
///
/// Two gates, then a latched straight line:
///
/// ```text
/// 801e614c  lh    v0,0x2(s7)      ; ctx[+0x6D8], the band countdown
/// 801e6154  slti  v0,v0,0xc
/// 801e6158  beq   v0,zero,<exit>  ;   >= 0xC -> nothing yet
/// 801e6160  lbu   v0,0x6(s5)      ; ctx[+0x17], the "already torn down" latch
/// 801e6168  bne   v0,zero,<exit>
/// 801e6170  jal   0x801d99bc      ; the sprite-handle table hard reset
/// 801e6178  lbu   v0,0x7(s5)      ; ctx[+0x18] - the action's own element
/// 801e6188  jal   FUN_801D8DE8(elem, 1)
/// 801e6198  bne   v1,0x6,...      ;   element 6 also drops 0x4E and 0x4F
/// 801e61b4  lbu   v0,0x15(s5)     ; ctx[+0x26] - the level-up banner
/// 801e61c4  jal   FUN_801D8DE8(elem, 1)
/// 801e61cc  lbu   v0,0x8(s5)      ; ctx[+0x19] - the Spirit-action latch
/// 801e61dc  jal   FUN_801D8DE8(0x0F, 1) ; and 0x52
/// 801e61f0  lbu   v1,0x1de(s3)    ; actor category, `5` = Run
/// 801e6200  jal   FUN_801D8DE8(0x44, 1) ; every non-Run action
/// 801e6214  sb    v0,0x6(s5)      ; ctx[+0x17] += 1  - the latch
/// ```
///
/// The latch is what makes it once-per-action: [`done_cleanup`] (state
/// `0x50`) clears `ctx[+0x17]` at `0x801E5F60`, so the next action arms it
/// again. Every unload is `FUN_801D8DE8(id, 1)`, mode `1` being the
/// terminate half of the element scheduler
/// ([`BattleActionHost::ui_element`]).
///
/// This is where the level-up banner's element id round-trips: the banner
/// path stamps [`BattleActionCtx::levelup_banner_element`], `done_cleanup`
/// reads it to stretch the countdown to `0x96`, this arm unloads it, and the
/// next [`ActionState::ActionSeed`] clears the byte (`0x801E2CFC`).
///
/// The block does **not** end at the latch. `0x801E6218` falls straight
/// through from the `sb v0,0x6(s5)` at `0x801E6214`, and nothing branches
/// into it: both of the gates above jump to `0x801E6814`, past the whole
/// tail. So the sweep that follows is inside the same once-per-action latch,
/// not outside it. It was recorded here (and in `battle-action.md`) as
/// "unlatched", which is refuted by the two branch targets.
///
/// That tail is [`done_band_capture_and_banner_sweep`], and its multi-cast
/// half turns out to be code this crate already ports: retail's
/// `(id, id - 4)` pairing off the `(count - 1) * 4 + i` row is
/// `battle_value_readout`'s `teardown_pair` / `teardown_row_offset`, because
/// `0x801E6218..0x801E6368` is `FUN_801E805C`'s teardown half inlined into
/// the action SM. `battle_value_readout` already cites `0x801E6360` - inside
/// this range - as where element `0x50` closes.
///
/// **Not ported:** the sprite-table reset `FUN_801D99BC` at `0x801E6170` (a
/// whole-table rebuild the engine's HUD does not model as handles; it carries
/// a scope row instead).
fn done_band_ui_teardown<H: BattleActionHost + ?Sized>(host: &mut H, ctx: &mut BattleActionCtx) {
    if ctx.frame_timer >= DONE_MENU_HOLD_FRAMES || ctx.done_ui_torn_down != 0 {
        return;
    }
    if ctx.action_ui_element != 0 {
        host.ui_element(ctx.action_ui_element, UI_UNLOAD);
        if ctx.action_ui_element == DONE_PAIRED_ELEMENT {
            host.ui_element(DONE_PAIRED_ELEMENT_A, UI_UNLOAD);
            host.ui_element(DONE_PAIRED_ELEMENT_B, UI_UNLOAD);
        }
    }
    if ctx.levelup_banner_element != 0 {
        host.ui_element(ctx.levelup_banner_element, UI_UNLOAD);
    }
    if ctx.spirit_action_count != 0 {
        host.ui_element(DONE_SPIRIT_ELEMENT_A, UI_UNLOAD);
        host.ui_element(DONE_SPIRIT_ELEMENT_B, UI_UNLOAD);
    }
    let category = host
        .actor(ctx.active_actor)
        .map(|a| a.action_category)
        .unwrap_or(0);
    if category != ActionCategory::Run as u8 {
        host.ui_element(DONE_ACTION_ELEMENT, UI_UNLOAD);
    }
    ctx.done_ui_torn_down = ctx.done_ui_torn_down.wrapping_add(1);
    done_band_capture_and_banner_sweep(host, ctx);
}

/// PORT: FUN_801E295C (`0x801E6218..0x801E6368`) - the tail the teardown
/// block falls into, under the same latch.
///
/// ```text
/// 801e6224  lbu   v0,0x269(v1)     ; the capture byte
/// 801e6234  jal   0x801e92dc       ;   FUN_801E92DC(byte)
/// 801e6240  jal   0x801d8de8       ;   raise 0x59   (a1 = 0)
/// 801e624c  lhu   v1,0x6980(...)   ; the four-slot value window
/// 801e6284  beq   v0,zero,0x801e6314 ;   all zero -> the banner arm
/// 801e62ac  ...                    ; else the (id, id-4) unload loop
/// 801e6314  lw    t2,0x20(sp)      ; the acting actor's +0x1DD target
/// 801e6320  sltiu v0,v0,0x5        ;   target - 3 < 5
/// 801e6334  ...                    ;   and 0 < category < 4
/// 801e6344  jal   0x801d8de8       ;     unload 0x51
/// 801e6360  jal   0x801d8de8       ;   unload 0x50 if _DAT_8007BD14
/// ```
///
/// Three arms, and the port covers the two whose inputs the action context
/// already carries.
///
/// **The capture grant.** `ctx[+0x269]` is the byte the Done band's exit test
/// routes on, and this is its *other* reader: it is passed to
/// `FUN_801E92DC`, whose parameter is a **spell id** - the routine walks
/// `0x80084140 + char*0x414 + 0x704` for the acting character and prepends
/// there (`legaia_engine_core::magic_xp::learn_spell_prepend`). Element
/// `0x59` raised in the same breath is the banner that announces it. The
/// grant itself needs a host hook this trait does not have, so the port
/// raises the banner and leaves the record write to `engine-core`'s own
/// capture path, which already performs it.
///
/// **The multi-cast unload loop** is gated on the readout value window
/// `0x801F6980` and driven by `_DAT_801F6974` / `_DAT_801F6834`. Its kernels
/// are ported - `battle_value_readout::teardown_pair` and
/// `teardown_row_offset` - but neither the window nor the count is carried on
/// the action context, so the loop is driven from `FUN_801E805C`'s own port
/// rather than from here. Wiring it in this block would need the readout
/// window on `BattleActionCtx`, which is the readout's state and not the
/// action's.
///
/// **The single-target banner close** is ported: both of its gates are actor
/// bytes this block already reads.
///
/// One consequence of the two gates above worth stating, because it bounds
/// when the capture arm can fire at all: a non-zero `ctx[+0x269]` on the
/// band's *exit* pass re-seeds the countdown to `0xB4` (`sh v0,0x2(s7)` at
/// `0x801E6138`) **before** the teardown reloads it at `0x801E614C`, which
/// fails the `< 0xC` gate. So the only frames that see a live capture byte
/// and a running teardown are the last twelve of the countdown, before it
/// crosses zero - and the latch then makes it once.
fn done_band_capture_and_banner_sweep<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) {
    if ctx.multi_cast_gate != 0 {
        host.ui_element(DONE_CAPTURE_BANNER_ELEMENT, UI_RAISE);
    }
    let Some(actor) = host.actor(ctx.active_actor) else {
        return;
    };
    let (target, category) = (actor.active_target, actor.action_category);
    // `0 < category < 4` - two branches, `beq v0,zero` then `sltiu v0,v0,0x4`
    // at `0x801E6334..0x801E633C`.
    if DONE_TARGET_BANNER_SLOTS.contains(&target) && (1..4).contains(&category) {
        host.ui_element(DONE_TARGET_BANNER_ELEMENT, UI_UNLOAD);
    }
}

pub(super) fn done_multi_cast<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let slot = ctx.active_actor;
    host.pose(slot, Pose::Recover);
    // Same countdown shape as the band above: the entry seeded
    // [`DONE_MULTI_CAST_FRAMES`], the decrement stops at the sign change, and
    // the exit is a test on the value rather than on the crossing.
    if ctx.frame_timer >= 0 {
        ctx.frame_timer = ctx.frame_timer.saturating_sub(host.frame_dt());
    }
    let outcome = if ctx.frame_timer >= 0 {
        stay(ctx)
    } else {
        ctx.multi_cast_gate = 0;
        transition(ctx, ActionState::EndOfAction)
    };
    // The band's own teardown tail (`0x801E63F4..0x801E6424`), which runs on
    // the way out of *every* pass the way the `0x51` block's does - the exit
    // arm above falls into it rather than jumping to the epilogue. It closes
    // the capture banner the `0x51` sweep raised
    // ([`DONE_CAPTURE_BANNER_ELEMENT`]) behind the same `ctx[+0x17]` latch,
    // and clears the latch, so a raise cannot leave a banner standing.
    //
    // `FUN_801D99BC` at `0x801E640C` is the same per-actor UI-element array
    // wipe the `0x51` block calls, and carries the same scope row: the
    // engine's HUD is rebuilt from state each frame.
    if ctx.frame_timer < DONE_MULTI_CAST_TEARDOWN_BELOW && ctx.done_ui_torn_down != 0 {
        host.ui_element(DONE_CAPTURE_BANNER_ELEMENT, UI_UNLOAD);
        ctx.done_ui_torn_down = 0;
    }
    outcome
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

    // "No party seated" is not "party dead". Retail's scan counts the
    // seated-count byte's worth of table slots and nothing else - a battle
    // with zero seated party members is unrepresentable there (battle load
    // always seats the present party; see `BattleActionHost::slot_seated`).
    // The port can reach that state (a host entering battle with stamped but
    // roster-less party slots), and reporting it as a wipe turned every
    // unseeded battle into an instant game over before anything was hit. A
    // wipe requires at least one seated slot with nobody standing in it.
    let party_seated = (0..party_count).filter(|&s| host.slot_seated(s)).count();
    if party_seated > 0 && party_alive == 0 {
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

    // Advance the turn cursor past the actor that just acted; if it is still
    // short of the round's length, restart at PreActionWait. Otherwise every
    // living actor has acted → the ROUND is over (retail's `0x5A` non-wipe
    // arm writes state `0xFF`, the round boundary - not a battle end; see
    // `round_end`).
    //
    // Retail's bound is `ctx[+0x00] + ctx[+0x01] - ctx[+0x25]` (`0x801E67B4`):
    // the seated party and monster counts, less the **round-skip** count.
    // `+0x25` is cleared once per round by the initiative seeder
    // (`0x801DAB84`, the delay slot of its `jal 0x801DABA4`) and bumped inside
    // `FUN_801DABA4` at `0x801DAC2C` for each actor-table slot that is dead
    // (`+0x14C == 0`, guard `0x801DABD8`) *and* still holds an unspent
    // initiative key (`+0x16C != 0`, guard `0x801DABE8`) - i.e. a combatant
    // that died before its turn came up.
    //
    // The engine counts the **living** instead. The two agree while nobody
    // dies mid-round; they diverge for an actor that dies *after* acting,
    // which shrinks this bound but not retail's, so the engine can end a round
    // one action early. Closing that needs all three bytes on the context -
    // "seated" is not recoverable from the actor table once a slot is dead.
    // See `docs/subsystems/battle-action.md` § "The three bytes the bound is
    // built from".
    //
    // REF: FUN_801DABA4 (the round-skip bump this bound reads)
    //
    // PORT: FUN_801E295C (`0x801E679C..0x801E67C8`)
    ctx.turn_cursor = ctx.turn_cursor.saturating_add(1);
    let bumped = ctx.turn_cursor;
    let alive_total = (party_alive + monsters_alive) as u8;
    if bumped < alive_total {
        return transition(ctx, ActionState::PreActionWait);
    }
    transition(ctx, ActionState::RoundEnd)
}

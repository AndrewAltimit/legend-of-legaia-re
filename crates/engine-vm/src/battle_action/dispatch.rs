//! Battle-action state-machine `step` dispatcher, command-queue resolution, and shared transition/timer helpers.

use super::*;

/// Resolve a player's directional command sequence into an action queue,
/// applying Miracle Art and Super Art expansion in the canonical order.
///
/// This is the entry point the battle UI layer calls *before* feeding the
/// queue to the action state machine via `ctx.queued_action`. It runs the
/// **byte-level** appliers ported from the retail queue-builder
/// `FUN_801EED1C`, on a raw `actor[+0x1DF..+0x1F2]`-shaped byte window, in
/// retail's own finish order:
///
/// 1. Translate raw commands to directional [`ActionConstant`] bytes and
///    append `[RegularStarter, art]` pairs per the chained art selection,
///    capped at [`QUEUE_SCAN_LEN`] - retail's build loop bound.
/// 2. **Miracle Art replacement** ([`apply_miracle_replace`], the
///    `0x801EF4E8` block): when the command sequence is the character's
///    Miracle string, the whole 16-byte window is overwritten from the
///    resident Miracle row, padding included.
/// 3. **MSB clear** ([`clear_queue_msb`], the `0x801EF85C` sweep) - strips
///    the on-disc `0x8C..0x8F` quirk off the Miracle row's direction bytes.
/// 4. **Super Art find/replace at tail** ([`apply_super_tail_replace`], the
///    `jal 0x801EF9E4` at `0x801EF9AC`), run **once** and in **table order**,
///    unconditionally - retail does not skip it after a Miracle, and the
///    Miracle row's tail matches no `find` row.
/// 5. Decode the byte window back to typed constants, stopping at the first
///    `0x00` (the queue terminator the action SM scans for).
///
/// Two behaviours here are retail's, and differ from the structural
/// `legaia_art` matchers this used to call:
///
/// - **first matching row wins, in resident-table order.** The applier's row
///   loop terminates on the first full tail match (`a1 = 5` at `0x801EFBD8`);
///   `SuperMatcher::try_trigger_at_tail` ranks by longest `find` instead.
///   Over the shipped tables the two agree - `super_applier_agrees_with_
///   structural_matcher` pins that - but the disassembly is the baseline.
/// - **one application, not a fixpoint.** `FUN_801EF9E4` is called exactly
///   once per queue build and applies at most one replace; the previous
///   `SuperMatcher::expand_to_fixpoint` call could apply several.
///
/// `chained_arts` are the art [`ActionConstant`]s the player has
/// successfully chained this turn (e.g. `[Art22, Art28]` for Spin Combo →
/// Charging Scorch). Each is bracketed with [`ActionConstant::RegularStarter`]
/// when assembled into the queue, matching the retail builder.
///
/// [`ActionConstant`]: legaia_art::ActionConstant
/// [`ActionConstant::RegularStarter`]: legaia_art::ActionConstant::RegularStarter
///
/// REF: FUN_801EED1C (the retail builder this reproduces the finish order of)
pub fn resolve_action_queue(
    character: legaia_art::Character,
    command_input: &[legaia_art::Command],
    chained_arts: &[legaia_art::ActionConstant],
) -> legaia_art::ActionQueue {
    use legaia_art::{ActionConstant, ActionQueue, MiracleMatcher};

    // Step 1: build the raw byte window. Retail's build loop is bounded by
    // the same 16-byte scan window the appliers use.
    let staged = command_input
        .iter()
        .map(|cmd| cmd.as_action().as_byte())
        .chain(
            chained_arts
                .iter()
                .flat_map(|art| [ActionConstant::RegularStarter.as_byte(), art.as_byte()]),
        );
    let mut bytes = [0u8; ACTION_QUEUE_CAP];
    for (slot, b) in bytes[..QUEUE_SCAN_LEN].iter_mut().zip(staged) {
        *slot = b;
    }

    // Step 2: Miracle Art replacement. Retail's gate is the per-slot marker
    // `ctx[+0x25F + slot]`, armed by the input recognizer; the engine's
    // equivalent is the whole-string match against the character's Miracle
    // command table.
    if MiracleMatcher::with_default_table()
        .find(character, command_input)
        .is_some()
    {
        apply_miracle_replace(&mut bytes, &miracle_row_for(character));
    }

    // Step 3: the MSB-clear sweep.
    clear_queue_msb(&mut bytes);

    // Step 4: the Super tail-replace, once, in table order.
    let (find_rows, replace_rows) = super_rows_for(character);
    let mut starter_marks = [0u32; ACTION_QUEUE_CAP];
    apply_super_tail_replace(&mut bytes, &mut starter_marks, &find_rows, &replace_rows);

    // Step 5: decode up to the terminator.
    let mut queue = ActionQueue::new();
    for &b in bytes.iter() {
        if b == 0 {
            break;
        }
        // Every byte reachable here comes from the modeled tables or from a
        // typed `ActionConstant`, so the decode cannot fail; an unmapped byte
        // would mean a table defect, and dropping the tail is the safe read.
        let Some(action) = ActionConstant::from_byte(b) else {
            break;
        };
        queue.push(action);
    }
    queue
}

/// Dispatch one frame of the battle action state machine.
///
/// Reads `ctx.action_state`, runs the corresponding case body, and may write
/// a new `action_state` value (transitioning to the next state for the next
/// frame).
///
/// Returns a [`StepOutcome`] describing what happened.
pub fn step<H: BattleActionHost + ?Sized>(host: &mut H, ctx: &mut BattleActionCtx) -> StepOutcome {
    let from = ctx.action_state;
    // REF: FUN_801E295C - retail dispatches `ctx[7]` through a 256-entry `jr`
    // table at 0x801CED44 with no default; unmapped bytes index a slot that
    // falls to the shared post-switch epilogue (an inert no-op), so surfacing
    // them here as UnknownState is a safe superset of retail's behaviour.
    let Some(state) = ActionState::from_byte(from) else {
        return StepOutcome::UnknownState { state: from };
    };

    match state {
        ActionState::Begin => begin(host, ctx),
        ActionState::PreActionWait => pre_action_wait(host, ctx),
        ActionState::QueuedFromMenu => queued_from_menu(ctx),
        ActionState::ActionSeed => action_seed(host, ctx),

        ActionState::AttackFace => attack_face(host, ctx),
        ActionState::AttackWindup => attack_windup(host, ctx),
        ActionState::AttackAdvance => attack_advance(host, ctx),
        ActionState::AttackCloseRange => attack_close_range(host, ctx),
        ActionState::AttackStrike => attack_strike(host, ctx),
        ActionState::AttackShortStep => attack_short_step(host, ctx),
        ActionState::AttackChain => attack_chain(host, ctx),
        ActionState::AttackRecovery => attack_recovery(host, ctx),
        ActionState::AttackReturn => attack_return(host, ctx),

        ActionState::MagicCastBegin => magic_cast_begin(host, ctx),
        ActionState::MagicPreCastWait => magic_pre_cast_wait(host, ctx),
        ActionState::MagicAnimChain => magic_anim_chain(host, ctx),
        ActionState::MagicSustain => magic_sustain(host, ctx),
        ActionState::MagicHitLoop => magic_hit_loop(host, ctx),
        ActionState::MagicRecovery => magic_recovery(host, ctx),
        ActionState::MagicExit => magic_exit(host, ctx),

        ActionState::SummonInvoke => summon_invoke(host, ctx),
        ActionState::SummonFadeIn => summon_fade_in(host, ctx),
        ActionState::SummonActorFreeze => summon_actor_freeze(host, ctx),
        ActionState::SummonSustain => summon_sustain(host, ctx),
        ActionState::SummonReturn => summon_return(host, ctx),
        ActionState::SummonVerifyAlive => summon_verify_alive(host, ctx),
        ActionState::SummonDone => summon_done(host, ctx),

        ActionState::SpiritPreArm => spirit_pre_arm(host, ctx),
        ActionState::SpiritWait => spirit_wait(host, ctx),
        ActionState::SpiritFire => spirit_fire(host, ctx),
        ActionState::SpiritFireDamage => spirit_fire_damage(host, ctx),
        ActionState::SpiritPostDamage => spirit_post_damage(host, ctx),

        ActionState::SpiritArtsEntry => spirit_arts_entry(host, ctx),
        ActionState::SpiritArtsSustain => spirit_arts_sustain(host, ctx),
        ActionState::SpiritArtsFlush => spirit_arts_flush(host, ctx),

        ActionState::DoneCleanup => done_cleanup(host, ctx),
        ActionState::DoneFadeDown => done_fade_down(host, ctx),
        ActionState::DoneMultiCast => done_multi_cast(host, ctx),
        ActionState::EndOfAction => end_of_action(host, ctx),

        ActionState::RunBegin => run_begin(host, ctx),
        ActionState::RunWait => run_wait(host, ctx),
        ActionState::RunEscape => run_escape(host, ctx),
        ActionState::CaptureStart => capture_start(host, ctx),
        ActionState::CaptureWait => capture_wait(host, ctx),
        ActionState::CaptureSustain => capture_sustain(host, ctx),
        ActionState::CaptureEnd => capture_end(host, ctx),

        ActionState::MagicCaptureBranch => magic_capture_branch(host, ctx),
        ActionState::MagicCaptureFade => magic_capture_fade(host, ctx),
        ActionState::MagicCapturePhase2 => magic_capture_phase2(host, ctx),
        ActionState::MagicCaptureFinalize => magic_capture_finalize(host, ctx),

        ActionState::IdleHold => idle_hold(host, ctx),
        ActionState::RoundEnd => round_end(host, ctx),
    }
}

// --- helper macros + utilities ----------------------------------------------

pub(super) fn transition(ctx: &mut BattleActionCtx, to: ActionState) -> StepOutcome {
    let from = ctx.action_state;
    ctx.action_state = to.as_byte();
    StepOutcome::Transition {
        from,
        to: to.as_byte(),
    }
}

pub(super) fn stay(_ctx: &BattleActionCtx) -> StepOutcome {
    StepOutcome::Stay
}

/// Decrement `frame_timer` by `host.frame_dt()`, return `true` if it crossed
/// zero (i.e. went from non-negative to negative).
pub(super) fn tick_frame_timer<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> bool {
    let prev = ctx.frame_timer;
    let dt = host.frame_dt();
    ctx.frame_timer = ctx.frame_timer.saturating_sub(dt);
    prev >= 0 && ctx.frame_timer < 0
}

// --- state handlers ---------------------------------------------------------

/// The **formation arm** of retail state `0x00` - the whole of
/// `0x801E2AC0..0x801E2B48`, in one place, over the one byte pair retail has.
///
/// Two stores, in this order:
///
/// 1. **Seed the turn cursor** `ctx[+0x1A]` from the formation-advantage byte
///    `ctx[+0x290]` (`0x801E2ACC` reads it). The switch has four arms and only
///    three of them store: `0` seeds the cursor at the head of the order
///    (`0x801E2B08`), `1` at `ctx[+0x00]` - the party count - (`0x801E2B0C`
///    /`0x801E2B14`) and `2` at `ctx[+0x01]` - the monster count -
///    (`0x801E2B18`/`0x801E2B20`), so an ambushed side starts its round partway
///    down the order. Any other byte leaves the cursor alone (`0x801E2AEC`
///    jumps clear of all three stores).
/// 2. **Latch** `+0x290` into `+0x291` (`0x801E2B30`/`0x801E2B38`) and *then*
///    clear the original (`0x801E2B48`). The latched copy is what the escape
///    roll reads as "escape assured" (value `2` = the party opened with a
///    pre-emptive strike), so the copy must happen before the clear - see
///    [`BattleActionCtx::formation_latched`].
///
/// `ctx[+0x00]` / `ctx[+0x01]` are not modelled on [`BattleActionCtx`]; the
/// caller passes the seated counts (in the engine, the host's party count and
/// the slots above it).
///
/// Runs at most once per battle - see [`BattleActionCtx::formation_armed`] for
/// why the port needs a flag where retail does not. Engines that stage the
/// battle themselves may call this at battle open (retail's placement: the
/// flow SM arms `ctx[7] = 0` at `0x801D3224` and the action SM runs `0x00` on
/// the next frame, *before* the command menu resolves); [`begin`] runs it
/// otherwise.
///
/// PORT: FUN_801E295C (`0x801E2AC0..0x801E2B48`)
pub fn begin_formation_arm(
    seated_party: u8,
    seated_monsters: u8,
    ctx: &mut BattleActionCtx,
) -> bool {
    if ctx.formation_armed {
        return false;
    }
    ctx.formation_armed = true;
    match ctx.formation_advantage {
        0 => ctx.turn_cursor = 0,
        1 => ctx.turn_cursor = seated_party,
        2 => ctx.turn_cursor = seated_monsters,
        _ => {}
    }
    ctx.formation_latched = ctx.formation_advantage;
    ctx.formation_advantage = 0;
    true
}

/// [`begin_formation_arm`] with the two seated counts read off a host.
///
/// `ctx[+0x00]` / `ctx[+0x01]` are the counts battle setup seated, so the
/// monster figure is the number of **occupied** monster slots, not the width
/// of the slot table - the engine's `slot_count()` is the fixed 8-slot table
/// and the tail of it is empty in most formations.
pub fn begin_formation_arm_for<H: BattleActionHost + ?Sized>(
    host: &H,
    ctx: &mut BattleActionCtx,
) -> bool {
    let party = host.party_count();
    let monsters = (party..host.slot_count())
        .filter(|&slot| host.actor(slot).is_some_and(|a| a.liveness != 0))
        .count() as u8;
    begin_formation_arm(party, monsters, ctx)
}

pub(super) fn begin<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    // Reset ctx counters at +0x6DA..+0x6DB.
    ctx.combo_timer = 0;
    // Seed the turn cursor from the formation advantage, then latch it away.
    begin_formation_arm_for(host, ctx);
    // Branch to QueuedFromMenu if menu still open, otherwise PreActionWait.
    if ctx.menu_open != 0 {
        transition(ctx, ActionState::QueuedFromMenu)
    } else {
        transition(ctx, ActionState::PreActionWait)
    }
}

pub(super) fn pre_action_wait<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    if host.previous_action_cleared(1) {
        transition(ctx, ActionState::ActionSeed)
    } else {
        stay(ctx)
    }
}

pub(super) fn queued_from_menu(ctx: &mut BattleActionCtx) -> StepOutcome {
    if ctx.menu_open == 0 {
        transition(ctx, ActionState::PreActionWait)
    } else {
        stay(ctx)
    }
}

/// Raise the per-action target banner at the tail of [`action_seed`].
///
/// The plan comes from [`crate::battle_cue_group::plan_target_banner`], the
/// port of `FUN_801E6D84`; each HUD element id it lists is raised through
/// [`BattleActionHost::ui_element`], which is the port's channel for retail's
/// `FUN_801D8DE8` (the same channel the run / spirit / attack arms already
/// use). Two of the retail routine's inputs are abstracted rather than read,
/// and neither reaches the id list:
///
/// * `ctx[+0x24B]`, the override target slot the `target == 9` arms consult,
///   has no `BattleActionCtx` field and is passed as `0` - the arm retail
///   takes when nothing has overridden the target.
/// * the banner *width* term is `FUN_80035F04` over the target's `+0x1BC`
///   animation descriptor, which this crate has no pool to walk. It is passed
///   as `0`, and the resulting `banner_width` is not consumed: the engine's
///   HUD sizes its own banners.
///
/// The engine compacts its battle seating (monsters start at `party_count`,
/// retail always at slot 3), so slot inputs are re-expressed in retail's
/// numbering on the way in and the plan's `slots` are left alone - only the
/// id list is used here.
///
/// REF: FUN_801E6D84
fn raise_target_banner<H: BattleActionHost + ?Sized>(
    host: &mut H,
    actor_slot: u8,
    party_count: u8,
) {
    use crate::battle_cue_group::{BannerInputs, MONSTER_SLOT_FIRST, plan_target_banner};
    let Some(actor) = host.actor(actor_slot) else {
        return;
    };
    let (action_category, action_id, target) =
        (actor.action_category, actor.params[0], actor.active_target);
    let mut monster_alive = [false; 4];
    for (i, alive) in monster_alive.iter_mut().enumerate() {
        *alive = host
            .actor(party_count + i as u8)
            .is_some_and(|a| a.liveness != 0);
    }
    // The plan's slot tests are written in retail's fixed numbering (monsters
    // at 3..=6, the sentinels 8 and 9 above them), so re-express both slot
    // inputs in it. The sentinels pass through untouched.
    let to_retail = |slot: u8| -> u8 {
        if slot < party_count {
            slot
        } else if slot < 8 {
            MONSTER_SLOT_FIRST + (slot - party_count)
        } else {
            slot
        }
    };
    let plan = plan_target_banner(
        &BannerInputs {
            active_slot: to_retail(actor_slot),
            action_category,
            action_id,
            target: to_retail(target),
            ctx_override_slot: 0,
            monster_alive,
        },
        0,
    );
    for id in plan.hud_elements {
        host.ui_element(id, 0);
    }
    // `plan.ctx_18` is dropped: `ctx[+0x18]` has no `BattleActionCtx` field
    // (the port's `+0x18` is the battle actor's `ui_element_id`, a different
    // byte), and nothing in the engine reads a context HUD-state byte.
}

pub(super) fn action_seed<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    let actor_slot = ctx.active_actor;
    let Some(actor) = host.actor(actor_slot) else {
        return stay(ctx);
    };
    let category = ActionCategory::from_byte(actor.action_category);
    let field_flags = actor.field_flags;
    let party_count = host.party_count();

    // Setup hooks.
    if actor_slot < party_count {
        host.party_setup(actor_slot);
    } else if (field_flags & 0x380) != 0 {
        host.monster_setup(actor_slot);
    }

    // Per-action camera framing (`FUN_801F0348` at `801e2d2c`). Retail runs
    // this unconditionally on the seed path, ahead of - and independent of -
    // the gated `FUN_801EFE44` bounds walk below.
    // Read `+0x1DD` *after* the setup hooks: `monster_setup` is where the
    // engine expands a monster's targeting class into a concrete slot, and
    // retail's `801f0348` reads the live byte.
    let target_slot = host.actor(actor_slot).map_or(8, |a| a.active_target);
    let frame_height = crate::battle_formulas::camera_height_for_frame(
        actor_slot,
        target_slot,
        party_count,
        |slot| host.monster_size_class(slot),
    );
    host.camera_frame_height(frame_height);

    // Camera bounds (skipped for run actions per docs).
    if !matches!(category, ActionCategory::Run) {
        host.camera_bounds();
    }

    // Idle pose.
    host.pose(actor_slot, Pose::Idle);

    // Per-action target banner. Retail's seed body ends every category arm at
    // the same `jal 0x801e6d84` (`overlay_battle_action_801e295c.txt`
    // `0x801E3028`) before jumping to the epilogue, so the plan is raised here
    // rather than per-band.
    raise_target_banner(host, actor_slot, party_count);

    // Dispatch into the appropriate band.
    let next = match category {
        ActionCategory::TacticalArts => {
            // Skip - UI input chain handles the chain.
            ActionState::DoneCleanup
        }
        ActionCategory::Item => {
            // Item route - a runtime check on the param byte chooses between
            // 0x3C and 0x28; default to 0x3C (the more common path).
            ActionState::SpiritPreArm
        }
        ActionCategory::Magic => ActionState::MagicCastBegin,
        ActionCategory::Attack => {
            // Set ctx combo timer and emit weapon-slash UI for party.
            ctx.combo_timer = 2;
            if actor_slot < party_count {
                if let Some(actor) = host.actor_mut(actor_slot) {
                    actor.ui_element_id = 7;
                }
                host.ui_element(7, 0);
            }
            ActionState::AttackFace
        }
        ActionCategory::Spirit => ActionState::SpiritArtsEntry,
        ActionCategory::Run => {
            if actor_slot < party_count {
                ActionState::RunBegin
            } else {
                ActionState::CaptureStart
            }
        }
        ActionCategory::ItemRetargetA | ActionCategory::ItemRetargetB => {
            // Should never hit ActionSeed with these; but if they do, treat
            // as Item route.
            ActionState::SpiritPreArm
        }
    };
    transition(ctx, next)
}

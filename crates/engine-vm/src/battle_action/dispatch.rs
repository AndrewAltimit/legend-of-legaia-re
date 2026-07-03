//! Battle-action state-machine `step` dispatcher, command-queue resolution, and shared transition/timer helpers.

use super::*;

/// Resolve a player's directional command sequence into an action queue,
/// applying Miracle Art and Super Art expansion in the canonical order.
///
/// This is the entry point the battle UI layer calls *before* feeding the
/// queue to the action state machine via `ctx.queued_action`. The retail
/// runtime applies the same two passes as part of the command-resolution
/// step that runs once per turn.
///
/// Order of operations (matches retail):
/// 1. Translate raw commands to directional [`ActionConstant`]s and append
///    starter/art constants per the chained art selection.
/// 2. **Miracle Art match** - full-queue replacement if the command
///    sequence is the character's Miracle Art string.
/// 3. **Super Art find/replace at tail** - runs to fixpoint to allow
///    nested triggers (none exist in retail tables, but the API handles
///    them).
///
/// `chained_arts` are the art [`ActionConstant`]s the player has
/// successfully chained this turn (e.g. `[Art22, Art28]` for Spin Combo →
/// Charging Scorch). Each is bracketed with [`ActionConstant::RegularStarter`]
/// when assembled into the queue, matching the retail builder.
pub fn resolve_action_queue(
    character: legaia_art::Character,
    command_input: &[legaia_art::Command],
    chained_arts: &[legaia_art::ActionConstant],
) -> legaia_art::ActionQueue {
    use legaia_art::{ActionQueue, MiracleMatcher, SuperMatcher};

    let mut queue = ActionQueue::new();

    // Step 1: literal directional inputs followed by chained arts. Each
    // chained art is preceded by a Regular Starter (matches the retail
    // queue layout: `19 <art> 19 <art> ...`).
    for cmd in command_input {
        queue.push(cmd.as_action());
    }
    for art in chained_arts {
        queue.push(legaia_art::ActionConstant::RegularStarter);
        queue.push(*art);
    }

    // Step 2: Miracle Art replacement - if the input commands match a
    // Miracle Art exactly, the entire queue is replaced.
    let miracle = MiracleMatcher::with_default_table();
    if miracle.try_trigger(character, command_input, &mut queue) {
        // Miracle Arts swallow all chained input - return immediately
        // since Super Art expansion is not applied on top.
        return queue;
    }

    // Step 3: Super Art find/replace at tail, run to fixpoint.
    let super_matcher = SuperMatcher::with_default_table();
    super_matcher.expand_to_fixpoint(character, &mut queue);

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
        ActionState::BattleComplete => battle_complete(host, ctx),
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

pub(super) fn begin<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
) -> StepOutcome {
    // Reset ctx counters at +0x6DA..+0x6DB.
    ctx.combo_timer = 0;
    // Copy ctx[+0x274] (queued action) → actor[+0x1A].
    if let Some(actor) = host.actor_mut(ctx.active_actor) {
        actor.action_queue_counter = ctx.queued_action;
    }
    // Clear ctx[+0x290].
    ctx.clear_at_begin = 0;
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

    // Camera bounds (skipped for run actions per docs).
    if !matches!(category, ActionCategory::Run) {
        host.camera_bounds();
    }

    // Idle pose.
    host.pose(actor_slot, Pose::Idle);

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

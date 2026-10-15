//! The magic cast-begin band and the full magic flow.

use super::*;

#[test]
fn magic_cast_begin_capture_spell_routes_to_capture_branch() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCastBegin.as_byte();
    host.actors[1].params[0] = 0x42;
    host.capture_spells.insert(0x42);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::MagicCaptureBranch.as_byte()
    ));
    assert!(host.take().contains(&Event::LoadCapture(0x42)));
}

/// The capture route raises the action-in-progress marker `ctx[+0x243]`
/// (`sb v0,0x243(v1)` at `0x801E44FC`) that dims the stage through the
/// capture band; a plain cast leaves it alone.
#[test]
fn magic_cast_begin_capture_spell_raises_the_action_marker() {
    let marker = |capture: bool| {
        let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
        ctx.action_state = ActionState::MagicCastBegin.as_byte();
        host.actors[1].params[0] = 0x42;
        if capture {
            host.capture_spells.insert(0x42);
        }
        step(&mut host, &mut ctx);
        ctx.gauge_rearm_latch
    };
    assert_eq!(marker(true), 1);
    assert_eq!(marker(false), 0);
}

/// Retail's capture route (`ctx[7] = 0x6E` at `0x801E4490`) falls through
/// into the MP debit at `0x801E4500`: a capture-class cast pays its MP.
#[test]
fn magic_cast_begin_capture_spell_still_pays_its_mp() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCastBegin.as_byte();
    host.actors[1].mp = 1000;
    host.actors[1].params[0] = 0xB7;
    host.capture_spells.insert(0xB7);
    host.spell_costs.insert(0xB7, 100);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition { to, .. } if to == ActionState::MagicCaptureBranch.as_byte()
    ));
    assert_eq!(host.actors[1].mp, 900);
    assert_eq!(host.actors[1].last_mp_cost, 100);
}

#[test]
fn magic_cast_begin_subtracts_mp_with_ability_bits() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCastBegin.as_byte();
    host.actors[1].mp = 50;
    host.actors[1].params[0] = 0x10;
    host.spell_costs.insert(0x10, 20);
    host.ability_bits.insert(1, 0x20); // half cost
    step(&mut host, &mut ctx);
    // 50 - 10 = 40
    assert_eq!(host.actors[1].mp, 40);
    assert_eq!(host.actors[1].last_mp_cost, 10);
}

#[test]
fn magic_cast_begin_quarter_cost_with_bit_10() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCastBegin.as_byte();
    host.actors[1].mp = 50;
    host.actors[1].params[0] = 0x10;
    host.spell_costs.insert(0x10, 20);
    host.ability_bits.insert(1, 0x10); // quarter (shave 25%: cost - cost>>2)
    step(&mut host, &mut ctx);
    // cost 20 -> 20 - (20>>2) = 15; 50 - 15 = 35
    assert_eq!(host.actors[1].mp, 35);
}

#[test]
fn magic_pre_cast_wait_summon_route() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicPreCastWait.as_byte();
    ctx.frame_timer = 1;
    host.actors[1].sub_route = 9;
    // First step: timer goes to 0 (still positive). Stay.
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    // Second step: timer crosses 0 → next state.
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::SummonInvoke.as_byte()
    ));
}

/// Full magic-spell flow walking from `MagicCastBegin` all the way to
/// `EndOfAction`, asserting each band transition. Mirrors the attack-flow
/// round-trip but exercises the magic dispatch table - `magic_cast_begin`
/// → `magic_pre_cast_wait` (with a cleared sub-route so we don't divert
/// to summon) → `magic_anim_chain` → `magic_sustain` → `magic_hit_loop`
/// → `magic_recovery` → `magic_exit` → `done_cleanup` → `done_fade_down`
/// → `end_of_action`.
#[test]
fn full_magic_flow_round_trips() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCastBegin.as_byte();

    // Set spell ID + MP cost so MagicCastBegin doesn't crash on division.
    host.actors[1].params[0] = 0x10;
    // One `(clip, shot)` pair, as every retail stream carries them.
    host.actors[1].params[1] = 0x21; // first chain anim
    host.actors[1].params[2] = 0x07; // its shot
    host.actors[1].params[3] = 0xFF; // chain terminator
    host.actors[1].mp = 100;
    host.spell_costs.insert(0x10, 20);
    host.actors[1].sub_route = 0; // not summon
    host.actors[1].current_anim = 0;
    host.actors[1].hit_count_bound = 0;

    // MagicCastBegin → MagicPreCastWait (no capture spell).
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::MagicPreCastWait.as_byte());
    assert_eq!(host.actors[1].mp, 80); // 100 - 20

    // MagicPreCastWait gates on frame_timer; it was set to 0x14 by the
    // previous step. Tick until the timer fires the transition.
    let mut iters = 0;
    while ctx.action_state == ActionState::MagicPreCastWait.as_byte() {
        step(&mut host, &mut ctx);
        iters += 1;
        assert!(iters < 1000, "stuck in MagicPreCastWait");
    }
    assert_eq!(ctx.action_state, ActionState::MagicAnimChain.as_byte());
    // The wait's exit bumped the cursor past the spell id and staged the
    // first anim byte (`0x801E4644..0x801E4664`): `params[1] = 0x21` is on
    // the stage, and for a non-Seru id the second bump parked the cursor on
    // the terminator. The spell id itself is never staged as a clip.
    assert_eq!(host.actors[1].queued_anim, 0x21, "0x29 stages params[1]");
    assert_eq!(host.actors[1].strike_index, 2);

    // MagicAnimChain looks one past the cursor (`params[3]`): the
    // terminator, so the chain transitions on its first step.
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::MagicSustain.as_byte());

    // MagicSustain holds while spell_iter != 0; we need to clear it.
    host.actors[1].spell_iter = 0;
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::MagicHitLoop.as_byte());

    // MagicHitLoop exits when current_anim == 0 (default).
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::MagicRecovery.as_byte());

    // MagicRecovery stays unless gate is 0 (default 0).
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::MagicExit.as_byte());

    // MagicExit similarly stays unless gate is 0 (default 0).
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::DoneCleanup.as_byte());

    // DoneCleanup → DoneFadeDown.
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::DoneFadeDown.as_byte());

    // Drain DoneFadeDown's frame timer. Should land on EndOfAction.
    let mut tick_count = 0;
    while ctx.action_state == ActionState::DoneFadeDown.as_byte() {
        step(&mut host, &mut ctx);
        tick_count += 1;
        assert!(tick_count < 1000, "stuck in DoneFadeDown");
    }
    assert_eq!(ctx.action_state, ActionState::EndOfAction.as_byte());
}

/// `MagicCastBegin` with both `bits & 0x10` and `bits & 0x20` set - verifies
/// the cost path picks the **Half** (`0x20`) branch over Quarter (`0x10`).
/// Dump-confirmed against the retail state-`0x28` block (`FUN_801E295C`
/// `0x801E3D0C`): `andi 0x20; bne <half>` short-circuits the `0x10` test.
#[test]
fn magic_cast_begin_half_takes_priority_over_quarter() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCastBegin.as_byte();
    host.actors[1].mp = 100;
    host.actors[1].params[0] = 0x10;
    host.spell_costs.insert(0x10, 40);
    // Both bits set - retail applies Half (0x20) and skips the 0x10 test.
    host.ability_bits.insert(1, 0x10 | 0x20);
    step(&mut host, &mut ctx);
    // Half: 40 - (40>>1) = 20; 100 - 20 = 80.
    assert_eq!(host.actors[1].mp, 80);
    assert_eq!(host.actors[1].last_mp_cost, 20);
}

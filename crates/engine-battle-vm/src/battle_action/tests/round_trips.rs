//! Whole-flow round trips and the unmapped / idle states.

use super::*;

#[test]
fn full_attack_flow_round_trips() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::Begin.as_byte();
    ctx.queued_action = 1;

    // Begin → PreActionWait.
    let out = step(&mut host, &mut ctx);
    assert!(matches!(out, StepOutcome::Transition { .. }));
    assert_eq!(ctx.action_state, ActionState::PreActionWait.as_byte());

    // PreActionWait → ActionSeed (prev_cleared = true by default).
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::ActionSeed.as_byte());

    // ActionSeed → AttackFace.
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::AttackFace.as_byte());

    // AttackFace → AttackChain (in range by default).
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::AttackChain.as_byte());

    // AttackChain: walk one anim then terminator.
    host.actors[1].params[0] = 0x10;
    host.actors[1].params[1] = 0xFF;
    step(&mut host, &mut ctx); // queue 0x10; the strike loop applies no damage
    assert!(
        !host
            .take()
            .iter()
            .any(|e| matches!(e, Event::ApplyDamage(..)))
    );
    // Anim system signals the staged swing finished (clears the
    // 0x801E370C read gate) before the chain reads the next byte.
    host.actors[1].flag_bits.clear(ActorFlags::ADVANCE_DONE);
    step(&mut host, &mut ctx); // terminator → AttackRecovery, SM clears ADVANCE_DONE
    assert_eq!(ctx.action_state, ActionState::AttackRecovery.as_byte());
    assert!(!host.actors[1].flag_bits.has(ActorFlags::ADVANCE_DONE));

    // AttackRecovery (advance_done cleared by SM) → AttackReturn.
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::AttackReturn.as_byte());

    // AttackReturn → DoneCleanup.
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::DoneCleanup.as_byte());

    // DoneCleanup → DoneFadeDown.
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::DoneFadeDown.as_byte());

    // Tick timer down until it transitions to EndOfAction.
    loop {
        let out = step(&mut host, &mut ctx);
        match out {
            StepOutcome::Stay => continue,
            StepOutcome::Transition { to, .. } => {
                assert_eq!(to, ActionState::EndOfAction.as_byte());
                break;
            }
            other => panic!("unexpected outcome during fade-down: {other:?}"),
        }
    }

    // EndOfAction (both sides alive) → PreActionWait.
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::PreActionWait.as_byte()
    ));
}

#[test]
fn unmapped_state_byte_surfaces_unknown() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = 0x07; // gap in the table
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::UnknownState { state: 0x07 });
}

#[test]
fn idle_hold_stays_and_pose_recover() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::IdleHold.as_byte();
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::Stay);
    assert!(host.take().contains(&Event::Pose(1, Pose::Recover)));
}

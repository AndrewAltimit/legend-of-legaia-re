//! The wait timer and `actor_tick`'s per-frame gating.

use super::*;

// ---- actor_tick / decrement_wait_timer wiring ----

#[test]
fn decrement_wait_timer_subtracts_delta() {
    let mut state = ActorState::new();
    state.wait_timer = 10;
    decrement_wait_timer(&mut state, 4);
    assert_eq!(state.wait_timer, 6);
}

#[test]
fn decrement_wait_timer_wraps_to_negative() {
    let mut state = ActorState::new();
    state.wait_timer = 1;
    decrement_wait_timer(&mut state, 3);
    // 1 - 3 = -2 (wrapping i16).
    assert_eq!(state.wait_timer, -2);
}

#[test]
fn actor_tick_skips_vm_when_timer_nonneg() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.wait_timer = 0;

    let bc = program(&[0x06, 99, 0x08]); // WRITE_26, HALT
    let r = actor_tick(&mut host, &mut state, &bc, 16);
    assert_eq!(r, ActorTickOutcome::Waiting);
    // VM was not entered - render_26 unchanged, no HALT flag.
    assert_eq!(state.render_26, 0);
    assert_eq!(state.flags & 0x8, 0);
    assert_eq!(state.pc, 0);
}

#[test]
fn actor_tick_runs_vm_when_timer_negative() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.wait_timer = -1;

    let bc = program(&[0x06, 99, 0x08]); // WRITE_26, HALT
    let r = actor_tick(&mut host, &mut state, &bc, 16);
    assert_eq!(r, ActorTickOutcome::Halted);
    assert_eq!(state.render_26, 99);
    assert_eq!(state.flags & 0x8, 0x8);
}

#[test]
fn actor_tick_reports_wait_seeded() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.wait_timer = -1;

    // op 0x09 WAIT_SET sets wait_timer = arg << 3 and breaks.
    let bc = program(&[0x09, 5]);
    let r = actor_tick(&mut host, &mut state, &bc, 16);
    assert_eq!(r, ActorTickOutcome::WaitSeeded);
    assert_eq!(state.wait_timer, 40); // 5 << 3
    // No HALT flag.
    assert_eq!(state.flags & 0x8, 0);
}

#[test]
fn actor_tick_reports_end_of_buffer_on_oor_opcode() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.wait_timer = -1;

    let bc = program(&[0x47]); // out of range
    let r = actor_tick(&mut host, &mut state, &bc, 16);
    assert!(matches!(r, ActorTickOutcome::EndOfBuffer { opcode: 0x47 }));
}

#[test]
fn actor_tick_reports_budget_exhausted() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.wait_timer = -1;

    // Infinite loop: SAVE_LOOP18 + LOOP_JUMP19 (no break opcode).
    // Easier: just use a long sequence of WRITE_26 ops with no HALT.
    let mut words = vec![];
    for _ in 0..32 {
        words.push(0x06);
        words.push(0x00);
    }
    let bc = program(&words);
    let r = actor_tick(&mut host, &mut state, &bc, 4);
    assert_eq!(r, ActorTickOutcome::BudgetExhausted);
}

/// `actor_tick` composed with `decrement_wait_timer` for a multi-frame
/// scenario where the script seeds a new wait inside the VM and the
/// next frame must decrement that wait before re-entering. Mirrors a
/// retail per-frame loop where one script `WAIT_SET` keeps the actor
/// idle for a known number of frames.
#[test]
fn actor_tick_wait_set_then_decrements_to_resume() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.wait_timer = -1; // VM eligible

    // WAIT_SET 4 (sets timer = 32 = 4<<3) then HALT.
    let bc = program(&[0x09, 4, 0x08]);

    // Frame 1: VM runs, hits WAIT_SET, breaks with WaitSeeded.
    let r1 = actor_tick(&mut host, &mut state, &bc, 16);
    assert_eq!(r1, ActorTickOutcome::WaitSeeded);
    assert_eq!(state.wait_timer, 32);

    // Frames 2..N: pre-tick decrements; until timer is back negative,
    // VM is gated. Use delta=8 so it takes 5 frames (32 → 24 → 16 → 8 → 0 → -8).
    for expected in [24, 16, 8, 0] {
        decrement_wait_timer(&mut state, 8);
        assert_eq!(state.wait_timer, expected);
        assert_eq!(
            actor_tick(&mut host, &mut state, &bc, 16),
            ActorTickOutcome::Waiting
        );
    }

    // One more pre-tick → timer = -8 (negative). VM runs, but we're
    // sitting at the HALT instruction now (PC was advanced past the
    // 2-word WAIT_SET on the seed step).
    decrement_wait_timer(&mut state, 8);
    assert_eq!(state.wait_timer, -8);
    assert_eq!(
        actor_tick(&mut host, &mut state, &bc, 16),
        ActorTickOutcome::Halted
    );
}

#[test]
fn actor_tick_pretick_then_tick_models_retail_frame() {
    // Compose decrement_wait_timer + actor_tick to model a full frame.
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.wait_timer = 8; // initially "still waiting"

    let bc = program(&[0x06, 42, 0x08]);

    // Frame 1: pre-tick takes timer 8 → 5 (delta = 3). Still nonneg, skip.
    decrement_wait_timer(&mut state, 3);
    assert_eq!(
        actor_tick(&mut host, &mut state, &bc, 16),
        ActorTickOutcome::Waiting
    );

    // Frame 2: 5 → 2 (still nonneg).
    decrement_wait_timer(&mut state, 3);
    assert_eq!(
        actor_tick(&mut host, &mut state, &bc, 16),
        ActorTickOutcome::Waiting
    );

    // Frame 3: 2 → -1 (now negative). VM runs, hits HALT.
    decrement_wait_timer(&mut state, 3);
    assert_eq!(
        actor_tick(&mut host, &mut state, &bc, 16),
        ActorTickOutcome::Halted
    );
    assert_eq!(state.render_26, 42);
}

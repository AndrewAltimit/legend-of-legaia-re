//! Op `0x2F` sub-ops that test and branch: the global predicate, the counter and slot saves, box / axis / radius predicates.

use super::*;

#[test]
fn op2f_subop_08_sets_global_predicate_and_subop_09_clears_it() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    step(&mut host, &mut state, &program(&[0x2F, 0x08]));
    assert_eq!(host.global_predicate, 1);
    state.pc = 0;
    step(&mut host, &mut state, &program(&[0x2F, 0x09]));
    assert_eq!(host.global_predicate, 0);
}

#[test]
fn op2f_subop_0a_branches_by_delta_when_predicate_set() {
    // `[2F][0A][delta]`. The predicate-set side falls to the shared
    // 0x801D38C8 tail (`lhu v0,0x4(s3); addiu s2,v0,0x3`) = `3 + op[2]`.
    let mut host = TestHost::default();
    host.global_predicate = 1;
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x0A, 5]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.pc, 8, "predicate set → pc += 3 + delta(5)");
}

#[test]
fn op2f_subop_0a_falls_through_when_predicate_clear() {
    let mut host = TestHost::default();
    host.global_predicate = 0;
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x0A, 5]);
    step(&mut host, &mut state, &bc);
    // `beq v0,zero,0x801d4a3c` with `li s2, 0x3` in the delay slot.
    assert_eq!(state.pc, 3);
}

#[test]
fn op2f_subop_0b_falls_through_when_predicate_set() {
    let mut host = TestHost::default();
    host.global_predicate = 1;
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x0B, 5]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.pc, 3);
}

#[test]
fn op2f_subop_0b_branches_by_delta_when_predicate_clear() {
    let mut host = TestHost::default();
    host.global_predicate = 0;
    let mut state = ActorState::new();
    step(&mut host, &mut state, &program(&[0x2F, 0x0B, 5]));
    assert_eq!(state.pc, 8, "predicate clear → pc += 3 + delta(5)");
}

#[test]
fn op2f_subop_0f_clears_global_counter() {
    let mut host = TestHost::default();
    host.global_counter = 7;
    let mut state = ActorState::new();
    step(&mut host, &mut state, &program(&[0x2F, 0x0F]));
    assert_eq!(host.global_counter, 0);
}

#[test]
fn op2f_subop_10_cycles_counter_and_writes_low_byte_to_field_86() {
    let mut host = TestHost::default();
    host.global_counter = 5;
    let mut state = ActorState::new();
    // Pre-set the high byte of field_86 to verify it's preserved.
    state.field_86 = 0xAA00;
    step(&mut host, &mut state, &program(&[0x2F, 0x10]));
    // Captured value (5) goes to low byte of field_86; counter increments.
    assert_eq!(state.field_86, 0xAA05);
    assert_eq!(host.global_counter, 6);
}

#[test]
fn op2f_subop_10_wraps_counter_at_16() {
    let mut host = TestHost::default();
    host.global_counter = 16; // > 15 → wraps to 0 first
    let mut state = ActorState::new();
    step(&mut host, &mut state, &program(&[0x2F, 0x10]));
    // Counter wrapped to 0, captured 0, then incremented to 1.
    assert_eq!(state.field_86 & 0xFF, 0);
    assert_eq!(host.global_counter, 1);
}

#[test]
fn op2f_subop_11_saves_world_to_slot_indexed_by_field_86_low_byte() {
    // The pair 0x10 + 0x11 round-trips: cycle counter writes low byte
    // of field_86, then 0x11 saves world to that slot. Verify with a
    // pre-set field_86 value to keep the test free of cycle-counter
    // sequencing.
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.field_86 = 0xAA0B; // low byte = 11 → slot index 11 (& 0xF = 11)
    state.world_x = -50;
    state.world_y = -100;
    state.world_z = -150;
    state.world_y_mirror = -200;
    step(&mut host, &mut state, &program(&[0x2F, 0x11]));
    assert_eq!(host.move_slot_load_u16(11, 0) as i16, -50);
    assert_eq!(host.move_slot_load_u16(11, 2) as i16, -100);
    assert_eq!(host.move_slot_load_u16(11, 4) as i16, -150);
    assert_eq!(host.move_slot_load_u16(11, 6) as i16, -200);
}

#[test]
fn op2f_subop_10_then_subop_11_produces_a_running_capture() {
    // Cycle the counter twice (each captures the pre-increment value
    // into field_86 low byte) and verify the slot writes hit the right
    // indices. Counter starts at 0:
    //   step 0x10: captures 0 → field_86 lo = 0; counter becomes 1.
    //   step 0x11: saves world to slot 0.
    //   step 0x10: captures 1 → field_86 lo = 1; counter becomes 2.
    //   step 0x11: saves world to slot 1.
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.world_x = 100;
    for expected_slot in 0..3 {
        state.world_x = 100 + expected_slot as i16;
        state.pc = 0;
        step(&mut host, &mut state, &program(&[0x2F, 0x10]));
        state.pc = 0;
        step(&mut host, &mut state, &program(&[0x2F, 0x11]));
        assert_eq!(
            host.move_slot_load_u16(expected_slot as u16, 0) as i16,
            100 + expected_slot as i16
        );
    }
    assert_eq!(host.global_counter, 3);
}

#[test]
fn op2f_subop_06_branches_by_delta_when_player_outside_box() {
    // `[2F][06][xa][za][xb][zb][delta]` - a 7-halfword conditional branch.
    // Box corners (xa=10, za=20, xb=20, zb=30) scaled by 0x80 + 0x40 =
    // x in [1344, 2624], z in [2624, 3904]. Player at (0, 0, 0) is
    // outside → 0x06 takes the branch: `7 + op[6]`.
    let mut host = TestHost::default();
    host.player_xyz = [0, 0, 0];
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x06, 10, 20, 20, 30, 4]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.pc, 11, "outside → pc += 7 + delta(4)");
}

#[test]
fn op2f_subop_06_falls_through_when_player_inside_box() {
    let mut host = TestHost::default();
    host.player_xyz = [2000, 0, 3000]; // inside the [1344..2624] × [2624..3904] band
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x06, 10, 20, 20, 30, 4]);
    step(&mut host, &mut state, &bc);
    // Untaken side is the preset `li s2, 0x7` at 0x801D3838 - the full
    // instruction width, not one word.
    assert_eq!(state.pc, 7);
}

#[test]
fn op2f_subop_07_is_the_inverse_branch_of_06() {
    let mut host = TestHost::default();
    let bc = program(&[0x2F, 0x07, 10, 20, 20, 30, 4]);

    // Inside → 0x07 branches.
    host.player_xyz = [2000, 0, 3000];
    let mut inside = ActorState::new();
    step(&mut host, &mut inside, &bc);
    assert_eq!(inside.pc, 11, "inside → pc += 7 + delta(4)");

    // Outside → 0x07 falls through.
    host.player_xyz = [0, 0, 0];
    let mut outside = ActorState::new();
    step(&mut host, &mut outside, &bc);
    assert_eq!(outside.pc, 7);
}

#[test]
fn op2f_subop_06_canonicalises_the_box_in_the_bytecode() {
    // `sh v1,0x4(s3)` / `sh a0,0x4(s0)` at 0x801D3784 swap op[2] with op[4]
    // when op[4] < op[2]; the z pair swaps op[3] with op[5]. Both are
    // ordinary self-modifying writes through the deferred bytecode hook.
    let mut host = TestHost::default();
    host.player_xyz = [0, 0, 0];
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x06, 20, 30, 10, 20, 0]);
    step(&mut host, &mut state, &bc);
    assert_eq!(host.move_bytecode_read_u16(2), 10, "op[2] takes op[4]");
    assert_eq!(host.move_bytecode_read_u16(4), 20, "op[4] takes op[2]");
    assert_eq!(host.move_bytecode_read_u16(3), 20, "op[3] takes op[5]");
    assert_eq!(host.move_bytecode_read_u16(5), 30, "op[5] takes op[3]");
}

#[test]
fn op2f_subop_13_branches_by_delta_when_flag_set() {
    // `[2F][13][flag][delta]`: flag SET takes the branch, `pc += 4 + delta`
    // (`lhu v0,0x6(s3); addiu s2,v0,4` at the 0x801D4838 epilogue arm).
    let mut host = TestHost::default();
    host.ext_query_flag_bank_returns = 1;
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x13, 7, 3]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.pc, 7, "flag set → pc += 4 + delta(3)");
}

#[test]
fn op2f_subop_13_falls_through_when_flag_clear() {
    let mut host = TestHost::default();
    host.ext_query_flag_bank_returns = 0;
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x13, 7, 3]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.pc, 4, "flag clear → fall through the 4-word op");
}

#[test]
fn op2f_subop_14_branches_by_delta_when_flag_clear() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    // 0x14 with the flag set falls through (size 4).
    host.ext_query_flag_bank_returns = 1;
    let bc = program(&[0x2F, 0x14, 7, 3]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.pc, 4, "flag set → fall through");
    // Flag clear takes the branch: `pc += 4 + delta`.
    let mut host2 = TestHost::default();
    host2.ext_query_flag_bank_returns = 0;
    let mut state2 = ActorState::new();
    step(&mut host2, &mut state2, &bc);
    assert_eq!(state2.pc, 7, "flag clear → pc += 4 + delta(3)");
}

#[test]
fn op2f_subop_14_negative_delta_spin_waits() {
    // jou's ambient lightning cyclers idle on `2F 14 0364 FFFA` placed at
    // word 2 after a wait op: flag clear → `pc += 4 - 6 = -2`, landing back
    // on the preceding op - the retail spin-wait-until-flag idiom.
    let mut host = TestHost::default();
    host.ext_query_flag_bank_returns = 0;
    let mut state = ActorState::new();
    state.pc = 2;
    let bc = program(&[0x00, 0x00, 0x2F, 0x14, 0x364, 0xFFFA]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.pc, 0, "flag clear → pc += 4 + (-6) = back 2 words");
}

#[test]
fn op2f_subop_36_axis_threshold_below_branches_by_delta() {
    // 0x36 predicate: op[2] < (0x8E - axis). axis=0, op[2]=0x40 → 0x40 < 0x8E
    // true. All four of 0x36..0x39 share the 0x801D4830 branch tail with
    // 0x13 / 0x14: `[2F][36][arg][delta]`, taken side = `4 + op[3]`.
    let mut host = TestHost::default();
    host.axis_threshold = 0;
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x36, 0x40, 3]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.pc, 7, "predicate true → pc += 4 + delta(3)");
}

#[test]
fn op2f_subop_36_axis_threshold_above_falls_through() {
    // op[2]=0xFF, axis=0: 0xFF < 0x8E is false → the preset `li s2, 0x4`.
    let mut host = TestHost::default();
    host.axis_threshold = 0;
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x36, 0xFF, 3]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.pc, 4);
}

#[test]
fn op2f_subop_36_negative_delta_walks_the_pc_back() {
    // The delta is read `lhu` but truncated to 16 bits and sign-extended by
    // the epilogue, so a negative displacement is a backwards branch.
    let mut host = TestHost::default();
    host.axis_threshold = 0;
    let mut state = ActorState::new();
    state.pc = 2;
    let bc = program(&[0x00, 0x00, 0x2F, 0x36, 0x40, 0xFFFA]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.pc, 0, "predicate true → pc += 4 + (-6)");
}

#[test]
fn op2f_subop_37_is_inverse_of_36() {
    // 0x37: (0x8E - axis) < op[2]. With axis=0, op[2]=0xFF → 0x8E < 0xFF true.
    let mut host = TestHost::default();
    host.axis_threshold = 0;
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x37, 0xFF, 3]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.pc, 7);
    // axis=0, op[2]=0x40 → 0x8E < 0x40 false → fall through.
    let mut state2 = ActorState::new();
    let bc2 = program(&[0x2F, 0x37, 0x40, 3]);
    step(&mut host, &mut state2, &bc2);
    assert_eq!(state2.pc, 4);
}

#[test]
fn op2f_subop_38_predicate_outside_radius() {
    let mut host = TestHost::default();
    host.player_xyz = [0, 0, 0];
    let mut state = ActorState::new();
    // Actor at (10, 0, 0), player at origin → dist² = 100. r=8 → r²=64.
    // 0x38: r² < dist² → 64 < 100 true → branch by delta.
    state.world_x = 10;
    let bc = program(&[0x2F, 0x38, 8, 3]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.pc, 7);
}

#[test]
fn op2f_subop_39_predicate_inside_radius() {
    let mut host = TestHost::default();
    host.player_xyz = [0, 0, 0];
    let mut state = ActorState::new();
    // Actor at (3, 0, 4), player at origin → dist² = 25. r=10 → r²=100.
    // 0x39: dist² < r² → 25 < 100 true → branch by delta.
    state.world_x = 3;
    state.world_z = 4;
    let bc = program(&[0x2F, 0x39, 10, 3]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.pc, 7);
    // Move actor to (100, 0, 0): dist² = 10000, r²=100 → false → fall through.
    let mut state2 = ActorState::new();
    state2.world_x = 100;
    let bc2 = program(&[0x2F, 0x39, 10, 3]);
    step(&mut host, &mut state2, &bc2);
    assert_eq!(state2.pc, 4);
}

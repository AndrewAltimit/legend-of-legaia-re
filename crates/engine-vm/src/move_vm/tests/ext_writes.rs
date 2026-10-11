//! Op `0x2F` sub-ops that write actor state: anim blocks, world coordinates, tweens, lerps, the Bezier evaluator and in-bytecode edits.

use super::*;

#[test]
fn op2f_dispatches_extension_vm() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    // op[0] = 0x2F, op[1] = sub 0x01 (debug print).
    state.world_x = 1;
    state.world_y = 2;
    state.world_z = 3;
    let bc = program(&[0x2F, 0x01]);
    let r = step(&mut host, &mut state, &bc);
    assert_eq!(r, StepResult::Advance);
    assert_eq!(host.ext_debug_world_calls, vec![(1, 2, 3)]);
    // Sub-0x01 returns size 2.
    assert_eq!(state.pc, 2);
}

#[test]
fn op2f_subop_02_clears_face_rotation() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.face_rotation = 7;
    let bc = program(&[0x2F, 0x02]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.face_rotation, 0);
    // Size 2 - `li s2, 0x2` in the 0x801D36B0 exit.
    assert_eq!(state.pc, 2);
}

#[test]
fn op2f_subop_29_ramp_path_calls_host_with_negated_target() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    // [0x2F, 0x29, slot=3, target=10, ticks=5]
    let bc = program(&[0x2F, 0x29, 3, 10, 5]);
    step(&mut host, &mut state, &bc);
    assert_eq!(host.ext_scratchpad_ramp_calls, vec![(3, -10, 5)]);
    assert!(host.ext_scratchpad_write_calls.is_empty());
}

#[test]
fn op2f_subop_29_immediate_path_when_ticks_zero() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x29, 3, 10, 0]);
    step(&mut host, &mut state, &bc);
    assert!(host.ext_scratchpad_ramp_calls.is_empty());
    assert_eq!(host.ext_scratchpad_write_calls, vec![(3, -10)]);
}

#[test]
fn op2f_subop_3c_immediate_fade_color() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x3C, 0xFF, 0x80, 0x40, 0]);
    step(&mut host, &mut state, &bc);
    assert_eq!(host.ext_fade_calls, vec![(([0xFF, 0x80, 0x40]), 0)]);
    assert_eq!(state.pc, 6);
}

#[test]
fn op2f_subop_1c_sets_flag_bank_via_host() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x1C, 42]);
    step(&mut host, &mut state, &bc);
    assert_eq!(host.ext_set_flag_bank_calls, vec![42]);
    // Sub-0x1C returns size 3.
    assert_eq!(state.pc, 3);
}

#[test]
fn op2f_subop_15_sets_flag_bit_800000() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x15]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.flags & 0x800000, 0x800000);
}

// ---- 0x2B / 0x2D / 0x33 anim-block writes ----

#[test]
fn op2f_subop_2b_writes_anim_block_b4_through_ba() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    // [0x2F, 0x2B, w0, w1, w2, w3] - writes to +0xB4/B6/B8/BA.
    let bc = program(&[0x2F, 0x2B, 0x1111, 0x2222, 0x3333, 0x4444]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.anim_block_u16(8), 0x1111);
    assert_eq!(state.anim_block_u16(10), 0x2222);
    assert_eq!(state.anim_block_u16(12), 0x3333);
    assert_eq!(state.anim_block_u16(14), 0x4444);
}

#[test]
fn op2f_subop_2d_adds_to_anim_block_b4_through_ba() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.anim_block_u16_set(8, 100);
    state.anim_block_u16_set(10, 200);
    state.anim_block_u16_set(12, 300);
    state.anim_block_u16_set(14, 400);
    let bc = program(&[0x2F, 0x2D, 5, 6, 7, 8]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.anim_block_u16(8), 105);
    assert_eq!(state.anim_block_u16(10), 206);
    assert_eq!(state.anim_block_u16(12), 307);
    assert_eq!(state.anim_block_u16(14), 408);
}

#[test]
fn op2f_subop_2d_wrapping_add_when_overflowed() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.anim_block_u16_set(8, 0xFFFF);
    let bc = program(&[0x2F, 0x2D, 1, 0, 0, 0]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.anim_block_u16(8), 0x0000);
}

#[test]
fn op2f_subop_33_adds_to_anim_block_c0_through_c6() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.anim_block_u16_set(20, 1000);
    state.anim_block_u16_set(22, 2000);
    state.anim_block_u16_set(24, 3000);
    state.anim_block_u16_set(26, 4000);
    let bc = program(&[0x2F, 0x33, 11, 22, 33, 44]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.anim_block_u16(20), 1011);
    assert_eq!(state.anim_block_u16(22), 2022);
    assert_eq!(state.anim_block_u16(24), 3033);
    assert_eq!(state.anim_block_u16(26), 4044);
}

#[test]
fn op2f_subop_0c_writes_field_50() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x0C, 0xABCD]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.field_50, 0xABCD);
}

#[test]
fn op2f_subop_0d_adds_to_field_50_with_wrap() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.field_50 = 0xFFFE;
    let bc = program(&[0x2F, 0x0D, 5]);
    step(&mut host, &mut state, &bc);
    // 0xFFFE + 5 wraps to 0x0003.
    assert_eq!(state.field_50, 0x0003);
}

#[test]
fn op2f_subop_25_then_26_round_trips_world_coords() {
    // Save world coords to slot 3, perturb the actor, then load back.
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.world_x = 100;
    state.world_y = 200;
    state.world_z = 300;
    state.world_y_mirror = 400;
    let save = program(&[0x2F, 0x25, 3]);
    step(&mut host, &mut state, &save);
    // Perturb.
    state.world_x = -1;
    state.world_y = -1;
    state.world_z = -1;
    state.world_y_mirror = -1;
    // Reset PC for the second program.
    state.pc = 0;
    let load = program(&[0x2F, 0x26, 3]);
    step(&mut host, &mut state, &load);
    assert_eq!(state.world_x, 100);
    assert_eq!(state.world_y, 200);
    assert_eq!(state.world_z, 300);
    assert_eq!(state.world_y_mirror, 400);
}

#[test]
fn op2f_subop_27_saves_tween_src_triple_into_first_six_bytes_of_slot() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.tween_src_x = 0x1111;
    state.tween_src_y = 0x2222;
    state.tween_src_z = 0x3333;
    let bc = program(&[0x2F, 0x27, 5]);
    step(&mut host, &mut state, &bc);
    // slot[5] bytes 0..2/2..4/4..6 should match the three i16 values.
    assert_eq!(host.move_slot_load_u16(5, 0), 0x1111);
    assert_eq!(host.move_slot_load_u16(5, 2), 0x2222);
    assert_eq!(host.move_slot_load_u16(5, 4), 0x3333);
}

#[test]
fn op2f_subop_28_loads_scales_and_clamps() {
    // Pre-load slot 7 with three known u16 values, then run sub-op 0x28
    // with scale operands chosen so that the y-axis result clamps to
    // -0xFF and the z-axis result clamps to +0xFF.
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    host.move_slot_save_u16(7, 0, 1000); // becomes tween_src_x as-is
    host.move_slot_save_u16(7, 2, -2000i16 as u16); // raw_y = -2000
    host.move_slot_save_u16(7, 4, 2000); // raw_z = 2000
    // op_w(2)=slot=7, op_w(3)=scale_y, op_w(4)=scale_z. Use 0x4000
    // (=2.0 in 12.4 fixed) so raw * scale >> 12 hits the clamp band.
    let bc = program(&[0x2F, 0x28, 7, 0x4000, 0x4000]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.tween_src_x, 1000);
    // -2000 * 0x4000 >> 12 = -8000 → clamps to -0xFF.
    assert_eq!(state.tween_src_y, -0xFF);
    // 2000 * 0x4000 >> 12 = 8000 → clamps to +0xFF.
    assert_eq!(state.tween_src_z, 0xFF);
}

#[test]
fn op2f_subop_31_then_32_round_trips_render_banks() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.render_24 = -100;
    state.render_26 = -200;
    state.render_28 = -300;
    state.world_y_mirror = -400;
    step(&mut host, &mut state, &program(&[0x2F, 0x31, 9]));
    state.render_24 = 0;
    state.render_26 = 0;
    state.render_28 = 0;
    state.world_y_mirror = 0;
    state.pc = 0;
    step(&mut host, &mut state, &program(&[0x2F, 0x32, 9]));
    assert_eq!(state.render_24, -100);
    assert_eq!(state.render_26, -200);
    assert_eq!(state.render_28, -300);
    assert_eq!(state.world_y_mirror, -400);
}

#[test]
fn op2f_subop_34_then_35_round_trips_field_72() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.field_72 = 0xCAFE;
    step(&mut host, &mut state, &program(&[0x2F, 0x34, 12]));
    state.field_72 = 0;
    state.pc = 0;
    step(&mut host, &mut state, &program(&[0x2F, 0x35, 12]));
    assert_eq!(state.field_72, 0xCAFE);
}

#[test]
fn op2f_subop_2a_lerps_world_toward_player_position() {
    // op_w(2..4) = base x/y/z; op_w(5..7) = per-axis t (`>> 12` shift).
    // With t=0x1000 (= 1.0 in 12.4 fixed) the result lands exactly on
    // the player position.
    let mut host = TestHost::default();
    host.player_xyz = [1000, 2000, 3000];
    let mut state = ActorState::new();
    let bc = program(&[
        0x2F, 0x2A, // sub-op
        500, 800, 1500, // base
        0x1000, 0x1000, 0x1000, // t = 1.0
    ]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.world_x, 1000);
    assert_eq!(state.world_y, 2000);
    assert_eq!(state.world_z, 3000);
}

#[test]
fn op2f_subop_2a_at_t_zero_keeps_base() {
    let mut host = TestHost::default();
    host.player_xyz = [9999, 9999, 9999];
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x2A, 500, 800, 1500, 0, 0, 0]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.world_x, 500);
    assert_eq!(state.world_y, 800);
    assert_eq!(state.world_z, 1500);
}

#[test]
fn op2f_subop_24_uses_fixed_origin_for_x_and_z() {
    // Sub-0x24 X: target = -(base + origin); Y still toward player.
    let mut host = TestHost::default();
    host.fixed_origin_xz = (200, 300);
    host.player_xyz = [0, 5000, 0]; // Y target
    let mut state = ActorState::new();
    // base = (100, 1000, 50), t = (0x1000, 0x1000, 0x1000)
    let bc = program(&[0x2F, 0x24, 100, 1000, 50, 0x1000, 0x1000, 0x1000]);
    step(&mut host, &mut state, &bc);
    // X target = -(100 + 200) = -300. Lerp at t=1 → -300.
    assert_eq!(state.world_x, -300);
    // Y target = player.world_y (5000). Lerp at t=1 → 5000.
    assert_eq!(state.world_y, 5000);
    // Z target = -(50 + 300) = -350.
    assert_eq!(state.world_z, -350);
}

#[test]
fn op2f_subop_0e_advances_pc_by_eleven_and_writes_the_curve_point() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    // a = P0 = (10, 20, 30), off = (1, 2, 3), b = P2 = (40, 60, 90)
    // control C = ((10+40)/2 + 1, (20+60)/2 + 2, (30+90)/2 + 3) = (26, 42, 63)
    // FUN_801E45BC at t = actor[+0x50] = 0x800: (P0 + 2C + P2) / 4, floored
    // = (102/4, 164/4, 246/4) = (25, 41, 61).
    state.field_50 = 0x800;
    let bc = program(&[0x2F, 0x0E, 10, 20, 30, 1, 2, 3, 40, 60, 90]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.world_x, 25);
    assert_eq!(state.world_y, 41);
    assert_eq!(state.world_z, 61);
    assert_eq!(
        state.pc, 11,
        "0x0E must advance past the entire 11-word instruction"
    );
}

#[test]
fn op2f_subop_0e_curve_endpoints_are_p0_and_p2() {
    // t = 0 lands on P0 and t = 0x1000 on P2 - the control point (the old
    // port's answer for every t) is never reached at either end.
    for (t, want) in [(0u16, (10i16, 20i16, 30i16)), (0x1000, (40, 60, 90))] {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.field_50 = t;
        let bc = program(&[0x2F, 0x0E, 10, 20, 30, 1, 2, 3, 40, 60, 90]);
        step(&mut host, &mut state, &bc);
        assert_eq!((state.world_x, state.world_y, state.world_z), want);
    }
}

#[test]
fn op2f_subop_12_uses_slot_indexed_by_field_86_low_byte() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.field_86 = 0x4205; // slot index = 5
    // Pre-populate slot 5 with a = (50, 60, 70).
    host.move_slot_save_u32(5, 0, (50u16 as u32) | ((60u16 as u32) << 16));
    host.move_slot_save_u32(5, 4, (70u16 as u32) | ((0u16 as u32) << 16));
    // off = (1, 2, 3), b = P2 = (50, 60, 70); P0 = slot = (50, 60, 70)
    // control C = ((50 + 50)/2 + 1, (60 + 60)/2 + 2, (70 + 70)/2 + 3)
    //           = (51, 62, 73)
    // at t = 0x800: (P0 + 2C + P2) / 4 = (50, 61, 71) (floored)
    state.field_50 = 0x800;
    let bc = program(&[0x2F, 0x12, 1, 2, 3, 50, 60, 70]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.world_x, 50);
    assert_eq!(state.world_y, 61);
    assert_eq!(state.world_z, 71);
    assert_eq!(state.pc, 8, "0x12 must advance past the 8-word instruction");
}

#[test]
fn op2f_subop_23_anim_lerp_zero_denom_is_noop() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.anim_3c = 100;
    state.anim_3e = 200;
    state.anim_40 = 300;
    // op[5] = 0 → divide-trap path; we skip the update.
    let bc = program(&[0x2F, 0x23, 0, 0, 0, 0]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.anim_3c, 100);
    assert_eq!(state.anim_3e, 200);
    assert_eq!(state.anim_40, 300);
    // The trap guard changes whether the lerp lands, not how wide the
    // instruction is - `li s2, 0x6` at 0x801D4100 is on both paths.
    assert_eq!(state.pc, 6);
}

#[test]
fn op2f_subop_23_anim_lerp_full_ratio_writes_target_offset() {
    let mut host = TestHost::default();
    host.dat_1f800393 = 1;
    let mut state = ActorState::new();
    // With dat=1, denom=1: t = (1 << 12) / 1 = 4096.
    // anim_3c = 0; first pass: 0 - (0 * 4096 >> 12) = 0
    //          ; second pass: 0 + ((100 - 0) * 4096 >> 12) = 100
    let bc = program(&[0x2F, 0x23, 100, 200, 300, 1]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.anim_3c, 100);
    assert_eq!(state.anim_3e, 200);
    assert_eq!(state.anim_40, 300);
}

#[test]
fn op2f_subop_04_writes_actor_world_into_bytecode_buffer() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.world_x = 10;
    state.world_y = 20;
    state.world_z = 30;
    // op[2] = 5 → writes at state.pc(0) + 5 + 3 = word indices 8, 9, 10.
    let bc = program(&[0x2F, 0x04, 5]);
    host.bytecode_buffer = bc.clone();
    step(&mut host, &mut state, &bc);
    assert_eq!(host.bytecode_buffer[8], 10);
    assert_eq!(host.bytecode_buffer[9], 20);
    assert_eq!(host.bytecode_buffer[10], 30);
    // Size 3 - `li s2, 0x3` at 0x801D36F8, in the exit that follows the
    // three coordinate stores.
    assert_eq!(state.pc, 3);
}

#[test]
fn op2f_subop_1e_in_place_add_to_bytecode() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    // op[2] = 3, op[3] = 5 → buffer[state.pc(0) + 3 + 4 = 7] += 5.
    let bc = program(&[0x2F, 0x1E, 3, 5]);
    host.bytecode_buffer = bc.clone();
    host.bytecode_buffer[7] = 100;
    step(&mut host, &mut state, &bc);
    assert_eq!(host.bytecode_buffer[7], 105);
}

#[test]
fn op2f_subop_1b_copy_loop_within_bytecode() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    // op[2] = 0 (src), op[3] = 4 (dst), op[4] = 3 (count).
    // src base = state.pc(0) + 0 + 5 = 5. dst base = 0 + 4 + 5 = 9.
    // Copies buffer[5..8] → buffer[9..12].
    let bc = program(&[0x2F, 0x1B, 0, 4, 3]);
    host.bytecode_buffer = bc.clone();
    host.bytecode_buffer[5] = 0xAAAA;
    host.bytecode_buffer[6] = 0xBBBB;
    host.bytecode_buffer[7] = 0xCCCC;
    // Pre-fill destination with sentinels so we can detect writes.
    host.bytecode_buffer[9] = 0;
    host.bytecode_buffer[10] = 0;
    host.bytecode_buffer[11] = 0;
    step(&mut host, &mut state, &bc);
    assert_eq!(host.bytecode_buffer[9], 0xAAAA);
    assert_eq!(host.bytecode_buffer[10], 0xBBBB);
    assert_eq!(host.bytecode_buffer[11], 0xCCCC);
}

#[test]
fn op2f_subop_1b_zero_count_is_noop() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x1B, 0, 4, 0]);
    host.bytecode_buffer = bc.clone();
    let before = host.bytecode_buffer.clone();
    step(&mut host, &mut state, &bc);
    assert_eq!(host.bytecode_buffer, before);
}

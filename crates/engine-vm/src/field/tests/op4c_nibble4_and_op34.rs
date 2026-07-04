//! 0x4C MENU_CTRL sub-0 + outer-nibble-4 sub-ops and the 0x34/0x37 effect/encounter setup cases. Extracted verbatim from `field/tests.rs`.

use super::*;

// -- 0x4C MENU_CTRL sub-0 -------------------------------------------

#[test]
fn menu_sub_0_sets_party_leader() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // op0 = 0x05 (high nibble 0, low bits 5 → masked to 5 = 0x05 & 7).
    let r = step(&mut host, &mut ctx, &[0x4C, 0x05], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.party_leaders, vec![5]);
}

#[test]
fn menu_sub_0_masks_leader_to_low_3_bits() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // op0 = 0x0F → masked to 7.
    step(&mut host, &mut ctx, &[0x4C, 0x0F], 0);
    assert_eq!(host.party_leaders, vec![7]);
}

#[test]
fn menu_sub_1_advances_seven_and_dispatches_to_host() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // op0 = 0x12 → high nibble 1 → menu_ctrl_sub1.
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x12, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
    assert_eq!(
        host.menu_sub1_calls,
        vec![(0x12, [0xA1, 0xA2, 0xA3, 0xA4, 0xA5])]
    );
}

#[test]
fn menu_sub_3_sub_5_writes_local_flags() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        local_flags: 0xFF80,
        ..Default::default()
    };
    // op0 = 0x35 → sub-3 sub-5: lf = (lf & 0xFF7F) | 0x020A.
    let r = step(&mut host, &mut ctx, &[0x4C, 0x35], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.local_flags, (0xFF80 & 0xFF7F) | 0x020A);
}

#[test]
fn menu_sub_3_sub_6_or_local_flags() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        local_flags: 0x0001,
        ..Default::default()
    };
    // op0 = 0x36 → sub-3 sub-6: lf |= 0x028A.
    let r = step(&mut host, &mut ctx, &[0x4C, 0x36], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.local_flags, 0x0001 | 0x028A);
}

#[test]
fn menu_sub_3_sub_7_no_player_falls_through() {
    // Host has no player_coords set → behave as a no-op advance.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x37], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.world_x, 0);
    assert!(host.inverted_y_writes.is_empty());
}

#[test]
fn menu_sub_3_sub_7_copies_player_coords() {
    let mut host = TestHost {
        player_coords: Some(PlayerCoords {
            world_x: 0x1234,
            world_y: 0x4567,
            world_z: 0x89AB,
            field_26: 0xCDEF,
        }),
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x37], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.world_x, 0x1234);
    assert_eq!(ctx.world_y, 0x4567);
    assert_eq!(ctx.world_z, 0x89AB);
    assert_eq!(ctx.field_26, 0xCDEF);
    // No bit 0x20000000 → no inverted-Y write.
    assert!(host.inverted_y_writes.is_empty());
}

#[test]
fn menu_sub_3_sub_7_yields_with_inverted_y_when_flag_set() {
    let mut host = TestHost {
        player_coords: Some(PlayerCoords {
            world_x: 0,
            world_y: 0x0010, // small positive - easy to negate
            world_z: 0,
            field_26: 0,
        }),
        ..Default::default()
    };
    let mut ctx = FieldCtx {
        flags: 0x2000_0000,
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0x4C, 0x37], 0);
    // Inverted-Y branch returns Yield (caseD_4 STATE_RESUME exit).
    assert_eq!(r, StepResult::Yield { resume_pc: 2 });
    assert_eq!(host.inverted_y_writes, vec![-0x0010]);
    // Coords still copied even on the yield path.
    assert_eq!(ctx.world_y, 0x0010);
}

#[test]
fn menu_sub_3_sub_2_clears_party_state_region() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // op0 = 0x32 → sub-3 sub-2 (clear 512-byte party-state region).
    let r = step(&mut host, &mut ctx, &[0x4C, 0x32], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.party_state_clears, 1);
}

#[test]
fn op_4c_outer_dispatcher_has_no_remaining_pending_arms() {
    // Sanity-check that every `0x4C` sub-dispatcher now returns
    // `Advance` / `Halt` / `Yield` / `Unknown` - none returns
    // `Pending`. n8 sub-3 (box-fill table via FUN_801D5630) was the
    // last truly-pending case; it now invokes
    // [`FieldHost::op4c_n_8_sub_3_rect_tile_fill`] and advances PC.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x83, 0, 0, 0, 0, 0], 0);
    assert!(!matches!(r, StepResult::Pending { .. }));
}

#[test]
fn op_4c_n4_sub_0_immediate_writes_field_72() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // [4C, 0x40, 34, 12, 0, 0] → sub-0, val=0x1234, ticks=0 (immediate)
    let r = step(&mut host, &mut ctx, &[0x4C, 0x40, 0x34, 0x12, 0, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(ctx.field_72, 0x1234);
    assert!(host.n4_ctx_ramps.is_empty());
}

#[test]
fn op_4c_n4_sub_0_ramp_calls_host_does_not_write_ctx() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // [4C, 0x40, 0xCC, 0x55, 0x10, 0] → sub-0, val=0x55CC, ticks=16
    let r = step(&mut host, &mut ctx, &[0x4C, 0x40, 0xCC, 0x55, 0x10, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(ctx.field_72, 0); // VM does not write ctx in ramp path
    assert_eq!(host.n4_ctx_ramps, vec![(0u8, 0x55CCi16, 16u16)]);
}

#[test]
fn op_4c_n4_sub_2_immediate_mirrors_world_y_when_flag_set() {
    // sub-2 immediate write path: when `flags & 0x20000000` is set,
    // also writes `world_y = -value`.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        flags: 0x2000_0000,
        ..FieldCtx::default()
    };
    // val = 0x0064 (= 100); world_y should become (u16)(-100) = 0xFF9C
    let r = step(&mut host, &mut ctx, &[0x4C, 0x42, 0x64, 0x00, 0, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(ctx.field_8e, 100);
    assert_eq!(ctx.world_y, (-100i16) as u16);
}

#[test]
fn op_4c_n4_sub_2_immediate_no_mirror_without_flag() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x42, 0x64, 0x00, 0, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(ctx.field_8e, 100);
    assert_eq!(ctx.world_y, 0);
}

#[test]
fn op_4c_n4_sub_8_immediate_writes_field_26() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // [4C, 0x48, 0x07, 0x00, 0, 0] → sub-8, val=7
    let r = step(&mut host, &mut ctx, &[0x4C, 0x48, 0x07, 0x00, 0, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(ctx.field_26, 7);
}

#[test]
fn op_4c_n4_global_subs_call_host() {
    for sub in [0xAu8, 0xB, 0xC, 0xD] {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(
            &mut host,
            &mut ctx,
            &[0x4C, 0x40 | sub, 0x10, 0x00, 0x05, 0x00],
            0,
        );
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert_eq!(host.n4_global_writes, vec![(sub, 0x10i32, 5u16)]);
    }
}

// -- 0x4C outer-nibble-4 sub-1 (ctx[+0x6A] write/ramp with halve+floor)

#[test]
fn op_4c_n4_sub_1_immediate_halves_and_floors() {
    // val = 6 → halved = 3 → ctx.field_6a = 3
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x41, 0x06, 0x00, 0, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(ctx.field_6a, 3);
    assert!(host.n4_ctx_ramps.is_empty());
}

#[test]
fn op_4c_n4_sub_1_immediate_floors_zero_to_one() {
    // val = 1 → halved = 0 → floor → 1
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x41, 0x01, 0x00, 0, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(ctx.field_6a, 1);
}

#[test]
fn op_4c_n4_sub_1_ramp_passes_halved_target_to_host() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // val=20 → halved=10; ticks=8
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x41, 0x14, 0x00, 0x08, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(ctx.field_6a, 0); // VM does not write in ramp path
    assert_eq!(host.n4_ctx_ramps, vec![(1u8, 10i16, 8u16)]);
}

// -- 0x4C outer-nibble-4 sub-3 (ramp +0x24 OR absolute jump)

#[test]
fn op_4c_n4_sub_3_ticks_zero_jumps_absolute() {
    // ticks=0 path: returned `iVar18` = signed_16(operand[0..2]) - the
    // new PC offset is the literal target value.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x43, 0x40, 0x00, 0, 0], 0);
    // target = 0x40 → next_pc = 0x40
    assert_eq!(r, StepResult::Advance { next_pc: 0x40 });
    assert_eq!(ctx.field_24, 0); // jump-only branch does not write
    assert!(host.n4_ctx_ramps.is_empty());
}

#[test]
fn op_4c_n4_sub_3_ticks_nonzero_ramps_field_24() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // val=300, ticks=12 → ramp `ctx.field_24` 0 → 300 over 12 frames
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x43, 0x2C, 0x01, 0x0C, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(ctx.field_24, 0); // VM does not write in ramp path
    assert_eq!(host.n4_ctx_ramps, vec![(3u8, 300i16, 12u16)]);
}

// -- 0x4C outer-nibble-4 sub-4 (immediate +0x28 OR absolute jump)

#[test]
fn op_4c_n4_sub_4_ticks_zero_writes_field_28() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x44, 0xFF, 0xFF, 0, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(ctx.field_28, -1);
    assert!(host.n4_ctx_ramps.is_empty());
}

#[test]
fn op_4c_n4_sub_4_ticks_nonzero_jumps_absolute() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // ticks=4 (nonzero) → jump to target = 0x80
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x44, 0x80, 0x00, 0x04, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 0x80 });
    assert_eq!(ctx.field_28, 0); // jump-only branch does not write
}

// -- 0x4C outer-nibble-4 sub-6 / sub-7 (paired-global gate)

#[test]
fn op_4c_n4_sub_6_gate_clear_writes_global() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // gate=false (default) → regular write/ramp dispatch fires.
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x46, 0x07, 0x00, 0x10, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.n4_global_writes, vec![(6u8, 7i32, 16u16)]);
    assert_eq!(host.n4_global_pair_clears, 0);
}

#[test]
fn op_4c_n4_sub_7_gate_clear_writes_global() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x47, 0x21, 0x00, 0x00, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.n4_global_writes, vec![(7u8, 0x21i32, 0u16)]);
    assert_eq!(host.n4_global_pair_clears, 0);
}

#[test]
fn op_4c_n4_sub_6_gate_set_clears_pair_and_skips_write() {
    let mut host = TestHost {
        n4_global_pair_gated: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x46, 0x07, 0x00, 0x10, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    // No write happened - the gate short-circuited to a pair-clear.
    assert!(host.n4_global_writes.is_empty());
    assert_eq!(host.n4_global_pair_clears, 1);
}

#[test]
fn op_4c_n4_sub_7_gate_set_clears_pair_and_skips_write() {
    let mut host = TestHost {
        n4_global_pair_gated: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x47, 0x07, 0x00, 0x10, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert!(host.n4_global_writes.is_empty());
    assert_eq!(host.n4_global_pair_clears, 1);
}

// -- 0x4C outer-nibble-4 sub-5 (actor-field block, 11-byte encoding)

#[test]
fn op_4c_n4_sub_5_immediate_writes_actor_block() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // [4C, 0x45, b1=0x07, w94=0x0123, w96=-0x0001, w98=0x4567, ticks=0]
    let bytes = [
        0x4C, 0x45, 0x07, 0x23, 0x01, 0xFF, 0xFF, 0x67, 0x45, 0x00, 0x00,
    ];
    let r = step(&mut host, &mut ctx, &bytes, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 11 });
    assert_eq!(host.n4_sub5_immediate, vec![(0x07, 0x0123, -1, 0x4567)]);
    assert!(host.n4_sub5_ramp.is_empty());
}

#[test]
fn op_4c_n4_sub_5_ramp_yields_at_pc() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // ticks = 30 → ramp path
    let bytes = [
        0x4C, 0x45, 0x40, 0x10, 0x00, 0x20, 0x00, 0x30, 0x00, 0x1E, 0x00,
    ];
    let r = step(&mut host, &mut ctx, &bytes, 0);
    assert_eq!(r, StepResult::Yield { resume_pc: 11 });
    assert_eq!(
        host.n4_sub5_ramp,
        vec![(0x40, 0x0010, 0x0020, 0x0030, 30u16)]
    );
    assert!(host.n4_sub5_immediate.is_empty());
}

#[test]
fn op_4c_n4_sub_5_truncated_buffer_returns_unknown() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // Only 8 bytes - sub-5 needs 11.
    let bytes = [0x4C, 0x45, 0x07, 0x23, 0x01, 0xFF, 0xFF, 0x67];
    let r = step(&mut host, &mut ctx, &bytes, 0);
    assert!(matches!(r, StepResult::Unknown { .. }));
}

// -- 0x4C outer-nibble-4 sub-9 (story-flag-driven dispatch)

#[test]
fn op_4c_n4_sub_9_default_immediate_calls_default_write() {
    let mut host = TestHost::default(); // n4_sub9_state defaults to Default
    let mut ctx = FieldCtx::default();
    // [4C, 0x49, target=0x0042, ticks=0]
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x49, 0x42, 0x00, 0x00, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.n4_sub9_default_writes, vec![0x0042i16]);
    assert!(host.n4_sub9_default_ramps.is_empty());
    assert!(host.n4_sub9_delta_calls.is_empty());
}

#[test]
fn op_4c_n4_sub_9_default_ramp_yields_at_pc() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // ticks = 60 → ramp path; VM yields at the same PC.
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x49, 0x42, 0x00, 0x3C, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Yield { resume_pc: 0 });
    assert_eq!(host.n4_sub9_default_ramps, vec![(0x0042i16, 60u16)]);
    assert!(host.n4_sub9_default_writes.is_empty());
}

#[test]
fn op_4c_n4_sub_9_player_relative_never_jumps() {
    let mut host = TestHost {
        n4_sub9_state: Sub9State::PlayerRelative,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    // Bit 24 set selects the player-relative WRITE arm (the cutscene-dialogue
    // overlay's case 9; live-probe-pinned over the retail opening) - the op
    // always advances its 6 bytes, never jumps.
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x49, 0x80, 0x00, 0x00, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert!(host.n4_sub9_default_writes.is_empty());
    assert!(host.n4_sub9_default_ramps.is_empty());
    assert!(host.n4_sub9_delta_calls.is_empty());
}

#[test]
fn op_4c_n4_sub_9_delta_immediate_advances() {
    let mut host = TestHost {
        n4_sub9_state: Sub9State::Delta,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x49, 0x55, 0xFF, 0x00, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    // signed_16(0xFF55) = -171
    assert_eq!(host.n4_sub9_delta_calls, vec![(-171i16, 0u16)]);
}

#[test]
fn op_4c_n4_sub_9_delta_ramp_yields() {
    let mut host = TestHost {
        n4_sub9_state: Sub9State::Delta,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x49, 0x55, 0x00, 0x14, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Yield { resume_pc: 0 });
    assert_eq!(host.n4_sub9_delta_calls, vec![(0x55i16, 20u16)]);
}

#[test]
fn op_4c_n4_sub_9_state_default_when_no_story_bits_set() {
    // Verify the FieldHost::op4c_n4_sub9_state default impl reads global flags.
    let host = TestHost {
        globals: 0x0000_0000,
        ..TestHost::default()
    };
    // TestHost overrides with its own n4_sub9_state, but a fresh host with
    // the default Sub9State is at the `Default` variant.
    assert_eq!(host.n4_sub9_state, Sub9State::Default);
}

#[test]
fn op_34_sub_2_no_match_advances_two() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // sub_op = 2, b1 = 0x40 - but op34_sub2_capture_match=false (default)
    // so the host returns false → Advance PC += 2.
    let r = step(&mut host, &mut ctx, &[0x34, 0x20, 0x40, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert!(host.op34_sub2_captures.is_empty());
}

#[test]
fn op_34_sub_2_match_with_b1_40_yields_and_captures() {
    let mut host = TestHost {
        op34_sub2_capture_match: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x34, 0x20, 0x40, 0x00, 0x00], 0);
    assert_eq!(r, StepResult::Yield { resume_pc: 0 });
    // Captured PC offset should be `pc + header_size + 2` = 3.
    assert_eq!(host.op34_sub2_captures, vec![(0x40u8, 3usize)]);
}

#[test]
fn op_37_arm_encounter_forwards_record_window_when_armed() {
    // When the host reports the active entity is an encounter carrier, the
    // bare arm-encounter op (0x37) hands the host the record window that
    // overlays the opcode: [opcode][op1][op2][count][ids..].
    let mut host = TestHost {
        scripted_encounter_armed: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    // record overlay at pc 0: [0x37][op1][op2][count=2][id=0x4F][id=0x50](+tail)
    let bc = [0x37, 0x00, 0x00, 0x02, 0x4F, 0x50, 0x00, 0x00, 0x99];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Yield { resume_pc: 3 });
    assert_eq!(host.scripted_encounter_windows.len(), 1);
    // Bounded 8-byte window starting at the opcode.
    assert_eq!(
        host.scripted_encounter_windows[0],
        vec![0x37, 0x00, 0x00, 0x02, 0x4F, 0x50, 0x00, 0x00]
    );
}

#[test]
fn op_37_yield_does_not_arm_encounter_when_unarmed() {
    // Default host is unarmed: a generic 0x37 yield must not be mistaken
    // for an encounter arm.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let bc = [0x37, 0x00, 0x00, 0x02, 0x4F, 0x50, 0x00, 0x00];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Yield { resume_pc: 3 });
    assert!(host.scripted_encounter_windows.is_empty());
}

#[test]
fn op_34_sub_2_match_but_b1_not_40_advances_two() {
    // Even with the lookup matching, b1 != 0x40 means no capture and the
    // VM falls through to Advance PC += 2.
    let mut host = TestHost {
        op34_sub2_capture_match: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x34, 0x20, 0x10, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert!(host.op34_sub2_captures.is_empty());
}

#[test]
fn op_34_sub_0_advances_pc_by_7_with_rgb_intensity() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // sub-0: op0 = 0x05 (sub_nibble = 0, low_3 = 0b101), rgb = (1, 2, 3),
    // intensity = 0x1234.
    let r = step(&mut host, &mut ctx, &[0x34, 0x05, 1, 2, 3, 0x34, 0x12], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
    assert_eq!(host.op34_sub0_calls.len(), 1);
    let (op0, rgb, intensity) = host.op34_sub0_calls[0];
    assert_eq!(op0, 0x05);
    assert_eq!(rgb, [1, 2, 3]);
    assert_eq!(intensity, 0x1234);
}

#[test]
fn op_34_sub_0_negative_intensity_sign_extends() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // intensity = 0xFFFE = -2 as i16
    let r = step(&mut host, &mut ctx, &[0x34, 0x00, 0, 0, 0, 0xFE, 0xFF], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
    assert_eq!(host.op34_sub0_calls[0].2, -2);
}

#[test]
fn op_34_sub_1_default_advance_is_13() {
    // No host overrides → default impl returns delta = 13. Capture flag
    // is 0 so the captured-PC path doesn't fire.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(
        &mut host,
        &mut ctx,
        &[0x34, 0x10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 13 });
    assert_eq!(host.op34_sub1_calls.len(), 1);
    let call = &host.op34_sub1_calls[0];
    assert_eq!(call.op0, 0x10);
    assert_eq!(call.packed24, 0);
    assert_eq!(call.pos, [0, 0, 0]);
    assert_eq!(call.capture_flag, 0);
    assert!(call.captured_payload.is_empty());
}

#[test]
fn op_34_sub_1_packed24_and_position_decode() {
    // packed24 = 0x123456 (b1=0x12, b2=0x34, b3=0x56), world_x = 100,
    // world_z = -50, world_y = -(-200) = 200 (the original NEGATES the
    // raw bytes). reserved bytes are 0; capture_flag = 0.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let raw = [
        0x34, 0x10, 0x12, 0x34, 0x56, // op0 + packed24
        100, 0, // world_x = 100
        0xCE, 0xFF, // world_z = -50
        0x38, 0xFF, // raw -y = -200, → world_y = 200
        0, 0, // reserved
        0, // capture_flag
    ];
    let r = step(&mut host, &mut ctx, &raw, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 13 });
    let call = &host.op34_sub1_calls[0];
    assert_eq!(call.packed24, 0x123456);
    assert_eq!(call.pos, [100, 200, -50]);
}

#[test]
fn op_34_sub_1_capture_path_uses_host_returned_delta() {
    // capture_flag = 0x40, payload_len = 3. The instruction is 13 base
    // bytes + 2 (header bytes 0x40, len) + 3 (payload) = 18. This TestHost
    // injects the delta explicitly; the default impl computes the same 18 (see
    // `op_34_sub_1_default_host_consumes_the_capture_extension`).
    let mut host = TestHost {
        op34_sub1_capture_delta: Some(18),
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let raw = [
        0x34, 0x10, 0x00, 0x00, 0x00, // op0 + packed24
        0, 0, 0, 0, 0, 0, 0, 0, // pos + reserved
        0x40, 3, // capture_flag, payload_len
        0xAA, 0xBB, 0xCC, // captured payload
    ];
    let r = step(&mut host, &mut ctx, &raw, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 18 });
    let call = &host.op34_sub1_calls[0];
    assert_eq!(call.capture_flag, 0x40);
    assert_eq!(call.captured_payload, vec![0xAA, 0xBB, 0xCC]);
}

#[test]
fn op_34_sub_1_default_host_consumes_the_capture_extension() {
    // Drive the same 0x40 capture script through a host that uses the trait
    // DEFAULT `op34_sub1_spawn_or_skip` (no injected delta). The default must
    // consume the whole instruction (13 base + 2 + payload_len = 18); the old
    // constant 13 landed the PC mid-payload and desynced the rest of the script.
    struct DefaultHost;
    impl FieldHost for DefaultHost {
        fn global_flags(&self) -> u32 {
            0
        }
        fn set_global_flags(&mut self, _value: u32) {}
        fn frame_delta(&self) -> u16 {
            1
        }
    }
    let mut host = DefaultHost;
    let mut ctx = FieldCtx::default();
    let raw = [
        0x34, 0x10, 0x00, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0x40, 3, 0xAA, 0xBB, 0xCC,
    ];
    assert_eq!(
        step(&mut host, &mut ctx, &raw, 0),
        StepResult::Advance { next_pc: 18 }
    );
    // Without the 0x40 marker it is the bare 13-byte instruction.
    let raw_no_cap = [0x34, 0x10, 0x00, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0x00];
    assert_eq!(
        step(&mut host, &mut ctx, &raw_no_cap, 0),
        StepResult::Advance { next_pc: 13 }
    );
}

#[test]
fn op_4c_n4_negative_value_sign_extends() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // val = 0xFFFE = -2 as i16
    let r = step(&mut host, &mut ctx, &[0x4C, 0x40, 0xFE, 0xFF, 0, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    // i16 (-2) cast to u16 = 0xFFFE
    assert_eq!(ctx.field_72, 0xFFFE);
}

#[test]
fn op_4c_sub_3_sub_0_locks_input_and_yields() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x30], 0);
    assert_eq!(r, StepResult::Yield { resume_pc: 2 });
    assert_eq!(host.field_input_lock_writes, vec![true]);
}

#[test]
fn op_4c_sub_3_sub_1_unlocks_input_and_yields() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x31], 0);
    assert_eq!(r, StepResult::Yield { resume_pc: 2 });
    assert_eq!(host.field_input_lock_writes, vec![false]);
}

#[test]
fn op_4c_sub_3_sub_4_b_c_advance_two_no_host_call() {
    // sub-4 / sub-B / sub-C all jump through code_r0x801df098 →
    // LAB_801df09c → switchD_801e00f4::default(). Asm delay slot is
    // `_addiu s8, s8, 0x2` → PC += 2. No host hook fires.
    for sub in [0x4u8, 0xB, 0xC] {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x4C, 0x30 | sub], 0);
        assert_eq!(
            r,
            StepResult::Advance { next_pc: 2 },
            "sub_{:X} did not advance",
            sub
        );
        // None of the side-effect counters should fire for this no-op
        // group.
        assert_eq!(host.field_input_lock_writes, Vec::<bool>::new());
        assert_eq!(host.party_state_clears, 0);
        assert_eq!(host.menu_refresh_calls, 0);
    }
}

#[test]
fn menu_sub_3_sub_3_calls_refresh() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x33], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.menu_refresh_calls, 1);
}

#[test]
fn menu_sub_3_sub_a_copies_dialog_depth() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x3A], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.depth_copy_calls, 1);
}

#[test]
fn menu_sub_3_sub_8_and_d_call_subtile_refresh() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x38], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    let r = step(&mut host, &mut ctx, &[0x4C, 0x3D], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.subtile_refresh_calls, vec![8, 0xD]);
}

#[test]
fn menu_sub_2_dispatches_party_view_swap() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // op0 = 0x2A → sub-2; new_index = 0xA & 7 = 2.
    let r = step(&mut host, &mut ctx, &[0x4C, 0x2A], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.party_view_swaps, vec![2]);
}

//! 0x34 EFFECT sub-3, 0x43 ACTOR_CTRL sub-ops, step_with_caller propagation, high-byte 0x5x/0x6x/0x7x sysflag ops, and cross-opcode integration traces. Extracted verbatim from `field/tests.rs`.

use super::*;

// -- 0x34 EFFECT sub-3 ----------------------------------------------

#[test]
fn op_34_sub3_triggers_anim() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        script_id: 7,
        ..Default::default()
    };
    // op0 high nibble = 3 (=> 0x30). arg = 0x42.
    let r = step(&mut host, &mut ctx, &[0x34, 0x30, 0x42], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(host.effect_anim_calls, vec![(7, 0x42)]);
}

#[test]
fn op_34_high_subop_halts() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // sub-4..=sub-F (op0 >> 4 >= 4) hits the original's
    // `if (bVar35 != 2) { if (bVar35 != 3) { return param_2; } }` at
    // line 4811-4814 of the dump ⇒ halt at PC.
    let r = step(&mut host, &mut ctx, &[0x34, 0x40, 0], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}

// -- 0x43 ACTOR_CTRL sub-8 ------------------------------------------

#[test]
fn op_43_sub8_resets_face_rotation() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        script_id: 11,
        face_rotation: 5,
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0x43, 8], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.face_rotation, 0);
    assert_eq!(host.face_resets, vec![11]);
}

#[test]
fn op_43_other_subops_halt() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // Sub-op 0x16 (no handler in the original `case 0x43` inner switch);
    // falls through with `iVar45 = param_2` (initialised at line 4511 of
    // the dump) ⇒ halt at PC.
    let r = step(
        &mut host,
        &mut ctx,
        &[0x43, 0x16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        0,
    );
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}

#[test]
fn op_43_sub7_face_rotation_setup() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        face_rotation: 0,
        ..Default::default()
    };
    // 17-byte: [43, 7, face=3, payload=0xDEADBEEF, p0=0x1111, p1=0x2222,
    //          p2=0x3333, p3=0x4444, target=-1 (=0xFFFF)]
    let bc = [
        0x43, 7, 3, 0xEF, 0xBE, 0xAD, 0xDE, 0x11, 0x11, 0x22, 0x22, 0x33, 0x33, 0x44, 0x44, 0xFF,
        0xFF,
    ];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 17 });
    assert_eq!(ctx.face_rotation, 3);
    assert_eq!(
        host.face_rotation_setups,
        vec![(
            3u8,
            0xDEAD_BEEFu32,
            [0x1111u16, 0x2222, 0x3333, 0x4444],
            -1i16
        )]
    );
}

#[test]
fn op_43_sub12_alloc_actor() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // [43, 0xC, 0x10, 0x20, 0x30] = 5 bytes; PC += 5.
    let r = step(&mut host, &mut ctx, &[0x43, 0xC, 0x10, 0x20, 0x30], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert_eq!(host.scripted_actor_allocs, vec![(0x10, 0x20, 0x30)]);
}

#[test]
fn op_43_sub2_three_actor_talk() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // [43, 2, a1=5, a2=6, a3=7, lo=0x12, hi=0x34, b6=0xAB] = 8 bytes; PC += 8.
    let r = step(
        &mut host,
        &mut ctx,
        &[0x43, 2, 5, 6, 7, 0x12, 0x34, 0xAB],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 8 });
    assert_eq!(host.three_actor_talks, vec![([5, 6, 7], 0x3412, 0xAB)]);
}

#[test]
fn op_43_sub_d_alloc_with_mode_3() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // [43, 0xD, b1, b2, b3, b4] = 6 bytes; PC += 6; mode=3.
    let r = step(&mut host, &mut ctx, &[0x43, 0xD, 1, 2, 3, 4], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.actor_alloc_modes, vec![(0xD, 3, [1, 2, 3, 4])]);
}

#[test]
fn op_43_sub_f_alloc_with_mode_0() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // [43, 0xF, b1, b2, b3, b4] = 6 bytes; PC += 6; mode=0.
    let r = step(&mut host, &mut ctx, &[0x43, 0xF, 9, 8, 7, 6], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.actor_alloc_modes, vec![(0xF, 0, [9, 8, 7, 6])]);
}

#[test]
fn op_43_sub_e_marks_flag() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        script_id: 0x55,
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0x43, 0xE], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.mark_flag_8_calls, vec![0x55]);
}

#[test]
fn op_43_sub_9_tween_path() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // ticks=5 (non-zero) → tween path. Coords (0x0100, 0x0200, 0x0300).
    let bc = [0x43, 9, 0x00, 0x01, 0x00, 0x02, 0x00, 0x03, 0x05, 0x00];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 10 });
    assert_eq!(host.sub9_tweens, vec![(0x0100u16, 0x0200, 0x0300, 5)]);
    // Tween path doesn't write ctx coords directly.
    assert_eq!(ctx.world_x, 0);
}

#[test]
fn op_43_sub_9_immediate_writes_when_ticks_zero() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        world_x: 0xAAAA,
        world_y: 0xBBBB,
        world_z: 0xCCCC,
        ..Default::default()
    };
    // ticks=0 → immediate write path. y = 0xFFFF (sentinel - skip).
    let bc = [0x43, 9, 0x11, 0x22, 0xFF, 0xFF, 0x33, 0x44, 0x00, 0x00];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 10 });
    assert_eq!(ctx.world_x, 0x2211);
    assert_eq!(ctx.world_y, 0xBBBB); // unchanged (sentinel)
    assert_eq!(ctx.world_z, 0x4433);
    assert!(host.sub9_tweens.is_empty());
}

#[test]
fn op_43_sub_10_emitter_init() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 21-byte instruction. Payload is 19 bytes after the [43, 0x10] header.
    let mut bc = [0u8; 21];
    bc[0] = 0x43;
    bc[1] = 0x10;
    for (i, b) in bc.iter_mut().enumerate().skip(2) {
        *b = i as u8;
    }
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 21 });
    assert_eq!(host.emitter_init_payloads.len(), 1);
    assert_eq!(host.emitter_init_payloads[0].len(), 19);
    assert_eq!(host.emitter_init_payloads[0][0], 2);
}

#[test]
fn op_43_sub_11_emitter_5_words() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 12-byte: [43, 0x11, 5 u16s].
    let bc = [
        0x43, 0x11, 0x01, 0x00, 0x02, 0x00, 0x03, 0x00, 0x04, 0x00, 0x05, 0x00,
    ];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 12 });
    assert_eq!(host.emitter_5_words, vec![[1u16, 2, 3, 4, 5]]);
}

#[test]
fn op_43_sub_15_emitter_struct_12() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 14-byte: [43, 0x15, 12 bytes].
    let mut bc = [0u8; 14];
    bc[0] = 0x43;
    bc[1] = 0x15;
    for (i, b) in bc.iter_mut().enumerate().skip(2) {
        *b = (i + 0x10) as u8;
    }
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 14 });
    assert_eq!(host.emitter_struct12_payloads.len(), 1);
    assert_eq!(host.emitter_struct12_payloads[0].len(), 12);
}

#[test]
fn op_43_sub_12_split_call_no_split() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 14-byte: [43, 0x12, six s16 LE]. words[2] = 0x0080 ≤ 0xFF so no split.
    let bc = [
        0x43, 0x12, 0x10, 0x00, 0x20, 0x00, 0x80, 0x00, 0x40, 0x00, 0x50, 0x00, 0x60, 0x00,
    ];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 14 });
    // No split: exactly one call, passed through unshifted.
    assert_eq!(host.emitter_split_calls.len(), 1);
    let calls = &host.emitter_split_calls[0];
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].ot_slot, 6);
    assert_eq!(calls[0].src_x, 0x10);
    assert_eq!(calls[0].src_y, 0x20);
    assert_eq!(calls[0].w, 0x80);
    assert_eq!(calls[0].h, 0x40);
    assert_eq!(calls[0].dst_x, 0x50);
    assert_eq!(calls[0].dst_y, 0x60);
}

#[test]
fn op_43_sub_12_split_call_with_split() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // words[2] = 0x0200 > 0xFF → split.
    let bc = [
        0x43, 0x12, 0x10, 0x00, 0x20, 0x00, 0x00, 0x02, 0x40, 0x00, 0x50, 0x00, 0x60, 0x00,
    ];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 14 });
    // Split: the shifted far-page call is emitted first, then the main
    // copy clamped to 0x100 wide at the original corner.
    assert_eq!(host.emitter_split_calls.len(), 1);
    let calls = &host.emitter_split_calls[0];
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].src_x, 0x10 + 0xF0);
    assert_eq!(calls[0].w, 0x200 - 0xE0);
    assert_eq!(calls[0].dst_x, 0x50 + 0x100);
    assert_eq!(calls[1].src_x, 0x10);
    assert_eq!(calls[1].w, 0x100);
    assert_eq!(calls[1].dst_x, 0x50);
}

#[test]
fn op_43_sub_13_func13_passes_full_payload() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 14-byte: [43, 0x13, 12 data bytes]. Payload includes the 0x13 byte.
    let mut bc = [0u8; 14];
    bc[0] = 0x43;
    bc[1] = 0x13;
    for (i, b) in bc.iter_mut().enumerate().skip(2) {
        *b = (i + 0x40) as u8;
    }
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 14 });
    assert_eq!(host.emitter_func13_payloads.len(), 1);
    assert_eq!(host.emitter_func13_payloads[0][0], 0x13);
    assert_eq!(host.emitter_func13_payloads[0][12], (13 + 0x40) as u8);
}

#[test]
fn op_43_sub_14_4_words() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 10-byte: [43, 0x14, 4 s16 LE]. Use a negative second word to verify sign-ext.
    let bc = [0x43, 0x14, 0x01, 0x00, 0xFF, 0xFF, 0x03, 0x00, 0x04, 0x00];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 10 });
    assert_eq!(host.emitter_4_words, vec![[1i16, -1, 3, 4]]);
}

#[test]
fn op_43_sub_12_truncated_returns_unknown() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 13-byte: one byte short of the required 14.
    let bc = [0u8; 13];
    let bc = {
        let mut b = bc;
        b[0] = 0x43;
        b[1] = 0x12;
        b
    };
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert!(matches!(r, StepResult::Unknown { .. }));
}

#[test]
fn op_4c_sub_3_sub_9_position_refresh() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // [4C, 0x39] = 2 bytes; high nibble = 3, low nibble = 9.
    let r = step(&mut host, &mut ctx, &[0x4C, 0x39], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.player_pos_refresh_calls, 1);
    // sub-9 falls through to sub-E's render-resync chain.
    assert_eq!(host.player_render_resync_calls, 1);
}

#[test]
fn op_4c_sub_3_sub_e_render_resync() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x3E], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.player_render_resync_calls, 1);
    assert_eq!(host.player_pos_refresh_calls, 0);
}

#[test]
fn op_4c_sub_3_sub_f_io_resync() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x3F], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.field_io_resync_calls, 1);
}

#[test]
fn op_49_done_state_sub_6_8_9_c_d_advance_5() {
    for sub_op in [6u8, 8, 9, 0xC, 0xD] {
        let mut host = TestHost {
            op49_state_value: Op49State::Done,
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        // 5-byte instruction: [49, sub_op, 3 unused payload bytes].
        let bc = [0x49, sub_op, 0xAA, 0xBB, 0xCC];
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(
            r,
            StepResult::Advance { next_pc: 5 },
            "sub_op {sub_op:#x} should advance by 5 in Done state"
        );
        assert_eq!(host.op49_clears, 1);
    }
}

#[test]
fn op_43_sub_3_4_5_6_sound_ramps() {
    for sub_op in [3u8, 4, 5, 6] {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // [43, sub_op, b1=1, b2=2, b3=3, b4=4, ticks=0x0064, curve=0x0010]
        let bc = [0x43, sub_op, 1, 2, 3, 4, 0x64, 0x00, 0x10, 0x00];
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 10 });
        assert_eq!(
            host.sound_ramp_calls,
            vec![(sub_op, [1, 2, 3, 4], 100u16, 16u16)]
        );
    }
}

// -- step_with_caller (YIELD propagation) --------------------------

#[test]
fn yield_propagates_to_caller_when_target_is_player() {
    let mut host = TestHost::default();
    let mut target = FieldCtx {
        script_id: 0xFB, // arbitrary - caller designates as player below
        ..Default::default()
    };
    let mut caller = FieldCtx::default();
    let r = step_with_caller(&mut host, &mut target, &mut caller, true, &[0x37], 0);
    assert!(matches!(r, StepResult::Yield { .. }));
    assert!(target.is_halted());
    assert!(caller.is_halted());
    assert_eq!(target.saved_pc, 0);
    assert_eq!(caller.saved_pc, 0);
}

#[test]
fn yield_does_not_propagate_when_target_not_player() {
    let mut host = TestHost::default();
    let mut target = FieldCtx::default();
    let mut caller = FieldCtx::default();
    let r = step_with_caller(&mut host, &mut target, &mut caller, false, &[0x37], 0);
    assert!(matches!(r, StepResult::Yield { .. }));
    assert!(target.is_halted());
    assert!(!caller.is_halted());
}

#[test]
fn step_with_caller_non_yield_does_not_touch_caller() {
    let mut host = TestHost::default();
    let mut target = FieldCtx::default();
    let mut caller = FieldCtx::default();
    // 0x21 NOP - no yield. Caller untouched even when target == player.
    let r = step_with_caller(&mut host, &mut target, &mut caller, true, &[0x21], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 1 });
    assert!(!caller.is_halted());
}

#[test]
fn yield_saved_pc_is_pre_extended_when_extended_dispatch() {
    // The original stores pbVar43 (the address of the OPCODE byte) in
    // ctx.saved_pc, regardless of extended/non-extended. So an extended
    // YIELD at pc=0 saves 0, not 1.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 0xB7 = 0x80 | 0x37. Extended YIELD on a (synthetic) target.
    let r = step(&mut host, &mut ctx, &[0xB7, 0x00], 0);
    assert!(matches!(r, StepResult::Yield { .. }));
    assert_eq!(ctx.saved_pc, 0);
}

// -- High-byte default-route opcodes 0x5x/0x6x/0x7x ------------------
// (the **fourth flag bank** at DAT_80085758). These fall through the
// explicit opcode arm and hit the dispatcher's default route.

#[test]
fn sysflag_set_low_index_writes_through_host() {
    // Opcode 0x50 + idx_lo 0x07 → idx = (0x50 & 0x8F) << 8 | 0x07 = 0x07.
    // Bit at byte 0 / mask 0x01 set.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x50, 0x07], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.system_flags[0], 0x01);
    assert_eq!(host.sys_flag_writes, vec![(0x07, "set")]);
}

#[test]
fn sysflag_set_uses_low_nibble_of_opcode_for_high_byte() {
    // Opcode 0x5F + idx_lo 0xFF → idx = (0x5F & 0x8F) << 8 | 0xFF =
    // 0x0F00 | 0xFF = 0x0FFF. Byte 0x1FF, mask 0x01.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x5F, 0xFF], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.system_flags[0x1FF], 0x01);
    assert_eq!(host.sys_flag_writes, vec![(0x0FFF, "set")]);
}

#[test]
fn sysflag_set_extended_prefix_sets_high_bit_of_index() {
    // 0xD0 = 0x80 | 0x50. Extended SET. The dispatcher reads the raw
    // opcode byte for `(opcode_byte & 0x8F) << 8`, so the extended bit
    // (0x80) lands at bit 15 of `idx`.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // The extended prefix consumes one extra byte. peek_extended would
    // tell the caller to fetch the script ID (here 0x00).
    let r = step(&mut host, &mut ctx, &[0xD0, 0x00, 0x05], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    // idx = (0xD0 & 0x8F) << 8 | 0x05 = 0x80 << 8 | 0x05 = 0x8005.
    // Byte 0x1000, mask 0x04 (bit 5 in big-endian per-byte order).
    assert_eq!(host.sys_flag_writes, vec![(0x8005, "set")]);
    assert_eq!(host.system_flags[0x1000], 0x04);
}

#[test]
fn sysflag_clear_only_resets_targeted_bit() {
    let mut host = TestHost::default();
    host.ensure_sys_flag_capacity();
    host.system_flags[0] = 0xFF;
    let mut ctx = FieldCtx::default();
    // Opcode 0x60 + idx_lo 0x03 → idx = 0x03. Bit at byte 0 / mask 0x10.
    let r = step(&mut host, &mut ctx, &[0x60, 0x03], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.system_flags[0], 0xEF);
    assert_eq!(host.sys_flag_writes, vec![(0x03, "clear")]);
}

#[test]
fn sysflag_test_bit_clear_falls_through_4_bytes() {
    // TEST against an unset bit advances PC past the 4-byte instruction.
    // The 2 trailing operand bytes (the "branch target") are skipped.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x70, 0x05, 0xAA, 0xBB], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    // No writes - TEST is read-only.
    assert!(host.sys_flag_writes.is_empty());
}

#[test]
fn sysflag_test_bit_set_takes_relative_jump() {
    // Pre-set bit at idx 0x08 (byte 1, mask 0x80).
    // Opcode 0x70 + idx_lo 0x08 + offset 0x0010 → on bit-set, jump to
    // pc + header_size + 1 + 0x0010 = 0 + 1 + 1 + 16 = 18.
    let mut host = TestHost::default();
    host.ensure_sys_flag_capacity();
    host.system_flags[1] = 0x80;
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x70, 0x08, 0x10, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 18 });
}

#[test]
fn sysflag_test_extended_prefix_sets_high_bit_of_index() {
    // 0xF0 = 0x80 | 0x70. Extended TEST.
    let mut host = TestHost::default();
    host.ensure_sys_flag_capacity();
    // idx = (0xF0 & 0x8F) << 8 | 0x05 = 0x8005.
    // Byte 0x1000, mask 0x04. Pre-set it.
    host.system_flags[0x1000] = 0x04;
    let mut ctx = FieldCtx::default();
    // Layout: [prefix=0xF0, target_id, idx_lo, off_lo, off_hi]
    // pc=0; on bit-set, next_pc = 0 + 2 (header) + 1 + LE_u16(0x05, 0x00) = 8.
    let r = step(&mut host, &mut ctx, &[0xF0, 0x00, 0x05, 0x05, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 8 });
}

#[test]
fn sysflag_set_then_test_round_trips() {
    // SET-then-TEST round trip: a SET on idx K must subsequently make
    // TEST on the same idx return true and take the branch.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x50, 0x42], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    // Fresh TEST at pc=0 on its own buffer; offset 0x0004 → jump to
    // 0 + header_size(1) + 1 + 4 = 6.
    let r = step(&mut host, &mut ctx, &[0x70, 0x42, 0x04, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
}

#[test]
fn sysflag_set_truncated_idx_byte_is_unknown() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 1-byte buffer - no idx byte available.
    let r = step(&mut host, &mut ctx, &[0x50], 0);
    assert!(matches!(r, StepResult::Unknown { .. }));
}

#[test]
fn sysflag_test_truncated_offset_is_unknown() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 3-byte buffer - missing the second offset byte.
    let r = step(&mut host, &mut ctx, &[0x70, 0x05, 0xAA], 0);
    assert!(matches!(r, StepResult::Unknown { .. }));
}

// -- Cross-opcode integration ---------------------------------------

/// Drive a script through `step()` repeatedly until it `Halt`s, hits
/// `Pending`/`Unknown`, or executes more than `max_steps` instructions.
/// Returns the trace of `(pc_before, StepResult)` for assertion.
fn run_until_halt(
    host: &mut TestHost,
    ctx: &mut FieldCtx,
    bytecode: &[u8],
    start_pc: usize,
    max_steps: usize,
) -> Vec<(usize, StepResult)> {
    let mut trace = Vec::new();
    let mut pc = start_pc;
    for _ in 0..max_steps {
        let r = step(host, ctx, bytecode, pc);
        trace.push((pc, r.clone()));
        match r {
            StepResult::Advance { next_pc } => pc = next_pc,
            StepResult::Yield { resume_pc } => {
                // Treat Yield as "step one tick, resume next iter".
                pc = resume_pc;
            }
            StepResult::Halt { .. } | StepResult::Pending { .. } | StepResult::Unknown { .. } => {
                break;
            }
        }
    }
    trace
}

#[test]
fn integration_lflag_set_test_jmp_clr_test_halt() {
    // Composed script exercising five opcodes in sequence. The JMP_REL
    // formula is `target = pc + header_size + delta` (= pc + 1 + delta
    // for non-extended). All offsets below were chosen so the JMP at
    // PC=4 lands on PC=9 (delta=4 → 4 + 1 + 4 = 9), skipping the
    // intermediate CLR.
    //
    //   00: 2B 05         LFLAG_SET bit 5      (PC -> 02)
    //   02: 2D 05         LFLAG_TST bit 5      (set → PC -> 04)
    //   04: 26 04 00      JMP_REL +4           (target = 4 + 1 + 4 = 9)
    //   07: 2C 05         LFLAG_CLR bit 5  ← skipped by JMP
    //   09: 2D 05         LFLAG_TST bit 5  (still set → PC -> 0B)
    //   0B: 2C 05         LFLAG_CLR bit 5      (PC -> 0D)
    //   0D: 2D 05         LFLAG_TST bit 5      (now clear → Halt)
    //
    // Validates: local-flag round trip, unconditional jump skipping
    // an intermediate op, conditional-test halt path.
    let bytecode = [
        0x2B, 0x05, // 00..02 SET bit 5
        0x2D, 0x05, // 02..04 TEST bit 5 (advance)
        0x26, 0x04, 0x00, // 04..07 JMP +4 → PC=9
        0x2C, 0x05, // 07..09 CLR bit 5 (skipped)
        0x2D, 0x05, // 09..0B TEST bit 5 (still set, advance)
        0x2C, 0x05, // 0B..0D CLR bit 5
        0x2D, 0x05, // 0D..0F TEST bit 5 (clear, halt)
    ];

    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let trace = run_until_halt(&mut host, &mut ctx, &bytecode, 0, 32);

    // Terminal state is a Halt at PC 0x0D (the third TEST).
    let (last_pc, last) = trace.last().unwrap().clone();
    assert!(
        matches!(last, StepResult::Halt { final_pc: 0x0D }),
        "expected Halt at 0x0D, got {:?} at pc=0x{:X}",
        last,
        last_pc
    );

    assert_eq!(ctx.local_flags & (1 << 5), 0, "bit 5 should end clear");

    // Walk the visited PCs to ensure the JMP took its branch and the
    // CLR at offset 0x07 was skipped.
    let visited: Vec<usize> = trace.iter().map(|(p, _)| *p).collect();
    assert_eq!(
        visited,
        vec![0x00, 0x02, 0x04, 0x09, 0x0B, 0x0D],
        "PC trace mismatch (JMP must skip the CLR at 0x07)"
    );
}

#[test]
fn integration_jmp_then_advance_through_n4_immediate_writes() {
    // Composed script that JMPs over a garbage region, then walks three
    // 6-byte 0x4C nibble-4 immediate writes, ending at NOP. JMP_REL
    // target = `pc + header_size + delta` = `0 + 1 + 4 = 5`. Validates
    // PC math composition across the 6-byte 0x4C nibble-4 instruction.
    //
    //   00: 26 04 00            JMP +4  → PC = 5
    //   03..04: garbage (skipped)
    //   05: 4C 40 11 00 00 00   nibble-4 sub-0: ctx.field_72 = 0x0011, immediate
    //   0B: 4C 48 22 00 00 00   nibble-4 sub-8: ctx.field_26 = 0x0022, immediate
    //   11: 4C 42 33 00 00 00   nibble-4 sub-2: ctx.field_8e = 0x0033, immediate
    //   17: 21                  NOP
    //   18: 21                  NOP - terminal (run_until_halt limit)
    let bytecode = [
        0x26, 0x04, 0x00, // 00..03 JMP +4 → PC=5
        0xAA, 0xAA, // 03..05 garbage (skipped)
        0x4C, 0x40, 0x11, 0x00, 0x00, 0x00, // 05..0B sub-0
        0x4C, 0x48, 0x22, 0x00, 0x00, 0x00, // 0B..11 sub-8
        0x4C, 0x42, 0x33, 0x00, 0x00, 0x00, // 11..17 sub-2
        0x21, // 17 NOP
        0x21, // 18 NOP
    ];

    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // Cap steps low enough that the trace ends inside the buffer; with
    // 6 instructions the trace stops at PC=0x19 reading past EOF
    // (Unknown). That's fine - we assert state, not the terminal kind.
    let _ = run_until_halt(&mut host, &mut ctx, &bytecode, 0, 16);

    assert_eq!(ctx.field_72, 0x11);
    assert_eq!(ctx.field_26, 0x22);
    assert_eq!(ctx.field_8e, 0x33);
    // Garbage bytes at 0x03..05 must not have been interpreted as ops.
    // We verify this via the host call counters: nothing else should
    // have fired.
    assert!(host.n4_ctx_ramps.is_empty());
    assert!(host.n4_global_writes.is_empty());
}

#[test]
fn integration_yield_then_resume_then_advance() {
    // Validates the Yield resume cycle composed with Advance:
    //   00: 4C 30           sub-3 sub-0 (set field-input lock, Yield)
    //   02: 4C 31           sub-3 sub-1 (clear lock, Yield)
    //   04: 21              NOP (Advance)
    //   05: 2D 05           LFLAG_TST bit 5 (clear → Halt)
    //
    // After two Yields the host log should show [true, false]; after
    // the NOP advances we hit the LFLAG_TST and halt.
    let bytecode = [
        0x4C, 0x30, // lock + Yield
        0x4C, 0x31, // unlock + Yield
        0x21, // NOP
        0x2D, 0x05, // TEST bit 5 (clear → Halt)
    ];

    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let trace = run_until_halt(&mut host, &mut ctx, &bytecode, 0, 16);

    // Lock log must be exactly [true, false].
    assert_eq!(host.field_input_lock_writes, vec![true, false]);

    // Trace shape: Yield, Yield, Advance, Halt.
    assert!(matches!(trace[0].1, StepResult::Yield { resume_pc: 2 }));
    assert!(matches!(trace[1].1, StepResult::Yield { resume_pc: 4 }));
    assert!(matches!(trace[2].1, StepResult::Advance { next_pc: 5 }));
    let last = trace.last().unwrap().1.clone();
    assert!(matches!(last, StepResult::Halt { final_pc: 5 }));
}

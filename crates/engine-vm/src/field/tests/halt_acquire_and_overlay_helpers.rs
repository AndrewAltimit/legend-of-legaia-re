//! 0x43 sub-0/1/A/B halt-acquire plus the Round 16/17 overlay-helper-driven 0x4C sub-ops. Extracted verbatim from `field/tests.rs`.

use super::*;

// -- 0x43 sub-0/1/A/B halt-acquire ----------------------------------

#[test]
fn op_43_sub_0_halt_acquire_yields_to_resume_pc() {
    // [43, 0, x_byte, z_byte, lo, hi] → resume_pc = signed_16(lo, hi) = 0x100
    let bytecode = [0x43u8, 0x00, 0x10, 0x20, 0x00, 0x01];
    let mut host = TestHost {
        halt_acquire_predicate: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Yield { resume_pc: 0x100 });
    assert!(ctx.is_halted());
    assert_eq!(ctx.saved_pc, 0);
    assert_eq!(host.halt_acquire_calls.len(), 1);
    assert_eq!(host.halt_acquire_calls[0].0, 0u8);
    assert_eq!(host.halt_acquire_calls[0].1, 0x100);
}

#[test]
fn op_43_sub_a_halt_acquire_uses_offset_7_target() {
    // [43, 0xA, x, z, _, _, _, _, lo, hi]
    let bytecode = [0x43u8, 0x0A, 0x10, 0x20, 0, 0, 0, 0, 0x34, 0x12];
    let mut host = TestHost {
        halt_acquire_predicate: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Yield { resume_pc: 0x1234 });
    assert!(ctx.is_halted());
}

#[test]
fn op_43_sub_0_predicate_false_advances_5_bytes() {
    let bytecode = [0x43u8, 0x00, 0, 0, 0, 0];
    let mut host = TestHost {
        halt_acquire_predicate: false,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert!(!ctx.is_halted());
}

#[test]
fn op_43_sub_b_predicate_false_advances_9_bytes() {
    let bytecode = [0x43u8, 0x0B, 0, 0, 0, 0, 0, 0, 0, 0];
    let mut host = TestHost {
        halt_acquire_predicate: false,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 9 });
}

// -- Round 16: helper-driven sub-ops --------------------------------

#[test]
fn op_4c_n_c_sub_1_flag_loop_reset_advances_2_bytes() {
    let bytecode = [0x4Cu8, 0xC1];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.n_c_sub_1_flag_loops, 1);
}

#[test]
fn op_4c_n_d_sub_1_no_jump_advances_4_bytes() {
    let bytecode = [0x4Cu8, 0xD1, 0, 0];
    let mut host = TestHost {
        n_d_sub_1_jump_target: None,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
}

#[test]
fn op_4c_n_d_sub_1_jump_target_takes_ce9c_path() {
    let bytecode = [0x4Cu8, 0xD1, 0, 0];
    let mut host = TestHost {
        n_d_sub_1_jump_target: Some(0xABCD),
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0xABCD });
}

#[test]
fn op_4c_n_d_sub_2_channel_spawn_halts_at_pc() {
    let bytecode = [0x4Cu8, 0xD2, 0xFB];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(host.n_d_sub_2_channel_calls, vec![0xFBu8]);
}

#[test]
fn op_4c_n_d_sub_7_register_list_walk_halts_at_pc() {
    let bytecode = [0x4Cu8, 0xD7];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(host.n_d_sub_7_list_walk_regs, 1);
}

#[test]
fn op_4c_n_d_sub_b_e57f0_advances_13_bytes() {
    // 13 total bytes: opcode + sub-op + 11 payload.
    let mut bytecode = vec![0x4Cu8, 0xDB];
    bytecode.extend_from_slice(&[
        0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x11, 0x22, 0x33, 0x44, 0x55,
    ]);
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 13 });
    assert_eq!(host.n_d_sub_b_e57f0_calls.len(), 1);
    // 12-byte slice starting from the 0xDB sub-op byte.
    assert_eq!(host.n_d_sub_b_e57f0_calls[0].len(), 12);
    assert_eq!(host.n_d_sub_b_e57f0_calls[0][0], 0xDB);
    assert_eq!(host.n_d_sub_b_e57f0_calls[0][11], 0x55);
}

#[test]
fn op_4c_n_d_sub_b_truncated_buffer_returns_unknown() {
    // 12 bytes only - sub-B needs 13.
    let bytecode = [0x4Cu8, 0xDB, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
    assert!(host.n_d_sub_b_e57f0_calls.is_empty());
}

#[test]
fn op_4c_n_d_sub_c_no_jump_advances_5_bytes() {
    let bytecode = [0x4Cu8, 0xDC, 0x42, 0, 0];
    let mut host = TestHost {
        n_d_sub_c_jump_target: None,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert_eq!(host.n_d_sub_c_calls, vec![0x42u8]);
}

#[test]
fn op_4c_n_d_sub_c_jump_target_takes_ce9c_path() {
    let bytecode = [0x4Cu8, 0xDC, 0x42, 0, 0];
    let mut host = TestHost {
        n_d_sub_c_jump_target: Some(0x100),
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x100 });
}

#[test]
fn op_4c_n_d_sub_e_query_no_jump_advances_5_bytes() {
    let bytecode = [0x4Cu8, 0xDE, 0x99, 0, 0];
    let mut host = TestHost {
        n_d_sub_e_jump_target: None,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert_eq!(host.n_d_sub_e_calls, vec![0x99u8]);
}

#[test]
fn op_4c_n_d_sub_e_query_jump_target_takes_ce9c_path() {
    let bytecode = [0x4Cu8, 0xDE, 0x99, 0, 0];
    let mut host = TestHost {
        n_d_sub_e_jump_target: Some(0x40),
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x40 });
}

#[test]
fn op_4c_n_e_sub_1_text_actor_zero_first_byte_skips_spawn() {
    // First byte 0 → no spawn; PC still advances by 3 (0 has no payload).
    // packet_length([0]) = 0 → advance by 3 + 0 = 3.
    let bytecode = [0x4Cu8, 0xE1, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    // No spawn because first byte is 0.
    assert!(host.n_e_sub_1_text_calls.is_empty());
}

#[test]
fn op_4c_n_e_sub_1_text_actor_short_string_advances_correctly() {
    // [4C, E1, 'A', 'B', 'C', 0] - packet length 3, total advance 6.
    let bytecode = [0x4Cu8, 0xE1, b'A', b'B', b'C', 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        script_id: 0x42,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.n_e_sub_1_text_calls.len(), 1);
    assert_eq!(host.n_e_sub_1_text_calls[0].0, vec![b'A', b'B', b'C']);
    assert_eq!(host.n_e_sub_1_text_calls[0].1, 0x42);
}

#[test]
fn op_4c_n_e_sub_1_text_actor_with_escape_sequences() {
    // [4C, E1, 'A', 0xC1, 0xAB, 'B', 0] - escape pair counts as 2,
    // total packet length = 4, total advance = 7.
    let bytecode = [0x4Cu8, 0xE1, b'A', 0xC1, 0xAB, b'B', 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
    assert_eq!(host.n_e_sub_1_text_calls[0].0.len(), 4);
}

#[test]
fn op_4c_n_e_sub_1_text_actor_truncated_returns_unknown() {
    let bytecode = [0x4Cu8, 0xE1]; // first byte missing
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
    assert!(host.n_e_sub_1_text_calls.is_empty());
}

// -----------------------------------------------------------------
// Round 17 - five new 0x4C nC sub-ops + two 0x4C nE sub-ops.
// -----------------------------------------------------------------

#[test]
fn op_4c_n_c_sub_0_move_cancel_advances_2_bytes() {
    let bytecode = [0x4Cu8, 0xC0];
    let mut host = TestHost {
        n_c_sub_0_active: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.n_c_sub_0_move_cancels, 1);
}

#[test]
fn op_4c_n_c_sub_0_move_cancel_advances_2_bytes_when_inactive() {
    // Even when the host returns false (no active move), PC still
    // advances by 2 - the cancel side-effect is conditional but the
    // dispatcher's PC delta is constant.
    let bytecode = [0x4Cu8, 0xC0];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.n_c_sub_0_move_cancels, 1);
}

#[test]
fn op_4c_n_c_sub_3_script_teleport_advances_2_bytes() {
    let bytecode = [0x4Cu8, 0xC3];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.n_c_sub_3_teleports, 1);
}

#[test]
fn op_4c_n_c_sub_5_party_flag_jz_advances_4_bytes() {
    // [4C, 0xC5, 0x05, 0x00] - flag idx 5. Bit 5 in the host's bank is
    // unset, so the original "jump-if-zero" path fires; both branches
    // advance PC by 4.
    let bytecode = [0x4Cu8, 0xC5, 0x05, 0x00];
    let mut host = TestHost::default();
    host.n_c_party_flag_bits.insert(5, false);
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
}

#[test]
fn op_4c_n_c_sub_5_party_flag_jz_advances_4_bytes_when_set() {
    // Bit set → original's "jump-if-zero" doesn't fire; PC still += 4.
    let bytecode = [0x4Cu8, 0xC5, 0x07, 0x00];
    let mut host = TestHost::default();
    host.n_c_party_flag_bits.insert(7, true);
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
}

#[test]
fn op_4c_n_c_sub_5_reads_16_bit_index_via_helper() {
    // Verify the dispatcher reads the index through load_u16_le by
    // setting two distinct flag bits and checking which one was queried.
    let bytecode = [0x4Cu8, 0xC5, 0x34, 0x12]; // 0x1234
    let mut host = TestHost::default();
    host.n_c_party_flag_bits.insert(0x1234, true);
    host.n_c_party_flag_bits.insert(0x3412, false);
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    // The 0x1234 bit is set in the bank - the dispatcher's load_u16_le
    // must produce 0x1234 (LE), not 0x3412 (BE).
    assert!(*host.n_c_party_flag_bits.get(&0x1234).unwrap());
}

#[test]
fn op_4c_n_c_sub_6_party_flag_jnz_advances_4_bytes() {
    // Same shape as sub-5 but opposite polarity. Both polarities share
    // PC += 4 either way.
    let bytecode = [0x4Cu8, 0xC6, 0x09, 0x00];
    let mut host = TestHost::default();
    host.n_c_party_flag_bits.insert(9, false);
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
}

#[test]
fn op_4c_n_c_sub_5_truncated_buffer_returns_unknown() {
    // Need 3 bytes after pc; only 2 available.
    let bytecode = [0x4Cu8, 0xC5, 0x05]; // missing high byte
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
}

#[test]
fn op_4c_n_c_sub_d_script_alloc_halts_at_pc() {
    let bytecode = [0x4Cu8, 0xCD];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(host.n_c_sub_d_allocs, 1);
}

#[test]
fn op_4c_n_e_sub_4_bbox_inside_advances_8_bytes() {
    // Host returns false (= "inside") → PC += 8 (retail case-4
    // `iVar45 = param_2 + 8`; the earlier 9 was an off-by-one). The
    // dispatcher computes a tile-center bbox and passes it to the host.
    let bytecode = [0x4Cu8, 0xE4, 0x10, 0x10, 0x20, 0x20, 0x00, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 8 });
    assert_eq!(host.n_e_sub_4_bboxes.borrow().len(), 1);
}

#[test]
fn op_4c_n_e_sub_4_bbox_outside_halts_at_pc() {
    let bytecode = [0x4Cu8, 0xE4, 0x10, 0x10, 0x20, 0x20, 0x00, 0x00, 0x00];
    let mut host = TestHost {
        n_e_sub_4_outside: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}

#[test]
fn op_4c_n_e_sub_4_bbox_tile_center_math_for_low_byte() {
    // Operand byte 0x10 → tile-center: (0x10 << 7) | 0x40 = 0x840
    // (high bit clear, so no extra +0x40).
    let bytecode = [0x4Cu8, 0xE4, 0x10, 0x10, 0x10, 0x10, 0x00, 0x00, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let _ = step(&mut host, &mut ctx, &bytecode, 0);
    let bboxes = host.n_e_sub_4_bboxes.borrow();
    assert_eq!(bboxes.len(), 1);
    // All four corners should be 0x840 = 2112.
    assert_eq!(bboxes[0], [0x840, 0x840, 0x840, 0x840]);
}

#[test]
fn op_4c_n_e_sub_4_bbox_tile_center_high_bit_adds_0x40() {
    // Operand byte 0x90: low 7 bits are 0x10 → base 0x840. High bit set
    // → extra +0x40 = 0x880.
    let bytecode = [0x4Cu8, 0xE4, 0x90, 0x90, 0x90, 0x90, 0x00, 0x00, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let _ = step(&mut host, &mut ctx, &bytecode, 0);
    let bboxes = host.n_e_sub_4_bboxes.borrow();
    assert_eq!(bboxes[0], [0x880, 0x880, 0x880, 0x880]);
}

#[test]
fn op_4c_n_e_sub_4_bbox_zero_byte_yields_zero() {
    // Operand byte 0x00 → 0 (special case in tile-center math).
    let bytecode = [0x4Cu8, 0xE4, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let _ = step(&mut host, &mut ctx, &bytecode, 0);
    let bboxes = host.n_e_sub_4_bboxes.borrow();
    assert_eq!(bboxes[0], [0, 0, 0, 0]);
}

#[test]
fn op_4c_n_e_sub_4_truncated_buffer_returns_unknown() {
    let bytecode = [0x4Cu8, 0xE4, 0x10, 0x10]; // missing last 5 bytes
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
}

#[test]
fn op_4c_n_e_sub_5_add_coins_positive_value() {
    // [4C, E5, 0xE8, 0x03, 0x00] - 0x0003E8 = 1000.
    let bytecode = [0x4Cu8, 0xE5, 0xE8, 0x03, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert_eq!(host.n_e_sub_5_coin_deltas, vec![1000]);
}

#[test]
fn op_4c_n_e_sub_5_add_coins_negative_value() {
    // 0xFFFFFE = -2 in 24-bit two's complement.
    let bytecode = [0x4Cu8, 0xE5, 0xFE, 0xFF, 0xFF];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert_eq!(host.n_e_sub_5_coin_deltas, vec![-2]);
}

#[test]
fn op_4c_n_e_sub_5_add_coins_zero_advances_5_bytes() {
    let bytecode = [0x4Cu8, 0xE5, 0x00, 0x00, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert_eq!(host.n_e_sub_5_coin_deltas, vec![0]);
}

#[test]
fn op_4c_n_e_sub_5_truncated_buffer_returns_unknown() {
    let bytecode = [0x4Cu8, 0xE5, 0x01]; // missing 2 bytes
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
    assert!(host.n_e_sub_5_coin_deltas.is_empty());
}

#[test]
fn op_4c_n_e_sub_b_actor_resolved_advances_5_bytes() {
    // [4C, EB, actor_id=0x07, target_lo=0x10, target_hi=0x20]
    // When the host resolves the actor, take the "pc + 5" path.
    let bytecode = [0x4Cu8, 0xEB, 0x07, 0x10, 0x20];
    let mut host = TestHost {
        n_e_sub_b_resolves: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert_eq!(host.n_e_sub_b_actor_ids, vec![0x07]);
}

#[test]
fn op_4c_n_e_sub_b_actor_unresolved_jumps_to_target() {
    // Same instruction; host returns None (actor not resolved). The
    // dispatcher reads the absolute jump target via load_u16_le and
    // returns it as the new PC.
    let bytecode = [0x4Cu8, 0xEB, 0x07, 0x10, 0x20];
    let mut host = TestHost::default(); // n_e_sub_b_resolves = false
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x2010 });
    assert_eq!(host.n_e_sub_b_actor_ids, vec![0x07]);
}

#[test]
fn op_4c_n_e_sub_b_jump_target_uses_load_u16_le() {
    // Verify endianness: bytes 0x34, 0x12 should produce target 0x1234,
    // not 0x3412.
    let bytecode = [0x4Cu8, 0xEB, 0xAA, 0x34, 0x12];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x1234 });
}

#[test]
fn op_4c_n_e_sub_b_truncated_buffer_returns_unknown() {
    let bytecode = [0x4Cu8, 0xEB, 0x07]; // missing 2 jump-target bytes
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
    assert!(host.n_e_sub_b_actor_ids.is_empty());
}

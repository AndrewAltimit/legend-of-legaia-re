//! 0x4C outer-nibbles 5..F sub-op coverage. Extracted verbatim from `field/tests.rs`.

use super::*;

// -- 0x4C outer-nibbles 5..F ----------------------------------------

#[test]
fn op_4c_n5_sub0_low_clears_flag_bit_and_advances() {
    // [4C, 0x50, 0x40, 0x00] → value = 0x40, < 0xF0 → low half.
    let bytecode = [0x4Cu8, 0x50, 0x40, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        flags: 0x0100_0000,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    assert_eq!(ctx.flags & 0x0100_0000, 0);
    assert_eq!(host.n5_sub0_calls, vec![(0x40, false)]);
}

#[test]
fn op_4c_n5_sub0_high_sets_flag_bit() {
    // [4C, 0x50, 0xF0, 0x00] → value = 0xF0, >= 0xF0 → high half.
    let bytecode = [0x4Cu8, 0x50, 0xF0, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    assert!(ctx.flags & 0x0100_0000 != 0);
    assert_eq!(host.n5_sub0_calls, vec![(0xF0, true)]);
}

#[test]
fn op_4c_n6_sub_60_emitter6_passes_six_signed_words() {
    // [4C, 0x60, 12 bytes of 6 s16 words]
    let mut bytecode = vec![0x4C, 0x60];
    for w in &[1i16, -2, 3, -4, 5, -6] {
        bytecode.extend_from_slice(&w.to_le_bytes());
    }
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 14 });
    assert_eq!(host.n6_sub0_calls, vec![[1, -2, 3, -4, 5, -6]]);
}

#[test]
fn op_4c_n6_unrecognized_halts_at_pc() {
    // n6 op0 in {0x62..=0x6F}: original dispatcher returns `param_2`
    // unchanged (halt at PC). Only 0x60 (6-word emitter) and 0x61
    // (halt-acquire emitter) are recognized.
    let bytecode = [0x4Cu8, 0x62, 0, 0, 0, 0];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(r, StepResult::Halt { .. }));
}

#[test]
fn op_4c_n7_sub_0_yields_at_next_pc() {
    // [4C, 0x70, col0=1, row0=2, col1=3, row1=4]. Sub-0 has no mask
    // byte, so it is a 6-byte op (yield at pc+6). Columns
    // [col0, col1+1) = [1, 4); rows [row0+1, row1+2) = [3, 6). The
    // trailing 0xAA is the next op's first byte, not consumed here.
    let bytecode = [0x4Cu8, 0x70, 1, 2, 3, 4, 0xAA];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Yield { resume_pc: 6 });
    assert_eq!(host.n7_tile_calls, vec![(0u8, (1u8, 4u8), (3u8, 6u8), 0u8)]);
}

#[test]
fn op_4c_n7_sub_2_advances() {
    // sub-2 → mask-clear, advance not yield.
    let bytecode = [0x4Cu8, 0x72, 0, 0, 0, 0, 0x0F];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
    assert_eq!(host.n7_tile_calls.len(), 1);
    assert_eq!(host.n7_tile_calls[0].0, 2);
}

#[test]
fn op_4c_n8_sub2_party_mirror_advances_three_bytes() {
    let bytecode = [0x4Cu8, 0x82, 0x03];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(host.n8_party_mirrors, vec![3u8]);
}

#[test]
fn op_4c_n8_sub_c_branch_taken_when_field_68_zero() {
    // [4C, 0x8C, 0x10, 0x00] - field_68 = 0 → branch to 0x0010.
    let bytecode = [0x4Cu8, 0x8C, 0x10, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        field_68: 0,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x10 });
}

#[test]
fn op_4c_n8_sub_c_advances_past_when_field_68_nonzero() {
    let bytecode = [0x4Cu8, 0x8C, 0x10, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        field_68: 1,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
}

#[test]
fn op_4c_n5_sub_1_decodes_coords_and_advances_six() {
    // [4C, 0x51, b1, b2, b3, b4]. Tile-coord formula: world = (b & 0x7F)*0x80 + 0x40
    // (+0x40 if high bit). b1=0x82 → (2*0x80) + 0x40 + 0x40 = 0x180.
    // b2=0x03 → (3*0x80) + 0x40 = 0x1C0.
    let bytecode = [0x4Cu8, 0x51, 0x82, 0x03, 0x07, 0x2A];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.n_5_sub_1_npc_runs.len(), 1);
    let (wx, wz, depth, move_id, is_player) = host.n_5_sub_1_npc_runs[0];
    assert_eq!(wx, 0x180);
    assert_eq!(wz, 0x1C0);
    assert_eq!(depth, 0x07);
    assert_eq!(move_id, 0x2A);
    assert!(!is_player, "ctx.flags & 0x01000000 == 0 → NPC path");
}

#[test]
fn op_4c_n5_sub_1_is_player_when_flag_bit_set() {
    let bytecode = [0x4Cu8, 0x51, 0x00, 0x00, 0x00, 0x63];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        flags: 0x0100_0000,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert!(host.n_5_sub_1_npc_runs[0].4);
    // 99 (0x63) is the player-path cancel sentinel.
    assert_eq!(host.n_5_sub_1_npc_runs[0].3, 99);
}

#[test]
fn op_4c_n5_sub_2_advances_when_menu_activated() {
    let bytecode = [0x4Cu8, 0x52, 0x05];
    let mut host = TestHost {
        n_5_sub_2_menu_state: std::collections::HashMap::from([(0x05u8, true)]),
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(host.n_5_sub_2_polls.borrow().as_slice(), &[0x05]);
}

#[test]
fn op_4c_n5_sub_2_halts_while_menu_still_loading() {
    let bytecode = [0x4Cu8, 0x52, 0x05];
    let mut host = TestHost::default();
    // Menu state defaults to false - polling halts at PC.
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert!(host.n_6_sub_61_emitter_calls.is_empty());
}

#[test]
fn op_4c_n6_sub_61_acquires_and_advances_sixteen() {
    // 16-byte instruction; TestHost's predicate defaults to `false`, so
    // arm it for the acquire-success path.
    let bytecode = [
        0x4Cu8, 0x61, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D,
        0x0E,
    ];
    let mut host = TestHost {
        halt_acquire_predicate: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 16 });
    // ctx mutation: halt bit set, saved_pc and wait_accum updated.
    assert_eq!(ctx.flags & 0x400, 0x400);
    assert_eq!(ctx.saved_pc, 0);
    assert_eq!(ctx.wait_accum, 0);
    // Apply hook fired with which = 0x61.
    assert!(host.halt_acquire_calls.iter().any(|(w, _, _)| *w == 0x61));
    // Emitter received bytes +2..+15 (14 data bytes; sub-byte 0x61 stripped).
    assert_eq!(host.n_6_sub_61_emitter_calls.len(), 1);
    assert_eq!(
        host.n_6_sub_61_emitter_calls[0],
        [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E
        ],
    );
}

#[test]
fn op_4c_n6_sub_61_halts_when_predicate_refuses() {
    let bytecode = [
        0x4Cu8, 0x61, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00,
    ];
    // halt_acquire_predicate defaults to false → refuse.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert!(host.n_6_sub_61_emitter_calls.is_empty());
    assert_eq!(ctx.flags & 0x400, 0, "ctx unmutated on refusal");
}

#[test]
fn op_4c_n8_sub_0_acquires_and_advances_three() {
    let bytecode = [0x4Cu8, 0x80, 0x03, 0xAA, 0xBB, 0xCC];
    let mut host = TestHost {
        halt_acquire_predicate: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(ctx.flags & 0x400, 0x400);
    assert!(host.halt_acquire_calls.iter().any(|(w, _, _)| *w == 0x80));
    assert_eq!(host.n_8_sub_0_allocator_calls.len(), 1);
    let (count, tail) = &host.n_8_sub_0_allocator_calls[0];
    assert_eq!(*count, 3);
    assert_eq!(tail.as_slice(), &[0xAA, 0xBB, 0xCC]);
}

#[test]
fn op_4c_n8_sub_0_halts_when_predicate_refuses() {
    let bytecode = [0x4Cu8, 0x80, 0x00];
    // halt_acquire_predicate defaults to false.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert!(host.n_8_sub_0_allocator_calls.is_empty());
}

#[test]
fn op_4c_n8_sub_3_rect_tile_fill_emits_host_hook_and_advances_7() {
    // [4C, 0x83, col_start, row_start, col_end, row_end, value]
    // Total instruction = 7 bytes (header + op0 + 5 operand bytes).
    let bytecode = [0x4Cu8, 0x83, 2, 4, 5, 6, 0x7F];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
    assert_eq!(host.n8_rect_tile_fills, vec![(2u8, 4, 5, 6, 0x7F)]);
}

#[test]
fn op_4c_n8_sub_3_truncated_bytecode_returns_unknown() {
    // Operand list cut short (only 4 operand bytes instead of 5).
    let bytecode = [0x4Cu8, 0x83, 1, 1, 1, 1];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    );
    assert!(host.n8_rect_tile_fills.is_empty());
}

#[test]
fn op_4c_n8_sub_a_writes_quad() {
    // [4C, 0x8A, s0_lo, s0_hi, s1_lo, s1_hi, s2_lo, s2_hi, u24_b0, u24_b1, u24_b2]
    // Original encodes the trailing slot as a 24-bit LE integer (read
    // via `func_0x8003CEB8`). Total instruction = 11 bytes.
    let mut bytecode = vec![0x4C, 0x8A];
    for s in &[10i16, 20, 30] {
        bytecode.extend_from_slice(&s.to_le_bytes());
    }
    // u24 = 0x00ADBEEF (bytes 0xEF, 0xBE, 0xAD).
    bytecode.extend_from_slice(&[0xEF, 0xBE, 0xAD]);
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 11 });
    assert_eq!(host.n8_quad_writes, vec![([10i16, 20, 30], 0x00AD_BEEFu32)]);
}

#[test]
fn op_4c_n8_sub_9_writes_signed_16_and_advances_4() {
    // [4C, 0x89, 0x34, 0x12] writes 0x1234 then PC += 4.
    let bytecode = [0x4Cu8, 0x89, 0x34, 0x12];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    assert_eq!(host.n8_sub9_writes, vec![0x1234i16]);
}

#[test]
fn op_4c_n9_sub_0_passes_b1_and_three_words() {
    // [4C, 0x90, b1=7, lo,hi, lo,hi, lo,hi]
    let mut bytecode = vec![0x4C, 0x90, 0x07];
    for w in &[1i16, 2, 3] {
        bytecode.extend_from_slice(&w.to_le_bytes());
    }
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 9 });
    assert_eq!(host.n9_dde34_calls, vec![(0u8, 0x07u8, [1i16, 2, 3])]);
}

#[test]
fn op_4c_n9_sub_e_copies_16_signed_words() {
    let mut bytecode = vec![0x4C, 0x9E];
    for i in 0..16i16 {
        bytecode.extend_from_slice(&(i * 100 - 700).to_le_bytes());
    }
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x22 });
    let mut expected = [0i16; 16];
    for (i, e) in expected.iter_mut().enumerate() {
        *e = (i as i16) * 100 - 700;
    }
    assert_eq!(host.n9_table_copies, vec![expected]);
}

#[test]
fn op_4c_n_a_sub_0_advances_when_ctx_flag_clear() {
    // [4C, 0xA0, bit=4, lo, hi] - ctx.flags bit 4 clear → skip 5 bytes.
    // The asm at 0x801e2580/4 ANDs ctx[+0x10] with `(1 << bit)` and only
    // branches to the take-jump label when the result is non-zero.
    let bytecode = [0x4Cu8, 0xA0, 0x04, 0x00, 0x01];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
}

#[test]
fn op_4c_n_a_sub_0_branches_when_ctx_flag_set() {
    // ctx.flags bit 4 SET → take the absolute jump (0x100).
    let bytecode = [0x4Cu8, 0xA0, 0x04, 0x00, 0x01];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        flags: 1u32 << 4,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x100 });
}

#[test]
fn op_4c_n_a_sub_2_uses_global_flags() {
    // Global flag set → take jump.
    let bytecode = [0x4Cu8, 0xA2, 0x03, 0x20, 0x00];
    let mut host = TestHost {
        globals: 1u32 << 3,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x20 });
}

#[test]
fn op_4c_n_a_sub_3_through_f_skip_5() {
    // Sub-ops 3..=0xF have no `case` arm in the asm. The dispatch at
    // 0x801e2568 (`bne a1, zero, 0x801e258c`) jumps past every bank
    // check, both `bne a1, 1` and `bne a1, 2` then branch to the
    // skip-5 exit. The s8 += 5 in the delay slot of the first bne is
    // the PC delta.
    let bytecode = [0x4Cu8, 0xA5, 0xFF, 0x00, 0x01];
    let mut host = TestHost {
        globals: !0u32,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx {
        flags: !0u32,
        local_flags: !0u16,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
}

#[test]
fn op_4c_n9_sub_f_registers_callback_and_halts() {
    // [4C, 0x9F] - register `LAB_801DA930` callback then halt at PC.
    // Same dispatch pattern as nibble-8 sub-7 (callback target differs).
    let bytecode = [0x4Cu8, 0x9F];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(host.n9_callback_regs, 1);
}

#[test]
fn op_4c_n8_sub_7_registers_callback_and_halts() {
    // [4C, 0x87] - register actor-list callback (LAB_801E5154) then halt
    // at PC. The original goes through `switchD_801e00f4::default()`,
    // which for opcode 0x4C (`& 0x70 = 0x40`) returns `param_2` -
    // halt at PC. Our hook is one-shot per dispatch entry.
    let bytecode = [0x4Cu8, 0x87];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(host.n8_callback_regs, 1);
}

#[test]
fn op_4c_n_c_sub_4_subtile_broadcast() {
    let bytecode = [0x4Cu8, 0xC4, 0x05, 0x07];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    assert_eq!(host.n_c_subtile_broadcasts, vec![(5u8, 7u8)]);
}

#[test]
fn op_4c_n_c_sub_8_xors_field_74() {
    let bytecode = [0x4Cu8, 0xC8];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        field_74: 0,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.field_74, 0x1000_0000);

    // Re-applying flips back.
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.field_74, 0);
}

#[test]
fn op_4c_n_c_sub_a_writes_slot() {
    let bytecode = [0x4Cu8, 0xCA, 0x05, 0x10, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert_eq!(host.n_c_slot_writes, vec![(5u8, 0x10i16)]);
}

#[test]
fn op_4c_n_c_sub_b_substitutes_frame_delta_when_value_is_ffff() {
    let bytecode = [0x4Cu8, 0xCB, 0x07, 0xFF, 0xFF];
    let mut host = TestHost {
        frame_delta: 3,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert_eq!(host.n_c_slot_adjusts, vec![(7u8, 3i16, false)]);
}

#[test]
fn op_4c_n_c_sub_c_subtracts() {
    let bytecode = [0x4Cu8, 0xCC, 0x02, 0x05, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert_eq!(host.n_c_slot_adjusts, vec![(2u8, 5i16, true)]);
}

#[test]
fn op_4c_n_c_sub_2_writes_field_42() {
    let bytecode = [0x4Cu8, 0xC2, 0x55];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(ctx.field_42, 0x55);
}

#[test]
fn op_4c_n8_sub_5_halt_acquire_writes_ctx_and_advances() {
    // Success path (default predicate = acquire): the 5-byte op
    // `[4C, 0x85, p0, p1, p2]` halts the TARGET context and advances the
    // CALLER past the payload (retail `iVar24 = 5` then the standard advance
    // exit - overlay_0897_801de840.txt:6550 / overlay_world_map:7179).
    let bytecode = [0x4Cu8, 0x85, 0x00, 0x00, 0x10];
    let mut host = TestHost {
        halt_acquire_predicate: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx {
        wait_accum: 7,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert_eq!(ctx.saved_pc, 0);
    assert_eq!(ctx.wait_accum, 0);
    assert_eq!(ctx.flags & 0x400, 0x400);
    assert_eq!(host.n8_halt_acquires, vec![0u32]);
}

#[test]
fn op_4c_n8_sub_5_halt_acquire_parks_on_predicate_failure() {
    // TestHost's predicate defaults to false → refuse: park at PC, no ctx
    // mutation (retail `iVar24 = 0` → `LAB_801dee50`).
    let bytecode = [0x4Cu8, 0x85, 0x00, 0x00, 0x10];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(ctx.flags & 0x400, 0);
    assert!(host.n8_halt_acquires.is_empty());
}

#[test]
fn op_4c_n8_sub_e_and_f_share_halt_acquire_body() {
    for sub in [0x8Eu8, 0x8F] {
        let bytecode = [0x4Cu8, sub, 0x00, 0x00, 0x00];
        let mut host = TestHost {
            halt_acquire_predicate: true,
            ..TestHost::default()
        };
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 5 });
        assert_eq!(host.n8_halt_acquires, vec![0u32]);
    }
}

#[test]
fn op_4c_n_c_sub_9_advances_when_globals_match() {
    let bytecode = [0x4Cu8, 0xC9];
    let mut host = TestHost {
        n_c_sub9_globals_differ: false,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
}

#[test]
fn op_4c_n_c_sub_9_halts_when_globals_differ() {
    let bytecode = [0x4Cu8, 0xC9];
    let mut host = TestHost {
        n_c_sub9_globals_differ: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}

#[test]
fn op_4c_n_d_sub_6_b1_eq_4_clears_top_bit_only() {
    let bytecode = [0x4Cu8, 0xD6, 0x04];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        field_74: 0xFFFF_FFFF,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(ctx.field_74, 0x7FFF_FFFF);
    assert_eq!(host.n_d_sub6_acks, 1);
}

#[test]
fn op_4c_n_d_sub_6_b1_neq_4_sets_high_byte() {
    let bytecode = [0x4Cu8, 0xD6, 0x12];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        field_74: 0,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    // Sets bit 0x80000000 + (0x12 << 24) = 0x92000000.
    assert_eq!(ctx.field_74, 0x9200_0000);
}

#[test]
fn op_4c_n_d_sub_8_passes_b1_and_three_words_advances_9() {
    let bytecode = [0x4Cu8, 0xD8, 0x05, 0x10, 0x00, 0x20, 0x00, 0x30, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 9 });
    assert_eq!(host.n_d_sub8_calls, vec![(5u8, [0x10i16, 0x20, 0x30])]);
}

#[test]
fn op_4c_n_e_sub_0_writes_b1_and_halts() {
    let bytecode = [0x4Cu8, 0xE0, 0x42];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(host.n_e_sub0_writes, vec![0x42u8]);
}

#[test]
fn op_4c_n_e_sub_9_clears_global_and_advances_2() {
    let bytecode = [0x4Cu8, 0xE9];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.n_e_sub9_clears, 1);
}

#[test]
fn op_4c_n_e_sub_a_calls_overlay_and_halts() {
    let bytecode = [0x4Cu8, 0xEA];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(host.n_e_sub_a_calls, 1);
}

#[test]
fn op_4c_n_c_sub_f_uses_actor_world_when_byte_is_ff() {
    // [4C, 0xCF, 0xFF, 0xFF] - both bytes select the actor's coords.
    let bytecode = [0x4Cu8, 0xCF, 0xFF, 0xFF];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        world_x: 0x1234,
        world_z: 0x5678,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    assert_eq!(
        host.n_c_sub_f_broadcasts,
        vec![(0x1234u16 as i16, 0x5678u16 as i16)]
    );
}

#[test]
fn op_4c_n_c_sub_f_zero_yields_zero_nonzero_yields_tile_center() {
    // b1=0 → 0; b2=2 → 2*0x80 + 0x40 = 0x140.
    let bytecode = [0x4Cu8, 0xCF, 0x00, 0x02];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    assert_eq!(host.n_c_sub_f_broadcasts, vec![(0, 0x0140)]);
}

#[test]
fn op_4c_n_d_sub_3_party_setup_advances_14_bytes() {
    let mut bytecode = vec![0x4C, 0xD3];
    bytecode.extend_from_slice(&0x1234i16.to_le_bytes());
    bytecode.extend_from_slice(&0x5678i16.to_le_bytes());
    bytecode.extend_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
    bytecode.extend_from_slice(&0xCAFE_BABEu32.to_le_bytes());
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 14 });
    assert_eq!(host.n_d_party_setups.len(), 1);
    let (ab, cd, ef) = host.n_d_party_setups[0];
    assert_eq!(ab, (0x1234u32 << 16) | 0x5678u32);
    assert_eq!(cd, 0xDEAD_BEEF);
    assert_eq!(ef, 0xCAFE_BABE);
}

#[test]
fn op_4c_n_d_sub_9_sets_inverted_y_mirror_and_negates_world_y() {
    // value = 5 → field_8e = 5, world_y = -5 (= 0xFFFB).
    let bytecode = [0x4Cu8, 0xD9, 0x05, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    assert!(ctx.flags & 0x2000_0000 != 0);
    assert_eq!(ctx.field_8e, 5);
    assert_eq!(ctx.world_y, 0xFFFB);
}

#[test]
fn op_4c_n_d_sub_9_9999_sentinel_keeps_world_y_unchanged() {
    let bytecode = [0x4Cu8, 0xD9, 0x0F, 0x27]; // 9999 LE
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        world_y: 0x0040,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    // value = -world_y = -0x40, ctx.field_8e = -0x40 = 0xFFC0.
    assert_eq!(ctx.field_8e, -0x40);
    // world_y = -value = world_y = 0x40 (unchanged).
    assert_eq!(ctx.world_y, 0x0040);
}

#[test]
fn op_4c_n_d_sub_a_clears_inverted_y_and_calls_collision_y() {
    let bytecode = [0x4Cu8, 0xDA];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        flags: 0x2000_0000,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.flags & 0x2000_0000, 0);
    assert_eq!(host.n_d_collision_y_calls, 1);
}

#[test]
fn op_4c_n_d_sub_d_writes_field_58() {
    let bytecode = [0x4Cu8, 0xDD, 0x77];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(ctx.field_58, 0x77);
}

#[test]
fn op_4c_n_d_sub_f_scene_byte_write() {
    let bytecode = [0x4Cu8, 0xDF, 0xAB];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(host.n_d_scene_byte_writes, vec![0xABu8]);
}

#[test]
fn op_4c_n_e_sub_2_fmv_trigger_decodes_fmv_id() {
    // The 6-byte form `[4C, 0xE2, lo, hi, _, _]` triggers an FMV
    // (game-mode 26 / StrInit). The fmv_id is the s16 at +1..+3
    // and selects an entry in the runtime FMV-state table.
    let bytecode = [0x4Cu8, 0xE2, 0x03, 0x00, 0, 0];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.n_e_fmv_triggers, vec![3i16]);
}

#[test]
fn op_4c_n_e_sub_2_fmv_trigger_sign_extends_negative() {
    // s16 sign-extension - retail dispatcher reads through
    // FUN_8003CE9C which sign-extends. fmv_id can be negative
    // when the script wants to clear the trigger / signal a
    // sentinel.
    let bytecode = [0x4Cu8, 0xE2, 0xFF, 0xFF, 0, 0];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.n_e_fmv_triggers, vec![-1i16]);
}

#[test]
fn op_4c_n_e_sub_6_d8280_passes_three_words() {
    let mut bytecode = vec![0x4C, 0xE6];
    for w in &[5i16, 10, 15] {
        bytecode.extend_from_slice(&w.to_le_bytes());
    }
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 8 });
    assert_eq!(host.n_e_d8280_calls, vec![[5i16, 10, 15]]);
}

#[test]
fn op_4c_n_e_sub_c_capture_call() {
    let bytecode = [0x4Cu8, 0xEC];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.n_e_capture_ddf48_calls, 1);
}

#[test]
fn op_4c_n_e_sub_d_writes_ba66() {
    let bytecode = [0x4Cu8, 0xED, 0x88];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(host.n_e_ba66_writes, vec![0x88u8]);
}

#[test]
fn op_4c_n_e_sub_e_snapshot_call() {
    let bytecode = [0x4Cu8, 0xEE];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.n_e_snapshot_84570_calls, 1);
}

#[test]
fn op_4c_n_f_pass_through_advances_two_bytes() {
    let bytecode = [0x4Cu8, 0xFF];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
}

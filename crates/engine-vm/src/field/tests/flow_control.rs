//! Core flow control: NOP/JMP_REL, flag triplets, YIELD, DATA_BLOCK, WAIT_FRAMES, peek_extended, cross-context extended dispatch. Extracted verbatim from `field/tests.rs`.

use super::*;

// -- NOP cluster ----------------------------------------------------

#[test]
fn nop_cluster_advances_one_byte() {
    for op in [0x21u8, 0x24, 0x25, 0x48] {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[op], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 1 });
    }
}

// -- JMP_REL --------------------------------------------------------

#[test]
fn jmp_rel_forward() {
    // 0x26 with offset 0x0008 should land at PC = 0 + 1 + 8 = 9.
    let bc = [0x26, 0x08, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 9 });
}

#[test]
fn jmp_rel_with_high_byte() {
    // Offset 0x0100 from PC 0 lands at 0x0101.
    let bc = [0x26, 0x00, 0x01];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x0101 });
}

#[test]
fn jmp_rel_backward_wraps_at_16_bits() {
    // Retail stores PC as a 16-bit `short`, so a delta with the high bit
    // set is a *backward* jump - `0xFFFE` = -2. From base = pc + 1 this
    // must land 2 bytes back, NOT race off to base + 0xFFFE. This is the
    // "PC runs away to 0x10102" bug: real field scripts use backward
    // JMP_REL for per-frame wait loops, so without the 16-bit wrap every
    // parked script explodes off the end of its buffer.
    let mut bc = vec![0u8; 0x80];
    bc[0x42] = 0x26;
    bc[0x43] = 0xFE;
    bc[0x44] = 0xFF;
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // base = pc(0x42) + header_size(1) = 0x43; 0x43 + (-2) = 0x41.
    let r = step(&mut host, &mut ctx, &bc, 0x42);
    assert_eq!(r, StepResult::Advance { next_pc: 0x41 });
    // From PC 0 the same -2 delta wraps to 0xFFFF (a deliberately
    // out-of-range PC, matching retail's 16-bit truncation).
    let bc0 = [0x26, 0xFE, 0xFF];
    let r0 = step(&mut host, &mut ctx, &bc0, 0);
    assert_eq!(r0, StepResult::Advance { next_pc: 0xFFFF });
}

#[test]
fn flag_test_backward_jump_wraps_at_16_bits() {
    // The 0x7x flag-TEST conditional jump shares the same 16-bit-wrap
    // rule. With the bit set and a `0xFFF0` (-16) delta from base
    // pc+header+1, a backward jump must wrap rather than overflow.
    let mut bc = vec![0u8; 0x80];
    bc[0x30] = 0x70;
    bc[0x31] = 0x00;
    bc[0x32] = 0xF0;
    bc[0x33] = 0xFF;
    let mut host = TestHost::default();
    host.system_flags.resize(8192, 0);
    host.system_flags[0] = 0x80; // idx 0 set (0x80 >> 0)
    let mut ctx = FieldCtx::default();
    // base = pc(0x30) + header(1) + 1 = 0x32; 0x32 + (-16) = 0x22.
    let r = step(&mut host, &mut ctx, &bc, 0x30);
    assert_eq!(r, StepResult::Advance { next_pc: 0x22 });
}

// -- Local flag triplet 0x2B / 0x2C / 0x2D ---------------------------

#[test]
fn lflag_set_then_clear() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // Set bit 5 (mask 0x20).
    let r = step(&mut host, &mut ctx, &[0x2B, 5], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.local_flags, 0x0020);

    // Clear bit 5.
    let r = step(&mut host, &mut ctx, &[0x2C, 5], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.local_flags, 0);
}

#[test]
fn lflag_set_masks_to_5_bits() {
    // The original masks the bit index by 0x1F. Index 0x25 should set
    // bit 5 (0x20), not bit 0x25.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    step(&mut host, &mut ctx, &[0x2B, 0x25], 0);
    assert_eq!(ctx.local_flags, 0x0020);
}

#[test]
fn lflag_tst_bit_set_advances() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        local_flags: 0x0040, // bit 6 set
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0x2D, 6], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
}

#[test]
fn lflag_tst_bit_clear_halts() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x2D, 6], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}

// -- Global flag triplet 0x2E / 0x2F / 0x30 --------------------------

#[test]
fn gflag_set_writes_through_host() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x2E, 17], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.globals, 1u32 << 17);
}

#[test]
fn gflag_clr_preserves_other_bits() {
    let mut host = TestHost {
        globals: 0xFFFF_FFFF,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    step(&mut host, &mut ctx, &[0x2F, 3], 0);
    assert_eq!(host.globals, !(1u32 << 3));
}

#[test]
fn gflag_tst_branches_on_global() {
    let mut host = TestHost {
        globals: 1u32 << 12,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x30, 12], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });

    // bit 13 is clear -> halt
    let r = step(&mut host, &mut ctx, &[0x30, 13], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}

// -- Context flag triplet 0x31 / 0x32 / 0x33 -------------------------

#[test]
fn cflag_set_normal_path_advances() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x31, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.flags, 1);
}

#[test]
fn cflag_set_bit_8_copies_field_26_and_advances() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        field_26: 0x1234,
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0x31, 8], 0);
    // Bit 8 = mask 0x100. The original calls
    // `switchD_801e0f24::caseD_4()` which is 0x801df098 → s8 += 2 →
    // return; PC advances by 2 like every other 0x31 path.
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.flags, 0x100);
    assert_eq!(ctx.saved_26, 0x1234);
}

#[test]
fn cflag_clr_basic() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        flags: 0xFFFF_FFFF,
        ..Default::default()
    };
    step(&mut host, &mut ctx, &[0x32, 10], 0);
    assert_eq!(ctx.flags, !(1u32 << 10));
}

#[test]
fn cflag_tst_halts_when_clear() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x33, 0], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}

// -- YIELD ----------------------------------------------------------

#[test]
fn yield_sets_halt_and_saves_pc() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x37], 0);
    assert_eq!(r, StepResult::Yield { resume_pc: 3 });
    assert!(ctx.is_halted());
    assert_eq!(ctx.saved_pc, 0);
}

#[test]
fn yield_4_uses_pc_plus_4() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // Pad so pc=5 is in-bounds (yield reads only the opcode byte).
    let mut bc = [0u8; 6];
    bc[5] = 0x47;
    let r = step(&mut host, &mut ctx, &bc, 5);
    assert_eq!(r, StepResult::Yield { resume_pc: 9 });
    assert_eq!(ctx.saved_pc, 5);
}

// -- DATA_BLOCK ------------------------------------------------------

#[test]
fn data_block_skips_len_bytes() {
    // len = 4: skip operand byte + 4 inline bytes => PC += 6.
    let bc = [0x40, 0x04, 0xAA, 0xBB, 0xCC, 0xDD];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
}

// -- WAIT_FRAMES ----------------------------------------------------

#[test]
fn wait_frames_accumulates_until_target() {
    let mut host = TestHost {
        frame_delta: 1,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let bc = [0x4A, 0x03, 0x00]; // wait 3 frames

    // Tick 1: accum=1, halt.
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(ctx.wait_accum, 1);

    // Tick 2: accum=2, still halt.
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(ctx.wait_accum, 2);

    // Tick 3: accum=3, advance + reset.
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(ctx.wait_accum, 0);
}

// -- Pending / Unknown -----------------------------------------------

#[test]
fn op_4c_n8_sub_3_no_longer_pending() {
    // Sanity-check that the rectangular tile-fill case (`0x4C n8 sub-3`,
    // host hook [`FieldHost::op4c_n_8_sub_3_rect_tile_fill`]) is fully
    // ported. The dispatcher previously returned `Pending` for this
    // opcode; it now invokes the host and advances PC by 7. There are
    // no remaining `0x4C` sub-ops that return `Pending`.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x83, 0x00, 0x00, 0x00, 0x00, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
    assert_eq!(host.n8_rect_tile_fills, vec![(0u8, 0, 0, 0, 0)]);
}

// -- Multi-instruction trace -----------------------------------------

#[test]
fn trace_runs_through_chain() {
    // Set local-flag bit 0, jump forward over 3 garbage bytes, set local
    // bit 1, fall off the end. JMP_REL formula:
    //   target = (pc + header_size) + ((hi << 8) | lo)
    // Jumping from pc=2 with offset 5 lands at byte 8 = the second SET.
    let bc = [
        0x2B, 0, // [0..2] LFLAG_SET 0
        0x26, 0x05, 0x00, // [2..5] JMP +5 -> target = 3 + 5 = 8
        0xFF, 0xFF, 0xFF, // [5..8] unreachable
        0x2B, 1, // [8..10] LFLAG_SET 1
    ];

    let mut host = TestHost::default();
    let (ctx, _trace) = run(&mut host, &bc);
    assert_eq!(ctx.local_flags, 0b11);
}

// -- peek_extended ---------------------------------------------------

#[test]
fn peek_extended_returns_target_id_when_high_bit_set() {
    // 0xA1 = 0x80 | 0x21 (extended NOP). Next byte is target script ID.
    let bc = [0xA1, 0x42, 0x00];
    assert_eq!(peek_extended(&bc, 0), Some(0x42));
}

#[test]
fn peek_extended_returns_none_for_normal_opcode() {
    let bc = [0x21, 0x42];
    assert_eq!(peek_extended(&bc, 0), None);
}

#[test]
fn peek_extended_handles_eof() {
    // Empty bytecode -> None.
    assert_eq!(peek_extended(&[], 0), None);
    // Lone extended-bit byte at end (no script-id byte) -> None.
    assert_eq!(peek_extended(&[0xA1], 0), None);
}

// -- Cross-context dispatch (extended bit) --------------------------

#[test]
fn extended_lflag_set_advances_three_bytes() {
    // Encoding: [0xAB (= 0x80 | 0x2B), script_id, bit].
    // Header size = 2; tail = 1; next_pc = 0 + 2 + 1 = 3.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        script_id: 5,
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0xAB, 5, 3], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(ctx.local_flags, 1u16 << 3);
}

#[test]
fn extended_jmp_rel_uses_header_size_2() {
    // [0xA6, script_id, lo, hi]. target = pc + 2 + delta. With pc=0,
    // delta=4, expect target=6.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0xA6, 0x42, 0x04, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
}

#[test]
fn extended_halt_target_returns_halt() {
    // Halted ctx + extended dispatch + non-carve-out op -> Halt.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        flags: 0x400, // halted
        script_id: 7,
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0xAB, 7, 0], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    // Local flag should NOT have been written.
    assert_eq!(ctx.local_flags, 0);
}

#[test]
fn extended_system_channel_bypasses_halt() {
    // script_id == 0xFB is the system channel; halted state is ignored.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        flags: 0x400,
        script_id: 0xFB,
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0xAB, 0xFB, 4], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(ctx.local_flags, 1u16 << 4);
}

#[test]
fn extended_032_with_bit_10_unhalt_carve_out() {
    // 0x32 (CFLAG_CLR) with bit 10 (mask 0x400) is the unique opcode that
    // can run on a halted target - it's how a script un-halts another.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        flags: 0x400,
        script_id: 9,
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0xB2, 9, 10], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(ctx.flags, 0); // halt bit cleared
    assert!(!ctx.is_halted());
}

#[test]
fn extended_032_with_other_bit_does_not_bypass_halt() {
    // Same opcode but a different bit -> halt path still wins.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        flags: 0x400 | 0x10,
        script_id: 9,
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0xB2, 9, 4], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    // Bit 4 should NOT have been cleared (the dispatch never fired).
    assert_eq!(ctx.flags, 0x400 | 0x10);
}

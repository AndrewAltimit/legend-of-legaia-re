//! 0x4D BBOX_TEST, 0x36 SCENE_FADE, 0x45 CAMERA, 0x4E inventory compare, 0x49 STATE_RESUME. Extracted verbatim from `field/tests.rs`.

use super::*;

// -- 0x4D BBOX_TEST --------------------------------------------------

#[test]
fn bbox_inside_default_tile_derivation() {
    // world_x = 0x1C0 → tile = (0x1C0 - 0x40) >> 7 = 3
    // world_z = 0x140 → tile = (0x140 - 0x40) >> 7 = 2
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        world_x: 0x01C0,
        world_z: 0x0140,
        ..Default::default()
    };
    // bbox: x in [2..4], z in [1..3] → inside.
    let r = step(&mut host, &mut ctx, &[0x4D, 2, 1, 4, 3, 0xAA, 0xBB], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
}

// -- 0x36 SCENE_FADE -------------------------------------------------

#[test]
fn scene_fade_done_advances_5() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x36, 0x10, 0x80, 0x05, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert_eq!(host.scene_fade_calls, vec![(0x8010u16, 0x0005u16)]);
}

#[test]
fn scene_fade_busy_halts() {
    let mut host = TestHost {
        scene_fade_busy: true,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x36, 0xFF, 0xFF, 0x00, 0x00], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(host.scene_fade_calls, vec![(0xFFFFu16, 0u16)]);
}

// -- 0x45 CAMERA -----------------------------------------------------

#[test]
fn camera_load_advances_20() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // op0 = 0x40 → LOAD; 18 bytes payload.
    let mut bc = vec![0x45, 0x40];
    bc.extend((0..18u8).map(|i| 0x10 + i));
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 20 });
    assert_eq!(host.camera_loads.len(), 1);
    assert_eq!(host.camera_loads[0].len(), 18);
    assert_eq!(host.camera_loads[0][0], 0x10);
    assert_eq!(host.camera_loads[0][17], 0x21);
}

#[test]
fn camera_save_advances_2_and_pings_host() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // op0 = 0x80 → SAVE.
    let r = step(&mut host, &mut ctx, &[0x45, 0x80], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.camera_saves, 1);
}

#[test]
fn camera_apply_jumps_to_absolute_pc() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // op0 = 0xC0 → APPLY; absolute target = LE_u16(operand[1..3]) = 0x0042.
    let r = step(&mut host, &mut ctx, &[0x45, 0xC0, 0x42, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x42 });
    assert_eq!(host.camera_applies, 1);
}

#[test]
fn camera_configure_decodes_mask_and_advances() {
    // op0 = 0x00 (top 2 bits clear; mode = (0 >> 2) & 0xF = 0).
    // op1 = 0b1010_0000 = 0xA0 → mask bit 7 (slot 2), bit 5 (slot 4).
    // Wait - the bit interpretation is "MSB-first across the 10-bit
    // mask" from CONCAT11(op0, op1). So mask = u16(op0:op1) =
    // 0x00A0 = 0b0000_0000_1010_0000. Bit 9 → slot 0, bit 0 → slot 9.
    // Set bits at positions 7 and 5 → slot indices 9-7=2 and 9-5=4.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // [45, 0x00, 0xA0, lo_trig, hi_trig, val0_lo, val0_hi, val1_lo, val1_hi]
    let bc = [
        0x45, 0x00, 0xA0, // opcode + op0/op1 mask
        0x34, 0x12, // apply_trigger = 0x1234
        0x55, 0x44, // first set bit (slot 2) → 0x4455
        0x66, 0x77, // second set bit (slot 4) → 0x7766
    ];
    let r = step(&mut host, &mut ctx, &bc, 0);
    // PC += 5 + 2*set_count = 5 + 4 = 9.
    assert_eq!(r, StepResult::Advance { next_pc: 9 });
    assert_eq!(host.camera_configs.len(), 1);
    let (params, trigger, mode) = &host.camera_configs[0];
    assert_eq!(*trigger, 0x1234);
    assert_eq!(*mode, 0);
    assert_eq!(params.len(), 2);
    assert_eq!(
        params[0],
        CameraParam {
            slot: 2,
            value: 0x4455
        }
    );
    assert_eq!(
        params[1],
        CameraParam {
            slot: 4,
            value: 0x7766
        }
    );
}

#[test]
fn camera_configure_zero_mask_advances_5() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // No bits set → no params, but still PC += 5.
    let bc = [0x45, 0x00, 0x00, 0x12, 0x34];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert_eq!(host.camera_configs.len(), 1);
    assert!(host.camera_configs[0].0.is_empty());
    assert_eq!(host.camera_configs[0].1, 0x3412);
}

#[test]
fn camera_configure_mode_is_op0_shifted_right_2_low_4() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // op0 = 0x14 (top 2 bits clear → CONFIGURE; (0x14 >> 2) & 0xF = 5).
    let bc = [0x45, 0x14, 0x00, 0x00, 0x00];
    step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(host.camera_configs[0].2, 5);
}

#[test]
fn bbox_outside_jumps_via_skip_delta() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        world_x: 0x01C0, // tile 3
        world_z: 0x0140, // tile 2
        ..Default::default()
    };
    // bbox: x in [10..20] → outside. skip = 0x100; outside-box jump
    // target = pc + header_size + 4 + delta = 0 + 1 + 4 + 0x100 = 261.
    let r = step(&mut host, &mut ctx, &[0x4D, 10, 0, 20, 10, 0, 1], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 261 });
}

#[test]
fn bbox_outside_zero_skip_advances_5() {
    // Confirm that skip=0 still produces a non-zero next_pc (= pc + 5).
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        world_x: 0x01C0,
        world_z: 0x0140,
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0x4D, 10, 0, 20, 10, 0, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
}

// -- 0x4E inventory comparison-and-jump ----------------------------

#[test]
fn op_4e_sub0_lt_taken_jumps_relative() {
    // Sub-op 0, comparison 0 (state < scaled). state=5, factor=10,
    // arg=128 -> scaled = (10 * 128) >> 8 = 5; 5 < 5 = false; not taken.
    let mut host = TestHost::default();
    host.inventory_pairs.insert((0, 0), (5, 10));
    let mut ctx = FieldCtx::default();
    // skip = 100 (0x64). Not taken -> PC += 7.
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4E, 0, 0x00, 0x80, 0x00, 0x64, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
}

#[test]
fn op_4e_sub0_lt_strict_taken() {
    // state=5, factor=10, arg=200 -> scaled = (10*200)>>8 = 7; 5 < 7
    // is true. Forward jump: PC = pc + 1 + 4 + 100 = 105.
    let mut host = TestHost::default();
    host.inventory_pairs.insert((0, 0), (5, 10));
    let mut ctx = FieldCtx::default();
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4E, 0, 0x00, 200, 0x00, 100, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 105 });
}

#[test]
fn op_4e_sub1_gt_uses_second_pair() {
    // mode_byte = 0x11 (sub=1, op=1 → scaled < state). page=2.
    // pair (2, 1) returns (state=20, factor=8). arg = 1024.
    // scaled = (8 * 1024) >> 8 = 32. 32 < 20 is false; not taken.
    let mut host = TestHost::default();
    host.inventory_pairs.insert((2, 1), (20, 8));
    let mut ctx = FieldCtx::default();
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4E, 2, 0x11, 0x00, 0x04, 0x42, 0x00],
        0,
    );
    // not-taken -> PC += 7
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
}

#[test]
fn op_4e_sub2_compares_char_level() {
    // Sub-op 2 loads the character-record level byte (+0x130) and joins
    // the shared 7-byte compare-and-skip continuation (raw loader
    // 0x801E0AC0 -> 0x801E0B40); the old "absolute jump" reading was the
    // Ghidra decomp's collapsed switch arm. mode low nibble 0
    // (state < arg): level 5 < 10 -> taken, jump pc + 1 + 4 + 0x10 = 21.
    let mut host = TestHost::default();
    host.op4e_char_levels.insert(2, 5);
    let mut ctx = FieldCtx::default();
    let bc = [0x4E, 0x02, 0x20, 0x0A, 0x00, 0x10, 0x00];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 21 });
    // Level 10: 10 < 10 is false -> PC += 7.
    let mut host = TestHost::default();
    host.op4e_char_levels.insert(2, 10);
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
}

#[test]
fn op_4e_sub5_through_8_compare_slot_table() {
    // Sub-ops 5..=8 load the slot table 0x801C6460[sub - 5] (s16) via the
    // loader at 0x801E0B0C and join the shared compare. This is the
    // cave01 P2[12] spawn gate shape: `4E 00 50 08 00 06 00` = skip +6
    // while slot 0 < 8.
    let mut ctx = FieldCtx::default();
    let bc = [0x4E, 0x00, 0x50, 0x08, 0x00, 0x06, 0x00];
    // slot 0 = 3 -> 3 < 8 taken: pc + 1 + 4 + 6 = 11.
    let mut host = TestHost::default();
    host.slot_table_values.insert(0, 3);
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 11 });
    // slot 0 = 8 -> not taken: PC += 7 (falls into the next op).
    let mut host = TestHost::default();
    host.slot_table_values.insert(0, 8);
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
    // Sub-8 reads slot 3.
    let mut host = TestHost::default();
    host.slot_table_values.insert(3, 100);
    let bc = [0x4E, 0x00, 0x81, 0x63, 0x00, 0x02, 0x00];
    let r = step(&mut host, &mut ctx, &bc, 0);
    // mode 1: arg 0x63 = 99 < 100 -> taken: pc + 5 + 2 = 7... (delta 2)
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
}

#[test]
fn op_4e_sub_c_through_f_advance_seven() {
    // mode_byte high-nibble in 12..=15 hits the dispatcher's default arm
    // at switchD_801e0a38_default. With no party-bank state initialised,
    // `uVar31 = uVar27 = 0`; the comparison is false, and `(sub-10) < 2`
    // is false for sub-op >= 12, so the original returns `param_2 + 7`
    // (= PC += 7 from the opcode).
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    for sub in 12..=15u8 {
        let mode = sub << 4;
        let bc = [0x4E, 0, mode, 0, 0, 0, 0, 0, 0];
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(
            r,
            StepResult::Advance { next_pc: 7 },
            "sub-op {sub}: expected PC += 7"
        );
    }
}

#[test]
fn op_4e_sub_a_compares_party_bank() {
    // sub-op 10 (party-bank A). state=1000, scaled-from-operands=
    // (0x0064 | (0x0000 << 16)) = 100. mode=0 (state < scaled): false ->
    // PC += 9.
    let mut host = TestHost::default();
    host.party_bank.insert(10, 1000);
    let mut ctx = FieldCtx::default();
    let bc = [0x4E, 0, 0xA0, 0x64, 0x00, 0x42, 0x00, 0x00, 0x00];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 9 });
}

#[test]
fn op_4e_sub_a_compare_taken() {
    // sub-op 10. state=50, scaled = 0x0064 = 100. mode=0 (state < scaled):
    // 50 < 100 = true -> jump pc + 1 + 4 + delta = 5 + 0x10 = 21.
    let mut host = TestHost::default();
    host.party_bank.insert(10, 50);
    let mut ctx = FieldCtx::default();
    let bc = [0x4E, 0, 0xA0, 0x64, 0x00, 0x10, 0x00, 0x00, 0x00];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 21 });
}

#[test]
fn op_4e_sub_4_compares_bios_rand_low_byte() {
    // Sub-op 4 calls BIOS Rand (FUN_80056798), masks the result to
    // `& 0xFF` (raw loader 0x801E0AFC: `andi s1, v0, 0xff` then
    // `j 0x801e0b3c` into the shared compare) - a random-chance branch,
    // not a jump-to-rand PC. rand = 0x142 -> state = 0x42 = 66.
    // mode 0 (state < arg): 66 < 100 -> taken, jump pc + 5 + 0x10 = 21.
    let mut host = TestHost {
        op4e_sub4_bios_rand_value: 0x142,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let bc = [0x4E, 0, 0x40, 0x64, 0x00, 0x10, 0x00];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 21 });
    assert_eq!(host.op4e_sub4_bios_rand_calls, 1);
    // rand low byte 200: 200 < 100 false -> PC += 7.
    let mut host = TestHost {
        op4e_sub4_bios_rand_value: 200,
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
}

#[test]
fn op_4e_sub_b_uses_high_word() {
    // sub-op 11. scaled = 0x0001 | (0x0001 << 16) = 0x10001. state =
    // 0x10000. mode=1 (scaled < state): 0x10001 < 0x10000 = false ->
    // PC += 9.
    let mut host = TestHost::default();
    host.party_bank.insert(11, 0x10000);
    let mut ctx = FieldCtx::default();
    let bc = [0x4E, 0, 0xB1, 0x01, 0x00, 0, 0, 0x01, 0x00];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 9 });
}

// -- 0x49 STATE_RESUME ---------------------------------------------

#[test]
fn op_49_idle_arms_and_halts() {
    let mut host = TestHost {
        op49_state_value: Op49State::Idle,
        ..Default::default()
    };
    let mut ctx = FieldCtx {
        field_90: 0xDEAD_BEEF,
        ..Default::default()
    };
    // Sub-op 1: captures field_90 into the arm record.
    let r = step(&mut host, &mut ctx, &[0x49, 1, 0, 0, 0], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(host.op49_arms, vec![(0, 0xDEAD_BEEF)]);
    assert_eq!(host.op49_setups, 1);
}

#[test]
fn op_49_idle_invalid_subop_halts_without_arming() {
    let mut host = TestHost {
        op49_state_value: Op49State::Idle,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    // sub-op 0xE is out of range (> 0xD).
    let r = step(&mut host, &mut ctx, &[0x49, 0xE, 0, 0, 0], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert!(host.op49_arms.is_empty());
    assert_eq!(host.op49_setups, 0);
}

#[test]
fn op_49_armed_halts() {
    let mut host = TestHost {
        op49_state_value: Op49State::Armed,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x49, 1, 0, 0, 0], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert!(host.op49_arms.is_empty());
}

#[test]
fn op_49_done_advances_per_subop() {
    // Sub-op 1, 3, 7 jump to 0x801e00b8 in the original, where
    // `addiu s8, s8, 0x3` runs before the dispatcher tail; PC += 3.
    let mut host = TestHost {
        op49_state_value: Op49State::Done,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x49, 1, 0, 0, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(host.op49_clears, 1);

    // Sub-op 2, 4: original returns `param_2 + 7`; PC += 7.
    host.op49_state_value = Op49State::Done;
    let r = step(&mut host, &mut ctx, &[0x49, 2, 0, 0, 0, 0, 0, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 7 });

    // Sub-op 5: original returns `param_2 + 0xe`; PC += 14.
    host.op49_state_value = Op49State::Done;
    let r = step(
        &mut host,
        &mut ctx,
        &[0x49, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 14 });
}

#[test]
fn op_49_done_subop_a_halts_at_pc() {
    // Done-side catch-all in `FUN_801de840 case 0x49`: any sub-op that
    // isn't explicitly handled (here sub-0xA, which falls through the
    // 1/3/7, 2/4, 5, 6/8/9/C/D arms) clears the resume slot and returns
    // `param_2` (= halt at the same PC).
    let mut host = TestHost {
        op49_state_value: Op49State::Done,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x49, 0xA, 0, 0, 0], 0);
    assert!(matches!(r, StepResult::Halt { final_pc: 0 }));
    assert_eq!(host.op49_clears, 1, "should clear the resume slot");
}

#[test]
fn op_49_done_sub0_walks_inline_mes_payload() {
    // `[49, 0, length, ...length args..., ...mes_bytes, terminator, ...]`.
    // The original at FUN_801DE840 (case 0x49 / DONE / sub-0) reads
    // `length = pbVar47[2]`, then walks the inline MES bytecode starting
    // at `pbVar47 + length + 3` via `func_0x8003ca38` (counts bytes > 0x1E,
    // pair-extends 0xCx prefix bytes), and advances PC by `5 + length +
    // walked`.
    let mut host = TestHost {
        op49_state_value: Op49State::Done,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    // length = 2 (two args), MES bytes = [0x50, 0xC3, 0xAA, 0x00 terminator]
    // → walker counts 4 bytes (0x50 = 1, 0xC3 0xAA pair = 2, then sees 0x00).
    let bc = &[
        0x49, // opcode
        0x00, // sub-op
        0x02, // length
        0xAA, 0xBB, // 2 args (ignored by walker - walker starts past them)
        0x50, 0xC3, 0xAA, 0x00, // MES body (3 bytes walked) + terminator
    ];
    let r = step(&mut host, &mut ctx, bc, 0);
    // PC = 0 + (header_size=1) + 4 + length=2 + walked=3 = 10. The
    // terminator is NOT consumed by the walker - it stops at it.
    assert_eq!(r, StepResult::Advance { next_pc: 10 });
    assert_eq!(host.op49_clears, 1);
}

#[test]
fn op_49_done_sub0_empty_mes_advances_5_plus_length() {
    // length = 0, no MES body, immediate terminator.
    let mut host = TestHost {
        op49_state_value: Op49State::Done,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let bc = &[0x49, 0x00, 0x00, 0x00];
    let r = step(&mut host, &mut ctx, bc, 0);
    // PC = 1 + 4 + 0 + 0 = 5.
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
}

#[test]
fn walk_mes_bytecode_terminates_on_low_byte() {
    // Standard byte run terminates on a byte ≤ 0x1E.
    assert_eq!(walk_mes_bytecode(&[0x50, 0x60, 0x70, 0x10, 0x80]), 3);
    // Empty buffer.
    assert_eq!(walk_mes_bytecode(&[]), 0);
    // Immediate terminator.
    assert_eq!(walk_mes_bytecode(&[0x05]), 0);
}

#[test]
fn walk_mes_bytecode_pair_extends_cx_prefix() {
    // 0xC0..0xCF prefix bytes consume one extra byte each (the pair byte).
    // 0xC0 0x05 (pair 0x05 NOT a terminator, because it's a pair byte not
    // a top-level byte) - counts 2.
    assert_eq!(walk_mes_bytecode(&[0xC0, 0x05, 0x10]), 2);
    // 0xCF 0x99 - counts 2; then 0x50 - counts 3 total; then 0x10 stops.
    assert_eq!(walk_mes_bytecode(&[0xCF, 0x99, 0x50, 0x10]), 3);
}

#[test]
fn walk_mes_bytecode_eof_mid_pair_stops_gracefully() {
    // 0xC0 at end of buffer - original would read past EOF; we stop.
    assert_eq!(walk_mes_bytecode(&[0xC0]), 1);
}

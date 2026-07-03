//! Field action opcodes: EXEC_MOVE/MOVE_TO, SCENE_CHANGE, BGM, CAM_CFG, item/money/party, COND_JMP, WARP/INTERACT, RENDER_CFG, COUNTER, ANIMATE. Extracted verbatim from `field/tests.rs`.

use super::*;

// -- 0x22 EXEC_MOVE --------------------------------------------------

#[test]
fn exec_move_writes_state_and_calls_host() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        script_id: 3,
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0x22, 0x05], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.move_id, 5);
    assert_eq!(ctx.field_5e, 0xFFFE);
    assert_eq!(ctx.move_substate, 1);
    assert_eq!(host.exec_moves, vec![(3u16, 5u8)]);
}

#[test]
fn exec_move_zero_sets_substate_5() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x22, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.move_substate, 5);
}

#[test]
fn exec_move_extended_threads_target_id() {
    // Extended dispatch on a non-halted target.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        script_id: 0x42,
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0xA2, 0x42, 0x07], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(host.exec_moves, vec![(0x42u16, 7u8)]);
}

// -- 0x23 MOVE_TO ----------------------------------------------------

#[test]
fn move_to_decodes_grid_coords_no_high_bit() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        script_id: 4,
        ..Default::default()
    };
    // x=2, z=3, neither has high bit -> world_x = 2*0x80 + 0x40 = 0x140,
    // world_z = 3*0x80 + 0x40 = 0x1C0.
    let r = step(&mut host, &mut ctx, &[0x23, 0x02, 0x03], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(ctx.world_x, 0x0140);
    assert_eq!(ctx.world_z, 0x01C0);
    assert_eq!(ctx.npc_x, 0x02);
    assert_eq!(ctx.npc_facing, 0x03);
    assert_eq!(host.move_tos, vec![(4u16, 0x0140u16, 0x01C0u16, false)]);
}

#[test]
fn move_to_high_bit_adds_offset() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // x = 0x82 (low bits 2, high bit set) -> 2*0x80 + 0x40 + 0x40 = 0x180.
    let r = step(&mut host, &mut ctx, &[0x23, 0x82, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(ctx.world_x, 0x0180);
    assert_eq!(ctx.world_z, 0x0040); // 0*0x80 + 0x40
}

#[test]
fn move_to_player_path_uses_flag_bit() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        flags: 0x1000000, // player chain bit
        ..Default::default()
    };
    step(&mut host, &mut ctx, &[0x23, 0x01, 0x01], 0);
    assert_eq!(host.move_tos.len(), 1);
    assert!(host.move_tos[0].3); // is_player == true
}

// -- 0x3F SCENE_CHANGE (named warp) ----------------------------------

#[test]
fn scene_change_decodes_name_and_entry() {
    // idx = 0x0042, name_len = 4 ("dolk"), entry_x = 1, entry_z = 2, dir = 3.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let bc = [
        0x3F, 0x42, 0x00, // opcode + idx (LE)
        0x04, b'd', b'o', b'l', b'k', // name_len + name
        0x01, 0x02, 0x03, // entry_x, entry_z, dir
    ];
    let r = step(&mut host, &mut ctx, &bc, 0);
    // header_size 1 + 3 (idx,len) + 4 (name) + 3 (entry) = 11.
    assert_eq!(r, StepResult::Advance { next_pc: 11 });
    assert_eq!(
        host.named_scene_transitions,
        vec![("dolk".to_string(), 0x01, 0x02, 0x03)]
    );
    // It is not a dialog opener.
    assert!(host.dialogs.is_empty());
}

#[test]
fn scene_change_phantom_name_stages_no_transition() {
    // A 0x3F whose "name" is uppercase/punctuation (a literal '?' inside text)
    // fails the clean-CDNAME gate: no transition, but the PC still advances.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let bc = [
        0x3F, 0x10, 0x00, 0x04, b'H', b'i', b'!', b' ', // idx + len + "Hi! "
        0x00, 0x00, 0x00, // entry_x, entry_z, dir
    ];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 11 });
    assert!(host.named_scene_transitions.is_empty());
}

#[test]
fn scene_change_empty_name_no_transition() {
    // name_len = 0 -> empty name -> no transition; advances header + 6.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let bc = [
        0x3F, 0x10, 0x00, 0x00, // opcode + idx + name_len=0
        0x00, 0x00, 0x00, // entry_x, entry_z, dir
    ];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
    assert!(host.named_scene_transitions.is_empty());
}

#[test]
fn scene_change_truncated_buffer_returns_unknown() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // name_len = 10 but bytecode only has 5 trailing bytes - should error.
    let bc = [0x3F, 0x00, 0x00, 0x0A, 0x01, 0x02, 0x03, 0x04, 0x05];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x3F,
            pc: 0
        }
    ));
    assert!(host.named_scene_transitions.is_empty());
}

// -- 0x35 BGM --------------------------------------------------------

#[test]
fn bgm_decodes_text_id_and_sub_op() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x35, 0x12, 0x00, 0x01], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    assert_eq!(host.bgm_calls, vec![(0x12u16, 1u8)]);
}

// -- 0x38 CAM_CFG ----------------------------------------------------

#[test]
fn cam_cfg_simple_path_writes_field_26_from_table() {
    let mut host = TestHost {
        cam_cfg_table: vec![0xAA, 0xBB, 0xCC, 0xDD],
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    // op0 = 2 (low nibble), op1 = 0 (& 0x7F == 0 -> simple path).
    let r = step(&mut host, &mut ctx, &[0x38, 0x02, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(ctx.field_26, 0xCC);
}

#[test]
fn cam_cfg_halt_acquire_succeeds_and_yields() {
    // op1 != 0 → halt-acquire path. With the predicate set to true (the
    // default test impl), the VM marks ctx halted (saved_pc + wait_accum
    // + flag 0x400) and yields with `resume_pc = pc + 3`.
    let mut host = TestHost {
        halt_acquire_predicate: true,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x38, 0x05, 0x01], 0);
    assert_eq!(r, StepResult::Yield { resume_pc: 3 });
    assert!(ctx.is_halted());
    assert_eq!(ctx.flags & 0x400, 0x400);
    assert_eq!(ctx.saved_pc, 0);
    assert_eq!(ctx.wait_accum, 0);
    assert_eq!(host.halt_acquire_calls, vec![(0x38u8, 3usize, [0i16; 3])]);
}

#[test]
fn cam_cfg_halt_acquire_failed_predicate_halts_at_pc() {
    // op1 != 0 + predicate false → original falls into the dispatcher
    // default-arm path; for op 0x38 (not a 0x50/0x60/0x70 opcode) that
    // halts the VM at the current PC.
    let mut host = TestHost {
        halt_acquire_predicate: false,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x38, 0x00, 0x01], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert!(!ctx.is_halted());
    assert_eq!(host.halt_acquire_calls, vec![]);
}

// -- 0x39 GIVE_ITEM --------------------------------------------------

#[test]
fn give_item_calls_host() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x39, 0x42], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.give_item_calls, vec![0x42]);
}

// -- 0x3A ADD_MONEY --------------------------------------------------

#[test]
fn add_money_positive_24_bit() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 0x000100 = +256.
    let r = step(&mut host, &mut ctx, &[0x3A, 0x00, 0x01, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    assert_eq!(host.money_deltas, vec![256]);
}

#[test]
fn add_money_negative_sign_extended() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 0xFFFFFF = -1 in 24-bit two's complement.
    let r = step(&mut host, &mut ctx, &[0x3A, 0xFF, 0xFF, 0xFF], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    assert_eq!(host.money_deltas, vec![-1]);
}

#[test]
fn add_money_min_negative() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 0x800000 = -8388608 (24-bit min signed).
    let r = step(&mut host, &mut ctx, &[0x3A, 0x00, 0x00, 0x80], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    assert_eq!(host.money_deltas, vec![-8388608]);
}

// -- 0x3B SET_ITEM_COUNT --------------------------------------------

#[test]
fn set_item_count_passes_raw_slot_and_count() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x3B, 0x23, 0x05], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(host.item_writes, vec![(0x23u8, 0x05u8)]);
}

// -- 0x3C / 0x3D party ----------------------------------------------

#[test]
fn party_add_and_remove_route_to_host() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x3C, 0x02], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    let r = step(&mut host, &mut ctx, &[0x3D, 0x02], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.party_added, vec![0x02]);
    assert_eq!(host.party_removed, vec![0x02]);
}

// -- 0x42 COND_JMP --------------------------------------------------

#[test]
fn cond_jmp_mode_0_skips_when_flag_clear() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // mode=0, bit=3, lo=0xFF, hi=0xFF - flag clear -> skip 5 bytes.
    let r = step(&mut host, &mut ctx, &[0x42, 0x00, 0x03, 0xFF, 0xFF], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
}

#[test]
fn cond_jmp_mode_0_jumps_when_flag_set() {
    let mut host = TestHost {
        extras: 1u32 << 3,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    // mode=0, bit=3, delta=0x10. Jump target = pc + 3 + 0x10 = 19. The
    // original computes `iVar18 = param_2 + 3; return iVar18 + delta`.
    let r = step(&mut host, &mut ctx, &[0x42, 0x00, 0x03, 0x10, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 19 });
}

#[test]
fn cond_jmp_mode_1_table_match_jumps() {
    // mode=1, op1=3 (< 8 → table lookup path), table[3] = high-nibble
    // value of screen_mode. delta = 0x20.
    let mut host = TestHost {
        screen_mode: 0x4000,
        screen_mode_table: {
            let mut v = vec![None; 8];
            v[3] = Some(0x4000);
            v
        },
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x42, 0x01, 0x03, 0x20, 0x00], 0);
    // Take jump: pc + 3 + 0x20 = 35.
    assert_eq!(r, StepResult::Advance { next_pc: 35 });
}

#[test]
fn cond_jmp_mode_1_table_mismatch_skips() {
    // Same as above but screen_mode high nibble doesn't match table[3].
    let mut host = TestHost {
        screen_mode: 0x1000,
        screen_mode_table: {
            let mut v = vec![None; 8];
            v[3] = Some(0x4000);
            v
        },
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x42, 0x01, 0x03, 0x20, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
}

#[test]
fn cond_jmp_mode_1_bit_test_path() {
    // op1 = 8 → tests screen_mode bit 0x20.
    let mut host = TestHost {
        screen_mode: 0x20,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    // Bit set → take jump.
    let r = step(&mut host, &mut ctx, &[0x42, 0x01, 0x08, 0x10, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 19 });

    // Bit clear → skip.
    host.screen_mode = 0;
    let r = step(&mut host, &mut ctx, &[0x42, 0x01, 0x08, 0x10, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
}

#[test]
fn cond_jmp_mode_2_halts_at_pc() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // Mode >= 2 hits the dispatcher's default arm, which returns
    // `param_2` for opcodes whose high nibble isn't 0x5x/0x6x/0x7x.
    // 0x42 & 0x70 = 0x40 → halt at PC.
    let r = step(&mut host, &mut ctx, &[0x42, 0x02, 0x00, 0x00, 0x00], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}

#[test]
fn cond_jmp_mode_1_op1_at_least_c_takes_jump() {
    // Mode 1 op1 >= 0xC: original at line 5176 of the dump falls
    // through every `if (uVar31 == N) ... return iVar18` branch and
    // ends up in the unconditional take-jump path with
    // `iVar18 = param_2 + 3` and `delta = LE_u16(operand[2..4])`.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // [0x42, 0x01, 0x0C, 0x10, 0x00] → mode 1, op1=0xC, delta=0x0010.
    // Expected next_pc = 0 + 3 + 0x10 = 19 (header_size=2, +1 for mode,
    // +0x10 delta - 3 + delta).
    let r = step(&mut host, &mut ctx, &[0x42, 0x01, 0x0C, 0x10, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 19 });
}

#[test]
fn op_34_sub_f_halts_at_pc() {
    // Top of the 4-bit sub-op range. Same dispatch path as sub-4.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x34, 0xF0, 0], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}

#[test]
fn op_43_subop_ff_halts_at_pc() {
    // 0xFF is the sentinel "uninitialised bytecode" byte. 0x43 sub-op
    // 0xFF has no original handler ⇒ halt at PC.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(
        &mut host,
        &mut ctx,
        &[0x43, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        0,
    );
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}

#[test]
fn op_4c_n4_sub_e_or_f_halts_at_pc() {
    // 0x4C outer-nibble 4 inner switch (line 5901 of the dump) has cases
    // 0..=0xD followed by an explicit `default:` that prints
    // `SUB_40_ERROR` and routes via `switchD_801e00f4::default()` ⇒
    // halt at PC for sub-ops 0xE/0xF. The instruction is 6 bytes.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x4E, 0x00, 0x00, 0x00, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x4F, 0x00, 0x00, 0x00, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}

// -- 0x3E WARP / INTERACT -------------------------------------------

#[test]
fn warp_interact_path_advances_3() {
    // op0 = 5 (< 100) → INTERACT path.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x3E, 0x05, 0x02], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(host.interacts, vec![(0x05u8, 0x02u8)]);
    assert!(host.scene_transitions.is_empty());
}

#[test]
fn warp_interact_handles_0xff_sentinel() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x3E, 0xFF, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(host.interacts, vec![(0xFFu8, 0x00u8)]);
}

#[test]
fn warp_scene_transition_path_advances_6_and_clears_flag() {
    // op0 = 105 (>= 100) → WARP. map_id = 5.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        flags: 0xFFFF_FFFF,
        ..Default::default()
    };
    let bc = [0x3E, 105, 0, 0, 0, 0];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.scene_transitions, vec![5u8]);
    // Bit 0x80000 cleared on the active ctx.
    assert_eq!(ctx.flags & 0x80000, 0);
    // Other bits preserved.
    assert_eq!(ctx.flags & 0x40000, 0x40000);
}

// -- 0x46 RENDER_CFG ------------------------------------------------

#[test]
fn render_cfg_long_form_advances_6() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(
        &mut host,
        &mut ctx,
        &[0x46, 0x24, 0x11, 0x22, 0x33, 0x44],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.render_long, vec![(0x11u8, 0x22u8, 0x33u8, 0x44u8)]);
    assert!(host.render_short.is_empty());
}

#[test]
fn render_cfg_short_form_advances_3_and_computes_bitfield() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // op0 = 0x10, op1 = 0x06.
    // r = !(0x10 >> 1) & 0xFF = !0x08 & 0xFF = 0xF7
    // g = 2 - (0x06 >> 1) = 2 - 3 = 0xFF (wrap)
    // b = (0x10 >> 1) - 1 = 0x07
    // packed = (0x06 >> 1) + 2 = 5
    let r = step(&mut host, &mut ctx, &[0x46, 0x10, 0x06], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(host.render_short, vec![(0xF7u8, 0xFFu8, 0x07u8, 0x05u8)]);
    assert!(host.render_long.is_empty());
}

// -- 0x4F SCENE_REGISTER_WRITE --------------------------------------

#[test]
fn scene_register_write_passes_three_bytes() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4F, 0xAA, 0xBB, 0xCC], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    assert_eq!(host.scene_regs, vec![(0xAAu8, 0xBBu8, 0xCCu8)]);
}

// -- 0x44 COUNTER ----------------------------------------------------

#[test]
fn counter_advances_and_calls_host() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x44, 0x55], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.counter_calls, vec![0x55]);
}

// -- 0x4B ANIMATE ----------------------------------------------------

#[test]
fn animate_advances_3_plus_4_count_and_writes_flags() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        script_id: 7,
        local_flags: 0xFFFF, // all bits set; check mask
        ..Default::default()
    };
    // count = 2, base_id = 5, 8 keyframe bytes.
    let bc = [0x4B, 2, 5, 0x10, 0x11, 0x12, 0x13, 0x20, 0x21, 0x22, 0x23];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 11 });
    assert_eq!(ctx.flags & 0x1000, 0x1000);
    // 0xD3FF = 0b1101001111111111 keeps bits 12, 6-9, 0-5; clears 10, 11, 13, 14.
    assert_eq!(ctx.local_flags & 0x1000, 0x1000);
    assert_eq!(ctx.local_flags & 0x0C00, 0); // bits 10, 11 cleared by mask
    assert_eq!(ctx.local_flags & 0x2000, 0); // bit 13 cleared
    assert_eq!(ctx.face_rotation, 2);
    assert_eq!(host.animations.len(), 1);
    assert_eq!(host.animations[0].0, 7);
    assert_eq!(host.animations[0].1, 2);
    assert_eq!(host.animations[0].2, 5);
    assert_eq!(
        host.animations[0].3,
        vec![0x10, 0x11, 0x12, 0x13, 0x20, 0x21, 0x22, 0x23]
    );
}

#[test]
fn animate_zero_count_advances_3() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4B, 0, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(host.animations.len(), 1);
    assert!(host.animations[0].3.is_empty());
}

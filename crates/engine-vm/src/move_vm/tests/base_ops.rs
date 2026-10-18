//! The base opcodes: decode, world writes, halt / wait, flags, key buffers, morph slots, emitter and keyframe setup.

use super::*;

#[test]
fn opcode_decode_round_trip_in_bound() {
    for v in 0u16..=0x46 {
        let op = MoveOpcode::from_u16(v).expect("must decode");
        assert_eq!(op as u16, v);
    }
    assert!(MoveOpcode::from_u16(0x47).is_none());
    assert!(MoveOpcode::from_u16(0xFFFF).is_none());
}

#[test]
fn op00_anim_bank_set_writes_shifted_triple_and_advances_pc_by_4() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    let bc = program(&[0x00, 1, 2, 3]);
    let r = step(&mut host, &mut state, &bc);
    assert_eq!(r, StepResult::Advance);
    assert_eq!(state.anim_3c, 1 << 3);
    assert_eq!(state.anim_3e, 2 << 3);
    assert_eq!(state.anim_40, 3 << 3);
    assert_eq!(state.pc, 4);
}

#[test]
fn op01_world_add_advances_y_mirror_too() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.world_x = 100;
    state.world_y = 50;
    state.world_y_mirror = 50;
    state.world_z = 25;
    let bc = program(&[0x01, 5, 10, 15]);
    let r = step(&mut host, &mut state, &bc);
    assert_eq!(r, StepResult::Advance);
    assert_eq!(state.world_x, 105);
    assert_eq!(state.world_y, 60);
    assert_eq!(state.world_y_mirror, 60);
    assert_eq!(state.world_z, 40);
    assert_eq!(state.pc, 4);
}

#[test]
fn op07_world_set_overrides_y_mirror() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.world_y = 999;
    state.world_y_mirror = 999;
    let bc = program(&[0x07, 11, 22, 33]);
    let r = step(&mut host, &mut state, &bc);
    assert_eq!(r, StepResult::Advance);
    assert_eq!(state.world_x, 11);
    assert_eq!(state.world_y, 22);
    assert_eq!(state.world_y_mirror, 22);
    assert_eq!(state.world_z, 33);
}

#[test]
fn op08_halt_sets_flag_8_and_breaks_loop() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    let bc = program(&[0x08]);
    let r = step(&mut host, &mut state, &bc);
    assert_eq!(r, StepResult::Halt);
    assert_eq!(state.flags & 0x8, 0x8);
    // Halt does not advance PC (size=0).
    assert_eq!(state.pc, 0);
}

#[test]
fn op09_wait_set_seeds_timer_and_yields() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    let bc = program(&[0x09, 5]);
    let r = step(&mut host, &mut state, &bc);
    assert_eq!(r, StepResult::Wait);
    assert_eq!(state.wait_timer, 5 << 3);
    assert_eq!(state.pc, 2);
}

#[test]
fn op16_stub_calls_host_and_advances() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    let bc = program(&[0x16, 0x42]);
    let r = step(&mut host, &mut state, &bc);
    assert_eq!(r, StepResult::Advance);
    assert_eq!(host.stub16_calls, vec![0x42]);
    assert_eq!(state.pc, 2);
}

#[test]
fn op21_face_rotation_writes_index_and_calls_host() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    let bc = program(&[0x21, 7, 100, 200, 300, 400, 0x8000]);
    let r = step(&mut host, &mut state, &bc);
    assert_eq!(r, StepResult::Advance);
    assert_eq!(state.face_rotation, 7);
    assert_eq!(host.face_rot_calls.len(), 1);
    let (face_id, params, target) = &host.face_rot_calls[0];
    assert_eq!(*face_id, 7);
    assert_eq!(*params, [100, 200, 300, 400]);
    // 0x8000 sign-extended to i32 is -32768.
    assert_eq!(*target, -32768);
    assert_eq!(state.pc, 7);
}

#[test]
fn op2c_alloc_path_calls_host_when_w_ge_0x11() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    // [op, count_x, count_y, w, h] - w=0x11, h=4, bytes = 0x11*4*2 = 136.
    let bc = program(&[0x2C, 1, 2, 0x11, 4]);
    let r = step(&mut host, &mut state, &bc);
    assert_eq!(r, StepResult::Advance);
    assert_eq!(host.keyframe_allocs, vec![136]);
    assert_eq!(host.keyframe_inits, 1);
    assert_eq!(state.field_a8, 0xC0DE_0000u32 as i32);
    assert_eq!(state.field_9c, 1);
    assert_eq!(state.pc, 5);
}

#[test]
fn op2c_inline_path_skips_alloc_when_w_lt_0x11() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    let bc = program(&[0x2C, 1, 2, 4, 4]);
    let r = step(&mut host, &mut state, &bc);
    assert_eq!(r, StepResult::Advance);
    assert!(host.keyframe_allocs.is_empty());
    assert_eq!(host.keyframe_inits, 1);
    assert_eq!(state.field_9c, 1);
}

#[test]
fn op30_key_buffer_free_clears_field_9c() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.field_9c = 1;
    state.field_a8 = 0xDEAD_BEEFu32 as i32;
    let bc = program(&[0x30]);
    let r = step(&mut host, &mut state, &bc);
    assert_eq!(r, StepResult::Advance);
    assert_eq!(host.keyframe_frees, 1);
    assert_eq!(state.field_9c, 0);
    assert_eq!(state.pc, 1);
}

#[test]
fn op31_lflag_and_with_op32_lflag_or_round_trip() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.local_flags = 0xFFFF;
    // AND with 0x000F → low nibble only.
    let bc = program(&[0x31, 0x000F]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.local_flags, 0x000F);
    // OR with 0xFF00 → restore high byte.
    state.pc = 0;
    let bc2 = program(&[0x32, 0xFF00]);
    step(&mut host, &mut state, &bc2);
    assert_eq!(state.local_flags, 0xFF0F);
}

#[test]
fn op33_clear_bit_40000000_only_touches_that_bit() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.field_74 = 0xFFFF_FFFF;
    let bc = program(&[0x33]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.field_74, 0xBFFF_FFFF);
    assert_eq!(state.pc, 1);
}

#[test]
fn op3a_and_op3b_toggle_flag_2() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    let bc = program(&[0x3A]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.flags & 2, 2);
    state.pc = 0;
    let bc2 = program(&[0x3B]);
    step(&mut host, &mut state, &bc2);
    assert_eq!(state.flags & 2, 0);
}

#[test]
fn out_of_range_opcode_is_end_of_buffer() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    let bc = program(&[0x47]);
    let r = step(&mut host, &mut state, &bc);
    assert!(matches!(r, StepResult::EndOfBuffer { opcode: 0x47 }));
}

#[test]
fn run_until_break_steps_through_until_halt() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    // Program: WORLD_SET (4), WRITE_26 (2), HALT (0). Total 3 ticks.
    let bc = program(&[
        0x07, 1, 2, 3, // WORLD_SET
        0x06, 99,   // WRITE_26
        0x08, // HALT
    ]);
    let r = run_until_break(&mut host, &mut state, &bc, 16);
    assert_eq!(r, StepResult::Halt);
    assert_eq!(state.world_x, 1);
    assert_eq!(state.render_26, 99);
    assert_eq!(state.flags & 0x8, 0x8);
    assert_eq!(state.pc, 6); // 4 + 2 + 0 (halt doesn't advance).
}

#[test]
fn op03_world_rotate_add_uses_host_lut() {
    let mut host = TestHost::default();
    // Pretend rotation index 0 has sin = 0x1000, cos = 0x0800.
    host.rotation_table.insert(0, (0x1000, 0x0800));
    let mut state = ActorState::new();
    state.tween_scale_x = 0; // index = 0
    let bc = program(&[0x03, 100]);
    step(&mut host, &mut state, &bc);
    // dx = (0x1000 * 100) >> 12 = 100.
    assert_eq!(state.world_x, 100);
    // dz = (0x0800 * 100) >> 12 = 50.
    assert_eq!(state.world_z, 50);
    assert_eq!(state.pc, 2);
}

/// Composed bytecode walks several explicit-size opcodes - WORLD_SET,
/// FACE_ROTATION, ext sub 0x1C (set-flag-bank, size 3), then HALT.
/// `run_until_break` should exit at HALT with all per-op state changes
/// applied. Avoids the default-arm sub-ops whose `size_u16 = 1`
/// semantics leave PC pointing at the sub-op byte (which would then be
/// re-interpreted as a new opcode). Mirrors the field VM's integration
/// style but exercises the move-VM dispatch table.
#[test]
fn run_until_break_walks_explicit_size_opcodes_then_halts() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();

    // Bytecode:
    // op 0x07 1 2 3                       - WORLD_SET (size 4)
    // op 0x21 7 100 200 300 400 0x8000   - face rotation (size 7)
    // op 0x2F sub 0x1C 42                 - ext set_flag_bank (size 3)
    // op 0x08                              - HALT (size 0)
    let bc = program(&[
        0x07, 1, 2, 3, // WORLD_SET
        0x21, 7, 100, 200, 300, 400, 0x8000, // FACE_ROT
        0x2F, 0x1C, 42,   // ext set_flag_bank(42), size 3
        0x08, // HALT
    ]);

    let r = run_until_break(&mut host, &mut state, &bc, 64);
    assert_eq!(r, StepResult::Halt);

    // World coords from WORLD_SET.
    assert_eq!(state.world_x, 1);
    assert_eq!(state.world_y, 2);
    assert_eq!(state.world_z, 3);
    // Face rotation index recorded.
    assert_eq!(state.face_rotation, 7);
    assert_eq!(host.face_rot_calls.len(), 1);
    // Ext set_flag_bank captured the index.
    assert_eq!(host.ext_set_flag_bank_calls, vec![42]);
    // HALT bit set.
    assert_eq!(state.flags & 0x8, 0x8);
    // PC stops at the HALT word (size 0). 4 + 7 + 3 = 14.
    assert_eq!(state.pc, 14);
}

#[test]
fn op0a_packs_slot_indices_one_byte_apart_and_keeps_curves_per_slot() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    // [0x0A, op1, count=3, (idx, up, down) x 3]
    let bc = program(&[0x0A, 1, 3, 0x11, 10, 20, 0x22, 11, 21, 0x33, 12, 22]);
    let r = step(&mut host, &mut state, &bc);
    assert_eq!(r, StepResult::Advance);
    assert_eq!(state.pc, 3 + 3 * 3);
    assert_eq!(state.keyframe_count, 3);
    assert_eq!(state.flags & 0x1000, 0x1000);

    // `+0xB0 + i` - a BYTE per slot, so the three indices land one
    // u16 slot apart in the modelled window, not two.
    assert_eq!(state.anim_block_u8(0x04), 0x11);
    assert_eq!(state.anim_block_u8(0x05), 0x22);
    assert_eq!(state.anim_block_u8(0x06), 0x33);

    // `+0xB8 + i*2` / `+0xC8 + i*2` - both curves are per slot. The
    // third slot's down curve lands at byte offset 0x20, past the old
    // 16-slot window that silently dropped it.
    let mul = i32::from(host.keyframe_curve_multiplier());
    for (i, (up, down)) in [(10i32, 20i32), (11, 21), (12, 22)].iter().enumerate() {
        assert_eq!(state.anim_block_u16(0x0C + i * 2), ((up * mul) >> 3) as u16);
        assert_eq!(
            state.anim_block_u16(0x1C + i * 2),
            ((down * mul) >> 3) as u16
        );
    }
}

#[test]
fn op0a_reset_arm_zeroes_the_morph_weights_not_the_anim_block_head() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    // Pre-load the `+0xA0` weight window (which the retail record shares
    // with the op-0x2C keyframe descriptor) and the `+0x7C` lane mask.
    state.keyframe_desc = [0x1000, 0x1000, 0x1000, 0x1000];
    state.field_7c = 0x8000_0003;
    state.anim_block_u16_set(0x00, 0xBEEF);

    // `op[1] == 0` arms the reset for each of the two slots.
    let bc = program(&[0x0A, 0, 2, 0x01, 1, 2, 0x02, 3, 4]);
    let r = step(&mut host, &mut state, &bc);
    assert_eq!(r, StepResult::Advance);

    // Slots 0 and 1 -> `+0xA0` and `+0xA2`; slots 2 and 3 untouched.
    assert_eq!(state.keyframe_desc, [0, 0, 0x1000, 0x1000]);
    assert_eq!(state.field_7c, 0);
    // `+0xAC` is a different field entirely and must survive.
    assert_eq!(state.anim_block_u16(0x00), 0xBEEF);
}

#[test]
fn op0a_without_the_reset_arm_leaves_the_weights_alone() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.keyframe_desc = [0x1000; 4];
    state.field_7c = 0x8000_0003;
    let bc = program(&[0x0A, 1, 2, 0x01, 1, 2, 0x02, 3, 4]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.keyframe_desc, [0x1000; 4]);
    assert_eq!(state.field_7c, 0x8000_0003);
}

#[test]
fn zero_keyframe_weight_walks_the_retail_field_overlap() {
    let mut state = ActorState::new();
    state.keyframe_desc = [1, 2, 3, 4];
    state.field_a8 = 0x1111_2222;
    state.anim_block_u16_set(0x00, 0x3333);

    // lanes 0..4 land in the op-0x2C descriptor at `+0xA0..+0xA8`.
    state.zero_keyframe_weight(2);
    assert_eq!(state.keyframe_desc, [1, 2, 0, 4]);
    // lane 4 -> `+0xA8` low half, lane 5 -> `+0xAA` high half.
    state.zero_keyframe_weight(4);
    assert_eq!(state.field_a8, 0x1111_0000u32 as i32);
    state.zero_keyframe_weight(5);
    assert_eq!(state.field_a8, 0);
    // lane 6 -> `+0xAC`, the head of the anim block.
    state.zero_keyframe_weight(6);
    assert_eq!(state.anim_block_u16(0x00), 0);
}

// ---- draw-kind-4 multi-target setup ops (`0x13` / `0x23` / `0x42`) --------
//
// Each test's expected offsets are read off the arm's own stores in
// `ghidra/scripts/funcs/80023070.txt` (`s1 = actor + 0x80`, so `sh v0,0x1c(s1)`
// is `+0x9C`). The earlier port routed these through the `+0xAC`-relative
// block with eight-bit-wrapped offsets and missed every field below `+0xAC`.

#[test]
fn op42_ribbon_setup_lands_every_field_the_emitter_reads() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    let bc = program(&[
        0x42, 0x0001, 9, 0x40, 0x30, 0x60, 0x3039, 0x20, 0x0123, // v1..v8
        0x10, 0x20, 0x30, // +0xA0 colour
        0x40, 0x50, 0x60, // +0xA4 colour
    ]);
    assert_eq!(step(&mut host, &mut state, &bc), StepResult::Advance);
    assert_eq!(state.pc, 0xF);
    assert_eq!(state.move_substate, 4, "+0x56 draw kind");
    assert_eq!(state.move_submode, 2, "+0x5A");
    assert_eq!(state.actor_u16(0x9E), 0x2001);
    assert_eq!(state.actor_u16(0x9C), 9);
    assert_eq!(state.actor_u16(0xC8), 0x40, "unshifted, unlike op 0x13");
    assert_eq!(state.actor_u16(0xB4), 0x30);
    assert_eq!(state.actor_u16(0xB6), 0x60);
    assert_eq!(state.actor_u16(0xB8), 0x3039);
    assert_eq!(state.actor_u16(0xBA), 0x20);
    assert_eq!(state.actor_u16(0xA8), 0x0123);
    assert_eq!(state.actor_u32(0xA0), 0x0030_2010);
    assert_eq!(state.actor_u32(0xA4), 0x0060_5040);
}

#[test]
fn op23_sprite_setup_writes_below_the_anim_block() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    let bc = program(&[0x23, 0x0002, 0x11, 0x22, 0x33, 5, 6, 7, 8, 9, 10, 11, 12]);
    assert_eq!(step(&mut host, &mut state, &bc), StepResult::Advance);
    assert_eq!(state.pc, 0xD);
    assert_eq!(state.actor_u16(0x9E), 0x4002);
    assert_eq!(state.actor_u32(0xA0), 0x0033_2211);
    for (off, want) in [
        (0xB4, 5),
        (0xB6, 6),
        (0xB0, 7),
        (0xB2, 8),
        (0xA8, 9),
        (0xAA, 10),
        (0xAC, 11),
        (0xAE, 12),
    ] {
        assert_eq!(state.actor_u16(off), want, "+{off:#X}");
    }
    // `+0xA8` / `+0xAA` are the halves of `field_a8`, which the strip slab
    // reader consumes as its `(u0, v0)` pair.
    assert_eq!(state.field_a8, (10 << 16) | 9);
}

#[test]
fn op13_default_emitter_setup_shifts_c8_and_fills_b4_to_be() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    let mut words = vec![0x13, 0x0000, 7, 2, 1, 2, 3, 4, 5, 6];
    words.extend(10u16..=15);
    let bc = program(&words);
    assert_eq!(step(&mut host, &mut state, &bc), StepResult::Advance);
    assert_eq!(state.pc, 0x10);
    assert_eq!(state.move_substate, 4);
    assert_eq!(state.actor_u16(0x9E), 0);
    assert_eq!(state.actor_u16(0x9C), 7);
    assert_eq!(state.actor_u16(0xC8), 16, "v3 << 3");
    assert_eq!(state.actor_u32(0xA0), 0x0003_0201);
    assert_eq!(state.actor_u32(0xA4), 0x0006_0504);
    for (i, off) in (0xB4..=0xBE).step_by(2).enumerate() {
        assert_eq!(state.actor_u16(off), 10 + i as u16);
    }
}

#[test]
fn packed_word_borrows_on_a_negative_operand() {
    // `lh` sign-extends, and the sum is an unmasked `addu` chain.
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    let bc = program(&[0x23, 0, 0xFFFF, 0x01, 0x00, 0, 0, 0, 0, 0, 0, 0, 0]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.actor_u32(0xA0), 0x0000_00FF);
}

#[test]
fn op24_and_op26_address_field_a8_not_a_phantom_block_slot() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    let bc = program(&[0x26, 1, 2, 3, 4]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.field_a8, (2 << 16) | 1);
    assert_eq!(state.anim_block_u16(0), 3);
    assert_eq!(state.anim_block_u16(2), 4);
    let mut state2 = state.clone();
    state2.pc = 0;
    let bc = program(&[0x24, 10, 20]);
    step(&mut host, &mut state2, &bc);
    assert_eq!(state2.field_a8, (22 << 16) | 11);
    assert_eq!(state2.anim_block_u16(0), 13);
    assert_eq!(state2.anim_block_u16(2), 24);
}

/// Op `0x34`'s arm (`0x80023B64`, `s4 = actor + 0x90`) stores its `lh`
/// operands as sign-extended words. The earlier port wrote op[6] into
/// `+0xA8`, op[7] into the block offset `0xF8` (`+0xA4` only after an
/// eight-bit wrap, so into a slot nothing reads), and dropped op[8] and the
/// sign halves altogether.
#[test]
fn op34_setup_stores_sign_extended_words_through_actor_plus_0x90() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    let bc = program(&[
        0x34, 0xFFFE, 0x1234, 0x0011, 0x0022, 0x8001, 0x0005, 0xFFF0, 0x0077,
    ]);
    assert_eq!(step(&mut host, &mut state, &bc), StepResult::Advance);
    assert_eq!(state.pc, 9);
    assert_eq!(state.actor_u32(0xAC), 0xFFFF_FFFE, "op[1] sign-extended");
    assert_eq!(state.actor_u16(0xB0), 0x1234);
    assert_eq!(state.tween_src_x, 0x11, "+0x90");
    assert_eq!(state.tween_src_y, 0x22, "+0x92");
    assert_eq!(state.actor_u16(0x9C), 0x8001);
    assert_eq!(state.actor_u16(0x9E), 0xFFFF, "word store's sign half");
    assert_eq!(state.actor_u32(0xA0), 5);
    assert_eq!(state.actor_u32(0xA4), 0xFFFF_FFF0);
    assert_eq!(state.field_a8, 0x77, "op[8], not op[6]");
}

/// `FUN_80021DF4`'s mode-2/6 channel block (`0x80021E78..0x80021FA0`): each
/// of `+0xB4..+0xBA` and `+0xC8` steps by its rate at `+0xC0..+0xC6` / `+0xCA`,
/// `(rate * delta) >> 6`, and `+0xC8` clamps at zero.
#[test]
fn draw_channels_integrate_in_modes_2_and_6_only() {
    let mut s = ActorState::new();
    s.set_actor_u16(0xCA, 0x40);
    s.set_actor_u16(0xC0, (-0x20i16) as u16);
    s.set_actor_u16(0xB4, 100);
    integrate_draw_channels(&mut s, 8);
    assert_eq!(s.actor_u16(0xC8), 0, "mode 0 does not integrate");
    s.move_submode = 2;
    integrate_draw_channels(&mut s, 8);
    assert_eq!(s.actor_u16(0xC8), 8, "0x40 * 8 >> 6");
    assert_eq!(s.actor_u16(0xB4), 96, "-0x20 * 8 >> 6 = -4");
    s.set_actor_u16(0xCA, (-0x400i16) as u16);
    integrate_draw_channels(&mut s, 8);
    assert_eq!(s.actor_u16(0xC8), 0, "negative total clamps to zero");
    s.move_submode = 6;
    s.set_actor_u16(0xCA, 0x40);
    integrate_draw_channels(&mut s, 8);
    assert_eq!(s.actor_u16(0xC8), 8);
}

/// Op `0x1C` (`0x8002366C..0x8002367C`) stores `op[1] << 3` at `+0xCA`
/// (`sh v0,0x4a(s1)`, `s1 = actor + 0x80`), the length channel's rate, and
/// the part tick grows `+0xC8` off it: PROT 0946's Call Wave rays `1C 0C`.
#[test]
fn op1c_rate_drives_the_length_channel() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    let bc = program(&[0x1C, 0x000C]);
    assert_eq!(step(&mut host, &mut state, &bc), StepResult::Advance);
    assert_eq!(state.pc, 2);
    assert_eq!(state.actor_u16(0xCA), 0x60);
    state.move_submode = 2;
    integrate_draw_channels(&mut state, 64);
    assert_eq!(state.actor_u16(0xC8), 0x60);
}

/// Op `0x3C` (`0x80023CA4`) seats a keyframe pose: mode `6`, the cursor
/// cleared, each part's six operands as both current and target keyframe.
/// Op `0x3D` (`0x80023D64`) sets the cursor rate `+0xD0`, latches the PC at
/// `+0xCE` and loads new targets; once a keyframe is latched, the next `0x3D`
/// first moves every current keyframe to where the cursor blended it.
#[test]
fn op3c_seats_and_op3d_retargets_the_keyframe_pose() {
    let mut host = TestHost::default();
    let mut s = ActorState::new();
    s.y_rot = 0x123;
    let bc = program(&[
        0x3C,
        2, //
        0,
        0x10,
        0x20,
        100,
        0,
        200, //
        0,
        0,
        0,
        (-100i16) as u16,
        0,
        0, //
        0x3D,
        0x0C,
        2, //
        0,
        0,
        0,
        300,
        0,
        400, //
        0,
        0,
        0,
        100,
        0,
        0, //
        0x3D,
        0x0C,
        2, //
        0,
        0,
        0,
        0,
        0,
        0, //
        0,
        0,
        0,
        0,
        0,
        0,
    ]);
    assert_eq!(step(&mut host, &mut s, &bc), StepResult::Advance);
    assert_eq!(s.pc, 14);
    assert_eq!(s.move_submode, 6);
    assert_eq!(s.y_rot, 0, "op 0x3C clears the cursor");
    assert_eq!(s.keyframe_pose.len(), 2);
    assert_eq!(&s.keyframe_pose[0][..6], &s.keyframe_pose[0][6..]);
    assert_eq!(s.keyframe_pose[1][3], -100);

    step(&mut host, &mut s, &bc);
    assert_eq!(s.pc, 29);
    assert_eq!(s.actor_u16(0xD0), 0x0C, "cursor rate");
    assert_eq!(s.actor_u16(0xCE), 14, "the latch is this op's PC");
    assert_eq!(s.keyframe_pose[0][3], 100, "no bake before a latch");
    assert_eq!(s.keyframe_pose[0][9], 300, "new X target");

    // Half way through the blend, the next 0x3D bakes it into `cur`.
    s.y_rot = 0x800;
    step(&mut host, &mut s, &bc);
    assert_eq!(s.keyframe_pose[0][3], 200, "100 + (300 - 100) / 2");
    assert_eq!(s.keyframe_pose[0][5], 300, "200 + (400 - 200) / 2");
    assert_eq!(s.keyframe_pose[1][3], 0, "-100 + (100 + 100) / 2");
    assert_eq!(s.y_rot, 0);
    assert_eq!(s.actor_u16(0xCC), 14, "the previous latch moves to +0xCC");
    assert_eq!(s.actor_u16(0xD2), 1);
}

/// The part tick steps the cursor by `+0xD0 * frame step`
/// (`0x80021E50..0x80021E74`), and the mode-6 tail packs each part's blended
/// keyframe into the clip entry `FUN_8001BE80` decodes
/// (`0x80022EFC..0x8002303C`): halfwords 3 / 4 / 5 as 12-bit X / Y / Z,
/// halfword 1 `>> 4` as the X and Z angle bytes, halfword 2 `>> 4` as Y.
#[test]
fn the_keyframe_cursor_steps_and_the_mode_6_tail_packs_the_pose() {
    let mut s = ActorState::new();
    s.set_actor_u16(0xD0, 0x0C);
    advance_keyframe_cursor(&mut s, 2);
    assert_eq!(s.y_rot, 0x18);
    s.keyframe_pose = vec![[0, 0x100, 0x200, -862, 5, 454, 0, 0x100, 0x200, -862, 5, 454]];
    assert!(keyframe_pose_entries(&s).is_empty(), "mode 6 only");
    s.move_submode = 6;
    let e = keyframe_pose_entries(&s);
    // X = -862 = 0xCA2, Y = 5, Z = 454 = 0x1C6.
    assert_eq!(e, vec![[0xA2, 0x05, 0x0C, 0xC6, 0x01, 0x10, 0x20, 0x10]]);
    // Blended half way toward a target.
    s.keyframe_pose[0][9] = -862 + 200;
    s.y_rot = 0x800;
    assert_eq!(
        keyframe_pose_entries(&s)[0][0],
        ((-762i32) as u32 & 0xFF) as u8
    );
}

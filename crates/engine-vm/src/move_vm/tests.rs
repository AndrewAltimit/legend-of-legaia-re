use super::*;

#[derive(Default)]
struct TestHost {
    rotation_table: std::collections::HashMap<u16, (i16, i16)>,
    stub16_calls: Vec<i16>,
    ext17_calls: Vec<i16>,
    global_writes_1d: Vec<u16>,
    ext20_calls: Vec<(i16, i16)>,
    face_rot_calls: Vec<(u8, [u16; 4], i32)>,
    spawn_calls: Vec<i16>,
    keyframe_allocs: Vec<i32>,
    keyframe_inits: u32,
    keyframe_frees: u32,
    move_image_calls: Vec<([u16; 4], i16, i16)>,
    ext_debug_world_calls: Vec<(i16, i16, i16)>,
    ext_world_struct_writes: Vec<(i16, [i16; 5])>,
    ext_world_struct_inits: Vec<(i16, [i16; 5])>,
    ext_set_flag_bank_calls: Vec<i16>,
    ext_clear_flag_bank_calls: Vec<i16>,
    ext_scratchpad_ramp_calls: Vec<(i16, i16, i16)>,
    ext_scratchpad_write_calls: Vec<(i16, i16)>,
    ext_emit_ot_calls: u32,
    ext_set_8007b9d8_calls: Vec<i32>,
    ext_fade_calls: Vec<([u8; 3], u16)>,
    /// Models the 16-slot 8-byte-stride scratch table at `&DAT_801F3498`.
    slot_table: [[u8; 8]; 16],
    /// Models `_DAT_801F22F4` (move-VM global predicate).
    global_predicate: u32,
    /// Models `_DAT_801F22F6` (move-VM global counter, mod 16).
    global_counter: u16,
    /// Models the player's world coords at `_DAT_8007C364 + 0x14..+0x1A`.
    player_xyz: [i16; 3],
    /// Models the fixed-origin pair at `_DAT_80089118 / _DAT_80089120`.
    fixed_origin_xz: (i32, i32),
    /// Models `_DAT_1F800393` (scratchpad ramp ratio numerator).
    dat_1f800393: u8,
    /// Models `_DAT_8007C348` (axis threshold offset).
    axis_threshold: i16,
    /// Constant value returned by `ext_query_flag_bank` (lets predicate
    /// tests select the expected branch).
    ext_query_flag_bank_returns: u32,
    /// Mirrors the actor's move bytecode buffer for sub-ops 0x04 / 0x1B
    /// / 0x1E. Tests that exercise these ops pre-seed the buffer to
    /// match the program they pass to `step`, then assert on the
    /// post-step contents.
    bytecode_buffer: Vec<u16>,
}

impl MoveHost for TestHost {
    fn rotation_lut(&self, index: u16) -> (i16, i16) {
        self.rotation_table.get(&index).copied().unwrap_or((0, 0))
    }
    fn stub_16(&mut self, _state: &mut ActorState, arg: i16) {
        self.stub16_calls.push(arg);
    }
    fn ext_17(&mut self, _state: &mut ActorState, arg: i16) {
        self.ext17_calls.push(arg);
    }
    fn global_write_1d(&mut self, value: u16) {
        self.global_writes_1d.push(value);
    }
    fn ext_20(&mut self, _state: &mut ActorState, a: i16, b: i16) {
        self.ext20_calls.push((a, b));
    }
    fn face_rotation_setup(&mut self, face_id: u8, params: [u16; 4], target: i32) {
        self.face_rot_calls.push((face_id, params, target));
    }
    fn spawn_child(&mut self, _state: &mut ActorState, slot: i16) {
        self.spawn_calls.push(slot);
    }
    fn keyframe_alloc(&mut self, bytes: i32) -> i32 {
        self.keyframe_allocs.push(bytes);
        // Return a non-zero pseudo-pointer so tests can verify it was
        // captured into ActorState.field_a8.
        0xC0DE_0000u32 as i32
    }
    fn keyframe_init(&mut self, _state: &mut ActorState, _buf: i32) {
        self.keyframe_inits += 1;
    }
    fn keyframe_free(&mut self, _state: &mut ActorState, _buf: i32) {
        self.keyframe_frees += 1;
    }
    fn move_image(&mut self, block: [u16; 4], a: i16, b: i16) {
        self.move_image_calls.push((block, a, b));
    }
    fn ext_debug_world(&mut self, x: i16, y: i16, z: i16) {
        self.ext_debug_world_calls.push((x, y, z));
    }
    fn ext_set_flag_bank(&mut self, idx: i16) {
        self.ext_set_flag_bank_calls.push(idx);
    }
    fn ext_clear_flag_bank(&mut self, idx: i16) {
        self.ext_clear_flag_bank_calls.push(idx);
    }
    fn ext_scratchpad_ramp(&mut self, slot: i16, target: i16, ticks: i16) {
        self.ext_scratchpad_ramp_calls.push((slot, target, ticks));
    }
    fn ext_scratchpad_write(&mut self, slot: i16, value: i16) {
        self.ext_scratchpad_write_calls.push((slot, value));
    }
    fn ext_emit_ot_packet(&mut self, _: &[u16]) {
        self.ext_emit_ot_calls += 1;
    }
    fn ext_set_8007b9d8(&mut self, value: i32) {
        self.ext_set_8007b9d8_calls.push(value);
    }
    fn ext_fade_color(&mut self, rgb: [u8; 3], ticks: u16) {
        self.ext_fade_calls.push((rgb, ticks));
    }
    fn ext_world_struct_write(&mut self, idx: i16, vals: [i16; 5]) {
        self.ext_world_struct_writes.push((idx, vals));
    }
    fn ext_world_struct_init(&mut self, idx: i16, vals: [i16; 5]) {
        self.ext_world_struct_inits.push((idx, vals));
    }
    fn move_slot_save_u32(&mut self, slot: u16, dword_off: u8, value: u32) {
        let slot_idx = (slot as usize) & 0xF;
        let off = dword_off as usize;
        self.slot_table[slot_idx][off..off + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn move_slot_load_u32(&self, slot: u16, dword_off: u8) -> u32 {
        let slot_idx = (slot as usize) & 0xF;
        let off = dword_off as usize;
        u32::from_le_bytes(self.slot_table[slot_idx][off..off + 4].try_into().unwrap())
    }
    fn move_slot_save_u16(&mut self, slot: u16, byte_off: u8, value: u16) {
        let slot_idx = (slot as usize) & 0xF;
        let off = byte_off as usize;
        self.slot_table[slot_idx][off..off + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn move_slot_load_u16(&self, slot: u16, byte_off: u8) -> u16 {
        let slot_idx = (slot as usize) & 0xF;
        let off = byte_off as usize;
        u16::from_le_bytes(self.slot_table[slot_idx][off..off + 2].try_into().unwrap())
    }
    fn move_global_predicate_get(&self) -> u32 {
        self.global_predicate
    }
    fn move_global_predicate_set(&mut self, value: u32) {
        self.global_predicate = value;
    }
    fn move_global_counter_get(&self) -> u16 {
        self.global_counter
    }
    fn move_global_counter_set(&mut self, value: u16) {
        self.global_counter = value;
    }
    fn move_player_world_xyz(&self) -> [i16; 3] {
        self.player_xyz
    }
    fn move_fixed_origin_xz(&self) -> (i32, i32) {
        self.fixed_origin_xz
    }
    fn move_dat_1f800393(&self) -> u8 {
        self.dat_1f800393
    }
    fn move_axis_threshold(&self) -> i16 {
        self.axis_threshold
    }
    fn ext_query_flag_bank(&self, _idx: i16) -> u32 {
        self.ext_query_flag_bank_returns
    }
    fn move_bytecode_read_u16(&self, word_off: usize) -> u16 {
        self.bytecode_buffer.get(word_off).copied().unwrap_or(0)
    }
    fn move_bytecode_write_u16(&mut self, word_off: usize, value: u16) {
        if word_off >= self.bytecode_buffer.len() {
            self.bytecode_buffer.resize(word_off + 1, 0);
        }
        self.bytecode_buffer[word_off] = value;
    }
}

fn program(words: &[u16]) -> Vec<u16> {
    // Append a guard word so the VM never reads past end during the
    // tested handler; the test asserts PC after one step.
    let mut v = words.to_vec();
    v.extend_from_slice(&[
        0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF,
    ]);
    v
}

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
    // default-arm size is 1.
    assert_eq!(state.pc, 1);
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
fn op2f_subop_0a_falls_through_when_predicate_set() {
    let mut host = TestHost::default();
    host.global_predicate = 1;
    let mut state = ActorState::new();
    // The convention in this VM: dispatcher's `default_arm()` returns
    // `size_u16 = 1` so the main step advances PC by 1 (matching the
    // PSX dispatcher's `iVar16 = 1; default: iVar16 << 0x10; return
    // iVar16 >> 0x10` shape).
    let bc = program(&[0x2F, 0x0A]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.pc, 1);
}

#[test]
fn op2f_subop_0a_skips_when_predicate_clear() {
    let mut host = TestHost::default();
    host.global_predicate = 0;
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x0A]);
    step(&mut host, &mut state, &bc);
    // Skip path = `with_size(3)` → PC += 3.
    assert_eq!(state.pc, 3);
}

#[test]
fn op2f_subop_0b_skips_when_predicate_set() {
    let mut host = TestHost::default();
    host.global_predicate = 1;
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x0B]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.pc, 3);
}

#[test]
fn op2f_subop_0b_falls_through_when_predicate_clear() {
    let mut host = TestHost::default();
    host.global_predicate = 0;
    let mut state = ActorState::new();
    step(&mut host, &mut state, &program(&[0x2F, 0x0B]));
    assert_eq!(state.pc, 1);
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
fn op2f_subop_06_skips_when_player_outside_box() {
    // Box corners (xa=10, za=20, xb=20, zb=30) scaled by 0x80 + 0x40 =
    // x in [1344, 2624], z in [2624, 3904]. Player at (0, 0, 0) is
    // outside → 0x06 takes the size-7 skip.
    let mut host = TestHost::default();
    host.player_xyz = [0, 0, 0];
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x06, 10, 20, 20, 30]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.pc, 7);
}

#[test]
fn op2f_subop_06_continues_when_player_inside_box() {
    let mut host = TestHost::default();
    host.player_xyz = [2000, 0, 3000]; // inside the [1344..2624] × [2624..3904] band
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x06, 10, 20, 20, 30]);
    step(&mut host, &mut state, &bc);
    // default-arm = size_u16 = 1 → PC += 1.
    assert_eq!(state.pc, 1);
}

// ---- actor_tick / decrement_wait_timer wiring ----

#[test]
fn decrement_wait_timer_subtracts_delta() {
    let mut state = ActorState::new();
    state.wait_timer = 10;
    decrement_wait_timer(&mut state, 4);
    assert_eq!(state.wait_timer, 6);
}

#[test]
fn decrement_wait_timer_wraps_to_negative() {
    let mut state = ActorState::new();
    state.wait_timer = 1;
    decrement_wait_timer(&mut state, 3);
    // 1 - 3 = -2 (wrapping i16).
    assert_eq!(state.wait_timer, -2);
}

#[test]
fn actor_tick_skips_vm_when_timer_nonneg() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.wait_timer = 0;

    let bc = program(&[0x06, 99, 0x08]); // WRITE_26, HALT
    let r = actor_tick(&mut host, &mut state, &bc, 16);
    assert_eq!(r, ActorTickOutcome::Waiting);
    // VM was not entered - render_26 unchanged, no HALT flag.
    assert_eq!(state.render_26, 0);
    assert_eq!(state.flags & 0x8, 0);
    assert_eq!(state.pc, 0);
}

#[test]
fn actor_tick_runs_vm_when_timer_negative() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.wait_timer = -1;

    let bc = program(&[0x06, 99, 0x08]); // WRITE_26, HALT
    let r = actor_tick(&mut host, &mut state, &bc, 16);
    assert_eq!(r, ActorTickOutcome::Halted);
    assert_eq!(state.render_26, 99);
    assert_eq!(state.flags & 0x8, 0x8);
}

#[test]
fn actor_tick_reports_wait_seeded() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.wait_timer = -1;

    // op 0x09 WAIT_SET sets wait_timer = arg << 3 and breaks.
    let bc = program(&[0x09, 5]);
    let r = actor_tick(&mut host, &mut state, &bc, 16);
    assert_eq!(r, ActorTickOutcome::WaitSeeded);
    assert_eq!(state.wait_timer, 40); // 5 << 3
    // No HALT flag.
    assert_eq!(state.flags & 0x8, 0);
}

#[test]
fn actor_tick_reports_end_of_buffer_on_oor_opcode() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.wait_timer = -1;

    let bc = program(&[0x47]); // out of range
    let r = actor_tick(&mut host, &mut state, &bc, 16);
    assert!(matches!(r, ActorTickOutcome::EndOfBuffer { opcode: 0x47 }));
}

#[test]
fn actor_tick_reports_budget_exhausted() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.wait_timer = -1;

    // Infinite loop: SAVE_LOOP18 + LOOP_JUMP19 (no break opcode).
    // Easier: just use a long sequence of WRITE_26 ops with no HALT.
    let mut words = vec![];
    for _ in 0..32 {
        words.push(0x06);
        words.push(0x00);
    }
    let bc = program(&words);
    let r = actor_tick(&mut host, &mut state, &bc, 4);
    assert_eq!(r, ActorTickOutcome::BudgetExhausted);
}

/// Composed bytecode walks several explicit-size opcodes - WORLD_SET,
/// FACE_ROTATION, ext sub 0x1C (set-flag-bank, size 3), then HALT.
/// `run_until_break` should exit at HALT with all per-op state changes
/// applied. Avoids the default-arm sub-ops whose `size_u16 = 1`
/// semantics leave PC pointing at the sub-op byte (which would then be
/// re-interpreted as a new opcode). Mirrors session-20's integration
/// style for the field VM but exercises the move-VM dispatch table.
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

/// `actor_tick` composed with `decrement_wait_timer` for a multi-frame
/// scenario where the script seeds a new wait inside the VM and the
/// next frame must decrement that wait before re-entering. Mirrors a
/// retail per-frame loop where one script `WAIT_SET` keeps the actor
/// idle for a known number of frames.
#[test]
fn actor_tick_wait_set_then_decrements_to_resume() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.wait_timer = -1; // VM eligible

    // WAIT_SET 4 (sets timer = 32 = 4<<3) then HALT.
    let bc = program(&[0x09, 4, 0x08]);

    // Frame 1: VM runs, hits WAIT_SET, breaks with WaitSeeded.
    let r1 = actor_tick(&mut host, &mut state, &bc, 16);
    assert_eq!(r1, ActorTickOutcome::WaitSeeded);
    assert_eq!(state.wait_timer, 32);

    // Frames 2..N: pre-tick decrements; until timer is back negative,
    // VM is gated. Use delta=8 so it takes 5 frames (32 → 24 → 16 → 8 → 0 → -8).
    for expected in [24, 16, 8, 0] {
        decrement_wait_timer(&mut state, 8);
        assert_eq!(state.wait_timer, expected);
        assert_eq!(
            actor_tick(&mut host, &mut state, &bc, 16),
            ActorTickOutcome::Waiting
        );
    }

    // One more pre-tick → timer = -8 (negative). VM runs, but we're
    // sitting at the HALT instruction now (PC was advanced past the
    // 2-word WAIT_SET on the seed step).
    decrement_wait_timer(&mut state, 8);
    assert_eq!(state.wait_timer, -8);
    assert_eq!(
        actor_tick(&mut host, &mut state, &bc, 16),
        ActorTickOutcome::Halted
    );
}

#[test]
fn actor_tick_pretick_then_tick_models_retail_frame() {
    // Compose decrement_wait_timer + actor_tick to model a full frame.
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.wait_timer = 8; // initially "still waiting"

    let bc = program(&[0x06, 42, 0x08]);

    // Frame 1: pre-tick takes timer 8 → 5 (delta = 3). Still nonneg, skip.
    decrement_wait_timer(&mut state, 3);
    assert_eq!(
        actor_tick(&mut host, &mut state, &bc, 16),
        ActorTickOutcome::Waiting
    );

    // Frame 2: 5 → 2 (still nonneg).
    decrement_wait_timer(&mut state, 3);
    assert_eq!(
        actor_tick(&mut host, &mut state, &bc, 16),
        ActorTickOutcome::Waiting
    );

    // Frame 3: 2 → -1 (now negative). VM runs, hits HALT.
    decrement_wait_timer(&mut state, 3);
    assert_eq!(
        actor_tick(&mut host, &mut state, &bc, 16),
        ActorTickOutcome::Halted
    );
    assert_eq!(state.render_26, 42);
}

#[test]
fn op2f_subop_0e_advances_pc_by_eleven_and_writes_world_average() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    // a = (10, 20, 30), off = (1, 2, 3), b = (40, 60, 90)
    // mid = ((10+40)/2 + 1, (20+60)/2 + 2, (30+90)/2 + 3) = (26, 42, 63)
    let bc = program(&[0x2F, 0x0E, 10, 20, 30, 1, 2, 3, 40, 60, 90]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.world_x, 26);
    assert_eq!(state.world_y, 42);
    assert_eq!(state.world_z, 63);
    assert_eq!(
        state.pc, 11,
        "0x0E must advance past the entire 11-word instruction"
    );
}

#[test]
fn op2f_subop_12_uses_slot_indexed_by_field_86_low_byte() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.field_86 = 0x4205; // slot index = 5
    // Pre-populate slot 5 with a = (50, 60, 70).
    host.move_slot_save_u32(5, 0, (50u16 as u32) | ((60u16 as u32) << 16));
    host.move_slot_save_u32(5, 4, (70u16 as u32) | ((0u16 as u32) << 16));
    // off = (1, 2, 3), b = (50, 60, 70)
    // mid_x = ((50 + 50)/2) + 1 = 51
    // mid_y = ((60 + 60)/2) + 2 = 62
    // mid_z = ((70 + 70)/2) + 3 = 73
    let bc = program(&[0x2F, 0x12, 1, 2, 3, 50, 60, 70]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.world_x, 51);
    assert_eq!(state.world_y, 62);
    assert_eq!(state.world_z, 73);
    assert_eq!(state.pc, 8, "0x12 must advance past the 8-word instruction");
}

#[test]
fn op2f_subop_13_falls_through_when_flag_set() {
    let mut host = TestHost::default();
    host.ext_query_flag_bank_returns = 1;
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x13, 7]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.pc, 1, "predicate-true → default-arm size 1");
}

#[test]
fn op2f_subop_13_skips_when_flag_clear() {
    let mut host = TestHost::default();
    host.ext_query_flag_bank_returns = 0;
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x13, 7]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.pc, 4, "predicate-false → skip past 3-u16 follow-up");
}

#[test]
fn op2f_subop_14_inverts_predicate() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    // 0x14 with predicate set → SKIP (size 4).
    host.ext_query_flag_bank_returns = 1;
    let bc = program(&[0x2F, 0x14, 7]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.pc, 4);
    // 0x14 with predicate clear → fall through (size 1).
    let mut host2 = TestHost::default();
    host2.ext_query_flag_bank_returns = 0;
    let mut state2 = ActorState::new();
    step(&mut host2, &mut state2, &bc);
    assert_eq!(state2.pc, 1);
}

#[test]
fn op2f_subop_36_axis_threshold_below() {
    // 0x36 predicate: op[2] < (0x8E - axis). axis=0, op[2]=0x40 → 0x40 < 0x8E true.
    let mut host = TestHost::default();
    host.axis_threshold = 0;
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x36, 0x40]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.pc, 1, "predicate true → default-arm");
}

#[test]
fn op2f_subop_36_axis_threshold_above_skips() {
    // op[2]=0xFF, axis=0: 0xFF < 0x8E is false → skip 4.
    let mut host = TestHost::default();
    host.axis_threshold = 0;
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x36, 0xFF]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.pc, 4);
}

#[test]
fn op2f_subop_37_is_inverse_of_36() {
    // 0x37: (0x8E - axis) < op[2]. With axis=0, op[2]=0xFF → 0x8E < 0xFF true.
    let mut host = TestHost::default();
    host.axis_threshold = 0;
    let mut state = ActorState::new();
    let bc = program(&[0x2F, 0x37, 0xFF]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.pc, 1);
    // axis=0, op[2]=0x40 → 0x8E < 0x40 false → skip.
    let mut state2 = ActorState::new();
    let bc2 = program(&[0x2F, 0x37, 0x40]);
    step(&mut host, &mut state2, &bc2);
    assert_eq!(state2.pc, 4);
}

#[test]
fn op2f_subop_38_predicate_outside_radius() {
    let mut host = TestHost::default();
    host.player_xyz = [0, 0, 0];
    let mut state = ActorState::new();
    // Actor at (10, 0, 0), player at origin → dist² = 100. r=8 → r²=64.
    // 0x38: r² < dist² → 64 < 100 true → default-arm.
    state.world_x = 10;
    let bc = program(&[0x2F, 0x38, 8]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.pc, 1);
}

#[test]
fn op2f_subop_39_predicate_inside_radius() {
    let mut host = TestHost::default();
    host.player_xyz = [0, 0, 0];
    let mut state = ActorState::new();
    // Actor at (3, 0, 4), player at origin → dist² = 25. r=10 → r²=100.
    // 0x39: dist² < r² → 25 < 100 true → default-arm.
    state.world_x = 3;
    state.world_z = 4;
    let bc = program(&[0x2F, 0x39, 10]);
    step(&mut host, &mut state, &bc);
    assert_eq!(state.pc, 1);
    // Move actor to (100, 0, 0): dist² = 10000, r²=100 → false → skip.
    let mut state2 = ActorState::new();
    state2.world_x = 100;
    let bc2 = program(&[0x2F, 0x39, 10]);
    step(&mut host, &mut state2, &bc2);
    assert_eq!(state2.pc, 4);
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
    assert_eq!(state.pc, 1);
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
    assert_eq!(state.pc, 1, "default-arm");
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

// --- HSV helpers + ext sub-op 0x1F / 0x20 -----------------------------

#[test]
fn rgb_to_hsv_pure_red_round_trip() {
    let (h, s, v) = rgb_to_hsv(0xFF, 0, 0);
    assert_eq!(h, 0, "pure red has H = 0");
    assert!(s > 0xF0, "pure red is fully saturated, got {s:#x}");
    assert_eq!(v, 0xFF, "pure red has V = 0xFF");
}

#[test]
fn rgb_to_hsv_pure_green_lands_in_segment_2() {
    let (h, _s, _v) = rgb_to_hsv(0, 0xFF, 0);
    // Green = 120 deg = 0x78 in this encoding.
    assert_eq!(h, 0x78);
}

#[test]
fn rgb_to_hsv_pure_blue_lands_in_segment_4() {
    let (h, _s, _v) = rgb_to_hsv(0, 0, 0xFF);
    // Blue = 240 deg = 0xF0.
    assert_eq!(h, 0xF0);
}

#[test]
fn rgb_to_hsv_zero_returns_zero() {
    assert_eq!(rgb_to_hsv(0, 0, 0), (0, 0, 0));
}

#[test]
fn hsv_to_rgb_segment_dispatch_matches_each_arm() {
    // V = 0xFF, S = 0xFF - picks the segment based on H.
    // Segment 0 (H=0): (V, t, p) - pure red.
    let (r, g, b) = hsv_to_rgb(0, 0xFF, 0xFF);
    assert!(r >= 0xF0 && g <= 1 && b <= 1, "segment 0 ≈ pure red");
    // Segment 2 (H=0x78=120 deg): green.
    let (r, g, b) = hsv_to_rgb(0x78, 0xFF, 0xFF);
    assert!(r <= 1 && g >= 0xF0 && b <= 1, "segment 2 ≈ pure green");
    // Segment 4 (H=0xF0=240 deg): blue.
    let (r, g, b) = hsv_to_rgb(0xF0, 0xFF, 0xFF);
    assert!(r <= 1 && g <= 1 && b >= 0xF0, "segment 4 ≈ pure blue");
}

#[test]
fn hsv_to_rgb_zero_saturation_returns_grey() {
    let (r, g, b) = hsv_to_rgb(0x55, 0, 0x80);
    assert_eq!((r, g, b), (0x80, 0x80, 0x80));
}

#[test]
fn op2f_subop_1f_rotates_hue_on_keyframe_desc_lo() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    // Pre-set actor[+0xa0..+0xa3] = packed pure-red RGB.
    state.keyframe_desc[0] = 0x00FF; // R=0xFF, G=0
    state.keyframe_desc[1] = 0x0000; // B=0
    // Sub-op 0x1F: delta H = 0x78 (= 120 deg), delta S = 0, delta V = 0.
    // Should rotate red → green.
    let bc = program(&[0x2F, 0x1F, 0x78, 0, 0]);
    step(&mut host, &mut state, &bc);
    let r = state.keyframe_desc[0] & 0xFF;
    let g = (state.keyframe_desc[0] >> 8) & 0xFF;
    let b = state.keyframe_desc[1] & 0xFF;
    assert!(
        g > r,
        "hue-rotated by 120 deg should make G dominant ({r:#x},{g:#x},{b:#x})"
    );
    assert!(
        g > b,
        "hue-rotated by 120 deg should make G dominate B ({r:#x},{g:#x},{b:#x})"
    );
    // FUN_8001a6c8 caps at 0xF8.
    assert!(g <= 0xF8);
    // PC advances by 1 (default_arm).
    assert_eq!(state.pc, 1);
}

#[test]
fn op2f_subop_20_targets_keyframe_desc_hi_pair() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    // Pre-set actor[+0xa4..+0xa7] = packed pure-blue.
    state.keyframe_desc[2] = 0x0000;
    state.keyframe_desc[3] = 0x00FF; // B=0xFF
    // Sub-op 0x20: delta H = 0x78 (= 120 deg). Blue → red.
    let bc = program(&[0x2F, 0x20, 0x78, 0, 0]);
    step(&mut host, &mut state, &bc);
    let r = state.keyframe_desc[2] & 0xFF;
    let g = (state.keyframe_desc[2] >> 8) & 0xFF;
    let b = state.keyframe_desc[3] & 0xFF;
    assert!(r > g);
    assert!(r > b);
    // 0x1F slot must be untouched.
    assert_eq!(state.keyframe_desc[0], 0);
    assert_eq!(state.keyframe_desc[1], 0);
}

#[test]
fn op2f_subop_1f_value_decrement_dims_color() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.keyframe_desc[0] = 0x80FF; // R=0xFF, G=0x80
    state.keyframe_desc[1] = 0x0040; // B=0x40
    let v_before = (state.keyframe_desc[0] & 0xFF).max((state.keyframe_desc[0] >> 8) & 0xFF);
    // Delta H = 0, delta S = 0, delta V = -0x40 (use signed).
    let bc = program(&[0x2F, 0x1F, 0, 0, (-0x40i16) as u16]);
    step(&mut host, &mut state, &bc);
    let r_after = state.keyframe_desc[0] & 0xFF;
    let g_after = (state.keyframe_desc[0] >> 8) & 0xFF;
    let v_after = r_after.max(g_after);
    assert!(
        v_after < v_before,
        "lowering V should reduce the dominant channel ({v_before:#x} -> {v_after:#x})"
    );
}

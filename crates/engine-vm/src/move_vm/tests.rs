use super::*;

mod actor_tick;
mod base_ops;
mod color;
mod ext_branches;
mod ext_widths;
mod ext_writes;

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
    ext_world_struct_adds: Vec<(i16, [i16; 5])>,
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
    /// Constant value returned by `ext_party_member_lookup` (lets sub-op
    /// 0x3B tests select the hit or the miss path).
    party_member: Option<[i16; 3]>,
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
    fn ext_world_struct_add(&mut self, idx: i16, deltas: [i16; 5]) {
        self.ext_world_struct_adds.push((idx, deltas));
    }
    fn ext_party_member_lookup(&self, _slot: i16) -> Option<[i16; 3]> {
        self.party_member
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

// ---- 0x2F extension widths: every arm against the JT-derived table ----

/// Build `[0x2F, sub_op, 0, 0, ...]` with **zero** padding.
///
/// `program()` pads with `0xFFFF`, which would feed the branch sub-ops a
/// `-1` displacement; the width table is stated for a zero delta.
fn ext_program(sub_op: u16) -> Vec<u16> {
    let mut v = vec![0x2Fu16, sub_op];
    v.resize(32, 0);
    v
}

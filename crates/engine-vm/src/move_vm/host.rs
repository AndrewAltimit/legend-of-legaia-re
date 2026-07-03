//! Engine-side callback trait for the move VM.

use super::*;

/// Engine-side callbacks for the move VM.
///
/// All methods have default impls so a minimal host (only animation-state
/// reads) compiles. Each method documents which SCUS function it stands in for.
pub trait MoveHost {
    /// Op 0x03 sin/cos table lookup. The original reads a table at
    /// `_DAT_8007B81C` (sin) and `_DAT_8007B7F8` (cos), each 0x1000-entry
    /// signed-16-bit, indexed by `actor[+0x96] & 0xFFF`. Hosts return the
    /// `(sin, cos)` pair for the given index.
    fn rotation_lut(&self, _index: u16) -> (i16, i16) {
        (0, 0)
    }

    /// Op 0x16 STUB. The original's `FUN_80024C80` is a pure `jr ra` - a
    /// no-op the VM only calls to mirror dispatch. Default impl is a no-op.
    fn stub_16(&mut self, _state: &mut ActorState, _arg: i16) {}

    /// Op 0x17 - `func_0x801f30c4(actor, op[1])`. Overlay-resident effect
    /// trigger (the actual handler isn't yet decompiled). Default no-op.
    fn ext_17(&mut self, _state: &mut ActorState, _arg: i16) {}

    /// Op 0x1D - write `DAT_8007B6DE = op[1]`. A single-byte global slot.
    /// Hosts model their own global state; default impl ignores.
    fn global_write_1d(&mut self, _value: u16) {}

    /// Op 0x20 - `(*gp[0x714])(actor, op[1], op[2])`. Indirect call through
    /// an SDK-style vtable. The function is not yet identified. Default no-op.
    fn ext_20(&mut self, _state: &mut ActorState, _arg0: i16, _arg1: i16) {}

    /// Op 0x21 - `DAT_8007BE60 + face_id*12` write. The original maintains a
    /// `12 * N`-byte table indexed by face id. The VM passes the decoded
    /// face_id + the four u16s + final i32. Default no-op.
    fn face_rotation_setup(&mut self, _face_id: u8, _params: [u16; 4], _target: i32) {}

    /// Op 0x25 - `FUN_80021B04`. Spawns a child actor at the slot given by
    /// `op[1]`. Default no-op (engine-side actor pool).
    fn spawn_child(&mut self, _state: &mut ActorState, _slot: i16) {}

    /// Op 0x2C - heap allocation for the keyframe buffer.
    /// `bytes` is `w * h * 2` from the encoded params. Original calls
    /// `FUN_80017888(0, bytes)` to allocate. Returns the heap pointer (or
    /// `0` to indicate "use the inline buffer").
    fn keyframe_alloc(&mut self, _bytes: i32) -> i32 {
        0
    }

    /// Op 0x2C - initialize the keyframe descriptor at `&actor + 0xA0`.
    /// Original calls `FUN_8005842C(descriptor_ptr, buffer_ptr)`. Default
    /// no-op.
    fn keyframe_init(&mut self, _state: &mut ActorState, _buffer_ptr: i32) {}

    /// Op 0x30 - release the keyframe buffer if heap-allocated. Default
    /// no-op (FUN_800583C8 in the original).
    fn keyframe_free(&mut self, _state: &mut ActorState, _buffer_ptr: i32) {}

    /// Op 0x40 - libgpu `MoveImage(rect, dst_x, dst_y)` (`FUN_80058490`):
    /// a VRAM-to-VRAM copy whose source RECT `[x, y, w, h]` comes from
    /// `op[1..5]` and destination `(x, y)` from `op[5..7]`. Retail move
    /// programs use it to cycle animated-texture strips (VRAM-resident
    /// frames stamped over the live texel rect; live-traced from the field
    /// scene strip animators). Default no-op.
    fn move_image(&mut self, _src_rect: [u16; 4], _dst_x: i16, _dst_y: i16) {}

    /// Multiplier byte read by op 0x0A KEYFRAME_LOAD - `DAT_1F80037D`.
    /// Default 0x100 (a sane multiplier; the original is initialized at boot).
    fn keyframe_curve_multiplier(&self) -> u8 {
        0x10
    }

    /// Extension VM entry. The VM has already decoded the sub-opcode word.
    /// Hosts may stub the entire extension VM by returning
    /// `MoveExtResult::default_arm()` or implement per-sub-op behaviour.
    fn ext_dispatch(
        &mut self,
        state: &mut ActorState,
        sub_opcode: u16,
        operand: &[u16],
    ) -> MoveExtResult {
        // Default impl walks the well-understood arms and falls through to
        // `default_arm()` for the rest.
        ext_default_dispatch(self, state, sub_opcode, operand)
    }

    /// Extension sub-op 0x01 - `func_0x8001a068(fmt, x, y, z)` - debug print
    /// of world coords. Default no-op.
    fn ext_debug_world(&mut self, _x: i16, _y: i16, _z: i16) {}

    /// Extension sub-op 0x05 / 0x30 - `func_0x80056798()` - opaque, returns
    /// the size to advance by. Default returns `default_arm()`.
    fn ext_func56798(&mut self, _state: &mut ActorState) -> MoveExtResult {
        MoveExtResult::default_arm()
    }

    /// Extension sub-op 0x0E - `FUN_801E45BC(out_xy, sub_a, sub_b, mode)` -
    /// computes a midpoint-style position; the VM passes the three i16
    /// triples plus the mode word read from `actor[+0x50]`. Default no-op
    /// (the VM still writes the result fields if the host returns them).
    fn ext_midpoint_set(
        &mut self,
        _state: &mut ActorState,
        _a: [i16; 3],
        _b: [i16; 3],
        _offsets: [i16; 3],
        _mode: u16,
    ) {
    }

    /// Extension sub-op 0x13 / 0x14 - `func_0x8003CE64(flag_index)` - query
    /// the fourth-flag-bank bitfield (`DAT_80085758`). Returns 0 if clear,
    /// non-zero otherwise. The VM uses the result to decide between the
    /// fall-through and predicate-skip arms.
    fn ext_query_flag_bank(&self, _flag_index: i16) -> u32 {
        0
    }

    /// Extension sub-op 0x1C - `func_0x8003CE08(flag_index)` - set a bit in
    /// the fourth-flag-bank.
    fn ext_set_flag_bank(&mut self, _flag_index: i16) {}

    /// Extension sub-op 0x1D - `func_0x8003CE34(flag_index)` - clear a bit
    /// in the fourth-flag-bank.
    fn ext_clear_flag_bank(&mut self, _flag_index: i16) {}

    /// Extension sub-op 0x29 - `func_0x8003C5F0(0, &DAT_1F80035C[idx], 2,
    /// current, -value, ticks)` - schedules a per-frame ramp of a scratchpad
    /// slot. Default no-op.
    fn ext_scratchpad_ramp(&mut self, _slot_index: i16, _target: i16, _ticks: i16) {}

    /// Extension sub-op 0x29 immediate path - direct write to scratchpad
    /// slot. The VM only fires this when `ticks == 0`.
    fn ext_scratchpad_write(&mut self, _slot_index: i16, _value: i16) {}

    /// Extension sub-op 0x2C - `FUN_801D31B0(actor, op)` - overlay-resident
    /// helper. Default no-op.
    fn ext_func801d31b0(&mut self, _state: &mut ActorState, _operand: &[u16]) {}

    /// Extension sub-op 0x2E - `func_0x80059010(...)` and friends - emits
    /// a packet onto the GP0 OT (the PSX render-list ring). Default no-op.
    fn ext_emit_ot_packet(&mut self, _operand: &[u16]) {}

    /// Extension sub-op 0x2F - `_DAT_8007B9D8 = (i32) op[1]`. A globally-
    /// shared 32-bit slot. Default no-op.
    fn ext_set_8007b9d8(&mut self, _value: i32) {}

    /// Extension sub-op 0x3A - `func_0x80019B28(z1, x1, z2, x2)` - angle
    /// computation between the actor's world coords and the player's. The
    /// VM stores the result at `op[op[1] + 3]`. Default returns 0.
    fn ext_compute_angle(&self, _state: &ActorState) -> u16 {
        0
    }

    /// Extension sub-op 0x3B - read `_DAT_8007B898 + 0x22` triple via
    /// `func_0x8003D064`. Returns the resolved actor index. Default returns
    /// `None` (no party member).
    fn ext_party_member_lookup(&self, _slot: i16) -> Option<[i16; 3]> {
        None
    }

    /// Extension sub-op 0x3C - fade colour write. When `ticks == 0`, writes
    /// the three colour bytes immediately to scratchpad globals; non-zero
    /// schedules a ramp. The VM passes both branches through this hook so
    /// hosts can model whichever is convenient. PC += 6.
    fn ext_fade_color(&mut self, _rgb: [u8; 3], _ticks: u16) {}

    /// Extension sub-op 0x18/0x19/0x1A - three world-derived writes to a
    /// shared 0x14-byte struct at `iVar16 - 0x7FF7C008` (offset-resolved by
    /// `*op[1] * 0x14`). Hosts model the table; the VM passes index + the
    /// 5 u16 values written. Default no-op.
    fn ext_world_struct_write(&mut self, _index: i16, _values: [i16; 5]) {}

    /// Extension sub-op 0x17 - initial / configuration write to the same
    /// struct as `ext_world_struct_write`. 5 u16 values from operand+6..16.
    fn ext_world_struct_init(&mut self, _index: i16, _values: [i16; 5]) {}

    /// Read 4 bytes from the move-VM 16-slot scratch table at
    /// `&DAT_801F3498`. Each slot is 8 bytes wide; `dword_off` is `0` or
    /// `4`. Used by ext sub-ops 0x26 / 0x32 / 0x35 (load) and 0x12 / 0x28
    /// (read-modify-write). Default impl returns 0 - hosts that care
    /// about cross-actor save/restore override this.
    fn move_slot_load_u32(&self, _slot: u16, _dword_off: u8) -> u32 {
        0
    }

    /// Write 4 bytes to the move-VM 16-slot scratch table. See
    /// [`Self::move_slot_load_u32`]. Used by ext sub-ops 0x25 / 0x31 (and
    /// the partial-slot pair below). `dword_off` is `0` or `4`.
    fn move_slot_save_u32(&mut self, _slot: u16, _dword_off: u8, _value: u32) {}

    /// Read 2 bytes from the move-VM 16-slot scratch table. `byte_off` is
    /// `0`..`6` (even). Used by ext sub-op 0x35 (single-u16 reload).
    fn move_slot_load_u16(&self, _slot: u16, _byte_off: u8) -> u16 {
        0
    }

    /// Write 2 bytes to the move-VM 16-slot scratch table. `byte_off` is
    /// `0`..`6` (even). Used by ext sub-ops 0x27 (3 × u16 from `+0x90`) and
    /// 0x34 (1 × u16 from `+0x72`).
    fn move_slot_save_u16(&mut self, _slot: u16, _byte_off: u8, _value: u16) {}

    /// Read the move-VM global predicate at `&DAT_801F22F4` (set by ext
    /// sub-op 0x08, cleared by 0x09). Sub-ops 0x0A / 0x0B branch on it.
    /// Default returns 0 - equivalent to "predicate is false / never set"
    /// (sub-op 0x0A skips, sub-op 0x0B falls through).
    fn move_global_predicate_get(&self) -> u32 {
        0
    }

    /// Write the move-VM global predicate. Used by ext sub-ops 0x08 / 0x09.
    fn move_global_predicate_set(&mut self, _value: u32) {}

    /// Read the move-VM global counter at `&DAT_801F22F6`. Cleared by ext
    /// sub-op 0x0F; cycled mod 16 by sub-op 0x10 (which also writes the
    /// captured low byte into `actor[+0x86]`).
    fn move_global_counter_get(&self) -> u16 {
        0
    }

    /// Write the move-VM global counter. Used by ext sub-ops 0x0F / 0x10.
    fn move_global_counter_set(&mut self, _value: u16) {}

    /// Player position read from `_DAT_8007C364 + 0x14..+0x1A` - a 3 × i16
    /// triple. Used by ext sub-op 0x2A (world position lerp toward player)
    /// and the bbox-vs-player tests at 0x06 / 0x07 / 0x36 / 0x39. Default
    /// returns the origin (engine-vm test hosts override).
    fn move_player_world_xyz(&self) -> [i16; 3] {
        [0, 0, 0]
    }

    /// Map fixed-origin pair `(_DAT_80089118, _DAT_80089120)` - the (x, z)
    /// origin used by ext sub-op 0x24 (world position lerp toward fixed
    /// map origin). Default returns `(0, 0)`.
    fn move_fixed_origin_xz(&self) -> (i32, i32) {
        (0, 0)
    }

    /// Read `_DAT_1F800393` - a u8 scratchpad slot used by ext sub-op 0x23
    /// as the numerator of a 12.0 fixed-point ramp ratio against the
    /// operand-supplied denominator. Default returns 0 (the lerp becomes
    /// a no-op).
    fn move_dat_1f800393(&self) -> u8 {
        0
    }

    /// Read `_DAT_8007C348` - the axis offset used by ext sub-ops 0x36 / 0x37
    /// for the `0x8E - axis` threshold predicate. Default returns 0 (so the
    /// threshold collapses to `op[2] < 0x8E` / `op[2] > 0x8E`).
    fn move_axis_threshold(&self) -> i16 {
        0
    }

    /// Read a u16 from the actor's move-bytecode buffer at the given absolute
    /// word offset (`actor[+0x48][word_off]`). Used by ext sub-ops 0x1B (copy
    /// loop) and 0x1E (read-modify-write). Default returns 0 - hosts that
    /// model the move buffer (e.g. engine-core's `World`) override.
    fn move_bytecode_read_u16(&self, _word_off: usize) -> u16 {
        0
    }

    /// Write a u16 to the actor's move-bytecode buffer. Used by ext sub-ops
    /// 0x04 (write actor world to operand slot), 0x1B (copy loop), and 0x1E
    /// (read-modify-write). Default no-op.
    fn move_bytecode_write_u16(&mut self, _word_off: usize, _value: u16) {}
}

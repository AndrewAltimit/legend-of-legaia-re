//! Main move-VM dispatch loop (`FUN_80023070`) + per-frame actor tick gate.

use super::*;

/// Decode and execute one instruction.
///
/// `bytecode` is the move buffer as u16 words (the original stores it as
/// `int* actor[+0x48]` and indexes with `actor[+0x70] * 2`); `state.pc` is
/// the current u16-word offset.
///
/// Returns a [`StepResult`] describing the outcome. The dispatcher loop is
/// the caller's responsibility - the move VM's outer loop in the original
/// was just "step until break".
pub fn step<H: MoveHost + ?Sized>(
    host: &mut H,
    state: &mut ActorState,
    bytecode: &[u16],
) -> StepResult {
    let pc_start = state.pc as usize;
    let Some(opcode) = bytecode.get(pc_start).copied() else {
        return StepResult::EndOfBuffer { opcode: 0 };
    };
    if opcode > 0x46 {
        return StepResult::EndOfBuffer { opcode };
    }

    // Operand reader - pure function over the bytecode slice, doesn't borrow
    // `state`. Out-of-range reads return 0 (matching the original's reliance
    // on the move buffer being correctly sized for each opcode).
    let read = |i: usize| -> u16 { bytecode.get(pc_start + i).copied().unwrap_or(0) };

    // Handlers return the size in u16 units (a.k.a. `param_3`). Halt and
    // Wait are signalled by setting `outcome` to non-Advance.
    let mut outcome = StepResult::Advance;
    let mut size: i16;

    match opcode {
        // 0x00 - ANIM_BANK_SET. size 4.
        // actor[+0x3C..+0x40] = op[1..3] << 3.
        0x00 => {
            state.anim_3c = (read(1) as i16).wrapping_shl(3);
            state.anim_3e = (read(2) as i16).wrapping_shl(3);
            state.anim_40 = (read(3) as i16).wrapping_shl(3);
            size = 4;
        }
        // 0x01 - WORLD_ADD. size 4.
        0x01 => {
            let v1 = read(1) as i16;
            let v2 = read(2) as i16;
            let v3 = read(3) as i16;
            state.world_x = state.world_x.wrapping_add(v1);
            state.world_y = state.world_y.wrapping_add(v2);
            state.world_y_mirror = state.world_y_mirror.wrapping_add(v2);
            state.world_z = state.world_z.wrapping_add(v3);
            size = 4;
        }
        // 0x02 - BANK_SET_98. size 2. actor[+0x98] = op[1] << 3.
        0x02 => {
            state.tween_scale_y = (read(1) as i16).wrapping_shl(3);
            size = 2;
        }
        // 0x03 - WORLD_ROTATE_ADD. size 2. Sin/cos rotated add into world XZ.
        0x03 => {
            let v1 = read(1) as i16;
            let (sin_v, cos_v) = host.rotation_lut(state.tween_scale_x as u16 & 0xFFF);
            // x += (sin * v1) >> 12
            let dx = ((sin_v as i32) * (v1 as i32)) >> 12;
            // z += (cos * v1) >> 12
            let dz = ((cos_v as i32) * (v1 as i32)) >> 12;
            state.world_x = state.world_x.wrapping_add(dx as i16);
            state.world_z = state.world_z.wrapping_add(dz as i16);
            size = 2;
        }
        // 0x04 - ANIM_BANK_2. size 4.
        0x04 => {
            state.anim_80 = (read(1) as i16).wrapping_shl(3);
            state.anim_82 = (read(2) as i16).wrapping_shl(3);
            state.anim_84 = (read(3) as i16).wrapping_shl(3);
            size = 4;
        }
        // 0x05 - RENDER_BANK_ADD. size 4.
        0x05 => {
            state.render_24 = state.render_24.wrapping_add(read(1) as i16);
            state.render_26 = state.render_26.wrapping_add(read(2) as i16);
            state.render_28 = state.render_28.wrapping_add(read(3) as i16);
            size = 4;
        }
        // 0x06 - WRITE_26. size 2.
        0x06 => {
            state.render_26 = read(1) as i16;
            size = 2;
        }
        // 0x07 - WORLD_SET. size 4.
        0x07 => {
            state.world_x = read(1) as i16;
            state.world_y = read(2) as i16;
            state.world_y_mirror = read(2) as i16;
            state.world_z = read(3) as i16;
            size = 4;
        }
        // 0x08 - HALT. size 0, ends loop.
        0x08 => {
            state.flags |= 0x8;
            outcome = StepResult::Halt;
            size = 0;
        }
        // 0x09 - WAIT_SET. size 2, ends loop.
        0x09 => {
            state.wait_timer = (read(1) as i16).wrapping_shl(3);
            outcome = StepResult::Wait;
            size = 2;
        }
        // 0x0A - KEYFRAME_LOAD. variable size = 3 + count*3.
        0x0A => {
            state.flags |= 0x1000;
            let header_op2 = read(2);
            state.keyframe_count = header_op2 as u8;
            let count = header_op2 as i16;
            let curve_mul = host.keyframe_curve_multiplier() as i32;
            size = 3;
            for i in 0..count.max(0) {
                let base = (3 + 3 * i) as usize;
                // Note: byte writes to `+0xB0+i` are abstracted into anim_block.
                let descriptor_byte = read(base) as u8;
                state.anim_block_u16_set(0x04 + (i as usize) * 2, descriptor_byte as u16);

                let raw_b8 = read(base + 1) as i16 as i32;
                let scaled_b8 = (raw_b8 * curve_mul) >> 3;
                state.anim_block_u16_set(0x0C + (i as usize) * 2, scaled_b8 as u16);

                let raw_c8 = read(base + 2) as i16 as i32;
                let scaled_c8 = (raw_c8 * curve_mul) >> 3;
                state.anim_block_u16_set(0x1C + (i as usize) * 2, scaled_c8 as u16);

                size += 3;
            }
            // op[1] == 0 path also clears anim_block[0] + a 4-byte slot.
            if read(1) == 0 {
                state.anim_block_u16_set(0x00, 0);
            }
            state.local_flags = 0;
        }
        // 0x0B - DefaultBreak: drops out of switch with size 0; the original
        // skipped the size set, and the epilogue still ran (no PC advance).
        0x0B => {
            // No change to PC; runtime continues the loop unless a prior
            // handler set bVar3 = false. We keep size = 0, advance, and rely
            // on the caller to detect a no-progress loop if it cares.
            size = 0;
        }
        // 0x0C - composite control word build. size 6.
        0x0C => {
            let v1 = read(1) as i16 as i32;
            let v2 = read(2) as i16 as i32;
            let v3 = read(3) as i16 as i32;
            let v4 = read(4) as i16 as i32;
            state.field_74 = ((v1 << 24) as u32 | 0x4000_0000)
                .wrapping_add((v2) as u32)
                .wrapping_add((v3 as u32) << 8)
                .wrapping_add((v4 as u32) << 16);
            state.field_78 = read(5);
            size = 6;
        }
        // 0x0D - write tween_src_x = op[1] << 3. size 2.
        0x0D => {
            state.tween_src_x = (read(1) as i16).wrapping_shl(3);
            size = 2;
        }
        // 0x0E - write field_72 = op[1]. size 2.
        0x0E => {
            state.field_72 = read(1);
            size = 2;
        }
        // 0x0F - write tween_src_y = op[1] << 3.
        0x0F => {
            state.tween_src_y = (read(1) as i16).wrapping_shl(3);
            size = 2;
        }
        // 0x10 - write field_42.
        0x10 => {
            state.field_42 = read(1);
            size = 2;
        }
        // 0x11 - write tween_src_z = op[1] << 3.
        0x11 => {
            state.tween_src_z = (read(1) as i16).wrapping_shl(3);
            size = 2;
        }
        // 0x12 - write field_7a.
        0x12 => {
            state.field_7a = read(1);
            size = 2;
        }
        // 0x13 - sub-mode init (16 ops mostly write the descriptor area).
        0x13 => {
            state.move_submode = 2;
            state.move_substate = 4;
            state.flags &= 0xFFFF_FFFD; // clear bit 2
            // The remaining 14 u16 writes target a chunk of fields we model
            // as `anim_block`. Map them generically.
            for i in 1..=15u16 {
                state.anim_block_u16_set((i as usize) * 2, read(i as usize));
            }
            size = 0x10;
        }
        // 0x14 - write four `anim_block` slots `<< 3`. size 5.
        0x14 => {
            for i in 0..4 {
                let v = (read(1 + i) as i16).wrapping_shl(3);
                state.anim_block_u16_set(0x14 + i * 2, v as u16);
            }
            size = 5;
        }
        // 0x15 - write field_52, with the 0x400 bit additionally clearing
        // flags & 0x80.
        0x15 => {
            let v = read(1);
            state.field_52 = v;
            if (v & 0x400) != 0 {
                state.flags &= 0xFFFF_FF7F;
            }
            size = 2;
        }
        // 0x16 - STUB. size 2. Calls FUN_80024C80 which is just `jr ra`.
        0x16 => {
            host.stub_16(state, read(1) as i16);
            size = 2;
        }
        // 0x17 - overlay-resident extension. size 2.
        0x17 => {
            host.ext_17(state, read(1) as i16);
            size = 2;
        }
        // 0x18 - save current PC into field_88, then field_8c = op[1].
        0x18 => {
            state.field_88 = state.pc as u16;
            state.field_8c = read(1);
            size = 2;
        }
        // 0x19 - counter-decrement loop (for 0x18 setup).
        0x19 => {
            let next = state.field_8c.wrapping_sub(1);
            if (state.field_8c & 0x4000) == 0 {
                state.field_8c = next;
                size = 1;
                if next > 40000 {
                    // Branch out: continue iterating, jump back.
                    state.pc = state.field_88 as i16;
                    return StepResult::Advance;
                }
            } else {
                state.pc = state.field_88 as i16;
                return StepResult::Advance;
            }
        }
        // 0x1A / 0x1B - second loop pair.
        0x1A => {
            state.field_8a = state.pc as u16;
            state.field_8e = read(1);
            size = 2;
        }
        0x1B => {
            let next = state.field_8e.wrapping_sub(1);
            if (state.field_8e & 0x4000) == 0 {
                state.field_8e = next;
                size = 1;
                if next > 40000 {
                    state.pc = state.field_8a as i16;
                    return StepResult::Advance;
                }
            } else {
                state.pc = state.field_8a as i16;
                return StepResult::Advance;
            }
        }
        // 0x1C - write field_ca = op[1] << 3.
        0x1C => {
            state.field_ca = (read(1) as i16).wrapping_shl(3) as u16;
            size = 2;
        }
        // 0x1D - global write.
        0x1D => {
            host.global_write_1d(read(1));
            size = 2;
        }
        // 0x1E - write 7 anim_block slots. size 8.
        0x1E => {
            state.move_submode = 4;
            for (n, off) in [
                (1, 0x18u16),
                (2, 0x20),
                (3, 0x22),
                (4, 0x24),
                (5, 0x26),
                (6, 0x28),
                (7, 0x2A),
            ] {
                state.anim_block_u16_set(off as usize, read(n));
            }
            size = 8;
        }
        // 0x1F - write 7 anim_block slots with merged descriptor. size 8.
        0x1F => {
            // Merges current low byte of `+0x9E` into op[1].
            let merged = (state.field_9e & 0xFF) | read(1);
            state.field_9e = merged;
            for (n, off) in [
                (2, 0x04u16),
                (3, 0x06),
                (4, 0xFC),
                (5, 0xFE),
                (6, 0x00),
                (7, 0x02),
            ] {
                state.anim_block_u16_set(off as usize, read(n));
            }
            size = 8;
        }
        // 0x20 - vtable thunk. size 3.
        0x20 => {
            host.ext_20(state, read(1) as i16, read(2) as i16);
            size = 3;
        }
        // 0x21 - face rotation setup. size 7.
        0x21 => {
            let face_id = read(1) as u8;
            state.face_rotation = face_id;
            let params = [read(2), read(3), read(4), read(5)];
            let target = read(6) as i16 as i32;
            host.face_rotation_setup(face_id, params, target);
            size = 7;
        }
        // 0x22 - epilogue shortcut (size 1, like the default break path).
        0x22 => {
            size = 1;
        }
        // 0x23 - table write. size 0xD.
        0x23 => {
            state.move_submode = 2;
            state.move_substate = 4;
            state.flags &= 0xFFFF_FFFD;
            state.field_9e = read(1) | 0x4000;
            // The original writes a 24-bit packed value at +0xA0 from op[2..4],
            // then 9 u16 writes; we approximate as anim_block writes.
            for (n, off) in [
                (5, 0x18u16),
                (6, 0x1A),
                (7, 0x04),
                (8, 0x06),
                (9, 0xFC),
                (10, 0xFE),
                (11, 0x00),
                (12, 0x02),
            ] {
                state.anim_block_u16_set(off as usize, read(n));
            }
            size = 0xD;
        }
        // 0x24 - anim_block additive. size 3.
        0x24 => {
            let v1 = read(1) as i16;
            let v2 = read(2) as i16;
            // Shifts add v1 into +0xA8, +0xAC; v2 into +0xAA, +0xAE.
            for (off, val) in [(0xFC, v1), (0xFE, v2), (0x00, v1), (0x02, v2)] {
                let cur = state.anim_block_u16(off) as i16;
                state.anim_block_u16_set(off, cur.wrapping_add(val) as u16);
            }
            size = 3;
        }
        // 0x25 - child-actor spawn dispatch. size 2.
        0x25 => {
            host.spawn_child(state, read(1) as i16);
            size = 2;
        }
        // 0x26 - write 4 anim_block slots. size 5.
        0x26 => {
            for (n, off) in [(1, 0xFCu16), (2, 0xFE), (3, 0x00), (4, 0x02)] {
                state.anim_block_u16_set(off as usize, read(n));
            }
            size = 5;
        }
        // 0x27 - write 2 anim_block slots. size 3.
        0x27 => {
            state.anim_block_u16_set(0x04, read(1));
            state.anim_block_u16_set(0x06, read(2));
            size = 3;
        }
        // 0x28 - additive scale Z. size 2.
        0x28 => {
            state.tween_scale_z = state.tween_scale_z.wrapping_add(read(1) as i16);
            size = 2;
        }
        // 0x29 - write tween_scale_x = op[1] (no shift). size 2.
        0x29 => {
            state.tween_scale_x = read(1) as i16;
            size = 2;
        }
        // 0x2A - write tween_scale_z = op[1] << 3. size 2.
        0x2A => {
            state.tween_scale_z = (read(1) as i16).wrapping_shl(3);
            size = 2;
        }
        // 0x2B - TWEEN_ABS_TRIPLE. size 4.
        0x2B => {
            state.tween_src_x = read(1) as i16;
            state.tween_src_y = read(2) as i16;
            state.tween_src_z = read(3) as i16;
            size = 4;
        }
        // 0x2C - KEY_BUFFER_ALLOC. size 5.
        0x2C => {
            for (n, slot) in [(1usize, 0usize), (2, 1), (3, 2), (4, 3)] {
                state.keyframe_desc[slot] = read(n);
            }
            let w = state.keyframe_desc[2] as i16;
            let h = state.keyframe_desc[3] as i16;
            let buffer_ptr = if w >= 0x11 {
                let bytes = (w as i32) * (h as i32) * 2;
                let ptr = host.keyframe_alloc(bytes);
                state.field_a8 = ptr;
                ptr
            } else {
                0
            };
            host.keyframe_init(state, buffer_ptr);
            state.field_9c = 1;
            size = 5;
        }
        // 0x2D - WORLD_INC_VARIANT. size 4.
        0x2D => {
            state.tween_src_x = state.tween_src_x.wrapping_add(read(1) as i16);
            state.tween_src_y = state.tween_src_y.wrapping_add(read(2) as i16);
            state.tween_src_z = state.tween_src_z.wrapping_add(read(3) as i16);
            size = 4;
        }
        // 0x2E - TWEEN_SCALE_SET. size 4.
        0x2E => {
            state.tween_scale_x = (read(1) as i16).wrapping_shl(3);
            state.tween_scale_y = (read(2) as i16).wrapping_shl(3);
            state.tween_scale_z = (read(3) as i16).wrapping_shl(3);
            size = 4;
        }
        // 0x2F - overlay extension dispatch.
        0x2F => {
            let sub = read(1);
            // Build an operand window; the extension VM walks `param_2 = op`,
            // i.e. the opcode word itself is at offset 0 and the sub-op at +1.
            let start = state.pc as usize;
            let window = if start < bytecode.len() {
                &bytecode[start..]
            } else {
                &[]
            };
            let result = host.ext_dispatch(state, sub, window);
            size = result.size_u16;
        }
        // 0x30 - KEY_BUFFER_FREE. ends the loop epilogue but advances by 1.
        0x30 => {
            host.keyframe_free(state, state.field_a8);
            state.field_9c = 0;
            // Original goto-jumps to caseD_22 (size 1, then bVar3 stays true).
            size = 1;
        }
        // 0x31 - LFLAG_AND. size 2.
        0x31 => {
            state.local_flags &= read(1);
            size = 2;
        }
        // 0x32 - LFLAG_OR. size 2.
        0x32 => {
            state.local_flags |= read(1);
            size = 2;
        }
        // 0x33 - clear bit 0x40000000 in field_74. size 1.
        0x33 => {
            state.field_74 &= !0x4000_0000u32;
            size = 1;
        }
        // 0x34 - TWEEN_SETUP. size 9.
        0x34 => {
            state.anim_block_u16_set(0x00, read(1)); // +0xAC
            state.anim_block_u16_set(0x04, read(2)); // +0xB0
            state.tween_src_x = read(3) as i16;
            state.tween_src_y = read(4) as i16;
            state.field_9c = read(5) as i16 as i32;
            state.field_a8 = read(6) as i16 as i32;
            state.anim_block_u16_set(0xF8, read(7));
            // Original writes `(int) op[8]` at +0xA8 - we update field_a8 too
            // since the slot overlaps; for the test surface we treat it as
            // the 32-bit value.
            size = 9;
        }
        // 0x35 - WORLD_INC_VARIANT2. size 3.
        0x35 => {
            state.tween_src_x = state.tween_src_x.wrapping_add(read(1) as i16);
            state.tween_src_y = state.tween_src_y.wrapping_add(read(2) as i16);
            size = 3;
        }
        // 0x36 - TWEEN_DURATION_SET. size 3.
        0x36 => {
            state.tween_scale_y = (read(1) as i16).wrapping_shl(3);
            state.tween_scale_z = (read(2) as i16).wrapping_shl(3);
            state.anim_block_u16_set(0x0C, 0); // +0xB8 = 0
            state.anim_block_u16_set(0x0E, 0);
            size = 3;
        }
        // 0x37 - WORLD_SET_VARIANT2. size 3.
        0x37 => {
            state.tween_src_x = read(1) as i16;
            state.tween_src_y = read(2) as i16;
            size = 3;
        }
        // 0x38 - B2_ADD. size 2.
        0x38 => {
            let cur = state.anim_block_u16(0x06) as i16;
            state.anim_block_u16_set(0x06, cur.wrapping_add(read(1) as i16) as u16);
            size = 2;
        }
        // 0x39 - RENDER_BANK_SET (absolute). size 4.
        0x39 => {
            state.render_24 = read(1) as i16;
            state.render_26 = read(2) as i16;
            state.render_28 = read(3) as i16;
            size = 4;
        }
        // 0x3A - flags |= 2. size 1.
        0x3A => {
            state.flags |= 2;
            size = 1;
        }
        // 0x3B - flags &= ~2. size 1.
        0x3B => {
            state.flags &= !2u32;
            size = 1;
        }
        // 0x3C - SCRATCH_WRITE. size 2 (the original branches out via the
        // alloc-on-first-use path; we keep the simple shape and leave the
        // alloc-call to the host via keyframe_alloc).
        0x3C => {
            state.move_submode = 6;
            state.y_rot = 0;
            state.field_68 = 0;
            state.field_5c = 0;
            // NB: full opcode body iterates `count` slots of 6 u16s each;
            // we surface the count and let hosts that need the inner loop
            // read it from anim_block themselves. The safe forward-progress
            // size is 2 + count * 6 worst-case; the runtime does this in a
            // double-loop body. For port correctness we mirror the original
            // PC advance, walking the operand stream.
            let count = read(1) as i16 as i32;
            // The original allocates a buffer if `actor[+0x4C] == 0`; we
            // fold into keyframe_alloc with `bytes = count << 5 | 8`.
            if state.field_a8 == 0 && count > 0 {
                state.field_a8 = host.keyframe_alloc(count.wrapping_shl(5) | 8);
            }
            state.anim_block_u16_set(0x18, state.pc as u16); // mirror +0xCC = pc
            size = 2 + count.max(0) as i16 * 6;
        }
        // 0x3D - anim interpolate. size 3 (variable sub-loops in original).
        0x3D => {
            state.y_rot = 0;
            state.anim_block_u16_set(0x1A, state.pc as u16); // +0xCE = pc
            size = 3 + (read(2) as i16).max(0) * 6;
        }
        // 0x3E - write +0x22. size 2.
        0x3E => {
            state.y_rot = read(1) as i16;
            size = 2;
        }
        // 0x3F - write anim_block +0xD0 slot. size 2.
        0x3F => {
            state.anim_block_u16_set(0x1C, read(1));
            size = 2;
        }
        // 0x40 - VRAM MoveImage strip-frame copy (FUN_80058490). size 7.
        0x40 => {
            let block = [read(1), read(2), read(3), read(4)];
            host.move_image(block, read(5) as i16, read(6) as i16);
            size = 7;
        }
        // 0x41 - write anim_block +0xB2 slot. size 2.
        0x41 => {
            state.anim_block_u16_set(0x06, read(1));
            size = 2;
        }
        // 0x42 - anim_block init variant. size 0xF.
        0x42 => {
            state.move_substate = 4;
            state.move_submode = 2;
            state.flags &= 0xFFFF_FFFD;
            state.field_9e = read(1) | 0x2000;
            for (n, off) in [
                (2, 0xF8u16),
                (3, 0xC4),
                (4, 0x08),
                (5, 0x0A),
                (6, 0x0C),
                (7, 0x0E),
                (8, 0xFC),
            ] {
                state.anim_block_u16_set(off as usize, read(n));
            }
            size = 0xF;
        }
        // 0x43 - `actor[+0x86] |= 0x2000`. size 1.
        0x43 => {
            state.field_86 |= 0x2000;
            size = 1;
        }
        // 0x44 - triplet write. size 4.
        0x44 => {
            state.field_9e = read(1);
            state.field_68 = read(2) as i16;
            state.field_6a = (read(3) as i16).wrapping_shl(3);
            size = 4;
        }
        // 0x45 - anim_block + sub_mode = 7. size 8.
        0x45 => {
            state.move_submode = 7;
            for (n, off) in [
                (1, 0x14u16),
                (2, 0x18),
                (3, 0x1A),
                (4, 0x20),
                (5, 0x22),
                (6, 0x1C),
                (7, 0x1E),
            ] {
                state.anim_block_u16_set(off as usize, read(n));
            }
            size = 8;
        }
        // 0x46 - TWEEN_INIT. size 4.
        0x46 => {
            state.tween_src_z = state.tween_src_x;
            state.tween_scale_x = state.tween_src_y;
            state.tween_scale_y = (read(1) as i16).wrapping_sub(state.tween_src_x);
            state.tween_scale_z = (read(2) as i16).wrapping_sub(state.tween_src_y);
            // anim_block +0xB8 = 1, +0xC0 = (i32) op[3].
            state.anim_block_u16_set(0x0C, 1);
            state.anim_block_u16_set(0x14, read(3));
            size = 4;
        }
        _ => {
            // Bound check above caught >= 0x47; remaining opcodes in 0x00..=0x46
            // are exhaustively listed. Anything reaching here is a logic bug.
            return StepResult::EndOfBuffer { opcode };
        }
    }

    state.pc = state.pc.wrapping_add(size);
    outcome
}

/// Run the VM in a tick loop until it breaks (`Halt`, `Wait`, `EndOfBuffer`,
/// `Pending`, or budget exhaustion). Mirrors the per-frame entry-point in
/// `FUN_80021DF4 → FUN_80023070`.
///
/// `budget` caps the number of opcodes per frame so a buggy script can't hang
/// the engine. The original has no explicit cap (relies on opcodes naturally
/// breaking), but a cap is the only safe thing for a software port.
pub fn run_until_break<H: MoveHost + ?Sized>(
    host: &mut H,
    state: &mut ActorState,
    bytecode: &[u16],
    budget: usize,
) -> StepResult {
    for _ in 0..budget {
        match step(host, state, bytecode) {
            StepResult::Advance => continue,
            other => return other,
        }
    }
    StepResult::Pending { opcode: 0xFFFF }
}

/// Outcome of one [`actor_tick`] call. Mirrors the move-VM-relevant control
/// flow at `FUN_80021DF4 + 0x800..0x83C`: gate on `wait_timer`, run the VM,
/// inspect the `HALT` flag bit on return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorTickOutcome {
    /// `wait_timer >= 0` - the VM was not entered this frame. The retail
    /// `bgez` at `0x80022B9C` skips the move-VM call. Engines run their
    /// post-tick work normally; the actor is just animating in place.
    Waiting,
    /// VM ran and exited via `0x08` HALT (`actor.flags & 0x8` set). Retail
    /// branches to its halt-handler at `0x80023040` after seeing this bit.
    Halted,
    /// VM ran and exited via `0x09` WAIT_SET - the wait timer has been
    /// re-seeded; further opcodes deferred to next frame.
    WaitSeeded,
    /// VM ran past the bytecode buffer (out-of-range opcode `>= 0x47`).
    /// Retail's bound check (`sltiu v0, v1, 0x47`) terminates the dispatch
    /// loop. Engines normally treat this as "script finished".
    EndOfBuffer { opcode: u16 },
    /// VM exhausted its per-frame opcode budget without breaking. Retail
    /// has no explicit cap; this is a defensive port-only outcome.
    BudgetExhausted,
    /// VM hit an opcode the port hasn't implemented yet. Retail would
    /// dispatch normally; engines decide whether to log + skip or panic.
    Pending { opcode: u16 },
}

/// Per-frame actor advance, ported from the move-VM-relevant slice of
/// `FUN_80021DF4` (lines `80022B94..80022BBC` in the dump).
///
/// Retail behaviour:
///
/// 1. Pre-tick (caller's responsibility - see [`decrement_wait_timer`]):
///    `actor[+0x54] -= delta` (delta is the product of two scratchpad
///    speed scalars).
/// 2. **Move-VM gate**: `if (wait_timer >= 0) skip; else run VM`. The retail
///    bgez at 0x80022B9C is the canonical gate.
/// 3. After the VM call: `if (actor.flags & 0x8) goto halt-handler`.
///
/// Engines compose their per-frame work around this - pre-move integration,
/// post-move animation/render - and gate the move-VM step through this
/// function so the wait-timer + HALT semantics stay faithful to retail.
///
/// This is a thin port: the heavy per-frame work (animation interpolation,
/// position integration, GPU primitive emission) is host-side. The function
/// returns a typed outcome the engine can pattern-match on.
pub fn actor_tick<H: MoveHost + ?Sized>(
    host: &mut H,
    state: &mut ActorState,
    bytecode: &[u16],
    budget: usize,
) -> ActorTickOutcome {
    // Move-VM gate. Retail: `bgez wait_timer, skip-vm`. We branch on the
    // signed value: only step when timer is strictly negative.
    if state.wait_timer >= 0 {
        return ActorTickOutcome::Waiting;
    }

    let result = run_until_break(host, state, bytecode, budget);

    // Post-call flag check. Retail: `andi v0, flags, 0x8; bne v0, zero,
    // halt-handler`. The HALT opcode already sets `flags |= 0x8` inside
    // `step` (see op 0x08). We mirror retail's branch by reading the bit
    // back here so callers don't need to.
    if state.flags & 0x8 != 0 {
        return ActorTickOutcome::Halted;
    }

    match result {
        StepResult::Wait => ActorTickOutcome::WaitSeeded,
        StepResult::EndOfBuffer { opcode } => ActorTickOutcome::EndOfBuffer { opcode },
        StepResult::Pending { opcode: 0xFFFF } => ActorTickOutcome::BudgetExhausted,
        StepResult::Pending { opcode } => ActorTickOutcome::Pending { opcode },
        // Halt was already handled above via the flag check (defensive).
        StepResult::Halt => ActorTickOutcome::Halted,
        // Advance shouldn't escape run_until_break, but match exhaustively.
        StepResult::Advance => ActorTickOutcome::BudgetExhausted,
    }
}

/// Pre-tick wait-timer decrement, ported from the head of `FUN_80021DF4`
/// (line `param_1 + 0x54 -= ...`).
///
/// Retail does:
///
/// ```text
///   actor[+0x54] -= (ushort)DAT_1F800393 * (ushort)DAT_1F80037D;
/// ```
///
/// where the two factors are scratchpad speed scalars (per-actor anim speed
/// × global frame-rate compensation). Engines compute their own `delta` -
/// however they expose those scalars - and pass it here.
///
/// The cast back to `i16` matches retail's `*(ushort *)(param_1 + 0x54)
/// = ...` write-back; the wraparound is intentional and the move-VM gate
/// in [`actor_tick`] interprets the result as `i16`.
pub fn decrement_wait_timer(state: &mut ActorState, delta: u16) {
    // Retail uses unsigned subtraction with `ushort` truncation. The
    // wrapping i16 sub gives the same bytewise result.
    state.wait_timer = state.wait_timer.wrapping_sub(delta as i16);
}

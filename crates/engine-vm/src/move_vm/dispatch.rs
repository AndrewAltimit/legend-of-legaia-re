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
            // `op[1] == 0` arms the reset arm *inside* the per-slot loop:
            // each slot's morph weight at `+0xA0 + i*2` is zeroed and the
            // lane-completion bitfield at `+0x7C` is cleared.
            let reset = read(1) == 0;
            for i in 0..count.max(0) {
                let base = (3 + 3 * i) as usize;
                let lane = i as usize;
                // `sb v1, 0xb0(v0)` with `v0 = actor + i` - a **byte** per
                // lane, so the indices pack 1 byte apart, not 2. In the
                // retail record that leaves room for 8 before the array runs
                // into the `+0xB8` up-ramp curves.
                let descriptor_byte = read(base) as u8;
                state.anim_block_u8_set(0x04 + lane, descriptor_byte);

                // `+0xB8 + i*2` and `+0xC8 + i*2` - per-lane ramp velocities
                // (`move_buffer::MoveBufferState::up_velocity` /
                // `down_velocity`), not one global rate.
                let raw_b8 = read(base + 1) as i16 as i32;
                let scaled_b8 = (raw_b8 * curve_mul) >> 3;
                state.anim_block_u16_set(0x0C + lane * 2, scaled_b8 as u16);

                let raw_c8 = read(base + 2) as i16 as i32;
                let scaled_c8 = (raw_c8 * curve_mul) >> 3;
                state.anim_block_u16_set(0x1C + lane * 2, scaled_c8 as u16);

                if reset {
                    state.zero_keyframe_weight(lane);
                    state.field_7c = 0;
                }

                size += 3;
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
        // 0x13 - draw-kind-4 multi-target node, default emitter. size 0x10.
        // Arm `0x80023454..0x80023524`: `+0x5A = 2`, `+0x56 = 4`, flag `0x2`
        // cleared, `+0x9E = v1` (no `0x2000` / `0x4000` bit, so
        // `FUN_8001ADA4` case 4 takes its default emitter), `+0x9C = v2`,
        // `+0xC8 = v3 << 3`, the two packed words `+0xA0` / `+0xA4` from
        // `v4..v6` / `v7..v9`, then `+0xB4..+0xBE = v10..v15`.
        0x13 => {
            state.move_submode = 2;
            state.move_substate = 4;
            state.flags &= 0xFFFF_FFFD; // clear bit 2
            state.set_actor_u16(0x9E, read(1));
            state.set_actor_u16(0x9C, read(2));
            state.set_actor_u16(0xC8, (read(3) as i16).wrapping_shl(3) as u16);
            state.set_actor_u32(0xA0, packed_word(read(4), read(5), read(6)));
            state.set_actor_u32(0xA4, packed_word(read(7), read(8), read(9)));
            for (n, off) in (10..=15).zip((0xB4..=0xBE).step_by(2)) {
                state.set_actor_u16(off, read(n));
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
        // 0x19 - counter-decrement loop (for 0x18 setup). Retail: with the
        // 0x4000 bit clear, decrement and LOOP while the new count has not
        // underflowed (`uVar8 <= 40000`); an underflow past zero retires
        // with size 1. The 0x4000 bit marks an infinite loop (never
        // decrements, always jumps back) - the ambient effect records'
        // `0x18 0x4000` idle loops. A counter of N runs the body N+1 times.
        0x19 => {
            if (state.field_8c & 0x4000) == 0 {
                let next = state.field_8c.wrapping_sub(1);
                state.field_8c = next;
                size = 1;
                if next <= 40000 {
                    state.pc = (state.field_88 as i16).wrapping_add(2);
                    return StepResult::Advance;
                }
            } else {
                state.pc = (state.field_88 as i16).wrapping_add(2);
                return StepResult::Advance;
            }
        }
        // 0x1A / 0x1B - second loop pair.
        0x1A => {
            state.field_8a = state.pc as u16;
            state.field_8e = read(1);
            size = 2;
        }
        // 0x1B - mirror of 0x19 on the second register pair (see above for
        // the retail loop/retire law).
        0x1B => {
            if (state.field_8e & 0x4000) == 0 {
                let next = state.field_8e.wrapping_sub(1);
                state.field_8e = next;
                size = 1;
                if next <= 40000 {
                    state.pc = (state.field_8a as i16).wrapping_add(2);
                    return StepResult::Advance;
                }
            } else {
                state.pc = (state.field_8a as i16).wrapping_add(2);
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
            // `0x800236F4..0x80023758`: `+0xB0/+0xB2/+0xA8/+0xAA/+0xAC/+0xAE`.
            for (n, off) in [
                (2, 0xB0),
                (3, 0xB2),
                (4, 0xA8),
                (5, 0xAA),
                (6, 0xAC),
                (7, 0xAE),
            ] {
                state.set_actor_u16(off, read(n));
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
        // 0x23 - draw-kind-4 sprite node (`+0x9E | 0x4000`). size 0xD.
        0x23 => {
            state.move_submode = 2;
            state.move_substate = 4;
            state.flags &= 0xFFFF_FFFD;
            state.field_9e = read(1) | 0x4000;
            // Arm `0x800237D8..0x80023888`: the packed word `+0xA0` from
            // `v2..v4`, then `+0xB4/+0xB6/+0xB0/+0xB2` and the four
            // `+0xA8..+0xAE` halfwords the `0x4000` sprite arm reads.
            state.set_actor_u32(0xA0, packed_word(read(2), read(3), read(4)));
            for (n, off) in [
                (5, 0xB4),
                (6, 0xB6),
                (7, 0xB0),
                (8, 0xB2),
                (9, 0xA8),
                (10, 0xAA),
                (11, 0xAC),
                (12, 0xAE),
            ] {
                state.set_actor_u16(off, read(n));
            }
            size = 0xD;
        }
        // 0x24 - anim_block additive. size 3.
        0x24 => {
            let v1 = read(1) as i16;
            let v2 = read(2) as i16;
            // Shifts add v1 into +0xA8, +0xAC; v2 into +0xAA, +0xAE.
            for (off, val) in [(0xA8, v1), (0xAA, v2), (0xAC, v1), (0xAE, v2)] {
                let cur = state.actor_u16(off) as i16;
                state.set_actor_u16(off, cur.wrapping_add(val) as u16);
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
            for (n, off) in [(1, 0xA8), (2, 0xAA), (3, 0xAC), (4, 0xAE)] {
                state.set_actor_u16(off, read(n));
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
        // 0x34 - TWEEN_SETUP. size 9. Arm 0x80023B64 (jump-table word
        // 0x80010848) stores every operand through `s4 = actor + 0x90`: the
        // `lh` operands go out as sign-extended **words** (`sw`), the `lhu`
        // ones as halfwords. Word stores: op[1] -> +0xAC, op[5] -> +0x9C,
        // op[6] -> +0xA0, op[7] -> +0xA4, op[8] -> +0xA8; halfwords: op[2] ->
        // +0xB0, op[3] -> +0x90, op[4] -> +0x92.
        0x34 => {
            let word = |n: usize| read(n) as i16 as i32 as u32;
            state.set_actor_u32(0xAC, word(1));
            state.set_actor_u16(0xB0, read(2));
            state.tween_src_x = read(3) as i16;
            state.tween_src_y = read(4) as i16;
            // A word store at +0x9C also writes +0x9E (the sign half).
            state.set_actor_u32(0x9C, word(5));
            state.set_actor_u32(0xA0, word(6));
            state.set_actor_u32(0xA4, word(7));
            state.set_actor_u32(0xA8, word(8));
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
        // 0x3C - keyframe-pose seat, size `2 + count * 6`
        // (`0x80023CA4..0x80023D60`: the part count goes to the model list's
        // count word (`*(+0x44)`, here the pose's length), `+0x5A = 6` (the
        // keyframe-mesh mode), the cursor `+0x22`, `+0x68` and `+0x5C` clear,
        // the pose block behind `+0x4C` is allocated once (`count << 5 | 8`
        // bytes) and `+0xCC = PC`, `+0xCE = +0xD0 = +0xD2 = 0`. Each part's
        // six operands seed both its current and its target keyframe.
        //
        // PORT: FUN_80023070 (op `0x3C`, `0x80023CA4..0x80023D60`)
        0x3C => {
            state.move_submode = 6;
            state.y_rot = 0;
            state.field_68 = 0;
            state.field_5c = 0;
            let count = (read(1) as i16).max(0) as usize;
            state.set_actor_u16(0xCC, state.pc as u16);
            state.set_actor_u16(0xCE, 0);
            state.set_actor_u16(0xD0, 0);
            state.set_actor_u16(0xD2, 0);
            state.keyframe_pose = (0..count)
                .map(|t| {
                    let mut kf = [0i16; 12];
                    for k in 0..6 {
                        let v = read(2 + t * 6 + k) as i16;
                        kf[k] = v;
                        kf[6 + k] = v;
                    }
                    kf
                })
                .collect();
            size = 2 + count as i16 * 6;
        }
        // 0x3D - next keyframe (`0x80023D64..0x80023F18`). `+0xD0` takes the
        // cursor rate `op[1]`; when a keyframe is already latched
        // (`+0xCE != 0`) every part's current keyframe is first moved to
        // where the cursor has blended it (`cur += (tgt - cur) * +0x22 >> 12`,
        // all six halfwords) and `+0xCC = +0xCE`, `+0xD2 = 1`. Then the
        // cursor clears, `+0xCE = PC`, and the `op[2]` parts' six operands
        // become their new targets.
        //
        // PORT: FUN_80023070 (op `0x3D`, `0x80023D64..0x80023F18`)
        0x3D => {
            state.set_actor_u16(0xD0, read(1));
            let count = (read(2) as i16).max(0) as usize;
            if state.actor_u16(0xCE) != 0 {
                state.set_actor_u16(0xCC, state.actor_u16(0xCE));
                state.set_actor_u16(0xD2, 1);
                let cursor = i32::from(state.y_rot);
                for kf in state.keyframe_pose.iter_mut().take(count) {
                    for k in 0..6 {
                        let d =
                            (i32::from(kf[6 + k]) - i32::from(kf[k])).wrapping_mul(cursor) >> 12;
                        kf[k] = kf[k].wrapping_add(d as i16);
                    }
                }
            }
            state.y_rot = 0;
            state.set_actor_u16(0xCE, state.pc as u16);
            for (t, kf) in state.keyframe_pose.iter_mut().take(count).enumerate() {
                for k in 0..6 {
                    kf[6 + k] = read(3 + t * 6 + k) as i16;
                }
            }
            size = 3 + count as i16 * 6;
        }
        // 0x3E - write +0x22. size 2.
        0x3E => {
            state.y_rot = read(1) as i16;
            size = 2;
        }
        // 0x3F - the keyframe cursor rate `+0xD0` (`sh v0,0x50(s1)` with
        // `s1 = actor + 0x80`, `0x80023F2C`). size 2.
        0x3F => {
            state.set_actor_u16(0xD0, read(1));
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
        // 0x42 - draw-kind-4 ribbon node (`+0x9E | 0x2000`). size 0xF.
        0x42 => {
            state.move_substate = 4;
            state.move_submode = 2;
            state.flags &= 0xFFFF_FFFD;
            state.field_9e = read(1) | 0x2000;
            // Arm `0x80023F94..0x80024058` (`s1 = actor + 0x80`): `+0x9C = v2`,
            // `+0xC8 = v3` (no shift, unlike op `0x13`), `+0xB4..+0xBA =
            // v4..v7`, `+0xA8 = v8`, and the two packed colour words `+0xA0` /
            // `+0xA4` from `v9..v11` / `v12..v14` - every field the ribbon
            // emitter `FUN_801CFA48` reads off the actor.
            for (n, off) in [
                (2, 0x9C),
                (3, 0xC8),
                (4, 0xB4),
                (5, 0xB6),
                (6, 0xB8),
                (7, 0xBA),
                (8, 0xA8),
            ] {
                state.set_actor_u16(off, read(n));
            }
            state.set_actor_u32(0xA0, packed_word(read(9), read(10), read(11)));
            state.set_actor_u32(0xA4, packed_word(read(12), read(13), read(14)));
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

/// The part tick's **channel integration** for render modes `2` and `6` - the
/// block of `FUN_80021DF4` right after the `+0x22` spin (`0x80021E78..0x80021FA0`,
/// taken when `actor[+0x5A]` is `2` or `6`, with `s6 = actor + 0x80`).
///
/// Five halfword channels step by their rate halfwords, each rate scaled by the
/// two scratchpad speed bytes and shifted `>> 6`:
///
/// ```text
///   +0xB4 += (s16 +0xC0 * DAT_1F800393 * DAT_1F80037D) >> 6
///   +0xB6 += (s16 +0xC2 * ...) >> 6
///   +0xB8 += (s16 +0xC4 * ...) >> 6
///   +0xBA += (s16 +0xC6 * ...) >> 6
///   +0xC8 += (s16 +0xCA * ...) >> 6;  if (s16)+0xC8 < 0 then +0xC8 = 0
/// ```
///
/// `delta` is the product of the two speed bytes, the same factor
/// [`decrement_wait_timer`] takes. On a draw-kind-4 ribbon node (move-VM op
/// `0x42`) the channels are the emitter's radius, step length, RNG seed, turn
/// rate and step total, so `+0xC8` growing is what extends a bolt over its
/// life; the clamp keeps a shrinking one from wrapping.
///
/// PORT: FUN_80021DF4 (`0x80021E78..0x80021FA0`, the mode-`2`/`6` channel block)
pub fn integrate_draw_channels(state: &mut ActorState, delta: u16) {
    if state.move_submode != 2 && state.move_submode != 6 {
        return;
    }
    let delta = i32::from(delta);
    for (dst, rate) in [
        (0xB4, 0xC0),
        (0xB6, 0xC2),
        (0xB8, 0xC4),
        (0xBA, 0xC6),
        (0xC8, 0xCA),
    ] {
        let step = (i32::from(state.actor_u16(rate) as i16).wrapping_mul(delta)) >> 6;
        let v = state.actor_u16(dst).wrapping_add(step as u16);
        state.set_actor_u16(dst, v);
    }
    if (state.actor_u16(0xC8) as i16) < 0 {
        state.set_actor_u16(0xC8, 0);
    }
}

/// The part tick's keyframe-cursor step, right after the wait drain and ahead
/// of the mode dispatch, in every mode: `+0x22 += (s16)+0xD0 * DAT_1F800393`
/// (the frame step alone, not the speed product the drain uses). `+0xD0` is
/// the rate ops `0x3D` / `0x3F` set; it is zero on any part that never issues
/// one, so the step is a no-op there.
///
/// PORT: FUN_80021DF4 (`0x80021E50..0x80021E74`, the `+0x22` step)
pub fn advance_keyframe_cursor(state: &mut ActorState, frame_step: u8) {
    let step = i32::from(state.actor_u16(0xD0) as i16).wrapping_mul(i32::from(frame_step));
    state.y_rot = state.y_rot.wrapping_add(step as i16);
}

/// The part tick's mode-`6` render tail: each part of the op-`0x3C` pose
/// block blended between its current and target keyframe by the cursor
/// `+0x22` (`cur + ((tgt - cur) * cursor >> 12)`) and packed into the 8-byte
/// clip entry the animated renderer `FUN_8001B964` decodes (`FUN_8001BE80`):
/// halfwords 3 / 4 / 5 are the 12-bit X / Y / Z translation, halfword 1
/// `>> 4` is the X **and** Z rotation byte, halfword 2 `>> 4` the Y rotation
/// byte; halfword 0 is blended by op `0x3D` but never packed. Retail also
/// stamps the block's header (part count, one frame, rate 1), which is what
/// makes `+0x4C` a one-frame clip of the model list's part count.
///
/// Empty unless `+0x5A == 6` and the pose block is seated (the tail's own
/// `+0x4C != 0` gate).
///
/// PORT: FUN_80021DF4 (`0x80022EFC..0x8002303C`, the mode-`6` pack)
pub fn keyframe_pose_entries(state: &ActorState) -> Vec<[u8; 8]> {
    if state.move_submode != 6 {
        return Vec::new();
    }
    let cursor = i32::from(state.y_rot);
    state
        .keyframe_pose
        .iter()
        .map(|kf| {
            let c = |k: usize| -> i32 {
                let (cur, tgt) = (i32::from(kf[k]), i32::from(kf[6 + k]));
                cur + ((tgt - cur).wrapping_mul(cursor) >> 12)
            };
            let (r_xz, r_y) = (c(1) >> 4, c(2) >> 4);
            let (x, y, z) = (c(3), c(4), c(5));
            [
                x as u8,
                y as u8,
                (((x >> 8) & 0xF) + ((y >> 4) & 0xF0)) as u8,
                z as u8,
                ((z >> 8) & 0xF) as u8,
                r_xz as u8,
                r_y as u8,
                r_xz as u8,
            ]
        })
        .collect()
}

/// The 24-bit packed word ops `0x13` / `0x23` / `0x42` build from three
/// operands: each is loaded **sign-extended** (`lh`) and the three are summed
/// as `a + (b << 8) + (c << 16)` with no masking, so a negative operand
/// borrows from the byte above it exactly as retail's `addu` chain does.
fn packed_word(a: u16, b: u16, c: u16) -> u32 {
    let (a, b, c) = (a as i16 as i32, b as i16 as i32, c as i16 as i32);
    a.wrapping_add(b.wrapping_shl(8))
        .wrapping_add(c.wrapping_shl(16)) as u32
}

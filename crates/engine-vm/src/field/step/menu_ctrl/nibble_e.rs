//! `0x4C` MENU_CTRL outer-nibble 0xE handler. Extracted verbatim from `op_4c`.

use super::*;

// Outer nibble 0xE - misc scene writes + emitter helper calls.
// All sub-ops 0x0..=0xE are ported: sub-0 (3-way state write,
// halt at PC), sub-1 (variable-length text balloon), sub-2
// (set globals, 6-byte), sub-3 (camera-anchored teleport,
// 2-byte), sub-4 (bbox-test halt-or-advance, 9-byte), sub-5
// (XP add, 5-byte), sub-6 (FUN_801D8280, 8-byte), sub-7
// (camera animate, 7-byte), sub-8 (camera zoom, 10-byte),
// sub-9 (clear b9c4, 2-byte), sub-0xA (call c7ec then halt),
// sub-0xB (actor lookup + conditional jump, 5-byte), sub-0xC
// (capture FUN_801DDF48, 2-byte), sub-0xD (set ba66, 3-byte),
// sub-0xE (snapshot 84570, 2-byte). Sub-0xF has no `case` arm
// in the original and falls through to the default halt.
pub(super) fn op_4c_ne<H: FieldHost>(
    host: &mut H,
    ctx: &mut FieldCtx,
    bytecode: &[u8],
    pc: usize,
    opcode: u8,
    header_size: usize,
    operand: usize,
    op0: u8,
) -> StepResult {
    match op0 & 0x0F {
        // Sub-0: 2-byte `[4C, 0xE0, b1]`. 3-way write (host
        // performs based on b1 value); halt at PC.
        0 => {
            let Some(&b1) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.op4c_n_e_sub0_state_write(b1);
            StepResult::Halt { final_pc: pc }
        }
        // Sub-1: variable-length text balloon. Spawns a
        // screen-anchored text actor when the leading byte is
        // non-zero; PC always advances by `3 + packet_length`
        // (opcode byte + sub-op byte + terminator + payload).
        1 => {
            let Some(&first) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let payload = &bytecode[operand + 1..];
            let length = crate::field_helpers::packet_length(payload);
            if first != 0 {
                host.op4c_n_e_sub_1_text_actor(&payload[..length], ctx.script_id);
            }
            StepResult::Advance {
                next_pc: pc + header_size + 2 + length,
            }
        }
        2 => {
            if operand + 3 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let fmv_id = i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
            host.op4c_n_e_sub2_fmv_trigger(fmv_id);
            StepResult::Advance {
                next_pc: pc + header_size + 5,
            }
        }
        // Sub-3: 2-byte `[4C, 0xE3, actor_id]`. Camera-anchored
        // teleport: copy active camera position+rotation onto
        // the resolved actor. Dispatcher lines 7208-7227. PC
        // advances by 2; missing actor is a silent no-op.
        3 => {
            let Some(&actor_id) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.op4c_n_e_sub_3_actor_sync_camera(ctx, actor_id);
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }
        // Sub-7: 7-byte `[4C, 0xE7, t0, t1, t2, d0, d1]`.
        // Camera animate: target (24-bit LE) at +1 and duration
        // (16-bit LE) at +4. Dispatcher lines 7281-7297. PC
        // advances by 7.
        7 => {
            if operand + 6 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let target = crate::field_helpers::load_u24_le(&bytecode[operand + 1..]);
            let duration = crate::field_helpers::load_u16_le(&bytecode[operand + 4..]);
            host.op4c_n_e_sub_7_camera_animate(target, duration);
            StepResult::Advance {
                next_pc: pc + header_size + 6,
            }
        }
        // Sub-8: 10-byte `[4C, 0xE8, x0, x1, y0, y1, z0, z1, m0,
        // m1]`. Camera zoom: four 16-bit LE values for zoom_x,
        // zoom_y, zoom_z, mode. Dispatcher lines 7298-7361 reads
        // `func_0x8003ce9c` four times at offsets +1/+3/+5/+7.
        // PC advances by 10.
        8 => {
            if operand + 9 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let zoom_x = crate::field_helpers::load_u16_le(&bytecode[operand + 1..]) as i16;
            let zoom_y = crate::field_helpers::load_u16_le(&bytecode[operand + 3..]) as i16;
            let zoom_z = crate::field_helpers::load_u16_le(&bytecode[operand + 5..]) as i16;
            let mode = crate::field_helpers::load_u16_le(&bytecode[operand + 7..]) as i16;
            host.op4c_n_e_sub_8_camera_zoom(zoom_x, zoom_y, zoom_z, mode);
            StepResult::Advance {
                next_pc: pc + header_size + 9,
            }
        }
        // Sub-4: 9-byte `[4C, 0xE4, x0, z0, x1, z1, scale, ?, ?]`.
        // BBox collision query. Each operand byte goes through
        // the standard tile-center conversion (`(b & 0x7F) * 0x80
        // + 0x40`, plus 0x40 if the high bit is set). When the
        // host predicate says "outside", the original calls the
        // halt helper FUN_801E3614; we model that as Halt at PC.
        // When inside, advance PC by 8.
        4 => {
            if operand + 8 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let bbox = [
                crate::field_helpers::tile_center(bytecode[operand + 1]),
                crate::field_helpers::tile_center(bytecode[operand + 2]),
                crate::field_helpers::tile_center(bytecode[operand + 3]),
                crate::field_helpers::tile_center(bytecode[operand + 4]),
            ];
            if host.op4c_n_e_sub_4_bbox_outside(ctx, bbox) {
                StepResult::Halt { final_pc: pc }
            } else {
                StepResult::Advance {
                    next_pc: pc + header_size + 8,
                }
            }
        }
        // Sub-5: 5-byte `[4C, 0xE5, b1, b2, b3]`. Read 24-bit
        // signed XP delta via load_u24_le + sign_extend_24, then
        // call the host's add-xp hook. PC += 4.
        5 => {
            if operand + 4 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let raw = crate::field_helpers::load_u24_le(&bytecode[operand + 1..]);
            let xp_delta = crate::field_helpers::sign_extend_24(raw);
            host.op4c_n_e_sub_5_add_xp(xp_delta);
            StepResult::Advance {
                next_pc: pc + header_size + 4,
            }
        }
        6 => {
            if operand + 7 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let mut words = [0i16; 3];
            for (i, w) in words.iter_mut().enumerate() {
                *w = i16::from_le_bytes([
                    bytecode[operand + 1 + i * 2],
                    bytecode[operand + 2 + i * 2],
                ]);
            }
            host.op4c_n_e_sub6_call_d8280(words);
            StepResult::Advance {
                next_pc: pc + header_size + 7,
            }
        }
        // Sub-9: 1-byte. Clear `_DAT_8007B9C4` then PC += 2 via
        // `caseD_4` (the standard `addiu s8, s8, 0x2; j epilogue`
        // block at 0x801df098).
        9 => {
            host.op4c_n_e_sub9_clear_b9c4();
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }
        // Sub-A: 1-byte. Call overlay-resident `func_0x8003C7EC`,
        // halt at PC.
        0xA => {
            host.op4c_n_e_sub_a_call_c7ec();
            StepResult::Halt { final_pc: pc }
        }
        0xC => {
            host.op4c_n_e_sub_c_capture_ddf48();
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }
        0xD => {
            let Some(&b1) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.op4c_n_e_sub_d_set_ba66(b1);
            StepResult::Advance {
                next_pc: pc + header_size + 2,
            }
        }
        // Sub-B: 5-byte `[4C, 0xEB, actor_id, target_lo, target_hi]`.
        // Conditional actor lookup with embedded jump target.
        // When the host resolves the actor, advance PC by 5;
        // otherwise jump to absolute `LE_u16(operand+2..=operand+3)`.
        0xB => {
            if operand + 4 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let actor_id = bytecode[operand + 1];
            match host.op4c_n_e_sub_b_actor_jump(actor_id) {
                Some(()) => StepResult::Advance {
                    next_pc: pc + header_size + 4,
                },
                None => {
                    let target = crate::field_helpers::load_u16_le(&bytecode[operand + 2..]);
                    StepResult::Advance {
                        next_pc: target as usize,
                    }
                }
            }
        }
        0xE => {
            host.op4c_n_e_sub_e_snapshot_84570();
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }
        // Sub-F: no `case` arm in the original; falls through to
        // `switchD_801e00f4::default()` which returns `param_2`
        // (= halt at PC) for outer nibble 0xE opcodes.
        _ => StepResult::Halt { final_pc: pc },
    }
}

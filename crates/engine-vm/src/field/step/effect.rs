//! Field VM opcode `0x34` (EFFECT) sub-dispatcher, extracted verbatim from `step`.

use super::*;

pub(super) fn op_34<H: FieldHost>(
    host: &mut H,
    ctx: &mut FieldCtx,
    bytecode: &[u8],
    pc: usize,
    opcode: u8,
    header_size: usize,
    operand: usize,
) -> StepResult {
    let Some(&op0) = bytecode.get(operand) else {
        return StepResult::Unknown { opcode, pc };
    };
    let sub = op0 >> 4;
    match sub {
        0 => {
            // 7-byte instruction: [op0, r, g, b, intensity_lo, intensity_hi].
            let Some(rgb_int) = bytecode.get(operand + 1..operand + 6) else {
                return StepResult::Unknown { opcode, pc };
            };
            let rgb = [rgb_int[0], rgb_int[1], rgb_int[2]];
            let intensity = i16::from_le_bytes([rgb_int[3], rgb_int[4]]);
            host.op34_sub0_color_intensity_setup(op0, rgb, intensity);
            StepResult::Advance {
                next_pc: pc + header_size + 6,
            }
        }
        1 => {
            // Base instruction is 13 bytes (opcode + 12 operand
            // bytes). The "capture flag" at `pbVar46[0xC]` is the
            // BYTE JUST PAST the instruction - the runtime peeks at
            // the first byte of the next instruction to decide
            // whether to consume it as a capture extension.
            let Some(payload) = bytecode.get(operand + 1..operand + 12) else {
                return StepResult::Unknown { opcode, pc };
            };
            let packed24 =
                ((payload[0] as u32) << 16) | ((payload[1] as u32) << 8) | (payload[2] as u32);
            let world_x = i16::from_le_bytes([payload[3], payload[4]]);
            let world_z = i16::from_le_bytes([payload[5], payload[6]]);
            // The original NEGATES the y component (`local_a6 = -local_a6`)
            // before the spawn call - undo the sign here.
            let raw_neg_y = i16::from_le_bytes([payload[7], payload[8]]);
            let world_y = raw_neg_y.wrapping_neg();
            // Peek the byte AT pc + 13 (first byte after the
            // 13-byte base instruction). When it's 0x40, the
            // runtime treats it as a capture-extension marker and
            // PC advances by an extra `2 + payload_len`.
            let capture_flag = bytecode.get(operand + 12).copied().unwrap_or(0);
            let captured_pc_payload: &[u8] = if capture_flag == 0x40 {
                let payload_len = bytecode.get(operand + 13).copied().unwrap_or(0) as usize;
                let start = operand + 14;
                let end = start + payload_len;
                bytecode.get(start..end).unwrap_or(&[])
            } else {
                &[]
            };
            let delta_from_opcode = host.op34_sub1_spawn_or_skip(
                ctx,
                op0,
                packed24,
                [world_x, world_y, world_z],
                capture_flag,
                captured_pc_payload,
            );
            StepResult::Advance {
                next_pc: pc + delta_from_opcode,
            }
        }
        2 => {
            // sub-2: 3-byte instruction `[34, 0x2N, b1, ...]`. The
            // original walks the actor list at `_DAT_8007C354` looking
            // for an entry with `[+0x90] == iVar18` (current ctx). If
            // found AND `b1 == 0x40`, it captures `pbVar47 + 3` into
            // the matched actor's `+0x94` (a forwarded-PC pointer) and
            // returns via `caseD_4()` (STATE_RESUME → `Yield`).
            // Otherwise it falls through `code_r0x801df098` for PC += 2.
            let Some(&b1) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let captured_pc_offset = pc + header_size + 2;
            let captured = host.op34_capture_pc_for_existing_actor(ctx, b1, captured_pc_offset);
            if captured {
                StepResult::Yield { resume_pc: pc }
            } else {
                StepResult::Advance {
                    next_pc: pc + header_size + 1,
                }
            }
        }
        3 => {
            let Some(&arg) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.effect_anim_trigger(ctx, arg);
            StepResult::Advance {
                next_pc: pc + header_size + 2,
            }
        }
        // Sub-ops 4..=0xF: original has no `case` arm; falls through
        // `if (bVar35 != 2) { if (bVar35 != 3) { return param_2; } }`
        // at line 4811-4814 of the dump ⇒ halt at PC.
        4..=15 => StepResult::Halt { final_pc: pc },
        // `op0 >> 4` is at most 0xF; arms above cover every value.
        16..=u8::MAX => unreachable!("op0 >> 4 is at most 0xF"),
    }
}

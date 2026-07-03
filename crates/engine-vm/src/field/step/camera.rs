//! Field VM opcode `0x45` (CAMERA) sub-dispatcher, extracted verbatim from `step`.

use super::*;

pub(super) fn op_45<H: FieldHost>(
    host: &mut H,
    bytecode: &[u8],
    pc: usize,
    opcode: u8,
    header_size: usize,
    operand: usize,
) -> StepResult {
    let Some(&op0) = bytecode.get(operand) else {
        return StepResult::Unknown { opcode, pc };
    };
    match op0 & 0xC0 {
        0x40 => {
            // LOAD: 18-byte payload after op0.
            let payload_end = operand + 1 + 18;
            if payload_end > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            host.camera_load(&bytecode[operand + 1..payload_end]);
            StepResult::Advance {
                next_pc: pc + header_size + 19,
            }
        }
        0x80 => {
            host.camera_save();
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }
        0xC0 => {
            let Some(&lo) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&hi) = bytecode.get(operand + 2) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.camera_apply();
            let target = u16::from_le_bytes([lo, hi]) as usize;
            StepResult::Advance { next_pc: target }
        }
        0x00 => {
            let Some(&op1) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&trig_lo) = bytecode.get(operand + 2) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&trig_hi) = bytecode.get(operand + 3) else {
                return StepResult::Unknown { opcode, pc };
            };
            let mask = (u16::from(op0) << 8) | u16::from(op1);
            let apply_trigger = u16::from_le_bytes([trig_lo, trig_hi]);
            let mode = (op0 >> 2) & 0x0F;
            // Cursor starts at 4 (past opcode+op0+op1+trigger u16 - i.e.
            // operand + 3, since operand=pc+header_size means
            // operand+3 = pc+header_size+3). The original `iVar18 = 4`
            // is the byte index relative to pbVar47 (= operand), so
            // first param is at operand + 4 in the bytecode.
            let mut cursor = operand + 4;
            let mut params: Vec<CameraParam> = Vec::with_capacity(10);
            for slot in 0u8..10 {
                let bit = 1u16 << (9 - slot);
                if mask & bit == 0 {
                    continue;
                }
                if cursor + 1 >= bytecode.len() {
                    return StepResult::Unknown { opcode, pc };
                }
                let v = u16::from_le_bytes([bytecode[cursor], bytecode[cursor + 1]]);
                params.push(CameraParam { slot, value: v });
                cursor += 2;
            }
            let consumed = cursor - operand; // = 4 + 2 * set_count
            host.camera_configure(&params, apply_trigger, mode);
            StepResult::Advance {
                next_pc: pc + header_size + consumed,
            }
        }
        _ => unreachable!(),
    }
}

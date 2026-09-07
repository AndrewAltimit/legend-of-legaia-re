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
            // APPLY: `[45][C0 | mode<<2][s16 apply_trigger]` - four bytes, and
            // the `s16` is the SAME apply-trigger argument the configure arm
            // (`0x00`) reads at `operand + 2`, not a jump target.
            //
            // Retail's arm is `overlay_0897` `0x801DF210`: it calls
            // `FUN_801DAB90` (apply) + `FUN_801DAA50` (read-back), reads the
            // unaligned `s16` at `operand + 1` through `FUN_8003CE9C`, hands it
            // to `FUN_801DE084(0x801C6EA8, trigger, mode)` - the identical call
            // the configure arm makes - and exits `j 0x801E3624` with
            // `addiu s8, s8, 4` in the delay slot. `s8` is the PC cursor, so
            // the instruction is a plain four-byte advance. The sibling arms
            // pin the same reading: `0x40` LOAD exits `addiu s8, s8, 0x14`
            // (20 bytes) and `0x80` SAVE `addiu s8, s8, 2`.
            //
            // Reading the `s16` as an absolute jump target made every record
            // whose trigger is `0` restart from byte 0 forever - `urudre2`
            // `P2[9]`, whose `45 C0 00 00` sits ~0x670 bytes before the
            // record's `0x3F` -> `map01` tail, replayed its conversation
            // instead of ever leaving the scene.
            let Some(&lo) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&hi) = bytecode.get(operand + 2) else {
                return StepResult::Unknown { opcode, pc };
            };
            let _apply_trigger = i16::from_le_bytes([lo, hi]);
            host.camera_apply();
            StepResult::Advance {
                next_pc: pc + header_size + 3,
            }
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

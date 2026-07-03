//! Field VM control-flow opcode handlers extracted verbatim from `step`.

use super::*;

/// Field VM opcode `0x4E` (inventory compare-and-jump), extracted verbatim from `step`.
pub(super) fn op_4e<H: FieldHost>(
    host: &mut H,
    bytecode: &[u8],
    pc: usize,
    opcode: u8,
    header_size: usize,
    operand: usize,
) -> StepResult {
    let Some(&page) = bytecode.get(operand) else {
        return StepResult::Unknown { opcode, pc };
    };
    let Some(&mode_byte) = bytecode.get(operand + 1) else {
        return StepResult::Unknown { opcode, pc };
    };
    let sub_op = mode_byte >> 4;
    match sub_op {
        0 | 1 => {
            let Some(&arg_lo) = bytecode.get(operand + 2) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&arg_hi) = bytecode.get(operand + 3) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&skip_lo) = bytecode.get(operand + 4) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&skip_hi) = bytecode.get(operand + 5) else {
                return StepResult::Unknown { opcode, pc };
            };
            let arg = i32::from(u16::from_le_bytes([arg_lo, arg_hi]));
            let (state, factor) = host.inventory_compare_pair(page, sub_op);
            let raw = factor.wrapping_mul(arg);
            let scaled = if raw < 0 { (raw + 0xFF) >> 8 } else { raw >> 8 };
            let cmp = mode_byte & 0x0F;
            let taken = match cmp {
                0 => state < scaled,
                1 => scaled < state,
                _ => false,
            };
            if taken {
                StepResult::Advance {
                    next_pc: rel_jump(pc + header_size + 4, skip_lo, skip_hi),
                }
            } else {
                StepResult::Advance {
                    next_pc: pc + header_size + 6,
                }
            }
        }
        2 | 3 | 5 | 6 | 7 | 8 | 9 => {
            let Some(&lo) = bytecode.get(operand + 2) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&hi) = bytecode.get(operand + 3) else {
                return StepResult::Unknown { opcode, pc };
            };
            let target = u16::from_le_bytes([lo, hi]) as usize;
            StepResult::Advance { next_pc: target }
        }
        12..=15 => {
            // sub-ops 12..=15 hit the dispatcher's default arm at
            // `switchD_801e0a38_default`: with `uVar31 = uVar27 = 0`
            // the boolean test is false either way, and the
            // `(sub_op - 10) < 2` check fails for sub-op >= 12, so
            // the original returns `param_2 + 7` (= PC += 7).
            StepResult::Advance {
                next_pc: pc + header_size + 6,
            }
        }
        10 | 11 => {
            // 9-byte party-bank comparison:
            //   [4E, _, mode, lo1, hi1, skip_lo, skip_hi, lo2, hi2]
            // Original packs `LE_u16(operand[2..4])` into the low half
            // of a u32 and `LE_u16(operand[6..8])` into the high half,
            // then compares it (signed) against the bank value.
            let Some(&lo1) = bytecode.get(operand + 2) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&hi1) = bytecode.get(operand + 3) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&skip_lo) = bytecode.get(operand + 4) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&skip_hi) = bytecode.get(operand + 5) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&lo2) = bytecode.get(operand + 6) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&hi2) = bytecode.get(operand + 7) else {
                return StepResult::Unknown { opcode, pc };
            };
            let low = u16::from_le_bytes([lo1, hi1]) as u32;
            let high = u16::from_le_bytes([lo2, hi2]) as u32;
            let scaled = (low | (high << 16)) as i32;
            let state = host.party_bank_value(sub_op);
            let cmp = mode_byte & 0x0F;
            let taken = match cmp {
                0 => state < scaled,
                1 => scaled < state,
                _ => false,
            };
            if taken {
                StepResult::Advance {
                    next_pc: rel_jump(pc + header_size + 4, skip_lo, skip_hi),
                }
            } else {
                StepResult::Advance {
                    next_pc: pc + header_size + 8,
                }
            }
        }
        // Sub-op 4: `iVar18 = func_0x80056798(); return iVar18;` -
        // FUN_80056798 is a BIOS Rand thunk (`jr 0xA0; t1=0x2F`).
        // The original returns the random value as the next PC. There
        // are no captured callers, so this is almost certainly a dev
        // stub. The host hook returns the next-PC value (default 0,
        // matching broken-as-shipped behaviour).
        4 => {
            let next = host.op4e_sub4_bios_rand();
            StepResult::Advance {
                next_pc: next as usize,
            }
        }
        // `mode_byte >> 4` is at most 0xF; arms above cover every
        // value, so this arm is dead code.
        16..=u8::MAX => unreachable!("mode_byte >> 4 is at most 0xF"),
    }
}

/// Field VM opcode `0x49` (STATE_RESUME), extracted verbatim from `step`.
pub(super) fn op_49<H: FieldHost>(
    host: &mut H,
    ctx: &mut FieldCtx,
    bytecode: &[u8],
    pc: usize,
    opcode: u8,
    header_size: usize,
    operand: usize,
) -> StepResult {
    let Some(&sub_op) = bytecode.get(operand) else {
        return StepResult::Unknown { opcode, pc };
    };
    match host.op49_state() {
        Op49State::Idle => {
            if sub_op > 0xD {
                return StepResult::Halt { final_pc: pc };
            }
            // Hand the host the instruction (opcode onward) so it can
            // recognise an inline menu payload (e.g. a shop record) and
            // open the right overlay before the op suspends. `operand`
            // is `pc + header_size`, so `operand - 1` is the opcode byte
            // (works for the extended-script header too).
            host.op49_menu_request(sub_op, &bytecode[operand - 1..]);
            host.op49_invoke_setup();
            let captured = if sub_op == 1 { ctx.field_90 } else { 0 };
            host.op49_arm(pc, captured);
            StepResult::Halt { final_pc: pc }
        }
        Op49State::Armed => StepResult::Halt { final_pc: pc },
        Op49State::Done => {
            host.op49_clear();
            match sub_op {
                // sub-0 in DONE state - embedded MES bytecode walker.
                // Instruction: `[49, 0, length, ...length args..., ...mes_bytes]`.
                // The original reads `length = pbVar47[2]`, then calls
                // `func_0x8003ca38(pbVar47 + length + 3)` (= the MES-shape
                // walker that counts bytes > 0x1E, with one-byte
                // peek-extension for 0xCx prefix bytes), and returns
                // `param_2 + length + 5 + mes_count`.
                0 => {
                    let Some(&length) = bytecode.get(operand + 1) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let mes_start = operand + 2 + length as usize;
                    if mes_start > bytecode.len() {
                        return StepResult::Unknown { opcode, pc };
                    }
                    let mes_count = walk_mes_bytecode(&bytecode[mes_start..]);
                    StepResult::Advance {
                        next_pc: pc + header_size + 4 + length as usize + mes_count,
                    }
                }
                1 | 3 | 7 => StepResult::Advance {
                    next_pc: pc + header_size + 2,
                },
                2 | 4 => StepResult::Advance {
                    next_pc: pc + header_size + 6,
                },
                5 => StepResult::Advance {
                    next_pc: pc + header_size + 13,
                },
                // sub-6/8/9/C/D in Done all jump through LAB_801df898
                // which does `addiu s8, s8, 0x5; j 0x801df89c` -
                // PC += 5 from the opcode (= header_size + 4 past
                // the sub-op byte). The original reads
                // `_DAT_8007babc` into a register but only
                // sub-paths lower in the dispatch (e.g. the 0x4C
                // sub-3 sub-C wrapper) consume it; in the 0x49 Done
                // arm it's used purely as a side-effect register.
                6 | 8 | 9 | 0xC | 0xD => StepResult::Advance {
                    next_pc: pc + header_size + 4,
                },
                // sub-A / sub-B / any other byte > 0xD: the Done-side
                // catch-all in `FUN_801de840 case 0x49` clears the
                // resume slot and returns `param_2` (halt at PC).
                // `op49_clear()` was already called above, so this is
                // just the halt.
                _ => StepResult::Halt { final_pc: pc },
            }
        }
    }
}

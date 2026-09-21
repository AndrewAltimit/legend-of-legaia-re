//! `0x4C` MENU_CTRL outer-nibble 9 + 0xA handlers. Extracted verbatim from `op_4c`.

use super::*;

// Outer nibble 9 - the scene floor-height ladder: rung oscillator +
// whole-ladder install + retire sweep. Sub-0/1/2 (set one rung
// oscillating, 9-byte) and sub-0xE (install all sixteen rungs at
// 0x1F80035C, 34-byte) ported. Sub-0xF (the retire sweep over
// LAB_801da930) remains `Pending`. Sub-3..=0xD have no
// `case` arm in the original (line 6694–6696 of the dump
// returns `param_2` when `2 < uVar27 < 0xE`) - halt at PC.
pub(super) fn op_4c_n9<H: FieldHost>(
    host: &mut H,
    _ctx: &mut FieldCtx,
    bytecode: &[u8],
    pc: usize,
    opcode: u8,
    header_size: usize,
    operand: usize,
    op0: u8,
) -> StepResult {
    {
        let sub = op0 & 0x0F;
        match sub {
            0..=2 => {
                if operand + 8 > bytecode.len() {
                    return StepResult::Unknown { opcode, pc };
                }
                let b1 = bytecode[operand + 1];
                let mut words = [0i16; 3];
                for (i, w) in words.iter_mut().enumerate() {
                    *w = i16::from_le_bytes([
                        bytecode[operand + 2 + i * 2],
                        bytecode[operand + 3 + i * 2],
                    ]);
                }
                host.op4c_n9_sub0_2_dde34(sub, b1, words);
                StepResult::Advance {
                    next_pc: pc + header_size + 8,
                }
            }
            0xE => {
                if operand + 33 > bytecode.len() {
                    return StepResult::Unknown { opcode, pc };
                }
                let mut words = [0i16; 16];
                for (i, w) in words.iter_mut().enumerate() {
                    *w = i16::from_le_bytes([
                        bytecode[operand + 1 + i * 2],
                        bytecode[operand + 2 + i * 2],
                    ]);
                }
                host.op4c_n9_sub_e_table_copy(words);
                StepResult::Advance {
                    next_pc: pc + header_size + 33,
                }
            }
            // Sub-0xF: retire every floor-height-ladder
            // oscillator. The arm at `0x801E2548` tail-jumps to
            // the shared exit `0x801E2DC4`, which is
            // `jal 0x8003CF40` with `addiu s8,s8,2` in the delay
            // slot - a retire sweep and a two-byte advance, with
            // no registration and no park.
            0xF => {
                host.op4c_n9_sub_f_retire_ladder_oscillators();
                StepResult::Advance {
                    next_pc: pc + header_size + 1,
                }
            }
            _ => StepResult::Halt { final_pc: pc },
        }
    }
}

// Outer nibble 0xA - conditional jump on flag bit. The 5-byte
// instruction `[4C, 0xAN, bit, lo, hi]` dispatches first on
// sub-op (`bne a1, zero, 0x801e258c` at 0x801e2568 of the
// overlay disassembly), then per-sub-op checks one bit:
// sub-0 → ctx.flags, sub-1 → ctx.local_flags, sub-2 → global
// story flag word. When the bit is **set** the original
// branches to the absolute-jump label (`bne v1, zero,
// 0x801e360c`); clear (or sub-op 3..=0xF) falls through to
// `s8 += 5` (PC += 5).
pub(super) fn op_4c_na<H: FieldHost>(
    host: &mut H,
    ctx: &mut FieldCtx,
    bytecode: &[u8],
    pc: usize,
    opcode: u8,
    header_size: usize,
    operand: usize,
    op0: u8,
) -> StepResult {
    {
        if operand + 4 > bytecode.len() {
            return StepResult::Unknown { opcode, pc };
        }
        let sub = op0 & 0x0F;
        let bit = bytecode[operand + 1];
        let target = i16::from_le_bytes([bytecode[operand + 2], bytecode[operand + 3]]);
        if host.op4c_n_a_flag_set(ctx, sub, bit) {
            StepResult::Advance {
                next_pc: target as i32 as usize,
            }
        } else {
            StepResult::Advance {
                next_pc: pc + header_size + 4,
            }
        }
    }
}

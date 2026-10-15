//! `0x4C` MENU_CTRL outer-nibble 9 + 0xA handlers.

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
                host.start_floor_ladder_oscillator(sub, b1, words);
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
                host.install_floor_height_ladder(words);
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
                host.retire_floor_ladder_oscillators();
                StepResult::Advance {
                    next_pc: pc + header_size + 1,
                }
            }
            _ => StepResult::Halt { final_pc: pc },
        }
    }
}

// Outer nibble 0xA - conditional skip on a flag bit. The 5-byte
// instruction `[4C, 0xAN, bit, skip_lo, skip_hi]` advances `s8 += 5`
// in the delay slot of the sub-op dispatch (`0x801e256c`), then per
// sub-op checks one bit: sub-0 → ctx.flags, sub-1 → ctx.local_flags,
// sub-2 → the scratchpad word `0x1F800394`. A **set** bit takes the
// shared exit `LAB_801E360C` with `a0 = operand + 2`: `FUN_8003CE9C`
// reads the signed halfword and the exit adds it less two to `s8`
// (`addiu v0,v0,-0x2; addu s8,s8,v0`) - a skip **relative** to the end
// of the instruction, not an absolute target. Clear (or sub-op
// 3..=0xF) leaves `s8` advanced by 5. The shipped uses pair it with
// the op it skips: `town01` P0[35]'s `CC 00 A0 00 05 00` steps over the
// 3-byte `JmpRel` that follows it.
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
        let skip = i16::from_le_bytes([bytecode[operand + 2], bytecode[operand + 3]]);
        let next_pc = pc + header_size + 4;
        if !host.flag_bit_is_set(ctx, sub, bit) {
            return StepResult::Advance { next_pc };
        }
        let target = next_pc as isize + isize::from(skip) - 2;
        if target < 0 {
            return StepResult::Unknown { opcode, pc };
        }
        StepResult::Advance {
            next_pc: target as usize,
        }
    }
}

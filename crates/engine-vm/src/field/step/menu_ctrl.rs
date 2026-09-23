//! Field VM opcode `0x4C` (MENU_CTRL) sub-dispatcher, extracted verbatim from `step`.

use super::*;

mod nibble_3_4;
mod nibble_5_6_7;
mod nibble_8;
mod nibble_9_a;
mod nibble_c;
mod nibble_d;
mod nibble_e;

/// Sub-op of `0x4C` outer nibble 1 that clones an actor
/// (`0x801CEEA0` table slot 4 -> `0x801E0E80`).
pub const CLONE_ACTOR_SUB_OP: u8 = 0x14;

pub(super) fn op_4c<H: FieldHost>(
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
    match op0 >> 4 {
        0 => {
            host.set_party_leader(op0 & 7);
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }
        1 => {
            let Some(&b1) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&b2) = bytecode.get(operand + 2) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&b3) = bytecode.get(operand + 3) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&b4) = bytecode.get(operand + 4) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&b5) = bytecode.get(operand + 5) else {
                return StepResult::Unknown { opcode, pc };
            };
            // Sub-op `0x14` is the one eight-byte form in this nibble: its
            // arm (`0x801E0E80`) reads a sixth payload byte and exits through
            // `j 0x801E3624` with `addiu s8,s8,1` in the delay slot, on top of
            // the `addiu s8,s8,7` the nibble entry already did. Every other
            // sub-op advances seven. `legaia_asset::field_disasm` has always
            // decoded the extra byte; the executing VM did not, which slid the
            // PC one byte on every `4C 14` the disc issues.
            // REF: FUN_801D835C
            let extra = usize::from(op0 == CLONE_ACTOR_SUB_OP);
            if extra != 0 {
                let Some(&b6) = bytecode.get(operand + 6) else {
                    return StepResult::Unknown { opcode, pc };
                };
                host.menu_ctrl_clone_actor(
                    b6,
                    u32::from_le_bytes([b1, b2, b3, 0]),
                    i16::from_le_bytes([b4, b5]),
                );
            }
            host.menu_ctrl_sub1(op0, &[b1, b2, b3, b4, b5]);
            StepResult::Advance {
                next_pc: pc + header_size + 6 + extra,
            }
        }
        2 => {
            host.party_view_swap(op0 & 7);
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }
        3 => nibble_3_4::op_4c_n3(host, ctx, bytecode, pc, opcode, header_size, operand, op0),
        4 => nibble_3_4::op_4c_n4(host, ctx, bytecode, pc, opcode, header_size, operand, op0),
        5 => nibble_5_6_7::op_4c_n5(host, ctx, bytecode, pc, opcode, header_size, operand, op0),
        6 => nibble_5_6_7::op_4c_n6(host, ctx, bytecode, pc, opcode, header_size, operand, op0),
        7 => nibble_5_6_7::op_4c_n7(host, ctx, bytecode, pc, opcode, header_size, operand, op0),
        8 => nibble_8::op_4c_n8(host, ctx, bytecode, pc, opcode, header_size, operand, op0),
        9 => nibble_9_a::op_4c_n9(host, ctx, bytecode, pc, opcode, header_size, operand, op0),
        0xA => nibble_9_a::op_4c_na(host, ctx, bytecode, pc, opcode, header_size, operand, op0),
        0xC => nibble_c::op_4c_nc(host, ctx, bytecode, pc, opcode, header_size, operand, op0),
        0xD => nibble_d::op_4c_nd(host, ctx, bytecode, pc, opcode, header_size, operand, op0),
        0xE => nibble_e::op_4c_ne(host, ctx, bytecode, pc, opcode, header_size, operand, op0),
        // Outer nibble 0xF - only `op0 == 0xFF` is valid; falls
        // through to the default arm (PC += 2). Other sub-ops in
        // this nibble print SUB_CMD_0F_ERROR and also fall through.
        0xF => StepResult::Advance {
            next_pc: pc + header_size + 1,
        },
        // Outer nibble 0xB has no `case 0xb` in the original 0x4C
        // switch (the dump goes case 0xa → default → case 0xc).
        // The default arm (line 6718) prints SUB_CMD_ERROR and
        // returns the dispatcher default - halt at PC.
        0xB => StepResult::Halt { final_pc: pc },
        // `op0 >> 4` is at most 0xF; outer nibble is fully covered
        // above, so this arm is dead code.
        16..=u8::MAX => unreachable!("op0 >> 4 is at most 0xF"),
    }
}

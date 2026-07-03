//! `0x4C` MENU_CTRL outer-nibble 0xC handler. Extracted verbatim from `op_4c`.

use super::*;

// Outer nibble 0xC - small per-actor / per-scene writes.
// Ported subset covers sub-0 (move-table cancel, PC += 2),
// sub-1 (flag-loop reset, PC += 2), sub-2 (field_42), sub-3
// (script-table teleport, PC += 2), sub-4 (sub-tile
// broadcast), sub-5/6 (party-flag conditional jumps via
// `func_0x8003CE9C` + `party_flag_test`), sub-7 (sound
// trigger), sub-8 (field_74 XOR), sub-9 (global-pair compare
// gate - PC += 2 unless globals differ, then halt), sub-0xA
// / sub-0xB / sub-0xC (slot table writes), sub-0xD
// (script-context alloc, halt), sub-0xE (b6ac write), sub-0xF
// (position broadcast). All 16 sub-ops in nibble 0xC are now
// handled.
pub(super) fn op_4c_nc<H: FieldHost>(
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
        // Sub-0: 2-byte. Cancel move-table animation if active
        // (`func_0x800204F8`). Always advances PC += 2.
        0 => {
            host.op4c_n_c_sub_0_move_cancel(ctx);
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }
        // Sub-3: 2-byte. Script-table teleport with tile-center
        // math. Helper `func_0x8003C8F0`/`8003D0BC` is overlay-
        // resident; hosts decide what fields to write. PC += 2.
        3 => {
            host.op4c_n_c_sub_3_script_teleport(ctx);
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }
        // Sub-5: 4-byte `[4C, 0xC5, idx_lo, idx_hi]`. Reads the
        // 16-bit flag index via `load_u16_le`, queries the host's
        // party-flag bank, and jumps to `LAB_801E2A10` when the
        // bit is **clear** (jump-if-zero polarity). The original's
        // jump target is the dispatcher's "no-op fallthrough" -
        // both branches advance PC += 4.
        5 => {
            if operand + 3 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let flag_idx = crate::field_helpers::load_u16_le(&bytecode[operand + 1..]);
            let _bit_set = host.op4c_n_c_party_flag_test(flag_idx);
            // Both polarities (5 = jump-if-zero, 6 = jump-if-nonzero)
            // share the same dispatcher fallthrough - the original's
            // `joined_r0x801e28c4` block returns `param_2 + 4` either
            // way. The polarity selects which arm runs the
            // host-visible side effect, but PC delta is constant.
            StepResult::Advance {
                next_pc: pc + header_size + 3,
            }
        }
        // Sub-6: 4-byte. Sister of sub-5 with opposite polarity
        // (jump-if-nonzero). PC always advances by 4.
        6 => {
            if operand + 3 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let flag_idx = crate::field_helpers::load_u16_le(&bytecode[operand + 1..]);
            let _bit_set = host.op4c_n_c_party_flag_test(flag_idx);
            StepResult::Advance {
                next_pc: pc + header_size + 3,
            }
        }
        // Sub-D: 2-byte. Allocate / register a script context.
        // Halts at PC regardless of allocation outcome.
        0xD => {
            host.op4c_n_c_sub_d_script_alloc();
            StepResult::Halt { final_pc: pc }
        }
        // Sub-1: 1-byte. Walk the trigger-flag record array,
        // resetting each record's byte-0 from a per-record
        // 16-bit index queried against the flag bit-array.
        // Always advances PC += 2 (whether the array is empty
        // or the loop completes).
        1 => {
            host.op4c_n_c_sub_1_flag_loop_reset(&[]);
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }
        2 => {
            let Some(&b1) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            ctx.field_42 = u16::from(b1);
            StepResult::Advance {
                next_pc: pc + header_size + 2,
            }
        }
        4 => {
            let Some(&xb) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&zb) = bytecode.get(operand + 2) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.op4c_n_c_sub4_subtile_broadcast(xb & 0x7F, zb & 0x7F);
            StepResult::Advance {
                next_pc: pc + header_size + 3,
            }
        }
        7 => {
            let Some(&b1) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&b2) = bytecode.get(operand + 2) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.op4c_n_c_sub7_sound_trigger(b1, b2);
            StepResult::Advance {
                next_pc: pc + header_size + 3,
            }
        }
        8 => {
            ctx.field_74 ^= 0x1000_0000;
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }
        0xA => {
            if operand + 4 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let slot = bytecode[operand + 1];
            let value = i16::from_le_bytes([bytecode[operand + 2], bytecode[operand + 3]]);
            host.op4c_n_c_sub_a_set_slot(slot, value);
            StepResult::Advance {
                next_pc: pc + header_size + 4,
            }
        }
        sub @ (0xB | 0xC) => {
            if operand + 4 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let slot = bytecode[operand + 1];
            let raw = u16::from_le_bytes([bytecode[operand + 2], bytecode[operand + 3]]);
            // Sentinel substitution: `0xFFFF` → frame delta byte.
            let value = if raw == 0xFFFF {
                host.frame_delta() as i16
            } else {
                raw as i16
            };
            host.op4c_n_c_sub_bc_adjust_slot(slot, value, sub == 0xC);
            StepResult::Advance {
                next_pc: pc + header_size + 4,
            }
        }
        0xE => {
            let Some(&value) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.op4c_n_c_sub_e_set_b6ac(value);
            StepResult::Advance {
                next_pc: pc + header_size + 2,
            }
        }
        // Sub-9: 2-byte `[4C, 0xC9]`. PC += 2 unless host says
        // globals differ, then halt at PC.
        9 => {
            if host.op4c_n_c_sub9_globals_differ() {
                StepResult::Halt { final_pc: pc }
            } else {
                StepResult::Advance {
                    next_pc: pc + header_size + 1,
                }
            }
        }
        // Sub-0xF: 4-byte `[4C, 0xCF, b1, b2]`. Each byte selects
        // either the actor's world coordinate (0xFF), the
        // tile-center conversion (`b * 0x80 + 0x40` for non-zero),
        // or 0. PC += 4.
        0xF => {
            if operand + 3 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let b1 = bytecode[operand + 1];
            let b2 = bytecode[operand + 2];
            let resolve = |b: u8, world: u16| -> i16 {
                if b == 0xFF {
                    world as i16
                } else if b != 0 {
                    ((u16::from(b) << 7) | 0x40) as i16
                } else {
                    0
                }
            };
            let x = resolve(b1, ctx.world_x);
            let z = resolve(b2, ctx.world_z);
            host.op4c_n_c_sub_f_position_broadcast(x, z);
            StepResult::Advance {
                next_pc: pc + header_size + 3,
            }
        }
        // All 16 sub-ops 0x0..=0xF are covered above; values 16+
        // are unreachable because `op0 & 0x0F` is at most 0xF.
        16..=u8::MAX => unreachable!("op0 & 0x0F is at most 0xF"),
    }
}

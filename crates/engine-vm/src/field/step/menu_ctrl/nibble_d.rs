//! `0x4C` MENU_CTRL outer-nibble 0xD handler. Extracted verbatim from `op_4c`.

use super::*;

// Outer nibble 0xD - party state + camera-ish setup.
// All 16 sub-ops are ported: sub-0 (field SE trigger, 6-byte),
// sub-1 (linked-list lookup gate, 2-byte), sub-2 (channel-spawn
// halt, 2-byte), sub-3 (party state setup, 14-byte), sub-4
// (VRAM STP-bit set on 16x1 rect, 6-byte), sub-5 (VRAM STP-bit
// clear on 16x1 rect, 6-byte), sub-6 (`field_74` bitfield
// mutation, halts at PC), sub-7 (list-walk register + halt,
// 1-byte), sub-8 (`FUN_801D77F4` 4-arg call, 9-byte), sub-9
// (inverted-Y mirror set, 4-byte), sub-0xA (clear mirror +
// collision-Y refresh, 2-byte), sub-0xB (FUN_801E57F0 yield,
// 13-byte), sub-0xC (party search-and-set, 5-byte), sub-0xD
// (field_58 write, 3-byte), sub-0xE (party search query,
// 5-byte), sub-0xF (scene byte write, 3-byte).
pub(super) fn op_4c_nd<H: FieldHost>(
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
        // Sub-0: 6-byte `[4C, 0xD0, a_lo, a_hi, b_lo, b_hi]`.
        // Field SE trigger with conditional u16 pair. The
        // original at lines 6936-6944 of the dispatcher dump
        // gates the call on three flag globals; PC advances by
        // 6 in both branches. Host owns the gate state and the
        // SE pipeline.
        0 => {
            if operand + 5 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let a = crate::field_helpers::load_u16_le(&bytecode[operand + 1..]);
            let b = crate::field_helpers::load_u16_le(&bytecode[operand + 3..]);
            host.op4c_n_d_sub_0_field_se_trigger(a, b);
            StepResult::Advance {
                next_pc: pc + header_size + 5,
            }
        }
        // Sub-1: 1-byte. Linked-list lookup via `FUN_8003CF04`.
        // Host returns `Some(new_pc)` for the ce9c-jump path,
        // `None` for PC += 4 on miss. Default host returns None.
        1 => match host.op4c_n_d_sub_1_list_lookup_jump(ctx) {
            Some(new_pc) => StepResult::Advance { next_pc: new_pc },
            None => StepResult::Advance {
                next_pc: pc + header_size + 3,
            },
        },
        // Sub-2: 2-byte `[4C, 0xD2, b1]`. Calls the channel
        // resolver; halts at PC after the (possibly conditional)
        // spawn.
        2 => {
            let Some(&b1) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.op4c_n_d_sub_2_channel_spawn(b1);
            StepResult::Halt { final_pc: pc }
        }
        // Sub-7: 1-byte. Register `FUN_801DC0BC` callback then
        // halt at PC.
        7 => {
            host.op4c_n_d_sub_7_register_list_walk();
            StepResult::Halt { final_pc: pc }
        }
        3 => {
            if operand + 13 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let a = i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
            let b = i16::from_le_bytes([bytecode[operand + 3], bytecode[operand + 4]]);
            let cd = u32::from_le_bytes([
                bytecode[operand + 5],
                bytecode[operand + 6],
                bytecode[operand + 7],
                bytecode[operand + 8],
            ]);
            let ef = u32::from_le_bytes([
                bytecode[operand + 9],
                bytecode[operand + 10],
                bytecode[operand + 11],
                bytecode[operand + 12],
            ]);
            let ab = ((a as i32 as u32) << 16) | ((b as u16) as u32);
            host.op4c_n_d_sub3_party_setup(ab, cd, ef);
            StepResult::Advance {
                next_pc: pc + header_size + 13,
            }
        }
        // Sub-6: 3-byte `[4C, 0xD6, b1]`. Pure ctx.field_74
        // bitfield mutation; halts at PC.
        6 => {
            let Some(&b1) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            if b1 == 4 {
                ctx.field_74 &= 0x7FFF_FFFF;
            } else {
                ctx.field_74 = (ctx.field_74 & 0x7CFF_FFFF) | 0x8000_0000 | (u32::from(b1) << 24);
            }
            host.op4c_n_d_sub6_field74_mutate_ack();
            StepResult::Halt { final_pc: pc }
        }
        // Sub-8: 9-byte `[4C, 0xD8, b1, lo_x, hi_x, lo_y, hi_y, lo_z, hi_z]`.
        // Calls the overlay-resident `FUN_801D77F4` with `(b1, x, y, z)`
        // (the host applies the call); PC += 9.
        8 => {
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
            host.op4c_n_d_sub8_call_d77f4(b1, words);
            StepResult::Advance {
                next_pc: pc + header_size + 8,
            }
        }
        9 => {
            if operand + 3 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            ctx.flags |= 0x2000_0000;
            let raw = i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
            let value = if raw == 9999 {
                (ctx.world_y as i16).wrapping_neg()
            } else {
                raw
            };
            ctx.field_8e = value;
            ctx.world_y = value.wrapping_neg() as u16;
            StepResult::Advance {
                next_pc: pc + header_size + 3,
            }
        }
        0xA => {
            ctx.flags &= !0x2000_0000;
            host.op4c_n_d_sub_a_collision_y_refresh(ctx);
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }
        0xD => {
            let Some(&b1) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            ctx.field_58 = u16::from(b1);
            StepResult::Advance {
                next_pc: pc + header_size + 2,
            }
        }
        0xF => {
            let Some(&b1) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.op4c_n_d_sub_f_scene_byte_write(b1);
            StepResult::Advance {
                next_pc: pc + header_size + 2,
            }
        }
        // Sub-B: 13-byte. Call FUN_801E57F0(operand) then PC += 13.
        // Total instruction = opcode (1) + 12 operand bytes; the
        // helper receives the 12-byte operand slice starting at
        // the sub-op byte.
        0xB => {
            if operand + 12 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            host.op4c_n_d_sub_b_call_e57f0(&bytecode[operand..operand + 12]);
            StepResult::Advance {
                next_pc: pc + header_size + 12,
            }
        }
        // Sub-C: 5-byte `[4C, 0xDC, b1, ?, ?]`. Small-table search
        // + party-record write. Host returns `Some(new_pc)` for
        // the ce9c-jump path or `None` for the PC += 5 miss path.
        0xC => {
            let Some(&b1) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            match host.op4c_n_d_sub_c_party_search_set(b1) {
                Some(new_pc) => StepResult::Advance { next_pc: new_pc },
                None => StepResult::Advance {
                    next_pc: pc + header_size + 4,
                },
            }
        }
        // Sub-E: 5-byte. Sister of sub-C without the per-record
        // write - same control flow.
        0xE => {
            let Some(&b1) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            match host.op4c_n_d_sub_e_party_search_query(b1) {
                Some(new_pc) => StepResult::Advance { next_pc: new_pc },
                None => StepResult::Advance {
                    next_pc: pc + header_size + 4,
                },
            }
        }
        // Sub-4: 6-byte `[4C, 0xD4, x_lo, x_hi, y_lo, y_hi]`.
        // VRAM 16x1 rect read-modify-write that sets PSX STP bit
        // 15 on every non-zero pixel. Original (lines 7621-7642
        // of overlay_world_map_walk_801de840.txt) reads two u16
        // operands as `(vram_x, vram_y)` with hardcoded `w=0x10,
        // h=1`, runs `DrawSync; StoreImage(rect, buf);
        // DrawSync; for each of 16 pixels: if != 0 then OR
        // with 0x8000; LoadImage(rect, buf)`. `StoreImage` =
        // `FUN_8005842c`, `LoadImage` = `FUN_800583c8`,
        // `DrawSync` = `FUN_80058104`. Returns `iVar47 + 6`.
        4 => {
            if operand + 5 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let x = crate::field_helpers::load_u16_le(&bytecode[operand + 1..]);
            let y = crate::field_helpers::load_u16_le(&bytecode[operand + 3..]);
            host.op4c_n_d_sub_4_vram_stp_set(x, y);
            StepResult::Advance {
                next_pc: pc + header_size + 5,
            }
        }
        // Sub-5: 6-byte `[4C, 0xD5, x_lo, x_hi, y_lo, y_hi]`.
        // Sister of sub-4 that clears PSX STP bit 15 on every
        // pixel that isn't already exactly `0x8000` (STP-only
        // transparent black). Inner loop is `if pixel != 0x8000
        // then AND with 0x7FFF`. Same libgs round-trip as sub-4.
        // Returns `iVar47 + 6`.
        5 => {
            if operand + 5 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let x = crate::field_helpers::load_u16_le(&bytecode[operand + 1..]);
            let y = crate::field_helpers::load_u16_le(&bytecode[operand + 3..]);
            host.op4c_n_d_sub_5_vram_stp_clear(x, y);
            StepResult::Advance {
                next_pc: pc + header_size + 5,
            }
        }
        16..=u8::MAX => unreachable!("op0 & 0x0F is at most 0xF"),
    }
}

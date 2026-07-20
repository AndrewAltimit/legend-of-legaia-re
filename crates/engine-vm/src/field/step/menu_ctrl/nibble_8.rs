//! `0x4C` MENU_CTRL outer-nibble 8 handler. Extracted verbatim from `op_4c`.

use super::*;

// Outer nibble 8 - large multi-purpose dispatcher. Sub-ops
// 1..=F minus sub-0 and sub-3 are fully ported: sub-1
// (actor model + anim, 9-byte), sub-2 (mirror write, 2-byte),
// sub-4 (b630 write, 2-byte), sub-5/E/F (halt-acquire idiom,
// 2-byte - the original at lines 6550-6570 shares one body),
// sub-6 (actor set rotation, 15-byte), sub-7 (callback
// register + halt), sub-8 (globals write, 6-byte), sub-9
// (DAT_80073F00 write, 4-byte), sub-0xA (write quad, 11-byte),
// sub-0xB (actor-type conditional jump, 5-byte), sub-0xC
// (field_68 conditional jump, 4-byte), sub-0xD (char actor
// search, 6-byte). Sub-0 (actor allocator, needs
// `func_0x80020de0`) and sub-3 (box-fill table, needs
// `FUN_801D5630`) remain `Pending`.
pub(super) fn op_4c_n8<H: FieldHost>(
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
        // Sub-1: 9-byte `[4C, 0x81, m0, m1, m2, anim_lo,
        // anim_hi, frames_lo, frames_hi]`. Set actor model +
        // animation frame, optionally with a tween if
        // `tween_frames != 0`. Dispatcher lines 6496-6515; the
        // host applies whichever path applies based on its own
        // state model. PC always advances by 9.
        1 => {
            if operand + 8 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let model_id = crate::field_helpers::load_u24_le(&bytecode[operand + 1..]);
            let anim_frame = crate::field_helpers::load_u16_le(&bytecode[operand + 4..]);
            let tween_frames = crate::field_helpers::load_u16_le(&bytecode[operand + 6..]);
            host.op4c_n_8_sub_1_set_model_anim(ctx, model_id, anim_frame, tween_frames);
            StepResult::Advance {
                next_pc: pc + header_size + 8,
            }
        }
        2 => {
            let Some(&slot) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.op4c_n8_sub2_restore_party_slot(slot);
            StepResult::Advance {
                next_pc: pc + header_size + 2,
            }
        }
        // Sub-6: 15-byte `[4C, 0x86, x_lo..rz_hi, actor_id]`.
        // Six 16-bit LE values for position+rotation matrix
        // axes, then a 1-byte actor selector at the tail.
        // Dispatcher lines 6571-6585: actor lookup misses fall
        // through to PC + 15 with no side effect; on hit, host
        // applies the rotation matrix. PC always advances by
        // 15.
        6 => {
            if operand + 14 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let position = [
                crate::field_helpers::load_u16_le(&bytecode[operand + 1..]) as i16,
                crate::field_helpers::load_u16_le(&bytecode[operand + 3..]) as i16,
                crate::field_helpers::load_u16_le(&bytecode[operand + 5..]) as i16,
            ];
            let rotation = [
                crate::field_helpers::load_u16_le(&bytecode[operand + 7..]) as i16,
                crate::field_helpers::load_u16_le(&bytecode[operand + 9..]) as i16,
                crate::field_helpers::load_u16_le(&bytecode[operand + 11..]) as i16,
            ];
            let actor_id = bytecode[operand + 13];
            host.op4c_n_8_sub_6_actor_set_rotation(ctx, actor_id, position, rotation);
            StepResult::Advance {
                next_pc: pc + header_size + 14,
            }
        }
        4 => {
            let Some(&value) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.op4c_n8_sub4_set_b630(value);
            StepResult::Advance {
                next_pc: pc + header_size + 2,
            }
        }
        7 => {
            // Register callback then halt at PC. The original
            // calls `switchD_801e00f4::default()` (= halt for
            // 0x4C since `0x4C & 0x70 = 0x40`); script resumes
            // when the registered callback fires. Distinct from
            // an Advance - a re-entry of the dispatcher at the
            // same PC re-registers, so the host's hook should
            // be idempotent.
            host.op4c_n8_sub7_register_callback();
            StepResult::Halt { final_pc: pc }
        }
        8 => {
            if operand + 5 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let value = i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
            let b3 = bytecode[operand + 3];
            let b4 = bytecode[operand + 4];
            host.op4c_n8_sub8_write_globals(value, b3, b4);
            StepResult::Advance {
                next_pc: pc + header_size + 5,
            }
        }
        0xA => {
            if operand + 10 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let s0 = i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
            let s1 = i16::from_le_bytes([bytecode[operand + 3], bytecode[operand + 4]]);
            let s2 = i16::from_le_bytes([bytecode[operand + 5], bytecode[operand + 6]]);
            // Original uses `func_0x8003CEB8` - a 24-bit LE
            // decoder. High byte is zero-padded; hosts can
            // sign-extend if they need to.
            let packed = u32::from_le_bytes([
                bytecode[operand + 7],
                bytecode[operand + 8],
                bytecode[operand + 9],
                0,
            ]);
            host.op4c_n8_sub_a_write_quad([s0, s1, s2], packed);
            StepResult::Advance {
                next_pc: pc + header_size + 10,
            }
        }
        0xC => {
            if operand + 3 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            if host.op4c_n8_sub_c_branch_on_field_68(ctx) {
                let target = i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
                StepResult::Advance {
                    next_pc: target as i32 as usize,
                }
            } else {
                StepResult::Advance {
                    next_pc: pc + header_size + 3,
                }
            }
        }
        // Sub-9: write `_DAT_80073F00 = i16(operand[1..3])`, then
        // PC += 4 (`code_r0x801e3620` exit label).
        9 => {
            if operand + 3 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let value = i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
            host.op4c_n8_sub9_set_73f00(value);
            StepResult::Advance {
                next_pc: pc + header_size + 3,
            }
        }
        // Sub-5/E/F all share the halt-acquire body: acquire the
        // (possibly cross-context) target - write its `+0x94`
        // payload pointer, clear its wait accumulator, set its
        // HALT bit - then ADVANCE the caller past the 5-byte op
        // (`[4C, op0, p0, p1, p2]`); on predicate failure the
        // caller parks at PC. Pinned from BOTH dispatcher dumps:
        // `overlay_world_map_801de840.txt:7179` (`uVar31 = 5;
        // iVar47 += uVar31`) and `overlay_0897_801de840.txt:6550`
        // (`iVar24 = 5` then the standard advance exit
        // `switchD_801e00f4::default()` - previously misread as a
        // halt; only the `iVar24 == 0` failure path halts via
        // `LAB_801dee50`). Predicate:
        // `(saved_pc != 0 || target is player) && (!halted ||
        // scene busy)` - the standard halt-acquire gate.
        5 | 0xE | 0xF => {
            if host.field_halt_acquire_predicate(ctx, op0) {
                host.op4c_n8_halt_acquire(ctx, pc as u32);
                StepResult::Advance {
                    next_pc: pc + header_size + 4,
                }
            } else {
                StepResult::Halt { final_pc: pc }
            }
        }
        // Sub-B: 5-byte `[4C, 0x8B, type_byte, target_lo,
        // target_hi]`. Conditional jump if any actor of
        // `type_byte` is active. Dispatcher lines 6621-6644:
        // count > 0 → jump to absolute u16; count == 0 →
        // advance PC by 5.
        0xB => {
            if operand + 4 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let type_byte = bytecode[operand + 1];
            if host.op4c_n_8_sub_b_actor_type_present(type_byte) {
                let target = crate::field_helpers::load_u16_le(&bytecode[operand + 2..]);
                StepResult::Advance {
                    next_pc: target as usize,
                }
            } else {
                StepResult::Advance {
                    next_pc: pc + header_size + 4,
                }
            }
        }
        // Sub-D: 6-byte `[4C, 0x8D, char_idx, marker, target_lo,
        // target_hi]`. Tristate per-character actor sub-table
        // search. Dispatcher lines 6652-6667.
        0xD => {
            if operand + 5 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let char_idx = bytecode[operand + 1];
            let marker = bytecode[operand + 2];
            match host.op4c_n_8_sub_d_actor_search(char_idx, marker) {
                ActorSearchResult::EmptySlot => StepResult::Advance {
                    next_pc: pc + header_size + 5,
                },
                ActorSearchResult::Found => {
                    let target = crate::field_helpers::load_u16_le(&bytecode[operand + 3..]);
                    StepResult::Advance {
                        next_pc: target as usize,
                    }
                }
                ActorSearchResult::NoMatch => StepResult::Halt { final_pc: pc },
            }
        }
        // Sub-0: 3-byte header `[4C, 0x80, count]` + `count`
        // variable-length child-actor records. Halt-acquire
        // prelude (dispatcher lines 6456-6495) - identical
        // predicate to n6 sub-0x61. On success: standard ctx
        // mutation, actor-allocator + record-walking via host,
        // advance PC by 3. On failure: halt at PC. The records
        // past offset +2 are owned by the spawned actor's
        // bytecode-pointer field (`+0x90`), not by the script.
        0 => {
            if operand + 1 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            if host.field_halt_acquire_predicate(ctx, 0x80) {
                let resume = pc + header_size + 2;
                ctx.flags |= 0x400;
                ctx.wait_accum = 0;
                ctx.saved_pc = pc as u32;
                host.field_halt_acquire_apply(ctx, 0x80, resume, [0; 3]);
                let count = bytecode[operand + 1];
                let tail_start = operand + 2;
                let tail = if tail_start <= bytecode.len() {
                    &bytecode[tail_start..]
                } else {
                    &[][..]
                };
                host.op4c_n8_sub_0_actor_allocator(ctx, count, tail);
                StepResult::Advance { next_pc: resume }
            } else {
                StepResult::Halt { final_pc: pc }
            }
        }
        // Sub-3: 7-byte rectangular tile fill `[4C, 0x83,
        // col_start, row_start, col_end, row_end, value]`. The
        // dispatcher walks the inclusive rectangle and calls
        // `FUN_801D5630(col, row, ...)` per tile; on hit, writes
        // `tile[+0x2] = value`. The clean-room port surfaces
        // the rectangle through one host hook and lets the
        // engine drive its tile-pool semantics. The original
        // dispatcher's post-loop also writes
        // `_DAT_8007B630 = col_start` - the host hook owns that
        // side effect too (engines that don't care can ignore
        // it). PC advances by `header_size + 6` (= 7 bytes).
        3 => {
            if operand + 6 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let col_start = bytecode[operand + 1];
            let row_start = bytecode[operand + 2];
            let col_end = bytecode[operand + 3];
            let row_end = bytecode[operand + 4];
            let value = bytecode[operand + 5];
            host.op4c_n_8_sub_3_rect_tile_fill(col_start, row_start, col_end, row_end, value);
            StepResult::Advance {
                next_pc: pc + header_size + 6,
            }
        }
        _ => StepResult::Pending { opcode, pc },
    }
}

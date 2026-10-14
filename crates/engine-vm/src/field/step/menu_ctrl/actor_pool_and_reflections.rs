//! `0x4C` MENU_CTRL outer-nibble 8 handler.

use super::*;

// Outer nibble 8 - large multi-purpose dispatcher. Sub-ops
// 1..=F minus sub-0 and sub-3 are fully ported: sub-1
// (actor model + anim, 9-byte), sub-2 (mirror write, 2-byte),
// sub-4 (b630 write, 2-byte), sub-5/E/F (halt-acquire idiom,
// 2-byte - the original at lines 6550-6570 shares one body),
// sub-6 (reflection-controller install, 15-byte), sub-7
// (the matching teardown, 2-byte), sub-8 (globals write, 6-byte), sub-9
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
        // Sub-1: 9-byte `[4C, 0x81, r, g, b, blend_lo, blend_hi,
        // ticks_lo, ticks_hi]` - the actor's draw tint (`+0x74` colour,
        // `+0x78` blend), written outright or tweened over `ticks`
        // (`0x801E1FC4..0x801E2068`; see the host hook). PC always
        // advances by 9.
        1 => {
            if operand + 8 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let colour = crate::field::dispatch_leaves::load_u24_le(&bytecode[operand + 1..]);
            let blend = crate::field::dispatch_leaves::load_u16_le(&bytecode[operand + 4..]);
            let ticks = crate::field::dispatch_leaves::load_u16_le(&bytecode[operand + 6..]);
            let target = crate::field::peek_extended(bytecode, pc);
            host.set_actor_tint(ctx, target, colour, blend, ticks);
            StepResult::Advance {
                next_pc: pc + header_size + 8,
            }
        }
        2 => {
            let Some(&slot) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.restore_party_slot(slot);
            StepResult::Advance {
                next_pc: pc + header_size + 2,
            }
        }
        // Sub-6: 15-byte `[4C, 0x86, w0..w5, source_id]` - the
        // **reflection controller install** (`0x801E21E0` ->
        // `FUN_801E573C`). The six `s16` are the controller's
        // mirror line and tracking rect; the last byte is the
        // cross-context id of the actor to reflect. A resolve
        // miss still advances, because the arm's
        // `addiu s8,s8,0xf` sits in the resolve's delay slot.
        // PC always advances by 15.
        6 => {
            if operand + 14 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let mut words = [0i16; 6];
            for (i, w) in words.iter_mut().enumerate() {
                *w = crate::field::dispatch_leaves::load_u16_le(&bytecode[operand + 1 + i * 2..])
                    as i16;
            }
            let source_id = bytecode[operand + 13];
            host.install_reflection(ctx, source_id, words);
            StepResult::Advance {
                next_pc: pc + header_size + 14,
            }
        }
        4 => {
            let Some(&value) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.set_shake_amplitude(value);
            StepResult::Advance {
                next_pc: pc + header_size + 2,
            }
        }
        7 => {
            // Sub-6's teardown: `FUN_8003CF40(_DAT_8007C34C,
            // 0x801E5154)` retires every live reflection
            // controller, and the arm's own `addiu s8,s8,2`
            // rides the call's delay slot at `0x801E2DC4`. The
            // sweep registers nothing and the script does not
            // park - a halt here would stall any scene that
            // issued it.
            host.retire_reflections();
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }
        8 => {
            if operand + 5 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let value = i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
            let b3 = bytecode[operand + 3];
            let b4 = bytecode[operand + 4];
            host.set_door_of_light_destination(value, b3, b4);
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
            host.set_field_light([s0, s1, s2], packed);
            StepResult::Advance {
                next_pc: pc + header_size + 10,
            }
        }
        // Sub-C: 4-byte `[4C, 0x8C, skip_lo, skip_hi]`. `lh v0,0x68(s5)`
        // at `0x801E23EC`, `s8 += 4` in the delay slot of the branch; a
        // non-zero `ctx[+0x68]` exits there, a zero one takes the shared
        // relative skip `LAB_801E360C` with `a0 = operand + 1`
        // (`pc + 4 + skip - 2`). `retock` P1[46] pairs it with nibble A:
        // `CC 0D A0 00 05 00`, `26 1D 00`, `CC 0D 8C 18 00`.
        0xC => {
            if operand + 3 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let next_pc = pc + header_size + 3;
            if !host.op4c_n8_subc_field_68_is_zero(ctx) {
                return StepResult::Advance { next_pc };
            }
            let skip = i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
            let target = next_pc as isize + isize::from(skip) - 2;
            if target < 0 {
                return StepResult::Unknown { opcode, pc };
            }
            StepResult::Advance {
                next_pc: target as usize,
            }
        }
        // Sub-9: write `_DAT_80073F00 = i16(operand[1..3])`, then
        // PC += 4 (`code_r0x801e3620` exit label).
        9 => {
            if operand + 3 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let value = i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
            host.set_dialog_auto_press(value);
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
                host.halt_acquire(ctx, pc as u32);
                StepResult::Advance {
                    next_pc: pc + header_size + 4,
                }
            } else {
                StepResult::Halt { final_pc: pc }
            }
        }
        // Sub-B: 5-byte `[4C, 0x8B, type_byte, skip_lo, skip_hi]`.
        // `s8 += 5` at `0x801E2320`, then a count over the present
        // members; a non-zero count takes the shared relative skip
        // `LAB_801E360C` with `a0 = operand + 2` (`pc + 5 + skip - 2`),
        // a zero count advances 5.
        0xB => {
            if operand + 4 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let type_byte = bytecode[operand + 1];
            let next_pc = pc + header_size + 4;
            if !host.op4c_n8_subb_spell_known(type_byte) {
                return StepResult::Advance { next_pc };
            }
            let skip = crate::field::dispatch_leaves::load_u16_le(&bytecode[operand + 2..]) as i16;
            let target = next_pc as isize + isize::from(skip) - 2;
            if target < 0 {
                return StepResult::Unknown { opcode, pc };
            }
            StepResult::Advance {
                next_pc: target as usize,
            }
        }
        // Sub-D: 6-byte `[4C, 0x8D, char_idx, skill, skip_lo, skip_hi]`.
        // `s8 += 6` at `0x801E2404`; a character whose displayed-skill list
        // holds `skill` takes the shared relative skip (`0x801E3608`,
        // `pc + 6 + skip - 2`), an empty list or no match advances 6 (both
        // exits return `s8`: there is no halt).
        0xD => {
            if operand + 5 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let char_idx = bytecode[operand + 1];
            let skill = bytecode[operand + 2];
            let next_pc = pc + header_size + 5;
            if !host.op4c_n8_subd_skill_known(char_idx, skill) {
                return StepResult::Advance { next_pc };
            }
            let skip = crate::field::dispatch_leaves::load_u16_le(&bytecode[operand + 3..]) as i16;
            let target = next_pc as isize + isize::from(skip) - 2;
            if target < 0 {
                return StepResult::Unknown { opcode, pc };
            }
            StepResult::Advance {
                next_pc: target as usize,
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
                host.allocate_child_actors(ctx, count, tail);
                StepResult::Advance { next_pc: resume }
            } else {
                StepResult::Halt { final_pc: pc }
            }
        }
        // Sub-3: 7-byte rectangular tile fill `[4C, 0x83,
        // col_start, row_start, col_end, row_end, value]`. The
        // dispatcher walks the inclusive rectangle and calls
        // `FUN_801D5630(col, row, ...)` per tile; on hit, writes
        // `tile[+0x2] = value`. The port surfaces
        // the rectangle through one host hook and lets the
        // engine drive its tile-pool semantics. PC advances by
        // `header_size + 6` (= 7 bytes).
        //
        // This arm does NOT write `_DAT_8007B630`. An earlier note
        // (and `script-vm-menuctrl.md`) claimed a post-loop trailer
        // that does; the disassembly ends the loop on a clean
        // `j 0x801e3624 / _addiu s8,s8,0x7` at `0x801E212C`, and
        // the `sw` into `-0x49d0(0x8008)` two instructions later is
        // the first instruction of the SEPARATE sub-4 arm at
        // `0x801E2134` (its own jump-table slot, `0x801CEF58`).
        // The two are adjacent, not sequential.
        3 => {
            if operand + 6 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let col_start = bytecode[operand + 1];
            let row_start = bytecode[operand + 2];
            let col_end = bytecode[operand + 3];
            let row_end = bytecode[operand + 4];
            let value = bytecode[operand + 5];
            host.rect_tile_fill(col_start, row_start, col_end, row_end, value);
            StepResult::Advance {
                next_pc: pc + header_size + 6,
            }
        }
        _ => StepResult::Pending { opcode, pc },
    }
}

//! `0x4C` MENU_CTRL outer-nibble 5, 6, 7 handlers.

use super::*;

// Outer nibble 5 - sound directional + dialog dispatch.
// Only sub-0 (sound directional, 4-byte) is ported; sub-1
// (NPC move + run with halt-acquire), sub-2/3/4 (dialog
// query cluster) remain `Pending` because they thread
// halt-acquire and STATE_RESUME branches that need their
// own host-hook surface. Sub-ops 5..=0xF have no `case`
// arm in the original inner switch, so they silently
// fall through and the function returns `iVar45 = param_2`
// (initialised at the top of FUN_801de840) - halt at PC.
pub(super) fn op_4c_n5<H: FieldHost>(
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
        0 => {
            let Some(&lo) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&hi) = bytecode.get(operand + 2) else {
                return StepResult::Unknown { opcode, pc };
            };
            let value = i16::from_le_bytes([lo, hi]);
            // `CC F8 50 ..`: the target is the player object, which the VM
            // does not thread through - the host owns the write.
            if header_size == 2
                && bytecode.get(pc + 1) == Some(&0xF8)
                && host.player_set_model(value)
            {
                return StepResult::Advance {
                    next_pc: pc + header_size + 3,
                };
            }
            let high = (value as i32) >= 0xF0;
            if high {
                ctx.flags |= 0x0100_0000;
            } else {
                ctx.flags &= !0x0100_0000;
            }
            host.set_actor_model(ctx, value, high);
            StepResult::Advance {
                next_pc: pc + header_size + 3,
            }
        }
        // Sub-3: 2-byte `[4C, 0x53]`. Dialog-wait poll. The
        // original at lines 6295-6298 of the dispatcher dump
        // calls `FUN_801D65D8(1)` then `goto
        // joined_r0x801E28C4`; both branches halt-style return
        // after `param_2 = param_2 + 2`. We model this as
        // "halt at pc+2" - the host pumps its dialog state
        // machine through the side-effect hook.
        3 => {
            host.op4c_n5_sub3_dialog_wait(ctx);
            StepResult::Halt {
                final_pc: pc + header_size + 1,
            }
        }
        // Sub-4: 2-byte `[4C, 0x54]`. Dialog-advance poll. The
        // original at lines 6299-6310 calls `FUN_801D65D8(0)`;
        // if non-zero (dialog still active) goes to
        // `LAB_801DEE50` (halt at PC), else clears
        // `DAT_8007B648`, snapshots state bytes, advances PC by
        // 2. Host returns `true` when still active.
        4 => {
            if host.op4c_n5_sub4_dialog_advance(ctx) {
                StepResult::Halt { final_pc: pc }
            } else {
                StepResult::Advance {
                    next_pc: pc + header_size + 1,
                }
            }
        }
        // Sub-1: 6-byte `[4C, 0x51, x_enc, z_enc, depth, move_id]`.
        // NPC / player move-to-tile with run dispatch.
        // Dispatcher lines 6216-6285. Standard tile-coord decode,
        // is_player from `ctx.flags & 0x0100_0000`, dispatch to
        // the move-table consumer via the host. PC += 6.
        1 => {
            if operand + 5 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let b1 = bytecode[operand + 1];
            let b2 = bytecode[operand + 2];
            let b3 = bytecode[operand + 3];
            let move_id = bytecode[operand + 4];
            let world_x =
                ((b1 & 0x7F) as u16) * 0x80 + 0x40 + if b1 & 0x80 != 0 { 0x40 } else { 0 };
            let world_z =
                ((b2 & 0x7F) as u16) * 0x80 + 0x40 + if b2 & 0x80 != 0 { 0x40 } else { 0 };
            let is_player = ctx.flags & 0x0100_0000 != 0;
            host.run_to_tile(ctx, world_x, world_z, b3, move_id, is_player);
            StepResult::Advance {
                next_pc: pc + header_size + 5,
            }
        }
        // Sub-2: 3-byte `[4C, 0x52, item_id]` - TAKE_ITEM, the
        // give-side mirror of op 0x39. Retail arm `0x801E1ABC`:
        // window setup, bag consume, and on the `0x100` miss the
        // party unequip-by-id fallback. The `addiu s8,s8,0x3` is
        // in the `jal 0x800430ac` delay slot and the `bne` target
        // `0x801E00B8` advances too, so **both** paths advance -
        // there is no poll and no halt here. See
        // `FieldPartyInventoryHost::take_item` for the arm.
        2 => {
            if operand + 1 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            host.take_item(bytecode[operand + 1]);
            StepResult::Advance {
                next_pc: pc + header_size + 2,
            }
        }
        _ => StepResult::Halt { final_pc: pc },
    }
}

// Outer nibble 6 - the two VRAM-write families. op0 == 0x60
// is the literal-operand VRAM MoveImage (six misaligned LE16
// words, `jal FUN_80058490`); op0 == 0x61 is the halt-acquire
// CLUT-cell effect (one-shot cell write / cross-fade spawn,
// FUN_801E4C58). Remaining values (0x62..=0x6F) hit
// `else { return param_2; }` in the original at line 6330 of
// the dump - halt at PC.
pub(super) fn op_4c_n6<H: FieldHost>(
    host: &mut H,
    ctx: &mut FieldCtx,
    bytecode: &[u8],
    pc: usize,
    opcode: u8,
    header_size: usize,
    operand: usize,
    op0: u8,
) -> StepResult {
    match op0 {
        0x60 => {
            if operand + 13 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let mut words = [0i16; 6];
            for (i, w) in words.iter_mut().enumerate() {
                *w = i16::from_le_bytes([
                    bytecode[operand + 1 + i * 2],
                    bytecode[operand + 2 + i * 2],
                ]);
            }
            host.vram_move_image(words);
            StepResult::Advance {
                next_pc: pc + header_size + 13,
            }
        }
        // Sub-0x61: 16-byte `[4C, 0x61, ...14 operand bytes]`, the CLUT-cell
        // emitter `FUN_801E4C58`.
        //
        // The halt-acquire is conditional twice over (`0x801E1B9C..
        // 0x801E1BBC`): the arm reads the opcode byte back (`lbu v0,0(s0)`,
        // `andi 0x80`) and the frame count at operand `+0xD`, and branches
        // straight to the emitter call - with `a0 = 0`, no owning context -
        // unless the op is the **extended** form *and* the count is non-zero.
        // Only a cross-context fade waits for its end; a plain `4C 61`, or a
        // zero-frame one-shot of either form, never raises the halt bit.
        // Every kingdom map's entry script issues plain one-shots
        // (`map01 P1[0] +0x56..+0x86`, behind flag `0x27B`): halted by them,
        // the system context returned at the halted-target early-out on the
        // `CD F8` box test its per-frame loop opens with, for the whole
        // visit.
        //
        // The acquire itself, when it runs: predicate `ctx.field_94 != 0 ||
        // ctx == player` AND `ctx.flags & 0x400 == 0 || _DAT_801c6ea4+8 !=
        // 0`; on success the standard mutation and the optional
        // system-channel mirror, on failure halt at PC (`0x801E1C4C`).
        // Modeled through the shared host predicate with `which = 0x61`.
        0x61 => {
            if operand + 15 > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            let resume = pc + header_size + 15;
            let frames = u16::from_le_bytes([bytecode[operand + 13], bytecode[operand + 14]]);
            if header_size == 2 && frames != 0 {
                if !host.field_halt_acquire_predicate(ctx, 0x61) {
                    return StepResult::Halt { final_pc: pc };
                }
                ctx.flags |= 0x400;
                ctx.wait_accum = 0;
                ctx.saved_pc = pc as u32;
                host.field_halt_acquire_apply(ctx, 0x61, resume, [0; 3]);
            }
            let payload: [u8; 14] = bytecode[operand + 1..operand + 15]
                .try_into()
                .expect("14-byte payload slice");
            host.spawn_clut_cell_fx(ctx, payload);
            StepResult::Advance { next_pc: resume }
        }
        _ => StepResult::Halt { final_pc: pc },
    }
}

// Outer nibble 7 - collision-grid bulk wall paint
// `[4C, 0x7s, col0, row0, col1, row1 (, mask)]`. Other
// sub-ops halt at PC.
//
// Two operand shapes (FUN_801de840 case 7, dump 6364-6444):
// - sub-0 (`& 0xf`, clear walls) / sub-1 (`| 0xf0`, block all)
//   ignore the mask, so they are **6-byte** ops and exit via
//   the `s8 += 6` PC-delta idiom (yield via STATE_RESUME).
// - sub-2 (`& ~(mask<<4)`) / sub-3 (`| mask<<4`) consume a
//   trailing mask byte, so they are **7-byte** ops and
//   `return param_2 + 7` (advance directly).
//
// Paint range: columns `[col0, col1+1)` but rows
// `[row0+1, row1+2)` - the row bounds carry an extra `+1` the
// column bounds do not (`uVar27 = pbVar47[2] + 1` ranged
// against `pbVar47[4] + 2`, vs `uVar32 = pbVar47[1]` against
// `pbVar47[3] + 1`).
pub(super) fn op_4c_n7<H: FieldHost>(
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
        if !matches!(sub, 0..=3) {
            return StepResult::Halt { final_pc: pc };
        }
        let has_mask = sub >= 2;
        let last_operand = if has_mask { operand + 5 } else { operand + 4 };
        if last_operand >= bytecode.len() {
            return StepResult::Unknown { opcode, pc };
        }
        let x0 = bytecode[operand + 1];
        let x1 = bytecode[operand + 3].wrapping_add(1);
        let z0 = bytecode[operand + 2].wrapping_add(1);
        let z1 = bytecode[operand + 4].wrapping_add(2);
        let mask = if has_mask { bytecode[operand + 5] } else { 0 };
        host.paint_collision_walls(sub, (x0, x1), (z0, z1), mask);
        // All four paints CONTINUE the slice - but every one of them
        // genuinely RETURNS from FUN_801de840. There is no label-call idiom
        // here; that reading was wrong. Per the disassembly:
        //   sub 0 (801e1cb4) `j 801df8dc` / `addiu fp,fp,6` -> pc + 6
        //   sub 1 (801e1d28) `j 801df8dc` / `addiu fp,fp,6` -> pc + 6
        //   sub 2 (801e1d9c) `j 801e3624` / `addiu fp,fp,7` -> pc + 7
        //   sub 3 (801e1e20) `j 801e3624` / `addiu fp,fp,7` -> pc + 7
        // 801e3624 is `move v0,fp` falling into the function epilogue at
        // 801e3628 (`lw ra,0x104(sp)` .. `jr ra`); 801df8dc is the same
        // epilogue one hop earlier. Neither is a "continue label".
        //
        // The slice continues because the CALLER loops: FUN_8003a1e4's
        // `jal 801de840` at 8003a4b8 re-enters on the returned PC and breaks
        // only on an executed 0x21 (8003a4c4), a stalled PC (8003a4d4), or a
        // next opcode whose MASKED value is < 0x20 (`andi v0,s1,0x7f` then
        // `sltiu v0,v0,0x20` at 8003a4ec - the mask is & 0x7F, so wide-flag
        // opcodes with the high bit set still continue). A paint is none of
        // those. Modelling sub-0/1 as a Yield broke the retail install
        // pre-run one op after the paint - the ropeway P1[30] NPC stayed
        // parked because its `23 2A 70` seat two ops later never ran.
        // Not modelled from the 801df8d8 tail: the `FUN_8003cf04(actor_list,
        // FUN_801dd9d4)` lookup whose hit gets `flags |= 8`.
        StepResult::Advance {
            next_pc: pc + header_size + if has_mask { 6 } else { 5 },
        }
    }
}

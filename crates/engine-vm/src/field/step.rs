//! Field VM instruction dispatch: the single-instruction `step` executor and
//! its cross-context `step_with_caller` wrapper. Split out of `field.rs`.

use super::*;

mod actor_ctrl;
mod camera;
mod effect;
mod flow;
mod menu_ctrl;

/// Decode and execute one instruction.
///
/// `bytecode` is the script buffer, `pc` is the current byte offset. Returns
/// a [`StepResult`] describing the outcome.
///
/// **Cross-context dispatch:** when the opcode at `pc` has the extended bit
/// set, `ctx` should be the *target* script's context (not the caller's). Use
/// [`peek_extended`] to look up the target ID first.
pub fn step<H: FieldHost>(
    host: &mut H,
    ctx: &mut FieldCtx,
    bytecode: &[u8],
    pc: usize,
) -> StepResult {
    let Some(&opcode_byte) = bytecode.get(pc) else {
        return StepResult::Unknown { opcode: 0, pc };
    };

    let extended = opcode_byte & 0x80 != 0;
    let opcode = opcode_byte & 0x7F;
    let header_size = if extended { 2 } else { 1 };
    let operand = pc + header_size;

    // On extended (cross-context) dispatch, the resolved target ctx may be
    // halted; the original returns immediately rather than running the
    // instruction. Carve-out: opcode 0x32 (CFLAG_CLR) with bit 10 (mask 0x400)
    // is the only instruction allowed to run while halted - it's how a script
    // un-halts a target. The system channel (script_id == 0xFB) also bypasses
    // the halt check.
    if extended && ctx.is_halted() && ctx.script_id != 0xFB {
        let halt_bypass = opcode == 0x32
            && bytecode
                .get(operand)
                .map(|b| (b & 0x1F) == 10)
                .unwrap_or(false);
        if !halt_bypass {
            return StepResult::Halt { final_pc: pc };
        }
    }

    match opcode {
        // 0x21 / 0x24 / 0x25 / 0x48 - NOP cluster.
        0x21 | 0x24 | 0x25 | 0x48 => StepResult::Advance {
            next_pc: pc + header_size,
        },

        // 0x22 - EXEC_MOVE: schedule move-table playback on ctx.
        // Encoding: `[22, move_id]`. Sets ctx[+0x5C] = move_id, ctx[+0x5E] =
        // 0xFFFE, ctx[+0x56] = 5 if move_id==0 else 1. Then dispatches into
        // the move-table consumer via `host.exec_move`.
        0x22 => {
            let Some(&move_id) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            ctx.move_id = u16::from(move_id);
            ctx.field_5e = 0xFFFE;
            ctx.move_substate = if move_id == 0 { 5 } else { 1 };
            host.exec_move(ctx, move_id);
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }

        // 0x23 - MOVE_TO: teleport ctx to grid (x_byte, z_byte).
        // World coords use grid_to_world(). Player path also calls camera/
        // scroll; NPC path sets facing + movement init. Both go through
        // host.move_to(). PC += 3 (or +4 if extended).
        0x23 => {
            let Some(&xb) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&zb) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let world_x = grid_to_world(xb);
            let world_z = grid_to_world(zb);
            ctx.world_x = world_x;
            ctx.world_z = world_z;
            ctx.npc_x = xb;
            ctx.npc_facing = zb;
            ctx.field_8b = 0;
            let is_player = ctx.flags & 0x1000000 != 0;
            host.move_to(ctx, world_x, world_z, is_player);
            StepResult::Advance {
                next_pc: pc + header_size + 2,
            }
        }

        // 0x26 - JMP_REL: PC = pc + header_size + (lo + hi*0x100). Unconditional.
        0x26 => {
            let Some(&lo) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&hi) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let target = rel_jump(pc + header_size, lo, hi);
            StepResult::Advance { next_pc: target }
        }

        // 0x2B - LFLAG_SET: ctx.local_flags |= 1 << (operand & 0x1F).
        0x2B => {
            let Some(&b) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            ctx.local_flags |= 1u16 << (b & 0x1F);
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }

        // 0x2C - LFLAG_CLR.
        0x2C => {
            let Some(&b) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            ctx.local_flags &= !(1u16 << (b & 0x1F));
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }

        // 0x2D - LFLAG_TST: if bit set, advance; else halt.
        0x2D => {
            let Some(&b) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let set = ctx.local_flags & (1u16 << (b & 0x1F)) != 0;
            if set {
                StepResult::Advance {
                    next_pc: pc + header_size + 1,
                }
            } else {
                StepResult::Halt { final_pc: pc }
            }
        }

        // 0x2E - GFLAG_SET on _DAT_1F800394.
        0x2E => {
            let Some(&b) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.set_global_flags(host.global_flags() | (1u32 << (b & 0x1F)));
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }

        // 0x2F - GFLAG_CLR.
        0x2F => {
            let Some(&b) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.set_global_flags(host.global_flags() & !(1u32 << (b & 0x1F)));
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }

        // 0x30 - GFLAG_TST.
        0x30 => {
            let Some(&b) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let set = host.global_flags() & (1u32 << (b & 0x1F)) != 0;
            if set {
                StepResult::Advance {
                    next_pc: pc + header_size + 1,
                }
            } else {
                StepResult::Halt { final_pc: pc }
            }
        }

        // 0x31 - CFLAG_SET on ctx.flags. Bit 8 has a side-effect: copy
        // ctx[+0x26] -> ctx[+0x5A]. Both paths advance PC by 2 - the original
        // calls `switchD_801e0f24::caseD_4()` (entry 0x801df098, which does
        // `addiu s8, s8, 0x2; j 0x801e3628`) for bit-8, and falls through to
        // the same advance for normal bits via `code_r0x801df098`.
        0x31 => {
            let Some(&b) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let bit = b & 0x1F;
            ctx.flags |= 1u32 << bit;
            if (1u32 << bit) == 0x100 {
                ctx.saved_26 = ctx.field_26;
            }
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }

        // 0x32 - CFLAG_CLR.
        0x32 => {
            let Some(&b) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            ctx.flags &= !(1u32 << (b & 0x1F));
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }

        // 0x33 - CFLAG_TST.
        0x33 => {
            let Some(&b) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let set = ctx.flags & (1u32 << (b & 0x1F)) != 0;
            if set {
                StepResult::Advance {
                    next_pc: pc + header_size + 1,
                }
            } else {
                StepResult::Halt { final_pc: pc }
            }
        }

        // 0x37 / 0x41 - YIELD: save PC, set halt bit. Resume PC is the byte
        // AFTER this opcode + tail-2. The original's iVar24 = 3 lines up with
        // pc + header_size + 2 in our model (header_size = 1 non-extended,
        // 2 extended; +2 is the original's "+3" minus the implicit +1 for the
        // opcode byte that's already in header_size).
        //
        // saved_pc stores the byte address of the *opcode* (pre-extended) -
        // the original writes `pbVar43`, which is `buffer_base + pc_offset`
        // BEFORE the extended-bit increment. So saved_pc == pc regardless.
        //
        // The original also propagates the halt to caller_ctx (param_3) when
        // ctx is the player. Use [`step_with_caller`] to get that propagation.
        0x37 | 0x41 => {
            // Bare arm-encounter: when the host reports the active entity is an
            // encounter carrier (the consumer-SM discriminator), the record
            // overlays this opcode (`record[+0]=opcode`, count@+3, ids@+4). Hand
            // the host the bounded window so it can install the formation.
            if host.is_scripted_encounter_armed() {
                let end = (pc + 8).min(bytecode.len());
                host.install_scripted_encounter(bytecode.get(pc..end).unwrap_or(&[]));
            }
            ctx.saved_pc = pc as u32;
            ctx.wait_accum = 0;
            ctx.halt();
            StepResult::Yield {
                resume_pc: pc + header_size + 2,
            }
        }

        // 0x47 - YIELD_4: same as 0x37/0x41 but the post-yield PC delta is 4
        // (i.e. iVar24 = 4 in the original).
        0x47 => {
            ctx.saved_pc = pc as u32;
            ctx.wait_accum = 0;
            ctx.halt();
            StepResult::Yield {
                resume_pc: pc + header_size + 3,
            }
        }

        // 0x35 - BGM: 4-byte instruction. text_id (LE u16) at [operand],
        // sub_op at [operand + 2]. Host dispatches on sub_op.
        0x35 => {
            let Some(&lo) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&hi) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&sub_op) = bytecode.get(operand + 2) else {
                return StepResult::Unknown { opcode, pc };
            };
            let text_id = u16::from_le_bytes([lo, hi]);
            host.bgm(text_id, sub_op);
            StepResult::Advance {
                next_pc: pc + header_size + 3,
            }
        }

        // 0x38 - CAM_CFG. Two paths share the 3-byte instruction `[38, op0,
        // op1]`:
        //
        // - **Simple path** (`op1 & 0x7F == 0`): the original copies
        //   `*(short *)(0x80073F04 + (op0 & 0xF) * 2)` into `ctx.field_26`
        //   and returns `pc + 3`.
        //
        // - **Halt-acquire path** (`op1 & 0x7F != 0`): identical predicate +
        //   apply pair to op 0x43 sub-0/1/A/B (see
        //   [`field_halt_acquire_predicate`]). Predicate succeeds → `ctx`
        //   acquires the HALT bit + `saved_pc + wait_accum=0`, the player-vs-
        //   caller mirror fires, and the VM yields with `resume_pc = pc + 3`
        //   (script halts but its post-instruction PC is the resume target).
        //   Predicate fails → the original falls into
        //   `switchD_801e00f4::default()`; for op 0x38 that path is not in the
        //   0x50/0x60/0x70 system-flag arm, so the dispatcher halts at PC.
        //
        // [`field_halt_acquire_predicate`]: FieldHost::field_halt_acquire_predicate
        0x38 => {
            let Some(&op0) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&op1) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            if op1 & 0x7F == 0 {
                if let Some(value) = host.cam_cfg_lookup(op0 & 0x0F) {
                    ctx.field_26 = value;
                }
                StepResult::Advance {
                    next_pc: pc + header_size + 2,
                }
            } else if host.field_halt_acquire_predicate(ctx, 0x38) {
                let resume = pc + header_size + 2;
                ctx.flags |= 0x400;
                ctx.wait_accum = 0;
                ctx.saved_pc = pc as u32;
                host.field_halt_acquire_apply(ctx, 0x38, resume, [0; 3]);
                StepResult::Yield { resume_pc: resume }
            } else {
                StepResult::Halt { final_pc: pc }
            }
        }

        // 0x39 - GIVE_ITEM. 2-byte instruction: [0x39, item_id].
        0x39 => {
            let Some(&item_id) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.give_item(item_id);
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }

        // 0x3A - ADD_MONEY. 4-byte instruction. Three operand bytes form a
        // 24-bit signed integer (little-endian; bit 23 = sign). Host applies
        // the delta and decides clamping (original clamps to [0, 9999999]).
        0x3A => {
            let Some(&b0) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&b1) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&b2) = bytecode.get(operand + 2) else {
                return StepResult::Unknown { opcode, pc };
            };
            let raw = u32::from(b0) | (u32::from(b1) << 8) | (u32::from(b2) << 16);
            // Sign-extend 24 → 32.
            let signed = if raw & 0x80_0000 != 0 {
                (raw | 0xFF00_0000) as i32
            } else {
                raw as i32
            };
            host.add_money(signed);
            StepResult::Advance {
                next_pc: pc + header_size + 3,
            }
        }

        // 0x3B - SET_ITEM_COUNT. 3-byte instruction.
        0x3B => {
            let Some(&slot) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&count) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.set_item_count(slot, count);
            StepResult::Advance {
                next_pc: pc + header_size + 2,
            }
        }

        // 0x3C - PARTY_ADD. 2-byte instruction.
        0x3C => {
            let Some(&char_id) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.party_add(char_id);
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }

        // 0x3D - PARTY_REMOVE. 2-byte instruction.
        0x3D => {
            let Some(&char_id) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.party_remove(char_id);
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }

        // 0x42 - COND_JMP. Multi-mode conditional.
        //
        // Mode 0: extra-flags test. `[42, 0, bit, lo, hi]`. If
        //   `host.extra_flags() & (1 << bit)` is clear → skip 5 bytes.
        //   If set → jump to `pc + 3 + LE_u16(lo, hi)` (non-extended).
        //
        // Mode 1: screen-mode test. `[42, 1, op1, lo, hi]`. The original at
        //   `case 0x42` of FUN_801de840 (line 5176 of the dump) tests
        //   `host.screen_mode()` against:
        //     - `host.screen_mode_table(op1)` for `op1 < 8` (high-nibble check)
        //     - bit `0x20` for `op1 == 8`
        //     - bit `0x40` for `op1 == 9`
        //     - bit `0x80` for `op1 == 10`
        //     - bit `0x10` for `op1 == 0xB`
        //     - `op1 >= 0xC`: none of the conditional-skip branches match,
        //       so control falls through to the unconditional take-jump
        //       path (`iVar18 = param_2 + 3; LAB_801e35f8`). Treat as
        //       always-take.
        //   If the test FAILS, skip 5 bytes; if it succeeds, take the jump
        //   `pc + 3 + LE_u16(lo, hi)`.
        //
        // Mode 2+: the original calls `switchD_801e00f4::default()`. The
        //   dispatcher's default arm checks `opcode_byte & 0x70`; since
        //   0x42 & 0x70 = 0x40 (not in {0x50,0x60,0x70}), it returns
        //   `param_2` - halt at PC.
        0x42 => {
            let Some(&mode) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&op1) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&lo) = bytecode.get(operand + 2) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&hi) = bytecode.get(operand + 3) else {
                return StepResult::Unknown { opcode, pc };
            };
            let test_passed = match mode {
                0 => host.extra_flags() & (1u32 << (op1 & 0x1F)) != 0,
                1 => match op1 {
                    0..=7 => host
                        .screen_mode_table(op1)
                        .is_some_and(|tbl| host.screen_mode() & 0xF000 == tbl),
                    8 => host.screen_mode() & 0x20 != 0,
                    9 => host.screen_mode() & 0x40 != 0,
                    10 => host.screen_mode() & 0x80 != 0,
                    11 => host.screen_mode() & 0x10 != 0,
                    // op1 >= 0xC: unconditional take-jump path.
                    _ => true,
                },
                _ => return StepResult::Halt { final_pc: pc },
            };
            if !test_passed {
                // Skip the whole 5-byte instruction (header + 4 operand bytes).
                StepResult::Advance {
                    next_pc: pc + header_size + 4,
                }
            } else {
                // Take the jump. The original computes
                // `iVar18 = param_2 + 3; return iVar18 + LE_u16(lo, hi)`,
                // then stores the result back into the 16-bit PC.
                StepResult::Advance {
                    next_pc: rel_jump(pc + header_size + 2, lo, hi),
                }
            }
        }

        // 0x3E - WARP / INTERACT. Two paths:
        //
        // - INTERACT (`op0 == 0xFF` or `op0 < 100`): `[3E, op0, op1]`,
        //   PC += 3. Calls `host.field_interact(op0, op1)`.
        //
        // - WARP / scene transition (`op0 >= 100`): `[3E, op0, _, _, _, _]`,
        //   PC += 6. `map_id = op0 - 100`. The original clears the player
        //   ctx's bit `0x80000`; we mirror that on the active ctx (which is
        //   the player at the time scripts call this) and let the host
        //   override scene-side state.
        0x3E => {
            let Some(&op0) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            if op0 == 0xFF || op0 < 100 {
                let Some(&op1) = bytecode.get(operand + 1) else {
                    return StepResult::Unknown { opcode, pc };
                };
                host.field_interact(op0, op1);
                StepResult::Advance {
                    next_pc: pc + header_size + 2,
                }
            } else {
                let map_id = op0 - 100;
                ctx.flags &= !0x80000;
                host.scene_transition(map_id);
                StepResult::Advance {
                    next_pc: pc + header_size + 5,
                }
            }
        }

        // 0x46 - RENDER_CFG. Two forms keyed off `op0`:
        //
        // - Long form (`op0 == 0x24`): `[46, 0x24, b1, b2, b3, b4]`, PC += 6.
        //   Writes the four bytes via `host.render_cfg_long`.
        //
        // - Short form (anything else): `[46, op0, op1]`, PC += 3.
        //   The VM does the bitfield math:
        //     r = !(op0 >> 1) & 0xFF
        //     g = 2 - (op1 >> 1)
        //     b = (op0 >> 1) - 1
        //     packed = (op1 >> 1) + 2
        0x46 => {
            let Some(&op0) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            if op0 == 0x24 {
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
                host.render_cfg_long(b1, b2, b3, b4);
                StepResult::Advance {
                    next_pc: pc + header_size + 5,
                }
            } else {
                let Some(&op1) = bytecode.get(operand + 1) else {
                    return StepResult::Unknown { opcode, pc };
                };
                let r = !(op0 >> 1);
                let g = 2u8.wrapping_sub(op1 >> 1);
                let b = (op0 >> 1).wrapping_sub(1);
                let packed = (op1 >> 1).wrapping_add(2);
                host.render_cfg_short(r, g, b, packed);
                StepResult::Advance {
                    next_pc: pc + header_size + 2,
                }
            }
        }

        // 0x4F - SCENE_REGISTER_WRITE. `[4F, b0, b1, b2]`, PC += 4. The
        // original writes three u16 values (zero-extended bytes) to scene
        // offsets +0x10, +0x12, +0x14.
        0x4F => {
            let Some(&b0) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&b1) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&b2) = bytecode.get(operand + 2) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.scene_register_write(b0, b1, b2);
            StepResult::Advance {
                next_pc: pc + header_size + 3,
            }
        }

        // 0x44 - SPAWN_RECORD. `[44, global_index]`, PC += 2. Spawns a MAN
        // partition-2 record as a new field-VM context: the dispatcher calls
        // `FUN_8003BDE0(0, 0, global_index - N0 - N1, 1)` where N0/N1 are the
        // partition-0/1 record counts (so the operand is a GLOBAL record
        // index; the callee re-bases it into partition 2) and the gate is
        // forced to 1 (spawn unconditionally, subject to the record's own
        // C1/C2 story-flag gates). This is how scene-entry system scripts
        // launch cutscene records - e.g. the opening chain's `opstati` P1[0]
        // runs `44 21` to spawn its prologue timeline P2[0]. The host owns
        // the partition math + record install.
        // REF: FUN_8003BDE0
        0x44 => {
            let Some(&global_index) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.op44_spawn_scene_record(global_index);
            StepResult::Advance {
                next_pc: pc + header_size + 1,
            }
        }

        // 0x4B - ANIMATE. `[4B, count, base_id, ...4*count keyframe bytes]`.
        // PC += 3 + count * 4. Sets ctx.flags |= 0x1000, ctx.local_flags |=
        // 0x1000 (with bits 0x2000+0x0C00 cleared via mask 0xD3FF), writes
        // ctx[+0x6c] = count (face_rotation slot is reused - the original
        // stores the count there).
        0x4B => {
            let Some(&count) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&base_id) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let frame_bytes = (count as usize) * 4;
            let frames_start = operand + 2;
            let frames_end = frames_start + frame_bytes;
            if frames_end > bytecode.len() {
                return StepResult::Unknown { opcode, pc };
            }
            ctx.flags |= 0x1000;
            ctx.local_flags = (ctx.local_flags & 0xD3FF) | 0x1000;
            ctx.face_rotation = count;
            let frames = &bytecode[frames_start..frames_end];
            host.setup_animation(ctx, count, base_id, frames);
            StepResult::Advance {
                next_pc: pc + header_size + 2 + frame_bytes,
            }
        }

        // 0x4C - MENU_CTRL. Sub-dispatched by `op0 >> 4`.
        //
        // - sub-0: party leader change. `[4C, op0]` (2 bytes). leader_id =
        //   op0 & 7.
        // - sub-1: menu/effect sub-dispatcher. `[4C, op0, ...5 more bytes]`
        //   (7 bytes). Inner sub-ops 0x10/0x12/0x13/0x14 are host-delegated.
        // - sub-3 sub-5: `[4C, 0x35]` (2 bytes). `ctx.local_flags = (lf &
        //   0xFF7F) | 0x20A`.
        // - sub-3 sub-6: `[4C, 0x36]` (2 bytes). `ctx.local_flags |= 0x28A`.
        // - other sub-ops are Pending.
        0x4C => menu_ctrl::op_4c(host, ctx, bytecode, pc, opcode, header_size, operand),

        // 0x36 - SCENE_FADE. `[36, lo0, hi0, lo1, hi1]`, PC += 5 normally.
        // The host decides whether the fade applies (`SceneFadeResult::Done`)
        // or the scene is busy (`Busy` → halt at same PC). The original has
        // many sub-paths (0xFFFF wait, bit-15-set sub-cases 0..4, bit-15-clear
        // sub-paths) - they all funnel through the host.
        0x36 => {
            let Some(&lo0) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&hi0) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&lo1) = bytecode.get(operand + 2) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&hi1) = bytecode.get(operand + 3) else {
                return StepResult::Unknown { opcode, pc };
            };
            let op0_word = u16::from_le_bytes([lo0, hi0]);
            let op1_word = u16::from_le_bytes([lo1, hi1]);
            match host.scene_fade(op0_word, op1_word) {
                SceneFadeResult::Done => StepResult::Advance {
                    next_pc: pc + header_size + 4,
                },
                SceneFadeResult::Busy => StepResult::Halt { final_pc: pc },
            }
        }

        // 0x45 - CAMERA. Sub-dispatched by `op0 & 0xC0`:
        // - 0x40 LOAD: `[45, op0, ...18-byte payload]`, PC += 20.
        // - 0x80 SAVE: `[45, op0]`, PC += 2.
        // - 0x00 CONFIGURE: 10-bit mask in `[op0, op1]` selects slots; each
        //   set bit consumes a u16. `[45, op0, op1, lo, hi, ...2*set_count]`.
        //   PC += 5 + 2 * set_count. The two bytes at operand+2..4 are the
        //   `apply_trigger` value passed to the host.
        // - 0xC0 APPLY: `[45, op0, lo, hi]`, host applies the camera and
        //   the new PC is the absolute `LE_u16(operand[1..3])`.
        0x45 => camera::op_45(host, bytecode, pc, opcode, header_size, operand),

        // 0x4D - BBOX_TEST. `[4D, x_min, z_min, x_max, z_max, lo_skip,
        // hi_skip]` (7 bytes). Inside box → PC += 7. Outside box →
        // forward-skip jump per `FUN_801e3614` (which is just
        // `addiu v0, v0, -2; j 0x801e3624; addu s8, s8, v0`).
        //
        // The first bbox compare's branch-delay slot does
        // `addiu s8, s8, 0x7` unconditionally, so by the time we reach the
        // helper s8 = `param_2 + 7`. The helper then adds `skip - 2`,
        // giving `param_2 + 5 + skip = pc + header_size + 4 + skip`.
        //
        // Tile derivation depends on a global flag (`_DAT_1F800394 &
        // 0x20000`). Hosts toggle via `world_to_tile_use_alt()`.
        0x4D => {
            let Some(&x_min) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&z_min) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&x_max) = bytecode.get(operand + 2) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&z_max) = bytecode.get(operand + 3) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&skip_lo) = bytecode.get(operand + 4) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&skip_hi) = bytecode.get(operand + 5) else {
                return StepResult::Unknown { opcode, pc };
            };
            let (x_tile, z_tile) = if host.world_to_tile_use_alt() {
                let xt = ((i32::from(ctx.world_x as i16)) << 16) >> 23;
                let zt = ((i32::from(ctx.world_z as i16)) << 16) >> 23;
                (xt, zt)
            } else {
                let xt = (i32::from(ctx.world_x as i16) - 0x40) >> 7;
                let zt = (i32::from(ctx.world_z as i16) - 0x40) >> 7;
                (xt, zt)
            };
            let inside = x_tile >= i32::from(x_min)
                && z_tile >= i32::from(z_min)
                && x_tile <= i32::from(x_max)
                && z_tile <= i32::from(z_max);
            if inside {
                StepResult::Advance {
                    next_pc: pc + header_size + 6,
                }
            } else {
                StepResult::Advance {
                    next_pc: rel_jump(pc + header_size + 4, skip_lo, skip_hi),
                }
            }
        }

        // 0x3F - SCENE_CHANGE (named warp). `[3F, idx_lo, idx_hi, name_len,
        // <name_len name bytes>, entry_x, entry_z, dir]`. Copies the inline
        // destination scene NAME and (in retail) hands it to the scene-change
        // packet FUN_8001FD44; here it calls `host.scene_transition_named`. The
        // name is gated as a clean CDNAME label so a desync-phantom `0x3F` inside
        // message text doesn't warp to garbage - on a failed gate the op is a
        // no-op transition but still advances the PC. (This op is NOT a dialog
        // opener; field dialogue is the field-interact path. See
        // docs/subsystems/script-vm.md.) PC += header + 6 + name_len.
        0x3F => {
            let Some(&name_len) = bytecode.get(operand + 2) else {
                return StepResult::Unknown { opcode, pc };
            };
            let name_len = name_len as usize;
            let name_start = operand + 3;
            // Need the name + entry_x / entry_z / dir to advance correctly.
            if bytecode.len() < name_start + name_len + 3 {
                return StepResult::Unknown { opcode, pc };
            }
            let raw = &bytecode[name_start..name_start + name_len];
            if let Some(name) = crate::field_disasm::clean_scene_name(raw) {
                let entry_x = bytecode[name_start + name_len];
                let entry_z = bytecode[name_start + name_len + 1];
                let dir = bytecode[name_start + name_len + 2];
                host.scene_transition_named(&name, entry_x, entry_z, dir);
            }
            StepResult::Advance {
                next_pc: pc + header_size + 6 + name_len,
            }
        }

        // 0x40 - DATA_BLOCK: skip `len` bytes after the header.
        // Encoding: [0x40, len, ...len bytes]. PC += header_size + 1 + len.
        0x40 => {
            let Some(&len) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            StepResult::Advance {
                next_pc: pc + header_size + 1 + len as usize,
            }
        }

        // 0x4A - WAIT_FRAMES: ctx.wait_accum += frame_delta. If accum < target,
        // halt at the same PC (script will resume on next tick); else clear
        // and advance. Target is read via the SCUS helper `func_0x8003CE9C`,
        // which reads a 16-bit little-endian value from the operand cursor.
        0x4A => {
            let Some(&lo) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let Some(&hi) = bytecode.get(operand + 1) else {
                return StepResult::Unknown { opcode, pc };
            };
            let target = i32::from(u16::from_le_bytes([lo, hi]));
            ctx.wait_accum = ctx.wait_accum.saturating_add(host.frame_delta() as i16);
            if i32::from(ctx.wait_accum) < target {
                StepResult::Halt { final_pc: pc }
            } else {
                ctx.wait_accum = 0;
                StepResult::Advance {
                    next_pc: pc + header_size + 2,
                }
            }
        }

        // 0x4E - inventory comparison-and-jump. Sub-dispatched by
        // `op1 >> 4`. Encoding for sub-ops 0/1:
        //   `[4E, page, mode, arg_lo, arg_hi, skip_lo, skip_hi]` (7 bytes).
        // `mode` high nibble = sub-op; low nibble = comparison operator
        // (0 = state < scaled, 1 = scaled < state).
        //
        // Sub-ops 0/1 read `(state, factor)` from the inventory page via
        // `host.inventory_compare_pair(page, sub_op)`, compute
        // `scaled = (factor * arg) >> 8` (signed; the original rounds toward
        // zero for negative results), then compare per the operator. On
        // success, jump to `pc + header_size + 4 + LE_u16(operand[4..6])`;
        // on failure, advance past the 7-byte instruction.
        //
        // Sub-ops 2..=9 share the same 7-byte compare-and-skip shape
        // without the `>> 8` scaling; each has a dedicated value loader
        // (raw jump table `0x801CEE30`): 2 = char level byte `+0x130`
        // ([`FieldHost::op4e_char_level`]), 3 / 9 = gold / coin bank
        // ([`FieldHost::party_bank_value`]), 4 = BIOS `Rand() & 0xFF`
        // ([`FieldHost::op4e_sub4_bios_rand`], a random-chance branch),
        // 5..=8 = slot table `0x801C6460[sub - 5]`
        // ([`FieldHost::slot_table_read`]). Sub-ops 10/11 are the
        // party-bank comparison (9-byte u32 encoding).
        0x4E => flow::op_4e(host, bytecode, pc, opcode, header_size, operand),

        // 0x49 - STATE_RESUME. Multi-frame state machine on `_DAT_8007B450`.
        // The host surfaces the tristate via [`FieldHost::op49_state`].
        //
        // - `Idle`: arm a new resume. Sub-op 1 also captures `ctx.field_90`
        //   into `_DAT_8007B44C` (the original's actor-handle save). The
        //   instruction halts at the same PC; the host advances the
        //   underlying state machine and flips to `Done` on the next
        //   resume.
        // - `Armed`: another resume is already in flight - halt.
        // - `Done`: clear state and dispatch on the sub-op:
        //   - 1, 3, 7: PC += 3
        //   - 2, 4: PC += 7
        //   - 5: PC += 14
        //   - all other sub-ops are not yet ported (Pending).
        0x49 => flow::op_49(host, ctx, bytecode, pc, opcode, header_size, operand),

        // 0x34 - EFFECT. Sub-dispatched by `op0 >> 4` (4 sub-ops).
        // Sub-ops ported:
        // - 0: effect-global colour + intensity setup. PC += 7. Reads RGB at
        //   operand[1..4] + s16 intensity at operand[4..6]; the original
        //   falls through `LAB_801E212C` whose `iVar43 = iVar47 + 7` advances
        //   the PC by 7 bytes total.
        // - 1: effect/sprite spawn with optional captured-PC. PC += 13 (or
        //   `13 + 2 + pbVar46[0xD]` if `pbVar46[0xC] == 0x40`). Reads a
        //   24-bit packed value at operand[1..4] + four s16 fields at
        //   operand[4..0xC] + capture_flag at operand[0xC] + pc_payload_len
        //   at operand[0xD]. The original walks the actor list to skip the
        //   spawn if a matching actor is already alive.
        // - 2: actor-pool capture-and-yield (linked-list lookup; if found and
        //   `b1 == 0x40` the runtime captures the post-PC into the actor's
        //   `+0x94` slot and yields via STATE_RESUME, otherwise PC += 2).
        // - 3: 3D-model animation trigger via `host.effect_anim_trigger`.
        0x34 => effect::op_34(host, ctx, bytecode, pc, opcode, header_size, operand),

        // 0x43 - ACTOR_CTRL. Massive sub-dispatcher (22+ sub-ops keyed on
        // `pbVar47[0]`). Sub-ops ported:
        // - 2: 3-actor talk via FUN_801D2D38, 8-byte instruction.
        // - 7: face/body rotation setup, 17-byte instruction.
        // - 8: face/rotation reset, 2-byte instruction.
        // - 12 (0xC): allocate scripted actor via FUN_801de754, 5-byte.
        // - 13/15 (0xD/0xF): allocate actor via FUN_801de7bc with mode
        //   (3 for 0xD, 0 for 0xF), 6-byte.
        // - 14 (0xE): mark currently-iterating actor flag bit 0x8, 2-byte.
        // Other sub-ops (movement targeting, party-actor lookup, eye-blink
        // setup, model-swap, etc.) remain `Pending`.
        0x43 => actor_ctrl::op_43(host, ctx, bytecode, pc, opcode, header_size, operand),

        // Default arm: high-byte route. The original dispatcher's default
        // case checks `*pbVar43 & 0x70` (raw opcode byte) and routes to one
        // of three SCUS helpers - SET / CLEAR / TEST against the 256-bit
        // bitfield at DAT_80085758 (the **fourth flag bank**).
        //
        // The masked opcode here is `opcode_byte & 0x7F`, so `0x5x`/`0x6x`/
        // `0x7x` ranges fall through to this arm. The flag index is built
        // from the low nibble of the raw opcode byte plus the extended-bit:
        //   idx = ((opcode_byte & 0x8F) << 8) | operand[0]
        //
        // - 0x5_ SET   : PC += 1 idx byte. host.system_flag_set(idx).
        // - 0x6_ CLEAR : PC += 1 idx byte. host.system_flag_clear(idx).
        // - 0x7_ TEST  : 4-byte instruction (idx byte + 2 target bytes).
        //                When the bit IS set, jump to
        //                `pc + header_size + 1 + LE_u16(operand[1..3])`
        //                (relative offset from after the idx byte).
        //                When clear, fall through past the 4 bytes.
        0x50..=0x77 => {
            let route = opcode & 0x70;
            let Some(&idx_lo) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let idx = (u16::from(opcode_byte & 0x8F) << 8) | u16::from(idx_lo);
            match route {
                0x50 => {
                    host.system_flag_set(idx);
                    StepResult::Advance {
                        next_pc: pc + header_size + 1,
                    }
                }
                0x60 => {
                    host.system_flag_clear(idx);
                    StepResult::Advance {
                        next_pc: pc + header_size + 1,
                    }
                }
                0x70 => {
                    let Some(&off_lo) = bytecode.get(operand + 1) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&off_hi) = bytecode.get(operand + 2) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    if host.system_flag_test(idx) {
                        let target = rel_jump(pc + header_size + 1, off_lo, off_hi);
                        StepResult::Advance { next_pc: target }
                    } else {
                        StepResult::Advance {
                            next_pc: pc + header_size + 3,
                        }
                    }
                }
                // For opcode in 0x50..=0x77, `opcode & 0x70` can only be
                // 0x50, 0x60, or 0x70 - every value handled above.
                _ => unreachable!("opcode 0x{:02X} & 0x70 must be 0x50/0x60/0x70", opcode),
            }
        }

        // Top-level catch-all. The original dispatcher's `default:` arm
        // (line 4622 of the dump) routes through `*pbVar43 & 0x70`; for any
        // raw opcode byte whose high nibble is NOT 0x5x/0x6x/0x7x and whose
        // masked opcode isn't matched explicitly above, the original returns
        // `param_2` - halt at PC. The masked opcode here is `opcode_byte &
        // 0x7F`; the field VM has 43 documented opcodes, all of which are
        // explicitly cased above, so reaching this arm means a malformed or
        // future-extension byte. Halt rather than panic so we behave like
        // the original on garbage input.
        _ => StepResult::Halt { final_pc: pc },
    }
}

/// Execute one instruction in *cross-context* mode.
///
/// Use this only when [`peek_extended`] returned `Some(target_id)` and
/// `target_ctx != caller_ctx`. Equivalent to [`step`] for the dispatch +
/// state-write side, plus the original's "if target is the player, propagate
/// the YIELD halt to caller as well" behaviour.
///
/// `caller_player` is `true` when `caller_ctx` is the active player script
/// (the original branches on `iVar18 == _DAT_8007C364`). The host is the
/// authority - pass `caller_ctx.script_id == host.player_script_id()` or
/// equivalent. When in doubt, pass `false` and only the target halts.
pub fn step_with_caller<H: FieldHost>(
    host: &mut H,
    target_ctx: &mut FieldCtx,
    caller_ctx: &mut FieldCtx,
    target_is_player: bool,
    bytecode: &[u8],
    pc: usize,
) -> StepResult {
    let result = step(host, target_ctx, bytecode, pc);
    if target_is_player && let StepResult::Yield { .. } = result {
        // The original copies pbVar43 (opcode pointer) into both
        // target.saved_pc and caller.saved_pc. step() already set
        // target.saved_pc = pc; mirror it onto caller.
        caller_ctx.saved_pc = pc as u32;
        caller_ctx.wait_accum = 0;
        caller_ctx.halt();
    }
    result
}

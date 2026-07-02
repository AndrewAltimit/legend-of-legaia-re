//! Field VM instruction dispatch: the single-instruction `step` executor and
//! its cross-context `step_with_caller` wrapper. Split out of `field.rs`.

use super::*;

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

        // 0x44 - COUNTER. `[44, op0]`, PC += 2.
        0x44 => {
            let Some(&op0) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            host.counter_update(op0);
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
        0x4C => {
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
                    host.menu_ctrl_sub1(op0, &[b1, b2, b3, b4, b5]);
                    StepResult::Advance {
                        next_pc: pc + header_size + 6,
                    }
                }
                2 => {
                    host.party_view_swap(op0 & 7);
                    StepResult::Advance {
                        next_pc: pc + header_size + 1,
                    }
                }
                3 => match op0 & 0x0F {
                    0 => {
                        // sub-0: lock field input, exit via STATE_RESUME.
                        host.set_field_input_lock(true);
                        StepResult::Yield {
                            resume_pc: pc + header_size + 1,
                        }
                    }
                    1 => {
                        // sub-1: unlock field input, exit via STATE_RESUME.
                        host.set_field_input_lock(false);
                        StepResult::Yield {
                            resume_pc: pc + header_size + 1,
                        }
                    }
                    2 => {
                        host.clear_party_state_region();
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    4 | 0xB | 0xC => {
                        // sub-4 / sub-B / sub-C: original `goto code_r0x801df098`
                        // which falls through to `LAB_801df09c: switchD_801e00f4::default()`.
                        // The asm at 0x801df208 jumps with delay slot
                        // `_addiu s8, s8, 0x2` → PC += 2 from the opcode.
                        // No host hook fires; the original writes
                        // `_DAT_8007b5f0 = uVar31` (current view-index slot)
                        // before falling through, but `uVar31` was just read
                        // from the same slot a few lines earlier - net effect
                        // is a no-op write.
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    3 => {
                        host.menu_refresh();
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    5 => {
                        ctx.local_flags = (ctx.local_flags & 0xFF7F) | 0x020A;
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    6 => {
                        ctx.local_flags |= 0x028A;
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    7 => {
                        // sub-3 sub-7: copy player coords onto non-player ctx.
                        // The host gates on `ctx == player_ctx`; returning
                        // `None` means "no copy" (either the player ctx is
                        // unset OR ctx IS the player), in which case we
                        // fall through to a regular advance.
                        if let Some(p) = host.fetch_player_coords(ctx) {
                            ctx.world_x = p.world_x;
                            ctx.world_y = p.world_y;
                            ctx.world_z = p.world_z;
                            ctx.field_26 = p.field_26;
                            if ctx.flags & 0x2000_0000 != 0 {
                                let inverted_y = (p.world_y as i16).wrapping_neg();
                                host.set_inverted_y_mirror(ctx, inverted_y);
                                // Original returns via `caseD_4()` - the
                                // STATE_RESUME exit. We surface that as a
                                // Yield so the host's state-resume layer
                                // decides whether the next caller resumes.
                                return StepResult::Yield {
                                    resume_pc: pc + header_size + 1,
                                };
                            }
                        }
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    8 | 0xD => {
                        host.player_subtile_refresh(op0 & 0x0F);
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    9 => {
                        host.player_position_refresh_with_collision_y(ctx);
                        host.player_render_resync();
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    0xA => {
                        host.copy_dialog_depth_to_player();
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    0xE => {
                        host.player_render_resync();
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    0xF => {
                        host.field_io_resync();
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    // All 16 sub-ops covered above; this arm is dead code
                    // but the compiler can't prove it because the value is
                    // narrowed to `op0 & 0x0F` in this match's scrutinee.
                    16..=u8::MAX => unreachable!("op0 & 0x0F is at most 0xF"),
                },
                4 => {
                    // 0x4C outer nibble 4 - immediate-or-ramp cluster.
                    // 6-byte instruction `[4C, op0, val_lo, val_hi,
                    // ticks_lo, ticks_hi]`. The original at line ~5901 of
                    // FUN_801DE840 reads: target = signed_16(op+1..3),
                    // ticks = signed_16(op+3..5), then dispatches on
                    // `op0 & 0x0F`. PC advance = 6 (= header_size + 5)
                    // for the immediate/ramp sub-ops; sub-3 / sub-4 reuse the
                    // same 6-byte encoding as an absolute jump (their `target`
                    // becomes the new PC) on the branch noted at each arm.
                    if operand + 5 > bytecode.len() {
                        return StepResult::Unknown { opcode, pc };
                    }
                    let target_lo = bytecode[operand + 1];
                    let target_hi = bytecode[operand + 2];
                    let ticks_lo = bytecode[operand + 3];
                    let ticks_hi = bytecode[operand + 4];
                    let target = i16::from_le_bytes([target_lo, target_hi]);
                    let ticks = u16::from_le_bytes([ticks_lo, ticks_hi]);
                    let advance = StepResult::Advance {
                        next_pc: pc + header_size + 5,
                    };
                    let sub = op0 & 0x0F;
                    match sub {
                        0 => {
                            // ctx[+0x72] write or ramp.
                            if ticks == 0 {
                                ctx.field_72 = target as u16;
                            } else {
                                host.op4c_nibble4_ctx_ramp(ctx, sub, target, ticks);
                            }
                            advance
                        }
                        1 => {
                            // ctx[+0x6A] write or ramp. The input is halved
                            // (s16 arithmetic shift right 1) with a floor of
                            // 1 - see FUN_801DE840 line ~5923:
                            //   `iVar46 = signed_16(operand[0..2]);`
                            //   `uVar31 = iVar46 >> 1;`
                            //   `if (uVar31 == 0) uVar31 = 1;`
                            let halved = (target >> 1).max(1);
                            if ticks == 0 {
                                ctx.field_6a = halved;
                            } else {
                                host.op4c_nibble4_ctx_ramp(ctx, sub, halved, ticks);
                            }
                            advance
                        }
                        2 => {
                            // ctx[+0x8E] write or ramp; immediate-write
                            // path also mirrors `world_y = -value` when
                            // `flags & 0x20000000` is set.
                            if ticks == 0 {
                                ctx.field_8e = target;
                                if ctx.flags & 0x2000_0000 != 0 {
                                    ctx.world_y = (-target) as u16;
                                }
                            } else {
                                host.op4c_nibble4_ctx_ramp(ctx, sub, target, ticks);
                            }
                            advance
                        }
                        3 => {
                            // sub-3: ticks!=0 ramps `ctx.field_24`; ticks==0
                            // reuses the same 6-byte encoding as an absolute
                            // jump - the original at FUN_801DE840 line ~5961
                            // returns `iVar18 = signed_16(operand[0..2])`,
                            // which propagates back through the dispatcher
                            // as the new PC offset.
                            if ticks == 0 {
                                StepResult::Advance {
                                    next_pc: target as i32 as usize,
                                }
                            } else {
                                host.op4c_nibble4_ctx_ramp(ctx, sub, target, ticks);
                                advance
                            }
                        }
                        4 => {
                            // sub-4: mirror of sub-3 - ticks==0 writes
                            // `ctx.field_28`; ticks!=0 is the absolute jump.
                            if ticks == 0 {
                                ctx.field_28 = target;
                                advance
                            } else {
                                StepResult::Advance {
                                    next_pc: target as i32 as usize,
                                }
                            }
                        }
                        6 | 7 => {
                            // sub-6 (`_DAT_8007B92C`) / sub-7 (`_DAT_8007B930`)
                            // - paired global writes gated by `_DAT_800845A8`.
                            // When the gate is set, the original short-circuits
                            // both ports of the pair and clears them together;
                            // otherwise the regular global-write/ramp dispatch
                            // proceeds with the per-sub slot.
                            if host.op4c_nibble4_global_pair_gate() {
                                host.op4c_nibble4_global_pair_clear();
                            } else {
                                host.op4c_nibble4_global_write(sub, target as i32, ticks);
                            }
                            advance
                        }
                        8 => {
                            // ctx[+0x26] (`field_26`) write or ramp.
                            if ticks == 0 {
                                ctx.field_26 = target as u16;
                            } else {
                                host.op4c_nibble4_ctx_ramp(ctx, sub, target, ticks);
                            }
                            advance
                        }
                        0xA..=0xD => {
                            // Global slot ramp/write. Sub-D additionally
                            // multiplies by `_DAT_8008457C` and shifts
                            // right 12; the host owns the transform.
                            host.op4c_nibble4_global_write(sub, target as i32, ticks);
                            advance
                        }
                        5 => {
                            // sub-5 has a wider 11-byte encoding instead of 6.
                            // Override the operand-length precheck made above.
                            // `[4C, 0x45, b1, w94_lo, w94_hi, w96_lo, w96_hi,
                            //   w98_lo, w98_hi, ticks_lo, ticks_hi]`.
                            // Bytecode boundary: pc + 11 must fit.
                            if operand + 10 > bytecode.len() {
                                return StepResult::Unknown { opcode, pc };
                            }
                            let b1 = bytecode[operand + 1];
                            let w94 =
                                i16::from_le_bytes([bytecode[operand + 2], bytecode[operand + 3]]);
                            let w96 =
                                i16::from_le_bytes([bytecode[operand + 4], bytecode[operand + 5]]);
                            let w98 =
                                i16::from_le_bytes([bytecode[operand + 6], bytecode[operand + 7]]);
                            let sub5_ticks =
                                u16::from_le_bytes([bytecode[operand + 8], bytecode[operand + 9]]);
                            if sub5_ticks == 0 {
                                host.op4c_n4_sub5_write_immediate(ctx, b1, w94, w96, w98);
                                StepResult::Advance {
                                    next_pc: pc + header_size + 10,
                                }
                            } else {
                                host.op4c_n4_sub5_ramp(ctx, b1, w94, w96, w98, sub5_ticks);
                                // Ramp path falls through STATE_RESUME - yield
                                // and let the host's resume layer signal when
                                // to advance past the 11-byte instruction.
                                StepResult::Yield {
                                    resume_pc: pc + header_size + 10,
                                }
                            }
                        }
                        9 => {
                            // sub-9: dispatch on two bits of the global story
                            // flag word. See `FieldHost::op4c_n4_sub9_state`.
                            match host.op4c_n4_sub9_state() {
                                Sub9State::AbsJump => {
                                    // Absolute jump to signed_16(operand[0..2]).
                                    StepResult::Advance {
                                        next_pc: target as i32 as usize,
                                    }
                                }
                                Sub9State::Default => {
                                    if ticks == 0 {
                                        host.op4c_n4_sub9_default_write(target);
                                        advance
                                    } else {
                                        host.op4c_n4_sub9_default_ramp(target, ticks);
                                        StepResult::Yield { resume_pc: pc }
                                    }
                                }
                                Sub9State::Delta => {
                                    host.op4c_n4_sub9_delta_write_or_ramp(target, ticks);
                                    if ticks == 0 {
                                        advance
                                    } else {
                                        StepResult::Yield { resume_pc: pc }
                                    }
                                }
                            }
                        }
                        // Sub-ops 0xE/0xF have no `case` arm in the original
                        // case-4 inner switch (line 6188 of the dump): they
                        // hit `default: func_0x8001a068(s_SUB_40_ERROR_801cec88);
                        // iVar18 = switchD_801e00f4::default(); return iVar18;`
                        // - the dispatcher's default returns `param_2` ⇒ halt
                        // at PC.
                        0xE..=0xF => StepResult::Halt { final_pc: pc },
                        // `op0 & 0x0F` is at most 0xF; the arms above cover
                        // every value, so this arm is dead code.
                        16..=u8::MAX => unreachable!("op0 & 0x0F is at most 0xF"),
                    }
                }
                // Outer nibble 5 - sound directional + dialog dispatch.
                // Only sub-0 (sound directional, 4-byte) is ported; sub-1
                // (NPC move + run with halt-acquire), sub-2/3/4 (dialog
                // query cluster) remain `Pending` because they thread
                // halt-acquire and STATE_RESUME branches that need their
                // own host-hook surface. Sub-ops 5..=0xF have no `case`
                // arm in the original inner switch, so they silently
                // fall through and the function returns `iVar45 = param_2`
                // (initialised at the top of FUN_801de840) - halt at PC.
                5 => match op0 & 0x0F {
                    0 => {
                        let Some(&lo) = bytecode.get(operand + 1) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        let Some(&hi) = bytecode.get(operand + 2) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        let value = i16::from_le_bytes([lo, hi]);
                        let high = (value as i32) >= 0xF0;
                        if high {
                            ctx.flags |= 0x0100_0000;
                        } else {
                            ctx.flags &= !0x0100_0000;
                        }
                        host.op4c_n5_sub0_set_actor_model(ctx, value, high);
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
                        host.op4c_n_5_sub_3_dialog_wait(ctx);
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
                        if host.op4c_n_5_sub_4_dialog_advance(ctx) {
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
                        let world_x = ((b1 & 0x7F) as u16) * 0x80
                            + 0x40
                            + if b1 & 0x80 != 0 { 0x40 } else { 0 };
                        let world_z = ((b2 & 0x7F) as u16) * 0x80
                            + 0x40
                            + if b2 & 0x80 != 0 { 0x40 } else { 0 };
                        let is_player = ctx.flags & 0x0100_0000 != 0;
                        host.op4c_n5_sub1_npc_run(ctx, world_x, world_z, b3, move_id, is_player);
                        StepResult::Advance {
                            next_pc: pc + header_size + 5,
                        }
                    }
                    // Sub-2: 3-byte `[4C, 0x52, menu_id]`. Menu activation
                    // poll. Dispatcher lines 6286-6294: host returns `true`
                    // once `func_0x80042310(menu_id, 1) == 0x100` (the
                    // "menu fully activated" sentinel); the VM advances by 3.
                    // Otherwise the script halts at PC and polls next tick.
                    2 => {
                        if operand + 1 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let menu_id = bytecode[operand + 1];
                        if host.op4c_n5_sub2_menu_activation(menu_id) {
                            StepResult::Advance {
                                next_pc: pc + header_size + 2,
                            }
                        } else {
                            StepResult::Halt { final_pc: pc }
                        }
                    }
                    _ => StepResult::Halt { final_pc: pc },
                },
                // Outer nibble 6 - emitter call families.
                // Only op0 == 0x60 (6-word emitter) is ported. op0 == 0x61
                // is a halt-acquire variant whose 16-byte encoding interacts
                // with cross-context dispatch; remaining values (0x62..=0x6F)
                // hit `else { return param_2; }` in the original at line
                // 6330 of the dump - halt at PC.
                6 => match op0 {
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
                        host.op4c_n6_sub0_emitter6(words);
                        StepResult::Advance {
                            next_pc: pc + header_size + 13,
                        }
                    }
                    // Sub-0x61: 16-byte `[4C, 0x61, ...14 operand bytes]`.
                    // Halt-acquire emitter variant - dispatcher lines 6330-6364.
                    // Predicate checks: `ctx.field_94 != 0 || ctx == player`
                    // AND `ctx.flags & 0x400 == 0 || _DAT_801c6ea4+8 != 0`.
                    // On success: standard mutation, optional system-channel
                    // mirror, emitter call, advance PC by 16. On failure:
                    // halt at PC. Modeled through the shared host predicate
                    // with `which = 0x61`.
                    0x61 => {
                        if operand + 15 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        if host.field_halt_acquire_predicate(ctx, 0x61) {
                            let resume = pc + header_size + 15;
                            ctx.flags |= 0x400;
                            ctx.wait_accum = 0;
                            ctx.saved_pc = pc as u32;
                            host.field_halt_acquire_apply(ctx, 0x61, resume, [0; 3]);
                            let payload: [u8; 14] = bytecode[operand + 1..operand + 15]
                                .try_into()
                                .expect("14-byte payload slice");
                            host.op4c_n6_sub_61_emitter(ctx, payload);
                            StepResult::Advance { next_pc: resume }
                        } else {
                            StepResult::Halt { final_pc: pc }
                        }
                    }
                    _ => StepResult::Halt { final_pc: pc },
                },
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
                7 => {
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
                    host.op4c_n7_tile_flag_bulk(sub, (x0, x1), (z0, z1), mask);
                    if has_mask {
                        StepResult::Advance {
                            next_pc: pc + header_size + 6,
                        }
                    } else {
                        StepResult::Yield {
                            resume_pc: pc + header_size + 5,
                        }
                    }
                }
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
                8 => match op0 & 0x0F {
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
                        let anim_frame =
                            crate::field_helpers::load_u16_le(&bytecode[operand + 4..]);
                        let tween_frames =
                            crate::field_helpers::load_u16_le(&bytecode[operand + 6..]);
                        host.op4c_n_8_sub_1_set_model_anim(ctx, model_id, anim_frame, tween_frames);
                        StepResult::Advance {
                            next_pc: pc + header_size + 8,
                        }
                    }
                    2 => {
                        let Some(&page) = bytecode.get(operand + 1) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        host.op4c_n8_sub2_party_page_mirror(page);
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
                        let value =
                            i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
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
                            let target =
                                i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
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
                        let value =
                            i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
                        host.op4c_n8_sub9_set_73f00(value);
                        StepResult::Advance {
                            next_pc: pc + header_size + 3,
                        }
                    }
                    // Sub-5/E/F all share the halt-acquire body. The host
                    // hook applies the standard ctx mutation; the dispatch
                    // halts at PC regardless of acquire success/failure
                    // (both paths in the dump halt - success via
                    // `switchD_801e00f4::default()`, failure via
                    // `LAB_801dee50`).
                    5 | 0xE | 0xF => {
                        host.op4c_n8_halt_acquire(ctx, pc as u32);
                        StepResult::Halt { final_pc: pc }
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
                            let target =
                                crate::field_helpers::load_u16_le(&bytecode[operand + 2..]);
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
                                let target =
                                    crate::field_helpers::load_u16_le(&bytecode[operand + 3..]);
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
                        host.op4c_n_8_sub_3_rect_tile_fill(
                            col_start, row_start, col_end, row_end, value,
                        );
                        StepResult::Advance {
                            next_pc: pc + header_size + 6,
                        }
                    }
                    _ => StepResult::Pending { opcode, pc },
                },
                // Outer nibble 9 - fade family + table copy + callback.
                // Sub-0/1/2 (fade dispatch, 9-byte) and sub-0xE (16-word
                // table copy, 34-byte) ported. Sub-0xF (callback registration
                // via LAB_801da930) remains `Pending`. Sub-3..=0xD have no
                // `case` arm in the original (line 6694–6696 of the dump
                // returns `param_2` when `2 < uVar27 < 0xE`) - halt at PC.
                9 => {
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
                        // Sub-0xF: register `LAB_801DA930` callback then halt
                        // at PC. The original goes through
                        // `switchD_801e00f4::default()`, which for opcode 0x4C
                        // (`& 0x70 = 0x40`) returns `param_2` - halt at PC.
                        // The script resumes when the registered callback
                        // fires.
                        0xF => {
                            host.op4c_n9_sub_f_register_callback();
                            StepResult::Halt { final_pc: pc }
                        }
                        _ => StepResult::Halt { final_pc: pc },
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
                0xA => {
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
                0xC => match op0 & 0x0F {
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
                        let value =
                            i16::from_le_bytes([bytecode[operand + 2], bytecode[operand + 3]]);
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
                        let raw =
                            u16::from_le_bytes([bytecode[operand + 2], bytecode[operand + 3]]);
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
                },
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
                0xD => match op0 & 0x0F {
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
                            ctx.field_74 =
                                (ctx.field_74 & 0x7CFF_FFFF) | 0x8000_0000 | (u32::from(b1) << 24);
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
                        let raw =
                            i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
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
                },
                // Outer nibble 0xE - misc scene writes + emitter helper calls.
                // All sub-ops 0x0..=0xE are ported: sub-0 (3-way state write,
                // halt at PC), sub-1 (variable-length text balloon), sub-2
                // (set globals, 6-byte), sub-3 (camera-anchored teleport,
                // 2-byte), sub-4 (bbox-test halt-or-advance, 9-byte), sub-5
                // (XP add, 5-byte), sub-6 (FUN_801D8280, 8-byte), sub-7
                // (camera animate, 7-byte), sub-8 (camera zoom, 10-byte),
                // sub-9 (clear b9c4, 2-byte), sub-0xA (call c7ec then halt),
                // sub-0xB (actor lookup + conditional jump, 5-byte), sub-0xC
                // (capture FUN_801DDF48, 2-byte), sub-0xD (set ba66, 3-byte),
                // sub-0xE (snapshot 84570, 2-byte). Sub-0xF has no `case` arm
                // in the original and falls through to the default halt.
                0xE => match op0 & 0x0F {
                    // Sub-0: 2-byte `[4C, 0xE0, b1]`. 3-way write (host
                    // performs based on b1 value); halt at PC.
                    0 => {
                        let Some(&b1) = bytecode.get(operand + 1) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        host.op4c_n_e_sub0_state_write(b1);
                        StepResult::Halt { final_pc: pc }
                    }
                    // Sub-1: variable-length text balloon. Spawns a
                    // screen-anchored text actor when the leading byte is
                    // non-zero; PC always advances by `3 + packet_length`
                    // (opcode byte + sub-op byte + terminator + payload).
                    1 => {
                        let Some(&first) = bytecode.get(operand + 1) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        let payload = &bytecode[operand + 1..];
                        let length = crate::field_helpers::packet_length(payload);
                        if first != 0 {
                            host.op4c_n_e_sub_1_text_actor(&payload[..length], ctx.script_id);
                        }
                        StepResult::Advance {
                            next_pc: pc + header_size + 2 + length,
                        }
                    }
                    2 => {
                        if operand + 3 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let fmv_id =
                            i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
                        host.op4c_n_e_sub2_fmv_trigger(fmv_id);
                        StepResult::Advance {
                            next_pc: pc + header_size + 5,
                        }
                    }
                    // Sub-3: 2-byte `[4C, 0xE3, actor_id]`. Camera-anchored
                    // teleport: copy active camera position+rotation onto
                    // the resolved actor. Dispatcher lines 7208-7227. PC
                    // advances by 2; missing actor is a silent no-op.
                    3 => {
                        let Some(&actor_id) = bytecode.get(operand + 1) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        host.op4c_n_e_sub_3_actor_sync_camera(ctx, actor_id);
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    // Sub-7: 7-byte `[4C, 0xE7, t0, t1, t2, d0, d1]`.
                    // Camera animate: target (24-bit LE) at +1 and duration
                    // (16-bit LE) at +4. Dispatcher lines 7281-7297. PC
                    // advances by 7.
                    7 => {
                        if operand + 6 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let target = crate::field_helpers::load_u24_le(&bytecode[operand + 1..]);
                        let duration = crate::field_helpers::load_u16_le(&bytecode[operand + 4..]);
                        host.op4c_n_e_sub_7_camera_animate(target, duration);
                        StepResult::Advance {
                            next_pc: pc + header_size + 6,
                        }
                    }
                    // Sub-8: 10-byte `[4C, 0xE8, x0, x1, y0, y1, z0, z1, m0,
                    // m1]`. Camera zoom: four 16-bit LE values for zoom_x,
                    // zoom_y, zoom_z, mode. Dispatcher lines 7298-7361 reads
                    // `func_0x8003ce9c` four times at offsets +1/+3/+5/+7.
                    // PC advances by 10.
                    8 => {
                        if operand + 9 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let zoom_x =
                            crate::field_helpers::load_u16_le(&bytecode[operand + 1..]) as i16;
                        let zoom_y =
                            crate::field_helpers::load_u16_le(&bytecode[operand + 3..]) as i16;
                        let zoom_z =
                            crate::field_helpers::load_u16_le(&bytecode[operand + 5..]) as i16;
                        let mode =
                            crate::field_helpers::load_u16_le(&bytecode[operand + 7..]) as i16;
                        host.op4c_n_e_sub_8_camera_zoom(zoom_x, zoom_y, zoom_z, mode);
                        StepResult::Advance {
                            next_pc: pc + header_size + 9,
                        }
                    }
                    // Sub-4: 9-byte `[4C, 0xE4, x0, z0, x1, z1, scale, ?, ?]`.
                    // BBox collision query. Each operand byte goes through
                    // the standard tile-center conversion (`(b & 0x7F) * 0x80
                    // + 0x40`, plus 0x40 if the high bit is set). When the
                    // host predicate says "outside", the original calls the
                    // halt helper FUN_801E3614; we model that as Halt at PC.
                    // When inside, advance PC by 8.
                    4 => {
                        if operand + 8 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let bbox = [
                            crate::field_helpers::tile_center(bytecode[operand + 1]),
                            crate::field_helpers::tile_center(bytecode[operand + 2]),
                            crate::field_helpers::tile_center(bytecode[operand + 3]),
                            crate::field_helpers::tile_center(bytecode[operand + 4]),
                        ];
                        if host.op4c_n_e_sub_4_bbox_outside(ctx, bbox) {
                            StepResult::Halt { final_pc: pc }
                        } else {
                            StepResult::Advance {
                                next_pc: pc + header_size + 8,
                            }
                        }
                    }
                    // Sub-5: 5-byte `[4C, 0xE5, b1, b2, b3]`. Read 24-bit
                    // signed XP delta via load_u24_le + sign_extend_24, then
                    // call the host's add-xp hook. PC += 4.
                    5 => {
                        if operand + 4 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let raw = crate::field_helpers::load_u24_le(&bytecode[operand + 1..]);
                        let xp_delta = crate::field_helpers::sign_extend_24(raw);
                        host.op4c_n_e_sub_5_add_xp(xp_delta);
                        StepResult::Advance {
                            next_pc: pc + header_size + 4,
                        }
                    }
                    6 => {
                        if operand + 7 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let mut words = [0i16; 3];
                        for (i, w) in words.iter_mut().enumerate() {
                            *w = i16::from_le_bytes([
                                bytecode[operand + 1 + i * 2],
                                bytecode[operand + 2 + i * 2],
                            ]);
                        }
                        host.op4c_n_e_sub6_call_d8280(words);
                        StepResult::Advance {
                            next_pc: pc + header_size + 7,
                        }
                    }
                    // Sub-9: 1-byte. Clear `_DAT_8007B9C4` then PC += 2 via
                    // `caseD_4` (the standard `addiu s8, s8, 0x2; j epilogue`
                    // block at 0x801df098).
                    9 => {
                        host.op4c_n_e_sub9_clear_b9c4();
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    // Sub-A: 1-byte. Call overlay-resident `func_0x8003C7EC`,
                    // halt at PC.
                    0xA => {
                        host.op4c_n_e_sub_a_call_c7ec();
                        StepResult::Halt { final_pc: pc }
                    }
                    0xC => {
                        host.op4c_n_e_sub_c_capture_ddf48();
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    0xD => {
                        let Some(&b1) = bytecode.get(operand + 1) else {
                            return StepResult::Unknown { opcode, pc };
                        };
                        host.op4c_n_e_sub_d_set_ba66(b1);
                        StepResult::Advance {
                            next_pc: pc + header_size + 2,
                        }
                    }
                    // Sub-B: 5-byte `[4C, 0xEB, actor_id, target_lo, target_hi]`.
                    // Conditional actor lookup with embedded jump target.
                    // When the host resolves the actor, advance PC by 5;
                    // otherwise jump to absolute `LE_u16(operand+2..=operand+3)`.
                    0xB => {
                        if operand + 4 > bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let actor_id = bytecode[operand + 1];
                        match host.op4c_n_e_sub_b_actor_jump(actor_id) {
                            Some(()) => StepResult::Advance {
                                next_pc: pc + header_size + 4,
                            },
                            None => {
                                let target =
                                    crate::field_helpers::load_u16_le(&bytecode[operand + 2..]);
                                StepResult::Advance {
                                    next_pc: target as usize,
                                }
                            }
                        }
                    }
                    0xE => {
                        host.op4c_n_e_sub_e_snapshot_84570();
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                    // Sub-F: no `case` arm in the original; falls through to
                    // `switchD_801e00f4::default()` which returns `param_2`
                    // (= halt at PC) for outer nibble 0xE opcodes.
                    _ => StepResult::Halt { final_pc: pc },
                },
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
        0x45 => {
            let Some(&op0) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            match op0 & 0xC0 {
                0x40 => {
                    // LOAD: 18-byte payload after op0.
                    let payload_end = operand + 1 + 18;
                    if payload_end > bytecode.len() {
                        return StepResult::Unknown { opcode, pc };
                    }
                    host.camera_load(&bytecode[operand + 1..payload_end]);
                    StepResult::Advance {
                        next_pc: pc + header_size + 19,
                    }
                }
                0x80 => {
                    host.camera_save();
                    StepResult::Advance {
                        next_pc: pc + header_size + 1,
                    }
                }
                0xC0 => {
                    let Some(&lo) = bytecode.get(operand + 1) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&hi) = bytecode.get(operand + 2) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    host.camera_apply();
                    let target = u16::from_le_bytes([lo, hi]) as usize;
                    StepResult::Advance { next_pc: target }
                }
                0x00 => {
                    let Some(&op1) = bytecode.get(operand + 1) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&trig_lo) = bytecode.get(operand + 2) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&trig_hi) = bytecode.get(operand + 3) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let mask = (u16::from(op0) << 8) | u16::from(op1);
                    let apply_trigger = u16::from_le_bytes([trig_lo, trig_hi]);
                    let mode = (op0 >> 2) & 0x0F;
                    // Cursor starts at 4 (past opcode+op0+op1+trigger u16 - i.e.
                    // operand + 3, since operand=pc+header_size means
                    // operand+3 = pc+header_size+3). The original `iVar18 = 4`
                    // is the byte index relative to pbVar47 (= operand), so
                    // first param is at operand + 4 in the bytecode.
                    let mut cursor = operand + 4;
                    let mut params: Vec<CameraParam> = Vec::with_capacity(10);
                    for slot in 0u8..10 {
                        let bit = 1u16 << (9 - slot);
                        if mask & bit == 0 {
                            continue;
                        }
                        if cursor + 1 >= bytecode.len() {
                            return StepResult::Unknown { opcode, pc };
                        }
                        let v = u16::from_le_bytes([bytecode[cursor], bytecode[cursor + 1]]);
                        params.push(CameraParam { slot, value: v });
                        cursor += 2;
                    }
                    let consumed = cursor - operand; // = 4 + 2 * set_count
                    host.camera_configure(&params, apply_trigger, mode);
                    StepResult::Advance {
                        next_pc: pc + header_size + consumed,
                    }
                }
                _ => unreachable!(),
            }
        }

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
        // Sub-ops 2/3/5/6/7/8/9 fall through to an absolute jump:
        // `next_pc = LE_u16(operand[2..4])`.
        //
        // Sub-op 4 invokes [`FieldHost::op4e_sub4_bios_rand`] (BIOS Rand stub)
        // and uses the returned value as the next PC; the default is 0, which
        // restarts the script at the bytecode origin. Sub-ops 10/11 are the
        // party-bank comparison (9-byte encoding) ported above.
        0x4E => {
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
        0x49 => {
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
        0x34 => {
            let Some(&op0) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            let sub = op0 >> 4;
            match sub {
                0 => {
                    // 7-byte instruction: [op0, r, g, b, intensity_lo, intensity_hi].
                    let Some(rgb_int) = bytecode.get(operand + 1..operand + 6) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let rgb = [rgb_int[0], rgb_int[1], rgb_int[2]];
                    let intensity = i16::from_le_bytes([rgb_int[3], rgb_int[4]]);
                    host.op34_sub0_color_intensity_setup(op0, rgb, intensity);
                    StepResult::Advance {
                        next_pc: pc + header_size + 6,
                    }
                }
                1 => {
                    // Base instruction is 13 bytes (opcode + 12 operand
                    // bytes). The "capture flag" at `pbVar46[0xC]` is the
                    // BYTE JUST PAST the instruction - the runtime peeks at
                    // the first byte of the next instruction to decide
                    // whether to consume it as a capture extension.
                    let Some(payload) = bytecode.get(operand + 1..operand + 12) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let packed24 = ((payload[0] as u32) << 16)
                        | ((payload[1] as u32) << 8)
                        | (payload[2] as u32);
                    let world_x = i16::from_le_bytes([payload[3], payload[4]]);
                    let world_z = i16::from_le_bytes([payload[5], payload[6]]);
                    // The original NEGATES the y component (`local_a6 = -local_a6`)
                    // before the spawn call - undo the sign here.
                    let raw_neg_y = i16::from_le_bytes([payload[7], payload[8]]);
                    let world_y = raw_neg_y.wrapping_neg();
                    // Peek the byte AT pc + 13 (first byte after the
                    // 13-byte base instruction). When it's 0x40, the
                    // runtime treats it as a capture-extension marker and
                    // PC advances by an extra `2 + payload_len`.
                    let capture_flag = bytecode.get(operand + 12).copied().unwrap_or(0);
                    let captured_pc_payload: &[u8] = if capture_flag == 0x40 {
                        let payload_len = bytecode.get(operand + 13).copied().unwrap_or(0) as usize;
                        let start = operand + 14;
                        let end = start + payload_len;
                        bytecode.get(start..end).unwrap_or(&[])
                    } else {
                        &[]
                    };
                    let delta_from_opcode = host.op34_sub1_spawn_or_skip(
                        ctx,
                        op0,
                        packed24,
                        [world_x, world_y, world_z],
                        capture_flag,
                        captured_pc_payload,
                    );
                    StepResult::Advance {
                        next_pc: pc + delta_from_opcode,
                    }
                }
                2 => {
                    // sub-2: 3-byte instruction `[34, 0x2N, b1, ...]`. The
                    // original walks the actor list at `_DAT_8007C354` looking
                    // for an entry with `[+0x90] == iVar18` (current ctx). If
                    // found AND `b1 == 0x40`, it captures `pbVar47 + 3` into
                    // the matched actor's `+0x94` (a forwarded-PC pointer) and
                    // returns via `caseD_4()` (STATE_RESUME → `Yield`).
                    // Otherwise it falls through `code_r0x801df098` for PC += 2.
                    let Some(&b1) = bytecode.get(operand + 1) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let captured_pc_offset = pc + header_size + 2;
                    let captured =
                        host.op34_capture_pc_for_existing_actor(ctx, b1, captured_pc_offset);
                    if captured {
                        StepResult::Yield { resume_pc: pc }
                    } else {
                        StepResult::Advance {
                            next_pc: pc + header_size + 1,
                        }
                    }
                }
                3 => {
                    let Some(&arg) = bytecode.get(operand + 1) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    host.effect_anim_trigger(ctx, arg);
                    StepResult::Advance {
                        next_pc: pc + header_size + 2,
                    }
                }
                // Sub-ops 4..=0xF: original has no `case` arm; falls through
                // `if (bVar35 != 2) { if (bVar35 != 3) { return param_2; } }`
                // at line 4811-4814 of the dump ⇒ halt at PC.
                4..=15 => StepResult::Halt { final_pc: pc },
                // `op0 >> 4` is at most 0xF; arms above cover every value.
                16..=u8::MAX => unreachable!("op0 >> 4 is at most 0xF"),
            }
        }

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
        0x43 => {
            let Some(&sub_op) = bytecode.get(operand) else {
                return StepResult::Unknown { opcode, pc };
            };
            match sub_op {
                // Halt-acquire dispatcher (sub-0/1/A/B). 5-byte for sub-0/1,
                // 9-byte for sub-A/B. Acquire = save HALT bit + saved_pc on
                // ctx; on success, return absolute resume PC via the s16
                // operand at +3 (sub-0/1) or +7 (sub-A/B). On failure (the
                // host's predicate returns false), advance PC by the standard
                // amount (5 or 9). See `docs/subsystems/script-vm.md`
                // (opcode 0x43, halt-acquire dispatcher).
                0 | 1 | 0xA | 0xB => {
                    let wide = sub_op == 0xA || sub_op == 0xB;
                    let needed = if wide { 8 } else { 4 };
                    if operand + needed >= bytecode.len() {
                        return StepResult::Unknown { opcode, pc };
                    }
                    let coords = [
                        i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]),
                        0,
                        i16::from_le_bytes([
                            bytecode.get(operand + 3).copied().unwrap_or(0),
                            bytecode.get(operand + 4).copied().unwrap_or(0),
                        ]),
                    ];
                    let target_offset = if wide { 7 } else { 3 };
                    if host.field_halt_acquire_predicate(ctx, sub_op) {
                        let resume = i16::from_le_bytes([
                            bytecode[operand + target_offset],
                            bytecode[operand + target_offset + 1],
                        ]) as i32 as usize;
                        ctx.flags |= 0x400;
                        ctx.wait_accum = 0;
                        ctx.saved_pc = pc as u32;
                        host.field_halt_acquire_apply(ctx, sub_op, resume, coords);
                        StepResult::Yield { resume_pc: resume }
                    } else {
                        let advance_by = if wide { 9 } else { 5 };
                        StepResult::Advance {
                            next_pc: pc + header_size + advance_by - 1,
                        }
                    }
                }
                2 => {
                    let Some(&a1) = bytecode.get(operand + 1) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&a2) = bytecode.get(operand + 2) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&a3) = bytecode.get(operand + 3) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&lo) = bytecode.get(operand + 4) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&hi) = bytecode.get(operand + 5) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&b6) = bytecode.get(operand + 6) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let arg_word = u16::from_le_bytes([lo, hi]);
                    host.op43_three_actor_talk([a1, a2, a3], arg_word, b6);
                    StepResult::Advance {
                        next_pc: pc + header_size + 7,
                    }
                }
                3..=6 => {
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
                    let Some(&t_lo) = bytecode.get(operand + 5) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&t_hi) = bytecode.get(operand + 6) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&c_lo) = bytecode.get(operand + 7) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&c_hi) = bytecode.get(operand + 8) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let ticks = u16::from_le_bytes([t_lo, t_hi]);
                    let curve = u16::from_le_bytes([c_lo, c_hi]);
                    host.op43_sound_register_ramp(sub_op, [b1, b2, b3, b4], ticks, curve);
                    StepResult::Advance {
                        next_pc: pc + header_size + 9,
                    }
                }
                7 => {
                    let Some(&face_id) = bytecode.get(operand + 1) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    if operand + 16 > bytecode.len() {
                        return StepResult::Unknown { opcode, pc };
                    }
                    let payload_4 = u32::from_le_bytes([
                        bytecode[operand + 2],
                        bytecode[operand + 3],
                        bytecode[operand + 4],
                        bytecode[operand + 5],
                    ]);
                    let params = [
                        u16::from_le_bytes([bytecode[operand + 6], bytecode[operand + 7]]),
                        u16::from_le_bytes([bytecode[operand + 8], bytecode[operand + 9]]),
                        u16::from_le_bytes([bytecode[operand + 10], bytecode[operand + 11]]),
                        u16::from_le_bytes([bytecode[operand + 12], bytecode[operand + 13]]),
                    ];
                    let target =
                        u16::from_le_bytes([bytecode[operand + 14], bytecode[operand + 15]]) as i16;
                    ctx.face_rotation = face_id;
                    host.actor_face_rotation_setup(ctx, face_id, payload_4, params, target);
                    StepResult::Advance {
                        next_pc: pc + header_size + 16,
                    }
                }
                8 => {
                    ctx.face_rotation = 0;
                    host.actor_face_reset(ctx);
                    StepResult::Advance {
                        next_pc: pc + header_size + 1,
                    }
                }
                0xC => {
                    let Some(&b1) = bytecode.get(operand + 1) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&b2) = bytecode.get(operand + 2) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&b3) = bytecode.get(operand + 3) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    host.op43_alloc_scripted_actor(b1, b2, b3);
                    StepResult::Advance {
                        next_pc: pc + header_size + 4,
                    }
                }
                0xD | 0xF => {
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
                    let mode = if sub_op == 0xD { 3 } else { 0 };
                    host.op43_alloc_actor_with_mode(sub_op, mode, [b1, b2, b3, b4]);
                    StepResult::Advance {
                        next_pc: pc + header_size + 5,
                    }
                }
                0xE => {
                    host.op43_mark_actor_flag_8(ctx);
                    StepResult::Advance {
                        next_pc: pc + header_size + 1,
                    }
                }
                9 => {
                    // 10-byte: [43, 9, x_lo, x_hi, y_lo, y_hi, z_lo, z_hi, t_lo, t_hi]
                    let Some(&xl) = bytecode.get(operand + 1) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&xh) = bytecode.get(operand + 2) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&yl) = bytecode.get(operand + 3) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&yh) = bytecode.get(operand + 4) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&zl) = bytecode.get(operand + 5) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&zh) = bytecode.get(operand + 6) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&tl) = bytecode.get(operand + 7) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let Some(&th) = bytecode.get(operand + 8) else {
                        return StepResult::Unknown { opcode, pc };
                    };
                    let x = u16::from_le_bytes([xl, xh]);
                    let y = u16::from_le_bytes([yl, yh]);
                    let z = u16::from_le_bytes([zl, zh]);
                    let ticks = u16::from_le_bytes([tl, th]);
                    if ticks != 0 {
                        host.op43_sub9_tween(ctx, x, y, z, ticks);
                    } else {
                        // Immediate write: only if value != 0xFFFF (sentinel).
                        if x != 0xFFFF {
                            ctx.world_x = x;
                        }
                        if y != 0xFFFF {
                            ctx.world_y = y;
                        }
                        if z != 0xFFFF {
                            ctx.world_z = z;
                        }
                        // ctx.flags & 0x20000000 mirrors -y onto +0x8E (face_8E).
                        // We don't have that field exposed yet; the host can read
                        // ctx.flags + ctx.world_y after the call.
                    }
                    StepResult::Advance {
                        next_pc: pc + header_size + 9,
                    }
                }
                0x10 => {
                    if operand + 20 > bytecode.len() {
                        return StepResult::Unknown { opcode, pc };
                    }
                    host.op43_widget_sprite_spawn(&bytecode[operand + 1..operand + 20]);
                    StepResult::Advance {
                        next_pc: pc + header_size + 20,
                    }
                }
                0x11 => {
                    if operand + 11 > bytecode.len() {
                        return StepResult::Unknown { opcode, pc };
                    }
                    let mut words = [0u16; 5];
                    for (i, w) in words.iter_mut().enumerate() {
                        *w = u16::from_le_bytes([
                            bytecode[operand + 1 + i * 2],
                            bytecode[operand + 2 + i * 2],
                        ]);
                    }
                    host.op43_widget_mask_rect(words);
                    StepResult::Advance {
                        next_pc: pc + header_size + 11,
                    }
                }
                0x12 => {
                    if operand + 13 > bytecode.len() {
                        return StepResult::Unknown { opcode, pc };
                    }
                    let mut words = [0i16; 6];
                    for (i, w) in words.iter_mut().enumerate() {
                        *w = u16::from_le_bytes([
                            bytecode[operand + 1 + i * 2],
                            bytecode[operand + 2 + i * 2],
                        ]) as i16;
                    }
                    let did_split = words[2] > 0xFF;
                    host.op43_vram_rect_copy(words, did_split);
                    StepResult::Advance {
                        next_pc: pc + header_size + 13,
                    }
                }
                0x13 => {
                    if operand + 13 > bytecode.len() {
                        return StepResult::Unknown { opcode, pc };
                    }
                    let mut payload = [0u8; 13];
                    payload.copy_from_slice(&bytecode[operand..operand + 13]);
                    host.op43_widget_panel_spawn(&payload);
                    StepResult::Advance {
                        next_pc: pc + header_size + 13,
                    }
                }
                0x14 => {
                    if operand + 9 > bytecode.len() {
                        return StepResult::Unknown { opcode, pc };
                    }
                    let mut words = [0i16; 4];
                    for (i, w) in words.iter_mut().enumerate() {
                        *w = u16::from_le_bytes([
                            bytecode[operand + 1 + i * 2],
                            bytecode[operand + 2 + i * 2],
                        ]) as i16;
                    }
                    host.op43_widget_panel_move(words);
                    StepResult::Advance {
                        next_pc: pc + header_size + 9,
                    }
                }
                0x15 => {
                    if operand + 13 > bytecode.len() {
                        return StepResult::Unknown { opcode, pc };
                    }
                    host.op43_widget_letterbox(&bytecode[operand + 1..operand + 13]);
                    StepResult::Advance {
                        next_pc: pc + header_size + 13,
                    }
                }
                // Sub-ops 0x16..=0xFF: original `case 0x43` inner switch has
                // no `case` arm beyond 0x15. Such sub-ops fall out of the
                // inner switch with `iVar45 = param_2` (initialised at
                // line 4511 of the dump) and hit the outer `break;` ⇒
                // halt at PC.
                _ => StepResult::Halt { final_pc: pc },
            }
        }

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

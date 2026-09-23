//! Field VM opcode `0x43` (ACTOR_CTRL) sub-dispatcher, extracted verbatim from `step`.

use super::*;

pub(super) fn op_43<H: FieldHost>(
    host: &mut H,
    ctx: &mut FieldCtx,
    bytecode: &[u8],
    pc: usize,
    opcode: u8,
    header_size: usize,
    operand: usize,
) -> StepResult {
    let Some(&sub_op) = bytecode.get(operand) else {
        return StepResult::Unknown { opcode, pc };
    };
    match sub_op {
        // Halt-acquire dispatcher (sub-0/1/A/B). 8-byte for sub-0/1,
        // 10-byte for sub-A/B. Acquire = save HALT bit + saved_pc on
        // ctx, then arc the target. A refused acquire leaves the PC on
        // the instruction (`beqz v0, 0x801DEE4C` at `0x801DF410`;
        // `0x801DEE4C` is `move s8, s4`, the invocation's own PC), so the
        // op is retried next frame, never skipped. See
        // `docs/subsystems/script-vm.md` (opcode 0x43, the arc jump).
        0 | 1 | 0xA | 0xB => {
            // Scripted arc jump - halt-acquire, then arc the target actor.
            // Operand `[sub][tile_x][tile_z][apex: s16][frames: s16]` plus
            // `[y: s16]` on sub-A/B, decoded by
            // `crate::field_ledge_hop_arc::ScriptArcRequest` from the arm's own
            // operand loads (`overlay_0897` `0x801DF418..0x801DF5B0`). The
            // shared exit is `j 0x801E3624` with `addiu s8, s8, 8` in the delay
            // slot plus a `+2` at `0x801DF534` on the `sub >= 0xA` arm, over an
            // `s8` the prologue already advanced past the extended channel
            // byte (`0x801DE948`).
            //
            // None of the halfwords is a jump target: `+3` / `+5` go to
            // `FUN_801D25EC` as `a2` (apex) / `a3` (frames), and the sub-A/B
            // `+7` is negated into the landing Y. Reading `+7` as an absolute
            // resume PC sent `urudre2` `P2[9]` backwards to body `0x00A0` on
            // every arrival - its `C3 2B 0A 76 70 50 00 32 00 A0` names
            // Y = -160, not PC 160; reading bytes `1..2` as one `i16` merged
            // the two tile bytes into a single coordinate.
            let Some(req) =
                crate::field_ledge_hop_arc::ScriptArcRequest::decode(&bytecode[operand..])
            else {
                return StepResult::Unknown { opcode, pc };
            };
            let operand_len = req.operand_len();
            // The landing triple as far as the operand alone names it: the
            // tile X / Z and sub-A/B's explicit Y. The floor-sampled Y of a
            // tile target is the host's (`FUN_80019278`), so it is 0 here.
            let (lx, lz) = req.landing_xz().unwrap_or((0, 0));
            let coords = [lx, req.explicit_y.unwrap_or(0), lz];
            // Retail falls through to the next instruction on the acquire path;
            // the context stops running because the acquire raised the halt bit
            // (`ctx[+0x10] |= 0x400`), not because the PC moved.
            let next_pc = pc + header_size + operand_len;
            let ext = crate::field::peek_extended(bytecode, pc);
            // The acquire refuses a target still carrying the halt bit while
            // the scene word `*(_DAT_801C6EA4) + 8` is zero
            // (`0x801DF3A4..0x801DF3CC`) - an actor already mid arc.
            let refused = host.op43_arc_target_halted(ctx, ext);
            if !refused && host.field_halt_acquire_predicate(ctx, sub_op) {
                ctx.flags |= 0x400;
                ctx.wait_accum = 0;
                ctx.saved_pc = pc as u32;
                host.field_halt_acquire_apply(ctx, sub_op, next_pc, coords);
                // `jal 0x801D25EC` at `0x801DF5AC` is unconditional on the
                // acquire side: the halt is released by the arc's watcher.
                host.op43_arc_jump(ctx, ext, &req);
                StepResult::Yield { resume_pc: next_pc }
            } else {
                StepResult::Halt { final_pc: pc }
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
            let start = i16::from_le_bytes([t_lo, t_hi]);
            let end = i16::from_le_bytes([c_lo, c_hi]);
            host.op43_camera_register_ramp(sub_op, [b1, b2, b3, b4], start, end);
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
            // The >256-wide two-page split lives in `vram_rect_copy`
            // alongside the packet builder it feeds.
            let calls = crate::vram_rect_copy::op43_sub12_calls(words);
            host.op43_vram_rect_copy(&calls);
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

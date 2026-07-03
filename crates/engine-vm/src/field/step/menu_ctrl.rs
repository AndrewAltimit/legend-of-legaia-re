//! Field VM opcode `0x4C` (MENU_CTRL) sub-dispatcher, extracted verbatim from `step`.

use super::*;

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
                    let w94 = i16::from_le_bytes([bytecode[operand + 2], bytecode[operand + 3]]);
                    let w96 = i16::from_le_bytes([bytecode[operand + 4], bytecode[operand + 5]]);
                    let w98 = i16::from_le_bytes([bytecode[operand + 6], bytecode[operand + 7]]);
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
                let world_x =
                    ((b1 & 0x7F) as u16) * 0x80 + 0x40 + if b1 & 0x80 != 0 { 0x40 } else { 0 };
                let world_z =
                    ((b2 & 0x7F) as u16) * 0x80 + 0x40 + if b2 & 0x80 != 0 { 0x40 } else { 0 };
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
                let anim_frame = crate::field_helpers::load_u16_le(&bytecode[operand + 4..]);
                let tween_frames = crate::field_helpers::load_u16_le(&bytecode[operand + 6..]);
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
                let fmv_id = i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
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
                let zoom_x = crate::field_helpers::load_u16_le(&bytecode[operand + 1..]) as i16;
                let zoom_y = crate::field_helpers::load_u16_le(&bytecode[operand + 3..]) as i16;
                let zoom_z = crate::field_helpers::load_u16_le(&bytecode[operand + 5..]) as i16;
                let mode = crate::field_helpers::load_u16_le(&bytecode[operand + 7..]) as i16;
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
                        let target = crate::field_helpers::load_u16_le(&bytecode[operand + 2..]);
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

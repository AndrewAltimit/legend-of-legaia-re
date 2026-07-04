//! `0x4C` MENU_CTRL outer-nibble 3 + 4 handlers. Extracted verbatim from `op_4c`.

use super::*;

pub(super) fn op_4c_n3<H: FieldHost>(
    host: &mut H,
    ctx: &mut FieldCtx,
    _bytecode: &[u8],
    pc: usize,
    _opcode: u8,
    header_size: usize,
    _operand: usize,
    op0: u8,
) -> StepResult {
    match op0 & 0x0F {
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
    }
}

pub(super) fn op_4c_n4<H: FieldHost>(
    host: &mut H,
    ctx: &mut FieldCtx,
    bytecode: &[u8],
    pc: usize,
    opcode: u8,
    header_size: usize,
    operand: usize,
    op0: u8,
) -> StepResult {
    {
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
                let sub5_ticks = u16::from_le_bytes([bytecode[operand + 8], bytecode[operand + 9]]);
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
                    Sub9State::PlayerRelative => {
                        // Player-relative write: `+0x4A = target +
                        // player_anchor[+0x16]` (ramped when ticks != 0).
                        // Same advance/yield shape as the default path -
                        // this arm NEVER jumps (cutscene-dialogue overlay
                        // `case 9`, live-probe-pinned over the opening).
                        host.op4c_n4_sub9_player_relative_write(target, ticks);
                        if ticks == 0 {
                            advance
                        } else {
                            StepResult::Yield { resume_pc: pc }
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
}

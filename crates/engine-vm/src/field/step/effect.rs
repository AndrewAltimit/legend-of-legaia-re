//! Field VM opcode `0x34` (EFFECT) sub-dispatcher, extracted verbatim from `step`.

use super::*;

pub(super) fn op_34<H: FieldHost>(
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
            // Attached light spawn (`overlay_0897` `0x801DFEFC..0x801E0018`):
            // twelve operand bytes, then `FUN_801E5668` unless the target
            // already carries one. The exit adds `0xD` to an `s8` the
            // prologue already moved past any extended channel byte, so the
            // base width is `header_size + 12`. Only a **spawned** light
            // consumes a following `0x40` block as its keyframe script
            // (`+0x94 = s6 + 2`, `s8 += 2 + len`); on the skip path the block
            // stays in the stream and runs as op `0x40`, which skips itself.
            let Some(spawn) = crate::field_actor_billboard::AttachedSpriteSpawn::decode(
                bytecode.get(operand..).unwrap_or(&[]),
            ) else {
                return StepResult::Unknown { opcode, pc };
            };
            let base = header_size + 12;
            let after = pc + base;
            let script = match bytecode.get(after) {
                Some(0x40) => bytecode.get(after + 2..),
                _ => None,
            };
            let ext = crate::field::peek_extended(bytecode, pc);
            let spawned = host.op34_sub1_spawn_attached(ctx, ext, &spawn, script);
            let extra = match (spawned, script) {
                (true, Some(_)) => 2 + bytecode.get(after + 1).copied().unwrap_or(0) as usize,
                _ => 0,
            };
            StepResult::Advance {
                next_pc: after + extra,
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
            let captured = host.op34_capture_pc_for_existing_actor(ctx, b1, captured_pc_offset);
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

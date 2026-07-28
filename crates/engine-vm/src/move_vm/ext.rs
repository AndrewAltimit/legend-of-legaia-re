//! Extension-VM (opcode 0x2F) default dispatcher, ported from `FUN_801D362C`.
//!
//! ## Where each arm's size comes from
//!
//! Every arm returns the width of its instruction in u16 halfwords - the
//! value the dispatcher leaves in `s2` before joining the shared epilogue
//! at `0x801D4A3C` (`sll v0, s2, 0x10` then `>> 0x10`, i.e. a sign-extended
//! 16-bit count). There is **no size-1 arm** in the in-range switch: the
//! only `li s2, 0x1` in the function is the bounds-check failure at
//! `0x801D365C`, taken when the sub-opcode is `>= 0x3D`.
//!
//! The size is invisible in the decompiled C. Ghidra renders the shared
//! `j 0x801D4A3C` exit as a `func_0x801d4a3c()` label-call and drops the
//! `li s2, N` that sits in its branch delay slot, so a C-sourced reading of
//! any arm reports the default. Each arm below therefore cites the raw
//! instruction that sets `s2`, per
//! `docs/tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims`.
//!
//! Ten arms have a second, data-dependent exit: they are conditional
//! **branches** whose last operand word is a signed halfword displacement
//! added to the fall-through width. All ten funnel through the same tail at
//! `0x801D4830`:
//!
//! ```text
//!   801d4830  beq  v0,zero,0x801d4a40   ; predicate false -> size = s2 (preset)
//!   801d4834  _sll v0,s2,0x10
//!   801d4838  lhu  v0,0x6(s3)           ; the delta word
//!   801d483c  j    0x801d4a3c
//!   801d4840  _addiu s2,v0,0x4          ; predicate true  -> size = delta + 4
//! ```
//!
//! - `0x06` / `0x07` - base 7, delta at `op[6]` (`0x801D3868`).
//! - `0x0A` / `0x0B` - base 3, delta at `op[2]` (`0x801D38C8`).
//! - `0x13` / `0x14` - base 4, delta at `op[3]`.
//! - `0x36`..`0x39` - base 4, delta at `op[3]`.
//!
//! The delta is read `lhu` but the result is truncated to 16 bits and
//! sign-extended by the epilogue, so a negative displacement walks the PC
//! backwards - the spin-wait-until-condition idiom.
//!
//! The full per-arm table with its `li s2` sites lives in
//! [`crate::move_vm_overlay_ext::canonical_size`], which is the
//! fall-through width for every sub-opcode; the unit tests cross-check
//! every arm here against it.

use super::*;

pub(crate) fn ext_default_dispatch<H: MoveHost + ?Sized>(
    host: &mut H,
    state: &mut ActorState,
    sub_opcode: u16,
    operand: &[u16],
) -> MoveExtResult {
    // The original C reads `*(short *)(param_2 + N)` where param_2 points
    // at the opcode word itself. So `param_2 + 4` is u16-index 2 (the third
    // word: opcode + sub_opcode + first param). `op_w(N)` reads u16-index N
    // of `operand`, which is `bytecode[pc..]`.
    let op_w = |i: usize| -> u16 { operand.get(i).copied().unwrap_or(0) };

    match sub_opcode {
        // 0x00 - no side effect, but it skips 16 halfwords: the arm at
        // `0x801D3680` is a bare `j 0x801d4a3c` with `li s2, 0x10` in the
        // delay slot. It is the widest arm in the table and the only one
        // whose whole body is the size.
        0x00 => MoveExtResult::with_size(16),

        // 0x01 - `func_0x8001a068("EFC %d %d %d", x, y, z)` debug print.
        // Original sets `iVar16 = 0x20000` then breaks (size = 2).
        0x01 => {
            host.ext_debug_world(state.world_x, state.world_y, state.world_z);
            MoveExtResult::with_size(2)
        }

        // 0x02 - clear face_rotation. Size 2 (`li s2, 0x2` at 0x801D36B4).
        0x02 => {
            state.face_rotation = 0;
            MoveExtResult::with_size(2)
        }

        // 0x03 - clear flags bit 0x1000. Size 2 (`li s2, 0x2` at 0x801D36B8).
        0x03 => {
            state.flags &= !0x1000;
            MoveExtResult::with_size(2)
        }

        // 0x04 - write actor world XYZ into the operand slot at u16-index
        // `op[2] + 3`. The "self-modifying" pattern stores a copy of the
        // current world coords into the move-bytecode itself; the absolute
        // word offset is `state.pc + op[2] + 3`. 3 consecutive writes for
        // x/y/z. Size 3 (`li s2, 0x3` at 0x801D36F8).
        0x04 => {
            let base = state.pc as usize + op_w(2) as usize + 3;
            host.move_bytecode_write_u16(base, state.world_x as u16);
            host.move_bytecode_write_u16(base + 1, state.world_y as u16);
            host.move_bytecode_write_u16(base + 2, state.world_z as u16);
            MoveExtResult::with_size(3)
        }

        // 0x05 - RAND_ADD self-modify: write `op_w(2) + rand() % op_w(3)`
        // into the bytecode word at `pc + op_w(4) + 5` (the raw arm at
        // `overlay_0897_801d362c.txt` 0x801D3704.. computes
        // `s0 = operands + op_w(4)*2 + 0xA`, calls the BIOS `A(2Fh) rand`
        // thunk `FUN_80056798`, takes the modulo and stores; `li s2, 5`).
        // jou's lightning director points it one word past this op - the
        // following `0x09` wait's operand - which is what randomises the
        // strike cadence.
        0x05 => {
            let div = op_w(3);
            let r = if div == 0 { 0 } else { host.ext_rand16() % div };
            let dst = state.pc as usize + op_w(4) as usize + 5;
            host.move_bytecode_write_u16(dst, op_w(2).wrapping_add(r));
            MoveExtResult::with_size(5)
        }

        // 0x06 / 0x07 - bbox-vs-player **conditional branch**, 7 halfwords
        // wide: `[2F][06|07][xa][za][xb][zb][delta]`.
        //
        // The arm at `0x801D3764` first canonicalises the box *in the
        // bytecode*: `sh v1,0x4(s3)` / `sh a0,0x4(s0)` swap `op[2]`/`op[4]`
        // when `op[4] < op[2]`, and the sibling pair swaps `op[3]`/`op[5]`.
        // Those are ordinary self-modifying writes like sub-ops
        // 0x04 / 0x1B / 0x1E, so they go through the deferred bytecode
        // callback; the predicate below uses the swapped values directly,
        // which is what retail's re-reads see.
        //
        // The test then asks whether the player is inside
        // `[xa*0x80+0x40 ..= xb*0x80+0x40] × [za*0x80+0x40 ..= zb*0x80+0x40]`
        // (`a2 = 1` means **outside** at `0x801D3828`).
        //
        // Both sub-ops preset `s2 = 7` (`li s2, 0x7` at `0x801D3838`, in the
        // delay slot of the `bne` that separates them) and take the branch
        // through `0x801D3868` (`lhu v0,0xc(s3); addu s2,s2,v0`) - so the
        // taken side is `7 + op[6]` and the untaken side is a plain 7.
        // 0x06 branches when the player is OUTSIDE; 0x07 when INSIDE.
        0x06 | 0x07 => {
            let player = host.move_player_world_xyz();
            let mut xa = op_w(2) as i16 as i32;
            let mut xb = op_w(4) as i16 as i32;
            let mut za = op_w(3) as i16 as i32;
            let mut zb = op_w(5) as i16 as i32;
            if xb < xa {
                std::mem::swap(&mut xa, &mut xb);
                host.move_bytecode_write_u16(state.pc as usize + 2, xa as u16);
                host.move_bytecode_write_u16(state.pc as usize + 4, xb as u16);
            }
            if zb < za {
                std::mem::swap(&mut za, &mut zb);
                host.move_bytecode_write_u16(state.pc as usize + 3, za as u16);
                host.move_bytecode_write_u16(state.pc as usize + 5, zb as u16);
            }
            // Original scales by 0x80 with a 0x40 half-cell margin.
            let outside = (player[0] as i32) < xa * 0x80 + 0x40
                || (player[2] as i32) < za * 0x80 + 0x40
                || xb * 0x80 + 0x40 < player[0] as i32
                || zb * 0x80 + 0x40 < player[2] as i32;
            let branch = if sub_opcode == 0x06 {
                outside
            } else {
                !outside
            };
            if branch {
                MoveExtResult::with_size(7i16.wrapping_add(op_w(6) as i16))
            } else {
                MoveExtResult::with_size(7)
            }
        }

        // 0x08 - `DAT_801F22F4 = 1` (set move-VM global predicate).
        // Size 2 (`li s2, 0x2` at 0x801D3884).
        0x08 => {
            host.move_global_predicate_set(1);
            MoveExtResult::with_size(2)
        }

        // 0x09 - `DAT_801F22F4 = 0` (clear). Size 2 (`li s2, 0x2` at 0x801D3894).
        0x09 => {
            host.move_global_predicate_set(0);
            MoveExtResult::with_size(2)
        }

        // 0x0A / 0x0B - **conditional branch** on the move-VM global
        // predicate `DAT_801F22F4`, 3 halfwords wide: `[2F][0A|0B][delta]`.
        //
        // `0x801D38A4` is `beq v0,zero,0x801d4a3c` with `li s2, 0x3` in the
        // delay slot, so the untaken side is a plain 3; the taken side falls
        // to the shared `0x801D38C8` tail (`lhu v0,0x4(s3); addiu s2,v0,0x3`)
        // for `3 + op[2]`. 0x0A branches when the predicate is SET, 0x0B when
        // it is CLEAR (`bne v0,zero,0x801d4a3c` at `0x801D38C0`).
        0x0A | 0x0B => {
            let set = host.move_global_predicate_get() != 0;
            let branch = if sub_opcode == 0x0A { set } else { !set };
            if branch {
                MoveExtResult::with_size(3i16.wrapping_add(op_w(2) as i16))
            } else {
                MoveExtResult::with_size(3)
            }
        }

        // 0x0C - `actor[+0x50] = op_w(2)` (set midpoint blend / sub-state).
        // Size 3 (`li s2, 0x3` in the 0x801D38DC exit).
        0x0C => {
            state.field_50 = op_w(2);
            MoveExtResult::with_size(3)
        }

        // 0x0D - `actor[+0x50] += op_w(2)` (additive variant). Size 3.
        0x0D => {
            state.field_50 = state.field_50.wrapping_add(op_w(2));
            MoveExtResult::with_size(3)
        }

        // 0x0E - midpoint position calc + write to actor world.
        // Reads param_2 + 4/6/8 (a) + 10/12/14 (off) + 16/18/20 (b). The
        // midpoint helper consumes `actor[+0x50]` (the blend amount set by
        // ext ops 0x0C/0x0D). Original returns `iVar16 = 0xb0000` → size 11
        // (opcode + sub-op + 9 operand u16s).
        0x0E => {
            let a = [op_w(2) as i16, op_w(3) as i16, op_w(4) as i16];
            let off = [op_w(5) as i16, op_w(6) as i16, op_w(7) as i16];
            let b = [op_w(8) as i16, op_w(9) as i16, op_w(10) as i16];
            let mode = state.field_50;
            host.ext_midpoint_set(state, a, b, off, mode);
            // Pre-stage the world coords the way the original does (the
            // helper may overwrite them depending on `mode`, but a host that
            // doesn't model the helper still gets the average write-through).
            state.world_x = off[0].wrapping_add(((a[0] as i32 + b[0] as i32) >> 1) as i16);
            state.world_y = off[1].wrapping_add(((a[1] as i32 + b[1] as i32) >> 1) as i16);
            state.world_z = off[2].wrapping_add(((a[2] as i32 + b[2] as i32) >> 1) as i16);
            MoveExtResult::with_size(11)
        }

        // 0x0F - `DAT_801F22F6 = 0` (clear move-VM global counter).
        // Size 2 (`li s2, 0x2` in the 0x801D39E4 exit).
        0x0F => {
            host.move_global_counter_set(0);
            MoveExtResult::with_size(2)
        }

        // 0x10 - wrap `DAT_801F22F6` mod 16, capture low byte into
        // `actor.field_86` (preserving the high byte), then increment the
        // counter. Per the original:
        //   if (0xf < c) c = 0;
        //   captured = c & 0xff;
        //   c += 1;
        //   actor.field_86 = (actor.field_86 & 0xff00) | captured;
        0x10 => {
            let mut counter = host.move_global_counter_get();
            if counter > 0xF {
                counter = 0;
            }
            let captured = counter & 0xFF;
            host.move_global_counter_set(counter.wrapping_add(1));
            state.field_86 = (state.field_86 & 0xFF00) | captured;
            // Size 2 (`li s2, 0x2` in the 0x801D3A28 exit).
            MoveExtResult::with_size(2)
        }

        // 0x11 - save `actor[+0x14..+0x1C]` (world coords + Y mirror) into
        // the scratch slot indexed by `actor.field_86 & 0xFF`. Same shape
        // as sub-op 0x25 but the slot index comes from the cycle counter
        // updated by sub-ops 0x0F / 0x10 instead of an operand u16.
        0x11 => {
            let slot = state.field_86 & 0xFF;
            let lo = (state.world_x as u16 as u32) | ((state.world_y as u16 as u32) << 16);
            let hi = (state.world_z as u16 as u32) | ((state.world_y_mirror as u16 as u32) << 16);
            host.move_slot_save_u32(slot, 0, lo);
            host.move_slot_save_u32(slot, 4, hi);
            // Size 2 (`li s2, 0x2` in the 0x801D3A64 exit) - unlike 0x25,
            // which takes its slot from the bytecode and is 3 wide.
            MoveExtResult::with_size(2)
        }

        // 0x12 - slot-indexed midpoint variant of 0x0E. Reads
        // `actor[+0x86] & 0xFF` as a slot index, loads `slot.x/y/z` from the
        // 16-slot scratch table at `&DAT_801F3498`, then computes
        //   actor.world.{x,y,z} = op[2/3/4] + (slot.{x,y,z} + op[5/6/7]) / 2
        // before passing through the same midpoint helper as 0x0E. The
        // operand layout is `(op[2..4]=offset, op[5..7]=b)` - `a` comes from
        // the slot, not from the bytecode. Original returns `iVar16 =
        // 0x80000` → size 8 (opcode + sub-op + 6 operand u16s).
        0x12 => {
            let slot = state.field_86 & 0xFF;
            let slot_lo = host.move_slot_load_u32(slot, 0);
            let slot_hi = host.move_slot_load_u32(slot, 4);
            let a = [
                (slot_lo & 0xFFFF) as i16,
                ((slot_lo >> 16) & 0xFFFF) as i16,
                (slot_hi & 0xFFFF) as i16,
            ];
            let b = [op_w(5) as i16, op_w(6) as i16, op_w(7) as i16];
            let off = [op_w(2) as i16, op_w(3) as i16, op_w(4) as i16];
            let mode = state.field_50;
            host.ext_midpoint_set(state, a, b, off, mode);
            state.world_x = off[0].wrapping_add(((a[0] as i32 + b[0] as i32) >> 1) as i16);
            state.world_y = off[1].wrapping_add(((a[1] as i32 + b[1] as i32) >> 1) as i16);
            state.world_z = off[2].wrapping_add(((a[2] as i32 + b[2] as i32) >> 1) as i16);
            MoveExtResult::with_size(8)
        }

        // 0x13 / 0x14 - flag-bank **conditional branches** on the fourth
        // flag bank `DAT_80085758` (via `func_0x8003CE64`). Encoding
        // `[2F][13|14][flag][delta]`: op_w(2) = flag index, op_w(3) =
        // signed u16-word displacement. The shared `LAB_801D4830` epilogue
        // returns size 4 (fall through past the 4-word op) on the
        // untaken side and `4 + delta` on the taken side (`lhu v0,0x6(s3);
        // addiu s2,v0,4` at `0x801D4838` - the delta may be negative,
        // forming a spin-wait back onto a preceding wait op). 0x13 takes
        // the branch when the flag is SET; 0x14 when it is CLEAR - jou's
        // ambient lightning cyclers idle on `0x14 [0x364, -6]` until the
        // director raises the flag, and exit their strobe loop through
        // `0x14 [0x364, +1]` when it drops.
        0x13 => {
            if host.ext_query_flag_bank(op_w(2) as i16) != 0 {
                MoveExtResult::with_size(4i16.wrapping_add(op_w(3) as i16))
            } else {
                MoveExtResult::with_size(4)
            }
        }
        0x14 => {
            if host.ext_query_flag_bank(op_w(2) as i16) != 0 {
                MoveExtResult::with_size(4)
            } else {
                MoveExtResult::with_size(4i16.wrapping_add(op_w(3) as i16))
            }
        }

        // 0x15 / 0x16 - set a flag bit on actor.flags (mask 0x800000 /
        // 0x200000). Both preset `li s2, 0x2` (0x801D3B8C / 0x801D3B9C) and
        // share the 0x801D3BAC exit, so both are size 2.
        0x15 => {
            state.flags |= 0x800000;
            MoveExtResult::with_size(2)
        }
        0x16 => {
            state.flags |= 0x200000;
            MoveExtResult::with_size(2)
        }

        // 0x17 - world-struct init. op_w(2) = idx (param_2+4); op_w(3..7) = 5 vals.
        // Size 8 (`li s2, 0x8` at 0x801D3C00).
        0x17 => {
            let idx = op_w(2) as i16;
            let vals = [
                op_w(3) as i16,
                op_w(4) as i16,
                op_w(5) as i16,
                op_w(6) as i16,
                op_w(7) as i16,
            ];
            host.ext_world_struct_init(idx, vals);
            MoveExtResult::with_size(8)
        }
        // 0x18 - world-struct **reset** variant, and the one member of the
        // 0x17..0x1A family that is not 8 halfwords wide. The arm at
        // `0x801D3C0C` zeroes the record's first three u16s outright and
        // seeds only the last two, from `actor.world_y + op[3]` and
        // `actor.world_y + op[4]` - so it reads two operand words, not five,
        // and exits `li s2, 0x5` at `0x801D3C58`. Grouping it with 0x19 /
        // 0x1A costs 3 halfwords of PC per execution.
        0x18 => {
            let idx = op_w(2) as i16;
            let y = state.world_y;
            let vals = [
                0,
                0,
                0,
                y.wrapping_add(op_w(3) as i16),
                y.wrapping_add(op_w(4) as i16),
            ];
            host.ext_world_struct_write(idx, vals);
            MoveExtResult::with_size(5)
        }
        // 0x19 / 0x1A - world-struct write variants. Same op_w(2) = idx.
        // Size 8 (`li s2, 0x8` at 0x801D3CD0 / in the 0x801D3D7C exit).
        0x19 | 0x1A => {
            let idx = op_w(2) as i16;
            let vals = [
                op_w(3) as i16,
                op_w(4) as i16,
                op_w(5) as i16,
                op_w(6) as i16,
                op_w(7) as i16,
            ];
            host.ext_world_struct_write(idx, vals);
            MoveExtResult::with_size(8)
        }

        // 0x1B - in-bytecode copy loop. For `i in 0..op[4]`:
        //   buffer[state.pc + op[3] + i + 5] = buffer[state.pc + op[2] + i + 5]
        // The base offset of 5 (= u16-index 5) targets the operand region
        // *past* the count word - the bytecode following this instruction
        // is treated as an inline scratch buffer indexed by op[2]/op[3].
        // Size 5 regardless of the count: both the `blez` early-out at
        // `0x801D3D90` and the loop exit reach a `li s2, 0x5`.
        0x1B => {
            let count = op_w(4) as i16;
            if count > 0 {
                let src_base = state.pc as usize + op_w(2) as usize + 5;
                let dst_base = state.pc as usize + op_w(3) as usize + 5;
                for i in 0..count as usize {
                    let v = host.move_bytecode_read_u16(src_base + i);
                    host.move_bytecode_write_u16(dst_base + i, v);
                }
            }
            MoveExtResult::with_size(5)
        }

        // 0x1C / 0x1D - set / clear flag bank. op_w(2) = flag index.
        0x1C => {
            host.ext_set_flag_bank(op_w(2) as i16);
            MoveExtResult::with_size(3)
        }
        0x1D => {
            host.ext_clear_flag_bank(op_w(2) as i16);
            MoveExtResult::with_size(3)
        }

        // 0x1E - in-place add: `buffer[state.pc + op[2] + 4] += op[3]`.
        // Read-modify-write of a single u16 inside the move bytecode.
        // Wrapping i16 add per the original `*(short *)(...) + *(short *)(...)`.
        //
        // Size 4 per the raw arm at `overlay_0897_801d362c` `0x801D3E18..`:
        // `li s2, 0x4` before the joint `j 0x801D4A3C` return (the decompiled
        // C renders the return as a `func_0x801d4a3c()` label-call, hiding
        // the size). The instruction therefore skips its own operand words -
        // jou's ambient CLUT-cycler record relies on this to step the
        // *following* op-`0x2C` capture cell per spawned instance without
        // re-executing the patched word as an opcode.
        0x1E => {
            let off = state.pc as usize + op_w(2) as usize + 4;
            let cur = host.move_bytecode_read_u16(off) as i16;
            let new = cur.wrapping_add(op_w(3) as i16);
            host.move_bytecode_write_u16(off, new as u16);
            MoveExtResult::with_size(4)
        }

        // 0x1F / 0x20 - HSV-space color ramp on `actor[+0xa0]` (sub-op 0x1F)
        // or `actor[+0xa4]` (sub-op 0x20). The packed u32 holds an RGB triple:
        // R = byte 0, G = byte 1, B = byte 2 (bit 24-31 reserved).
        //
        // Per FUN_801D362C (overlay_0897_801d362c, case 0x1f/0x20):
        //   1. Decompose puVar14[0..3] into R/G/B (each 0..255).
        //   2. RGB→HSV via FUN_8001a78c → (H ∈ 0..0x167, S ∈ 0..255, V ∈ 0..255).
        //   3. H += op[2]; wrap into [0, 0x167]. S += op[3], clamp 0..255.
        //      V += op[4], clamp 0..255.
        //   4. HSV→RGB via FUN_8001a6c8 (which clamps each output to 0..0xF8).
        //   5. Re-pack `puVar14[0] = R | G<<8 | B<<16` with a full 32-bit
        //      `sw` (`sw v1,0x0(s0)` at 0x801D3F84), so the top byte of the
        //      packed word is cleared, not preserved.
        //
        // **Size 5**, not 1. The single arm serving both sub-ops sets
        // `li s2, 0x5` at `0x801D3F60` - after the HSV→RGB call and before
        // the shared `j 0x801d4a3c` at `0x801D3F80` - and there is no other
        // `s2` write on any path through it. The "size-1 return is an
        // intentional bytecode-density trick that re-reads `op[2..]` as a
        // fresh outer opcode" reading came from the decompiled C, where the
        // `j` renders as a label-call and the delay-slot-adjacent `li` is
        // dropped; the instruction is an ordinary 5-halfword op whose three
        // operand words are the H/S/V deltas.
        0x1F | 0x20 => {
            let kd_offset = if sub_opcode == 0x1F { 0 } else { 2 };
            let lo = state.keyframe_desc[kd_offset];
            let hi = state.keyframe_desc[kd_offset + 1];
            let r = (lo & 0xFF) as i32;
            let g = ((lo >> 8) & 0xFF) as i32;
            let b = (hi & 0xFF) as i32;
            let (mut h, mut s, mut v) = rgb_to_hsv(r, g, b);
            h += op_w(2) as i16 as i32;
            while h < 0 {
                h += 0x168;
            }
            while h > 0x167 {
                h -= 0x168;
            }
            s = (s + op_w(3) as i16 as i32).clamp(0, 0xFF);
            v = (v + op_w(4) as i16 as i32).clamp(0, 0xFF);
            let (nr, ng, nb) = hsv_to_rgb(h, s, v);
            // FUN_8001a6c8 clamps each channel to 0..0xF8.
            let nr = nr.min(0xF8) as u16;
            let ng = ng.min(0xF8) as u16;
            let nb = nb.min(0xF8) as u16;
            state.keyframe_desc[kd_offset] = nr | (ng << 8);
            state.keyframe_desc[kd_offset + 1] = nb;
            MoveExtResult::with_size(5)
        }

        // 0x21 - `actor.anim_3c..40 += op_w(2..4)`.
        // Size 5 (`li s2, 0x5` at 0x801D3FBC).
        0x21 => {
            state.anim_3c = state.anim_3c.wrapping_add(op_w(2) as i16);
            state.anim_3e = state.anim_3e.wrapping_add(op_w(3) as i16);
            state.anim_40 = state.anim_40.wrapping_add(op_w(4) as i16);
            MoveExtResult::with_size(5)
        }

        // 0x22 - `actor.world += op_w(2..4)`. Size 5 (`li s2, 0x5` at 0x801D3FF0).
        0x22 => {
            state.world_x = state.world_x.wrapping_add(op_w(2) as i16);
            state.world_y = state.world_y.wrapping_add(op_w(3) as i16);
            state.world_z = state.world_z.wrapping_add(op_w(4) as i16);
            MoveExtResult::with_size(5)
        }

        // 0x23 - animation lerp toward target world coords using the
        // scratchpad ramp counter at `_DAT_1F800393`. Per the dump:
        //   t = (DAT_1F800393 << 12) / op[5];                  // 12.0 ratio
        //   anim_3c -= (anim_3c * t) >> 12;                     // remove old
        //   anim_3e -= (anim_3e * t) >> 12;
        //   anim_40 -= (anim_40 * t) >> 12;
        //   anim_3c += ((op[2] - actor.world_x) * t) >> 12;     // toward target
        //   anim_3e += ((op[3] - actor.world_y) * t) >> 12;
        //   anim_40 += ((op[4] - actor.world_z) * t) >> 12;
        // The original `trap(0x1c00)` / `trap(0x1800)` divide-by-zero traps are
        // skipped at the source-line level by the MIPS divide-trap pattern;
        // we simply guard `denom == 0` and skip the update. This is faithful
        // to the in-game behavior because the trap signal would terminate
        // execution rather than continue with a bogus ratio.
        0x23 => {
            let denom = op_w(5) as i16 as i32;
            if denom != 0 {
                let dat = host.move_dat_1f800393() as u32 as i32;
                let t = (dat << 12) / denom;
                let s = |v: i16| -> i16 { v.wrapping_sub(((v as i32 * t) >> 12) as i16) };
                state.anim_3c = s(state.anim_3c);
                state.anim_3e = s(state.anim_3e);
                state.anim_40 = s(state.anim_40);
                let dx = ((op_w(2) as i16 as i32 - state.world_x as i32) * t) >> 12;
                let dy = ((op_w(3) as i16 as i32 - state.world_y as i32) * t) >> 12;
                let dz = ((op_w(4) as i16 as i32 - state.world_z as i32) * t) >> 12;
                state.anim_3c = state.anim_3c.wrapping_add(dx as i16);
                state.anim_3e = state.anim_3e.wrapping_add(dy as i16);
                state.anim_40 = state.anim_40.wrapping_add(dz as i16);
            }
            // Size 6 (`li s2, 0x6` at 0x801D4100) - the trap-guard path does
            // not change the width, only whether the update lands.
            MoveExtResult::with_size(6)
        }

        // 0x24 / 0x2A - fixed-point lerp on actor world coords. Both share
        // the per-axis form `actor[axis] = op[axis] + ((target - op[axis]) *
        // op[axis_t]) >> 12`. The Y axis always lerps toward player.world_y
        // (with operand `op_w(3)` as base, `op_w(6)` as t). The X axis and
        // Z axis differ by sub-op:
        //   0x24 - uses `(_DAT_80089118, _DAT_80089120)` map origin: target
        //          = `-(op + origin)` (i.e. fixed map-relative anchor).
        //   0x2A - uses `(player.world_x, player.world_z)`.
        //
        // Operand layout: op_w(2,3,4) = base x/y/z; op_w(5)=t_x, op_w(6)=t_y,
        // op_w(7)=t_z (each scaled by `>> 12`).
        0x24 | 0x2A => {
            let player = host.move_player_world_xyz();
            let (origin_x, origin_z) = host.move_fixed_origin_xz();
            let base_x = op_w(2) as i16 as i32;
            let base_y = op_w(3) as i16 as i32;
            let base_z = op_w(4) as i16 as i32;
            let t_x = op_w(5) as i16 as i32;
            let t_y = op_w(6) as i16 as i32;
            let t_z = op_w(7) as i16 as i32;

            let (target_x, target_z) = if sub_opcode == 0x24 {
                // Fixed-origin path: the dump's `-op - origin` is the
                // signed displacement from `-(op + origin)`.
                (-(base_x + origin_x), -(base_z + origin_z))
            } else {
                (player[0] as i32, player[2] as i32)
            };

            // X axis.
            state.world_x = (base_x + (((target_x - base_x).wrapping_mul(t_x)) >> 12)) as i16;
            // Y axis (always vs. player).
            state.world_y =
                (base_y + (((player[1] as i32 - base_y).wrapping_mul(t_y)) >> 12)) as i16;
            // Z axis.
            state.world_z = (base_z + (((target_z - base_z).wrapping_mul(t_z)) >> 12)) as i16;
            // Size 8 for both sub-ops - the shared arm at 0x801D411C exits
            // through 0x801D4208 with `li s2, 0x8`.
            MoveExtResult::with_size(8)
        }

        // 0x25 - save `actor[+0x14..+0x1C]` (world coords + Y mirror) into
        // the 16-slot scratch table at `&DAT_801F3498`. Each slot is 8 bytes:
        // `slot[0..4] = (world_x:u16, world_y:u16)`,
        // `slot[4..8] = (world_z:u16, world_y_mirror:u16)`.
        //
        // Size 3 (`li s2, 0x3` in the 0x801D4244 exit): opcode + sub-op +
        // the slot operand. This is the arm whose under-advance re-enters
        // the outer dispatcher on the `0x25` word, where the *outer* opcode
        // space reads `0x25` as CHILD_SPAWN.
        0x25 => {
            let slot = op_w(2);
            let lo = (state.world_x as u16 as u32) | ((state.world_y as u16 as u32) << 16);
            let hi = (state.world_z as u16 as u32) | ((state.world_y_mirror as u16 as u32) << 16);
            host.move_slot_save_u32(slot, 0, lo);
            host.move_slot_save_u32(slot, 4, hi);
            MoveExtResult::with_size(3)
        }

        // 0x26 - load 8 bytes from the scratch slot back into
        // `actor[+0x14..+0x1C]`. Size 3 (`li s2, 0x3` in the 0x801D4280 exit).
        0x26 => {
            let slot = op_w(2);
            let lo = host.move_slot_load_u32(slot, 0);
            let hi = host.move_slot_load_u32(slot, 4);
            state.world_x = (lo & 0xFFFF) as i16;
            state.world_y = ((lo >> 16) & 0xFFFF) as i16;
            state.world_z = (hi & 0xFFFF) as i16;
            state.world_y_mirror = ((hi >> 16) & 0xFFFF) as i16;
            MoveExtResult::with_size(3)
        }

        // 0x27 - save the three tween-source u16s `actor[+0x90..+0x96]` into
        // the slot's first 6 bytes (slot[0/2/4] = tween_src_x/y/z).
        // Size 3 (`li s2, 0x3` in the 0x801D42D0 exit).
        0x27 => {
            let slot = op_w(2);
            host.move_slot_save_u16(slot, 0, state.tween_src_x as u16);
            host.move_slot_save_u16(slot, 2, state.tween_src_y as u16);
            host.move_slot_save_u16(slot, 4, state.tween_src_z as u16);
            MoveExtResult::with_size(3)
        }

        // 0x28 - load 3 × u16 from the slot, scale `+0x92/+0x94` by
        // `op_w(3)/op_w(4)` (with `>> 12` fixed-point shift), and clamp the
        // scaled outputs to `[-0xFF, 0xFF]`.
        //
        // **Size 5 on every path.** The clamp cascade ends
        // `bne v0,zero,0x801d4a3c` with `li s2, 0x5` in the delay slot
        // (`0x801D43A8`), and the fall-through stores the upper bound and
        // exits at `0x801D43B4` with `s2` still 5 - the branch selects
        // whether z is clamped, not how wide the instruction is. The earlier
        // reading turned that branch into a size difference.
        #[allow(clippy::manual_clamp)]
        0x28 => {
            let slot = op_w(2);
            let scale_y = op_w(3) as i16 as i32;
            let scale_z = op_w(4) as i16 as i32;
            state.tween_src_x = host.move_slot_load_u16(slot, 0) as i16;
            let raw_y = host.move_slot_load_u16(slot, 2) as i16 as i32;
            let raw_z = host.move_slot_load_u16(slot, 4) as i16 as i32;
            let mut y_scaled = ((raw_y * scale_y) >> 12) as i16;
            let mut z_scaled = ((raw_z * scale_z) >> 12) as i16;
            if y_scaled < -0xFF {
                y_scaled = -0xFF;
            }
            if y_scaled > 0xFF {
                y_scaled = 0xFF;
            }
            if z_scaled < -0xFF {
                z_scaled = -0xFF;
            }
            if z_scaled > 0xFF {
                z_scaled = 0xFF;
            }
            state.tween_src_y = y_scaled;
            state.tween_src_z = z_scaled;
            MoveExtResult::with_size(5)
        }

        // 0x31 - save `actor[+0x24..+0x2C]` (the three render banks +
        // `+0x2A` Y mirror) into the slot. Size 3 (`li s2, 0x3` at 0x801D4664).
        0x31 => {
            let slot = op_w(2);
            let lo = (state.render_24 as u16 as u32) | ((state.render_26 as u16 as u32) << 16);
            let hi = (state.render_28 as u16 as u32) | ((state.world_y_mirror as u16 as u32) << 16);
            host.move_slot_save_u32(slot, 0, lo);
            host.move_slot_save_u32(slot, 4, hi);
            MoveExtResult::with_size(3)
        }

        // 0x32 - load 8 bytes from the slot back into the render-bank
        // section at `+0x24..+0x2C`. Size 3 (`li s2, 0x3` at 0x801D46A0).
        0x32 => {
            let slot = op_w(2);
            let lo = host.move_slot_load_u32(slot, 0);
            let hi = host.move_slot_load_u32(slot, 4);
            state.render_24 = (lo & 0xFFFF) as i16;
            state.render_26 = ((lo >> 16) & 0xFFFF) as i16;
            state.render_28 = (hi & 0xFFFF) as i16;
            state.world_y_mirror = ((hi >> 16) & 0xFFFF) as i16;
            MoveExtResult::with_size(3)
        }

        // 0x34 - save `actor[+0x72]` (`field_72`) into slot[0..2].
        // Size 3 (`li s2, 0x3` at 0x801D46FC).
        0x34 => {
            let slot = op_w(2);
            host.move_slot_save_u16(slot, 0, state.field_72);
            MoveExtResult::with_size(3)
        }

        // 0x35 - load slot[0..2] into `actor[+0x72]`.
        // Size 3 (`li s2, 0x3` at 0x801D4738).
        0x35 => {
            let slot = op_w(2);
            state.field_72 = host.move_slot_load_u16(slot, 0);
            MoveExtResult::with_size(3)
        }

        // 0x29 - scratchpad ramp or immediate write. op_w(2)=slot,
        // op_w(3)=target, op_w(4)=ticks. Size 5 on both paths - the
        // immediate arm jumps into the shared 0x801D4624 tail that 0x30
        // also ends on (`li s2, 0x5` at 0x801D4628).
        0x29 => {
            let slot = op_w(2) as i16;
            let target = op_w(3) as i16;
            let ticks = op_w(4) as i16;
            if ticks != 0 {
                host.ext_scratchpad_ramp(slot, -target, ticks);
            } else {
                host.ext_scratchpad_write(slot, -target);
            }
            MoveExtResult::with_size(5)
        }

        // 0x2B - `actor[+0xB4..+0xBC] = op_w(2..6)`. Writes 4 u16 anim-block
        // slots (`anim_block_u16` at byte-off 8/10/12/14 = `+0xB4/B6/B8/BA`).
        // Size 6 (`li s2, 0x6` at 0x801D4460) - opcode + sub-op + 4 operands.
        0x2B => {
            state.anim_block_u16_set(8, op_w(2));
            state.anim_block_u16_set(10, op_w(3));
            state.anim_block_u16_set(12, op_w(4));
            state.anim_block_u16_set(14, op_w(5));
            MoveExtResult::with_size(6)
        }

        // 0x2C - overlay sub-routine (`jal 0x801d31b0`, the per-scanline
        // POLY_FT4 strip emitter). Size 7 (`li s2, 0x7` at 0x801D44D4).
        0x2C => {
            host.ext_func801d31b0(state, operand);
            MoveExtResult::with_size(7)
        }

        // 0x2D - additive variant of 0x2B.
        // `actor[+0xB4..+0xBC] += op_w(2..6)`. Wrapping add per the
        // `*(short *)` semantics in the original. Size 6 (`li s2, 0x6` at
        // 0x801D44B4).
        0x2D => {
            for (slot, idx) in [(8, 2), (10, 3), (12, 4), (14, 5)] {
                let cur = state.anim_block_u16(slot);
                let add = op_w(idx);
                state.anim_block_u16_set(slot, cur.wrapping_add(add));
            }
            MoveExtResult::with_size(6)
        }

        // 0x2E - build the GP0 draw-mode packet and link it into the OT.
        // The widest arm after 0x00: size 13 (the 0x801D45CC exit), covering
        // opcode + sub-op + 11 operand words.
        0x2E => {
            host.ext_emit_ot_packet(operand);
            MoveExtResult::with_size(13)
        }

        // 0x2F - write `_DAT_8007B9D8`. op_w(2) = the i16 value.
        // Size 3 (`li s2, 0x3` at 0x801D45D4, the arm's first instruction).
        0x2F => {
            host.ext_set_8007b9d8(op_w(2) as i16 as i32);
            MoveExtResult::with_size(3)
        }

        // 0x30 - RAND_PICK self-modify: write `op_w(2)` or `op_w(3)`
        // (coin-flip on `rand() & 1`) into the bytecode word at
        // `pc + op_w(4) + 5` - same destination law as 0x05
        // (`overlay_0897_801d362c.txt` 0x801D45E8..; `li s2, 5`).
        0x30 => {
            let pick = if host.ext_rand16() & 1 != 0 {
                op_w(2)
            } else {
                op_w(3)
            };
            let dst = state.pc as usize + op_w(4) as usize + 5;
            host.move_bytecode_write_u16(dst, pick);
            MoveExtResult::with_size(5)
        }

        // 0x33 - `actor[+0xC0..+0xC8] += op_w(2..6)` (4 i16 anim-block slots
        // at byte-off 20/22/24/26 = `+0xC0/C2/C4/C6`). Wrapping add per the
        // `*(short *) +` semantics in the original. Size 6 (`li s2, 0x6` at
        // 0x801D46EC).
        0x33 => {
            for (slot, idx) in [(20, 2), (22, 3), (24, 4), (26, 5)] {
                let cur = state.anim_block_u16(slot);
                let add = op_w(idx);
                state.anim_block_u16_set(slot, cur.wrapping_add(add));
            }
            MoveExtResult::with_size(6)
        }

        // 0x36 / 0x37 - axis threshold against `0x8E - DAT_8007C348`.
        // 0x38 / 0x39 - squared-distance to the player.
        //
        // All four are **conditional branches** of the same shape as 0x13 /
        // 0x14, 4 halfwords wide: `[2F][36..39][arg][delta]`. Each arm
        // presets `li s2, 0x4` (0x801D4744 / 0x801D4764 / 0x801D47C8 /
        // 0x801D4820) and jumps to the shared `0x801D4830` tail, whose
        // predicate-true side is `lhu v0,0x6(s3); addiu s2,v0,0x4` - i.e.
        // `4 + op[3]`, with the same signed-truncation rule that lets a
        // negative delta walk the PC backwards. The "true -> size 1" reading
        // dropped both the base and the branch.
        //
        //  - 0x36: op[2] < (0x8E - axis)            ; "outside lower bound"
        //  - 0x37: (0x8E - axis) < op[2]            ; "above upper bound"
        //  - 0x38: op[2]^2 < ((dx*dx) + (dz*dz))    ; "outside radius"
        //  - 0x39: ((dx*dx) + (dz*dz)) < op[2]^2    ; "inside radius"
        //
        // dx/dz are `actor.world - player.world`. The default `MoveHost`
        // returns the origin for the player, so engines that don't model
        // the player position get "actor at the origin offset" - close
        // enough for static unit tests; real hosts override.
        0x36..=0x39 => {
            let v = op_w(2) as i16 as i32;
            let predicate = match sub_opcode {
                0x36 => v < 0x8E - (host.move_axis_threshold() as i32),
                0x37 => 0x8E - (host.move_axis_threshold() as i32) < v,
                _ => {
                    let player = host.move_player_world_xyz();
                    let dx = state.world_x as i32 - player[0] as i32;
                    let dz = state.world_z as i32 - player[2] as i32;
                    let dist_sq = dx * dx + dz * dz;
                    let r_sq = v * v;
                    if sub_opcode == 0x38 {
                        r_sq < dist_sq
                    } else {
                        dist_sq < r_sq
                    }
                }
            };
            if predicate {
                MoveExtResult::with_size(4i16.wrapping_add(op_w(3) as i16))
            } else {
                MoveExtResult::with_size(4)
            }
        }

        // 0x3A - angle to player. Original:
        //   sVar9 = *(short*)(param_2 + 4);
        //   uVar6 = func_0x80019B28(actor.z, actor.x, player.z, player.x);
        //   *(short*)(param_2 + sVar9*2 + 6) = uVar6;
        //
        // Self-modifying write: the dst index is `op_w(2) + 3` in u16 units
        // off the current pc. We write through `move_bytecode_write_u16`
        // (deferred + flushed by the host) so the write survives to the
        // engine's bytecode buffer.
        //
        // Size 3 (`li s2, 0x3` at 0x801D4844, the arm's first instruction).
        0x3A => {
            let angle = host.ext_compute_angle(state);
            let dst = state.pc as usize + op_w(2) as usize + 3;
            host.move_bytecode_write_u16(dst, angle);
            MoveExtResult::with_size(3)
        }

        // 0x3B - party-member position lookup. Original:
        //   puVar15 = (short*)(param_2 + op_w(3)*2 + 8);
        //   *puVar15 = puVar15[1] = puVar15[2] = 0;
        //   func_0x8003D064(_DAT_8007B898 + 0x22, &local, ...);
        //   actor = func_0x8003C83C(local + op_w(2) + 1);
        //   iVar16 = 4;
        //   if (actor) { puVar15[0..2] = actor.world; }
        //
        // dst slot = `op_w(3) + 4` in u16 units off pc. Pre-clear the 3
        // slots before the lookup so a host that returns `None` still has
        // the zero-pre-clear behavior the original guarantees.
        //
        // **Size 4 on both paths.** `beq v1,zero,0x801d4a3c` at 0x801D48D0
        // carries `li s2, 0x4` in its delay slot, so the success path exits
        // at 0x801D48F4 with the same 4 - the branch chooses whether the
        // triple is written, not the instruction width.
        0x3B => {
            let dst = state.pc as usize + op_w(3) as usize + 4;
            host.move_bytecode_write_u16(dst, 0);
            host.move_bytecode_write_u16(dst + 1, 0);
            host.move_bytecode_write_u16(dst + 2, 0);
            let slot = op_w(2) as i16;
            match host.ext_party_member_lookup(slot) {
                Some([x, y, z]) => {
                    host.move_bytecode_write_u16(dst, x as u16);
                    host.move_bytecode_write_u16(dst + 1, y as u16);
                    host.move_bytecode_write_u16(dst + 2, z as u16);
                    MoveExtResult::with_size(4)
                }
                None => MoveExtResult::with_size(4),
            }
        }

        // 0x3C - fade colour. op_w(2,3,4) = r/g/b (low bytes), op_w(5)=ticks.
        // The original reads `*(undefined1 *)(param_2 + 4)` etc - that's the
        // low byte of u16-index 2. op_w returns u16, so we cast to u8.
        0x3C => {
            let r = op_w(2) as u8;
            let g = op_w(3) as u8;
            let b = op_w(4) as u8;
            let ticks = op_w(5);
            host.ext_fade_color([r, g, b], ticks);
            MoveExtResult::with_size(6)
        }

        // Anything `>= 0x3D` is reserved / unknown. This is the **only**
        // size-1 path in the dispatcher: `li s2, 0x1` at `0x801D365C`, in
        // the delay slot of the bounds-check branch that skips the whole
        // jump table. Every in-range sub-opcode has its own arm and its own
        // wider `li s2, N`.
        // FUN_801D362C guards the JT jump with `sltiu sub_op, 0x3D` (the
        // sub-opcode is loaded `lh` = sign-extended, so the *unsigned* compare
        // also rejects negative values), branching to its return on
        // out-of-range - so the extension dispatch has no OOB-jump path; this
        // catch-all is the faithful mirror of that guarded return.
        _ => MoveExtResult::default_arm(),
    }
}

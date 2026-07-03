//! Extension-VM (opcode 0x2F) default dispatcher, ported from `FUN_801D362C`.

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
        // 0x00 - falls into default arm (size 1).
        0x00 => MoveExtResult::default_arm(),

        // 0x01 - `func_0x8001a068("EFC %d %d %d", x, y, z)` debug print.
        // Original sets `iVar16 = 0x20000` then breaks (size = 2).
        0x01 => {
            host.ext_debug_world(state.world_x, state.world_y, state.world_z);
            MoveExtResult::with_size(2)
        }

        // 0x02 - clear face_rotation. Default-arm size.
        0x02 => {
            state.face_rotation = 0;
            MoveExtResult::default_arm()
        }

        // 0x03 - clear flags bit 0x1000.
        0x03 => {
            state.flags &= !0x1000;
            MoveExtResult::default_arm()
        }

        // 0x04 - write actor world XYZ into the operand slot at u16-index
        // `op[2] + 3`. The "self-modifying" pattern stores a copy of the
        // current world coords into the move-bytecode itself; the absolute
        // word offset is `state.pc + op[2] + 3`. 3 consecutive writes for
        // x/y/z. Default-arm size.
        0x04 => {
            let base = state.pc as usize + op_w(2) as usize + 3;
            host.move_bytecode_write_u16(base, state.world_x as u16);
            host.move_bytecode_write_u16(base + 1, state.world_y as u16);
            host.move_bytecode_write_u16(base + 2, state.world_z as u16);
            MoveExtResult::default_arm()
        }

        // 0x05 - opaque func56798().
        0x05 => host.ext_func56798(state),

        // 0x06 / 0x07 - bbox-vs-player test. The original canonicalises the
        // box in-place by swapping `op_w(2)/(4)` if `op_w(4) < op_w(2)`, then
        // tests whether the player position falls inside `[(xa-0x40)/0x80,
        // (xb+0x40)/0x80]` × `[(za-0x40)/0x80, (zb+0x40)/0x80]`. 0x06 means
        // "default-arm if inside, size 7 if outside"; 0x07 means the opposite.
        // The size-7 branch skips a 7-u16 follow-up payload. We can't mutate
        // the operand stream from here (it's a read-only slice), so the port
        // forwards the predicate to the host: `move_player_world_xyz` returns
        // the player position. If the host reports the origin (the default
        // impl), the test always reads "outside the box" - so we model 0x06
        // as a skip (size 7) and 0x07 as a continue (default-arm). Hosts
        // that surface real player coords get the right behavior.
        0x06 | 0x07 => {
            let player = host.move_player_world_xyz();
            // op_w(2)..op_w(5) are the four corner u16s. We do NOT swap in
            // place because the operand is a read-only slice.
            let mut xa = op_w(2) as i16 as i32;
            let mut xb = op_w(4) as i16 as i32;
            let mut za = op_w(3) as i16 as i32;
            let mut zb = op_w(5) as i16 as i32;
            if xb < xa {
                std::mem::swap(&mut xa, &mut xb);
            }
            if zb < za {
                std::mem::swap(&mut za, &mut zb);
            }
            // Original scales by 0x80 with a 0x40 half-cell margin.
            let outside = (player[0] as i32) < xa * 0x80 + 0x40
                || (player[2] as i32) < za * 0x80 + 0x40
                || xb * 0x80 + 0x40 < player[0] as i32
                || zb * 0x80 + 0x40 < player[2] as i32;
            // 0x06: outside is the skip path (size 7); 0x07: inside is.
            let skip = if sub_opcode == 0x06 {
                outside
            } else {
                !outside
            };
            if skip {
                MoveExtResult::with_size(7)
            } else {
                MoveExtResult::default_arm()
            }
        }

        // 0x08 - `DAT_801F22F4 = 1` (set move-VM global predicate).
        0x08 => {
            host.move_global_predicate_set(1);
            MoveExtResult::default_arm()
        }

        // 0x09 - `DAT_801F22F4 = 0` (clear).
        0x09 => {
            host.move_global_predicate_set(0);
            MoveExtResult::default_arm()
        }

        // 0x0A - predicate gate: if `DAT_801F22F4 != 0` falls into default
        // (size 1, advance and continue); else returns size 3 (skip a 3-u16
        // payload). Per the dump's `iVar16 = 3; if (DAT_801f22f4 != 0)
        // goto LAB_801d38c8; ...; default:` shape - `LAB_801d38c8` is the
        // default-arm label, and the fall-through hits `iVar16 << 0x10 break`
        // with `iVar16 = 3`.
        0x0A => {
            if host.move_global_predicate_get() != 0 {
                MoveExtResult::default_arm()
            } else {
                MoveExtResult::with_size(3)
            }
        }

        // 0x0B - opposite predicate (skip when set).
        0x0B => {
            if host.move_global_predicate_get() == 0 {
                MoveExtResult::default_arm()
            } else {
                MoveExtResult::with_size(3)
            }
        }

        // 0x0C - `actor[+0x50] = op_w(2)` (set midpoint blend / sub-state).
        0x0C => {
            state.field_50 = op_w(2);
            MoveExtResult::default_arm()
        }

        // 0x0D - `actor[+0x50] += op_w(2)` (additive variant).
        0x0D => {
            state.field_50 = state.field_50.wrapping_add(op_w(2));
            MoveExtResult::default_arm()
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
        0x0F => {
            host.move_global_counter_set(0);
            MoveExtResult::default_arm()
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
            MoveExtResult::default_arm()
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
            MoveExtResult::default_arm()
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

        // 0x13 / 0x14 - flag-bank predicate tests against the fourth flag
        // bank `DAT_80085758` (via `func_0x8003CE64`). op_w(2) = flag index.
        // Both jump to the shared `LAB_801D4830` epilogue: returns 1 when
        // `predicate != 0`, else 4. 0x13 falls through unconditionally
        // (`goto LAB_801D4830`); 0x14 inverts (returns 4 when predicate is
        // set, default-arm when clear).
        0x13 => {
            if host.ext_query_flag_bank(op_w(2) as i16) != 0 {
                MoveExtResult::default_arm()
            } else {
                MoveExtResult::with_size(4)
            }
        }
        0x14 => {
            if host.ext_query_flag_bank(op_w(2) as i16) != 0 {
                MoveExtResult::with_size(4)
            } else {
                MoveExtResult::default_arm()
            }
        }

        // 0x15 / 0x16 - set a flag bit on actor.flags (mask 0x800000 / 0x200000).
        0x15 => {
            state.flags |= 0x800000;
            MoveExtResult::default_arm()
        }
        0x16 => {
            state.flags |= 0x200000;
            MoveExtResult::default_arm()
        }

        // 0x17 - world-struct init. op_w(2) = idx (param_2+4); op_w(3..7) = 5 vals.
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
            MoveExtResult::default_arm()
        }
        // 0x18/0x19/0x1A - world-struct write variants. Same op_w(2) = idx.
        0x18..=0x1A => {
            let idx = op_w(2) as i16;
            let vals = [
                op_w(3) as i16,
                op_w(4) as i16,
                op_w(5) as i16,
                op_w(6) as i16,
                op_w(7) as i16,
            ];
            host.ext_world_struct_write(idx, vals);
            MoveExtResult::default_arm()
        }

        // 0x1B - in-bytecode copy loop. For `i in 0..op[4]`:
        //   buffer[state.pc + op[3] + i + 5] = buffer[state.pc + op[2] + i + 5]
        // The base offset of 5 (= u16-index 5) targets the operand region
        // *past* the count word - the bytecode following this instruction
        // is treated as an inline scratch buffer indexed by op[2]/op[3].
        // Falls into default-arm regardless of the count.
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
            MoveExtResult::default_arm()
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
        0x1E => {
            let off = state.pc as usize + op_w(2) as usize + 4;
            let cur = host.move_bytecode_read_u16(off) as i16;
            let new = cur.wrapping_add(op_w(3) as i16);
            host.move_bytecode_write_u16(off, new as u16);
            MoveExtResult::default_arm()
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
        //   5. Re-pack `puVar14[0] = R | G<<8 | B<<16`.
        //
        // Returns default_arm() (size 1) - the bytecode operand stream is
        // re-interpreted as outer opcode 0x1F / 0x20 on the next dispatch
        // (intentional self-modifying layout - see also sub-ops 0x04/0x1B/0x1E).
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
            state.keyframe_desc[kd_offset + 1] = (hi & 0xFF00) | nb;
            MoveExtResult::default_arm()
        }

        // 0x21 - `actor.anim_3c..40 += op_w(2..4)`.
        0x21 => {
            state.anim_3c = state.anim_3c.wrapping_add(op_w(2) as i16);
            state.anim_3e = state.anim_3e.wrapping_add(op_w(3) as i16);
            state.anim_40 = state.anim_40.wrapping_add(op_w(4) as i16);
            MoveExtResult::default_arm()
        }

        // 0x22 - `actor.world += op_w(2..4)`.
        0x22 => {
            state.world_x = state.world_x.wrapping_add(op_w(2) as i16);
            state.world_y = state.world_y.wrapping_add(op_w(3) as i16);
            state.world_z = state.world_z.wrapping_add(op_w(4) as i16);
            MoveExtResult::default_arm()
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
            MoveExtResult::default_arm()
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
            MoveExtResult::default_arm()
        }

        // 0x25 - save `actor[+0x14..+0x1C]` (world coords + Y mirror) into
        // the 16-slot scratch table at `&DAT_801F3498`. Each slot is 8 bytes:
        // `slot[0..4] = (world_x:u16, world_y:u16)`,
        // `slot[4..8] = (world_z:u16, world_y_mirror:u16)`.
        0x25 => {
            let slot = op_w(2);
            let lo = (state.world_x as u16 as u32) | ((state.world_y as u16 as u32) << 16);
            let hi = (state.world_z as u16 as u32) | ((state.world_y_mirror as u16 as u32) << 16);
            host.move_slot_save_u32(slot, 0, lo);
            host.move_slot_save_u32(slot, 4, hi);
            MoveExtResult::default_arm()
        }

        // 0x26 - load 8 bytes from the scratch slot back into
        // `actor[+0x14..+0x1C]`.
        0x26 => {
            let slot = op_w(2);
            let lo = host.move_slot_load_u32(slot, 0);
            let hi = host.move_slot_load_u32(slot, 4);
            state.world_x = (lo & 0xFFFF) as i16;
            state.world_y = ((lo >> 16) & 0xFFFF) as i16;
            state.world_z = (hi & 0xFFFF) as i16;
            state.world_y_mirror = ((hi >> 16) & 0xFFFF) as i16;
            MoveExtResult::default_arm()
        }

        // 0x27 - save the three tween-source u16s `actor[+0x90..+0x96]` into
        // the slot's first 6 bytes (slot[0/2/4] = tween_src_x/y/z).
        0x27 => {
            let slot = op_w(2);
            host.move_slot_save_u16(slot, 0, state.tween_src_x as u16);
            host.move_slot_save_u16(slot, 2, state.tween_src_y as u16);
            host.move_slot_save_u16(slot, 4, state.tween_src_z as u16);
            MoveExtResult::default_arm()
        }

        // 0x28 - load 3 × u16 from the slot, scale `+0x92/+0x94` by
        // `op_w(3)/op_w(4)` (with `>> 12` fixed-point shift), and clamp the
        // scaled outputs to `[-0xFF, 0xFF]`. Returns size 5 when the third
        // result needs the upper-bound branch (mirroring the original
        // `iVar16 = 5` shortcut), default-arm size otherwise.
        //
        // The upper-z bound check has to stay as a separate `if` (rather
        // than a `clamp` call) because the dump's "did we clamp z to the
        // upper bound" flag is what selects the return size.
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
            let upper_bound_branch = z_scaled > 0xFF;
            if upper_bound_branch {
                z_scaled = 0xFF;
            }
            state.tween_src_y = y_scaled;
            state.tween_src_z = z_scaled;
            if upper_bound_branch {
                MoveExtResult::default_arm()
            } else {
                MoveExtResult::with_size(5)
            }
        }

        // 0x31 - save `actor[+0x24..+0x2C]` (the three render banks +
        // `+0x2A` Y mirror) into the slot.
        0x31 => {
            let slot = op_w(2);
            let lo = (state.render_24 as u16 as u32) | ((state.render_26 as u16 as u32) << 16);
            let hi = (state.render_28 as u16 as u32) | ((state.world_y_mirror as u16 as u32) << 16);
            host.move_slot_save_u32(slot, 0, lo);
            host.move_slot_save_u32(slot, 4, hi);
            MoveExtResult::default_arm()
        }

        // 0x32 - load 8 bytes from the slot back into the render-bank
        // section at `+0x24..+0x2C`.
        0x32 => {
            let slot = op_w(2);
            let lo = host.move_slot_load_u32(slot, 0);
            let hi = host.move_slot_load_u32(slot, 4);
            state.render_24 = (lo & 0xFFFF) as i16;
            state.render_26 = ((lo >> 16) & 0xFFFF) as i16;
            state.render_28 = (hi & 0xFFFF) as i16;
            state.world_y_mirror = ((hi >> 16) & 0xFFFF) as i16;
            MoveExtResult::default_arm()
        }

        // 0x34 - save `actor[+0x72]` (`field_72`) into slot[0..2].
        0x34 => {
            let slot = op_w(2);
            host.move_slot_save_u16(slot, 0, state.field_72);
            MoveExtResult::default_arm()
        }

        // 0x35 - load slot[0..2] into `actor[+0x72]`.
        0x35 => {
            let slot = op_w(2);
            state.field_72 = host.move_slot_load_u16(slot, 0);
            MoveExtResult::default_arm()
        }

        // 0x29 - scratchpad ramp or immediate write. op_w(2)=slot,
        // op_w(3)=target, op_w(4)=ticks.
        0x29 => {
            let slot = op_w(2) as i16;
            let target = op_w(3) as i16;
            let ticks = op_w(4) as i16;
            if ticks != 0 {
                host.ext_scratchpad_ramp(slot, -target, ticks);
            } else {
                host.ext_scratchpad_write(slot, -target);
            }
            MoveExtResult::default_arm()
        }

        // 0x2B - `actor[+0xB4..+0xBC] = op_w(2..6)`. Writes 4 u16 anim-block
        // slots (`anim_block_u16` at byte-off 8/10/12/14 = `+0xB4/B6/B8/BA`).
        // Per the dump, the dispatcher's default-arm closes the case after
        // the writes; engines treating the sub-op as a 5-u16 instruction
        // can override `ext_dispatch` if they need accurate PC math.
        0x2B => {
            state.anim_block_u16_set(8, op_w(2));
            state.anim_block_u16_set(10, op_w(3));
            state.anim_block_u16_set(12, op_w(4));
            state.anim_block_u16_set(14, op_w(5));
            MoveExtResult::default_arm()
        }

        // 0x2C - overlay sub-routine.
        0x2C => {
            host.ext_func801d31b0(state, operand);
            MoveExtResult::default_arm()
        }

        // 0x2D - additive variant of 0x2B.
        // `actor[+0xB4..+0xBC] += op_w(2..6)`. Wrapping add per the
        // `*(short *)` semantics in the original.
        0x2D => {
            for (slot, idx) in [(8, 2), (10, 3), (12, 4), (14, 5)] {
                let cur = state.anim_block_u16(slot);
                let add = op_w(idx);
                state.anim_block_u16_set(slot, cur.wrapping_add(add));
            }
            MoveExtResult::default_arm()
        }

        // 0x2E - emit OT packet.
        0x2E => {
            host.ext_emit_ot_packet(operand);
            MoveExtResult::with_size(2)
        }

        // 0x2F - write `_DAT_8007B9D8`. op_w(2) = the i16 value.
        0x2F => {
            host.ext_set_8007b9d8(op_w(2) as i16 as i32);
            MoveExtResult::default_arm()
        }

        // 0x30 - opaque func56798 (same as 0x05).
        0x30 => host.ext_func56798(state),

        // 0x33 - `actor[+0xC0..+0xC8] += op_w(2..6)` (4 i16 anim-block slots
        // at byte-off 20/22/24/26 = `+0xC0/C2/C4/C6`). Wrapping add per the
        // `*(short *) +` semantics in the original.
        0x33 => {
            for (slot, idx) in [(20, 2), (22, 3), (24, 4), (26, 5)] {
                let cur = state.anim_block_u16(slot);
                let add = op_w(idx);
                state.anim_block_u16_set(slot, cur.wrapping_add(add));
            }
            MoveExtResult::default_arm()
        }

        // 0x36 / 0x37 - axis threshold against `0x8E - DAT_8007C348`.
        // 0x38 / 0x39 - squared-distance to the player.
        // All four predicates funnel through `LAB_801D4830`: returns 1 when
        // the predicate is true, else 4 (skip a 3-u16 follow-up payload).
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
                MoveExtResult::default_arm()
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
        0x3A => {
            let angle = host.ext_compute_angle(state);
            let dst = state.pc as usize + op_w(2) as usize + 3;
            host.move_bytecode_write_u16(dst, angle);
            MoveExtResult::default_arm()
        }

        // 0x3B - party-member position lookup. Original:
        //   puVar15 = (short*)(param_2 + op_w(3)*2 + 8);
        //   *puVar15 = puVar15[1] = puVar15[2] = 0;
        //   func_0x8003D064(_DAT_8007B898 + 0x22, &local, ...);
        //   actor = func_0x8003C83C(local + op_w(2) + 1);
        //   if (actor) { puVar15[0..2] = actor.world; default_arm; }
        //   else { iVar16 = 4; goto skip; }
        //
        // dst slot = `op_w(3) + 4` in u16 units off pc. Pre-clear the 3
        // slots before the lookup so a host that returns `None` still has
        // the zero-pre-clear behavior the original guarantees.
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
                    MoveExtResult::default_arm()
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

        // Anything `>= 0x3D` is reserved / unknown - treat as default arm
        // since the original switch had no entries past 0x3C and would land
        // in `default:` (size 1 + iVar16 << 16, treated as fall-through here).
        // FUN_801D362C guards the JT jump with `sltiu sub_op, 0x3D` (the
        // sub-opcode is loaded `lh` = sign-extended, so the *unsigned* compare
        // also rejects negative values), branching to its return on
        // out-of-range - so the extension dispatch has no OOB-jump path; this
        // catch-all is the faithful mirror of that guarded return.
        _ => MoveExtResult::default_arm(),
    }
}

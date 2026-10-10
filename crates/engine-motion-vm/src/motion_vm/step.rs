//! `FUN_8003774C` itself: the post-touch gate, the compass walk and the per-actor step.

use super::*;

/// The "no move installed" sentinel in byte 0 of an actor's
/// `DAT_801C6470[+0x50]` record, and the value that suppresses a touch post.
///
/// It is not a class. The motion VM's prologue stamps it into bytes 0 and 1
/// of the record (`li v1,0x8c; sb v1,0x0(v0); sb v1,0x1(v0)` at
/// `0x800382C8..0x800382D0`), its arms restamp the record from byte 0 or 1
/// whenever byte 0 is not `0x8C`, and `FUN_8003D038` drops the touch post
/// for a record holding it. Read as an unsigned byte (`lbu` against the
/// immediate `0x8C`); the Ghidra C renders the comparison as the signed
/// `-0x74`, the same bit pattern.
pub const TOUCH_POST_SUPPRESS_CLASS: u8 = 0x8C;

/// Stride of the bind-record table at `DAT_801C6470`, in bytes. The byte the
/// filter tests is the record's first byte.
pub const BIND_RECORD_STRIDE: usize = 4;

/// Post a collision touch to the motion VM's pending-touch slot - port of
/// `FUN_8003D038`.
///
/// The field collision probe (`FUN_801CFC40`) calls this with the touched
/// actor's bind-record index (`other[+0x50]`) whenever two actors overlap.
/// The retail body is a single guarded store into `DAT_80073F1C`, the
/// one-slot mailbox the motion VM's wait-for-touch opcode
/// (`0x8003882C`, inside `FUN_80038158`) consumes and resets.
///
/// The guard reads byte 0 of `bind_records[index]` and drops the post when
/// it is [`TOUCH_POST_SUPPRESS_CLASS`], the motion VM's "no move installed"
/// sentinel - so an actor whose record carries no installed move can be
/// walked into without ever waking a script waiting on a touch. Returns the value to store, or `None` when the post is
/// suppressed and the previous mailbox contents must be left alone.
///
/// Retail does **no** bounds check on `index`; an out-of-range index reads
/// whatever follows the table. This port returns `None` instead, which is
/// the safe reading of the same "don't post" outcome.
///
/// `bind_records` is the `DAT_801C6470` table, one [`BIND_RECORD_STRIDE`]
/// byte record per entry.
///
// The mailbox has both ends. `World::post_ambient_motion_touch` calls this
// from the locomotion step's contact check, and the consumer is the head of
// the ambient VM's `0x05` wait arm - `crate::ambient_motion`'s
// `AmbientMotion::take_touch_wake`, the port of `0x8003882C..0x8003887C`.
// So an NPC parked in an authored wait breaks out of it when the player
// walks into its box, which is what the arm is for.
//
// The earlier reason here claimed the whole arm was unported and put the
// engine's slice of `FUN_80038158` at "ops 0x04 / 0x0D plus the static MAN
// decode". Both halves were wrong: `crate::ambient_motion` runs ops `0x01`,
// `0x03`, `0x04`, `0x05`, `0x0D`, `0x17`, `0x18`, `0x19` and `0x20`, and op
// `0x05` *is* that arm - what was missing was only its mailbox head, which
// is four instructions of gating ahead of the countdown the port already had.
//
// PORT: FUN_8003d038
// REF: FUN_801cfc40 (the collision probe that posts), FUN_80038158
//      (the wait-for-touch consumer at 0x8003882C)
pub fn post_touch(bind_records: &[u8], index: usize) -> Option<u32> {
    let class = *bind_records.get(index.checked_mul(BIND_RECORD_STRIDE)?)?;
    if class == TOUCH_POST_SUPPRESS_CLASS {
        return None;
    }
    Some(index as u32)
}

/// Axis-sign bits per compass index, the 8-byte table at `0x80073F14` the
/// compass walk indexes with `body0 & 7`: bit 0 = `+Z`, bit 1 = `-Z`,
/// bit 2 = `+X`, bit 3 = `-X`. Index 0 is `-Z` and the index turns
/// clockwise through `-X`, `+Z`, `+X`.
pub const COMPASS_AXIS_SIGNS: [u8; 8] = [0x2, 0xA, 0x8, 0x9, 0x1, 0x5, 0x4, 0x6];

/// One tick of the compass walk shared by ops `0x37` / `0x41` (the arm at
/// `0x8003789C..0x800379F8`). Returns `true` when the leg completes.
///
/// `body0 & 7` picks the direction ([`COMPASS_AXIS_SIGNS`]);
/// `sel = ((body0 >> 5) & 4) | (body1 >> 6)` picks a divisor `div = 4 << sel`,
/// and the leg lasts `(body1 & 0x3F) * div` speed units. Each tick spends
/// `s = min(speed, remaining)` of them (`remaining` measured off `+0x54`,
/// read signed) and moves every flagged axis by `rate * s / div` (a signed
/// divide; `rate` is `0x80` for `0x37`, `0x40` for `0x41`), with 16-bit
/// wrapping stores to `+0x14` / `+0x18`. The leg ends when the signed
/// `+0x54` reaches the total; the shared epilogue then zeroes `+0x54`.
///
/// PORT: FUN_8003774C (the `0x37` / `0x41` arm)
pub fn compass_walk(state: &mut MotionState, body0: u8, body1: u8, rate: i32) -> bool {
    let sel = ((body0 >> 5) & 4) | (body1 >> 6);
    let div = 4i32 << sel;
    let total = i32::from(body1 & 0x3F) * div;
    let mut spend = state.speed as i32;
    let remaining = total - i32::from(state.op_accum as i16);
    if remaining < spend {
        spend = remaining;
    }
    let dir = COMPASS_AXIS_SIGNS[usize::from(body0 & 7)];
    let step = ((rate * spend) / div) as i16;
    if dir & 1 != 0 {
        state.world_z = state.world_z.wrapping_add(step);
    }
    if dir & 2 != 0 {
        state.world_z = state.world_z.wrapping_sub(step);
    }
    if dir & 4 != 0 {
        state.world_x = state.world_x.wrapping_add(step);
    }
    if dir & 8 != 0 {
        state.world_x = state.world_x.wrapping_sub(step);
    }
    state.op_accum = state.op_accum.wrapping_add(spend as u16);
    i32::from(state.op_accum as i16) >= total
}

/// One per-frame step over the script. Reads the opcode at `state.pc`,
/// dispatches, advances PC if the op is byte-tagged `0x80` (consume the
/// target byte too), mutates `state` per the body. The caller wires the
/// `target` from its own actor list.
///
/// This is a port of the dispatcher's outer switch, verified
/// against the Ghidra decompilation at `ghidra/scripts/funcs/8003774c.txt`.
/// All six opcodes are implemented and covered by unit tests including full
/// patrol-leg sequences (move + face-target in order).
pub fn step(state: &mut MotionState, target: MotionTarget, bytecode: &[u8]) -> StepResult {
    state.yaw_written = false;
    let pc = state.pc as usize;
    if pc >= bytecode.len() {
        return StepResult::Done;
    }
    let op_byte = bytecode[pc];
    let mut body_off = pc + 1;
    if op_byte & 0x80 != 0 {
        // Target-select: skip the target id byte.
        body_off += 1;
    }
    let Some(op) = MotionOp::from_byte(op_byte) else {
        return StepResult::Done;
    };
    // Compute displacements in i32: an i16 displacement can reach `i16::MIN`
    // (e.g. target `i16::MIN`, world `> 0`), and `i16::MIN.abs()` overflow-panics
    // in debug; widening also stops a `speed > 0x7FFF` from flipping the step's
    // sign when cast to i16 (it is a u16, never negative).
    let speed = state.speed as i32;
    // Retail keeps PC on the active opcode while the per-frame budget is
    // partially consumed (`Yield`); only `Done` / terminal arms move PC
    // past the body. Engines reset PC themselves when starting a new
    // script.
    match op {
        MotionOp::CompassWalkFast | MotionOp::CompassWalkSlow => {
            let (Some(&b0), Some(&b1)) = (bytecode.get(body_off), bytecode.get(body_off + 1))
            else {
                return StepResult::Done;
            };
            let rate = if op == MotionOp::CompassWalkFast {
                0x80
            } else {
                0x40
            };
            if compass_walk(state, b0, b1, rate) {
                state.op_accum = 0;
                state.pc = (body_off + 2) as u16;
                StepResult::Done
            } else {
                StepResult::Yield
            }
        }
        MotionOp::Hold => StepResult::Yield,
        MotionOp::MoveTowardTarget => {
            let cur_x = state.world_x as i32;
            let cur_z = state.world_z as i32;
            let dx = target.x as i32 - cur_x;
            let dz = target.z as i32 - cur_z;
            // Retail's axis mask (`0x80037C18`): bit 0 = X still to close,
            // bit 1 = Z still to close.
            let mut mask = u8::from(dx != 0) | (u8::from(dz != 0) << 1);
            if mask == 0 {
                state.walk_facing = None;
                state.pc = body_off as u16;
                return StepResult::Done;
            }
            let mut step = speed;
            if mask == 3 && state.approach != 0 {
                // A non-zero approach mode masks the open axes at the full
                // step (`0x80037C34..0x80037C48`): mode `3` cuts the diagonal
                // from the first frame, so a leg longer on one axis ends on
                // that axis alone - and faces along it.
                mask &= state.approach;
            } else if mask == 3 {
                // Retail's default approach mode (`0x80037C4C..0x80037C98`):
                // an actor with both axes open walks the **dominant axis
                // alone**, clamped to the difference, until the two remaining
                // deltas are equal - only then does it cut the diagonal. That
                // sequencing is what makes a walking NPC face a cardinal
                // direction for most of a leg instead of a diagonal from
                // frame one.
                let (ax, az) = (dx.abs(), dz.abs());
                if ax != az {
                    mask = if az < ax { 1 } else { 2 };
                    step = step.min(ax.abs_diff(az) as i32);
                }
            }
            let step_x = if mask & 1 != 0 {
                dx.signum() * step.min(dx.abs())
            } else {
                0
            };
            let step_z = if mask & 2 != 0 {
                dz.signum() * step.min(dz.abs())
            } else {
                0
            };
            state.world_x = (cur_x + step_x) as i16;
            state.world_z = (cur_z + step_z) as i16;
            // Walk-direction-implied facing: the `0x47` tail (`0x80037D4C`)
            // snaps `+0x26` to the 8-way compass from the step's axis signs.
            // The heading is written **once per leg** at the first moving
            // frame and again only when the step direction changes (the
            // dominant-axis → diagonal cut) - runtime-pinned on the Mei
            // dinner walk-on, where every leg holds a single heading for its
            // whole run. Re-writing the same compass value every moving
            // frame is indistinguishable for the walk itself, but would
            // clobber an interleaved writer (an interact-bearing pose), so
            // the port writes on change only.
            if let Some(idx) = walk_facing_index(step_x, step_z)
                && state.walk_facing != Some(idx)
            {
                state.walk_facing = Some(idx);
                if let Some(yaw) = heading_lut_engine(idx) {
                    state.yaw = yaw;
                    state.yaw_written = true;
                }
            }
            if state.world_x == target.x && state.world_z == target.z {
                state.walk_facing = None;
                state.pc = body_off as u16;
                StepResult::Done
            } else {
                StepResult::Yield
            }
        }
        MotionOp::RotateToAngle => {
            if body_off + 2 > bytecode.len() {
                return StepResult::Done;
            }
            let body0 = bytecode[body_off];
            let body1 = bytecode[body_off + 1];
            // Retail masks the LUT index `& 0xF`; indices 8..=15 read past the
            // eight direction entries into unrelated SCUS data, so this port
            // treats them as no-ops rather than reproducing the overread.
            let Some(target_yaw) = heading_lut_engine(body0 & 0x0F) else {
                state.pc = (body_off + 2) as u16;
                return StepResult::Done;
            };
            let total_budget = (body1 & 0x7f) as u16;
            let remaining = total_budget.saturating_sub(state.op_accum);
            if remaining <= state.speed {
                // Terminal frame: exact snap onto the compass entry - the
                // only write of the ramp guaranteed to land in 0..0xFFF.
                state.yaw = target_yaw;
                state.yaw_written = true;
                state.op_accum = 0;
                state.pc = (body_off + 2) as u16;
                StepResult::Done
            } else {
                state.op_accum += state.speed;
                // Direction: `body1 & 0x80` forces one, unless `body0 & 0x80`
                // opts into shortest-path, which decreases when the
                // increasing arc would exceed a half-turn.
                let decreasing = if body0 & 0x80 != 0 {
                    let arc =
                        (i32::from(target_yaw) - i32::from(state.yaw & 0x0FFF)).rem_euclid(0x1000);
                    arc > 0x800
                } else {
                    body1 & 0x80 != 0
                };
                state.yaw = rotate_step(
                    state.yaw,
                    target_yaw,
                    decreasing,
                    u32::from(state.speed),
                    u32::from(remaining),
                );
                state.yaw_written = true;
                StepResult::Yield
            }
        }
        MotionOp::FaceTarget => {
            if body_off + 4 > bytecode.len() {
                return StepResult::Done;
            }
            let body0 = bytecode[body_off];
            let body1 = bytecode[body_off + 1];
            let body2 = bytecode[body_off + 2];
            let _body3 = bytecode[body_off + 3];
            if body0 != 0x85 && body0 != 0x8e && body0 != 0x8f {
                state.pc = (body_off + 4) as u16;
                return StepResult::Done;
            }
            let total_budget = (body1 as u16) | ((body2 as u16) << 8);
            let remaining = total_budget.saturating_sub(state.op_accum);
            let dx = target.x as i32 - state.world_x as i32;
            let dz = target.z as i32 - state.world_z as i32;
            let target_yaw = bearing_to_yaw(dx, dz);
            if remaining <= state.speed {
                // Terminal snap onto the live arctan bearing - unlike the
                // 0x38 compass snap this endpoint is NOT compass-aligned
                // (the interact face-the-player pose lands on e.g. 1075).
                state.yaw = target_yaw;
                state.yaw_written = true;
                state.op_accum = 0;
                state.pc = (body_off + 4) as u16;
                StepResult::Done
            } else {
                state.op_accum += state.speed;
                let current_yaw = i32::from(state.yaw) & 0x0FFF;
                let tgt = i32::from(target_yaw) & 0x0FFF;
                // Shortest arc, always - sub-mode `0x8F` is the one that
                // overrides it and forces the decreasing direction.
                let decreasing = (current_yaw - tgt).rem_euclid(0x1000)
                    < (tgt - current_yaw).rem_euclid(0x1000)
                    || body0 == 0x8f;
                state.yaw = rotate_step(
                    state.yaw,
                    target_yaw,
                    decreasing,
                    u32::from(state.speed),
                    u32::from(remaining),
                );
                state.yaw_written = true;
                StepResult::Yield
            }
        }
    }
}

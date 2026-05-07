//! Per-actor "third motion" VM, ported clean-room from `FUN_8003774C`
//! (SCUS_942.54). Distinct from the actor / sprite VM in [`super`] and the
//! move-table VM in [`super::move_vm`]:
//!
//! - The actor VM (FUN_801D6628) handles sprite spawn / despawn, bytecode-driven.
//! - The move VM (FUN_80023070) drives Tactical Arts / battle-action animation.
//! - The motion VM here drives **per-actor pursue / patrol / face-target** logic
//!   used for NPC movement on the field, camera follow paths, and "face the
//!   speaker" cinematic posing during dialog.
//!
//! ## Bytecode layout
//!
//! Each script entry is 1 + N bytes:
//!
//! ```text
//!   +0  u8 op_byte         ; bit 0x7F = opcode, bit 0x80 = "select target"
//!   +1  u8 target_id       ; only present if bit 0x80 set in op_byte;
//!                          ;   special ids: 0xF8 (self), 0xFB (linked)
//!   +N  u8 operand[...]    ; opcode-specific operands
//! ```
//!
//! When the high bit is set, the VM resolves a target actor before applying
//! the body. `0xF8` resolves to "this actor" (the retail engine reads
//! `_DAT_8007c364` — current player ptr), `0xFB` follows a linked list at
//! `_DAT_8007c34c` looking for a matching record-class signature, and any
//! other id linearly scans the actor list at `_DAT_8007c354` matching against
//! the actor's id field at `+0x14`.
//!
//! ## Opcodes implemented
//!
//! | byte | retail addr | name             | semantics                                |
//! |------|-------------|------------------|------------------------------------------|
//! | 0x37 | 80037894    | TranslateY       | accumulate Y axis by per-frame speed     |
//! | 0x41 | 80037894    | TranslateX       | accumulate X axis by per-frame speed     |
//! | 0x43 | 80037f5c    | NoOp             | tick budget consumed, no actor mutation  |
//! | 0x47 | 80037ba8    | MoveTowardTarget | step actor XZ toward `(tx, tz)`          |
//! |      |             | (default arm)    | terminate with `done=true`               |
//!
//! Opcodes `0x38` (RotateToAngle) and `0x4C` (FaceTarget) implement 12-bit
//! fixed-point yaw with shortest-path quadrant logic. See the `step`
//! implementation and the `ANGLE_TABLE` constant for details.
//!
//! ## Clean-room boundary
//!
//! No bytes from `SCUS_942.54` live in this crate. The Ghidra decompilation at
//! `ghidra/scripts/funcs/8003774c.txt` is the *spec*. Tests use synthetic
//! bytecode.

/// Per-actor target the VM steps toward (when the bytecode's "select target"
/// bit is set). The retail engine resolves this through engine-side actor
/// lists; the VM just receives an `(x, y, z)` triple.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MotionTarget {
    pub x: i16,
    pub y: i16,
    pub z: i16,
    /// Actor's record-id field at retail `+0x14`. Used for linear search of
    /// the linked-actor list. Not consumed by the per-frame math.
    pub id: u16,
}

/// Per-actor motion-VM state, tracking the bytecode pointer and the per-frame
/// speed scalar (retail `_DAT_1f800393`).
#[derive(Debug, Clone, Default)]
pub struct MotionState {
    /// World coords of the actor whose script is being driven. Mutated by
    /// the VM per opcode.
    pub world_x: i16,
    pub world_y: i16,
    pub world_z: i16,
    /// Per-frame speed scalar (retail `_DAT_1f800393`). Engines update once
    /// per frame; the VM consumes it as the budget for incremental motion.
    pub speed: u16,
    /// Yaw / Y rotation in 12-bit fixed-point (units of `0x1000` = full turn).
    /// Mutated by op 0x4C `FaceTarget` and consumed by the renderer.
    pub yaw: u16,
    /// Per-script accumulator at retail `actor[0x15]` — number of speed
    /// units already consumed for the current opcode body.
    pub op_accum: u16,
    /// Bytecode-buffer cursor at retail `actor[0x25]` (byte offset).
    pub pc: u16,
}

/// Opcode tag for the motion VM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MotionOp {
    /// `0x37` — translate along Y axis at per-frame speed.
    TranslateY = 0x37,
    /// `0x38` — rotate yaw toward absolute angle (not yet implemented).
    RotateToAngle = 0x38,
    /// `0x41` — translate along X axis at per-frame speed.
    TranslateX = 0x41,
    /// `0x43` — no-op (tick consumed, no mutation).
    NoOp = 0x43,
    /// `0x47` — move actor's (X, Z) toward the target's (X, Z). Used by NPC
    /// pursue / camera-follow scripts.
    MoveTowardTarget = 0x47,
    /// `0x4C` — face the target (yaw rotates to bearing). Sub-modes
    /// 0x85 / 0x8E / 0x8F gate which component is rotated.
    FaceTarget = 0x4C,
}

impl MotionOp {
    pub fn from_byte(b: u8) -> Option<Self> {
        Some(match b & 0x7F {
            0x37 => Self::TranslateY,
            0x38 => Self::RotateToAngle,
            0x41 => Self::TranslateX,
            0x43 => Self::NoOp,
            0x47 => Self::MoveTowardTarget,
            0x4C => Self::FaceTarget,
            _ => return None,
        })
    }
}

/// One step's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepResult {
    /// Script consumed all the per-frame speed budget; resume next tick.
    Yield,
    /// Script reached a terminal opcode (default arm or 0x43 NoOp explicit
    /// done flag); engines clear the bytecode cursor.
    Done,
}

/// 16-entry angle lookup table at retail `DAT_80073f04`. Each entry is a
/// 12-bit yaw value (0x000..0xFFF = 0°..360°), evenly spaced at 22.5°
/// increments. Common angles used by NPC patrol scripts and camera paths.
const ANGLE_TABLE: [u16; 16] = [
    0x000, 0x100, 0x200, 0x300, 0x400, 0x500, 0x600, 0x700, 0x800, 0x900, 0xA00, 0xB00, 0xC00,
    0xD00, 0xE00, 0xF00,
];

/// Convert a 2D displacement `(dx, dz)` to a 12-bit fixed-point yaw
/// (0x000..0xFFF, clockwise, 0x000 = +Z). Retail calls `FUN_80019b28`.
fn bearing_to_yaw(dx: i32, dz: i32) -> u16 {
    if dx == 0 && dz == 0 {
        return 0;
    }
    let radians = (dx as f32).atan2(dz as f32);
    let raw = (radians * (0x1000 as f32) / (2.0 * std::f32::consts::PI)).round() as i32;
    // Normalize to 12-bit [0, 0x1000).
    ((raw % 0x1000 + 0x1000) % 0x1000) as u16
}

/// One per-frame step over the script. Reads the opcode at `state.pc`,
/// dispatches, advances PC if the op is byte-tagged `0x80` (consume the
/// target byte too), mutates `state` per the body. The caller wires the
/// `target` from its own actor list.
///
/// This is a thin port of the dispatcher's outer switch — full semantics of
/// the angle ops are TBD but the position-update arms are faithful.
pub fn step(state: &mut MotionState, target: MotionTarget, bytecode: &[u8]) -> StepResult {
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
    let speed = state.speed as i16;
    // Retail keeps PC on the active opcode while the per-frame budget is
    // partially consumed (`Yield`); only `Done` / terminal arms move PC
    // past the body. Engines reset PC themselves when starting a new
    // script.
    match op {
        MotionOp::TranslateY => {
            let dy = target.y.saturating_sub(state.world_y);
            let step = dy.signum() * speed.min(dy.abs());
            state.world_y = state.world_y.wrapping_add(step);
            if state.world_y == target.y {
                state.pc = body_off as u16;
                StepResult::Done
            } else {
                StepResult::Yield
            }
        }
        MotionOp::TranslateX => {
            let dx = target.x.saturating_sub(state.world_x);
            let step = dx.signum() * speed.min(dx.abs());
            state.world_x = state.world_x.wrapping_add(step);
            if state.world_x == target.x {
                state.pc = body_off as u16;
                StepResult::Done
            } else {
                StepResult::Yield
            }
        }
        MotionOp::NoOp => {
            state.pc = body_off as u16;
            StepResult::Yield
        }
        MotionOp::MoveTowardTarget => {
            // Retail computes Manhattan-distance ratios + dispatches between
            // (X-dominant, Z-dominant, both) but the net effect is "move
            // both axes by `speed` clamped at the target".
            let dx = target.x.saturating_sub(state.world_x);
            let dz = target.z.saturating_sub(state.world_z);
            let step_x = dx.signum() * speed.min(dx.abs());
            let step_z = dz.signum() * speed.min(dz.abs());
            state.world_x = state.world_x.wrapping_add(step_x);
            state.world_z = state.world_z.wrapping_add(step_z);
            if state.world_x == target.x && state.world_z == target.z {
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
            let total_budget = (body1 & 0x7f) as u16;
            let remaining = total_budget.saturating_sub(state.op_accum);
            if remaining <= state.speed {
                let angle_idx = (body0 & 0x0f) as usize;
                state.yaw = ANGLE_TABLE[angle_idx];
                state.op_accum = 0;
                state.pc = (body_off + 2) as u16;
                StepResult::Done
            } else {
                state.op_accum += state.speed;
                let angle_idx = (body0 & 0x0f) as usize;
                let target_yaw = ANGLE_TABLE[angle_idx] as i32;
                let current_yaw = (state.yaw & 0x0fff) as i32;
                let mut clockwise = body1 & 0x80 != 0;
                if body0 & 0x80 != 0 {
                    let mut delta = target_yaw - current_yaw;
                    if target_yaw < current_yaw {
                        delta += 0x1000;
                    }
                    clockwise = delta > 0x800;
                }
                let delta = if clockwise {
                    let mut d = current_yaw - target_yaw;
                    if current_yaw < target_yaw {
                        d += 0x1000;
                    }
                    d & 0x0fff
                } else {
                    let mut d = target_yaw - current_yaw;
                    if target_yaw < current_yaw {
                        d += 0x1000;
                    }
                    d & 0x0fff
                } as u16;
                let step = (delta * state.speed) / remaining.max(1);
                if clockwise {
                    state.yaw = state.yaw.wrapping_sub(step) & 0x0FFF;
                } else {
                    state.yaw = state.yaw.wrapping_add(step) & 0x0FFF;
                }
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
                state.yaw = target_yaw;
                state.op_accum = 0;
                state.pc = (body_off + 4) as u16;
                StepResult::Done
            } else {
                state.op_accum += state.speed;
                let current_yaw = (state.yaw & 0x0fff) as i32;
                let tgt = target_yaw as i32;
                let ccw_dist = if tgt >= current_yaw {
                    tgt - current_yaw
                } else {
                    tgt + 0x1000 - current_yaw
                } & 0x0fff;
                let cw_dist = if current_yaw >= tgt {
                    current_yaw - tgt
                } else {
                    current_yaw + 0x1000 - tgt
                } & 0x0fff;
                let clockwise = cw_dist < ccw_dist || body0 == 0x8f;
                let delta = if clockwise { cw_dist } else { ccw_dist } as u16;
                let step = (delta * state.speed) / remaining.max(1);
                if clockwise {
                    state.yaw = state.yaw.wrapping_sub(step) & 0x0FFF;
                } else {
                    state.yaw = state.yaw.wrapping_add(step) & 0x0FFF;
                }
                StepResult::Yield
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(x: i16, z: i16, speed: u16) -> MotionState {
        MotionState {
            world_x: x,
            world_y: 0,
            world_z: z,
            speed,
            yaw: 0,
            op_accum: 0,
            pc: 0,
        }
    }

    fn tgt(x: i16, y: i16, z: i16) -> MotionTarget {
        MotionTarget { x, y, z, id: 0 }
    }

    #[test]
    fn step_translate_x_walks_toward_target() {
        let mut s = st(0, 0, 4);
        let t = tgt(10, 0, 0);
        // 0x41 TranslateX with high bit (target select), target id = 0xF8 (self).
        let bc = [0x41 | 0x80, 0xF8];
        // First two steps yield (each moves 4 units; PC stays on op).
        for _ in 0..2 {
            assert_eq!(step(&mut s, t, &bc), StepResult::Yield);
        }
        assert_eq!(s.world_x, 8);
        assert_eq!(s.pc, 0, "PC should stay on op while yielding");
        // Third step moves remaining 2 units; arrives -> Done. PC moves past op.
        assert_eq!(step(&mut s, t, &bc), StepResult::Done);
        assert_eq!(s.world_x, 10);
        assert_eq!(s.pc, 2);
    }

    #[test]
    fn step_translate_y_clamps_at_target() {
        let mut s = MotionState {
            world_x: 0,
            world_y: 5,
            world_z: 0,
            speed: 100,
            yaw: 0,
            op_accum: 0,
            pc: 0,
        };
        let t = tgt(0, 7, 0);
        // 0x37 TranslateY without target byte (no high bit).
        let bc = [0x37];
        // First step: move 2 units (clamped — speed > dy).
        assert_eq!(step(&mut s, t, &bc), StepResult::Done);
        assert_eq!(s.world_y, 7);
    }

    #[test]
    fn step_move_toward_target_walks_diagonally() {
        let mut s = st(0, 0, 3);
        let t = tgt(5, 0, 5);
        let bc = [0x47];
        // First step: x += 3, z += 3 — yields, PC stays on op.
        assert_eq!(step(&mut s, t, &bc), StepResult::Yield);
        assert_eq!((s.world_x, s.world_z), (3, 3));
        assert_eq!(s.pc, 0);
        // Second step: x += 2 (clamped), z += 2 -> arrives.
        assert_eq!(step(&mut s, t, &bc), StepResult::Done);
        assert_eq!((s.world_x, s.world_z), (5, 5));
        assert_eq!(s.pc, 1);
    }

    #[test]
    fn step_no_op_consumes_tick_only() {
        let mut s = st(2, 3, 1);
        let bc = [0x43];
        assert_eq!(step(&mut s, tgt(99, 99, 99), &bc), StepResult::Yield);
        assert_eq!((s.world_x, s.world_z), (2, 3));
        assert_eq!(s.pc, 1);
    }

    #[test]
    fn step_rotate_to_angle_ccw() {
        let mut s = MotionState {
            yaw: 0x000,
            speed: 4,
            op_accum: 0,
            pc: 0,
            ..Default::default()
        };
        // body0 = 0x04 (index 4 = 0x400 = 90°), body1 = 0x10 (budget=16)
        let bc = [0x38, 0x04, 0x10];
        // Total delta = 0x400, budget = 16, speed = 4
        // First step: moves 0x400 * 4 / 16 = 0x100
        assert_eq!(step(&mut s, tgt(0, 0, 0), &bc), StepResult::Yield);
        assert_eq!(s.yaw, 0x0100);
        assert_eq!(s.op_accum, 4);
        // Second step: moves another 0x100
        assert_eq!(step(&mut s, tgt(0, 0, 0), &bc), StepResult::Yield);
        assert_eq!(s.yaw, 0x0200);
        // Third
        assert_eq!(step(&mut s, tgt(0, 0, 0), &bc), StepResult::Yield);
        assert_eq!(s.yaw, 0x0300);
        // Fourth: remaining=0, snap to target and Done
        assert_eq!(step(&mut s, tgt(0, 0, 0), &bc), StepResult::Done);
        assert_eq!(s.yaw, 0x0400);
        assert_eq!(s.op_accum, 0);
        assert_eq!(s.pc, 3);
    }

    #[test]
    fn step_rotate_to_angle_shortest_path_ccw() {
        let mut s = MotionState {
            yaw: 0x000,
            speed: 8,
            op_accum: 0,
            pc: 0,
            ..Default::default()
        };
        // target = 0x800, delta = 0x800 (ccw = cw, equal => ccw chosen).
        let bc = [0x38, 0x88, 0x10];
        assert_eq!(step(&mut s, tgt(0, 0, 0), &bc), StepResult::Yield);
        assert_eq!(s.yaw, 0x0400);
    }

    #[test]
    fn step_rotate_to_angle_shortest_path_cw() {
        let mut s = MotionState {
            yaw: 0x000,
            speed: 16,
            op_accum: 0,
            pc: 0,
            ..Default::default()
        };
        // body0 = 0x8E: auto-dir bit set, angle index = 0x0E = 14 → target = 0xE00.
        // delta = 0xE00, which is > 0x800 => clockwise.
        // CW delta = 0x000 - 0xE00, wrap => 0x200.
        // budget=32 (body1=0x20), speed=16 → remaining=32 > 16: Yield.
        // step = 0x200 * 16 / 32 = 0x100; yaw = 0x000 - 0x100 (12-bit mask) = 0xF00.
        let bc = [0x38, 0x8E, 0x20];
        assert_eq!(step(&mut s, tgt(0, 0, 0), &bc), StepResult::Yield);
        assert_eq!(s.yaw, 0x0F00);
    }

    #[test]
    fn step_face_target_computes_bearing() {
        let mut s = MotionState {
            world_x: 0,
            world_z: 0,
            yaw: 0,
            speed: 100,
            op_accum: 0,
            pc: 0,
            ..Default::default()
        };
        // 0x4C, body0=0x85, body1=0x10, body2=0x00, body3=0xF8
        // target at (100, 0, 0) → atan2(dx=100, dz=0) = π/2 = 0x400
        let bc = [0x4C, 0x85, 0x10, 0x00, 0xF8];
        let t = MotionTarget {
            x: 100,
            y: 0,
            z: 0,
            id: 0,
        };
        let r = step(&mut s, t, &bc);
        // With high speed and small budget, should complete in one step
        assert_eq!(r, StepResult::Done);
        assert_eq!(s.yaw, 0x0400);
    }

    #[test]
    fn step_face_target_invalid_submode_is_done() {
        let mut s = st(0, 0, 1);
        let bc = [0x4C, 0x00, 0x01, 0x00, 0xF8];
        assert_eq!(step(&mut s, tgt(0, 0, 0), &bc), StepResult::Done);
        assert_eq!(s.pc, 5);
    }

    #[test]
    fn step_unknown_opcode_terminates() {
        let mut s = st(0, 0, 1);
        let bc = [0x10];
        assert_eq!(step(&mut s, tgt(0, 0, 0), &bc), StepResult::Done);
    }

    #[test]
    fn step_at_buffer_end_is_immediate_done() {
        let mut s = MotionState {
            pc: 5,
            ..Default::default()
        };
        let bc = [0x47];
        assert_eq!(step(&mut s, tgt(0, 0, 0), &bc), StepResult::Done);
    }

    #[test]
    fn target_select_bit_skips_target_byte() {
        let mut s = st(0, 0, 5);
        let t = tgt(2, 0, 0);
        // 0x47 + high bit -> consumes a target byte. With speed > distance,
        // a single step lands on target -> Done -> PC moves past op + target.
        let bc = [0x47 | 0x80, 0xFB];
        assert_eq!(step(&mut s, t, &bc), StepResult::Done);
        assert_eq!(s.pc, 2, "PC should advance past op byte + target byte");
    }
}

//! Per-actor "third motion" VM, ported clean-room from `FUN_8003774C`
//! (SCUS_942.54). Distinct from the actor / sprite VM in [`super`] and the
//! move-table VM in [`super::move_vm`]:
//!
//! PORT: FUN_8003774C, FUN_80019b28
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
//! `_DAT_8007c364` - current player ptr), `0xFB` follows a linked list at
//! `_DAT_8007c34c` looking for a matching record-class signature, and any
//! other id linearly scans the actor list at `_DAT_8007c354` matching against
//! the actor's id field at `+0x14`.
//!
//! ## Opcodes implemented
//!
//! Dispatch is a 22-entry jump table at `0x80010EE0` indexed by
//! `(op & 0x7F) - 0x37`; every slot not listed below is the default arm.
//!
//! | byte | case body  | name             | semantics                                |
//! |------|------------|------------------|------------------------------------------|
//! | 0x37 | 0x8003789C | TranslateY       | accumulate Y axis by per-frame speed     |
//! | 0x38 | 0x800379FC | RotateToAngle    | ramp yaw to a compass LUT entry over a frame budget |
//! | 0x41 | 0x8003789C | TranslateX       | accumulate X axis by per-frame speed     |
//! | 0x43 | 0x80037FF0 | NoOp             | tick budget consumed, no actor mutation  |
//! | 0x47 | 0x80037B84 | MoveTowardTarget | step actor XZ toward `(tx, tz)`, snapping facing per step-direction change |
//! | 0x4C | 0x80037DE0 | FaceTarget       | ramp yaw to the target's live bearing over a frame budget |
//! |      | 0x80037FEC | (default arm)    | terminate with `done=true`               |
//!
//! ## How this VM changes an actor's facing
//!
//! Three of the six opcodes write the actor's 12-bit heading (`+0x26`), and
//! they split into **two laws**:
//!
//! - **Snap.** `0x47` `MoveTowardTarget` quantises the step direction to the
//!   eight-point compass and writes it outright ([`walk_facing_yaw`]) - once
//!   per leg at the first moving frame, and again whenever the step's axis
//!   signs change (the dominant-axis → diagonal cut). A walking actor
//!   therefore never holds an in-between angle and holds one compass heading
//!   for a whole straight leg - retail has no walk-turn interpolation
//!   (runtime-pinned per-frame on the Mei dinner walk-on: every leg holds a
//!   single heading write for its whole run).
//! - **Ramp.** `0x38` and `0x4C` interpolate toward a target angle over an
//!   explicit frame budget carried in their own operands, stepping
//!   `arc * speed / frames_remaining` per tick and snapping to the exact
//!   target on the terminal frame ([`rotate_step`]). The per-tick write-back
//!   is **raw u16 wrapping, never normalised into `0..0xFFF`** - a
//!   wrap-crossing turn holds out-of-range headings (`0xFFxx` on a
//!   decreasing ramp through zero) until the terminal snap lands in range.
//!   `0x38`'s target is a compass LUT index; `0x4C`'s is the *live* bearing
//!   to another actor, so the ramp tracks a target that is itself moving.
//!
//! Which of the two an NPC is running is a property of the bytecode, not of
//! the engine - see `docs/subsystems/field-locomotion.md` for the priority
//! order across every facing source.
//!
//! ## Clean-room boundary
//!
//! No bytes from `SCUS_942.54` live in this crate. The Ghidra decompilation at
//! `ghidra/scripts/funcs/8003774c.txt` is the *spec*. Tests use synthetic
//! bytecode.
//! REF: FUN_80019B28, FUN_80023070, FUN_801D6628

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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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
    /// Mutated by the two rotate ops and the `0x47` walk snap; consumed by
    /// the renderer **modulo `0x1000`**. Mid-ramp the value is raw u16
    /// arithmetic - a wrap-crossing rotate leg holds values outside
    /// `0..0xFFF` (retail `+0x26` does the same; only the terminal snap
    /// lands in range), so consumers mask, this field does not.
    pub yaw: u16,
    /// Per-script accumulator at retail `actor[0x15]` - number of speed
    /// units already consumed for the current opcode body.
    pub op_accum: u16,
    /// Bytecode-buffer cursor at retail `actor[0x25]` (byte offset).
    pub pc: u16,
    /// The facing-LUT index the in-flight `0x47` leg last wrote (`None`
    /// before the leg's first moving frame; cleared when the leg completes).
    /// The walk snap writes `yaw` only when this changes - once per leg for
    /// a straight leg, plus once at the dominant-axis → diagonal cut.
    pub walk_facing: Option<u8>,
    /// `true` when the step that just ran wrote `yaw` (a walk-snap write, a
    /// rotate-ramp tick, or a terminal snap). Engines that mirror the VM yaw
    /// into a render-heading store gate the copy on this, so a heading some
    /// *other* writer set (the interact arctan bearing, a scripted pose) is
    /// not clobbered by an idle or unmoved leg's stale VM yaw.
    pub yaw_written: bool,
}

/// Opcode tag for the motion VM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MotionOp {
    /// `0x37` - translate along Y axis at per-frame speed.
    TranslateY = 0x37,
    /// `0x38` - ramp yaw toward an absolute compass angle (an index into the
    /// eight-entry heading LUT, [`heading_lut_engine`]) over a frame budget,
    /// shortest-path (`body0 & 0x80`) or forced-direction (`body1 & 0x80`).
    RotateToAngle = 0x38,
    /// `0x41` - translate along X axis at per-frame speed.
    TranslateX = 0x41,
    /// `0x43` - no-op (tick consumed, no mutation).
    NoOp = 0x43,
    /// `0x47` - move actor's (X, Z) toward the target's (X, Z). Used by NPC
    /// pursue / camera-follow scripts.
    MoveTowardTarget = 0x47,
    /// `0x4C` - face the target: yaw ramps to the target's live bearing over
    /// the operand frame budget. Retail accepts exactly three sub-mode bytes
    /// (`0x85` / `0x8E` / `0x8F`) and takes the same arm for all three;
    /// `0x8F` additionally forces the decreasing rotation direction instead
    /// of taking the shortest arc. Any other sub-mode byte is inert.
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

/// The 8-direction heading LUT at retail `DAT_80073F04`, expressed in the
/// **engine** heading space (`0` = +Z).
///
/// Retail stores `u16 entry[i] = i * 0x200` for `i` in `0..=7` (a 45° compass
/// in the retail heading space, `0` = -Z), and the ops index it `& 0xF` - the
/// upper eight slots are unrelated SCUS data, not direction entries, so this
/// port defines only the real eight. `engine = (retail + 0x800) & 0xFFF`, the
/// half-turn the `render_26` convention differs by (pinned from the
/// locomotion's pad->facing writes, `FUN_801d01b0` body
/// `0x801d04b8..0x801d0548`).
///
/// Index → direction: `0` = -Z, `2` = -X, `4` = +Z, `6` = +X, odd entries the
/// diagonals between them.
pub fn heading_lut_engine(idx: u8) -> Option<u16> {
    (idx <= 7).then(|| ((u16::from(idx) * 0x200).wrapping_add(0x800)) & 0x0FFF)
}

/// The **walk-direction-implied facing** index retail derives from a step's
/// axis signs - the tail of the `0x47` `MoveTowardTarget` case at
/// `0x80037D4C..0x80037DDC`, which writes `actor+0x26` from
/// [`heading_lut_engine`]. The written heading holds **one value per walk
/// leg** (runtime-pinned: a leg's whole run carries a single heading), so
/// the port issues the write at the leg's first moving frame and on a
/// step-direction change only.
///
/// This is the reason a walking NPC's facing is always one of eight compass
/// points and never an arbitrary bearing: retail quantises here, it does not
/// interpolate. Returns `None` when neither axis moved (retail's `a3 == 0`
/// early-out leaves the previous facing standing).
pub fn walk_facing_index(dx: i32, dz: i32) -> Option<u8> {
    let (sx, sz) = (dx.signum(), dz.signum());
    if sx == 0 && sz == 0 {
        return None;
    }
    Some(match sx {
        0 => (2 * sz + 2) as u8,
        s if s > 0 => (6 - sz) as u8,
        _ => (sz + 2) as u8,
    })
}

/// [`walk_facing_index`] resolved through [`heading_lut_engine`] - the 12-bit
/// engine-space heading a step of `(dx, dz)` snaps an actor to.
pub fn walk_facing_yaw(dx: i32, dz: i32) -> Option<u16> {
    walk_facing_index(dx, dz).and_then(heading_lut_engine)
}

/// One frame of the shared **rotate-toward-angle** law both yaw ops run
/// (`0x38` `RotateToAngle` and `0x4C` `FaceTarget`, retail `0x800379FC` /
/// `0x80037DE0`; the ambient VM's `0x04` facing ramp at `0x800385D0` is the
/// same law with a unit-per-tick cursor).
///
/// Given the arc still to travel and the frames still budgeted, retail steps
/// `arc * speed / remaining` - a linear ease whose per-frame magnitude is
/// recomputed from the *live* heading each tick (the arc is measured modulo
/// `0x1000`, so raw out-of-range headings feed back correctly). The caller
/// handles the terminal frame, which snaps to the exact target rather than
/// stepping.
///
/// The write-back is **plain u16 wrapping arithmetic - no `& 0xFFF`
/// normalisation per tick**. Retail's `+0x26` holds the raw value mid-ramp:
/// a decreasing ramp through zero runs `0xFFxx` headings frame after frame
/// and only the terminal snap lands back in `0..0xFFF` (runtime-pinned
/// frame-exact on the town01 Mei dinner-beat rotate legs, whose per-tick
/// values this law reproduces bit-for-bit including the wrap crossings).
/// Renderers consume the heading modulo `0x1000`, so the raw hold is
/// invisible on screen - but a port that masks every tick diverges from the
/// traced `+0x26` on any ramp that crosses the wrap.
///
/// `decreasing` selects the rotation direction; the arc is measured the long
/// way round when it has to be, so a forced direction still lands.
pub fn rotate_step(current: u16, target: u16, decreasing: bool, speed: u32, remaining: u32) -> u16 {
    let (cur, tgt) = (i32::from(current) & 0xFFF, i32::from(target) & 0xFFF);
    let arc = if decreasing { cur - tgt } else { tgt - cur };
    // Retail's `+0x1000` then `& 0xFFF`: normalise a possibly-negative arc
    // into `0..0xFFF` without a branch.
    let arc = ((arc + 0x1000) & 0xFFF) as u32;
    let inc = (arc * speed / remaining.max(1)) as u16;
    if decreasing {
        current.wrapping_sub(inc)
    } else {
        current.wrapping_add(inc)
    }
}

/// Bind-record class byte that suppresses a touch post.
///
/// Read as an unsigned byte in retail (`lbu` against the immediate
/// `0x8C`); the Ghidra C renders the comparison as the signed `-0x74`,
/// which is the same bit pattern.
pub const TOUCH_POST_SUPPRESS_CLASS: u8 = 0x8C;

/// Stride of the bind-record table at `DAT_801C6470`, in bytes. The class
/// byte the filter tests is the record's first byte.
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
/// The guard reads the class byte of `bind_records[index]` and drops the
/// post when it is [`TOUCH_POST_SUPPRESS_CLASS`] - so a record of that
/// class can be walked into without ever waking a script waiting on a
/// touch. Returns the value to store, or `None` when the post is
/// suppressed and the previous mailbox contents must be left alone.
///
/// Retail does **no** bounds check on `index`; an out-of-range index reads
/// whatever follows the table. This port returns `None` instead, which is
/// the safe reading of the same "don't post" outcome.
///
/// `bind_records` is the `DAT_801C6470` table, one [`BIND_RECORD_STRIDE`]
/// byte record per entry.
///
// PORT: FUN_8003d038
// REF: FUN_801cfc40 (the collision probe that posts), FUN_80038158
//      (the wait-for-touch consumer at 0x8003882C)
// NOT WIRED: the port's field collision path does not post touches - it
// resolves per-axis walls and stops, without identifying the actor it hit.
// A wired caller would be the actor-vs-actor overlap test standing in for
// FUN_801cfc40, storing this function's result into the mailbox the motion
// VM's wait-for-touch opcode reads. Reachable only from tests.
pub fn post_touch(bind_records: &[u8], index: usize) -> Option<u32> {
    let class = *bind_records.get(index.checked_mul(BIND_RECORD_STRIDE)?)?;
    if class == TOUCH_POST_SUPPRESS_CLASS {
        return None;
    }
    Some(index as u32)
}

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
/// This is a clean-room port of the dispatcher's outer switch, verified
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
        MotionOp::TranslateY => {
            let cur = state.world_y as i32;
            let dy = target.y as i32 - cur;
            let step = dy.signum() * speed.min(dy.abs());
            // `cur + step` lies between `cur` and `target.y` (both i16), so it
            // fits i16 without wrapping.
            state.world_y = (cur + step) as i16;
            if state.world_y == target.y {
                state.pc = body_off as u16;
                StepResult::Done
            } else {
                StepResult::Yield
            }
        }
        MotionOp::TranslateX => {
            let cur = state.world_x as i32;
            let dx = target.x as i32 - cur;
            let step = dx.signum() * speed.min(dx.abs());
            state.world_x = (cur + step) as i16;
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
            if mask == 3 {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn st(x: i16, z: i16, speed: u16) -> MotionState {
        MotionState {
            world_x: x,
            world_z: z,
            speed,
            ..Default::default()
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
    fn step_translate_handles_extreme_coords_and_huge_speed() {
        let bc = [0x41 | 0x80, 0xF8]; // TranslateX, self target

        // Target at i16::MIN with the actor at a positive position: the i16
        // displacement would be i16::MIN, whose `.abs()` overflow-panics in
        // debug. The i32 path must move toward it without panicking.
        let mut s = st(100, 0, 4);
        let t = tgt(i16::MIN, 0, 0);
        assert_eq!(step(&mut s, t, &bc), StepResult::Yield);
        assert_eq!(s.world_x, 96, "moved 4 toward i16::MIN, no panic");

        // A speed > 0x7FFF would flip the step's sign if cast to i16, sending the
        // actor the WRONG way. It must still move toward the target and clamp.
        let mut s2 = st(-100, 0, 0xFFFF);
        let t2 = tgt(200, 0, 0);
        assert_eq!(step(&mut s2, t2, &bc), StepResult::Done);
        assert_eq!(
            s2.world_x, 200,
            "huge speed clamps at the target in the correct direction"
        );
    }

    #[test]
    fn step_translate_y_clamps_at_target() {
        let mut s = MotionState {
            world_y: 5,
            speed: 100,
            ..Default::default()
        };
        let t = tgt(0, 7, 0);
        // 0x37 TranslateY without target byte (no high bit).
        let bc = [0x37];
        // First step: move 2 units (clamped - speed > dy).
        assert_eq!(step(&mut s, t, &bc), StepResult::Done);
        assert_eq!(s.world_y, 7);
    }

    #[test]
    fn step_move_toward_target_walks_diagonally() {
        let mut s = st(0, 0, 3);
        let t = tgt(5, 0, 5);
        let bc = [0x47];
        // First step: x += 3, z += 3 - yields, PC stays on op.
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
        // body0 = 0x06 (LUT index 6 = +X = engine yaw 0x400),
        // body1 = 0x10 (budget = 16, force bit clear -> increasing).
        let bc = [0x38, 0x06, 0x10];
        // Total arc = 0x400, budget = 16, speed = 4. Retail recomputes the
        // step from the *live* arc each tick, so the increment stays 0x100.
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
        // LUT index 0 = -Z = engine yaw 0x800; the two arcs tie at 0x800 and
        // retail's `arc > 0x800` test breaks the tie toward increasing.
        let bc = [0x38, 0x80, 0x10];
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
        // body0 = 0x83: shortest-path bit set, LUT index 3 -> engine yaw 0xE00.
        // The increasing arc is 0xE00 > 0x800, so retail rotates decreasing
        // instead: arc = 0x200, step = 0x200 * 16 / 32 = 0x100, and the raw
        // write-back wraps below zero to 0xFF00 (== 0xF00 mod 0x1000) - the
        // pre-unwrap hold retail's `+0x26` shows on a wrap-crossing ramp.
        let bc = [0x38, 0x83, 0x20];
        assert_eq!(step(&mut s, tgt(0, 0, 0), &bc), StepResult::Yield);
        assert_eq!(s.yaw, 0xFF00);
        assert_eq!(s.yaw & 0x0FFF, 0x0F00, "renderers mask to the same angle");
    }

    /// A decreasing ramp that crosses the wrap holds **raw** u16 headings
    /// outside `0..0xFFF` on every mid-ramp tick and only the terminal snap
    /// lands back in range - the retail `+0x26` behaviour (runtime-pinned on
    /// the Mei dinner-beat rotate legs, which run `0xFFxx` values live). A
    /// per-tick `& 0xFFF` would keep the same angle mod 0x1000 but diverge
    /// from the traced raw values.
    #[test]
    fn rotate_ramp_holds_raw_headings_across_the_wrap() {
        // From yaw 0, rotate DECREASING (body1 bit 7) to LUT index 0
        // (engine 0x800), budget 12, speed 2 - the retail cadence of the
        // traced ramps. arc = (0 - 0x800) mod 0x1000 = 0x800.
        let bc = [0x38, 0x00, 0x8C];
        let mut s = MotionState {
            yaw: 0,
            speed: 2,
            ..Default::default()
        };
        let mut raws = Vec::new();
        loop {
            let r = step(&mut s, tgt(0, 0, 0), &bc);
            assert!(s.yaw_written, "every ramp tick writes the heading");
            raws.push(s.yaw);
            if r == StepResult::Done {
                break;
            }
        }
        let (last, mid) = raws.split_last().unwrap();
        assert!(!mid.is_empty(), "ramp has mid-ramp ticks");
        for &y in mid {
            assert!(
                y > 0x0FFF,
                "mid-ramp heading {y:#06X} should hold the raw wrapped value"
            );
        }
        // First tick: 0x800 * 2 / 12 = 0x155, written as 0 - 0x155 = 0xFEAB.
        assert_eq!(mid[0], 0xFEAB);
        assert_eq!(*last, 0x800, "terminal snap lands exactly on the target");
    }

    /// The walk snap is **once per leg** (plus once per step-direction
    /// change), not a rewrite on every moving frame: `yaw_written` fires on
    /// the first moving frame and on the dominant-axis → diagonal cut only,
    /// so an interleaved writer's pose survives a straight leg.
    #[test]
    fn walk_leg_writes_heading_once_per_direction() {
        let mut s = st(0, 0, 4);
        let t = tgt(100, 0, 20);
        let bc = [0x47];
        // Frame 1: leg start - the write.
        assert_eq!(step(&mut s, t, &bc), StepResult::Yield);
        assert!(s.yaw_written, "leg start writes the walk facing");
        assert_eq!(s.yaw, 0x400, "+X compass");
        // Frames 2..20: X still dominates - same direction, no rewrite.
        for i in 0..19 {
            assert_eq!(step(&mut s, t, &bc), StepResult::Yield);
            assert!(!s.yaw_written, "straight-leg frame {i} must not rewrite");
            assert_eq!(s.yaw, 0x400, "held compass heading");
        }
        // Deltas equal: the diagonal cut is a direction change - one write.
        assert_eq!(step(&mut s, t, &bc), StepResult::Yield);
        assert!(s.yaw_written, "direction change writes again");
        assert_eq!(s.yaw, 0x200, "+X +Z diagonal");
        // Next leg re-writes even in the same direction: leg state clears.
        while step(&mut s, t, &bc) != StepResult::Done {}
        assert_eq!(s.walk_facing, None, "leg completion clears the leg state");
        s.pc = 0;
        let t2 = tgt(200, 0, 120);
        assert_eq!(step(&mut s, t2, &bc), StepResult::Yield);
        assert!(s.yaw_written, "a new leg writes at its start");
    }

    /// The traced per-op ramp rates come from `arc / budget` - the law's
    /// per-tick floor-divide sequence, at the retail speed scalar 2, matches
    /// the recomp trace's increments exactly (170 170 170 171 ... for arc
    /// 0x600 over 18 frames, the Mei beat's first turn: rate 85/frame).
    #[test]
    fn rotate_ramp_is_linear_at_the_op_budget_rate() {
        // Increasing to LUT index 6 (engine 0x400 from a 0xE00 start:
        // arc (0x400 - 0xE00) mod 0x1000 = 0x600), budget 18, speed 2.
        let bc = [0x38, 0x06, 0x12];
        let mut s = MotionState {
            yaw: 0xE00,
            speed: 2,
            ..Default::default()
        };
        let mut incs = Vec::new();
        let mut prev = s.yaw;
        loop {
            let r = step(&mut s, tgt(0, 0, 0), &bc);
            if r == StepResult::Done {
                break;
            }
            incs.push(s.yaw.wrapping_sub(prev));
            prev = s.yaw;
        }
        assert_eq!(
            incs,
            vec![170, 170, 170, 171, 171, 171, 171, 171],
            "per-tick increments follow arc*speed/remaining with floor"
        );
        assert_eq!(s.yaw, 0x400, "terminal compass snap");
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

    /// Validate a complete NPC-patrol-style script: move toward a waypoint,
    /// then face it. This covers the full opcode sequence a field-scene NPC
    /// would execute during one patrol leg, verifying that:
    ///
    /// - `MoveTowardTarget` (0x47) yields while approaching and completes
    ///   when the actor reaches the waypoint.
    /// - `FaceTarget` (0x4C, sub-mode 0x85) yields while rotating and
    ///   snaps to the computed bearing when the budget runs out.
    /// - PC advances correctly through both opcodes in sequence.
    ///
    /// Algorithm verified against `FUN_8003774C` in SCUS_942.54
    /// (`ghidra/scripts/funcs/8003774c.txt`).
    #[test]
    fn patrol_leg_move_then_face_sequence() {
        // Actor starts at (0, 0). Waypoint at (6, 0). Speed 3 per frame.
        let mut s = MotionState {
            world_x: 0,
            world_z: 0,
            speed: 3,
            yaw: 0,
            op_accum: 0,
            pc: 0,
            ..Default::default()
        };
        let waypoint = tgt(6, 0, 0);

        // Script: 0x47 MoveTowardTarget (1 byte), then 0x4C FaceTarget sub-mode 0x85.
        // FaceTarget body: [body0=0x85, body1=budget_lo=8, body2=budget_hi=0, body3=0xF8].
        let bc = [
            0x47, // op: MoveTowardTarget
            0x4C, // op: FaceTarget
            0x85, // body0: sub-mode (0x85 = valid rotate yaw)
            0x08, // body1: budget low byte = 8
            0x00, // body2: budget high byte = 0
            0xF8, // body3: target id (self)
        ];

        // Frame 1: x += 3, z += 0. Still short of target (need 6). Yield.
        assert_eq!(step(&mut s, waypoint, &bc), StepResult::Yield);
        assert_eq!(s.world_x, 3);
        assert_eq!(s.pc, 0, "PC stays on op while yielding");

        // Frame 2: x += 3. Arrives at x=6. Done. PC advances to next op.
        assert_eq!(step(&mut s, waypoint, &bc), StepResult::Done);
        assert_eq!(s.world_x, 6);
        assert_eq!(s.pc, 1, "PC moved past MoveTowardTarget op");

        // Now FaceTarget: waypoint is (6, 0, 0), actor is at (6, 0, 0).
        // dx = 6 - 6 = 0, dz = 0 - 0 = 0. atan2(0, 0) = 0. target_yaw = 0.
        // Budget = 8, speed = 3. remaining = 8 - 0 = 8 > 3 → Yield.
        // Step: ccw_dist = 0, cw_dist = 0 → clockwise=false, delta=0, step=0.
        assert_eq!(step(&mut s, waypoint, &bc), StepResult::Yield);
        assert_eq!(s.pc, 1, "PC stays on FaceTarget while yielding");

        // Frame 4: op_accum = 3, remaining = 5 > 3 → Yield.
        assert_eq!(step(&mut s, waypoint, &bc), StepResult::Yield);

        // Frame 5: op_accum = 6, remaining = 2 <= 3 → Done. Snap to bearing.
        assert_eq!(step(&mut s, waypoint, &bc), StepResult::Done);
        assert_eq!(s.yaw, 0, "yaw snapped to computed bearing");
        assert_eq!(s.op_accum, 0, "accumulator reset on Done");
        assert_eq!(s.pc, 6, "PC advanced past FaceTarget body (4 bytes)");
    }

    /// The eight compass entries of the retail heading LUT `DAT_80073F04`,
    /// expressed in the engine's `0` = +Z space. Indices past the eight real
    /// direction entries have no heading.
    #[test]
    fn heading_lut_covers_the_compass_and_stops_at_eight() {
        assert_eq!(heading_lut_engine(0), Some(0x800), "index 0 = -Z");
        assert_eq!(heading_lut_engine(2), Some(0xC00), "index 2 = -X");
        assert_eq!(heading_lut_engine(4), Some(0x000), "index 4 = +Z");
        assert_eq!(heading_lut_engine(6), Some(0x400), "index 6 = +X");
        // 45 degrees per index, in order, all the way round.
        for i in 0..8u8 {
            let expect = (u16::from(i) * 0x200 + 0x800) & 0x0FFF;
            assert_eq!(heading_lut_engine(i), Some(expect), "index {i}");
        }
        for i in 8..16u8 {
            assert_eq!(heading_lut_engine(i), None, "index {i} is not a direction");
        }
    }

    /// The walk-direction-implied facing table retail derives from the step's
    /// axis signs (`0x80037D4C`). Cardinal steps land on the even indices,
    /// diagonals on the odd ones between them.
    #[test]
    fn walk_facing_index_matches_the_retail_sign_table() {
        // (dx, dz) -> LUT index.
        let cases: [(i32, i32, u8); 8] = [
            (0, -1, 0),  // -Z
            (-1, -1, 1), // -X -Z
            (-1, 0, 2),  // -X
            (-1, 1, 3),  // -X +Z
            (0, 1, 4),   // +Z
            (1, 1, 5),   // +X +Z
            (1, 0, 6),   // +X
            (1, -1, 7),  // +X -Z
        ];
        for (dx, dz, idx) in cases {
            assert_eq!(walk_facing_index(dx, dz), Some(idx), "({dx}, {dz})");
            // Magnitude never matters - only the signs.
            assert_eq!(
                walk_facing_index(dx * 97, dz * 3),
                Some(idx),
                "scaled ({dx}, {dz})"
            );
        }
        assert_eq!(
            walk_facing_index(0, 0),
            None,
            "a standing actor keeps its facing"
        );
    }

    /// A walking actor's facing is a **snap**, not a ramp: the leg's first
    /// moving frame writes one of the eight compass headings outright.
    #[test]
    fn move_toward_target_snaps_facing_to_the_compass() {
        for (tx, tz, want) in [
            (100i16, 0i16, 0x400u16), // +X
            (-100, 0, 0xC00),         // -X
            (0, 100, 0x000),          // +Z
            (0, -100, 0x800),         // -Z
        ] {
            let mut s = st(0, 0, 4);
            s.yaw = 0x111; // an angle the compass can never produce
            let bc = [0x47];
            assert_eq!(step(&mut s, tgt(tx, 0, tz), &bc), StepResult::Yield);
            assert_eq!(s.yaw, want, "walking toward ({tx}, {tz})");
        }
    }

    /// Retail closes the **dominant axis first** and only cuts the diagonal
    /// once the two remaining deltas are equal - so a walking NPC holds a
    /// cardinal facing for most of a leg and turns to a diagonal late,
    /// instead of moving (and facing) diagonally from frame one.
    #[test]
    fn move_toward_target_closes_dominant_axis_first() {
        let mut s = st(0, 0, 4);
        let t = tgt(100, 0, 20);
        let bc = [0x47];
        // 20 steps of 4 close X from 0 to 80, leaving |dx| == |dz| == 20.
        for i in 0..20 {
            assert_eq!(step(&mut s, t, &bc), StepResult::Yield);
            assert_eq!(s.world_z, 0, "Z is untouched while X dominates (step {i})");
            assert_eq!(s.yaw, 0x400, "facing +X while closing X (step {i})");
        }
        assert_eq!((s.world_x, s.world_z), (80, 0));
        // Deltas are now equal: the leg cuts the diagonal and the facing turns.
        assert_eq!(step(&mut s, t, &bc), StepResult::Yield);
        assert_eq!(
            (s.world_x, s.world_z),
            (84, 4),
            "both axes advance together"
        );
        assert_eq!(s.yaw, 0x200, "facing the +X +Z diagonal");
    }

    /// Validate that RotateToAngle reaches its target yaw monotonically and
    /// stops exactly at the table entry, regardless of budget pacing.
    #[test]
    fn rotate_to_angle_reaches_target_monotonically() {
        // LUT index 5 -> engine yaw 0x200. Budget = 20 frames, force bit
        // clear, so the ramp increases from 0x000 to 0x200 and lands exactly
        // on the table entry via the terminal-frame snap.
        let bc = [0x38, 0x05, 0x14]; // op, body0 = index 5, body1 = budget 20
        let mut s = MotionState {
            yaw: 0x000,
            speed: 4,
            op_accum: 0,
            pc: 0,
            ..Default::default()
        };
        let mut prev_yaw = s.yaw as i32;
        let mut steps = 0usize;
        loop {
            let r = step(&mut s, tgt(0, 0, 0), &bc);
            steps += 1;
            // Yaw should be moving CCW (increasing) toward 0x200.
            let cur = s.yaw as i32;
            // Allow the last snap (which sets exactly 0x200) to break monotonicity
            // only on Done.
            if r == StepResult::Yield {
                assert!(
                    cur >= prev_yaw,
                    "yaw should advance CCW on each Yield step: {} → {}",
                    prev_yaw,
                    cur
                );
            }
            prev_yaw = cur;
            if r == StepResult::Done {
                break;
            }
            assert!(steps < 50, "should converge in fewer than 50 steps");
        }
        assert_eq!(s.yaw, 0x0200, "final yaw must equal table entry");
        assert_eq!(s.op_accum, 0, "accumulator must reset on Done");
        assert_eq!(s.pc, 3, "PC advanced past 3-byte op");
    }

    /// Four bind records, stride 4. Record 2 carries the suppress class.
    fn bind_records() -> Vec<u8> {
        let mut t = vec![0u8; 4 * BIND_RECORD_STRIDE];
        t[0] = 0x01;
        t[BIND_RECORD_STRIDE] = 0x00;
        t[2 * BIND_RECORD_STRIDE] = TOUCH_POST_SUPPRESS_CLASS;
        t[3 * BIND_RECORD_STRIDE] = 0xFF;
        t
    }

    #[test]
    fn touch_post_stores_index_for_ordinary_records() {
        let t = bind_records();
        assert_eq!(post_touch(&t, 0), Some(0));
        // A zero class byte is ordinary - only 0x8C suppresses.
        assert_eq!(post_touch(&t, 1), Some(1));
        assert_eq!(post_touch(&t, 3), Some(3));
    }

    #[test]
    fn touch_post_suppressed_by_class_8c() {
        let t = bind_records();
        assert_eq!(
            post_touch(&t, 2),
            None,
            "class 0x8C must leave the mailbox untouched"
        );
    }

    #[test]
    fn touch_post_reads_class_at_record_stride() {
        // Only the first byte of each 4-byte record is the class; a 0x8C
        // sitting in a later byte of record 0 must not suppress it.
        let mut t = vec![0u8; 2 * BIND_RECORD_STRIDE];
        t[1] = TOUCH_POST_SUPPRESS_CLASS;
        t[2] = TOUCH_POST_SUPPRESS_CLASS;
        t[3] = TOUCH_POST_SUPPRESS_CLASS;
        assert_eq!(post_touch(&t, 0), Some(0));
    }

    #[test]
    fn touch_post_out_of_range_does_not_post() {
        let t = bind_records();
        assert_eq!(post_touch(&t, 4), None);
        assert_eq!(post_touch(&t, usize::MAX), None);
    }
}

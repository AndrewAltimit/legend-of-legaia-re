//! The motion target, the target selector, the per-actor motion state, the decoded op and the step result.

/// Per-actor target the VM steps toward (when the bytecode's "select target"
/// bit is set). The retail engine resolves this through engine-side actor
/// lists; the VM just receives an `(x, y, z)` triple.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MotionTarget {
    pub x: i16,
    pub y: i16,
    pub z: i16,
    /// The target actor's id, retail `+0x50` - the field the selector scan
    /// compares ([`TargetSelector`]). Not consumed by the per-frame math.
    pub id: u16,
}

/// Which actor a target-select op (`op_byte & 0x80`) runs its body on, as
/// `0x8003774C..0x80037858` decodes the byte that follows the opcode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetSelector {
    /// No select bit, or the byte equals the running actor's own `+0x50`
    /// (`beq v1,v0` at `0x80037798`): the body runs on the script's actor.
    Own,
    /// `0xF8` - the player actor `*0x8007C364`.
    Player,
    /// `0xFB` - the first node of the list at `*0x8007C34C` whose handler
    /// word `+0xC` is `0x801DA51C` (the world-map entity SM).
    WorldMapEntity,
    /// Any other id - the first node of the list at `*0x8007C354` whose
    /// `+0x50` equals it. An id nothing carries ends the call, returning 0.
    Id(u8),
}

/// Decode the target selector of the entry at `pc`
/// (`0x80037770..0x80037858`). The comparison against the running actor's
/// id comes first, so an actor naming its own id - even `0xF8` or `0xFB` -
/// runs the body on itself.
pub fn target_selector(bytecode: &[u8], pc: usize, own_id: u16) -> TargetSelector {
    let Some(&op) = bytecode.get(pc) else {
        return TargetSelector::Own;
    };
    if op & 0x80 == 0 {
        return TargetSelector::Own;
    }
    let Some(&id) = bytecode.get(pc + 1) else {
        return TargetSelector::Own;
    };
    if u16::from(id) == own_id {
        return TargetSelector::Own;
    }
    match id {
        0xF8 => TargetSelector::Player,
        0xFB => TargetSelector::WorldMapEntity,
        other => TargetSelector::Id(other),
    }
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
    /// The `0x47` leg's **approach mode**, the walk op's mode byte `>> 4`
    /// (`srl a1,a1,0x4` at `0x80037BEC`). `0` is the default law - the
    /// dominant axis alone until the two deltas are equal, then the
    /// diagonal. Any other value is a mask over the axis set while both are
    /// still open (`and a3,v1,a1` at `0x80037C48`): `3` walks the diagonal
    /// from the first frame and finishes on the longer axis alone, `1` closes
    /// X first, `2` closes Z first. Cutscene walks carry it in their mode byte
    /// (`C7 <id> <tx> <tz> 33`).
    pub approach: u8,
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
    /// `0x37` - compass walk at rate `0x80`: see [`compass_walk`]. (Formerly
    /// read as "translate Y toward the target"; the arm at `0x8003789C`
    /// never reads a target position or `+0x16`.)
    CompassWalkFast = 0x37,
    /// `0x38` - ramp yaw toward an absolute compass angle (an index into the
    /// eight-entry heading LUT, [`heading_lut_engine`]) over a frame budget,
    /// shortest-path (`body0 & 0x80`) or forced-direction (`body1 & 0x80`).
    RotateToAngle = 0x38,
    /// `0x41` - compass walk at rate `0x40` (same arm as `0x37`; the rate is
    /// picked by `op == 0x37` at `0x80037904`).
    CompassWalkSlow = 0x41,
    /// `0x43` - hold: the jump-table slot is the shared epilogue
    /// `0x80037FF0` entered with the done flag `s3 = 0`, so the op never
    /// completes and the script parks on it.
    Hold = 0x43,
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
            0x37 => Self::CompassWalkFast,
            0x38 => Self::RotateToAngle,
            0x41 => Self::CompassWalkSlow,
            0x43 => Self::Hold,
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

//! The camera pose the routine folds into, the animation push, the per-arm phase cursor and its thresholds.

use super::*;

/// The camera triple the arms build on the stack before the setter call.
///
/// Retail's frame is three halfword triples at `sp+0x10`, `sp+0x18` and
/// `sp+0x20`, passed as `a0`/`a1`/`a2` with a mode selector in `a3`. Field
/// names follow what the seeds fix; the `look_at` triple is the actor's
/// position **negated**, which is how a camera translation is expressed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CameraPose {
    /// `sp+0x10`, `sp+0x12`, `sp+0x14` - rotation triple. The middle slot seeds
    /// to the actor's negated facing angle (`-actor[+0x46]`).
    pub rot: [i16; 3],
    /// `sp+0x18`, `sp+0x1A`, `sp+0x1C` - distance triple. The second and third
    /// slots seed to [`POSE_SEED_ANGLE`].
    pub dist: [i16; 3],
    /// `sp+0x20`, `sp+0x22`, `sp+0x24` - look-at, the negated actor position
    /// (`-actor[+0x34]`, `-actor[+0x36]`, `-actor[+0x38]`).
    pub look_at: [i16; 3],
}

impl CameraPose {
    /// Seed the triple exactly as `0x801D7230..0x801D729C` does.
    pub fn seed(actor_pos: [u16; 3], actor_facing: u16) -> Self {
        Self {
            rot: [0, (actor_facing as i16).wrapping_neg(), 0],
            dist: [0, POSE_SEED_ANGLE, POSE_SEED_ANGLE],
            look_at: [
                (actor_pos[0] as i16).wrapping_neg(),
                (actor_pos[1] as i16).wrapping_neg(),
                (actor_pos[2] as i16).wrapping_neg(),
            ],
        }
    }
}

/// The animation-frame-driven push the arms share
/// (`0x801D735C..0x801D73DC` and its siblings at the `0xE0` / `0xF0`
/// thresholds).
///
/// `anim_frame` is `actor[+0x22C][+0x68]`, the live animation counter. Below
/// [`ANIM_PUSH_FLOOR`] the arm skips the push entirely; at or above it the push
/// is `(frame - 0x60) << 4`, clamped to [`ANIM_PUSH_CAP`]. Returns `None` for
/// the skip so a caller cannot confuse "no push" with "a push of zero" - retail
/// distinguishes them by branching around the pose writes.
pub const fn anim_push(anim_frame: i16) -> Option<i32> {
    if anim_frame < ANIM_PUSH_FLOOR {
        return None;
    }
    let scaled = ((anim_frame - ANIM_PUSH_BIAS) as i32) << 4;
    Some(if (scaled as u32) < ANIM_PUSH_CAP as u32 + 1 {
        scaled
    } else {
        ANIM_PUSH_CAP
    })
}

/// How the push splits across the pose in the first arm's two sub-branches
/// (`0x801D7398..0x801D73D8`).
///
/// The split is selected by whether the phase cursor `ctx[+0x26D]` is zero:
///
/// * non-zero: `rot[1] += push >> 1`, `dist[2] -= push >> 2`, `dist[0] += push`;
/// * zero: `rot[1] -= push`, `dist[2] -= push >> 1`.
///
/// `dist[1]` takes `-push` either way (the store at `0x801D7394` precedes the
/// branch). Retail's halvings are `srl`, not `sra`; the push is always
/// non-negative here (it is `(frame - 0x60) << 4` past a `frame >= 0x61` gate,
/// clamped at `0x100`), so the two agree.
pub fn apply_anim_push(pose: &mut CameraPose, phase_cursor: u8, push: i32) {
    pose.dist[1] = pose.dist[1].wrapping_sub(push as i16);
    if phase_cursor != 0 {
        pose.rot[1] = pose.rot[1].wrapping_add((push >> 1) as i16);
        pose.dist[2] = pose.dist[2].wrapping_sub((push >> 2) as i16);
        pose.dist[0] = pose.dist[0].wrapping_add(push as i16);
    } else {
        pose.rot[1] = pose.rot[1].wrapping_sub(push as i16);
        pose.dist[2] = pose.dist[2].wrapping_sub((push >> 1) as i16);
    }
}

/// How an arm addresses the per-phase halfword tracks at `0x801F4E10`.
///
/// Each arm computes one base, `0x801F4E10 + phase_cursor * 2`, and then reads
/// fixed byte offsets off it. The offsets the arms use are `0x00, 0x04, …,
/// 0x4C` - dense over twenty rows - so the table is twenty parallel halfword
/// tracks four bytes apart, each holding one value per phase.
/// [`legaia_game_tables::battle_attack_camera_table`] parses it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArmPhase {
    /// `ctx[+0x26D]` - the phase cursor.
    pub cursor: u8,
}

impl ArmPhase {
    /// Byte offset from the table base for track `track` at this phase.
    pub const fn track_offset(self, track: usize) -> usize {
        self.cursor as usize * 2 + track * 4
    }

    /// Read one track out of a parsed table, at this phase.
    ///
    /// Named `track_value` rather than `read`: the reachability analysis
    /// matches free functions and methods by NAME, so a method called `read`
    /// here picks up an edge from every `read` call in the workspace and
    /// reports this whole module as live.
    pub fn track_value(
        self,
        table: &legaia_game_tables::battle_attack_camera_table::AttackCameraTracks,
        track: usize,
    ) -> Option<i16> {
        table.track(track, self.cursor as usize)
    }
}

/// Retail's per-arm animation-frame thresholds, in the order the arms test them.
///
/// The first arm gates at `0x61`, later arms at `0xE0` and `0xF0`; each is a
/// `slti` against the same `actor[+0x22C][+0x68]` counter, so an arm's framing
/// changes only once the swing clip has run that far.
pub const ANIM_THRESHOLDS: [i16; 3] = [0x61, 0xE0, 0xF0];

// ---------------------------------------------------------------------------
// The executable arms.
// ---------------------------------------------------------------------------

/// The two conditions `FUN_801D5854`'s tail tests before it calls this
/// routine at all (`0x801D7138..0x801D7180`).
///
/// `render_mode` is `_DAT_800846C0` (the per-art camera is skipped in mode
/// `2`) and `target_slot` is the acting actor's `+0x1DD`; a slot of `8` or
/// more - retail's "no target" encoding - also skips it. Both are outside the
/// gate [`attack_camera_gate`] ports, which lives *inside* the routine.
pub const fn outer_gate(render_mode: u32, target_slot: u8) -> bool {
    render_mode != OUTER_GATE.0 && target_slot < OUTER_GATE.1
}

/// The literals [`outer_gate`] tests: the `_DAT_800846C0` value that skips the
/// per-art camera, and the exclusive bound on `actor[+0x1DD]`.
pub const OUTER_GATE: (u32, u8) = (2, 8);

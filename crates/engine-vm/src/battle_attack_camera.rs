//! Per-art attack-camera framing.
//!
//! PORT: FUN_801D71B8
//!
//! The per-frame routine that frames the camera on a party member's Arts swing.
//! It runs only while an Attack action is executing, resolves a **per-character,
//! per-art** script arm, and hands a rotation / translation / look-at triple to
//! the camera setter. One of the hottest bodies in the attack chain - see
//! [`docs/reference/functions/battle.md`](../../docs/reference/functions/battle.md).
//!
//! Transcribed from the DISASSEMBLY in
//! `ghidra/scripts/funcs/overlay_battle_action_801d71b8.txt` (1081
//! instructions).
//!
//! ## What this module ports
//!
//! The parts whose every instruction is accounted for:
//!
//! * [`gate`] - the four-way entry test.
//! * [`CameraPose`] + [`CameraPose::seed`] - the local triple the arms mutate,
//!   including the negations that make the look-at the *inverse* of the actor's
//!   position and facing.
//! * [`character_arm`] / [`art_arm`] - the two dispatch layers: character id
//!   `1`/`2`/`3` and then art id `0x1A..=0x2A` through a 17-slot jump table.
//! * [`anim_push`] - the animation-frame-driven camera push the arms share,
//!   with its `0x60` bias, `<< 4` scale and `0x100` clamp.
//! * [`ArmPhase`] - the phase-table stride the arms index by `ctx[+0x26D]`.
//!
//! ## What it does not port
//!
//! The seventeen per-art arms themselves. Each is a short straight-line block
//! that reads its own halfword track out of the per-phase table at
//! `0x801F4E10` and folds it into the pose, and the tracks are **disc data** in
//! PROT 0898's tail - reproducing the arms without that table would be
//! transcribing offsets with nothing to check them against. [`ArmPhase`]
//! records the indexing so the table can be parsed later; the folds stay
//! undocumented rather than guessed.
//!
//! # NOT WIRED
//!
//! No engine caller, and the missing prerequisite is the table above: the
//! engine frames battles with a phase-scripted snap
//! (`BattleActionHost::camera_bounds` into `engine-shell`'s
//! `retail_battle_mvp`), not a per-art script, so there is nothing holding a
//! `ctx[+0x26D]` phase cursor or the `0x801F4E10` tracks for the arms to read.
//! Wiring means a disc parser for that table plus a per-art camera channel on
//! the engine's battle camera, both outside this crate.

/// Action category the gate demands (`actor[+0x1DE] == 3`, Attack).
pub const CATEGORY_ATTACK: u8 = 3;
/// Battle-phase byte the gate demands (`ctx[+6] == 0xFF`).
pub const PHASE_ACTIVE: u8 = 0xFF;
/// Party slots this routine frames (`ctx[+0x13] < 3`).
pub const PARTY_SLOTS: u8 = 3;
/// First art id with a camera arm (`actor[+0x1DB] - 0x1A`, table index `0`).
pub const FIRST_ART: u8 = 0x1A;
/// Camera arms in the per-art jump table (`sltiu v0, v1, 0x11`).
pub const ART_ARMS: u8 = 0x11;
/// Base pitch/yaw magnitude the pose seeds two of its slots with (`0x400`).
pub const POSE_SEED_ANGLE: i16 = 0x400;
/// Animation-frame threshold below which the push is skipped (`slti a0, 0x61`).
pub const ANIM_PUSH_FLOOR: i16 = 0x61;
/// Bias subtracted from the animation frame before scaling (`addiu v0, -0x60`).
pub const ANIM_PUSH_BIAS: i16 = 0x60;
/// Ceiling on the scaled push (`sltiu a0, 0x101`, so the cap is `0x100`).
pub const ANIM_PUSH_CAP: i32 = 0x100;

/// The four-way entry test (`0x801D71B8..0x801D722C`).
///
/// All four conditions are `bne`/`beq` straight to the epilogue, so any one
/// failing makes the whole frame a no-op:
///
/// 1. the active actor's target slot holds a live actor (`+0x14C != 0`);
/// 2. the active actor's category is Attack (`+0x1DE == 3`);
/// 3. the battle phase byte `ctx[+6]` is `0xFF`;
/// 4. the active slot is a party slot (`ctx[+0x13] < 3`) whose participant id
///    is `1`, `2` or `3` - any other id, including a monster slot, exits.
pub const fn gate(target_hp: u16, category: u8, phase: u8, active_slot: u8) -> bool {
    target_hp != 0
        && category == CATEGORY_ATTACK
        && phase == PHASE_ACTIVE
        && active_slot < PARTY_SLOTS
}

/// Which character's camera script runs, keyed on the participant-id byte
/// `DAT_8007BD10[active_slot]`.
///
/// The dispatch is a three-way compare with no default arm: `2` first, then
/// `< 3` splitting off `1`, then `== 3`. Everything else falls to the epilogue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharacterArm {
    /// Participant id `1`.
    One,
    /// Participant id `2`.
    Two,
    /// Participant id `3`.
    Three,
}

/// Resolve [`CharacterArm`], or `None` for an id with no camera script.
pub const fn character_arm(participant_id: u8) -> Option<CharacterArm> {
    match participant_id {
        1 => Some(CharacterArm::One),
        2 => Some(CharacterArm::Two),
        3 => Some(CharacterArm::Three),
        _ => None,
    }
}

/// The per-art jump-table index, or `None` when the current art has no arm.
///
/// Retail biases by [`FIRST_ART`] and bounds with an **unsigned** compare, so an
/// art id below the bias wraps to a huge index and is rejected by the same test
/// that rejects one above the range.
pub const fn art_arm(art_id: u8) -> Option<u8> {
    let index = art_id.wrapping_sub(FIRST_ART);
    if art_id >= FIRST_ART && index < ART_ARMS {
        Some(index)
    } else {
        None
    }
}

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
/// fixed byte offsets off it (`+0`, `+4`, `+8`, `+0xC`, `+0x14` are the ones the
/// dumped arms use). So the table is a set of parallel halfword tracks four
/// bytes apart, each holding two phases - which is the indexing, not a claim
/// about what any track means.
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
}

/// Retail's per-arm animation-frame thresholds, in the order the arms test them.
///
/// The first arm gates at `0x61`, later arms at `0xE0` and `0xF0`; each is a
/// `slti` against the same `actor[+0x22C][+0x68]` counter, so an arm's framing
/// changes only once the swing clip has run that far.
pub const ANIM_THRESHOLDS: [i16; 3] = [0x61, 0xE0, 0xF0];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_gate_condition_is_independently_fatal() {
        assert!(gate(100, CATEGORY_ATTACK, PHASE_ACTIVE, 0));
        assert!(!gate(0, CATEGORY_ATTACK, PHASE_ACTIVE, 0), "dead target");
        assert!(!gate(100, 2, PHASE_ACTIVE, 0), "magic, not attack");
        assert!(!gate(100, CATEGORY_ATTACK, 0x14, 0), "wrong phase");
        assert!(!gate(100, CATEGORY_ATTACK, PHASE_ACTIVE, 3), "monster slot");
        assert!(!gate(100, CATEGORY_ATTACK, PHASE_ACTIVE, 7));
    }

    #[test]
    fn only_three_participant_ids_have_camera_scripts() {
        assert_eq!(character_arm(1), Some(CharacterArm::One));
        assert_eq!(character_arm(2), Some(CharacterArm::Two));
        assert_eq!(character_arm(3), Some(CharacterArm::Three));
        for id in [0u8, 4, 5, 0xFF] {
            assert_eq!(character_arm(id), None, "id {id}");
        }
    }

    #[test]
    fn art_arm_covers_seventeen_ids_and_rejects_both_sides() {
        assert_eq!(art_arm(0x1A), Some(0));
        assert_eq!(art_arm(0x2A), Some(0x10));
        assert_eq!(art_arm(0x2B), None, "one past the table");
        assert_eq!(art_arm(0x19), None, "one before the bias wraps unsigned");
        assert_eq!(art_arm(0), None);
        let covered = (0u8..=0xFF).filter(|&i| art_arm(i).is_some()).count();
        assert_eq!(covered, ART_ARMS as usize);
    }

    #[test]
    fn art_arm_indices_are_dense_and_in_order() {
        for (want, id) in (FIRST_ART..(FIRST_ART + ART_ARMS)).enumerate() {
            assert_eq!(art_arm(id), Some(want as u8));
        }
    }

    #[test]
    fn pose_seed_negates_the_position_and_the_facing() {
        let p = CameraPose::seed([100, 200, 300], 0x800);
        assert_eq!(p.look_at, [-100, -200, -300]);
        assert_eq!(p.rot, [0, -0x800, 0]);
        assert_eq!(p.dist, [0, POSE_SEED_ANGLE, POSE_SEED_ANGLE]);
    }

    #[test]
    fn pose_seed_negation_wraps_at_the_halfword_boundary() {
        // The actor position is read with `lhu` and negated with `subu`, so a
        // coordinate above 0x7FFF comes back as a positive i16.
        let p = CameraPose::seed([0x8000, 0, 0], 0);
        assert_eq!(p.look_at[0], -0x8000i32 as i16);
        let p = CameraPose::seed([0xFFFF, 0, 0], 0);
        assert_eq!(p.look_at[0], 1);
    }

    #[test]
    fn anim_push_skips_below_the_floor_and_clamps_above_the_cap() {
        assert_eq!(anim_push(0), None);
        assert_eq!(anim_push(ANIM_PUSH_FLOOR - 1), None);
        // At the floor the push is (0x61 - 0x60) << 4 = 0x10.
        assert_eq!(anim_push(ANIM_PUSH_FLOOR), Some(0x10));
        // 0x70 - 0x60 = 0x10, << 4 = 0x100, exactly at the cap and kept.
        assert_eq!(anim_push(0x70), Some(0x100));
        // One frame later the scale overshoots and the clamp bites.
        assert_eq!(anim_push(0x71), Some(ANIM_PUSH_CAP));
        assert_eq!(anim_push(0x7FF), Some(ANIM_PUSH_CAP));
    }

    #[test]
    fn push_split_differs_between_the_two_phase_arms() {
        let base = CameraPose::seed([0, 0, 0], 0);

        let mut a = base;
        apply_anim_push(&mut a, 1, 0x100);
        assert_eq!(a.dist[1], POSE_SEED_ANGLE - 0x100);
        assert_eq!(a.rot[1], 0x80);
        assert_eq!(a.dist[2], POSE_SEED_ANGLE - 0x40);
        assert_eq!(a.dist[0], 0x100);

        let mut b = base;
        apply_anim_push(&mut b, 0, 0x100);
        assert_eq!(b.dist[1], POSE_SEED_ANGLE - 0x100);
        assert_eq!(b.rot[1], -0x100);
        assert_eq!(b.dist[2], POSE_SEED_ANGLE - 0x80);
        assert_eq!(b.dist[0], 0, "the zero-phase arm leaves dist[0] alone");
    }

    #[test]
    fn push_of_zero_still_writes_the_shared_slot() {
        // The store at 0x801D7394 precedes the branch, so it happens even for a
        // push the arms would otherwise ignore.
        let mut p = CameraPose::seed([0, 0, 0], 0);
        apply_anim_push(&mut p, 1, 0);
        assert_eq!(p.dist[1], POSE_SEED_ANGLE);
    }

    #[test]
    fn track_offsets_step_by_two_per_phase_and_four_per_track() {
        let p = ArmPhase { cursor: 0 };
        assert_eq!(p.track_offset(0), 0);
        assert_eq!(p.track_offset(1), 4);
        assert_eq!(p.track_offset(2), 8);
        assert_eq!(p.track_offset(3), 0xC);
        assert_eq!(p.track_offset(5), 0x14);
        let q = ArmPhase { cursor: 3 };
        assert_eq!(q.track_offset(0), 6);
        assert_eq!(q.track_offset(1), 10);
    }

    #[test]
    fn thresholds_are_ordered() {
        assert!(ANIM_THRESHOLDS.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(ANIM_THRESHOLDS[0], ANIM_PUSH_FLOOR);
    }
}

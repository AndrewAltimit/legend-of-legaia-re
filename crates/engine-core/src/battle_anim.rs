//! Per-actor battle animation playback.
//!
//! The battle counterpart of the field [`legaia_anm::AnimPlayer`]. Where field
//! actors animate from an ANM record (8-byte per-bone keyframes, decoded by
//! `FUN_8001BE80`), a battle monster (and the player summon, which retail poses
//! exactly like an enemy body) animates from the per-object rigid-transform
//! keyframe stream in its archive record (`legaia_asset::monster_archive`
//! [`MonsterAnimation`], the `+0x8c` `[u8 parts][u8 frames][9-byte TRS]` stream
//! decoded by `FUN_8004998C`). Action 0 is the idle loop.
//!
//! [`MonsterAnimPlayer::tick`] advances a fixed-point frame cursor and produces
//! a [`legaia_anm::PoseFrame`] — the same per-object `(translation, rotation)`
//! shape the field player produces — so the renderer's existing posed-mesh path
//! consumes both. Battle meshes deform through
//! [`legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot`] (the rigid `R·v + T`
//! builder) so the per-object rotations actually turn the limbs.
//!
//! Interpolation matches the retail decoder + the visually-validated site
//! animator (`monsters.html` `_frameTransforms`): translation lerps linearly,
//! rotation takes the shortest-path 12-bit-angle step. The per-tick phase
//! advance is retail-pinned when the clip carries its entry's rate byte:
//! `FUN_80047430` advances the node's 12.4 cursor by
//! `(frame_dt * actor[+0x21D] * record[+0x78]) >> 1` per frame, which with the
//! normal `+0x21D == 4` and a 1-frame delta is `rate / 8` keyframes per tick
//! ([`step_for_rate`]). A zero rate (clip built without entry context) keeps
//! the historical display default.

use legaia_anm::PoseFrame;
use legaia_asset::monster_archive::{MonsterAnimation, PartPose};

/// Fixed-point fractional bits for the frame cursor (8.8): `1 << 8` phase units
/// per keyframe.
const PHASE_FRAC_BITS: u32 = 8;
const PHASE_ONE: u32 = 1 << PHASE_FRAC_BITS;

/// Per-actor battle animation player. Holds one decoded
/// [`MonsterAnimation`]'s frames and an 8.8 fixed-point loop cursor.
#[derive(Debug, Clone)]
pub struct MonsterAnimPlayer {
    frames: Vec<Vec<PartPose>>,
    frame_count: u32,
    part_count: usize,
    /// The clip's `action_id` (the action-table slot for player files /
    /// archive entry index for monsters), retained so per-frame consumers —
    /// the facial animator looks up the playing entry's face tracks by it —
    /// can identify the clip without holding the `MonsterAnimation`.
    action_id: u8,
    /// 8.8 fixed-point frame cursor (integer part = keyframe index).
    phase: u32,
    /// Phase units added per [`tick`](Self::tick). Seeded from the clip's
    /// entry rate byte via [`step_for_rate`] (retail: `rate * 2` units of
    /// 1/16 keyframe per frame); still adjustable per player.
    pub step: u32,
    /// `true` (default) loops the clip forever (idle). `false` plays the clip
    /// once: the cursor clamps at the last keyframe and [`Self::finished`]
    /// turns `true` (action clips - attack ready / hit recovery / defeat).
    looping: bool,
    /// One-shot completion latch (see [`Self::new_one_shot`]).
    finished: bool,
}

/// Retail-pinned per-tick phase advance for an entry rate byte: the
/// `FUN_80047430` cursor delta `(1 * 4 * rate) >> 1` = `2 * rate` units of
/// 1/16 keyframe, scaled to this player's 8.8 phase (`rate * 32`). A zero
/// rate (no entry context) falls back to the historical display default
/// (`64`, which equals rate `2` - the faster of the two retail values).
// PORT: FUN_80047430 - the per-frame anim-node cursor advance
// (`node+0x68 += (DAT_1F800393 * actor[+0x21D] * record[+0x78]) >> 1`),
// reduced to the normal case `frame_dt = 1`, `actor[+0x21D] = 4`.
pub fn step_for_rate(rate: u8) -> u32 {
    if rate == 0 { 64 } else { rate as u32 * 32 }
}

impl MonsterAnimPlayer {
    /// Build a player around one decoded animation (typically action 0 = idle).
    /// Returns `None` for a degenerate animation (no parts or no frames).
    /// The playback step comes from the clip's entry rate byte
    /// ([`step_for_rate`]).
    pub fn new(anim: &MonsterAnimation) -> Option<Self> {
        if anim.frame_count == 0 || anim.part_count == 0 {
            return None;
        }
        Some(Self {
            frames: anim.frames.clone(),
            frame_count: anim.frame_count as u32,
            part_count: anim.part_count,
            action_id: anim.action_id,
            phase: 0,
            step: step_for_rate(anim.rate),
            looping: true,
            finished: false,
        })
    }

    /// Build a **one-shot** player: the clip plays once, the cursor clamps at
    /// the last keyframe, and [`Self::finished`] reports completion. Used for
    /// battle action clips (ready / recover / defeat) where idle is the loop
    /// to fall back to.
    pub fn new_one_shot(anim: &MonsterAnimation) -> Option<Self> {
        let mut p = Self::new(anim)?;
        p.looping = false;
        Some(p)
    }

    /// `true` once a one-shot clip has reached its last keyframe. Always
    /// `false` for a looping player.
    pub fn finished(&self) -> bool {
        self.finished
    }

    /// Number of animated parts (= TMD objects the pose addresses).
    pub fn part_count(&self) -> usize {
        self.part_count
    }

    /// The playing clip's `action_id` (see the field docs).
    pub fn action_id(&self) -> u8 {
        self.action_id
    }

    /// Integer keyframe index of the cursor — the value retail's render-node
    /// update passes to the facial animator as the frame counter
    /// (`FUN_80047430` hands `FUN_8004C7B4` the node's 12.4 `+0x68` cursor
    /// shifted to whole keyframes; this player's 8.8 phase shifts the same
    /// way). The facial tracks' `start`/`end` bytes are in these units.
    pub fn current_frame(&self) -> i16 {
        (self.phase >> PHASE_FRAC_BITS) as i16
    }

    /// Reset the loop cursor to the start of the idle clip.
    pub fn rewind(&mut self) {
        self.phase = 0;
    }

    /// Advance one tick and return the interpolated per-object pose. A looping
    /// player wraps over the clip (`PoseFrame::finished` always `false`); a
    /// one-shot player clamps at the last keyframe and reports `finished`.
    pub fn tick(&mut self) -> PoseFrame {
        let total = self.frame_count * PHASE_ONE;
        let (f0, f1);
        if self.looping {
            self.phase = (self.phase + self.step) % total;
            f0 = (self.phase >> PHASE_FRAC_BITS) as usize % self.frames.len();
            f1 = (f0 + 1) % self.frames.len();
        } else {
            // One-shot: clamp the cursor on the final keyframe.
            let last = (self.frame_count - 1) * PHASE_ONE;
            self.phase = (self.phase + self.step).min(last);
            self.finished = self.phase >= last;
            f0 = (self.phase >> PHASE_FRAC_BITS) as usize % self.frames.len();
            f1 = (f0 + 1).min(self.frames.len() - 1);
        }
        let frac = (self.phase & (PHASE_ONE - 1)) as i32; // 0..=255

        let a = &self.frames[f0];
        let b = &self.frames[f1];
        let bone_outputs = (0..self.part_count)
            .map(|p| {
                let pa = &a[p];
                let pb = &b[p];
                let t = [
                    lerp_lin(pa.tx, pb.tx, frac),
                    lerp_lin(pa.ty, pb.ty, frac),
                    lerp_lin(pa.tz, pb.tz, frac),
                ];
                let r = [
                    lerp_angle(pa.rx, pb.rx, frac),
                    lerp_angle(pa.ry, pb.ry, frac),
                    lerp_angle(pa.rz, pb.rz, frac),
                ];
                (t, r)
            })
            .collect();

        PoseFrame {
            bone_outputs,
            factor: (frac as u8),
            finished: self.finished,
        }
    }
}

/// Linear translation lerp by `frac/256`, matching the site animator's
/// `pa + (pb - pa) * frac`.
fn lerp_lin(a: i16, b: i16, frac: i32) -> i16 {
    (a as i32 + (b as i32 - a as i32) * frac / PHASE_ONE as i32) as i16
}

/// Shortest-path 12-bit-angle lerp (`((b - a + 6144) % 4096) - 2048` step),
/// matching the retail wrap and the site animator. The result stays an `i16`
/// 12-bit angle (the posed-mesh builder converts it to radians); it may sit
/// slightly outside `0..4096` mid-step, which is fine for `cos`/`sin`.
fn lerp_angle(a: u16, b: u16, frac: i32) -> i16 {
    let step = (b as i32 - a as i32 + 6144).rem_euclid(4096) - 2048;
    (a as i32 + step * frac / PHASE_ONE as i32) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anim_2frame() -> MonsterAnimation {
        // One part, two frames: frame0 at rest, frame1 translated +100 on X and
        // rotated a quarter turn (1024 = 4096/4) about Z.
        MonsterAnimation {
            action_id: 0,
            rate: 2,
            part_count: 1,
            frame_count: 2,
            frames: vec![
                vec![PartPose {
                    tx: 0,
                    ty: 0,
                    tz: 0,
                    rx: 0,
                    ry: 0,
                    rz: 0,
                }],
                vec![PartPose {
                    tx: 100,
                    ty: 0,
                    tz: 0,
                    rx: 0,
                    ry: 0,
                    rz: 1024,
                }],
            ],
        }
    }

    #[test]
    fn new_rejects_degenerate() {
        let a = MonsterAnimation {
            action_id: 0,
            rate: 2,
            part_count: 0,
            frame_count: 0,
            frames: vec![],
        };
        assert!(MonsterAnimPlayer::new(&a).is_none());
    }

    #[test]
    fn tick_interpolates_toward_next_frame() {
        let anim = anim_2frame();
        let mut p = MonsterAnimPlayer::new(&anim).unwrap();
        // Land the cursor exactly halfway into frame 0->1 (phase = 0.5 frames).
        p.step = PHASE_ONE / 2; // 128
        let f = p.tick();
        assert_eq!(f.bone_outputs.len(), 1);
        let (t, r) = f.bone_outputs[0];
        assert_eq!(t[0], 50, "translation halfway = 50");
        assert_eq!(r[2], 512, "rotation halfway = 1024/2 = 512");
    }

    #[test]
    fn tick_loops_over_the_clip() {
        let anim = anim_2frame();
        let mut p = MonsterAnimPlayer::new(&anim).unwrap();
        p.step = PHASE_ONE; // one whole keyframe per tick
        let _ = p.tick(); // frame 1
        let f = p.tick(); // wraps to frame 0
        let (t, r) = f.bone_outputs[0];
        assert_eq!(t[0], 0, "looped back to rest translation");
        assert_eq!(r[2], 0, "looped back to rest rotation");
        assert!(!f.finished);
    }

    #[test]
    fn rotation_takes_shortest_path() {
        // 3840 -> 256 is a +512 wrap (through 0), not a -3584 sweep. Halfway
        // should land near the wrap midpoint (4096/0), not near 2048.
        let anim = MonsterAnimation {
            action_id: 0,
            rate: 2,
            part_count: 1,
            frame_count: 2,
            frames: vec![
                vec![PartPose {
                    tx: 0,
                    ty: 0,
                    tz: 0,
                    rx: 0,
                    ry: 0,
                    rz: 3840,
                }],
                vec![PartPose {
                    tx: 0,
                    ty: 0,
                    tz: 0,
                    rx: 0,
                    ry: 0,
                    rz: 256,
                }],
            ],
        };
        let mut p = MonsterAnimPlayer::new(&anim).unwrap();
        p.step = PHASE_ONE / 2;
        let (_, r) = p.tick().bone_outputs[0];
        // step = ((256 - 3840 + 6144) % 4096) - 2048 = (2560 % 4096) - 2048 = 512.
        // halfway: 3840 + 512/2 = 4096.
        assert_eq!(r[2], 4096);
    }
}

#[cfg(test)]
mod one_shot_tests {
    use super::*;
    use legaia_asset::monster_archive::PartPose;

    fn clip(frames: usize) -> MonsterAnimation {
        MonsterAnimation {
            action_id: 8,
            rate: 2,
            part_count: 1,
            frame_count: frames,
            frames: (0..frames)
                .map(|f| {
                    vec![PartPose {
                        tx: f as i16 * 10,
                        ty: 0,
                        tz: 0,
                        rx: 0,
                        ry: 0,
                        rz: 0,
                    }]
                })
                .collect(),
        }
    }

    #[test]
    fn one_shot_clamps_on_last_keyframe_and_finishes() {
        let mut p = MonsterAnimPlayer::new_one_shot(&clip(3)).unwrap();
        p.step = 256; // one keyframe per tick
        assert!(!p.finished());
        let _ = p.tick(); // frame 1
        assert!(!p.finished());
        let f = p.tick(); // frame 2 (last)
        assert!(p.finished());
        assert!(f.finished);
        let (t, _) = f.bone_outputs[0];
        assert_eq!(t[0], 20, "clamped on the final keyframe");
        // Further ticks hold the final pose.
        let f2 = p.tick();
        assert_eq!(f2.bone_outputs[0].0[0], 20);
        assert!(f2.finished);
    }

    #[test]
    fn looping_player_never_finishes() {
        let mut p = MonsterAnimPlayer::new(&clip(3)).unwrap();
        p.step = 256;
        for _ in 0..10 {
            assert!(!p.tick().finished);
        }
        assert!(!p.finished());
    }
}

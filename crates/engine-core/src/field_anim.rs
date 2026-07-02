//! Field-player locomotion clip playback.
//!
//! Drives the party locomotion ANM bundle (PROT 0874 §1,
//! [`legaia_asset::character_pack::field_locomotion_anm`]) at runtime: the
//! standing **idle** loop (bank slot
//! [`legaia_asset::character_pack::LOCOMOTION_IDLE_SLOT`]) while the player
//! stands, the **walk** loop (bank slot
//! [`legaia_asset::character_pack::LOCOMOTION_WALK_SLOT`]) while pad / nav
//! locomotion moves the player - the same two records the retail player
//! actor's `+0x4C` anim pointer alternates between (pinned live via
//! `scripts/pcsx-redux/autorun_locomotion_clip_pin.lua`).
//!
//! REF: FUN_8001B964 - the retail per-actor animated renderer walks the
//! record's per-(bone, frame) 8-byte entries each draw; the per-entry decode
//! is ported at [`legaia_asset::player_anm::BoneTransform::decode`]
//! (`FUN_8001BE80`). This module owns the playhead: it pre-decodes every
//! frame of a clip and emits one [`PoseFrame`] per engine tick, which the
//! host's posed-mesh rebuild consumes exactly like the battle
//! [`crate::battle_anim::MonsterAnimPlayer`] output.

use legaia_anm::PoseFrame;
use legaia_asset::player_anm::PlayerAnmBundle;

/// Engine ticks per clip frame. The clip streams carry no rate byte of their
/// own (unlike the monster-archive `+0x78` rate); retail advances the field
/// anim on the 30 Hz field tick while the host renders at 60, so two host
/// ticks per clip frame reproduces the retail cadence (a 15-frame walk loop
/// ≈ half a second per cycle).
pub const DEFAULT_TICKS_PER_FRAME: u32 = 2;

/// Looping playback over one locomotion clip: all frames pre-decoded to the
/// `(translation, rotation)` pairs [`PoseFrame`] carries, one frame advance
/// every [`FieldClipPlayer::ticks_per_frame`] ticks.
#[derive(Debug, Clone)]
pub struct FieldClipPlayer {
    /// Per-frame, per-bone rigid transforms (`bone_outputs` rows).
    frames: Vec<Vec<([i16; 3], [i16; 3])>>,
    frame: usize,
    counter: u32,
    /// Engine ticks between frame advances (min 1).
    pub ticks_per_frame: u32,
}

impl FieldClipPlayer {
    /// Pre-decode record `record_index` of a locomotion bundle. `None` when
    /// the record is out of range / malformed or carries no frames.
    pub fn from_record(bundle: &PlayerAnmBundle, record_index: usize) -> Option<Self> {
        let rec = bundle.record(record_index).ok()?;
        let (bones, frame_count) = (rec.bone_count as usize, rec.frame_count as usize);
        if bones == 0 || frame_count == 0 {
            return None;
        }
        let mut frames = Vec::with_capacity(frame_count);
        for f in 0..frame_count {
            let mut row = Vec::with_capacity(bones);
            for b in 0..bones {
                let t = bundle.bone_transform(record_index, f, b)?;
                row.push((
                    [t.t_x as i16, t.t_y as i16, t.t_z as i16],
                    [t.r_x as i16, t.r_y as i16, t.r_z as i16],
                ));
            }
            frames.push(row);
        }
        Some(Self {
            frames,
            frame: 0,
            counter: 0,
            ticks_per_frame: DEFAULT_TICKS_PER_FRAME,
        })
    }

    /// Bones per frame.
    pub fn bone_count(&self) -> usize {
        self.frames.first().map_or(0, Vec::len)
    }

    /// Frames in the clip.
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Restart the clip at frame 0 (called on an idle↔walk switch so the
    /// incoming loop starts at its first keyframe).
    pub fn rewind(&mut self) {
        self.frame = 0;
        self.counter = 0;
    }

    /// Emit the current frame's pose and advance the playhead (wrapping -
    /// locomotion clips loop, `finished` stays `false`).
    pub fn tick(&mut self) -> PoseFrame {
        let pose = PoseFrame {
            bone_outputs: self.frames[self.frame].clone(),
            factor: 0,
            finished: false,
        };
        self.counter += 1;
        if self.counter >= self.ticks_per_frame.max(1) {
            self.counter = 0;
            self.frame = (self.frame + 1) % self.frames.len();
        }
        pose
    }
}

/// The player's live idle/walk clip pair plus the per-tick movement signal
/// locomotion feeds it. Hosts install one via
/// [`crate::world::World::set_field_player_anim`]; [`crate::world::World`]
/// ticks it after the locomotion step each field frame and folds the output
/// into the player actor's `pose_frame`.
#[derive(Debug, Clone)]
pub struct FieldPlayerAnim {
    pub idle: FieldClipPlayer,
    pub walk: FieldClipPlayer,
    /// Which clip is currently playing (`true` = walk).
    pub walking: bool,
    /// Set by the locomotion step when the player attempted a move this tick
    /// (held pad or nav step - a wall-blocked step still walks in place, as
    /// retail does). Consumed and cleared by the anim tick.
    pub moved_this_frame: bool,
}

impl FieldPlayerAnim {
    pub fn new(idle: FieldClipPlayer, walk: FieldClipPlayer) -> Self {
        Self {
            idle,
            walk,
            walking: false,
            moved_this_frame: false,
        }
    }

    /// One field tick: switch clips on a movement-state edge (rewinding the
    /// incoming clip) and emit the active clip's pose.
    pub fn tick(&mut self) -> PoseFrame {
        let moved = std::mem::take(&mut self.moved_this_frame);
        if moved != self.walking {
            self.walking = moved;
            if moved {
                self.walk.rewind();
            } else {
                self.idle.rewind();
            }
        }
        if self.walking {
            self.walk.tick()
        } else {
            self.idle.tick()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::player_anm::{ANM_MARKER_1, parse};

    /// Synthetic 2-record bundle: record 0 = "walk" (2 bones, 3 frames),
    /// record 1 = "idle" (2 bones, 2 frames). Frame f bone b tags t_x with
    /// a recognisable low byte.
    fn synth_bundle() -> PlayerAnmBundle {
        let mut buf = Vec::new();
        let count: u32 = 2;
        buf.extend_from_slice(&count.to_le_bytes());
        let rec0_size = 8 + 8 * 2 * 3 + 8;
        let off0 = (4 + 8) as u32;
        let off1 = off0 + rec0_size as u32;
        buf.extend_from_slice(&off0.to_le_bytes());
        buf.extend_from_slice(&off1.to_le_bytes());
        for (rec, frames) in [(0u8, 3u8), (1, 2)] {
            buf.extend_from_slice(&2u16.to_le_bytes()); // a: 2 bones
            buf.extend_from_slice(&(frames as u16).to_le_bytes()); // b
            buf.extend_from_slice(&ANM_MARKER_1.to_le_bytes());
            buf.extend_from_slice(&0x0002u16.to_le_bytes());
            for f in 0..frames {
                for b in 0..2u8 {
                    // t_x low byte = rec*100 + f*10 + b; everything else 0.
                    buf.extend_from_slice(&[rec * 100 + f * 10 + b, 0, 0, 0, 0, 0, 0, 0]);
                }
            }
            buf.extend_from_slice(&[0u8; 8]);
        }
        parse(&buf).expect("synthetic bundle parses")
    }

    #[test]
    fn clip_player_decodes_and_wraps() {
        let bundle = synth_bundle();
        let mut p = FieldClipPlayer::from_record(&bundle, 0).expect("record 0");
        p.ticks_per_frame = 1;
        assert_eq!(p.bone_count(), 2);
        assert_eq!(p.frame_count(), 3);
        // Frames 0,1,2 then wrap to 0.
        for expect in [0i16, 10, 20, 0] {
            let pose = p.tick();
            assert_eq!(pose.bone_outputs[0].0[0], expect);
            assert_eq!(pose.bone_outputs[1].0[0], expect + 1);
            assert!(!pose.finished);
        }
    }

    #[test]
    fn ticks_per_frame_holds_frames() {
        let bundle = synth_bundle();
        let mut p = FieldClipPlayer::from_record(&bundle, 0).unwrap();
        p.ticks_per_frame = 2;
        assert_eq!(p.tick().bone_outputs[0].0[0], 0);
        assert_eq!(p.tick().bone_outputs[0].0[0], 0);
        assert_eq!(p.tick().bone_outputs[0].0[0], 10);
    }

    #[test]
    fn world_tick_drives_walk_idle_switch_into_pose_frame() {
        use crate::world::{SceneMode, World};
        let mut w = World {
            mode: SceneMode::Field,
            ..World::default()
        };
        w.install_field_player(0);
        let bundle = synth_bundle();
        let mut idle = FieldClipPlayer::from_record(&bundle, 1).unwrap();
        let mut walk = FieldClipPlayer::from_record(&bundle, 0).unwrap();
        idle.ticks_per_frame = 1;
        walk.ticks_per_frame = 1;
        w.set_field_player_anim(Some(FieldPlayerAnim::new(idle, walk)));
        // Standing frame: idle clip pose lands in the player's pose_frame.
        w.set_pad(0);
        let _ = w.tick();
        let pose = w.actors[0].pose_frame.clone().expect("idle pose set");
        assert_eq!(pose.bone_outputs[0].0[0], 100, "idle record tag");
        assert!(!w.field_player_anim.as_ref().unwrap().walking);
        // Held direction: locomotion flags the move, the walk clip plays.
        w.set_pad(crate::input::PadButton::Up.mask());
        let _ = w.tick();
        let pose = w.actors[0].pose_frame.clone().expect("walk pose set");
        assert_eq!(pose.bone_outputs[0].0[0], 0, "walk record restarts");
        assert!(w.field_player_anim.as_ref().unwrap().walking);
        // Release: back to idle, restarted at frame 0.
        w.set_pad(0);
        let _ = w.tick();
        let pose = w.actors[0].pose_frame.clone().expect("idle pose set");
        assert_eq!(pose.bone_outputs[0].0[0], 100);
        assert!(!w.field_player_anim.as_ref().unwrap().walking);
    }

    #[test]
    fn player_anim_switches_clips_on_move_edge() {
        let bundle = synth_bundle();
        let mut idle = FieldClipPlayer::from_record(&bundle, 1).unwrap();
        let mut walk = FieldClipPlayer::from_record(&bundle, 0).unwrap();
        idle.ticks_per_frame = 1;
        walk.ticks_per_frame = 1;
        let mut anim = FieldPlayerAnim::new(idle, walk);
        // Standing: idle record (tag 100+).
        assert_eq!(anim.tick().bone_outputs[0].0[0], 100);
        assert_eq!(anim.tick().bone_outputs[0].0[0], 110);
        // Move: walk record restarts at frame 0 (tag 0+).
        anim.moved_this_frame = true;
        assert_eq!(anim.tick().bone_outputs[0].0[0], 0);
        anim.moved_this_frame = true;
        assert_eq!(anim.tick().bone_outputs[0].0[0], 10);
        // Release: idle restarts at frame 0.
        assert_eq!(anim.tick().bone_outputs[0].0[0], 100);
        assert!(!anim.walking);
    }
}

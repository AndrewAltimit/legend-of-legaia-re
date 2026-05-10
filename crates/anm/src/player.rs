//! Per-frame ANM playback driver.
//!
//! Wraps [`KeyframeReader`] + [`BoneKeyframe::interpolate`] in a small
//! state machine engines call once per frame. Mirrors the slice of
//! `FUN_80021DF4` (block at `0x80022ec4..0x80023040`) that walks the
//! keyframe table indexed by bone count from the actor's mesh context.
//!
//! Engines wire one [`AnimPlayer`] per active actor with a registered
//! animation. The driver:
//!
//!  1. Holds the record bytes + bone count + current factor (`u8`, the
//!     retail `actor[+0x22]` interpolation factor).
//!  2. On [`AnimPlayer::tick`] advances the factor by `actor[+0x23]`
//!     (the per-frame factor delta) and produces the per-bone output
//!     deltas - one `(pos, rot)` pair per bone.
//!  3. Reports `Done` when the factor wraps past 0xFF (one cycle done);
//!     looped animations let the engine reset.

use crate::{BoneKeyframe, KeyframeReader};

/// One frame's per-bone output. Engines push these into the renderer's
/// per-actor pose buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoseFrame {
    pub bone_outputs: Vec<([i16; 3], [i16; 3])>,
    /// Current interpolation factor (0..=255). Engines that want to
    /// blend two animations together read this as the t-coefficient.
    pub factor: u8,
    /// `true` once the factor has wrapped past 0xFF for the current cycle.
    pub finished: bool,
}

/// Per-actor animation playback state.
#[derive(Debug, Clone)]
pub struct AnimPlayer {
    record: Vec<u8>,
    bone_count: usize,
    factor: u16,
    /// Per-frame factor delta (retail `actor[+0x23]`). Default `4` ≈ 64
    /// frames per cycle - typical for idle / walk loops.
    pub frame_delta: u16,
    /// `true` to loop. When `false`, [`AnimPlayer::tick`] reports
    /// `finished = true` once and clamps the factor at 0xFF for
    /// subsequent ticks.
    pub looping: bool,
}

impl AnimPlayer {
    /// Build a player around an animation record. Errors if the record
    /// can't fit a keyframe table for `bone_count`.
    pub fn new(record: Vec<u8>, bone_count: usize) -> anyhow::Result<Self> {
        // Smoke-validate the keyframe table layout up front.
        let _ = KeyframeReader::parse(&record, bone_count)?;
        Ok(Self {
            record,
            bone_count,
            factor: 0,
            frame_delta: 4,
            looping: true,
        })
    }

    /// Advance one frame. Bumps the internal factor by `frame_delta`,
    /// computes per-bone outputs, returns the resulting [`PoseFrame`].
    pub fn tick(&mut self) -> PoseFrame {
        let prev = self.factor;
        self.factor = self.factor.saturating_add(self.frame_delta);
        let mut finished = false;
        if self.factor > 0xFF {
            if self.looping {
                self.factor %= 0x100;
            } else {
                self.factor = 0xFF;
                finished = true;
            }
        }
        let _ = prev;
        let factor_u8 = self.factor as u8;
        let reader = KeyframeReader::parse(&self.record, self.bone_count).expect(
            "record validated in `new` - re-parse should always succeed since the record \
             is held by value here and never mutated",
        );
        let bone_outputs = reader
            .iter()
            .map(|kf: BoneKeyframe| kf.interpolate(factor_u8))
            .collect();
        PoseFrame {
            bone_outputs,
            factor: factor_u8,
            finished,
        }
    }

    /// Reset the playhead to the start of the cycle. Engines call this
    /// when transitioning to a new animation segment.
    pub fn rewind(&mut self) {
        self.factor = 0;
    }

    /// Read the current factor (0..=0xFF).
    pub fn factor(&self) -> u8 {
        self.factor as u8
    }

    /// Bone count this player is configured for.
    pub fn bone_count(&self) -> usize {
        self.bone_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RECORD_HEADER_SIZE;

    fn synth_record(bone_count: usize) -> Vec<u8> {
        // Header (8 bytes) + N output slots (8 bytes each) + N keyframes (24 bytes each).
        let total = RECORD_HEADER_SIZE + 8 * bone_count + 24 * bone_count;
        let mut buf = vec![0u8; total];
        // Set marker_1 = 0x080C (canonical) so any future detector matches.
        buf[4] = 0x0C;
        buf[5] = 0x08;
        // For bone 0: src_pos=(10,20,30), dst_pos=(50,60,70).
        let kf_off = RECORD_HEADER_SIZE + 8 * bone_count;
        let write_i16 = |buf: &mut [u8], off: usize, v: i16| {
            buf[off..off + 2].copy_from_slice(&v.to_le_bytes())
        };
        write_i16(&mut buf, kf_off, 10);
        write_i16(&mut buf, kf_off + 2, 20);
        write_i16(&mut buf, kf_off + 4, 30);
        write_i16(&mut buf, kf_off + 6, 50);
        write_i16(&mut buf, kf_off + 8, 60);
        write_i16(&mut buf, kf_off + 10, 70);
        buf
    }

    #[test]
    fn tick_lerps_bone_pose_toward_destination() {
        let rec = synth_record(1);
        let mut player = AnimPlayer::new(rec, 1).unwrap();
        player.frame_delta = 0x40;
        // Tick 1: factor 0 -> 0x40 (~25%). Expect pos[0] ~ 10 + (50-10)/4 = 20.
        let frame = player.tick();
        assert_eq!(frame.factor, 0x40);
        assert_eq!(frame.bone_outputs.len(), 1);
        let (pos, _rot) = frame.bone_outputs[0];
        assert_eq!(pos[0], 20);
    }

    #[test]
    fn looping_resets_factor_after_full_cycle() {
        let mut player = AnimPlayer::new(synth_record(2), 2).unwrap();
        player.frame_delta = 0x80;
        let _ = player.tick(); // factor = 0x80
        let frame = player.tick(); // factor = 0x100 -> wraps to 0
        assert!(!frame.finished);
        assert_eq!(frame.factor, 0);
    }

    #[test]
    fn non_looping_clamps_and_reports_done() {
        let mut player = AnimPlayer::new(synth_record(1), 1).unwrap();
        player.looping = false;
        player.frame_delta = 0x80;
        let _ = player.tick();
        let _ = player.tick();
        let frame = player.tick();
        assert!(frame.finished);
        assert_eq!(frame.factor, 0xFF);
    }

    #[test]
    fn rewind_resets_factor_to_zero() {
        let mut player = AnimPlayer::new(synth_record(1), 1).unwrap();
        player.frame_delta = 0x40;
        let _ = player.tick();
        assert!(player.factor() != 0);
        player.rewind();
        assert_eq!(player.factor(), 0);
    }
}

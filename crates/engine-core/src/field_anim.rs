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
//! [`crate::battle_anim::MonsterAnimPlayer`] output. A clip whose record
//! carries the blend gate is posed between two keyframes on the ticks that
//! fall inside a frame, through the same blender
//! ([`legaia_asset::player_anm::blend_bone_transform`]) the prop path samples
//! through.

use legaia_anm::PoseFrame;
use legaia_asset::player_anm::{BoneTransform, PlayerAnmBundle, blend_bone_transform};

/// The clip rate `actor+0x6A` a field actor plays at: the placed-object
/// template's `0x10` halved by `FUN_8003A55C`, and the value the player's
/// pad step stamps every frame it runs (`FUN_801D01B0`). The frame cursor is
/// in 1/16-frame units, so at this rate an ungated clip advances one frame
/// every two engine ticks (one tick = one vsync, the frame step `1`).
pub const CLIP_RATE: u16 = 8;

/// The clip step and clip-end timing live in `legaia_engine_vm::field_player_clip`
/// so the minigame crate shares them.
pub use legaia_engine_vm::field_player_clip::{clip_end_ticks, clip_step};

/// Looping playback over one field clip: all frames pre-decoded to the
/// `(translation, rotation)` pairs [`PoseFrame`] carries, walked by retail's
/// frame cursor - `1/16`-frame units, [`Self::step`] of them a tick, wrapping
/// to `0` on the tick it reaches the clip's last position.
#[derive(Debug, Clone)]
pub struct FieldClipPlayer {
    /// Per-frame, per-bone rigid transforms (`bone_outputs` rows).
    frames: Vec<Vec<([i16; 3], [i16; 3])>>,
    /// The frame cursor `actor+0x68`, in 1/16-frame units. Always a multiple
    /// of [`Self::step`]: a rewind zeroes it and every tick adds one step.
    cursor: u32,
    /// Cursor units added per engine tick ([`clip_step`]).
    step: u32,
    /// The record's blend gate ([`legaia_asset::player_anm::PlayerAnmRecord::blends`]):
    /// set, a cursor that falls inside a frame poses between it and the next.
    blend: bool,
    /// The clamp bit (`actor+0x62 & 8`) the frame blender reads on the last
    /// frame: set, a fraction there blends toward the frame itself instead of
    /// wrapping to frame 0. Only a cursor driven by [`Self::set_cursor`] (an
    /// actor whose script parks or one-shots the clip) ever sets it.
    hold_at_end: bool,
}

impl FieldClipPlayer {
    /// Pre-decode record `record_index` of a locomotion bundle. `None` when
    /// the record is out of range / malformed or carries no frames.
    ///
    /// The step is the record's own: [`clip_step`] at [`CLIP_RATE`] with the
    /// header's gate and divisor.
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
        let blend = rec.blends();
        Some(Self {
            frames,
            cursor: 0,
            step: u32::from(clip_step(CLIP_RATE, blend, (rec.flag & 0xFF) as u8)),
            blend,
            hold_at_end: false,
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

    /// Cursor units the playhead advances per engine tick (`16` = one frame a
    /// tick). See [`clip_step`].
    pub fn step(&self) -> u32 {
        self.step
    }

    /// Replace the per-tick step and restart the clip at frame 0 (the cursor
    /// must stay a multiple of the step).
    pub fn set_step(&mut self, step: u32) {
        self.step = step.max(1);
        self.rewind();
    }

    /// Engine ticks one pass of the clip takes: bind to end latch, which is
    /// also the loop period. See [`clip_end_ticks`].
    pub fn loop_ticks(&self) -> u32 {
        let span = self.frames.len() as u32 * 16;
        if span == 0 {
            return 0;
        }
        (span - 1).div_ceil(self.step.max(1))
    }

    /// Index of the frame the *next* [`Self::tick`] will emit
    /// (`cursor >> 4`, the draw walker's `frame`). A clip is a short loop
    /// over a fixed set of poses, so this doubles as a cache key for a clip
    /// that does not blend; [`Self::pose_key`] is the key that also covers
    /// the blended in-between poses.
    pub fn frame(&self) -> usize {
        (self.cursor >> 4) as usize
    }

    /// The sub-frame fraction the playhead sits at, in the blender's 1/16
    /// units: the cursor's low nibble. `0` when the clip's record does not
    /// blend - retail's gate discards the low nibble then.
    pub fn sub_frame(&self) -> u32 {
        if !self.blend {
            return 0;
        }
        self.cursor & 0xF
    }

    /// A key that changes exactly when [`Self::current_pose`] can: the frame
    /// times 16 plus [`Self::sub_frame`]. What a host memoises a posed mesh
    /// on, since [`Self::frame`] alone would alias the blended in-between
    /// poses onto their keyframe.
    pub fn pose_key(&self) -> usize {
        let held = self.hold_at_end && self.sub_frame() != 0;
        self.frame() * 16 + self.sub_frame() as usize + (usize::from(held) << 20)
    }

    /// The raw frame cursor (`actor+0x68`, 1/16-frame units).
    pub fn cursor(&self) -> u32 {
        self.cursor
    }

    /// Pose from an externally ticked cursor - the world-owned
    /// [`crate::field_env::PropAnim`] an NPC actor's `+0x62` word drives
    /// ([`crate::world::World::sync_npc_clip`]) - instead of this player's own
    /// free-running loop. `clamp` is the cursor's clamp bit (`+0x62 & 8`).
    pub fn set_cursor(&mut self, cursor: u32, clamp: bool) {
        let span = (self.frames.len() as u32 * 16).max(1);
        self.cursor = cursor.min(span - 1);
        self.hold_at_end = clamp && self.frame() + 1 >= self.frames.len();
    }

    /// Restart the clip at frame 0 (called on an idle↔walk switch so the
    /// incoming loop starts at its first keyframe).
    pub fn rewind(&mut self) {
        self.cursor = 0;
    }

    /// Emit the current frame's pose and advance the playhead (wrapping -
    /// locomotion clips loop, `finished` stays `false`).
    pub fn tick(&mut self) -> PoseFrame {
        let pose = self.current_pose();
        self.advance(1);
        pose
    }

    /// The current frame's pose WITHOUT advancing the playhead. Hosts whose
    /// render rate is decoupled from the sim rate read the pose here every
    /// redraw and call [`Self::advance`] once per *sim tick*, so the clip
    /// plays at the retail cadence regardless of display refresh rate.
    ///
    /// A blend-gated clip between keyframes is posed by retail's frame
    /// blender: toward the next frame, wrapping to frame 0 after the last (the
    /// loop arm of the next-entry rule - these clips loop).
    ///
    /// REF: FUN_8001BE80
    pub fn current_pose(&self) -> PoseFrame {
        let frac = self.sub_frame();
        let frame = self.frame().min(self.frames.len() - 1);
        let bone_outputs = if frac == 0 {
            self.frames[frame].clone()
        } else {
            // The blender's next-entry rule: wrap to frame 0 after the last,
            // unless the clamp bit holds the clip there.
            let next = if self.hold_at_end && frame + 1 == self.frames.len() {
                &self.frames[frame]
            } else {
                &self.frames[(frame + 1) % self.frames.len()]
            };
            self.frames[frame]
                .iter()
                .zip(next)
                .map(|(c, n)| {
                    let t = blend_bone_transform(row_bone(c), row_bone(n), frac as i32);
                    (
                        [t.t_x as i16, t.t_y as i16, t.t_z as i16],
                        [t.r_x as i16, t.r_y as i16, t.r_z as i16],
                    )
                })
                .collect()
        };
        PoseFrame {
            bone_outputs,
            factor: 0,
            finished: false,
        }
    }

    /// Advance the playhead by `n` engine ticks (`0` = hold the frame). O(1)
    /// in `n`; equivalent to `n` post-emit advances of [`Self::tick`].
    ///
    /// Each tick adds the step, and the tick that reaches the clip's last
    /// position (`frames * 16 - 1`) wraps the cursor to `0` rather than
    /// carrying the overshoot (`FUN_800204F8`'s loop arm stores zero), so the
    /// cursor walks `0, step, 2*step, ...` with period [`Self::loop_ticks`].
    pub fn advance(&mut self, n: u32) {
        if n == 0 || self.frames.is_empty() {
            return;
        }
        self.hold_at_end = false;
        let step = self.step.max(1);
        let period = u64::from(self.loop_ticks().max(1));
        let k = (u64::from(self.cursor / step) + u64::from(n)) % period;
        self.cursor = k as u32 * step;
    }
}

/// A pre-decoded `(translation, rotation)` row back as the decoder's type.
/// Lossless: the row holds the decode's 16-bit values verbatim.
fn row_bone(r: &([i16; 3], [i16; 3])) -> BoneTransform {
    BoneTransform {
        t_x: r.0[0].into(),
        t_y: r.0[1].into(),
        t_z: r.0[2].into(),
        r_x: r.1[0].into(),
        r_y: r.1[1].into(),
        r_z: r.1[2].into(),
    }
}

/// The player's live idle/walk clip pair plus the per-tick movement signal
/// locomotion feeds it. Hosts install one via
/// [`crate::world::World::set_field_player_anim`]; [`crate::world::World`]
/// ticks it after the locomotion step each field frame and folds the output
/// into the player actor's `pose_frame`.
///
/// Built by [`Self::from_locomotion_bank`] it also carries the leader's whole
/// seven-record bank, and the settle tail
/// ([`crate::world::World::step_field_vertical`], retail `FUN_801D1BA0`)
/// selects the slot retail's clip base names each frame through
/// [`Self::select_retail_slot`] - the run clip, the hop's two clips, and the
/// walk-in-place a warp holds, none of which the idle/walk pair can express.
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
    /// Set by the pad step on every frame it ran and wrote the clip base -
    /// the frames whose clip retail's pick owns. A frame that moved the
    /// player without it (a cutscene `MoveTo`, a channel walk-on) keeps the
    /// motion-derived walk instead. Consumed and cleared by the anim tick.
    pub pad_drove_this_frame: bool,
    /// Queued scripted one-shot clips (field-VM `A2 F8 <move_id>` ExecMove
    /// pokes against the player). Each plays through exactly once - front of
    /// the queue first - overriding idle/walk; the retail player actor's
    /// `+0x4C` anim pointer does the same (live-pinned across the `town01`
    /// post-naming beats: ExecMove 48/49 point it at scene-bundle records
    /// 47/48 for one playthrough each, then locomotion clips resume).
    pub scripted: std::collections::VecDeque<FieldClipPlayer>,
    /// Engine ticks left on the front scripted clip.
    scripted_ticks_left: u32,
    /// The leader's bank, indexed by bank slot (`0..7`); empty for a pair
    /// built with [`Self::new`].
    bank: Vec<Option<FieldClipPlayer>>,
    /// Party leader the bank belongs to (retail `_DAT_8007B8F8`).
    pub leader: u16,
    /// The bank slot the settle tail last picked, when it picked a
    /// party-bank clip.
    retail_slot: Option<usize>,
    /// The bank slot currently playing, for the rewind-on-change rule
    /// (`FUN_800204F8` rewinds when `+0x5C != +0x5E`).
    playing_slot: Option<usize>,
    /// The **scene-bank** record the settle tail last picked (0-based), when
    /// its pick bound from the scene's own ANM bundle rather than the party
    /// bank: the op-`4C CE` override arm, the `99` scene sentinel, or an
    /// actor whose party-bank bit is down. See [`Self::select_scene_record`].
    scene_record: Option<u16>,
    /// The scene-bank clip the host resolved for [`Self::scene_record`] -
    /// `(record, clip)` - through [`Self::resolve_scene_clip`].
    scene_clip: Option<(u16, FieldClipPlayer)>,
    /// Whether the scene clip was the one playing last tick (rewind rule).
    playing_scene: Option<u16>,
}

impl FieldPlayerAnim {
    pub fn new(idle: FieldClipPlayer, walk: FieldClipPlayer) -> Self {
        Self {
            idle,
            walk,
            walking: false,
            moved_this_frame: false,
            pad_drove_this_frame: false,
            scripted: std::collections::VecDeque::new(),
            scripted_ticks_left: 0,
            bank: Vec::new(),
            leader: 0,
            retail_slot: None,
            playing_slot: None,
            scene_record: None,
            scene_clip: None,
            playing_scene: None,
        }
    }

    /// Build the player's clips from the party locomotion bundle: the
    /// capture-pinned idle / walk pair plus every record of `leader`'s bank.
    /// `None` when the bundle lacks the leader's idle or walk record.
    ///
    /// Both play hosts build the player's clips here, so a bank slot one host
    /// can play the other can too.
    pub fn from_locomotion_bank(bundle: &PlayerAnmBundle, leader: usize) -> Option<Self> {
        use legaia_asset::character_pack::{
            LOCOMOTION_BANK_STRIDE, LOCOMOTION_IDLE_SLOT, LOCOMOTION_WALK_SLOT,
            locomotion_record_index,
        };
        let rec = |slot| locomotion_record_index(leader, slot);
        let idle = FieldClipPlayer::from_record(bundle, rec(LOCOMOTION_IDLE_SLOT))?;
        let walk = FieldClipPlayer::from_record(bundle, rec(LOCOMOTION_WALK_SLOT))?;
        let mut anim = Self::new(idle, walk);
        anim.leader = leader as u16;
        anim.bank = (0..LOCOMOTION_BANK_STRIDE)
            .map(|slot| FieldClipPlayer::from_record(bundle, rec(slot)))
            .collect();
        Some(anim)
    }

    /// Whether bank slot `slot` has a playable clip.
    pub fn has_bank_slot(&self, slot: usize) -> bool {
        self.bank.get(slot).is_some_and(Option::is_some)
    }

    /// The settle tail's pick for this frame: a party-bank slot, or `None`
    /// when the pick did not land in the leader's bank (the scene-sentinel
    /// clip, a zero clip), which falls back to the motion-derived pair.
    pub fn select_retail_slot(&mut self, slot: Option<usize>) {
        self.retail_slot = slot;
    }

    /// The bank slot the settle tail last picked.
    pub fn retail_slot(&self) -> Option<usize> {
        self.retail_slot
    }

    /// The settle tail's pick when it binds from the **scene** bank
    /// (`FUN_800204F8` with the party-bank bit down, `+0x5C < 0x400`):
    /// record `clip - 1` of the scene's own ANM bundle. `None` for every
    /// other pick. The host resolves the record through
    /// [`Self::resolve_scene_clip`], since the scene bundle is host-owned.
    pub fn select_scene_record(&mut self, record: Option<u16>) {
        self.scene_record = record;
    }

    /// The scene-bank record the last pick asked for.
    pub fn scene_record(&self) -> Option<u16> {
        self.scene_record
    }

    /// Load the scene-bank clip the pick asked for from `bundle` (the scene's
    /// own ANM bundle, retail `*(0x8007B888)`), once per record change. A
    /// record whose bone count differs from the player's locomotion clips is
    /// refused - a pose for another skeleton would tear the mesh - and the
    /// pick then falls back to the motion-derived pair. Both play hosts call
    /// this once per frame with the bundle they bind scripted clips from.
    pub fn resolve_scene_clip(&mut self, bundle: &PlayerAnmBundle) {
        let Some(record) = self.scene_record else {
            return;
        };
        if self.scene_clip.as_ref().is_some_and(|(r, _)| *r == record) {
            return;
        }
        self.scene_clip = FieldClipPlayer::from_record(bundle, record as usize)
            .filter(|c| c.bone_count() == self.idle.bone_count())
            .map(|c| (record, c));
    }

    /// Queue a scripted one-shot clip (an `A2 F8` ExecMove resolution). The
    /// clip starts at frame 0 when it reaches the front of the queue and
    /// plays [`FieldClipPlayer::loop_ticks`] engine ticks - its own step, so
    /// a gated clip's pass takes as long as retail's end latch does.
    pub fn push_scripted(&mut self, mut clip: FieldClipPlayer) {
        clip.rewind();
        self.scripted.push_back(clip);
    }

    /// `true` while a scripted one-shot is playing or queued.
    pub fn scripted_active(&self) -> bool {
        !self.scripted.is_empty()
    }

    /// Frame count of whichever locomotion clip is playing now.
    pub fn active_frame_count(&self) -> usize {
        if let Some((_, clip)) = self
            .scene_clip
            .as_ref()
            .filter(|(r, _)| self.playing_scene == Some(*r))
        {
            return clip.frame_count();
        }
        if let Some(clip) = self
            .playing_slot
            .and_then(|s| self.bank.get(s))
            .and_then(Option::as_ref)
        {
            return clip.frame_count();
        }
        if self.walking {
            self.walk.frame_count()
        } else {
            self.idle.frame_count()
        }
    }

    /// One field tick: a queued scripted one-shot takes priority (playing
    /// through once, then handing back); then the settle tail's retail pick
    /// on the frames the pad step owned; otherwise switch idle/walk clips on
    /// a movement-state edge (rewinding the incoming clip) and emit the
    /// active clip's pose.
    pub fn tick(&mut self) -> PoseFrame {
        let moved = std::mem::take(&mut self.moved_this_frame);
        let pad_drove = std::mem::take(&mut self.pad_drove_this_frame);
        if let Some(front) = self.scripted.front_mut() {
            if self.scripted_ticks_left == 0 {
                // Freshly-promoted front clip: arm its full playthrough.
                self.scripted_ticks_left = front.loop_ticks().max(1);
            }
            let pose = front.tick();
            self.scripted_ticks_left = self.scripted_ticks_left.saturating_sub(1);
            if self.scripted_ticks_left == 0 {
                self.scripted.pop_front();
                // Restart whichever locomotion loop resumes underneath.
                self.idle.rewind();
                self.walk.rewind();
                self.playing_slot = None;
            }
            return pose;
        }
        // A move nobody's pad made (a script walked the player) keeps the
        // motion-derived walk: retail would hold whatever base the script
        // last set, which the port does not track for script moves.
        let script_moved = moved && !pad_drove;
        // A scene-bank pick binds every frame it is the pick, whoever moved
        // the player: retail's `FUN_800204F8` binds whatever the settle
        // picked, and a scene-bank id is never the motion pair's to replace.
        if let Some(record) = self.scene_record
            && let Some((r, clip)) = self.scene_clip.as_mut()
            && *r == record
        {
            if self.playing_scene != Some(record) {
                clip.rewind();
            }
            self.playing_scene = Some(record);
            self.playing_slot = None;
            return clip.tick();
        }
        if self.playing_scene.take().is_some() {
            // Leaving the scene clip: the next pick restarts from frame 0.
            self.idle.rewind();
            self.walk.rewind();
        }
        if !script_moved && let Some(slot) = self.retail_slot.filter(|&s| self.has_bank_slot(s)) {
            let rewind = self.playing_slot != Some(slot);
            self.playing_slot = Some(slot);
            self.walking = slot == legaia_asset::character_pack::LOCOMOTION_WALK_SLOT;
            let clip = self.bank[slot].as_mut().expect("has_bank_slot");
            if rewind {
                clip.rewind();
            }
            return clip.tick();
        }
        if self.playing_slot.take().is_some() {
            // Leaving the bank: the pair restarts from the edge below.
            self.walking = !moved;
        }
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

/// Test fixture: a synthetic ANM bundle whose record `r` has `bones[r]`
/// bones and `frames[r]` frames, each frame's bone `b` tagging `t_x` with
/// `r * 100 + f * 10 + b` (low byte).
#[cfg(test)]
pub(crate) fn synth_anm_bundle(records: &[(u16, u8)]) -> PlayerAnmBundle {
    use legaia_asset::player_anm::{ANM_MARKER_1, parse};
    let mut buf = Vec::new();
    buf.extend_from_slice(&(records.len() as u32).to_le_bytes());
    let mut off = (4 + 4 * records.len()) as u32;
    for &(bones, frames) in records {
        buf.extend_from_slice(&off.to_le_bytes());
        off += (8 + 8 * bones as usize * frames as usize + 8) as u32;
    }
    for (rec, &(bones, frames)) in records.iter().enumerate() {
        buf.extend_from_slice(&bones.to_le_bytes());
        buf.extend_from_slice(&(frames as u16).to_le_bytes());
        buf.extend_from_slice(&ANM_MARKER_1.to_le_bytes());
        buf.extend_from_slice(&0x0002u16.to_le_bytes());
        for f in 0..frames {
            for b in 0..bones {
                let tag = (rec as u32 * 100 + f as u32 * 10 + b as u32) as u8;
                buf.extend_from_slice(&[tag, 0, 0, 0, 0, 0, 0, 0]);
            }
        }
        buf.extend_from_slice(&[0u8; 8]);
    }
    parse(&buf).expect("synthetic bundle parses")
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
        p.set_step(16);
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

    /// `advance(n)` must land exactly where `n` sequential `tick()`s land,
    /// including the sub-frame counter, for every phase of the clip - the
    /// contract that makes the host's "pose per redraw, advance per sim tick"
    /// split equivalent to the old per-tick emit at a 1:1 tick:redraw ratio.
    #[test]
    fn advance_matches_sequential_ticks() {
        let bundle = synth_bundle();
        for step in [16u32, 8, 6, 4, 3] {
            for n in 0..20u32 {
                let mut seq = FieldClipPlayer::from_record(&bundle, 0).unwrap();
                seq.set_step(step);
                let mut jump = seq.clone();
                for _ in 0..n {
                    let _ = seq.tick();
                }
                jump.advance(n);
                assert_eq!(
                    seq.cursor, jump.cursor,
                    "cursor after {n} ticks (step={step})"
                );
                assert_eq!(
                    seq.current_pose().bone_outputs,
                    jump.current_pose().bone_outputs
                );
            }
        }
    }

    /// `current_pose` is a pure read: it never moves the playhead, and
    /// `advance(0)` holds the frame.
    #[test]
    fn current_pose_does_not_advance() {
        let bundle = synth_bundle();
        let mut p = FieldClipPlayer::from_record(&bundle, 0).unwrap();
        p.set_step(16);
        let a = p.current_pose();
        let b = p.current_pose();
        assert_eq!(a.bone_outputs, b.bone_outputs);
        assert_eq!(p.frame(), 0);
        p.advance(0);
        assert_eq!(p.frame(), 0);
        p.advance(1);
        assert_eq!(p.frame(), 1);
    }

    #[test]
    fn ticks_per_frame_holds_frames() {
        let bundle = synth_bundle();
        let mut p = FieldClipPlayer::from_record(&bundle, 0).unwrap();
        p.set_step(8);
        assert_eq!(p.tick().bone_outputs[0].0[0], 0);
        assert_eq!(p.tick().bone_outputs[0].0[0], 0);
        assert_eq!(p.tick().bone_outputs[0].0[0], 10);
    }

    #[test]
    fn a_blend_gated_clip_poses_the_in_between_tick() {
        let mut bundle = synth_bundle();
        // Raise record 0's blend gate (clip byte +1 bit 0).
        bundle.decoded[bundle.record_offsets[0] as usize + 1] |= 1;
        let mut p = FieldClipPlayer::from_record(&bundle, 0).unwrap();
        p.set_step(8);
        let mut seen = Vec::new();
        let mut keys = Vec::new();
        for _ in 0..6 {
            keys.push(p.pose_key());
            seen.push(p.tick().bone_outputs[0].0[0]);
        }
        // Keyframes 0 / 10 / 20 exact on the even ticks; half-way between on
        // the odd ones, the last one wrapping toward frame 0.
        assert_eq!(seen, vec![0, 5, 10, 15, 20, 10]);
        assert_eq!(keys, vec![0, 8, 16, 24, 32, 40]);
        // The ungated record keeps whole frames and frame-only keys.
        let mut q = FieldClipPlayer::from_record(&bundle, 1).unwrap();
        q.set_step(8);
        q.advance(1);
        assert_eq!(q.pose_key(), 0);
        assert_eq!(q.current_pose().bone_outputs[0].0[0], 100);
    }

    /// The step select of `FUN_800204F8` at the template rate: ungated
    /// clips step by the rate, gated ones by `(rate*2 + div - 1) / div`.
    #[test]
    fn clip_step_matches_the_retail_select() {
        assert_eq!(
            clip_step(CLIP_RATE, false, 4),
            8,
            "ungated ignores the divisor"
        );
        let gated: Vec<u16> = [1u8, 2, 3, 4, 6]
            .iter()
            .map(|&d| clip_step(CLIP_RATE, true, d))
            .collect();
        assert_eq!(gated, vec![16, 8, 6, 4, 3]);
        assert_eq!(clip_step(CLIP_RATE, true, 0), CLIP_RATE, "div 0 falls back");
        // A script's `4C 41` rate (`16` = a door's one frame a tick).
        assert_eq!(clip_step(16, true, 4), 8);
    }

    /// The end latch lands on the first tick whose cursor reaches the last
    /// position; at the ungated step that is the old two-ticks-a-frame length,
    /// at divisor 4 twice it, and the player's loop period agrees.
    #[test]
    fn clip_end_ticks_is_the_first_tick_at_the_last_position() {
        for frames in 1u16..=20 {
            for step in [16u16, 8, 6, 4, 3] {
                let last = u32::from(frames) * 16 - 1;
                let mut cursor = 0u32;
                let mut ticks = 0u32;
                loop {
                    cursor += u32::from(step);
                    ticks += 1;
                    if cursor >= last {
                        break;
                    }
                }
                assert_eq!(
                    clip_end_ticks(frames, step),
                    ticks,
                    "{frames} frames, step {step}"
                );
            }
            assert_eq!(clip_end_ticks(frames, 8), u32::from(frames) * 2);
            assert_eq!(clip_end_ticks(frames, 4), u32::from(frames) * 4);
        }
        assert_eq!(clip_end_ticks(0, 8), 0);
    }

    /// A gated divisor-4 record plays at half the ungated speed: four ticks a
    /// frame, the three in-between ticks posed a quarter, a half and three
    /// quarters of the way to the next frame.
    #[test]
    fn a_divisor_four_gated_clip_plays_at_half_speed() {
        let mut bundle = synth_bundle();
        let off = bundle.record_offsets[0] as usize;
        bundle.decoded[off + 1] |= 1; // the gate
        bundle.decoded[off + 6] = 4; // the divisor
        let mut p = FieldClipPlayer::from_record(&bundle, 0).unwrap();
        assert_eq!(p.step(), 4);
        assert_eq!(p.loop_ticks(), 12, "3 frames x 4 ticks");
        let seen: Vec<i16> = (0..13).map(|_| p.tick().bone_outputs[0].0[0]).collect();
        // Keyframes 0 / 10 / 20 on every fourth tick; frame 2 blends toward
        // frame 0 (the loop wrap) before the cursor returns to 0.
        assert_eq!(seen, vec![0, 2, 5, 7, 10, 12, 15, 17, 20, 15, 10, 5, 0]);
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
        idle.set_step(16);
        walk.set_step(16);
        w.set_field_player_anim(Some(FieldPlayerAnim::new(idle, walk)));
        // Standing frame: idle clip pose lands in the player's pose_frame.
        w.set_pad(0);
        let _ = w.tick();
        let pose = w.actors[0].pose_frame.clone().expect("idle pose set");
        assert_eq!(pose.bone_outputs[0].0[0], 100, "idle record tag");
        assert!(!w.locomotion.player_anim.as_ref().unwrap().walking);
        // Held direction: locomotion flags the move, the walk clip plays.
        w.set_pad(crate::input::PadButton::Up.mask());
        let _ = w.tick();
        let pose = w.actors[0].pose_frame.clone().expect("walk pose set");
        assert_eq!(pose.bone_outputs[0].0[0], 0, "walk record restarts");
        assert!(w.locomotion.player_anim.as_ref().unwrap().walking);
        // Release: back to idle, restarted at frame 0.
        w.set_pad(0);
        let _ = w.tick();
        let pose = w.actors[0].pose_frame.clone().expect("idle pose set");
        assert_eq!(pose.bone_outputs[0].0[0], 100);
        assert!(!w.locomotion.player_anim.as_ref().unwrap().walking);
    }

    /// A scripted one-shot (ExecMove) overrides idle/walk for exactly one
    /// playthrough per queued clip, then locomotion resumes from frame 0 -
    /// the retail post-naming shape (clip 47 then 48, then idle/walk).
    #[test]
    fn scripted_one_shots_play_once_in_order_then_locomotion_resumes() {
        let bundle = synth_bundle();
        let mut idle = FieldClipPlayer::from_record(&bundle, 1).unwrap();
        let mut walk = FieldClipPlayer::from_record(&bundle, 0).unwrap();
        idle.set_step(16);
        walk.set_step(16);
        let mut anim = FieldPlayerAnim::new(idle, walk);
        // Queue the "walk" record (tags 0/10/20) then the "idle" record
        // (tags 100/110) as scripted clips.
        let mut a = FieldClipPlayer::from_record(&bundle, 0).unwrap();
        a.set_step(16);
        let mut b = FieldClipPlayer::from_record(&bundle, 1).unwrap();
        b.set_step(16);
        anim.push_scripted(a);
        anim.push_scripted(b);
        assert!(anim.scripted_active());
        // First clip: 3 frames exactly once.
        for expect in [0i16, 10, 20] {
            assert_eq!(anim.tick().bone_outputs[0].0[0], expect);
        }
        // Second clip: 2 frames.
        for expect in [100i16, 110] {
            assert_eq!(anim.tick().bone_outputs[0].0[0], expect);
        }
        assert!(!anim.scripted_active());
        // Idle loop resumes from frame 0.
        assert_eq!(anim.tick().bone_outputs[0].0[0], 100);
    }

    #[test]
    fn player_anim_switches_clips_on_move_edge() {
        let bundle = synth_bundle();
        let mut idle = FieldClipPlayer::from_record(&bundle, 1).unwrap();
        let mut walk = FieldClipPlayer::from_record(&bundle, 0).unwrap();
        idle.set_step(16);
        walk.set_step(16);
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

    #[test]
    fn a_scene_bank_pick_plays_the_scene_record_once_resolved() {
        // Party bank: walk (rec 0) / idle (rec 1), two bones each.
        let party = synth_anm_bundle(&[(2, 3), (2, 2)]);
        let idle = FieldClipPlayer::from_record(&party, 1).unwrap();
        let walk = FieldClipPlayer::from_record(&party, 0).unwrap();
        let mut anim = FieldPlayerAnim::new(idle, walk);
        // Scene bundle: record 0 has 3 bones (another skeleton), record 1
        // two bones and 4 frames.
        let scene = synth_anm_bundle(&[(3, 2), (2, 4)]);
        anim.select_scene_record(Some(1));
        // Nothing resolved yet: the motion pair still plays.
        assert_eq!(anim.tick().bone_outputs[0].0[0], 100);
        anim.resolve_scene_clip(&scene);
        // The scene record plays from frame 0 (tag 100 + 0 = record 1).
        let pose = anim.tick();
        assert_eq!(pose.bone_outputs[0].0[0], 100);
        assert_eq!(anim.active_frame_count(), 4);
        anim.tick();
        assert_eq!(anim.tick().bone_outputs[0].0[0], 110, "second frame");
        // A record for another skeleton is refused; the pair takes over.
        anim.select_scene_record(Some(0));
        anim.resolve_scene_clip(&scene);
        assert_eq!(anim.tick().bone_outputs.len(), 2);
        assert_eq!(anim.active_frame_count(), 2, "back on the idle clip");
        // Dropping the scene pick hands back to the motion pair too.
        anim.select_scene_record(None);
        assert_eq!(anim.tick().bone_outputs.len(), 2);
    }
}

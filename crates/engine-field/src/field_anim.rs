//! Field-player locomotion clip playback.
//!
//! Drives the party locomotion ANM bundle (PROT 0874 §1,
//! [`legaia_actor_formats::character_pack::field_locomotion_anm`]) at runtime: the
//! standing **idle** loop (bank slot
//! [`legaia_actor_formats::character_pack::LOCOMOTION_IDLE_SLOT`]) while the player
//! stands, the **walk** loop (bank slot
//! [`legaia_actor_formats::character_pack::LOCOMOTION_WALK_SLOT`]) while pad / nav
//! locomotion moves the player - the same two records the retail player
//! actor's `+0x4C` anim pointer alternates between (pinned live via
//! `scripts/pcsx-redux/autorun_locomotion_clip_pin.lua`).
//!
//! REF: FUN_8001B964 - the retail per-actor animated renderer walks the
//! record's per-(bone, frame) 8-byte entries each draw; the per-entry decode
//! is ported at [`legaia_actor_formats::player_anm::BoneTransform::decode`]
//! (`FUN_8001BE80`). This module owns the playhead: it pre-decodes every
//! frame of a clip and emits one [`PoseFrame`] per engine tick, which the
//! host's posed-mesh rebuild consumes exactly like the battle
//! `legaia_engine_battle::battle_anim::MonsterAnimPlayer` output. A clip whose record
//! carries the blend gate is posed between two keyframes on the ticks that
//! fall inside a frame, through the same blender
//! ([`legaia_actor_formats::player_anm::blend_bone_transform`]) the prop path samples
//! through.

use legaia_actor_formats::player_anm::{BoneTransform, PlayerAnmBundle, blend_bone_transform};
use legaia_anm::PoseFrame;

/// The clip rate `actor+0x6A` a field actor plays at: the placed-object
/// template's `0x10` halved by `FUN_8003A55C`, and the value the player's
/// pad step stamps every frame it runs (`FUN_801D01B0`). The frame cursor is
/// in 1/16-frame units, so at this rate an ungated clip advances one frame
/// every two engine ticks (one tick = one vsync, the frame step `1`).
pub const CLIP_RATE: u16 = 8;

use legaia_engine_vm::actor_tick::{CLIP_CLAMP, clip_cursor_tick, clip_step};

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
    /// The record's blend gate ([`legaia_actor_formats::player_anm::PlayerAnmRecord::blends`]):
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
    /// also the loop period. See [`legaia_engine_vm::actor_tick::clip_end_ticks`].
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
    /// `legaia_engine_scene::field_env::PropAnim` an NPC actor's `+0x62` word drives
    /// (`legaia_engine_core::world::World::sync_npc_clip`) - instead of this player's own
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

    /// Emit the current pose, then advance one clip tick `scale` vsyncs long
    /// under the actor's control word `flags` (`actor+0x62`) - retail's
    /// cursor rules: restart, hold, reverse, and the clamp that stops a
    /// one-shot on its last position instead of wrapping it. Returns the pose
    /// and the word the tick leaves (the end latch `0x100` raised on the tick
    /// the cursor reaches an end, a restart request consumed).
    ///
    /// REF: FUN_800204F8 (`legaia_engine_vm::actor_tick::clip_cursor_tick`)
    pub fn tick_with(&mut self, scale: u16, flags: u16) -> (PoseFrame, u16) {
        let pose = self.current_pose();
        let flags = self.advance_with(scale, flags);
        (pose, flags)
    }

    /// The advance half of [`Self::tick_with`].
    pub fn advance_with(&mut self, scale: u16, flags: u16) -> u16 {
        let frames = u16::try_from(self.frames.len()).unwrap_or(u16::MAX);
        if frames == 0 {
            return flags;
        }
        let step = (self.step.max(1) * u32::from(scale.max(1))).min(i16::MAX as u32) as i16;
        let (cursor, out) = clip_cursor_tick(self.cursor as i16, flags, step, frames);
        self.cursor = cursor.max(0) as u32;
        self.hold_at_end = out & CLIP_CLAMP != 0 && self.frame() + 1 >= self.frames.len();
        out
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
/// `legaia_engine_core::world::World::set_field_player_anim`; `legaia_engine_core::world::World`
/// ticks it after the locomotion step each field frame and folds the output
/// into the player actor's `pose_frame`.
///
/// Built by [`Self::from_locomotion_bank`] it also carries the leader's whole
/// seven-record bank, and the settle tail
/// (`legaia_engine_core::world::World::step_field_vertical`, retail `FUN_801D1BA0`)
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
    /// Ticks the motion-derived walk outlives the last script-made move
    /// ([`SCRIPT_WALK_HOLD`]). A script moves the player on game-frame
    /// boundaries, every other vsync, and without the hold the pair flipped
    /// between walk and the pick on alternate ticks, rewinding each time.
    script_walk_hold: u8,
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
            script_walk_hold: 0,
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
        use legaia_actor_formats::character_pack::{
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

    /// A script poked scene-bank clip `record + 1` onto the player
    /// (`A2 F8 <move_id>` with the party-bank bit down, which the pick also
    /// names through [`Self::select_scene_record`]): bind `clip` as that
    /// record's player and restart it from frame 0 on the next tick.
    ///
    /// There is no queue and no fixed playthrough: the clip is the scene pick
    /// like any other, so it runs under the player's control word - a script
    /// that raised the clamp first (`AB F8 03`, then `A2 F8 <id>`, then
    /// `AD F8 08`) gets one pass held on its last frame, exactly as retail's
    /// `FUN_800204F8` holds it, and the next pick (`A2 F8 02`, or the pad's
    /// own base once control returns) replaces it. The town01 post-naming
    /// beats (ExecMove 48 / 49 onto scene records 47 / 48, then locomotion)
    /// are two picks in a row on that rule.
    pub fn bind_scripted(&mut self, record: u16, mut clip: FieldClipPlayer) {
        clip.rewind();
        self.scene_clip = Some((record, clip));
        self.playing_scene = None;
    }

    /// Which clip plays now and where its cursor stands: `(source, id,
    /// cursor, frame_count)`, `source` one of `scene` / `bank` / `walk` /
    /// `idle`. A diagnostic view for tests and traces.
    pub fn playing_clip(&self) -> (&'static str, usize, u32, usize) {
        if let Some((r, c)) = self
            .scene_clip
            .as_ref()
            .filter(|(r, _)| self.playing_scene == Some(*r))
        {
            return ("scene", usize::from(*r), c.cursor(), c.frame_count());
        }
        if let Some((s, c)) = self
            .playing_slot
            .and_then(|s| self.bank.get(s).and_then(Option::as_ref).map(|c| (s, c)))
        {
            return ("bank", s, c.cursor(), c.frame_count());
        }
        if self.walking {
            ("walk", 0, self.walk.cursor(), self.walk.frame_count())
        } else {
            ("idle", 0, self.idle.cursor(), self.idle.frame_count())
        }
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

    /// One field tick with the player's control word at its template state
    /// (looping playback); see [`Self::tick_with_flags`].
    pub fn tick(&mut self) -> PoseFrame {
        self.tick_with_flags(0).0
    }

    /// One field tick: the settle tail's retail pick - a scene-bank record,
    /// else a party-bank slot - on the frames it owns; otherwise switch the
    /// motion-derived idle/walk pair on a movement-state edge (rewinding the
    /// incoming clip). Emits the active clip's pose and the control word
    /// `flags` (the player's `actor+0x62`) as the picked clip's tick leaves
    /// it.
    ///
    /// A picked clip advances under `flags` the way retail's `FUN_800204F8`
    /// advances whatever the pick bound: a clamped clip plays once and holds
    /// its last frame (the ledge hop's take-off and landing clips, a
    /// cutscene's one-shot gesture), a held one freezes, the rest loop. The
    /// motion pair is the port's own stand-in for frames retail's pick does
    /// not own, and always loops.
    ///
    /// REF: FUN_800204F8
    pub fn tick_with_flags(&mut self, flags: u16) -> (PoseFrame, u16) {
        let moved = std::mem::take(&mut self.moved_this_frame);
        let pad_drove = std::mem::take(&mut self.pad_drove_this_frame);
        // A move nobody's pad made (a script walked the player) plays the
        // motion-derived walk over a standing pick: retail would hold the
        // idle the base still names, and the port walks the body instead.
        // The hold bridges the vsyncs between a script's moves.
        if moved && !pad_drove {
            self.script_walk_hold = SCRIPT_WALK_HOLD;
        } else if pad_drove {
            self.script_walk_hold = 0;
        } else {
            self.script_walk_hold = self.script_walk_hold.saturating_sub(1);
        }
        let script_walking = !pad_drove && self.script_walk_hold > 0;
        let moving = moved || script_walking;
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
            return clip.tick_with(1, flags);
        }
        if self.playing_scene.take().is_some() {
            // Leaving the scene clip: the next pick restarts from frame 0.
            self.idle.rewind();
            self.walk.rewind();
        }
        // The pick's own clip, unless a script is walking a standing body.
        // A moving pick - the run a cutscene's `A2 F8 03` names, the hop's
        // take-off and landing - is retail's clip whoever moved the player.
        let slot = self
            .retail_slot
            .filter(|&s| self.has_bank_slot(s))
            .filter(|&s| {
                !script_walking || s != legaia_actor_formats::character_pack::LOCOMOTION_IDLE_SLOT
            });
        if let Some(slot) = slot {
            let rewind = self.playing_slot != Some(slot);
            self.playing_slot = Some(slot);
            self.walking = slot == legaia_actor_formats::character_pack::LOCOMOTION_WALK_SLOT;
            let clip = self.bank[slot].as_mut().expect("has_bank_slot");
            if rewind {
                clip.rewind();
            }
            return clip.tick_with(1, flags);
        }
        if self.playing_slot.take().is_some() {
            // Leaving the bank: the pair restarts from the edge below.
            self.walking = !moving;
        }
        if moving != self.walking {
            self.walking = moving;
            if moving {
                self.walk.rewind();
            } else {
                self.idle.rewind();
            }
        }
        let pose = if self.walking {
            self.walk.tick()
        } else {
            self.idle.tick()
        };
        (pose, flags)
    }
}

/// Ticks [`FieldPlayerAnim`]'s motion-derived walk outlives a script-made
/// move: one game frame of two vsyncs, so a script stepping the player every
/// other vsync reads as one continuous walk.
pub const SCRIPT_WALK_HOLD: u8 = 2;

/// Test fixture: a synthetic ANM bundle whose record `r` has `bones[r]`
/// bones and `frames[r]` frames, each frame's bone `b` tagging `t_x` with
/// `r * 100 + f * 10 + b` (low byte).
#[doc(hidden)]
pub fn synth_anm_bundle(records: &[(u16, u8)]) -> PlayerAnmBundle {
    use legaia_actor_formats::player_anm::{ANM_MARKER_1, parse};
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
    use legaia_actor_formats::player_anm::{ANM_MARKER_1, parse};

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

    /// A script's one-shot on the player (`AB F8 03`, `A2 F8 <id>`): the
    /// clamped clip plays once, holds its last frame with the end latch up
    /// for as long as the pick names it, and hands back on the next pick.
    /// Without the clamp the same clip loops. (The cutscene shape: `cave01`
    /// `P2[2]` raises the clamp, pokes Noa's getting-up clip and waits on
    /// the latch, with dialogue in between.)
    #[test]
    fn a_clamped_scripted_clip_plays_once_and_holds_until_the_next_pick() {
        use legaia_engine_vm::actor_tick::CLIP_END;
        let bundle = synth_bundle();
        let mut idle = FieldClipPlayer::from_record(&bundle, 1).unwrap();
        let mut walk = FieldClipPlayer::from_record(&bundle, 0).unwrap();
        idle.set_step(16);
        walk.set_step(16);
        let mut anim = FieldPlayerAnim::new(idle, walk);
        let mut clip = FieldClipPlayer::from_record(&bundle, 0).unwrap();
        clip.set_step(16);
        anim.select_scene_record(Some(0));
        anim.bind_scripted(0, clip.clone());
        let mut flags = CLIP_CLAMP;
        let mut seen = Vec::new();
        let mut latched = Vec::new();
        for _ in 0..8 {
            let (pose, out) = anim.tick_with_flags(flags);
            flags = out;
            seen.push(pose.bone_outputs[0].0[0]);
            latched.push(flags & CLIP_END != 0);
        }
        // Frames 0 / 10 / 20 once, then frame 2 held; the latch lands on the
        // tick the cursor reaches the end and stays up while it rests there.
        assert_eq!(seen, vec![0, 10, 20, 20, 20, 20, 20, 20]);
        assert_eq!(
            latched,
            vec![false, false, true, true, true, true, true, true]
        );
        // The next pick (the script's `A2 F8 02`) hands back to locomotion.
        anim.select_scene_record(None);
        flags &= !CLIP_CLAMP;
        assert_eq!(anim.tick_with_flags(flags).0.bone_outputs[0].0[0], 100);
        // Unclamped, the same poke loops.
        anim.select_scene_record(Some(0));
        anim.bind_scripted(0, clip);
        let seen: Vec<i16> = (0..5)
            .map(|_| anim.tick_with_flags(0).0.bone_outputs[0].0[0])
            .collect();
        assert_eq!(seen, vec![0, 10, 20, 0, 10]);
    }

    /// The ledge hop's take-off clip is the pick on frames the arc - not the
    /// pad - moves the player, and the clamp the hop raises holds it on its
    /// last frame instead of looping it; a script walking a standing body
    /// still gets the motion-derived walk, held across the vsyncs between
    /// its steps.
    #[test]
    fn a_moving_pick_beats_the_script_walk_and_a_standing_one_does_not() {
        use legaia_actor_formats::character_pack::{LOCOMOTION_BANK_STRIDE, LOCOMOTION_IDLE_SLOT};
        // Seven two-bone records of 2 frames each: slot s tags 100*s.
        let bank = synth_anm_bundle(&[(2, 2); LOCOMOTION_BANK_STRIDE]);
        let mut anim = FieldPlayerAnim::from_locomotion_bank(&bank, 0).unwrap();
        const HOP: usize = 5;
        anim.select_retail_slot(Some(HOP));
        let mut flags = CLIP_CLAMP;
        let mut seen = Vec::new();
        for _ in 0..12 {
            anim.moved_this_frame = true; // the arc moved the player
            let (pose, out) = anim.tick_with_flags(flags);
            flags = out;
            seen.push(pose.bone_outputs[0].0[0] as u8);
        }
        // Two frames at step 8: 0, 0, 10, then held on frame 1.
        let hop = (HOP * 100) as u8;
        assert_eq!(&seen[..4], &[hop, hop, hop + 10, hop + 10]);
        assert!(seen[3..].iter().all(|&t| t == hop + 10), "{seen:?}");
        // A standing pick under a script walk: the walk, without a rewind on
        // the vsync between two steps.
        anim.select_retail_slot(Some(LOCOMOTION_IDLE_SLOT));
        let mut keys = Vec::new();
        for t in 0..6 {
            anim.moved_this_frame = t % 2 == 0;
            let _ = anim.tick_with_flags(0);
            keys.push((anim.playing_clip().0, anim.playing_clip().2));
        }
        assert!(keys.iter().all(|k| k.0 == "walk"), "{keys:?}");
        let cursors: Vec<u32> = keys.iter().map(|k| k.1).collect();
        assert_eq!(
            cursors,
            vec![8, 16, 24, 0, 8, 16],
            "one walk, never rewound"
        );
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
        // Move (a pad walk): walk record restarts at frame 0 (tag 0+).
        anim.moved_this_frame = true;
        anim.pad_drove_this_frame = true;
        assert_eq!(anim.tick().bone_outputs[0].0[0], 0);
        anim.moved_this_frame = true;
        anim.pad_drove_this_frame = true;
        assert_eq!(anim.tick().bone_outputs[0].0[0], 10);
        // The pad step still owns the frame it stands still on.
        anim.pad_drove_this_frame = true;
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

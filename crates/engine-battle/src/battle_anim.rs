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
//! a [`legaia_anm::PoseFrame`] - the same per-object `(translation, rotation)`
//! shape the field player produces - so the renderer's existing posed-mesh path
//! consumes both. Battle meshes deform through
//! [`legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot`] (the rigid `R·v + T`
//! builder) so the per-object rotations actually turn the limbs.
//!
//! Interpolation is the retail decoder's own blend
//! ([`legaia_engine_vm::battle_pose_blend::blend_part_pose`]): the cursor's
//! 1/16-keyframe nibble, translations lerped with an arithmetic shift, angles
//! on the short arc, and the Euler-flip retry when a part's three angle steps
//! total more than `0xC00`. The per-tick phase
//! advance is retail-pinned when the clip carries its entry's rate byte:
//! `FUN_80047430` advances the node's 12.4 cursor by
//! `(frame_dt * actor[+0x21D] * record[+0x78]) >> 1` per frame (`>> 2` on
//! the idle branch), where `actor[+0x21D]` is the per-actor anim-rate byte
//! (normal `8` - the arts slow-motion channel, see
//! `legaia_engine_vm::battle_anim_rate`). [`step_for_rate`] is the idle
//! advance at the normal rate; [`MonsterAnimPlayer::tick_rated`] applies the
//! rate and the idle/action branch split. A zero clip rate (clip built
//! without entry context) keeps the historical display default.

use legaia_anm::PoseFrame;
use legaia_asset::monster_archive::{MonsterAnimation, PartPose};
use legaia_engine_vm::battle_pose_blend::blend_part_pose;

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
    /// archive entry index for monsters), retained so per-frame consumers -
    /// the facial animator looks up the playing entry's face tracks by it -
    /// can identify the clip without holding the `MonsterAnimation`.
    action_id: u8,
    /// The clip's attach-key / clip-identity byte (entry `+0x77`,
    /// [`MonsterAnimation::attach_key`]) - what retail's per-clip effect
    /// triggers compare against the committed record (`FUN_8005112C`
    /// weapon trail, `FUN_8004CE2C` impact freeze/tint arms). `0` when the
    /// source stream carried no entry header.
    attach_key: u8,
    /// The clip's solo / freeze byte (entry `+0x87`,
    /// [`MonsterAnimation::solo_flag`]) - the second input of the retail
    /// history-ring id a monster seat stamps each frame
    /// (`legaia_engine_core::battle_afterimage::monster_ring_id`).
    solo_flag: u8,
    /// The clip's impact-effect class (entry `+0x7A`,
    /// [`MonsterAnimation::impact_class`]) - the selector the hit routine
    /// `FUN_801EC3E4` reads off the acting actor's committed record and
    /// stamps onto the struck actor (`World::arm_impact_tint`).
    impact_class: u8,
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
    /// The entry's authored **loop window** - retail's `+0x84` count seeded
    /// into `actor+0x176` (as `count << 4`, one whole 12.4 unit per cycle)
    /// and the `[+0x85, +0x86]` frame pair - carried in this player's phase
    /// units: `loop_budget` = cycles remaining × [`PHASE_ONE`],
    /// `loop_start` / `loop_end` = the window's frames × [`PHASE_ONE`].
    /// A zero budget is "no window" (the idle / walk / swing case).
    loop_budget: u32,
    loop_start: u32,
    loop_end: u32,
    /// Set by [`Self::apply_loop_window`] when a real window (`start !=
    /// end`) rewound the cursor this tick; taken by
    /// [`Self::take_loop_rewound`]. The tick re-zeroes the per-clip hit
    /// index on exactly that edge (`FUN_80047430` `0x80047840..0x80047878`).
    loop_rewound: bool,
    /// The entry's signed root-motion speed (`+0x0C`), `0` for a headless
    /// clip - what the anim tick's position term multiplies per frame
    /// (`FUN_80047430` `0x80047D34..0x80047E18`).
    root_speed: i16,
    /// The entry's power run (`+0x00..+0x04`) and hit-event frame list
    /// (`+0x10..+0x14`) plus the `+0x76` event-commit lock, kept on the
    /// player so the per-frame hit-event driver reads them off the clip
    /// that is actually playing (retail: the node's committed entry
    /// `node[+0x4C]`). `None` for a clip built without an entry head.
    hit_source: Option<HitEventSource>,
    /// The entry's signed end-of-clip displacement (`+0x0E`), `0` for a
    /// headless clip - the Z term the decoder folds into the last frame's
    /// tween and the distance the anim tick moves the actor along its
    /// facing at the natural end (`FUN_80047430` `0x80047A68..0x80047B2C`).
    end_root_step: i16,
    /// Frame 0 of the clip queued behind this one, set by the world each
    /// tick ([`Self::set_tween_target`]); `None` blends the last frame
    /// toward itself.
    tween: Option<TweenTarget>,
    /// Set on the tick the cursor crosses the stream's frame count (a
    /// one-shot finishing, a looping clip wrapping); taken by
    /// [`Self::take_natural_end`].
    natural_end: bool,
}

/// What the decoder blends a clip's **last** frame toward: frame 0 of the
/// queued clip, with the committed entry's `+0x0E` added to the Z delta
/// (`FUN_8004998C` `0x80049A7C..0x80049BD0`, the Z term at `0x80049D64`).
/// Built by the world from the actor's queued id
/// (`World::battle_tween_target`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TweenTarget {
    /// The queued clip's frame 0, one pose per part. `None` - the gate
    /// failed (HP `0`, a queued id `>= 0x10`) or a monster's queued stream
    /// has a different part count - blends the last frame toward itself.
    pub frame0: Option<Vec<PartPose>>,
    /// The Z-delta term: the committed entry's `+0x0E`, or `0` when the
    /// actor's `+0x228` byte is set.
    pub z_bias: i16,
}

/// The committed entry's hit-event side, as the damage kernel `FUN_801EC3E4`
/// and the anim tick's event-path commit read it
/// ([`legaia_engine_vm::battle_action::hit_event_admits`] /
/// [`legaia_engine_vm::battle_action::event_commit_due`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HitEventSource {
    /// Entry `+0x00..+0x04` - one power byte per hit.
    pub power_run: [u8; 4],
    /// Entry `+0x10..+0x14` - the zero-terminated hit-frame list.
    pub event_frames: [u8; 4],
    /// Entry `+0x76` - non-zero locks the mid-clip event-path commit.
    pub event_lock: u8,
}

/// Retail-pinned **base** per-tick phase advance for an entry rate byte:
/// the `FUN_80047430` idle-branch cursor delta at the normal anim rate -
/// `(dt * 8 * rate) >> 2` per game frame, per engine tick `2 * rate` units
/// of 1/16 keyframe, scaled to this player's 8.8 phase (`rate * 32`). A
/// zero rate (no entry context) falls back to the historical display
/// default (`64`, which equals clip rate `2`).
///
/// This is the **idle** advance; a playing action clip runs double
/// (`>> 1` vs `>> 2` at `0x800476EC..0x8004775C`), and both scale by the
/// per-actor anim-rate byte `actor[+0x21D]` - see
/// [`MonsterAnimPlayer::tick_rated`] and
/// `legaia_engine_vm::battle_anim_rate`.
// PORT: FUN_80047430 - the per-frame anim-node cursor advance
// (`node+0x68 += (DAT_1F800393 * actor[+0x21D] * record[+0x78]) >> 1`, idle
// branch `>> 2`), reduced to the idle case at the normal rate
// `actor[+0x21D] = 8` with a 1-vsync engine tick.
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
        // The loop window (`+0x84..+0x87`): the commit `FUN_8004AD80` seeds
        // `actor+0x176 = count << 4` and `+0x21B = count`
        // (`0x8004BDEC..0x8004BE0C`); the tick's window test needs
        // `start <= end`, and a window past the stream is left to the
        // natural end (the disc census filter `animation_loop_windows`
        // applies is the same bound).
        let frames = anim.frame_count as u32;
        let (loop_budget, loop_start, loop_end) = match anim.entry_loop_window() {
            Some((count, start, end))
                if u32::from(start) <= u32::from(end) && u32::from(end) <= frames =>
            {
                (
                    u32::from(count) * PHASE_ONE,
                    u32::from(start) * PHASE_ONE,
                    u32::from(end) * PHASE_ONE,
                )
            }
            _ => (0, 0, 0),
        };
        let hit_source = match (anim.entry_power_run(), anim.entry_event_frames()) {
            (Some(power_run), Some(event_frames)) => Some(HitEventSource {
                power_run,
                event_frames,
                event_lock: anim.entry_event_commit_lock().unwrap_or(0),
            }),
            _ => None,
        };
        Some(Self {
            frames: anim.frames.clone(),
            frame_count: frames,
            part_count: anim.part_count,
            action_id: anim.action_id,
            attach_key: anim.attach_key,
            solo_flag: anim.solo_flag,
            impact_class: anim.impact_class,
            phase: 0,
            step: step_for_rate(anim.rate),
            looping: true,
            finished: false,
            loop_budget,
            loop_start,
            loop_end,
            loop_rewound: false,
            root_speed: anim.entry_root_speed().unwrap_or(0),
            hit_source,
            end_root_step: anim.entry_end_root_step().unwrap_or(0),
            tween: None,
            natural_end: false,
        })
    }

    /// The entry's signed root-motion speed (`+0x0C`); `0` for a headless
    /// clip.
    pub fn root_speed(&self) -> i16 {
        self.root_speed
    }

    /// The entry's signed end-of-clip displacement (`+0x0E`); `0` for a
    /// headless clip.
    pub fn end_root_step(&self) -> i16 {
        self.end_root_step
    }

    /// Frame 0 of this clip, what a re-commit of it tweens into.
    pub fn first_frame(&self) -> &[PartPose] {
        &self.frames[0]
    }

    /// Install (or clear) the queued clip the last frame tweens into. The
    /// world recomputes it every tick from the actor's `+0x1DA` mirror.
    pub fn set_tween_target(&mut self, target: Option<TweenTarget>) {
        self.tween = target;
    }

    /// `true` once on the tick the cursor crossed the stream's frame count
    /// (retail's natural end, `FUN_80047430` `0x80047A48..0x80047A60`).
    /// Clears on read.
    pub fn take_natural_end(&mut self) -> bool {
        std::mem::take(&mut self.natural_end)
    }

    /// The committed entry's hit-event side, or `None` for a clip built
    /// without an entry head (which then has no hit events at all).
    pub fn hit_source(&self) -> Option<HitEventSource> {
        self.hit_source
    }

    /// Cycles the loop window still owes, in whole clip frames - retail's
    /// `actor+0x21B` mirror (`+0x176 >> 4`). `0` when the clip carries no
    /// window or has spent it.
    pub fn loop_cycles_remaining(&self) -> u8 {
        (self.loop_budget / PHASE_ONE).min(255) as u8
    }

    /// Release the loop window early - the `actor+0x176` / `+0x21B` clear a
    /// cast module performs to end an authored park (the Delilas modules do
    /// this; see `docs/formats/monster-animation.md` § Playback).
    pub fn release_loop_window(&mut self) {
        self.loop_budget = 0;
    }

    /// `true` once per tick on which a real loop window rewound the cursor
    /// (the `0x800477EC..0x8004783C` arm; a `start == end` park does not
    /// count). Clears on read. The hit-event driver re-zeroes the actor's
    /// per-clip hit index on this edge under retail's own gate.
    pub fn take_loop_rewound(&mut self) -> bool {
        std::mem::take(&mut self.loop_rewound)
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

    /// `true` for a looping player (the idle / walk cycles), `false` for a
    /// one-shot.
    pub fn is_looping(&self) -> bool {
        self.looping
    }

    /// Number of keyframes in the clip.
    pub fn frame_count(&self) -> u32 {
        self.frame_count
    }

    /// Number of animated parts (= TMD objects the pose addresses).
    pub fn part_count(&self) -> usize {
        self.part_count
    }

    /// The playing clip's `action_id` (see the field docs).
    pub fn action_id(&self) -> u8 {
        self.action_id
    }

    /// The playing clip's attach-key / identity byte (entry `+0x77`; see
    /// the field docs). `0` = no entry header.
    pub fn attach_key(&self) -> u8 {
        self.attach_key
    }

    /// The playing clip's solo / freeze byte (entry `+0x87`; see the field
    /// docs). `0` = no entry header.
    pub fn solo_flag(&self) -> u8 {
        self.solo_flag
    }

    /// The playing clip's impact-effect class (entry `+0x7A`; see the
    /// field docs). `0` = a landed hit from this clip tints nothing.
    pub fn impact_class(&self) -> u8 {
        self.impact_class
    }

    /// The cursor in retail's own 12.4 fixed-point unit (sixteenths of a
    /// keyframe) - the scale of the anim node's `+0x68` word. The per-clip
    /// impact arms (`FUN_8004CE2C`) window their effects in this unit
    /// (e.g. `0x40..=0x80` = keyframes 4..8), so consumers comparing
    /// against those disassembly constants read this rather than
    /// [`Self::current_frame`]. This player's phase is 8.8, so the
    /// conversion is a `>> 4`.
    pub fn cursor_sixteenths(&self) -> u16 {
        (self.phase >> 4) as u16
    }

    /// Integer keyframe index of the cursor - the value retail's render-node
    /// update passes to the facial animator as the frame counter
    /// (`FUN_80047430` hands `FUN_8004C7B4` the node's 12.4 `+0x68` cursor
    /// shifted to whole keyframes; this player's 8.8 phase shifts the same
    /// way). The facial tracks' `start`/`end` bytes are in these units.
    pub fn current_frame(&self) -> i16 {
        (self.phase >> PHASE_FRAC_BITS) as i16
    }

    /// Reset the cursor to the clip's first keyframe. A one-shot player that
    /// had run out plays again from here (retail's re-commit of a still
    /// queued clip zeroes the node cursor, `FUN_8004AD80`), so the
    /// completion flag clears with it.
    pub fn rewind(&mut self) {
        self.phase = 0;
        self.finished = false;
    }

    /// Advance one tick and return the interpolated per-object pose. A looping
    /// player wraps over the clip (`PoseFrame::finished` always `false`); a
    /// one-shot player clamps at the last keyframe and reports `finished`.
    pub fn tick(&mut self) -> PoseFrame {
        self.advance(self.step)
    }

    /// Advance one tick under the retail anim-rate law: the effective step is
    /// `base_step * rate * (idle ? 1 : 2) / 8`
    /// (`legaia_engine_vm::battle_anim_rate::scaled_anim_step`, mirroring the
    /// `FUN_80047430` `>> 1` / `>> 2` branch pair). At the normal rate `8` an
    /// action clip runs at retail's double-idle speed; the arts slow-motion
    /// rates (`4` / `2` / `0`) stretch or freeze it.
    // REF: FUN_80047430 (the rate-scaled cursor advance)
    pub fn tick_rated(
        &mut self,
        rate: legaia_engine_vm::battle_anim_rate::AnimRate,
        idle: bool,
    ) -> PoseFrame {
        let step = legaia_engine_vm::battle_anim_rate::scaled_anim_step(self.step, rate, idle);
        self.advance(step)
    }

    /// The retail loop-window test (`FUN_80047430` `0x80047768..0x8004783C`),
    /// run on the advanced cursor **before** the natural-end test: while the
    /// hold budget (`actor+0x176`) is non-zero and the cursor has reached
    /// `end`, a `start == end` window parks the cursor on `start` and
    /// spends the overshoot from the budget (`0x800477A4..0x800477D4`), and
    /// a real window rewinds by the span once per whole budget unit until
    /// the cursor is back below `end` or the budget is spent
    /// (`0x800477EC..0x8004783C`).
    // PORT: FUN_80047430 (the +0x176 loop-window arm)
    fn apply_loop_window(&mut self, mut phase: u32) -> u32 {
        if self.loop_budget == 0 || phase < self.loop_end {
            return phase;
        }
        if self.loop_end == self.loop_start {
            let over = phase - self.loop_start;
            self.loop_budget = self.loop_budget.saturating_sub(over);
            return self.loop_start;
        }
        let span = self.loop_end - self.loop_start;
        loop {
            phase = phase.saturating_sub(span);
            self.loop_budget = self.loop_budget.saturating_sub(PHASE_ONE);
            if self.loop_budget == 0 || phase < self.loop_end {
                break;
            }
        }
        self.loop_rewound = true;
        phase
    }

    fn advance(&mut self, step: u32) -> PoseFrame {
        let total = self.frame_count * PHASE_ONE;
        let last = self.frames.len() - 1;
        // A finished one-shot holds: retail would have committed the queued
        // clip on the crossing tick; the engine keeps the player until the
        // world installs the next one (or forever, for the downed hold).
        if !self.looping && self.finished {
            return self.hold_pose();
        }
        // Window before natural end, exactly like the tick.
        let raw = self.apply_loop_window(self.phase + step);
        // The natural end: the cursor crossed the stream's frame count
        // (`FUN_80047430` `0x80047A48..0x80047A60`, `slt frame, frames`).
        if raw >= total {
            self.natural_end = true;
            if !self.looping {
                self.finished = true;
                self.phase = last as u32 * PHASE_ONE;
                return self.hold_pose();
            }
            self.phase = raw % total;
        } else {
            self.phase = raw;
        }
        let f0 = ((self.phase >> PHASE_FRAC_BITS) as usize).min(last);
        // Retail blends on the 12.4 cursor's low nibble (`+0x68 & 0xF`); this
        // player's phase is 8.8, so the nibble is bits 4..8.
        let frac16 = ((self.phase >> 4) & 0xF) as u8;
        // The next-entry rule (`FUN_8004998C` `0x80049A28..0x80049BE4`).
        // First the loop-window arm: on the frame before the window's end,
        // while cycles remain, the blend target is the window's start frame
        // (frame == `+0x86 - 1` and `+0x21B != 0` -> entry `+0x85`).
        let mut next: &[PartPose] = if f0 < last {
            &self.frames[f0 + 1]
        } else {
            self.last_frame_target()
        };
        let mut z_bias = 0;
        let mut windowed = false;
        if self.loop_cycles_remaining() != 0 && self.loop_end > 0 {
            let end_frame = (self.loop_end / PHASE_ONE) as usize;
            let start_frame = (self.loop_start / PHASE_ONE) as usize;
            if f0 + 1 == end_frame && start_frame < self.frames.len() {
                next = &self.frames[start_frame];
                windowed = true;
            }
        }
        // Then the last frame: the queued clip's frame 0 (or the clip
        // itself), with the committed entry's `+0x0E` on the Z delta.
        if f0 == last && !windowed {
            z_bias = self.tween.as_ref().map_or(0, |t| t.z_bias);
        }

        let a = &self.frames[f0];
        let bone_outputs = (0..self.part_count)
            .map(|p| {
                let blended = blend_part_pose(a[p], next[p], frac16, z_bias);
                let r = blended.rotation.map(|v| v as i16);
                (blended.translation, r)
            })
            .collect();

        PoseFrame {
            bone_outputs,
            factor: (self.phase & (PHASE_ONE - 1)) as u8,
            finished: self.finished,
        }
    }

    /// The pose the last frame blends toward: the tween target's frame 0
    /// when its part count matches, the clip's own last frame for a
    /// self-blend (`move a1,t0`, `0x80049BCC` / `0x80049B9C`), and - with no
    /// target installed - the historical default: a looping clip's own
    /// frame 0 (the re-queued idle), a one-shot's own last frame.
    fn last_frame_target(&self) -> &[PartPose] {
        let last = &self.frames[self.frames.len() - 1];
        match &self.tween {
            Some(TweenTarget {
                frame0: Some(f), ..
            }) if f.len() == self.part_count => f,
            Some(_) => last,
            None if self.looping => &self.frames[0],
            None => last,
        }
    }

    /// A finished one-shot's pose: the frame the crossing tick shows - the
    /// queued clip's frame 0 exact (retail's commit zeroes the cursor, so the
    /// crossing tick draws the new entry's first frame), or the clip's own
    /// last frame when it tweens into itself.
    fn hold_pose(&self) -> PoseFrame {
        let target = match &self.tween {
            Some(TweenTarget {
                frame0: Some(f), ..
            }) if f.len() == self.part_count => f.as_slice(),
            _ => &self.frames[self.frames.len() - 1],
        };
        let bone_outputs = target
            .iter()
            .take(self.part_count)
            .map(|p| {
                let b = blend_part_pose(*p, *p, 0, 0);
                (b.translation, b.rotation.map(|v| v as i16))
            })
            .collect();
        PoseFrame {
            bone_outputs,
            factor: 0,
            finished: true,
        }
    }
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
            attach_key: 0,
            solo_flag: 0,
            impact_class: 0,
            effect_script: Vec::new(),
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
            attach_key: 0,
            solo_flag: 0,
            impact_class: 0,
            effect_script: Vec::new(),
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
            attach_key: 0,
            solo_flag: 0,
            impact_class: 0,
            effect_script: Vec::new(),
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
        // halfway: (3840 + 512/2) & 0xFFF = 0 - retail masks the blended
        // angle back to 12 bits.
        assert_eq!(r[2], 0);
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
            attach_key: 0,
            solo_flag: 0,
            impact_class: 0,
            effect_script: Vec::new(),
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
        // Retail's natural end is the cursor crossing the frame count
        // (`FUN_80047430` `slt frame, frames`), so the last keyframe's
        // interval plays before the clip finishes.
        let mut p = MonsterAnimPlayer::new_one_shot(&clip(3)).unwrap();
        p.step = 256; // one keyframe per tick
        assert!(!p.finished());
        let _ = p.tick(); // frame 1
        assert!(!p.finished());
        let f = p.tick(); // frame 2 (last), its interval still to play
        assert!(!p.finished());
        assert_eq!(f.bone_outputs[0].0[0], 20);
        let f = p.tick(); // crosses the frame count
        assert!(p.finished());
        assert!(f.finished);
        assert!(p.take_natural_end(), "the crossing reports the natural end");
        assert!(!p.take_natural_end(), "once");
        let (t, _) = f.bone_outputs[0];
        assert_eq!(t[0], 20, "held on the final keyframe");
        assert_eq!(p.current_frame(), 2);
        // Further ticks hold the final pose.
        let f2 = p.tick();
        assert_eq!(f2.bone_outputs[0].0[0], 20);
        assert!(f2.finished);
        assert!(!p.take_natural_end());
    }

    #[test]
    fn the_last_frame_tweens_into_the_queued_clip_with_the_z_term() {
        // `FUN_8004998C` `0x80049A7C..0x80049BD0`: on the last frame the
        // blend target is the queued clip's frame 0, and the committed
        // entry's `+0x0E` joins the Z delta (`0x80049D64`).
        let mut p = MonsterAnimPlayer::new_one_shot(&clip(3)).unwrap();
        p.step = 256 / 2; // half a keyframe per tick
        let target = PartPose {
            tx: 100,
            ty: 0,
            tz: 40,
            rx: 0,
            ry: 0,
            rz: 0,
        };
        p.set_tween_target(Some(TweenTarget {
            frame0: Some(vec![target]),
            z_bias: -8,
        }));
        for _ in 0..4 {
            p.tick(); // frames 0.5 .. 2.0
        }
        let f = p.tick(); // 2.5: halfway from the last frame into the target
        let (t, _) = f.bone_outputs[0];
        assert_eq!(t[0], 20 + (100 - 20) / 2);
        assert_eq!(t[2], (40 - 8) / 2, "the Z term rides the delta");
        assert!(!p.finished());
        let f = p.tick(); // crosses: the new entry's frame 0, exact
        assert!(p.finished());
        assert_eq!(f.bone_outputs[0].0, [100, 0, 40]);
        // A failed gate (`frame0: None`) blends toward itself: a static
        // last interval, and the hold is the clip's own last frame.
        let mut q = MonsterAnimPlayer::new_one_shot(&clip(3)).unwrap();
        q.step = 128;
        q.set_tween_target(Some(TweenTarget {
            frame0: None,
            z_bias: 0,
        }));
        for _ in 0..4 {
            q.tick();
        }
        assert_eq!(q.tick().bone_outputs[0].0[0], 20);
        assert_eq!(q.tick().bone_outputs[0].0[0], 20);
        // A mismatched part count is a self-blend that keeps the Z term.
        let mut r = MonsterAnimPlayer::new_one_shot(&clip(3)).unwrap();
        r.step = 128;
        r.set_tween_target(Some(TweenTarget {
            frame0: Some(vec![target, target]),
            z_bias: -8,
        }));
        for _ in 0..4 {
            r.tick();
        }
        assert_eq!(r.tick().bone_outputs[0].0, [20, 0, -4]);
    }

    #[test]
    fn a_looping_clip_wraps_into_its_own_frame_zero_and_reports_it() {
        let mut p = MonsterAnimPlayer::new(&clip(3)).unwrap();
        p.step = 128;
        for _ in 0..4 {
            p.tick();
        }
        // 2.5: halfway from the last frame back to frame 0.
        assert_eq!(p.tick().bone_outputs[0].0[0], 10);
        assert!(!p.take_natural_end());
        p.tick(); // wraps
        assert!(p.take_natural_end());
        assert_eq!(p.current_frame(), 0);
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

    /// A clip whose entry head carries a loop window `[start, end]` with
    /// `count` cycles (`+0x84..+0x87`), a root speed and a hit-event pair.
    fn windowed_clip(frames: usize, count: u8, start: u8, end: u8) -> MonsterAnimation {
        let mut c = clip(frames);
        let mut head = vec![0u8; legaia_asset::monster_archive::EFFECT_SCRIPT_HEAD_BYTES];
        head[0] = 0x0C; // power byte 0 (in band)
        head[0x0C..0x0E].copy_from_slice(&(-20i16).to_le_bytes());
        head[0x10] = 3; // hit frame
        head[0x76] = 1; // event-commit lock
        head[0x84] = count;
        head[0x85] = start;
        head[0x86] = end;
        c.effect_script = head;
        c
    }

    #[test]
    fn loop_window_replays_its_frames_the_authored_number_of_times() {
        // 10-frame one-shot, window [4, 6] x 2: the cursor runs 0..6, rewinds
        // to 4 twice, then continues to the end.
        let mut p = MonsterAnimPlayer::new_one_shot(&windowed_clip(10, 2, 4, 6)).unwrap();
        p.step = 256;
        assert_eq!(p.loop_cycles_remaining(), 2);
        let mut seen = Vec::new();
        for _ in 0..16 {
            p.tick();
            seen.push(p.current_frame());
            if p.finished() {
                break;
            }
        }
        // The last keyframe's interval plays before the natural end.
        assert_eq!(seen, vec![1, 2, 3, 4, 5, 4, 5, 4, 5, 6, 7, 8, 9, 9]);
        assert_eq!(p.loop_cycles_remaining(), 0);
        assert!(p.finished());
    }

    #[test]
    fn a_parking_window_holds_the_cursor_until_the_budget_is_spent() {
        // [5, 5] x 3: park on frame 5 for three frames of budget, then run.
        let mut p = MonsterAnimPlayer::new_one_shot(&windowed_clip(8, 3, 5, 5)).unwrap();
        p.step = 256;
        let mut seen = Vec::new();
        for _ in 0..14 {
            p.tick();
            seen.push(p.current_frame());
            if p.finished() {
                break;
            }
        }
        assert_eq!(seen, vec![1, 2, 3, 4, 5, 5, 5, 5, 6, 7, 7]);
    }

    #[test]
    fn a_real_window_reports_each_rewind_once_and_a_park_never() {
        // [4, 6] x 2 on a 10-frame clip: two rewinds, each reported on the
        // tick it happens and cleared by the read.
        let mut p = MonsterAnimPlayer::new_one_shot(&windowed_clip(10, 2, 4, 6)).unwrap();
        p.step = 256;
        let mut edges = Vec::new();
        for _ in 0..16 {
            p.tick();
            if p.take_loop_rewound() {
                edges.push(p.current_frame());
            }
            if p.finished() {
                break;
            }
        }
        assert_eq!(
            edges,
            vec![4, 4],
            "one edge per rewind, on the rewound frame"
        );
        assert!(!p.take_loop_rewound(), "cleared by the read");
        // A parking window ([5, 5]) holds without rewinding: no edge.
        let mut q = MonsterAnimPlayer::new_one_shot(&windowed_clip(8, 3, 5, 5)).unwrap();
        q.step = 256;
        for _ in 0..12 {
            q.tick();
            assert!(!q.take_loop_rewound());
        }
    }

    #[test]
    fn release_loop_window_lets_the_clip_run_out() {
        let mut p = MonsterAnimPlayer::new_one_shot(&windowed_clip(8, 0xFF, 5, 5)).unwrap();
        p.step = 256;
        for _ in 0..40 {
            p.tick();
        }
        assert_eq!(p.current_frame(), 5, "an 0xFF budget parks for a long time");
        p.release_loop_window();
        for _ in 0..4 {
            p.tick();
        }
        assert!(p.finished());
    }

    #[test]
    fn the_player_carries_the_entry_head_the_drivers_read() {
        let p = MonsterAnimPlayer::new(&windowed_clip(8, 1, 2, 3)).unwrap();
        assert_eq!(p.root_speed(), -20);
        assert_eq!(
            p.hit_source(),
            Some(HitEventSource {
                power_run: [0x0C, 0, 0, 0],
                event_frames: [3, 0, 0, 0],
                event_lock: 1,
            })
        );
        let bare = MonsterAnimPlayer::new(&clip(3)).unwrap();
        assert_eq!(bare.root_speed(), 0);
        assert_eq!(bare.hit_source(), None);
        assert_eq!(bare.loop_cycles_remaining(), 0);
    }
}

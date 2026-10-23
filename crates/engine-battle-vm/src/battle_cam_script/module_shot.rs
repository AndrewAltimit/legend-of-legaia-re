//! The summon module's own shots, as the battle camera steps them.

use super::camera::Glide;
use super::*;

/// The action-SM states whose camera belongs to the summon module: the
/// sustain `0x35` and the return `0x36` re-enter the paged module through
/// `FUN_801F1ED4` every pass and call no framing case themselves
/// (`docs/subsystems/battle-action.md`). The module's arm-0 shot lands on
/// `0x34`'s last pass, the same one that leaves for `0x35`.
pub const SUMMON_MODULE_STATES: [u8; 2] = [0x35, 0x36];

/// The capture band's module tick `0x70`: like the summon band's sustain it
/// calls no framing case (`0x801E50C8..0x801E50E8`), so the camera holds the
/// pose `0x6F`'s last case-6 pass left and moves only as the module moves it
/// (`legaia_engine_cast::cast_module_camera::capture_camera_director`).
pub const CAPTURE_MODULE_STATE: u8 = 0x70;

/// The yaw the escape roll's success arm snaps the live camera to before it
/// arms the flee shot (`li v0,0xf00 ; sh v0,0x2(a1)` at `0x801E7EE0`, into
/// `0x8007B792`).
pub const ESCAPE_SNAP_YAW: f32 = 3840.0;

impl BattleCamera {
    /// Arm one of the summon module's `FUN_801D829C` calls: a tween from the
    /// live pose to `target` over `frames` display frames. A later shot
    /// rebuilds from wherever the previous one has reached, exactly as a
    /// second builder call does.
    ///
    /// The builder runs once, on the arm, over the module's own `frames`:
    /// each component's per-vsync increment is `ceil(|gap| / frames)`
    /// (`addu v0,v0,t2 ; addiu v0,v0,-1 ; divu v0,t2` at
    /// `0x801D845C..0x801D8464`). The walker task `FUN_8002149C` then adds
    /// `increment * frame_step` (`0x1F800393`, the pass's own step) every
    /// pass and lands each component on its endpoint the pass it gets there
    /// (`0x80021524..0x80021534` for the angles, `0x800215C8` / `0x8002165C`
    /// for the translation and focus), independently of the others; the
    /// task lives until all nine have landed. So the shot is not an
    /// arrive-together glide over `frames / frame_step` passes: a component
    /// whose gap does not divide by `frames` runs on the rounded-up
    /// increment and lands early. `vera_summon_mid_cast`'s arm-1 shot (yaw
    /// `1000` units over `0x60` frames) reads yaw increment `11` in the step
    /// table, the live yaw `11 * 72` past its start beside pitch, TR y and
    /// TR z all `72` vsyncs in.
    ///
    /// `raw_tr_z` is the target's TR z in world units, the value the module
    /// hands the builder.
    ///
    /// REF: FUN_801D829C, FUN_8002149C
    pub fn arm_module_shot(&mut self, target: BattleCamPose, raw_tr_z: i32, frames: u32) {
        let mut from = self.pose;
        if self.frames_to_steps(frames) == 1 {
            // A one-frame tween is a cut: retail's walker lands it the frame
            // it is armed, before the next arm can rebuild from it. The
            // camera's step cadence would otherwise let a following shot
            // start from the pose the cut replaced.
            let mut g = Glide::linear(&mut from, target, raw_tr_z, 1, true);
            self.pose = from;
            self.land(&g);
            g.steps_left = Some(0);
            self.module_glide = Some(g);
            return;
        }
        let mut g = Glide::linear(&mut from, target, raw_tr_z, frames, true);
        self.pose = from;
        g.steps_left = None;
        g.per_vsync = true;
        self.module_glide = Some(g);
    }

    /// Arm `FUN_801D5854(7, 6)` out of a module arm: case 6 on `actor` (the
    /// creature seat) with the module's own yaw base `ctx[+0x6DA]` and depth
    /// `ctx[+0x6D0]`, the rest of case 6's context taken live. The arm
    /// re-arms it every pass, so the tween chases the walking creature over
    /// case 6's own `0xC` frames.
    ///
    /// REF: FUN_801D5854 (case 6)
    pub fn arm_module_follow(&mut self, actor: BattleCamActor, yaw_base: i32, depth_raw: i32) {
        let f = ActionFraming {
            yaw_base,
            depth_raw,
            ..self.live_action_framing()
        };
        let target = action_framing(actor, f);
        let mut from = self.pose;
        let steps = self.steps_of(ACTION_STEPS);
        let g = Glide::linear(&mut from, target, f.raw_z(), steps, true);
        self.pose = from;
        self.module_glide = Some(g);
    }

    /// A module arm's call into the action SM's **case 8** on the acting
    /// actor (`FUN_801D5854(ctx[+0x13], 8)`): the post-action framing of the
    /// actor and its target, re-armed every pass over case 8's own `a3 =
    /// 0xC`. Its dead-target arm zeroes the yaw ladder `ctx[+0x6DA]`
    /// ([`Self::action_end_pose`]), which is the half a module's settle arm
    /// leaves behind for the Done band's case 6.
    ///
    /// REF: FUN_801D5854 (case 8)
    pub fn arm_module_end_frame(&mut self) {
        let (target, raw_z) = self.action_end_pose();
        let mut from = self.pose;
        let steps = self.steps_of(POST_ACTION_STEPS);
        let g = Glide::linear(&mut from, target, raw_z, steps, true);
        self.pose = from;
        self.module_glide = Some(g);
    }

    /// A module arm's direct writes into the live camera globals (pitch, TR
    /// y, TR z in its prescaled units), added to the pose as it stands.
    pub fn nudge_module(&mut self, pitch: i16, tr_y: i16, tr_z: i16) {
        self.pose.pitch += f32::from(pitch);
        self.pose.tr[1] += f32::from(tr_y);
        self.pose.tr[2] += f32::from(tr_z);
    }

    /// The same direct write into the TR x global `0x800840B8` (PROT 0920's
    /// arm 7 pan), added to the pose as it stands.
    pub fn nudge_module_tr_x(&mut self, tr_x: i16) {
        self.pose.tr[0] += f32::from(tr_x);
    }

    /// A capture-class arm's drift: [`Self::nudge_module`] plus the yaw
    /// global `0x8007B792`, which the capture arms also walk.
    ///
    /// What a shot in flight does with it depends on the component. The
    /// tween walker `FUN_8002149C` skips a component whose increment is `0`
    /// (`beq v1,zero` at `0x800214C8` / `0x80021574` / `0x80021608`), so a
    /// drift on a component the shot does not move survives it; the port
    /// folds that into the armed shot's target, whose last step lands there.
    /// A component the shot moves is walked by `increment * step` and then
    /// stored back on its endpoint once it reaches it (`sw a0,0x0(a1)` at
    /// `0x800215C8` / `0x8002165C`, the angle lanes off their own remaining
    /// count at `0x80021524..0x80021534`), and the walker task lives until
    /// every component has landed. A drift on such a component only delays
    /// the landing, and the shot ends on its own endpoint: the drift is
    /// spent. `cort_guilty_cross_mid_cast`'s arm 1 drifts TR z `+4` a vsync
    /// from the first pass of arm 0's `0x20`-frame shot, and the capture
    /// reads `3420` - the endpoint `3276` plus only the drift since the shot
    /// landed.
    pub fn drift_module(&mut self, pitch: i16, yaw: i16, tr_y: i16, tr_z: i16) {
        self.nudge_module(pitch, tr_y, tr_z);
        self.pose.yaw = (self.pose.yaw + f32::from(yaw)).rem_euclid(4096.0);
        if let Some(g) = self.module_glide.as_mut().filter(|g| !g.landed()) {
            if g.rate[0] == 0.0 {
                g.target.pitch += f32::from(pitch);
            }
            if g.rate[3] == 0.0 {
                g.target.tr[1] += f32::from(tr_y);
            }
            if g.rate[4] == 0.0 {
                g.target.tr[2] += f32::from(tr_z);
            }
            if g.rate[1] == 0.0 {
                g.target.yaw = (g.target.yaw + f32::from(yaw)).rem_euclid(4096.0);
            }
        }
    }

    /// A capture arm's direct write into the TR x global `0x800840B8` - only
    /// PROT 0966's arms walk it. Folded into the armed shot's target like
    /// [`Self::drift_module`], when the shot does not move TR x itself.
    pub fn drift_module_tr_x(&mut self, tr_x: i16) {
        self.pose.tr[0] += f32::from(tr_x);
        if let Some(g) = self.module_glide.as_mut().filter(|g| !g.landed())
            && g.rate[2] == 0.0
        {
            g.target.tr[0] += f32::from(tr_x);
        }
    }

    fn land(&mut self, g: &Glide) {
        self.pose.pitch = g.target.pitch;
        self.pose.tr = g.target.tr;
        self.pose.focus = g.target.focus;
        self.pose.yaw = g.target.yaw.rem_euclid(4096.0);
    }

    /// Whether the cast-effect driver's shot handed in by the last tick is
    /// still waiting for a camera step to arm it. The action SM runs one
    /// pass a battle frame and the camera steps once a frame on its own
    /// clock, so the hand-off holds until a step takes it.
    pub fn spell_cam_pending(&self) -> bool {
        self.spell_cam.is_some()
    }

    /// Phase-align a module shot still walking to a retail instant: every
    /// component whose shot endpoint agrees with retail's tween-table
    /// endpoint (`end`) takes retail's `live` value, and the walk goes on
    /// from there on the shot's own increments. Returns the components
    /// moved, in [`Self::align_glide_origin`]'s bit order.
    ///
    /// A module arm that polls the creature stream (`FUN_8003F2B8(1)`; PROT
    /// 0905's arm 3, PROT 0910 / 0913's arm 2) holds for as long as the
    /// drive's read takes, while the shot armed before it walks on; the
    /// engine's stream is resident and the arm passes on its first tick. So
    /// the shot's progress at a capture taken in the wait is the read's
    /// length, which no word the replay carries records:
    /// `vera_summon_mid_cast` stands `72` vsyncs into arm 1's walk, where a
    /// PCSX-Redux cast of the same module enters arm 3 `65` vsyncs in and
    /// leaves it two passes later. The capture
    /// harness's handle, the module twin of [`Self::align_glide_origin`];
    /// nothing in play calls it.
    pub fn align_module_shot(&mut self, live: BattleCamPose, end: BattleCamPose) -> u8 {
        use super::camera_step::{
            ALIGN_ANGLE_TOLERANCE, ALIGN_FOCUS_TOLERANCE, ALIGN_TR_TOLERANCE,
        };
        let Some(g) = self.module_glide.filter(|g| !g.landed()) else {
            return 0;
        };
        let ang = |a: f32, b: f32| ((a - b).rem_euclid(4096.0)).min((b - a).rem_euclid(4096.0));
        let nearest = |from: f32, to: f32| {
            let d = (to - from).rem_euclid(4096.0);
            from + if d > 2048.0 { d - 4096.0 } else { d }
        };
        let mut mask = 0u8;
        if ang(g.target.pitch, end.pitch) <= ALIGN_ANGLE_TOLERANCE {
            self.pose.pitch = nearest(self.pose.pitch, live.pitch);
            mask |= 1;
        }
        if ang(g.target.yaw, end.yaw) <= ALIGN_ANGLE_TOLERANCE {
            self.pose.yaw = nearest(self.pose.yaw, live.yaw);
            mask |= 2;
        }
        for k in 0..3 {
            if (g.target.tr[k] - end.tr[k]).abs() <= ALIGN_TR_TOLERANCE {
                self.pose.tr[k] = live.tr[k];
                mask |= 4 << k;
            }
            if (g.target.focus[k] - end.focus[k]).abs() <= ALIGN_FOCUS_TOLERANCE {
                self.pose.focus[k] = live.focus[k];
                mask |= 32 << k;
            }
        }
        mask
    }

    /// Whether a module shot is armed (tweening or landed and holding).
    pub fn module_shot_armed(&self) -> bool {
        self.module_glide.is_some()
    }

    /// One camera step while the module owns the camera: walk the armed shot,
    /// land it on its last step, and otherwise hold the landed pose.
    pub(super) fn step_module_shot(&mut self) {
        let Some(g) = self.module_glide else {
            return;
        };
        match g.steps_left {
            Some(0) => {}
            Some(1) => {
                self.land(&g);
                if let Some(m) = self.module_glide.as_mut() {
                    m.steps_left = Some(0);
                }
            }
            Some(n) => {
                if let Some(m) = self.module_glide.as_mut() {
                    m.steps_left = Some(n - 1);
                }
                self.step_components(&g);
            }
            None if g.per_vsync => {
                // The walker's `increment * frame_step`, on this pass's step.
                let mut walk = g;
                let step = f32::from(self.frame_step.max(1));
                for r in &mut walk.rate {
                    *r *= step;
                }
                self.step_components(&walk);
                // Every component on its endpoint: the walker task ends, and
                // a drift after it survives.
                let p = &self.pose;
                if p.pitch == g.target.pitch
                    && p.yaw == g.target.yaw
                    && p.tr == g.target.tr
                    && p.focus == g.target.focus
                {
                    self.land(&g);
                    if let Some(m) = self.module_glide.as_mut() {
                        m.steps_left = Some(0);
                    }
                }
            }
            None => self.step_components(&g),
        }
    }

    /// The granted-flee shot. On success the escape roll snaps the live
    /// camera globals - pitch / roll `0`, yaw [`ESCAPE_SNAP_YAW`], TR
    /// `(0, 0x600, 0x2000)` in the prescaled units the globals hold, focus at
    /// the origin (`0x801E7E98..0x801E7EE4`) - and then hands
    /// `FUN_801D829C` the reverse-angle target: yaw `0x800`, TR
    /// `(0, 0x600, 0)`, focus `-(0, 0, -0x400)` - the regrouped party's
    /// centre - over `0x30` frames (`0x801E7FE4..0x801E8030`). The shot then
    /// holds until the battle ends: the run band's `0x65` / `0x66` and the
    /// teardown hold call no framing case.
    ///
    /// REF: FUN_801E791C (the success arm's camera writes), FUN_801D829C
    pub fn arm_escape_shot(&mut self) {
        self.pose = BattleCamPose {
            pitch: 0.0,
            yaw: ESCAPE_SNAP_YAW,
            tr: [0.0, 1536.0, 8192.0],
            focus: [0.0, 0.0, 0.0],
        };
        let target = ModuleShot {
            angles: [0, 0x800, 0],
            tr: [0, 0x600, 0],
            focus: [0, 0, -0x400],
            frames: 0x30,
        };
        let (pose, raw_z) = target.pose();
        self.arm_module_shot(pose, raw_z, u32::from(target.frames));
        self.escape_shot = true;
    }

    /// Drop the module's shot on the way out of its states.
    pub(super) fn release_module_shot(&mut self, state: u8) {
        if self.escape_shot {
            return;
        }
        if !SUMMON_MODULE_STATES.contains(&state) && state != 0x34 && state != CAPTURE_MODULE_STATE
        {
            self.module_glide = None;
        }
    }
}

/// One `FUN_801D829C` call out of a module arm, in retail's own value space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleShot {
    /// `sp+0x20..0x24`: pitch, yaw, roll (12-bit units).
    pub angles: [i16; 3],
    /// `sp+0x28..0x2C`: the translation trio, TR z un-prescaled.
    pub tr: [i16; 3],
    /// `sp+0x30..0x34`: the focus trio, negated world position.
    pub focus: [i16; 3],
    /// `a3`: the tween duration in display frames.
    pub frames: u16,
}

impl ModuleShot {
    /// The shot as the battle camera holds a pose: yaw masked to 12 bits,
    /// TR z prescaled, the focus un-negated. Returns the raw TR z beside it,
    /// which is what the tween builder is handed.
    pub fn pose(&self) -> (BattleCamPose, i32) {
        let raw_z = i32::from(self.tr[2]);
        (
            BattleCamPose {
                pitch: f32::from(self.angles[0]),
                yaw: f32::from(self.angles[1] & 0xFFF),
                tr: [
                    f32::from(self.tr[0]),
                    f32::from(self.tr[1]),
                    prescale_tr_z(raw_z),
                ],
                focus: [
                    -f32::from(self.focus[0]),
                    -f32::from(self.focus[1]),
                    -f32::from(self.focus[2]),
                ],
            },
            raw_z,
        )
    }
}

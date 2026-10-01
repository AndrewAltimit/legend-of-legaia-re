//! The summon module's own shots, as the battle camera steps them.

use super::camera::Glide;
use super::*;

/// The action-SM states whose camera belongs to the summon module: the
/// sustain `0x35` and the return `0x36` re-enter the paged module through
/// `FUN_801F1ED4` every pass and call no framing case themselves
/// (`docs/subsystems/battle-action.md`). The module's arm-0 shot lands on
/// `0x34`'s last pass, the same one that leaves for `0x35`.
pub const SUMMON_MODULE_STATES: [u8; 2] = [0x35, 0x36];

/// The yaw the escape roll's success arm snaps the live camera to before it
/// arms the flee shot (`li v0,0xf00 ; sh v0,0x2(a1)` at `0x801E7EE0`, into
/// `0x8007B792`).
pub const ESCAPE_SNAP_YAW: f32 = 3840.0;

impl BattleCamera {
    /// Arm one of the summon module's `FUN_801D829C` calls: a tween from the
    /// live pose to `target` over `frames` display frames, stepped on the
    /// camera's 2-frame cadence like every framing case's. A later shot
    /// rebuilds from wherever the previous one has reached, exactly as a
    /// second builder call does.
    ///
    /// `raw_tr_z` is the target's TR z in world units, the value the module
    /// hands the builder.
    ///
    /// REF: FUN_801D829C
    pub fn arm_module_shot(&mut self, target: BattleCamPose, raw_tr_z: i32, frames: u32) {
        let mut from = self.pose;
        let steps = (frames / 2).max(1);
        let mut g = Glide::linear(&mut from, target, raw_tr_z, steps, true);
        self.pose = from;
        if steps == 1 {
            // A one-frame tween is a cut: retail's walker lands it the frame
            // it is armed, before the next arm can rebuild from it. The
            // camera's 2-frame cadence would otherwise let a following shot
            // start from the pose the cut replaced.
            self.land(&g);
            g.steps_left = Some(0);
        }
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
        let g = Glide::linear(&mut from, target, f.raw_z(), ACTION_STEPS, true);
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

    fn land(&mut self, g: &Glide) {
        self.pose.pitch = g.target.pitch;
        self.pose.tr = g.target.tr;
        self.pose.focus = g.target.focus;
        self.pose.yaw = g.target.yaw.rem_euclid(4096.0);
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
        let target = crate::cast_module_camera::ModuleShot {
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
        if !SUMMON_MODULE_STATES.contains(&state) && state != 0x34 {
            self.module_glide = None;
        }
    }
}

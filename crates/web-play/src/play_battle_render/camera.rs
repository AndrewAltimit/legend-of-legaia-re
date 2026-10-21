//! The battle camera: the pose and view-projection exports, the phase label
//! and the draw plan.

use super::*;

#[wasm_bindgen]
impl LegaiaRuntime {
    /// The battle camera pose for this frame, in the retail value space:
    /// `{"active":true,"pitch":P,"yaw":Y,"tr":[x,y,z],"focus":[x,y,z],
    /// "h":256}` - pitch/yaw in PSX 12-bit units, `tr` the eye-space
    /// translation trio (TR.z already through the `(z << 8) / 0xA0`
    /// projection prescale), `focus` the world point the camera orbits in
    /// RAW battle units. The live pose of the SAME phase-scripted camera
    /// the native window runs ([`legaia_engine_battle_vm::battle_cam_script`]:
    /// dialogue close-up / far menu framing with the idle orbit /
    /// per-character submenu close-up, with the measured glides between).
    /// Kept as a diagnostic/value-space export; the page's projection
    /// consumes [`Self::play_battle_camera_vp`] instead.
    pub fn play_battle_camera_json(&self) -> String {
        if !self.play_battle_active() {
            return r#"{"active":false}"#.to_string();
        }
        let pose = self.battle_cam_pose();
        format!(
            r#"{{"active":true,"phase":"{}","pitch":{},"yaw":{},"tr":[{},{},{}],"focus":[{},{},{}],"h":{}}}"#,
            self.battle_cam_phase_label(),
            pose.pitch,
            pose.yaw,
            pose.tr[0],
            pose.tr[1],
            pose.tr[2],
            pose.focus[0],
            pose.focus[1],
            pose.focus[2],
            legaia_engine_camera::projection::GTE_H,
        )
    }

    /// The battle view-projection matrix for this frame: 16 column-major
    /// floats (WebGL `mat4` layout), or empty outside battle. The shared
    /// [`legaia_engine_camera::projection::battle_vp`] - the native
    /// window's `psx_camera_mvp` composition - over the live phase-scripted
    /// pose, with the retail 4x battle world scale folded into the focus
    /// exactly like the native `battle_dome_camera_mvp`. The page hands
    /// this straight to its renderer (`cam.vp`), so there is no JS-side
    /// projection model to drift.
    pub fn play_battle_camera_vp(&self, aspect: f32) -> Vec<f32> {
        if !self.play_battle_active() {
            return Vec::new();
        }
        let pose = self.battle_cam_pose();
        legaia_engine_camera::projection::battle_vp(&pose, BATTLE_WORLD_SCALE, aspect).to_vec()
    }
}

impl LegaiaRuntime {
    /// The live phase-scripted pose, or the shared BOOT_POSE on the first
    /// frame before the camera tick armed the state (the same fallback the
    /// native `battle_dome_camera_mvp` renders).
    /// Which framing the shared phase script has the camera in, as the JSON
    /// export's `phase` string.
    fn battle_cam_phase_label(&self) -> &'static str {
        use legaia_engine_battle_vm::battle_cam_script::BattleCamPhase as P;
        match self
            .scene_host
            .host()
            .and_then(|h| h.world.battle.camera.as_ref())
            .map(|c| c.phase())
        {
            Some(P::Dialogue) => "dialogue",
            Some(P::Submenu) => "submenu",
            Some(P::Action) => "action",
            Some(P::Recover) => "recover",
            Some(P::ActionEnd) => "action-end",
            Some(P::TargetEnemy) => "target-enemy",
            Some(P::TargetAlly) => "target-ally",
            Some(P::Menu) | None => "menu",
        }
    }

    /// Battle body `actor_idx`'s draw decision this frame - the shared
    /// `World::battle_actor_draw_plan` under the camera this page projects
    /// with, judged at retail's parked depth when no stage dome is up (the
    /// native window's rule).
    pub(crate) fn battle_draw_plan(
        &self,
        actor_idx: usize,
    ) -> Option<legaia_engine_core::world::BattleActorDrawPlan> {
        let br = self.battle_render.as_ref()?;
        let host = self.scene_host.host()?;
        let pose = br.backdrop.is_some().then(|| self.battle_cam_pose());
        host.world
            .battle_actor_draw_plan(actor_idx, pose.as_ref(), BATTLE_WORLD_SCALE, br.outdoor)
    }

    /// Battle body `actor_idx`'s body-to-view map under the same camera, for
    /// the shatter renderer's sweep rows (`World::battle_shatter_view`).
    pub(crate) fn battle_shatter_view(&self, actor_idx: usize) -> [[f32; 3]; 3] {
        let backdrop = self
            .battle_render
            .as_ref()
            .is_some_and(|b| b.backdrop.is_some());
        let pose = backdrop.then(|| self.battle_cam_pose());
        self.scene_host
            .host()
            .map_or(legaia_render_kernels::shatter::BODY_AXES, |h| {
                h.world
                    .battle_shatter_view(actor_idx, pose.as_ref(), BATTLE_WORLD_SCALE)
            })
    }

    /// The engine's battle-camera pose (`World::battle_cam_pose` - the state
    /// `World::tick` steps for every host), or the shared boot pose off a
    /// scene host.
    pub(crate) fn battle_cam_pose(&self) -> legaia_engine_camera::projection::BattleCamPose {
        self.scene_host
            .host()
            .map(|h| h.world.battle_cam_pose())
            .unwrap_or(legaia_engine_battle_vm::battle_cam_script::BOOT_POSE)
    }
}

//! The play page's camera: the engine's, not the page's.
//!
//! This host used to carry no [`legaia_engine_core::camera::Camera`] at all.
//! The page ran a spherical orbit projection (`buildWorldOrbitVp` - yaw,
//! pitch, a half-window) that *approximated* the retail GTE camera, fed the
//! engine an azimuth derived from its own yaw, and re-mapped the op-`0x45`
//! cutscene params onto that orbit; nothing on this side routed the Camera
//! Configure beats into a controller, advanced the mover, wrote the follow
//! focus back into the retail camera globals, or reset them on scene entry.
//! The native window did all of that. Two hosts, one engine, two cameras.
//!
//! Now both hosts resolve the frame through
//! [`legaia_engine_core::camera_view`] and upload the matrix it returns. The
//! page keeps exactly one camera of its own - the drag-orbit debug vantage -
//! and it is an explicit override with the native window's `F3` semantics,
//! not the default projection.
//!
//! REF: FUN_801DE084 (the op-`0x45` Configure apply handler), FUN_801DBE9C
//! (the field follow camera).

use crate::runtime::LegaiaRuntime;
use legaia_engine_core::camera_view::{self, FieldCameraFrame};
use legaia_engine_core::world::SceneMode;
use wasm_bindgen::prelude::*;

impl LegaiaRuntime {
    /// Advance the engine camera one tick, in the native session's order
    /// (`BootSession::tick`): snap back to the follow default in free-roam,
    /// publish the compass azimuth the d-pad remap reads, route this tick's
    /// op-`0x45` events into the controller, then advance the retail globals.
    /// A scene entry resets the globals so a departing scene's shot cannot
    /// leak its focus or eye depth into the next one (`FUN_80025C24`).
    pub(crate) fn tick_camera(&mut self, scene_entered: bool) {
        let Some(host) = self.scene_host.as_mut() else {
            return;
        };
        self.camera.reset_for_free_roam(&host.world);
        // The engine camera publishes the azimuth unless the page spoke over
        // it this tick (VR first-person, where the headset gaze is the
        // heading).
        let az = self
            .camera_azimuth_override
            .take()
            .unwrap_or_else(|| self.camera.compass_azimuth_units());
        host.world.locomotion.camera_azimuth = az % 4096;
        self.camera.route_camera_events(&mut host.world);
        self.camera.tick(&host.world);
        if scene_entered {
            self.camera.reset_globals_for_scene_entry();
            self.cutscene_cam.reset();
        }
    }

    /// The scene AABB the world map's top-view debug camera frames: the
    /// **world-space** union of the scene's static env draws, through the
    /// shared kernel `engine_core::field_env::env_draws_world_aabb` the
    /// native window also calls. Built on first use and cached until the next
    /// scene rebuild - no other camera reads it, so a field session never pays
    /// for it.
    ///
    /// It used to be the union of the built meshes' *local* extents, which
    /// boxes the authoring origin rather than the map: every env-pack mesh is
    /// authored about its own origin while the geometry that draws sits at
    /// placement coordinates. The top view framed the origin corner and the
    /// map sat off-centre.
    fn scene_aabb(&mut self) -> ([f32; 3], [f32; 3]) {
        if let Some(b) = self.scene_aabb {
            return b;
        }
        const EMPTY: ([f32; 3], [f32; 3]) = ([-1.0; 3], [1.0; 3]);
        let out = match (self.field.as_ref(), self.res()) {
            (Some(f), Some(res)) => legaia_engine_core::field_env::env_draws_world_aabb(
                &[&f.terrain, &f.placements],
                res,
                f.ground.as_ref(),
            )
            .unwrap_or(EMPTY),
            _ => EMPTY,
        };
        self.scene_aabb = Some(out);
        out
    }

    /// This frame's resolved camera, with the cutscene glide advanced.
    ///
    /// The cutscene gate is the native window's: a running timeline owns the
    /// camera, except on the world map, where it only takes it when a beat
    /// actually staged a param (a world-map beat record with no camera beats -
    /// the Drake mist-wall force-walk bands - keeps the walk camera).
    fn resolve_camera_frame(&mut self) -> FieldCameraFrame {
        let Some(host) = self.scene_host.as_ref() else {
            return FieldCameraFrame::HostDebugOrbit;
        };
        let world = &host.world;
        let scripted = world.cutscene_timeline_active()
            && (world.mode != SceneMode::WorldMap || !world.camera.state.params.is_empty());
        let cutscene = if scripted {
            let target = camera_view::cutscene_view(world, [0.0, 0.0]);
            let apply = u32::from(world.camera.state.apply_trigger);
            let mode = world.camera.state.mode;
            let now = world.clock.display_frames;
            let steps = u32::try_from(now.saturating_sub(self.cutscene_cam_frames))
                .unwrap_or(u32::MAX)
                .max(1);
            self.cutscene_cam_frames = now;
            Some(self.cutscene_cam.glide_view(target, apply, mode, steps))
        } else {
            self.cutscene_cam.reset();
            None
        };
        let world = &self.scene_host.as_ref().expect("checked above").world;
        camera_view::resolve_field_camera(world, &self.camera, cutscene, [0.0, 0.0])
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// This frame's engine view-projection, 16 floats column-major (the
    /// layout a WebGL `uniformMatrix4fv` takes), for a `view_w` x `view_h`
    /// canvas.
    ///
    /// Empty when the engine has no camera for this frame - no player actor
    /// and no scripted shot - which is where the page falls back to its own
    /// debug vantage, the same fallback the native window's
    /// `field_follow_camera_mvp` takes.
    ///
    /// The matrix is for the page's **Y-up** render frame: the page's model
    /// matrices carry the PSX `scale(1,-1,1)` and the projection's trailing
    /// flip cancels it, so the retail chain sees the raw Y-down vertex. The
    /// native window keeps its world state Y-down instead and post-multiplies
    /// one more flip onto this same matrix.
    pub fn play_camera_vp(&mut self, view_w: f32, view_h: f32) -> Vec<f32> {
        let aspect = view_w.max(1.0) / view_h.max(1.0);
        let frame = self.resolve_camera_frame();
        let aabb = match frame {
            FieldCameraFrame::WorldMapTopView { .. } => self.scene_aabb(),
            _ => ([0.0; 3], [0.0; 3]),
        };
        camera_view::frame_vp(&frame, aabb, aspect)
            .map(|m| m.to_vec())
            .unwrap_or_default()
    }

    /// This frame's world-space lens, in raw retail Y-down world coordinates -
    /// what the camera-occlusion fade's visibility gate ray-casts from
    /// ([`Self::field_player_occluded`]). Empty where the frame has no
    /// retail-model eye (the top-view debug camera, or the page's own
    /// vantage).
    pub fn play_camera_eye(&mut self) -> Vec<f32> {
        let frame = self.resolve_camera_frame();
        camera_view::frame_eye(&frame)
            .map(|e| e.to_vec())
            .unwrap_or_default()
    }

    /// This frame's camera inputs as JSON, for the page's diagnostics and for
    /// the parity oracles:
    /// ```text
    /// { "arm": "follow" | "cutscene" | "worldmap_walk" | "worldmap_topview"
    ///          | "host_debug_orbit",
    ///   "focus": [x, y, z], "pitch": rad, "yaw": rad, "roll": rad,
    ///   "h": f, "tr": [x, y, z],         // absent on the two orbit arms
    ///   "zone": true | false }           // follow arm composed from the
    ///                                    // scene's camera-region record
    /// ```
    pub fn play_camera_view_json(&mut self) -> String {
        let frame = self.resolve_camera_frame();
        let (arm, view) = match &frame {
            FieldCameraFrame::Cutscene(v) => ("cutscene", Some(*v)),
            FieldCameraFrame::Follow(v) => ("follow", Some(*v)),
            FieldCameraFrame::WorldMapWalk { view, .. } => ("worldmap_walk", Some(*view)),
            FieldCameraFrame::WorldMapTopView { .. } => ("worldmap_topview", None),
            FieldCameraFrame::HostDebugOrbit => ("host_debug_orbit", None),
        };
        match view {
            Some(v) => serde_json::json!({
                "arm": arm,
                "focus": v.focus, "pitch": v.pitch, "yaw": v.yaw, "roll": v.roll,
                "h": v.h, "tr": v.tr_eye,
                "zone": self.camera.zone.active,
            })
            .to_string(),
            None => serde_json::json!({ "arm": arm }).to_string(),
        }
    }

    /// The GTE `H` the engine camera is projecting through this frame - the
    /// live camera global with `camera_view`'s field fallback applied. The
    /// parity test compares the page's resolved frame against this rather
    /// than against a pinned constant: a scene's entry beat may glide slot
    /// `9` (town01's names `500`), so the value is a per-frame output.
    pub fn play_camera_gte_h(&self) -> f32 {
        match self.camera.globals.h() {
            0 => camera_view::FIELD_H,
            v => v as f32,
        }
    }

    /// The compass azimuth the engine camera is publishing this tick
    /// (`Camera::compass_azimuth_units`), for the page's diagnostics and the
    /// compass oracle.
    pub fn debug_compass_azimuth(&self) -> u16 {
        self.camera.compass_azimuth_units()
    }

    /// The zone-driven follow camera's live `(pitch, yaw)` in PSX 12-bit units
    /// - the composed `_DAT_8007B790/92` the engine eases per frame - or
    /// empty while no scene terrain drives it. What the parity oracle
    /// compares the page's follow view against: the value is per scene and
    /// per tile, not a pin.
    pub fn play_camera_follow_angles(&self) -> Vec<i32> {
        match self.camera.zone_follow_yaw_units() {
            Some(yaw) => vec![i32::from(self.camera.globals.0[0] as i16), yaw],
            None => Vec::new(),
        }
    }

    /// The user's drag-orbit around the follow target, radians, compass sense
    /// - [`legaia_engine_core::camera::Camera::manual_orbit`], the same field
    /// the native window's left-mouse drag writes. Feeding it here rather
    /// than keeping a page-local yaw is what keeps the movement compass
    /// (`compass_azimuth_units`) tracking the view on both hosts.
    pub fn play_camera_set_orbit(&mut self, radians: f32) {
        self.camera.manual_orbit = radians.rem_euclid(std::f32::consts::TAU);
    }

    /// Current drag-orbit, radians.
    pub fn play_camera_orbit(&self) -> f32 {
        self.camera.manual_orbit
    }

    /// Cycle the camera-distance preset (retail -> far -> farther), the
    /// native window's `T`. Returns the new preset's label.
    pub fn play_camera_cycle_distance(&mut self) -> String {
        self.camera.distance = self.camera.distance.cycle();
        self.camera.distance.label().to_string()
    }

    /// `true` while the world map's **top-view debug camera** is live - the
    /// vantage retail's R1+R2+Cross chord toggles
    /// (`legaia_engine_core::world_map::WorldMapController`). The toggle
    /// itself is engine-side and identical on both hosts; this is only the
    /// readout a page needs to label its HUD.
    pub fn play_camera_is_top_view(&self) -> bool {
        self.scene_host
            .as_ref()
            .and_then(|h| h.world.world_map.ctrl.as_ref())
            .is_some_and(|c| c.is_top_view())
    }
}

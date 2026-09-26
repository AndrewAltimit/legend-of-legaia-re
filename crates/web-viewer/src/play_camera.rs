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

    /// Does a scripted shot own this frame's camera? The gate
    /// [`Self::resolve_camera_frame`] runs, without its side effects: that
    /// one advances the cutscene glide and drains the snap-beat bank, so an
    /// export that only needs the verdict must not call it.
    pub(crate) fn cutscene_owns_frame(&self) -> bool {
        let Some(host) = self.scene_host.as_ref() else {
            return false;
        };
        let world = &host.world;
        world.cutscene_timeline_active()
            && (world.mode != SceneMode::WorldMap || !world.camera.state.params.is_empty())
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
    pub(crate) fn scene_aabb(&mut self) -> ([f32; 3], [f32; 3]) {
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
    ///
    /// Read-only sibling: [`Self::cutscene_owns_frame`] answers the gate
    /// alone, for the exports that need the answer without advancing the
    /// glide - this one is a per-frame STEP, not a query.
    fn resolve_camera_frame(&mut self) -> FieldCameraFrame {
        if self.scene_host.is_none() {
            return FieldCameraFrame::HostDebugOrbit;
        }
        // The host's fallback focus: the loaded scene's own centre, the same
        // quantity the native window passes (`scene_aabb` mid X/Z). It backs
        // a cutscene beat that stages no focus slot with no lead actor live,
        // and the overworld walk arm's player position; a pinned `[0, 0]`
        // framed the world origin instead of the map.
        let centre = {
            let (lo, hi) = self.scene_aabb();
            [(lo[0] + hi[0]) * 0.5, (lo[2] + hi[2]) * 0.5]
        };
        let scripted = self.cutscene_owns_frame();
        let host = self.scene_host.as_ref().expect("checked above");
        let world = &host.world;
        let cutscene = if scripted {
            let target = camera_view::cutscene_view(world, centre);
            let apply = u32::from(world.camera.state.apply_trigger);
            let mode = world.camera.state.mode;
            let now = world.clock.display_frames;
            let steps = u32::try_from(now.saturating_sub(self.cutscene_cam_frames))
                .unwrap_or(u32::MAX)
                .max(1);
            self.cutscene_cam_frames = now;
            // Retail snaps the live globals to an `apply == 0` beat
            // immediately, so a snap+glide pair committed in ONE world tick
            // glides FROM the snapped pose. The beats are banked on the
            // engine camera (`Camera::take_camera_snap_beats`) because
            // `route_camera_events` is what consumes them off the world
            // queue; the native window replays the same bank.
            for comps in self.camera.take_camera_snap_beats() {
                self.cutscene_cam.snap_components(&comps);
            }
            Some(self.cutscene_cam.glide_view(target, apply, mode, steps))
        } else {
            self.cutscene_cam.reset();
            // Nothing is interpolating, so a banked snap would move a pose
            // no draw reads.
            self.camera.clear_camera_snap_beats();
            None
        };
        let world = &self.scene_host.as_ref().expect("checked above").world;
        camera_view::resolve_field_camera(world, &self.camera, cutscene, centre)
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
    /// `compute_scene_camera` takes on `HostDebugOrbit`.
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
    ///   "zone": true | false,            // follow arm composed from the
    ///                                    // scene's camera-region record
    ///   "knobs": { "orbit": rad, "tilt": rad, "zoom": f, "live": bool } }
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
        let knobs = serde_json::json!({
            "orbit": self.camera.manual_orbit,
            "tilt": self.camera.manual_tilt,
            "zoom": self.camera.manual_zoom,
            "live": self.play_camera_knobs_live(),
        });
        match view {
            Some(v) => serde_json::json!({
                "arm": arm,
                "focus": v.focus, "pitch": v.pitch, "yaw": v.yaw, "roll": v.roll,
                "h": v.h, "tr": v.tr_eye,
                "zone": self.camera.zone.active,
                "knobs": knobs,
            })
            .to_string(),
            None => serde_json::json!({ "arm": arm, "knobs": knobs }).to_string(),
        }
    }

    /// This frame's retail GTE **NCLIP** winding-rejection mode for the
    /// scene pass - the word the page hands `TmdRenderer.setNclipCull`, from
    /// the shared [`camera_view::nclip_cull_mode`] the native window's
    /// `Renderer::set_backface_cull` call also reads. `2` only while the
    /// in-engine cutscene camera owns a non-overworld frame; `0` otherwise,
    /// which is both-sided drawing.
    pub fn play_render_nclip_mode(&self) -> u32 {
        let in_world_map = self
            .scene_host
            .as_ref()
            .is_some_and(|h| h.world.mode == SceneMode::WorldMap);
        camera_view::nclip_cull_mode(self.cutscene_owns_frame(), in_world_map)
    }

    /// This frame's overworld-curvature scale for the scene pass - the
    /// `clip.w`-to-`SZ` factor the page hands `TmdRenderer.setOverworldCurve`
    /// (the GLSL `u_curve`), from the shared
    /// `legaia_engine_core::overworld_curvature::frame_curve_scale` the
    /// native window's `Renderer::set_overworld_curvature` call also reads.
    /// `0` (flat) off the kingdom overworld and under the top-view debug
    /// camera. A query, not a step: it resolves the frame's **arm** without
    /// advancing the cutscene glide, and the scale depends on nothing else.
    pub fn play_render_curve_scale(&mut self) -> f32 {
        if !self
            .scene_host
            .as_ref()
            .is_some_and(|h| h.world.overworld_bit())
        {
            return 0.0;
        }
        let centre = {
            let (lo, hi) = self.scene_aabb();
            [(lo[0] + hi[0]) * 0.5, (lo[2] + hi[2]) * 0.5]
        };
        let world = &self.scene_host.as_ref().expect("checked above").world;
        let cutscene = self
            .cutscene_owns_frame()
            .then(|| camera_view::cutscene_view(world, centre));
        let frame = camera_view::resolve_field_camera(world, &self.camera, cutscene, centre);
        legaia_engine_core::overworld_curvature::frame_curve_scale(true, &frame)
    }

    /// The camera-occlusion fade's focus point for this frame, in the page's
    /// **Y-up draw frame**, or empty when the engine side of the arming gate
    /// says no ([`legaia_engine_core::field_occlusion::fade_armed`] + a live
    /// player actor).
    ///
    /// It is the same point [`Self::field_player_occluded`] ray-casts to -
    /// the shared `player_body_centre` kernel - which is the whole reason it
    /// is an export rather than three lines of JS: the page used to stage the
    /// actor's own `world_y` while the gate tested the floor tier under it,
    /// so on any tile where those differ the dissolve hole sat off the
    /// character. The host's own terms (its master toggle, a pause menu or
    /// name-entry overlay owning the screen, the `F3` debug vantage, a VR
    /// first-person eye) stay on the page.
    pub fn play_occlusion_focus(&self) -> Vec<f32> {
        let cutscene = self.cutscene_owns_frame();
        let Some(host) = self.scene_host.as_ref() else {
            return Vec::new();
        };
        if !legaia_engine_core::field_occlusion::fade_armed(&host.world, cutscene) {
            return Vec::new();
        }
        match legaia_engine_core::field_occlusion::player_body_centre(&host.world) {
            // Retail Y-down -> the page's Y-up draw frame.
            Some(c) => vec![c[0], -c[1], c[2]],
            None => Vec::new(),
        }
    }

    /// Project the lead onto the 240-line stage and hand the result to the
    /// field party HUD's decision kernel - the browser twin of the native
    /// window's `field_hud_projected_player_y`, and now the same derivation:
    /// the actor origin raised `0x80` (retail's own `addiu v0,v0,-0x80`)
    /// through **this frame's camera with the cutscene arm suppressed**.
    ///
    /// The page used to project in JS through the live draw VP, so under a
    /// scripted shot the readout's band test ran against the cutscene
    /// framing while the native window's ran against the follow camera.
    /// `[`Self::set_field_player_screen_y`]` stays for the headless oracles.
    pub fn play_field_hud_project(&mut self, view_w: f32, view_h: f32) {
        let aspect = view_w.max(1.0) / view_h.max(1.0);
        // The HUD band is the FOLLOW camera's reading: a scripted shot must
        // not decide where the readout sits.
        let aabb = self.scene_aabb();
        let centre = [(aabb.0[0] + aabb.1[0]) * 0.5, (aabb.0[2] + aabb.1[2]) * 0.5];
        let Some(host) = self.scene_host.as_ref() else {
            self.set_field_player_screen_y(crate::runtime::NO_FIELD_PROJECTION);
            return;
        };
        let world = &host.world;
        let pos = world
            .player_actor_slot
            .map(usize::from)
            .and_then(|s| world.actors.get(s))
            .filter(|a| a.active || a.tmd_binding.is_some())
            .map(|a| {
                [
                    a.move_state.world_x as f32,
                    a.move_state.world_y as f32 - 128.0,
                    a.move_state.world_z as f32,
                ]
            });
        let frame = camera_view::resolve_field_camera(world, &self.camera, None, centre);
        let out = match (pos, camera_view::frame_vp(&frame, aabb, aspect)) {
            (Some(p), Some(m)) => {
                // Column-major 4x4: row 1 is the Y row, row 3 the W row.
                let cy = m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13];
                let cw = m[3] * p[0] + m[7] * p[1] + m[11] * p[2] + m[15];
                if cw > 0.0 {
                    // NDC has +Y up; the stage is 240 lines with +Y down.
                    ((1.0 - cy / cw) * 120.0).clamp(-4096.0, 4096.0) as i32
                } else {
                    crate::runtime::NO_FIELD_PROJECTION
                }
            }
            _ => crate::runtime::NO_FIELD_PROJECTION,
        };
        self.set_field_player_screen_y(out);
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

    /// Swing the orbit by `radians` from the page's **debug orbit** vantage
    /// (`F3`), through the shared
    /// [`legaia_engine_core::camera::Camera::debug_orbit_by`] the native
    /// window's own `F3` drag calls. Un-gated by the cutscene (the debug
    /// vantage is a dev viewpoint) but field-only.
    ///
    /// Both hosts compose that vantage as `fixed diagonal + manual_orbit`, so
    /// steering the one field is what makes leaving the toggle continuous.
    /// The page used to steer a private yaw while `F3` was on and then write
    /// its NEGATION into `manual_orbit` on the way out - which, since the two
    /// track together with `F3` off, flipped the orbit by twice its value
    /// every time the toggle was cycled.
    pub fn play_camera_debug_orbit_by(&mut self, radians: f32) -> bool {
        match self.scene_host.as_ref() {
            Some(h) => self.camera.debug_orbit_by(&h.world, radians),
            None => false,
        }
    }

    /// Whether the user's follow-camera knobs steer this frame
    /// ([`legaia_engine_core::camera::Camera::follow_knobs_live`]): free-roam
    /// field with no cutscene timeline owning the camera. `false` while a
    /// scripted shot runs - the camera is locked where the script put it -
    /// and off the field.
    pub fn play_camera_knobs_live(&self) -> bool {
        self.scene_host
            .as_ref()
            .is_some_and(|h| self.camera.follow_knobs_live(&h.world))
    }

    /// Swing the follow camera's orbit by `radians` (compass sense) through
    /// the cutscene-gated engine setter
    /// ([`legaia_engine_core::camera::Camera::orbit_by`]). Returns whether
    /// the gesture was taken; a drag during a cutscene is dropped rather
    /// than banked, so the view never snaps when control returns.
    pub fn play_camera_orbit_by(&mut self, radians: f32) -> bool {
        match self.scene_host.as_ref() {
            Some(h) => self.camera.orbit_by(&h.world, radians),
            None => false,
        }
    }

    /// Tip the follow camera's tilt by `radians` (positive = further down),
    /// gated like [`Self::play_camera_orbit_by`]
    /// ([`legaia_engine_core::camera::Camera::tilt_by`]).
    pub fn play_camera_tilt_by(&mut self, radians: f32) -> bool {
        match self.scene_host.as_ref() {
            Some(h) => self.camera.tilt_by(&h.world, radians),
            None => false,
        }
    }

    /// Scale the follow camera's continuous zoom by `factor` (`> 1` pulls
    /// the eye back), gated like [`Self::play_camera_orbit_by`]
    /// ([`legaia_engine_core::camera::Camera::zoom_by`]).
    pub fn play_camera_zoom_by(&mut self, factor: f32) -> bool {
        match self.scene_host.as_ref() {
            Some(h) => self.camera.zoom_by(&h.world, factor),
            None => false,
        }
    }

    /// Current follow-camera tilt, radians.
    pub fn play_camera_tilt(&self) -> f32 {
        self.camera.manual_tilt
    }

    /// Current follow-camera zoom multiplier.
    pub fn play_camera_zoom(&self) -> f32 {
        self.camera.manual_zoom
    }

    /// Put orbit, tilt and zoom back at their retail-identical defaults
    /// ([`legaia_engine_core::camera::Camera::reset_follow_knobs`]).
    pub fn play_camera_reset_framing(&mut self) {
        self.camera.reset_follow_knobs();
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

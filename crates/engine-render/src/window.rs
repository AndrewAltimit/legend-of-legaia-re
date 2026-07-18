//! Shared windowed-app helpers used by asset-viewer, engine-shell, and any
//! other binary that opens a wgpu surface via winit.
//!
//! # Usage
//!
//! ```ignore
//! struct MyApp {
//!     win: EngineWindow,
//!     // … per-app fields …
//! }
//!
//! impl ApplicationHandler for MyApp {
//!     fn resumed(&mut self, evl: &ActiveEventLoop) {
//!         if !self.win.open(evl, "My Title") { return; }
//!         self.upload_assets();
//!         self.win.request_redraw();
//!     }
//!
//!     fn window_event(&mut self, evl: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
//!         match event {
//!             WindowEvent::CloseRequested => evl.exit(),
//!             WindowEvent::Resized(s) => self.win.handle_resize(s.width, s.height),
//!             WindowEvent::RedrawRequested => {
//!                 let dt = self.win.advance_tick();
//!                 for _ in 0..target_frames(dt, 8) { self.tick(); }
//!                 // … render …
//!                 self.win.request_redraw();
//!             }
//!             _ => {}
//!         }
//!     }
//! }
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::{Mat4, Vec3};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes};

use crate::Renderer;

/// Common windowed-app state: window handle, renderer, and timing.
///
/// Replaces the four identical `Option<Arc<Window>>`, `Option<Renderer>`,
/// `started_at: Instant`, `last_tick: Instant` fields that appear verbatim in
/// every windowed subcommand.
pub struct EngineWindow {
    pub window: Option<Arc<Window>>,
    pub renderer: Option<Renderer>,
    started_at: Instant,
    last_tick: Instant,
    /// Fixed-timestep accumulator (seconds). Driven by [`Self::drain_ticks`].
    accumulator: f64,
}

impl Default for EngineWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl EngineWindow {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            window: None,
            renderer: None,
            started_at: now,
            last_tick: now,
            accumulator: 0.0,
        }
    }

    /// Call in `ApplicationHandler::resumed`. Creates a 960×720 window and
    /// renderer. Returns `true` on success; on failure logs the error, calls
    /// `evl.exit()`, and returns `false`.
    ///
    /// Returns `false` immediately (no-op) if the window is already open.
    pub fn open(&mut self, evl: &ActiveEventLoop, title: &str) -> bool {
        if self.window.is_some() {
            return false;
        }
        let attrs = WindowAttributes::default()
            .with_title(title)
            .with_inner_size(winit::dpi::LogicalSize::new(960.0_f64, 720.0_f64));
        let window = match evl.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                log::error!("create_window: {e:#}");
                evl.exit();
                return false;
            }
        };
        let size = window.inner_size();
        let renderer = match Renderer::new(window.clone(), size.width, size.height) {
            Ok(r) => r,
            Err(e) => {
                log::error!("Renderer::new: {e:#}");
                evl.exit();
                return false;
            }
        };
        self.started_at = Instant::now();
        self.last_tick = self.started_at;
        self.window = Some(window);
        self.renderer = Some(renderer);
        true
    }

    /// Call in `WindowEvent::Resized`.
    pub fn handle_resize(&mut self, width: u32, height: u32) {
        if let Some(r) = self.renderer.as_mut() {
            r.resize(width, height);
        }
    }

    pub fn renderer(&self) -> Option<&Renderer> {
        self.renderer.as_ref()
    }

    pub fn renderer_mut(&mut self) -> Option<&mut Renderer> {
        self.renderer.as_mut()
    }

    /// Surface dimensions `(width, height)`, or `(0, 0)` before open.
    pub fn surface_size(&self) -> (u32, u32) {
        self.renderer.as_ref().map_or((0, 0), |r| r.surface_size())
    }

    /// Seconds elapsed since the window was opened.
    pub fn elapsed_secs(&self) -> f32 {
        self.started_at.elapsed().as_secs_f32()
    }

    /// Advance the last-tick timestamp and return how much time has passed
    /// since the previous call (capped at `cap_ms` milliseconds).
    pub fn advance_tick(&mut self, cap_ms: u64) -> Duration {
        let now = Instant::now();
        let dt = now
            .duration_since(self.last_tick)
            .min(Duration::from_millis(cap_ms));
        self.last_tick = now;
        dt
    }

    /// Fixed-timestep helper: add `dt` to the internal accumulator, drain it
    /// in 1/60-s chunks, and return how many game ticks to run (capped at
    /// `max_ticks`). Keeps the game tick rate at 60 Hz independent of VSync.
    pub fn drain_ticks(&mut self, dt: Duration, max_ticks: u32) -> u32 {
        const TICK_DT: f64 = 1.0 / 60.0;
        self.accumulator += dt.as_secs_f64();
        let mut count = 0u32;
        while self.accumulator >= TICK_DT && count < max_ticks {
            self.accumulator -= TICK_DT;
            count += 1;
        }
        count
    }

    /// How many game ticks correspond to `dt` at 60 Hz, capped at `max_ticks`.
    /// Simpler alternative to `drain_ticks` when fixed-timestep isn't needed.
    pub fn frames_for(dt: Duration, max_ticks: u32) -> u32 {
        ((dt.as_secs_f32() * 60.0).round() as u32).min(max_ticks)
    }

    pub fn request_redraw(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

/// The far clip plane every engine camera uses.
///
/// **Nothing in a Legaia scene is ever distance-culled.** A field map is
/// `256 x 256` tiles of 128 units (~16 k units across, ~23 k diagonal); the
/// overworld walk camera projects that through a 6x world scale, so eye-space
/// depth runs to ~140 k. One flat constant an order of magnitude past that
/// keeps *every* mesh - floor slabs, staircases, wall sections, the far side
/// of a continent - inside the frustum from any vantage, at any camera-distance
/// preset. The engine is not on a PSX depth budget, so the far plane is not a
/// draw-distance knob.
///
/// Raising the far plane costs no depth precision: for `near << far` the
/// projected depth is `1 - near/z + O(near/far)`, i.e. governed by the *near*
/// plane alone.
pub const SCENE_FAR: f32 = 1_000_000.0;

/// Near plane for the orbit-family cameras, from the framing distance.
///
/// The near plane is the *only* clip that can make close geometry vanish, so it
/// is kept at a few units instead of the old `distance * 0.05` (which put the
/// near plane 100-1600 units in front of the eye on a town / world-map framing
/// and clipped whole wall and floor bodies out of the view as the camera moved
/// over them). The tiny-model asset-viewer previews still need a sub-unit near
/// plane, so it scales with the framing distance but clamps into
/// `[0.05, 8.0]` - big scenes get the constant 8-unit plane, a unit-radius TMD
/// preview keeps its 0.05.
pub fn scene_clip_planes(distance: f32) -> (f32, f32) {
    ((distance * 0.005).clamp(0.05, 8.0), SCENE_FAR)
}

/// Orbit-camera MVP used by all scene viewers.
///
/// Frames the given AABB, orbits it at `orbit_speed` radians/second, and
/// uses the standard PSX convention (60° FOV, Y-up camera, Y-down geometry).
/// Because callers draw their geometry Y-flipped (`scale(1,-1,1)`), this frames
/// the *flipped* centre (`-(lo.y+hi.y)/2`) with the eye above it - otherwise an
/// off-centre scene renders from underneath ("ground on the ceiling").
///
/// # Parameters
/// - `aabb_lo`, `aabb_hi` - world-space bounding box of the scene.
/// - `orbit_speed` - angular velocity in radians/second (typical: 0.15–0.25).
/// - `eye_height` - fraction of `distance` to push the eye above centre
///   (positive = up in camera space; typical: 0.4–0.45).
/// - `elapsed_secs` - time since the window was opened; drives the angle.
/// - `aspect` - viewport width / height.
pub fn orbit_camera_mvp(
    aabb_lo: [f32; 3],
    aabb_hi: [f32; 3],
    orbit_speed: f32,
    eye_height: f32,
    elapsed_secs: f32,
    aspect: f32,
) -> Mat4 {
    let lo = Vec3::from(aabb_lo);
    let hi = Vec3::from(aabb_hi);
    // Meshes are drawn Y-flipped (`scale(1,-1,1)`) to convert PSX Y-down to the
    // renderer's Y-up, so the drawn geometry's Y-range is `[-hi.y, -lo.y]`.
    // Frame *that* flipped centre. Framing the raw, un-flipped AABB (as before)
    // put the look target on the opposite Y side from the geometry whenever the
    // model isn't centred on Y=0 - large off-centre field/battle scenes then
    // rendered from underneath ("ground on the ceiling"). Centred single models
    // (the asset-viewer's TMD previews) have `lo.y ≈ -hi.y`, so this is a no-op
    // for them. Same correction the world-map camera already carries.
    let center = Vec3::new(
        (lo.x + hi.x) * 0.5,
        -(lo.y + hi.y) * 0.5,
        (lo.z + hi.z) * 0.5,
    );
    let radius = ((hi - lo).length() * 0.5).max(1.0);
    let distance = radius / 30f32.to_radians().tan() * 1.6;
    let angle = elapsed_secs * orbit_speed;
    // +Y is up after the flip, so the eye sits *above* the centre and looks down
    // at the model (a natural 3/4 vantage), matching `world_map_camera_mvp`.
    let eye = center
        + Vec3::new(
            distance * angle.cos(),
            distance * eye_height,
            distance * angle.sin(),
        );
    let view = Mat4::look_at_rh(eye, center, Vec3::Y);
    let (near, far) = scene_clip_planes(distance);
    let proj = Mat4::perspective_rh(60f32.to_radians(), aspect.max(0.01), near, far);
    proj * view
}

/// Camera MVP for the world-map mode, driven by the live
/// [`WorldMapController`](../../engine_core/world_map/struct.WorldMapController.html)
/// state instead of wall-clock time.
///
/// The world map is viewed from an elevated, slightly-angled vantage that the
/// player rotates (`azimuth`), raises/lowers (`zoom`), and pans (`pan_x` /
/// `pan_z`) - mirroring the top-view debug camera's controls. The kingdom pack
/// is drawn **Y-flipped** (`scale(1,-1,1)`) to convert the PSX Y-down geometry
/// to the renderer's Y-up, so its drawn Y-range is `[-hi.y, -lo.y]`; this camera
/// frames that flipped centre and sits at *positive* Y, looking down on the
/// terrain. (Framing the raw, un-flipped AABB - as an earlier version did -
/// put the eye on the opposite Y side and rendered the map from underneath.)
///
/// # Parameters
/// - `aabb_lo`, `aabb_hi` - world-space bounding box of the loaded map meshes.
/// - `azimuth` - PSX angle units (`4096` = full turn) from `controller.azimuth`.
/// - `zoom` - height/zoom delta from `controller.zoom`; positive pulls the
///   camera in (lower + closer), negative pushes it out.
/// - `pan_x`, `pan_z` - top-view scroll from `controller.camera_x/_z`, added to
///   the framing centre in world units.
/// - `aspect` - viewport width / height.
pub fn world_map_camera_mvp(
    aabb_lo: [f32; 3],
    aabb_hi: [f32; 3],
    azimuth: i32,
    zoom: i32,
    pan_x: i32,
    pan_z: i32,
    aspect: f32,
) -> Mat4 {
    let lo = Vec3::from(aabb_lo);
    let hi = Vec3::from(aabb_hi);
    // The kingdom pack is drawn Y-flipped, so its drawn Y-range is
    // `[-hi.y, -lo.y]`; frame that flipped centre, offset by the top-view pan.
    let center = Vec3::new(
        (lo.x + hi.x) * 0.5 + pan_x as f32,
        -(lo.y + hi.y) * 0.5,
        (lo.z + hi.z) * 0.5 + pan_z as f32,
    );
    let radius = ((hi - lo).length() * 0.5).max(1.0);
    let base_distance = radius / 30f32.to_radians().tan() * 1.6;
    // Zoom nudges the framing distance; clamp so the player can't invert or
    // fly infinitely far out. `zoom` accrues in steps of 4, so 512 ~= a wide
    // usable band.
    let zoom_mult = (1.0 - (zoom as f32) / 512.0).clamp(0.25, 3.0);
    let distance = base_distance * zoom_mult;
    // PSX angle units: 4096 == 2*pi.
    let angle = (azimuth as f32) / 4096.0 * std::f32::consts::TAU;
    // Elevated bird's-eye vantage above the (Y-up) flipped terrain: +Y is up,
    // so the eye sits at positive Y and looks down on the map.
    let eye = center
        + Vec3::new(
            distance * angle.cos(),
            distance * 0.7,
            distance * angle.sin(),
        );
    let view = Mat4::look_at_rh(eye, center, Vec3::Y);
    let (near, far) = scene_clip_planes(distance);
    let proj = Mat4::perspective_rh(60f32.to_radians(), aspect.max(0.01), near, far);
    proj * view
}

/// Camera MVP for the **overworld walk view** (game mode `0x03` on a kingdom
/// continent, `map01`/`map02`/`map03`).
///
/// Same player-follow / azimuth / zoom / pan controls as
/// [`world_map_camera_mvp`], but framed the way retail frames the overworld:
/// a **wider, more steeply-overhead** vantage. The top-view debug camera
/// ([`world_map_camera_mvp`]) sits at a `0.7·distance` height (a shallow ~35deg
/// pitch good for surveying the whole continent); walking around a kingdom
/// instead wants a higher, closer-overhead radius around the player so the
/// nearby terrain reads at a useful scale. This keeps the two cameras separate
/// so tuning one doesn't disturb the other.
///
/// Geometry matches [`world_map_camera_mvp`]: the continent is drawn Y-flipped
/// (`scale(1,-1,1)`), so the eye sits at *positive* Y looking down on the
/// flipped terrain centre (offset by the player-follow `pan_x` / `pan_z`).
///
/// # Parameters
/// Identical to [`world_map_camera_mvp`].
pub fn walk_view_camera_mvp(
    aabb_lo: [f32; 3],
    aabb_hi: [f32; 3],
    azimuth: i32,
    zoom: i32,
    pan_x: i32,
    pan_z: i32,
    aspect: f32,
) -> Mat4 {
    let lo = Vec3::from(aabb_lo);
    let hi = Vec3::from(aabb_hi);
    let center = Vec3::new(
        (lo.x + hi.x) * 0.5 + pan_x as f32,
        -(lo.y + hi.y) * 0.5,
        (lo.z + hi.z) * 0.5 + pan_z as f32,
    );
    let radius = ((hi - lo).length() * 0.5).max(1.0);
    let base_distance = radius / 30f32.to_radians().tan() * 1.6;
    let zoom_mult = (1.0 - (zoom as f32) / 512.0).clamp(0.25, 3.0);
    let distance = base_distance * zoom_mult;
    let angle = (azimuth as f32) / 4096.0 * std::f32::consts::TAU;
    // Steeper overhead than the top-view survey camera: `1.4·distance` of
    // height gives a ~54deg downward pitch (vs the top-view's ~35deg), the
    // higher-angle framing retail uses while walking the continent.
    const WALK_EYE_HEIGHT: f32 = 1.4;
    let eye = center
        + Vec3::new(
            distance * angle.cos(),
            distance * WALK_EYE_HEIGHT,
            distance * angle.sin(),
        );
    let view = Mat4::look_at_rh(eye, center, Vec3::Y);
    let (near, far) = scene_clip_planes(distance);
    let proj = Mat4::perspective_rh(60f32.to_radians(), aspect.max(0.01), near, far);
    proj * view
}

/// Legacy orbit-based cutscene framing, kept as a reference / regression target.
///
/// SUPERSEDED in production by the exact retail PSX GTE path: the shell's
/// `compute_scene_camera` cutscene branch now builds the shot with
/// `psx_camera_mvp` (`screen = H*(R*(v - focus) + tr_eye)/Ze`, `FUN_800172c0`),
/// driving the eye-back depth from the op-`0x45` offset-slot-5 translation
/// (`0x800840B8`) instead of a scene-AABB radius. This orbit approximation
/// remains only because its heading/pitch/FOV unit tests document the framing
/// invariants; it is no longer wired into a live render path.
///
/// Camera MVP for an in-engine cutscene (the `opdeene` opening prologue),
/// framing the cutscene's look-at point from a heading-rotated vantage.
///
/// The parameters come from the cutscene timeline's executed op-`0x45` Camera
/// Configure ops, committed to the engine globals by `FUN_801DE084`:
///
/// - `look_at` - the camera focus/target. The retail camera stores the
///   *negated* focus X/Z in `_DAT_80089118` / `_DAT_80089120` (the GTE
///   translation = `-focus`, confirmed by the follow-cam `FUN_801DBE9C` which
///   sets them to `-(anchor+0x14)` / `-(anchor+0x18)`), so the engine negates
///   params 6 / 8 back to a world-space focus before calling this; Y (param 7)
///   is stored un-negated.
/// - `yaw_radians` - heading, from op-`0x45` param 1 (`_DAT_8007b792`, the
///   camera yaw; PSX `4096` = full turn, converted by the caller).
/// - `fov_radians` - vertical FOV derived from op-`0x45` param 9
///   (`_DAT_8007b6f4`), which retail writes to the GTE H projection register
///   via `setCopControlWord` - the focal length / zoom.
///
/// Unlike [`orbit_camera_mvp`] (which rotates with wall-clock time) this is a
/// static cinematic shot: the eye orbits the focus by `(yaw, pitch)` at a
/// distance derived from the scene AABB. Geometry uses the same PSX Y-down
/// convention as [`orbit_camera_mvp`] / [`world_map_camera_mvp`] (eye pushed to
/// *negative* Y to sit above, `+Y` look-at up vector).
///
/// `pitch_radians` is the decoded op-`0x45` camera pitch (slot 0 =
/// `_DAT_8007B790`, the GTE `RotMatrixX` angle; see `docs/subsystems/cutscene.md`):
/// positive tilts the eye **above** the focus looking down. Heading and pitch
/// are real params; the FOV is the op-`0x45` GTE `H` (slot 9). The eye
/// **distance** is *not* a pinned param (retail places the eye at the GTE
/// translation and projects through `H` rather than using an explicit eye
/// offset), so the orbit radius stays a scene-sized approximation.
///
/// REF: FUN_8001CF50, FUN_800461A4, FUN_8004629C, FUN_8004638C
///
/// `FUN_8001CF50` (SCUS) composes the retail view rotation by rotating about
/// each axis with the three camera-angle globals - `RotMatrixX(pitch)` at
/// `0x800461A4`, `RotMatrixY(yaw)` at `0x8004629C`, `RotMatrixZ(roll)` at
/// `0x8004638C` - each masking the 12-bit angle (`4096 = 360 deg`), indexing
/// the sin/cos LUT at `0x80070A2C`, and composing via GTE `mvmva`. Roll is
/// rarely non-zero in retail shots, so this MVP folds the pitch + yaw
/// composition into a spherical orbit + `glam::Mat4::look_at_rh` rather than
/// a literal `RotMatrixX` * `RotMatrixY` matrix product; the visible result
/// matches retail's framing for non-rolled shots.
pub fn cutscene_camera_mvp(
    look_at: [f32; 3],
    pitch_radians: f32,
    yaw_radians: f32,
    fov_radians: f32,
    aabb_lo: [f32; 3],
    aabb_hi: [f32; 3],
    aspect: f32,
) -> Mat4 {
    let center = Vec3::from(look_at);
    let lo = Vec3::from(aabb_lo);
    let hi = Vec3::from(aabb_hi);
    // Eye distance is the one un-pinned retail quantity (the GTE camera
    // carries no distance scalar - docs/subsystems/cutscene.md). Frame a
    // fixed world half-extent around the focus through the DECODED FOV:
    // retail's narrow GTE-H projections (H 792 -> ~17 degrees) then pull
    // the shot in to the close vignette framing the captured intro shots
    // show, and wider H values naturally back the eye off. The previous
    // scene-AABB radius model broke on multi-area cutscene scenes
    // (opdeene stages its vignette islands across the whole map, so the
    // AABB radius pushed the shot out to a satellite view). The AABB now
    // only caps the distance so a small scene can't be over-shot.
    const FRAME_HALF_EXTENT: f32 = 600.0;
    let radius = ((hi - lo).length() * 0.5).max(1.0);
    let fov = fov_radians.clamp(10f32.to_radians(), 120f32.to_radians());
    let distance = (FRAME_HALF_EXTENT / (fov * 0.5).tan())
        .min(radius / 30f32.to_radians().tan() * 1.2)
        .max(64.0);
    // Spherical orbit of the focus by the decoded heading + pitch. At yaw 0 the
    // eye sits in front (`+Z`); positive pitch raises it above the focus (`-Y`
    // under Y-down) so the shot looks down. Pitch is clamped shy of straight
    // down/up so the look-at up vector never degenerates.
    let pitch = pitch_radians.clamp(-80f32.to_radians(), 80f32.to_radians());
    let (ys, yc) = yaw_radians.sin_cos();
    let (ps, pc) = pitch.sin_cos();
    let eye = center + Vec3::new(distance * pc * ys, -distance * ps, distance * pc * yc);
    let view = Mat4::look_at_rh(eye, center, Vec3::Y);
    let (near, far) = scene_clip_planes(distance);
    let proj = Mat4::perspective_rh(fov, aspect.max(0.01), near, far);
    proj * view
}

/// Glides the cutscene camera between Camera Configure beats.
///
/// [`cutscene_camera_mvp`] frames whatever the timeline's *current* op-`0x45`
/// params decode to, so each new beat re-targets the shot instantly and the
/// camera snaps. Retail stages each Configure's params into a persistent
/// control block and moves the live camera globals toward them over the beat's
/// `apply_trigger` frames; this mirrors that per component.
///
/// Nine components (focus xyz, pitch, yaw, H, eye-trio xyz) each carry their
/// own in-flight glide: when a component's target changes, a glide is armed
/// from the CURRENT pose over the staging beat's `apply` frames (`apply == 0`
/// commits that component immediately - the snap cut). Components whose
/// targets did not change keep their in-flight glide untouched, so a
/// follow-up single-slot poke (opdeene re-stages H alone one frame after
/// arming its 480-frame tableau dolly) cannot cancel the dolly - the earlier
/// whole-tuple ease-rate model snapped the entire shot on exactly that poke,
/// which is what planted the eye inside the crater-rim geometry.
///
/// Motion arrives exactly, advanced in SIM ticks by the caller (`steps`).
/// The mover law is pinned from a per-frame RAM capture of the live camera
/// globals (`0x8007B790` angle trio / `0x800840B8` eye trio /
/// `0x80089118` focus trio) across the whole retail New-Game opening chain:
///
/// - **Duration**: `apply` IS the glide length in retail frames, 1:1
///   (measured arrivals 48/50, 85/90, 239/240, ~965/1000, ~900/900), and
///   the engine's sim tick counts retail frames 1:1 (`WaitFrames` targets
///   drain one per tick), so a glide spans exactly `apply` sim ticks.
///   A long `apply` is a *dolly velocity* spec, not a promise of arrival:
///   opurud stages an `apply 2300` eye glide whose next snap beat lands
///   ~1/4 of the way through - retail never reaches that staged target.
///   (The earlier `apply / 3.5` reading compressed those dollies ~6x, so
///   the engine ARRIVED at extreme staged eye targets retail only drifts
///   toward - parking the camera inside scene geometry.)
/// - **Shape**: the op-`0x45` opcode's high bits (engine: the decoded
///   `mode` nibble, `op0 >> 2`) select the beat's ease curve.
///   `mode 1` (the common `45 07 ..` form): the eye trio / focus / H move
///   at CONSTANT VELOCITY (`delta / apply` per frame - measured exactly
///   linear across opdeene's `apply 840` grove dolly and opurud's
///   `apply 50/240/2300` moves) while pitch / yaw decelerate along a
///   quadratic ease-out (initial rate `2·delta/apply`, measured).
///   `mode 2` (`45 0B ..`): every component eases out quadratically -
///   the map01 Rim Elm fly-in (`apply 900`) fits `1-(1-t)^2` to within
///   sampling noise over its whole 6330-unit descent.
///   `mode 4` (`45 13 ..`): every component eases in-out (slow-fast-slow;
///   opdeene's crater-rim tableau dolly) - modeled as smoothstep.
///
/// Angles glide along the shortest arc so a wrap across ±π doesn't spin the
/// long way round. [`Self::reset`] makes the next [`Self::glide`] snap
/// directly to the target - call it when a cutscene (re)starts so the
/// opening shot doesn't sweep in from a stale pose.
///
/// REF: FUN_801DB510 (cutscene overlay) - retail's per-frame camera mover
/// (constant-velocity eye-trio head + the typed `0x801F2798` param table).
/// REF: FUN_801DE084 - the op-`0x45` Configure apply handler (per-slot
/// staging into the persistent control block; `apply == 0` = immediate).
/// The *dialog* overlay's copy is this same function (the dialog and
/// cutscene_dialogue dumps are instruction-identical); only the menu overlay
/// hosts different code at this VA - see `docs/reference/functions.md`.
#[derive(Debug, Clone, Default)]
pub struct CutsceneCameraInterp {
    /// Current pose, packed `[look_at xyz, pitch, yaw, h, tr_eye xyz]`.
    cur: [f32; 9],
    /// Per-component glide start pose (the pose when the target last changed).
    start: [f32; 9],
    /// Per-component staged target.
    target: [f32; 9],
    /// Per-component glide length in sim frames (0 = committed immediately).
    total: [u32; 9],
    /// Per-component frames elapsed since the glide was armed.
    done: [u32; 9],
    /// Per-component ease curve, latched from the staging beat's mode.
    curve: [EaseCurve; 9],
    initialized: bool,
}

/// Per-component ease curve of an op-`0x45` glide (see
/// [`CutsceneCameraInterp`]'s mover-law provenance).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum EaseCurve {
    /// Constant velocity, exact arrival (`mode 1` eye/focus/H).
    #[default]
    Linear,
    /// Quadratic ease-out (`mode 1` angles; every `mode 2` component).
    QuadOut,
    /// Smoothstep ease-in-out (every `mode 4` component).
    InOut,
}

impl CutsceneCameraInterp {
    /// Packed indices of the two angle components (shortest-arc glide).
    const PITCH: usize = 3;
    const YAW: usize = 4;

    pub fn new() -> Self {
        Self::default()
    }

    /// Drop the held pose so the next [`Self::glide`] snaps to its target.
    pub fn reset(&mut self) {
        self.initialized = false;
    }

    /// Snap individual packed components (0..2 look_at, 3 pitch, 4 yaw, 5 H,
    /// 6..8 tr_eye) to `value` immediately - an `apply == 0` Configure beat.
    ///
    /// Needed for **same-tick beat pairs**: the field VM runs until yield, so
    /// a snap beat immediately followed by a glide beat (map01's fly-in: the
    /// aerial snap at `+0x109`, then the `apply 900` descent at `+0x11E` with
    /// no yield between) commits both in ONE world tick - the merged
    /// `camera_state` the caller reads only shows the glide beat's targets.
    /// Retail's mover snaps the live globals to the first beat and glides
    /// from there (the captured fly-in trajectory starts exactly at the
    /// aerial pose); replaying the drained beat events' snaps through this
    /// before arming the glide reproduces that. A snapped component's target
    /// equals its current value, so the follow-up [`Self::glide`] arms the
    /// glide FROM the snapped pose rather than seeing it as a re-stage.
    /// No-op before the first glide (which snaps the whole pose anyway).
    pub fn snap_components(&mut self, components: &[(usize, f32)]) {
        if !self.initialized {
            return;
        }
        for &(i, v) in components {
            if i >= 9 {
                continue;
            }
            self.cur[i] = v;
            self.start[i] = v;
            self.target[i] = v;
            self.total[i] = 0;
            self.done[i] = 0;
        }
    }

    /// Advance the camera `steps` sim frames toward the staged `target_*`
    /// pose and return the current `(look_at, pitch, yaw, h, tr_eye)`.
    ///
    /// `apply` is the staging beat's op-`0x45` `apply_trigger` (= the glide
    /// length in frames, 1:1) and `mode` its decoded mode nibble (`op0 >> 2`;
    /// the ease-curve selector - see the type doc). Any component whose
    /// target changed this call re-arms its glide from the current pose over
    /// `apply` frames (`0` = snap that component) with the beat's curve.
    /// Unchanged components keep their in-flight glide. The first call after
    /// a reset (or construction) snaps the whole pose to the target.
    #[allow(clippy::too_many_arguments)]
    pub fn glide(
        &mut self,
        target_look_at: [f32; 3],
        target_pitch: f32,
        target_yaw: f32,
        target_h: f32,
        target_tr_eye: [f32; 3],
        apply: u32,
        mode: u8,
        steps: u32,
    ) -> ([f32; 3], f32, f32, f32, [f32; 3]) {
        let packed = [
            target_look_at[0],
            target_look_at[1],
            target_look_at[2],
            target_pitch,
            target_yaw,
            target_h,
            target_tr_eye[0],
            target_tr_eye[1],
            target_tr_eye[2],
        ];
        if !self.initialized {
            self.cur = packed;
            self.start = packed;
            self.target = packed;
            self.total = [0; 9];
            self.done = [0; 9];
            self.curve = [EaseCurve::Linear; 9];
            self.initialized = true;
        } else {
            for (i, &target_i) in packed.iter().enumerate() {
                // Exact compare is intentional: targets decode from the same
                // op-0x45 integer params every frame, so a change is a real
                // re-stage, not float drift.
                if target_i != self.target[i] {
                    self.target[i] = target_i;
                    self.start[i] = self.cur[i];
                    // `apply` is the glide length in retail frames = sim
                    // ticks, 1:1 (capture-pinned; see the type doc).
                    self.total[i] = apply;
                    self.done[i] = 0;
                    let is_angle = i == Self::PITCH || i == Self::YAW;
                    self.curve[i] = match mode {
                        // 45 0B ..: every component eases out (map01 fly-in).
                        2 => EaseCurve::QuadOut,
                        // 45 13 ..: every component eases in-out.
                        4 => EaseCurve::InOut,
                        // 45 07 .. (and unseen modes): linear eye/focus/H,
                        // ease-out angles.
                        _ if is_angle => EaseCurve::QuadOut,
                        _ => EaseCurve::Linear,
                    };
                }
                self.done[i] = self.done[i].saturating_add(steps).min(self.total[i]);
                let s = if self.total[i] == 0 {
                    1.0
                } else {
                    self.done[i] as f32 / self.total[i] as f32
                };
                let s = match self.curve[i] {
                    EaseCurve::Linear => s,
                    EaseCurve::QuadOut => 1.0 - (1.0 - s) * (1.0 - s),
                    EaseCurve::InOut => s * s * (3.0 - 2.0 * s),
                };
                let delta = if i == Self::PITCH || i == Self::YAW {
                    wrap_pi(self.target[i] - self.start[i])
                } else {
                    self.target[i] - self.start[i]
                };
                self.cur[i] = self.start[i] + delta * s;
                if i == Self::PITCH || i == Self::YAW {
                    self.cur[i] = wrap_pi(self.cur[i]);
                }
            }
        }
        (
            [self.cur[0], self.cur[1], self.cur[2]],
            self.cur[3],
            self.cur[4],
            self.cur[5],
            [self.cur[6], self.cur[7], self.cur[8]],
        )
    }
}

/// Wrap an angle (radians) into `(-π, π]`.
fn wrap_pi(a: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let mut a = a % TAU;
    if a > PI {
        a -= TAU;
    } else if a <= -PI {
        a += TAU;
    }
    a
}

#[cfg(test)]
mod camera_tests {
    use super::*;

    const LO: [f32; 3] = [-100.0, -100.0, -100.0];
    const HI: [f32; 3] = [100.0, 100.0, 100.0];

    fn finite(m: &Mat4) -> bool {
        m.to_cols_array().iter().all(|v| v.is_finite())
    }

    /// A field map is 256x256 tiles of 128 units.
    const FIELD_MAP_SPAN: f32 = 256.0 * 128.0;

    /// Depth of `p` under `mvp`, or `None` when the point is behind the eye
    /// (`w <= 0`, which the rasteriser clips regardless of the far plane).
    fn depth_of(mvp: &Mat4, p: Vec3) -> Option<f32> {
        let c = *mvp * p.extend(1.0);
        (c.w > 0.0).then(|| c.z / c.w)
    }

    /// GUARD (no distance cull): every corner of a full-size field map stays
    /// inside the depth range of the orbit-family cameras, even though those
    /// cameras frame only a small player-sized box. The old
    /// `far = distance * 4 + 1000` was derived from the *framing* box, not the
    /// scene, so the far half of a town (floor slabs, stairs, the far wall)
    /// fell behind the far plane and blinked in and out as the framing box
    /// moved with the player.
    #[test]
    fn far_plane_never_clips_a_full_field_map() {
        // The follow/debug orbit frames a 700-unit half-box around the player
        // (`camera_mvp`'s FIELD_VIEW_HALF), while the map itself spans 32 k.
        let (lo, hi) = ([-700.0, -700.0, -700.0], [700.0, 700.0, 700.0]);
        let cams = [
            orbit_camera_mvp(lo, hi, 0.25, 0.85, 3.0, 4.0 / 3.0),
            world_map_camera_mvp(lo, hi, 0, 0, 0, 0, 4.0 / 3.0),
            walk_view_camera_mvp(lo, hi, 0, 0, 0, 0, 4.0 / 3.0),
        ];
        let s = FIELD_MAP_SPAN;
        for (i, mvp) in cams.iter().enumerate() {
            for &x in &[-s, 0.0, s] {
                for &y in &[-2000.0f32, 0.0, 2000.0] {
                    for &z in &[-s, 0.0, s] {
                        let Some(d) = depth_of(mvp, Vec3::new(x, y, z)) else {
                            continue; // behind the eye: not a distance cull
                        };
                        assert!(
                            d <= 1.0,
                            "camera {i}: ({x}, {y}, {z}) is far-clipped (depth {d})"
                        );
                    }
                }
            }
        }
    }

    /// GUARD (no near cull): the near plane stays within a few units of the
    /// eye at every framing distance the engine uses, so a wall / floor body
    /// the camera passes close to is never clipped away. The old
    /// `near = distance * 0.05` put the plane 100-1600 units out on town and
    /// world-map framings - whole bodies vanished when the camera swept over
    /// them.
    #[test]
    fn near_plane_stays_within_a_few_units_of_the_lens() {
        for distance in [2.8f32, 50.0, 138.0, 2400.0, 32_000.0, 250_000.0] {
            let (near, far) = scene_clip_planes(distance);
            assert!(
                (0.05..=8.0).contains(&near),
                "distance {distance}: near {near} outside [0.05, 8]"
            );
            assert!(near < distance * 0.5, "distance {distance}: near {near}");
            assert_eq!(far, SCENE_FAR);
        }
    }

    /// GUARD: the far plane clears the overworld walk camera's 6x world scale.
    /// `psx_camera_mvp` (engine-shell) projects the continent through a 6x
    /// scale with an eye-back depth of ~11 k, so the eye-space depth of the
    /// far side of a kingdom is `6 * span + tr.z`. The old 60 000 cut that to
    /// ~8.5 k *world* units of draw distance - visible terrain pop-in.
    #[test]
    fn far_plane_clears_the_overworld_six_x_world_scale() {
        const WORLD_SCALE: f32 = 6.0;
        const EYE_BACK: f32 = 11_041.0;
        let deepest = WORLD_SCALE * FIELD_MAP_SPAN * std::f32::consts::SQRT_2 + EYE_BACK;
        assert!(
            SCENE_FAR > deepest * 2.0,
            "SCENE_FAR {SCENE_FAR} must clear the 6x-scaled continent depth {deepest}"
        );
    }

    #[test]
    fn world_map_camera_mvp_is_finite() {
        let m = world_map_camera_mvp(LO, HI, 0, 0, 0, 0, 16.0 / 9.0);
        assert!(finite(&m));
    }

    #[test]
    fn world_map_camera_azimuth_rotates_view() {
        let a = world_map_camera_mvp(LO, HI, 0, 0, 0, 0, 1.5);
        let b = world_map_camera_mvp(LO, HI, 1024, 0, 0, 0, 1.5);
        // A quarter turn must change the projection.
        assert_ne!(a.to_cols_array(), b.to_cols_array());
        assert!(finite(&b));
    }

    #[test]
    fn world_map_camera_zoom_changes_distance() {
        let near = world_map_camera_mvp(LO, HI, 0, 256, 0, 0, 1.5);
        let far = world_map_camera_mvp(LO, HI, 0, -256, 0, 0, 1.5);
        assert_ne!(near.to_cols_array(), far.to_cols_array());
    }

    #[test]
    fn world_map_camera_pan_shifts_view() {
        let a = world_map_camera_mvp(LO, HI, 0, 0, 0, 0, 1.5);
        let b = world_map_camera_mvp(LO, HI, 0, 0, 64, -64, 1.5);
        assert_ne!(a.to_cols_array(), b.to_cols_array());
    }

    #[test]
    fn world_map_camera_extreme_zoom_clamps_finite() {
        // Zoom far past the clamp band in both directions stays finite.
        let zoomed_in = world_map_camera_mvp(LO, HI, 0, 100_000, 0, 0, 1.5);
        let zoomed_out = world_map_camera_mvp(LO, HI, 0, -100_000, 0, 0, 1.5);
        assert!(finite(&zoomed_in));
        assert!(finite(&zoomed_out));
    }

    #[test]
    fn walk_view_camera_mvp_is_finite_and_responds_to_controls() {
        let base = walk_view_camera_mvp(LO, HI, 0, 0, 0, 0, 16.0 / 9.0);
        assert!(finite(&base));
        // azimuth / zoom / pan each change the projection, and extremes clamp.
        assert_ne!(
            base.to_cols_array(),
            walk_view_camera_mvp(LO, HI, 1024, 0, 0, 0, 16.0 / 9.0).to_cols_array()
        );
        assert_ne!(
            base.to_cols_array(),
            walk_view_camera_mvp(LO, HI, 0, 256, 0, 0, 16.0 / 9.0).to_cols_array()
        );
        assert_ne!(
            base.to_cols_array(),
            walk_view_camera_mvp(LO, HI, 0, 0, 64, -64, 16.0 / 9.0).to_cols_array()
        );
        assert!(finite(&walk_view_camera_mvp(LO, HI, 0, 100_000, 0, 0, 1.5)));
    }

    #[test]
    fn walk_view_camera_is_more_overhead_than_top_view() {
        // The walk camera sits steeper (higher eye for the same framing) than
        // the top-view survey camera, so the two MVPs differ.
        let walk = walk_view_camera_mvp(LO, HI, 0, 0, 0, 0, 1.5);
        let top = world_map_camera_mvp(LO, HI, 0, 0, 0, 0, 1.5);
        assert_ne!(walk.to_cols_array(), top.to_cols_array());
    }

    #[test]
    fn cutscene_camera_mvp_is_finite() {
        let m = cutscene_camera_mvp(
            [0.0, 0.0, 0.0],
            0.0,
            0.0,
            60f32.to_radians(),
            LO,
            HI,
            16.0 / 9.0,
        );
        assert!(finite(&m));
    }

    #[test]
    fn cutscene_camera_tracks_its_look_at() {
        // Re-targeting the camera (the pinned op-0x45 focus params changing)
        // must change the projection - the shot follows the cutscene target.
        let fov = 60f32.to_radians();
        let a = cutscene_camera_mvp([0.0, 0.0, 0.0], 0.0, 0.0, fov, LO, HI, 1.5);
        let b = cutscene_camera_mvp([500.0, 0.0, -300.0], 0.0, 0.0, fov, LO, HI, 1.5);
        assert_ne!(a.to_cols_array(), b.to_cols_array());
        assert!(finite(&b));
    }

    #[test]
    fn cutscene_camera_pitch_yaw_and_fov_change_the_view() {
        let fov = 60f32.to_radians();
        let base = cutscene_camera_mvp([0.0, 0.0, 0.0], 0.0, 0.0, fov, LO, HI, 1.5);
        // A quarter-turn heading orbits the eye -> different projection.
        let yawed = cutscene_camera_mvp(
            [0.0, 0.0, 0.0],
            0.0,
            std::f32::consts::FRAC_PI_2,
            fov,
            LO,
            HI,
            1.5,
        );
        assert_ne!(base.to_cols_array(), yawed.to_cols_array());
        // The decoded op-0x45 pitch (slot 0) tilts the eye -> different view.
        let pitched =
            cutscene_camera_mvp([0.0, 0.0, 0.0], 30f32.to_radians(), 0.0, fov, LO, HI, 1.5);
        assert_ne!(base.to_cols_array(), pitched.to_cols_array());
        assert!(finite(&pitched));
        // Pitch is clamped shy of straight-down so the up vector never
        // degenerates into a non-finite look-at.
        let steep =
            cutscene_camera_mvp([0.0, 0.0, 0.0], 200f32.to_radians(), 0.0, fov, LO, HI, 1.5);
        assert!(finite(&steep));
        // A narrower FOV (more zoom, from a larger GTE H) also changes it, and
        // an out-of-range FOV is clamped to a finite matrix.
        let zoomed =
            cutscene_camera_mvp([0.0, 0.0, 0.0], 0.0, 0.0, 20f32.to_radians(), LO, HI, 1.5);
        assert_ne!(base.to_cols_array(), zoomed.to_cols_array());
        let clamped = cutscene_camera_mvp([0.0, 0.0, 0.0], 0.0, 0.0, 0.0, LO, HI, 1.5);
        assert!(finite(&clamped));
    }

    const TR0: [f32; 3] = [0.0, 200.0, 2800.0];

    #[test]
    fn cutscene_interp_first_call_snaps_to_target() {
        let mut it = CutsceneCameraInterp::new();
        let (la, pitch, yaw, h, tr) = it.glide(
            [100.0, 0.0, -50.0],
            0.3,
            1.0,
            768.0,
            [10.0, 220.0, 2900.0],
            480,
            1,
            1,
        );
        assert_eq!(la, [100.0, 0.0, -50.0]);
        assert_eq!(pitch, 0.3);
        assert_eq!(yaw, 1.0);
        assert_eq!(h, 768.0);
        assert_eq!(tr, [10.0, 220.0, 2900.0]);
    }

    #[test]
    fn cutscene_interp_mode1_moves_linearly_and_arrives_on_schedule() {
        // The capture-pinned default (`45 07 ..`, mode 1) mover: eye trio /
        // focus / H travel at CONSTANT velocity over exactly `apply` ticks
        // (opdeene's apply-840 grove dolly and opurud's apply-50/240 moves
        // are measured linear with exact arrival).
        let mut it = CutsceneCameraInterp::new();
        // Snap to beat A.
        it.glide([0.0, 0.0, 0.0], 0.0, 0.0, 512.0, TR0, 0, 1, 1);
        // Beat B re-targets with apply 100: 25 ticks in = exactly 1/4 of the
        // positional travel (linear), while the yaw (an angle) is ahead of
        // linear on its quadratic ease-out.
        let (la, _, yaw, h, tr) = it.glide(
            [100.0, 0.0, 0.0],
            0.0,
            1.0,
            1024.0,
            [0.0, 200.0, 3200.0],
            100,
            1,
            25,
        );
        assert!(
            (la[0] - 25.0).abs() < 1e-3,
            "linear quarter travel: {}",
            la[0]
        );
        assert!((h - 640.0).abs() < 1e-3, "h linear mid-glide: {h}");
        assert!((tr[2] - 2900.0).abs() < 1e-3, "tr_eye linear: {}", tr[2]);
        let quad = 1.0 - (1.0 - 0.25) * (1.0 - 0.25);
        assert!(
            (yaw - quad).abs() < 1e-3,
            "angle eases out: {yaw} vs {quad}"
        );
        // The remaining 75 ticks arrive EXACTLY (no asymptote), and further
        // steps hold there.
        let (la, _, _, h, tr) = it.glide(
            [100.0, 0.0, 0.0],
            0.0,
            1.0,
            1024.0,
            [0.0, 200.0, 3200.0],
            100,
            1,
            75,
        );
        assert_eq!(la[0], 100.0);
        assert_eq!(h, 1024.0);
        assert_eq!(tr[2], 3200.0);
        let (la, _, _, _, _) = it.glide(
            [100.0, 0.0, 0.0],
            0.0,
            1.0,
            1024.0,
            [0.0, 200.0, 3200.0],
            100,
            1,
            10,
        );
        assert_eq!(la[0], 100.0, "arrived glide holds");
    }

    #[test]
    fn cutscene_interp_mode2_eases_out_all_components() {
        // `45 0B ..` (mode 2): every component quad-eases out - the map01
        // Rim Elm fly-in (`apply 900`) fits `1-(1-t)^2` across its whole
        // descent in the retail capture.
        let mut it = CutsceneCameraInterp::new();
        it.glide([0.0, 0.0, 0.0], 0.0, 0.0, 512.0, TR0, 0, 1, 1);
        let (la, _, _, _, tr) = it.glide(
            [100.0, 0.0, 0.0],
            0.0,
            0.0,
            512.0,
            [0.0, 200.0, 3200.0],
            100,
            2,
            25,
        );
        let quad = 100.0 * (1.0 - (1.0 - 0.25) * (1.0 - 0.25));
        assert!(
            (la[0] - quad).abs() < 1e-2,
            "focus eases out: {} vs {quad}",
            la[0]
        );
        let trq = 2800.0 + 400.0 * (1.0 - (1.0 - 0.25) * (1.0 - 0.25));
        assert!(
            (tr[2] - trq).abs() < 1e-2,
            "tr eases out: {} vs {trq}",
            tr[2]
        );
    }

    #[test]
    fn cutscene_interp_mode4_eases_in_out() {
        // `45 13 ..` (mode 4): slow-fast-slow (opdeene's crater-rim tableau
        // dolly starts near-still in the retail capture).
        let mut it = CutsceneCameraInterp::new();
        it.glide([0.0, 0.0, 0.0], 0.0, 0.0, 512.0, TR0, 0, 1, 1);
        let (la, _, _, _, _) = it.glide([100.0, 0.0, 0.0], 0.0, 0.0, 512.0, TR0, 100, 4, 10);
        let smooth = 100.0 * (0.1f32 * 0.1 * (3.0 - 0.2));
        assert!(
            (la[0] - smooth).abs() < 1e-2,
            "slow start: {} vs {smooth}",
            la[0]
        );
        assert!(la[0] < 5.0, "ease-in start is near-still: {}", la[0]);
    }

    #[test]
    fn cutscene_interp_long_apply_is_a_drift_the_next_snap_interrupts() {
        // opurud stages an apply-2300 eye dolly whose next snap beat fires
        // ~600 ticks in: retail never reaches the staged target. The mover
        // must still be ~600/2300 of the way (constant velocity), and the
        // following snap re-stage takes over immediately.
        let mut it = CutsceneCameraInterp::new();
        it.glide([0.0, 0.0, 0.0], 0.0, 0.0, 512.0, [420.0, 0.0, 0.0], 0, 1, 1);
        let (_, _, _, _, tr) = it.glide(
            [0.0, 0.0, 0.0],
            0.0,
            0.0,
            512.0,
            [-7420.0, 0.0, 0.0],
            2300,
            1,
            600,
        );
        let expect = 420.0 - 7840.0 * (600.0 / 2300.0);
        assert!(
            (tr[0] - expect).abs() < 1.0,
            "mid-drift at constant velocity: {} vs {expect}",
            tr[0]
        );
        let (_, _, _, _, tr) = it.glide(
            [0.0, 0.0, 0.0],
            0.0,
            0.0,
            512.0,
            [-1204.0, 0.0, 0.0],
            0,
            1,
            1,
        );
        assert_eq!(tr[0], -1204.0, "next snap beat interrupts the drift");
    }

    #[test]
    fn cutscene_interp_apply_zero_snaps_the_beat() {
        let mut it = CutsceneCameraInterp::new();
        it.glide([0.0, 0.0, 0.0], 0.0, 0.0, 512.0, TR0, 0, 1, 1);
        let (la, _, _, _, _) = it.glide([100.0, 0.0, 0.0], 0.0, 0.0, 512.0, TR0, 0, 1, 1);
        assert_eq!(la[0], 100.0, "apply 0 commits immediately (snap cut)");
    }

    #[test]
    fn cutscene_interp_single_slot_poke_keeps_inflight_glide() {
        // The opdeene tableau shape: an apply-480 dolly is armed, then ONE
        // frame later a Configure re-stages only H with apply 0. The dolly
        // components must keep gliding (per-component arming); only H snaps.
        let mut it = CutsceneCameraInterp::new();
        it.glide([0.0, 0.0, 0.0], 0.0, 0.0, 776.0, TR0, 0, 1, 1);
        // Arm the dolly: focus X -> 480 over 480 ticks.
        it.glide([480.0, 0.0, 0.0], 0.0, 0.0, 776.0, TR0, 480, 1, 1);
        // One frame later: H-only re-stage with apply 0.
        let (la, _, _, h, _) = it.glide([480.0, 0.0, 0.0], 0.0, 0.0, 792.0, TR0, 0, 1, 1);
        assert_eq!(h, 792.0, "H snapped by its apply-0 poke");
        assert!(
            la[0] > 0.0 && la[0] < 100.0,
            "dolly still in flight (2 of 480 ticks), not snapped: {}",
            la[0]
        );
    }

    #[test]
    fn cutscene_interp_yaw_takes_shortest_arc() {
        use std::f32::consts::PI;
        let mut it = CutsceneCameraInterp::new();
        // Start just below +π, target just above -π (i.e. ~6° apart across the
        // wrap). The glide must move the short way (toward +π / over the
        // seam), never unwind ~352° the long way.
        it.glide([0.0, 0.0, 0.0], 0.0, PI - 0.05, 512.0, TR0, 0, 1, 1);
        let (_, _, yaw, _, _) = it.glide([0.0, 0.0, 0.0], 0.0, -PI + 0.05, 512.0, TR0, 7, 1, 1);
        // Partway across a ~0.1 rad arc lands near ±π, not near 0.
        assert!(
            yaw.abs() > PI - 0.1,
            "shortest arc stays near the seam: {yaw}"
        );
    }

    #[test]
    fn cutscene_interp_reset_resnaps() {
        let mut it = CutsceneCameraInterp::new();
        it.glide([0.0, 0.0, 0.0], 0.0, 0.0, 512.0, TR0, 0, 1, 1);
        it.reset();
        let (la, _, _, _, _) = it.glide([500.0, 0.0, 0.0], 0.0, 0.25, 512.0, TR0, 480, 1, 1);
        assert_eq!(la, [500.0, 0.0, 0.0], "reset snaps the next glide");
    }
}

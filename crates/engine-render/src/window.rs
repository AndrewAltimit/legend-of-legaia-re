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

/// Orbit-camera MVP used by all scene viewers.
///
/// Frames the given AABB, orbits it at `orbit_speed` radians/second, and
/// uses the standard PSX convention (60° FOV, Y-up camera, Y-down geometry).
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
    let center = (lo + hi) * 0.5;
    let radius = ((hi - lo).length() * 0.5).max(1.0);
    let distance = radius / 30f32.to_radians().tan() * 1.6;
    let angle = elapsed_secs * orbit_speed;
    let eye = center
        + Vec3::new(
            distance * angle.cos(),
            -distance * eye_height,
            distance * angle.sin(),
        );
    let view = Mat4::look_at_rh(eye, center, Vec3::Y);
    let near = (distance * 0.05).max(0.1);
    let far = distance * 4.0 + 1000.0;
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
    let near = (distance * 0.05).max(0.1);
    let far = distance * 4.0 + 1000.0;
    let proj = Mat4::perspective_rh(60f32.to_radians(), aspect.max(0.01), near, far);
    proj * view
}

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
/// static cinematic shot: the eye orbits the focus by `yaw_radians` at a
/// distance derived from the scene AABB. Geometry uses the same PSX Y-down
/// convention as [`orbit_camera_mvp`] / [`world_map_camera_mvp`] (eye pushed to
/// *negative* Y to sit above, `+Y` look-at up vector).
///
/// The eye **distance** itself is not a pinned op-`0x45` param (retail derives
/// it through the GTE projection rather than an explicit translate), so the
/// orbit radius is a scene-sized approximation; the FOV and heading are from
/// real params. See `docs/subsystems/cutscene.md`.
pub fn cutscene_camera_mvp(
    look_at: [f32; 3],
    yaw_radians: f32,
    fov_radians: f32,
    aabb_lo: [f32; 3],
    aabb_hi: [f32; 3],
    aspect: f32,
) -> Mat4 {
    let center = Vec3::from(look_at);
    let lo = Vec3::from(aabb_lo);
    let hi = Vec3::from(aabb_hi);
    let radius = ((hi - lo).length() * 0.5).max(1.0);
    // Slightly tighter than the orbit framing so the cinematic shot fills more
    // of the screen.
    let distance = radius / 30f32.to_radians().tan() * 1.2;
    // Orbit the focus by the cutscene heading. At yaw 0 the eye sits in front
    // (`+Z`) and above (`-Y` under Y-down), matching the default framing.
    let (sin, cos) = yaw_radians.sin_cos();
    let eye = center + Vec3::new(distance * sin, -distance * 0.45, distance * cos);
    let view = Mat4::look_at_rh(eye, center, Vec3::Y);
    let near = (distance * 0.05).max(0.1);
    let far = distance * 4.0 + 1000.0;
    let fov = fov_radians.clamp(10f32.to_radians(), 120f32.to_radians());
    let proj = Mat4::perspective_rh(fov, aspect.max(0.01), near, far);
    proj * view
}

/// Smooths the cutscene camera between Camera Configure beats.
///
/// [`cutscene_camera_mvp`] frames whatever the timeline's *current* op-`0x45`
/// params decode to, so each new beat re-targets the shot instantly and the
/// camera snaps. Retail's GTE camera eases toward the new focus/heading over a
/// handful of frames; this holds the last rendered `(look_at, yaw, fov)` and
/// eases it toward the live target each frame, so beats blend instead of cut.
///
/// Frame-rate-agnostic by design: the caller passes the per-frame easing factor
/// `t` (0..=1), so a faster redraw cadence simply converges sooner. Yaw eases
/// along the shortest arc so a wrap across ±π doesn't spin the long way round.
/// [`Self::reset`] makes the next [`Self::approach`] snap directly to the
/// target — call it when a cutscene (re)starts so the opening shot doesn't
/// sweep in from a stale pose.
#[derive(Debug, Clone, Default)]
pub struct CutsceneCameraInterp {
    look_at: [f32; 3],
    yaw: f32,
    fov: f32,
    initialized: bool,
}

impl CutsceneCameraInterp {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop the held pose so the next [`Self::approach`] snaps to its target.
    pub fn reset(&mut self) {
        self.initialized = false;
    }

    /// Ease the held camera toward `target` by `t` (clamped to `0..=1`) and
    /// return the smoothed `(look_at, yaw, fov)`. The first call after a reset
    /// (or construction) snaps directly to the target.
    pub fn approach(
        &mut self,
        target_look_at: [f32; 3],
        target_yaw: f32,
        target_fov: f32,
        t: f32,
    ) -> ([f32; 3], f32, f32) {
        if !self.initialized {
            self.look_at = target_look_at;
            self.yaw = target_yaw;
            self.fov = target_fov;
            self.initialized = true;
            return (self.look_at, self.yaw, self.fov);
        }
        let t = t.clamp(0.0, 1.0);
        for (cur, &tgt) in self.look_at.iter_mut().zip(target_look_at.iter()) {
            *cur += (tgt - *cur) * t;
        }
        // Shortest-arc yaw ease so a wrap across ±π takes the short way.
        let delta = wrap_pi(target_yaw - self.yaw);
        self.yaw = wrap_pi(self.yaw + delta * t);
        self.fov += (target_fov - self.fov) * t;
        (self.look_at, self.yaw, self.fov)
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
    fn cutscene_camera_mvp_is_finite() {
        let m = cutscene_camera_mvp([0.0, 0.0, 0.0], 0.0, 60f32.to_radians(), LO, HI, 16.0 / 9.0);
        assert!(finite(&m));
    }

    #[test]
    fn cutscene_camera_tracks_its_look_at() {
        // Re-targeting the camera (the pinned op-0x45 focus params changing)
        // must change the projection - the shot follows the cutscene target.
        let fov = 60f32.to_radians();
        let a = cutscene_camera_mvp([0.0, 0.0, 0.0], 0.0, fov, LO, HI, 1.5);
        let b = cutscene_camera_mvp([500.0, 0.0, -300.0], 0.0, fov, LO, HI, 1.5);
        assert_ne!(a.to_cols_array(), b.to_cols_array());
        assert!(finite(&b));
    }

    #[test]
    fn cutscene_camera_yaw_and_fov_change_the_view() {
        let fov = 60f32.to_radians();
        let base = cutscene_camera_mvp([0.0, 0.0, 0.0], 0.0, fov, LO, HI, 1.5);
        // A quarter-turn heading orbits the eye -> different projection.
        let yawed = cutscene_camera_mvp(
            [0.0, 0.0, 0.0],
            std::f32::consts::FRAC_PI_2,
            fov,
            LO,
            HI,
            1.5,
        );
        assert_ne!(base.to_cols_array(), yawed.to_cols_array());
        // A narrower FOV (more zoom, from a larger GTE H) also changes it, and
        // an out-of-range FOV is clamped to a finite matrix.
        let zoomed = cutscene_camera_mvp([0.0, 0.0, 0.0], 0.0, 20f32.to_radians(), LO, HI, 1.5);
        assert_ne!(base.to_cols_array(), zoomed.to_cols_array());
        let clamped = cutscene_camera_mvp([0.0, 0.0, 0.0], 0.0, 0.0, LO, HI, 1.5);
        assert!(finite(&clamped));
    }

    #[test]
    fn cutscene_interp_first_call_snaps_to_target() {
        let mut it = CutsceneCameraInterp::new();
        let (la, yaw, fov) = it.approach([100.0, 0.0, -50.0], 1.0, 0.8, 0.2);
        assert_eq!(la, [100.0, 0.0, -50.0]);
        assert_eq!(yaw, 1.0);
        assert_eq!(fov, 0.8);
    }

    #[test]
    fn cutscene_interp_eases_toward_a_changed_beat() {
        let mut it = CutsceneCameraInterp::new();
        // Snap to beat A.
        it.approach([0.0, 0.0, 0.0], 0.0, 1.0, 0.25);
        // Beat B re-targets; one ease step covers a fraction, not all of it.
        let (la, _, fov) = it.approach([100.0, 0.0, 0.0], 0.0, 2.0, 0.25);
        assert!(
            (la[0] - 25.0).abs() < 1e-3,
            "look_at eased 25% -> {}",
            la[0]
        );
        assert!((fov - 1.25).abs() < 1e-3, "fov eased 25% -> {fov}");
        // Repeated steps converge toward the target.
        for _ in 0..200 {
            it.approach([100.0, 0.0, 0.0], 0.0, 2.0, 0.25);
        }
        let (la, _, fov) = it.approach([100.0, 0.0, 0.0], 0.0, 2.0, 0.25);
        assert!((la[0] - 100.0).abs() < 1e-2);
        assert!((fov - 2.0).abs() < 1e-2);
    }

    #[test]
    fn cutscene_interp_yaw_takes_shortest_arc() {
        use std::f32::consts::PI;
        let mut it = CutsceneCameraInterp::new();
        // Start just below +π, target just above -π (i.e. ~6° apart across the
        // wrap). The ease must move the short way (toward +π / over the seam),
        // never unwind ~352° the long way.
        it.approach([0.0, 0.0, 0.0], PI - 0.05, 1.0, 0.5);
        let (_, yaw, _) = it.approach([0.0, 0.0, 0.0], -PI + 0.05, 1.0, 0.5);
        // Halfway across a ~0.1 rad arc lands near ±π, not near 0.
        assert!(
            yaw.abs() > PI - 0.1,
            "shortest arc stays near the seam: {yaw}"
        );
    }

    #[test]
    fn cutscene_interp_reset_resnaps() {
        let mut it = CutsceneCameraInterp::new();
        it.approach([0.0, 0.0, 0.0], 0.0, 1.0, 0.25);
        it.reset();
        let (la, _, _) = it.approach([500.0, 0.0, 0.0], 0.0, 0.25, 0.25);
        assert_eq!(la, [500.0, 0.0, 0.0], "reset snaps the next approach");
    }
}

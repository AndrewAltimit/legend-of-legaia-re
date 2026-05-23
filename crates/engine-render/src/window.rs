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
/// `pan_z`) - mirroring the top-view debug camera's controls. Geometry uses
/// the PSX Y-down convention (same as [`orbit_camera_mvp`]), so the eye is
/// pushed to *negative* Y to sit above the terrain.
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
    // Frame the AABB centre, offset by the player's top-view pan.
    let center = (lo + hi) * 0.5 + Vec3::new(pan_x as f32, 0.0, pan_z as f32);
    let radius = ((hi - lo).length() * 0.5).max(1.0);
    let base_distance = radius / 30f32.to_radians().tan() * 1.6;
    // Zoom nudges the framing distance; clamp so the player can't invert or
    // fly infinitely far out. `zoom` accrues in steps of 4, so 512 ~= a wide
    // usable band.
    let zoom_mult = (1.0 - (zoom as f32) / 512.0).clamp(0.25, 3.0);
    let distance = base_distance * zoom_mult;
    // PSX angle units: 4096 == 2*pi.
    let angle = (azimuth as f32) / 4096.0 * std::f32::consts::TAU;
    // Elevated vantage (0.7 of distance above centre) for the map's bird's-eye
    // look; negative Y is "up" under the Y-down geometry convention.
    let eye = center
        + Vec3::new(
            distance * angle.cos(),
            -distance * 0.7,
            distance * angle.sin(),
        );
    let view = Mat4::look_at_rh(eye, center, Vec3::Y);
    let near = (distance * 0.05).max(0.1);
    let far = distance * 4.0 + 1000.0;
    let proj = Mat4::perspective_rh(60f32.to_radians(), aspect.max(0.01), near, far);
    proj * view
}

/// Camera MVP for an in-engine cutscene (the `opdeene` opening prologue),
/// framing the cutscene's look-at target from a fixed cinematic vantage.
///
/// Unlike [`orbit_camera_mvp`] (which slowly rotates around the player) this is
/// a static cinematic shot: the eye sits above and in front of `look_at` at a
/// distance derived from the scene AABB, so the framing tracks wherever the
/// cutscene directs the camera while staying sized to the loaded geometry. The
/// `look_at` point comes from the cutscene timeline's executed op-`0x45` Camera
/// Configure params (the pinned X/Z camera target, params 6 / 8); the engine
/// reads them from `World::camera_state` and supplies them here.
///
/// Geometry uses the same PSX Y-down convention as [`orbit_camera_mvp`] and
/// [`world_map_camera_mvp`]: the eye is pushed to *negative* Y to sit above the
/// target, with `+Y` as the look-at up vector.
pub fn cutscene_camera_mvp(
    look_at: [f32; 3],
    aabb_lo: [f32; 3],
    aabb_hi: [f32; 3],
    aspect: f32,
) -> Mat4 {
    let center = Vec3::from(look_at);
    let lo = Vec3::from(aabb_lo);
    let hi = Vec3::from(aabb_hi);
    let radius = ((hi - lo).length() * 0.5).max(1.0);
    // Slightly tighter than the orbit framing so the cinematic shot fills more
    // of the screen. The op-`0x45` eye / distance params are not yet pinned, so
    // this vantage is an approximation (see `docs/subsystems/cutscene.md`).
    let distance = radius / 30f32.to_radians().tan() * 1.2;
    // Three-quarter vantage: in front (`+Z`) and above (`-Y` under Y-down).
    let eye = center + Vec3::new(0.0, -distance * 0.45, distance);
    let view = Mat4::look_at_rh(eye, center, Vec3::Y);
    let near = (distance * 0.05).max(0.1);
    let far = distance * 4.0 + 1000.0;
    let proj = Mat4::perspective_rh(60f32.to_radians(), aspect.max(0.01), near, far);
    proj * view
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
        let m = cutscene_camera_mvp([0.0, 0.0, 0.0], LO, HI, 16.0 / 9.0);
        assert!(finite(&m));
    }

    #[test]
    fn cutscene_camera_tracks_its_look_at() {
        // Re-targeting the camera (the pinned op-0x45 X/Z params changing)
        // must change the projection - the shot follows the cutscene target.
        let a = cutscene_camera_mvp([0.0, 0.0, 0.0], LO, HI, 1.5);
        let b = cutscene_camera_mvp([500.0, 0.0, -300.0], LO, HI, 1.5);
        assert_ne!(a.to_cols_array(), b.to_cols_array());
        assert!(finite(&b));
    }
}

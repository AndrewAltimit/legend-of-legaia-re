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
/// - `aabb_lo`, `aabb_hi` — world-space bounding box of the scene.
/// - `orbit_speed` — angular velocity in radians/second (typical: 0.15–0.25).
/// - `eye_height` — fraction of `distance` to push the eye above centre
///   (positive = up in camera space; typical: 0.4–0.45).
/// - `elapsed_secs` — time since the window was opened; drives the angle.
/// - `aspect` — viewport width / height.
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

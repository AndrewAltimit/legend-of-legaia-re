//! The host frame loop's engine-side rules: how many sim ticks a display
//! frame runs, the camera's place around the world tick, and the cutscene
//! glide's clock.
//!
//! The port ships three hosts on one engine (the native play window, the
//! browser play page, the browser minigames page). Each one owns its own
//! display loop - winit's `RedrawRequested` natively, `requestAnimationFrame`
//! on the page - and each used to spell these rules out locally. Two
//! spellings of one rule drift without a diff showing it: the native
//! accumulator kept any backlog past its per-frame cap while the page
//! discarded it, the page published the camera azimuth one tick late, and
//! both hosts advanced the cutscene glide on idle redraws. The rules live
//! here instead, and every host calls them.
//!
//! ## The frame model
//!
//! One [`crate::world::World::tick`] is one retail vsync. Retail runs one sim
//! step per vsync and never catches up: a frame that runs long makes the game
//! run slow (the adaptive frame step `DAT_1F800393` changes the game-tick
//! cadence, not the number of vsyncs a second of play is made of). The hosts
//! render at the display's refresh, which is not 60 Hz on most monitors, so
//! each display frame drains wall time through a [`SimStepper`]:
//!
//! - it runs as many whole 1/60 s ticks as have elapsed, carrying the
//!   remainder, so a 120 or 144 Hz display runs the world at retail speed
//!   and a 30 Hz one runs two ticks a frame;
//! - it keeps at most [`MAX_BACKLOG_TICKS`] of backlog, so a stall (a hidden
//!   tab, a shader compile, a debugger break) costs wall time rather than
//!   returning as a burst of catch-up ticks - the nearest a host can come to
//!   retail's "a slow frame is a slow game".
//!
//! Everything that advances in retail-frame time counts ticks, never
//! redraws. The cutscene glide ([`CutsceneGlide`]) diffs
//! [`crate::world::FrameClock::display_frames`], so a redraw on which no tick
//! ran advances it by nothing.

use legaia_engine_vm::psx_camera::{CutsceneCameraInterp, FieldCameraView};

use crate::camera::Camera;
use crate::world::World;

/// One sim tick, in seconds: the retail NTSC vsync period the engine's tick
/// is denominated in (`SIM_HZ == RETAIL_FPS`).
pub const TICK_SECS: f64 = 1.0 / 60.0;

/// The most ticks one display frame may run, and the most backlog the
/// stepper keeps: wall time beyond it is dropped.
pub const MAX_BACKLOG_TICKS: u32 = 4;

/// Wall-clock to sim-tick accumulator shared by every host.
///
/// A host calls [`Self::drain`] once per display frame with the wall time
/// since the previous call and runs that many [`crate::world::World::tick`]s.
/// While the game is paused or single-stepped it calls [`Self::resync`] so
/// the gap does not come back as catch-up ticks.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SimStepper {
    /// Undrained wall time, seconds, in `[0, MAX_BACKLOG_TICKS * TICK_SECS]`.
    accum: f64,
}

impl SimStepper {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add `elapsed_secs` of wall time and return how many ticks to run now.
    ///
    /// The backlog is clamped to [`MAX_BACKLOG_TICKS`] ticks **before** it
    /// drains, so the return never exceeds that and no remainder above it
    /// survives into the next frame. A negative or non-finite delta (a clock
    /// that stepped backwards) adds nothing.
    pub fn drain(&mut self, elapsed_secs: f64) -> u32 {
        if elapsed_secs.is_finite() && elapsed_secs > 0.0 {
            self.accum += elapsed_secs;
        }
        let cap = f64::from(MAX_BACKLOG_TICKS) * TICK_SECS;
        if self.accum > cap {
            self.accum = cap;
        }
        // Whole ticks elapsed. The epsilon keeps a delta that is exactly a
        // tick in decimal (16.666.. ms) from flooring to zero on rounding.
        let ticks = ((self.accum + 1e-9) / TICK_SECS).floor() as u32;
        let ticks = ticks.min(MAX_BACKLOG_TICKS);
        self.accum = (self.accum - f64::from(ticks) * TICK_SECS).max(0.0);
        ticks
    }

    /// Drop the undrained backlog (pause, single-step, a scene rebuild the
    /// host timed separately).
    pub fn resync(&mut self) {
        self.accum = 0.0;
    }

    /// Undrained wall time, seconds.
    pub fn backlog_secs(&self) -> f64 {
        self.accum
    }
}

/// The camera's half of a world tick, before it: return a free-roam camera
/// to the follow default, then publish the compass azimuth the d-pad remap
/// reads **this** tick - `azimuth_override` when the host has a heading the
/// engine camera cannot know (VR first-person gaze, the page's debug orbit),
/// else the camera's own ([`Camera::compass_azimuth_units`]).
///
/// The native session always ran this before [`crate::scene::SceneHost::tick`];
/// the browser page ran it after, so its locomotion read the azimuth the
/// camera had one tick earlier.
pub fn camera_before_world_tick(
    camera: &mut Camera,
    world: &mut World,
    azimuth_override: Option<u16>,
) {
    camera.reset_for_free_roam(world);
    let az = azimuth_override.unwrap_or_else(|| camera.compass_azimuth_units_for(world));
    world.locomotion.camera_azimuth = az % 4096;
}

/// The camera's half of a world tick, after it: fold this tick's op-`0x45`
/// events into the controller, advance the retail camera globals, and on a
/// scene entry reset them (`FUN_80025C24`) so the departing scene's shot
/// cannot leak into the next.
///
/// REF: FUN_80025C24
pub fn camera_after_world_tick(camera: &mut Camera, world: &mut World, scene_entered: bool) {
    camera.route_camera_events(world);
    camera.tick_on_stream(world);
    if scene_entered {
        camera.reset_globals_for_scene_entry();
    }
}

/// The cutscene camera's between-beat glide plus the clock it advances on.
///
/// The interpolator ([`CutsceneCameraInterp`]) moves each component over the
/// staging beat's `apply` frames, and `apply` is a count of retail display
/// frames. [`Self::advance`] therefore steps it by the display frames the
/// world ran since the previous call - `0` on a redraw that ran no tick. Both
/// hosts used to floor that count at `1`, which advanced the glide once per
/// *redraw*: at 120 Hz a glide finished in half its frames.
///
/// A scene entry drops the glide ([`Self::reset`]): retail's field entry
/// (`FUN_80025C24`) rewrites the camera globals and kills the mover in
/// flight, so no pose survives a door for the next scene's first beat to
/// glide from. The native window used to keep its interpolator across a door
/// whose destination opened on a timeline, gliding the new scene's first shot
/// out of the old scene's pose; the page reset.
#[derive(Debug, Clone, Default)]
pub struct CutsceneGlide {
    interp: CutsceneCameraInterp,
    /// [`crate::world::FrameClock::display_frames`] at the previous advance.
    mark: u64,
}

impl CutsceneGlide {
    pub fn new() -> Self {
        Self::default()
    }

    /// Display frames since the previous call, and move the mark to `now`.
    /// `0` when no world tick ran in between.
    pub fn take_steps(&mut self, now: u64) -> u32 {
        let steps = u32::try_from(now.saturating_sub(self.mark)).unwrap_or(u32::MAX);
        self.mark = now;
        steps
    }

    /// This frame's cutscene view: replay the `apply == 0` beats banked on
    /// `camera` as snaps (a snap and a glide committed in one tick glide from
    /// the snapped pose), then step the glide toward `target` by the display
    /// frames elapsed since the last call.
    pub fn advance(
        &mut self,
        world: &World,
        camera: &mut Camera,
        target: FieldCameraView,
    ) -> FieldCameraView {
        for comps in camera.take_camera_snap_beats() {
            self.interp.snap_components(&comps);
        }
        let apply = u32::from(world.camera.state.apply_trigger);
        let mode = world.camera.state.mode;
        let steps = self.take_steps(world.clock.display_frames);
        self.interp.glide_view(target, apply, mode, steps)
    }

    /// No scripted shot owns this frame: drop the held pose so the next shot
    /// snaps in, and drop the banked snaps, which would otherwise land on it.
    pub fn idle(&mut self, camera: &mut Camera) {
        self.interp.reset();
        camera.clear_camera_snap_beats();
    }

    /// Drop the held pose (scene entry, a direct scene pick).
    pub fn reset(&mut self) {
        self.interp.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: f64 = 1.0 / 1000.0;

    /// The world runs at retail speed at any refresh: 1 s of wall time is
    /// 60 ticks at 30, 60, 120 and 144 Hz.
    #[test]
    fn one_second_is_sixty_ticks_at_any_refresh() {
        for hz in [30.0f64, 60.0, 75.0, 120.0, 144.0, 240.0] {
            let mut s = SimStepper::new();
            let frames = hz as u32;
            let total: u32 = (0..frames).map(|_| s.drain(1.0 / hz)).sum();
            assert!(
                (59..=60).contains(&total),
                "{hz} Hz ran {total} ticks in one second"
            );
        }
    }

    /// A stall returns at most four ticks, and none of it carries over: the
    /// next ordinary frame runs one tick, not another burst.
    #[test]
    fn a_stall_is_clamped_and_its_excess_dropped() {
        let mut s = SimStepper::new();
        assert_eq!(s.drain(2.0), MAX_BACKLOG_TICKS);
        assert!(s.backlog_secs() < TICK_SECS);
        assert_eq!(s.drain(16.7 * MS), 1);
    }

    /// Sustained slow frames (a host that renders at 10 Hz) run four ticks a
    /// frame and never build a backlog - the game runs slow, as retail does
    /// under load, instead of the accumulator growing without bound.
    #[test]
    fn slow_frames_do_not_accumulate_backlog() {
        let mut s = SimStepper::new();
        for _ in 0..100 {
            assert_eq!(s.drain(100.0 * MS), MAX_BACKLOG_TICKS);
            assert!(s.backlog_secs() <= f64::from(MAX_BACKLOG_TICKS) * TICK_SECS);
        }
    }

    #[test]
    fn resync_and_backwards_clocks_add_nothing() {
        let mut s = SimStepper::new();
        s.drain(10.0 * MS);
        s.resync();
        assert_eq!(s.drain(10.0 * MS), 0);
        assert_eq!(s.drain(-5.0), 0);
        assert_eq!(s.drain(f64::NAN), 0);
        assert_eq!(s.drain(7.0 * MS), 1);
    }

    /// An idle redraw (no tick ran) steps the glide by zero.
    #[test]
    fn idle_redraws_take_no_steps() {
        let mut g = CutsceneGlide::new();
        assert_eq!(g.take_steps(10), 10);
        assert_eq!(g.take_steps(10), 0);
        assert_eq!(g.take_steps(12), 2);
    }

    /// A 60-frame glide takes 60 ticks of world time to arrive whatever the
    /// refresh rate: at 144 Hz the hosts redraw ~144 times over those ticks
    /// and the glide must still land on tick 60, not earlier.
    #[test]
    fn a_glide_spans_its_apply_frames_at_120_and_144_hz() {
        for hz in [60.0f64, 120.0, 144.0] {
            let mut world = World::default();
            let mut camera = Camera::default();
            let mut glide = CutsceneGlide::new();
            let mut stepper = SimStepper::new();
            let start = FieldCameraView {
                focus: [0.0, 0.0, 0.0],
                pitch: 0.0,
                yaw: 0.0,
                roll: 0.0,
                h: 512.0,
                tr_eye: [0.0, 0.0, 1000.0],
            };
            let target = FieldCameraView {
                focus: [600.0, 0.0, 0.0],
                ..start
            };
            // First call snaps the pose to `start`.
            glide.advance(&world, &mut camera, start);
            world.camera.state.apply_trigger = 60;
            world.camera.state.mode = 1;
            let mut arrived_at = None;
            for _ in 0..(hz as u32 * 2) {
                for _ in 0..stepper.drain(1.0 / hz) {
                    world.clock.display_frames += 1;
                }
                let v = glide.advance(&world, &mut camera, target);
                if arrived_at.is_none() && (v.focus[0] - 600.0).abs() < 1e-3 {
                    arrived_at = Some(world.clock.display_frames);
                }
            }
            assert_eq!(arrived_at, Some(60), "{hz} Hz");
        }
    }
}

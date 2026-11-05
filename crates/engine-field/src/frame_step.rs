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
//! One world tick (engine-core's `World::tick`) is one retail vsync. Retail
//! runs one sim step per vsync and never catches up: a frame that runs long
//! makes the game run slow (the adaptive frame step `DAT_1F800393` changes the game-tick
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
//! redraws. The camera mover ([`Camera::tick`]) diffs
//! [`legaia_engine_vm::frame_clock::FrameClock::display_frames`], so a redraw on which no tick
//! ran advances it by nothing.

use legaia_engine_camera::psx_camera::FieldCameraView;

use crate::camera::{Camera, CameraWorld};
use crate::camera_rig::CameraRig;

/// The slice of the world the frame-step rules touch beyond the camera's own
/// seat ([`CameraWorld`]): the compass azimuth the d-pad remap reads, the
/// positional sound emitters' listener focus, the battle exit's camera
/// re-seat latch, and the camera rig the cutscene glide reads its beat from.
/// engine-core's `World` implements it next to [`CameraWorld`].
pub trait FrameStepWorld: CameraWorld {
    /// Publish the compass azimuth (`0..4096`) the field locomotion's d-pad
    /// remap reads this tick.
    fn set_camera_azimuth(&mut self, azimuth: u16);
    /// Whether the d-pad remap reads the scene's authored octant
    /// (`gp+0x2D8`): the retail follow view with nothing turning it.
    fn set_pad_octant_live(&mut self, _live: bool) {}
    /// Install the camera focus the positional sound emitters listen from,
    /// as retail stores it (`_DAT_80089118` / `_DAT_80089120`, negated).
    fn set_camera_focus_stored(&mut self, focus_stored_x: i32, focus_stored_z: i32);
    /// Take the battle exit's camera re-seat latch: `true` once per return to
    /// the field or the overworld.
    fn take_field_camera_reseat(&mut self) -> bool;
    /// The field camera rig (the staging beat's `apply` count and mode).
    fn camera_rig(&self) -> &CameraRig;
}

/// One sim tick, in seconds: the retail NTSC vsync period the engine's tick
/// is denominated in (`SIM_HZ == RETAIL_FPS`).
pub const TICK_SECS: f64 = 1.0 / 60.0;

/// The most ticks one display frame may run, and the most backlog the
/// stepper keeps: wall time beyond it is dropped.
pub const MAX_BACKLOG_TICKS: u32 = 4;

/// Wall-clock to sim-tick accumulator shared by every host.
///
/// A host calls [`Self::drain`] once per display frame with the wall time
/// since the previous call and runs that many world ticks.
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
/// The native session always ran this before `legaia_engine_scene_host::scene_host::SceneHost::tick`;
/// the browser page ran it after, so its locomotion read the azimuth the
/// camera had one tick earlier.
pub fn camera_before_world_tick<W: FrameStepWorld + ?Sized>(
    camera: &mut Camera,
    world: &mut W,
    azimuth_override: Option<u16>,
) {
    camera.reset_for_free_roam(world);
    let az = azimuth_override.unwrap_or_else(|| camera.compass_azimuth_units_for(world));
    world.set_camera_azimuth(az % 4096);
    // Retail's remap rotates by the scene's authored octant, not by the view:
    // the two part on the follow cameras that sweep (`town01_field_card_boot`
    // yaw `-260`, octant `0`; `kor3_field_route_walk` yaw `511`, octant `6`).
    // A view the player or a host has turned has no authored octant to read.
    world.set_pad_octant_live(
        azimuth_override.is_none() && camera.manual_orbit == 0.0 && camera.render_yaw_bias != 0.0,
    );
    // The focus the camera last stored (`_DAT_80089118` / `_DAT_80089120`):
    // the positional sound emitters measure their box against it, and move-VM
    // ext `0x24` parts seat on it.
    let focus = camera.globals.focus_stored();
    world.set_camera_focus_stored(focus[0], focus[2]);
}

/// The camera's half of a world tick, after it: fold this tick's op-`0x45`
/// events into the controller, advance the retail camera globals, and on a
/// scene entry reset them (`FUN_80025C24`) so the departing scene's shot
/// cannot leak into the next - and on a battle's return to the field, which
/// retail runs through the same field init.
///
/// REF: FUN_80025C24, FUN_801D6704
pub fn camera_after_world_tick<W: FrameStepWorld + ?Sized>(
    camera: &mut Camera,
    world: &mut W,
    scene_entered: bool,
) {
    // A battle that returned to the field / overworld this tick re-enters
    // field mode in retail, whose init runs the same reset (`FUN_801D6704`
    // -> `FUN_80025C24` at `0x801D698C`). It runs ahead of this tick's camera
    // step so the arrival snap it arms lands on the first field frame: the
    // camera ticked through the fight with the party's battle seat as its
    // follow target, and the overworld's standing-player ease would
    // otherwise hold that focus (clamped to the region corner) until the
    // player moved.
    if world.take_field_camera_reseat() {
        camera.reset_globals_for_scene_entry();
    }
    camera.route_camera_events(world);
    camera.tick_on_stream(world);
    if scene_entered {
        camera.reset_globals_for_scene_entry();
    }
}

/// A scripted shot's view: the live camera globals.
///
/// Retail has one pose. An `apply == 0` Configure writes the ten globals, an
/// `apply != 0` one hands the mover `(live, target)` pairs, and `FUN_800172C0`
/// builds the frame's view from the globals whichever ran
/// ([`Camera::tick`] steps the mover on display frames). A second glide kept
/// beside them, armed by comparing targets on the frame a host looked, missed
/// any shot that opened on a snap and a glide in one slice: `taiku2`'s
/// arrival runs `45 07 .. 00 00` and `45 07 .. 58 02` back to back, and the
/// frame stood on the 600-frame glide's end pose from its first frame.
///
/// `target` (`camera_view::cutscene_view`) answers only for the two globals
/// with no usable value: an eye depth or `H` of `0` is the reset state, not a
/// shot.
///
/// REF: FUN_800172C0, FUN_801DC0BC, FUN_801DE084
pub fn scripted_view(camera: &Camera, target: FieldCameraView) -> FieldCameraView {
    use crate::camera::CUTSCENE_WORLD_SCALE as S;
    let g = &camera.globals;
    let angle = |v: i32| v as f32 / 4096.0 * std::f32::consts::TAU;
    FieldCameraView {
        focus: g.focus_world().map(|v| v as f32),
        pitch: angle(g.0[0]),
        yaw: angle(g.0[1]),
        roll: angle(g.0[2]),
        h: if g.h() > 1 { g.h() as f32 } else { target.h },
        tr_eye: [
            g.0[3] as f32 / S,
            g.0[4] as f32 / S,
            if g.0[5].abs() > 1 {
                g.0[5] as f32 / S
            } else {
                target.tr_eye[2]
            },
        ],
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

    fn target() -> FieldCameraView {
        FieldCameraView {
            focus: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            roll: 0.0,
            h: 512.0,
            tr_eye: [0.0, 0.0, 2000.0],
        }
    }

    /// `taiku2`'s arrival: the snap and the 600-frame glide run in one slice,
    /// and half way along the frame stands half way along, not on the end
    /// pose the merged staging holds.
    #[test]
    fn a_snap_and_a_glide_in_one_slice_draw_the_glide() {
        let mut cam = Camera::default();
        cam.globals.0 = [80, -900, 0, -3860, 323, 28864, -1984, 0, -2272, 536];
        let mut mv = legaia_engine_camera::camera_mover::CameraMover::default();
        mv.arm(
            cam.globals.0,
            [60, -940, 0, 380, -908, 28864, -1984, 0, -2272, 536],
            600,
            1,
        );
        cam.mover = Some(mv);
        cam.align_glide_frames_left(300);
        let v = scripted_view(&cam, target());
        let to_units = v.pitch / std::f32::consts::TAU * 4096.0;
        assert!((to_units - 70.0).abs() < 0.01, "{to_units}");
        assert!((v.tr_eye[0] - -1740.0 / 6.0).abs() < 0.01);
        assert_eq!(v.h, 536.0);
        assert_eq!(v.focus, [1984.0, 0.0, 2272.0]);
    }

    /// The reset state's zero depth and `H` are not a shot.
    #[test]
    fn a_zero_depth_and_h_fall_back_to_the_staged_target() {
        let mut cam = Camera::default();
        cam.globals.0[5] = 0;
        cam.globals.0[9] = 0;
        let v = scripted_view(&cam, target());
        assert_eq!(v.h, 512.0);
        assert_eq!(v.tr_eye[2], 2000.0);
    }
}

//! Per-scene camera controller.
//!
//! Consumes the field-VM op-`0x45` event stream (Configure / Save / Load /
//! Apply - see [`crate::field_events::FieldEvent`]) and projects a target
//! actor's world position into a screen-space view. Engines plug the result
//! into [`legaia_engine_render`] each frame.
//!
//! Two layers:
//!
//! - [`CameraState`] (in `legaia_engine_core::world`) - the raw scratch the field VM
//!   reads / writes. Holds the most recent op-`0x45` payloads.
//! - [`Camera`] (here) - the *runtime* camera. Reads `CameraState`, layers
//!   in a follow target, and exposes a `(eye, look_at)` pair plus a yaw /
//!   pitch the renderer can use to build a view matrix.
//!
//! The retail engine does the per-frame math via the third motion VM
//! ([`legaia_engine_motion_vm::motion_vm`]) and the move-VM ext sub-ops 0x06 / 0x36
//! / 0x39. This module assembles those primitives into a single Camera
//! that's easy to drive from `legaia_engine_scene_host::scene_host::SceneHost`.

use crate::field_events::FieldEvent;
use legaia_engine_camera::camera_distance::CameraDistance;
use legaia_engine_camera::camera_mover::{AXIS_COUNT, CameraMover};
use legaia_engine_motion_vm::motion_vm::{MotionState, MotionTarget, StepResult, step};

use legaia_engine_camera::retail_cam::RetailCamGlobals;

mod event_routing;
mod manual_knobs;
mod resets;
mod state;
mod tick;
mod world_seam;
mod zone_arrival;
mod zone_follow;

pub use state::*;
pub use world_seam::*;
pub use zone_arrival::*;
pub use zone_follow::*;

/// Retail folds a 6x uniform world scale into the camera rotation (base
/// matrix `DAT_8007BF10` = `24576 * I`, GTE `4096` = 1.0); the engine renders
/// at 1x, so an op-`0x45` eye-space translation reduces by this factor.
pub const CUTSCENE_WORLD_SCALE: f32 = 6.0;

/// The user's three **follow-camera knobs** - orbit, tilt and zoom - as one
/// vocabulary both hosts steer from: the play-window's left-mouse drag and
/// wheel, and the browser play page's `pointermove` / `wheel` handlers on the
/// canvas. The knobs compose onto the retail zone-driven follow camera
/// (`legaia_engine_world_views::camera_view::field_follow_view`) and stay centred on the
/// character; every one is identity at its default, so the untouched camera
/// is the retail shot bit for bit.
///
/// They are live only while the follow camera **owns the frame** - free-roam
/// field. A running cutscene timeline seizes the camera in retail, and the
/// port keeps that lock: [`Camera::follow_knobs_live`] answers `false` there
/// and the gated setters ([`Camera::orbit_by`], [`Camera::tilt_by`],
/// [`Camera::zoom_by`]) drop the gesture rather than banking an offset that
/// would snap the view when control returns. The world map's walk camera has
/// retail's own zoom (`WorldMapController`) and is not steered from here
/// either.
pub mod follow_knobs {
    /// Lower bound on the composed follow pitch, radians: keeps the eye
    /// above the floor plane. The same number the two hosts' debug orbit
    /// clamps to, so one drag feels the same on either vantage.
    pub const PITCH_MIN: f32 = 0.12;
    /// Upper bound on the composed follow pitch, radians: short of fully
    /// top-down, where the retail framing has no horizon to compose against.
    pub const PITCH_MAX: f32 = 1.35;
    /// Widest tilt offset a host can bank, radians, either sign - the whole
    /// clamp span, so a drag can always reach both ends from any scene pitch
    /// and never accumulates unseen beyond them.
    pub const TILT_LIMIT: f32 = PITCH_MAX - PITCH_MIN;
    /// Closest zoom (a multiplier on the eye-back depth): well outside the
    /// character but tight enough to read a face.
    pub const ZOOM_MIN: f32 = 0.35;
    /// Widest zoom: three times retail's depth, which under the `Farther`
    /// preset is still inside the scene clip volume.
    pub const ZOOM_MAX: f32 = 3.0;

    /// The composed follow pitch for a scene pitch `base` and a user tilt:
    /// `base + tilt` clamped into the knob range, with the range widened to
    /// include `base` itself. A scene whose own pitch sits outside the range
    /// is still framed exactly at a zero tilt, and the clamp is continuous
    /// in the tilt (no snap at the first pixel of drag).
    pub fn composed_pitch(base: f32, tilt: f32) -> f32 {
        (base + tilt).clamp(base.min(PITCH_MIN), base.max(PITCH_MAX))
    }
}

#[cfg(test)]
mod tests;

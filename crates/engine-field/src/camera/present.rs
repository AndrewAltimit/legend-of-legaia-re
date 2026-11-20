//! The smooth camera's stepped moves: what the draw shows of a move the
//! camera globals made in one step outside a glide.
//!
//! A glide has a fixed next pose, so the draw leads it ([`Camera::led_delta`]
//! with a phase). Everything else that moves the globals - the follow ease
//! `FUN_801DB510`, which steps once a game pass toward the composed shot; a
//! record that re-applies or re-stages a shot every frame (op `0x45` APPLY or
//! an `apply == 0` Configure in a loop); the camera-register ramps feeding
//! the composer - has no next pose known in advance, so it lands a whole
//! step on one vsync and holds on the vsyncs between. The smooth camera
//! draws such a move over the vsyncs that follow it instead, from where the
//! draw stood to the new stored pose, one step behind: the same trade the
//! battle draw makes (`legaia_engine_core::world::BattlePresent`).
//!
//! What stays live:
//!
//! - the focus a free-roam follow camera pins onto the player every vsync
//!   (`0x801DB820`): the player's figure moves every vsync under the smooth
//!   walk, and a lagged focus would shake it on screen;
//! - focus Y, which neither the mover nor the follow camera stores;
//! - the shake jitter pair the follow ease adds into eye X / Y;
//! - a cut: a scene entry, a battle's return to the field, the follow
//!   camera's snap (`FUN_801DB8EC`), or any step past [`CUT_ANGLE`],
//!   [`CUT_EYE`], [`CUT_FOCUS`] or [`CUT_H`] (the battle draw's cut rules,
//!   with the eye trio in the field globals' 6x scale).
//!
//! Presentation only: [`Camera::observe_present`] reads the globals the tick
//! stored and writes nothing the simulation reads, and a draw under
//! `pin_retail` never asks for it.

use super::*;
use legaia_engine_camera::camera_mover::FOCUS_Y_AXIS;

/// An angle step at least this large (12-bit units) is a cut.
pub const CUT_ANGLE: i32 = 480;
/// An eye-trio step at least this long (the globals' 6x-scaled units: `1800`
/// eye-space units, the battle draw's cut) is a cut.
pub const CUT_EYE: f32 = 1800.0 * CUTSCENE_WORLD_SCALE;
/// A focus step at least this long (world units) is a cut.
pub const CUT_FOCUS: f32 = 480.0;
/// A GTE `H` step at least this large is a cut.
pub const CUT_H: i32 = 256;

/// The last stepped move of the camera globals the draw is still showing.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PresentLag {
    /// The move, per axis, from the pose the draw stood at to the stored one.
    pub(super) jump: [f32; AXIS_COUNT],
    /// The display frame the move landed on.
    pub(super) at: u64,
    /// The vsyncs it is drawn over: the frame step it landed at.
    pub(super) span: u32,
    /// A cut landed since the last observation: the next one drops the move.
    pub(super) cut: bool,
}

impl PresentLag {
    /// The share of the move still to show at display frame `now`.
    fn remaining(&self, now: u64) -> f32 {
        if self.span == 0 {
            return 0.0;
        }
        let age = now.saturating_sub(self.at) as f32;
        (1.0 - age / self.span as f32).clamp(0.0, 1.0)
    }
}

/// The globals as a tick found them ([`Camera::present_mark`]).
#[derive(Debug, Clone, Copy)]
pub struct PresentMark {
    globals: [i32; AXIS_COUNT],
    shake: [i32; 2],
    gliding: bool,
}

/// Whether the step `d` (per axis, raw globals) is a cut.
pub fn is_cut(d: &[i32; AXIS_COUNT]) -> bool {
    let len = |a: &[i32]| a.iter().map(|&v| (v as f32).powi(2)).sum::<f32>().sqrt();
    d[..3].iter().any(|v| v.abs() >= CUT_ANGLE)
        || len(&d[3..6]) >= CUT_EYE
        || len(&[d[6], d[8]]) >= CUT_FOCUS
        || d[9].abs() >= CUT_H
}

impl Camera {
    /// The globals before a tick, for [`Self::observe_present`].
    pub fn present_mark(&self) -> PresentMark {
        PresentMark {
            globals: self.globals.0,
            shake: self.zone.shake_offset,
            gliding: self.mover.is_some(),
        }
    }

    /// Record the move this tick made to the globals since `mark`, for the
    /// draw to show over the vsyncs that follow ([`Self::led_delta`]).
    ///
    /// A move a glide in flight made is the lead's (the draw already showed
    /// it on the way), and a cut drops whatever was still to show. A new
    /// move picks up where the draw stands: the share of the last one not
    /// yet shown carries into it, so the drawn pose never jumps.
    pub fn observe_present<W: CameraWorld + ?Sized>(&mut self, world: &W, mark: PresentMark) {
        let now = world.display_frames();
        let g = self.globals.0;
        let mut d: [i32; AXIS_COUNT] = std::array::from_fn(|k| g[k] - mark.globals[k]);
        d[3] -= self.zone.shake_offset[0] - mark.shake[0];
        d[4] -= self.zone.shake_offset[1] - mark.shake[1];
        d[FOCUS_Y_AXIS] = 0;
        if std::mem::take(&mut self.present.cut) || is_cut(&d) {
            self.present = PresentLag::default();
            return;
        }
        if mark.gliding {
            d = [0; AXIS_COUNT];
        }
        if self.focus_pinned_live(world) {
            d[6] = 0;
            d[8] = 0;
        }
        if d == [0; AXIS_COUNT] {
            return;
        }
        let rem = self.present.remaining(now);
        let mut jump = self.present.jump.map(|j| j * rem);
        for (j, v) in jump.iter_mut().zip(d) {
            *j += v as f32;
        }
        self.present = PresentLag {
            jump,
            at: now,
            span: u32::from(world.camera_frame_step().max(1)),
            cut: false,
        };
    }

    /// The share of the last stepped move a draw at display frame `now`
    /// has still to show ([`DrawLead::lag`]).
    pub fn present_lag(&self, now: u64) -> f32 {
        self.present.remaining(now)
    }

    /// Mark a cut: the next [`Self::observe_present`] draws the stored pose.
    pub(super) fn present_cut(&mut self) {
        self.present.cut = true;
    }

    /// Whether the focus is the free-roam follow camera's per-vsync pin onto
    /// the player (`0x801DB820`, the non-zone writeback): drawn live. A
    /// mode-5 fixed shot eases its focus toward the anchor once a pass, and
    /// a script's focus is the script's, so both draw as stepped moves.
    fn focus_pinned_live<W: CameraWorld + ?Sized>(&self, world: &W) -> bool {
        let scripted = self.script_owns_focus
            || self.zone.script_view
            || world.cutscene_timeline_holds_camera();
        let mode5 = zone_camera_scene(world) && self.zone.config.mode_nibble() == 5;
        self.mode == CameraMode::Follow && !scripted && !mode5
    }
}

//! The player-driven follow knobs (orbit, tilt, zoom) and the compass azimuth they feed.

use super::*;

impl Camera {
    /// The camera azimuth to feed
    /// `legaia_engine_core::world::FieldLocomotion::camera_azimuth`
    /// this frame, in PSX 12-bit units (`4096` = full turn): scripted yaw +
    /// the user's manual orbit + the host renderer's fixed framing bias.
    /// This is what keeps the d-pad -> world-direction remap ("screen up
    /// walks away from the camera") tracking the yaw the player actually
    /// sees, including after a drag-orbit. All three terms default to `0`,
    /// so headless hosts keep the historical `yaw`-only feed bit-identical.
    pub fn compass_azimuth_units(&self) -> u16 {
        self.compass_azimuth_with_orbit(self.manual_orbit)
    }

    /// [`Self::compass_azimuth_units`] for `world`'s mode: the kingdom
    /// overworld frames its walk camera without the user's orbit
    /// (`camera_view::resolve_field_camera`), so the d-pad remap there must
    /// not turn by it either, or "up" stops walking up-screen.
    pub fn compass_azimuth_units_for<W: CameraWorld + ?Sized>(&self, world: &W) -> u16 {
        if world.scene_mode() == legaia_engine_mode::mode::SceneMode::WorldMap {
            self.compass_azimuth_with_orbit(0.0)
        } else {
            self.compass_azimuth_units()
        }
    }

    fn compass_azimuth_with_orbit(&self, manual_orbit: f32) -> u16 {
        // A host that renders the retail follow view declares it through a
        // non-zero `render_yaw_bias`; while the zone camera composes that
        // view, the bias it actually sees is the live follow yaw's negation
        // (compass sense = `-psi`), not the pinned anchor value.
        let bias = match self.zone_follow_yaw_units() {
            Some(yaw) if self.render_yaw_bias != 0.0 => {
                -(yaw as f32) / 4096.0 * std::f32::consts::TAU
            }
            _ => self.render_yaw_bias,
        };
        let az = (self.yaw + manual_orbit + bias) / std::f32::consts::TAU * 4096.0;
        az.rem_euclid(4096.0) as u16
    }

    /// Whether the user's follow-camera knobs steer this frame: free-roam
    /// field, with no cutscene timeline owning the camera. The same gate
    /// [`Self::reset_for_free_roam`] and the field locomotion step run on,
    /// so the camera is the player's exactly when the character is.
    pub fn follow_knobs_live<W: CameraWorld + ?Sized>(&self, world: &W) -> bool {
        matches!(
            world.scene_mode(),
            legaia_engine_mode::mode::SceneMode::Field
        ) && !world.cutscene_timeline_active()
    }

    /// Swing [`Self::manual_orbit`] by `radians` (compass sense), when the
    /// knobs are live. Returns whether the gesture was taken.
    pub fn orbit_by<W: CameraWorld + ?Sized>(&mut self, world: &W, radians: f32) -> bool {
        if !self.follow_knobs_live(world) {
            return false;
        }
        self.manual_orbit = (self.manual_orbit + radians).rem_euclid(std::f32::consts::TAU);
        true
    }

    /// Swing [`Self::manual_orbit`] by `radians` from the host's **debug
    /// orbit** vantage (the `F3` toggle both hosts carry).
    ///
    /// The debug orbit is not a second yaw: both hosts compose their vantage
    /// as `fixed diagonal + manual_orbit`, so the drag has to land on the
    /// same field the follow camera reads, or leaving the toggle snaps the
    /// view by however far the two drifted apart. Unlike [`Self::orbit_by`]
    /// this is NOT cutscene-gated - the debug vantage is a dev viewpoint and
    /// ignores whoever owns the scripted camera - but it is still field-only,
    /// so a drag on the world map or in battle cannot rewrite the compass the
    /// locomotion remap reads. Returns whether the gesture was taken.
    pub fn debug_orbit_by<W: CameraWorld + ?Sized>(&mut self, world: &W, radians: f32) -> bool {
        if !matches!(
            world.scene_mode(),
            legaia_engine_mode::mode::SceneMode::Field
        ) {
            return false;
        }
        self.manual_orbit = (self.manual_orbit + radians).rem_euclid(std::f32::consts::TAU);
        true
    }

    /// Tip [`Self::manual_tilt`] by `radians` (positive = further down),
    /// when the knobs are live. Returns whether the gesture was taken.
    pub fn tilt_by<W: CameraWorld + ?Sized>(&mut self, world: &W, radians: f32) -> bool {
        if !self.follow_knobs_live(world) {
            return false;
        }
        self.manual_tilt =
            (self.manual_tilt + radians).clamp(-follow_knobs::TILT_LIMIT, follow_knobs::TILT_LIMIT);
        true
    }

    /// Scale [`Self::manual_zoom`] by `factor` (`> 1` pulls the eye back),
    /// when the knobs are live. Returns whether the gesture was taken.
    pub fn zoom_by<W: CameraWorld + ?Sized>(&mut self, world: &W, factor: f32) -> bool {
        if !self.follow_knobs_live(world) || !factor.is_finite() || factor <= 0.0 {
            return false;
        }
        self.manual_zoom =
            (self.manual_zoom * factor).clamp(follow_knobs::ZOOM_MIN, follow_knobs::ZOOM_MAX);
        true
    }

    /// Put the three knobs back at their retail-identical defaults (the
    /// coarse distance preset is a persisted option and stays).
    pub fn reset_follow_knobs(&mut self) {
        self.manual_orbit = 0.0;
        self.manual_tilt = 0.0;
        self.manual_zoom = 1.0;
    }

    /// The **reset camera** control both play hosts carry (the page's
    /// button and canvas double-click, the native window's double-click):
    /// put the view back at the retail framing on the current lead.
    ///
    /// Always zeroes the user's knobs ([`Self::reset_follow_knobs`]), in any
    /// mode, so a drag left over from the field cannot greet the player when
    /// a fight or the world map hands the camera back. In free roam
    /// ([`Self::follow_knobs_live`]) it also releases a scripted shot the
    /// scene left holding the focus: an op-`0x45` beat with no hand-back
    /// keeps `script_owns_focus` latched, and the follow camera never
    /// re-pins the focus to the party until the next scene entry. The glide
    /// in flight, the cinematic angles and the latch go, and the zone
    /// camera's snap is armed so the composed follow pose lands on the next
    /// tick. While a cutscene owns the camera only the knobs reset - the
    /// shot is the script's. Returns whether the free-roam half ran.
    pub fn recenter_follow<W: CameraWorld + ?Sized>(&mut self, world: &W) -> bool {
        self.reset_follow_knobs();
        if !self.follow_knobs_live(world) {
            return false;
        }
        self.mode = CameraMode::Follow;
        self.yaw = 0.0;
        self.pitch = 0.0;
        self.roll = 0.0;
        self.mover = None;
        self.script_owns_focus = false;
        self.zone.snap_pending = true;
        self.zone.prev_player = None;
        true
    }

    /// Whether a scripted shot holds the focus (the op-`0x45` latch the
    /// follow camera yields to), for a host's diagnostics and tests.
    pub fn script_owns_focus(&self) -> bool {
        self.script_owns_focus
    }
}

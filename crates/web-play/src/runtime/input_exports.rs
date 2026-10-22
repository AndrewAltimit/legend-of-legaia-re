//! Per-frame input the page hands the engine: pad word, left stick, camera
//! azimuth, precise movement and the player's stage row.

use super::*;

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Route this frame's pad word into the engine. Bit layout is the PSX digital
    /// pad ([`legaia_engine_system::input::PadButton`]): `0x0008` Start, `0x0010`
    /// Up, `0x0020` Right, `0x0040` Down, `0x0080` Left, `0x1000` Triangle,
    /// `0x2000` Circle, `0x4000` Cross, `0x8000` Square. Edge detection is the
    /// engine's - just hand it the held set each frame.
    pub fn set_pad(&mut self, mask: u16) {
        match self.scene_host.host_mut() {
            Some(h) => h.world.set_pad(mask),
            None => self.world.set_pad(mask),
        }
    }

    /// Opt in / out of the engine's continuous locomotion decode
    /// ([`legaia_engine_core::world::FieldLocomotion::precise_movement`]): the camera
    /// azimuth rotates the movement vector at full angular resolution and the
    /// left analog stick ([`Self::set_left_stick`]) supplies an arbitrary
    /// screen angle. The play page's VR first-person mode drives this so
    /// "stick forward" walks exactly where the headset looks; the keyboard
    /// path keeps the retail quantised 8-way remap.
    pub fn set_precise_movement(&mut self, on: bool) {
        match self.scene_host.host_mut() {
            Some(h) => h.world.locomotion.precise_movement = on,
            None => self.world.locomotion.precise_movement = on,
        }
        // Persist it like the native window's `R` toggle does
        // (`legaia-options.toml`), so the page's checkbox survives a reload
        // and a trap-recovery rebuild re-applies it.
        if self.options_state.precise_movement != on {
            self.options_state.precise_movement = on;
            self.persist_and_apply_options();
        }
        // A live session override (VR first-person) still rules the world.
        if let Some(o) = self.precise_movement_override {
            match self.scene_host.host_mut() {
                Some(h) => h.world.locomotion.precise_movement = o,
                None => self.world.locomotion.precise_movement = o,
            }
        }
    }

    /// Lay a session-only precise-movement state over the persisted option,
    /// or lift it (`None`). The VR first-person drive needs free-angle
    /// locomotion for the stick while it is up; it used to get it through
    /// [`Self::set_precise_movement`], which persists, so leaving VR saved
    /// "off" over whatever the player had chosen. The override is re-applied
    /// by every options apply and never reaches the store.
    pub fn set_precise_movement_override(&mut self, on: Option<bool>) {
        self.precise_movement_override = on;
        self.apply_options_side_effects();
    }

    /// Whether precise (free-angle) movement is on - the persisted option
    /// the page's checkbox reflects on load.
    pub fn precise_movement(&self) -> bool {
        self.options_state.precise_movement
    }

    /// The lead's projected screen Y this frame in 240-line stage space, or
    /// [`NO_FIELD_PROJECTION`] for "not projectable" - the browser twin of
    /// the native window's `field_hud_projected_player_y`. The field party
    /// HUD's decision kernel reads it (retail compares the projected player
    /// against a band before the readout returns).
    ///
    /// The sentinel is an out-of-band value rather than "negative": a lead
    /// projected ABOVE the top of the stage has a negative stage Y, and the
    /// native window reports it as a number. Folding that into "no
    /// projection" narrowed the channel on this host only, which is exactly
    /// the shape `docs/tooling/host-drift.md` names.
    pub fn set_field_player_screen_y(&mut self, stage_y: i32) {
        self.field_hud_projection_reported = true;
        self.field_hud_projected_y = (stage_y != NO_FIELD_PROJECTION)
            .then(|| stage_y.clamp(i16::MIN as i32, i16::MAX as i32) as i16);
    }

    /// Read back the value [`Self::set_field_player_screen_y`] holds, or
    /// [`NO_FIELD_PROJECTION`] for "not projectable" - for the page's
    /// diagnostics and the parity ladder, which needs the number the
    /// decision kernel is about to compare rather than the draw it produces.
    pub fn field_player_screen_y(&self) -> i32 {
        self.field_hud_projected_y
            .map_or(NO_FIELD_PROJECTION, i32::from)
    }

    /// Route this frame's left analog stick into the engine. PSX convention:
    /// signed bytes, X right-positive, Y **down**-positive; only read by the
    /// precise-locomotion decode ([`Self::set_precise_movement`]).
    pub fn set_left_stick(&mut self, x: i8, y: i8) {
        match self.scene_host.host_mut() {
            Some(h) => h.world.input.set_lstick((x, y)),
            None => self.world.input.set_lstick((x, y)),
        }
    }

    /// Override where the engine thinks the camera is looking for the next
    /// tick, so the free-movement controller remaps the d-pad
    /// camera-relative ("up" walks away from the camera). PSX 12-bit angle
    /// units (`4096` = a full turn); the field controller quantises it to the
    /// nearest quarter-turn, as retail does.
    ///
    /// An **override**, not the normal path: the engine camera publishes its
    /// own compass azimuth every tick
    /// ([`legaia_engine_field::camera::Camera::compass_azimuth_units`], which
    /// sums the scripted yaw, the drag-orbit and the host framing bias), and
    /// the native window has nothing that needs to speak over it. The page's
    /// VR first-person mode does: there the headset gaze *is* the heading, so
    /// it sets the azimuth outright for that tick.
    pub fn set_camera_azimuth(&mut self, units: u16) {
        self.camera_azimuth_override = Some(units % 4096);
        if let Some(h) = self.scene_host.host_mut() {
            h.world.locomotion.camera_azimuth = units % 4096;
        }
    }
}

//! `Camera` resets: scene entry and the return to free roam.

use super::*;

impl Camera {
    /// Reset the retail camera globals to their field-entry values and drop
    /// any glide in flight - the engine side of `FUN_80025C24`, called when a
    /// scene is entered so a previous scene's shot can't leak into the next.
    ///
    /// PORT: FUN_80025C24
    pub fn reset_globals_for_scene_entry(&mut self) {
        // Six stores in retail: the three angles and the eye trio. The focus
        // trio is re-pinned to the player by the follow camera on the next
        // frame and `H` keeps the register's live value, so a scene whose
        // entry beat glides slot `9` starts that glide from the `H` the
        // player was just looking through, as retail does.
        for axis in RetailCamGlobals::FIELD_RESET_AXES {
            self.globals.0[axis] = RetailCamGlobals::FIELD_RESET.0[axis];
        }
        if self.globals.0[9] == 0 {
            self.globals.0[9] = RetailCamGlobals::FIELD_RESET.0[9];
        }
        self.mover = None;
        self.script_owns_focus = false;
        // The field draw context (`FUN_801DE37C`) re-stamps the visible-tile
        // window every field entry runs through, *before* the new scene's
        // script can retune it with op `0x46`. Without the re-stamp the
        // window was seeded once at `Camera::new` and then only ever
        // overwritten, so a scene that scripts a wide window handed it to
        // the next scene and widened that scene's focus edge clamp - the
        // primer `route_camera_events` already names in its op-`0x46` arm.
        // Both hosts reach this through their scene-entry reset.
        self.zone.view_window = {
            let (x0, z0, x1, z1) =
                legaia_engine_system::mode_entry_init::field_draw_context().view_window;
            [x0, z0, x1, z1]
        };
        // The shake jitter pair lives in the scene control block (`+0x18` /
        // `+0x1C`), which the scene reset `FUN_8003A024` zeroes. The eye trio
        // was just re-seeded, so a pair left over from the departing scene
        // must not be subtracted back out of it on the next ease.
        self.zone.shake_offset = [0, 0];
        // The arrival actor (`FUN_801DBE9C`) re-pins the focus and snaps the
        // composed shot in on the first frame of the new scene.
        self.zone.arm_arrival();
    }

    /// The whole camera-side reset a **direct** scene entry owes - a scene
    /// picker, a dev warp, a load from a save - as opposed to the in-world
    /// transition a `SceneEntered` tick event reports, which every host
    /// already answers with [`Self::reset_globals_for_scene_entry`].
    ///
    /// `SceneHost::enter_field_scene` clears the *world's* camera state (the
    /// timeline and the op-`0x45` param set), so the next frame resolves to
    /// the follow arm; but the follow view composes from THIS camera's
    /// globals, and without this call an interrupted cutscene left its pitch
    /// / yaw / eye trio in them and its `script_owns_focus` latch set, so
    /// the new scene was framed by the old scene's shot and never re-pinned
    /// to the player. The cinematic mode and angles go too. The user's own
    /// orbit / tilt / zoom are intent, not scene state, and stay.
    pub fn reset_for_scene_entry(&mut self) {
        self.reset_globals_for_scene_entry();
        self.mode = CameraMode::Follow;
        self.yaw = 0.0;
        self.pitch = 0.0;
        self.roll = 0.0;
    }
}

impl Camera {
    /// Snap the camera back to the follow default when the field is in
    /// free-roam (a plain [`SceneMode::Field`] with no cutscene timeline owning
    /// the scene).
    ///
    /// An opening / scripted cutscene folds op-`0x45` Camera Configure yaw into
    /// [`Self::yaw`] and flips [`Self::mode`] to [`CameraMode::Cinematic`] (see
    /// [`Self::route_camera_events`]). That stale cinematic yaw must not leak
    /// into free-roam: a renderer frames free-roam field with a fixed follow
    /// camera, and hosts feed [`Self::yaw`] into
    /// `legaia_engine_core::world::FieldLocomotion::camera_azimuth`
    /// to remap the d-pad camera-relative - so a non-zero leaked yaw rotates
    /// the controls off the on-screen camera (the New Game prologue → Rim Elm
    /// hand-off left the d-pad ~180deg inverted). Retail returns control on the
    /// follow camera; this restores it.
    ///
    /// Gated on `!cutscene_timeline_active()` (the same gate that unlocks
    /// `legaia_engine_core::world::World::step_field_locomotion`),
    /// so an active cutscene's own beats keep their configured yaw. No-op
    /// outside free-roam field (world map / battle / menu / cutscene).
    pub fn reset_for_free_roam<W: CameraWorld + ?Sized>(&mut self, world: &W) {
        if matches!(
            world.scene_mode(),
            legaia_engine_mode::mode::SceneMode::Field
        ) && !world.cutscene_timeline_active()
        {
            self.mode = CameraMode::Follow;
            self.yaw = 0.0;
            self.pitch = 0.0;
            self.roll = 0.0;
        }
    }
}

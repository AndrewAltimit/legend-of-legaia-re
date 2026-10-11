//! `FieldCameraMotionHost` for [`FieldHostImpl`].

use super::*;

impl<'a> FieldCameraMotionHost for FieldHostImpl<'a> {
    // Op `0x4C` outer-nibble-4 sub-9 - the writer of the two globals
    // `legaia_engine_field::camera_ease` eases between, read off the three arms at
    // `0x801E1480..0x801E162C` in `overlay_world_map_801de840.txt`:
    //
    // | `_DAT_1F800394` | scene ctrl `+0x4A`     | `_DAT_8007BCAC`      |
    // |---|---|---|
    // | bit 25 (delta)  | `target`               | `target - player[+0x16]` |
    // | bit 24 (rel)    | `target + player[+0x16]` | `target`           |
    // | neither         | `target`               | untouched            |
    //
    // The first two arms both write **both** globals, and both land the
    // accumulator on the same value the per-frame easing would have walked
    // to (`ctrl[+0x4A] - player[+0x16]`); they are snaps, not a different
    // destination. The bit-24 arm has to post the accumulator itself because
    // that same bit is `FUN_801DA390`'s input lock (`0x801DA398`), so while
    // it is raised the easing returns before its first store.
    // REF: FUN_801DA390 (the easing), FUN_801D6704 (seeds the accumulator)
    fn op4c_n4_sub9_default_write(&mut self, target: i16) {
        self.world.camera.scene_offset = target;
    }

    fn op4c_n4_sub9_default_ramp(&mut self, target: i16, ticks: u16) {
        // Retail schedules a ramp over `ticks` frames through the register
        // ramp helper; the engine has one ramp mechanism and this is not it,
        // so the endpoint is posted immediately and the per-frame easing
        // supplies the approach. `ease_step` caps the move at 12 units a
        // frame either way, so the visible difference is the shape of the
        // last few frames, not the destination.
        let _ = ticks;
        self.world.camera.scene_offset = target;
    }

    fn op4c_n4_sub9_delta_write_or_ramp(&mut self, target: i16, ticks: u16) {
        let _ = ticks;
        self.world.camera.scene_offset = target;
        let footing = self.world.camera_ease_player_footing();
        self.world.camera.offset_ease = i32::from(target.wrapping_sub(footing));
    }

    fn op4c_n4_sub9_player_relative_write(&mut self, target: i16, ticks: u16) {
        let _ = ticks;
        let footing = self.world.camera_ease_player_footing();
        self.world.camera.scene_offset = target.wrapping_add(footing);
        self.world.camera.offset_ease = i32::from(target);
    }

    fn op4c_n_c_sub_d_camera_mover_live(&mut self) -> bool {
        self.world.camera.state.glide_frames > 0
    }

    /// `[4C, 0x84, amplitude]` - the screen-shake amplitude `_DAT_8007B630`.
    ///
    /// This opcode is the global's only retail writer and the global is the
    /// only input to `FUN_801D9D30`, so the field script is the sole source
    /// of a camera shake. The world holds it for the camera to read; see
    /// [`crate::world::CameraRig::shake_amplitude`].
    fn op4c_n8_sub4_set_b630(&mut self, value: u8) {
        self.world.camera.shake_amplitude = value;
    }
}

//! Field camera rig: the saved camera snapshot, scene offset + ease, shake amplitude and the zone-ramp register file.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

use super::*;

/// Field camera rig: the saved camera snapshot, scene offset + ease, shake amplitude and the zone-ramp register file.
pub struct CameraRig {
    /// Screen-shake amplitude - retail `_DAT_8007B630`.
    ///
    /// Written by exactly one thing in retail: the field-VM opcode
    /// `0x4C` outer-nibble `8` sub-`4` (`[4C, 0x84, amplitude]`), ported at
    /// [`legaia_engine_vm::field::FieldHost::op4c_n8_sub4_set_b630`]. It is
    /// the only input to the LCG camera jitter `FUN_801D9D30`
    /// ([`legaia_engine_vm::battle_camera::apply_shake`]): `0` is the resting
    /// state and `1..=0x15` widens the jitter window.
    ///
    /// REF: FUN_801D9D30
    pub shake_amplitude: u8,
    /// Scene control block `+0x4A` (`_DAT_801C6EA4 + 0x4A`) - the camera
    /// vertical offset the current scene asks for, in the player actor's
    /// `+0x16` footing units. Written by the field VM's op `0x4C`
    /// outer-nibble-4 sub-9 (all three arms) and read once a frame by
    /// [`crate::world::World::tick_camera_offset_ease`].
    pub scene_offset: i16,
    /// `_DAT_8007BCAC` - the smoothed camera vertical offset
    /// [`crate::camera_ease::ease_camera_offset`] walks toward
    /// `camera_scene_offset - player_footing`. Seeded to `0x3C` by retail's
    /// per-scene initialiser `FUN_801D6704`, which is why the engine seeds it
    /// there too. The op `0x4C` n4 sub-9 **delta** arm snaps it instead of
    /// letting the easing arrive.
    pub offset_ease: i32,
    /// Previous tick's player `(world_y, world_z)`. Stands in for the
    /// `+0x1E`/`+0x20` slots retail's settle test compares `+0x16`/`+0x18`
    /// against; the question the comparison asks is whether the actor has
    /// stopped moving in Y and Z, and this answers it without asserting what
    /// retail keeps in those two slots. `None` until the first tick.
    pub ease_prev_yz: Option<(i16, i16)>,
    /// Last camera state snapshot - filled by `camera_save`, applied by
    /// `camera_apply` / `camera_load`. Engines that draw a camera read
    /// this between frames.
    pub state: CameraState,
    /// Live camera-register zone-ramp records spawned by the field-VM op
    /// `0x43` sub-3..6 (retail `FUN_8003C6A4` actors on the effect list).
    /// [`crate::world::World::tick_register_ramps`] runs each one's `FUN_80037018` handler
    /// against the player's position every field frame. See
    /// [`crate::register_ramp`].
    pub register_ramps: Vec<crate::register_ramp::RegisterRamp>,
    /// The four field camera-configuration registers
    /// (`0x8007B60C`/`B610`/`B614`/`B618`) the ramps above write. Seeded to
    /// [`crate::camera::CAMERA_ZONE_DEFAULTS`] on scene entry; consumed by
    /// [`crate::camera::Camera::tick`].
    pub registers: crate::register_ramp::CameraRegisterFile,
}

impl CameraRig {
    pub fn new() -> Self {
        Self {
            shake_amplitude: 0,
            scene_offset: 0,
            offset_ease: crate::camera_ease::CAMERA_OFFSET_EASE_SEED,
            ease_prev_yz: None,
            state: CameraState::default(),
            register_ramps: Vec::new(),
            registers: Default::default(),
        }
    }
}

impl Default for CameraRig {
    fn default() -> Self {
        Self::new()
    }
}

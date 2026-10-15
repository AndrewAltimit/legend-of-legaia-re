//! Field camera rig: the saved camera snapshot, scene offset + ease, shake amplitude and the zone-ramp register file.
//!
//! One subsystem's state, held as a unit on the composite `World`.
//! Fields keep their retail provenance notes.

use legaia_engine_vm::field::CameraParam;

/// Field camera rig: the saved camera snapshot, scene offset + ease, shake amplitude and the zone-ramp register file.
pub struct CameraRig {
    /// Screen-shake amplitude - retail `_DAT_8007B630`.
    ///
    /// Raised by exactly one thing in retail: the field-VM opcode
    /// `0x4C` outer-nibble `8` sub-`4` (`[4C, 0x84, amplitude]`), ported at
    /// [`legaia_engine_vm::field::FieldCameraMotionHost::set_shake_amplitude`]. The
    /// scene reset `FUN_8003A024` zeroes it on every scene load
    /// (`0x8003A07C`, ported in
    /// `World::reset_scene_control_block`), so a tremor a
    /// script leaves running ends at the door. Two
    /// routines read it: the LCG camera jitter `FUN_801D9D30` and the camera
    /// follow-ease `FUN_801DB510` (`lw a0,-0x49d0(v0)` at `0x801DB850`), whose
    /// tail (`0x801DB864..0x801DB8D4`) draws the same two masked `rand()`
    /// samples with the same centring and adds them straight onto the eye
    /// `0x800840B8` / `0x800840BC`, so one port
    /// ([`legaia_engine_camera::battle_camera::apply_shake`]) covers both. In
    /// both, `0` is the resting state and `1..=0x15` widens the jitter
    /// window.
    ///
    /// REF: FUN_801D9D30, FUN_801DB510
    pub shake_amplitude: u8,
    /// Scene control block `+0x4A` (`_DAT_801C6EA4 + 0x4A`) - the camera
    /// vertical offset the current scene asks for, in the player actor's
    /// `+0x16` footing units. Written by the field VM's op `0x4C`
    /// outer-nibble-4 sub-9 (all three arms) and read once a frame by
    /// `World::tick_camera_offset_ease`.
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
    /// `World::tick_register_ramps` runs each one's `FUN_80037018` handler
    /// against the player's position every field frame. See
    /// [`crate::register_ramp`].
    pub register_ramps: Vec<crate::register_ramp::RegisterRamp>,
    /// The four field camera-configuration registers
    /// (`0x8007B60C`/`B610`/`B614`/`B618`) the ramps above write. Seeded to
    /// [`crate::camera::CAMERA_ZONE_DEFAULTS`] on scene entry; consumed by
    /// [`crate::camera::Camera::tick`].
    pub registers: crate::register_ramp::CameraRegisterFile,
    /// Camera-zone arms the field VM ran this frame
    /// ([`crate::camera::CameraZoneRequest`]), drained by
    /// [`crate::camera::Camera::tick`]. The VM's host is `World` while the
    /// camera globals live on the host-owned `Camera`, so the arms queue
    /// here instead of writing the globals directly - see
    /// `camera_hooks`.
    pub zone_requests: Vec<crate::camera::CameraZoneRequest>,
    /// The script camera-focus override `_DAT_8007B628` (X) /
    /// `_DAT_8007B62A` (Z), written only by op `4C CF` (arm `0x801E2A34`) and
    /// read only by the focus edge clamp `FUN_801DAA50`, which replaces the
    /// clamped focus axis with the negated override when it is non-zero. A
    /// zero axis means "no override". Nothing else writes the pair - no scene
    /// reset clears it - so it lives as long as the script leaves it; `uru`,
    /// the one scene that sets it, clears both in its entry record.
    pub script_focus: [i16; 2],
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
            zone_requests: Vec::new(),
            script_focus: [0, 0],
        }
    }
}

impl Default for CameraRig {
    fn default() -> Self {
        Self::new()
    }
}

/// Camera state populated by `camera_save` / `camera_load` and read by
/// `camera_apply`. Engines render the configured camera each frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CameraState {
    /// Last `CameraConfigure` params applied (param-mask + values).
    pub params: Vec<CameraParam>,
    /// Last apply-trigger value.
    pub apply_trigger: u16,
    /// Last apply-mode nibble.
    pub mode: u8,
    /// Snapshot bytes from `camera_save`.
    pub saved: Vec<u8>,
    /// Last `camera_load` payload.
    pub loaded_payload: Vec<u8>,
    /// Display frames left on the cutscene camera mover's glide
    /// (`FUN_801DC0BC`): armed by an op-`0x45` beat with a non-zero apply
    /// trigger, zeroed by a snap, counted down once per display frame. The
    /// script-side view of the mover, which op `4C CD` waits on - the
    /// renderer's own glide lives on `crate::camera::Camera`, which folds
    /// the beat only after the slice that issued it, while retail allocates
    /// the mover inside the op and a `4C CD` later in the same slice already
    /// sees it.
    pub glide_frames: i32,
}

//! The camera's seam onto the world: `CameraWorld`, the zone request and cull view it exchanges, and the camera flag bits.

use super::*;

/// One queued camera-zone arm, drained by [`Camera::tick`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraZoneRequest {
    /// `[4C 38]` - query + load the camera parameter block from the record
    /// covering the player's tile. The ease walks the globals to the new
    /// block's composed pose.
    QueryAtPlayer,
    /// `[4C C4 x z]` - the same query at a tile the script names.
    QueryAtTile { x: u8, z: u8 },
    /// `[4C 39]` - query at the player's tile, re-conform the player's
    /// footing to the floor there, then snap the camera and clamp the focus.
    QueryConformAndSnap,
    /// `[4C 3E]` - snap the camera from the resident block and clamp the
    /// focus. No query.
    SnapAndClamp,
    /// `[4C 3D]` - re-latch the walk-region attribute box at the player's
    /// tile (`FUN_800180EC`). Camera-adjacent because the box is what the
    /// composer's position sweeps span.
    RefreshAttributes,
}

/// The camera state the cull reads, as the camera last left it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldCullView {
    /// `_DAT_80089118` / `_DAT_80089120`: the focus X / Z, **negated** as
    /// retail stores them.
    pub focus_stored: [i32; 2],
    /// The walk-region attribute box `0x1F800384..87`, `[x_lo, z_lo, x_hi,
    /// z_hi]` in world tiles.
    pub attr_box: [u8; 4],
    /// The visible tile window `0x1F8003E8..EB`, signed, focus-relative.
    pub window: [i8; 4],
}

/// Bit 10 of the scratchpad word `0x1F800394` (`World::flags.story_flags`):
/// the follow ease `FUN_801DB510` takes its pin leg (`0x801DB564`), the
/// settle skips its clip bind (`0x801D1E30`) and hop, and the player tick
/// skips the pad step (`0x801D16A8`). Retail's only setter is the world-map
/// top-view debug toggle (`0x801E7748`, gated on the debug word), and every
/// mode entry clears it.
pub const CAMERA_HOLD_FLAG: u32 = 0x0000_0400;

/// Bit 16 of the scratchpad word `0x1F800394`: the player tick `FUN_801D1344`
/// runs the follow ease even while the player is movement-locked. The tick
/// calls the ease (`jal 0x801DB510` at `0x801D1834`) only when its local
/// gate is up, and it raises the gate on this bit (`0x801D1634..0x801D1648`)
/// or on a player with `+0x10 & 0x80000` clear and this word's `0x400` clear
/// (`0x801D1694..0x801D16C0`); otherwise the frame runs only the shake
/// (`FUN_801D9D30`, `0x801D184C`) and the camera holds.
pub const CAMERA_LOCKED_EASE_FLAG: u32 = 0x0001_0000;

/// Bit 18 of the scratchpad word `0x1F800394`: the follow ease composes and
/// eases even on a frame the player did not move (`0x801DB578..0x801DB5A4`).
/// Field-VM `2E 12` sets it and `2F 12` clears it (`conc`, `opurud`,
/// `rikuroa`, `urudre2`, `bubu1` and their twins).
pub const CAMERA_FORCE_EASE_FLAG: u32 = 0x0004_0000;

/// The slice of the game world the camera reads and writes: the scene mode,
/// the player's position and footing, the walk-region / camera-zone /
/// collision blocks, the scratchpad story word, the camera register file and
/// shake, the field event queue and the zone requests the field VM queues,
/// the cull view and visible-tile window it publishes, and the `rand()`
/// stream its shake draws from. engine-core's `World` implements it; nothing
/// else in this module touches the world.
pub trait CameraWorld {
    /// The world's current scene mode.
    fn scene_mode(&self) -> legaia_engine_mode::mode::SceneMode;
    /// The player actor's world position, `None` without a player actor.
    fn player_world_pos(&self) -> Option<(i32, i32, i32)>;
    /// Actor `slot`'s world position while it is active, else `None`.
    fn active_actor_pos(&self, slot: usize) -> Option<(i32, i32, i32)>;
    /// Ground height under `(world_x, world_z)` off the static floor.
    fn sample_field_floor_height_static(&self, world_x: i32, world_z: i32) -> i32;
    /// The scene's walk-region attribute block.
    fn map_region_block(&self) -> &[u8];
    /// The scene's camera-zone record table.
    fn zone_table(&self) -> &[u8];
    /// The scene's walkability grid.
    fn collision_grid(&self) -> &[u8];
    /// The scratchpad story word `0x1F800394`.
    fn story_flags(&self) -> u32;
    /// The camera register file the field VM's camera ops write.
    fn camera_registers(&self) -> &crate::register_ramp::CameraRegisterFile;
    /// The camera shake amplitude.
    fn camera_shake_amplitude(&self) -> u8;
    /// The script camera-focus override `[X, Z]` op `4C CF` stages
    /// (`_DAT_8007B628` / `_DAT_8007B62A`); a zero axis is unset.
    fn script_focus_override(&self) -> [i16; 2];
    /// Whether the scene allows saving - the composer's half-eye term.
    fn scene_save_allowed(&self) -> bool;
    /// Displayed frames so far.
    fn display_frames(&self) -> u64;
    /// Whether a cutscene timeline owns the frame.
    fn cutscene_timeline_active(&self) -> bool;
    /// Whether a scripted arc drives the follow camera.
    fn script_arc_follow_camera(&self) -> bool;
    /// Whether the per-frame camera-zone re-query flag is raised.
    fn camera_zone_requery_per_frame(&self) -> bool;
    /// Whether the player's movement is locked.
    fn field_player_movement_locked(&self) -> bool;
    /// Whether a ledge hop is in flight.
    fn ledge_hop_active(&self) -> bool;
    /// Whether the player's scripted arc is live.
    fn player_script_arc_live(&self) -> bool;
    /// Take the camera-zone arms the field VM queued this frame.
    fn take_camera_zone_requests(&mut self) -> Vec<CameraZoneRequest>;
    /// Drain the field event queue.
    fn drain_field_events(&mut self) -> Vec<FieldEvent>;
    /// Hand the events the camera did not consume back to the queue.
    fn requeue_field_events(&mut self, events: Vec<FieldEvent>);
    /// Publish the field actor cull's camera view.
    fn set_npc_cull_view(&mut self, view: Option<FieldCullView>);
    /// Publish the live visible-tile window to the fog pool.
    fn set_fog_view_window(&mut self, window: [i8; 4]);
    /// The world's `rand()` stream state.
    fn rng_state(&self) -> u32;
    /// Write back the `rand()` stream state.
    fn set_rng_state(&mut self, state: u32);
}

//! Camera-zone arrival: the per-zone defaults and the arrival countdown tick.

/// The default camera-zone parameter set the tile re-query `FUN_801DE3E0`
/// installs when no camera-region record covers the player's tile (the
/// same nine stores sit in `FUN_801DBE9C`'s dev-only query leg). Raw values
/// and the `0x8007B607..` globals they land in; the typed form is
/// [`crate::camera_zone::CameraZoneConfig::ZONE_MISS`].
///
/// | field | global | value |
/// |---|---|---|
/// | `mode` | `DAT_8007B607` | `0x10` |
/// | `param_b608` | `DAT_8007B608` | `0x10` |
/// | `param_b609` | `DAT_8007B609` | `0x30` |
/// | `param_b60a` | `DAT_8007B60A` | `0x51` |
/// | `param_b60b` | `DAT_8007B60B` | `0x20` |
/// | `angle` | `DAT_8007B60C` | `0x1B8` |
/// | `b610` | `DAT_8007B610` | `0` |
/// | `b614` | `DAT_8007B614` | `0x4000` |
/// | `b618` | `DAT_8007B618` | `0x300` |
///
/// (`0x1B8` is the same default pitch `FUN_80025C24` seeds at scene entry;
/// see [`Camera::reset_globals_for_scene_entry`].)
// REF: FUN_801DBE9C (miss-path stores at 0x801dbf18..0x801dbf74)
pub const CAMERA_ZONE_DEFAULTS: [(u32, u32); 9] = [
    (0x8007B607, 0x10),
    (0x8007B608, 0x10),
    (0x8007B609, 0x30),
    (0x8007B60A, 0x51),
    (0x8007B60B, 0x20),
    (0x8007B60C, 0x1B8),
    (0x8007B610, 0),
    (0x8007B614, 0x4000),
    (0x8007B618, 0x300),
];

/// What one [`camera_zone_arrival_tick`] decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraZoneArrival {
    /// Countdown still running - nothing else happened, actor not flagged.
    Waiting,
    /// Countdown expired: the player tile was queried and a camera-region
    /// record hit - load it (`FUN_801DBC20`), then run the follow update.
    LoadZoneConfig,
    /// Countdown expired with no covering record: install
    /// [`CAMERA_ZONE_DEFAULTS`], then run the follow update.
    LoadDefaults,
}

/// The camera-zone **arrival tick** - one frame of `FUN_801DBE9C`'s
/// query arm (the `_DAT_8007B868 != 0` leg). `_DAT_8007B868` is the
/// dev/dual-mode gate and retail boots with it `0`, so **retail never runs
/// this arm**: its `== 0` leg counts the same `+0x54` countdown down, then
/// re-pins the focus to the player and snaps the composed shot in through
/// `FUN_801DB8EC` without touching the parameter block. The retail-side
/// zone query is the script-driven `FUN_801DE3E0` ([`crate::camera_zone`]).
///
/// PORT: FUN_801dbe9c
///
/// Decrements the actor's `+0x54` countdown; while it has not reached
/// `-1` the tick returns [`CameraZoneArrival::Waiting`] (and retail
/// neither flags the actor nor touches the camera). On expiry the player
/// tile quantises as `(pos - 0x40) >> 7` (the region-refresh form, NOT
/// the walk-on dispatch's raw `>> 7`) and the zone-record query
/// (`FUN_801DBA20` = [`crate::field_regions::zone_query`]) picks between
/// the record load and [`CAMERA_ZONE_DEFAULTS`]. Either way the follow
/// update (`FUN_801DB8EC` + the negated-focus store, see
/// [`Camera::tick`]) runs and the actor's `+0x10` flags gain bit `8`.
///
/// Provenance: `overlay_0897_locomotion_cluster.txt` at `0x801dbe9c..
/// 0x801dc0b8` (the committed `FUN_801DBEC4` name is a mid-function
/// label of this body, not its entry).
///
/// REPLACED-BY: [`ZoneFollow::arm_arrival`] and the zone-follow tick that
/// consumes it, which do the retail leg's job - re-pin the focus to the
/// player and snap the composed shot in - and run the tile query from the
/// same tick through [`crate::field_regions::zone_query`].
///
/// This is the stronger claim, not "nothing calls it". The body ported here
/// is the leg the entry gate selects when `_DAT_8007B868` is **non-zero**
/// (`lw v0,-0x4798(v0)` / `beq v0,zero,0x801dbff8` at `0x801dbe9c`), and that
/// word is the dev / dual-mode gate retail boots clear - the same gate that
/// closes the world-map dev menu's MAP_CHANGE row and selects the `h:\`
/// host-file asset arm. Retail therefore always takes the `0x801dbff8` leg,
/// which never touches the camera parameter block. A call site here would
/// host a leg the retail machine does not run, beside the leg the engine
/// already runs.
pub fn camera_zone_arrival_tick(
    countdown: &mut i16,
    player_pos: (i16, i16),
    zone_hit: impl FnOnce(i32, i32) -> bool,
) -> CameraZoneArrival {
    *countdown = countdown.wrapping_sub(1);
    if *countdown != -1 {
        return CameraZoneArrival::Waiting;
    }
    let tile_x = i32::from(player_pos.0 - 0x40) >> 7;
    let tile_z = i32::from(player_pos.1 - 0x40) >> 7;
    if zone_hit(tile_x, tile_z) {
        CameraZoneArrival::LoadZoneConfig
    } else {
        CameraZoneArrival::LoadDefaults
    }
}

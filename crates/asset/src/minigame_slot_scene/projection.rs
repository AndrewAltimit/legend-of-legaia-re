//! The machine's fixed GTE projection, restated as float helpers a host can
//! feed a model-space point through.

// ---------------------------------------------------------------------------
// Projection
// ---------------------------------------------------------------------------

/// Width of the retail framebuffer the machine draws into: `FUN_801CEC94` sets
/// video mode `0x280` = 640 wide. Horizontal pixels are half-width, which is why
/// [`PROJ_ASPECT`] is 2.
pub const SCREEN_W: f32 = 640.0;
/// Height of the retail framebuffer.
pub const SCREEN_H: f32 = 240.0;

/// The GTE's screen offset `OFX` while the machine draws, in pixels (the
/// control word reads `0x01400000`, 16.16). **Captured**: the
/// `minigame_slot_machine` mednafen state carries the COP2 register file in
/// its `GTE` section, and every value below is read from it.
pub const GTE_OFX: f32 = 320.0;
/// The GTE's screen offset `OFY` (`0x00720000`).
pub const GTE_OFY: f32 = 114.0;
/// The GTE's projection-plane distance `H`.
pub const GTE_H: f32 = 1024.0;
/// The rotation matrix's diagonal, `diag(0x6000, 0x3000, 0x3000)` in 4.12 -
/// the scale the init writes to `_DAT_8007bf10`. The 2:1 x:y ratio is the
/// 640-wide hi-res mode's pixel aspect.
pub const GTE_SCALE: [i32; 3] = [6, 3, 3];
/// The translation vector `TR` - `(-1440, 20, 24480)`.
pub const GTE_TR: [i32; 3] = [-1440, 20, 24480];

/// The GTE projection rearranged into the form an exporter that has no GTE
/// rebuilds (`asset slot-art`'s manifest, the VRChat kit, the minigames page):
///
/// ```text
/// k(z)     = PROJ_Z0 / (PROJ_Z0 + z)
/// screen_x = PROJ_OFX + PROJ_SX0 * k(z) * (x - PROJ_VANISH.0)
/// screen_y = PROJ_OFY + PROJ_SX0 / PROJ_ASPECT * k(z) * (y - PROJ_VANISH.1)
/// ```
///
/// which is [`project`] exactly: `vz = Sz * (z + TRz / Sz)`, so the depth
/// offset is `TRz / Sz`, the scale at `z = 0` is `H * Sx / TRz`, and the
/// translation's x / y halves move the vanishing point off the model origin to
/// `(-TRx / Sx, -TRy / Sy)`. This is the view depth of model `z = 0`.
pub const PROJ_Z0: f32 = GTE_TR[2] as f32 / GTE_SCALE[2] as f32;
/// The screen x-scale at `z = 0` - see [`PROJ_Z0`].
pub const PROJ_SX0: f32 = GTE_H * GTE_SCALE[0] as f32 / GTE_TR[2] as f32;
/// The screen point the vanishing point lands on (`OFX`) - see [`PROJ_Z0`].
pub const PROJ_OFX: f32 = GTE_OFX;
/// The screen point the vanishing point lands on (`OFY`) - see [`PROJ_Z0`].
pub const PROJ_OFY: f32 = GTE_OFY;
/// The model-space `(x, y)` every depth scales about: `(240, -20/3)`, not the
/// model origin - see [`PROJ_Z0`].
pub const PROJ_VANISH: (f32, f32) = (
    -(GTE_TR[0] as f32) / GTE_SCALE[0] as f32,
    -(GTE_TR[1] as f32) / GTE_SCALE[1] as f32,
);
/// x:y scale ratio of [`GTE_SCALE`].
pub const PROJ_ASPECT: f32 = 2.0;
/// The camera matrix's x scale ([`GTE_SCALE`]`[0]`). A billboard's view-space
/// half-extent carries no model scale: `FUN_800195a8` builds the corners
/// *after* the matrix multiply.
pub const PROJ_X_SCALE: f32 = 6.0;

/// A model-space point in view space: `MVMVA` with the machine's camera
/// (`R * v >> 12 + TR`, `cop2 0x480012` in `FUN_8003D344`).
pub fn view_point(x: i32, y: i32, z: i32) -> (f32, f32, f32) {
    (
        (GTE_SCALE[0] * x + GTE_TR[0]) as f32,
        (GTE_SCALE[1] * y + GTE_TR[1]) as f32,
        (GTE_SCALE[2] * z + GTE_TR[2]) as f32,
    )
}

/// View depth of model-space depth `z` (`-z` is toward the viewer).
pub fn view_depth(z: i32) -> f32 {
    (GTE_SCALE[2] * z + GTE_TR[2]) as f32
}

/// Screen x-scale at model-space depth `z`.
pub fn view_scale(z: i32) -> f32 {
    GTE_SCALE[0] as f32 * GTE_H / view_depth(z)
}

/// Project a view-space point the way `RTPS` does: `OF + H * v / vz`.
pub fn project_view(vx: f32, vy: f32, vz: f32) -> (f32, f32) {
    (GTE_OFX + GTE_H * vx / vz, GTE_OFY + GTE_H * vy / vz)
}

/// Project a model-space point onto the retail 640x240 framebuffer through
/// the captured camera - the transform the cabinet's actor renderer, the
/// reel renderer (`RotTransPers4` under the camera matrix) and the billboard
/// centres (`MVMVA`, then `RTPS` under an identity matrix) all reduce to.
pub fn project(x: i32, y: i32, z: i32) -> (f32, f32) {
    let (vx, vy, vz) = view_point(x, y, z);
    project_view(vx, vy, vz)
}

/// Screen half-extent of a billboard whose view-space half-extent is `(hw, hh)`
/// and whose centre is at depth `z`. `FUN_800195a8` loads an identity matrix
/// with a zero translation (`FUN_8003D178`) before projecting the corners, so
/// both axes divide by the same `H / vz`: a billboard is *not*
/// aspect-corrected, and a 2:1 view-space extent renders square on the
/// half-width hi-res pixel grid.
pub fn billboard_half(hw: i32, hh: i32, z: i32) -> (f32, f32) {
    let k = GTE_H / view_depth(z);
    (hw as f32 * k, hh as f32 * k)
}

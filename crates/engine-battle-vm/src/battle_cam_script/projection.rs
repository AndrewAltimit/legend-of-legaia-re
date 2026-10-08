//! Projection: the retail GTE battle camera as one view-projection matrix.
//! Split out of `battle_cam_script.rs`.

use super::*;

// ---------------------------------------------------------------------------
// Projection: the retail GTE battle camera as one view-projection matrix.
// ---------------------------------------------------------------------------

/// GTE projection focal length the battle camera runs on (`_DAT_8007B6F4`).
pub const GTE_H: f32 = 256.0;

/// GTE screen-centre X (`OFX`), in whole pixels over the 320x240 frame.
pub const GTE_OFX: f32 = 160.0;

/// GTE screen-centre Y (`OFY`), in whole pixels over the 320x240 frame.
///
/// **Not** `240 / 2`. The draw-environment setup writes the pair once into
/// the GTE control file and never rewrites it, so the projected origin sits
/// six rows *above* the geometric centre of the frame for the whole game -
/// field, battle, battle load and minigame alike. `H` is the counter-example
/// that makes that a finding rather than a property of the measurement: it is
/// `256` in battle and `512` in the field, written per phase, while `OFX` /
/// `OFY` / `DQA` / `DQB` do not move. Read off the `GTE` section of nine
/// save states by `crates/mednafen/tests/gte_projection_real.rs`.
///
/// Both hosts fold the six-pixel bias into their projection matrix as a
/// constant `w`-scaled term on clip `y` ([`battle_vp`] here, `psx_camera_mvp`
/// in the native window), which is the matrix form of retail's
/// `screen.y = OFY + H * Ey / Ez`.
pub const GTE_OFY: f32 = 114.0;

/// The clip-space `y`-from-`w` term that reproduces [`GTE_OFY`] under the
/// `ndc.y = 1 - 2 * screen_y / 240` viewport map both hosts use: solving
/// `120 - 120 * bias = OFY` gives `(120 - OFY) / 120`, i.e. `+0.05`.
pub const GTE_OFY_NDC_BIAS: f32 = (120.0 - GTE_OFY) / 120.0;

/// Near plane both hosts project with (governs depth precision only - the
/// retail GTE has no near plane; 4 units is the engine's shared choice).
pub const PSX_NEAR: f32 = 4.0;

/// Far plane both hosts project with. Paired with the native renderer's
/// `legaia_engine_render::window::SCENE_FAR` (a hard wgpu link this crate
/// cannot take); the pairing is pinned by `scene_far_paired_with_renderer`
/// in the engine-shell camera tests.
pub const SCENE_FAR: f32 = 1_000_000.0;

/// Column-major 4x4 multiply: `out = a * b` (same layout as WebGL `mat4`
/// and `glam::Mat4::to_cols_array`). One implementation, in the shared camera
/// kernel.
pub use crate::psx_camera::mat4_mul as mat_mul;

/// The full battle view-projection for one [`BattleCamPose`], column-major -
/// the shared rendition of the native window's `psx_camera_mvp` composition
/// specialised to the battle camera (`battle_dome_camera_mvp`): retail GTE
/// `screen = H * (R*(v - focus) + TR) / Ze` with `R = Rx(pitch)*Ry(yaw)`
/// (12-bit angles, roll never staged by the battle phase script), `H = 256`,
/// PSX screen `+Y` down over the 320x240 frame, X corrected so the 4:3
/// retail framing holds at any viewport aspect.
///
/// `world_scale` is the retail 4x battle world scale (base matrix
/// `0x8007BF10 = 16384*I`) the host composes onto the ACTOR draws; the focus
/// trio targets those scaled actors, so it is pre-scaled here exactly as the
/// native camera pre-scales it. The trailing `scale(1,-1,1)` factor cancels
/// the per-model Y-flip every draw's model matrix carries, recovering the
/// raw PSX Y-down vertex the retail transform expects.
///
/// Depth maps `[near, far]` to `[0, 1]` (the native wgpu convention; well
/// inside WebGL's `[-1, 1]` clip range, so the same matrix serves both
/// hosts). Pinned against the native glam composition by
/// `battle_vp_matches_the_native_glam_composition` in the engine-shell
/// camera tests.
///
/// REF: FUN_800172C0, FUN_80026988 (the camera matrix build, ported as
/// `euler_rot_psx`), FUN_80026f50 (the projection), FUN_80048A08 (per-actor
/// world-scale composition).
pub fn battle_vp(pose: &BattleCamPose, world_scale: f32, aspect: f32) -> [f32; 16] {
    let to_rad = |units: f32| units / 4096.0 * std::f32::consts::TAU;
    // The focus targets the world-scaled actor stage (see the doc above), and
    // the battle phase script never stages a roll. Everything else - the
    // `Rx*Ry*Rz` build, the eye-space translation, the PSX projection with its
    // `GTE_OFY` bias, and the trailing `scale(1,-1,1)` that cancels the
    // per-model Y-flip - is the one shared retail camera kernel.
    crate::psx_camera::psx_camera_vp(
        to_rad(pose.pitch),
        to_rad(pose.yaw),
        0.0,
        GTE_H,
        pose.tr,
        [
            pose.focus[0] * world_scale,
            pose.focus[1] * world_scale,
            pose.focus[2] * world_scale,
        ],
        aspect,
    )
}

/// View-space depth of a raw battle-world point under `pose` - the `MAC3`
/// an `MVMVA` of the point leaves, which the battle tint pass
/// (`crate::battle_actor_tint`) reads back as the node's `+0x34`.
///
/// `raw_pos` is in raw retail battle units (Y down, the actor's `+0x14`
/// trio); both it and the focus are lifted by `world_scale` exactly as
/// [`battle_vp`] lifts them, and the depth is the third row of
/// `R * (s*p - s*focus) + tr` with `R = Rx(pitch) * Ry(yaw)`. Measured
/// against the stored `+0x34` of the battle bodies in the catalogued battle
/// states (camera trio `0x8007B790`, translation `0x800840B8`, focus
/// `0x80089118` negated, position `node[+0x14]`): 296 of 379 within two
/// units and 341 within 32; the rest read as states where the camera
/// globals or the body moved after the frame drew it.
pub fn battle_view_depth(pose: &BattleCamPose, world_scale: f32, raw_pos: [f32; 3]) -> f32 {
    let to_rad = |units: f32| units / 4096.0 * std::f32::consts::TAU;
    let (sa, ca) = to_rad(pose.pitch).sin_cos();
    let (sb, cb) = to_rad(pose.yaw).sin_cos();
    let row = [-ca * sb, sa, ca * cb];
    let v = [
        (raw_pos[0] - pose.focus[0]) * world_scale,
        (raw_pos[1] - pose.focus[1]) * world_scale,
        (raw_pos[2] - pose.focus[2]) * world_scale,
    ];
    row[0] * v[0] + row[1] * v[1] + row[2] * v[2] + pose.tr[2]
}

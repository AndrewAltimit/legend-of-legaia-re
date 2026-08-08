//! The overworld camera-relative movement remap
//! ([`world_map_camera_relative_bits`]) must agree with what the player sees:
//! pressing "up" walks the character *away from the camera* on screen, and
//! "right" walks *screen-right*, for any camera azimuth.
//!
//! This is the ground-truth check for the remap, and it projects through the
//! **walk-view** camera - the retail GTE composition the play window renders
//! overworld locomotion under (`window/event_handler/redraw_passes.rs`) - not
//! the top-view debug camera `world_map_camera_mvp`. Locomotion early-returns
//! in top view (`World::step_world_map_locomotion`), so the walk view is the
//! only frame the remap is ever seen through; a remap verified against the
//! debug camera can be (and once was) inverted in the frame that matters.
//!
//! The composition is rebuilt here from the same savestate-pinned constants
//! the play window uses (the bin target's camera isn't linkable from an
//! integration test): `screen = proj(H=368) * T(tr) * Rx(pitch) * Ry(az) *
//! Yflip * FIELD_WORLD_FLIP * S(6) * T(-player)`, with the sebucus zoom pin
//! (pitch 360 units, tr (0, 536, 9139)). The two Y negations cancel, which is
//! the point of the `FIELD_WORLD_FLIP` pairing - the whole thing runs on raw
//! retail Y-down world coordinates. If the remap's trig ever drifts from this
//! geometry, this fails. Pure math - runs in CI without disc data.

use glam::{Mat4, Vec3, Vec4};
use legaia_engine_core::world::world_map_camera_relative_bits;

/// World-space XZ unit step for a post-remap direction-bit set.
fn bits_to_world_dir(bits: u16) -> (f32, f32) {
    let mut x = 0.0;
    let mut z = 0.0;
    if bits & 0x1000 != 0 {
        z += 1.0; // Z+
    }
    if bits & 0x4000 != 0 {
        z -= 1.0; // Z-
    }
    if bits & 0x2000 != 0 {
        x += 1.0; // X+
    }
    if bits & 0x8000 != 0 {
        x -= 1.0; // X-
    }
    (x, z)
}

/// The walk-view camera MVP at `azimuth`, player at the origin. Mirrors the
/// play window's `psx_camera_mvp(pitch, az, 0, 368, tr, ZERO, aspect) *
/// FIELD_WORLD_FLIP * Scale(6) * Translate(-player)` with the sebucus pin.
fn walk_view_mvp(azimuth: i32, aspect: f32) -> Mat4 {
    let to_rad = |units: f32| units / 4096.0 * std::f32::consts::TAU;
    let pitch = to_rad(360.0);
    let yaw = to_rad(azimuth as f32);
    let tr = Vec3::new(0.0, 536.0, 9139.0);
    let h = 368.0f32;

    let r = Mat4::from_rotation_x(pitch) * Mat4::from_rotation_y(yaw);
    let t = Mat4::from_translation(tr);
    // psx_camera_mvp's internal pre-flip and FIELD_WORLD_FLIP - kept as the
    // explicit pair so the composition reads like the play window's.
    let f = Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
    let field_world_flip = Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));

    let (near, far) = (4.0f32, legaia_engine_render::window::SCENE_FAR);
    let a = far / (far - near);
    let b = -near * far / (far - near);
    let aspect_fix = (4.0 / 3.0) / aspect.max(0.01);
    let proj = Mat4::from_cols(
        Vec4::new(h / 160.0 * aspect_fix, 0.0, 0.0, 0.0),
        Vec4::new(0.0, -h / 120.0, 0.0, 0.0),
        Vec4::new(
            0.0,
            legaia_engine_vm::battle_cam_script::GTE_OFY_NDC_BIAS,
            a,
            1.0,
        ),
        Vec4::new(0.0, 0.0, b, 0.0),
    );
    proj * t * r * f * field_world_flip * Mat4::from_scale(Vec3::splat(6.0))
}

/// Project a world point to normalized device coords (NDC: +x right, +y up).
fn project(mvp: &Mat4, p: Vec3) -> (f32, f32) {
    let c: Vec4 = *mvp * p.extend(1.0);
    (c.x / c.w, c.y / c.w)
}

/// Screen-space delta (Δndc) when the player walks one big step in the world
/// direction the remap chose for screen input `(sx, sy)` at `azimuth`.
fn screen_delta(azimuth: i32, sx: i32, sy: i32) -> (f32, f32) {
    let mvp = walk_view_mvp(azimuth, 16.0 / 9.0);
    let bits = world_map_camera_relative_bits(azimuth, sx, sy);
    let (dx, dz) = bits_to_world_dir(bits);
    let step = 20.0; // a visible nudge in world units
    let p0 = project(&mvp, Vec3::ZERO);
    let p1 = project(&mvp, Vec3::new(dx * step, 0.0, dz * step));
    (p1.0 - p0.0, p1.1 - p0.1)
}

#[test]
fn screen_up_walks_away_from_camera_for_every_azimuth() {
    // Sample the full turn, including the diagonal framings.
    for az in (0..4096).step_by(128) {
        let (ddx, ddy) = screen_delta(az, 0, 1); // press Up
        assert!(
            ddy > 0.0,
            "az={az}: pressing Up must move the player UP on screen (Δndc_y={ddy}, Δndc_x={ddx})"
        );
        // And essentially straight up (no strong sideways drift) at cardinal
        // framings; allow drift at rotated framings where the move is diagonal.
        if az % 1024 == 0 {
            assert!(
                ddx.abs() < ddy.abs() * 0.25,
                "az={az}: Up at a cardinal framing should be ~vertical (Δndc_x={ddx}, Δndc_y={ddy})"
            );
        }
    }
}

#[test]
fn screen_right_walks_right_for_every_azimuth() {
    for az in (0..4096).step_by(128) {
        let (ddx, ddy) = screen_delta(az, 1, 0); // press Right
        assert!(
            ddx > 0.0,
            "az={az}: pressing Right must move the player RIGHT on screen (Δndc_x={ddx}, Δndc_y={ddy})"
        );
        if az % 1024 == 0 {
            assert!(
                ddy.abs() < ddx.abs() * 0.25,
                "az={az}: Right at a cardinal framing should be ~horizontal (Δndc_x={ddx}, Δndc_y={ddy})"
            );
        }
    }
}

/// At azimuth 0 the walk camera is retail's identity frame, so the remap must
/// be the identity onto the world axes: Up = `Z+`, Right = `X+` - the retail
/// compass ring (`FUN_800467E8`) with zero octant offset.
#[test]
fn azimuth_zero_is_the_retail_identity_mapping() {
    assert_eq!(world_map_camera_relative_bits(0, 0, 1), 0x1000, "Up -> Z+");
    assert_eq!(
        world_map_camera_relative_bits(0, 1, 0),
        0x2000,
        "Right -> X+"
    );
    assert_eq!(
        world_map_camera_relative_bits(0, 0, -1),
        0x4000,
        "Down -> Z-"
    );
    assert_eq!(
        world_map_camera_relative_bits(0, -1, 0),
        0x8000,
        "Left -> X-"
    );
}

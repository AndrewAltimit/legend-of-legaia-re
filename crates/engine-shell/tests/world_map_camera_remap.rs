//! The overworld camera-relative movement remap
//! ([`world_map_camera_relative_bits`]) must agree with what the player sees:
//! pressing "up" walks the character *away from the camera* on screen, and
//! "right" walks *screen-right*, for any camera azimuth.
//!
//! This is the ground-truth check for the remap. The remap lives in
//! `engine-core` (no renderer dependency, so it can't see the camera matrix);
//! this test projects the world direction the remap chooses back through the
//! real `world_map_camera_mvp` and asserts its screen-space motion points the
//! right way. If the remap's trig ever drifts from the camera geometry, this
//! fails. Pure math — runs in CI without disc data.

use glam::{Mat4, Vec3, Vec4};
use legaia_engine_core::world::world_map_camera_relative_bits;
use legaia_engine_render::window::world_map_camera_mvp;

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

/// Project a world point to normalized device coords (NDC: +x right, +y up).
fn project(mvp: &Mat4, p: Vec3) -> (f32, f32) {
    let c: Vec4 = *mvp * p.extend(1.0);
    (c.x / c.w, c.y / c.w)
}

/// Screen-space delta (Δndc) when the player walks one big step in the world
/// direction the remap chose for screen input `(sx, sy)` at `azimuth`.
fn screen_delta(azimuth: i32, sx: i32, sy: i32) -> (f32, f32) {
    // Unit AABB so framing is symmetric; the player sits at the centre.
    let lo = [-100.0, -10.0, -100.0];
    let hi = [100.0, 10.0, 100.0];
    let center = Vec3::new(
        (lo[0] + hi[0]) * 0.5,
        (lo[1] + hi[1]) * 0.5,
        (lo[2] + hi[2]) * 0.5,
    );
    // pan 0 frames the AABB centre; the player is at the centre.
    let mvp = world_map_camera_mvp(lo, hi, azimuth, 0, 0, 0, 16.0 / 9.0);

    let bits = world_map_camera_relative_bits(azimuth, sx, sy);
    let (dx, dz) = bits_to_world_dir(bits);
    let step = 20.0; // a visible nudge well inside the AABB
    let p0 = project(&mvp, center);
    let p1 = project(&mvp, center + Vec3::new(dx * step, 0.0, dz * step));
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

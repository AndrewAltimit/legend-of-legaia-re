//! The camera projection and the camera-relative node rotation.

use super::*;

#[test]
fn camera_identity_keeps_origin_at_screen_center() {
    let cam = Camera::for_viewport(320, 240);
    let mut cam = cam;
    cam.trans = GteVec3::new(0, 0, ROT_ONE * 1024); // 1024 units forward
    // A vertex at world origin sits 1024 in front of the camera.
    let p = cam.transform(GteVec3::new(0, 0, 0));
    assert_eq!(p.clip, Clip::SafeFront);
    assert_eq!(p.screen_xy.x, 160);
    assert_eq!(p.screen_xy.y, 120);
}

#[test]
fn camera_projects_x_to_right_of_screen() {
    let mut cam = Camera::for_viewport(320, 240);
    cam.trans = GteVec3::new(0, 0, ROT_ONE * 1024);
    // Vertex at +X (right of camera): screen.x > 160.
    let p = cam.transform(GteVec3::from_f32_q12(100.0, 0.0, 0.0));
    assert_eq!(p.clip, Clip::SafeFront);
    assert!(
        p.screen_xy.x > 160,
        "expected right of center: {}",
        p.screen_xy.x
    );
}

#[test]
fn camera_marks_behind_camera_vertex() {
    let cam = Camera::for_viewport(320, 240);
    // No translation; vertex with view.z = 0 is on camera plane.
    let p = cam.transform(GteVec3::new(0, 0, 0));
    assert_eq!(p.clip, Clip::Behind);
}

#[test]
fn camera_projection_is_pixel_exact_for_realistic_depth() {
    // Pin one specific projection so we catch regressions. The GTE divides
    // via the UNR reciprocal (crate::gte::math::gte_divide), not an exact
    // divide, so this asserts the hardware value: with H=320 and a vertex at
    // real (50, 0, 200) world units (q19.12), SZ3 = 200 (> H/2 = 160, so no
    // overflow clamp) and gte_divide(320, 200) yields the same 320*50/200 = 80
    // the exact divide would (the UNR error is 0 here).
    let cam = Camera {
        rot: GteMat3::IDENTITY,
        trans: GteVec3::new(0, 0, 0),
        h: 320,
        ofx: 0,
        ofy: 0,
    };
    let p = cam.transform(GteVec3::new(ROT_ONE * 50, 0, ROT_ONE * 200));
    assert_eq!(p.clip, Clip::SafeFront);
    assert_eq!(p.screen_xy.x, 80);
    assert_eq!(p.screen_xy.y, 0);
}

#[test]
fn camera_near_camera_vertex_overflow_clamps_like_hardware() {
    // A vertex at real depth 0.25 units (q19.12 z = 1024) has SZ3 = 0 after
    // the >>12 depth reduction, so 2*SZ3 = 0 <= H: hardware raises the divide
    // overflow flag and saturates the reciprocal to 0x1FFFF instead of
    // dividing. With IR1 = 1024>>12 = 0 the projected X collapses to OFX.
    // (An exact divide would have wrongly placed it at 320.)
    let cam = Camera {
        rot: GteMat3::IDENTITY,
        trans: GteVec3::new(0, 0, 0),
        h: 320,
        ofx: 0,
        ofy: 0,
    };
    let p = cam.transform(GteVec3::new(1024, 0, 1024));
    assert_eq!(p.clip, Clip::SafeFront); // still in front of the camera plane
    assert_eq!(p.screen_xy.x, 0);
}

// --- FUN_8001CF50 camera-relative node rotation ------------------------

#[test]
fn camera_view_rotation_bit_0x400_defers_to_the_saved_matrix() {
    assert!(camera_view_rotation(view_rot_flags::USE_SAVED_MATRIX, 0.3, 0.4, 0.5).is_none());
    // The bypass wins even when the three per-axis bits are clear.
    assert!(camera_view_rotation(0x0400, 0.0, 0.0, 0.0).is_none());
}

#[test]
fn camera_view_rotation_all_flags_set_is_identity() {
    // Every axis suppressed, but not the 0x400 bypass: the build still runs
    // and yields the identity FUN_8003D178 left in the GTE.
    let m = camera_view_rotation(
        view_rot_flags::SKIP_PITCH | view_rot_flags::SKIP_YAW | view_rot_flags::SKIP_ROLL,
        1.0,
        1.0,
        1.0,
    )
    .expect("no bypass bit");
    assert_eq!(m, GteMat3::IDENTITY);
}

#[test]
fn camera_view_rotation_skips_exactly_the_flagged_axis() {
    let (p, y, r) = (0.3f32, 0.7f32, 1.1f32);
    // A set bit skips its factor, so the result is the product of the other
    // two - in the same Rx * Ry * Rz order.
    assert_eq!(
        camera_view_rotation(view_rot_flags::SKIP_PITCH, p, y, r).unwrap(),
        GteMat3::IDENTITY
            .mul(&GteMat3::rot_y(y))
            .mul(&GteMat3::rot_z(r))
    );
    assert_eq!(
        camera_view_rotation(view_rot_flags::SKIP_YAW, p, y, r).unwrap(),
        GteMat3::IDENTITY
            .mul(&GteMat3::rot_x(p))
            .mul(&GteMat3::rot_z(r))
    );
    assert_eq!(
        camera_view_rotation(view_rot_flags::SKIP_ROLL, p, y, r).unwrap(),
        GteMat3::IDENTITY
            .mul(&GteMat3::rot_x(p))
            .mul(&GteMat3::rot_y(y))
    );
}

#[test]
fn camera_view_rotation_composition_order_is_pitch_yaw_roll() {
    let (p, y, r) = (0.4f32, 0.9f32, 0.2f32);
    let got = camera_view_rotation(0, p, y, r).unwrap();
    let want = GteMat3::rot_x(p)
        .mul(&GteMat3::rot_y(y))
        .mul(&GteMat3::rot_z(r));
    assert_eq!(got, want);
    // Order is load-bearing: the reverse product is a different matrix, so a
    // port that composed Rz * Ry * Rx would not silently agree.
    let reversed = GteMat3::rot_z(r)
        .mul(&GteMat3::rot_y(y))
        .mul(&GteMat3::rot_x(p));
    assert_ne!(got, reversed);
}

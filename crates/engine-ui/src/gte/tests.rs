use super::*;

#[test]
fn identity_passes_vector_through() {
    let v = GteVec3::new(123, -456, 789);
    let r = GteMat3::IDENTITY.mul_vec(v);
    assert_eq!(r, v);
}

#[test]
fn rot_y_180_negates_x_and_z() {
    let rot = GteMat3::rot_y(std::f32::consts::PI);
    let v = GteVec3::new(1000, 0, 0);
    let r = rot.mul_vec(v);
    // 180° about Y flips X (and Z when non-zero). Allow rounding error
    // up to a few units (q12 quantization → ~0.024% error per element).
    assert!((r.x - (-1000)).abs() <= 2, "x={}", r.x);
    assert_eq!(r.y, 0);
    assert!(r.z.abs() <= 2, "z={}", r.z);
}

#[test]
fn rot_trans_applies_rotation_then_translation() {
    let rot = GteMat3::IDENTITY;
    let trans = GteVec3::new(10, 20, 30);
    let v = GteVec3::new(1, 2, 3);
    assert_eq!(rot_trans(&rot, v, trans), GteVec3::new(11, 22, 33));
}

#[test]
fn fixed_point_round_trip() {
    let original = (1.5f32, -3.25, 0.125);
    let v = GteVec3::from_f32_q12(original.0, original.1, original.2);
    let back = v.to_f32_q12();
    // q12 fixed-point gives ~1/4096 resolution. Each example here is
    // exactly representable.
    assert!((back.0 - original.0).abs() < 1.0 / ROT_ONE as f32 + 1e-6);
    assert!((back.1 - original.1).abs() < 1.0 / ROT_ONE as f32 + 1e-6);
    assert!((back.2 - original.2).abs() < 1.0 / ROT_ONE as f32 + 1e-6);
}

#[test]
fn mul_vec_does_not_overflow_on_max_inputs() {
    // Worst case: rotation with max elements (32767) applied to a
    // vector with max coordinates (i32::MAX / 4 to keep headroom).
    // i64 accumulator must absorb 3 × i32×i16 without panicking.
    let m = GteMat3 {
        m: [[32767, 32767, 32767], [0, 0, 0], [0, 0, 0]],
    };
    let v = GteVec3::new(i32::MAX / 4, i32::MAX / 4, i32::MAX / 4);
    let r = m.mul_vec(v);
    assert_eq!(r.x, i32::MAX);
}

#[test]
fn rot_x_90_y_to_z() {
    // 90° pitch around +X axis sends +Y to +Z.
    let rot = GteMat3::rot_x(std::f32::consts::FRAC_PI_2);
    let v = GteVec3::new(0, 1000, 0);
    let r = rot.mul_vec(v);
    assert!(r.y.abs() <= 2, "y={}", r.y);
    assert!((r.z - 1000).abs() <= 2, "z={}", r.z);
}

#[test]
fn rot_z_90_x_to_y() {
    // 90° roll around +Z axis sends +X to +Y.
    let rot = GteMat3::rot_z(std::f32::consts::FRAC_PI_2);
    let v = GteVec3::new(1000, 0, 0);
    let r = rot.mul_vec(v);
    assert!(r.x.abs() <= 2, "x={}", r.x);
    assert!((r.y - 1000).abs() <= 2, "y={}", r.y);
}

#[test]
fn mat3_mul_identity_is_noop() {
    let r = GteMat3::rot_y(0.7);
    let combined = r.mul(&GteMat3::IDENTITY);
    // Identity composition should be lossless within q3.12 rounding.
    for i in 0..3 {
        for j in 0..3 {
            assert!(
                (combined.m[i][j] as i32 - r.m[i][j] as i32).abs() <= 1,
                "[{i}][{j}] mismatch: combined={} vs r={}",
                combined.m[i][j],
                r.m[i][j],
            );
        }
    }
}

#[test]
fn mat3_mul_compose_two_y_rotations() {
    // rot_y(a) * rot_y(b) ≈ rot_y(a + b) - verify within q3.12 rounding.
    let a = std::f32::consts::FRAC_PI_4;
    let b = std::f32::consts::FRAC_PI_3;
    let composed = GteMat3::rot_y(a).mul(&GteMat3::rot_y(b));
    let direct = GteMat3::rot_y(a + b);
    for i in 0..3 {
        for j in 0..3 {
            assert!(
                (composed.m[i][j] as i32 - direct.m[i][j] as i32).abs() <= 4,
                "[{i}][{j}] composed={} direct={}",
                composed.m[i][j],
                direct.m[i][j],
            );
        }
    }
}

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

#[test]
fn nclip_signs_back_vs_front() {
    // CCW triangle: (0,0), (10,0), (0,10). Under PSX winding
    // (y-down), this is back-facing - nclip > 0. CW is front (negative).
    let a = ScreenXY::new(0, 0);
    let b = ScreenXY::new(10, 0);
    let c = ScreenXY::new(0, 10);
    // (b-a)x = 10, y = 0; (c-a)x = 0, y = 10. cross = 10*10 - 0*0 = 100.
    assert_eq!(nclip(a, b, c), 100);
    // Reversed triangle is front-facing (negative).
    assert_eq!(nclip(a, c, b), -100);
}

#[test]
fn nclip_zero_area_is_degenerate() {
    let a = ScreenXY::new(5, 5);
    let b = ScreenXY::new(15, 5);
    let c = ScreenXY::new(25, 5); // colinear
    assert_eq!(nclip(a, b, c), 0);
}

#[test]
fn avsz3_zsf_default_averages_q12() {
    // With ZSF3 = ROT_ONE, the formula is (z0+z1+z2)*ROT_ONE / ROT_ONE
    // = z0+z1+z2 (the q12 cancels). So the function returns the *sum*,
    // not a true 1/3 average - that matches a retail capture where ZSF3
    // was loaded with 4096 (the "sum" bucket scale).
    assert_eq!(avsz3(100, 200, 300), 600);
}

#[test]
fn avsz3_with_one_third_scale() {
    // ZSF3 = ROT_ONE / 3 ≈ 1365 gives true average. Allow rounding.
    let r = avsz3_with_scale(100, 200, 300, ROT_ONE / 3);
    assert!((r - 200).abs() <= 1, "expected ~200, got {r}");
}

#[test]
fn avsz4_sums_four_zs() {
    assert_eq!(avsz4(100, 200, 300, 400), 1000);
}

#[test]
fn screen_to_pixel_clamps_off_screen() {
    let off_left = ScreenXY::new(-100, 50);
    let (px, py) = screen_to_pixel(off_left, 320, 240);
    assert_eq!(px, 0);
    assert_eq!(py, 50);

    let off_right = ScreenXY::new(500, 300);
    let (px, py) = screen_to_pixel(off_right, 320, 240);
    assert_eq!(px, 319);
    assert_eq!(py, 239);
}

#[test]
fn saturate_sxy_clamps_to_hardware_screen_range() {
    // The SXY FIFO saturates to signed 11 bits, not i16 (the i16 bound is the
    // IR numerator clamp). Confirmed bit-exact against a real-COP2 capture.
    let big = ScreenXY::new(i32::MAX, i32::MIN).saturate_sxy();
    assert_eq!(big.x, SX_MAX);
    assert_eq!(big.y, SX_MIN);
}

#[test]
fn raster_bbox_from_triangle() {
    let bbox = raster::BBox::from_triangle(
        ScreenXY::new(10, 20),
        ScreenXY::new(40, 5),
        ScreenXY::new(25, 50),
    );
    assert_eq!(bbox.min_x, 10);
    assert_eq!(bbox.min_y, 5);
    assert_eq!(bbox.max_x, 40);
    assert_eq!(bbox.max_y, 50);
}

#[test]
fn raster_bbox_clamp_off_screen_returns_none() {
    let bbox = raster::BBox::from_triangle(
        ScreenXY::new(-100, -100),
        ScreenXY::new(-50, -100),
        ScreenXY::new(-100, -50),
    );
    assert!(bbox.clamp(320, 240).is_none());
}

#[test]
fn raster_contains_inside_point() {
    // CW triangle (front-facing under PSX winding).
    let a = ScreenXY::new(0, 0);
    let b = ScreenXY::new(0, 10);
    let c = ScreenXY::new(10, 0);
    assert!(
        raster::contains(a, b, c, 2, 2),
        "(2,2) should be inside CW triangle"
    );
}

#[test]
fn raster_contains_outside_point() {
    let a = ScreenXY::new(0, 0);
    let b = ScreenXY::new(0, 10);
    let c = ScreenXY::new(10, 0);
    assert!(
        !raster::contains(a, b, c, 20, 20),
        "(20,20) outside triangle"
    );
}

// ----- Software near-plane clip helpers (FUN_80029724 / FUN_80036C4C) -----

/// Build a 0x1C-byte clip-vertex record with the fields the interpolator reads.
fn clip_vert(xyz: [i16; 3], rgb_code: [u8; 4], uv: [u8; 2]) -> [u8; raster::CLIP_VERT_STRIDE] {
    let mut v = [0u8; raster::CLIP_VERT_STRIDE];
    v[0xc..0xe].copy_from_slice(&xyz[0].to_le_bytes());
    v[0xe..0x10].copy_from_slice(&xyz[1].to_le_bytes());
    v[0x10..0x12].copy_from_slice(&xyz[2].to_le_bytes());
    v[0x14..0x18].copy_from_slice(&rgb_code);
    v[0x18] = uv[0];
    v[0x19] = uv[1];
    v
}

#[test]
fn interp_clip_vertex_midpoint_leading() {
    // Two adjacent records; cur at offset 0x1C, leading neighbour at 0.
    let nb = clip_vert([0, 0, 0], [0, 0, 0, 0x24], [0, 0]);
    let cur = clip_vert([100, -40, 800], [80, 40, 12, 0x24], [64, 200]);
    let mut buf = [0u8; 2 * raster::CLIP_VERT_STRIDE];
    buf[..raster::CLIP_VERT_STRIDE].copy_from_slice(&nb);
    buf[raster::CLIP_VERT_STRIDE..].copy_from_slice(&cur);

    let mut out = [0u8; 16];
    // Half-way (frac = 0x800 == 0.5 in q12), leading neighbour, interp RGB + UV.
    raster::interp_clip_vertex(
        &mut out,
        &buf,
        raster::CLIP_VERT_STRIDE,
        raster::clip_flags::RGB | raster::clip_flags::UV,
        0x800,
    );
    // XYZ: nb + (cur - nb) >> 1.
    assert_eq!(i16::from_le_bytes([out[0], out[1]]), 50);
    assert_eq!(i16::from_le_bytes([out[2], out[3]]), -20);
    assert_eq!(i16::from_le_bytes([out[4], out[5]]), 400);
    // RGB at +0x8..0x0A.
    assert_eq!([out[8], out[9], out[10]], [40, 20, 6]);
    // UV at +0x0C / +0x0D.
    assert_eq!([out[0xc], out[0xd]], [32, 100]);
}

#[test]
fn interp_clip_vertex_flat_color_copies_whole_word() {
    let nb = clip_vert([0, 0, 0], [0, 0, 0, 0], [0, 0]);
    let cur = clip_vert([10, 20, 30], [0x11, 0x22, 0x33, 0x44], [0, 0]);
    let mut buf = [0u8; 2 * raster::CLIP_VERT_STRIDE];
    buf[..raster::CLIP_VERT_STRIDE].copy_from_slice(&nb);
    buf[raster::CLIP_VERT_STRIDE..].copy_from_slice(&cur);

    let mut out = [0u8; 16];
    // No RGB / UV flags: the flat colour word is copied verbatim from cur.
    raster::interp_clip_vertex(&mut out, &buf, raster::CLIP_VERT_STRIDE, 0, 0x800);
    assert_eq!([out[8], out[9], out[10], out[11]], [0x11, 0x22, 0x33, 0x44]);
    // Frac zero-cross still lerps XYZ toward cur by half.
    assert_eq!(i16::from_le_bytes([out[0], out[1]]), 5);
}

#[test]
fn interp_clip_vertex_trailing_neighbour_and_endpoints() {
    // cur at offset 0, trailing neighbour at 0x1C.
    let cur = clip_vert([0, 0, 0], [0, 0, 0, 0], [0, 0]);
    let nb = clip_vert([200, 0, 0], [0, 0, 0, 0], [0, 0]);
    let mut buf = [0u8; 2 * raster::CLIP_VERT_STRIDE];
    buf[..raster::CLIP_VERT_STRIDE].copy_from_slice(&cur);
    buf[raster::CLIP_VERT_STRIDE..].copy_from_slice(&nb);

    let mut out = [0u8; 16];
    // frac = 0 -> result equals the (trailing) neighbour endpoint exactly.
    raster::interp_clip_vertex(&mut out, &buf, 0, raster::clip_flags::TRAILING, 0);
    assert_eq!(i16::from_le_bytes([out[0], out[1]]), 200);
    // frac = 0x1000 (1.0 q12) -> result equals cur exactly.
    raster::interp_clip_vertex(&mut out, &buf, 0, raster::clip_flags::TRAILING, 0x1000);
    assert_eq!(i16::from_le_bytes([out[0], out[1]]), 0);
}

#[test]
fn spread_prim_colors_tri_and_quad() {
    // 4 source colour words [R, G, B, code].
    let colors = [
        0x10, 0x11, 0x12, 0xAA, 0x20, 0x21, 0x22, 0xAA, 0x30, 0x31, 0x32, 0xAA, 0x40, 0x41, 0x42,
        0xAA,
    ];
    // POLY_G4 packet: colour word at +4 + 8*i, command byte at +7 + 8*i.
    let mut packet = [0xEEu8; 36];
    raster::spread_prim_colors(&mut packet, &colors, 4);
    for i in 0..4 {
        let d = 4 + i * 8;
        assert_eq!(
            [packet[d], packet[d + 1], packet[d + 2]],
            [
                0x10 + 0x10 * i as u8,
                0x11 + 0x10 * i as u8,
                0x12 + 0x10 * i as u8
            ]
        );
        // Command byte left untouched.
        assert_eq!(packet[d + 3], 0xEE);
    }

    // Triangle: only the first 3 colour slots written.
    let mut tri = [0xEEu8; 24];
    raster::spread_prim_colors(&mut tri, &colors, 3);
    assert_eq!([tri[4], tri[5], tri[6]], [0x10, 0x11, 0x12]);
    assert_eq!([tri[20], tri[21], tri[22]], [0x30, 0x31, 0x32]);

    // Unsupported count is a no-op.
    let mut none = [0xEEu8; 24];
    raster::spread_prim_colors(&mut none, &colors, 5);
    assert!(none.iter().all(|&b| b == 0xEE));
}

// ----- Gte register-state emulator tests -----

#[test]
fn gte_default_state_is_identity_with_no_translation() {
    let g = Gte::new();
    assert_eq!(g.rot, GteMat3::IDENTITY);
    assert_eq!(g.trans, GteVec3::default());
    assert_eq!(g.h, DEFAULT_H);
    assert_eq!(g.flag, 0);
    assert_eq!(g.zsf3, ROT_ONE);
    assert_eq!(g.zsf4, ROT_ONE);
}

#[test]
fn gte_rtps_pushes_one_sxy_per_call() {
    let mut g = Gte::new();
    g.set_viewport(320, 240);
    g.trans = GteVec3::new(0, 0, ROT_ONE * 1024);
    g.v[0] = GteVec3::new(0, 0, 0);
    let xy = g.rtps();
    assert_eq!(xy.x, 160);
    assert_eq!(xy.y, 120);
    // SXY FIFO: latest in slot 2, slot 1 = previous (default), slot 0 = older.
    assert_eq!(g.sxy_fifo[2], xy);
}

#[test]
fn gte_rtpt_pushes_three_vertices_in_fifo_order() {
    let mut g = Gte::new();
    g.set_viewport(320, 240);
    g.trans = GteVec3::new(0, 0, ROT_ONE * 1024);
    g.v[0] = GteVec3::new(0, 0, 0);
    // V1 to the right.
    g.v[1] = GteVec3::from_f32_q12(100.0, 0.0, 0.0);
    // V2 up.
    g.v[2] = GteVec3::from_f32_q12(0.0, -100.0, 0.0);
    let [s0, s1, s2] = g.rtpt();
    // After 3 RTPS calls, FIFO holds [s0, s1, s2] in order.
    assert_eq!(g.sxy_fifo[0], s0);
    assert_eq!(g.sxy_fifo[1], s1);
    assert_eq!(g.sxy_fifo[2], s2);
    assert_eq!(s0.x, 160);
    assert_eq!(s0.y, 120);
    assert!(s1.x > 160, "V1 right of center: {}", s1.x);
    assert!(s2.y < 120, "V2 above center: {}", s2.y);
}

#[test]
fn gte_rtps_sets_mac_and_ir_registers() {
    let mut g = Gte::new();
    g.set_viewport(320, 240);
    g.trans = GteVec3::new(10, 20, ROT_ONE * 100);
    g.v[0] = GteVec3::new(0, 0, 0);
    let _ = g.rtps();
    // MAC = post-rotation view (rot=identity, so view = trans).
    assert_eq!(g.mac1, 10);
    assert_eq!(g.mac2, 20);
    assert_eq!(g.mac3, ROT_ONE as i64 * 100);
    // IR1 / IR2 fit in i16 (10, 20).
    assert_eq!(g.ir1, 10);
    assert_eq!(g.ir2, 20);
    // IR3 saturates to i16::MAX (mac3 = 409_600 > 32767).
    assert_eq!(g.ir3, i16::MAX as i32);
    assert_ne!(g.flag & flag_bits::IR3_SATURATED, 0);
    assert_ne!(g.flag & flag_bits::ANY_ERROR, 0);
}

#[test]
fn gte_rtps_behind_camera_overflows_divide_not_mac3_neg() {
    // A behind/on-plane vertex is NOT a special hardware case. SZ3 clamps to
    // 0 and the perspective divide overflows to the 0x1FFFF quotient, raising
    // DIVIDE_OVERFLOW. Hardware does not raise MAC3_OVERFLOW_NEG for a merely
    // behind-camera vertex - that bit is reserved for a genuine 44-bit MAC3
    // overflow. Matches gte_rtps_internal in the Beetle-validated reference.
    let mut g = Gte::new();
    g.set_viewport(320, 240);
    // view.z == 0 exactly: the divide overflows, but pushing SZ3 = 0 is not a
    // saturation, so SZ3_OTZ stays clear.
    g.v[0] = GteVec3::new(0, 0, 0);
    g.rtps();
    assert_ne!(g.flag & flag_bits::DIVIDE_OVERFLOW, 0);
    assert_eq!(g.flag & flag_bits::MAC3_OVERFLOW_NEG, 0);
    assert_eq!(g.flag & flag_bits::SZ3_OTZ_SATURATED, 0);

    // Strictly behind the camera: the negative MAC3 pushes SZ3 = 0 via the
    // clamp, so SZ3_OTZ joins DIVIDE_OVERFLOW; still no MAC3_OVERFLOW_NEG.
    let mut g = Gte::new();
    g.set_viewport(320, 240);
    g.trans = GteVec3::new(0, 0, -(ROT_ONE * 100));
    g.v[0] = GteVec3::new(0, 0, 0);
    g.rtps();
    assert_ne!(g.flag & flag_bits::DIVIDE_OVERFLOW, 0);
    assert_ne!(g.flag & flag_bits::SZ3_OTZ_SATURATED, 0);
    assert_eq!(g.flag & flag_bits::MAC3_OVERFLOW_NEG, 0);
}

/// Independent reference projection mirroring the Beetle-validated
/// `gte_rtps_internal` (psxrecomp `runtime/src/gte.cpp`) in the HARDWARE
/// register scale and with the reference's OFX-before-shift ordering -
/// deliberately different arithmetic from `Gte::rtps`, which holds MAC/IR in
/// q19.12 and adds OFX *after* the `>>16` floor-shift. Returns the
/// hardware-comparable subset: the saturated SXY, the pushed SZ3, and the
/// FLAG bits that do not depend on the q19.12 MAC/IR representation. The
/// IR/MAC-saturation bits are excluded on purpose - they diverge by the
/// documented scale convention, which is exactly why a naive register-exact
/// cosim comparison cannot be used against this port.
fn reference_rtps(
    rot: &GteMat3,
    trans: GteVec3,
    v: GteVec3,
    h: i32,
    ofx: i32,
    ofy: i32,
) -> (ScreenXY, u16, u32) {
    // Reference MAC = (RT*V + TR<<12) >> 12. `rot_trans` yields RT*V + trans
    // with trans already in q19.12, so the `>>12` recovers the hardware-scale
    // MAC the real GTE latches.
    let view = rot_trans(rot, v, trans);
    let mac1 = (view.x >> ROT_FRAC_BITS) as i64;
    let mac2 = (view.y >> ROT_FRAC_BITS) as i64;
    let mac3 = view.z >> ROT_FRAC_BITS;

    let mut flag = 0u32;
    // push_sz: clamp MAC3 into the u16 SZ FIFO, flagging on saturation.
    let sz3 = if mac3 < 0 {
        flag |= flag_bits::SZ3_OTZ_SATURATED;
        0
    } else if mac3 > u16::MAX as i32 {
        flag |= flag_bits::SZ3_OTZ_SATURATED;
        u16::MAX as i32
    } else {
        mac3
    } as u16;

    // Screen numerators are the i16-saturated hardware IR1/IR2.
    let ir1 = mac1.clamp(SXY_MIN as i64, SXY_MAX as i64);
    let ir2 = mac2.clamp(SXY_MIN as i64, SXY_MAX as i64);

    let (recip, overflow) = gte_divide(h as u16, sz3);
    if overflow {
        flag |= flag_bits::DIVIDE_OVERFLOW;
    }

    // Reference ordering: add the fixed-point OFX/OFY (`<<16`), THEN
    // floor-shift by 16. The port does `(IR*recip)>>16` first and adds an
    // integer-pixel OFX after; the two agree because retail's OFX/OFY have
    // zero low 16 bits (SetGeomOffset, FUN_8005B7F8). This sweep machine-
    // checks that equivalence across the full numerator-sign space.
    let sx = (((ofx as i64) << 16) + ir1 * recip) >> 16;
    let sy = (((ofy as i64) << 16) + ir2 * recip) >> 16;

    // Final screen coords saturate to the hardware signed-11-bit range,
    // raising SX2/SY2 - distinct from the i16 IR-numerator clamp above.
    if sx < SX_MIN as i64 || sx > SX_MAX as i64 {
        flag |= flag_bits::SX2_SATURATED;
    }
    if sy < SX_MIN as i64 || sy > SX_MAX as i64 {
        flag |= flag_bits::SY2_SATURATED;
    }
    let sxy = ScreenXY::new(sx as i32, sy as i32).saturate_sxy();
    (sxy, sz3, flag)
}

#[test]
fn gte_rtps_matches_independent_reference_over_input_sweep() {
    // Closes the RTPS integration-validation gap: `Gte::rtps` (q19.12 MAC/IR,
    // OFX added after the floor-shift) is checked against `reference_rtps`, an
    // independent second implementation in the hardware register scale with
    // the reference's OFX-before-shift ordering. Everything that determines
    // the on-screen projection - SXY, SZ, divide overflow, SXY saturation,
    // SZ3 clamp - is compared with ZERO tolerance across a wide input sweep.
    //
    // The IR/MAC-saturation FLAG bits (and thus ANY_ERROR) are intentionally
    // outside the mask: the port keeps MAC/IR in q19.12, so those bits diverge
    // from hardware by the documented scale convention. This test pins exactly
    // which cop2 outputs are hardware-comparable - the contract a future
    // real-silicon/cosim oracle would compare against.
    const MASK: u32 = flag_bits::DIVIDE_OVERFLOW
        | flag_bits::SZ3_OTZ_SATURATED
        | flag_bits::SX2_SATURATED
        | flag_bits::SY2_SATURATED;

    let rots = [
        GteMat3::IDENTITY,
        GteMat3::rot_x(0.6),
        GteMat3::rot_y(1.2),
        GteMat3::rot_z(-0.9),
        GteMat3::rot_y(0.7).mul(&GteMat3::rot_x(0.3)),
    ];
    // Depths in q19.12: on/behind plane, near the H/2 overflow knee, and far.
    let depths = [
        -(ROT_ONE * 100),
        0,
        ROT_ONE / 2,
        ROT_ONE * 2,
        ROT_ONE * 50,
        ROT_ONE * 200,
        ROT_ONE * 1000,
        ROT_ONE * 8000,
    ];
    let laterals = [
        GteVec3::new(0, 0, 0),
        GteVec3::new(ROT_ONE * 30, -(ROT_ONE * 20), 0),
        // Far off-axis: under the identity rotation this drives the projected
        // SXY past the ±0x400 hardware screen range, exercising SX2/SY2.
        GteVec3::new(ROT_ONE * 25000, -(ROT_ONE * 25000), 0),
    ];
    let verts = [
        GteVec3::new(0, 0, 0),
        GteVec3::new(100, 0, 0),
        GteVec3::new(0, 80, 0),
        GteVec3::new(-64, 40, 128),
        GteVec3::new(500, -300, 200),
    ];
    let hs = [256, 320];
    let offsets = [(0, 0), (160, 120)];

    let mut cases = 0u32;
    let mut flag_coverage = 0u32;
    for rot in &rots {
        for &depth in &depths {
            for lat in &laterals {
                let trans = GteVec3::new(lat.x, lat.y, depth);
                for v in &verts {
                    for &h in &hs {
                        for &(ofx, ofy) in &offsets {
                            let mut g = Gte::new();
                            g.rot = *rot;
                            g.trans = trans;
                            g.h = h;
                            g.ofx = ofx;
                            g.ofy = ofy;
                            g.v[0] = *v;
                            let sxy_port = g.rtps();

                            let (sxy_ref, sz_ref, flag_ref) =
                                reference_rtps(rot, trans, *v, h, ofx, ofy);

                            let ctx = format!("depth={depth} v={v:?} h={h} of=({ofx},{ofy})");
                            assert_eq!(sxy_port, sxy_ref, "SXY mismatch @ {ctx}");
                            assert_eq!(g.sz_fifo[3], sz_ref, "SZ mismatch @ {ctx}");
                            assert_eq!(
                                g.flag & MASK,
                                flag_ref & MASK,
                                "FLAG-subset mismatch @ {ctx}"
                            );
                            flag_coverage |= flag_ref & MASK;
                            cases += 1;
                        }
                    }
                }
            }
        }
    }
    // Guard against the sweep silently collapsing to nothing, and confirm the
    // flag comparison is non-vacuous - every masked bit fires somewhere, so
    // the equality above is exercised on set bits, not just zeros.
    assert_eq!(cases, 2400, "expected the full cartesian sweep");
    assert_eq!(
        flag_coverage, MASK,
        "sweep did not exercise every masked bit"
    );
}

#[test]
fn gte_nclip_writes_mac0_and_returns_signed_area() {
    let mut g = Gte::new();
    // Manually populate SXY FIFO.
    g.sxy_fifo = [
        ScreenXY::new(0, 0),
        ScreenXY::new(10, 0),
        ScreenXY::new(0, 10),
    ];
    let r = g.nclip();
    assert_eq!(r, 100);
    assert_eq!(g.mac0, 100);
}

#[test]
fn gte_avsz3_writes_otz_and_mac0() {
    let mut g = Gte::new();
    g.zsf3 = ROT_ONE; // sum-bucket scale (default)
    g.sz_fifo = [0, 100, 200, 300];
    let otz = g.avsz3();
    // (100 + 200 + 300) = 600. With zsf3=4096 ⇒ 600*4096 = 2_457_600.
    // OTZ = 2_457_600 >> 12 = 600. MAC0 = 2_457_600.
    assert_eq!(otz, 600);
    assert_eq!(g.otz, 600);
    assert_eq!(g.mac0, 2_457_600);
}

#[test]
fn gte_avsz4_uses_all_four_sz_entries() {
    let mut g = Gte::new();
    g.zsf4 = ROT_ONE;
    g.sz_fifo = [50, 100, 150, 200];
    let otz = g.avsz4();
    assert_eq!(otz, 500);
}

#[test]
fn gte_otz_saturates_high_to_u16_max() {
    let mut g = Gte::new();
    g.zsf3 = ROT_ONE;
    // 3 * 0xFFFF = 196_605, * 4096 = 805_273_600, >> 12 = 196_605.
    // 196_605 > 65_535 ⇒ clamp + flag.
    g.sz_fifo = [0, u16::MAX, u16::MAX, u16::MAX];
    let otz = g.avsz3();
    assert_eq!(otz, u16::MAX);
    assert_ne!(g.flag & flag_bits::SZ3_OTZ_SATURATED, 0);
}

#[test]
fn gte_mvmva_with_identity_passes_vector_through() {
    let mut g = Gte::new();
    g.mvmva(
        &GteMat3::IDENTITY,
        GteVec3::new(100, 200, 300),
        GteVec3::default(),
        true, // shift by ROT_FRAC_BITS
        false,
    );
    // identity (q3.12) * (100, 200, 300) gives (100*4096, 200*4096,
    // 300*4096) before the shift; shifted by 12 returns the original
    // vector. IR1/2/3 then take the same values (within i16 range).
    assert_eq!(g.mac1, 100);
    assert_eq!(g.mac2, 200);
    assert_eq!(g.mac3, 300);
    assert_eq!(g.ir1, 100);
    assert_eq!(g.ir2, 200);
    assert_eq!(g.ir3, 300);
}

#[test]
fn gte_mvmva_no_shift_keeps_full_precision() {
    let mut g = Gte::new();
    g.mvmva(
        &GteMat3::IDENTITY,
        GteVec3::new(100, 200, 300),
        GteVec3::default(),
        false,
        false,
    );
    // identity * v = q12 view. Without shift MAC keeps the full
    // q12 product (each element scaled by ROT_ONE).
    assert_eq!(g.mac1, 100 * ROT_ONE as i64);
    assert_eq!(g.mac2, 200 * ROT_ONE as i64);
    assert_eq!(g.mac3, 300 * ROT_ONE as i64);
    // IR clamps to i16::MAX.
    assert_eq!(g.ir1, i16::MAX as i32);
    assert_ne!(g.flag & flag_bits::IR1_SATURATED, 0);
}

#[test]
fn gte_mvmva_lm_clamps_to_zero_minimum() {
    let mut g = Gte::new();
    // Negative input + LM=true ⇒ IR clamps to 0, FLAG sets sat bit.
    g.mvmva(
        &GteMat3::IDENTITY,
        GteVec3::new(-50, -100, -200),
        GteVec3::default(),
        true,
        true, // LM
    );
    assert_eq!(g.ir1, 0);
    assert_eq!(g.ir2, 0);
    assert_eq!(g.ir3, 0);
    assert_ne!(g.flag & flag_bits::IR1_SATURATED, 0);
}

#[test]
fn gte_clear_flag_resets() {
    let mut g = Gte::new();
    g.flag = 0xFFFF_FFFF;
    g.clear_flag();
    assert_eq!(g.flag, 0);
}

#[test]
fn gte_rtpt_matches_camera_transform() {
    // Verify the register-state RTPT produces the same SXY as the
    // higher-level Camera::transform shim.
    let mut g = Gte::new();
    g.set_viewport(320, 240);
    g.trans = GteVec3::new(0, 0, ROT_ONE * 512);
    g.rot = GteMat3::rot_y(0.3);
    let v = [
        GteVec3::from_f32_q12(50.0, 0.0, 0.0),
        GteVec3::from_f32_q12(-50.0, 0.0, 0.0),
        GteVec3::from_f32_q12(0.0, 50.0, 0.0),
    ];
    g.v = v;
    let [s0, s1, s2] = g.rtpt();

    let cam = Camera {
        rot: g.rot,
        trans: g.trans,
        h: g.h,
        ofx: g.ofx,
        ofy: g.ofy,
    };
    let p0 = cam.transform(v[0]).screen_xy.saturate_sxy();
    let p1 = cam.transform(v[1]).screen_xy.saturate_sxy();
    let p2 = cam.transform(v[2]).screen_xy.saturate_sxy();
    assert_eq!(s0, p0);
    assert_eq!(s1, p1);
    assert_eq!(s2, p2);
}

#[test]
fn raster_iterates_inside_pixels() {
    // Simple CW right-triangle covering pixels (1,1)..(8,1), etc.
    // We just count to make sure the iterator covers a believable set.
    let a = ScreenXY::new(0, 0);
    let b = ScreenXY::new(0, 10);
    let c = ScreenXY::new(10, 0);
    let mut count = 0;
    raster::rasterize_triangle(a, b, c, 320, 240, |_, _, _| count += 1);
    // Triangle area = 50 px²; rasterizer hits ~50 inside pixels.
    // Allow a small fudge for top-left fill-rule edge inclusion.
    assert!((30..=60).contains(&count), "got {count} pixels");
}

// ---------------------------------------------------------------------
// Lighting / colour ops (NCDS / NCDT / DCPL / DPCS / DPCT / INTPL /
// SQR / OP / GPF / GPL).
// ---------------------------------------------------------------------

#[test]
fn rgb_fifo_starts_empty() {
    let g = Gte::new();
    for entry in g.rgb_fifo {
        assert_eq!(entry, [0; 4]);
    }
}

#[test]
fn ncds_pushes_rgb_fifo_entry() {
    let mut g = Gte::new();
    g.rgbc = [0xFF, 0xFF, 0xFF, 0x00];
    g.ir0 = 0; // disable far-color blend so we get pure light pass.
    // Configure light so a unit normal becomes a small intensity.
    g.light = GteMat3::IDENTITY;
    g.light_color = GteMat3::IDENTITY;
    g.v[0] = GteVec3::new(ROT_ONE, 0, 0);
    let _ = g.ncds();
    // The newest RGB should be in slot 2.
    let r = g.rgb_fifo[2];
    // alpha (CODE byte) preserved
    assert_eq!(r[3], 0x00);
}

#[test]
fn ncdt_writes_three_fifo_entries() {
    let mut g = Gte::new();
    g.rgbc = [0x80, 0x80, 0x80, 0x10];
    g.ir0 = 0;
    g.light = GteMat3::IDENTITY;
    g.light_color = GteMat3::IDENTITY;
    g.v[0] = GteVec3::new(ROT_ONE, 0, 0);
    g.v[1] = GteVec3::new(0, ROT_ONE, 0);
    g.v[2] = GteVec3::new(0, 0, ROT_ONE);
    let _ = g.ncdt();
    // Each FIFO entry should preserve the alpha byte.
    for entry in g.rgb_fifo {
        assert_eq!(entry[3], 0x10);
    }
}

#[test]
fn dcpl_modulates_rgbc_through_ir() {
    let mut g = Gte::new();
    g.rgbc = [0xFF, 0x00, 0x00, 0x00];
    g.ir1 = 0x10;
    g.ir2 = 0x10;
    g.ir3 = 0x10;
    g.ir0 = 0; // no far-color blend
    let out = g.dcpl();
    // R = (IR1 * 0xFF) >> 4 = (16 * 255) >> 4 = 255; G/B = 0
    assert_eq!(out[0], 0xFF);
    assert_eq!(out[1], 0);
    assert_eq!(out[2], 0);
}

#[test]
fn dpcs_blends_rgbc_toward_far_color_at_ir0_max() {
    let mut g = Gte::new();
    g.rgbc = [0x00, 0x00, 0x00, 0x00];
    // Far color full white in q3.12.
    g.far_color = GteVec3::new(0xFF, 0xFF, 0xFF);
    // IR0 at full-blend. Conventional GTE max for IR0 is 4096 (1.0 in q12).
    g.ir0 = ROT_ONE;
    let out = g.dpcs();
    // Full blend toward far_color should deliver close to (255, 255, 255).
    // Allow ±1 for rounding.
    assert!(out[0] >= 254);
    assert!(out[1] >= 254);
    assert!(out[2] >= 254);
}

#[test]
fn dpcs_zero_blend_preserves_rgbc() {
    let mut g = Gte::new();
    g.rgbc = [0x80, 0x40, 0x20, 0x10];
    g.far_color = GteVec3::new(0xFF, 0xFF, 0xFF);
    g.ir0 = 0; // no blend
    let out = g.dpcs();
    assert_eq!(out[0], 0x80);
    assert_eq!(out[1], 0x40);
    assert_eq!(out[2], 0x20);
    assert_eq!(out[3], 0x10);
}

#[test]
fn intpl_writes_macs_from_ir_and_far_color() {
    let mut g = Gte::new();
    g.ir1 = 100;
    g.ir2 = 200;
    g.ir3 = 50;
    g.far_color = GteVec3::new(500, 100, -50);
    g.ir0 = ROT_ONE; // full blend
    g.intpl();
    // MAC1 = IR1 + ((FC.x - IR1) * IR0 / 4096) = 100 + (400 * 1) = 500
    assert_eq!(g.mac1, 500);
    assert_eq!(g.mac2, 100);
    assert_eq!(g.mac3, -50);
    assert_eq!(g.ir1, 500);
    assert_eq!(g.ir2, 100);
    assert_eq!(g.ir3, -50);
}

#[test]
fn intpl_zero_blend_is_noop() {
    let mut g = Gte::new();
    g.ir1 = 100;
    g.ir2 = 200;
    g.ir3 = 50;
    g.far_color = GteVec3::new(500, 100, -50);
    g.ir0 = 0;
    g.intpl();
    assert_eq!(g.mac1, 100);
    assert_eq!(g.mac2, 200);
    assert_eq!(g.mac3, 50);
}

#[test]
fn sqr_squares_ir_and_writes_macs() {
    let mut g = Gte::new();
    g.ir1 = 30;
    g.ir2 = -40;
    g.ir3 = 50;
    g.sqr(false);
    assert_eq!(g.mac1, 900);
    assert_eq!(g.mac2, 1600);
    assert_eq!(g.mac3, 2500);
}

#[test]
fn op_cross_product_with_unit_diagonal() {
    let mut g = Gte::new();
    // D = (1,1,1) in q3.12; IR = (a, b, c).
    g.rot = GteMat3::IDENTITY;
    g.ir1 = 100;
    g.ir2 = 200;
    g.ir3 = 300;
    // Pre-shift so we don't have to undo q12 scaling for the unit test.
    g.op(true);
    // mac1 = D2 * IR3 - D3 * IR2 = 4096 * 300 - 4096 * 200 = 4096 * 100
    // After shift_frac: mac1 = 100, mac2 = D3*IR1 - D1*IR3 = 100-300 = -200,
    // mac3 = D1*IR2 - D2*IR1 = 200 - 100 = 100.
    assert_eq!(g.mac1, 100);
    assert_eq!(g.mac2, -200);
    assert_eq!(g.mac3, 100);
}

#[test]
fn gpf_multiplies_ir_by_ir0_and_writes_mac() {
    let mut g = Gte::new();
    g.ir0 = 2;
    g.ir1 = 5;
    g.ir2 = 10;
    g.ir3 = 15;
    g.gpf(false);
    assert_eq!(g.mac1, 10);
    assert_eq!(g.mac2, 20);
    assert_eq!(g.mac3, 30);
}

#[test]
fn gpl_accumulates_ir_times_ir0() {
    let mut g = Gte::new();
    g.mac1 = 100;
    g.mac2 = 200;
    g.mac3 = 300;
    g.ir0 = 3;
    g.ir1 = 4;
    g.ir2 = 5;
    g.ir3 = 6;
    g.gpl(false);
    assert_eq!(g.mac1, 100 + 12);
    assert_eq!(g.mac2, 200 + 15);
    assert_eq!(g.mac3, 300 + 18);
}

#[test]
fn intpl_chains_into_dpcs_pipeline() {
    // INTPL writes MAC/IR; DPCS reads IR0 + RGBC + FC. Verify the
    // composition makes sense.
    let mut g = Gte::new();
    g.ir1 = 100;
    g.ir2 = 100;
    g.ir3 = 100;
    g.far_color = GteVec3::new(200, 200, 200);
    g.ir0 = ROT_ONE / 2; // half blend
    g.intpl();
    assert_eq!(g.ir1, 150); // 100 + 50%*100
}

#[test]
fn rgb_fifo_advances_oldest_first() {
    let mut g = Gte::new();
    g.rgbc = [0x10, 0x20, 0x30, 0x40];
    g.far_color = GteVec3::default();
    g.ir0 = 0;
    let _ = g.dpcs();
    assert_eq!(g.rgb_fifo[2], [0x10, 0x20, 0x30, 0x40]);
    g.rgbc = [0x50, 0x60, 0x70, 0x80];
    let _ = g.dpcs();
    assert_eq!(g.rgb_fifo[2], [0x50, 0x60, 0x70, 0x80]);
    assert_eq!(g.rgb_fifo[1], [0x10, 0x20, 0x30, 0x40]);
}

#[test]
fn copop_cycle_counts_match_hardware_table() {
    // Spot-check the canonical Nocash entries.
    assert_eq!(CopOp::Rtps.cycles(), 15);
    assert_eq!(CopOp::Rtpt.cycles(), 23);
    assert_eq!(CopOp::Nclip.cycles(), 8);
    assert_eq!(CopOp::Avsz3.cycles(), 5);
    assert_eq!(CopOp::Avsz4.cycles(), 6);
    assert_eq!(CopOp::Mvmva.cycles(), 8);
    assert_eq!(CopOp::Ncds.cycles(), 19);
    assert_eq!(CopOp::Ncdt.cycles(), 44);
    assert_eq!(CopOp::Nccs.cycles(), 17);
    assert_eq!(CopOp::Ncct.cycles(), 39);
    assert_eq!(CopOp::Cc.cycles(), 11);
    assert_eq!(CopOp::Cdp.cycles(), 13);
    assert_eq!(CopOp::Ncs.cycles(), 14);
    assert_eq!(CopOp::Nct.cycles(), 30);
    assert_eq!(CopOp::Sqr.cycles(), 5);
    assert_eq!(CopOp::Op.cycles(), 6);
    assert_eq!(CopOp::Gpf.cycles(), 5);
    assert_eq!(CopOp::Gpl.cycles(), 5);
    assert_eq!(CopOp::Dcpl.cycles(), 8);
    assert_eq!(CopOp::Dpcs.cycles(), 8);
    assert_eq!(CopOp::Dpct.cycles(), 17);
    assert_eq!(CopOp::Intpl.cycles(), 8);
}

#[test]
fn cycle_accumulator_charges_per_op() {
    let mut g = Gte::new();
    assert_eq!(g.cycles, 0);
    let _ = g.rtps();
    assert_eq!(g.cycles, CopOp::Rtps.cycles() as u64);
    let _ = g.rtpt();
    assert_eq!(
        g.cycles,
        (CopOp::Rtps.cycles() + CopOp::Rtpt.cycles()) as u64
    );
    g.reset_cycles();
    assert_eq!(g.cycles, 0);
}

#[test]
fn cycle_accumulator_works_for_lighting_ops() {
    let mut g = Gte::new();
    g.rgbc = [0x80, 0x80, 0x80, 0];
    g.v[0] = GteVec3::new(0, 0, 0);
    let _ = g.ncds();
    assert_eq!(g.cycles, CopOp::Ncds.cycles() as u64);
    let _ = g.cdp();
    assert_eq!(
        g.cycles,
        (CopOp::Ncds.cycles() + CopOp::Cdp.cycles()) as u64
    );
}

#[test]
fn ncs_pushes_modulated_rgb() {
    let mut g = Gte::new();
    g.rgbc = [0xFF, 0x80, 0x40, 0x12];
    g.light = GteMat3::IDENTITY;
    g.light_color = GteMat3::IDENTITY;
    g.v[0] = GteVec3::new(ROT_ONE, ROT_ONE, ROT_ONE);
    let out = g.ncs();
    // Code byte should round-trip through the alpha channel.
    assert_eq!(out[3], 0x12);
    // RGB FIFO advanced.
    assert_eq!(g.rgb_fifo[2], out);
}

#[test]
fn nct_runs_three_pass_lighting() {
    let mut g = Gte::new();
    g.rgbc = [0x80, 0x80, 0x80, 0];
    g.light = GteMat3::IDENTITY;
    g.light_color = GteMat3::IDENTITY;
    g.v[0] = GteVec3::new(ROT_ONE, 0, 0);
    g.v[1] = GteVec3::new(0, ROT_ONE, 0);
    g.v[2] = GteVec3::new(0, 0, ROT_ONE);
    let outs = g.nct();
    assert_eq!(outs.len(), 3);
    // Three different normals → three different RGB outputs.
    assert!(outs[0] != outs[1] || outs[1] != outs[2]);
}

#[test]
fn nccs_runs_double_light_pass_relative_to_ncs() {
    // light_color matrix that scales by 0.5 per channel - the second
    // pass in NCCS should darken the result vs NCS.
    let mut lc = GteMat3::IDENTITY;
    lc.m[0][0] = (ROT_ONE / 2) as i16;
    lc.m[1][1] = (ROT_ONE / 2) as i16;
    lc.m[2][2] = (ROT_ONE / 2) as i16;

    // NCS reference run.
    let mut g = Gte::new();
    g.rgbc = [0xFF, 0xFF, 0xFF, 0];
    g.light = GteMat3::IDENTITY;
    g.light_color = lc;
    g.v[0] = GteVec3::new(ROT_ONE, ROT_ONE, ROT_ONE);
    let ncs_rgb = g.ncs();

    // NCCS - same inputs.
    let mut g2 = Gte::new();
    g2.rgbc = g.rgbc;
    g2.light = g.light;
    g2.light_color = g.light_color;
    g2.v[0] = g.v[0];
    let nccs_rgb = g2.nccs();
    assert!(nccs_rgb[0] <= ncs_rgb[0]);
    assert!(nccs_rgb[1] <= ncs_rgb[1]);
    assert!(nccs_rgb[2] <= ncs_rgb[2]);
}

#[test]
fn cdp_pushes_rgb_with_code_byte_preserved() {
    let mut g = Gte::new();
    g.rgbc = [0xFF, 0xFF, 0xFF, 0x42];
    g.ir1 = 0;
    g.ir2 = 0;
    g.ir3 = 0;
    g.light_color = GteMat3::IDENTITY;
    g.far_color = GteVec3::new(0x100, 0x200, 0x300);
    g.ir0 = ROT_ONE; // full blend toward far_color
    let _ = g.cdp();
    // RGB FIFO advanced and code byte preserved.
    assert_eq!(g.rgb_fifo[2][3], 0x42);
}

#[test]
fn cc_does_not_blend_against_far_color() {
    // CC should NOT touch far_color. Set IR very low so the post-modulate
    // result is well below saturation, then run CC with a large
    // far_color. If the implementation accidentally blended toward
    // far_color, the result would saturate to 0xFF; if it doesn't,
    // the modulated value stays small.
    let mut g = Gte::new();
    g.rgbc = [0x40, 0x40, 0x40, 0x12];
    g.ir1 = 0x10;
    g.ir2 = 0x10;
    g.ir3 = 0x10;
    g.light_color = GteMat3::IDENTITY;
    g.far_color = GteVec3::new(0xFFFF, 0xFFFF, 0xFFFF);
    g.ir0 = ROT_ONE;
    let out = g.cc();
    assert_eq!(out[3], 0x12);
    // Without far-color blending, the result should be small.
    assert!(
        out[0] < 0x80,
        "cc unexpectedly blended toward far_color (got {})",
        out[0]
    );
}

#[test]
fn ncds_saturates_overflow_to_ff() {
    let mut g = Gte::new();
    // Drive a big intensity through to force saturation.
    g.rgbc = [0xFF, 0xFF, 0xFF, 0x00];
    // Light matrix that amplifies aggressively.
    let mut amp = GteMat3::IDENTITY;
    amp.m[0][0] = i16::MAX; // 32767 - large q3.12 -> after >>12 stays positive.
    amp.m[1][1] = i16::MAX;
    amp.m[2][2] = i16::MAX;
    g.light = amp;
    g.light_color = amp;
    g.v[0] = GteVec3::new(ROT_ONE, ROT_ONE, ROT_ONE);
    g.ir0 = 0;
    let out = g.ncds();
    assert_eq!(out, [0xFF, 0xFF, 0xFF, 0x00]);
    assert!(g.flag & flag_bits::ANY_ERROR != 0);
}

// ---------------- GTE Phase 6: register-transfer + memory ops ---------

#[test]
fn pack_unpack_round_trips_signed_pair() {
    let pairs: &[(i16, i16)] = &[
        (0, 0),
        (-1, -1),
        (i16::MIN, i16::MAX),
        (1234, -5678),
        (i16::MAX, i16::MIN),
    ];
    for (lo, hi) in pairs {
        let packed = pack_i16_lo_hi(*lo, *hi);
        let (l2, h2) = unpack_i16_lo_hi(packed);
        assert_eq!(*lo, l2);
        assert_eq!(*hi, h2);
    }
}

#[test]
fn mtc2_then_mfc2_round_trips_v0_xy() {
    let mut g = Gte::new();
    // V0.x = 0x1234, V0.y = -2 (0xFFFE) packed as low/high i16 in cop2cr0.
    let val = pack_i16_lo_hi(0x1234, -2);
    g.mtc2(0, val);
    assert_eq!(g.v[0].x, 0x1234);
    assert_eq!(g.v[0].y, -2);
    let read = g.mfc2(0);
    assert_eq!(read, val);
}

#[test]
fn mtc2_v0_z_sign_extends_low_half() {
    let mut g = Gte::new();
    g.mtc2(1, 0xFFFFu32); // -1 as i16 in low half
    assert_eq!(g.v[0].z, -1);
    // mfc2 sign-extends back to a 32-bit -1.
    assert_eq!(g.mfc2(1), 0xFFFFFFFFu32);
}

#[test]
fn mtc2_rgbc_writes_byte_lane_layout() {
    let mut g = Gte::new();
    g.mtc2(6, 0xCC_BB_AA_99); // [0x99, 0xAA, 0xBB, 0xCC] little-endian
    assert_eq!(g.rgbc, [0x99, 0xAA, 0xBB, 0xCC]);
    assert_eq!(g.mfc2(6), 0xCC_BB_AA_99);
}

#[test]
fn mtc2_sxyp_pushes_through_fifo() {
    let mut g = Gte::new();
    // Write three values via SXYP (cop2cr15) - older entries shift down.
    let a = pack_i16_lo_hi(10, 20);
    let b = pack_i16_lo_hi(30, 40);
    let c = pack_i16_lo_hi(50, 60);
    g.mtc2(15, a);
    g.mtc2(15, b);
    g.mtc2(15, c);
    // After three pushes, FIFO is [a, b, c] in slot 0..2.
    assert_eq!(g.sxy_fifo[0], ScreenXY::new(10, 20));
    assert_eq!(g.sxy_fifo[1], ScreenXY::new(30, 40));
    assert_eq!(g.sxy_fifo[2], ScreenXY::new(50, 60));
}

#[test]
fn ctc2_then_cfc2_round_trips_rotation_row() {
    let mut g = Gte::new();
    let val = pack_i16_lo_hi(2048, -1024);
    g.ctc2(0, val); // RT11RT12
    assert_eq!(g.rot.m[0][0], 2048);
    assert_eq!(g.rot.m[0][1], -1024);
    assert_eq!(g.cfc2(0), val);
}

#[test]
fn ctc2_then_cfc2_round_trips_translation_z() {
    let mut g = Gte::new();
    g.ctc2(7, 0x1234_5678); // TRZ
    assert_eq!(g.trans.z, 0x1234_5678u32 as i32);
    assert_eq!(g.cfc2(7), 0x1234_5678);
}

#[test]
fn ctc2_h_writes_low_16_bits() {
    let mut g = Gte::new();
    g.ctc2(26, 0xDEAD_0140); // H is 16-bit; only 0x0140 lands.
    assert_eq!(g.h, 0x0140);
    assert_eq!(g.cfc2(26), 0x0140);
}

#[test]
fn ctc2_zsf3_sign_extends_to_i32() {
    let mut g = Gte::new();
    g.ctc2(29, 0xFFFFu32); // -1 in low half
    assert_eq!(g.zsf3, -1);
    // cfc2 returns the low 16 bits.
    assert_eq!(g.cfc2(29), 0xFFFF);
}

#[test]
fn lwc2_loads_packed_vertex_xy_through_memory() {
    let mut g = Gte::new();
    let mut mem = VecMem::new(1024);
    // Stage V0.x=100, V0.y=-50 at addr 0x40.
    mem.write_u32_at(0x40, pack_i16_lo_hi(100, -50));
    g.lwc2(&mut mem, 0, 0x40);
    assert_eq!(g.v[0].x, 100);
    assert_eq!(g.v[0].y, -50);
}

#[test]
fn swc2_stores_data_register_to_memory() {
    let mut g = Gte::new();
    let mut mem = VecMem::new(1024);
    g.write_data(6, 0x11_22_33_44); // RGBC bytes
    g.swc2(&mut mem, 6, 0x80);
    assert_eq!(&mem.bytes[0x80..0x84], &[0x44, 0x33, 0x22, 0x11]);
}

#[test]
fn lwc2_swc2_round_trips_one_vertex() {
    let mut g = Gte::new();
    let mut mem = VecMem::new(1024);
    g.write_data(0, pack_i16_lo_hi(7, -8));
    g.write_data(1, sign_extend_i16(9));
    g.swc2(&mut mem, 0, 0x10);
    g.swc2(&mut mem, 1, 0x14);
    // Reset GTE then load back.
    let mut g2 = Gte::new();
    g2.lwc2(&mut mem, 0, 0x10);
    g2.lwc2(&mut mem, 1, 0x14);
    assert_eq!(g2.v[0].x, 7);
    assert_eq!(g2.v[0].y, -8);
    assert_eq!(g2.v[0].z, 9);
}

#[test]
fn load_vertices_pulls_three_packed_pairs() {
    let mut g = Gte::new();
    let mut mem = VecMem::new(64);
    // 8 bytes per vertex: u32 xy pair, u32 z (sign-extended low half).
    mem.write_u32_at(0, pack_i16_lo_hi(1, 2));
    mem.write_u32_at(4, sign_extend_i16(3));
    mem.write_u32_at(8, pack_i16_lo_hi(4, 5));
    mem.write_u32_at(12, sign_extend_i16(6));
    mem.write_u32_at(16, pack_i16_lo_hi(7, 8));
    mem.write_u32_at(20, sign_extend_i16(9));
    g.load_vertices(&mut mem, 0);
    assert_eq!(g.v[0], GteVec3::new(1, 2, 3));
    assert_eq!(g.v[1], GteVec3::new(4, 5, 6));
    assert_eq!(g.v[2], GteVec3::new(7, 8, 9));
}

#[test]
fn cycles_charge_per_register_op() {
    let mut g = Gte::new();
    g.reset_cycles();
    g.mtc2(0, 0);
    g.mfc2(0);
    g.ctc2(5, 0);
    g.cfc2(5);
    // 4 transfers at 1 cycle each.
    assert_eq!(g.cycles, 4);
}

#[test]
fn cycles_charge_per_memory_op() {
    let mut g = Gte::new();
    let mut mem = NullMem;
    g.reset_cycles();
    g.lwc2(&mut mem, 0, 0);
    g.swc2(&mut mem, 0, 0);
    // 2 memory transfers at 1 cycle each.
    assert_eq!(g.cycles, 2);
}

#[test]
fn read_data_irgb_packs_5bit_per_channel() {
    let mut g = Gte::new();
    // IR1 = 0x1F << 7 = 0x0F80 (clamps to 0x1F when shifted >>7).
    g.ir1 = 0x0F80;
    g.ir2 = 0x0F80;
    g.ir3 = 0x0F80;
    let v = g.read_data(28); // IRGB
    assert_eq!(v, 0x1F | (0x1F << 5) | (0x1F << 10));
}

#[test]
fn read_data_lzcr_counts_leading_zeros_for_positive_lzcs() {
    let mut g = Gte::new();
    g.lzcs = 1; // 0x00000001 has 31 leading zeros.
    assert_eq!(g.read_data(31), 31);
}

#[test]
fn read_data_lzcr_counts_leading_ones_for_negative_lzcs() {
    let mut g = Gte::new();
    g.lzcs = -1i32; // 0xFFFFFFFF has 32 leading ones.
    assert_eq!(g.read_data(31), 32);
}

#[test]
fn write_data_lzcs_caches_source_for_lzcr_read() {
    let mut g = Gte::new();
    // Round-trip via MTC2: writing 0x4000_0000 to LZCS gives LZCR = 1.
    g.mtc2(30, 0x4000_0000);
    assert_eq!(g.read_data(31), 1);
}

#[test]
fn ctc2_flag_is_writeable_for_capture_replay() {
    let mut g = Gte::new();
    // Flag is normally set by the GTE itself, but engines replaying a
    // captured trace need to write the FLAG through CTC2 to reproduce
    // the post-instruction state.
    g.ctc2(31, flag_bits::IR1_SATURATED | flag_bits::ANY_ERROR);
    assert_eq!(g.flag, flag_bits::IR1_SATURATED | flag_bits::ANY_ERROR);
    assert_eq!(g.cfc2(31), flag_bits::IR1_SATURATED | flag_bits::ANY_ERROR);
}

#[test]
fn lwc2_into_v0_then_rtps_matches_direct_setup() {
    // Set up two GTEs identically - one via direct `g.v[0] = ...`, one
    // via LWC2 from memory - and verify RTPS produces the same SXY.
    let mut g_direct = Gte::new();
    g_direct.set_viewport(320, 240);
    g_direct.trans = GteVec3::new(0, 0, ROT_ONE * 1024);
    g_direct.v[0] = GteVec3::new(0, 0, 0);
    let xy_direct = g_direct.rtps();

    let mut g_mem = Gte::new();
    g_mem.set_viewport(320, 240);
    g_mem.trans = GteVec3::new(0, 0, ROT_ONE * 1024);
    let mut mem = VecMem::new(64);
    mem.write_u32_at(0, pack_i16_lo_hi(0, 0));
    mem.write_u32_at(4, sign_extend_i16(0));
    g_mem.lwc2(&mut mem, 0, 0); // V0.xy
    g_mem.lwc2(&mut mem, 1, 4); // V0.z
    let xy_mem = g_mem.rtps();

    assert_eq!(xy_direct, xy_mem);
}

#[test]
fn null_mem_returns_zero_loads_and_drops_stores() {
    let mut g = Gte::new();
    let mut mem = NullMem;
    g.lwc2(&mut mem, 6, 0x100);
    assert_eq!(g.rgbc, [0; 4]);
    // Stores are silently dropped.
    g.write_data(6, 0xDEAD_BEEF);
    g.swc2(&mut mem, 6, 0x100); // no panic
}

// ---------------------------------------------------------------------------
// Real-COP2 oracle: cross-check `Gte::rtpt` against a live PS1 execution.
//
// The in-repo `reference_rtps` sweep validates the UNR divide against a second
// clean-room implementation, but both share this crate's scale conventions, so
// a convention shared by BOTH is invisible to it. This test closes that gap by
// replaying real GTE RTPT (func 0x30) traffic captured from a Beetle-validated
// PS1 static recompilation's COP2 register file (its `gte.cpp` gte_divide is the
// same UNR algorithm this crate ports). Each capture record carries the inputs
// (RT, TR, H, OFX/OFY, V0/V1/V2) and the resulting SXY/SZ FIFOs + FLAG.
//
// Sony-free: the capture holds game-derived vertex/matrix bytes, so it lives
// OUTSIDE the repo and is supplied via `LEGAIA_RECOMP_GTE_CAPTURE` (a path to a
// JSON array of ring-dump entries). With the var unset the test skip-passes,
// exactly like the disc-gated integration tests - CI never needs the capture.
//
// Comparison is restricted to the hardware-scale-comparable subset (this crate
// holds MAC/IR in q19.12): the SZ FIFO, the SXY FIFO, and the FLAG bits
// {DIVIDE_OVERFLOW, SZ3_OTZ, SX2, SY2}. IR/MAC-saturation bits and ANY_ERROR
// diverge by the documented scale convention and are excluded.
//
// This crate saturates the SXY FIFO to the hardware signed-11-bit screen range
// (-0x400..=+0x3FF), so SXY value + SX2/SY2 flag are bit-exact against real
// COP2 on EVERY record, on- or off-screen. (A prior i16 SXY clamp - shared by
// this crate and its `reference_rtps`, hence invisible to the self-consistent
// in-repo sweep - diverged off-screen; this capture oracle is what surfaced and
// fixed it.) The SZ FIFO (the divide's own depth) is likewise checked on every
// record.

#[cfg(test)]
fn recomp_arr_i64(v: &serde_json::Value, key: &str) -> Vec<i64> {
    v[key]
        .as_array()
        .unwrap_or_else(|| panic!("capture entry missing array field {key}"))
        .iter()
        .map(|x| x.as_i64().expect("capture array element not an integer"))
        .collect()
}

#[test]
fn rtpt_matches_recomp_cop2_capture() {
    let path = match std::env::var("LEGAIA_RECOMP_GTE_CAPTURE") {
        Ok(p) if !p.is_empty() => p,
        _ => {
            eprintln!(
                "SKIP rtpt_matches_recomp_cop2_capture: set LEGAIA_RECOMP_GTE_CAPTURE \
                 to an external ring-dump JSON to run the real-COP2 cross-check"
            );
            return;
        }
    };
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read capture {path}: {e}"));
    let entries: Vec<serde_json::Value> =
        serde_json::from_str(&raw).expect("capture is not a JSON array of ring entries");

    const SUBSET: u32 = flag_bits::DIVIDE_OVERFLOW
        | flag_bits::SZ3_OTZ_SATURATED
        | flag_bits::SX2_SATURATED
        | flag_bits::SY2_SATURATED;

    let (mut processed, mut sz_checked, mut strict_checked, mut div_ovf_seen) = (0, 0, 0, 0);

    for e in &entries {
        // Only RTPT (func 0x30): all three SXY/SZ slots come from this op.
        let cmd = i64::from_str_radix(
            e["cmd"].as_str().unwrap_or("0x0").trim_start_matches("0x"),
            16,
        )
        .unwrap_or(0);
        if cmd & 0x3F != 0x30 {
            continue;
        }
        processed += 1;

        let rt = recomp_arr_i64(e, "RT"); // 9, row-major, i16 q3.12
        let tr = recomp_arr_i64(e, "TR"); // 3, raw control-reg integer scale
        let h = e["H"].as_i64().expect("H") as i32;
        let ofx = e["OFX"].as_i64().expect("OFX") as i32;
        let ofy = e["OFY"].as_i64().expect("OFY") as i32;
        let vs = [
            recomp_arr_i64(e, "V0"),
            recomp_arr_i64(e, "V1"),
            recomp_arr_i64(e, "V2"),
        ];

        // Map the recomp's hardware-scale inputs into this crate's units:
        //   rot: i16 q3.12 verbatim.
        //   vertex: << 12 so `rot.mul_vec` (which >>12's the product) yields the
        //           rotated vector in q19.12 (this crate's MAC scale).
        //   trans: << 12 -> q19.12.
        let rot = GteMat3 {
            m: [
                [rt[0] as i16, rt[1] as i16, rt[2] as i16],
                [rt[3] as i16, rt[4] as i16, rt[5] as i16],
                [rt[6] as i16, rt[7] as i16, rt[8] as i16],
            ],
        };
        let mut g = Gte::new();
        g.rot = rot;
        g.trans = GteVec3::new(
            (tr[0] << ROT_FRAC_BITS) as i32,
            (tr[1] << ROT_FRAC_BITS) as i32,
            (tr[2] << ROT_FRAC_BITS) as i32,
        );
        g.h = h;
        // Retail OFX/OFY are s15.16 with zero fractional; this crate stores the
        // integer pixel offset.
        g.ofx = ofx >> 16;
        g.ofy = ofy >> 16;
        for (i, v) in vs.iter().enumerate() {
            g.v[i] = GteVec3::new(
                (v[0] << ROT_FRAC_BITS) as i32,
                (v[1] << ROT_FRAC_BITS) as i32,
                (v[2] << ROT_FRAC_BITS) as i32,
            );
        }
        g.rtpt();

        // Expected outputs from the real COP2 register file.
        let exp_s = [
            recomp_arr_i64(e, "S0"),
            recomp_arr_i64(e, "S1"),
            recomp_arr_i64(e, "S2"),
        ];
        let exp_sz = recomp_arr_i64(e, "SZ"); // [SZ1, SZ2, SZ3] = V0.z, V1.z, V2.z
        let exp_flag = u32::from_str_radix(
            e["FLAG"].as_str().unwrap_or("0x0").trim_start_matches("0x"),
            16,
        )
        .unwrap_or(0);

        // SZ FIFO: the divide's own depth bucket. Checked on EVERY record - this
        // is the perspective-divide input and it must be bit-exact.
        assert_eq!(
            [
                g.sz_fifo[1] as i64,
                g.sz_fifo[2] as i64,
                g.sz_fifo[3] as i64
            ],
            [exp_sz[0], exp_sz[1], exp_sz[2]],
            "SZ FIFO mismatch (seq {:?})",
            e["seq"]
        );
        sz_checked += 1;

        // Strict SXY + flag-subset check on EVERY record. The SXY FIFO now
        // saturates to the hardware ±0x400 screen range (SX2/SY2 flags and the
        // clamped value both match), so on- and off-screen verts are bit-exact.
        for (i, s) in exp_s.iter().enumerate() {
            assert_eq!(
                (g.sxy_fifo[i].x as i64, g.sxy_fifo[i].y as i64),
                (s[0], s[1]),
                "SXY[{i}] mismatch (seq {:?})",
                e["seq"]
            );
        }
        assert_eq!(
            g.flag & SUBSET,
            exp_flag & SUBSET,
            "FLAG subset mismatch (seq {:?})",
            e["seq"]
        );
        strict_checked += 1;
        if exp_flag & flag_bits::DIVIDE_OVERFLOW != 0 {
            div_ovf_seen += 1;
        }
    }

    eprintln!(
        "rtpt_matches_recomp_cop2_capture: processed={processed} sz_checked={sz_checked} \
         strict={strict_checked} div_overflow_exercised={div_ovf_seen}"
    );
    assert!(
        processed >= 8,
        "capture had too few RTPT records ({processed}) to be a meaningful oracle"
    );
    assert!(
        strict_checked > 0,
        "no on-screen RTPT records to strictly cross-check"
    );
}

// --- FUN_8001CF50 camera view-rotation build ---------------------------

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

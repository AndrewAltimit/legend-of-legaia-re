//! NCLIP / AVSZ, screen clamps, the triangle rasteriser and the software near-plane clip helpers.

use super::*;

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

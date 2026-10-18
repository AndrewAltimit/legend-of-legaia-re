//! The ground coverage field ([`FogGround`]) over synthetic meshes.

use super::*;

/// Two triangles: an axis-aligned square `lo..hi` on X / Z at height `y`
/// (retail Y-down).
fn square(lo: [f32; 2], hi: [f32; 2], y: f32) -> [[[f32; 3]; 3]; 2] {
    let a = [lo[0], y, lo[1]];
    let b = [hi[0], y, lo[1]];
    let c = [hi[0], y, hi[1]];
    let d = [lo[0], y, hi[1]];
    [[a, b, c], [a, c, d]]
}

/// A field rasterised from `tris` over the rectangle `lo..hi`.
fn field(tris: &[[[f32; 3]; 3]], lo: [f32; 2], hi: [f32; 2], walk: Option<f32>) -> FogGround {
    let mut b = FogGroundBuilder::new(lo, hi);
    for t in tris {
        b.add_triangle(t);
    }
    b.finish(|_, _| walk)
}

/// A 1024-unit plateau at `y = -200` in the middle of a 3072-unit field.
fn plateau() -> FogGround {
    let tris = square([1024.0, 1024.0], [2048.0, 2048.0], -200.0);
    field(&tris, [0.0, 0.0], [3072.0, 3072.0], None)
}

#[test]
fn a_plateau_is_covered_inside_and_open_outside() {
    let f = plateau();
    // 1024 / 64 = 16 cells a side.
    assert_eq!(f.covered_count(), 16 * 16);
    assert!(f.covered_at(1536.0, 1536.0));
    assert!(f.covered_at(1040.0, 2040.0));
    assert!(!f.covered_at(1000.0, 1536.0));
    assert!(!f.covered_at(500.0, 500.0));
    let i = f.cell_index(1536.0, 1536.0).unwrap();
    assert_eq!(f.height[i], -200.0);
}

#[test]
fn the_edge_distance_is_signed_and_grows_away_from_the_rim() {
    let f = plateau();
    // Centre: half the plateau from the rim.
    let centre = f.dist_at(1536.0, 1536.0);
    assert!((centre - 512.0).abs() < 70.0, "centre {centre}");
    // At the rim, about zero.
    let rim = f.dist_at(1024.0, 1536.0);
    assert!(rim.abs() < 40.0, "rim {rim}");
    // Outside, negative and growing.
    let near = f.dist_at(1024.0 - 128.0, 1536.0);
    let far = f.dist_at(1024.0 - 512.0, 1536.0);
    assert!(near < 0.0 && far < near, "near {near} far {far}");
    assert!((near + 128.0).abs() < 40.0, "near {near}");
    // Monotone from the centre out across the rim.
    let mut last = f32::INFINITY;
    for k in 0..24 {
        let d = f.dist_at(1536.0 - k as f32 * 64.0, 1536.0);
        assert!(d <= last + 1.0e-3, "step {k}: {d} after {last}");
        last = d;
    }
}

#[test]
fn the_sheet_lies_on_the_floor_and_falls_off_the_rim() {
    let f = plateau();
    let on = f.sheet_at(1536.0, 1536.0).unwrap();
    assert_eq!((on.y, on.weight, on.spill), (-200.0, 1.0, false));

    // Just past the rim: spill, a little lower (Y-down: larger), most of
    // the bank still with it.
    let lip = f.sheet_at(1024.0 - 96.0, 1536.0).unwrap();
    assert!(lip.spill);
    assert!(
        lip.y > -200.0 && lip.y < -200.0 + 0.2 * SPILL_DEPTH,
        "{lip:?}"
    );
    assert!(lip.weight > 0.4, "{lip:?}");

    // Further out: lower and thinner.
    let deep = f.sheet_at(1024.0 - 320.0, 1536.0).unwrap();
    assert!(
        deep.spill && deep.y > lip.y && deep.weight < lip.weight,
        "{deep:?}"
    );

    // Past the reach: no bank at all.
    assert!(f.sheet_at(1024.0 - SPILL_REACH - 128.0, 1536.0).is_none());
    assert!(f.sheet_at(100.0, 100.0).is_none());
}

#[test]
fn the_texels_carry_coverage_spill_and_flow() {
    let f = plateau();
    let tex = f.texels();
    assert_eq!(tex.len(), f.w * f.h * 4);
    let px = |x: f32, z: f32| {
        let i = f.cell_index(x, z).unwrap() * 4;
        [tex[i], tex[i + 1], tex[i + 2], tex[i + 3]]
    };
    assert_eq!(px(1536.0, 1536.0), [255, 0, 0, 255]);
    let lip = px(1024.0 - 96.0, 1536.0);
    assert!(lip[0] < 128 && lip[1] > 64 && lip[2] > 0, "{lip:?}");
    assert_eq!(px(100.0, 100.0), [0, 0, 255, 255]);
}

#[test]
fn an_enclosed_hole_is_covered_and_takes_the_floor_height() {
    // A square ring: the middle 512-unit square has no geometry at all.
    let mut tris = Vec::new();
    for (lo, hi) in [
        ([1024.0, 1024.0], [2048.0, 1280.0]),
        ([1024.0, 1792.0], [2048.0, 2048.0]),
        ([1024.0, 1280.0], [1280.0, 1792.0]),
        ([1792.0, 1280.0], [2048.0, 1792.0]),
    ] {
        tris.extend(square(lo, hi, -100.0));
    }
    let f = field(&tris, [0.0, 0.0], [3072.0, 3072.0], None);
    assert!(f.covered_at(1536.0, 1536.0), "the pocket is not open air");
    let i = f.cell_index(1536.0, 1536.0).unwrap();
    assert_eq!(f.height[i], -100.0);
    assert!(f.sheet_at(1536.0, 1536.0).is_some_and(|p| !p.spill));
}

#[test]
fn a_one_cell_seam_is_not_a_crack_to_the_sky() {
    // Two slabs with a 64-unit gap between them running off the field.
    let mut tris = Vec::new();
    tris.extend(square([0.0, 0.0], [1500.0, 3072.0], 0.0));
    tris.extend(square([1564.0, 0.0], [3072.0, 3072.0], 0.0));
    let f = field(&tris, [0.0, 0.0], [3072.0, 3072.0], None);
    assert!(f.covered_at(1532.0, 1536.0), "the seam closes");
}

#[test]
fn a_roof_is_solid_but_not_floor() {
    // Ground at y = 0 everywhere, a roof 300 above it over the middle; the
    // walk floor reads 0.
    let mut tris = Vec::new();
    tris.extend(square([0.0, 0.0], [3072.0, 3072.0], 0.0));
    tris.extend(square([1024.0, 1024.0], [2048.0, 2048.0], -300.0));
    let f = field(&tris, [0.0, 0.0], [3072.0, 3072.0], Some(0.0));
    let i = f.cell_index(1536.0, 1536.0).unwrap();
    assert_eq!(f.height[i], 0.0, "the ground under the roof, not the roof");

    // A roof with nothing under it (hedge tops, a canopy) is still solid:
    // it is covered at the surrounding floor's height, not open air.
    let mut tris = Vec::new();
    tris.extend(square([512.0, 512.0], [1024.0, 2560.0], 0.0));
    tris.extend(square([1024.0, 512.0], [2560.0, 2560.0], -300.0));
    let g = field(&tris, [0.0, 0.0], [3072.0, 3072.0], Some(0.0));
    assert!(g.covered_at(1800.0, 1536.0));
    let i = g.cell_index(1800.0, 1536.0).unwrap();
    assert_eq!(g.height[i], 0.0);
}

#[test]
fn walls_are_not_floor() {
    // A vertical wall quad standing on X = 1536.
    let wall = [
        [
            [1536.0, 0.0, 0.0],
            [1536.0, -400.0, 0.0],
            [1536.0, -400.0, 3072.0],
        ],
        [
            [1536.0, 0.0, 0.0],
            [1536.0, -400.0, 3072.0],
            [1536.0, 0.0, 3072.0],
        ],
    ];
    let f = field(&wall, [0.0, 0.0], [3072.0, 3072.0], None);
    assert_eq!(f.covered_count(), 0);
}

#[test]
fn the_bank_lies_on_the_field_and_spills_off_it() {
    let mut v = FogVolume::new();
    v.set_style(Some(POOL_STYLE));
    v.strength = 1.0;
    v.set_ground_field("", Some(plateau()));
    // The walk grid reads a flat floor at 0 everywhere - what the field
    // overrides.
    v.recentre([1536.0, 1536.0], |_, _| 0.0);
    let n = MESH_DIM + 1;
    let mc = v.space.mesh_cell();
    let vert = |x: f32, z: f32| {
        let ix = (x / mc) as i32 - v.mesh_origin[0];
        let iz = (z / mc) as i32 - v.mesh_origin[1];
        let i = iz as usize * n + ix as usize;
        (v.ground[i], v.ground_weight[i])
    };
    let (y, w) = vert(1536.0, 1536.0);
    assert_eq!((y, w), (-200.0, 1.0), "on the plateau");
    let (y, w) = vert(1024.0 - 128.0, 1536.0);
    assert!(y > -200.0 && w > 0.2, "spilling off the west rim: {y} {w}");
    let (_, w) = vert(128.0, 128.0);
    assert_eq!(w, 0.0, "nothing over the open air");

    // The frame carries the texture; the toggle's reset keeps the field.
    let f = v.frame(None, None).unwrap();
    let g = f.ground_tex.expect("ground texture");
    assert_eq!(g.texels.len(), (g.w * g.h * 4) as usize);
    assert_ne!(f.ground_uv_rect(), [0.0; 4]);
    v.reset();
    assert!(v.ground_field().is_some());

    // Another scene's field is not this one's.
    v.scene = "elsewhere".into();
    assert!(v.ground_field().is_none());
}

#[test]
fn without_a_field_the_bank_is_unchanged() {
    let mut a = FogVolume::new();
    let mut b = FogVolume::new();
    for v in [&mut a, &mut b] {
        v.set_style(Some(POOL_STYLE));
        v.strength = 1.0;
    }
    b.set_ground_field("", None);
    let floor = |x: f32, _: f32| if x < 500.0 { -50.0 } else { 0.0 };
    a.recentre([0.0, 0.0], floor);
    b.recentre([0.0, 0.0], floor);
    assert_eq!(a.ground, b.ground);
    assert_eq!(a.ground_weight, b.ground_weight);
    let f = a.frame(None, None).unwrap();
    assert!(f.ground_tex.is_none());
    assert_eq!(f.ground_uv_rect(), [0.0; 4]);
}

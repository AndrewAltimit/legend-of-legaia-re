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
    // the spill's share of the bank still with it - and never more than
    // that share.
    let lip = f.sheet_at(1024.0 - 64.0, 1536.0).unwrap();
    assert!(lip.spill && !lip.haze);
    assert!(
        lip.y > -200.0 && lip.y < -200.0 + 0.2 * SPILL_DEPTH,
        "{lip:?}"
    );
    assert!(
        lip.weight > 0.8 * SPILL_SHARE && lip.weight <= SPILL_SHARE,
        "{lip:?}"
    );

    // Further out: lower and thinner.
    let deep = f.sheet_at(1024.0 - 200.0, 1536.0).unwrap();
    assert!(
        deep.spill && !deep.haze && deep.y > lip.y && deep.weight < lip.weight,
        "{deep:?}"
    );
    // The fall is near-vertical: most of the depth inside the short reach.
    assert!(deep.y - -200.0 > 0.5 * SPILL_DEPTH, "{deep:?}");

    // Past the reach: the faint distant haze, flat far below the rim.
    let near = f.sheet_at(1024.0 - SPILL_REACH - 128.0, 1536.0).unwrap();
    let far = f.sheet_at(100.0, 100.0).unwrap();
    for p in [near, far] {
        assert!(p.haze && p.spill, "{p:?}");
        assert_eq!(p.y, -200.0 + HAZE_DEPTH);
        assert!(p.weight <= HAZE_WEIGHT, "{p:?}");
    }
    assert!(near.weight < far.weight, "{near:?} {far:?}");

    // Past the field's edge: still the haze, off the nearest rim.
    let beyond = f.sheet_at(-500.0, 1536.0).unwrap();
    assert!(beyond.haze && beyond.y == -200.0 + HAZE_DEPTH, "{beyond:?}");
    assert!(
        (beyond.weight - haze_weight(1524.0)).abs() < 0.05,
        "{beyond:?}"
    );
    // A field with no floor carries nothing anywhere.
    let empty = field(&[], [0.0, 0.0], [512.0, 512.0], None);
    assert!(empty.sheet_at(100.0, 100.0).is_none());
    assert!(empty.sheet_at(-900.0, 100.0).is_none());
}

#[test]
fn the_haze_rises_peaks_and_fades_with_distance() {
    assert_eq!(haze_weight(0.0), 0.0);
    assert_eq!(haze_weight(SPILL_REACH), 0.0);
    assert!((haze_weight(HAZE_PEAK) - HAZE_WEIGHT).abs() < 1.0e-6);
    assert_eq!(haze_weight(HAZE_REACH), 0.0);
    let mut last = 0.0;
    for k in 0..=16 {
        let out = SPILL_REACH + (HAZE_PEAK - SPILL_REACH) * k as f32 / 16.0;
        let w = haze_weight(out);
        assert!(w >= last, "rising at {out}: {w} after {last}");
        last = w;
    }
}

#[test]
fn the_haze_never_lies_on_a_surface_above_it() {
    // A low ledge 600 below the plateau, out to the west: the haze plane
    // (HAZE_DEPTH below the rim) lies under it, so it carries no haze there.
    let mut tris = square([1024.0, 1024.0], [2048.0, 2048.0], -200.0).to_vec();
    // Steep wall faces only (not floor), so the ledge is drawn but solid
    // ground does not grow under it.
    let ledge_y = -200.0 + 600.0;
    tris.push([
        [200.0, ledge_y, 1400.0],
        [600.0, ledge_y, 1400.0],
        [400.0, ledge_y + 400.0, 1400.0],
    ]);
    let f = field(&tris, [0.0, 0.0], [3072.0, 3072.0], None);
    let under = f.sheet_at(400.0, 1400.0).unwrap();
    assert!(under.haze && under.weight == 0.0, "{under:?}");
    let clear = f.sheet_at(400.0, 2600.0).unwrap();
    assert!(clear.haze && clear.weight > 0.0, "{clear:?}");
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
    assert!(
        y > -200.0 && w > 0.6 * SPILL_SHARE && w <= SPILL_SHARE,
        "spilling off the west rim: {y} {w}"
    );
    // Far out over the open air only the faint haze, far below the rim,
    // flagged by a negative weight.
    let (y, w) = vert(128.0, 128.0);
    assert!(w < 0.0 && -w <= HAZE_WEIGHT, "haze over the open air: {w}");
    assert_eq!(y, -200.0 + HAZE_DEPTH);
    let f = v.frame(None, None).unwrap();
    assert!(f.mesh_positions().chunks(4).any(|p| p[3] < 0.0));

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

#[test]
fn the_header_carries_the_spill_and_haze_constants() {
    let mut v = FogVolume::new();
    v.set_style(Some(POOL_STYLE));
    v.strength = 1.0;
    v.set_ground_field("", Some(plateau()));
    v.recentre([1536.0, 1536.0], |_, _| 0.0);
    let h = v.frame(None, None).unwrap().header();
    assert_eq!(h.len(), header::LEN);
    let s = header::SPILL_CONSTANTS;
    assert_eq!(h[s..s + 4], FOG_SPILL_CONSTANTS);
    let z = header::HAZE_CONSTANTS;
    assert_eq!(h[z..z + 4], FOG_HAZE_CONSTANTS);
}

// The curtain falls near-vertically (deep against a short reach), slowly (a
// tendril crosses one noise cell in seconds, not fractions of one), and the
// haze is a faint, thick layer.
const _: () = assert!(SPILL_DEPTH > 4.0 * SPILL_REACH);
const _: () = assert!(SPILL_FLOW_TICKS >= 300);
const _: () = assert!(FOG_HAZE_CONSTANTS[0] > 1.0 && HAZE_WEIGHT < 0.5);
// What pours over an edge is a wisp, not a second bank.
const _: () = assert!(SPILL_SHARE <= 0.34 && HAZE_WEIGHT <= SPILL_SHARE);

#[test]
fn a_see_through_sheet_is_floor_only_when_level_and_never_a_wall() {
    // A level water sheet over an empty cell: floor.
    let mut b = FogGroundBuilder::new([0.0, 0.0], [512.0, 512.0]);
    b.add_see_through(&[[0.0, -50.0, 0.0], [512.0, -50.0, 0.0], [0.0, -50.0, 512.0]]);
    let g = b.finish(|_, _| None);
    assert!(g.covered_at(100.0, 100.0));
    // A veil hung across the air at 45 degrees: no floor, and nothing the
    // spill drapes over.
    let mut b = FogGroundBuilder::new([0.0, 0.0], [512.0, 512.0]);
    b.add_see_through(&[[0.0, 0.0, 0.0], [512.0, 0.0, 0.0], [0.0, -512.0, 512.0]]);
    let g = b.finish(|_, _| None);
    assert_eq!(g.covered_count(), 0);
    assert!(g.top.iter().all(|t| t.is_infinite()));
}

#[test]
fn a_diagonal_rim_reads_as_one_straight_edge() {
    // A square plateau turned 37 degrees about (1536, 1536): its rims run
    // diagonal to the cell grid, so the raw cell distance steps once per
    // cell along each of them.
    let (c, r, y) = (1536.0f32, 600.0f32, -200.0);
    let a = 37.0f32.to_radians();
    let corner = |k: f32| {
        let t = a + k * std::f32::consts::FRAC_PI_2;
        [c + r * t.cos(), y, c + r * t.sin()]
    };
    let q = [corner(0.0), corner(1.0), corner(2.0), corner(3.0)];
    let f = field(
        &[[q[0], q[1], q[2]], [q[0], q[2], q[3]]],
        [0.0, 0.0],
        [3072.0, 3072.0],
        None,
    );
    // Walk just past the rim from corner 0 to corner 1 (where the lip of
    // the spill reads it) and take the spill's outward direction as the
    // shaders do - the distance gradient over a cell either side. It must
    // hold to the rim's normal, not swing once per cell step.
    let (p0, p1) = (q[0], q[1]);
    let mid = [(p0[0] + p1[0]) * 0.5 - c, (p0[2] + p1[2]) * 0.5 - c];
    let ml = (mid[0] * mid[0] + mid[1] * mid[1]).sqrt();
    let normal = [mid[0] / ml, mid[1] / ml];
    let mut worst = 0.0f32;
    for k in 0..40 {
        let t = 0.25 + 0.5 * k as f32 / 39.0;
        let x = p0[0] + (p1[0] - p0[0]) * t + normal[0] * 24.0;
        let z = p0[2] + (p1[2] - p0[2]) * t + normal[1] * 24.0;
        // Outward is where the distance falls.
        let gx = f.dist_at(x - 64.0, z) - f.dist_at(x + 64.0, z);
        let gz = f.dist_at(x, z - 64.0) - f.dist_at(x, z + 64.0);
        let len = (gx * gx + gz * gz).sqrt();
        let cos = (gx * normal[0] + gz * normal[1]) / len;
        worst = worst.max(cos.clamp(-1.0, 1.0).acos().to_degrees());
    }
    // Unsmoothed it swings by over 25 degrees; what is left is the turned
    // square's corners reaching into the walk.
    assert!(
        worst < 12.0,
        "the outward direction swings {worst} degrees along the rim"
    );
}

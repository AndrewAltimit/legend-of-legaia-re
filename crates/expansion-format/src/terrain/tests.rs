use super::*;
use crate::map::MapDef;

fn map(extra: &str) -> MapDef {
    let src = format!(
        r#"
name = "t"
base = {{ scene = {{ kind = "scene", name = "town01" }} }}
floor_heights = [0, 64]
[tiles]
origin = [4, 4]
rows = [
  "..........",
  "..........",
  "....^^....",
  "....^^....",
  "..........",
  "..........",
]
[tiles.legend]
"." = {{ ground = {{ kind = "texture", tpage = 12, clut = 32005, tile = 38 }} }}
"^" = {{ height = 1, ground = {{ kind = "texture", tpage = 12, clut = 32000, tile = 9 }} }}
{extra}
"#
    );
    toml::from_str(&src).expect("parses")
}

#[test]
fn no_terrain_table_builds_nothing() {
    let m = map("");
    assert!(m.terrain_heightfield().unwrap().is_none());
    let g = m.tile_grid().unwrap();
    assert!(m.terrain_walls(&g).unwrap().iter().all(|w| *w == 0));
}

#[test]
fn the_base_rises_from_the_tiers_and_the_field_spans_the_skirt() {
    let m = map("[terrain]\nresolution = 4\nsmooth = 0\nskirt = 2");
    let f = m.terrain_heightfield().unwrap().unwrap();
    assert_eq!(f.cols, (10 + 4) * 4 + 1);
    assert_eq!(f.rows, (6 + 4) * 4 + 1);
    assert_eq!(f.step, 32);
    assert_eq!(f.origin, [(4 - 2) * 128, (4 - 2) * 128]);
    // The middle of the raised block stands at its tier; the far corner at 0.
    let mid = f.vertex_of_tile(5);
    assert_eq!(f.at(mid, f.vertex_of_tile(3)), 64);
    assert_eq!(f.at(0, 0), 0);
}

#[test]
fn the_sampler_reads_the_drawn_triangles() {
    let m =
        map("[terrain]\nsmooth = 1\nshapes = [{ kind = \"noise\", amplitude = 30, scale = 1.5 }]");
    let f = m.terrain_heightfield().unwrap().unwrap();
    for j in 0..f.rows - 1 {
        for i in 0..f.cols - 1 {
            let (x, z) = (
                f.origin[0] + i as i32 * f.step,
                f.origin[1] + j as i32 * f.step,
            );
            // Vertices sample exactly.
            assert_eq!(
                f.sample_world(x, z),
                Some(i32::from(f.at(i as isize, j as isize)))
            );
            // The quad's centre lies on the b-c diagonal: the mean of b and c.
            let (b, c) = (
                f32::from(f.at(i as isize + 1, j as isize)),
                f32::from(f.at(i as isize, j as isize + 1)),
            );
            let got = f.sample_world(x + f.step / 2, z + f.step / 2).unwrap();
            assert!((got as f32 - (b + c) / 2.0).abs() <= 1.0, "quad ({i}, {j})");
        }
    }
    assert_eq!(f.sample_world(f.origin[0] - 1, f.origin[1]), None);
}

#[test]
fn a_cliff_walls_its_face_and_leaves_the_tops_walkable() {
    let m = map(
        "[terrain]\nsmooth = 0\nshapes = [{ kind = \"plateau\", rect = [0, 0, 9, 1], height = 300, edge = 0.3 }]",
    );
    let g = m.tile_grid().unwrap();
    let w = m.terrain_walls(&g).unwrap();
    // Row 2 holds the face: its top quarters are wall.
    let face = w[2 * g.width + 2];
    assert_ne!(face & 0b0011, 0, "face quarters {face:04b}");
    // The plateau's top and the low ground stay open.
    assert_eq!(w[g.width + 7], 0);
    assert_eq!(w[5 * g.width + 1], 0);
}

#[test]
fn a_gentle_slope_stays_walkable() {
    let m = map(
        "[terrain]\nsmooth = 2\nshapes = [{ kind = \"ramp\", from = [0.0, 3.0], to = [10.0, 3.0], radius = 4, height = [0, 100] }]",
    );
    let g = m.tile_grid().unwrap();
    assert!(m.terrain_walls(&g).unwrap().iter().all(|w| *w == 0));
}

#[test]
fn deep_water_is_a_wall_and_the_shallows_wade() {
    let m = map(
        "[terrain]\nsmooth = 0\nwater = { level = 10, wade = 6, cells = [{ tpage = 9, clut = 31880, tile = 56 }] }\nshapes = [{ kind = \"plateau\", rect = [0, 4, 9, 5], height = -60, edge = 1.0 }]",
    );
    let g = m.tile_grid().unwrap();
    let w = m.terrain_walls(&g).unwrap();
    assert_eq!(w[5 * g.width + 3], 0xF, "the deep row");
    // Ground at 0 under 10 of water wades... deeper than 6: wall.
    assert_eq!(w[g.width + 1], 0xF);
    let m = map(
        "[terrain]\nsmooth = 0\nwater = { level = 4, wade = 6, cells = [{ tpage = 9, clut = 31880, tile = 56 }] }",
    );
    let w = m.terrain_walls(&g).unwrap();
    assert_eq!(w[g.width + 1], 0, "four units deep wades");
}

#[test]
fn materials_come_from_chars_and_style_grounds_and_dither_at_the_boundary() {
    let m = map(
        "[terrain]\nblend = 0.6\nmaterials = [{ name = \"grass\", chars = \"^\", cells = [{ tpage = 12, clut = 32001, tile = 1 }, { tpage = 12, clut = 32000, tile = 9 }], tint = [120, 140, 110] }]",
    );
    let s = m.terrain_surface().unwrap().unwrap();
    assert_eq!(s.materials.len(), 2, "grass + the '.' ground");
    assert_eq!(s.materials[1].cells[0].tile, 38);
    let qc = s.field.cols - 1;
    let used: std::collections::BTreeSet<u16> = s
        .paint
        .iter()
        .flatten()
        .flatten()
        .map(|p| p.material)
        .collect();
    assert_eq!(used.len(), 2);
    // Deep inside the sand field every quad is sand.
    assert_eq!(s.paint[2 * qc + 2][0].unwrap().material, 1);
    // Rotation is per tile: every quad of one tile shares it.
    let tile_quads: Vec<u8> = (0..4)
        .flat_map(|j| (0..4).map(move |i| (i, j)))
        .map(|(i, j)| s.paint[j * qc + i][1].unwrap().orient)
        .collect();
    assert!(tile_quads.iter().all(|o| *o == tile_quads[0]));
    // The same file paints the same surface.
    assert_eq!(s, m.terrain_surface().unwrap().unwrap());
}

#[test]
fn the_sun_shades_a_slope_facing_it_brighter_than_one_facing_away() {
    let m = map(
        "[terrain]\nshading = 0.8\nvariation = 0\nsun = [90, 40]\nshapes = [{ kind = \"raise\", at = [5.0, 3.0], radius = 3, height = 120 }]",
    );
    let s = m.terrain_surface().unwrap().unwrap();
    let f = &s.field;
    let j = f.vertex_of_tile(3) as usize;
    let east = s.colors[j * f.cols + f.vertex_of_tile(6) as usize][0];
    let west = s.colors[j * f.cols + f.vertex_of_tile(4) as usize][0];
    assert!(east > west + 10, "east {east} west {west}");
}

#[test]
fn orient_uv_covers_the_eight_symmetries() {
    let set: std::collections::BTreeSet<(i32, i32)> = (0..8)
        .map(|o| {
            let (u, v) = orient_uv(o, 0.25, 0.0);
            ((u * 4.0) as i32, (v * 4.0) as i32)
        })
        .collect();
    assert_eq!(set.len(), 8);
}

#[test]
fn a_boundary_softens_with_the_runner_up_and_soften_zero_turns_it_off() {
    let mats = "materials = [{ name = \"grass\", chars = \"^\", cells = [{ tpage = 12, clut = 32001, tile = 1 }] }]";
    let m = map(&format!("[terrain]\nsoften = 0.4\n{mats}"));
    let s = m.terrain_surface().unwrap().unwrap();
    let soft: Vec<_> = s
        .paint
        .iter()
        .flatten()
        .flatten()
        .filter(|p| p.overlay.is_some())
        .collect();
    assert!(!soft.is_empty(), "the boundary softens");
    // The overlay is always the other material.
    for p in &soft {
        let (cell, _) = p.overlay.unwrap();
        assert_ne!(cell, p.cell);
    }
    let m = map(&format!("[terrain]\nsoften = 0\n{mats}"));
    let s = m.terrain_surface().unwrap().unwrap();
    assert!(
        s.paint
            .iter()
            .flatten()
            .flatten()
            .all(|p| p.overlay.is_none())
    );
}

#[test]
fn the_surf_rolls_in_deepest_band_first() {
    // Find each band's brightest frame over one wave.
    let peak = |band| {
        (0..150u64)
            .max_by_key(|&f| (surf_level(f, 150, band), std::cmp::Reverse(f)))
            .unwrap()
    };
    let deep = peak(FOAM_BANDS - 1);
    let shallow = peak(0);
    assert!(deep < shallow, "deep {deep} shallow {shallow}");
    assert_eq!(surf_level(10, 0, 2), 31, "still surf is full");
    assert!(foam_palette(20)[FOAM_INDEX] & 0x8000 != 0);
}

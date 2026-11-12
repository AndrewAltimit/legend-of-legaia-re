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

#[test]
fn a_stroke_raises_along_its_path_without_adding_up_at_the_joints() {
    let m = map(
        "[terrain]\nsmooth = 0\nshapes = [{ kind = \"stroke\", path = [[1.5, 1.5], [5.0, 1.5], [8.5, 1.5]], radius = 1.0, height = 20 }]",
    );
    let f = m.terrain_heightfield().unwrap().unwrap();
    let at = |u: f32, v: f32| f.at((u * 4.0) as isize, (v * 4.0) as isize);
    // On the path, at a joint and mid-segment alike: the full height.
    assert_eq!(at(5.0, 1.5), 20);
    assert_eq!(at(3.0, 1.5), 20);
    // Off the brush: untouched.
    assert_eq!(at(5.0, 4.5), 0);
    // Flatten along a path pulls the raised block down to the level given.
    let m = map(
        "[terrain]\nsmooth = 0\nshapes = [{ kind = \"stroke\", op = \"flatten\", path = [[4.5, 2.5], [5.5, 3.5]], radius = 2.0, height = 0 }]",
    );
    let f = m.terrain_heightfield().unwrap().unwrap();
    assert!(f.at(5 * 4, 3 * 4) < 64);
}

fn gray_png(w: u32, h: u32, f: impl Fn(u32, u32) -> u8) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().unwrap();
        let data: Vec<u8> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| f(x, y))
            .collect();
        wr.write_image_data(&data).unwrap();
    }
    out
}

#[test]
fn a_heightmap_reads_its_png_from_the_assets() {
    let mut m = map(
        "[terrain]\nsmooth = 0\nshapes = [{ kind = \"heightmap\", image = \"hm/ramp.png\", height = [0, 100] }]",
    );
    // Before its image is read the shape shapes nothing.
    let flat = m.terrain_heightfield().unwrap().unwrap();
    assert_eq!(flat.at(4, 4), 0);
    let mut assets = std::collections::BTreeMap::new();
    assets.insert(
        "hm/ramp.png".to_string(),
        gray_png(11, 2, |x, _| (x * 255 / 10) as u8),
    );
    assert!(m.resolve_terrain_images(&assets).is_empty());
    let f = m.terrain_heightfield().unwrap().unwrap();
    // Black at the map's low-X edge, white at its far edge, linear between.
    assert_eq!(f.at(0, 8), 0);
    assert_eq!(f.at(40, 8), 100);
    assert!((f.at(20, 8) - 50).abs() <= 1);
    // A missing image is reported and shapes nothing.
    assert_eq!(m.resolve_terrain_images(&Default::default()).len(), 1);
    assert_eq!(m.terrain_heightfield().unwrap().unwrap(), flat);
}

#[test]
fn detail_subdivides_only_boundary_quads() {
    let mats = "materials = [{ name = \"a\", chars = \".\", cells = [{ tpage = 1, clut = 1, tile = 1 }] }, { name = \"b\", chars = \"^\", cells = [{ tpage = 2, clut = 2, tile = 2 }] }]";
    let plain = map(&format!("[terrain]\n{mats}"))
        .terrain_surface()
        .unwrap()
        .unwrap();
    assert!(plain.fine.is_empty());
    let fine = map(&format!("[terrain]\ndetail = 4\n{mats}"))
        .terrain_surface()
        .unwrap()
        .unwrap();
    assert!(!fine.fine.is_empty());
    assert!(
        fine.fine.len() < fine.paint.len() / 4,
        "only boundary quads subdivide"
    );
    assert_eq!(fine.detail, 4);
    for subs in fine.fine.values() {
        assert_eq!(subs.len(), 16);
        // A boundary quad's sub-triangles mix both materials somewhere
        // along it, or at least carry one.
        assert!(subs.iter().flatten().any(Option::is_some));
    }
    // The coarse paint and heights are unchanged by the detail.
    assert_eq!(plain.paint, fine.paint);
    assert_eq!(plain.field, fine.field);
}

#[test]
fn borrowed_scenes_list_each_from_once() {
    let m = map(
        "[terrain]\nmaterials = [{ chars = \".\", from = \"cave02\", cells = [{ tpage = 1, clut = 1, tile = 1 }] }, { chars = \"^\", from = \"cave02\", cells = [{ tpage = 1, clut = 2, tile = 3 }] }]\ncliff = { from = \"rugi\", cells = [{ tpage = 1, clut = 1, tile = 1 }] }",
    );
    let mut t = m.terrain.clone().unwrap();
    assert_eq!(t.borrowed(), vec!["cave02", "rugi"]);
    assert_eq!(t.borrowed_cells_mut("cave02").len(), 2);
    assert_eq!(t.borrowed_cells_mut("rugi").len(), 1);
}

#[test]
fn detail_reach_keeps_boundary_detail_near_the_walkable_floor() {
    // A walled block of `%` up in the thicket (rows 1-2) and a walkable
    // patch of `^` by the floor (rows 4-5).
    let src = r############"
name = "t"
base = { scene = { kind = "scene", name = "town01" } }
[tiles]
origin = [4, 4]
rows = [
  "##########",
  "##%%######",
  "##%%######",
  "##########",
  "......^^..",
  "......^^..",
]
[tiles.legend]
"#" = { walls = "all" }
"%" = { walls = "all" }
"." = {}
"^" = {}
[terrain]
detail = 4
materials = [{ name = "a", chars = ".#", cells = [{ tpage = 1, clut = 1, tile = 1 }] }, { name = "b", chars = "^%", cells = [{ tpage = 2, clut = 2, tile = 2 }] }]
"############;
    let all: MapDef = toml::from_str(src).expect("parses");
    let mut near = all.clone();
    near.terrain.as_mut().unwrap().detail_reach = 1;
    let (all, near) = (
        all.terrain_surface().unwrap().unwrap(),
        near.terrain_surface().unwrap().unwrap(),
    );
    let res = all.field.res;
    let qc = all.field.cols - 1;
    let row_of = |q: usize| (q / qc) / res;
    // Every boundary subdivides with no reach; with one, only those within
    // a tile of the walkable rows do, and the walled block's do not.
    assert!(all.fine.keys().any(|&q| row_of(q) <= 2));
    assert!(!near.fine.is_empty());
    assert!(near.fine.keys().all(|&q| row_of(q) >= 3));
    // Far from the floor no quad blends either; the coarse materials stay.
    for (q, (a, n)) in all.paint.iter().zip(&near.paint).enumerate() {
        for (a, n) in a.iter().zip(n) {
            assert_eq!(
                a.map(|p| (p.cell, p.material)),
                n.map(|p| (p.cell, p.material))
            );
            if row_of(q) <= 1 {
                assert!(n.is_none_or(|p| p.overlay.is_none()));
            }
        }
    }
    assert_eq!(all.field, near.field);
}

#[test]
fn a_deck_over_the_water_walks_and_the_water_round_it_does_not() {
    let water = "water = { level = 10, wade = 6, cells = [{ tpage = 9, clut = 31880, tile = 56 }] }\nshapes = [{ kind = \"plateau\", rect = [0, 4, 9, 5], height = -60, edge = 1.0 }]";
    let m = map(&format!(
        "[[props]]\nmesh = {{ object = 3 }}\ntile = [3, 5]\ntop = 12\n\n[terrain]\nsmooth = 0\n{water}"
    ));
    let g = m.tile_grid().unwrap();
    let w = m.terrain_walls(&g).unwrap();
    assert_eq!(w[5 * g.width + 3], 0, "the deck's tile walks");
    assert_eq!(w[5 * g.width + 6], 0xF, "the open water beside it");
    // A top under the water walls as before.
    let m = map(&format!(
        "[[props]]\nmesh = {{ object = 3 }}\ntile = [3, 5]\ntop = -40\n\n[terrain]\nsmooth = 0\n{water}"
    ));
    assert_eq!(m.terrain_walls(&g).unwrap()[5 * g.width + 3], 0xF);
}

#[test]
fn the_abyss_darkens_with_depth_and_leaves_the_floor_alone() {
    let a = AbyssDef {
        from: -60.0,
        to: -700.0,
        shade: 0.1,
    };
    assert_eq!(a.factor(0.0), 1.0);
    assert_eq!(a.factor(-60.0), 1.0);
    assert!((a.factor(-700.0) - 0.1).abs() < 1e-6);
    assert!((a.factor(-2000.0) - 0.1).abs() < 1e-6);
    let mid = a.factor(-380.0);
    assert!(mid > 0.1 && mid < 1.0);
    // On a surface: the sunk row draws darker than without the abyss.
    let pit = "shapes = [{ kind = \"plateau\", rect = [0, 4, 9, 5], height = -600, edge = 1.0 }]";
    let lit = map(&format!("[terrain]\nsmooth = 0\nvariation = 0\n{pit}"));
    let dark = map(&format!(
        "[terrain]\nsmooth = 0\nvariation = 0\nabyss = {{ from = -60, to = -500, shade = 0.1 }}\n{pit}"
    ));
    let (l, d) = (
        lit.terrain_surface().unwrap().unwrap(),
        dark.terrain_surface().unwrap().unwrap(),
    );
    assert_eq!(l.colors.len(), d.colors.len());
    let sum = |c: &[[u8; 3]]| c.iter().map(|v| u32::from(v[0])).sum::<u32>();
    assert!(sum(&d.colors) < sum(&l.colors));
    assert_eq!(l.colors[0], d.colors[0], "the high corner is untouched");
}

/// The field the rest checks and the workbench's preview read through the
/// memo is the one a whole build makes and the one the surface carries, over
/// a run of edits that come back to earlier inputs.
#[test]
fn the_memoed_field_is_the_whole_build_over_edits() {
    let mats = "materials = [{ name = \"a\", chars = \".\", lift = 3, roughness = 2, cells = [{ tpage = 1, clut = 1, tile = 1 }] }, { name = \"b\", chars = \"^\", cells = [{ tpage = 2, clut = 2, tile = 2 }] }]";
    let shapes = "shapes = [{ kind = \"plateau\", rect = [1, 1, 3, 2], height = 40, edge = 1.5, roughness = 0.5, seed = 3 }, { kind = \"flatten\", at = [6.0, 3.0], radius = 2.0, strength = 0.8 }, { kind = \"smooth\", at = [2.0, 2.0], radius = 2.0, strength = 0.5 }, { kind = \"noise\", amplitude = 4, scale = 2.0, octaves = 3, seed = 9 }]";
    let base = map(&format!("[terrain]\nsmooth = 1\n{mats}\n{shapes}"));
    let mut edits = vec![base.clone()];
    for (c, r, ch) in [
        (0, 0, '^'),
        (9, 5, '^'),
        (4, 2, '.'),
        (5, 3, '.'),
        (2, 4, '^'),
    ] {
        let mut m = edits.last().unwrap().clone();
        let mut row: Vec<char> = m.tiles.rows[r].chars().collect();
        row[c] = ch;
        m.tiles.rows[r] = row.into_iter().collect();
        edits.push(m);
    }
    let mut ladder = base.clone();
    ladder.floor_heights = vec![0, 90];
    edits.push(ladder);
    let mut shaped = base.clone();
    shaped.terrain.as_mut().unwrap().shapes.pop();
    edits.push(shaped);
    // Forwards, backwards and forwards again: cold, then from the memo.
    let order = (0..edits.len())
        .chain((0..edits.len()).rev())
        .chain(0..edits.len());
    for i in order {
        let m = &edits[i];
        let grid = m.tile_grid().unwrap();
        let whole = heightfield(m, m.terrain.as_ref().unwrap(), &grid).unwrap();
        assert_eq!(*m.terrain_field().unwrap().unwrap(), whole, "edit {i}");
        assert_eq!(m.terrain_heightfield().unwrap().unwrap(), whole, "edit {i}");
        assert_eq!(
            m.terrain_surface().unwrap().unwrap().field,
            whole,
            "edit {i}"
        );
    }
    // The edits do change the field: a memo that answered one input for all
    // would pass the rest.
    let first = base.terrain_field().unwrap().unwrap();
    for m in &edits[1..] {
        assert_ne!(*m.terrain_field().unwrap().unwrap(), *first);
    }
}

/// A cell pick is the full 64-bit hash's remainder on every target: a hash
/// with its high word set picks what a 64-bit build picks, not what the
/// hash narrowed to a 32-bit `usize` would (`1 % 3`).
#[test]
fn a_cell_pick_does_not_depend_on_the_pointer_width() {
    assert_eq!(pick_index(0x1_0000_0001, 3), 2);
    assert_eq!(pick_index(u64::MAX, 7), (u64::MAX % 7) as usize);
    assert_eq!(pick_index(5, 0), 0);
}

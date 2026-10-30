//! The terrain's meshes read back against its floor sampler.

use super::super::*;
use super::*;
use legaia_scene_formats::field_objects::{COLLISION_GRID_OFFSET, GRID_DIM, OBJECT_GRID_OFFSET};

const MAP: &str = r############"
name = "terrain"
base = { scene = { kind = "scene", name = "town01" } }
floor_heights = [0, 64]

[tiles]
origin = [6, 6]
rows = [
  "##########",
  "..........",
  "....^^....",
  "....^^....",
  "..........",
  "..........",
]
[tiles.legend]
"#" = { walls = "all", ground = { kind = "texture", tpage = 12, clut = 32006, tile = 36 } }
"." = { ground = { kind = "texture", tpage = 12, clut = 32005, tile = 38 } }
"^" = { height = 1, ground = { kind = "texture", tpage = 12, clut = 32000, tile = 9 } }

[[props]]
mesh = { object = 1 }
tile = [7, 4]

[terrain]
resolution = 4
skirt = 2
shapes = [
  { kind = "plateau", rect = [0, 0, 9, 0], height = 240, edge = 0.3 },
  { kind = "noise", amplitude = 10, scale = 1.5 },
]
"############;

fn def() -> MapDef {
    toml::from_str(MAP).unwrap()
}

/// A one-quad TMD per pack slot, for the bake's base parts.
fn pack(n: usize) -> Vec<Vec<u8>> {
    (0..n)
        .map(|_| super::super::wall_box_tmd(32, 12, 32005, 1).unwrap())
        .collect()
}

fn build(def: &MapDef) -> (FieldImage, usize) {
    let grid = def.tile_grid().unwrap();
    let p = pack(4);
    let lib = MeshLibrary::from_base_pack(p.clone(), def);
    let out = build_field_image(
        def,
        &grid,
        BaseInputs {
            base_map: None,
            geometry_entry: Some(99),
            geometry_len: Some(p.len()),
            library: Some(&lib),
        },
    )
    .unwrap();
    (out, p.len())
}

/// Every baked cell's mesh with the world position of its anchor tile.
fn cells(def: &MapDef, out: &FieldImage, base_len: usize) -> Vec<([i32; 2], Vec<u8>)> {
    let grid = def.tile_grid().unwrap();
    let mut v = Vec::new();
    for r in 0..grid.height as u16 {
        for c in 0..grid.width as u16 {
            let (gc, gr) = grid_tile(def, c, r);
            let o = OBJECT_GRID_OFFSET + (gr * GRID_DIM + gc) * 2;
            let cell = u16::from_le_bytes([out.image[o], out.image[o + 1]]);
            let rec = usize::from(cell & OBJECT_INDEX_MASK);
            if rec == 0 {
                continue;
            }
            let rb = &out.image[rec * OBJECT_RECORD_STRIDE..];
            let flags = u16::from_le_bytes([rb[0x12], rb[0x13]]);
            if flags & 0x4 != 0 {
                continue; // the placed prop
            }
            let mesh = usize::from(u16::from_le_bytes([rb[0x10], rb[0x11]]));
            let (x, z) = def.tile_centre(c, r);
            v.push(([x, z], out.added_tmds[mesh - base_len].clone()));
        }
    }
    v
}

#[test]
fn every_drawn_terrain_vertex_lies_on_the_floor_the_sampler_reads() {
    let def = def();
    let field = def.terrain_heightfield().unwrap().unwrap();
    let (out, base_len) = build(&def);
    let mut checked = 0;
    let mut tris = 0;
    for (at, raw) in cells(&def, &out, base_len) {
        let tmd = legaia_tmd::parse(&raw).unwrap();
        // Every part is terrain: the map has no other decorations.
        for v in tmd.objects.iter().flat_map(|o| o.vertices.iter()) {
            let (x, z) = (at[0] + i32::from(v.x), at[1] + i32::from(v.z));
            let y = field
                .sample_world(
                    x.min(field.origin[0] + (field.cols as i32 - 1) * field.step - 1),
                    z.min(field.origin[1] + (field.rows as i32 - 1) * field.step - 1),
                )
                .unwrap();
            // A vertex on the far edge samples the neighbour quad's edge.
            assert!(
                (-i32::from(v.y) - y).abs() <= 1,
                "vertex at ({x}, {z}): {} vs {y}",
                -v.y
            );
            checked += 1;
        }
        let drawn = legaia_tmd::mesh::tmd_to_vram_mesh_filtered(&tmd, &raw, |_, _, _| true);
        let n: usize = legaia_tmd::encode::decode_model(&tmd, &raw)
            .unwrap()
            .iter()
            .flat_map(|o| o.groups.iter())
            .map(|g| g.prims.len())
            .sum();
        assert_eq!(
            drawn.indices.len(),
            n * 3,
            "the baked cell draws every triangle"
        );
        tris += legaia_tmd::encode::decode_model(&tmd, &raw)
            .unwrap()
            .iter()
            .flat_map(|o| o.groups.iter())
            .map(|g| g.prims.len())
            .sum::<usize>();
    }
    assert!(checked > 1000);
    // Two triangles a quad, skirt included.
    assert_eq!(tris, 2 * (field.cols - 1) * (field.rows - 1));
}

#[test]
fn the_grid_floors_at_zero_and_the_terrain_walls_its_cliff() {
    let def = def();
    let grid = def.tile_grid().unwrap();
    let (out, _) = build(&def);
    let walls = def.terrain_walls(&grid).unwrap();
    let [ox, oy] = def.tiles.origin.map(usize::from);
    for r in 0..grid.height {
        for c in 0..grid.width {
            let b = out.image[COLLISION_GRID_OFFSET + (oy + r) * GRID_DIM + ox + c];
            assert_eq!(b & 0x0F, 0, "tile ({c}, {r}) tier");
            let w = out.image[COLLISION_GRID_OFFSET + (oy + r + 1) * GRID_DIM + ox + c] >> 4;
            let style = grid.get(c, r).unwrap().walls;
            if !(c == 7 && r == 4) {
                assert_eq!(
                    w,
                    style | walls[r * grid.width + c],
                    "tile ({c}, {r}) walls"
                );
            }
        }
    }
    // The cliff's foot (row 1) is walled toward the cliff, open below.
    assert_ne!(walls[grid.width + 3] & 0b0011, 0);
    assert_eq!(walls[5 * grid.width + 3], 0);
    assert_eq!(floor_lut(&def, Some([5; 16])), [0; 16]);
}

#[test]
fn no_tile_carries_a_ground_texture_and_the_prop_stands_on_the_surface() {
    let def = def();
    let field = def.terrain_heightfield().unwrap().unwrap();
    let (out, _) = build(&def);
    let placements = legaia_scene_formats::field_objects::parse_placements(&out.image);
    let grid = def.tile_grid().unwrap();
    for r in 0..grid.height as u16 {
        for c in 0..grid.width as u16 {
            let (gc, gr) = grid_tile(&def, c, r);
            let o = OBJECT_GRID_OFFSET + (gr * GRID_DIM + gc) * 2;
            let cell = u16::from_le_bytes([out.image[o], out.image[o + 1]]);
            assert_eq!(cell & 0x1000, 0, "tile ({c}, {r}) draws a flat ground");
        }
    }
    let (cx, cz) = def.tile_centre(7, 4);
    let want = field.sample_world(cx, cz).unwrap();
    let (gc, gr) = grid_tile(&def, 7, 4);
    let o = OBJECT_GRID_OFFSET + (gr * GRID_DIM + gc) * 2;
    let rec = usize::from(u16::from_le_bytes([out.image[o], out.image[o + 1]]) & OBJECT_INDEX_MASK);
    let rb = &out.image[rec * OBJECT_RECORD_STRIDE..];
    let y = -i16::from_le_bytes([rb[2], rb[3]]);
    assert_eq!(i32::from(y), want);
    assert!(!placements.is_empty());
    // The prop's tile still draws its terrain from a neighbour's cell.
    assert_eq!(out.dropped_decorations, 0);
}

#[test]
fn water_and_foam_layers_cover_the_flooded_quads() {
    let mut def = def();
    let t = def.terrain.as_mut().unwrap();
    t.shapes
        .push(legaia_expansion_format::terrain::TerrainShape::Plateau {
            rect: [0, 5, 9, 5],
            height: -40.0,
            edge: 1.0,
            roughness: 0.0,
            seed: 0,
        });
    t.water = Some(
        toml::from_str(
            r#"level = -8
cells = [{ tpage = 9, clut = 31880, tile = 56 }]"#,
        )
        .unwrap(),
    );
    let s = def.terrain_surface().unwrap().unwrap();
    let grid = def.tile_grid().unwrap();
    let sheet = FoamSheet {
        tpage: 0x1F,
        u0: 0,
        v0: 0,
        cluts: [0x7FC0, 0x7FC1, 0x7FC2, 0x7FC3],
    };
    let meshes = tile_meshes(&def, &grid, &s, Some(&sheet), &Default::default()).unwrap();
    let mut water = 0;
    let mut foam = 0;
    for (_, raw) in &meshes {
        let tmd = legaia_tmd::parse(raw).unwrap();
        for g in legaia_tmd::encode::decode_model(&tmd, raw).unwrap()[0]
            .groups
            .iter()
        {
            if g.semi_transparent {
                foam += g.prims.len();
                assert!(g.prims.iter().all(|p| p.tsb & 0x60 == 0x20), "additive");
            } else if g.prims.iter().any(|p| p.cba == 31880) {
                water += g.prims.len();
            }
        }
    }
    assert!(water > 0 && foam > 0, "water {water} foam {foam}");
}

#[test]
fn a_terrain_mesh_decodes_through_the_hosts_mesh_builder() {
    let def = def();
    let s = def.terrain_surface().unwrap().unwrap();
    let grid = def.tile_grid().unwrap();
    let meshes = tile_meshes(&def, &grid, &s, None, &Default::default()).unwrap();
    let (_, raw) = &meshes[grid.width * 3 + 3];
    let tmd = legaia_tmd::parse(raw).unwrap();
    let m = legaia_tmd::mesh::tmd_to_vram_mesh_filtered(&tmd, raw, |_, _, _| true);
    let quads = s.field.res * s.field.res;
    assert_eq!(m.indices.len(), quads * 2 * 3, "every triangle draws");
    // The heights the builder reads are the terrain's.
    let ys: Vec<f32> = m.positions.iter().map(|p| p[1]).collect();
    assert!(ys.iter().any(|y| *y != 0.0), "{ys:?}");
    // The baked colours survive: none reads back black.
    let want: Vec<[u8; 3]> = s.colors.iter().take(3).copied().collect();
    assert!(
        m.colors.iter().all(|c| c.iter().any(|v| *v > 40)),
        "colours {:?} (surface {want:?})",
        &m.colors[..6.min(m.colors.len())]
    );
}

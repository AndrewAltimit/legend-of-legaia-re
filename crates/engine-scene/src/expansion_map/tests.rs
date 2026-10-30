//! The synthetic field image read back through the retail `.MAP` parsers.
//! Expectations are written from `docs/formats/field-map.md` and
//! `docs/subsystems/field-locomotion.md`, not from the builder.

use super::*;
use legaia_engine_field::field_regions::{
    parse_intra_scene_teleports, parse_tile_triggers, refresh_object_grid_marks,
};
use legaia_scene_formats::field_objects::{
    CELL_VISIBLE, CELL_WALK_VISIBLE, GroundCellGate, build_ground_heightfield, parse_placements,
    parse_terrain_tiles,
};

const MAP: &str = r######"
name = "unit"
base = { scene = { kind = "scene", name = "dolk" } }
floor_heights = [0, 128]

[tiles]
origin = [10, 20]
rows = [
  "#####",
  "#.^p#",
  "#.dc#",
  "#####",
]
[tiles.legend]
"#" = { walls = "all" }
"." = { ground = { kind = "texture", tpage = 12, clut = 0x7C40, tile = 5 } }
"p" = { ground = { kind = "texture", tpage = 12, clut = 0x7C40, tile = 5 } }
"^" = { height = 1, ground = { kind = "texture", tpage = 13, clut = 0x7C80, tile = 9 } }
"d" = { ground = { kind = "mesh", index = 4 } }
"c" = { walls = ["tr", "bl"] }

[[props]]
mesh = { object = 7 }
tile = [3, 1]
offset = [8, 16, -24]
rotation = [0.0, 90.0, 0.0]
solid = true

[[props]]
mesh = { tmd = { kind = "tmd", prot = 99, index = 2 } }
tile = [1, 2]
"######;

fn def() -> MapDef {
    toml::from_str(MAP).unwrap()
}

fn image(def: &MapDef) -> Vec<u8> {
    let grid = def.tile_grid().unwrap();
    build_field_map_image(
        def,
        &grid,
        BaseInputs {
            base_map: None,
            geometry_entry: Some(99),
            geometry_len: Some(20),
            library: None,
        },
    )
    .unwrap()
}

fn byte(img: &[u8], col: usize, row: usize) -> u8 {
    img[COLLISION_GRID_OFFSET + row * GRID_DIM + col]
}

fn cell(img: &[u8], col: usize, row: usize) -> u16 {
    let o = OBJECT_GRID_OFFSET + (row * GRID_DIM + col) * 2;
    u16::from_le_bytes([img[o], img[o + 1]])
}

#[test]
fn the_image_has_the_retail_footprint_and_an_empty_trigger_chain() {
    let img = image(&def());
    assert_eq!(img.len(), 0x12000 + 0x800);
    // The detector's chain: offset[0] = 0x12, each next = end + 2, and the
    // leading u16 = last end + 2.
    let rd = |o: usize| u16::from_le_bytes([img[0x10000 + o], img[0x10001 + o]]);
    assert_eq!([rd(2), rd(6), rd(10), rd(14)], [0x12, 0x14, 0x16, 0x18]);
    assert_eq!([rd(4), rd(8), rd(12), rd(16)], [0; 4]);
    assert_eq!(rd(0), 0x1A);
    assert!(parse_tile_triggers(&img[0x10000..]).is_empty());
    assert!(parse_intra_scene_teleports(&img[0x10000..]).is_empty());
    assert!(parse_tile_triggers(&img[0x12000..]).is_empty());
}

#[test]
fn floor_tiers_sit_in_the_tiles_own_byte_and_walls_one_row_down() {
    let img = image(&def());
    // Map tile (c, r) is grid tile (10 + c, 20 + r).
    // Low nibble: the tile's own tier, plain-floor indexing.
    assert_eq!(byte(&img, 12, 21) & 0x0F, 1, "'^' is tier 1");
    assert_eq!(byte(&img, 11, 21) & 0x0F, 0);
    // High nibble: the wall probe's `+2` Z bias reads tile row r's walls
    // from byte row r + 1.
    assert_eq!(
        byte(&img, 10, 21) >> 4,
        0xF,
        "the '#' at (0, 0) is in row 21"
    );
    assert_eq!(byte(&img, 11, 22) >> 4, 0, "the '.' at (1, 1) is open");
    // 'c' at (3, 2): tr = bit 1, bl = bit 2 -> 0b0110, in byte row 23.
    assert_eq!(byte(&img, 13, 23) >> 4, 0b0110);
    // Off the map is wall.
    assert_eq!(byte(&img, 5, 5), 0xF0);
    assert_eq!(byte(&img, 10, 20) >> 4, 0xF, "the row above the map");
}

#[test]
fn a_solid_prop_closes_its_tile() {
    let img = image(&def());
    // The prop on 'p' (3, 1): open in the legend, closed by `solid`.
    assert_eq!(byte(&img, 13, 22) >> 4, 0xF);
}

/// `collision_grid` is the build's grid region, and a solid prop moved to
/// another tile takes its closed tile with it.
#[test]
fn the_collision_grid_rebuilds_alone_as_the_build_writes_it() {
    let mut d = def();
    let grid = d.tile_grid().unwrap();
    let base = || BaseInputs {
        base_map: None,
        geometry_entry: Some(99),
        geometry_len: Some(20),
        library: None,
    };
    let out = build_field_image(&d, &grid, base()).unwrap();
    let region = &out.image[COLLISION_GRID_OFFSET..COLLISION_GRID_OFFSET + GRID_DIM * GRID_DIM];
    assert_eq!(collision_grid(&d, &grid, &out.collision), region);
    let i = d.props.iter().position(|p| p.solid).unwrap();
    d.props[i].tile = [1, 1];
    let moved = collision_grid(&d, &grid, &out.collision);
    let rebuilt = build_field_image(&d, &grid, base()).unwrap();
    assert_eq!(
        moved,
        &rebuilt.image[COLLISION_GRID_OFFSET..COLLISION_GRID_OFFSET + GRID_DIM * GRID_DIM]
    );
    assert_ne!(moved, region, "the closed tile moved");
}

#[test]
fn props_come_back_as_placements_at_their_tiles() {
    let img = image(&def());
    let placed = parse_placements(&img);
    assert_eq!(placed.len(), 2, "{placed:?}");
    let p = placed.iter().find(|p| p.pack_index == Some(7)).unwrap();
    assert_eq!((p.col, p.row), (13, 21));
    // world_x = col*128 + x_off + 0x40; world_z = row*128 - (z_off - 0x40)
    assert_eq!(p.world_x, 13 * 128 + 64 + 8);
    assert_eq!(p.world_z, 21 * 128 + 64 - 24);
    // y_off is added to the Y-down floor: 16 up is -16.
    assert_eq!(p.y_off, -16);
    assert_eq!(p.rot_y, 0x400, "90 degrees");
    // A TMD inside the base geometry pack resolves to its index.
    assert!(
        placed
            .iter()
            .any(|p| p.pack_index == Some(2) && (p.col, p.row) == (11, 22))
    );
}

#[test]
fn a_tmd_outside_the_base_pack_needs_the_mesh_library() {
    // With no base entry to match, the PROT coordinate is another scene's
    // mesh: it resolves only through a loaded mesh library.
    let def = def();
    let grid = def.tile_grid().unwrap();
    let err = build_field_map_image(&def, &grid, BaseInputs::default()).unwrap_err();
    assert!(err.to_string().contains("mesh library"), "{err:#}");
}

#[test]
fn the_image_ships_the_gates_the_grid_mark_refresh_derives() {
    let img = image(&def());
    let mut refreshed = img.clone();
    refresh_object_grid_marks(&mut refreshed);
    assert_eq!(
        img[OBJECT_GRID_OFFSET..0x10000],
        refreshed[OBJECT_GRID_OFFSET..0x10000],
        "the refresh is the identity on the image's cells"
    );
}

#[test]
fn the_grid_mark_refresh_derives_the_ground_and_decoration_gates() {
    let mut img = image(&def());
    refresh_object_grid_marks(&mut img);
    // Texture cells: walk-visible ground (the heightfield's gate).
    assert_ne!(cell(&img, 11, 21) & CELL_WALK_VISIBLE, 0);
    assert_ne!(cell(&img, 12, 21) & CELL_WALK_VISIBLE, 0);
    // The prop on a textured tile keeps the ground under it.
    assert_ne!(cell(&img, 13, 21) & CELL_WALK_VISIBLE, 0);
    // The decoration-mesh cell is draw-gated, not ground.
    assert_ne!(cell(&img, 12, 22) & CELL_VISIBLE, 0);
    assert_eq!(cell(&img, 12, 22) & CELL_WALK_VISIBLE, 0);
    // Walls draw nothing.
    assert_eq!(cell(&img, 10, 20) & (CELL_VISIBLE | CELL_WALK_VISIBLE), 0);
    let deco: Vec<_> = parse_terrain_tiles(&img)
        .into_iter()
        .filter(|p| p.flags & 0x4 == 0)
        .collect();
    assert_eq!(deco.len(), 1);
    assert_eq!(
        (deco[0].col, deco[0].row, deco[0].pack_index),
        (12, 22, Some(4))
    );
}

#[test]
fn the_ground_heightfield_textures_each_cell_from_its_style() {
    let mut img = image(&def());
    refresh_object_grid_marks(&mut img);
    let lut = floor_lut(&def(), None);
    let hf = build_ground_heightfield(&img, &lut, GroundCellGate::WalkVisible);
    // '.' x2, 'p', '^': four textured cells.
    assert_eq!(hf.quad_count(), 4);
    let pages: std::collections::BTreeSet<[u16; 2]> = hf.cba_tsb.iter().copied().collect();
    assert!(pages.contains(&[0x7C40, 12]) && pages.contains(&[0x7C80, 13]));
    // The raised cell's own corner stands at -128 (Y-down).
    assert!(hf.positions.iter().any(|p| p[1] == -128.0));
}

#[test]
fn coordinates_map_between_tiles_and_world() {
    let def = def();
    let grid = def.tile_grid().unwrap();
    assert_eq!(
        tile_centre_world(&def, 1, 1),
        (11 * 128 + 64, 21 * 128 + 64)
    );
    assert_eq!(
        world_to_map_tile(&def, &grid, 11 * 128 + 5, 21 * 128 + 127),
        Some((1, 1))
    );
    assert_eq!(world_to_map_tile(&def, &grid, 9 * 128, 21 * 128), None);
}

#[test]
fn a_map_off_the_field_grid_is_refused() {
    let mut def = def();
    def.tiles.origin = [124, 0];
    let grid = def.tile_grid().unwrap();
    assert!(build_field_map_image(&def, &grid, BaseInputs::default()).is_err());
}

#[test]
fn wall_tiles_name_a_generated_box_past_the_geometry_pack() {
    let mut def = def();
    def.tiles.legend.get_mut("#").unwrap().wall = Some(WallMesh::Box {
        height: 200,
        tpage: 28,
        clut: 32576,
        tile: 9,
    });
    let grid = def.tile_grid().unwrap();
    let out = build_field_image(
        &def,
        &grid,
        BaseInputs {
            base_map: None,
            geometry_entry: Some(99),
            geometry_len: Some(20),
            library: None,
        },
    )
    .unwrap();
    assert_eq!(out.added_tmds.len(), 1, "one box for every wall tile");
    let tiles = legaia_scene_formats::field_objects::parse_terrain_tiles(&out.image);
    let walls = def
        .tiles
        .rows
        .iter()
        .flat_map(|r| r.chars())
        .filter(|&c| c == '#')
        .count();
    assert_eq!(
        tiles.iter().filter(|p| p.pack_index == Some(20)).count(),
        walls,
        "every wall cell draws pack mesh 20, the first past the pack"
    );
    // Without the pack's length there is nowhere to put the box.
    assert!(build_field_image(&def, &grid, BaseInputs::default()).is_err());
}

#[test]
fn a_wall_box_is_a_textured_tmd_of_the_tile_footprint() {
    let raw = wall_box_tmd(200, 28, 32576, 9).unwrap();
    let tmd = legaia_tmd::parse(&raw).expect("the box parses as a Legaia TMD");
    assert_eq!(tmd.objects.len(), 1);
    let v = &tmd.objects[0].vertices;
    let (lo, hi) = v
        .iter()
        .fold(([i16::MAX; 3], [i16::MIN; 3]), |(lo, hi), p| {
            let p = [p.x, p.y, p.z];
            (
                [lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])],
                [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])],
            )
        });
    assert_eq!(
        (lo, hi),
        ([-64, -200, -64], [64, 0, 64]),
        "Y down, tile wide"
    );
    let mesh = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &raw);
    // Four sides in two 128 / 72 storeys plus the top, both windings, two
    // triangles a face.
    assert_eq!(mesh.indices.len() / 3, (4 * 2 + 1) * 2 * 2);
    // Atlas cell 9 is (32, 32) on the page.
    assert!(
        mesh.uvs
            .iter()
            .all(|uv| (32..64).contains(&uv[0]) && (32..64).contains(&uv[1]))
    );
    assert!(wall_box_tmd(0, 28, 0, 0).is_err());
}

/// A base pack of small textured quads: mesh `i` is a 20-unit plate on
/// texture page `i`.
fn plate_pack(n: usize) -> Vec<Vec<u8>> {
    use legaia_tmd::descriptor::PacketShape;
    use legaia_tmd::encode::{LEGAIA_OBJECT_SCALE, ModelGroup, ModelObject, ModelPrim, encode};
    (0..n)
        .map(|i| {
            encode(&[ModelObject {
                vertices: vec![[-10, 0, -10], [10, 0, -10], [-10, 0, 10], [10, 0, 10]],
                groups: vec![ModelGroup {
                    shape: PacketShape::Ft4,
                    semi_transparent: false,
                    prims: vec![ModelPrim {
                        vertices: vec![0, 1, 2, 3],
                        uvs: vec![(0, 0), (31, 0), (0, 31), (31, 31)],
                        cba: 0x7F00,
                        tsb: i as u16 & 0xF,
                        colors: vec![[128, 128, 128]],
                    }],
                }],
                scale: LEGAIA_OBJECT_SCALE,
            }])
            .unwrap()
        })
        .collect()
}

#[test]
fn decorations_bake_into_one_cell_mesh_a_tile() {
    let mut def = def();
    def.props.clear();
    def.decor = vec![
        legaia_expansion_format::map_detail::DecorDef {
            mesh: PropMesh::Object { object: 3 },
            tile: [1, 1],
            offset: [30, 0, 0],
            ..Default::default()
        },
        legaia_expansion_format::map_detail::DecorDef {
            mesh: PropMesh::Object { object: 4 },
            tile: [1, 1],
            offset: [-30, 16, 0],
            rotation: [0.0, 90.0, 0.0],
            scale: Some(2.0),
            ..Default::default()
        },
    ];
    let grid = def.tile_grid().unwrap();
    let pack = plate_pack(8);
    let lib = MeshLibrary::from_base_pack(pack.clone(), &def);
    let out = build_field_image(
        &def,
        &grid,
        BaseInputs {
            base_map: None,
            geometry_entry: Some(99),
            geometry_len: Some(pack.len()),
            library: Some(&lib),
        },
    )
    .unwrap();
    assert_eq!(out.decoration_chunk, 1);
    assert_eq!(out.dropped_decorations, 0);
    // The tile's cell names a decoration record whose mesh is the bake.
    let (gc, gr) = grid_tile(&def, 1, 1);
    let o = OBJECT_GRID_OFFSET + (gr * GRID_DIM + gc) * 2;
    let cell = u16::from_le_bytes([out.image[o], out.image[o + 1]]);
    assert_ne!(cell & 0x2000, 0, "the decoration draw gate");
    let rec = usize::from(cell & OBJECT_INDEX_MASK);
    let r = &out.image[rec * OBJECT_RECORD_STRIDE..];
    let mesh = usize::from(u16::from_le_bytes([r[0x10], r[0x11]]));
    assert!(mesh >= pack.len(), "an added mesh");
    let baked = &out.added_tmds[mesh - pack.len()];
    let tmd = legaia_tmd::parse(baked).unwrap();
    assert_eq!(tmd.objects.len(), 2, "both decorations");
    // The second: scaled, turned and lifted 16 up (Y down).
    let ys: Vec<i16> = tmd.objects[1].vertices.iter().map(|v| v.y).collect();
    assert!(ys.iter().all(|&y| y == -16), "{ys:?}");
    let xs: Vec<i16> = tmd.objects[1].vertices.iter().map(|v| v.x).collect();
    assert_eq!(xs.iter().min(), Some(&-50));
}

#[test]
fn a_decorated_wall_tile_keeps_its_box() {
    let mut def = def();
    def.props.clear();
    def.tiles.legend.get_mut("#").unwrap().wall = Some(WallMesh::Box {
        height: 96,
        tpage: 12,
        clut: 31808,
        tile: 9,
    });
    def.decor = vec![legaia_expansion_format::map_detail::DecorDef {
        mesh: PropMesh::Object { object: 1 },
        tile: [0, 0],
        offset: [0, 96, 0],
        ..Default::default()
    }];
    let grid = def.tile_grid().unwrap();
    let pack = plate_pack(4);
    let lib = MeshLibrary::from_base_pack(pack.clone(), &def);
    let out = build_field_image(
        &def,
        &grid,
        BaseInputs {
            base_map: None,
            geometry_entry: Some(99),
            geometry_len: Some(pack.len()),
            library: Some(&lib),
        },
    )
    .unwrap();
    // The box, plus one bake of box + plate.
    assert_eq!(out.added_tmds.len(), 2);
    let baked = legaia_tmd::parse(&out.added_tmds[1]).unwrap();
    let boxed = legaia_tmd::parse(&out.added_tmds[0]).unwrap();
    assert_eq!(baked.objects.len(), boxed.objects.len() + 1);
}

#[test]
fn decorations_on_a_prop_tile_are_dropped() {
    let mut def = def();
    let tile = def.props[0].tile;
    def.props.truncate(1);
    def.props[0].mesh = PropMesh::Object { object: 2 };
    def.decor = vec![legaia_expansion_format::map_detail::DecorDef {
        mesh: PropMesh::Object { object: 1 },
        tile,
        ..Default::default()
    }];
    let grid = def.tile_grid().unwrap();
    let pack = plate_pack(4);
    let lib = MeshLibrary::from_base_pack(pack.clone(), &def);
    let out = build_field_image(
        &def,
        &grid,
        BaseInputs {
            base_map: None,
            geometry_entry: Some(99),
            geometry_len: Some(pack.len()),
            library: Some(&lib),
        },
    )
    .unwrap();
    assert_eq!(out.dropped_decorations, 1);
}

#[test]
fn decals_bake_as_flat_polygons_in_their_cell() {
    use legaia_expansion_format::map_detail::{AtlasCell, DecalDef};
    let mut def = def();
    def.props.clear();
    def.decals = vec![DecalDef {
        tile: Some([1, 1]),
        cell: AtlasCell {
            tpage: 28,
            clut: 32576,
            tile: 9,
        },
        size: [100, 60],
        sides: 8,
        ..Default::default()
    }];
    let grid = def.tile_grid().unwrap();
    let pack = plate_pack(4);
    let lib = MeshLibrary::from_base_pack(pack.clone(), &def);
    let out = build_field_image(
        &def,
        &grid,
        BaseInputs {
            base_map: None,
            geometry_entry: Some(99),
            geometry_len: Some(pack.len()),
            library: Some(&lib),
        },
    )
    .unwrap();
    assert_eq!(out.added_tmds.len(), 1);
    let raw = &out.added_tmds[0];
    let tmd = legaia_tmd::parse(raw).unwrap();
    let o = &tmd.objects[0];
    // Lifted 2 units off the floor (Y down), 50 x 30 half extents.
    assert!(o.vertices.iter().all(|v| v.y == -2));
    assert_eq!(o.vertices.iter().map(|v| v.x).max(), Some(50));
    let g = legaia_tmd::legaia_prims::iter_groups(
        raw,
        o.primitives_byte_offset,
        o.primitives_byte_size,
    )
    .unwrap();
    // Eight fan triangles, both windings, inside atlas cell 9 (u 32..63, v 32..63).
    assert_eq!(g[0].prims.len(), 16);
    assert!(
        g[0].prims
            .iter()
            .flat_map(|p| &p.uvs)
            .all(|&(u, v)| (32..=63).contains(&u) && (32..=63).contains(&v))
    );
}

#[test]
fn a_prop_on_a_mesh_cell_anchors_beside_it() {
    // Tile (2, 2) is a mesh ground ('d'): its cell keeps the mesh, and the
    // prop's record anchors on the nearest free tile with an offset back.
    let mut def = def();
    def.props.truncate(1);
    def.props[0].tile = [2, 2];
    def.props[0].offset = [0, 0, 0];
    def.props[0].solid = false;
    let grid = def.tile_grid().unwrap();
    let out = build_field_image(
        &def,
        &grid,
        BaseInputs {
            base_map: None,
            geometry_entry: Some(99),
            geometry_len: Some(20),
            library: None,
        },
    )
    .unwrap();
    assert!(out.covered_cells.is_empty());
    let anchor = out.prop_anchors[0];
    assert_ne!(anchor, (2, 2));
    let placed = parse_placements(&out.image);
    let p = placed.iter().find(|p| p.pack_index == Some(7)).unwrap();
    // The draw lands on tile (2, 2)'s centre all the same.
    let (cx, cz) = tile_centre_world(&def, 2, 2);
    assert_eq!((p.world_x, p.world_z), (i32::from(cx), i32::from(cz)));
    // The mesh ground still draws from its own cell.
    let (gc, gr) = grid_tile(&def, 2, 2);
    let c = cell(&out.image, gc, gr) & OBJECT_INDEX_MASK;
    let r = &out.image[usize::from(c) * OBJECT_RECORD_STRIDE..];
    assert_eq!(u16::from_le_bytes([r[0x10], r[0x11]]), 4);
}

#[test]
fn a_prop_on_a_kept_retail_floor_keeps_its_ground() {
    // A kept map whose tile style names no ground: the cell's retail record
    // draws the floor texture, and the prop's record standing in for it
    // carries the same texture words, so the floor under the prop stays.
    let mut def: MapDef = toml::from_str(&MAP.replace(
        "base = { scene = { kind = \"scene\", name = \"dolk\" } }",
        "base = { scene = { kind = \"scene\", name = \"dolk\" }, geometry = \"keep\" }",
    ))
    .unwrap();
    def.tiles.legend.get_mut(".").unwrap().ground = None;
    def.props.truncate(1);
    def.props[0].tile = [1, 1];
    def.props[0].solid = false;
    let grid = def.tile_grid().unwrap();
    let (gc, gr) = grid_tile(&def, 1, 1);
    let mut base = vec![0u8; 0x12000];
    let slot = 5usize;
    let r = &mut base[slot * OBJECT_RECORD_STRIDE..(slot + 1) * OBJECT_RECORD_STRIDE];
    r[0x12..0x14].copy_from_slice(&0x11u16.to_le_bytes());
    r[0x14] = 3;
    r[0x15] = 14;
    r[0x16..0x18].copy_from_slice(&0x7E00u16.to_le_bytes());
    let o = OBJECT_GRID_OFFSET + (gr * GRID_DIM + gc) * 2;
    base[o..o + 2].copy_from_slice(&(slot as u16).to_le_bytes());
    let out = build_field_image(
        &def,
        &grid,
        BaseInputs {
            base_map: Some(&base),
            geometry_entry: Some(99),
            geometry_len: Some(20),
            library: None,
        },
    )
    .unwrap();
    assert_eq!(out.prop_anchors[0], (1, 1));
    let c = usize::from(cell(&out.image, gc, gr) & OBJECT_INDEX_MASK);
    assert_ne!(c, slot);
    let r = &out.image[c * OBJECT_RECORD_STRIDE..(c + 1) * OBJECT_RECORD_STRIDE];
    assert_eq!(u16::from_le_bytes([r[0x10], r[0x11]]), 7);
    assert_ne!(u16::from_le_bytes([r[0x12], r[0x13]]) & FLAGS_GROUND, 0);
    assert_eq!((r[0x14], r[0x15]), (3, 14));
    assert_eq!(u16::from_le_bytes([r[0x16], r[0x17]]), 0x7E00);
}

#[test]
fn a_kept_room_keeps_its_floor_steps_and_props_stand_on_them() {
    // A retail dais on map tile (1, 1): its cell flagged 0x800, its kind-2
    // record two whole steps up (64 units) - the way retail rooms raise a
    // floor without a tier.
    let mut def: MapDef = toml::from_str(&MAP.replace(
        "base = { scene = { kind = \"scene\", name = \"dolk\" } }",
        "base = { scene = { kind = \"scene\", name = \"dolk\" }, geometry = \"keep\" }",
    ))
    .unwrap();
    def.props.truncate(1);
    def.props[0].tile = [1, 1];
    def.props[0].offset = [0, 10, 0];
    def.props[0].solid = false;
    let grid = def.tile_grid().unwrap();
    let (gc, gr) = grid_tile(&def, 1, 1);
    let mut base = vec![0u8; 0x12000];
    let o = OBJECT_GRID_OFFSET + (gr * GRID_DIM + gc) * 2;
    base[o..o + 2].copy_from_slice(&CELL_ELEVATION_OVERRIDE.to_le_bytes());
    // Trigger block: kind 2 holds the dais and one record off the map.
    let b = TRIGGER_BLOCK_OFFSET;
    let words: [u16; 9] = [0x24, 0x12, 0, 0x14, 0, 0x16, 2, 0x20, 0];
    for (i, w) in words.iter().enumerate() {
        base[b + i * 2..b + i * 2 + 2].copy_from_slice(&w.to_le_bytes());
    }
    base[b + 0x16..b + 0x1A].copy_from_slice(&[gc as u8, gr as u8, 2, 0]);
    base[b + 0x1A..b + 0x1E].copy_from_slice(&[100, 100, 5, 0]);
    let out = build_field_image(
        &def,
        &grid,
        BaseInputs {
            base_map: Some(&base),
            geometry_entry: Some(99),
            geometry_len: Some(20),
            library: None,
        },
    )
    .unwrap();
    // The image carries the dais's record, not the off-map one.
    let recs = parse_elevation_overrides(&out.image[TRIGGER_BLOCK_OFFSET..]);
    assert_eq!(
        recs,
        [ElevationOverride {
            tile_x: gc as u8,
            tile_z: gr as u8,
            coarse: 2,
            quads: 0,
        }]
    );
    // The prop's record took the cell, and the cell kept its flag.
    assert_ne!(cell(&out.image, gc, gr) & CELL_ELEVATION_OVERRIDE, 0);
    // The prop stands on the dais: 64 up plus its own 10 (y down).
    let c = usize::from(cell(&out.image, gc, gr) & OBJECT_INDEX_MASK);
    let r = &out.image[c * OBJECT_RECORD_STRIDE..];
    assert_eq!(i16::from_le_bytes([r[0x02], r[0x03]]), -74);
    // A map that does not keep the room carries nothing.
    let kept = KeptFloor::new(&def, &grid, Some(&base));
    assert_eq!(kept.step_up_on(&def, 1, 1, 0, 0), 64);
    assert_eq!(kept.step_up_on(&def, 2, 1, 0, 0), 0);
    def.base.geometry = BaseGeometry::None;
    assert!(
        KeptFloor::new(&def, &grid, Some(&base))
            .records()
            .is_empty()
    );
}

#[test]
fn no_mesh_record_takes_a_protagonist_object_id() {
    // Object ids 1..=3 never draw from the scene pack, so every record the
    // builder writes sits at 4 or past.
    let img = image(&def());
    for i in 1..4 {
        assert!(
            img[i * OBJECT_RECORD_STRIDE..(i + 1) * OBJECT_RECORD_STRIDE]
                .iter()
                .all(|&b| b == 0),
            "record {i} is written"
        );
    }
    let placed = parse_placements(&img);
    assert!(placed.iter().any(|p| p.pack_index == Some(7)));
    assert!(placed.iter().all(|p| p.obj_idx >= 4));
}

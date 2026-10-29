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
fn a_tmd_outside_the_geometry_pack_is_refused() {
    let def = def();
    let grid = def.tile_grid().unwrap();
    let err = build_field_map_image(&def, &grid, BaseInputs::default()).unwrap_err();
    assert!(err.to_string().contains("geometry pack"), "{err:#}");
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
        },
    )
    .unwrap();
    assert_eq!(out.wall_tmds.len(), 1, "one box for every wall tile");
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

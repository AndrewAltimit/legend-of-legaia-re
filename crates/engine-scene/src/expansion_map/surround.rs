//! A map's surround ([`SurroundDef`]): the ground past the floor's edge.
//!
//! A map built with no base geometry draws nothing past its floor, and the
//! follow camera at a map's edge frames the clear colour there. The surround
//! fills it through the decoration-cell path both hosts already draw: every
//! void tile of the map and every grid tile up to `reach` past its edges
//! whose cell draws nothing gets a cell naming a shared record, whose mesh
//! is a flat tile `depth` below tier `0` - with, where the tile borders the
//! map's floor or walls, a face from that neighbour's floor down to it. A
//! cell draws at its corners' mean floor height, so the mesh carries the
//! heights from there; one record serves every cell of the same shape,
//! which keeps the surround to a handful of records.

use anyhow::Result;
use legaia_expansion_format::map::{FLOOR_TIERS, MapDef, TileGrid};
use legaia_expansion_format::map_detail::{AtlasCell, SurroundDef};
use legaia_scene_formats::field_objects::{
    COLLISION_GRID_OFFSET, GRID_DIM, OBJECT_GRID_OFFSET, OBJECT_INDEX_MASK,
};

use super::{Added, FLAGS_DECORATION, WALL_FACE_HEIGHT, cell_gates, put_record, set_cell};

/// The sides of a tile, in [`SurroundCell::faces`] order: the neighbour
/// toward `-Z`, `+X`, `+Z`, `-X`.
const SIDES: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

/// One surround tile's mesh, in the decoration-cell frame (origin at the
/// tile centre on the floor the cell draws at, Y down).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurroundCell {
    /// For each side ([`SIDES`] order) bordering the map's floor or walls,
    /// the `Y` of that neighbour's floor: the top of the face dropping from
    /// it.
    pub faces: [Option<i32>; 4],
    /// The `Y` of the drop's ground.
    pub drop: i32,
}

/// The surround tile's mesh: the drop's ground, and a face from each
/// bordering neighbour's floor down to it. Both windings, so the faces read
/// from the floor above and the drop below alike.
pub fn surround_tmd(s: &SurroundDef, cell: SurroundCell) -> Result<Vec<u8>> {
    use legaia_tmd::descriptor::PacketShape;
    use legaia_tmd::encode::{LEGAIA_OBJECT_SCALE, ModelGroup, ModelObject, ModelPrim, encode};
    let px = usize::from(legaia_scene_formats::field_objects::GROUND_ATLAS_TILE_PX);
    let axis = legaia_scene_formats::field_objects::GROUND_ATLAS_AXIS;
    let uv = |c: &AtlasCell| {
        let u0 = ((usize::from(c.tile) % axis) * px).min(224) as u8;
        let v0 = ((usize::from(c.tile) / axis % axis) * px).min(224) as u8;
        [
            (u0, v0),
            (u0 + (px as u8 - 1), v0),
            (u0, v0 + (px as u8 - 1)),
            (u0 + (px as u8 - 1), v0 + (px as u8 - 1)),
        ]
    };
    let shade = [s.shade; 3];
    let mut vertices: Vec<[i16; 3]> = Vec::new();
    let mut prims = Vec::new();
    // `quad(a, b, c, d, cell)` is a face in perimeter order, `a`-`b` its top
    // edge; the file stores Z-order `a, b, d, c`.
    let mut quad = |p: [[i32; 3]; 4], cell: &AtlasCell| {
        let mut idx = [0u16; 4];
        for (k, v) in p.iter().enumerate() {
            let v = v.map(|c| c as i16);
            idx[k] = match vertices.iter().position(|w| *w == v) {
                Some(i) => i as u16,
                None => {
                    vertices.push(v);
                    (vertices.len() - 1) as u16
                }
            };
        }
        let [a, b, c, d] = idx;
        for order in [[a, b, d, c], [b, a, c, d]] {
            prims.push(ModelPrim {
                vertices: order.to_vec(),
                uvs: uv(cell).to_vec(),
                cba: cell.clut,
                tsb: u16::from(cell.tpage),
                colors: vec![shade],
            });
        }
    };
    let e = 64;
    let h = cell.drop;
    quad([[-e, h, -e], [e, h, -e], [e, h, e], [-e, h, e]], &s.ground);
    if let Some(side) = &s.side {
        let corners = [[-e, -e], [e, -e], [e, e], [-e, e]];
        for (k, top) in cell.faces.iter().enumerate() {
            let Some(mut y) = *top else {
                continue;
            };
            let [x0, z0] = corners[k];
            let [x1, z1] = corners[(k + 1) % 4];
            while y < h {
                let low = (y + WALL_FACE_HEIGHT).min(h);
                quad(
                    [[x0, y, z0], [x1, y, z1], [x1, low, z1], [x0, low, z0]],
                    side,
                );
                y = low;
            }
        }
    }
    encode(&[ModelObject {
        vertices,
        groups: vec![ModelGroup {
            shape: PacketShape::Ft4,
            semi_transparent: false,
            prims,
        }],
        scale: LEGAIA_OBJECT_SCALE,
    }])
}

/// Write the map's surround into the image's empty cells: void tiles of the
/// map and grid tiles up to `reach` past it. Returns how many cells it
/// filled. A no-op without a `[surround]`.
pub(super) fn write_surround(
    img: &mut [u8],
    def: &MapDef,
    grid: &TileGrid,
    added: &mut Added,
    free: &mut Vec<usize>,
) -> Result<usize> {
    let Some(s) = &def.surround else {
        return Ok(0);
    };
    let [ox, oy] = def.tiles.origin.map(i32::from);
    let (w, h) = (grid.width as i32, grid.height as i32);
    let reach = i32::from(s.reach);
    let cell_off =
        |gc: i32, gr: i32| OBJECT_GRID_OFFSET + (gr as usize * GRID_DIM + gc as usize) * 2;
    let on_grid =
        |gc: i32, gr: i32| gc >= 0 && gr >= 0 && gc < GRID_DIM as i32 && gr < GRID_DIM as i32;
    // A grid tile the surround stands on: within reach of the map, on the
    // grid, off the map's floor and walls, its cell drawing nothing.
    let fills = |img: &[u8], gc: i32, gr: i32| -> bool {
        let (c, r) = (gc - ox, gr - oy);
        if c < -reach || r < -reach || c >= w + reach || r >= h + reach || !on_grid(gc, gr) {
            return false;
        }
        let inside = c >= 0 && r >= 0 && c < w && r < h;
        if inside && !grid.get(c as usize, r as usize).is_some_and(|t| t.void) {
            return false;
        }
        let o = cell_off(gc, gr);
        u16::from_le_bytes([img[o], img[o + 1]]) & OBJECT_INDEX_MASK == 0
    };
    // A neighbour whose edge drops to the surround: a tile of the map that
    // is not void (floor or wall), and the height of its floor.
    let ladder = def.floor_ladder().unwrap_or([0; FLOOR_TIERS]);
    let edge = |gc: i32, gr: i32| -> Option<i32> {
        let (c, r) = (gc - ox, gr - oy);
        if c < 0 || r < 0 || c >= w || r >= h {
            return None;
        }
        let t = grid.get(c as usize, r as usize).filter(|t| !t.void)?;
        Some(i32::from(ladder[usize::from(t.tier)]))
    };
    // The floor a cell draws at: the mean of its four corner tiles' tier
    // heights, as the decoration-cell sweep places it.
    let tier_at = |gc: i32, gr: i32| -> i32 {
        if !on_grid(gc, gr) {
            return i32::from(ladder[0]);
        }
        let b = img[COLLISION_GRID_OFFSET + gr as usize * GRID_DIM + gc as usize];
        i32::from(ladder[usize::from(b & 0x0F)])
    };
    let floor = |gc: i32, gr: i32| -> i32 {
        (tier_at(gc, gr) + tier_at(gc + 1, gr) + tier_at(gc, gr + 1) + tier_at(gc + 1, gr + 1)) / 4
    };
    let bottom = i32::from(ladder[0]) - i32::from(s.depth);
    let mut cells: Vec<(i32, i32, SurroundCell)> = Vec::new();
    for gr in oy - reach..oy + h + reach {
        for gc in ox - reach..ox + w + reach {
            if !fills(img, gc, gr) {
                continue;
            }
            // Heights up; the mesh frame is Y down from the cell's floor.
            let at = floor(gc, gr);
            let faces = SIDES.map(|(dc, dr)| s.side.and(edge(gc + dc, gr + dr)).map(|nb| at - nb));
            cells.push((
                gc,
                gr,
                SurroundCell {
                    faces,
                    drop: at - bottom,
                },
            ));
        }
    }
    let mut records: Vec<(SurroundCell, usize)> = Vec::new();
    for &(gc, gr, key) in &cells {
        let slot = match records.iter().find(|(k, _)| *k == key) {
            Some(&(_, slot)) => slot,
            None => {
                let mesh = added.index(surround_tmd(s, key)?, "the surround")?;
                let slot = free
                    .pop()
                    .ok_or_else(|| anyhow::anyhow!("out of object records placing the surround"))?;
                put_record(img, slot, [0; 3], [0; 3], mesh, FLAGS_DECORATION, None);
                records.push((key, slot));
                slot
            }
        };
        set_cell(
            img,
            cell_off(gc, gr),
            slot as u16 | cell_gates(FLAGS_DECORATION),
        );
    }
    Ok(cells.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expansion_map::{BaseInputs, build_field_image};
    use legaia_scene_formats::field_objects::COLLISION_GRID_OFFSET;

    const MAP: &str = r######"
name = "surround"
base = { scene = { kind = "scene", name = "dolk" } }
floor_heights = [0, 64]

[tiles]
origin = [10, 20]
rows = [
  "_____",
  "#.. #",
  "#.^.#",
  "#####",
]
[tiles.legend]
"#" = { walls = "all", wall = { kind = "box", height = 200, tpage = 28, clut = 32576, tile = 9 } }
"_" = { walls = "all", wall = { kind = "box", height = 48, tpage = 28, clut = 32576, tile = 9 } }
"." = { ground = { kind = "texture", tpage = 12, clut = 0x7C40, tile = 5 } }
"^" = { height = 1, ground = { kind = "texture", tpage = 12, clut = 0x7C40, tile = 5 } }

[surround]
ground = { tpage = 5, clut = 30720, tile = 0 }
side = { tpage = 11, clut = 32523, tile = 38 }
depth = 300
reach = 3
"######;

    fn build(def: &MapDef) -> crate::expansion_map::FieldImage {
        let grid = def.tile_grid().unwrap();
        build_field_image(
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

    fn cell(img: &[u8], col: usize, row: usize) -> u16 {
        let o = OBJECT_GRID_OFFSET + (row * GRID_DIM + col) * 2;
        u16::from_le_bytes([img[o], img[o + 1]])
    }

    #[test]
    fn the_surround_fills_void_and_the_margin_and_nothing_else() {
        let def: MapDef = toml::from_str(MAP).unwrap();
        let with = build(&def);
        let without = build(&MapDef {
            surround: None,
            ..def.clone()
        });
        let img = &with.image;
        // The void tile (3, 1) and tiles up to three past every edge draw.
        for (c, r) in [(13, 21), (7, 17), (17, 20), (12, 26), (9, 22)] {
            let w = cell(img, c, r);
            assert_ne!(w & OBJECT_INDEX_MASK, 0, "({c}, {r}) draws the surround");
            assert_eq!(w & 0x3000, 0x2000, "a decoration cell, no ground gate");
        }
        // Four past an edge is out of reach.
        for (c, r) in [(6, 22), (12, 16), (18, 22), (12, 27)] {
            assert_eq!(cell(img, c, r), 0, "({c}, {r}) is out of reach");
        }
        // The map's own floor and walls are untouched, and so is collision:
        // the surround is drawn, never walked.
        for r in 20..24 {
            for c in 10..15 {
                if (c, r) != (13, 21) {
                    assert_eq!(cell(img, c, r), cell(&without.image, c, r), "({c}, {r})");
                }
            }
        }
        let cg = COLLISION_GRID_OFFSET..COLLISION_GRID_OFFSET + GRID_DIM * GRID_DIM;
        assert_eq!(img[cg.clone()], without.image[cg]);
        // A handful of shapes, not a record a tile.
        let added = with.added_tmds.len() - without.added_tmds.len();
        assert!((2..=16).contains(&added), "{added} surround meshes");
    }

    #[test]
    fn the_drop_lies_depth_below_tier_zero_and_faces_rise_to_the_neighbour() {
        let s = SurroundDef {
            ground: AtlasCell {
                tpage: 5,
                clut: 30720,
                tile: 0,
            },
            side: Some(AtlasCell {
                tpage: 11,
                clut: 32523,
                tile: 38,
            }),
            depth: 300,
            ..SurroundDef::default()
        };
        // A cell drawn at height 16 beside a neighbour at 64 (on its -Z
        // side): the face runs from Y -48 to the drop at Y 316.
        let raw = surround_tmd(
            &s,
            SurroundCell {
                faces: [Some(16 - 64), None, None, None],
                drop: 16 + 300,
            },
        )
        .unwrap();
        let tmd = legaia_tmd::parse(&raw).expect("parses as a Legaia TMD");
        let v = &tmd.objects[0].vertices;
        let ys: Vec<i16> = v.iter().map(|p| p.y).collect();
        assert_eq!(*ys.iter().min().unwrap(), -48);
        assert_eq!(*ys.iter().max().unwrap(), 316);
        // The face stands on the -Z edge only.
        assert!(v.iter().filter(|p| p.y < 316).all(|p| p.z == -64));
        // No side: the ground alone, one quad in both windings.
        let flat = surround_tmd(
            &SurroundDef { side: None, ..s },
            SurroundCell {
                faces: [None; 4],
                drop: 300,
            },
        )
        .unwrap();
        let tmd = legaia_tmd::parse(&flat).unwrap();
        assert!(tmd.objects[0].vertices.iter().all(|p| p.y == 300));
    }
}

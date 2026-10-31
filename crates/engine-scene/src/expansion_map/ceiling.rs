//! A map's ceiling ([`legaia_expansion_format::map_detail::CeilingDef`]): the underside over the floor and the
//! chains its hung items hang from.
//!
//! Both are decorations baked into the map's cells, so they draw through the
//! decoration-cell path both hosts already draw. The underside goes in a
//! piece per [`CEILING_BLOCK`] x [`CEILING_BLOCK`] tiles - one record each,
//! not one a tile - wound to face down: the field pass culls back faces, so
//! the follow camera, which looks down on the room, does not see it, and
//! the occlusion gate passes through it
//! ([`crate::field_occlusion::FieldOccluders`] skips a face only visible from
//! beneath). A chain is a square shaft from a hung item's top up to the
//! ceiling.

use anyhow::Result;
use legaia_expansion_format::map::{FLOOR_TIERS, MapDef, PropMesh, TileGrid};
use legaia_expansion_format::map_detail::{AtlasCell, ChainDef};
use legaia_scene_formats::field_objects::{
    COLLISION_GRID_OFFSET, FLAG_PLACED, GRID_DIM, OBJECT_GRID_OFFSET, OBJECT_INDEX_MASK,
    OBJECT_RECORD_STRIDE,
};
use legaia_tmd::descriptor::PacketShape;
use legaia_tmd::encode::{LEGAIA_OBJECT_SCALE, ModelGroup, ModelObject, ModelPrim, encode};

use super::detail::MeshLibrary;
use super::{Item, ItemSrc, WALL_FACE_HEIGHT, grid_tile};

/// Tiles a side of one underside piece.
pub const CEILING_BLOCK: u16 = 4;

/// The atlas cell's four texel corners, in perimeter order.
fn cell_uvs(c: &AtlasCell, width_px: Option<u8>) -> [(u8, u8); 4] {
    let px = usize::from(legaia_scene_formats::field_objects::GROUND_ATLAS_TILE_PX);
    let axis = legaia_scene_formats::field_objects::GROUND_ATLAS_AXIS;
    let u0 = ((usize::from(c.tile) % axis) * px).min(224) as u8;
    let v0 = ((usize::from(c.tile) / axis % axis) * px).min(224) as u8;
    let w = width_px.map_or(px as u8 - 1, |w| w.clamp(1, px as u8 - 1));
    let h = px as u8 - 1;
    [(u0, v0), (u0 + w, v0), (u0 + w, v0 + h), (u0, v0 + h)]
}

/// An object's vertex table and quads under construction.
#[derive(Default)]
struct Builder {
    vertices: Vec<[i16; 3]>,
    prims: Vec<ModelPrim>,
}

impl Builder {
    /// One textured quad, `p` in perimeter order (`p[0]`-`p[1]` the top
    /// edge for a wall), stored in the file's Z-order. Its visible side is
    /// the one from which `p` runs clockwise on screen - the parity the
    /// field pass's back-face cull keeps.
    fn quad(&mut self, p: [[i32; 3]; 4], uvs: [(u8, u8); 4], cell: &AtlasCell, shade: u8) {
        let mut idx = [0u16; 4];
        for (k, v) in p.iter().enumerate() {
            let v = v.map(|c| c as i16);
            idx[k] = match self.vertices.iter().position(|w| *w == v) {
                Some(i) => i as u16,
                None => {
                    self.vertices.push(v);
                    (self.vertices.len() - 1) as u16
                }
            };
        }
        self.prims.push(ModelPrim {
            vertices: vec![idx[0], idx[1], idx[3], idx[2]],
            uvs: vec![uvs[0], uvs[1], uvs[3], uvs[2]],
            cba: cell.clut,
            tsb: u16::from(cell.tpage),
            colors: vec![[shade; 3]],
        });
    }

    fn encode(self) -> Result<Vec<u8>> {
        encode(&[ModelObject {
            vertices: self.vertices,
            groups: vec![ModelGroup {
                shape: PacketShape::Ft4,
                semi_transparent: false,
                prims: self.prims,
            }],
            scale: LEGAIA_OBJECT_SCALE,
        }])
    }
}

/// The underside over `tiles` (offsets in tiles from the piece's anchor,
/// map frame), at the mesh's own `Y = 0`, facing down.
pub fn underside_tmd(cell: &AtlasCell, shade: u8, tiles: &[[i32; 2]]) -> Result<Vec<u8>> {
    let uv = cell_uvs(cell, None);
    let e = 64;
    let mut b = Builder::default();
    for &[dc, dr] in tiles {
        let (x, z) = (dc * 128, dr * 128);
        // Seen from below (Y down, so from +Y), clockwise.
        b.quad(
            [
                [x - e, 0, z - e],
                [x + e, 0, z - e],
                [x + e, 0, z + e],
                [x - e, 0, z + e],
            ],
            uv,
            cell,
            shade,
        );
    }
    b.encode()
}

/// A chain: a square shaft `len` units tall from the mesh's origin up
/// (`-Y`), its four sides facing out.
pub fn chain_tmd(c: &ChainDef, len: i32) -> Result<Vec<u8>> {
    let cell = AtlasCell {
        tpage: c.tpage,
        clut: c.clut,
        tile: c.tile,
    };
    let uv = cell_uvs(&cell, Some(c.width.min(31)));
    let h = (i32::from(c.width) / 2).max(1);
    let corners = [[-h, -h], [h, -h], [h, h], [-h, h]];
    let mut b = Builder::default();
    let mut y = 0;
    while y > -len {
        let top = (y - WALL_FACE_HEIGHT).max(-len);
        for k in 0..4 {
            let [x0, z0] = corners[k];
            let [x1, z1] = corners[(k + 1) % 4];
            // Seen from outside, clockwise: top edge first.
            b.quad(
                [[x0, top, z0], [x1, top, z1], [x1, y, z1], [x0, y, z0]],
                uv,
                &cell,
                c.shade,
            );
        }
        y = top;
    }
    b.encode()
}

/// The ceiling's decorations: its underside pieces and its chains, for the
/// decoration bake. `img` is the image with the map's own records written,
/// so an item never anchors on a placed prop's cell (the bake leaves a
/// placed cell's decorations out).
pub(super) fn ceiling_items(
    img: &[u8],
    def: &MapDef,
    grid: &TileGrid,
    lib: Option<&MeshLibrary>,
    surface: Option<&legaia_expansion_format::terrain::TerrainSurface>,
) -> Result<Vec<Item>> {
    let Some(c) = &def.ceiling else {
        return Ok(Vec::new());
    };
    let (w, h) = (grid.width as u16, grid.height as u16);
    let placed = |col: u16, row: u16| -> bool {
        let (gc, gr) = grid_tile(def, col, row);
        let o = OBJECT_GRID_OFFSET + (gr * GRID_DIM + gc) * 2;
        let idx = usize::from(u16::from_le_bytes([img[o], img[o + 1]]) & OBJECT_INDEX_MASK);
        idx != 0 && {
            let rec = &img[idx * OBJECT_RECORD_STRIDE..];
            u16::from_le_bytes([rec[0x12], rec[0x13]]) & FLAG_PLACED != 0
        }
    };
    // The tile of `[c0, r0] .. [c1, r1)` nearest `at` whose cell is not a
    // placed prop's.
    let free_tile = |c0: u16, r0: u16, c1: u16, r1: u16, at: [f32; 2]| -> Option<[u16; 2]> {
        (r0..r1)
            .flat_map(|r| (c0..c1).map(move |cc| [cc, r]))
            .filter(|t| !placed(t[0], t[1]))
            .min_by(|a, b| {
                let d = |t: &[u16; 2]| {
                    (f32::from(t[0]) - at[0]).powi(2) + (f32::from(t[1]) - at[1]).powi(2)
                };
                d(a).total_cmp(&d(b)).then(a.cmp(b))
            })
    };
    let ladder = def.floor_ladder().unwrap_or([0; FLOOR_TIERS]);
    // The bake stands a cell's items on its tile's tier, but the cell draws
    // at the mean of its four corner tiles' heights; at a tier's edge the
    // two differ. `lift` that lands an item `at` world units up on `anchor`
    // corrects for it, so a chain meets its lantern wherever it anchors.
    let lift_to = |at: f32, anchor: [u16; 2]| -> f32 {
        if surface.is_some() {
            return at;
        }
        let (gc, gr) = grid_tile(def, anchor[0], anchor[1]);
        let tier_at = |c: usize, r: usize| -> i32 {
            let b =
                img[COLLISION_GRID_OFFSET + r.min(GRID_DIM - 1) * GRID_DIM + c.min(GRID_DIM - 1)];
            i32::from(ladder[usize::from(b & 0x0F)])
        };
        let drawn =
            (tier_at(gc, gr) + tier_at(gc + 1, gr) + tier_at(gc, gr + 1) + tier_at(gc + 1, gr + 1))
                / 4;
        let tier = grid
            .get(usize::from(anchor[0]), usize::from(anchor[1]))
            .map_or(0, |t| i32::from(ladder[usize::from(t.tier)]));
        at + (tier - drawn) as f32
    };
    let height = f32::from(c.height);
    let mut items = Vec::new();
    if let Some(cell) = &c.cell {
        for br in (0..h).step_by(usize::from(CEILING_BLOCK)) {
            for bc in (0..w).step_by(usize::from(CEILING_BLOCK)) {
                let (c1, r1) = ((bc + CEILING_BLOCK).min(w), (br + CEILING_BLOCK).min(h));
                let mid = [
                    (f32::from(bc) + f32::from(c1) - 1.0) / 2.0,
                    (f32::from(br) + f32::from(r1) - 1.0) / 2.0,
                ];
                let Some(anchor) = free_tile(bc, br, c1, r1, mid) else {
                    continue;
                };
                let tiles: Vec<[i32; 2]> = (br..r1)
                    .flat_map(|r| (bc..c1).map(move |cc| [cc, r]))
                    .map(|[cc, r]| {
                        [
                            i32::from(cc) - i32::from(anchor[0]),
                            i32::from(r) - i32::from(anchor[1]),
                        ]
                    })
                    .collect();
                items.push(Item {
                    tile: anchor,
                    src: ItemSrc::Raw(underside_tmd(cell, c.shade, &tiles)?),
                    offset: [0.0; 3],
                    rotation: [0.0; 3],
                    scale: 1.0,
                    lift: Some(lift_to(height, anchor)),
                });
            }
        }
    }
    if let Some(chain) = &c.chain {
        let hung = def
            .props
            .iter()
            .filter(|p| p.hangs)
            .map(|p| (&p.id, &p.mesh, p.tile, p.offset, p.rotation, p.scale))
            .chain(
                def.decor
                    .iter()
                    .filter(|d| d.hangs)
                    .map(|d| (&d.id, &d.mesh, d.tile, d.offset, d.rotation, d.scale)),
            );
        for (id, mesh, tile, offset, rotation, scale) in hung {
            if !chain.items.is_empty() && !chain.items.iter().any(|i| i == id) {
                continue;
            }
            if usize::from(tile[0]) >= grid.width || usize::from(tile[1]) >= grid.height {
                continue;
            }
            let floor = match surface {
                Some(s) => {
                    let (cx, cz) = def.tile_centre(tile[0], tile[1]);
                    s.field
                        .sample_world(cx + i32::from(offset[0]), cz + i32::from(offset[2]))
                        .unwrap_or(0) as f32
                }
                None => grid
                    .get(usize::from(tile[0]), usize::from(tile[1]))
                    .map_or(0.0, |t| f32::from(ladder[usize::from(t.tier)])),
            };
            let top = if rotation[0] == 0.0 && rotation[2] == 0.0 {
                mesh_top(lib, mesh) * scale.unwrap_or(1.0)
            } else {
                0.0
            };
            let base = floor + f32::from(offset[1]) + top;
            let len = (height - base).round() as i32;
            if len <= 0 {
                continue;
            }
            // Anchored on the item's own tile when its cell takes
            // decorations, else on the nearest that does, the offset
            // carrying it back.
            let at = [f32::from(tile[0]), f32::from(tile[1])];
            let (c0, r0) = (tile[0].saturating_sub(2), tile[1].saturating_sub(2));
            let Some(anchor) = free_tile(c0, r0, (tile[0] + 3).min(w), (tile[1] + 3).min(h), at)
            else {
                continue;
            };
            let dx = (f32::from(tile[0]) - f32::from(anchor[0])) * 128.0;
            let dz = (f32::from(tile[1]) - f32::from(anchor[1])) * 128.0;
            items.push(Item {
                tile: anchor,
                src: ItemSrc::Raw(chain_tmd(chain, len)?),
                offset: [f32::from(offset[0]) + dx, 0.0, f32::from(offset[2]) + dz],
                rotation: [0.0; 3],
                scale: 1.0,
                lift: Some(lift_to(base, anchor)),
            });
        }
    }
    Ok(items)
}

/// How far `mesh` reaches above its origin, world units (`0` when the
/// library cannot measure it).
fn mesh_top(lib: Option<&MeshLibrary>, mesh: &PropMesh) -> f32 {
    let Some(raw) = lib.and_then(|l| l.raw(mesh)) else {
        return 0.0;
    };
    let Ok(tmd) = legaia_tmd::parse(raw) else {
        return 0.0;
    };
    tmd.objects
        .iter()
        .flat_map(|o| o.vertices.iter().map(|v| -f32::from(v.y)))
        .fold(0.0, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The visible side of each triangle as the renderers' mesh build emits
    /// it: `-cross(e1, e2)` (the parity `coplanar_draws` documents).
    fn visible_sides(raw: &[u8]) -> Vec<[f32; 3]> {
        let tmd = legaia_tmd::parse(raw).expect("parses");
        let (m, _) = legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids_lenient(&tmd, raw);
        m.indices
            .chunks(3)
            .map(|t| {
                let [a, b, c] = [0, 1, 2].map(|k| m.positions[t[k] as usize]);
                let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                [
                    -(e1[1] * e2[2] - e1[2] * e2[1]),
                    -(e1[2] * e2[0] - e1[0] * e2[2]),
                    -(e1[0] * e2[1] - e1[1] * e2[0]),
                ]
            })
            .collect()
    }

    const CELL: AtlasCell = AtlasCell {
        tpage: 28,
        clut: 32576,
        tile: 2,
    };

    #[test]
    fn the_underside_faces_down_only() {
        let raw = underside_tmd(&CELL, 48, &[[0, 0], [1, 0], [0, 1]]).unwrap();
        let sides = visible_sides(&raw);
        assert_eq!(sides.len(), 6, "two triangles a tile, one winding");
        // Y down: a face seen from beneath has its visible side along +Y.
        assert!(
            sides
                .iter()
                .all(|s| s[1] > 0.0 && s[0] == 0.0 && s[2] == 0.0)
        );
    }

    #[test]
    fn a_chain_rises_from_its_origin_and_faces_out() {
        let c = ChainDef {
            tpage: 28,
            clut: 32576,
            tile: 2,
            width: 8,
            shade: 80,
            items: Vec::new(),
        };
        let raw = chain_tmd(&c, 300).unwrap();
        let tmd = legaia_tmd::parse(&raw).unwrap();
        let ys: Vec<i16> = tmd.objects[0].vertices.iter().map(|v| v.y).collect();
        assert_eq!(ys.iter().copied().max(), Some(0));
        assert_eq!(ys.iter().copied().min(), Some(-300));
        assert!(
            tmd.objects[0]
                .vertices
                .iter()
                .all(|v| v.x.abs() == 4 && v.z.abs() == 4)
        );
        // Each side's visible direction points away from the shaft's axis.
        let (m, _) = legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids_lenient(&tmd, &raw);
        for (t, s) in m.indices.chunks(3).zip(visible_sides(&raw)) {
            let centre = [0, 1, 2]
                .map(|k| m.positions[t[k] as usize])
                .iter()
                .fold([0.0f32; 3], |a, p| [a[0] + p[0], a[1] + p[1], a[2] + p[2]]);
            assert!(s[0] * centre[0] + s[2] * centre[2] > 0.0, "faces out");
            assert_eq!(s[1], 0.0);
        }
    }
}

//! A pack map's sculpted terrain as meshes
//! ([`legaia_expansion_format::terrain`]).
//!
//! The terrain draws through the decoration-cell path both hosts already
//! run: every map tile gets one mesh holding its share of the heightfield's
//! quads (an edge tile also holds the skirt beyond it), in the cell frame -
//! origin at the tile centre on floor tier `0`, `Y` down - and the map's
//! decoration bake folds it into the tile's cell with the tile's other
//! decorations. Each quad is two `GT3` triangles split along the same
//! diagonal [`Heightfield::sample_world`] interpolates over, so the floor
//! the player walks is the surface that draws.
//!
//! Three layers, in this order:
//!
//! - **Ground**: textured, the quad's [`QuadPaint`] cell under the
//!   surface's vertex colours (material tint, macro variation, baked sun).
//! - **Water**: a flat textured surface at the water level over every quad
//!   whose ground dips below it, coloured by depth. The opaque surface
//!   meets the ground in the depth buffer, so the shoreline follows the
//!   sculpted slope rather than the quad grid.
//! - **Foam**: an additive band over the shallows, brightest at the
//!   waterline (its vertex colours fade to black, which adds nothing),
//!   textured from the engine's own [`FoamSheet`]: four palettes for four
//!   depth bands, which the map's surf pulse animates
//!   (`World::step_expansion_surf`).

use anyhow::{Context, Result};
use legaia_expansion_format::map::{MapDef, TileGrid};
use legaia_expansion_format::map_detail::AtlasCell;
use legaia_expansion_format::terrain::{Heightfield, TerrainSurface, orient_uv};
use legaia_scene_formats::field_objects::{GROUND_ATLAS_AXIS, GROUND_ATLAS_TILE_PX};
use legaia_tmd::descriptor::PacketShape;
use legaia_tmd::encode::{LEGAIA_OBJECT_SCALE, ModelGroup, ModelObject, ModelPrim, encode};

use super::detail::VramPatch;
use super::vram_alloc::{VramAlloc, cba_word};

use legaia_expansion_format::terrain::{FOAM_BANDS, FOAM_INDEX, foam_palette};

/// The foam's texture: a `4 x 4` 4bpp block of index [`FOAM_INDEX`] and one
/// 16-entry palette per depth band, entry [`FOAM_INDEX`] white with the STP
/// bit (so the band blends) and the rest zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoamSheet {
    /// TSB page bits (4bpp, blend mode clear).
    pub tpage: u16,
    pub u0: u8,
    pub v0: u8,
    /// CBA word of each depth band's palette, shallowest first.
    pub cluts: [u16; FOAM_BANDS],
}

/// Allocate the [`FoamSheet`] out of `alloc`, its words appended to
/// `patches`.
pub fn foam_sheet(alloc: &mut VramAlloc, patches: &mut Vec<VramPatch>) -> Result<FoamSheet> {
    let (rect, (px, py)) = alloc
        .alloc_texture(1, 4, 4, 1)
        .context("no free VRAM for the foam texture")?;
    let word = (FOAM_INDEX as u16) * 0x1111;
    patches.push(VramPatch {
        x: rect.x,
        y: rect.y,
        w: rect.w,
        h: rect.h,
        words: vec![word; usize::from(rect.w) * usize::from(rect.h)],
    });
    let mut cluts = [0u16; FOAM_BANDS];
    for slot in &mut cluts {
        let c = alloc
            .alloc_clut(16)
            .context("no free VRAM for a foam palette")?;
        patches.push(VramPatch {
            x: c.x,
            y: c.y,
            w: 16,
            h: 1,
            words: foam_palette(31),
        });
        *slot = cba_word(c.x, c.y);
    }
    Ok(FoamSheet {
        tpage: (px / 64) | ((py / 256) << 4),
        u0: ((rect.x - px) * 4) as u8,
        v0: (rect.y - py) as u8,
        cluts,
    })
}

/// A quad `a b / c d`'s two triangles, wound to face up (`-Y`, the field
/// frame's up) so the field draw's facing test keeps them seen from above:
/// `(a, c, b)` and `(b, c, d)`, split along `b`-`c` as the floor sampler
/// interpolates.
const UP: [[usize; 3]; 2] = [[0, 2, 1], [1, 2, 3]];

/// Rise over run past which a ground quad maps its texture from the side.
const SIDE_MAP_SLOPE: f32 = 0.8;

/// Copy every palette a soft boundary blends through
/// ([`legaia_expansion_format::terrain::QuadPaint::overlay`]) with the STP
/// bit on each non-zero entry, out of `alloc`, its words appended to
/// `patches`: a textured primitive blends only its STP texels, and a disc
/// ground palette carries none. Returns original CBA word to copy's.
pub fn stp_palettes(
    s: &TerrainSurface,
    vram: &legaia_tim::Vram,
    alloc: &mut VramAlloc,
    patches: &mut Vec<VramPatch>,
) -> Result<std::collections::BTreeMap<u16, u16>> {
    let mut out = std::collections::BTreeMap::new();
    for (cell, _) in s.paint.iter().flatten().flatten().filter_map(|p| p.overlay) {
        if out.contains_key(&cell.clut) {
            continue;
        }
        // 4bpp pages take 16 entries, 8bpp 256 (TSB bits 7..=8).
        let n: u16 = if (u16::from(cell.tpage) >> 7) & 3 == 1 {
            256
        } else {
            16
        };
        let (x, y) = ((cell.clut & 0x3F) * 16, cell.clut >> 6);
        let words: Vec<u16> = (0..n)
            .map(|k| {
                let w = vram.pixel(usize::from(x + k).min(1023), usize::from(y).min(511));
                if w & 0x7FFF == 0 { w } else { w | 0x8000 }
            })
            .collect();
        let c = alloc
            .alloc_clut(n)
            .context("no free VRAM for a terrain blend palette")?;
        patches.push(VramPatch {
            x: c.x,
            y: c.y,
            w: n,
            h: 1,
            words,
        });
        out.insert(cell.clut, cba_word(c.x, c.y));
    }
    Ok(out)
}

/// The TSB blend-mode bits of an additive (`B + F`) primitive.
const ABR_ADD: u16 = 1 << 5;

/// The map tile that owns quad column `i` (or row): its own tile, the edge
/// tile for a skirt quad.
fn owner(q: usize, res: usize, skirt: usize, len: usize) -> usize {
    ((q / res) as isize - skirt as isize).clamp(0, len as isize - 1) as usize
}

/// One mesh per map tile holding the tile's terrain, keyed by map-local
/// tile, row-major. A tile with nothing to draw is left out.
///
/// `stp` maps a palette to its blend copy ([`stp_palettes`]); a quad whose
/// overlay palette has none draws without its overlay.
pub fn tile_meshes(
    def: &MapDef,
    grid: &TileGrid,
    s: &TerrainSurface,
    foam: Option<&FoamSheet>,
    stp: &std::collections::BTreeMap<u16, u16>,
) -> Result<Vec<([u16; 2], Vec<u8>)>> {
    let f = &s.field;
    let (res, skirt) = (f.res, f.skirt);
    let qc = f.cols - 1;
    let mut buckets: Vec<Vec<(usize, usize)>> = vec![Vec::new(); grid.width * grid.height];
    for j in 0..f.rows - 1 {
        let r = owner(j, res, skirt, grid.height);
        for i in 0..qc {
            let c = owner(i, res, skirt, grid.width);
            buckets[r * grid.width + c].push((i, j));
        }
    }
    let mut out = Vec::new();
    for (k, quads) in buckets.iter().enumerate() {
        if quads.is_empty() {
            continue;
        }
        let (c, r) = ((k % grid.width) as u16, (k / grid.width) as u16);
        let (cx, cz) = def.tile_centre(c, r);
        if let Some(raw) = tile_mesh(s, quads, [cx, cz], foam, stp)? {
            out.push(([c, r], raw));
        }
    }
    Ok(out)
}

/// The world-space position of vertex `(i, j)` in the cell frame of the
/// tile centred at `centre`: `Y` down.
fn vertex(f: &Heightfield, i: usize, j: usize, y: f32, centre: [i32; 2]) -> [i16; 3] {
    let x = f.origin[0] + i as i32 * f.step - centre[0];
    let z = f.origin[1] + j as i32 * f.step - centre[1];
    [
        x.clamp(-32768, 32767) as i16,
        (-y).round().clamp(-32768.0, 32767.0) as i16,
        z.clamp(-32768, 32767) as i16,
    ]
}

fn tile_mesh(
    s: &TerrainSurface,
    quads: &[(usize, usize)],
    centre: [i32; 2],
    foam: Option<&FoamSheet>,
    stp: &std::collections::BTreeMap<u16, u16>,
) -> Result<Option<Vec<u8>>> {
    let f = &s.field;
    let res = f.res;
    let qc = f.cols - 1;
    let mut vertices: Vec<[i16; 3]> = Vec::new();
    let mut index: std::collections::HashMap<[i16; 3], u16> = Default::default();
    let mut vert = |p: [i16; 3]| -> u16 {
        *index.entry(p).or_insert_with(|| {
            vertices.push(p);
            (vertices.len() - 1) as u16
        })
    };
    let px = usize::from(GROUND_ATLAS_TILE_PX);
    let mut ground = Vec::new();
    let mut water = Vec::new();
    let mut foam_prims = Vec::new();
    let mut soft = Vec::new();
    for &(i, j) in quads {
        let corners = [(i, j), (i + 1, j), (i, j + 1), (i + 1, j + 1)];
        let h = |(a, b): (usize, usize)| f32::from(f.heights[b * f.cols + a]);
        let pair = s.paint[j * qc + i];
        if pair.iter().any(Option::is_some) {
            // Tile-local fraction of each corner; V runs against Z, as the
            // retail ground maps it (`build_ground_heightfield`). A steep
            // quad maps from the side instead - U along the face, V down
            // its height - so a cliff's texture is not smeared down it.
            let hs = corners.map(h);
            let (lo, hi) = (
                hs.iter().copied().fold(f32::MAX, f32::min),
                hs.iter().copied().fold(f32::MIN, f32::max),
            );
            let steep = hi - lo > f.step as f32 * SIDE_MAP_SLOPE;
            let gx = (hs[1] - hs[0] + hs[3] - hs[2]).abs();
            let gz = (hs[2] - hs[0] + hs[3] - hs[1]).abs();
            let band = (lo / 128.0).floor() * 128.0;
            let span = (hi - band).max(128.0);
            let frac = |q: usize, lo_q: usize| {
                (q % res) as f32 / res as f32
                    + if q.is_multiple_of(res) && q > lo_q {
                        1.0
                    } else {
                        0.0
                    }
            };
            let uv = |cell: AtlasCell, orient: u8, (a, b): (usize, usize)| -> (u8, u8) {
                let u0 = (usize::from(cell.tile) % GROUND_ATLAS_AXIS) * px;
                let v0 = (usize::from(cell.tile) / GROUND_ATLAS_AXIS % GROUND_ATLAS_AXIS) * px;
                let (fu, fv) = if steep {
                    let along = if gx > gz { frac(b, j) } else { frac(a, i) };
                    (along, 1.0 - (h((a, b)) - band) / span)
                } else {
                    (frac(a, i), 1.0 - frac(b, j))
                };
                let (ou, ov) = orient_uv(orient, fu, fv);
                (
                    (u0 as f32 + ou * (px as f32 - 1.0)).round().min(255.0) as u8,
                    (v0 as f32 + ov * (px as f32 - 1.0)).round().min(255.0) as u8,
                )
            };
            let ids = corners.map(|c| vert(vertex(f, c.0, c.1, h(c), centre)));
            let cols = corners.map(|(a, b)| s.colors[b * f.cols + a]);
            for (tri, p) in UP.iter().zip(pair) {
                let Some(p) = p else { continue };
                let uvs = corners.map(|c| uv(p.cell, p.orient, c));
                ground.push(ModelPrim {
                    vertices: tri.iter().map(|&t| ids[t]).collect(),
                    uvs: tri.iter().map(|&t| uvs[t]).collect(),
                    cba: p.cell.clut,
                    tsb: u16::from(p.cell.tpage),
                    colors: tri.iter().map(|&t| cols[t]).collect(),
                });
                // The soft boundary: the runner-up half-blended over the
                // triangle (ABR 0, `0.5B + 0.5F`) from its palette's STP copy.
                if let Some((cell, orient)) = p.overlay
                    && let Some(&clut) = stp.get(&cell.clut)
                {
                    let uvs = corners.map(|c| uv(cell, orient, c));
                    soft.push(ModelPrim {
                        vertices: tri.iter().map(|&t| ids[t]).collect(),
                        uvs: tri.iter().map(|&t| uvs[t]).collect(),
                        cba: clut,
                        tsb: u16::from(cell.tpage) & !0x60,
                        colors: tri.iter().map(|&t| cols[t]).collect(),
                    });
                }
            }
        }
        let Some(w) = &s.water else { continue };
        if !corners.iter().any(|&c| h(c) < w.level) {
            continue;
        }
        if let Some(cell) = w.cells.first().copied() {
            let pick = if w.cells.len() > 1 {
                let k = legaia_expansion_format::map_detail::splitmix64(
                    ((i / res) as u64) << 32 | (j / res) as u64,
                );
                w.cells[k as usize % w.cells.len()]
            } else {
                cell
            };
            let u0 = (usize::from(pick.tile) % GROUND_ATLAS_AXIS) * px;
            let v0 = (usize::from(pick.tile) / GROUND_ATLAS_AXIS % GROUND_ATLAS_AXIS) * px;
            let uv = |(a, b): (usize, usize)| -> (u8, u8) {
                let fu = (a % res) as f32 / res as f32
                    + if a.is_multiple_of(res) && a > i {
                        1.0
                    } else {
                        0.0
                    };
                let fz = (b % res) as f32 / res as f32
                    + if b.is_multiple_of(res) && b > j {
                        1.0
                    } else {
                        0.0
                    };
                (
                    (u0 as f32 + fu * (px as f32 - 1.0)).round() as u8,
                    (v0 as f32 + (1.0 - fz) * (px as f32 - 1.0)).round() as u8,
                )
            };
            let ids = corners.map(|c| vert(vertex(f, c.0, c.1, w.level, centre)));
            let uvs = corners.map(uv);
            let cols = corners.map(|(a, b)| s.water_color(a, b));
            for tri in UP {
                water.push(ModelPrim {
                    vertices: tri.iter().map(|&t| ids[t]).collect(),
                    uvs: tri.iter().map(|&t| uvs[t]).collect(),
                    cba: pick.clut,
                    tsb: u16::from(pick.tpage),
                    colors: tri.iter().map(|&t| cols[t]).collect(),
                });
            }
        }
        if let Some(sheet) = foam {
            let fo = corners.map(|(a, b)| s.foam_at(a, b));
            if fo.iter().all(|v| *v <= 0.0) {
                continue;
            }
            let mean_depth = corners.iter().map(|&c| w.level - h(c)).sum::<f32>() / 4.0;
            let band = ((mean_depth.max(0.0) / w.foam.max(1.0)) * FOAM_BANDS as f32) as usize;
            let clut = sheet.cluts[band.min(FOAM_BANDS - 1)];
            let ids = corners.map(|c| vert(vertex(f, c.0, c.1, w.level + 1.0, centre)));
            let cols = fo.map(|k| w.foam_color.map(|c| (f32::from(c) * k).round() as u8));
            let uv = (sheet.u0 + 1, sheet.v0 + 1);
            for tri in UP {
                foam_prims.push(ModelPrim {
                    vertices: tri.iter().map(|&t| ids[t]).collect(),
                    uvs: vec![uv; 3],
                    cba: clut,
                    tsb: sheet.tpage | ABR_ADD,
                    colors: tri.iter().map(|&t| cols[t]).collect(),
                });
            }
        }
    }
    let mut groups = Vec::new();
    for (prims, semi) in [
        (ground, false),
        (water, false),
        (soft, true),
        (foam_prims, true),
    ] {
        if !prims.is_empty() {
            groups.push(ModelGroup {
                shape: PacketShape::Gt3,
                semi_transparent: semi,
                prims,
            });
        }
    }
    if groups.is_empty() {
        return Ok(None);
    }
    encode(&[ModelObject {
        vertices,
        groups,
        scale: LEGAIA_OBJECT_SCALE,
    }])
    .map(Some)
}

/// The horizon ring ([`legaia_expansion_format::terrain::SkyDef`]): a
/// cylinder round the map's centre, `radius` tiles past its half-diagonal,
/// from `span[0]` to `span[1]` up, in the cell frame of the map's centre
/// tile. Returns that tile and the mesh.
pub fn sky_mesh(
    def: &MapDef,
    grid: &TileGrid,
    sky: &legaia_expansion_format::terrain::SkyDef,
) -> Result<([u16; 2], Vec<u8>)> {
    let anchor = [(grid.width / 2) as u16, (grid.height / 2) as u16];
    let (ax, az) = def.tile_centre(anchor[0], anchor[1]);
    let o = def.tiles.origin.map(|v| f32::from(v) * 128.0);
    let (mx, mz) = (
        o[0] + grid.width as f32 * 64.0,
        o[1] + grid.height as f32 * 64.0,
    );
    let half_diag = (grid.width as f32).hypot(grid.height as f32) * 64.0;
    let r = half_diag + sky.radius * 128.0;
    let n = usize::from(sky.segments.max(3));
    let copies = f32::from(sky.repeat.max(1));
    let [u0, v0, u1, v1] = sky.rect.map(f32::from);
    let mut vertices: Vec<[i16; 3]> = Vec::new();
    let mut prims = Vec::new();
    let at = |k: usize, y: f32| -> [i16; 3] {
        let a = k as f32 / n as f32 * std::f32::consts::TAU;
        [
            (mx + r * a.cos() - ax as f32).round() as i16,
            (-y).round() as i16,
            (mz + r * a.sin() - az as f32).round() as i16,
        ]
    };
    for k in 0..n {
        let t0 = k as f32 / n as f32 * copies;
        let t1 = (k + 1) as f32 / n as f32 * copies;
        let copy = t0.floor();
        let u = |t: f32| {
            let f = (t - copy).clamp(0.0, 1.0);
            let f = if copy as i32 % 2 == 1 { 1.0 - f } else { f };
            (u0 + (u1 - u0) * f).round() as u8
        };
        let (ua, ub) = (u(t0), u(t1));
        let base = vertices.len() as u16;
        vertices.extend([
            at(k, sky.span[1]),
            at(k + 1, sky.span[1]),
            at(k, sky.span[0]),
            at(k + 1, sky.span[0]),
        ]);
        let uvs = [
            (ua, v0 as u8),
            (ub, v0 as u8),
            (ua, v1 as u8),
            (ub, v1 as u8),
        ];
        for tri in [[0u16, 1, 2], [1, 3, 2], [0, 2, 1], [1, 2, 3]] {
            prims.push(ModelPrim {
                vertices: tri.iter().map(|&t| base + t).collect(),
                uvs: tri.iter().map(|&t| uvs[usize::from(t)]).collect(),
                cba: sky.clut,
                tsb: u16::from(sky.tpage),
                colors: vec![sky.color],
            });
        }
    }
    // Both windings: the ring reads from inside and out.
    let raw = encode(&[ModelObject {
        vertices,
        groups: vec![ModelGroup {
            shape: PacketShape::Ft3,
            semi_transparent: false,
            prims,
        }],
        scale: LEGAIA_OBJECT_SCALE,
    }])?;
    Ok((anchor, raw))
}

#[cfg(test)]
mod tests;

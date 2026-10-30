//! The measuring half of the borrowed-mesh catalog
//! (`legaia_expansion_format::mesh_catalog`): every mesh of a field
//! scene's geometry pack, measured the way a pack map's placement of it is
//! (`expansion_map::footprint` at scale 1, unrotated), and rendered into a
//! numbered contact sheet the labels are read from.
//!
//! Behind `legaia-engine mesh-catalog`. The sheet is Sony-derived (it draws
//! the disc's meshes in the disc's textures) and stays local; only the
//! numbers and the hand-written labels are committed.

use anyhow::{Context, Result};
use legaia_actor_formats::mesh_raster::{self, RasterOptions};
use legaia_engine_scene::expansion_map::detail::field_resources;
use legaia_engine_scene::expansion_map::footprint::{footprint, hull_extent};
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_tim::Vram;

/// One mesh of a scene's geometry pack, measured.
#[derive(Debug, Clone)]
pub struct MeasuredMesh {
    /// Index in the pack (a map's `object`).
    pub object: u16,
    /// Base-footprint width (`X`), depth (`Z`) and height, world units.
    pub size: [u16; 3],
    /// `Footprint::is_large_obstacle` at rest on the floor.
    pub solid: bool,
    pub tris: u32,
    /// The mesh's bytes, for rendering.
    pub raw: Vec<u8>,
}

/// A scene's geometry pack, measured, with the VRAM its meshes sample.
pub struct ScenePack {
    pub scene: String,
    pub meshes: Vec<MeasuredMesh>,
    pub vram: Vram,
}

/// Measure every mesh of `scene`'s field geometry pack.
pub fn measure_scene(index: &ProtIndex, scene: &str) -> Result<ScenePack> {
    let base = Scene::load(index, scene).with_context(|| format!("scene {scene}"))?;
    let res = field_resources(index, &base).context("the scene's field resources")?;
    let raws: Vec<Vec<u8>> = legaia_engine_scene::field_env::env_pack_tmd_indices(&base, &res)
        .into_iter()
        .map(|i| res.tmds[i].raw.clone())
        .collect();
    let mut meshes = Vec::new();
    for (i, raw) in raws.into_iter().enumerate() {
        let Ok(object) = u16::try_from(i) else { break };
        let tmd = legaia_tmd::parse(&raw).ok();
        let tris = tmd.as_ref().map_or(0, |t| {
            let (vm, _) = legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids_lenient(t, &raw);
            (vm.indices.len() / 3) as u32
        });
        let c = |v: f32| v.round().clamp(0.0, f32::from(u16::MAX)) as u16;
        let (size, solid) = match footprint(&raw, [0.0; 3], 1.0) {
            Some(f) => {
                let [w, d] = hull_extent(&f.hull);
                ([c(w), c(d), c(f.top - f.bottom)], f.is_large_obstacle(0.0))
            }
            // A flat card (a sprite tree, a fern) has no floor-plane hull:
            // its bounding box, and it closes nothing.
            None => {
                let mut lo = [f32::MAX; 3];
                let mut hi = [f32::MIN; 3];
                for v in tmd
                    .iter()
                    .flat_map(|t| t.objects.iter().flat_map(|o| o.vertices.iter()))
                {
                    for (k, q) in [v.x, v.y, v.z].into_iter().enumerate() {
                        lo[k] = lo[k].min(f32::from(q));
                        hi[k] = hi[k].max(f32::from(q));
                    }
                }
                let e = |k: usize| if hi[k] >= lo[k] { c(hi[k] - lo[k]) } else { 0 };
                ([e(0), e(2), e(1)], false)
            }
        };
        meshes.push(MeasuredMesh {
            object,
            size,
            solid,
            tris,
            raw,
        });
    }
    Ok(ScenePack {
        scene: scene.to_string(),
        meshes,
        vram: res.vram,
    })
}

/// One mesh drawn into a `px x px` RGBA8 thumbnail from three-quarters
/// above (the follow camera's side). Empty pixels are the background.
pub fn thumbnail(mesh: &MeasuredMesh, vram: &Vram, px: usize) -> Vec<u8> {
    let background = [24, 26, 32, 255];
    let Ok(tmd) = legaia_tmd::parse(&mesh.raw) else {
        return vec_of(px, background);
    };
    let (vm, ids) = legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids_lenient(&tmd, &mesh.raw);
    let opts = RasterOptions {
        width: px,
        height: px,
        yaw: 30f32.to_radians(),
        pitch: -28f32.to_radians(),
        margin: 0.08,
        background,
        shade: 0.35,
        auto_orient: false,
    };
    let img = mesh_raster::render_posed(&vm, &ids, &[], vram, &opts);
    if img.is_empty() {
        vec_of(px, background)
    } else {
        img
    }
}

fn vec_of(px: usize, c: [u8; 4]) -> Vec<u8> {
    c.repeat(px * px)
}

/// Every mesh of `pack` in a grid of `cols` columns, `px` a cell, with
/// each cell's object number drawn in its top-left corner. Returns
/// `(width, height, rgba)`.
pub fn contact_sheet(pack: &ScenePack, px: usize, cols: usize) -> (usize, usize, Vec<u8>) {
    let cols = cols.max(1);
    let rows = pack.meshes.len().div_ceil(cols).max(1);
    let (w, h) = (cols * px, rows * px);
    let mut out = [10u8, 10, 12, 255].repeat(w * h);
    for (n, m) in pack.meshes.iter().enumerate() {
        let (cx, cy) = ((n % cols) * px, (n / cols) * px);
        let mut cell = thumbnail(m, &pack.vram, px);
        // A one-pixel frame between cells.
        for i in 0..px {
            for p in [i * px, i * px + px - 1, i, (px - 1) * px + i] {
                cell[p * 4..p * 4 + 4].copy_from_slice(&[60, 60, 70, 255]);
            }
        }
        draw_number(&mut cell, px, u32::from(m.object), 3, 3);
        if m.solid {
            // A red corner tick: stands in the player's way.
            for y in 0..6 {
                for x in px - 7..px - 1 {
                    let p = (y + 1) * px + x;
                    cell[p * 4..p * 4 + 4].copy_from_slice(&[230, 70, 60, 255]);
                }
            }
        }
        mesh_raster::blit(
            &mut out,
            w,
            h,
            &mesh_raster::Rgba {
                width: px,
                height: px,
                pixels: &cell,
            },
            cx,
            cy,
        );
    }
    (w, h, out)
}

/// A 3x5 pixel digit font, scaled 2x, white on a dark box.
fn draw_number(img: &mut [u8], px: usize, n: u32, x0: usize, y0: usize) {
    const GLYPHS: [u16; 10] = [
        0b111_101_101_101_111,
        0b010_110_010_010_111,
        0b111_001_111_100_111,
        0b111_001_111_001_111,
        0b101_101_111_001_001,
        0b111_100_111_001_111,
        0b111_100_111_101_111,
        0b111_001_010_010_010,
        0b111_101_111_101_111,
        0b111_101_111_001_111,
    ];
    let digits: Vec<u32> = n.to_string().bytes().map(|b| u32::from(b - b'0')).collect();
    let s = 2;
    let bw = digits.len() * 4 * s + s;
    let bh = 5 * s + 2 * s;
    let mut put = |x: usize, y: usize, c: [u8; 4]| {
        if x < px && y < px {
            let p = y * px + x;
            img[p * 4..p * 4 + 4].copy_from_slice(&c);
        }
    };
    for y in 0..bh {
        for x in 0..bw {
            put(x0 + x, y0 + y, [0, 0, 0, 255]);
        }
    }
    for (k, &d) in digits.iter().enumerate() {
        let g = GLYPHS[d as usize];
        for row in 0..5 {
            for col in 0..3 {
                if g >> (14 - (row * 3 + col)) & 1 == 1 {
                    for dy in 0..s {
                        for dx in 0..s {
                            put(
                                x0 + s + (k * 4 + col) * s + dx,
                                y0 + s + row * s + dy,
                                [255, 255, 255, 255],
                            );
                        }
                    }
                }
            }
        }
    }
}

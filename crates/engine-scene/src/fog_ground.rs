//! The volumetric fog's **ground coverage field** for one assembled field
//! scene: the glue between the scene's drawn geometry and the wgpu-free
//! rasteriser in [`legaia_render_kernels::fog_volume::FogGroundBuilder`].
//!
//! Both play hosts build it once per scene load from the same inputs they
//! already hold - the static occluder set ([`FieldOccluders`], every opaque
//! terrain and placement triangle) and the walk-ground heightfield - and hand
//! it to the `World`'s bank, so the field the bank lies on is one kernel's
//! output on both. The walk grid's floor ladder is the roof test
//! ([`legaia_render_kernels::fog_volume::ROOF_CLEARANCE`]): a surface well
//! above the walkable floor under it is a roof or canopy, not ground.

use crate::field_occlusion::FieldOccluders;
use legaia_render_kernels::fog_volume::{FogGround, FogGroundBuilder};
use legaia_scene_formats::field_objects::WalkHeightfield;

/// Walk-grid tiles per side (`0x80`) and their size in world units.
const GRID_STRIDE: usize = 0x80;
const TILE: f32 = 128.0;

/// The walk floor the roof test compares against: the collision grid's
/// floor tier at the tile under `(x, z)` through the MAN floor ladder
/// (`-lut[tier]`, the heightfield's own frame). Nearest tile, no bilinear -
/// the test only needs the tier's level.
pub fn walk_floor_sampler<'a>(
    grid: Option<&'a [u8]>,
    lut: Option<&'a [i16; 16]>,
) -> impl Fn(f32, f32) -> Option<f32> + 'a {
    move |x, z| {
        let (g, l) = (grid?, lut?);
        if x < 0.0 || z < 0.0 {
            return None;
        }
        let (tx, tz) = ((x / TILE) as usize, (z / TILE) as usize);
        if tx >= GRID_STRIDE || tz >= GRID_STRIDE {
            return None;
        }
        let tier = *g.get(tz * GRID_STRIDE + tx)? & 0x0F;
        Some(-f32::from(l[tier as usize]))
    }
}

/// The field for one loaded field scene, from the static draw lists the host
/// resolved (terrain + placements, the occluder set's input) - the one call
/// both play hosts make at scene load. `None` for a world-map scene (the
/// bank never draws there) and for a scene with no geometry.
pub fn build_for_scene(
    draw_lists: &[&[crate::field_env::EnvDraw]],
    res: &crate::scene_resources::SceneResources,
    scene: &crate::scene::Scene,
    index: &crate::scene::ProtIndex,
) -> Option<FogGround> {
    if crate::scene::is_world_map_scene(&scene.name) {
        return None;
    }
    let surfaces = FieldOccluders::build_surfaces(draw_lists, res);
    let ground = scene.walk_heightfield(index).ok().flatten();
    let grid = scene.field_collision_grid(index).ok().flatten();
    let lut = scene.field_floor_height_lut(index).ok().flatten();
    build(
        &surfaces,
        ground.as_ref(),
        walk_floor_sampler(grid.as_deref(), lut.as_ref()),
    )
}

/// Rasterise the scene's floor: the occluder set's up-facing triangles plus
/// the walk heightfield's, resolved against `walk_floor`
/// ([`walk_floor_sampler`]). `None` when the scene has no geometry at all.
pub fn build(
    occluders: &FieldOccluders,
    ground: Option<&WalkHeightfield>,
    walk_floor: impl Fn(f32, f32) -> Option<f32>,
) -> Option<FogGround> {
    let mut hf_tris: Vec<[[f32; 3]; 3]> = Vec::new();
    if let Some(hf) = ground {
        for tri in hf.indices.as_chunks::<3>().0 {
            let (Some(a), Some(b), Some(c)) = (
                hf.positions.get(tri[0] as usize),
                hf.positions.get(tri[1] as usize),
                hf.positions.get(tri[2] as usize),
            ) else {
                continue;
            };
            hf_tris.push([*a, *b, *c]);
        }
    }
    let tris = occluders.triangles();
    if tris.is_empty() && hf_tris.is_empty() {
        return None;
    }
    let mut b = FogGroundBuilder::for_triangles(tris.iter().chain(hf_tris.iter()));
    for t in tris.iter().chain(hf_tris.iter()) {
        b.add_triangle(t);
    }
    Some(b.finish(walk_floor))
}

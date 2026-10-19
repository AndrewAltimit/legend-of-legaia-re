//! The volumetric fog's **ground coverage field** for one assembled field
//! scene: the glue between the scene's drawn geometry and the wgpu-free
//! rasteriser in [`legaia_render_kernels::fog_volume::FogGroundBuilder`].
//!
//! Both play hosts build it once per scene load from the same inputs they
//! already hold - the static draws they put on screen (terrain and the
//! placed objects left after the story-hidden gate, each at its render
//! scale) and the walk-ground heightfield - and hand it to the `World`'s
//! bank, so the field the bank lies on is one kernel's output on both. The
//! walk grid's floor ladder is the roof test
//! ([`legaia_render_kernels::fog_volume::ROOF_CLEARANCE`]): a surface well
//! above the walkable floor under it is a roof or canopy, not ground.
//!
//! The bank answers to what is **seen**, not to what exists in the data
//! ([`FogSurfaces`]): a placed object a script parked or scaled to nothing,
//! a prim whose texels are almost all the transparent word, and a blended
//! veil hung across the air all draw nothing a player reads as solid, so the
//! bank must not wrap around them.

use crate::field_env::EnvDraw;
use crate::scene_resources::SceneResources;
use legaia_render_kernels::fog_volume::{FogGround, FogGroundBuilder};
use legaia_scene_formats::field_objects::WalkHeightfield;

/// Below this opaque-texel fraction a textured prim is an all-but-empty
/// cutout (`Vram::prim_opaque_fraction`): nothing on screen, so not a
/// surface. Low on purpose - a hedge top or a grate is mostly see-through
/// and still ground the bank may lie on.
pub const SURFACE_MIN_OPACITY: f32 = 0.15;

type Tri = [[f32; 3]; 3];

/// The static triangles the bank may lie on, retail Y-down world space.
#[derive(Debug, Clone, Default)]
pub struct FogSurfaces {
    /// Opaque prims: floor candidates and what the spill drapes over.
    pub solid: Vec<Tri>,
    /// Semi-transparent (ABE) prims - a pond's surface, a mist veil. Floor
    /// only where level
    /// ([`FogGroundBuilder::add_see_through`]), never a wall.
    pub see_through: Vec<Tri>,
}

impl FogSurfaces {
    /// Instance every static draw of `terrain` and `placements` (each
    /// placement at `placement_scales[i]`, `1.0` past the slice's end; a
    /// placement at scale `0` draws nothing and is skipped). Animated
    /// draws are skipped, and so are prims whose texture never loaded
    /// (they never draw) or whose texels are almost all transparent
    /// ([`SURFACE_MIN_OPACITY`]).
    pub fn collect(
        terrain: &[EnvDraw],
        placements: &[EnvDraw],
        placement_scales: &[f32],
        res: &SceneResources,
    ) -> Self {
        use std::collections::HashMap;
        type Split = (Vec<[f32; 3]>, Vec<u32>, Vec<u32>);
        let mut cache: HashMap<usize, Split> = HashMap::new();
        let mut out = Self::default();
        let draws = terrain.iter().map(|d| (d, 1.0)).chain(
            placements
                .iter()
                .enumerate()
                .map(|(i, d)| (d, placement_scales.get(i).copied().unwrap_or(1.0))),
        );
        for (d, scale) in draws {
            if d.anim_id != 0 || scale <= 0.0 {
                continue;
            }
            let (positions, solid, see) = match cache.entry(d.res_tmd) {
                std::collections::hash_map::Entry::Occupied(e) => {
                    let (p, s, t) = e.into_mut();
                    (&*p, &*s, &*t)
                }
                std::collections::hash_map::Entry::Vacant(e) => {
                    let Some(rt) = res.tmds.get(d.res_tmd) else {
                        continue;
                    };
                    let (p, s, t) = e.insert(split_mesh(&rt.tmd, &rt.raw, res));
                    (&*p, &*s, &*t)
                }
            };
            let xf =
                |v: [f32; 3]| d.place_point([v[0] * scale, v[1] * scale, v[2] * scale], [0.0; 3]);
            for (idx, dst) in [(solid, &mut out.solid), (see, &mut out.see_through)] {
                for tri in idx.as_chunks::<3>().0 {
                    let (Some(a), Some(b), Some(c)) = (
                        positions.get(tri[0] as usize),
                        positions.get(tri[1] as usize),
                        positions.get(tri[2] as usize),
                    ) else {
                        continue;
                    };
                    dst.push([xf(*a), xf(*b), xf(*c)]);
                }
            }
        }
        out
    }

    /// No triangles at all.
    pub fn is_empty(&self) -> bool {
        self.solid.is_empty() && self.see_through.is_empty()
    }
}

/// One mesh's textured and untextured halves as one position stream, its
/// drawn triangles split into opaque and semi-transparent index lists.
fn split_mesh(
    tmd: &legaia_tmd::Tmd,
    raw: &[u8],
    res: &SceneResources,
) -> (Vec<[f32; 3]>, Vec<u32>, Vec<u32>) {
    let vram = &res.vram;
    let semi = legaia_tmd::mesh::TSB_SEMI_TRANSPARENT_BIT;
    let mesh = legaia_tmd::mesh::tmd_to_vram_mesh_filtered(tmd, raw, |cba, tsb, uvs| {
        vram.prim_has_texture_data(cba, tsb, uvs)
            && vram.prim_opaque_fraction(cba, tsb, uvs) >= SURFACE_MIN_OPACITY
    });
    let mut positions = mesh.positions;
    let (mut solid, mut see) = (Vec::new(), Vec::new());
    // The ABE enable is packed into the built stream's `cba_tsb`, not the
    // filter's raw TSB (see `field_occlusion`).
    for tri in mesh.indices.as_chunks::<3>().0 {
        let abe = mesh
            .cba_tsb
            .get(tri[0] as usize)
            .is_some_and(|ct| ct[1] & semi != 0);
        if abe { &mut see } else { &mut solid }.extend_from_slice(tri);
    }
    let cmesh = legaia_tmd::mesh::tmd_to_color_mesh(tmd, raw);
    let base = positions.len() as u32;
    positions.extend_from_slice(&cmesh.positions);
    for tri in cmesh.indices.as_chunks::<3>().0 {
        let abe = cmesh
            .blend
            .get(tri[0] as usize)
            .is_some_and(|w| w & semi != 0);
        if abe { &mut see } else { &mut solid }.extend(tri.iter().map(|i| i + base));
    }
    (positions, solid, see)
}

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

/// The field for one loaded field scene, from the static draws the host
/// put on screen ([`FogSurfaces::collect`]'s inputs) - the one call both
/// play hosts make at scene load. `None` for a world-map scene (the bank
/// never draws there) and for a scene with no geometry.
pub fn build_for_scene(
    terrain: &[EnvDraw],
    placements: &[EnvDraw],
    placement_scales: &[f32],
    res: &SceneResources,
    scene: &crate::scene::Scene,
    index: &crate::scene::ProtIndex,
) -> Option<FogGround> {
    if crate::scene::is_world_map_scene(&scene.name) {
        return None;
    }
    let surfaces = FogSurfaces::collect(terrain, placements, placement_scales, res);
    let ground = scene.walk_heightfield(index).ok().flatten();
    let grid = scene.field_collision_grid(index).ok().flatten();
    let lut = scene.field_floor_height_lut(index).ok().flatten();
    build(
        &surfaces,
        ground.as_ref(),
        walk_floor_sampler(grid.as_deref(), lut.as_ref()),
    )
}

/// Rasterise the scene's floor: the surfaces' up-facing triangles plus the
/// walk heightfield's (the ground layer both hosts draw), resolved against
/// `walk_floor` ([`walk_floor_sampler`]). `None` when the scene has no
/// geometry at all.
pub fn build(
    surfaces: &FogSurfaces,
    ground: Option<&WalkHeightfield>,
    walk_floor: impl Fn(f32, f32) -> Option<f32>,
) -> Option<FogGround> {
    let mut hf_tris: Vec<Tri> = Vec::new();
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
    if surfaces.is_empty() && hf_tris.is_empty() {
        return None;
    }
    let solid = || surfaces.solid.iter().chain(hf_tris.iter());
    let mut b = FogGroundBuilder::for_triangles(solid().chain(surfaces.see_through.iter()));
    for t in solid() {
        b.add_triangle(t);
    }
    for t in &surfaces.see_through {
        b.add_see_through(t);
    }
    Some(b.finish(walk_floor))
}

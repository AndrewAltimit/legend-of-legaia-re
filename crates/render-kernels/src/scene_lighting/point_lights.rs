//! Point-light derivation: emitter samples, clustering, prop placement and attenuation.

use super::*;
use glam::{Mat4, Vec3};

// ---------------------------------------------------------------------------
// Point-light derivation
// ---------------------------------------------------------------------------

/// Hard cap on lights shading at once - also the WGSL / GLSL array length
/// and the shadow-map layer count. WGSL twin: `SCENE_LIGHT_MAX`.
pub const MAX_SCENE_LIGHTS: usize = 8;

/// Triangles larger than this (model-space area, PSX units squared) never
/// count as emitters - glow *props* are small; big additive surfaces are
/// water / sky sheets.
pub const EMIT_MAX_TRI_AREA: f32 = 20_000.0;

/// Greedy cluster merge distance (world units). Two emitter samples
/// closer than this are one prop (a flame's two crossed quads, a
/// chandelier's candle ring).
pub const CLUSTER_MERGE_DIST: f32 = 320.0;

/// A cluster whose samples spread farther than this from its centroid is
/// a surface, not a prop - dropped.
pub const CLUSTER_MAX_EXTENT: f32 = 480.0;

/// Point-light radius from cluster weight: `radius = RADIUS_SCALE *
/// sqrt(weight)`, clamped to [`RADIUS_MIN`] ..= [`RADIUS_MAX`].
pub const RADIUS_SCALE: f32 = 14.0;
pub const RADIUS_MIN: f32 = 520.0;
pub const RADIUS_MAX: f32 = 1600.0;

/// How far above its emitting geometry (world units, -Y in the Y-down
/// field frame) a derived light sits. Lamps are authored on surfaces - a
/// glow decal on the ground, a pane in a wall - and a light exactly on the
/// surface grazes it; lifted, it pools onto the floor around it.
pub const LIGHT_LIFT: f32 = 48.0;

/// One derived world-space point light.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScenePointLight {
    /// World position (retail Y-down field frame - the same space the
    /// scene draws' model matrices target).
    pub pos: [f32; 3],
    /// Light colour, 0..=1 per channel (a *gain* colour: the fragment's
    /// baked colour is scaled by `1 + Σ colour_i * att_i * ...`).
    pub color: [f32; 3],
    /// Influence radius in world units; the attenuation reaches exactly
    /// zero here ([`point_attenuation`]).
    pub radius: f32,
}

/// One emitter-looking triangle, in whatever space the mesh data was in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EmitterSample {
    pub pos: [f32; 3],
    /// Normalised 0..=1 colour of the glow.
    pub color: [f32; 3],
    /// Ranking weight: `sqrt(area) * luminance`.
    pub weight: f32,
}

fn tri_area(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f32 {
    let ab = Vec3::from(b) - Vec3::from(a);
    let ac = Vec3::from(c) - Vec3::from(a);
    ab.cross(ac).length() * 0.5
}

fn centroid(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    [
        (a[0] + b[0] + c[0]) / 3.0,
        (a[1] + b[1] + c[1]) / 3.0,
        (a[2] + b[2] + c[2]) / 3.0,
    ]
}

fn sample_from_tri(pos: [[f32; 3]; 3], color: [u8; 3]) -> Option<EmitterSample> {
    let area = tri_area(pos[0], pos[1], pos[2]);
    if area <= 0.0 || area > EMIT_MAX_TRI_AREA {
        return None;
    }
    let m = color[0].max(color[1]).max(color[2]).max(1) as f32;
    // Neutral-modulated additive prims (0x80,0x80,0x80) glow with their
    // texel colour, which we don't decode here - assume warm flame.
    let (rgb, lum) = if color == [0x80; 3] {
        ([1.0, 0.82, 0.55], 1.0)
    } else {
        (
            [
                color[0] as f32 / m,
                color[1] as f32 / m,
                color[2] as f32 / m,
            ],
            (m / 255.0).min(1.0),
        )
    };
    Some(EmitterSample {
        pos: centroid(pos[0], pos[1], pos[2]),
        color: rgb,
        weight: area.sqrt() * lum,
    })
}

/// Emitter samples of a textured VRAM mesh (`legaia_tmd::mesh::VramMesh`
/// data, model space). A triangle is a sample when
/// [`textured_prim_glows`] holds for it and it is small enough to be a
/// prop ([`EMIT_MAX_TRI_AREA`]). With `texels` (`(vram, uvs)`), a glow prim
/// whose modulation is neutral takes its light colour from its own texels
/// (an additive green glow casts green), instead of the assumed flame.
pub fn vram_mesh_emitters(
    positions: &[[f32; 3]],
    cba_tsb: &[[u16; 2]],
    colors: &[[u8; 3]],
    indices: &[u32],
    texels: Option<(&legaia_tim::Vram, &[[u8; 2]])>,
) -> Vec<EmitterSample> {
    let mut out = Vec::new();
    for tri in indices.as_chunks::<3>().0 {
        let i0 = tri[0] as usize;
        if i0 >= cba_tsb.len() || i0 >= colors.len() {
            continue;
        }
        let c = colors[i0];
        if !textured_prim_glows(cba_tsb[i0][1], c) {
            continue;
        }
        let p = |i: u32| positions.get(i as usize).copied().unwrap_or([0.0; 3]);
        if let Some(mut s) = sample_from_tri([p(tri[0]), p(tri[1]), p(tri[2])], c) {
            if let Some((vram, uvs)) = texels {
                let corner_uvs: Vec<[u8; 2]> = tri
                    .iter()
                    .filter_map(|&i| uvs.get(i as usize).copied())
                    .collect();
                let [ct_cba, ct_tsb] = cba_tsb[i0];
                let mean =
                    prim_mean_texel(vram, ct_cba & 0x7FFF, ct_tsb, &corner_uvs).unwrap_or([0.0; 3]);
                let modulated = [
                    mean[0] * c[0] as f32 / 128.0,
                    mean[1] * c[1] as f32 / 128.0,
                    mean[2] * c[2] as f32 / 128.0,
                ];
                let m = modulated[0].max(modulated[1]).max(modulated[2]);
                if m > 0.05 {
                    s.color = modulated.map(|v| v / m);
                    s.weight *= m.min(1.0);
                }
            }
            out.push(s);
        }
    }
    out
}

/// Emitter samples of an untextured colour mesh
/// (`legaia_tmd::mesh::ColorMesh` data, model space): ABE + bright-warm
/// vertex colour ([`color_prim_glows`]).
pub fn color_mesh_emitters(
    positions: &[[f32; 3]],
    colors: &[[u8; 3]],
    blend: &[u16],
    indices: &[u32],
) -> Vec<EmitterSample> {
    let mut out = Vec::new();
    for tri in indices.as_chunks::<3>().0 {
        let i0 = tri[0] as usize;
        let (Some(&word), Some(&c)) = (blend.get(i0), colors.get(i0)) else {
            continue;
        };
        if !color_prim_glows(word, c) {
            continue;
        }
        let p = |i: u32| positions.get(i as usize).copied().unwrap_or([0.0; 3]);
        if let Some(s) = sample_from_tri([p(tri[0]), p(tri[1]), p(tri[2])], c) {
            out.push(s);
        }
    }
    out
}

/// Emitter samples of a **hybrid** stream (the browser's merged textured +
/// untextured mesh; `flat` is its `[r, g, b, flag]` side channel, `flag ==
/// 0` an untextured vertex whose TSB slot carries the colour half's blend
/// word). Each prim takes the test of the half it came from, so this is
/// exactly [`vram_mesh_emitters`] + [`color_mesh_emitters`] over the two
/// halves the native window keeps apart.
pub fn hybrid_mesh_emitters(
    positions: &[[f32; 3]],
    cba_tsb: &[[u16; 2]],
    colors: &[[u8; 3]],
    flat: &[u8],
    indices: &[u32],
    texels: Option<(&legaia_tim::Vram, &[[u8; 2]])>,
) -> Vec<EmitterSample> {
    let mut out = Vec::new();
    for tri in indices.as_chunks::<3>().0 {
        let i0 = tri[0] as usize;
        let Some(ct) = cba_tsb.get(i0) else {
            continue;
        };
        let untextured = flat.get(i0 * 4 + 3) == Some(&0);
        let (glows, c) = if untextured {
            let c = [flat[i0 * 4], flat[i0 * 4 + 1], flat[i0 * 4 + 2]];
            (color_prim_glows(ct[1], c), c)
        } else {
            let Some(&c) = colors.get(i0) else {
                continue;
            };
            (textured_prim_glows(ct[1], c), c)
        };
        if !glows {
            continue;
        }
        if untextured {
            let p = |i: u32| positions.get(i as usize).copied().unwrap_or([0.0; 3]);
            if let Some(s) = sample_from_tri([p(tri[0]), p(tri[1]), p(tri[2])], c) {
                out.push(s);
            }
        } else {
            // The textured half goes through the one textured rule (texel
            // tint included), one prim at a time.
            out.extend(vram_mesh_emitters(positions, cba_tsb, colors, tri, texels));
        }
    }
    out
}

/// A `.MAP` placement's model matrix in the retail field frame: translate
/// to the placement's world position, rotate by its three authored angles in
/// retail's `Rx * Ry * Rz` order ([`crate::battle_intro::placement_rotation`]),
/// scale. The transform both hosts instance static emitters through.
pub fn placement_model(world: [f32; 3], rot: [u16; 3], scale: f32) -> Mat4 {
    Mat4::from_translation(Vec3::from(world))
        * crate::battle_intro::placement_rotation(rot[0], rot[1], rot[2])
        * Mat4::from_scale(Vec3::splat(scale))
}

/// The single sample a curated [`EMISSIVE_MESHES`] entry contributes: one
/// strong light at the centre of its glowing prims (model space - the
/// [`CuratedHit::center`] tagging found), weighted so it clusters into a
/// light of the entry's radius.
pub fn curated_mesh_sample(hit: &CuratedHit) -> EmitterSample {
    EmitterSample {
        pos: hit.center,
        color: hit.entry.light,
        weight: (hit.entry.radius / RADIUS_SCALE).powi(2),
    }
}

/// Transform model-space samples into world space by `model`.
pub fn transform_samples(samples: &[EmitterSample], model: &Mat4) -> Vec<EmitterSample> {
    samples
        .iter()
        .map(|s| EmitterSample {
            pos: model.transform_point3(Vec3::from(s.pos)).to_array(),
            ..*s
        })
        .collect()
}

struct Cluster {
    pos_sum: Vec3,
    color_sum: Vec3,
    weight: f32,
    extent: f32,
}

/// Greedy weight-ordered clustering of world-space emitter samples into
/// at most [`MAX_SCENE_LIGHTS`] point lights (see the module docs).
pub fn cluster_scene_lights(samples: &[EmitterSample]) -> Vec<ScenePointLight> {
    let mut lights = cluster_all_scene_lights(samples);
    lights.truncate(MAX_SCENE_LIGHTS);
    lights
}

/// [`cluster_scene_lights`] without the count cap: every surviving
/// cluster, strongest first. A scene can carry dozens of candle props
/// while only [`MAX_SCENE_LIGHTS`] can shade at once, so a host that
/// knows the viewpoint keeps this full set and picks the nearest per
/// frame ([`nearest_lights`]).
pub fn cluster_all_scene_lights(samples: &[EmitterSample]) -> Vec<ScenePointLight> {
    let mut order: Vec<&EmitterSample> = samples.iter().collect();
    order.sort_by(|a, b| b.weight.total_cmp(&a.weight));
    let mut clusters: Vec<Cluster> = Vec::new();
    for s in order {
        let sp = Vec3::from(s.pos);
        let mut merged = false;
        for c in clusters.iter_mut() {
            let cpos = c.pos_sum / c.weight.max(1e-6);
            let d = cpos.distance(sp);
            if d < CLUSTER_MERGE_DIST {
                c.pos_sum += sp * s.weight;
                c.color_sum += Vec3::from(s.color) * s.weight;
                c.weight += s.weight;
                c.extent = c.extent.max(d);
                merged = true;
                break;
            }
        }
        if !merged {
            clusters.push(Cluster {
                pos_sum: sp * s.weight,
                color_sum: Vec3::from(s.color) * s.weight,
                weight: s.weight,
                extent: 0.0,
            });
        }
    }
    clusters.retain(|c| c.weight > 1e-3 && c.extent <= CLUSTER_MAX_EXTENT);
    clusters.sort_by(|a, b| b.weight.total_cmp(&a.weight));
    clusters
        .iter()
        .map(|c| {
            let w = c.weight.max(1e-6);
            let color = (c.color_sum / w).clamp(Vec3::ZERO, Vec3::ONE);
            ScenePointLight {
                pos: (c.pos_sum / w - Vec3::Y * LIGHT_LIFT).to_array(),
                color: color.to_array(),
                radius: (RADIUS_SCALE * c.weight.sqrt()).clamp(RADIUS_MIN, RADIUS_MAX),
            }
        })
        .collect()
}

/// One scene-actor prop's light set (a MAN-placed lamp, a glowing tree),
/// clustered in world space at the prop's spawn anchor. Props move - a
/// script walks them, hides them, re-spawns them - so the set is re-placed
/// every frame at the actor's live anchor ([`place_prop_lights`]).
#[derive(Debug, Clone, PartialEq)]
pub struct PropLights {
    /// The placement slot (the MAN placement index) the set belongs to.
    pub slot: u8,
    /// The spawn anchor `[x, floor y, z]` the lights were derived at.
    pub spawn: [f32; 3],
    /// The prop's lights, world space at `spawn`.
    pub lights: Vec<ScenePointLight>,
}

/// Every prop's light set moved to its live anchor. `live(slot, spawn_xz)`
/// is the host's actor position for the slot (the engine's
/// `World::field_npc_live_anchor` on both hosts); `None` (not drawn this
/// frame) drops the set.
pub fn place_prop_lights(
    props: &[PropLights],
    live: impl Fn(u8, (i16, i16)) -> Option<[f32; 3]>,
) -> Vec<ScenePointLight> {
    let mut out = Vec::new();
    for p in props {
        let Some(at) = live(p.slot, (p.spawn[0] as i16, p.spawn[2] as i16)) else {
            continue;
        };
        let d = Vec3::from(at) - Vec3::from(p.spawn);
        out.extend(p.lights.iter().map(|l| ScenePointLight {
            pos: (Vec3::from(l.pos) + d).to_array(),
            ..*l
        }));
    }
    out
}

/// The up-to-[`MAX_SCENE_LIGHTS`] lights nearest `focus` (typically the
/// player), nearest first - the per-frame selection over
/// [`cluster_all_scene_lights`]'s full set. Distance is measured to the
/// light's influence sphere (`dist - radius`), so a big nearby light
/// never loses its slot to a tiny slightly-closer one.
pub fn nearest_lights(lights: &[ScenePointLight], focus: [f32; 3]) -> Vec<ScenePointLight> {
    let f = Vec3::from(focus);
    let mut sorted: Vec<ScenePointLight> = lights.to_vec();
    sorted.sort_by(|a, b| {
        let da = Vec3::from(a.pos).distance(f) - a.radius;
        let db = Vec3::from(b.pos).distance(f) - b.radius;
        da.total_cmp(&db)
    });
    sorted.truncate(MAX_SCENE_LIGHTS);
    sorted
}

/// CPU mirror of the shaders' point-light attenuation:
/// `att = (1 - (d/r)^2)^2`, clamped - 1.0 at the light, exactly 0.0 at
/// the radius, smooth in between.
pub fn point_attenuation(dist: f32, radius: f32) -> f32 {
    if radius <= 0.0 || dist >= radius {
        return 0.0;
    }
    let x = (1.0 - (dist * dist) / (radius * radius)).clamp(0.0, 1.0);
    x * x
}

/// CPU mirror of the shaders' point-light gain *analytic* part (no shadow
/// term - the PCF lives on the GPU): per-channel gain added on top of the
/// global gain. `scale` is the mood's [`LightingMood::point_scale`].
pub fn point_gain(
    world_pos: [f32; 3],
    normal: [f32; 3],
    lights: &[ScenePointLight],
    scale: f32,
) -> [f32; 3] {
    let p = Vec3::from(world_pos);
    let n = Vec3::from(normal);
    let n_len = n.length();
    let mut gain = Vec3::ZERO;
    for l in lights.iter().take(MAX_SCENE_LIGHTS) {
        let to_l = Vec3::from(l.pos) - p;
        let dist = to_l.length();
        let att = point_attenuation(dist, l.radius);
        if att <= 0.0 {
            continue;
        }
        let lam = if n_len > 1e-6 && dist > 1e-3 {
            // Half-Lambert wrap: a point light grazing a wall still lifts it,
            // matching the soft pools of light the enhancement aims for.
            (n.dot(to_l) / (n_len * dist)).abs() * 0.5 + 0.5
        } else {
            LAMBERT_FALLBACK
        };
        gain += Vec3::from(l.color) * att * lam;
    }
    (gain * scale).to_array()
}

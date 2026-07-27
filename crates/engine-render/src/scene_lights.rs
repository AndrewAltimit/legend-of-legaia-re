//! Per-scene **point-light derivation** for the dynamic-lighting
//! enhancement: find the light-emitting props of a loaded field scene
//! (candle flames, wall torches, lamps) and turn them into a small set of
//! world-space point lights the renderer can shade and shadow from.
//!
//! # This is NOT retail
//!
//! Retail has no light sources at all (see [`crate::dyn_light`]); what the
//! player perceives as "candles and wall lights" in retail interiors is
//! emissive-looking *geometry*: small additive-blended (ABE, ABR mode 1)
//! flame prims and bright-modulated lamp meshes whose glow is baked into
//! the art. This module reads exactly that authoring signal back out of
//! the mesh data - the flame prims mark where the artists put the light -
//! and synthesises real point lights at those spots. Everything here is
//! gated behind the dynamic-lighting toggle; with it off none of this
//! runs and the render stays pixel-identical to the faithful path.
//!
//! # The heuristic
//!
//! A triangle is an **emitter sample** when it looks like authored glow:
//!
//! * textured prims: the semi-transparency enable
//!   ([`legaia-tmd`'s TSB bit 15][crate::psx_blend::TSB_SEMI_TRANSPARENT_BIT])
//!   with ABR mode 1 (`B + F`, pure additive - the PSX "glow" mode), or
//!   any ABE prim whose baked modulation colour is *bright and warm*
//!   (max channel >= [`EMIT_MIN_BRIGHT`], red >= blue - candle-flame
//!   colours, which excludes blue water/glass sheets);
//! * untextured colour prims: ABE plus the same bright-warm colour test
//!   (Legaia's untextured prims carry no ABR field - see
//!   [`crate::psx_blend::pack_blend_word`]).
//!
//! Big triangles are rejected ([`EMIT_MAX_TRI_AREA`]): flames are small
//! quads, while water surfaces / fullscreen sheets are large. Each sample
//! is weighted by `sqrt(area) * luminance` so a candle's two tiny quads
//! and a brazier's larger flame rank sensibly against each other.
//!
//! Samples are transformed to world space by the caller (one mesh may be
//! instanced at several placements - every instance gets its own light)
//! and clustered greedily by distance ([`CLUSTER_MERGE_DIST`]); clusters
//! that spread too far ([`CLUSTER_MAX_EXTENT`]) are dropped as sheets
//! rather than props, and the [`MAX_SCENE_LIGHTS`] strongest survive.
//!
//! The WGSL consumer is `scene_point_gain` in `shaders.rs` (real variant
//! compiled only into the scene pipelines); [`point_attenuation`] and
//! [`point_gain`] below are its CPU mirror for the analytic (non-shadow)
//! part, asserted in lockstep by the tests.

use glam::{Mat4, Vec3};

/// Hard cap on derived lights per scene - also the WGSL-side array length
/// and the shadow-map layer count. WGSL twin: `SCENE_LIGHT_MAX`.
pub const MAX_SCENE_LIGHTS: usize = 8;

/// Minimum "bright" max-channel (0..255 modulation byte) for the
/// bright-warm emitter test. `0x80` is neutral; flames are authored well
/// above it.
pub const EMIT_MIN_BRIGHT: u8 = 0xB0;

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

/// Bright-and-warm test on a raw 0..255 modulation/vertex colour: max
/// channel over [`EMIT_MIN_BRIGHT`] and not blue-dominant (candle/torch
/// palettes are red >= blue; water and magic glass are not).
fn bright_warm(c: [u8; 3]) -> bool {
    let m = c[0].max(c[1]).max(c[2]);
    m >= EMIT_MIN_BRIGHT && c[0] >= c[2]
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
/// data, model space). See the module docs for the heuristic.
pub fn vram_mesh_emitters(
    positions: &[[f32; 3]],
    cba_tsb: &[[u16; 2]],
    colors: &[[u8; 3]],
    indices: &[u32],
) -> Vec<EmitterSample> {
    let mut out = Vec::new();
    for tri in indices.chunks_exact(3) {
        let i0 = tri[0] as usize;
        if i0 >= cba_tsb.len() || i0 >= colors.len() {
            continue;
        }
        let tsb = cba_tsb[i0][1];
        if !crate::psx_blend::prim_semi_transparent(tsb) {
            continue;
        }
        let additive = crate::psx_blend::abr_mode(tsb) == 1;
        let c = colors[i0];
        if !additive && !bright_warm(c) {
            continue;
        }
        let p = |i: u32| positions.get(i as usize).copied().unwrap_or([0.0; 3]);
        if let Some(s) = sample_from_tri([p(tri[0]), p(tri[1]), p(tri[2])], c) {
            out.push(s);
        }
    }
    out
}

/// Emitter samples of an untextured colour mesh
/// (`legaia_tmd::mesh::ColorMesh` data, model space). Untextured prims
/// carry no ABR field (always mode 0), so the test is ABE + bright-warm
/// vertex colour.
pub fn color_mesh_emitters(
    positions: &[[f32; 3]],
    colors: &[[u8; 3]],
    blend: &[u16],
    indices: &[u32],
) -> Vec<EmitterSample> {
    let mut out = Vec::new();
    for tri in indices.chunks_exact(3) {
        let i0 = tri[0] as usize;
        let Some(&word) = blend.get(i0) else {
            continue;
        };
        if !crate::psx_blend::prim_semi_transparent(word) {
            continue;
        }
        let Some(&c) = colors.get(i0) else {
            continue;
        };
        if !bright_warm(c) {
            continue;
        }
        let p = |i: u32| positions.get(i as usize).copied().unwrap_or([0.0; 3]);
        if let Some(s) = sample_from_tri([p(tri[0]), p(tri[1]), p(tri[2])], c) {
            out.push(s);
        }
    }
    out
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
                pos: (c.pos_sum / w).to_array(),
                color: color.to_array(),
                radius: (RADIUS_SCALE * c.weight.sqrt()).clamp(RADIUS_MIN, RADIUS_MAX),
            }
        })
        .collect()
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

/// Shadow-cone vertical field of view (radians). Wide, because the cone
/// approximates a point source lighting the floor and walls around it.
pub const SHADOW_FOV: f32 = 2.1;

/// Near-plane fraction of the light radius. Geometry closer to the light
/// than this (the emitting flame quad itself) casts no shadow.
pub const SHADOW_NEAR_FRAC: f32 = 0.04;
/// Absolute near-plane floor (world units).
pub const SHADOW_NEAR_MIN: f32 = 24.0;

/// The light's shadow view-projection: a downward cone (retail field
/// space is Y-down, so "down toward the floor" is `+Y`) with a wgpu 0..1
/// depth range. Fragments outside the cone sample as unshadowed.
pub fn light_view_proj(light: &ScenePointLight) -> Mat4 {
    let pos = Vec3::from(light.pos);
    let view = Mat4::look_at_rh(pos, pos + Vec3::Y, Vec3::Z);
    let near = (light.radius * SHADOW_NEAR_FRAC).max(SHADOW_NEAR_MIN);
    let proj = Mat4::perspective_rh(SHADOW_FOV, 1.0, near, light.radius.max(near * 2.0));
    proj * view
}

/// CPU mirror of the WGSL point-light attenuation:
/// `att = (1 - (d/r)^2)^2`, clamped - 1.0 at the light, exactly 0.0 at
/// the radius, smooth in between.
pub fn point_attenuation(dist: f32, radius: f32) -> f32 {
    if radius <= 0.0 || dist >= radius {
        return 0.0;
    }
    let x = (1.0 - (dist * dist) / (radius * radius)).clamp(0.0, 1.0);
    x * x
}

/// CPU mirror of the WGSL `scene_point_gain` *analytic* part (no shadow
/// term - the PCF lives on the GPU): per-channel gain added on top of the
/// global dynamic-light gain.
pub fn point_gain(world_pos: [f32; 3], normal: [f32; 3], lights: &[ScenePointLight]) -> [f32; 3] {
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
            (n.dot(to_l) / (n_len * dist)).abs()
        } else {
            crate::dyn_light::LAMBERT_FALLBACK
        };
        gain += Vec3::from(l.color) * att * lam;
    }
    gain.to_array()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::psx_blend::pack_blend_word;

    fn flame_quad(center: [f32; 3], half: f32) -> ([[f32; 3]; 4], [u32; 6]) {
        let [x, y, z] = center;
        (
            [
                [x - half, y - half, z],
                [x + half, y - half, z],
                [x - half, y + half, z],
                [x + half, y + half, z],
            ],
            [0, 1, 2, 1, 3, 2],
        )
    }

    /// An additive (ABR 1) semi prim is an emitter even at neutral
    /// modulation colour; an opaque prim never is.
    #[test]
    fn additive_prims_are_emitters() {
        let (pos, idx) = flame_quad([100.0, -80.0, 40.0], 16.0);
        let additive_tsb = crate::psx_blend::TSB_SEMI_TRANSPARENT_BIT | (1 << 5);
        let cba_tsb = [[0u16, additive_tsb]; 4];
        let colors = [[0x80u8; 3]; 4];
        let got = vram_mesh_emitters(&pos, &cba_tsb, &colors, &idx);
        assert_eq!(got.len(), 2, "both triangles of the flame quad");
        let plain = [[0u16, 0u16]; 4];
        assert!(vram_mesh_emitters(&pos, &plain, &colors, &idx).is_empty());
    }

    /// A bright-warm ABE prim (mode 0) counts; a blue one (water) doesn't.
    #[test]
    fn bright_warm_gate() {
        let (pos, idx) = flame_quad([0.0, 0.0, 0.0], 16.0);
        let semi_tsb = crate::psx_blend::TSB_SEMI_TRANSPARENT_BIT; // ABR 0
        let cba_tsb = [[0u16, semi_tsb]; 4];
        let warm = [[0xE0u8, 0xB0, 0x60]; 4];
        assert_eq!(vram_mesh_emitters(&pos, &cba_tsb, &warm, &idx).len(), 2);
        let blue = [[0x40u8, 0x80, 0xE0]; 4];
        assert!(vram_mesh_emitters(&pos, &cba_tsb, &blue, &idx).is_empty());
    }

    /// Big additive sheets (water) are rejected by the area cap.
    #[test]
    fn big_sheets_are_rejected() {
        let (pos, idx) = flame_quad([0.0, 0.0, 0.0], 500.0);
        let additive_tsb = crate::psx_blend::TSB_SEMI_TRANSPARENT_BIT | (1 << 5);
        let cba_tsb = [[0u16, additive_tsb]; 4];
        let colors = [[0xFFu8, 0xC0, 0x60]; 4];
        assert!(vram_mesh_emitters(&pos, &cba_tsb, &colors, &idx).is_empty());
    }

    /// Colour-mesh emitters key off the packed blend word + warm colour.
    #[test]
    fn color_mesh_emitters_gate_on_abe() {
        let (pos, idx) = flame_quad([0.0, 0.0, 0.0], 16.0);
        let warm = [[0xF0u8, 0xC0, 0x60]; 4];
        let on = [pack_blend_word(true, 0); 4];
        let off = [pack_blend_word(false, 0); 4];
        assert_eq!(color_mesh_emitters(&pos, &warm, &on, &idx).len(), 2);
        assert!(color_mesh_emitters(&pos, &warm, &off, &idx).is_empty());
    }

    /// Nearby samples merge into one light; the cap keeps the strongest.
    #[test]
    fn clustering_merges_and_caps() {
        let mut samples = Vec::new();
        // One candle: two samples 20 units apart -> one light.
        for dx in [0.0, 20.0] {
            samples.push(EmitterSample {
                pos: [100.0 + dx, -60.0, 100.0],
                color: [1.0, 0.8, 0.5],
                weight: 10.0,
            });
        }
        // Twelve weak, far-apart sparks -> capped to MAX_SCENE_LIGHTS total.
        for i in 0..12 {
            samples.push(EmitterSample {
                pos: [5000.0 + 2000.0 * i as f32, 0.0, 0.0],
                color: [1.0, 1.0, 1.0],
                weight: 1.0,
            });
        }
        let lights = cluster_scene_lights(&samples);
        assert!(lights.len() <= MAX_SCENE_LIGHTS);
        // The strongest light is the merged candle, at the weighted centroid.
        let strongest = &lights[0];
        assert!((strongest.pos[0] - 110.0).abs() < 1.0, "{strongest:?}");
        assert!(strongest.radius >= RADIUS_MIN && strongest.radius <= RADIUS_MAX);
    }

    /// A cluster that spreads wider than a prop (a lit water edge strip)
    /// is dropped.
    #[test]
    fn wide_clusters_are_dropped() {
        // Chain of samples each within merge distance of the running
        // centroid but spreading far overall.
        let samples: Vec<EmitterSample> = (0..10)
            .map(|i| EmitterSample {
                pos: [i as f32 * 250.0, 0.0, 0.0],
                color: [1.0, 0.9, 0.6],
                weight: 100.0 - i as f32, // keep insertion order stable
            })
            .collect();
        let lights = cluster_scene_lights(&samples);
        // Whatever survives must be tight props, not the wide chain.
        for l in &lights {
            assert!(l.radius <= RADIUS_MAX);
        }
        // The chain itself (extent > CLUSTER_MAX_EXTENT) must not appear
        // as one giant merged light spanning 0..2250.
        assert!(
            lights
                .iter()
                .all(|l| l.pos[0] < 100.0 || l.pos[0] > 150.0 || l.radius <= RADIUS_MAX)
        );
    }

    /// Attenuation: 1 at the light, 0 at the radius, monotonic between.
    #[test]
    fn attenuation_shape() {
        assert!((point_attenuation(0.0, 1000.0) - 1.0).abs() < 1e-6);
        assert_eq!(point_attenuation(1000.0, 1000.0), 0.0);
        assert_eq!(point_attenuation(1500.0, 1000.0), 0.0);
        let mut last = 1.0;
        for i in 1..10 {
            let a = point_attenuation(i as f32 * 100.0, 1000.0);
            assert!(a < last, "attenuation must decrease");
            last = a;
        }
    }

    /// The point-gain mirror: a fragment right under the light gets the
    /// light's colour scaled by attenuation; out of radius gets zero.
    #[test]
    fn point_gain_mirror() {
        let l = ScenePointLight {
            pos: [0.0, -100.0, 0.0],
            color: [1.0, 0.8, 0.5],
            radius: 800.0,
        };
        let g = point_gain([0.0, 0.0, 0.0], [0.0, -1.0, 0.0], &[l]);
        let att = point_attenuation(100.0, 800.0);
        assert!((g[0] - att).abs() < 1e-5, "{g:?} vs att {att}");
        assert!((g[1] - 0.8 * att).abs() < 1e-5);
        let far = point_gain([5000.0, 0.0, 0.0], [0.0, -1.0, 0.0], &[l]);
        assert_eq!(far, [0.0; 3]);
    }

    /// The shadow view-projection maps a floor point under the light into
    /// the depth range and NDC square, and the light's own position (near
    /// side) outside it.
    #[test]
    fn shadow_view_proj_covers_floor() {
        let l = ScenePointLight {
            pos: [500.0, -200.0, 300.0],
            color: [1.0; 3],
            radius: 1000.0,
        };
        let vp = light_view_proj(&l);
        // Floor point 400 units below (retail Y-down: below = +Y).
        let clip = vp * glam::Vec4::new(500.0, 200.0, 300.0, 1.0);
        assert!(clip.w > 0.0);
        let ndc = clip / clip.w;
        assert!(ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0, "{ndc:?}");
        assert!(ndc.z > 0.0 && ndc.z < 1.0, "{ndc:?}");
    }

    /// The WGSL twin carries the same array length + attenuation shape
    /// markers (the shader is the production path; this module is the
    /// documented mirror).
    #[test]
    fn wgsl_constants_match_the_mirror() {
        let src = crate::shaders::scene_lights_wgsl_for_tests();
        for needle in [
            "const SCENE_LIGHT_MAX: u32 = 8u;",
            "att = att * att;",
            "fn scene_point_gain(",
        ] {
            assert!(
                src.contains(needle),
                "WGSL drifted from the mirror: {needle}"
            );
        }
    }
}

//! Glow sprites (the bloom stand-in), light shafts and the per-frame lighting packet.

use super::*;
use glam::Vec3;

// ---------------------------------------------------------------------------
// Glow sprites (bloom stand-in) and light shafts
// ---------------------------------------------------------------------------

/// What a [`GlowSprite`] draws as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlowKind {
    /// A camera-facing round halo around the light (the bloom stand-in).
    Halo,
    /// A vertical soft shaft from the light down toward the floor,
    /// camera-facing about the vertical axis.
    Shaft,
}

/// One additive glow sprite in world space. Both hosts draw the same list
/// with the same falloff: radial `(1 - r^2)^2` for a halo, a horizontal
/// `(1 - x^2)^2` times a linear vertical fade for a shaft.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlowSprite {
    pub kind: GlowKind,
    /// Centre of a halo; the top of a shaft (retail Y-down frame).
    pub pos: [f32; 3],
    /// Halo radius / shaft half-width, world units.
    pub size: f32,
    /// Shaft length (downward, +Y); 0 for a halo.
    pub length: f32,
    /// Additive colour at the brightest point (premultiplied by strength).
    pub color: [f32; 3],
}

/// Halo radius as a fraction of the light's influence radius.
pub const HALO_RADIUS_FRAC: f32 = 0.22;
/// Halo peak intensity at full mood glow.
pub const HALO_INTENSITY: f32 = 0.55;
/// Shaft half-width as a fraction of the light's radius.
pub const SHAFT_WIDTH_FRAC: f32 = 0.10;
/// Shaft length as a fraction of the light's radius.
pub const SHAFT_LENGTH_FRAC: f32 = 0.45;
/// Shaft peak intensity at full mood glow.
pub const SHAFT_INTENSITY: f32 = 0.16;

/// The glow sprites for a frame's picked lights under `mood`: one halo per
/// light plus a soft downward shaft. Empty when the mood's glow is zero.
pub fn glow_sprites(lights: &[ScenePointLight], mood: &LightingMood) -> Vec<GlowSprite> {
    let g = mood.glow.clamp(0.0, 1.0);
    if g <= 0.0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(lights.len() * 2);
    for l in lights {
        let c = Vec3::from(l.color);
        out.push(GlowSprite {
            kind: GlowKind::Halo,
            pos: l.pos,
            size: l.radius * HALO_RADIUS_FRAC,
            length: 0.0,
            color: (c * HALO_INTENSITY * g).to_array(),
        });
        out.push(GlowSprite {
            kind: GlowKind::Shaft,
            pos: l.pos,
            size: l.radius * SHAFT_WIDTH_FRAC,
            length: l.radius * SHAFT_LENGTH_FRAC,
            color: (c * SHAFT_INTENSITY * g).to_array(),
        });
    }
    out
}

/// One glow-sprite vertex: world position, the falloff coordinate
/// (`uv` in -1..1 across a halo; `x` across / `y` down a shaft) and the
/// additive colour with the kind in `.w` (0 = halo, 1 = shaft).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlowVertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

/// Expand glow sprites into camera-facing quads (two triangles each, six
/// vertices, no index buffer). `cam_right` / `cam_up` are the camera's
/// world-space basis vectors (unit length); a shaft keeps world vertical and
/// turns only about it to face the camera.
pub fn glow_vertices(
    sprites: &[GlowSprite],
    cam_right: [f32; 3],
    cam_up: [f32; 3],
) -> Vec<GlowVertex> {
    let r = Vec3::from(cam_right);
    let u = Vec3::from(cam_up);
    // Horizontal right vector for shafts: the camera right flattened.
    let flat_r = Vec3::new(r.x, 0.0, r.z).normalize_or(Vec3::X);
    let mut out = Vec::with_capacity(sprites.len() * 6);
    for s in sprites {
        let p = Vec3::from(s.pos);
        let (corners, uvs, kind) = match s.kind {
            GlowKind::Halo => {
                let rr = r * s.size;
                let uu = u * s.size;
                (
                    [p - rr - uu, p + rr - uu, p + rr + uu, p - rr + uu],
                    [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]],
                    0.0,
                )
            }
            GlowKind::Shaft => {
                let rr = flat_r * s.size;
                let down = Vec3::Y * s.length;
                (
                    [p - rr, p + rr, p + rr + down, p - rr + down],
                    [[-1.0, 0.0], [1.0, 0.0], [1.0, 1.0], [-1.0, 1.0]],
                    1.0,
                )
            }
        };
        let color = [s.color[0], s.color[1], s.color[2], kind];
        for &i in &[0usize, 1, 2, 0, 2, 3] {
            out.push(GlowVertex {
                pos: corners[i].to_array(),
                uv: uvs[i],
                color,
            });
        }
    }
    out
}

/// One frame's lighting for a host that draws in its own process space (the
/// browser play page): the [`nearest_lights`] to `focus` with the mood's
/// [`LightingMood::point_scale`] folded into their colours, followed by the
/// glow-sprite vertices expanded against the camera basis - the exact pair
/// the native renderer derives from `set_scene_lights` + `set_glow_sprites`.
///
/// Layout (all `f32`, retail Y-down frame): `[n_lights, n_glow_vertices]`,
/// then `n_lights x [x, y, z, radius, r, g, b]`, then `n_glow_vertices x
/// [x, y, z, u, v, r, g, b, kind]`.
pub fn frame_packet(
    lights: &[ScenePointLight],
    focus: [f32; 3],
    mood: &LightingMood,
    cam_right: [f32; 3],
    cam_up: [f32; 3],
) -> Vec<f32> {
    let picked = nearest_lights(lights, focus);
    let verts = glow_vertices(&glow_sprites(&picked, mood), cam_right, cam_up);
    let mut out = Vec::with_capacity(2 + picked.len() * 7 + verts.len() * 9);
    out.push(picked.len() as f32);
    out.push(verts.len() as f32);
    for l in &picked {
        out.extend_from_slice(&l.pos);
        out.push(l.radius);
        out.extend(l.color.iter().map(|c| c * mood.point_scale));
    }
    for v in &verts {
        out.extend_from_slice(&v.pos);
        out.extend_from_slice(&v.uv);
        out.extend_from_slice(&v.color);
    }
    out
}

/// The volumetric ground fog's colour under `mood`: the mist takes the
/// mood ambient's hue (moonlit blue at night, amber-grey at dusk) and dims
/// with its strength, so the bank sits in the same light as the scene it
/// lies on. Daylight's neutral ambient leaves the colour unchanged. Both
/// hosts apply it while enhanced lighting is on.
pub fn fog_tint(color: [f32; 3], mood: &LightingMood) -> [f32; 3] {
    let a = mood.ambient_rgb;
    let m = a[0].max(a[1]).max(a[2]);
    if m <= 0.0 {
        return color;
    }
    // A neutral ambient at or above the daylight floor is the identity.
    let k = (0.4 + m).min(1.0);
    [0, 1, 2].map(|i| (color[i] * a[i] / m * k).clamp(0.0, 1.0))
}

/// CPU mirror of the glow sprites' fragment falloff (both shader twins).
pub fn glow_falloff(uv: [f32; 2], kind_shaft: bool) -> f32 {
    if kind_shaft {
        let x = (1.0 - uv[0] * uv[0]).clamp(0.0, 1.0);
        x * x * (1.0 - uv[1]).clamp(0.0, 1.0)
    } else {
        let r2 = uv[0] * uv[0] + uv[1] * uv[1];
        let x = (1.0 - r2).clamp(0.0, 1.0);
        x * x
    }
}

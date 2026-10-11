//! The glow sprite shader.

/// Glow-sprite shader for the enhanced-lighting enhancement (NON-RETAIL):
/// the halos and soft light shafts `scene_lighting::glow_vertices` expands
/// around the frame's lights. Camera-facing quads in world space, drawn
/// additively with the depth test on and depth writes off, so a halo is
/// hidden by the wall in front of it. Falloff twin:
/// `scene_lighting::glow_falloff` (radial `(1 - r^2)^2` for a halo,
/// `(1 - x^2)^2 * (1 - y)` for a shaft; the kind rides `color.w`).
pub(crate) const GLOW_SHADER_SRC: &str = r#"
struct GlowUniforms {
    view_proj: mat4x4<f32>,
};
@group(0) @binding(0) var<uniform> g: GlowUniforms;

struct GlowOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
) -> GlowOut {
    var out: GlowOut;
    out.clip_pos = g.view_proj * vec4<f32>(position, 1.0);
    out.uv = uv;
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: GlowOut) -> @location(0) vec4<f32> {
    var f: f32;
    if (in.color.w > 0.5) {
        let x = clamp(1.0 - in.uv.x * in.uv.x, 0.0, 1.0);
        f = x * x * clamp(1.0 - in.uv.y, 0.0, 1.0);
    } else {
        let x = clamp(1.0 - dot(in.uv, in.uv), 0.0, 1.0);
        f = x * x;
    }
    return vec4<f32>(in.color.rgb * f, 1.0);
}
"#;

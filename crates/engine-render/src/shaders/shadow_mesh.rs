//! The point-light shadow-map depth shader.

/// Depth-only shadow-pass shader: one light-space MVP per (light, draw)
/// slot, position attribute only (both the 36-byte VRAM-mesh layout and
/// the 20-byte colour-mesh layout put the position at offset 0). No
/// fragment stage - opaque geometry writes depth; cutout texels of
/// foliage-style prims therefore shadow as solid, an accepted
/// approximation for the prop-scale casters this pass serves.
pub(crate) const SHADOW_MESH_SHADER_SRC: &str = r#"
struct ShadowUniforms {
    mvp: mat4x4<f32>,
};
@group(0) @binding(0) var<uniform> su: ShadowUniforms;

@vertex
fn vs_main(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {
    return su.mvp * vec4<f32>(position, 1.0);
}
"#;

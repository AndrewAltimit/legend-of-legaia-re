//! The window blit shader.

pub(crate) const SHADER_SRC: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct Uniforms {
    scale: vec4<f32>,
};
@group(1) @binding(0) var<uniform> u: Uniforms;

@vertex
fn vs_main(@builtin(vertex_index) vidx: u32) -> VertexOutput {
    // Unit quad in NDC, drawn as TriangleStrip with vertices in the order
    // (-1,-1), (1,-1), (-1,1), (1,1). The vertex_index pattern maps:
    //   vidx 0 -> (0,0)
    //   vidx 1 -> (1,0)
    //   vidx 2 -> (0,1)
    //   vidx 3 -> (1,1)
    let x_unit = f32(vidx & 1u);
    let y_unit = f32((vidx >> 1u) & 1u);
    let ndc_x = (x_unit * 2.0 - 1.0) * u.scale.x;
    let ndc_y = (y_unit * 2.0 - 1.0) * u.scale.y;

    var out: VertexOutput;
    out.position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    // Flip Y for texture coordinates: image Y grows down, NDC Y grows up.
    out.uv = vec2<f32>(x_unit, 1.0 - y_unit);
    return out;
}

@group(0) @binding(0) var t_diffuse: texture_2d<f32>;
@group(0) @binding(1) var s_diffuse: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(t_diffuse, s_diffuse, in.uv);
}

// The native frame scanned out as the 15-bit framebuffer it models: each
// channel's 5-bit value `v >> 3` expanded `(c5 << 3) | (c5 >> 2)`. PSX mode
// leaves every channel inside its 5-bit bin (`psx_blend`, "The GPU's 5-bit
// arithmetic"), so this reads the bin as the console's display does. Twin of
// `psx_blend::scanout15`.
@fragment
fn fs_scanout15(in: VertexOutput) -> @location(0) vec4<f32> {
    let c = textureSample(t_diffuse, s_diffuse, in.uv);
    let c5 = floor(clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * 255.0 / 8.0 + 0.001);
    return vec4<f32>((c5 * 8.0 + floor(c5 / 4.0)) / 255.0, c.a);
}
"#;

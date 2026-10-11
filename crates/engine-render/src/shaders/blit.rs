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
"#;

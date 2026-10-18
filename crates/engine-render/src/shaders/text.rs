//! The text / sprite overlay shader.

/// 2D text shader: pre-converted NDC positions + atlas UVs + per-vertex
/// RGBA tint. The fragment tints the sampled atlas texel through
/// `ui_tint` - retail's 5-bit texture modulation and the font CLUTs' own
/// shadow colours, generated from `legaia_render_kernels::ui_tint` - and the
/// alpha-blend pipeline handles final compositing.
///
/// `UI_QUANTIZE` is a pipeline-overridable constant: `true` (the default,
/// retail) truncates every tinted texel to the 5-bit grid, `false` keeps the
/// plain 8-bit multiply (`Renderer::set_ui_psx_tint`).
pub(crate) fn text_shader_src() -> String {
    format!(
        "{}\n{}",
        legaia_render_kernels::ui_tint::wgsl_ui_tint(),
        TEXT_SHADER_BODY
    )
}

const TEXT_SHADER_BODY: &str = r#"
override UI_QUANTIZE: bool = true;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
) -> VsOut {
    var out: VsOut;
    out.clip_pos = vec4<f32>(pos, 0.0, 1.0);
    out.uv = uv;
    out.color = color;
    return out;
}

@group(0) @binding(0) var t_atlas: texture_2d<f32>;
@group(0) @binding(1) var s_atlas: sampler;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let texel = textureSample(t_atlas, s_atlas, in.uv);
    let tinted = ui_tint(texel, in.color.rgb, UI_QUANTIZE);
    return vec4<f32>(tinted.rgb, in.color.a * tinted.a);
}

// PSX semi-transparent quads (`OverlayBlendSpan`): the foreground F is the
// modulated texel's colour, and the per-ABR fixed-function blend state
// (`psx_blend::blend_state`) combines it with the framebuffer. A texel whose
// atlas alpha is zero is PSX colour 0x0000 - never drawn - so it discards.
// The tint's alpha plays no part: the equation carries the blend.
fn blend_color(in: VsOut, scale: f32) -> vec4<f32> {
    let texel = textureSample(t_atlas, s_atlas, in.uv);
    if (texel.a < 0.5) {
        discard;
    }
    let tinted = ui_tint(texel, in.color.rgb, UI_QUANTIZE);
    return vec4<f32>(tinted.rgb * scale, 1.0);
}

@fragment
fn fs_blend(in: VsOut) -> @location(0) vec4<f32> {
    return blend_color(in, 1.0);
}

@fragment
fn fs_blend_quarter(in: VsOut) -> @location(0) vec4<f32> {
    return blend_color(in, 0.25);
}
"#;

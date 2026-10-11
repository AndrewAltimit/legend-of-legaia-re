//! The debug line shader.

/// Wireframe lines shader: pass per-vertex color through, output unchanged.
/// Stage geometry is unlit - there are no normals on a line - so the host
/// gets to encode whatever color signal it wants (per-record, depth-shade,
/// etc.) at upload time.
pub(crate) const LINES_SHADER_SRC: &str = r#"
struct MeshUniforms {
    mvp: mat4x4<f32>,
    depth_cue: vec4<f32>,
    // (viewport_w, viewport_h, snap_enable, dither_enable)
    psx_params: vec4<f32>,
    // GP0(0xE2) "Texture Window setting":
    //   .x = mask_x  (in 8-pixel steps, 0..31)
    //   .y = mask_y  (in 8-pixel steps, 0..31)
    //   .z = offset_x (in 8-pixel steps, 0..31)
    //   .w = offset_y (in 8-pixel steps, 0..31)
    // No-op when all four are zero (Legaia's default; the register only
    // gets written by some effect / scene-init scripts in retail).
    tex_window: vec4<u32>,
    // Full-scene colour grade (gold_rgb, strength). strength 0 = identity.
    grade: vec4<f32>,
    // Render flags. .x = backface cull mode: 0 = draw both sides,
    // 1 = discard back-facing fragments, 2 = discard front-facing.
    // (Retail GTE NCLIP winding rejection as a fragment discard.)
    flags: vec4<f32>,
    // Opt-in dynamic-lighting enhancement (see `dyn_light` in the prelude).
    // `.xyz` = direction TOWARD the light (mesh model space), `.w` = enable
    // (0.0 = off = retail-identical, the default).
    light_dir: vec4<f32>,
    // `.xyz` = warm light tint applied to the diffuse + pool terms,
    // `.w` = ambient floor.
    light_color: vec4<f32>,
    // View-depth IR0 ramp for the per-render-node depth cue (see
    // `cue_ramp_ir0` in the prelude): (near_z, inv_range, max_ir0, enable).
    // enable 0 (the default) = constant `depth_cue.a` (identity when 0).
    cue_ramp: vec4<f32>,
    // Prologue palette-collapse grade (see `palette_law_word` in the
    // prelude): .xyz = the global screen tint (neutral 1,1,1), .w = enable.
    // When enabled the textured/colour paths collapse texels + packet
    // colours to gold instead of the `apply_grade` multiply, and the
    // view-depth cue ramp is inert (the capture shows no node carries a
    // depth cue during the prologue).
    palette: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: MeshUniforms;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(@location(0) position: vec3<f32>, @location(1) color: vec4<f32>) -> VsOut {
    var out: VsOut;
    out.clip_pos = u.mvp * vec4<f32>(position, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}
"#;

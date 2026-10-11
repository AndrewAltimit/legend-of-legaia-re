//! The asset-viewer's bare-geometry and plain-textured mesh shaders.

/// Mesh shader: transforms positions by the host-supplied MVP, computes a
/// per-fragment normal from screen-space derivatives, and shades with a
/// single directional light.
///
/// This is the bare-geometry **preview** pipeline (asset-viewer's raw-TMD
/// view): its meshes carry neither texture nor colour, so there is nothing of
/// the game's own shading to show and the light below is a viewer aid, not a
/// claim about retail. The game's field/town meshes go through the VRAM-mesh
/// and vertex-colour pipelines, which draw the TMD's baked colours and have no
/// light source at all.
pub(crate) const MESH_SHADER_SRC: &str = r#"
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
    @location(0) world_pos: vec3<f32>,
};

@vertex
fn vs_main(@location(0) position: vec3<f32>) -> VsOut {
    var out: VsOut;
    out.clip_pos = u.mvp * vec4<f32>(position, 1.0);
    out.world_pos = position;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Compute face normal from screen-space derivatives. This gives flat
    // per-triangle shading regardless of vertex normal availability - the
    // source TMDs only carry per-object normals, so true Gouraud would need
    // additional work to map normals back to verts.
    let dx = dpdx(in.world_pos);
    let dy = dpdy(in.world_pos);
    let n = normalize(cross(dx, dy));
    // Fixed preview light (see the doc comment: this pipeline has no colour or
    // texture data to draw, so the shading is a viewer aid, not retail's).
    let l = -normalize(vec3<f32>(0.4, -0.8, 0.4));
    let lambert = max(dot(n, l), 0.0);
    // Soft amber-tinted ambient + directional fill, matching the site theme.
    let ambient = vec3<f32>(0.18, 0.20, 0.26);
    let diffuse = vec3<f32>(0.80, 0.78, 0.70) * lambert;
    let rgb = psx_dither(ambient + diffuse, in.clip_pos.xy, u.psx_params.w);
    return vec4<f32>(rgb, 1.0);
}
"#;

/// Textured-mesh shader: same depth-tested 3D pipeline as `MESH_SHADER_SRC`,
/// but the fragment samples a bound texture (group 1) using the per-vertex
/// UVs from attribute location 1. UVs are pre-normalized to `[0, 1)` by the
/// CPU side (PSX UV bytes / 256.0).
///
/// The single-binding sibling of the VRAM-mesh path. Its `TexturedMesh`
/// carries no per-prim colour word, so there is no modulation to apply and the
/// texel is drawn as-is - which is exactly what the GPU does for the neutral
/// colour `0x80`. The field/town renderer uses the VRAM-mesh pipeline, which
/// does carry the colour.
pub(crate) const TEXTURED_MESH_SHADER_SRC: &str = r#"
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
@group(1) @binding(0) var t_color: texture_2d<f32>;
@group(1) @binding(1) var s_color: sampler;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs_main(@location(0) position: vec3<f32>, @location(1) uv: vec2<f32>) -> VsOut {
    var out: VsOut;
    out.clip_pos = u.mvp * vec4<f32>(position, 1.0);
    out.world_pos = position;
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // No per-prim colour on this vertex format: draw the raw texel (the GPU's
    // behaviour for the neutral modulation colour 0x80), grade it, then
    // depth-cue it. Retail's DPCS runs on the packet colour BEFORE the texel
    // multiply, so the far term is the far colour modulated by the texel; the
    // grade tint models the modulation colour and belongs to the near term
    // only.
    let texel = textureSample(t_color, s_color, in.uv);
    let graded = apply_grade(texel.rgb, u.grade);
    let ir0 = cue_ramp_ir0(u.depth_cue.a, u.cue_ramp, in.clip_pos.w);
    let cued = psx_depth_cue(graded, psx_modulate(texel.rgb, u.depth_cue.rgb * 255.0), ir0);
    let rgb = psx_dither(cued, in.clip_pos.xy, u.psx_params.w);
    return vec4<f32>(rgb, texel.a);
}
"#;

//! The untextured colour mesh shader.

/// Vertex-colour mesh shader: untextured `F*`/`G*` props. Each vertex carries a
/// position, an RGB colour, and a blend word (ABE bit 15 + ABR bits 5..=6,
/// see `psx_blend::pack_blend_word`). No VRAM lookup - the colour comes
/// straight from the TMD's per-prim colour block (`legaia_tmd::mesh::ColorMesh`),
/// and it is drawn **as-is**: an untextured PSX prim is filled with its packet
/// colour, with no modulation and no light source. That colour is the baked
/// shading, so re-lighting it would double-shade the prim.
///
/// PSX semi-transparency (`psx_params.z >= 0.5`): an untextured ABE prim
/// blends **all** its pixels on retail hardware (no per-texel STP gate), so
/// the opaque entry discards those prims entirely and the `fs_blend` /
/// `fs_blend_quarter` entries re-draw them through the per-ABR-mode
/// fixed-function blend pipelines.
pub(crate) const COLOR_MESH_SHADER_SRC: &str = r#"
struct MeshUniforms {
    mvp: mat4x4<f32>,
    depth_cue: vec4<f32>,
    psx_params: vec4<f32>,
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
    // The draw's model (mesh -> world) matrix as its three affine rows
    // (see the VRAM-mesh shader). Only the per-scene point-light layer
    // reads it.
    model_r0: vec4<f32>,
    model_r1: vec4<f32>,
    model_r2: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: MeshUniforms;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) blend: u32,
    // World-space position (`u.model * position`) for the per-scene
    // point-light layer.
    @location(3) world_w: vec3<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) blend: u32,
    @location(7) prim_c0: vec4<f32>,
    @location(8) prim_c1: vec3<f32>,
    @location(9) prim_c2: vec3<f32>,
    @location(10) prim_c3: vec3<f32>,
) -> VsOut {
    var out: VsOut;
    out.clip_pos = overworld_curve_clip(u.mvp * vec4<f32>(position, 1.0), u.flags.w);
    // The PSX vertex snap, after the bend, as the VRAM mesh takes it: the
    // GTE hands an untextured prim integer screen coordinates too.
    if u.psx_params.z >= 0.5 {
        out.clip_pos = psx_snap_clip(out.clip_pos, u.psx_params.x, u.psx_params.y);
    }
    if prim_near_rejected(u.mvp, prim_c0, prim_c1, prim_c2, prim_c3) {
        out.clip_pos = PRIM_REJECTED_CLIP;
    }
    out.world_pos = position;
    out.color = color;
    out.blend = blend;
    let p4 = vec4<f32>(position, 1.0);
    out.world_w = vec3<f32>(dot(u.model_r0, p4), dot(u.model_r1, p4), dot(u.model_r2, p4));
    return out;
}

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    // Object-effect clip of a raised `+0x42` (see EFFECT_CLIP_WGSL).
    if effect_clipped(in.world_pos) {
        discard;
    }
    // Retail GTE NCLIP winding rejection (see MeshUniforms.flags).
    if (u.flags.x >= 0.5 && u.flags.x < 1.5 && !front_facing)
        || (u.flags.x >= 1.5 && front_facing) {
        discard;
    }
    // Double-sided pair copies (blend bit 14, set by
    // `legaia_tmd::mesh::resolve_hybrid` - the colour-half twin of the
    // VRAM-mesh shader's CBA bit 15): draw only the camera-facing copy,
    // same field-frame parity as the textured discard above it mirrors.
    if ((in.blend & 0x4000u) != 0u && front_facing) {
        discard;
    }
    // Semi-transparency (ABE): an untextured ABE prim blends every pixel, so
    // nothing of it belongs in the opaque pass - the blend pass re-draws it
    // from the per-ABR-mode index tail. Gated on the semi-blend flag
    // (`flags.y`, default on). Mirrors the textured opaque pass discarding
    // STP texels of semi prims.
    if (u.flags.y >= 0.5 && (in.blend & 0x8000u) != 0u) {
        discard;
    }
    // Camera-occlusion fade (opt-in see-through-walls enhancement):
    // screen-door discard of fragments between the camera and the player,
    // mirroring the textured opaque pass. Identity while disabled.
    if (u.flags.z > 0.5
        && occl_bayer(in.clip_pos.xy) >= occl_keep(in.clip_pos.xy, in.clip_pos.w)) {
        discard;
    }
    // An untextured PSX prim is filled with its packet colour directly - no
    // modulation, no light source. The colour IS the baked shading. The
    // opt-in dynamic light (identity when disabled) layers over it; this
    // vertex format carries no normals, so the helper falls back to the
    // screen-space geometric normal (flat per-face shading on props).
    // In prologue palette mode the authored colour word collapses to gold
    // (retail grades the loaded TMD colour words at load - see the prelude).
    let palette_on = u.palette.w > 0.5;
    var base = in.color.rgb;
    if palette_on {
        let m = max(base.r, max(base.g, base.b));
        base = prologue_sepia_word(m * 255.0) / 255.0;
    }
    let geo_n = cross(dpdx(in.world_pos), dpdy(in.world_pos));
    let pg = scene_point_gain(in.world_w, vec3<f32>(0.0), geo_n);
    let enhanced = dyn_light(
        base, vec3<f32>(0.0), geo_n, in.clip_pos.xy, u.psx_params.xy,
        u.light_dir, u.light_color, pg, (in.blend & 0x2000u) != 0u,
    );
    // An untextured prim is filled with the packet colour, so its DPCS far
    // term is the far colour itself (no texel multiply). Grade-then-cue,
    // matching the textured path; the far colour is not re-tinted. Palette
    // mode carries the global tint in `palette.rgb` and holds the cue ramp
    // inert, exactly as the textured path does.
    var graded: vec3<f32>;
    var ir0: f32;
    if palette_on {
        graded = enhanced * u.palette.rgb;
        ir0 = u.depth_cue.a;
    } else {
        graded = apply_grade(enhanced, u.grade);
        ir0 = cue_ramp_ir0(u.depth_cue.a, u.cue_ramp, in.clip_pos.w);
    }
    let cued = psx_depth_cue(graded, dyn_far(u.depth_cue.rgb, u.light_dir, u.light_color), ir0);
    let rgb = psx_dither(cued, in.clip_pos.xy, u.psx_params.w);
    return vec4<f32>(rgb, 1.0);
}

// Blend-pass entries: emit the prim colour as the foreground term F; the
// per-mode fixed-function blend state combines it with the framebuffer.
// Only runs in PSX-faithful mode, where the synthetic Lambert is off
// (shade = 1.0). Unlike the textured blend pass (raw texels - never
// dithered on retail), an untextured ABE prim's foreground IS shading
// arithmetic output, which retail dithers down to 15-bit before the blend
// math - so the dither stage applies to F here, before mode 3's 0.25
// pre-scale (retail folds that scale into the blend itself).
fn blend_pass_color(in: VsOut, front_facing: bool, f_scale: f32) -> vec4<f32> {
    if effect_clipped(in.world_pos) {
        discard;
    }
    // Retail GTE NCLIP winding rejection, the same test as the opaque entry:
    // retail's prim leaves cull a semi prim's back face exactly as they cull
    // an opaque one, so without it an open translucent strand blends twice
    // wherever its far side faces the camera.
    if (u.flags.x >= 0.5 && u.flags.x < 1.5 && !front_facing)
        || (u.flags.x >= 1.5 && front_facing) {
        discard;
    }
    // Double-sided pair copies: same facing discard as the opaque entry
    // (mirrors the textured blend pass).
    if ((in.blend & 0x4000u) != 0u && front_facing) {
        discard;
    }
    // Camera-occlusion fade: same screen-door as the opaque colour pass.
    if (u.flags.z > 0.5
        && occl_bayer(in.clip_pos.xy) >= occl_keep(in.clip_pos.xy, in.clip_pos.w)) {
        discard;
    }
    // Opt-in dynamic light first (identity when disabled), matching the
    // opaque colour pass so a semi prim blends against equally-lit pixels.
    // Prologue palette mode mirrors the opaque colour pass exactly.
    let palette_on = u.palette.w > 0.5;
    var base = in.color.rgb;
    if palette_on {
        let m = max(base.r, max(base.g, base.b));
        base = prologue_sepia_word(m * 255.0) / 255.0;
    }
    let geo_n = cross(dpdx(in.world_pos), dpdy(in.world_pos));
    let pg = scene_point_gain(in.world_w, vec3<f32>(0.0), geo_n);
    let enhanced = dyn_light(
        base, vec3<f32>(0.0), geo_n, in.clip_pos.xy, u.psx_params.xy,
        u.light_dir, u.light_color, pg, (in.blend & 0x2000u) != 0u,
    );
    // Grade-then-cue, matching the opaque colour pass (far colour direct -
    // an untextured prim has no texel multiply).
    var graded: vec3<f32>;
    var ir0: f32;
    if palette_on {
        graded = enhanced * u.palette.rgb;
        ir0 = u.depth_cue.a;
    } else {
        graded = apply_grade(enhanced, u.grade);
        ir0 = cue_ramp_ir0(u.depth_cue.a, u.cue_ramp, in.clip_pos.w);
    }
    let cued = psx_depth_cue(graded, dyn_far(u.depth_cue.rgb, u.light_dir, u.light_color), ir0);
    let rgb = psx_dither(cued, in.clip_pos.xy, u.psx_params.w);
    // Saturate before the mode-3 pre-scale (see the VRAM-mesh blend pass).
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * f_scale, 1.0);
}

@fragment
fn fs_blend(in: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    return blend_pass_color(in, front_facing, 1.0);
}

@fragment
fn fs_blend_quarter(in: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    return blend_pass_color(in, front_facing, 0.25);
}
"#;

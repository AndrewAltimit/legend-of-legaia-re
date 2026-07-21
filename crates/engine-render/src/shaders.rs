//! WGSL shader sources for the renderer's pipelines, plus the PSX
//! 24->15-bit ordered-dither compose helper. Extracted from the crate root.

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

/// PSX 24-bit -> 15-bit ordered-dither helper, prepended to every shaded
/// 3D shader via [`compose_psx_shader`]. Mirrors [`psx_dither`] on the CPU
/// (kept in lockstep - see that module's tests). Gated on `enable` so the
/// default (non-PSX) render path is bit-unchanged.
pub(crate) const PSX_DITHER_WGSL: &str = r#"
// PSX GPU ordered dithering: a signed 4x4 offset is added to each 8-bit
// colour component before truncation to the 5-bit-per-channel BGR555
// framebuffer. `enable < 0.5` passes the colour through unchanged.
// Reference: nocash PSX-SPX "GPU - Dithering/Color-Depth".
fn psx_dither(rgb: vec3<f32>, frag: vec2<f32>, dither_on: f32) -> vec3<f32> {
    if (dither_on < 0.5) {
        return rgb;
    }
    var dm = array<f32, 16>(
        -4.0,  0.0, -3.0,  1.0,
         2.0, -2.0,  3.0, -1.0,
        -3.0,  1.0, -4.0,  0.0,
         3.0, -1.0,  2.0, -2.0,
    );
    let xi = u32(frag.x) & 3u;
    let yi = u32(frag.y) & 3u;
    let d = dm[yi * 4u + xi];
    var outc = vec3<f32>(0.0, 0.0, 0.0);
    for (var i = 0u; i < 3u; i = i + 1u) {
        let c8 = clamp(rgb[i] * 255.0 + d, 0.0, 255.0);
        let c5 = floor(c8 / 8.0);            // truncate to 5 bits
        let e8 = c5 * 8.0 + floor(c5 / 4.0); // expand 5->8: (c5<<3)|(c5>>2)
        outc[i] = e8 / 255.0;
    }
    return outc;
}

// Full-scene colour grade. `grade.rgb` is a per-channel multiply tint and
// `grade.a` the strength: the shaded pixel is cross-faded toward
// `rgb * grade.rgb`. strength 0 (the identity default) passes the colour
// through unchanged.
//
// A *multiply*, not a luminance collapse, to match the retail mechanism:
// the prologue's amber is baked into the drawn colour (dim ambient + gold
// far-colour depth cue folded into per-vertex gouraud / texture-modulation
// words), while bulk backdrop textures draw at NEUTRAL 0x808080 modulation
// and keep their pre-baked warm texel chroma. A multiply preserves that
// texel chroma exactly the way retail does; a luminance collapse would
// flatten it. Drives the opening prologue (`opdeene`): green/blue crush
// toward amber while the narration text - drawn by a separate shader -
// stays white.
fn apply_grade(rgb: vec3<f32>, grade: vec4<f32>) -> vec3<f32> {
    if (grade.a <= 0.0) {
        return rgb;
    }
    return mix(rgb, rgb * grade.rgb, grade.a);
}

// Prologue palette-collapse law - the retail gold grade's ASSET half,
// capture-pinned against the recomp's live VRAM during the opdeene opening:
// every terrain/prop CLUT entry the scene uploaded had been rewritten from
// the disc TIM's entry `(r, g, b)` to
//     L = max(r, g, b);  (L, max(L - 1, 0), L >> 1)
// in 5-bit BGR555 space, STP bit preserved (0 mismatches across the graded
// rows - see docs/subsystems/cutscene.md "full-scene sepia grade"). Because
// a CLUT rewrite is per palette entry and a 4/8bpp texel IS a palette entry,
// applying the law to the decoded texel word is exactly equivalent.
fn palette_law_word(w: u32) -> u32 {
    let r = w & 0x1Fu;
    let g = (w >> 5u) & 0x1Fu;
    let b = (w >> 10u) & 0x1Fu;
    let l = max(r, max(g, b));
    let g2 = max(l, 1u) - 1u;
    return (w & 0x8000u) | ((l >> 1u) << 10u) | (g2 << 5u) | l;
}

// The packet-colour half of the same grade: retail's prologue draw list
// carries a small amber family of modulation words - each the collapse of a
// full-colour authored TMD word to `max(rgb)` scaled by the gold ratio
// (`~(1.0, 0.94, 0.43)` - the staged `grade.rgb`) - while runtime-emitted
// neutral `0x80,0x80,0x80` words (the ground tile kernel) stay neutral.
// `prim` is in 0..255 colour-byte units.
fn palette_collapse_prim(prim: vec3<f32>, gold: vec3<f32>) -> vec3<f32> {
    if (prim.r == 128.0 && prim.g == 128.0 && prim.b == 128.0) {
        return prim;
    }
    let m = max(prim.r, max(prim.g, prim.b));
    return gold * m;
}

// PSX GPU texture blending - THE field lighting model.
//
// A textured primitive's texel is modulated by the packet colour:
//     out = texel * colour / 128
// so colour 0x80 leaves the texel exactly as-is, 0x00 blacks it out, and 0xFF
// brightens it by 255/128 ~= 2x. The colour is the TMD prim's baked colour
// word - retail's two TMD renderers issue no GTE light op at all (their only
// colour op is DPCS, the depth cue below), so all field shading is baked here
// and this multiply is the whole of it. Dropping it flattens the scene to the
// raw texel and loses BOTH tails of the contrast: across a town's env packs
// ~81% of colour components sit below 0x80 (darkening) and ~12% above
// (brightening).
//
// `colour` is the raw 0..255 byte value. Saturates at 1.0, like the GPU.
// Reference: nocash PSX-SPX "GPU - Texture Color / Blending".
fn psx_modulate(texel: vec3<f32>, colour: vec3<f32>) -> vec3<f32> {
    return clamp(texel * colour / 128.0, vec3<f32>(0.0), vec3<f32>(1.0));
}

// GTE depth cue (DPCS, cop2 0x780010) - the one colour op the retail TMD
// renderers run. Blends the shaded colour toward a far term by IR0:
//     out = c + (fc - c) * ir0
// `far_rgb` is the far term in 0..1, `ir0` the blend factor in 0..1 (hardware
// 0..0x1000). ir0 = 0 is the identity - the unfogged field case, and what a
// town0c retail capture shows (baked colours leave the GTE's RGB FIFO
// byte-unchanged).
//
// Retail runs DPCS on the PACKET COLOUR before the GPU's texture blend
// (`texel * colour / 128`), so on a textured prim the far term arrives at the
// pixel as `texel * far / 128` - callers pass `psx_modulate(texel, far_bytes)`
// as `far_rgb`. An untextured prim is filled with the packet colour directly,
// so its far term is the far colour itself.
fn psx_depth_cue(rgb: vec3<f32>, far_rgb: vec3<f32>, ir0: f32) -> vec3<f32> {
    if (ir0 <= 0.0) {
        return rgb;
    }
    return clamp(mix(rgb, far_rgb, ir0), vec3<f32>(0.0), vec3<f32>(1.0));
}

// View-depth-driven IR0 - the prologue's per-render-node depth-cue pull.
//
// Retail stages the DPCS far colour + IR0 PER RENDER NODE (`+0x74` / `+0x78`,
// written by the cutscene host across the opening's narration beats), so far
// scenery (sky planes, distant spires) is pulled hard toward the gold far
// colour while near ground keeps the modulation tint almost unblended. The
// engine reproduces that depth dependence with a linear view-depth ramp:
//     ir0(z) = clamp((z - near) * inv_range, 0, 1) * max_ir0
// `ramp = (near_z, inv_range, max_ir0, enable)`; `frag_w` is the fragment
// `@builtin(position).w` = 1/clip_w, so `view_z = 1/frag_w` is the view-space
// depth the vertex projected at. `enable <= 0` falls back to the constant
// `cue_a` (the fog-op path; 0 = the unfogged identity default).
fn cue_ramp_ir0(cue_a: f32, ramp: vec4<f32>, frag_w: f32) -> f32 {
    if (ramp.w <= 0.0) {
        return cue_a;
    }
    let view_z = 1.0 / max(frag_w, 1e-8);
    return clamp((view_z - ramp.x) * ramp.y, 0.0, 1.0) * ramp.z;
}

// ---- Opt-in dynamic-lighting enhancement (NON-RETAIL) ----
//
// Retail field rendering has no light source at all (see `psx_modulate`
// above - the shading is baked into the TMD colour words). This helper is a
// deliberate opt-in enhancement layered *over* the baked shading, gated on
// `light_dir.w`: when it is zero (the default) the input colour is returned
// bit-unchanged, keeping the faithful path pixel-identical.
//
// Model:  out = baked * min(ambient + (diffuse*|N.L| + pool) * tint, MAX)
//   * |N.L| - a soft directional term off the smoothed per-vertex normals
//     (the TMD-derived normals retail authors but never feeds to the GTE on
//     the field path). abs() rather than max(,0) because the corpus' prim
//     winding is mixed - the accumulated normals have no canonical side -
//     and it keeps a mostly-vertical light Y-flip invariant.
//   * pool - a soft screen-space "pool of light" centred a touch above the
//     middle of the frame, falling off toward the corners: the reference
//     look's gentle vignette-of-light over the ground.
//   * the whole gain is clamped to DYN_MAX_GAIN so nothing ever exceeds
//     ~1.3x the baked brightness (no blow-out), and the result saturates.
//
// Tunables (kept in lockstep with the CPU mirror `crate::dyn_light`):
const DYN_DIFFUSE: f32 = 0.55;   // weight of the orientation (|N.L|) term
const DYN_POOL: f32 = 0.35;      // weight of the screen-space light pool
const DYN_MAX_GAIN: f32 = 1.3;   // gain ceiling relative to baked colour
const DYN_LAMBERT_FALLBACK: f32 = 0.6; // orientation term when no normal exists
const DYN_POOL_CENTER: vec2<f32> = vec2<f32>(0.5, 0.45); // pool centre, 0..1
const DYN_POOL_INNER: f32 = 0.15; // pool full-strength radius (screen frac)
const DYN_POOL_OUTER: f32 = 0.75; // pool fade-out radius (screen frac)

fn dyn_light(
    rgb: vec3<f32>,
    vert_normal: vec3<f32>,
    geo_normal: vec3<f32>,
    frag_px: vec2<f32>,
    viewport: vec2<f32>,
    light_dir: vec4<f32>,
    light_color: vec4<f32>,
) -> vec3<f32> {
    if (light_dir.w < 0.5) {
        return rgb;
    }
    // Orientation term. Prefer the smoothed per-vertex normal (continuous
    // shading across connected surfaces); fall back to the screen-space
    // geometric normal for zero entries (singleton normal bins, and the
    // untextured colour-mesh path which carries no normals at all).
    var lambert = DYN_LAMBERT_FALLBACK;
    var n = vert_normal;
    if (dot(n, n) < 1e-8) {
        n = geo_normal;
    }
    let n_len = length(n);
    if (n_len > 1e-6) {
        lambert = abs(dot(n / n_len, normalize(light_dir.xyz)));
    }
    // Soft screen-space light pool.
    var pool = 0.0;
    if (viewport.x > 0.0 && viewport.y > 0.0) {
        let d = distance(frag_px / viewport, DYN_POOL_CENTER);
        pool = 1.0 - smoothstep(DYN_POOL_INNER, DYN_POOL_OUTER, d);
    }
    let gain = light_color.w + (DYN_DIFFUSE * lambert + DYN_POOL * pool) * light_color.xyz;
    let capped = min(gain, vec3<f32>(DYN_MAX_GAIN));
    return clamp(rgb * capped, vec3<f32>(0.0), vec3<f32>(1.0));
}
"#;

/// Prepend [`PSX_DITHER_WGSL`] to a shaded 3D shader source so its fragment
/// stage can call `psx_dither`. WGSL module-scope declarations are
/// order-independent, so the leading helper is valid ahead of the shader's
/// own structs and entry points.
pub(crate) fn compose_psx_shader(base: &str) -> String {
    format!("{PSX_DITHER_WGSL}\n{base}")
}

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

/// VRAM-mesh shader: faithful PSX texture lookup + retail's lighting.
///
/// Each vertex carries `(u, v, cba, tsb)` and the prim's baked colour word
/// alongside its position. The fragment shader computes, per-fragment:
///   * texture-page origin from `tsb` (`tpage_x = (tsb & 0xF) * 64`,
///     `tpage_y = ((tsb >> 4) & 1) * 256`),
///   * pixel-format from `tsb` bits 7..8 (0 = 4bpp, 1 = 8bpp, 2 = 15bpp),
///   * for 4/8 bpp, indexes into the in-VRAM CLUT at
///     `(cba & 0x3F) * 16, (cba >> 6) & 0x1FF`,
///   * decodes the resulting BGR555 + STP word to RGBA,
///   * modulates it by the prim colour, then applies the GTE depth cue.
///
/// **Lighting.** Retail's two TMD renderers (`FUN_8002735c` and its
/// light-source sibling `FUN_80029888`) run exactly one GTE colour op between
/// them - `DPCS` (`cop2 0x780010`, depth cue). Neither ever issues `NCDS` /
/// `NCS` / `NCCS`, so there is no runtime light source on the field path: the
/// lighting is *baked into the TMD's colour words* and applied by the GPU's
/// texture blend, `texel * colour / 128`. `0x80` is neutral, below darkens,
/// above brightens (to nearly 2x). See [`psx_modulate`] and
/// [`crate::renderer::Renderer::set_depth_cue`].
pub(crate) const VRAM_MESH_SHADER_SRC: &str = r#"
struct MeshUniforms {
    mvp: mat4x4<f32>,
    // GTE depth cue: (far_r, far_g, far_b, ir0), all 0..1. ir0 = 0 = identity.
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
@group(1) @binding(0) var t_vram: texture_2d<u32>;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    // PSX hardware does affine (linear-in-screen-space) UV interpolation,
    // not perspective-correct. WGSL's `@interpolate(linear)` gives exactly
    // that. The float UV is converted to integer texel coordinates in the
    // fragment shader.
    @location(1) @interpolate(linear) uv_affine: vec2<f32>,
    @location(2) @interpolate(flat) cba_tsb: vec2<u32>,
    @location(3) normal: vec3<f32>,
    // Baked prim colour (raw 0..255 bytes). Gouraud prims carry one word per
    // corner, flat prims the same word on every corner - so interpolating is
    // correct for both. PSX gouraud interpolation is affine in screen space,
    // same as the UVs, hence `linear` rather than the default perspective.
    @location(4) @interpolate(linear) prim_color: vec3<f32>,
};

// Snap a clip-space x/y to the nearest integer pixel of a viewport sized
// (vp_w, vp_h). Returns the snapped clip position with z/w preserved. This
// is the GTE-style "vertex jitter" that gives PSX rendering its
// characteristic shimmer on slow-moving geometry.
fn psx_snap_clip(clip: vec4<f32>, vp_w: f32, vp_h: f32) -> vec4<f32> {
    if vp_w <= 0.0 || vp_h <= 0.0 {
        return clip;
    }
    // NDC after perspective divide.
    let ndc_x = clip.x / clip.w;
    let ndc_y = clip.y / clip.w;
    // Pixel coords (NDC -> [0, vp]).
    let px = (ndc_x * 0.5 + 0.5) * vp_w;
    let py = (ndc_y * 0.5 + 0.5) * vp_h;
    // Snap to nearest integer pixel.
    let snapped_x = floor(px + 0.5);
    let snapped_y = floor(py + 0.5);
    // Back to NDC.
    let nx = (snapped_x / vp_w) * 2.0 - 1.0;
    let ny = (snapped_y / vp_h) * 2.0 - 1.0;
    // Re-multiply by w to rebuild clip space.
    return vec4<f32>(nx * clip.w, ny * clip.w, clip.z, clip.w);
}

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) uv_in: vec4<u32>,
    @location(2) cba_tsb_in: vec2<u32>,
    @location(3) normal_in: vec3<f32>,
    @location(4) color_in: vec4<u32>,
) -> VsOut {
    var out: VsOut;
    var clip = u.mvp * vec4<f32>(position, 1.0);
    if u.psx_params.z >= 0.5 {
        clip = psx_snap_clip(clip, u.psx_params.x, u.psx_params.y);
    }
    out.clip_pos = clip;
    out.world_pos = position;
    out.uv_affine = vec2<f32>(f32(uv_in.x), f32(uv_in.y));
    out.cba_tsb = cba_tsb_in;
    out.normal = normal_in;
    out.prim_color = vec3<f32>(f32(color_in.x), f32(color_in.y), f32(color_in.z));
    return out;
}

fn bgr555_to_rgba(c: u32) -> vec4<f32> {
    let r = f32(c & 0x1Fu) / 31.0;
    let g = f32((c >> 5u) & 0x1Fu) / 31.0;
    let b = f32((c >> 10u) & 0x1Fu) / 31.0;
    let stp = (c >> 15u) & 1u;
    var alpha = 1.0;
    if c == 0u && stp == 0u {
        alpha = 0.0;
    }
    return vec4<f32>(r, g, b, alpha);
}

// Fetch the raw BGR555+STP VRAM word for one fragment: texture-window
// remap, texture-page origin, 4/8/15bpp decode + CLUT lookup.
fn fetch_vram_word(uv_affine: vec2<f32>, cba: u32, tsb: u32) -> u32 {
    // Convert linearly-interpolated affine UV float -> integer texel.
    // Truncate (PSX behaviour: GP0 G3 commands transmit signed 8-bit UV
    // bytes; the rasterizer takes the integer part of the interpolated
    // position).
    var u_pix = u32(max(uv_affine.x, 0.0)) & 0xFFu;
    var v_pix = u32(max(uv_affine.y, 0.0)) & 0xFFu;
    // Apply GP0(0xE2) texture-window register, in pixel space:
    //   coord = (coord & ~(mask*8)) | ((offset & mask) * 8)
    // No-op when mask == 0 (the all-zero default) since the AND-NOT is
    // identity and the OR adds zero. Hardware reference: GPU command list
    // section "GP0(E2h) - Texture Window setting (Mask/Offset)".
    let mask_x = u.tex_window.x * 8u;
    let mask_y = u.tex_window.y * 8u;
    let off_x = (u.tex_window.z & u.tex_window.x) * 8u;
    let off_y = (u.tex_window.w & u.tex_window.y) * 8u;
    u_pix = (u_pix & (~mask_x & 0xFFu)) | (off_x & 0xFFu);
    v_pix = (v_pix & (~mask_y & 0xFFu)) | (off_y & 0xFFu);

    let tpage_x = (tsb & 0xFu) * 64u;
    let tpage_y = ((tsb >> 4u) & 1u) * 256u;
    let depth = (tsb >> 7u) & 0x3u; // 0=4bpp, 1=8bpp, 2=15bpp

    if depth == 0u {
        // 4bpp: 4 nibbles per VRAM word.
        let vx = i32(tpage_x + (u_pix >> 2u));
        let vy = i32(tpage_y + v_pix);
        let word = textureLoad(t_vram, vec2<i32>(vx, vy), 0).r;
        let nibble = u_pix & 3u;
        let pal_idx = (word >> (nibble * 4u)) & 0xFu;
        let cx = i32((cba & 0x3Fu) * 16u + pal_idx);
        let cy = i32((cba >> 6u) & 0x1FFu);
        return textureLoad(t_vram, vec2<i32>(cx, cy), 0).r;
    } else if depth == 1u {
        // 8bpp: 2 bytes per VRAM word.
        let vx = i32(tpage_x + (u_pix >> 1u));
        let vy = i32(tpage_y + v_pix);
        let word = textureLoad(t_vram, vec2<i32>(vx, vy), 0).r;
        let byte_sel = u_pix & 1u;
        let pal_idx = (word >> (byte_sel * 8u)) & 0xFFu;
        let cx = i32((cba & 0x3Fu) * 16u + pal_idx);
        let cy = i32((cba >> 6u) & 0x1FFu);
        return textureLoad(t_vram, vec2<i32>(cx, cy), 0).r;
    }
    // 15/16 bpp direct: one VRAM word per texel.
    let vx = i32(tpage_x + u_pix);
    let vy = i32(tpage_y + v_pix);
    return textureLoad(t_vram, vec2<i32>(vx, vy), 0).r;
}

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    // Retail GTE NCLIP winding rejection (see MeshUniforms.flags).
    if (u.flags.x >= 0.5 && u.flags.x < 1.5 && !front_facing)
        || (u.flags.x >= 1.5 && front_facing) {
        discard;
    }
    let tsb = in.cba_tsb.y;
    let cba = in.cba_tsb.x;
    var word = fetch_vram_word(in.uv_affine, cba, tsb);
    // Prologue palette-collapse grade: retail rewrites the scene's uploaded
    // CLUTs (and TMD colour words) to the gold law at load; the engine's
    // VRAM carries the disc palettes, so the law applies per decoded texel
    // (exactly equivalent - see `palette_law_word`). Transparency is judged
    // on the raw word below, which the law preserves (0 -> 0, STP kept).
    let palette_on = u.palette.w > 0.5;
    if palette_on {
        word = palette_law_word(word);
    }
    let color = bgr555_to_rgba(word);

    // Discard fully transparent texels (PSX STP=0 with all-zero pixel) so
    // characters with cutout textures don't render solid black quads.
    if color.a <= 0.0 {
        discard;
    }

    // PSX semi-transparency, opaque pass: STP texels of a semi-transparent
    // prim (TSB bit 15 = the engine-packed ABE enable) belong to the blend
    // pass - defer them. Gated on the semi-blend flag (`flags.y`, default on),
    // the same flag the CPU-side blend pass keys off; when it is off the prim
    // draws fully opaque.
    if u.flags.y >= 0.5 && (tsb & 0x8000u) != 0u && ((word >> 15u) & 1u) == 1u {
        discard;
    }

    // Retail's lighting: modulate the texel by the prim's baked colour word
    // (`texel * colour / 128`), then apply the GTE depth cue. No light source,
    // no normals - see `psx_modulate`. The TMD normals are still carried on
    // the vertex (the lit descriptor rows do author them) but retail never
    // feeds them to the GTE on the field path; only the OPT-IN dynamic-light
    // enhancement below consumes them (identity when disabled, the default).
    // In palette mode the packet colour is the gold collapse of the authored
    // word (retail grades the loaded TMD colour words alongside the CLUTs;
    // runtime-emitted neutral words stay neutral).
    var prim = in.prim_color;
    if palette_on {
        prim = palette_collapse_prim(prim, u.grade.rgb);
    }
    let lit = psx_modulate(color.rgb, prim);
    let geo_n = cross(dpdx(in.world_pos), dpdy(in.world_pos));
    let enhanced = dyn_light(
        lit, in.normal, geo_n, in.clip_pos.xy, u.psx_params.xy, u.light_dir, u.light_color,
    );
    // Retail order: DPCS blends the PACKET COLOUR toward the far colour, then
    // the GPU multiplies the texel by the result - so the far term reaches the
    // pixel as `texel * far / 128` (`psx_modulate` with the far colour in
    // colour-byte units). The grade tint models the amber modulation colour
    // and belongs to the near term only; the far colour is staged in absolute
    // display units and is not re-tinted.
    //
    // Palette mode replaces the pixel multiply with the texel/packet collapse
    // above (`apply_grade` would double-grade), carries the global screen
    // tint in `palette.rgb`, and makes the view-depth cue ramp inert - the
    // opening capture shows every render node holds `IR0 = 0` across the
    // prologue, so the far-field crush is entirely the palette law + the
    // amber packet words, not a depth cue.
    var graded: vec3<f32>;
    var ir0: f32;
    if palette_on {
        graded = enhanced * u.palette.rgb;
        ir0 = u.depth_cue.a;
    } else {
        graded = apply_grade(enhanced, u.grade);
        ir0 = cue_ramp_ir0(u.depth_cue.a, u.cue_ramp, in.clip_pos.w);
    }
    let cued = psx_depth_cue(graded, psx_modulate(color.rgb, u.depth_cue.rgb * 255.0), ir0);
    let rgb = psx_dither(cued, in.clip_pos.xy, u.psx_params.w);
    return vec4<f32>(rgb, color.a);
}

// PSX semi-transparency blend pass: re-draws the semi-transparent prims
// (the per-ABR-mode index tail) keeping ONLY texels whose STP bit is set -
// the exact complement of the opaque pass's deferral. `f_scale` pre-scales
// the foreground for ABR mode 3 (`B + 0.25*F`); the rest of the equation is
// fixed-function blend state. Runs only in PSX-faithful mode, so the colour
// is unlit (matching the opaque pass's faithful branch). No dither here:
// this pass's foreground is a raw 15bpp texel - retail dithers only
// shading-arithmetic results (gouraud / texture blending), never raw
// texels, and the 5-bit blend math itself is not dithered (PSX-SPX
// "GPU - Dithering/Color-Depth").
fn blend_pass_color(in: VsOut, f_scale: f32) -> vec4<f32> {
    let tsb = in.cba_tsb.y;
    let cba = in.cba_tsb.x;
    var word = fetch_vram_word(in.uv_affine, cba, tsb);
    // Texel 0x0000 never draws; STP=0 texels were already drawn opaque by
    // the first pass. 0x8000 (black + STP) correctly blends.
    if word == 0u || ((word >> 15u) & 1u) == 0u {
        discard;
    }
    // Same prologue palette-collapse as the opaque pass (STP preserved, so
    // the discard test above is unaffected by ordering).
    let palette_on = u.palette.w > 0.5;
    if palette_on {
        word = palette_law_word(word);
    }
    let color = bgr555_to_rgba(word);
    // Semi-transparent prims are texture-blended by the GPU exactly like the
    // opaque ones - the modulation happens before the blend equation, so it
    // applies here too, and the opt-in dynamic light matches the opaque pass
    // (identity when disabled) so lit water/glass composites consistently.
    var prim = in.prim_color;
    if palette_on {
        prim = palette_collapse_prim(prim, u.grade.rgb);
    }
    let lit = psx_modulate(color.rgb, prim);
    let geo_n = cross(dpdx(in.world_pos), dpdy(in.world_pos));
    let enhanced = dyn_light(
        lit, in.normal, geo_n, in.clip_pos.xy, u.psx_params.xy, u.light_dir, u.light_color,
    );
    // Same grade-then-cue order as the opaque pass (see fs_main): the far
    // term is the texel-modulated far colour, un-tinted.
    var graded: vec3<f32>;
    var ir0: f32;
    if palette_on {
        graded = enhanced * u.palette.rgb;
        ir0 = u.depth_cue.a;
    } else {
        graded = apply_grade(enhanced, u.grade);
        ir0 = cue_ramp_ir0(u.depth_cue.a, u.cue_ramp, in.clip_pos.w);
    }
    let cued = psx_depth_cue(graded, psx_modulate(color.rgb, u.depth_cue.rgb * 255.0), ir0);
    return vec4<f32>(cued * f_scale, 1.0);
}

@fragment
fn fs_blend(in: VsOut) -> @location(0) vec4<f32> {
    return blend_pass_color(in, 1.0);
}

@fragment
fn fs_blend_quarter(in: VsOut) -> @location(0) vec4<f32> {
    return blend_pass_color(in, 0.25);
}
"#;

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
};
@group(0) @binding(0) var<uniform> u: MeshUniforms;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) blend: u32,
};

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) blend: u32,
) -> VsOut {
    var out: VsOut;
    out.clip_pos = u.mvp * vec4<f32>(position, 1.0);
    out.world_pos = position;
    out.color = color;
    out.blend = blend;
    return out;
}

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    // Retail GTE NCLIP winding rejection (see MeshUniforms.flags).
    if (u.flags.x >= 0.5 && u.flags.x < 1.5 && !front_facing)
        || (u.flags.x >= 1.5 && front_facing) {
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
        base = u.grade.rgb * m;
    }
    let geo_n = cross(dpdx(in.world_pos), dpdy(in.world_pos));
    let enhanced = dyn_light(
        base, vec3<f32>(0.0), geo_n, in.clip_pos.xy, u.psx_params.xy,
        u.light_dir, u.light_color,
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
    let cued = psx_depth_cue(graded, u.depth_cue.rgb, ir0);
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
fn blend_pass_color(in: VsOut, f_scale: f32) -> vec4<f32> {
    // Opt-in dynamic light first (identity when disabled), matching the
    // opaque colour pass so a semi prim blends against equally-lit pixels.
    // Prologue palette mode mirrors the opaque colour pass exactly.
    let palette_on = u.palette.w > 0.5;
    var base = in.color.rgb;
    if palette_on {
        let m = max(base.r, max(base.g, base.b));
        base = u.grade.rgb * m;
    }
    let geo_n = cross(dpdx(in.world_pos), dpdy(in.world_pos));
    let enhanced = dyn_light(
        base, vec3<f32>(0.0), geo_n, in.clip_pos.xy, u.psx_params.xy,
        u.light_dir, u.light_color,
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
    let cued = psx_depth_cue(graded, u.depth_cue.rgb, ir0);
    let rgb = psx_dither(cued, in.clip_pos.xy, u.psx_params.w);
    return vec4<f32>(rgb * f_scale, 1.0);
}

@fragment
fn fs_blend(in: VsOut) -> @location(0) vec4<f32> {
    return blend_pass_color(in, 1.0);
}

@fragment
fn fs_blend_quarter(in: VsOut) -> @location(0) vec4<f32> {
    return blend_pass_color(in, 0.25);
}
"#;

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

/// 2D text shader: pre-converted NDC positions + atlas UVs + per-vertex
/// RGBA tint. The fragment multiplies the tint with the sampled atlas
/// texel; the alpha-blend pipeline handles final compositing.
pub(crate) const TEXT_SHADER_SRC: &str = r#"
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
    return vec4<f32>(in.color.rgb * texel.rgb, in.color.a * texel.a);
}
"#;

/// Screen-space 2D overlay shader (see [`crate::screen_overlay`]): PSX
/// `POLY_FT4` textured quads + flat quads in NDC, sampling the shared PSX
/// VRAM (`R16Uint`, group 0) with the same 4/8/15-bpp + CLUT decode the 3D
/// VRAM-mesh path uses. `flags` bit 0 selects textured vs flat; `color` is
/// a `/128` modulation factor for textured quads and a straight `/255`
/// colour for flat quads. The opaque entry (`fs_opaque`) writes solid; the
/// blend entries (`fs_blend` / `fs_blend_quarter`) feed the per-ABR
/// fixed-function [`crate::psx_blend::blend_state`].
pub(crate) const SCREEN_OVERLAY_SHADER_SRC: &str = r#"
@group(0) @binding(0) var t_vram: texture_2d<u32>;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) @interpolate(linear) uv: vec2<f32>,
    @location(1) @interpolate(flat) cba_tsb: vec2<u32>,
    @location(2) color: vec4<f32>,
    @location(3) @interpolate(flat) flags: u32,
};

@vertex
fn vs_main(
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) cba_tsb: vec2<u32>,
    @location(3) color: vec4<f32>,
    @location(4) flags: u32,
) -> VsOut {
    var out: VsOut;
    out.clip_pos = vec4<f32>(pos, 0.0, 1.0);
    out.uv = uv;
    out.cba_tsb = cba_tsb;
    out.color = color;
    out.flags = flags;
    return out;
}

fn bgr555_to_rgba(c: u32) -> vec4<f32> {
    let r = f32(c & 0x1Fu) / 31.0;
    let g = f32((c >> 5u) & 0x1Fu) / 31.0;
    let b = f32((c >> 10u) & 0x1Fu) / 31.0;
    let stp = (c >> 15u) & 1u;
    var alpha = 1.0;
    if c == 0u && stp == 0u {
        alpha = 0.0;
    }
    return vec4<f32>(r, g, b, alpha);
}

// Same texture-page + 4/8/15bpp CLUT decode as the VRAM-mesh shader, minus
// the GP0(E2) texture-window remap (screen sprites never use it).
fn fetch_vram_word(uv: vec2<f32>, cba: u32, tsb: u32) -> u32 {
    let u_pix = u32(max(uv.x, 0.0)) & 0xFFu;
    let v_pix = u32(max(uv.y, 0.0)) & 0xFFu;
    let tpage_x = (tsb & 0xFu) * 64u;
    let tpage_y = ((tsb >> 4u) & 1u) * 256u;
    let depth = (tsb >> 7u) & 0x3u; // 0=4bpp, 1=8bpp, 2=15bpp

    if depth == 0u {
        let vx = i32(tpage_x + (u_pix >> 2u));
        let vy = i32(tpage_y + v_pix);
        let word = textureLoad(t_vram, vec2<i32>(vx, vy), 0).r;
        let nibble = u_pix & 3u;
        let pal_idx = (word >> (nibble * 4u)) & 0xFu;
        let cx = i32((cba & 0x3Fu) * 16u + pal_idx);
        let cy = i32((cba >> 6u) & 0x1FFu);
        return textureLoad(t_vram, vec2<i32>(cx, cy), 0).r;
    } else if depth == 1u {
        let vx = i32(tpage_x + (u_pix >> 1u));
        let vy = i32(tpage_y + v_pix);
        let word = textureLoad(t_vram, vec2<i32>(vx, vy), 0).r;
        let byte_sel = u_pix & 1u;
        let pal_idx = (word >> (byte_sel * 8u)) & 0xFFu;
        let cx = i32((cba & 0x3Fu) * 16u + pal_idx);
        let cy = i32((cba >> 6u) & 0x1FFu);
        return textureLoad(t_vram, vec2<i32>(cx, cy), 0).r;
    }
    let vx = i32(tpage_x + u_pix);
    let vy = i32(tpage_y + v_pix);
    return textureLoad(t_vram, vec2<i32>(vx, vy), 0).r;
}

// Resolve the RGB foreground for one fragment. `discard`s texels that must
// not draw (fully transparent VRAM word 0x0000). Textured quads modulate
// the sampled texel by `color` (a /128 factor); flat quads emit `color`.
fn overlay_color(in: VsOut) -> vec4<f32> {
    if (in.flags & 1u) != 0u {
        let word = fetch_vram_word(in.uv, in.cba_tsb.x, in.cba_tsb.y);
        if word == 0u {
            discard;
        }
        let texel = bgr555_to_rgba(word);
        return vec4<f32>(texel.rgb * in.color.rgb, in.color.a * texel.a);
    }
    return in.color;
}

@fragment
fn fs_opaque(in: VsOut) -> @location(0) vec4<f32> {
    return overlay_color(in);
}

// Blend entries: emit the foreground F; the per-ABR fixed-function blend
// state combines it with the framebuffer B. Mode 3 (`B + 0.25*F`) has no
// fixed-function factor, so `fs_blend_quarter` pre-scales F by 0.25.
@fragment
fn fs_blend(in: VsOut) -> @location(0) vec4<f32> {
    let c = overlay_color(in);
    return vec4<f32>(c.rgb, 1.0);
}

@fragment
fn fs_blend_quarter(in: VsOut) -> @location(0) vec4<f32> {
    let c = overlay_color(in);
    return vec4<f32>(c.rgb * 0.25, 1.0);
}
"#;

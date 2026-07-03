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
"#;

/// Prepend [`PSX_DITHER_WGSL`] to a shaded 3D shader source so its fragment
/// stage can call `psx_dither`. WGSL module-scope declarations are
/// order-independent, so the leading helper is valid ahead of the shader's
/// own structs and entry points.
pub(crate) fn compose_psx_shader(base: &str) -> String {
    format!("{PSX_DITHER_WGSL}\n{base}")
}

/// Mesh shader: transforms positions by the host-supplied MVP, computes a
/// per-fragment normal from screen-space derivatives, lights with a single
/// directional light, and outputs a flat-shaded result. With `psx_mode`
/// (see [`Renderer::set_psx_mode`]) the result is also ordered-dithered to
/// 15-bit via [`PSX_DITHER_WGSL`]; affine warp + vertex jitter live in the
/// VRAM-mesh path.
pub(crate) const MESH_SHADER_SRC: &str = r#"
struct MeshUniforms {
    mvp: mat4x4<f32>,
    light_dir: vec4<f32>,
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
    let l = -normalize(u.light_dir.xyz);
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
/// CPU side (PSX UV bytes / 256.0). Light is applied as a multiplicative
/// shade on top of the texel.
pub(crate) const TEXTURED_MESH_SHADER_SRC: &str = r#"
struct MeshUniforms {
    mvp: mat4x4<f32>,
    light_dir: vec4<f32>,
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
    let dx = dpdx(in.world_pos);
    let dy = dpdy(in.world_pos);
    let n = normalize(cross(dx, dy));
    let l = -normalize(u.light_dir.xyz);
    let lambert = max(dot(n, l), 0.0);
    // Bias so unlit areas aren't pitch black (PSX hardware doesn't really
    // do per-face lighting; we just want some shape readable).
    // In PSX-faithful mode the engine's synthetic directional Lambert is
    // disabled: retail bakes lighting into the GTE-shaded vertex colours /
    // texels rather than re-lighting per frame from a made-up light dir, so
    // faithful mode shows the source data unlit. Default keeps the readable
    // ambient-biased shade.
    let shade = select(0.45 + 0.55 * lambert, 1.0, u.psx_params.z >= 0.5);
    let texel = textureSample(t_color, s_color, in.uv);
    let rgb = psx_dither(texel.rgb * shade, in.clip_pos.xy, u.psx_params.w);
    return vec4<f32>(rgb, texel.a);
}
"#;

/// VRAM-mesh shader: faithful PSX texture lookup.
///
/// Each vertex carries `(u, v, cba, tsb)` alongside its position. The
/// fragment shader computes, per-fragment:
///   * texture-page origin from `tsb` (`tpage_x = (tsb & 0xF) * 64`,
///     `tpage_y = ((tsb >> 4) & 1) * 256`),
///   * pixel-format from `tsb` bits 7..8 (0 = 4bpp, 1 = 8bpp, 2 = 15bpp),
///   * for 4/8 bpp, indexes into the in-VRAM CLUT at
///     `(cba & 0x3F) * 16, (cba >> 6) & 0x1FF`,
///   * decodes the resulting BGR555 + STP word to RGBA.
///
/// Same Lambert-with-ambient-bias shading as the textured-mesh path so the
/// silhouette stays readable; PSX hardware doesn't really do per-face
/// lighting either.
pub(crate) const VRAM_MESH_SHADER_SRC: &str = r#"
struct MeshUniforms {
    mvp: mat4x4<f32>,
    light_dir: vec4<f32>,
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
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let tsb = in.cba_tsb.y;
    let cba = in.cba_tsb.x;
    let word = fetch_vram_word(in.uv_affine, cba, tsb);
    let color = bgr555_to_rgba(word);

    // Discard fully transparent texels (PSX STP=0 with all-zero pixel) so
    // characters with cutout textures don't render solid black quads.
    if color.a <= 0.0 {
        discard;
    }

    // PSX-faithful semi-transparency, opaque pass: STP texels of a
    // semi-transparent prim (TSB bit 15 = the engine-packed ABE enable)
    // belong to the blend pass - defer them. Gated on the same faithful
    // flag as the blend pass so the default path draws everything opaque.
    if u.psx_params.z >= 0.5 && (tsb & 0x8000u) != 0u && ((word >> 15u) & 1u) == 1u {
        discard;
    }

    // Per-vertex normals smooth-shade connected geometry. Mesh-builder
    // emits the zero vector for unbinned positions (singleton triangles or
    // degenerate fallback); detect that and fall back to screen-space
    // derivatives so the result still looks shaded.
    let n_len = length(in.normal);
    var n: vec3<f32>;
    if n_len > 0.001 {
        n = in.normal / n_len;
    } else {
        let dx = dpdx(in.world_pos);
        let dy = dpdy(in.world_pos);
        n = normalize(cross(dx, dy));
    }
    let l = -normalize(u.light_dir.xyz);
    let lambert = max(dot(n, l), 0.0);
    // In PSX-faithful mode the engine's synthetic directional Lambert is
    // disabled: retail bakes lighting into the GTE-shaded vertex colours /
    // texels rather than re-lighting per frame from a made-up light dir, so
    // faithful mode shows the source data unlit. Default keeps the readable
    // ambient-biased shade.
    let shade = select(0.45 + 0.55 * lambert, 1.0, u.psx_params.z >= 0.5);
    let rgb = psx_dither(color.rgb * shade, in.clip_pos.xy, u.psx_params.w);
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
    let word = fetch_vram_word(in.uv_affine, cba, tsb);
    // Texel 0x0000 never draws; STP=0 texels were already drawn opaque by
    // the first pass. 0x8000 (black + STP) correctly blends.
    if word == 0u || ((word >> 15u) & 1u) == 0u {
        discard;
    }
    let color = bgr555_to_rgba(word);
    return vec4<f32>(color.rgb * f_scale, 1.0);
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
/// see `psx_blend::pack_blend_word`), flat face-shaded (screen-space-derivative
/// normal times a Lambert term, the same ambient-biased shade as the textured /
/// VRAM paths) so the silhouette reads. No VRAM lookup - the colour comes
/// straight from the TMD's per-prim colour block (`legaia_tmd::mesh::ColorMesh`).
///
/// PSX semi-transparency (`psx_params.z >= 0.5`): an untextured ABE prim
/// blends **all** its pixels on retail hardware (no per-texel STP gate), so
/// the opaque entry discards those prims entirely and the `fs_blend` /
/// `fs_blend_quarter` entries re-draw them through the per-ABR-mode
/// fixed-function blend pipelines.
pub(crate) const COLOR_MESH_SHADER_SRC: &str = r#"
struct MeshUniforms {
    mvp: mat4x4<f32>,
    light_dir: vec4<f32>,
    psx_params: vec4<f32>,
    tex_window: vec4<u32>,
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
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // PSX-faithful mode: a semi-transparent (ABE) untextured prim blends
    // every pixel, so nothing of it belongs in the opaque pass - the blend
    // pass re-draws it from the per-ABR-mode index tail. Mirrors the
    // textured opaque pass discarding STP texels of semi prims.
    if (u.psx_params.z >= 0.5 && (in.blend & 0x8000u) != 0u) {
        discard;
    }
    let dx = dpdx(in.world_pos);
    let dy = dpdy(in.world_pos);
    let n = normalize(cross(dx, dy));
    let l = -normalize(u.light_dir.xyz);
    let lambert = max(dot(n, l), 0.0);
    // In PSX-faithful mode the engine's synthetic directional Lambert is
    // disabled: retail bakes lighting into the GTE-shaded vertex colours /
    // texels rather than re-lighting per frame from a made-up light dir, so
    // faithful mode shows the source data unlit. Default keeps the readable
    // ambient-biased shade.
    let shade = select(0.45 + 0.55 * lambert, 1.0, u.psx_params.z >= 0.5);
    let rgb = psx_dither(in.color.rgb * shade, in.clip_pos.xy, u.psx_params.w);
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
    let rgb = psx_dither(in.color.rgb, in.clip_pos.xy, u.psx_params.w);
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
    light_dir: vec4<f32>,
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

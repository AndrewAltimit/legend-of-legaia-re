//! The screen-space PSX primitive overlay shader.

/// Screen-space 2D overlay shader (see [`crate::screen_overlay`]): PSX
/// `POLY_FT4` textured quads + flat quads in NDC, sampling the shared PSX
/// VRAM (`R16Uint`, group 0) with the same 4/8/15-bpp + CLUT decode the 3D
/// VRAM-mesh path uses. `flags` bit 0 selects textured vs flat; `color` is
/// a `/128` modulation factor for textured quads and a straight `/255`
/// colour for flat quads. The opaque entry (`fs_opaque`) writes solid; the
/// blend entries ([`crate::psx_blend::blend_entry`]) feed the per-ABR
/// fixed-function [`crate::psx_blend::blend_state`]. A semi-transparent run
/// draws twice, as the VRAM-mesh path does: `fs_semi_opaque` writes its
/// textured prims' STP-clear texels solid, then the blend entries take the
/// STP-set texels and every pixel of its untextured prims.
pub(crate) fn screen_overlay_shader_src() -> String {
    format!("{}\n{}", super::PSX_BLEND_WGSL, SCREEN_OVERLAY_SHADER_BODY)
}

const SCREEN_OVERLAY_SHADER_BODY: &str = r#"
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
    @location(5) depth: f32,
) -> VsOut {
    var out: VsOut;
    // z = 1.0 is the reversed-Z near plane: overlay quads pass the
    // GreaterEqual test against any scene depth and composite on top. A
    // depth-tested quad (flags bit 1, the overworld markers) carries the
    // shared view-projection's normalised depth instead, remapped the way
    // the scene pass remaps it (`reverse_z`: z' = 1 - z), so it hides
    // behind nearer terrain.
    var z = 1.0;
    if (flags & 2u) != 0u {
        z = 1.0 - depth;
    }
    out.clip_pos = vec4<f32>(pos, z, 1.0);
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

// The PSX texture blend at the framebuffer's 5-bit depth
// (`legaia_render_kernels::screen_prim::psx_texture_blend`): `(t5 * c8) >> 7`,
// saturated at 31, so a dim packet colour drops a faint texel to zero rather
// than adding a fraction of a step. `texel` is the decoded 0..1 texel,
// `factor` the vertex's `/128` colour factor. Exact at the neutral 0x80.
fn psx_texture_blend(texel: vec3<f32>, factor: vec3<f32>) -> vec3<f32> {
    let t5 = vec3<u32>(round(texel * 31.0));
    let c8 = vec3<u32>(round(clamp(factor, vec3<f32>(0.0), vec3<f32>(255.0 / 128.0)) * 128.0));
    return vec3<f32>(min((t5 * c8) >> vec3<u32>(7u), vec3<u32>(31u))) / 31.0;
}

// Resolve the RGB foreground for one fragment. `discard`s texels that must
// not draw (fully transparent VRAM word 0x0000). Textured quads modulate
// the sampled texel by `color` (a /128 factor) through the GPU's 5-bit
// texture blend; flat quads emit `color`.
fn overlay_color(in: VsOut) -> vec4<f32> {
    if (in.flags & 1u) != 0u {
        let word = fetch_vram_word(in.uv, in.cba_tsb.x, in.cba_tsb.y);
        if word == 0u {
            discard;
        }
        let texel = bgr555_to_rgba(word);
        return vec4<f32>(psx_texture_blend(texel.rgb, in.color.rgb), in.color.a * texel.a);
    }
    return in.color;
}

@fragment
fn fs_opaque(in: VsOut) -> @location(0) vec4<f32> {
    return overlay_color(in);
}

// The STP bit of a textured fragment's VRAM word. A semi-transparent PSX
// prim blends only the texels whose STP bit is set; an STP-clear texel of the
// same prim draws opaque. An untextured semi prim blends every pixel.
fn texel_stp(in: VsOut) -> bool {
    return (fetch_vram_word(in.uv, in.cba_tsb.x, in.cba_tsb.y) & 0x8000u) != 0u;
}

// The opaque half of a semi-transparent run (drawn through the opaque
// pipeline, before the blend half): the STP-clear texels of its textured
// prims. Untextured semi prims and STP-set texels belong to the blend half.
@fragment
fn fs_semi_opaque(in: VsOut) -> @location(0) vec4<f32> {
    if (in.flags & 1u) == 0u || texel_stp(in) {
        discard;
    }
    return overlay_color(in);
}

// Blend entries: emit the foreground term; the per-ABR fixed-function blend
// state combines it with the framebuffer B (`psx_blend`). Off PSX mode the
// term is F times the mode's factor (`psx_blend_scale`). In PSX mode
// (`flags` bit 8, set by the renderer's staging) it is the foreground term
// of the GPU's 5-bit result; bit 9 takes the PCSX-Redux software GPU's
// rule and bit 10 that emulator's dither, which it applies to a Gouraud
// quad's 8-bit sum (`getTextureTransColShadeXDither`,
// `getShadeTransColDither`) and not to a flat one, whose mode 0 / 3 halve or
// quarter the texel before the modulate (`getTextureTransColShade`). A
// textured prim's STP-clear texels were drawn by `fs_semi_opaque`.
fn blend_out(in: VsOut, mode: u32) -> vec4<f32> {
    let textured = (in.flags & 1u) != 0u;
    if textured && !texel_stp(in) {
        discard;
    }
    let c = overlay_color(in);
    if (in.flags & 0x100u) == 0u {
        return vec4<f32>(c.rgb * psx_blend_scale(mode), 1.0);
    }
    if (in.flags & 0x200u) == 0u {
        return vec4<f32>(psx_blend_term_gpu(psx_bin5(c.rgb), mode), 1.0);
    }
    let dithered = (in.flags & 0x400u) != 0u;
    if textured {
        let word = fetch_vram_word(in.uv, in.cba_tsb.x, in.cba_tsb.y);
        let t5 = round(bgr555_to_rgba(word).rgb * 31.0);
        let c8 = round(clamp(in.color.rgb, vec3<f32>(0.0), vec3<f32>(255.0 / 128.0)) * 128.0);
        if dithered && (in.flags & 4u) != 0u {
            return vec4<f32>(psx_blend_term_pcsx_dithered(
                floor(t5 * c8 / 16.0), psx_pcsx_dither_offset(in.clip_pos.xy), mode), 1.0);
        }
        var ts = t5;
        if mode == 0u {
            ts = floor(t5 / 2.0);
        } else if mode == 3u {
            ts = floor(t5 / 4.0);
        }
        let fk = min(floor(ts * c8 / 128.0), vec3<f32>(31.0));
        return vec4<f32>(fk * (8.0 / 255.0), 1.0);
    }
    if dithered && !pcsx_flat_colour(in.color.rgb) {
        return vec4<f32>(psx_blend_term_pcsx_dithered(
            floor(clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * 255.0 + 0.001),
            psx_pcsx_dither_offset(in.clip_pos.xy), mode), 1.0);
    }
    let f5 = psx_bin5(c.rgb);
    var fk = f5;
    if mode == 0u {
        fk = floor(f5 / 2.0);
    } else if mode == 3u {
        fk = floor(f5 / 4.0);
    }
    return vec4<f32>(fk * (8.0 / 255.0), 1.0);
}

// An untextured quad whose colour is one word on every corner.
fn pcsx_flat_colour(rgb: vec3<f32>) -> bool {
    return all(fwidth(rgb) == vec3<f32>(0.0));
}

// The per-ABR entries (`psx_blend::blend_entry`).
@fragment
fn fs_blend_half(in: VsOut) -> @location(0) vec4<f32> {
    return blend_out(in, 0u);
}

@fragment
fn fs_blend(in: VsOut) -> @location(0) vec4<f32> {
    return blend_out(in, 1u);
}

@fragment
fn fs_blend_sub(in: VsOut) -> @location(0) vec4<f32> {
    return blend_out(in, 2u);
}

@fragment
fn fs_blend_quarter(in: VsOut) -> @location(0) vec4<f32> {
    return blend_out(in, 3u);
}
"#;

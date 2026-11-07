//! The VRAM-textured mesh shader: PSX texture fetch from the software VRAM, CLUT decode and the blend passes.

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
    // The draw's model (mesh -> world) matrix as its three affine rows,
    // staged by the renderer as `view_proj^-1 * mvp` when a scene camera
    // is registered (identity otherwise). Row form keeps the struct at one
    // 256-byte dynamic-offset slot. Only the per-scene point-light layer
    // reads it - it shades and shadow-tests in world space.
    model_r0: vec4<f32>,
    model_r1: vec4<f32>,
    model_r2: vec4<f32>,
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
    // World-space position (`u.model * position`) for the per-scene
    // point-light layer.
    @location(5) world_w: vec3<f32>,
    // The vertex's corner of its primitive, one-hot over the record's four
    // corners (`prim_corner_weight`): interpolated, the fragment's
    // barycentric weights, which keep PSX mode's texel row inside the
    // primitive (`psx_sample_uv`). All ones on a vertex with no single owner.
    @location(6) @interpolate(linear) prim_bary: vec4<f32>,
    // A quad's four corners for PCSX-Redux's quad walk (`quad_walk_uv`, an
    // instrument): snapped stage X, stage Y, then each corner's U and V.
    // `quad_u.x < 0` when the fragment's primitive is no such quad.
    @location(7) @interpolate(flat) quad_x: vec4<f32>,
    @location(8) @interpolate(flat) quad_y: vec4<f32>,
    @location(9) @interpolate(flat) quad_u: vec4<f32>,
    @location(10) @interpolate(flat) quad_v: vec4<f32>,
};

// `z = a * w + b` of `m`'s depth row over its `w` row, read the way
// `legaia_engine_field::overworld_draw_order::depth_affine` reads it: the
// column with the largest `w` weight gives `a`, the translation column `b`.
fn depth_affine(m: mat4x4<f32>) -> vec2<f32> {
    var col = m[2];
    if (abs(m[0].w) > abs(col.w) && abs(m[0].w) >= abs(m[1].w)) {
        col = m[0];
    } else if (abs(m[1].w) > abs(col.w)) {
        col = m[1];
    }
    var a = 0.0;
    if (abs(col.w) > 1.0e-7) {
        a = col.z / col.w;
    }
    return vec2<f32>(a, m[3].z - a * m[3].w);
}

// The overworld continent's flat bucket depth
// (`legaia_engine_field::overworld_draw_order`): retail links each ground
// cell (`FUN_801F89B8`) at `(max corner SZ >> 5) + 14` of the one ordering
// table the fog sheets link into, so the whole cell draws at that bucket's
// depth. `fa` / `fb` are the cell's `[x0, z0, x1, z1]` / `[y00, y10, y01,
// y11]` (zero on every other mesh, which keeps its per-pixel depth); the
// bucket's representative `SZ` is `32 k + 32`, taken back to this matrix's
// `w` through the frame's `clip.w`-to-`SZ` factor and to normalised depth
// through its depth row. Off the overworld (`sz_scale == 0`) nothing moves.
fn overworld_flat_depth(clip: vec4<f32>, fa: vec4<f32>, fb: vec4<f32>, sz_scale: f32) -> vec4<f32> {
    if (sz_scale <= 0.0 || fa.z <= fa.x || clip.w <= 0.0) {
        return clip;
    }
    let w0 = (u.mvp * vec4<f32>(fa.x, fb.x, fa.y, 1.0)).w;
    let w1 = (u.mvp * vec4<f32>(fa.z, fb.y, fa.y, 1.0)).w;
    let w2 = (u.mvp * vec4<f32>(fa.x, fb.z, fa.w, 1.0)).w;
    let w3 = (u.mvp * vec4<f32>(fa.z, fb.w, fa.w, 1.0)).w;
    let sz = clamp(i32(round(max(max(w0, w1), max(w2, w3)) * sz_scale)), 0, 0xFFFF);
    let bucket = (sz >> 5u) + 14;
    let rep_w = (f32(bucket) * 32.0 + 32.0) / sz_scale;
    let ab = depth_affine(u.mvp);
    let ndc = ab.x + ab.y / rep_w;
    return vec4<f32>(clip.x, clip.y, ndc * clip.w, clip.w);
}

// The field ground pass's far bucket (`legaia_engine_scene::field_ground::
// flat_refs`): retail links a field ground cell without the object-grid sort
// bit `0x8000` into the ordering table's fixed far bucket, so every other
// primitive paints over it. A sloped such cell is marked by a swapped x pair
// (`fa.x > fa.z`); its depth is pushed into the thin slice behind every other
// draw - reversed-Z, so scaling `z` toward 0 moves it toward the far plane
// while keeping the cell's own per-pixel order inside the slice. Off the
// field (`sz_scale > 0`, the overworld's key) and on every unmarked vertex
// nothing moves.
// A flat far-bucket cell (`fa.y > fa.w`, its z pair swapped) goes into the
// same slice: a decal authored under the floor plane (`conc`'s shadow strips,
// six units down) paints over it, as it does over retail's far bucket.
// Off retail's vantage (`ground_drop_param() > 0`) every far-bucket cell,
// flat or sloped, takes the dropped floor instead
// (`field_dropped_floor_depth`): a sloped one left in the slice behind flat
// cells that have left it would let them paint over it from behind.
const FIELD_FAR_BUCKET_DEPTH_SCALE: f32 = 0.001;
fn field_far_bucket_depth(clip: vec4<f32>, fa: vec4<f32>, fb: vec4<f32>, sz_scale: f32, py: f32) -> vec4<f32> {
    if (sz_scale > 0.0 || clip.w <= 0.0) {
        return clip;
    }
    if (fa.x > fa.z || fa.y > fa.w) {
        if (ground_drop_param() > 0.0) {
            return field_dropped_floor_depth(clip, fb, py);
        }
        return vec4<f32>(clip.x, clip.y, clip.z * FIELD_FAR_BUCKET_DEPTH_SCALE, clip.w);
    }
    return clip;
}

// The eye of a projection `m`, in its object space: the point every one of
// the clip `x`, `y` and `w` rows maps to zero. `(0, 0, 0)` for a projection
// with no eye (an orthographic one).
fn eye_of_matrix(m: mat4x4<f32>) -> vec3<f32> {
    let r0 = vec4<f32>(m[0].x, m[1].x, m[2].x, m[3].x);
    let r1 = vec4<f32>(m[0].y, m[1].y, m[2].y, m[3].y);
    let r3 = vec4<f32>(m[0].w, m[1].w, m[2].w, m[3].w);
    let c13 = cross(r1.xyz, r3.xyz);
    let det = dot(r0.xyz, c13);
    if (abs(det) < 1.0e-20) {
        return vec3<f32>(0.0);
    }
    return -(r0.w * c13 + r1.w * cross(r3.xyz, r0.xyz) + r3.w * cross(r0.xyz, r1.xyz)) / det;
}

// The field ground off retail's vantage (`ground_drop_param()`, staged by
// `Renderer::set_field_ground_drop`): a bucketed cell is depth-tested as the
// level plane `drop` units beyond its corner farthest from the eye, along
// each pixel's own ray (`legaia_engine_scene::field_ground::dropped_floor_w` /
// `dropped_floor_ref`) - a decal on or just under the floor still paints over
// it, a mesh buried deeper stays hidden. `fb` holds the corners' heights, `py`
// the vertex's own. The ray from the eye through the vertex meets the plane
// `(h + drop) / d` times as deep, `h` the eye's distance from the reference
// corner and `d` the vertex's from the eye's height (`d = h` on a flat cell);
// a plane is affine in `1 / w` on screen, so the corners' depths
// interpolate it exactly.
fn field_dropped_floor_depth(clip: vec4<f32>, fb: vec4<f32>, py: f32) -> vec4<f32> {
    let drop = ground_drop_param();
    if (drop <= 0.0 || clip.w <= 0.0) {
        return clip;
    }
    let e = eye_of_matrix(u.mvp).y;
    let lo = min(min(fb.x, fb.y), min(fb.z, fb.w));
    let hi = max(max(fb.x, fb.y), max(fb.z, fb.w));
    var floor_y = lo;
    if (abs(hi - e) > abs(lo - e)) {
        floor_y = hi;
    }
    let toward = sign(floor_y - e);
    let w = clip.w * (abs(floor_y - e) + drop) / max(toward * (py - e), 1.0);
    let ab = depth_affine(u.mvp);
    return vec4<f32>(clip.x, clip.y, (ab.x + ab.y / w) * clip.w, clip.w);
}

// The field ground pass's sorted cells (`legaia_engine_scene::field_ground::
// flat_refs`, object-grid bit `0x8000` set): `FUN_801F6D48` links such a cell
// at `(max corner SZ >> 5) + 2` (`0x801F6FAC..0x801F6FE8`), two buckets
// behind its farthest corner, so the whole cell draws at that bucket's depth
// and every primitive whose own bucket is nearer paints over it - a rock or
// a post sunk below a raised floor tier shows its buried faces, as retail's
// ordering table draws them. `fa` / `fb` are the cell's `[x0, z0, x1, z1]` /
// `[y00, y10, y01, y11]` (an unmarked cell has `x0 < x1` and `z0 < z1`; every
// other mesh carries zeros and keeps its per-pixel depth). The field frame's
// `SZ` is `clip.w` times the base matrix's scale
// (`legaia_engine_field::camera::CUTSCENE_WORLD_SCALE`). Off the field
// (`sz_scale > 0`, the overworld's key) nothing moves. Off retail's vantage
// a flat cell's bucket gives way to the dropped floor
// (`field_dropped_floor_depth`), a sloped one's to its own per-pixel depth.
const FIELD_SZ_PER_W: f32 = 6.0;
fn field_sorted_cell_depth(clip: vec4<f32>, fa: vec4<f32>, fb: vec4<f32>, sz_scale: f32, py: f32) -> vec4<f32> {
    if (sz_scale > 0.0 || clip.w <= 0.0 || fa.z <= fa.x || fa.w <= fa.y) {
        return clip;
    }
    if (ground_drop_param() > 0.0) {
        if (fb.x != fb.y || fb.x != fb.z || fb.x != fb.w) {
            return clip;
        }
        return field_dropped_floor_depth(clip, fb, py);
    }
    let w0 = (u.mvp * vec4<f32>(fa.x, fb.x, fa.y, 1.0)).w;
    let w1 = (u.mvp * vec4<f32>(fa.z, fb.y, fa.y, 1.0)).w;
    let w2 = (u.mvp * vec4<f32>(fa.x, fb.z, fa.w, 1.0)).w;
    let w3 = (u.mvp * vec4<f32>(fa.z, fb.w, fa.w, 1.0)).w;
    let sz = clamp(i32(round(max(max(w0, w1), max(w2, w3)) * FIELD_SZ_PER_W)), 0, 0xFFFF);
    let bucket = (sz >> 5u) + 2;
    let rep_w = f32(bucket * 32) / FIELD_SZ_PER_W;
    let ab = depth_affine(u.mvp);
    let ndc = ab.x + ab.y / rep_w;
    return vec4<f32>(clip.x, clip.y, ndc * clip.w, clip.w);
}

// The overworld continent's depth cue
// (`legaia_engine_field::overworld_ground_cue`): retail's ground emitter
// (`FUN_801F89B8`) runs each cell's packet colour through `DPCS` with
// `IR0 = max(SZ1 - 0x5000, 0) >> 3`, `SZ1` the depth of the cell's corner
// `(x1, z0)`, toward the far colour `0x1000` its caller sets with the literal
// `SetFarColor(0x100, 0x100, 0x100)` - one value per cell, since every
// vertex carries the same corners. Same gate as `overworld_flat_depth`.
fn overworld_ground_cue(color: vec3<f32>, fa: vec4<f32>, fb: vec4<f32>, sz_scale: f32) -> vec3<f32> {
    if (sz_scale <= 0.0 || fa.z <= fa.x) {
        return color;
    }
    let w1 = (u.mvp * vec4<f32>(fa.z, fb.y, fa.y, 1.0)).w;
    let sz1 = clamp(i32(round(w1 * sz_scale)), 0, 0xFFFF);
    let ir0 = max(sz1 - 0x5000, 0) >> 3u;
    let base = vec3<i32>(color) << vec3<u32>(16u);
    let far = vec3<i32>(0x1000 << 12u);
    let ir = clamp((far - base) >> vec3<u32>(12u), vec3<i32>(-0x8000), vec3<i32>(0x7FFF));
    let mac = (base + ir * ir0) >> vec3<u32>(12u);
    return vec3<f32>(clamp(mac >> vec3<u32>(4u), vec3<i32>(0), vec3<i32>(255)));
}

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) uv_in: vec4<u32>,
    @location(2) cba_tsb_in: vec2<u32>,
    @location(3) normal_in: vec3<f32>,
    @location(4) color_in: vec4<u32>,
    @location(5) flat_a: vec4<f32>,
    @location(6) flat_b: vec4<f32>,
    @location(7) prim_c0: vec4<f32>,
    @location(8) prim_c1: vec3<f32>,
    @location(9) prim_c2: vec3<f32>,
    @location(10) prim_c3: vec3<f32>,
) -> VsOut {
    var out: VsOut;
    // The overworld's per-vertex screen-Y bend (see OVERWORLD_CURVE_WGSL),
    // ahead of the pixel snap - retail bends SY before the packet is written.
    var clip = overworld_curve_clip(u.mvp * vec4<f32>(position, 1.0), u.flags.w);
    clip = overworld_flat_depth(clip, flat_a, flat_b, u.flags.w);
    clip = field_far_bucket_depth(clip, flat_a, flat_b, u.flags.w, position.y);
    clip = field_sorted_cell_depth(clip, flat_a, flat_b, u.flags.w, position.y);
    // A ground cell carries its own bucket (the two above); only a mesh
    // primitive links at its corners.
    if (all(flat_a == vec4<f32>(0.0))) {
        clip = prim_mean_depth(clip, prim_c0, prim_c1, prim_c2, prim_c3);
    }
    // PSX mode: the GTE's register limits (a corner past `IR = 0x7FFF` or
    // `SZ = 0xFFFF` lands where retail's packet put it), then the snap.
    if u.psx_params.z >= 0.5 {
        clip = gte_saturate_prim(clip, prim_c0, prim_c1, prim_c2, prim_c3);
        clip = psx_snap_clip(clip, PSX_STAGE_W, PSX_STAGE_H);
    }
    if u.psx_params.z >= 1.5 {
        clip = psx_native_centre(clip, PSX_STAGE_W, PSX_STAGE_H);
    }
    if prim_near_rejected(u.mvp, prim_c0, prim_c1, prim_c2, prim_c3) {
        clip = PRIM_REJECTED_CLIP;
    }
    out.clip_pos = clip;
    out.world_pos = position;
    out.uv_affine = vec2<f32>(f32(uv_in.x), f32(uv_in.y));
    out.cba_tsb = cba_tsb_in;
    out.normal = normal_in;
    out.prim_color = overworld_ground_cue(
        vec3<f32>(f32(color_in.x), f32(color_in.y), f32(color_in.z)),
        flat_a,
        flat_b,
        u.flags.w,
    );
    out.prim_bary = prim_corner_weight(position, prim_c0, prim_c1, prim_c2, prim_c3);
    out.quad_u = vec4<f32>(-1.0);
    if u.psx_params.z > 2.5 {
        // The normal lane carries the quad's UVs under this rule
        // (`Renderer::upload_vram_mesh_with_flat_refs`).
        out.normal = vec3<f32>(0.0, 1.0, 0.0);
        if prim_c0.w > 3.5 && normal_in.x >= 0.0 {
            quad_walk_stage(&out, prim_c0.xyz, prim_c1, prim_c2, prim_c3, normal_in);
        }
    }
    let p4 = vec4<f32>(position, 1.0);
    out.world_w = vec3<f32>(dot(u.model_r0, p4), dot(u.model_r1, p4), dot(u.model_r2, p4));
    return out;
}

// One corner of a quad on the snapped stage, as `vs_main` places a vertex
// ahead of `psx_native_centre`; `z < 0` for a corner with no screen position.
fn quad_corner_stage(c: vec3<f32>) -> vec3<f32> {
    let clip = gte_saturate_clip(overworld_curve_clip(u.mvp * vec4<f32>(c, 1.0), u.flags.w));
    if clip.w <= 0.0 {
        return vec3<f32>(0.0, 0.0, -1.0);
    }
    let s = psx_snap_clip(clip, PSX_STAGE_W, PSX_STAGE_H);
    return vec3<f32>(
        round((s.x / s.w * 0.5 + 0.5) * PSX_STAGE_W),
        round((0.5 - s.y / s.w * 0.5) * PSX_STAGE_H),
        1.0,
    );
}

// Hands the fragment stage a quad's corners. The UVs arrive as three whole
// numbers of 24 bits each, which a float holds exactly: `u0 v0 u1`, `v1 u2 v2`,
// `u3 v3`.
fn quad_walk_stage(
    out: ptr<function, VsOut>,
    c0: vec3<f32>,
    c1: vec3<f32>,
    c2: vec3<f32>,
    c3: vec3<f32>,
    packed: vec3<f32>,
) {
    let a = quad_corner_stage(c0);
    let b = quad_corner_stage(c1);
    let c = quad_corner_stage(c2);
    let d = quad_corner_stage(c3);
    if min(min(a.z, b.z), min(c.z, d.z)) < 0.0 {
        return;
    }
    let n = vec3<u32>(packed);
    (*out).quad_x = vec4<f32>(a.x, b.x, c.x, d.x);
    (*out).quad_y = vec4<f32>(a.y, b.y, c.y, d.y);
    (*out).quad_u = vec4<f32>(
        f32(n.x >> 16u),
        f32(n.x & 0xFFu),
        f32((n.y >> 8u) & 0xFFu),
        f32(n.z >> 8u),
    );
    (*out).quad_v = vec4<f32>(
        f32((n.x >> 8u) & 0xFFu),
        f32(n.y >> 16u),
        f32(n.y & 0xFFu),
        f32(n.z & 0xFFu),
    );
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
    // `tex_window.x` packs (mask_x, mask_y, off_x, off_y) one byte each;
    // the other three lanes are the object-effect clip (`effect_clipped`).
    let tw = u.tex_window.x;
    let tw_mx = tw & 0xFFu;
    let tw_my = (tw >> 8u) & 0xFFu;
    let mask_x = tw_mx * 8u;
    let mask_y = tw_my * 8u;
    let off_x = (((tw >> 16u) & 0xFFu) & tw_mx) * 8u;
    let off_y = (((tw >> 24u) & 0xFFu) & tw_my) * 8u;
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

// Which corner of its primitive record a vertex is, one-hot: the mesh
// builders give every vertex of a primitive the primitive's own corners
// (`legaia_render_kernels::prim_near_reject::prim_corner_refs`), so the vertex
// finds itself among them. A vertex two primitives share has no record
// (`c0.w == 0`) and weighs all ones, which no texel-row clamp ever moves.
fn prim_corner_weight(p: vec3<f32>, c0: vec4<f32>, c1: vec3<f32>, c2: vec3<f32>, c3: vec3<f32>) -> vec4<f32> {
    if c0.w < 2.5 {
        return vec4<f32>(1.0);
    }
    if all(p == c0.xyz) {
        return vec4<f32>(1.0, 0.0, 0.0, 0.0);
    }
    if all(p == c1) {
        return vec4<f32>(0.0, 1.0, 0.0, 0.0);
    }
    if all(p == c2) {
        return vec4<f32>(0.0, 0.0, 1.0, 0.0);
    }
    if c0.w > 3.5 && all(p == c3) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    return vec4<f32>(1.0);
}

// How far along a texel-row move the sample may go and stay on its own
// triangle: the fraction `t` of the move at which the first barycentric
// weight reaches zero (1 when none does). `bary` is the fragment's weights,
// `step` their change over the whole move.
fn psx_row_reach(bary: vec4<f32>, step: vec4<f32>) -> f32 {
    var t = 1.0;
    for (var i = 0; i < 4; i = i + 1) {
        let q = bary[i] + step[i];
        if q < 0.0 {
            t = min(t, max(bary[i], 0.0) / (bary[i] - q));
        }
    }
    return t;
}

// How far short of its triangle's edge the texel row stops, in stage rows.
// Retail's row point on a top edge reads the edge's own `V` exactly; the
// host's float interpolation lands a hair either side of it, and truncating
// the outside value takes the texel row above - on a leaf card packed under
// another, a line of the neighbour's texels.
const PSX_ROW_EDGE_MARGIN: f32 = 1.0 / 64.0;

// PSX mode's texel sample row. The GPU walks a polygon one scanline at a
// time and takes each pixel's texel at the scanline's own row: the interpolated
// `V` of stage row `y` is the value at `y`, not at the row's centre `y + 0.5`
// that the host rasteriser samples (along a scanline the two agree - the span
// texel lands on the pixel centre). So in PSX mode the sample moves up half a
// stage row, `uv_dy` being the UV's per-framebuffer-row step and
// `psx_params.y / PSX_STAGE_H` the framebuffer rows per stage row. Identity
// off PSX mode.
//
// The move stops at the fragment's own triangle, `PSX_ROW_EDGE_MARGIN` inside
// it (`bary_dy` the barycentric weights' row step, `psx_row_reach`). Retail's
// row point is a point the primitive covers, so its texel never comes from
// outside the primitive's UVs. The host also covers pixels whose row point
// lies outside - the top sub-rows of every primitive, and the whole of a
// sliver thinner than a stage row, which a snapped edge-on leaf card becomes;
// on a sliver the UV's row step is huge, so half a row of it lands far
// outside the primitive's texels and draws them as a streak.
fn psx_sample_uv(uv: vec2<f32>, uv_dy: vec2<f32>, bary: vec4<f32>, bary_dy: vec4<f32>) -> vec2<f32> {
    if u.psx_params.z < 0.5 {
        return uv;
    }
    let per_row = u.psx_params.y / PSX_STAGE_H;
    let rows = 0.5 * per_row;
    let margin = PSX_ROW_EDGE_MARGIN * per_row;
    let probe = rows + margin;
    let reach = psx_row_reach(bary, -bary_dy * probe) * probe;
    return uv - uv_dy * clamp(reach - margin, 0.0, rows);
}

// PSX mode on the native target (`psx_params.z >= 1.5`,
// `Renderer::set_native_resolution`): the frame is the 320 x 240 display and
// the snapped geometry sits half a pixel down and right
// (`psx_native_centre`), so the host's pixel centre is the GPU's integer
// sample point and the interpolated UV is the GPU's at that point. The GPU
// then takes the texel with a half-texel bias on both axes - its UV
// interpolators start from the vertex value plus one half, so the texel is
// the UV rounded rather than truncated. Every other mode takes
// `psx_sample_uv`.
fn psx_texel_uv(
    uv: vec2<f32>,
    uv_dx: vec2<f32>,
    uv_dy: vec2<f32>,
    bary: vec4<f32>,
    bary_dx: vec4<f32>,
    bary_dy: vec4<f32>,
) -> vec2<f32> {
    if u.psx_params.z < 1.5 {
        return psx_sample_uv(uv, uv_dy, bary, bary_dy);
    }
    if u.psx_params.z < 2.5 {
        return uv + vec2<f32>(0.5, 0.5);
    }
    // PCSX-Redux's software GPU (`PsxTexelRule::PcsxRedux`, an instrument
    // for scoring against its captures): no texel bias - the texel at the
    // pixel centre along the row and at the row itself, the move along the
    // row stopping `PSX_ROW_EDGE_MARGIN` inside the fragment's triangle.
    let cols = 0.5;
    let margin = PSX_ROW_EDGE_MARGIN;
    let probe = cols + margin;
    let reach = psx_row_reach(bary, bary_dx * probe) * probe;
    let rows = psx_row_inset(bary, bary_dy, margin);
    return uv + uv_dx * clamp(reach - margin, 0.0, cols) + uv_dy * rows;
}

// How far down (in rows) a fragment on its triangle's top edge moves to sit
// `margin` rows inside it. The row point of a fragment on a top edge is the
// edge itself, whose `V` is the edge's own corner value; the host's float
// interpolation lands a hair either side of it, and truncating the outside
// value takes the texel row above the primitive's UVs. Zero for every
// fragment already `margin` inside, and for a vertex with no corner record
// (`bary_dy` all zero).
fn psx_row_inset(bary: vec4<f32>, bary_dy: vec4<f32>, margin: f32) -> f32 {
    var d = 0.0;
    for (var i = 0; i < 4; i = i + 1) {
        if bary_dy[i] > 0.0 {
            d = max(d, margin - bary[i] / bary_dy[i]);
        }
    }
    return min(d, 2.0 * margin);
}

// Which side of the chord `a -> b` corner `c` falls on, as
// `SoftRenderer::setupSectionsFlatTextured4` tests it: negative means `c` is
// to the chord's right.
fn quad_side(a: vec3<f32>, b: vec3<f32>, c: vec3<f32>) -> f32 {
    var h = b.y - a.y;
    if h == 0.0 {
        h = 1.0;
    }
    return (c.y - a.y) / h * (b.x - a.x) + (a.x - c.x);
}

// Where the chain of up to four corners crosses row `y`: `x`, `u`, `v`.
fn quad_chain_at(chain: array<vec3<f32>, 4>, uvs: array<vec2<f32>, 4>, n: i32, y: f32) -> vec3<f32> {
    for (var i = 0; i < n - 1; i = i + 1) {
        let a = chain[i];
        let b = chain[i + 1];
        if y < b.y && b.y > a.y {
            let t = (y - a.y) / (b.y - a.y);
            return vec3<f32>(mix(a.x, b.x, t), mix(uvs[i], uvs[i + 1], t));
        }
    }
    return vec3<f32>(chain[n - 1].x, uvs[n - 1]);
}

// PCSX-Redux's software GPU (`PsxTexelRule::PcsxRedux`) draws a textured quad
// whole where the console draws two triangles: `drawPoly4TEx*` walks the
// outline's left and right chains (`setupSectionsFlatTextured4`) and steps U and
// V linearly along each row between them, so a quad whose corners do not lie on
// one UV plane maps its texture differently. An instrument, so the score
// measures the engine and not that emulator's rasteriser.
fn quad_walk_uv(p: vec2<f32>, qx: vec4<f32>, qy: vec4<f32>, qu: vec4<f32>, qv: vec4<f32>) -> vec2<f32> {
    // The call's corner order (`v0, v1, v3, v2`) and its exchange sort by row.
    var v = array<vec3<f32>, 4>(
        vec3<f32>(qx.x, qy.x, 0.0),
        vec3<f32>(qx.y, qy.y, 1.0),
        vec3<f32>(qx.w, qy.w, 3.0),
        vec3<f32>(qx.z, qy.z, 2.0),
    );
    let order = array<vec2<i32>, 6>(
        vec2<i32>(0, 1), vec2<i32>(0, 2), vec2<i32>(0, 3),
        vec2<i32>(1, 2), vec2<i32>(1, 3), vec2<i32>(2, 3),
    );
    for (var i = 0; i < 6; i = i + 1) {
        let o = order[i];
        if v[o.x].y > v[o.y].y {
            let t = v[o.x];
            v[o.x] = v[o.y];
            v[o.y] = t;
        }
    }
    let v1 = v[0];
    let v2 = v[1];
    let v3 = v[2];
    let v4 = v[3];
    var left = array<vec3<f32>, 4>(v1, v4, v4, v4);
    var right = array<vec3<f32>, 4>(v1, v4, v4, v4);
    var nl = 2;
    var nr = 2;
    if quad_side(v1, v4, v2) < 0.0 {
        if quad_side(v1, v4, v3) < 0.0 {
            if quad_side(v1, v3, v2) >= 0.0 {
                right = array<vec3<f32>, 4>(v1, v3, v4, v4);
                nr = 3;
            } else if quad_side(v2, v4, v3) >= 0.0 {
                right = array<vec3<f32>, 4>(v1, v2, v4, v4);
                nr = 3;
            } else {
                right = array<vec3<f32>, 4>(v1, v2, v3, v4);
                nr = 4;
            }
        } else {
            left = array<vec3<f32>, 4>(v1, v3, v4, v4);
            right = array<vec3<f32>, 4>(v1, v2, v4, v4);
            nl = 3;
            nr = 3;
        }
    } else if quad_side(v1, v4, v3) < 0.0 {
        left = array<vec3<f32>, 4>(v1, v2, v4, v4);
        right = array<vec3<f32>, 4>(v1, v3, v4, v4);
        nl = 3;
        nr = 3;
    } else if quad_side(v1, v3, v2) < 0.0 {
        left = array<vec3<f32>, 4>(v1, v3, v4, v4);
        nl = 3;
    } else if quad_side(v2, v4, v3) < 0.0 {
        left = array<vec3<f32>, 4>(v1, v2, v4, v4);
        nl = 3;
    } else {
        left = array<vec3<f32>, 4>(v1, v2, v3, v4);
        nl = 4;
    }
    var left_uv: array<vec2<f32>, 4>;
    var right_uv: array<vec2<f32>, 4>;
    for (var i = 0; i < 4; i = i + 1) {
        let li = i32(left[i].z);
        let ri = i32(right[i].z);
        left_uv[i] = vec2<f32>(qu[li], qv[li]);
        right_uv[i] = vec2<f32>(qu[ri], qv[ri]);
    }
    let l = quad_chain_at(left, left_uv, nl, p.y);
    let r = quad_chain_at(right, right_uv, nr, p.y);
    let xa = floor(l.x);
    let span = max(floor(r.x) - xa, 1.0);
    let t = clamp((p.x - xa) / span, 0.0, 1.0);
    return mix(l.yz, r.yz, t) + vec2<f32>(QUAD_WALK_TEXEL_EPS);
}

// A row's value lands on a whole texel exactly at a corner; the float walk must
// not fall just under it where the fixed-point walk does not.
const QUAD_WALK_TEXEL_EPS: f32 = 0.001;

// The fragment's sample UV: the quad walk on such a primitive, else the texel
// rule of its triangle.
fn fragment_sample_uv(
    in: VsOut,
    uv_dx: vec2<f32>,
    uv_dy: vec2<f32>,
    bary_dx: vec4<f32>,
    bary_dy: vec4<f32>,
) -> vec2<f32> {
    if u.psx_params.z > 2.5 && in.quad_u.x >= 0.0 {
        return quad_walk_uv(floor(in.clip_pos.xy), in.quad_x, in.quad_y, in.quad_u, in.quad_v);
    }
    return psx_texel_uv(in.uv_affine, uv_dx, uv_dy, in.prim_bary, bary_dx, bary_dy);
}

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    // Taken ahead of any discard, in uniform control flow.
    let uv_dy = dpdy(in.uv_affine);
    let bary_dy = dpdy(in.prim_bary);
    let uv_dx = dpdx(in.uv_affine);
    let bary_dx = dpdx(in.prim_bary);
    // Object-effect clip of a raised `+0x42` (see EFFECT_CLIP_WGSL).
    if effect_clipped(in.world_pos) {
        discard;
    }
    // Retail GTE NCLIP winding rejection (see MeshUniforms.flags).
    if (u.flags.x >= 0.5 && u.flags.x < 1.5 && !front_facing)
        || (u.flags.x >= 1.5 && front_facing) {
        discard;
    }
    // Double-sided prim pairs (CBA bit 15, set by
    // `legaia_tmd::mesh::mark_double_sided_pairs`): draw only the
    // camera-facing copy - the per-prim NCLIP retail applies, which the
    // both-sided pipelines otherwise lose, leaving the two coincident copies
    // to z-fight. The field frame draws retail vertices under one net
    // reflection (camera-side Y flip), so retail's visible faces arrive CW
    // in framebuffer space = NOT front_facing under the pipelines' default
    // Ccw front-face (same parity as the cutscene NCLIP mode 2 above).
    if (in.cba_tsb.x & 0x8000u) != 0u && front_facing {
        discard;
    }
    // Camera-occlusion fade (opt-in see-through-walls enhancement):
    // screen-door discard of fragments between the camera and the player.
    // Identity while disabled - `occl_keep` returns 1.0 and no threshold
    // reaches it (see the prelude's `occl_bayer`).
    if (u.flags.z > 0.5
        && occl_bayer(in.clip_pos.xy) >= occl_keep(in.clip_pos.xy, in.clip_pos.w)) {
        discard;
    }
    let tsb = in.cba_tsb.y;
    let cba = in.cba_tsb.x;
    var word = fetch_vram_word(fragment_sample_uv(in, uv_dx, uv_dy, bary_dx, bary_dy), cba, tsb);
    // Prologue palette-collapse grade: retail rewrites the scene's uploaded
    // CLUTs (and TMD colour words) to the gold law at load; the engine's
    // VRAM carries the disc palettes, so the law applies per decoded texel
    // (exactly equivalent - see `palette_law_word`). Transparency is judged
    // on the raw word below, which the law preserves (0 -> 0, STP kept).
    let palette_on = u.palette.w > 0.5;
    if palette_on && palette_law_reaches(cba, tsb) {
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
    // `flags.y >= 1.5`: the draw's colour word forces ABE on every prim.
    if u.flags.y >= 0.5 && ((tsb & 0x8000u) != 0u || u.flags.y >= 1.5)
        && ((word >> 15u) & 1u) == 1u {
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
    if palette_on && (tsb & 0x0800u) == 0u {
        prim = palette_collapse_prim(prim);
    }
    // Retail order: DPCS blends the PACKET COLOUR toward the far colour, then
    // the GPU multiplies the texel by the result and saturates. A cued draw
    // therefore cues `prim` here, ahead of the modulate - see `psx_cue_colour`
    // for what the other order loses on a bright texture. The grade tint
    // models the amber modulation colour and belongs to the near term only;
    // the far colour is staged in absolute display units and is not re-tinted.
    //
    // Palette mode replaces the pixel multiply with the texel/packet collapse
    // above (`apply_grade` would double-grade), carries the global screen
    // tint in `palette.rgb`, and makes the view-depth cue ramp inert - the
    // opening capture shows every render node holds `IR0 = 0` across the
    // prologue, so the far-field crush is entirely the palette law + the
    // amber packet words, not a depth cue.
    var ir0 = u.depth_cue.a;
    if !palette_on {
        ir0 = cue_ramp_ir0(u.depth_cue.a, u.cue_ramp, in.clip_pos.w);
    }
    let cue_first = !palette_on && ir0 > 0.0;
    if cue_first && u.psx_params.w >= 0.5 {
        prim = psx_cue_colour_gte(apply_grade(prim, u.grade), u.depth_cue.rgb * 255.0, ir0);
    } else if cue_first {
        prim = psx_cue_colour(apply_grade(prim, u.grade), u.depth_cue.rgb * 255.0, ir0);
    }
    // A dithered draw (PSX mode) blends at the GPU's precision, an 8-bit
    // value the dither then cuts; the clean path keeps the continuous blend.
    var lit = psx_modulate(color.rgb, prim);
    if u.psx_params.w >= 0.5 {
        lit = psx_modulate_gpu(color.rgb, prim);
    }
    let geo_n = cross(dpdx(in.world_pos), dpdy(in.world_pos));
    let pg = scene_point_gain(in.world_w, in.normal, geo_n);
    let enhanced = dyn_window(
        dyn_light(
            lit, in.normal, geo_n, in.clip_pos.xy, u.psx_params.xy, u.light_dir, u.light_color, pg,
            (tsb & 0x2000u) != 0u,
        ),
        color.rgb, u.light_dir, (tsb & 0x1000u) != 0u,
    );
    var cued: vec3<f32>;
    if palette_on {
        cued = psx_depth_cue(
            enhanced * u.palette.rgb,
            dyn_far(psx_modulate(color.rgb, u.depth_cue.rgb * 255.0), u.light_dir, u.light_color),
            ir0,
        );
    } else if cue_first {
        cued = enhanced;
    } else {
        cued = apply_grade(enhanced, u.grade);
    }
    // The PCSX-Redux instrument (`dither >= 1.5`) dithers a Gouraud polygon
    // only: that emulator draws a flat-shaded textured one at 5 bits, the
    // modulate floored and nothing added (`getTextureTransColShade`). The
    // packet colour's derivative tells the two apart: zero across a flat
    // polygon's four-pixel quad, and across a Gouraud one whose corners
    // happen to agree, which that emulator still dithers.
    let flat = pcsx_flat_shaded(fwidth(in.prim_color));
    var dither = u.psx_params.w;
    if dither >= 1.5 && flat {
        dither = 0.0;
        cued = psx_floor5(cued);
    }
    let rgb = psx_dither(cued, in.clip_pos.xy, dither);
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
fn blend_pass_color(in: VsOut, front_facing: bool, f_scale: f32) -> vec4<f32> {
    let uv_dy = dpdy(in.uv_affine);
    let bary_dy = dpdy(in.prim_bary);
    let uv_dx = dpdx(in.uv_affine);
    let bary_dx = dpdx(in.prim_bary);
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
    // Double-sided pair copies: blend only the camera-facing one (see
    // fs_main - same rule so a flagged semi prim can't double-blend).
    if (in.cba_tsb.x & 0x8000u) != 0u && front_facing {
        discard;
    }
    // Camera-occlusion fade: same screen-door as the opaque pass, so a
    // semi-transparent wall patch (water panes, glass) opens up too.
    if (u.flags.z > 0.5
        && occl_bayer(in.clip_pos.xy) >= occl_keep(in.clip_pos.xy, in.clip_pos.w)) {
        discard;
    }
    let tsb = in.cba_tsb.y;
    let cba = in.cba_tsb.x;
    var word = fetch_vram_word(fragment_sample_uv(in, uv_dx, uv_dy, bary_dx, bary_dy), cba, tsb);
    // Texel 0x0000 never draws; STP=0 texels were already drawn opaque by
    // the first pass. 0x8000 (black + STP) correctly blends.
    if word == 0u || ((word >> 15u) & 1u) == 0u {
        discard;
    }
    // Same prologue palette-collapse as the opaque pass (STP preserved, so
    // the discard test above is unaffected by ordering).
    let palette_on = u.palette.w > 0.5;
    if palette_on && palette_law_reaches(cba, tsb) {
        word = palette_law_word(word);
    }
    let color = bgr555_to_rgba(word);
    // Semi-transparent prims are texture-blended by the GPU exactly like the
    // opaque ones - the modulation happens before the blend equation, so it
    // applies here too, and the opt-in dynamic light matches the opaque pass
    // (identity when disabled) so lit water/glass composites consistently.
    var prim = in.prim_color;
    if palette_on && (tsb & 0x0800u) == 0u {
        prim = palette_collapse_prim(prim);
    }
    // Same cue-then-modulate order as the opaque pass (see fs_main).
    var ir0 = u.depth_cue.a;
    if !palette_on {
        ir0 = cue_ramp_ir0(u.depth_cue.a, u.cue_ramp, in.clip_pos.w);
    }
    let cue_first = !palette_on && ir0 > 0.0;
    if cue_first && u.psx_params.w >= 0.5 {
        prim = psx_cue_colour_gte(apply_grade(prim, u.grade), u.depth_cue.rgb * 255.0, ir0);
    } else if cue_first {
        prim = psx_cue_colour(apply_grade(prim, u.grade), u.depth_cue.rgb * 255.0, ir0);
    }
    let lit = psx_modulate(color.rgb, prim);
    let geo_n = cross(dpdx(in.world_pos), dpdy(in.world_pos));
    let pg = scene_point_gain(in.world_w, in.normal, geo_n);
    let enhanced = dyn_window(
        dyn_light(
            lit, in.normal, geo_n, in.clip_pos.xy, u.psx_params.xy, u.light_dir, u.light_color, pg,
            (tsb & 0x2000u) != 0u,
        ),
        color.rgb, u.light_dir, (tsb & 0x1000u) != 0u,
    );
    var cued: vec3<f32>;
    if palette_on {
        cued = psx_depth_cue(
            enhanced * u.palette.rgb,
            dyn_far(psx_modulate(color.rgb, u.depth_cue.rgb * 255.0), u.light_dir, u.light_color),
            ir0,
        );
    } else if cue_first {
        cued = enhanced;
    } else {
        cued = apply_grade(enhanced, u.grade);
    }
    // The GPU reads the foreground as a saturated 5-bit channel
    // (`psx_blend_foreground`), and the mode-3 quarter applies to that value
    // (`B + 0.25 * F`), as it does on the browser play page where the 0.25 is
    // a blend factor. Retail's quarter is `F >> 2`; the unfloored quarter
    // keeps the two hosts on one law.
    let flat = pcsx_flat_shaded(fwidth(in.prim_color));
    var fg = psx_blend_foreground(cued);
    if u.psx_params.w >= 1.5 && !flat {
        fg = psx_blend_foreground_pcsx(cued, in.clip_pos.xy);
    }
    return vec4<f32>(fg * f_scale, 1.0);
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

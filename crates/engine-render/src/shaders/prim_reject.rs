//! The effect clip and per-primitive near-reject WGSL chunks.

/// Retail's per-primitive near reject, evaluated in the vertex stage (see
/// `legaia_render_kernels::prim_near_reject`). Every vertex carries its
/// primitive's corners (`c0.w` = corner count, `0` = no single owner), so
/// each corner reaches the same verdict and the whole primitive drops
/// together. `m` is the draw's matrix: its `w` row is the eye depth. The
/// mean is over GTE `SZ` - each corner floored and saturated to
/// `0..=0xFFFF`, a corner behind the eye counting as `0` - weighted by
/// `ZSF3 = 0x555 >> s` / `ZSF4 = 0x400 >> s` exactly as `AVSZ3` / `AVSZ4`.
/// The **object-effect clip** of an actor whose `+0x42` is raised: retail's
/// `FUN_8002735C` clips each primitive against the slab its object-effect
/// row stages (`FUN_8001C204` -> `0x1F800380`, `FUN_80027F00`), which the
/// mesh shaders reproduce as a fragment discard on the mesh-space position.
/// Lanes `tex_window.yzw` hold `(m0, m1)`, `(m2, enable)`, `(lo, hi)` as
/// half floats ([`crate::renderer`]'s `pack_draw_clip`); enable `0` (every
/// draw a host did not stage a clip for) keeps every fragment.
pub(crate) const EFFECT_CLIP_WGSL: &str = r#"
fn effect_clipped(p: vec3<f32>) -> bool {
    let e = unpack2x16float(u.tex_window.z);
    if (e.y < 0.5) {
        return false;
    }
    let m = unpack2x16float(u.tex_window.y);
    let b = unpack2x16float(u.tex_window.w);
    let y = m.x * p.x + m.y * p.y + e.x * p.z;
    return y < b.x || y > b.y;
}
"#;

pub(crate) const PRIM_NEAR_WGSL: &str = r#"
fn prim_sz(m: mat4x4<f32>, c: vec3<f32>, sz_per_w: f32) -> i32 {
    let w = m[0].w * c.x + m[1].w * c.y + m[2].w * c.z + m[3].w;
    return clamp(i32(floor(w * sz_per_w)), 0, 0xFFFF);
}

fn prim_near_rejected(
    m: mat4x4<f32>,
    c0: vec4<f32>,
    c1: vec3<f32>,
    c2: vec3<f32>,
    c3: vec3<f32>,
) -> bool {
    let p = prim_near_params();
    if (p.x < 0.5 || c0.w < 2.5) {
        return false;
    }
    let shift = u32(p.z);
    var sum = prim_sz(m, c0.xyz, p.y) + prim_sz(m, c1, p.y) + prim_sz(m, c2, p.y);
    var zsf = 0x555 >> shift;
    if (c0.w > 3.5) {
        sum = sum + prim_sz(m, c3, p.y);
        zsf = 0x400 >> shift;
    }
    let otz = (zsf * sum) >> 12u;
    if (otz < i32(p.w)) {
        return true;
    }
    // The GPU polygon-size limit (`prim_near_reject::gpu_span_rejected`),
    // armed when the enable lane carries the projection's `H`. A quad is
    // split `[0,1,2]` / `[1,3,2]`; it drops only when both halves would.
    if (p.x < 1.5) {
        return false;
    }
    let s0 = prim_sxy(m, c0.xyz, p.y, p.x);
    let s1 = prim_sxy(m, c1, p.y, p.x);
    let s2 = prim_sxy(m, c2, p.y, p.x);
    if (!span_too_big(s0, s1, s2)) {
        return false;
    }
    if (c0.w > 3.5) {
        return span_too_big(s1, prim_sxy(m, c3, p.y, p.x), s2);
    }
    return true;
}

fn prim_sxy(m: mat4x4<f32>, c: vec3<f32>, sz_per_w: f32, h: f32) -> vec2<f32> {
    let cl = m * vec4<f32>(c, 1.0);
    let d = max(max(cl.w * sz_per_w, 0.0), h * 0.5);
    let half = vec2<f32>(160.0, 120.0);
    let off = cl.xy * half * sz_per_w / d;
    return clamp(off + half, vec2<f32>(-1024.0), vec2<f32>(1023.0)) - half;
}

fn span_too_big(a: vec2<f32>, b: vec2<f32>, c: vec2<f32>) -> bool {
    let lo = min(min(a, b), c);
    let hi = max(max(a, b), c);
    return hi.x - lo.x > 1023.0 || hi.y - lo.y > 511.0;
}

// Where a rejected primitive's corners go: one point outside the clip
// volume, so the triangle has no area and is clipped.
const PRIM_REJECTED_CLIP: vec4<f32> = vec4<f32>(2.0, 2.0, 2.0, 1.0);
"#;

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
// Retail's per-primitive painter order (`flags.w < 0`,
// `Renderer::DRAW_WORD_MEAN_DEPTH` on the battle shell,
// `Renderer::set_field_prim_order` on the field): a primitive links at the
// mean `SZ` of its corners, so its whole area sorts as that mean. `-flags.w - 1`
// is the draw's `DRAW_ORDER_*` word: bit 0 sorts at the farthest corner
// instead (colour word bit `0x10000000`, `FUN_80045194` at
// `0x800452AC..0x800452F8`), bit 1 draws ten ordering-table buckets nearer
// (`actor[+0x10] & 0x80800000`, `FUN_8001ADA4` at `0x8001B204..0x8001B21C`:
// the table base less `0x28`, ten slots of `32` `SZ`, `320 / 6` port units on
// the field's 6x scale). The depth row is `z = a * w + b` in clip space, read
// as `depth_affine` reads it.
fn prim_mean_depth(clip: vec4<f32>, c0: vec4<f32>, c1: vec3<f32>, c2: vec3<f32>, c3: vec3<f32>) -> vec4<f32> {
    if (u.flags.w >= 0.0 || c0.w < 2.5 || clip.w <= 0.0) {
        return clip;
    }
    let m = u.mvp;
    let w0 = (m * vec4<f32>(c0.xyz, 1.0)).w;
    let w1 = (m * vec4<f32>(c1, 1.0)).w;
    let w2 = (m * vec4<f32>(c2, 1.0)).w;
    var w = w0 + w1 + w2;
    var far = max(max(w0, w1), w2);
    var n = 3.0;
    if (c0.w > 3.5) {
        let w3 = (m * vec4<f32>(c3, 1.0)).w;
        w = w + w3;
        far = max(far, w3);
        n = 4.0;
    }
    let order = u32(round(-u.flags.w)) - 1u;
    var mean_w = w / n;
    if ((order & 1u) != 0u) {
        mean_w = far;
    }
    if ((order & 2u) != 0u) {
        mean_w = max(mean_w - 320.0 / 6.0, 1.0);
    }
    if (mean_w <= 0.0) {
        return clip;
    }
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
    let b = m[3].z - a * m[3].w;
    return vec4<f32>(clip.x, clip.y, (a + b / mean_w) * clip.w, clip.w);
}

fn prim_sz(m: mat4x4<f32>, c: vec3<f32>, sz_per_w: f32) -> i32 {
    let w = m[0].w * c.x + m[1].w * c.y + m[2].w * c.z + m[3].w;
    // `SZ_FLOAT_SLACK`: a corner a float's error under an integer floors to it.
    return clamp(i32(floor(w * sz_per_w + 0.015625)), 0, 0xFFFF);
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
    // A PCSX-Redux capture runs its own limit in place of the console's: it
    // drops more (a whole quad on one corner pair) and less (no pair counts
    // unless one of its corners is negative).
    if (u.psx_params.z > 2.5) {
        if (c0.w > 3.5) {
            return pcsx_quad_rejected(s0, s1, s2, prim_sxy(m, c3, p.y, p.x));
        }
        return pcsx_tri_rejected(s0, s1, s2);
    }
    if (!span_too_big(s0, s1, s2)) {
        return false;
    }
    if (c0.w > 3.5) {
        return span_too_big(s1, prim_sxy(m, c3, p.y, p.x), s2);
    }
    return true;
}

// A PCSX-Redux capture's polygon limit (`PsxTexelRule::PcsxRedux`, an
// instrument). Its software GPU measures a corner pair only from a corner
// that is negative, so a primitive wholly on the positive side of the screen
// origin is drawn at any size, where the console skips a triangle past
// `1023 x 511`. Triangles: `SoftRenderer::checkCoord3`.
// A `prim_sxy` offset (Y up) as the packet's screen corner (Y down), the
// frame the emulator's sign tests read.
fn pcsx_screen(a: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(160.0 + a.x, 120.0 - a.y);
}

fn pcsx_tri_rejected(a0: vec2<f32>, a1: vec2<f32>, a2: vec2<f32>) -> bool {
    let p0 = pcsx_screen(a0);
    let p1 = pcsx_screen(a1);
    let p2 = pcsx_screen(a2);
    let lim = vec2<f32>(1024.0, 512.0);
    for (var k = 0; k < 2; k++) {
        let l = lim[k];
        if (p0[k] < 0.0 && (p1[k] - p0[k] > l || p2[k] - p0[k] > l)) {
            return true;
        }
        if (p1[k] < 0.0 && (p0[k] - p1[k] > l || p2[k] - p1[k] > l)) {
            return true;
        }
        if (p2[k] < 0.0 && (p0[k] - p2[k] > l || p1[k] - p2[k] > l)) {
            return true;
        }
    }
    return false;
}

// Quads (`SoftRenderer::checkCoord4`): one corner pair past the limit drops
// the whole quad, where the console skips the one triangle.
fn pcsx_quad_rejected(a0: vec2<f32>, a1: vec2<f32>, a2: vec2<f32>, a3: vec2<f32>) -> bool {
    let p0 = pcsx_screen(a0);
    let p1 = pcsx_screen(a1);
    let p2 = pcsx_screen(a2);
    let p3 = pcsx_screen(a3);
    const X: f32 = 1024.0;
    const Y: f32 = 512.0;
    if (p0.x < 0.0 && (p1.x - p0.x > X || p2.x - p0.x > X) && p3.x < 0.0
        && (p1.x - p3.x > X || p2.x - p3.x > X)) {
        return true;
    }
    if (p1.x < 0.0 && (p0.x - p1.x > X || p2.x - p1.x > X || p3.x - p1.x > X)) {
        return true;
    }
    if (p2.x < 0.0 && (p0.x - p2.x > X || p1.x - p2.x > X || p3.x - p2.x > X)) {
        return true;
    }
    if (p3.x < 0.0 && (p1.x - p3.x > X || p2.x - p3.x > X) && p0.x < 0.0
        && (p1.x - p0.x > X || p2.x - p0.x > X)) {
        return true;
    }
    if (p0.y < 0.0 && (p1.y - p0.y > Y || p2.y - p0.y > Y)) {
        return true;
    }
    if (p1.y < 0.0 && (p0.y - p1.y > Y || p2.y - p1.y > Y || p3.y - p1.y > Y)) {
        return true;
    }
    if (p2.y < 0.0 && (p0.y - p2.y > Y || p1.y - p2.y > Y || p3.y - p2.y > Y)) {
        return true;
    }
    return p3.y < 0.0 && (p1.y - p3.y > Y || p2.y - p3.y > Y);
}

fn prim_sxy(m: mat4x4<f32>, c: vec3<f32>, sz_per_w: f32, h: f32) -> vec2<f32> {
    let cl = m * vec4<f32>(c, 1.0);
    let d = max(max(cl.w * sz_per_w, 0.0), h * 0.5);
    let half = vec2<f32>(160.0, 120.0);
    // `off` is Y up; `SY` saturates on the screen's own Y-down axis.
    let off = cl.xy * half * sz_per_w / d;
    let sx = clamp(half.x + off.x, -1024.0, 1023.0);
    let sy = clamp(half.y - off.y, -1024.0, 1023.0);
    return vec2<f32>(sx - half.x, half.y - sy);
}

fn span_too_big(a: vec2<f32>, b: vec2<f32>, c: vec2<f32>) -> bool {
    let lo = min(min(a, b), c);
    let hi = max(max(a, b), c);
    return hi.x - lo.x > 1023.0 || hi.y - lo.y > 511.0;
}

// The GTE's register limits on one projected corner
// (`prim_near_reject::gte_saturated_clip`): `IR1` / `IR2` saturate to
// `-0x8000..=0x7FFF`, `SZ` to `0..=0xFFFF`, the divide at `H / SZ = 2` and
// `SX` / `SY` to `-0x400..=0x3FF`, in GTE units (`sz_per_w` times the eye
// space; `gte_sat_params()` = `(sz_per_w, H)`, zero off). `w` is kept, a
// corner inside every limit is returned as it came.
//
// A corner on or behind the eye has `SZ = 0`, so the capped divide puts it at
// `OF + 2 * IR`, a real screen point the GPU draws the primitive to - not a
// point a near-plane clip trims the primitive at. Such a corner is returned
// at the depth the cap stands for (`SZ = H / 2`), its depth word the
// primitive's mean (`prim_mean_depth`) or, with none, the nearest.
const GTE_OFY_BIAS: f32 = 0.05;

// `gte_saturate_clip` for a mesh vertex: a corner behind the eye is seated
// on screen only when its primitive reaches in front of the eye. A
// primitive wholly behind it, or a vertex with no single owner (`c0.w`
// below 3: a welded ground mesh, whose own emitter culls such cells), keeps
// the near-plane clip.
fn gte_saturate_prim(clip: vec4<f32>, c0: vec4<f32>, c1: vec3<f32>, c2: vec3<f32>, c3: vec3<f32>) -> vec4<f32> {
    if (clip.w > 0.0) {
        return gte_saturate_clip(clip);
    }
    if (c0.w < 2.5) {
        return clip;
    }
    let m = u.mvp;
    var front = (m * vec4<f32>(c0.xyz, 1.0)).w > 0.0
        || (m * vec4<f32>(c1, 1.0)).w > 0.0
        || (m * vec4<f32>(c2, 1.0)).w > 0.0;
    if (c0.w > 3.5) {
        front = front || (m * vec4<f32>(c3, 1.0)).w > 0.0;
    }
    if (!front) {
        return clip;
    }
    // The seated corner's depth word: the primitive's mean when the draw
    // links at it (`prim_mean_depth` read at `w = 1`), else the nearest.
    // Only here - a corner left to the near-plane clip keeps its own depth,
    // since the clip's crossing point depends on it.
    var c = clip;
    if (clip.w < 0.0) {
        let md = prim_mean_depth(vec4<f32>(clip.xy, 2.0, 1.0), c0, c1, c2, c3);
        var z = 1.0;
        if (md.z >= 0.0 && md.z <= 1.0) {
            z = md.z;
        }
        c.z = z * clip.w;
    }
    return gte_saturate_clip(c);
}

fn gte_saturate_clip(clip: vec4<f32>) -> vec4<f32> {
    let p = gte_sat_params();
    let w = clip.w;
    if (p.x <= 0.0 || p.y <= 0.0) {
        return clip;
    }
    let h = p.y;
    let ir = vec2<f32>(clip.x * 160.0, -(clip.y - GTE_OFY_BIAS * w) * 120.0) * p.x / h;
    let irc = clamp(ir, vec2<f32>(-32768.0), vec2<f32>(32767.0));
    let sz = w * p.x;
    let d = max(clamp(sz, 0.0, 65535.0), h * 0.5);
    let centre = vec2<f32>(160.0, 120.0 - GTE_OFY_BIAS * 120.0);
    let s_raw = centre + h * irc / d;
    let s = clamp(s_raw, vec2<f32>(-1024.0), vec2<f32>(1023.0));
    if (w <= 0.0) {
        let wc = d / p.x;
        var z = 1.0;
        if (w < 0.0 && clip.z / w >= 0.0 && clip.z / w <= 1.0) {
            z = clip.z / w;
        }
        return vec4<f32>((s.x - 160.0) / 160.0 * wc, (120.0 - s.y) / 120.0 * wc, z * wc, wc);
    }
    if (all(irc == ir) && d == sz && all(s == s_raw)) {
        return clip;
    }
    return vec4<f32>((s.x - 160.0) / 160.0 * w, (120.0 - s.y) / 120.0 * w, clip.z, w);
}

// Where a rejected primitive's corners go: one point outside the clip
// volume, so the triangle has no area and is clipped.
const PRIM_REJECTED_CLIP: vec4<f32> = vec4<f32>(2.0, 2.0, 2.0, 1.0);
"#;

#[cfg(test)]
mod tests {
    /// The WGSL's `GTE_OFY_BIAS` literal is the projection's `OFY` term.
    #[test]
    fn the_saturation_centre_is_the_projection_ofy() {
        let lit = format!(
            "const GTE_OFY_BIAS: f32 = {:?};",
            legaia_engine_camera::projection::GTE_OFY_NDC_BIAS
        );
        assert!(super::PRIM_NEAR_WGSL.contains(&lit), "{lit}");
    }
}

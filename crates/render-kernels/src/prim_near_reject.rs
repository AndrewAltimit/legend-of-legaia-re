//! Retail's per-primitive near reject: the `OTZ` cut every TMD prim handler
//! runs before it links a packet.
//!
//! Each handler behind the per-prim dispatcher `FUN_80043390` projects its
//! corners (`RTPT` / `RTPS`), runs `AVSZ3` / `AVSZ4`, reads `OTZ` back and
//! drops the primitive when it is below the scratch halfword `0x1F80037E`
//! (kind 13, `FUN_80043768`: `mfc2 s2,$7` / `sub s1,s2,t4` / `bltz s1` at
//! `0x80043868..0x80043874`; `t4` is `lhu 0x6A(0x1F800314)` loaded per group
//! at `0x8004359C`). The dispatcher stages `ZSF3 = 0x555 >> s` and
//! `ZSF4 = 0x400 >> s` with `s` the ordering-table shift byte `0x1F8003A4`
//! (`0x80043568..0x8004357C`), so `OTZ` is the mean corner `SZ` shifted right
//! by `s`, and the cut drops a primitive whose mean depth is under
//! `0x10 << s`.
//!
//! The scene init (`FUN_8001D424` at `0x8001D4E8`, `FUN_8001DCF8` at
//! `0x8001DD5C`) writes `0x10` to `0x1F80037E`; the shift reads `3` in every
//! catalogued field state and `2` in every battle state. There is no
//! near-plane clip anywhere on this path, so a primitive whose mean depth sits
//! near or behind the eye is simply not drawn - where a port that projects it
//! and clips per pixel paints a huge stretched surface across the frame (a
//! battle body standing next to the camera, a wall the cutscene camera grazes).
//!
//! `SZ` is the GTE's saturated screen depth: a corner behind the eye
//! contributes `0`, not a negative depth, which is why the mean is taken over
//! clamped corners rather than over the primitive's centroid.
//!
//! Both hosts read this through their mesh vertex stage: every vertex carries
//! its primitive's corners ([`prim_corner_refs`]), and the stage computes the
//! same integer `OTZ` from the frame's matrix. [`prim_near_rejected`] is the
//! CPU statement of that test, kept in lockstep with the shaders by tests.
//!
//! REF: FUN_80043390, FUN_80043768

/// The `OTZ` floor the scene init stores in scratch `0x1F80037E`.
pub const NEAR_OTZ: i32 = 0x10;

/// Floats per vertex in a [`prim_corner_refs`] record: `[c0.xyz, n, c1.xyz,
/// c2.xyz, c3.xyz]`.
pub const PRIM_REF_FLOATS: usize = 13;

/// One vertex's primitive-corner record. `n` (lane 3) is the corner count -
/// `3` or `4` - or `0` when the vertex has no single owning primitive, which
/// the shaders read as "never reject".
pub type PrimRef = [f32; PRIM_REF_FLOATS];

/// How far under an integer a port depth may fall and still floor to it
/// ([`screen_z`]). Retail's `SZ` is an integer the GTE computes from integer
/// inputs; the port's is a float composed through the camera's matrices, a
/// few thousandths off. A corner retail puts exactly on the cut - PROT
/// 0922's arm-12 glow ring, locked in view space at `SZ = 64`, whose mean
/// `OTZ` in battle is the floor `0x10` itself - otherwise floors to `63` on
/// some frames and the whole ring drops out. The shaders carry the same
/// constant (`PRIM_NEAR_WGSL`, the play page's `primSz`).
pub const SZ_FLOAT_SLACK: f32 = 1.0 / 64.0;

/// The GTE's `SZ` for a view depth: floored to an integer (within
/// [`SZ_FLOAT_SLACK`]) and saturated to `0..=0xFFFF`.
pub fn screen_z(view_depth: f32) -> i32 {
    if view_depth.is_nan() {
        return 0;
    }
    ((view_depth + SZ_FLOAT_SLACK).floor() as i64).clamp(0, 0xFFFF) as i32
}

/// `AVSZ3` / `AVSZ4` over the corners' `SZ`, with the dispatcher's
/// `ZSF3 = 0x555 >> shift` / `ZSF4 = 0x400 >> shift`.
pub fn otz(sz: &[i32], ot_shift: u32) -> i32 {
    let sum: i64 = sz.iter().map(|&z| i64::from(z.clamp(0, 0xFFFF))).sum();
    let zsf: i64 = if sz.len() == 4 {
        0x400 >> ot_shift
    } else {
        0x555 >> ot_shift
    };
    ((zsf * sum) >> 12) as i32
}

/// Whether retail drops a primitive whose corners sit at `view_depths`.
pub fn prim_near_rejected(view_depths: &[f32], ot_shift: u32) -> bool {
    let sz: Vec<i32> = view_depths.iter().map(|&w| screen_z(w)).collect();
    otz(&sz, ot_shift) < NEAR_OTZ
}

/// The per-frame parameters both hosts' vertex stages read: `[enable,
/// sz_per_w, ot_shift, near_otz]`. `sz_per_w` converts the frame's clip `w`
/// into GTE `SZ` units (`1.0` for the field and battle cameras, whose `w` is
/// the eye-space depth). `None` is off.
///
/// `span_h` arms the GPU's polygon-size limit on top ([`gpu_span_rejected`]):
/// the `enable` lane then carries the projection's `H` (any value above `1.5`
/// reads as armed), which the span test needs for the GTE's divide.
pub fn shader_params(cut: Option<(f32, u32)>, span_h: Option<f32>) -> [f32; 4] {
    match cut {
        Some((sz_per_w, shift)) => [
            span_h.filter(|&h| h > 1.5).unwrap_or(1.0),
            sz_per_w,
            shift as f32,
            NEAR_OTZ as f32,
        ],
        None => [0.0; 4],
    }
}

/// The GPU's polygon-size limit: it skips a polygon whose screen corners lie
/// more than `1023` pixels apart horizontally or `511` vertically
/// ([`crate::prim_near_reject`]'s sibling cut, documented with the dance hall
/// in `minigame-dance.md`). Retail's prim leaves hand it the GTE's `SXY`
/// with no clip of their own, so a primitive with a corner just in front of
/// or behind the eye - a battle body seated between the camera and the
/// caster - projects across the screen and is not drawn, where a port that
/// clips per pixel paints it as long shards over the frame.
pub const GPU_MAX_SPAN: [f32; 2] = [1023.0, 511.0];

/// One corner's GTE screen offset from the projection centre, from its clip
/// coordinates: `RTPS` divides by `SZ` saturated to `0..` with its quotient
/// saturated at `0x1FFFF` (`H / SZ` capped at `2`, so a corner at or behind
/// the eye lands at twice its eye-space offset), and `SX` / `SY` saturate to
/// `-0x400..=0x3FF` around the centre. `half` is the logical screen's half
/// extent (`[160, 120]`); `sz_per_w` and `h` as in [`shader_params`].
pub fn gte_screen_offset(clip: [f32; 4], sz_per_w: f32, h: f32, half: [f32; 2]) -> [f32; 2] {
    let sz = (clip[3] * sz_per_w).max(0.0);
    let d = sz.max(h * 0.5);
    // Clip `y` is up; `SY` saturates on the screen's own Y-down axis.
    let off = [0, 1].map(|i| clip[i] * half[i] * sz_per_w / d);
    let sx = (half[0] + off[0]).clamp(-1024.0, 1023.0);
    let sy = (half[1] - off[1]).clamp(-1024.0, 1023.0);
    [sx - half[0], half[1] - sy]
}

/// The per-frame parameters of the GTE's register limits
/// ([`gte_saturated_clip`]) both hosts' vertex stages read under PSX mode:
/// `[sz_per_w, h, 0, 0]`, all zero for `None` (off).
pub fn gte_saturation_params(gte: Option<(f32, f32)>) -> [f32; 4] {
    match gte {
        Some((sz_per_w, h)) if sz_per_w > 0.0 && h > 0.0 => [sz_per_w, h, 0.0, 0.0],
        _ => [0.0; 4],
    }
}

/// One vertex's clip position as retail's `RTPS` places it, with the GTE's
/// register limits applied: `IR1` / `IR2` saturate to `-0x8000..=0x7FFF`,
/// `SZ` to `0..=0xFFFF`, the divide's quotient at `0x1FFFF` (`H / SZ` capped
/// at `2`) and `SX` / `SY` to `-0x400..=0x3FF`. `clip` is the vertex under a
/// [`legaia_engine_camera::psx_camera::psx_projection`] matrix (clip `x` =
/// `H * Ex / 160`, clip `y` = `-H * Ey / 120 + bias * w`, `w` = `Ez`), and
/// `sz_per_w` converts its eye space into GTE units: the field folds a 6x
/// world scale into its camera rotation (base matrix `_DAT_8007BF10 = 24576
/// * I`), so a field corner's `IR` is six times its eye-space offset.
///
/// A corner inside every limit comes back unchanged. One outside them moves
/// to where the GTE wrote it: `noaru`'s ice-floor bowl, `20 000` world units
/// across round the camera focus, reaches `SZ = 0xFFFF` on its far rim and
/// `IR = 0x7FFF` on its off-screen corners, so retail's packets hold the far
/// rim pushed outward (`6 * w / 0xFFFF`) and the off-screen corners pulled
/// in, which an unclamped projection draws `25` pixels off on average.
///
/// The rasteriser keeps `w`: retail's screen coordinates are affine across a
/// polygon, and the port interpolates its texture affinely in screen space
/// too, so only the corners' screen positions matter.
///
/// A corner at or behind the eye has `SZ = 0`, so the capped divide puts it
/// at `OF + 2 * IR`: a real screen point the GPU draws the primitive to, not
/// a point a near-plane clip trims the primitive at. It comes back at the
/// depth the cap stands for (`w = H / 2 / sz_per_w`, positive), its depth
/// word `clip.z / clip.w` when that lies in `0..=1` (the primitive's mean
/// depth) and the nearest (`1`) otherwise.
///
/// `None` when nothing saturates.
// REF: FUN_80044DC8
pub fn gte_saturated_clip(clip: [f32; 4], sz_per_w: f32, h: f32) -> Option<[f32; 4]> {
    let w = clip[3];
    if sz_per_w <= 0.0 || h <= 0.0 {
        return None;
    }
    let bias = legaia_engine_camera::projection::GTE_OFY_NDC_BIAS;
    // GTE units: `IR1 = S * Ex`, `IR2 = S * Ey` (Y down), `SZ = S * Ez`.
    let ir_x = clip[0] * 160.0 * sz_per_w / h;
    let ir_y = -(clip[1] - bias * w) * 120.0 * sz_per_w / h;
    let sz = w * sz_per_w;
    let ir_xc = ir_x.clamp(-32768.0, 32767.0);
    let ir_yc = ir_y.clamp(-32768.0, 32767.0);
    let szc = sz.clamp(0.0, 65535.0);
    let d = szc.max(h * 0.5);
    let ofy = 120.0 - bias * 120.0;
    let (sx_raw, sy_raw) = (160.0 + h * ir_xc / d, ofy + h * ir_yc / d);
    let sx = sx_raw.clamp(-1024.0, 1023.0);
    let sy = sy_raw.clamp(-1024.0, 1023.0);
    if w <= 0.0 {
        let wc = d / sz_per_w;
        let z = Some(clip[2] / w)
            .filter(|z| w < 0.0 && (0.0..=1.0).contains(z))
            .unwrap_or(1.0);
        return Some([
            (sx - 160.0) / 160.0 * wc,
            (120.0 - sy) / 120.0 * wc,
            z * wc,
            wc,
        ]);
    }
    if ir_xc == ir_x && ir_yc == ir_y && d == sz && sx == sx_raw && sy == sy_raw {
        return None;
    }
    Some([
        (sx - 160.0) / 160.0 * w,
        (120.0 - sy) / 120.0 * w,
        clip[2],
        w,
    ])
}

/// Whether the GPU skips a triangle with these clip-space corners
/// ([`GPU_MAX_SPAN`], [`gte_screen_offset`]).
pub fn gpu_span_rejected(clips: [[f32; 4]; 3], sz_per_w: f32, h: f32) -> bool {
    let s = clips.map(|c| gte_screen_offset(c, sz_per_w, h, [160.0, 120.0]));
    (0..2).any(|a| {
        let lo = s.iter().map(|p| p[a]).fold(f32::MAX, f32::min);
        let hi = s.iter().map(|p| p[a]).fold(f32::MIN, f32::max);
        hi - lo > GPU_MAX_SPAN[a]
    })
}

/// One corner's GTE `SXY` as `RTPS` writes it, from its clip coordinates:
/// `IR1` / `IR2` saturated to `-0x8000..=0x7FFF`, `SZ` to `0..=0xFFFF`, the
/// divide's quotient at `0x1FFFF` (`H / SZ` capped at `2`), `SX` / `SY` the
/// integer part of `OF + IR * H / SZ` saturated to `-0x400..=0x3FF`.
/// `aspect_fix` undoes the X correction a host's projection carries for a
/// viewport that is not 4:3 ([`legaia_engine_camera::psx_camera::psx_projection`]),
/// so `SX` is in retail's 320-wide frame whatever the window.
pub fn gte_sxy(clip: [f32; 4], sz_per_w: f32, h: f32, aspect_fix: f32) -> [i32; 2] {
    let bias = legaia_engine_camera::projection::GTE_OFY_NDC_BIAS;
    let w = clip[3];
    let ir_x = (clip[0] / aspect_fix.max(1e-6) * 160.0 * sz_per_w / h).clamp(-32768.0, 32767.0);
    let ir_y = (-(clip[1] - bias * w) * 120.0 * sz_per_w / h).clamp(-32768.0, 32767.0);
    let d = (w * sz_per_w).clamp(0.0, 65535.0).max(h * 0.5);
    let ofy = 120.0 - bias * 120.0;
    let s = |of: f32, ir: f32| ((of + h * ir / d).floor() as i32).clamp(-0x400, 0x3FF);
    [s(160.0, ir_x), s(ofy, ir_y)]
}

/// `NCLIP` over three `SXY`: `MAC0 = SX0*SY1 + SX1*SY2 + SX2*SY0 - SX0*SY2
/// - SX1*SY0 - SX2*SY1`, negative for a face that turns its back on the eye.
pub fn nclip(a: [i32; 2], b: [i32; 2], c: [i32; 2]) -> i64 {
    let [ax, ay] = a.map(i64::from);
    let [bx, by] = b.map(i64::from);
    let [cx, cy] = c.map(i64::from);
    ax * by + bx * cy + cx * ay - ax * cy - bx * ay - cx * by
}

/// Whether all four corners of a quad lie off one edge of the 320x240
/// frame: every `SY < 0`, every `SY >= 240`, every `SX <= 0` or every
/// `SX >= 320` (the left edge counts a corner on `0` as off, the top edge
/// does not).
///
/// PORT: FUN_80044798
pub fn quad_off_screen(sxy: [[i32; 2]; 4]) -> bool {
    sxy.iter().all(|p| p[1] < 0)
        || sxy.iter().all(|p| p[1] >= 240)
        || sxy.iter().all(|p| p[0] <= 0)
        || sxy.iter().all(|p| p[0] >= 320)
}

/// Whether handler bank 3's kind-19 leaf drops a whole `GT4` before it
/// splits it: `RTPT` over corners `0..=2` and `NCLIP`, the result masked by
/// the dispatcher's word (`-0x2D8`: `0xFFFFFFFF`, or `0x7FFFFFFF` for a colour
/// word carrying `0x08000000`, `0x80043520..0x80043540`) and the quad dropped
/// when it is negative (`0x80045C30..0x80045C54`); then `RTPS` over corner
/// `3` and [`quad_off_screen`] over the four (`0x80045C60..0x80045C70`).
/// `cull_back` is the unlowered mask. The sixteen triangles a kept quad
/// splits into take no winding test of their own, only the near reject
/// ([`prim_near_rejected`], `AVSZ3` / `OTZ` at `0x80046000..0x8004601C`).
///
/// PORT: FUN_80045BB4 (the whole-quad rejects, 0x80045C30..0x80045C78)
pub fn bank3_quad_rejected(sxy: [[i32; 2]; 4], cull_back: bool) -> bool {
    (cull_back && nclip(sxy[0], sxy[1], sxy[2]) < 0) || quad_off_screen(sxy)
}

/// A draw's view for [`bank3_quad_rejected`]: the matrix that takes the
/// mesh's local positions to clip space and the frame's GTE terms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuadView {
    /// Column-major local-to-clip matrix.
    pub mvp: [f32; 16],
    /// Eye-space units per GTE unit of depth (`1` in battle).
    pub sz_per_w: f32,
    /// The projection distance `H`.
    pub h: f32,
    /// [`projection_aspect_fix`] of the camera.
    pub aspect_fix: f32,
    /// The winding test runs (the colour word does not carry `0x08000000`).
    pub cull_back: bool,
}

impl QuadView {
    /// Whether the leaf drops the quad with these local corners, scaled by
    /// `scale` (the part's render scale, which retail carries in its object
    /// matrix).
    pub fn rejects(&self, corners: &[[f32; 3]; 4], scale: f32) -> bool {
        let m = &self.mvp;
        let sxy = corners.map(|p| {
            let p = p.map(|v| v * scale);
            let c: [f32; 4] = std::array::from_fn(|r| {
                m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r]
            });
            gte_sxy(c, self.sz_per_w, self.h, self.aspect_fix)
        });
        bank3_quad_rejected(sxy, self.cull_back)
    }
}

/// The `aspect_fix` a host's camera projection carries
/// ([`gte_sxy`]): the ratio of its clip-`x` row to its `w` row, both scaled
/// by the view's uniform scale, over `H / 160`. `vp` is column-major.
pub fn projection_aspect_fix(vp: &[f32; 16], h: f32) -> f32 {
    let row = |r: usize| (vp[r].powi(2) + vp[4 + r].powi(2) + vp[8 + r].powi(2)).sqrt();
    let w = row(3);
    if w <= 0.0 || h <= 0.0 {
        return 1.0;
    }
    row(0) / w * 160.0 / h
}

/// Per-vertex primitive-corner records for a triangle list.
///
/// A primitive is a triangle, or two consecutive triangles that share an
/// edge (the mesh builders split a quad `[0,1,2, 1,3,2]`). Every vertex of a
/// primitive gets the primitive's corners, so the vertex stage reaches the
/// same verdict on each and the whole primitive drops together. A vertex
/// that two primitives share (a welded heightfield) has no single owner;
/// every primitive touching such a vertex is written with `n = 0` and is never
/// rejected, which keeps a shared edge from tearing.
pub fn prim_corner_refs(positions: &[[f32; 3]], indices: &[u32]) -> Vec<PrimRef> {
    prim_corner_plan(positions.len(), indices)
        .iter()
        .map(|&[c0, c1, c2, c3, n]| {
            if n == 0 {
                return [0.0; PRIM_REF_FLOATS];
            }
            let (a, b, c, d) = (
                positions[c0 as usize],
                positions[c1 as usize],
                positions[c2 as usize],
                positions[c3 as usize],
            );
            [
                a[0], a[1], a[2], n as f32, b[0], b[1], b[2], c[0], c[1], c[2], d[0], d[1], d[2],
            ]
        })
        .collect()
}

/// [`prim_corner_refs`]'s half that depends on the indices alone, per vertex:
/// its primitive's four corner vertices and corner count (`n = 0` for a vertex
/// with no single owner). A mesh that re-poses keeps its plan and re-reads only
/// the positions - the browser host does, per mesh per frame.
pub fn prim_corner_plan(vertex_count: usize, indices: &[u32]) -> Vec<[u32; 5]> {
    let mut plan = Vec::new();
    prim_corner_plan_into(
        vertex_count,
        indices,
        &mut plan,
        &mut PrimPlanScratch::default(),
    );
    plan
}

/// The working buffers [`prim_corner_plan_into`] reuses between calls.
#[derive(Default)]
pub struct PrimPlanScratch {
    groups: Vec<([u32; 4], u8)>,
    owner: Vec<usize>,
    shared: Vec<bool>,
}

/// [`prim_corner_plan`] into a caller's buffers: `plan` is cleared and refilled,
/// and a caller that keeps `plan` and `scratch` between frames allocates nothing
/// once they have grown to its largest mesh.
pub fn prim_corner_plan_into(
    vertex_count: usize,
    indices: &[u32],
    plan: &mut Vec<[u32; 5]>,
    scratch: &mut PrimPlanScratch,
) {
    plan.clear();
    plan.resize(vertex_count, [0u32; 5]);
    let tris = indices.as_chunks::<3>().0;
    // Group the triangles into primitives (three or four corners, held inline).
    let groups = &mut scratch.groups;
    groups.clear();
    groups.reserve(tris.len());
    let mut i = 0;
    while i < tris.len() {
        let a = tris[i];
        if let Some(b) = tris.get(i + 1) {
            let shared = b.iter().filter(|v| a.contains(v)).count();
            let mut extras = b.iter().copied().filter(|v| !a.contains(v));
            if let (2, Some(extra), None) = (shared, extras.next(), extras.next()) {
                // Retail corner order for a quad: v0, v1, v2, v3.
                groups.push(([a[0], a[1], a[2], extra], 4));
                i += 2;
                continue;
            }
        }
        groups.push(([a[0], a[1], a[2], 0], 3));
        i += 1;
    }
    // A vertex owned by more than one primitive disqualifies all of them.
    let owner = &mut scratch.owner;
    owner.clear();
    owner.resize(vertex_count, usize::MAX);
    let shared = &mut scratch.shared;
    shared.clear();
    shared.resize(groups.len(), false);
    for (g, (corners, n)) in groups.iter().enumerate() {
        for &v in &corners[..*n as usize] {
            let Some(o) = owner.get_mut(v as usize) else {
                continue;
            };
            if *o == usize::MAX || *o == g {
                *o = g;
            } else {
                shared[g] = true;
                shared[*o] = true;
            }
        }
    }
    for (g, (corners, n)) in groups.iter().enumerate() {
        let verts = &corners[..*n as usize];
        if shared[g] || verts.iter().any(|&v| v as usize >= vertex_count) {
            continue;
        }
        let c = |k: usize| verts[k.min(verts.len() - 1)];
        let rec = [c(0), c(1), c(2), c(3), verts.len() as u32];
        for &v in verts {
            plan[v as usize] = rec;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quad_off_one_edge_is_dropped_and_one_across_two_is_not() {
        let on = [[10, 10], [300, 10], [10, 200], [300, 200]];
        assert!(!quad_off_screen(on));
        assert!(quad_off_screen([[0, 10], [-5, 20], [0, 30], [-1, 40]]));
        assert!(!quad_off_screen([[1, 10], [-5, 20], [0, 30], [-1, 40]]));
        assert!(!quad_off_screen([[10, 0], [20, -5], [30, 0], [40, -1]]));
        assert!(quad_off_screen([
            [10, 240],
            [20, 300],
            [30, 241],
            [40, 999]
        ]));
        assert!(quad_off_screen([
            [320, 10],
            [400, 20],
            [330, 30],
            [1023, 40]
        ]));
        // Off the top on two corners and off the left on the others: no one
        // edge holds all four.
        assert!(!quad_off_screen([[10, -5], [20, -5], [-5, 30], [-5, 40]]));
    }

    #[test]
    fn the_winding_test_reads_corners_zero_to_two_and_honours_the_mask() {
        // Screen Y down: 0 -> 1 -> 2 clockwise on screen is a front face.
        let front = [[10, 10], [100, 10], [10, 100], [100, 100]];
        assert!(nclip(front[0], front[1], front[2]) > 0);
        assert!(!bank3_quad_rejected(front, true));
        let back = [front[1], front[0], front[3], front[2]];
        assert!(bank3_quad_rejected(back, true));
        assert!(!bank3_quad_rejected(back, false));
    }

    #[test]
    fn a_corner_on_screen_projects_where_the_gte_puts_it() {
        let h = 256.0;
        let bias = legaia_engine_camera::projection::GTE_OFY_NDC_BIAS;
        // Eye-space (64, -32, 512): SX = 160 + 256 * 64 / 512, SY = 114 + 256 * -32 / 512.
        let (ex, ey, ez) = (64.0f32, -32.0f32, 512.0f32);
        let clip = [h * ex / 160.0, -h * ey / 120.0 + bias * ez, 0.0, ez];
        assert_eq!(gte_sxy(clip, 1.0, h, 1.0), [192, 98]);
        // Behind the eye the capped divide puts it at OF + 2 * IR.
        let behind = [-(h * ex / 160.0), bias * -10.0, 0.0, -10.0];
        assert_eq!(gte_sxy(behind, 1.0, h, 1.0)[0], 160 - 128);
    }

    /// A field corner under `H = 608` (the `noaru` camera), eye offset
    /// `(x, y)` (Y down) and depth `z`, all in the engine's 1x eye units.
    fn field_clip(x: f32, y: f32, z: f32) -> [f32; 4] {
        let h = 608.0;
        let bias = legaia_engine_camera::projection::GTE_OFY_NDC_BIAS;
        [h / 160.0 * x, -h / 120.0 * -y + bias * z, 0.5, z]
    }

    #[test]
    fn an_unsaturated_corner_is_left_alone() {
        assert_eq!(
            gte_saturated_clip(field_clip(100.0, -50.0, 3000.0), 6.0, 608.0),
            None
        );
    }

    /// `noaru_field_card_walk`'s bowl rim: the far corners pass `SZ =
    /// 0xFFFF` (`6 * w > 65535`) and land further out, the off-screen ones
    /// reach `IR = 0x7FFF` and land further in - the retail packets' corners
    /// to within the engine's own pose error.
    #[test]
    fn the_noaru_bowl_saturates_like_its_packets() {
        let h = 608.0;
        let px = |c: [f32; 4]| {
            (
                (c[0] / c[3] * 0.5 + 0.5) * 320.0,
                (0.5 - c[1] / c[3] * 0.5) * 240.0,
            )
        };
        // From screen pixel + depth back to a clip corner.
        let corner =
            |sx: f32, sy: f32, w: f32| [(sx - 160.0) / 160.0 * w, (120.0 - sy) / 120.0 * w, 0.5, w];
        // Far rim: SZ saturates, the corner moves away from the centre.
        let far = gte_saturated_clip(corner(166.0, 306.0, 12347.0), 6.0, h).unwrap();
        let (x, y) = px(far);
        assert!(
            (x - 166.8).abs() < 1.0 && (y - 330.2).abs() < 1.5,
            "{x} {y}"
        );
        // Off-screen right: IR1 saturates, the corner moves in (retail 468).
        let side = gte_saturated_clip(corner(524.1, 133.0, 10728.2), 6.0, h).unwrap();
        let (x, _) = px(side);
        assert!((x - 470.0).abs() < 2.0, "{x}");
    }

    /// A corner behind the eye lands where the capped divide puts it,
    /// `OF + 2 * IR`, at the cap's depth and in front of the eye - the
    /// screen point retail's packet holds, which a near-plane clip would
    /// trim the primitive at instead.
    #[test]
    fn a_corner_behind_the_eye_lands_at_twice_its_offset() {
        let (h, k) = (608.0, 6.0);
        // Eye offset (10, 40) units, 5 units behind the eye.
        let c = field_clip(10.0, 40.0, -5.0);
        let out = gte_saturated_clip(c, k, h).unwrap();
        assert!((out[3] - h * 0.5 / k).abs() < 1e-3, "{out:?}");
        let sx = 160.0 + out[0] / out[3] * 160.0;
        let sy = 120.0 - out[1] / out[3] * 120.0;
        let bias = legaia_engine_camera::projection::GTE_OFY_NDC_BIAS;
        let ofy = 120.0 - bias * 120.0;
        assert!((sx - (160.0 + 2.0 * 10.0 * k)).abs() < 1e-2, "{sx}");
        let ir_y = -(c[1] - bias * c[3]) * 120.0 * k / h;
        assert!((ir_y.abs() - 40.0 * k).abs() < 1e-2, "{ir_y}");
        assert!((sy - (ofy + 2.0 * ir_y)).abs() < 1e-2, "{sy} {ir_y}");
        // The depth word: nearest, as no mean depth rode in.
        assert!((out[2] / out[3] - 1.0).abs() < 1e-6);
    }

    /// A battle-frame clip corner at eye offset `(x, y)` and depth `z`
    /// under `H = 256` on the 320 x 240 screen.
    fn clip(x: f32, y: f32, z: f32) -> [f32; 4] {
        [x * 256.0 / 160.0, y * 256.0 / 120.0, 0.0, z]
    }

    #[test]
    fn the_gpu_skips_a_triangle_wider_than_its_span_limit() {
        // A body-sized triangle in front of the eye is drawn.
        assert!(!gpu_span_rejected(
            [
                clip(0.0, 0.0, 1000.0),
                clip(300.0, 0.0, 1000.0),
                clip(0.0, 300.0, 1000.0)
            ],
            1.0,
            256.0
        ));
        // A leg running from mid-stage to just past the eye: the near corner
        // lands at twice its eye offset, saturated, and the span passes 511
        // rows - the GPU skips it.
        assert!(gpu_span_rejected(
            [
                clip(0.0, 0.0, 1500.0),
                clip(40.0, 0.0, 1500.0),
                clip(0.0, -400.0, -50.0)
            ],
            1.0,
            256.0
        ));
        // Off unless the enable lane carries `H`.
        assert_eq!(shader_params(Some((1.0, 2)), None)[0], 1.0);
        assert_eq!(shader_params(Some((1.0, 2)), Some(256.0))[0], 256.0);
        assert_eq!(shader_params(None, Some(256.0)), [0.0; 4]);
    }

    #[test]
    fn the_field_cut_drops_a_quad_whose_mean_depth_is_under_128() {
        assert!(prim_near_rejected(&[127.0; 4], 3));
        assert!(!prim_near_rejected(&[128.0; 4], 3));
        // Mean, not minimum: one near corner does not drop a far quad.
        assert!(!prim_near_rejected(&[0.0, 200.0, 200.0, 200.0], 3));
    }

    #[test]
    fn the_battle_cut_drops_under_64() {
        assert!(prim_near_rejected(&[63.0; 4], 2));
        assert!(!prim_near_rejected(&[64.0; 4], 2));
        assert!(prim_near_rejected(&[63.0; 3], 2));
    }

    #[test]
    fn a_corner_behind_the_eye_counts_as_zero_not_negative() {
        // Centroid depth 100 (rejected on the field), but the behind-the-eye
        // corner saturates to 0, lifting the mean to 175.
        assert!(!prim_near_rejected(&[-500.0, 300.0, 300.0, 300.0], 3));
    }

    #[test]
    fn triangles_use_zsf3() {
        // (0xAA * sum) >> 12 < 16  <=>  sum <= 385.
        assert!(prim_near_rejected(&[128.0, 128.0, 129.0], 3));
        assert!(!prim_near_rejected(&[128.0, 129.0, 129.0], 3));
    }

    #[test]
    fn a_split_quad_shares_one_record_and_a_welded_vertex_disables() {
        let pos = vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]];
        let refs = prim_corner_refs(&pos, &[0, 1, 2, 1, 3, 2]);
        assert!(refs.iter().all(|r| r[3] == 4.0));
        assert_eq!(&refs[0][10..13], &[1.0, 1.0, 0.0]);
        // Two triangles sharing only one vertex: separate prims, shared vertex.
        let pos = vec![[0.0; 3]; 5];
        let refs = prim_corner_refs(&pos, &[0, 1, 2, 2, 3, 4]);
        assert!(refs.iter().all(|r| r[3] == 0.0));
        // Two independent triangles.
        let pos = vec![[0.0; 3]; 6];
        let refs = prim_corner_refs(&pos, &[0, 1, 2, 3, 4, 5]);
        assert!(refs.iter().all(|r| r[3] == 3.0));
    }
}

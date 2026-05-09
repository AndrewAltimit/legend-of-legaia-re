//! GTE-style fixed-point math primitives.
//!
//! The retail TMD renderer at `FUN_8002735c` (60 GTE coprocessor ops) uses
//! signed 16-bit fixed-point matrix coefficients (`q12.4` in the rotation
//! matrix, `q14.16` for translation) to transform a vertex from object →
//! world → view → screen. This module mirrors the multiply-add accumulator
//! shape with tested arithmetic so engines and downstream tooling have a
//! single place to reproduce per-vertex GTE behaviour.
//!
//! ## What lives here
//!
//! * [`GteVec3`] / [`GteMat3`] — fixed-point vector + 3×3 rotation matrix in
//!   q3.12 storage with i64-widened multiply-add (`mul_vec`).
//! * [`Camera`] — rotation matrix + q19.12 translation + projection focal
//!   length `h` (the GTE register named `H`).
//! * [`Camera::transform`] — PSX GTE `RTPT` (rotate-translate-perspective):
//!   `screen = perspective_divide(rot * v + trans, h)`. Returns the
//!   screen-space coordinate plus the post-rotation Z used for depth.
//! * [`nclip`] — the GTE `NCLIP` operation: signed area of the screen-space
//!   triangle. Negative ⇒ back-face under PSX winding rules.
//! * [`avsz3`] / [`avsz4`] — average screen-Z helpers used by the OT-bucket
//!   selector.
//! * [`screen_to_pixel`] — clamps GTE screen coords (q.0 fixed-point in
//!   pixels) to a render target, with the GTE's saturation behaviour.
//! * A small CPU rasterizer scaffold under [`raster`] that plugs the above
//!   together — useful for offline regression checks against captured
//!   GTE traces.
//!
//! Production rendering still goes through wgpu's f32 pipeline (see
//! `Renderer::set_psx_mode`); this module is the source of truth when
//! something needs **pixel-exact** PSX behaviour, and supplies the f32
//! pipeline with the same constants (focal length, screen half-width)
//! the GTE used at runtime.
//!
//! ## Units (matching `docs/subsystems/renderer.md`)
//!
//! - World-position vertices: q19.12 (3D coordinate, signed)
//! - Rotation matrix: q3.12 (3×3 unit-vector basis, scaled by 4096)
//! - Translation: q19.12
//! - Projection focal length `h`: q.0 (PSX uses 320 for the standard 320×240 frame)
//! - Output screen-space: q.0 pixel coordinates, signed; clamped to viewport

/// Fixed-point bit count for rotation-matrix coefficients (q3.12).
pub const ROT_FRAC_BITS: i32 = 12;

/// Fixed-point scale factor for rotation coefficients (`1 << 12 = 4096`).
/// A retail GTE rotation matrix is encoded with element values in
/// `[-32768, 32767]`; an identity rotation has diagonal = `4096`.
pub const ROT_ONE: i32 = 1 << ROT_FRAC_BITS;

/// Default GTE focal length in pixels (the value the retail TMD renderer
/// loads into `H` for the standard 320×240 PSX frame: `H = 320`).
pub const DEFAULT_H: i32 = 320;

/// Saturated 16-bit signed clamp. The GTE saturates to `[-32768, 32767]`
/// when storing screen-space coordinates back into the SXY FIFO.
pub const SXY_MIN: i32 = i16::MIN as i32;
pub const SXY_MAX: i32 = i16::MAX as i32;

/// 3-vector of i32. Matches the GTE's MAC0/MAC1/MAC2/MAC3 accumulator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GteVec3 {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl GteVec3 {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// Convert from f32 world coordinates assuming q19.12 fixed-point.
    /// Saturates at i32 bounds to avoid panics on rare overflow.
    pub fn from_f32_q12(x: f32, y: f32, z: f32) -> Self {
        Self {
            x: clamp_i32_from_f32(x * ROT_ONE as f32),
            y: clamp_i32_from_f32(y * ROT_ONE as f32),
            z: clamp_i32_from_f32(z * ROT_ONE as f32),
        }
    }

    /// Convert back to f32 world coordinates.
    pub fn to_f32_q12(&self) -> (f32, f32, f32) {
        let inv = 1.0 / ROT_ONE as f32;
        (
            self.x as f32 * inv,
            self.y as f32 * inv,
            self.z as f32 * inv,
        )
    }
}

/// Row-major 3×3 rotation matrix in q3.12 fixed-point.
///
/// Identity:
/// ```text
/// [4096, 0, 0]
/// [0, 4096, 0]
/// [0, 0, 4096]
/// ```
/// Each element is in `[-32768, 32767]`. The retail GTE stores these as
/// `i16`; we widen to `i32` to keep the multiply-add path overflow-free
/// without saturation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GteMat3 {
    pub m: [[i16; 3]; 3],
}

impl GteMat3 {
    pub const IDENTITY: Self = Self {
        m: [
            [ROT_ONE as i16, 0, 0],
            [0, ROT_ONE as i16, 0],
            [0, 0, ROT_ONE as i16],
        ],
    };

    /// Build a rotation matrix about the +Y axis by `angle` radians, with
    /// elements quantized to q3.12 (matches the GTE precision).
    pub fn rot_y(angle: f32) -> Self {
        let c = (angle.cos() * ROT_ONE as f32).round() as i16;
        let s = (angle.sin() * ROT_ONE as f32).round() as i16;
        Self {
            m: [[c, 0, s], [0, ROT_ONE as i16, 0], [-s, 0, c]],
        }
    }

    /// Build a rotation about the +X axis (pitch).
    pub fn rot_x(angle: f32) -> Self {
        let c = (angle.cos() * ROT_ONE as f32).round() as i16;
        let s = (angle.sin() * ROT_ONE as f32).round() as i16;
        Self {
            m: [[ROT_ONE as i16, 0, 0], [0, c, -s], [0, s, c]],
        }
    }

    /// Build a rotation about the +Z axis (roll).
    pub fn rot_z(angle: f32) -> Self {
        let c = (angle.cos() * ROT_ONE as f32).round() as i16;
        let s = (angle.sin() * ROT_ONE as f32).round() as i16;
        Self {
            m: [[c, -s, 0], [s, c, 0], [0, 0, ROT_ONE as i16]],
        }
    }

    /// Compose two q3.12 rotation matrices: `out = a * b`. Same i64-widened
    /// accumulator shape as [`Self::mul_vec`]; result is renormalised back
    /// to q3.12 with i16 saturation on each element.
    pub fn mul(&self, other: &Self) -> Self {
        let mut out = [[0i16; 3]; 3];
        for (r, row_out) in out.iter_mut().enumerate() {
            for (c, slot) in row_out.iter_mut().enumerate() {
                let acc = (self.m[r][0] as i64) * (other.m[0][c] as i64)
                    + (self.m[r][1] as i64) * (other.m[1][c] as i64)
                    + (self.m[r][2] as i64) * (other.m[2][c] as i64);
                let scaled = acc >> ROT_FRAC_BITS;
                *slot = scaled.clamp(i16::MIN as i64, i16::MAX as i64) as i16;
            }
        }
        Self { m: out }
    }

    /// Apply this rotation matrix to `v` and return the result in q19.12.
    /// The accumulator is widened to i64 internally to prevent overflow on
    /// cumulative multiply-adds.
    pub fn mul_vec(&self, v: GteVec3) -> GteVec3 {
        let row = |r: usize| -> i32 {
            let a = (self.m[r][0] as i64) * (v.x as i64);
            let b = (self.m[r][1] as i64) * (v.y as i64);
            let c = (self.m[r][2] as i64) * (v.z as i64);
            // Sum of three i32×i16 products fits in i64; shift by ROT_FRAC_BITS
            // to drop the q12 fractional. Clamp on conversion to i32 so an
            // overflow surfaces as saturation, not a panic.
            ((a + b + c) >> ROT_FRAC_BITS).clamp(i32::MIN as i64, i32::MAX as i64) as i32
        };
        GteVec3 {
            x: row(0),
            y: row(1),
            z: row(2),
        }
    }
}

impl Default for GteMat3 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Equivalent of `RTPT` (rotate, translate, perspective transform): apply
/// `rot * v + trans`. Result is in q19.12.
pub fn rot_trans(rot: &GteMat3, v: GteVec3, trans: GteVec3) -> GteVec3 {
    let r = rot.mul_vec(v);
    GteVec3::new(
        r.x.saturating_add(trans.x),
        r.y.saturating_add(trans.y),
        r.z.saturating_add(trans.z),
    )
}

fn clamp_i32_from_f32(f: f32) -> i32 {
    if !f.is_finite() {
        return 0;
    }
    f.round().clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

/// 2D screen-space coordinate, GTE-style q.0 in pixels (signed i16-clamped).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScreenXY {
    pub x: i32,
    pub y: i32,
}

impl ScreenXY {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// Saturate to the GTE's i16 SXY-FIFO range. The retail GTE pushes
    /// off-screen coordinates through this clamp before the OT writer
    /// reads them; reproduce that saturation here so out-of-bounds verts
    /// behave the same as on hardware.
    pub fn saturate_sxy(self) -> Self {
        Self {
            x: self.x.clamp(SXY_MIN, SXY_MAX),
            y: self.y.clamp(SXY_MIN, SXY_MAX),
        }
    }
}

/// GTE camera state — the per-frame "rotation matrix + translation +
/// projection focal length" tuple the retail engine writes to the GTE
/// registers (RT/TR/H) before each `RTPT` batch.
#[derive(Debug, Clone, Copy)]
pub struct Camera {
    /// Rotation matrix (RT11..RT33), q3.12.
    pub rot: GteMat3,
    /// Translation (TRX/TRY/TRZ), q19.12.
    pub trans: GteVec3,
    /// Projection focal length `H` in pixels (q.0). PSX standard = 320.
    pub h: i32,
    /// Screen-center X offset (`OFX` in q16.16 terms; we store the integer
    /// pixel value the GTE biases by). Default 0 — set to `screen_w / 2`
    /// when projecting to a centered viewport.
    pub ofx: i32,
    /// Screen-center Y offset (`OFY`). Default 0.
    pub ofy: i32,
}

impl Camera {
    pub const fn identity() -> Self {
        Self {
            rot: GteMat3::IDENTITY,
            trans: GteVec3 { x: 0, y: 0, z: 0 },
            h: DEFAULT_H,
            ofx: 0,
            ofy: 0,
        }
    }

    /// Build a camera centered on the given viewport, with the standard
    /// PSX `H = 320` focal length. q19.12 translation is set to zero —
    /// override `.trans` after construction if you need eye-space offset.
    pub const fn for_viewport(width: i32, height: i32) -> Self {
        Self {
            rot: GteMat3::IDENTITY,
            trans: GteVec3 { x: 0, y: 0, z: 0 },
            h: DEFAULT_H,
            ofx: width / 2,
            ofy: height / 2,
        }
    }

    /// Rotate-translate-perspective transform. Mirrors the GTE `RTPT`
    /// op-code: `view = rot * v + trans` (q19.12), then `screen.x = view.x
    /// * h / view.z + ofx`, `screen.y = view.y * h / view.z + ofy`.
    ///
    /// Returns:
    /// - `screen_xy`: 2D screen position in q.0 pixel coords, NOT yet
    ///   saturated to i16. Caller picks: `.saturate_sxy()` for
    ///   hardware-faithful clipping, or use as-is for offline tooling.
    /// - `view_z`: post-translation Z (q19.12) — used by [`avsz3`] /
    ///   [`avsz4`] to assign an OT bucket.
    /// - `clip`: GTE-style clip flags. `Clip::SafeFront` ⇒ vertex is in
    ///   front of the camera; `Clip::Behind` ⇒ behind (project skipped,
    ///   coordinates set to GTE saturation). Tooling rendering frames
    ///   should drop primitives with any vertex `Behind`.
    pub fn transform(&self, v: GteVec3) -> ProjectedVertex {
        let view = rot_trans(&self.rot, v, self.trans);
        // The GTE's projection: SX = (H * MAC1) / MAC3 + OFX.
        // We work in q19.12 for view-space and produce q.0 pixel output;
        // the H multiply is q.0, divisions are integer, so we shift
        // out the q12 fractional from view.x / view.y before dividing.
        let (sx, sy, clip) = if view.z <= 0 {
            // Behind-camera: GTE saturates SX/SY toward i16 extremes
            // following the sign of the numerator. Approximate the same
            // behaviour without dividing by 0/negative.
            let sx = saturate_behind(view.x);
            let sy = saturate_behind(view.y);
            (sx, sy, Clip::Behind)
        } else {
            // h * x_q12 / z_q12 = (h * x) / z (the q12 cancels). Saturated
            // i64 multiply, then i64 divide.
            let z = view.z as i64;
            let sx_full = (self.h as i64 * view.x as i64) / z;
            let sy_full = (self.h as i64 * view.y as i64) / z;
            let sx = (sx_full + self.ofx as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
            let sy = (sy_full + self.ofy as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
            // SafeFront marks "valid front-facing"; tooling that wants to
            // exactly match GTE saturation behaviour can call
            // `.screen_xy.saturate_sxy()` on the result.
            (sx, sy, Clip::SafeFront)
        };
        ProjectedVertex {
            screen_xy: ScreenXY::new(sx, sy),
            view_z: view.z,
            clip,
        }
    }
}

impl Default for Camera {
    fn default() -> Self {
        Self::identity()
    }
}

/// Per-vertex GTE clip status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clip {
    /// Vertex is in front of the near plane and projection is well-defined.
    SafeFront,
    /// Vertex is behind or on the camera plane (`view.z <= 0`). The retail
    /// GTE saturates SXY in this case; the OT entry is usually skipped.
    Behind,
}

/// Output of a single GTE projection (one `RTPT` slot).
#[derive(Debug, Clone, Copy)]
pub struct ProjectedVertex {
    pub screen_xy: ScreenXY,
    pub view_z: i32,
    pub clip: Clip,
}

fn saturate_behind(numerator: i32) -> i32 {
    if numerator > 0 {
        SXY_MAX
    } else if numerator < 0 {
        SXY_MIN
    } else {
        0
    }
}

/// `NCLIP`: signed 2× area of the screen-space triangle (a, b, c), used by
/// the GTE for back-face rejection. The retail TMD renderer reads `MAC0`
/// after `NCLIP` and drops primitives where the result is non-negative
/// (i.e. zero area or back-facing under PSX winding).
///
/// Returns the cross product `(b - a) × (c - a)` widened to i64 so a
/// quadrant-spanning triangle doesn't overflow.
pub fn nclip(a: ScreenXY, b: ScreenXY, c: ScreenXY) -> i64 {
    let ab_x = (b.x - a.x) as i64;
    let ab_y = (b.y - a.y) as i64;
    let ac_x = (c.x - a.x) as i64;
    let ac_y = (c.y - a.y) as i64;
    ab_x * ac_y - ab_y * ac_x
}

/// `AVSZ3`: average of three SZ values (post-RTPT view-space Z). Returns
/// the OT-bucket index `(z0 + z1 + z2) * ZSF3 / 4096`, but with `ZSF3 = 1`
/// the formula simplifies to `(z0 + z1 + z2) >> ROT_FRAC_BITS`. Caller can
/// supply a custom scale via [`avsz3_with_scale`] when reproducing a
/// retail-captured GTE state.
pub fn avsz3(z0: i32, z1: i32, z2: i32) -> i32 {
    avsz3_with_scale(z0, z1, z2, ROT_ONE)
}

/// Same as [`avsz3`] but with an explicit `zsf3` (q3.12). The retail GTE
/// uses `ZSF3 = 1024` for an "average" bucket and `ZSF3 = 4096` for a
/// "sum" bucket.
pub fn avsz3_with_scale(z0: i32, z1: i32, z2: i32, zsf3: i32) -> i32 {
    let sum = z0 as i64 + z1 as i64 + z2 as i64;
    let scaled = (sum * zsf3 as i64) >> ROT_FRAC_BITS;
    scaled.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// `AVSZ4`: same as [`avsz3`] for quads.
pub fn avsz4(z0: i32, z1: i32, z2: i32, z3: i32) -> i32 {
    avsz4_with_scale(z0, z1, z2, z3, ROT_ONE)
}

/// Same as [`avsz4`] but with an explicit `zsf4` (q3.12).
pub fn avsz4_with_scale(z0: i32, z1: i32, z2: i32, z3: i32, zsf4: i32) -> i32 {
    let sum = z0 as i64 + z1 as i64 + z2 as i64 + z3 as i64;
    let scaled = (sum * zsf4 as i64) >> ROT_FRAC_BITS;
    scaled.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// Convert a GTE-projected `ScreenXY` (q.0 pixel coord, possibly outside
/// the viewport) into integer pixel coordinates clamped to a render
/// target. Useful for offline rasterizers that don't want to handle
/// off-screen vertices specially.
pub fn screen_to_pixel(screen: ScreenXY, w: i32, h: i32) -> (i32, i32) {
    (screen.x.clamp(0, w - 1), screen.y.clamp(0, h - 1))
}

/// CPU rasterizer scaffold — small enough to use as a regression target
/// against captured retail GTE traces without dragging in wgpu. Not
/// production-grade: it's a validation tool, not a renderer replacement.
pub mod raster {
    use super::*;

    /// Bounding box of a triangle in pixel coordinates.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct BBox {
        pub min_x: i32,
        pub min_y: i32,
        pub max_x: i32,
        pub max_y: i32,
    }

    impl BBox {
        pub fn from_triangle(a: ScreenXY, b: ScreenXY, c: ScreenXY) -> Self {
            Self {
                min_x: a.x.min(b.x).min(c.x),
                min_y: a.y.min(b.y).min(c.y),
                max_x: a.x.max(b.x).max(c.x),
                max_y: a.y.max(b.y).max(c.y),
            }
        }

        /// Clamp this bounding box to a render target. Returns `None` if the
        /// triangle is entirely off-screen.
        pub fn clamp(&self, w: i32, h: i32) -> Option<Self> {
            let r = Self {
                min_x: self.min_x.max(0),
                min_y: self.min_y.max(0),
                max_x: self.max_x.min(w - 1),
                max_y: self.max_y.min(h - 1),
            };
            if r.min_x > r.max_x || r.min_y > r.max_y {
                None
            } else {
                Some(r)
            }
        }
    }

    /// 2D edge function — positive when `p` is on the inside (right-hand
    /// side) of the directed edge `a→b` under PSX winding. Sums of three
    /// edge functions over a triangle's bbox give the barycentric weights
    /// for an inside-triangle test (all-positive ⇒ inside).
    pub fn edge(a: ScreenXY, b: ScreenXY, px: i32, py: i32) -> i64 {
        let ab_x = (b.x - a.x) as i64;
        let ab_y = (b.y - a.y) as i64;
        let ap_x = (px - a.x) as i64;
        let ap_y = (py - a.y) as i64;
        ab_x * ap_y - ab_y * ap_x
    }

    /// Whether `(px, py)` lies inside the triangle `(a, b, c)` under PSX
    /// winding rules. Assumes the triangle is front-facing
    /// ([`super::nclip`] returned negative); caller should reject
    /// back-facing triangles before rasterising.
    ///
    /// Edges on the bottom-right are counted as outside (top-left fill
    /// rule), matching the PSX rasteriser's pixel-center convention.
    pub fn contains(a: ScreenXY, b: ScreenXY, c: ScreenXY, px: i32, py: i32) -> bool {
        let w0 = edge(b, c, px, py);
        let w1 = edge(c, a, px, py);
        let w2 = edge(a, b, px, py);
        // Front-facing triangle: nclip < 0; the three edge functions then
        // share sign for inside points. Accept zero-area only on top-left
        // edges to avoid double-shading shared pixels.
        (w0 < 0 && w1 < 0 && w2 < 0)
            || (w0 == 0 && top_left(b, c))
            || (w1 == 0 && top_left(c, a))
            || (w2 == 0 && top_left(a, b))
    }

    /// PSX top-left fill rule: an edge counts as inside if it's exactly
    /// horizontal pointing leftward, OR a non-horizontal edge pointing
    /// upward.
    fn top_left(a: ScreenXY, b: ScreenXY) -> bool {
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        (dy == 0 && dx < 0) || dy < 0
    }

    /// Iterate every (px, py) inside `triangle`, calling `emit(px, py, w)`
    /// where `w = (w0, w1, w2)` is the unnormalised edge-function triple
    /// (caller can divide by triangle area to get barycentrics).
    pub fn rasterize_triangle(
        a: ScreenXY,
        b: ScreenXY,
        c: ScreenXY,
        viewport_w: i32,
        viewport_h: i32,
        mut emit: impl FnMut(i32, i32, (i64, i64, i64)),
    ) {
        let bbox = match BBox::from_triangle(a, b, c).clamp(viewport_w, viewport_h) {
            Some(b) => b,
            None => return,
        };
        for py in bbox.min_y..=bbox.max_y {
            for px in bbox.min_x..=bbox.max_x {
                let w0 = edge(b, c, px, py);
                let w1 = edge(c, a, px, py);
                let w2 = edge(a, b, px, py);
                if w0 < 0 && w1 < 0 && w2 < 0 {
                    emit(px, py, (w0, w1, w2));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_passes_vector_through() {
        let v = GteVec3::new(123, -456, 789);
        let r = GteMat3::IDENTITY.mul_vec(v);
        assert_eq!(r, v);
    }

    #[test]
    fn rot_y_180_negates_x_and_z() {
        let rot = GteMat3::rot_y(std::f32::consts::PI);
        let v = GteVec3::new(1000, 0, 0);
        let r = rot.mul_vec(v);
        // 180° about Y flips X (and Z when non-zero). Allow rounding error
        // up to a few units (q12 quantization → ~0.024% error per element).
        assert!((r.x - (-1000)).abs() <= 2, "x={}", r.x);
        assert_eq!(r.y, 0);
        assert!(r.z.abs() <= 2, "z={}", r.z);
    }

    #[test]
    fn rot_trans_applies_rotation_then_translation() {
        let rot = GteMat3::IDENTITY;
        let trans = GteVec3::new(10, 20, 30);
        let v = GteVec3::new(1, 2, 3);
        assert_eq!(rot_trans(&rot, v, trans), GteVec3::new(11, 22, 33));
    }

    #[test]
    fn fixed_point_round_trip() {
        let original = (1.5f32, -3.25, 0.125);
        let v = GteVec3::from_f32_q12(original.0, original.1, original.2);
        let back = v.to_f32_q12();
        // q12 fixed-point gives ~1/4096 resolution. Each example here is
        // exactly representable.
        assert!((back.0 - original.0).abs() < 1.0 / ROT_ONE as f32 + 1e-6);
        assert!((back.1 - original.1).abs() < 1.0 / ROT_ONE as f32 + 1e-6);
        assert!((back.2 - original.2).abs() < 1.0 / ROT_ONE as f32 + 1e-6);
    }

    #[test]
    fn mul_vec_does_not_overflow_on_max_inputs() {
        // Worst case: rotation with max elements (32767) applied to a
        // vector with max coordinates (i32::MAX / 4 to keep headroom).
        // i64 accumulator must absorb 3 × i32×i16 without panicking.
        let m = GteMat3 {
            m: [[32767, 32767, 32767], [0, 0, 0], [0, 0, 0]],
        };
        let v = GteVec3::new(i32::MAX / 4, i32::MAX / 4, i32::MAX / 4);
        let r = m.mul_vec(v);
        assert_eq!(r.x, i32::MAX);
    }

    #[test]
    fn rot_x_90_y_to_z() {
        // 90° pitch around +X axis sends +Y to +Z.
        let rot = GteMat3::rot_x(std::f32::consts::FRAC_PI_2);
        let v = GteVec3::new(0, 1000, 0);
        let r = rot.mul_vec(v);
        assert!(r.y.abs() <= 2, "y={}", r.y);
        assert!((r.z - 1000).abs() <= 2, "z={}", r.z);
    }

    #[test]
    fn rot_z_90_x_to_y() {
        // 90° roll around +Z axis sends +X to +Y.
        let rot = GteMat3::rot_z(std::f32::consts::FRAC_PI_2);
        let v = GteVec3::new(1000, 0, 0);
        let r = rot.mul_vec(v);
        assert!(r.x.abs() <= 2, "x={}", r.x);
        assert!((r.y - 1000).abs() <= 2, "y={}", r.y);
    }

    #[test]
    fn mat3_mul_identity_is_noop() {
        let r = GteMat3::rot_y(0.7);
        let combined = r.mul(&GteMat3::IDENTITY);
        // Identity composition should be lossless within q3.12 rounding.
        for i in 0..3 {
            for j in 0..3 {
                assert!(
                    (combined.m[i][j] as i32 - r.m[i][j] as i32).abs() <= 1,
                    "[{i}][{j}] mismatch: combined={} vs r={}",
                    combined.m[i][j],
                    r.m[i][j],
                );
            }
        }
    }

    #[test]
    fn mat3_mul_compose_two_y_rotations() {
        // rot_y(a) * rot_y(b) ≈ rot_y(a + b) — verify within q3.12 rounding.
        let a = std::f32::consts::FRAC_PI_4;
        let b = std::f32::consts::FRAC_PI_3;
        let composed = GteMat3::rot_y(a).mul(&GteMat3::rot_y(b));
        let direct = GteMat3::rot_y(a + b);
        for i in 0..3 {
            for j in 0..3 {
                assert!(
                    (composed.m[i][j] as i32 - direct.m[i][j] as i32).abs() <= 4,
                    "[{i}][{j}] composed={} direct={}",
                    composed.m[i][j],
                    direct.m[i][j],
                );
            }
        }
    }

    #[test]
    fn camera_identity_keeps_origin_at_screen_center() {
        let cam = Camera::for_viewport(320, 240);
        let mut cam = cam;
        cam.trans = GteVec3::new(0, 0, ROT_ONE * 1024); // 1024 units forward
        // A vertex at world origin sits 1024 in front of the camera.
        let p = cam.transform(GteVec3::new(0, 0, 0));
        assert_eq!(p.clip, Clip::SafeFront);
        assert_eq!(p.screen_xy.x, 160);
        assert_eq!(p.screen_xy.y, 120);
    }

    #[test]
    fn camera_projects_x_to_right_of_screen() {
        let mut cam = Camera::for_viewport(320, 240);
        cam.trans = GteVec3::new(0, 0, ROT_ONE * 1024);
        // Vertex at +X (right of camera): screen.x > 160.
        let p = cam.transform(GteVec3::from_f32_q12(100.0, 0.0, 0.0));
        assert_eq!(p.clip, Clip::SafeFront);
        assert!(
            p.screen_xy.x > 160,
            "expected right of center: {}",
            p.screen_xy.x
        );
    }

    #[test]
    fn camera_marks_behind_camera_vertex() {
        let cam = Camera::for_viewport(320, 240);
        // No translation; vertex with view.z = 0 is on camera plane.
        let p = cam.transform(GteVec3::new(0, 0, 0));
        assert_eq!(p.clip, Clip::Behind);
    }

    #[test]
    fn camera_projection_is_pixel_exact_for_unit_z() {
        // Pin one specific projection so we catch regressions: with H=320,
        // a vertex at (1024, 0, 1024) (in q19.12 = 0.25 world units in X,
        // 0.25 in Z) projects to screen.x = 320 * 1024 / 1024 = 320,
        // i.e. one full focal-length offset to the right of the screen
        // origin. (No center offset here.)
        let cam = Camera {
            rot: GteMat3::IDENTITY,
            trans: GteVec3::new(0, 0, 0),
            h: 320,
            ofx: 0,
            ofy: 0,
        };
        let p = cam.transform(GteVec3::new(1024, 0, 1024));
        assert_eq!(p.clip, Clip::SafeFront);
        assert_eq!(p.screen_xy.x, 320);
        assert_eq!(p.screen_xy.y, 0);
    }

    #[test]
    fn nclip_signs_back_vs_front() {
        // CCW triangle: (0,0), (10,0), (0,10). Under PSX winding
        // (y-down), this is back-facing — nclip > 0. CW is front (negative).
        let a = ScreenXY::new(0, 0);
        let b = ScreenXY::new(10, 0);
        let c = ScreenXY::new(0, 10);
        // (b-a)x = 10, y = 0; (c-a)x = 0, y = 10. cross = 10*10 - 0*0 = 100.
        assert_eq!(nclip(a, b, c), 100);
        // Reversed triangle is front-facing (negative).
        assert_eq!(nclip(a, c, b), -100);
    }

    #[test]
    fn nclip_zero_area_is_degenerate() {
        let a = ScreenXY::new(5, 5);
        let b = ScreenXY::new(15, 5);
        let c = ScreenXY::new(25, 5); // colinear
        assert_eq!(nclip(a, b, c), 0);
    }

    #[test]
    fn avsz3_zsf_default_averages_q12() {
        // With ZSF3 = ROT_ONE, the formula is (z0+z1+z2)*ROT_ONE / ROT_ONE
        // = z0+z1+z2 (the q12 cancels). So the function returns the *sum*,
        // not a true 1/3 average — that matches a retail capture where ZSF3
        // was loaded with 4096 (the "sum" bucket scale).
        assert_eq!(avsz3(100, 200, 300), 600);
    }

    #[test]
    fn avsz3_with_one_third_scale() {
        // ZSF3 = ROT_ONE / 3 ≈ 1365 gives true average. Allow rounding.
        let r = avsz3_with_scale(100, 200, 300, ROT_ONE / 3);
        assert!((r - 200).abs() <= 1, "expected ~200, got {r}");
    }

    #[test]
    fn avsz4_sums_four_zs() {
        assert_eq!(avsz4(100, 200, 300, 400), 1000);
    }

    #[test]
    fn screen_to_pixel_clamps_off_screen() {
        let off_left = ScreenXY::new(-100, 50);
        let (px, py) = screen_to_pixel(off_left, 320, 240);
        assert_eq!(px, 0);
        assert_eq!(py, 50);

        let off_right = ScreenXY::new(500, 300);
        let (px, py) = screen_to_pixel(off_right, 320, 240);
        assert_eq!(px, 319);
        assert_eq!(py, 239);
    }

    #[test]
    fn saturate_sxy_clamps_to_i16() {
        let big = ScreenXY::new(i32::MAX, i32::MIN).saturate_sxy();
        assert_eq!(big.x, SXY_MAX);
        assert_eq!(big.y, SXY_MIN);
    }

    #[test]
    fn raster_bbox_from_triangle() {
        let bbox = raster::BBox::from_triangle(
            ScreenXY::new(10, 20),
            ScreenXY::new(40, 5),
            ScreenXY::new(25, 50),
        );
        assert_eq!(bbox.min_x, 10);
        assert_eq!(bbox.min_y, 5);
        assert_eq!(bbox.max_x, 40);
        assert_eq!(bbox.max_y, 50);
    }

    #[test]
    fn raster_bbox_clamp_off_screen_returns_none() {
        let bbox = raster::BBox::from_triangle(
            ScreenXY::new(-100, -100),
            ScreenXY::new(-50, -100),
            ScreenXY::new(-100, -50),
        );
        assert!(bbox.clamp(320, 240).is_none());
    }

    #[test]
    fn raster_contains_inside_point() {
        // CW triangle (front-facing under PSX winding).
        let a = ScreenXY::new(0, 0);
        let b = ScreenXY::new(0, 10);
        let c = ScreenXY::new(10, 0);
        assert!(
            raster::contains(a, b, c, 2, 2),
            "(2,2) should be inside CW triangle"
        );
    }

    #[test]
    fn raster_contains_outside_point() {
        let a = ScreenXY::new(0, 0);
        let b = ScreenXY::new(0, 10);
        let c = ScreenXY::new(10, 0);
        assert!(
            !raster::contains(a, b, c, 20, 20),
            "(20,20) outside triangle"
        );
    }

    #[test]
    fn raster_iterates_inside_pixels() {
        // Simple CW right-triangle covering pixels (1,1)..(8,1), etc.
        // We just count to make sure the iterator covers a believable set.
        let a = ScreenXY::new(0, 0);
        let b = ScreenXY::new(0, 10);
        let c = ScreenXY::new(10, 0);
        let mut count = 0;
        raster::rasterize_triangle(a, b, c, 320, 240, |_, _, _| count += 1);
        // Triangle area = 50 px²; rasterizer hits ~50 inside pixels.
        // Allow a small fudge for top-left fill-rule edge inclusion.
        assert!((30..=60).contains(&count), "got {count} pixels");
    }
}

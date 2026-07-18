use super::*;

/// GTE camera state - the per-frame "rotation matrix + translation +
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
    /// pixel value the GTE biases by). Default 0 - set to `screen_w / 2`
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
    /// PSX `H = 320` focal length. q19.12 translation is set to zero -
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
    /// - `view_z`: post-translation Z (q19.12) - used by [`avsz3`] /
    ///   [`avsz4`] to assign an OT bucket.
    /// - `clip`: GTE-style clip flags. `Clip::SafeFront` ⇒ vertex is in
    ///   front of the camera; `Clip::Behind` ⇒ behind (project skipped,
    ///   coordinates set to GTE saturation). Tooling rendering frames
    ///   should drop primitives with any vertex `Behind`.
    pub fn transform(&self, v: GteVec3) -> ProjectedVertex {
        let view = rot_trans(&self.rot, v, self.trans);
        // The GTE's projection: SX = OFX + (IR1 * (H / SZ3)) >> 16, where the
        // divide is the UNR reciprocal, not an exact division.
        // REF: gte_divide (crate::gte::math). View-space is held in q19.12
        // (4096x the hardware IR/SZ scale), so reduce to the hardware IR1/IR2
        // numerator and SZ3 depth with a >>12 before feeding the divide. A
        // behind-camera vertex is not a special hardware case: SZ3 clamps to 0
        // and the divide overflows to the 0x1FFFF quotient just like on a real
        // GTE. Tooling drops such primitives via the `Clip` flag rather than
        // via bogus coordinates - the numbers below are exactly what hardware
        // would latch.
        let sz3 = ((view.z >> ROT_FRAC_BITS).clamp(0, u16::MAX as i32)) as u16;
        let ir_x = (view.x >> ROT_FRAC_BITS).clamp(SXY_MIN, SXY_MAX);
        let ir_y = (view.y >> ROT_FRAC_BITS).clamp(SXY_MIN, SXY_MAX);
        let (recip, _overflow) = gte_divide(self.h as u16, sz3);
        let sx = (gte_persp_term(ir_x, recip) + self.ofx as i64)
            .clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        let sy = (gte_persp_term(ir_y, recip) + self.ofy as i64)
            .clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        // `Behind` marks vertices on/behind the camera plane; `SafeFront`
        // marks valid front-facing ones. Tooling that wants to exactly match
        // GTE SXY saturation can call `.screen_xy.saturate_sxy()`.
        let clip = if view.z <= 0 {
            Clip::Behind
        } else {
            Clip::SafeFront
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

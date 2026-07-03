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

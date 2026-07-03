use super::*;

impl Gte {
    /// Push an SXY entry, advancing the FIFO. SXY0 ← SXY1 ← SXY2 ← new.
    fn push_sxy(&mut self, xy: ScreenXY) {
        let saturated = xy.saturate_sxy();
        if saturated.x != xy.x {
            self.flag |= flag_bits::SX2_SATURATED | flag_bits::ANY_ERROR;
        }
        if saturated.y != xy.y {
            self.flag |= flag_bits::SY2_SATURATED | flag_bits::ANY_ERROR;
        }
        self.sxy_fifo[0] = self.sxy_fifo[1];
        self.sxy_fifo[1] = self.sxy_fifo[2];
        self.sxy_fifo[2] = saturated;
    }

    /// Push an SZ entry, advancing the FIFO. SZ0 ← SZ1 ← SZ2 ← SZ3 ← new.
    fn push_sz(&mut self, z: i64) {
        let clamped = if z > u16::MAX as i64 {
            self.flag |= flag_bits::SZ3_OTZ_SATURATED | flag_bits::ANY_ERROR;
            u16::MAX
        } else if z < 0 {
            self.flag |= flag_bits::SZ3_OTZ_SATURATED | flag_bits::ANY_ERROR;
            0
        } else {
            z as u16
        };
        self.sz_fifo[0] = self.sz_fifo[1];
        self.sz_fifo[1] = self.sz_fifo[2];
        self.sz_fifo[2] = self.sz_fifo[3];
        self.sz_fifo[3] = clamped;
    }

    /// `RTPS` (Rotate-Translate-Perspective, single vertex): transform `V0`
    /// using the current RT/TR/H/OFX/OFY and push the result onto the SXY
    /// and SZ FIFOs. Sets MAC1/MAC2/MAC3 to the post-rotation view-space
    /// vector and IR1/IR2/IR3 to its saturated short form. Returns the
    /// projected ScreenXY.
    pub fn rtps(&mut self) -> ScreenXY {
        self.begin_op(CopOp::Rtps);
        self.rtps_inner(self.v[0])
    }

    /// `RTPT` (Rotate-Translate-Perspective, three vertices): apply RTPS to
    /// V0, V1, V2 in order. The SXY FIFO ends up with the three projected
    /// vertices in oldest-first order (SXY0 = V0's projection, SXY2 = V2's).
    pub fn rtpt(&mut self) -> [ScreenXY; 3] {
        self.begin_op(CopOp::Rtpt);
        let v = self.v;
        let s0 = self.rtps_inner(v[0]);
        let s1 = self.rtps_inner(v[1]);
        let s2 = self.rtps_inner(v[2]);
        [s0, s1, s2]
    }

    fn rtps_inner(&mut self, vertex: GteVec3) -> ScreenXY {
        // view = rot * v + trans
        let view = rot_trans(&self.rot, vertex, self.trans);
        // Update MAC1/2/3 with the view-space components (i64-widened).
        self.mac1 = view.x as i64;
        self.mac2 = view.y as i64;
        self.mac3 = view.z as i64;
        // IR1/2/3 ← saturated MAC1/2/3 to i16.
        self.ir1 = self.saturate_ir(self.mac1, flag_bits::IR1_SATURATED);
        self.ir2 = self.saturate_ir(self.mac2, flag_bits::IR2_SATURATED);
        self.ir3 = self.saturate_ir(self.mac3, flag_bits::IR3_SATURATED);

        // Perspective divide. SX = (H * MAC1) / MAC3 + OFX.
        let (sx, sy) = if view.z <= 0 {
            self.flag |= flag_bits::MAC3_OVERFLOW_NEG | flag_bits::ANY_ERROR;
            (saturate_behind(view.x), saturate_behind(view.y))
        } else {
            let z = view.z as i64;
            let sx_full = (self.h as i64 * view.x as i64) / z;
            let sy_full = (self.h as i64 * view.y as i64) / z;
            let sx = (sx_full + self.ofx as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
            let sy = (sy_full + self.ofy as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
            (sx, sy)
        };
        // Push SXY and SZ; the FIFOs handle their own saturation flags.
        let xy = ScreenXY::new(sx, sy);
        self.push_sxy(xy);
        // SZ FIFO stores view-space Z scaled by 1/4096 (q19.12 → integer
        // bucket). Hardware divides by 4096 before storing; we mirror that
        // and then clamp to u16.
        let sz_in = (view.z as i64) >> ROT_FRAC_BITS;
        self.push_sz(sz_in);
        // Output SXY is the saturated form already in the FIFO.
        self.sxy_fifo[2]
    }

    /// `NCLIP` - signed area of the triangle SXY0/SXY1/SXY2. Writes MAC0.
    /// Returns the same value the FLAG and MAC0 reflect.
    pub fn nclip(&mut self) -> i64 {
        self.begin_op(CopOp::Nclip);
        let v = nclip(self.sxy_fifo[0], self.sxy_fifo[1], self.sxy_fifo[2]);
        // MAC0 saturation is at i32 bounds; track overflow via FLAG.
        self.mac0 = if v > i32::MAX as i64 {
            self.flag |= flag_bits::MAC0_OVERFLOW_POS | flag_bits::ANY_ERROR;
            i32::MAX
        } else if v < i32::MIN as i64 {
            self.flag |= flag_bits::MAC0_OVERFLOW_NEG | flag_bits::ANY_ERROR;
            i32::MIN
        } else {
            v as i32
        };
        v
    }

    /// `AVSZ3` - write OTZ ← `((SZ1 + SZ2 + SZ3) * ZSF3) >> ROT_FRAC_BITS`.
    /// Writes MAC0 to the un-shifted product so callers can recover the
    /// full-precision intermediate.
    pub fn avsz3(&mut self) -> u16 {
        self.begin_op(CopOp::Avsz3);
        let sum = self.sz_fifo[1] as i64 + self.sz_fifo[2] as i64 + self.sz_fifo[3] as i64;
        let scaled = sum * self.zsf3 as i64;
        self.mac0 = scaled.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        let shifted = scaled >> ROT_FRAC_BITS;
        let otz = if shifted > u16::MAX as i64 {
            self.flag |= flag_bits::SZ3_OTZ_SATURATED | flag_bits::ANY_ERROR;
            u16::MAX
        } else if shifted < 0 {
            self.flag |= flag_bits::SZ3_OTZ_SATURATED | flag_bits::ANY_ERROR;
            0
        } else {
            shifted as u16
        };
        self.otz = otz;
        otz
    }

    /// `AVSZ4` - write OTZ ← `((SZ0 + SZ1 + SZ2 + SZ3) * ZSF4) >> ROT_FRAC_BITS`.
    pub fn avsz4(&mut self) -> u16 {
        self.begin_op(CopOp::Avsz4);
        let sum = self.sz_fifo[0] as i64
            + self.sz_fifo[1] as i64
            + self.sz_fifo[2] as i64
            + self.sz_fifo[3] as i64;
        let scaled = sum * self.zsf4 as i64;
        self.mac0 = scaled.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        let shifted = scaled >> ROT_FRAC_BITS;
        let otz = if shifted > u16::MAX as i64 {
            self.flag |= flag_bits::SZ3_OTZ_SATURATED | flag_bits::ANY_ERROR;
            u16::MAX
        } else if shifted < 0 {
            self.flag |= flag_bits::SZ3_OTZ_SATURATED | flag_bits::ANY_ERROR;
            0
        } else {
            shifted as u16
        };
        self.otz = otz;
        otz
    }

    /// `MVMVA` - generic matrix-vector multiply with selectable matrix
    /// (rotation / light / color), vector source (V0/V1/V2/IR), and
    /// translation source (TR / BK / FC / none). This is the most flexible
    /// GTE primitive - engines wire it for lighting passes and arbitrary
    /// affine transforms.
    ///
    /// Args:
    /// - `mat`: the 3×3 matrix to multiply by.
    /// - `vec`: the 3-vector input.
    /// - `trans`: the optional translation to add (pass `GteVec3::default()`
    ///   for no translation).
    /// - `shift_frac`: `true` to right-shift the result by `ROT_FRAC_BITS`
    ///   (matches GTE's `SF` flag); `false` to keep full-precision MAC.
    /// - `lm`: `true` to clamp IR1/IR2/IR3 to `[0, 0x7FFF]` instead of the
    ///   default `[-0x8000, 0x7FFF]` (matches GTE's `LM` flag, used for
    ///   colour interpolation).
    ///
    /// Result lives in MAC1/MAC2/MAC3 and IR1/IR2/IR3 after the call.
    pub fn mvmva(
        &mut self,
        mat: &GteMat3,
        vec: GteVec3,
        trans: GteVec3,
        shift_frac: bool,
        lm: bool,
    ) {
        self.begin_op(CopOp::Mvmva);
        self.mvmva_inner(mat, vec, trans, shift_frac, lm);
    }

    /// Internal MVMVA without cycle / FLAG bookkeeping. Used by lighting
    /// helpers that have already charged their parent op (NCDS / CDP / CC
    /// etc.) and don't want to double-count cycles for the inner
    /// matrix-vector pass.
    pub(super) fn mvmva_inner(
        &mut self,
        mat: &GteMat3,
        vec: GteVec3,
        trans: GteVec3,
        shift_frac: bool,
        lm: bool,
    ) {
        let row = |r: usize| -> i64 {
            (mat.m[r][0] as i64) * (vec.x as i64)
                + (mat.m[r][1] as i64) * (vec.y as i64)
                + (mat.m[r][2] as i64) * (vec.z as i64)
        };
        let raw = [
            row(0) + (trans.x as i64) * (ROT_ONE as i64),
            row(1) + (trans.y as i64) * (ROT_ONE as i64),
            row(2) + (trans.z as i64) * (ROT_ONE as i64),
        ];
        let macs: [i64; 3] = if shift_frac {
            [
                raw[0] >> ROT_FRAC_BITS,
                raw[1] >> ROT_FRAC_BITS,
                raw[2] >> ROT_FRAC_BITS,
            ]
        } else {
            raw
        };
        self.mac1 = macs[0];
        self.mac2 = macs[1];
        self.mac3 = macs[2];

        // IR1/2/3 saturation. `lm` clamps the lower bound to 0.
        let lo = if lm { 0 } else { i16::MIN as i64 };
        let sat = |v: i64, bit: u32, flag: &mut u32| -> i32 {
            if v > i16::MAX as i64 {
                *flag |= bit | flag_bits::ANY_ERROR;
                i16::MAX as i32
            } else if v < lo {
                *flag |= bit | flag_bits::ANY_ERROR;
                lo as i32
            } else {
                v as i32
            }
        };
        self.ir1 = sat(macs[0], flag_bits::IR1_SATURATED, &mut self.flag);
        self.ir2 = sat(macs[1], flag_bits::IR2_SATURATED, &mut self.flag);
        self.ir3 = sat(macs[2], flag_bits::IR3_SATURATED, &mut self.flag);
    }

    /// Convenience: project the current SXY FIFO contents into a vertex
    /// triangle using [`raster::rasterize_triangle`]. Iterates only the
    /// inside pixels, calling `emit(px, py, w)` per pixel.
    pub fn rasterize_sxy_triangle(
        &self,
        viewport_w: i32,
        viewport_h: i32,
        emit: impl FnMut(i32, i32, (i64, i64, i64)),
    ) {
        raster::rasterize_triangle(
            self.sxy_fifo[0],
            self.sxy_fifo[1],
            self.sxy_fifo[2],
            viewport_w,
            viewport_h,
            emit,
        );
    }
}

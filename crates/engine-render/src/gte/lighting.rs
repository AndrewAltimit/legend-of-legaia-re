use super::*;

impl Gte {
    // ---------------------------------------------------------------------
    // Lighting / colour ops.
    //
    // These are the cop2 "depth cue" / "normal colour" instructions used by
    // shaded TMD primitives. They consume a normal vector (V0..V2 for the
    // triple variants) and write per-vertex RGB into the RGB FIFO.
    // ---------------------------------------------------------------------

    /// Push an RGB FIFO entry. RGB0 ← RGB1 ← RGB2 ← new.
    fn push_rgb(&mut self, rgb: [u8; 4]) {
        self.rgb_fifo[0] = self.rgb_fifo[1];
        self.rgb_fifo[1] = self.rgb_fifo[2];
        self.rgb_fifo[2] = rgb;
    }

    /// Saturate a 24-bit signed RGB component to `[0, 255]`. Mirrors the
    /// GTE colour-clamp that fires when MAC1/MAC2/MAC3 are written into
    /// the RGB FIFO.
    fn saturate_rgb_u8(&mut self, v: i64, sat_bit: u32) -> u8 {
        if v < 0 {
            self.flag |= sat_bit | flag_bits::ANY_ERROR;
            0
        } else if v > 0xFF {
            self.flag |= sat_bit | flag_bits::ANY_ERROR;
            0xFF
        } else {
            v as u8
        }
    }

    /// Common helper: multiply the light matrix against a vertex normal,
    /// clamp to IR, then run through the light-color matrix + back-color
    /// bias. Stores intermediate per-component intensity in MAC1/2/3.
    fn light_pass(&mut self, normal: GteVec3) {
        // L * normal (q3.12 * q3.12 -> q6.24, shifted back to q3.12).
        self.mvmva_inner(&self.light.clone(), normal, GteVec3::default(), true, true);
        let intensity = GteVec3::new(self.ir1, self.ir2, self.ir3);
        // light_color * intensity + back_color
        // back_color is q3.12, apply through MVMVA's translation argument.
        let bc = self.back_color;
        self.mvmva_inner(&self.light_color.clone(), intensity, bc, true, true);
    }

    /// `NCDS` - normal colour depth (single vertex). Computes per-vertex
    /// shaded RGB using the light matrix + light-color matrix + far-color
    /// blend, modulated by the input RGBC. Pushes the result onto the
    /// RGB FIFO.
    pub fn ncds(&mut self) -> [u8; 4] {
        self.begin_op(CopOp::Ncds);
        self.ncds_inner(self.v[0])
    }

    /// `NCDT` - normal colour depth, triple. Applies NCDS to V0/V1/V2 in
    /// order; the RGB FIFO ends up with the three shaded colours.
    pub fn ncdt(&mut self) -> [[u8; 4]; 3] {
        self.begin_op(CopOp::Ncdt);
        let v = self.v;
        let r0 = self.ncds_inner(v[0]);
        let r1 = self.ncds_inner(v[1]);
        let r2 = self.ncds_inner(v[2]);
        [r0, r1, r2]
    }

    fn ncds_inner(&mut self, normal: GteVec3) -> [u8; 4] {
        self.light_pass(normal);
        // After light_pass IR1/IR2/IR3 are the diffuse colour. Blend
        // toward far_color by IR0 (depth fade): IR_n += (FC_n - IR_n) * IR0 / 4096.
        let fc = self.far_color;
        let blend = |fc_n: i32, ir_n: i32, ir0: i32| -> i32 {
            let delta = (fc_n - ir_n) as i64;
            let scaled = (delta * ir0 as i64) >> ROT_FRAC_BITS;
            (ir_n as i64 + scaled).clamp(i16::MIN as i64, i16::MAX as i64) as i32
        };
        let r_blended = blend(fc.x, self.ir1, self.ir0);
        let g_blended = blend(fc.y, self.ir2, self.ir0);
        let b_blended = blend(fc.z, self.ir3, self.ir0);
        // Modulate by RGBC. (IR_n * RGBC_n) >> 4 fits the GTE's 12.4 layout.
        let modulate = |ir: i32, mat: u8| -> i64 { (ir as i64 * mat as i64) >> 4 };
        let r = modulate(r_blended, self.rgbc[0]);
        let g = modulate(g_blended, self.rgbc[1]);
        let b = modulate(b_blended, self.rgbc[2]);
        let r_u8 = self.saturate_rgb_u8(r, flag_bits::IR1_SATURATED);
        let g_u8 = self.saturate_rgb_u8(g, flag_bits::IR2_SATURATED);
        let b_u8 = self.saturate_rgb_u8(b, flag_bits::IR3_SATURATED);
        let out = [r_u8, g_u8, b_u8, self.rgbc[3]];
        self.push_rgb(out);
        out
    }

    /// `DCPL` - depth-cued primary color. Modulates the input RGBC with
    /// IR1/2/3 then blends toward far_color by IR0 - same depth-fade
    /// behaviour as NCDS but starting from the existing RGBC instead of
    /// running a light pass. Pushes the result onto the RGB FIFO.
    pub fn dcpl(&mut self) -> [u8; 4] {
        self.begin_op(CopOp::Dcpl);
        let fc = self.far_color;
        let blend = |fc_n: i32, ir_n: i32, ir0: i32| -> i32 {
            let delta = (fc_n - ir_n) as i64;
            let scaled = (delta * ir0 as i64) >> ROT_FRAC_BITS;
            (ir_n as i64 + scaled).clamp(i16::MIN as i64, i16::MAX as i64) as i32
        };
        let r = blend(fc.x, self.ir1, self.ir0);
        let g = blend(fc.y, self.ir2, self.ir0);
        let b = blend(fc.z, self.ir3, self.ir0);
        let modulate = |ir: i32, mat: u8| -> i64 { (ir as i64 * mat as i64) >> 4 };
        let rr = self.saturate_rgb_u8(modulate(r, self.rgbc[0]), flag_bits::IR1_SATURATED);
        let gg = self.saturate_rgb_u8(modulate(g, self.rgbc[1]), flag_bits::IR2_SATURATED);
        let bb = self.saturate_rgb_u8(modulate(b, self.rgbc[2]), flag_bits::IR3_SATURATED);
        let out = [rr, gg, bb, self.rgbc[3]];
        self.push_rgb(out);
        out
    }

    /// `DPCS` - depth-cued color, single. Blend RGBC toward far_color
    /// using IR0 - no IR multiplication. Pushes the result onto the
    /// RGB FIFO.
    pub fn dpcs(&mut self) -> [u8; 4] {
        self.begin_op(CopOp::Dpcs);
        let r_in = (self.rgbc[0] as i64) << 4;
        let g_in = (self.rgbc[1] as i64) << 4;
        let b_in = (self.rgbc[2] as i64) << 4;
        let blend = |fc_n: i64, in_n: i64, ir0: i64| -> i64 {
            let scaled = ((fc_n - in_n) * ir0) >> ROT_FRAC_BITS;
            in_n + scaled
        };
        let r = blend((self.far_color.x as i64) << 4, r_in, self.ir0 as i64) >> 4;
        let g = blend((self.far_color.y as i64) << 4, g_in, self.ir0 as i64) >> 4;
        let b = blend((self.far_color.z as i64) << 4, b_in, self.ir0 as i64) >> 4;
        let rr = self.saturate_rgb_u8(r, flag_bits::IR1_SATURATED);
        let gg = self.saturate_rgb_u8(g, flag_bits::IR2_SATURATED);
        let bb = self.saturate_rgb_u8(b, flag_bits::IR3_SATURATED);
        let out = [rr, gg, bb, self.rgbc[3]];
        self.push_rgb(out);
        out
    }

    /// `DPCT` - depth-cued color, triple. Apply DPCS to RGB0/RGB1/RGB2
    /// in the FIFO. The retail engine uses this to fade the output of a
    /// previous lighting pass toward the far-color.
    pub fn dpct(&mut self) -> [[u8; 4]; 3] {
        self.begin_op(CopOp::Dpct);
        let mut out = [[0u8; 4]; 3];
        for (i, slot) in out.iter_mut().enumerate() {
            let saved_rgbc = self.rgbc;
            self.rgbc = self.rgb_fifo[i];
            *slot = self.dpcs();
            self.rgbc = saved_rgbc;
        }
        out
    }

    /// `INTPL` - interpolate vector (V0 toward FC by IR0). Writes
    /// MAC1/MAC2/MAC3 = `IR1 + ((FC - IR) * IR0 >> 12)`; saturates IR1/2/3.
    /// Used by DCPL internally; exposed for engines that want the bare
    /// blend.
    pub fn intpl(&mut self) {
        self.begin_op(CopOp::Intpl);
        let blend = |fc_n: i32, ir_n: i32, ir0: i32| -> i64 {
            let delta = (fc_n - ir_n) as i64;
            let scaled = (delta * ir0 as i64) >> ROT_FRAC_BITS;
            ir_n as i64 + scaled
        };
        self.mac1 = blend(self.far_color.x, self.ir1, self.ir0);
        self.mac2 = blend(self.far_color.y, self.ir2, self.ir0);
        self.mac3 = blend(self.far_color.z, self.ir3, self.ir0);
        self.ir1 = self.saturate_ir(self.mac1, flag_bits::IR1_SATURATED);
        self.ir2 = self.saturate_ir(self.mac2, flag_bits::IR2_SATURATED);
        self.ir3 = self.saturate_ir(self.mac3, flag_bits::IR3_SATURATED);
    }

    /// `SQR` - squares IR1/IR2/IR3 in place. Writes MAC1..MAC3 = IR^2,
    /// then re-saturates IR. Used by some lighting passes that want the
    /// dot of a vector with itself.
    pub fn sqr(&mut self, shift_frac: bool) {
        self.begin_op(CopOp::Sqr);
        let s = |a: i32| -> i64 { (a as i64) * (a as i64) };
        let raw = [s(self.ir1), s(self.ir2), s(self.ir3)];
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
        self.ir1 = self.saturate_ir(macs[0], flag_bits::IR1_SATURATED);
        self.ir2 = self.saturate_ir(macs[1], flag_bits::IR2_SATURATED);
        self.ir3 = self.saturate_ir(macs[2], flag_bits::IR3_SATURATED);
    }

    /// `OP` - outer product. Computes the cross product of `[D1, D2, D3]`
    /// (where `D1..D3` are the diagonal entries of the rotation matrix
    /// in retail GTE) and IR1/IR2/IR3, returning the result in MAC and
    /// IR registers.
    ///
    /// Cross product: `mac = D × IR` =
    ///   - mac1 = D2 * IR3 - D3 * IR2
    ///   - mac2 = D3 * IR1 - D1 * IR3
    ///   - mac3 = D1 * IR2 - D2 * IR1
    pub fn op(&mut self, shift_frac: bool) {
        self.begin_op(CopOp::Op);
        let d1 = self.rot.m[0][0] as i64;
        let d2 = self.rot.m[1][1] as i64;
        let d3 = self.rot.m[2][2] as i64;
        let ir1 = self.ir1 as i64;
        let ir2 = self.ir2 as i64;
        let ir3 = self.ir3 as i64;
        let raw = [
            d2 * ir3 - d3 * ir2,
            d3 * ir1 - d1 * ir3,
            d1 * ir2 - d2 * ir1,
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
        self.ir1 = self.saturate_ir(macs[0], flag_bits::IR1_SATURATED);
        self.ir2 = self.saturate_ir(macs[1], flag_bits::IR2_SATURATED);
        self.ir3 = self.saturate_ir(macs[2], flag_bits::IR3_SATURATED);
    }

    /// `GPF` - general-purpose fixed-point multiply: `MAC = IR * IR0`.
    /// Used for "fixed alpha" blends - engine-shell composes this with
    /// DPCS to fade UI panels.
    pub fn gpf(&mut self, shift_frac: bool) {
        self.begin_op(CopOp::Gpf);
        let ir0 = self.ir0 as i64;
        let raw = [
            (self.ir1 as i64) * ir0,
            (self.ir2 as i64) * ir0,
            (self.ir3 as i64) * ir0,
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
        self.ir1 = self.saturate_ir(macs[0], flag_bits::IR1_SATURATED);
        self.ir2 = self.saturate_ir(macs[1], flag_bits::IR2_SATURATED);
        self.ir3 = self.saturate_ir(macs[2], flag_bits::IR3_SATURATED);
    }

    /// `NCS` - normal-color (single, no shading, no depth blend). Runs the
    /// light pass against V0 and pushes the resulting `(R, G, B, code)` onto
    /// the RGB FIFO without depth-cueing. Used for fully-lit primitives that
    /// shouldn't fade with distance.
    pub fn ncs(&mut self) -> [u8; 4] {
        self.begin_op(CopOp::Ncs);
        self.ncs_inner(self.v[0])
    }

    /// `NCT` - normal-color (triple). Apply NCS to V0/V1/V2 in order.
    pub fn nct(&mut self) -> [[u8; 4]; 3] {
        self.begin_op(CopOp::Nct);
        let v = self.v;
        let r0 = self.ncs_inner(v[0]);
        let r1 = self.ncs_inner(v[1]);
        let r2 = self.ncs_inner(v[2]);
        [r0, r1, r2]
    }

    fn ncs_inner(&mut self, normal: GteVec3) -> [u8; 4] {
        self.light_pass(normal);
        // Modulate by RGBC. (IR_n * RGBC_n) >> 4 fits the GTE's 12.4 layout.
        let modulate = |ir: i32, mat: u8| -> i64 { (ir as i64 * mat as i64) >> 4 };
        let r = self.saturate_rgb_u8(modulate(self.ir1, self.rgbc[0]), flag_bits::IR1_SATURATED);
        let g = self.saturate_rgb_u8(modulate(self.ir2, self.rgbc[1]), flag_bits::IR2_SATURATED);
        let b = self.saturate_rgb_u8(modulate(self.ir3, self.rgbc[2]), flag_bits::IR3_SATURATED);
        let out = [r, g, b, self.rgbc[3]];
        self.push_rgb(out);
        out
    }

    /// `NCCS` - normal-color color (single, no depth fade). Runs the light
    /// pass like NCS but threads the result through the light-color matrix
    /// once more (which `light_pass` already does), giving a per-vertex
    /// material × light × color modulated RGB.
    ///
    /// In hardware NCCS additionally modulates by the input RGBC after the
    /// second light-color pass - the practical effect for the engine is the
    /// same RGB stream as NCS but pre-multiplied by the light-color matrix
    /// during `light_pass`. Surfaces this as a distinct entry point so
    /// engines can branch on the captured opcode byte.
    pub fn nccs(&mut self) -> [u8; 4] {
        self.begin_op(CopOp::Nccs);
        self.nccs_inner(self.v[0])
    }

    /// `NCCT` - normal-color color (triple). Apply NCCS to V0/V1/V2.
    pub fn ncct(&mut self) -> [[u8; 4]; 3] {
        self.begin_op(CopOp::Ncct);
        let v = self.v;
        let r0 = self.nccs_inner(v[0]);
        let r1 = self.nccs_inner(v[1]);
        let r2 = self.nccs_inner(v[2]);
        [r0, r1, r2]
    }

    fn nccs_inner(&mut self, normal: GteVec3) -> [u8; 4] {
        self.light_pass(normal);
        // Second light-color pass: re-modulate by light_color matrix.
        let intensity = GteVec3::new(self.ir1, self.ir2, self.ir3);
        let bc = self.back_color;
        self.mvmva_inner(&self.light_color.clone(), intensity, bc, true, true);
        let modulate = |ir: i32, mat: u8| -> i64 { (ir as i64 * mat as i64) >> 4 };
        let r = self.saturate_rgb_u8(modulate(self.ir1, self.rgbc[0]), flag_bits::IR1_SATURATED);
        let g = self.saturate_rgb_u8(modulate(self.ir2, self.rgbc[1]), flag_bits::IR2_SATURATED);
        let b = self.saturate_rgb_u8(modulate(self.ir3, self.rgbc[2]), flag_bits::IR3_SATURATED);
        let out = [r, g, b, self.rgbc[3]];
        self.push_rgb(out);
        out
    }

    /// `CDP` - color depth-cued (no normal pass). Skips the light pass -
    /// uses the existing IR1/2/3 as the per-component intensity - but runs
    /// the depth-fade blend toward far_color and the RGBC modulation.
    /// Engines call this after a custom IR setup when they want the
    /// distance fade without re-running the light matrix.
    pub fn cdp(&mut self) -> [u8; 4] {
        self.begin_op(CopOp::Cdp);
        let intensity = GteVec3::new(self.ir1, self.ir2, self.ir3);
        let bc = self.back_color;
        self.mvmva_inner(&self.light_color.clone(), intensity, bc, true, true);
        let fc = self.far_color;
        let blend = |fc_n: i32, ir_n: i32, ir0: i32| -> i32 {
            let delta = (fc_n - ir_n) as i64;
            let scaled = (delta * ir0 as i64) >> ROT_FRAC_BITS;
            (ir_n as i64 + scaled).clamp(i16::MIN as i64, i16::MAX as i64) as i32
        };
        let r_blended = blend(fc.x, self.ir1, self.ir0);
        let g_blended = blend(fc.y, self.ir2, self.ir0);
        let b_blended = blend(fc.z, self.ir3, self.ir0);
        let modulate = |ir: i32, mat: u8| -> i64 { (ir as i64 * mat as i64) >> 4 };
        let r = self.saturate_rgb_u8(modulate(r_blended, self.rgbc[0]), flag_bits::IR1_SATURATED);
        let g = self.saturate_rgb_u8(modulate(g_blended, self.rgbc[1]), flag_bits::IR2_SATURATED);
        let b = self.saturate_rgb_u8(modulate(b_blended, self.rgbc[2]), flag_bits::IR3_SATURATED);
        let out = [r, g, b, self.rgbc[3]];
        self.push_rgb(out);
        out
    }

    /// `CC` - color color (no normal, no depth). Just modulates RGBC by
    /// the existing IR1/2/3 through the light-color matrix. Used by some
    /// 2D effects that want the colour modulation primitive without the
    /// rest of the lighting pipeline.
    pub fn cc(&mut self) -> [u8; 4] {
        self.begin_op(CopOp::Cc);
        let intensity = GteVec3::new(self.ir1, self.ir2, self.ir3);
        let bc = self.back_color;
        self.mvmva_inner(&self.light_color.clone(), intensity, bc, true, true);
        let modulate = |ir: i32, mat: u8| -> i64 { (ir as i64 * mat as i64) >> 4 };
        let r = self.saturate_rgb_u8(modulate(self.ir1, self.rgbc[0]), flag_bits::IR1_SATURATED);
        let g = self.saturate_rgb_u8(modulate(self.ir2, self.rgbc[1]), flag_bits::IR2_SATURATED);
        let b = self.saturate_rgb_u8(modulate(self.ir3, self.rgbc[2]), flag_bits::IR3_SATURATED);
        let out = [r, g, b, self.rgbc[3]];
        self.push_rgb(out);
        out
    }

    /// `GPL` - general-purpose load: `MAC += IR * IR0`. Accumulating
    /// counterpart to GPF - used to chain blend operations.
    pub fn gpl(&mut self, shift_frac: bool) {
        self.begin_op(CopOp::Gpl);
        let ir0 = self.ir0 as i64;
        let raw = [
            (self.ir1 as i64) * ir0,
            (self.ir2 as i64) * ir0,
            (self.ir3 as i64) * ir0,
        ];
        let increments: [i64; 3] = if shift_frac {
            [
                raw[0] >> ROT_FRAC_BITS,
                raw[1] >> ROT_FRAC_BITS,
                raw[2] >> ROT_FRAC_BITS,
            ]
        } else {
            raw
        };
        self.mac1 = self.mac1.saturating_add(increments[0]);
        self.mac2 = self.mac2.saturating_add(increments[1]);
        self.mac3 = self.mac3.saturating_add(increments[2]);
        self.ir1 = self.saturate_ir(self.mac1, flag_bits::IR1_SATURATED);
        self.ir2 = self.saturate_ir(self.mac2, flag_bits::IR2_SATURATED);
        self.ir3 = self.saturate_ir(self.mac3, flag_bits::IR3_SATURATED);
    }
}

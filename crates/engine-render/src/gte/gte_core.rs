use super::*;

/// PSX cop2 (GTE) register-state emulator.
///
/// The shape mirrors the hardware: the GTE has 32 data registers and 32
/// control registers. Data registers hold the working accumulators (MAC0,
/// MAC1, MAC2, MAC3), the truncated/rounded short results (IR0, IR1, IR2,
/// IR3), the screen XY FIFO (SXY0/SXY1/SXY2/SXYP), the screen Z FIFO
/// (SZ0/SZ1/SZ2/SZ3), the RGB FIFO (RGB0/RGB1/RGB2), per-vertex inputs
/// (V0/V1/V2), the average-Z output (OTZ), and the saturation flag (FLAG).
/// Control registers hold the rotation matrix (RT11..RT33), the translation
/// vector (TRX/TRY/TRZ), the projection focal length (H), the screen offset
/// (OFX/OFY), and the average-Z scaling factors (ZSF3/ZSF4).
///
/// This isn't a cycle-accurate emulator - it doesn't model the per-stage
/// pipeline latency or the exact MAC/IR overflow flag bits the hardware
/// produces - but the high-level instruction shape, register file, and
/// saturation behaviour all match the PSX GTE manual. Used by the engine
/// for offline regression checks against captured GTE traces and as the
/// substrate for downstream tooling that wants opcode-level visibility
/// without re-deriving the math.
///
/// Source for the register layout: PSX hardware reference (cop2). The
/// engine's existing [`Camera::transform`] is a higher-level wrapper around
/// the same arithmetic - both produce identical results for the RTPT path.
#[derive(Debug, Clone)]
pub struct Gte {
    // ----- Data registers -----
    /// V0/V1/V2 - three input vertices for batch ops (RTPT, NCDT, COLOR).
    pub v: [GteVec3; 3],
    /// RGBC - the input colour (R/G/B/CODE bytes).
    pub rgbc: [u8; 4],
    /// OTZ - average-Z output (0..=0xFFFF).
    pub otz: u16,
    /// IR0 - scalar accumulator (sign-extended i16).
    pub ir0: i32,
    /// IR1/IR2/IR3 - truncated MAC1/MAC2/MAC3 (i16 saturating).
    pub ir1: i32,
    pub ir2: i32,
    pub ir3: i32,
    /// SXY0/SXY1/SXY2 - screen-XY FIFO (3 entries, oldest at index 0).
    pub sxy_fifo: [ScreenXY; 3],
    /// SZ0/SZ1/SZ2/SZ3 - screen-Z FIFO (4 entries, oldest at index 0).
    pub sz_fifo: [u16; 4],
    /// RGB0/RGB1/RGB2 - output RGB FIFO (3 entries).
    pub rgb_fifo: [[u8; 4]; 3],
    /// MAC0 - 32-bit scalar accumulator.
    pub mac0: i32,
    /// MAC1/MAC2/MAC3 - wide vector accumulator (per-component, i64-widened).
    pub mac1: i64,
    pub mac2: i64,
    pub mac3: i64,
    /// FLAG - saturation flag bits accumulated across the last instruction.
    /// Each bit corresponds to a clamp / overflow event; bit 31 is the
    /// "any error" sticky bit.
    pub flag: u32,

    // ----- Control registers -----
    /// RT11..RT33 - rotation matrix (q3.12).
    pub rot: GteMat3,
    /// TRX/TRY/TRZ - translation vector (q19.12).
    pub trans: GteVec3,
    /// H - projection focal length (q.0).
    pub h: i32,
    /// OFX - screen-X offset (q16.16; we store the integer pixel value).
    pub ofx: i32,
    /// OFY - screen-Y offset (q16.16).
    pub ofy: i32,
    /// ZSF3 - average-Z scale factor for AVSZ3.
    pub zsf3: i32,
    /// ZSF4 - average-Z scale factor for AVSZ4.
    pub zsf4: i32,
    /// DQA - depth-cue interpolation slope.
    pub dqa: i32,
    /// DQB - depth-cue interpolation intercept.
    pub dqb: i32,
    /// L11..L33 - light source matrix (q3.12). Used by NCDS / NCDT
    /// (normal-color triple) to compute per-vertex light intensity from
    /// the surface normal.
    pub light: GteMat3,
    /// LR1..LB3 - light color matrix (q3.12). Maps light intensity to
    /// the actor's RGB material colour.
    pub light_color: GteMat3,
    /// RBK / GBK / BBK - back-color (q3.12). Ambient bias added before
    /// modulating by RGBC.
    pub back_color: GteVec3,
    /// RFC / GFC / BFC - far-color (q3.12). Distance-fade target colour
    /// blended by DPCS / DCPL / DPCT.
    pub far_color: GteVec3,

    /// Accumulated cop2 cycle count since `reset_cycles` (or
    /// construction). Each instruction adds its [`CopOp::cycles`] entry.
    /// Engines that pace MIPS execution against cop2 stalls read this
    /// after each batch of ops.
    pub cycles: u64,

    /// `LZCS` source register (cop2cr30). Reading `LZCR` (cop2cr31) returns
    /// the leading-zero / leading-one count of this value, depending on
    /// its sign. Writes via MTC2 / LWC2 cache the new source.
    pub lzcs: i32,
    /// `RES1` (cop2cr23) - reserved register on hardware. Some retail GTE
    /// traces stash a temporary here; the emulator passes the value through
    /// without interpreting it.
    pub res1: u32,
}

/// PSX cop2 instruction set - symbolic identifiers used by the cycle
/// counter and any disassembly tooling. Matches the public hardware
/// instruction list (Nocash PSX spec).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopOp {
    Rtps,
    Nclip,
    Op,
    Dpcs,
    Intpl,
    Mvmva,
    Ncds,
    Cdp,
    Ncdt,
    Nccs,
    Cc,
    Ncs,
    Nct,
    Sqr,
    Dcpl,
    Dpct,
    Avsz3,
    Avsz4,
    Rtpt,
    Gpf,
    Gpl,
    Ncct,
}

impl CopOp {
    /// Cycle count consumed by this cop2 operation on the retail GTE
    /// (Nocash PSX hardware reference). Engines that pace MIPS execution
    /// against cop2 stalls accumulate these per emitted op.
    ///
    /// These are the un-pipelined cycle counts - actual pipeline overlap
    /// with neighbouring MIPS instructions can hide some of the latency,
    /// but the worst-case ceiling is what callers usually need for budget
    /// math.
    pub const fn cycles(self) -> u32 {
        match self {
            CopOp::Rtps => 15,
            CopOp::Nclip => 8,
            CopOp::Op => 6,
            CopOp::Dpcs => 8,
            CopOp::Intpl => 8,
            CopOp::Mvmva => 8,
            CopOp::Ncds => 19,
            CopOp::Cdp => 13,
            CopOp::Ncdt => 44,
            CopOp::Nccs => 17,
            CopOp::Cc => 11,
            CopOp::Ncs => 14,
            CopOp::Nct => 30,
            CopOp::Sqr => 5,
            CopOp::Dcpl => 8,
            CopOp::Dpct => 17,
            CopOp::Avsz3 => 5,
            CopOp::Avsz4 => 6,
            CopOp::Rtpt => 23,
            CopOp::Gpf => 5,
            CopOp::Gpl => 5,
            CopOp::Ncct => 39,
        }
    }
}

/// FLAG-register saturation bits the GTE sets after each instruction.
///
/// The hardware puts these at specific bit positions in cop2cr31; this
/// module follows the same layout so a captured FLAG dump can be compared
/// directly. `BIT_ERROR_FLAG` is the sticky "any clamp happened" bit.
pub mod flag_bits {
    /// MAC1 overflowed (positive).
    pub const MAC1_OVERFLOW_POS: u32 = 1 << 30;
    /// MAC2 overflowed (positive).
    pub const MAC2_OVERFLOW_POS: u32 = 1 << 29;
    /// MAC3 overflowed (positive).
    pub const MAC3_OVERFLOW_POS: u32 = 1 << 28;
    /// MAC1 overflowed (negative).
    pub const MAC1_OVERFLOW_NEG: u32 = 1 << 27;
    /// MAC2 overflowed (negative).
    pub const MAC2_OVERFLOW_NEG: u32 = 1 << 26;
    /// MAC3 overflowed (negative).
    pub const MAC3_OVERFLOW_NEG: u32 = 1 << 25;
    /// IR1 saturated to i16.
    pub const IR1_SATURATED: u32 = 1 << 24;
    /// IR2 saturated to i16.
    pub const IR2_SATURATED: u32 = 1 << 23;
    /// IR3 saturated to i16.
    pub const IR3_SATURATED: u32 = 1 << 22;
    /// SX2 saturated to ±0x400 (the GTE clamps SXY2 more tightly than
    /// the i16-wide internal representation; engines that need bit-exact
    /// SX/SY clamping can mask against this bit).
    pub const SX2_SATURATED: u32 = 1 << 14;
    /// SY2 saturated.
    pub const SY2_SATURATED: u32 = 1 << 13;
    /// SZ3 / OTZ saturated.
    pub const SZ3_OTZ_SATURATED: u32 = 1 << 18;
    /// MAC0 overflowed positive.
    pub const MAC0_OVERFLOW_POS: u32 = 1 << 16;
    /// MAC0 overflowed negative.
    pub const MAC0_OVERFLOW_NEG: u32 = 1 << 15;
    /// IR0 saturated.
    pub const IR0_SATURATED: u32 = 1 << 12;
    /// Sticky "any error happened" bit (set when any of the above set).
    pub const ANY_ERROR: u32 = 1 << 31;
}

impl Default for Gte {
    fn default() -> Self {
        Self::new()
    }
}

impl Gte {
    /// Construct a GTE with all registers zeroed and the rotation matrix
    /// at identity. Caller writes RT/TR/H/OFX/OFY through the field accessors
    /// before issuing instructions.
    pub fn new() -> Self {
        Self {
            v: [GteVec3::default(); 3],
            rgbc: [0; 4],
            otz: 0,
            ir0: 0,
            ir1: 0,
            ir2: 0,
            ir3: 0,
            sxy_fifo: [ScreenXY::default(); 3],
            sz_fifo: [0; 4],
            rgb_fifo: [[0; 4]; 3],
            mac0: 0,
            mac1: 0,
            mac2: 0,
            mac3: 0,
            flag: 0,
            rot: GteMat3::IDENTITY,
            trans: GteVec3::default(),
            h: DEFAULT_H,
            ofx: 0,
            ofy: 0,
            zsf3: ROT_ONE,
            zsf4: ROT_ONE,
            dqa: 0,
            dqb: 0,
            light: GteMat3::IDENTITY,
            light_color: GteMat3::IDENTITY,
            back_color: GteVec3::default(),
            far_color: GteVec3::default(),
            cycles: 0,
            lzcs: 0,
            res1: 0,
        }
    }

    /// Reset the cycle accumulator. Engines pacing MIPS execution against
    /// cop2 stalls call this at the start of each frame / batch.
    pub fn reset_cycles(&mut self) {
        self.cycles = 0;
    }

    /// Bump the cycle accumulator by `op`'s cycle count.
    pub fn charge(&mut self, op: CopOp) {
        self.cycles = self.cycles.saturating_add(op.cycles() as u64);
    }

    /// Mirror of [`Camera::for_viewport`] - set up the projection matrices
    /// for a centred 320×240-style viewport.
    pub fn set_viewport(&mut self, width: i32, height: i32) {
        self.ofx = width / 2;
        self.ofy = height / 2;
        self.h = DEFAULT_H;
    }

    /// Reset only the FLAG register. Call before each instruction sequence
    /// to mirror the hardware's per-instruction FLAG semantics.
    pub fn clear_flag(&mut self) {
        self.flag = 0;
    }

    /// Start of every cop2 op: clear FLAG and bump the cycle accumulator.
    /// Internal helper - every public instruction calls this first.
    fn begin_op(&mut self, op: CopOp) {
        self.clear_flag();
        self.charge(op);
    }

    /// Saturate `v` to i16 and update the IR-saturation FLAG bit.
    fn saturate_ir(&mut self, v: i64, sat_bit: u32) -> i32 {
        if v > i16::MAX as i64 {
            self.flag |= sat_bit | flag_bits::ANY_ERROR;
            i16::MAX as i32
        } else if v < i16::MIN as i64 {
            self.flag |= sat_bit | flag_bits::ANY_ERROR;
            i16::MIN as i32
        } else {
            v as i32
        }
    }

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
    fn mvmva_inner(
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

    // ---------------------------------------------------------------------
    // Register-transfer + memory ops.
    //
    // The PSX cop2 (GTE) sits behind four MIPS instructions for moving data
    // between the CPU register file and the cop2 register file:
    //
    //   - MFC2 rt, rd        -- CPU rt ← data register rd
    //   - MTC2 rt, rd        -- data register rd ← CPU rt
    //   - CFC2 rt, rd        -- CPU rt ← control register rd
    //   - CTC2 rt, rd        -- control register rd ← CPU rt
    //
    // …plus two memory ops:
    //
    //   - LWC2 rd, off(base) -- data register rd ← *(base + off)
    //   - SWC2 rd, off(base) -- *(base + off) ← data register rd
    //
    // The retail TMD renderer + lighting pipeline use these heavily - every
    // vertex load is `LWC2 cop2cr0..cop2cr5` (V0/V1/V2 packed pairs), every
    // captured RGB writeback is `SWC2 cop2cr20..22`. Engines that want to
    // replay a captured GTE trace exactly need this transport layer.
    //
    // The data/control register indices match the public cop2 layout
    // (Nocash PSX hardware reference).
    // ---------------------------------------------------------------------

    /// Read one of the 32 cop2 data registers (cop2cr0..cop2cr31).
    /// Returns the raw 32-bit value - the same layout an MFC2 instruction
    /// would observe in the receiving CPU register.
    pub fn read_data(&self, idx: u8) -> u32 {
        match idx & 0x1F {
            0 => pack_i16_lo_hi(self.v[0].x as i16, self.v[0].y as i16),
            1 => sign_extend_i16(self.v[0].z as i16),
            2 => pack_i16_lo_hi(self.v[1].x as i16, self.v[1].y as i16),
            3 => sign_extend_i16(self.v[1].z as i16),
            4 => pack_i16_lo_hi(self.v[2].x as i16, self.v[2].y as i16),
            5 => sign_extend_i16(self.v[2].z as i16),
            6 => u32::from_le_bytes(self.rgbc),
            7 => self.otz as u32,
            8 => sign_extend_i16(self.ir0 as i16),
            9 => sign_extend_i16(self.ir1 as i16),
            10 => sign_extend_i16(self.ir2 as i16),
            11 => sign_extend_i16(self.ir3 as i16),
            12 => pack_i16_lo_hi(self.sxy_fifo[0].x as i16, self.sxy_fifo[0].y as i16),
            13 => pack_i16_lo_hi(self.sxy_fifo[1].x as i16, self.sxy_fifo[1].y as i16),
            14 | 15 => pack_i16_lo_hi(self.sxy_fifo[2].x as i16, self.sxy_fifo[2].y as i16),
            16 => self.sz_fifo[0] as u32,
            17 => self.sz_fifo[1] as u32,
            18 => self.sz_fifo[2] as u32,
            19 => self.sz_fifo[3] as u32,
            20 => u32::from_le_bytes(self.rgb_fifo[0]),
            21 => u32::from_le_bytes(self.rgb_fifo[1]),
            22 => u32::from_le_bytes(self.rgb_fifo[2]),
            23 => self.res1,
            24 => self.mac0 as u32,
            25 => clamp_i32_from_i64(self.mac1) as u32,
            26 => clamp_i32_from_i64(self.mac2) as u32,
            27 => clamp_i32_from_i64(self.mac3) as u32,
            // IRGB / ORGB read the IR1/IR2/IR3 saturation as a 15-bit BGR555
            // packed colour (Nocash PSX cop2cr28/cr29 read shape).
            28 | 29 => packed_irgb(self.ir1, self.ir2, self.ir3),
            // LZCS / LZCR - `LZCS` is the source the next read of LZCR will
            // count leading zeros / ones on. We expose the raw cached value
            // and the count.
            30 => self.lzcs as u32,
            31 => count_leading_same(self.lzcs),
            _ => unreachable!(),
        }
    }

    /// Write one of the 32 cop2 data registers (MTC2 / LWC2 destination).
    /// Most writes mirror straight back into the typed register file; the
    /// SXY FIFO slots advance / push as the hardware does.
    pub fn write_data(&mut self, idx: u8, val: u32) {
        match idx & 0x1F {
            0 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.v[0].x = lo as i32;
                self.v[0].y = hi as i32;
            }
            1 => self.v[0].z = (val as i32 as i16) as i32,
            2 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.v[1].x = lo as i32;
                self.v[1].y = hi as i32;
            }
            3 => self.v[1].z = (val as i32 as i16) as i32,
            4 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.v[2].x = lo as i32;
                self.v[2].y = hi as i32;
            }
            5 => self.v[2].z = (val as i32 as i16) as i32,
            6 => self.rgbc = val.to_le_bytes(),
            7 => self.otz = (val & 0xFFFF) as u16,
            8 => self.ir0 = (val as i32 as i16) as i32,
            9 => self.ir1 = (val as i32 as i16) as i32,
            10 => self.ir2 = (val as i32 as i16) as i32,
            11 => self.ir3 = (val as i32 as i16) as i32,
            12 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.sxy_fifo[0] = ScreenXY::new(lo as i32, hi as i32);
            }
            13 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.sxy_fifo[1] = ScreenXY::new(lo as i32, hi as i32);
            }
            14 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.sxy_fifo[2] = ScreenXY::new(lo as i32, hi as i32);
            }
            // SXYP - write-only "push": SXY0 ← SXY1 ← SXY2 ← new.
            15 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.sxy_fifo[0] = self.sxy_fifo[1];
                self.sxy_fifo[1] = self.sxy_fifo[2];
                self.sxy_fifo[2] = ScreenXY::new(lo as i32, hi as i32);
            }
            16 => self.sz_fifo[0] = (val & 0xFFFF) as u16,
            17 => self.sz_fifo[1] = (val & 0xFFFF) as u16,
            18 => self.sz_fifo[2] = (val & 0xFFFF) as u16,
            19 => self.sz_fifo[3] = (val & 0xFFFF) as u16,
            20 => self.rgb_fifo[0] = val.to_le_bytes(),
            21 => self.rgb_fifo[1] = val.to_le_bytes(),
            22 => self.rgb_fifo[2] = val.to_le_bytes(),
            23 => self.res1 = val,
            24 => self.mac0 = val as i32,
            25 => self.mac1 = val as i32 as i64,
            26 => self.mac2 = val as i32 as i64,
            27 => self.mac3 = val as i32 as i64,
            28 => {
                // IRGB write: unpack 15-bit BGR555 and broadcast to IR1/2/3.
                let r = (val & 0x1F) as i32 * 0x80;
                let g = ((val >> 5) & 0x1F) as i32 * 0x80;
                let b = ((val >> 10) & 0x1F) as i32 * 0x80;
                self.ir1 = r;
                self.ir2 = g;
                self.ir3 = b;
            }
            // ORGB and LZCR are read-only on hardware; ignore writes.
            29 | 31 => {}
            // LZCS write caches the source for the next LZCR read.
            30 => self.lzcs = val as i32,
            _ => unreachable!(),
        }
    }

    /// Read one of the 32 cop2 control registers (cop2cr32..cop2cr63 in
    /// hardware terms, indexed 0..31 here).
    pub fn read_ctrl(&self, idx: u8) -> u32 {
        match idx & 0x1F {
            0 => pack_i16_lo_hi(self.rot.m[0][0], self.rot.m[0][1]),
            1 => pack_i16_lo_hi(self.rot.m[0][2], self.rot.m[1][0]),
            2 => pack_i16_lo_hi(self.rot.m[1][1], self.rot.m[1][2]),
            3 => pack_i16_lo_hi(self.rot.m[2][0], self.rot.m[2][1]),
            4 => sign_extend_i16(self.rot.m[2][2]),
            5 => self.trans.x as u32,
            6 => self.trans.y as u32,
            7 => self.trans.z as u32,
            8 => pack_i16_lo_hi(self.light.m[0][0], self.light.m[0][1]),
            9 => pack_i16_lo_hi(self.light.m[0][2], self.light.m[1][0]),
            10 => pack_i16_lo_hi(self.light.m[1][1], self.light.m[1][2]),
            11 => pack_i16_lo_hi(self.light.m[2][0], self.light.m[2][1]),
            12 => sign_extend_i16(self.light.m[2][2]),
            13 => self.back_color.x as u32,
            14 => self.back_color.y as u32,
            15 => self.back_color.z as u32,
            16 => pack_i16_lo_hi(self.light_color.m[0][0], self.light_color.m[0][1]),
            17 => pack_i16_lo_hi(self.light_color.m[0][2], self.light_color.m[1][0]),
            18 => pack_i16_lo_hi(self.light_color.m[1][1], self.light_color.m[1][2]),
            19 => pack_i16_lo_hi(self.light_color.m[2][0], self.light_color.m[2][1]),
            20 => sign_extend_i16(self.light_color.m[2][2]),
            21 => self.far_color.x as u32,
            22 => self.far_color.y as u32,
            23 => self.far_color.z as u32,
            24 => self.ofx as u32,
            25 => self.ofy as u32,
            26 => (self.h as u32) & 0xFFFF,
            27 => self.dqa as u32,
            28 => self.dqb as u32,
            29 => (self.zsf3 as u32) & 0xFFFF,
            30 => (self.zsf4 as u32) & 0xFFFF,
            31 => self.flag,
            _ => unreachable!(),
        }
    }

    /// Write one of the 32 cop2 control registers (CTC2 / LWC2 destination).
    pub fn write_ctrl(&mut self, idx: u8, val: u32) {
        match idx & 0x1F {
            0 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.rot.m[0][0] = lo;
                self.rot.m[0][1] = hi;
            }
            1 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.rot.m[0][2] = lo;
                self.rot.m[1][0] = hi;
            }
            2 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.rot.m[1][1] = lo;
                self.rot.m[1][2] = hi;
            }
            3 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.rot.m[2][0] = lo;
                self.rot.m[2][1] = hi;
            }
            4 => self.rot.m[2][2] = val as i32 as i16,
            5 => self.trans.x = val as i32,
            6 => self.trans.y = val as i32,
            7 => self.trans.z = val as i32,
            8 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.light.m[0][0] = lo;
                self.light.m[0][1] = hi;
            }
            9 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.light.m[0][2] = lo;
                self.light.m[1][0] = hi;
            }
            10 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.light.m[1][1] = lo;
                self.light.m[1][2] = hi;
            }
            11 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.light.m[2][0] = lo;
                self.light.m[2][1] = hi;
            }
            12 => self.light.m[2][2] = val as i32 as i16,
            13 => self.back_color.x = val as i32,
            14 => self.back_color.y = val as i32,
            15 => self.back_color.z = val as i32,
            16 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.light_color.m[0][0] = lo;
                self.light_color.m[0][1] = hi;
            }
            17 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.light_color.m[0][2] = lo;
                self.light_color.m[1][0] = hi;
            }
            18 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.light_color.m[1][1] = lo;
                self.light_color.m[1][2] = hi;
            }
            19 => {
                let (lo, hi) = unpack_i16_lo_hi(val);
                self.light_color.m[2][0] = lo;
                self.light_color.m[2][1] = hi;
            }
            20 => self.light_color.m[2][2] = val as i32 as i16,
            21 => self.far_color.x = val as i32,
            22 => self.far_color.y = val as i32,
            23 => self.far_color.z = val as i32,
            24 => self.ofx = val as i32,
            25 => self.ofy = val as i32,
            26 => self.h = (val & 0xFFFF) as i32,
            27 => self.dqa = val as i32,
            28 => self.dqb = val as i32,
            29 => self.zsf3 = (val & 0xFFFF) as i16 as i32,
            30 => self.zsf4 = (val & 0xFFFF) as i16 as i32,
            31 => self.flag = val,
            _ => unreachable!(),
        }
    }

    /// `MFC2` - move from cop2 data register `rd` to a returned `u32`. CPU
    /// callers stash the result in their integer register file.
    pub fn mfc2(&mut self, rd: u8) -> u32 {
        // MFC2 has a 1-cycle stall (no GTE op charge); we model it as a
        // single cycle to keep the pacing accumulator monotonic.
        self.cycles = self.cycles.saturating_add(1);
        self.read_data(rd)
    }

    /// `MTC2` - move CPU `val` into cop2 data register `rd`.
    pub fn mtc2(&mut self, rd: u8, val: u32) {
        self.cycles = self.cycles.saturating_add(1);
        self.write_data(rd, val);
    }

    /// `CFC2` - move from cop2 control register `rd`.
    pub fn cfc2(&mut self, rd: u8) -> u32 {
        self.cycles = self.cycles.saturating_add(1);
        self.read_ctrl(rd)
    }

    /// `CTC2` - move CPU `val` into cop2 control register `rd`.
    pub fn ctc2(&mut self, rd: u8, val: u32) {
        self.cycles = self.cycles.saturating_add(1);
        self.write_ctrl(rd, val);
    }

    /// `LWC2 rd, off(base)` - load 32 bits from memory and write into cop2
    /// data register `rd`. The caller supplies a [`Cop2Mem`] for the actual
    /// load - the GTE doesn't model main memory itself.
    ///
    /// The effective address is `base + off` (the `off` is sign-extended to
    /// 32 bits by the MIPS pipeline before the call). The host's memory
    /// implementation is responsible for the alignment guarantee - most
    /// retail traces hit aligned addresses.
    pub fn lwc2<M: Cop2Mem + ?Sized>(&mut self, mem: &mut M, rd: u8, addr: u32) {
        self.cycles = self.cycles.saturating_add(1);
        let val = mem.cop2_load(addr);
        self.write_data(rd, val);
    }

    /// `SWC2 rd, off(base)` - store cop2 data register `rd` into memory.
    pub fn swc2<M: Cop2Mem + ?Sized>(&mut self, mem: &mut M, rd: u8, addr: u32) {
        self.cycles = self.cycles.saturating_add(1);
        let val = self.read_data(rd);
        mem.cop2_store(addr, val);
    }

    /// Bulk load V0/V1/V2 from three consecutive packed vertices at `addr`.
    /// Each vertex is 8 bytes (xy as a packed u32 at +0, z sign-extended in
    /// the next u32 at +4); the helper consumes 24 bytes total. Mirrors the
    /// canonical retail emit:
    ///
    /// ```text
    /// LWC2 0, 0(t0)    # V0.xy
    /// LWC2 1, 4(t0)    # V0.z
    /// LWC2 2, 8(t0)    # V1.xy
    /// LWC2 3, 12(t0)   # V1.z
    /// LWC2 4, 16(t0)   # V2.xy
    /// LWC2 5, 20(t0)   # V2.z
    /// ```
    pub fn load_vertices<M: Cop2Mem + ?Sized>(&mut self, mem: &mut M, addr: u32) {
        for i in 0..3u32 {
            let xy_off = addr + i * 8;
            let z_off = xy_off + 4;
            self.lwc2(mem, (i as u8) * 2, xy_off);
            self.lwc2(mem, (i as u8) * 2 + 1, z_off);
        }
    }
}

/// Memory bridge for the GTE's load / store ops. Engines wire this up to
/// their main-memory implementation (the retail PSX uses 2 MB of physical
/// RAM mirrored to KSEG0/KSEG1; the Rust port can use a simple `Vec<u8>`
/// with bounds-checking, or anything else that produces u32 reads).
///
/// The default impl returns `0` from `cop2_load` and silently drops
/// `cop2_store`; tests that don't need memory can rely on that.
pub trait Cop2Mem {
    fn cop2_load(&mut self, addr: u32) -> u32;
    fn cop2_store(&mut self, addr: u32, val: u32);
}

/// Vec-backed [`Cop2Mem`]. The address is wrapped to the buffer length
/// (PSX RAM mirror), and out-of-bounds reads return zero rather than
/// panicking. Suitable for capturing GTE traces against a recorded RAM
/// snapshot.
pub struct VecMem {
    pub bytes: Vec<u8>,
}

impl VecMem {
    pub fn new(size: usize) -> Self {
        Self {
            bytes: vec![0; size],
        }
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    pub fn write_u32_at(&mut self, addr: u32, val: u32) {
        let a = addr as usize % self.bytes.len().max(1);
        for (i, b) in val.to_le_bytes().iter().enumerate() {
            if a + i < self.bytes.len() {
                self.bytes[a + i] = *b;
            }
        }
    }
}

impl Cop2Mem for VecMem {
    fn cop2_load(&mut self, addr: u32) -> u32 {
        let n = self.bytes.len();
        if n == 0 {
            return 0;
        }
        let a = (addr as usize) % n;
        let mut buf = [0u8; 4];
        for (i, slot) in buf.iter_mut().enumerate() {
            if a + i < n {
                *slot = self.bytes[a + i];
            }
        }
        u32::from_le_bytes(buf)
    }

    fn cop2_store(&mut self, addr: u32, val: u32) {
        self.write_u32_at(addr, val);
    }
}

/// No-op [`Cop2Mem`]. Loads return `0`, stores are dropped. Useful when
/// instantiating a GTE for unit tests that don't exercise LWC2/SWC2.
pub struct NullMem;

impl Cop2Mem for NullMem {
    fn cop2_load(&mut self, _addr: u32) -> u32 {
        0
    }
    fn cop2_store(&mut self, _addr: u32, _val: u32) {}
}

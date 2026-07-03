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
    pub(super) fn begin_op(&mut self, op: CopOp) {
        self.clear_flag();
        self.charge(op);
    }

    /// Saturate `v` to i16 and update the IR-saturation FLAG bit.
    pub(super) fn saturate_ir(&mut self, v: i64, sat_bit: u32) -> i32 {
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
}

//! Clean-room PSX SPU reverb.
//!
//! Faithful register-driven port of the documented hardware reverb network
//! (the SPU "Reverb Formula": per-22050 Hz-step same-side / different-side
//! IIR reflections, a 4-tap comb early-echo, and two all-pass stages, run
//! over a work buffer at the top of SPU RAM). This replaces the earlier
//! single-tap perceptual delay: the wet tail now has the real comb/all-pass
//! colouration the retail modes produce, so Spirit Arts and echo cues sound
//! like the hardware rather than a flat slap-back.
//!
//! The per-mode register sets are the standard libspu reverb-type presets.
//! Those are public PlayStation hardware-reference constants (the same
//! tables every open SPU emulator ships) - hardware documentation, not Sony
//! game data - so they live in the engine, not behind the disc gate.
//!
//! ## Model boundaries
//!
//! - The hardware runs reverb at 22050 Hz with a 39-tap FIR resampler on the
//!   input and output. This port decimates/zero-order-holds across the
//!   44.1 kHz mixer rate instead of running the FIR; the tail's character
//!   comes from the network, the resampler only affects high-frequency
//!   detail.
//! - Output volume (`vLOUT` / `vROUT`) is not part of the mode preset on
//!   real hardware (libspu sets it separately via `SpuSetReverbDepth`). The
//!   engine only tracks the mode byte, so a fixed depth is applied; override
//!   it with [`Reverb::set_output_volume`].
//!
//! Set per-voice routing with [`crate::spu::voice::Voice::set_reverb_send`]
//! (libspu `SpuSetVoiceReverb` analogue) and select the active mode via
//! [`super::Spu::set_reverb_mode`]. See `docs/subsystems/audio.md`.

/// Standard PSX SPU reverb modes. Names + ordering match libspu's
/// `SpuReverbAttr.mode` byte (`SPU_REV_MODE_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReverbMode {
    /// Reverb disabled - voices with `reverb_send` produce no echo.
    Off,
    /// Small room.
    Room,
    /// Studio A (small studio).
    StudioA,
    /// Studio B (medium studio).
    StudioB,
    /// Studio C (large studio).
    StudioC,
    /// Concert hall.
    Hall,
    /// Space echo.
    Space,
    /// Echo (near-infinite feedback).
    Echo,
    /// One-shot delay.
    Delay,
    /// Pipe (half echo).
    Pipe,
}

impl ReverbMode {
    /// Decode from a libspu-style mode byte. Out-of-range values map to
    /// [`ReverbMode::Off`] to keep the engine silent rather than panic.
    pub fn from_byte(b: u8) -> Self {
        match b {
            0 => ReverbMode::Off,
            1 => ReverbMode::Room,
            2 => ReverbMode::StudioA,
            3 => ReverbMode::StudioB,
            4 => ReverbMode::StudioC,
            5 => ReverbMode::Hall,
            6 => ReverbMode::Space,
            7 => ReverbMode::Echo,
            8 => ReverbMode::Delay,
            9 => ReverbMode::Pipe,
            _ => ReverbMode::Off,
        }
    }

    /// The nine non-`Off` reverb modes, in `preset()` index order.
    pub const ALL: [ReverbMode; 9] = [
        ReverbMode::Room,
        ReverbMode::StudioA,
        ReverbMode::StudioB,
        ReverbMode::StudioC,
        ReverbMode::Hall,
        ReverbMode::Space,
        ReverbMode::Echo,
        ReverbMode::Delay,
        ReverbMode::Pipe,
    ];

    /// Identify which standard preset a raw reverb-register block (the 32
    /// SPU registers `0x1F801DC0..0x1F801DFF` in hardware order) corresponds
    /// to. Returns the mode whose preset matches the identity registers
    /// exactly, or `None` if the block matches no standard preset (a
    /// custom/edited reverb or a non-reverb capture). The trailing
    /// `vLIN`/`vRIN` input-volume registers (indices 30/31) are *not* part of
    /// the preset identity - they are set separately by `SpuSetReverbDepth` -
    /// so they are excluded from the comparison.
    pub fn identify(regs: &[u16; 32]) -> Option<ReverbMode> {
        Self::ALL
            .into_iter()
            .find(|m| m.preset().is_some_and(|p| p.regs[..30] == regs[..30]))
    }

    /// Per-register mismatch count to each preset, for diagnosing a near-miss
    /// when [`Self::identify`] returns `None`. Returns `(mode,
    /// mismatched_register_count)` for every standard preset, sorted
    /// closest-first. Compares the 30 identity registers (excludes
    /// `vLIN`/`vRIN`).
    pub fn closest(regs: &[u16; 32]) -> Vec<(ReverbMode, usize)> {
        let mut v: Vec<(ReverbMode, usize)> = Self::ALL
            .into_iter()
            .filter_map(|m| {
                let p = m.preset()?;
                let n = (0..30).filter(|&i| p.regs[i] != regs[i]).count();
                Some((m, n))
            })
            .collect();
        v.sort_by_key(|&(_, n)| n);
        v
    }

    /// Resolve the hardware preset: work-area byte size + the 32 reverb
    /// registers in SPU layout order. `None` for [`ReverbMode::Off`] (the
    /// network is bypassed).
    fn preset(self) -> Option<&'static Preset> {
        let idx = match self {
            ReverbMode::Off => return None,
            ReverbMode::Room => 0,
            ReverbMode::StudioA => 1,
            ReverbMode::StudioB => 2,
            ReverbMode::StudioC => 3,
            ReverbMode::Hall => 4,
            ReverbMode::Space => 5,
            ReverbMode::Echo => 6,
            ReverbMode::Delay => 7,
            ReverbMode::Pipe => 8,
        };
        Some(&PRESETS[idx])
    }
}

/// A reverb preset: SPU-RAM work-area size in bytes + the 32 reverb
/// registers in the hardware's `0x1F801DC0..0x1F801DFE` order (with the two
/// APF displacement registers leading, matching the documented preset rows).
struct Preset {
    /// Work-area size in bytes. `mBASE = 0x80000 - size`.
    size: u32,
    /// `[dAPF1, dAPF2, vIIR, vCOMB1, vCOMB2, vCOMB3, vCOMB4, vWALL, vAPF1,
    /// vAPF2, mLSAME, mRSAME, mLCOMB1, mRCOMB1, mLCOMB2, mRCOMB2, dLSAME,
    /// dRSAME, mLDIFF, mRDIFF, mLCOMB3, mRCOMB3, mLCOMB4, mRCOMB4, dLDIFF,
    /// dRDIFF, mLAPF1, mRAPF1, mLAPF2, mRAPF2, vLIN, vRIN]`. Address-type
    /// registers are in 8-byte units; volume-type registers are signed
    /// Q15 (used `as i16`).
    regs: [u16; 32],
}

// Standard libspu reverb-type presets (public hardware-reference values).
// Order here matches `ReverbMode::preset` indices, NOT the mode byte.
#[rustfmt::skip]
static PRESETS: [Preset; 9] = [
    // Room
    Preset { size: 0x26C0, regs: [
        0x007D, 0x005B, 0x6D80, 0x54B8, 0xBED0, 0x0000, 0x0000, 0xBA80,
        0x5800, 0x5300, 0x04D6, 0x0333, 0x03F0, 0x0227, 0x0374, 0x01EF,
        0x0334, 0x01B5, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
        0x0000, 0x0000, 0x01B4, 0x0136, 0x00B8, 0x005C, 0x8000, 0x8000,
    ]},
    // Studio A (small)
    Preset { size: 0x1F40, regs: [
        0x0033, 0x0025, 0x70F0, 0x4FA8, 0xBCE0, 0x4410, 0xC0F0, 0x9C00,
        0x5280, 0x4EC0, 0x03E4, 0x031B, 0x03A4, 0x02AF, 0x0372, 0x0266,
        0x031C, 0x025D, 0x025C, 0x018E, 0x022F, 0x0135, 0x01D2, 0x00B7,
        0x018F, 0x00B5, 0x00B4, 0x0080, 0x004C, 0x0026, 0x8000, 0x8000,
    ]},
    // Studio B (medium)
    Preset { size: 0x4840, regs: [
        0x00B1, 0x007F, 0x70F0, 0x4FA8, 0xBCE0, 0x4510, 0xBEF0, 0xB4C0,
        0x5280, 0x4EC0, 0x0904, 0x076B, 0x0824, 0x065F, 0x07A2, 0x0616,
        0x076C, 0x05ED, 0x05EC, 0x042E, 0x050F, 0x0305, 0x0462, 0x02B7,
        0x042F, 0x0265, 0x0264, 0x01B2, 0x0100, 0x0080, 0x8000, 0x8000,
    ]},
    // Studio C (large)
    Preset { size: 0x6FE0, regs: [
        0x00E3, 0x00A9, 0x6F60, 0x4FA8, 0xBCE0, 0x4510, 0xBEF0, 0xA680,
        0x5680, 0x52C0, 0x0DFB, 0x0B58, 0x0D09, 0x0A3C, 0x0BD9, 0x0973,
        0x0B59, 0x08DA, 0x08D9, 0x05E9, 0x07EC, 0x04B0, 0x06EF, 0x03D2,
        0x05EA, 0x031D, 0x031C, 0x0238, 0x0154, 0x00AA, 0x8000, 0x8000,
    ]},
    // Hall
    Preset { size: 0xADE0, regs: [
        0x01A5, 0x0139, 0x6000, 0x5000, 0x4C00, 0xB800, 0xBC00, 0xC000,
        0x6000, 0x5C00, 0x15BA, 0x11BB, 0x14C2, 0x10BD, 0x11BC, 0x0DC1,
        0x11C0, 0x0DC3, 0x0DC0, 0x09C1, 0x0BC4, 0x07C1, 0x0A00, 0x06CD,
        0x09C2, 0x05C1, 0x05C0, 0x041A, 0x0274, 0x013A, 0x8000, 0x8000,
    ]},
    // Space echo
    Preset { size: 0xF6C0, regs: [
        0x033D, 0x0231, 0x7E00, 0x5000, 0xB400, 0xB000, 0x4C00, 0xB000,
        0x6000, 0x5400, 0x1ED6, 0x1A31, 0x1D14, 0x183B, 0x1BC2, 0x16B2,
        0x1A32, 0x15EF, 0x15EE, 0x1055, 0x1334, 0x0F2D, 0x11F6, 0x0C5D,
        0x1056, 0x0AE1, 0x0AE0, 0x07A2, 0x0464, 0x0232, 0x8000, 0x8000,
    ]},
    // Echo (near-infinite feedback)
    Preset { size: 0x18040, regs: [
        0x0001, 0x0001, 0x7FFF, 0x7FFF, 0x0000, 0x0000, 0x0000, 0x8100,
        0x0000, 0x0000, 0x1FFF, 0x0FFF, 0x1005, 0x0005, 0x0000, 0x0000,
        0x1005, 0x0005, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
        0x0000, 0x0000, 0x1004, 0x1002, 0x0004, 0x0002, 0x8000, 0x8000,
    ]},
    // Delay (one-shot echo)
    Preset { size: 0x18040, regs: [
        0x0001, 0x0001, 0x7FFF, 0x7FFF, 0x0000, 0x0000, 0x0000, 0x0000,
        0x0000, 0x0000, 0x1FFF, 0x0FFF, 0x1005, 0x0005, 0x0000, 0x0000,
        0x1005, 0x0005, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
        0x0000, 0x0000, 0x1004, 0x1002, 0x0004, 0x0002, 0x8000, 0x8000,
    ]},
    // Pipe (half echo)
    Preset { size: 0x3C00, regs: [
        0x0017, 0x0013, 0x70F0, 0x4FA8, 0xBCE0, 0x4510, 0xBEF0, 0x8500,
        0x5F80, 0x54C0, 0x0371, 0x02AF, 0x02E5, 0x01DF, 0x02B0, 0x01D7,
        0x0358, 0x026A, 0x01D6, 0x011E, 0x012D, 0x00B1, 0x011F, 0x0059,
        0x01A0, 0x00E3, 0x0058, 0x0040, 0x0028, 0x0014, 0x8000, 0x8000,
    ]},
];

// Register indices into `Preset::regs`.
const DAPF1: usize = 0;
const DAPF2: usize = 1;
const VIIR: usize = 2;
const VCOMB1: usize = 3;
const VCOMB2: usize = 4;
const VCOMB3: usize = 5;
const VCOMB4: usize = 6;
const VWALL: usize = 7;
const VAPF1: usize = 8;
const VAPF2: usize = 9;
const MLSAME: usize = 10;
const MRSAME: usize = 11;
const MLCOMB1: usize = 12;
const MRCOMB1: usize = 13;
const MLCOMB2: usize = 14;
const MRCOMB2: usize = 15;
const DLSAME: usize = 16;
const DRSAME: usize = 17;
const MLDIFF: usize = 18;
const MRDIFF: usize = 19;
const MLCOMB3: usize = 20;
const MRCOMB3: usize = 21;
const MLCOMB4: usize = 22;
const MRCOMB4: usize = 23;
const DLDIFF: usize = 24;
const DRDIFF: usize = 25;
const MLAPF1: usize = 26;
const MRAPF1: usize = 27;
const MLAPF2: usize = 28;
const MRAPF2: usize = 29;
const VLIN: usize = 30;
const VRIN: usize = 31;

/// Default reverb output volume (`vLOUT`/`vROUT`) when the host hasn't set a
/// depth. Roughly half-scale Q15 - audible without swamping the dry signal.
const DEFAULT_OUTPUT_VOL: i16 = 0x4000;

/// `(a * vol) / 0x8000`, the SPU's reverb multiply. `vol` is signed Q15, so
/// `0x8000` (= -1.0) inverts phase exactly as the hardware does.
#[inline]
fn rmul(a: i32, vol: u16) -> i32 {
    (a * (vol as i16 as i32)) >> 15
}

/// Saturate an accumulator to the SPU's signed 16-bit range.
#[inline]
fn sat(v: i32) -> i16 {
    v.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

/// Faithful PSX reverb processor: a recirculating work buffer driven by the
/// active mode's register set.
#[derive(Clone)]
pub struct Reverb {
    pub mode: ReverbMode,
    /// Work buffer, one `i16` per halfword of the mode's work area. Empty
    /// for [`ReverbMode::Off`].
    buf: Vec<i16>,
    /// Work-area size in halfwords (`buf.len()`), cached for the wrap math.
    size_hw: i32,
    /// Current buffer position, a halfword index in `0..size_hw`.
    cur: i32,
    /// Active register set (resolved preset). Empty for `Off`.
    regs: [u16; 32],
    /// Output volume (`vLOUT`/`vROUT`), signed Q15.
    out_vol_l: i16,
    out_vol_r: i16,
    /// 44.1 kHz -> 22.05 kHz decimation toggle.
    run_phase: bool,
    /// Last 22.05 kHz wet output, held across the in-between 44.1 kHz tick.
    last: (i16, i16),
}

impl std::fmt::Debug for Reverb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reverb")
            .field("mode", &self.mode)
            .field("size_hw", &self.size_hw)
            .field("cur", &self.cur)
            .finish()
    }
}

impl Reverb {
    pub fn new(mode: ReverbMode) -> Self {
        let mut r = Self {
            mode: ReverbMode::Off,
            buf: Vec::new(),
            size_hw: 0,
            cur: 0,
            regs: [0; 32],
            out_vol_l: DEFAULT_OUTPUT_VOL,
            out_vol_r: DEFAULT_OUTPUT_VOL,
            run_phase: false,
            last: (0, 0),
        };
        r.set_mode(mode);
        r
    }

    /// Reconfigure the active mode. The work buffer is resized + cleared and
    /// any in-flight wet tail is dropped (matching the retail
    /// `SpuSetReverbModeParam` -> work-area zero-fill).
    pub fn set_mode(&mut self, mode: ReverbMode) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        self.cur = 0;
        self.run_phase = false;
        self.last = (0, 0);
        match mode.preset() {
            None => {
                self.buf = Vec::new();
                self.size_hw = 0;
                self.regs = [0; 32];
            }
            Some(p) => {
                let size_hw = (p.size / 2) as usize;
                self.buf = vec![0i16; size_hw];
                self.size_hw = size_hw as i32;
                self.regs = p.regs;
            }
        }
    }

    /// Override the reverb output volume (`vLOUT`/`vROUT`, signed Q15). The
    /// engine's mode byte doesn't carry depth; the host can set it from the
    /// retail `REVERB_VOL_L/R` registers if it has them.
    pub fn set_output_volume(&mut self, left: i16, right: i16) {
        self.out_vol_l = left;
        self.out_vol_r = right;
    }

    /// Resolve an absolute buffer index from a halfword offset relative to
    /// the current position, wrapped within the work area.
    #[inline]
    fn idx(&self, off_hw: i32) -> usize {
        (self.cur + off_hw).rem_euclid(self.size_hw) as usize
    }

    /// Register value (8-byte units) as a halfword offset relative to `cur`.
    #[inline]
    fn off(&self, reg: usize) -> i32 {
        self.regs[reg] as i32 * 4
    }

    /// Run one 22.05 kHz reverb step over the work buffer.
    fn step(&mut self, in_l: i16, in_r: i16) -> (i16, i16) {
        let r = self.regs;

        let lin = rmul(in_l as i32, r[VLIN]);
        let rin = rmul(in_r as i32, r[VRIN]);

        // ---- Same-side reflection (L->L, R->R) ----
        let i_mlsame = self.idx(self.off(MLSAME));
        let i_mlsame_m = self.idx(self.off(MLSAME) - 1);
        let i_dlsame = self.idx(self.off(DLSAME));
        let l = sat(rmul(
            lin + rmul(self.buf[i_dlsame] as i32, r[VWALL]) - self.buf[i_mlsame_m] as i32,
            r[VIIR],
        ) + self.buf[i_mlsame_m] as i32);
        self.buf[i_mlsame] = l;

        let i_mrsame = self.idx(self.off(MRSAME));
        let i_mrsame_m = self.idx(self.off(MRSAME) - 1);
        let i_drsame = self.idx(self.off(DRSAME));
        let rr = sat(rmul(
            rin + rmul(self.buf[i_drsame] as i32, r[VWALL]) - self.buf[i_mrsame_m] as i32,
            r[VIIR],
        ) + self.buf[i_mrsame_m] as i32);
        self.buf[i_mrsame] = rr;

        // ---- Different-side reflection (R->L, L->R) ----
        let i_mldiff = self.idx(self.off(MLDIFF));
        let i_mldiff_m = self.idx(self.off(MLDIFF) - 1);
        let i_drdiff = self.idx(self.off(DRDIFF));
        let l = sat(rmul(
            lin + rmul(self.buf[i_drdiff] as i32, r[VWALL]) - self.buf[i_mldiff_m] as i32,
            r[VIIR],
        ) + self.buf[i_mldiff_m] as i32);
        self.buf[i_mldiff] = l;

        let i_mrdiff = self.idx(self.off(MRDIFF));
        let i_mrdiff_m = self.idx(self.off(MRDIFF) - 1);
        let i_dldiff = self.idx(self.off(DLDIFF));
        let rr = sat(rmul(
            rin + rmul(self.buf[i_dldiff] as i32, r[VWALL]) - self.buf[i_mrdiff_m] as i32,
            r[VIIR],
        ) + self.buf[i_mrdiff_m] as i32);
        self.buf[i_mrdiff] = rr;

        // ---- Early echo (comb filters) ----
        let mut lout = rmul(self.buf[self.idx(self.off(MLCOMB1))] as i32, r[VCOMB1])
            + rmul(self.buf[self.idx(self.off(MLCOMB2))] as i32, r[VCOMB2])
            + rmul(self.buf[self.idx(self.off(MLCOMB3))] as i32, r[VCOMB3])
            + rmul(self.buf[self.idx(self.off(MLCOMB4))] as i32, r[VCOMB4]);
        let mut rout = rmul(self.buf[self.idx(self.off(MRCOMB1))] as i32, r[VCOMB1])
            + rmul(self.buf[self.idx(self.off(MRCOMB2))] as i32, r[VCOMB2])
            + rmul(self.buf[self.idx(self.off(MRCOMB3))] as i32, r[VCOMB3])
            + rmul(self.buf[self.idx(self.off(MRCOMB4))] as i32, r[VCOMB4]);

        // ---- All-pass filter 1 ----
        let dapf1 = self.regs[DAPF1] as i32 * 4;
        let i_lapf1_tap = self.idx(self.off(MLAPF1) - dapf1);
        let i_rapf1_tap = self.idx(self.off(MRAPF1) - dapf1);
        let lapf1_in = self.buf[i_lapf1_tap] as i32;
        let rapf1_in = self.buf[i_rapf1_tap] as i32;
        lout -= rmul(lapf1_in, r[VAPF1]);
        rout -= rmul(rapf1_in, r[VAPF1]);
        let i_mlapf1 = self.idx(self.off(MLAPF1));
        let i_mrapf1 = self.idx(self.off(MRAPF1));
        self.buf[i_mlapf1] = sat(lout);
        self.buf[i_mrapf1] = sat(rout);
        lout = rmul(lout, r[VAPF1]) + lapf1_in;
        rout = rmul(rout, r[VAPF1]) + rapf1_in;

        // ---- All-pass filter 2 ----
        let dapf2 = self.regs[DAPF2] as i32 * 4;
        let i_lapf2_tap = self.idx(self.off(MLAPF2) - dapf2);
        let i_rapf2_tap = self.idx(self.off(MRAPF2) - dapf2);
        let lapf2_in = self.buf[i_lapf2_tap] as i32;
        let rapf2_in = self.buf[i_rapf2_tap] as i32;
        lout -= rmul(lapf2_in, r[VAPF2]);
        rout -= rmul(rapf2_in, r[VAPF2]);
        let i_mlapf2 = self.idx(self.off(MLAPF2));
        let i_mrapf2 = self.idx(self.off(MRAPF2));
        self.buf[i_mlapf2] = sat(lout);
        self.buf[i_mrapf2] = sat(rout);
        lout = rmul(lout, r[VAPF2]) + lapf2_in;
        rout = rmul(rout, r[VAPF2]) + rapf2_in;

        // ---- Output ----
        let out_l = sat(rmul(lout, self.out_vol_l as u16));
        let out_r = sat(rmul(rout, self.out_vol_r as u16));

        // Advance the recirculating buffer position.
        self.cur += 1;
        if self.cur >= self.size_hw {
            self.cur = 0;
        }

        (out_l, out_r)
    }

    /// Push one stereo sample of *reverb send* signal into the processor and
    /// pull one stereo sample of *reverb wet* signal out, at the 44.1 kHz
    /// mixer rate. The caller mixes the wet output into the master alongside
    /// the dry signal. The network itself runs at 22.05 kHz; every other
    /// call advances it and the in-between call holds the last output.
    pub fn tick(&mut self, send_l: i16, send_r: i16) -> (i16, i16) {
        if self.size_hw == 0 {
            return (0, 0);
        }
        self.run_phase = !self.run_phase;
        if self.run_phase {
            self.last = self.step(send_l, send_r);
        }
        self.last
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Off mode bypasses the network entirely.
    #[test]
    fn off_mode_is_silent() {
        let mut r = Reverb::new(ReverbMode::Off);
        for _ in 0..1000 {
            assert_eq!(r.tick(0x4000, 0x4000), (0, 0));
        }
    }

    /// Every preset resolves with a work-area size matching `mBASE`.
    #[test]
    fn presets_are_consistent() {
        for p in PRESETS.iter() {
            // Size is a whole number of halfwords and fits under 0x80000.
            assert_eq!(p.size % 2, 0);
            assert!(p.size < 0x80000);
            // mBASE in 8-byte units * 8 reconstructs the work-area base.
            let mbase = (0x80000 - p.size) / 8;
            assert_eq!(mbase * 8, 0x80000 - p.size);
        }
    }

    /// `identify` round-trips every preset's own register block to its mode.
    #[test]
    fn identify_round_trips_every_preset() {
        for m in ReverbMode::ALL {
            let regs = m.preset().unwrap().regs;
            assert_eq!(ReverbMode::identify(&regs), Some(m), "{m:?}");
            // `closest` ranks the true preset first with zero mismatches.
            let (best, n) = ReverbMode::closest(&regs)[0];
            assert_eq!(best, m);
            assert_eq!(n, 0);
        }
    }

    /// A register block that matches no preset is unidentified, and `closest`
    /// still ranks something. (Flip one identity register of Studio C.)
    #[test]
    fn identify_rejects_non_preset_block() {
        let mut regs = ReverbMode::StudioC.preset().unwrap().regs;
        regs[10] ^= 0x1; // perturb mLSAME
        assert_eq!(ReverbMode::identify(&regs), None);
        assert_eq!(ReverbMode::closest(&regs)[0], (ReverbMode::StudioC, 1));
    }

    /// The reverb-register block retail installs (captured from real
    /// mednafen save states across field / battle / summon scenes - all
    /// byte-identical) is the Studio C preset. This pins what the live engine
    /// must select to match retail's global reverb. See the C7-REVERB hunt.
    #[test]
    fn retail_reverb_block_is_studio_c() {
        let retail: [u16; 32] = [
            0x00E3, 0x00A9, 0x6F60, 0x4FA8, 0xBCE0, 0x4510, 0xBEF0, 0xA680, 0x5680, 0x52C0, 0x0DFB,
            0x0B58, 0x0D09, 0x0A3C, 0x0BD9, 0x0973, 0x0B59, 0x08DA, 0x08D9, 0x05E9, 0x07EC, 0x04B0,
            0x06EF, 0x03D2, 0x05EA, 0x031D, 0x031C, 0x0238, 0x0154, 0x00AA, 0x8000, 0x8000,
        ];
        assert_eq!(ReverbMode::identify(&retail), Some(ReverbMode::StudioC));
    }

    /// A sustained send produces a non-zero wet tail that stays inside the
    /// SPU's i16 range (no overflow, no silence).
    #[test]
    fn room_produces_bounded_wet_output() {
        let mut r = Reverb::new(ReverbMode::Room);
        let mut saw_output = false;
        // Run ~1 s of 44.1 kHz ticks with a steady drive.
        for _ in 0..44_100 {
            let (l, rr) = r.tick(0x2000, 0x2000);
            assert!((-0x8000..=0x7FFF).contains(&(l as i32)));
            assert!((-0x8000..=0x7FFF).contains(&(rr as i32)));
            if l != 0 || rr != 0 {
                saw_output = true;
            }
        }
        assert!(saw_output, "Room reverb produced no wet output");
    }

    /// An impulse into Hall leaves a decaying tail: energy long after the
    /// input stops is non-zero but bounded (the network recirculates).
    #[test]
    fn hall_impulse_leaves_a_tail() {
        let mut r = Reverb::new(ReverbMode::Hall);
        // One loud impulse, then silence.
        r.tick(0x7FFF, 0x7FFF);
        let mut peak = 0i32;
        for _ in 0..88_200 {
            let (l, rr) = r.tick(0, 0);
            peak = peak.max((l as i32).abs()).max((rr as i32).abs());
        }
        assert!(peak > 0, "Hall reverb produced no tail after an impulse");
    }

    /// The near-infinite Echo preset must stay bounded under a loud impulse
    /// (saturation, not runaway overflow / panic).
    #[test]
    fn echo_preset_stays_bounded() {
        let mut r = Reverb::new(ReverbMode::Echo);
        r.tick(0x7FFF, -0x7FFF);
        for _ in 0..200_000 {
            let (l, rr) = r.tick(0, 0);
            assert!((-0x8000..=0x7FFF).contains(&(l as i32)));
            assert!((-0x8000..=0x7FFF).contains(&(rr as i32)));
        }
    }

    /// Changing mode resizes + clears the buffer, dropping the old tail.
    #[test]
    fn mode_change_resets_buffer() {
        let mut r = Reverb::new(ReverbMode::Hall);
        for _ in 0..1000 {
            r.tick(0x4000, 0x4000);
        }
        r.set_mode(ReverbMode::Room);
        assert!(r.buf.iter().all(|&s| s == 0));
        assert_eq!(r.cur, 0);
    }

    #[test]
    fn from_byte_matches_known_modes() {
        assert_eq!(ReverbMode::from_byte(0), ReverbMode::Off);
        assert_eq!(ReverbMode::from_byte(1), ReverbMode::Room);
        assert_eq!(ReverbMode::from_byte(7), ReverbMode::Echo);
        assert_eq!(ReverbMode::from_byte(9), ReverbMode::Pipe);
        // Out of range falls back to Off.
        assert_eq!(ReverbMode::from_byte(0xFF), ReverbMode::Off);
    }
}

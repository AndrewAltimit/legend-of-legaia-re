//! The CD controller's XA-ADPCM rate converter: the 37.8 kHz (and 18.9 kHz)
//! decoder output to the SPU's 44.1 kHz CD-input rate.
//!
//! The conversion is done by the CD decoder before the stream reaches the SPU,
//! not by the SPU's per-voice 4-point Gaussian interpolator. It is a fixed
//! 7-for-6 polyphase filter: every six input samples are pushed into a 32-entry
//! ring, and seven outputs are produced from the last 29 ring entries, each
//! output against its own 29-tap coefficient row ([`ZIGZAG_TABLES`]). The rows
//! are a windowed low-pass ("zig-zag") kernel, so the conversion also filters
//! out the band above the source Nyquist that a linear interpolator leaves as
//! imaging.
//!
//! An 18.9 kHz stream goes through the same filter with every sample pushed
//! twice, so it emits fourteen outputs for every six source samples - 44.1 /
//! 18.9 = 7/3.
//!
//! The coefficient rows are hardware behaviour, documented in the nocash
//! PSX specifications ("CDROM XA Audio ADPCM Compression", 25-point Zigzag
//! Interpolation). That page notes the published rows reproduce the hardware
//! with small rounding differences; this module sums each output's products
//! and scales the sum by `1 / 0x8000` once, then clamps to `i16`. The rows sum
//! to about `0x7400`, so the converter passes DC at about `0.906` (-0.86 dB);
//! nothing renormalises that.
//!
//! The six-step counter's power-on value is undefined on hardware (it is the
//! phase at which the extra seventh sample lands); a fresh converter starts it
//! at six, the phase a sector boundary always restores.

/// Number of taps in each coefficient row.
pub const ZIGZAG_TAPS: usize = 29;

/// Ring buffer length (the hardware's 32-entry history).
const RING: usize = 32;

/// The seven coefficient rows, `ZIGZAG_TABLES[k][i - 1]` applied to the ring
/// entry `i` samples behind the write head. Values are the hardware's, as
/// tabulated in the nocash PSX specifications.
pub const ZIGZAG_TABLES: [[i32; ZIGZAG_TAPS]; 7] = [
    [
        0, 0, 0, 0, 0, -0x0002, 0x000A, -0x0022, 0x0041, -0x0054, 0x0034, 0x0009, -0x010A, 0x0400,
        -0x0A78, 0x234C, 0x6794, -0x1780, 0x0BCD, -0x0623, 0x0350, -0x016D, 0x006B, 0x000A,
        -0x0010, 0x0011, -0x0008, 0x0003, -0x0001,
    ],
    [
        0, 0, 0, -0x0002, 0, 0x0003, -0x0013, 0x003C, -0x004B, 0x00A2, -0x00E3, 0x0132, -0x0043,
        -0x0267, 0x0C9D, 0x74BB, -0x11B4, 0x09B8, -0x05BF, 0x0372, -0x01A8, 0x00A6, -0x001B,
        0x0005, 0x0006, -0x0008, 0x0003, -0x0001, 0,
    ],
    [
        0, 0, -0x0001, 0x0003, -0x0002, -0x0005, 0x001F, -0x004A, 0x00B3, -0x0192, 0x02B1, -0x039E,
        0x04F8, -0x05A6, 0x7939, -0x05A6, 0x04F8, -0x039E, 0x02B1, -0x0192, 0x00B3, -0x004A,
        0x001F, -0x0005, -0x0002, 0x0003, -0x0001, 0, 0,
    ],
    [
        0, -0x0001, 0x0003, -0x0008, 0x0006, 0x0005, -0x001B, 0x00A6, -0x01A8, 0x0372, -0x05BF,
        0x09B8, -0x11B4, 0x74BB, 0x0C9D, -0x0267, -0x0043, 0x0132, -0x00E3, 0x00A2, -0x004B,
        0x003C, -0x0013, 0x0003, 0, -0x0002, 0, 0, 0,
    ],
    [
        -0x0001, 0x0003, -0x0008, 0x0011, -0x0010, 0x000A, 0x006B, -0x016D, 0x0350, -0x0623,
        0x0BCD, -0x1780, 0x6794, 0x234C, -0x0A78, 0x0400, -0x010A, 0x0009, 0x0034, -0x0054, 0x0041,
        -0x0022, 0x000A, -0x0001, 0, 0x0001, 0, 0, 0,
    ],
    [
        0x0002, -0x0008, 0x0010, -0x0023, 0x002B, 0x001A, -0x00EB, 0x027B, -0x0548, 0x0AFA,
        -0x16FA, 0x53E0, 0x3C07, -0x1249, 0x080E, -0x0347, 0x015B, -0x0044, -0x0017, 0x0046,
        -0x0023, 0x0011, -0x0005, 0, 0, 0, 0, 0, 0,
    ],
    [
        -0x0005, 0x0011, -0x0023, 0x0046, -0x0017, -0x0044, 0x015B, -0x0347, 0x080E, -0x1249,
        0x3C07, 0x53E0, -0x16FA, 0x0AFA, -0x0548, 0x027B, -0x00EB, 0x001A, 0x002B, -0x0023, 0x0010,
        -0x0008, 0x0002, 0, 0, 0, 0, 0, 0,
    ],
];

/// One channel's converter state: the 32-entry history ring and the write
/// head. The six-step counter is shared across channels and lives in
/// [`ZigZagResampler`].
#[derive(Debug, Clone)]
struct Channel {
    ring: [i16; RING],
}

impl Channel {
    fn new() -> Self {
        Self { ring: [0; RING] }
    }

    /// Output for coefficient row `table` with the write head at `p` (the
    /// slot the next sample will land in).
    fn interpolate(&self, p: usize, table: &[i32; ZIGZAG_TAPS]) -> i16 {
        let mut sum: i64 = 0;
        for (i, &c) in table.iter().enumerate() {
            let s = self.ring[(p + RING - (i + 1)) % RING] as i64;
            sum += s * c as i64;
        }
        (sum >> 15).clamp(i16::MIN as i64, i16::MAX as i64) as i16
    }
}

/// Stereo-shaped zig-zag converter. A mono stream feeds the same sample to
/// both sides.
#[derive(Debug, Clone)]
pub struct ZigZagResampler {
    left: Channel,
    right: Channel,
    /// Write head into both rings.
    p: usize,
    /// Pushes left before the next burst of seven outputs.
    six_step: u8,
    /// Outputs produced by the last burst and not yet taken.
    out: [(i16, i16); 7],
    /// Next index of `out` to hand back; `7` = empty.
    out_pos: usize,
}

impl Default for ZigZagResampler {
    fn default() -> Self {
        Self::new()
    }
}

impl ZigZagResampler {
    /// A converter with a zeroed history and the six-step counter at six.
    pub fn new() -> Self {
        Self {
            left: Channel::new(),
            right: Channel::new(),
            p: 0,
            six_step: 6,
            out: [(0, 0); 7],
            out_pos: 7,
        }
    }

    /// `true` when no converted output is waiting: the caller must
    /// [`Self::push`] more input before [`Self::pop`] yields a sample.
    pub fn needs_input(&self) -> bool {
        self.out_pos >= self.out.len()
    }

    /// Push one input sample pair (at the converter's input rate, 37.8 kHz).
    /// Every sixth push produces a burst of seven 44.1 kHz outputs.
    pub fn push(&mut self, s: (i16, i16)) {
        self.left.ring[self.p % RING] = s.0;
        self.right.ring[self.p % RING] = s.1;
        self.p = (self.p + 1) % RING;
        self.six_step -= 1;
        if self.six_step == 0 {
            self.six_step = 6;
            for (k, table) in ZIGZAG_TABLES.iter().enumerate() {
                self.out[k] = (
                    self.left.interpolate(self.p, table),
                    self.right.interpolate(self.p, table),
                );
            }
            self.out_pos = 0;
        }
    }

    /// Take the next 44.1 kHz output, or `None` when the burst is spent.
    pub fn pop(&mut self) -> Option<(i16, i16)> {
        let v = self.out.get(self.out_pos).copied()?;
        self.out_pos += 1;
        Some(v)
    }
}

/// The converter plus the input-side rule for its two source rates: a
/// 37.8 kHz sample is pushed once, an 18.9 kHz sample twice.
#[derive(Debug, Clone, Default)]
pub struct XaRateConverter {
    zz: ZigZagResampler,
    /// The 18.9 kHz sample still owed its second push.
    owed: Option<(i16, i16)>,
}

impl XaRateConverter {
    /// Produce the next 44.1 kHz output, pulling source frames from `read`
    /// as the converter needs them. `doubled` selects the 18.9 kHz rule.
    /// Returns `None` once `read` runs dry with no output pending.
    pub fn next(
        &mut self,
        doubled: bool,
        mut read: impl FnMut() -> Option<(i16, i16)>,
    ) -> Option<(i16, i16)> {
        loop {
            if let Some(v) = self.zz.pop() {
                return Some(v);
            }
            if let Some(s) = self.owed.take() {
                self.zz.push(s);
                continue;
            }
            let s = read()?;
            self.zz.push(s);
            if doubled {
                self.owed = Some(s);
            }
        }
    }
}

/// `true` for the two source rates the CD decoder's converter handles.
pub fn is_cd_xa_rate(rate: u32) -> bool {
    rate == 37_800 || rate == 18_900
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each row is one phase of a low-pass kernel. The published rows sum to
    /// `0x73E5..=0x741D`, not `0x8000`: the converter passes DC at about
    /// `0.906` (-0.86 dB), and that loss is part of the hardware's level.
    #[test]
    fn every_row_passes_dc_at_the_hardware_level() {
        for (k, row) in ZIGZAG_TABLES.iter().enumerate() {
            let sum: i32 = row.iter().sum();
            assert!((0x73E0..=0x7420).contains(&sum), "row {k} sums to {sum:#x}");
        }
    }

    /// The middle row (the phase that lands on an input sample) is symmetric
    /// about its centre tap, and rows 1 and 3 mirror each other: a
    /// transcription slip in the table breaks one of the two.
    #[test]
    fn rows_have_the_kernel_symmetries() {
        let mid = &ZIGZAG_TABLES[2];
        for d in 0..=14 {
            assert_eq!(mid[14 - d], mid[14 + d], "middle row, tap 15 +/- {d}");
        }
        let (a, b) = (&ZIGZAG_TABLES[1], &ZIGZAG_TABLES[3]);
        for d in 0..=13 {
            assert_eq!(a[15 + d], b[13 - d], "rows 1/3, offset {d}");
        }
    }

    /// Seven outputs per six pushes: one second of 37.8 kHz input is exactly
    /// one second of 44.1 kHz output.
    #[test]
    fn rate_is_seven_for_six() {
        let mut z = ZigZagResampler::new();
        let mut outs = 0usize;
        for _ in 0..37_800 {
            z.push((1000, 1000));
            while z.pop().is_some() {
                outs += 1;
            }
        }
        assert_eq!(outs, 44_100);
    }

    /// A constant input settles to the DC level of the rows (about 0.906 of
    /// the input) once the 29-tap history is full.
    #[test]
    fn constant_input_settles_to_the_dc_level() {
        let mut z = ZigZagResampler::new();
        let mut last = (0, 0);
        for _ in 0..120 {
            z.push((10_000, -10_000));
            while let Some(v) = z.pop() {
                last = v;
            }
        }
        assert!((9_020..=9_090).contains(&last.0), "{last:?}");
        assert!((-9_090..=-9_020).contains(&last.1), "{last:?}");
    }

    /// Amplitude of `freq` in `x` (sampled at `rate`), by a single-bin DFT.
    fn tone_amplitude(x: &[f64], freq: f64, rate: f64) -> f64 {
        let (mut re, mut im) = (0.0, 0.0);
        for (n, &v) in x.iter().enumerate() {
            let ph = 2.0 * std::f64::consts::PI * freq * n as f64 / rate;
            re += v * ph.cos();
            im -= v * ph.sin();
        }
        2.0 * (re * re + im * im).sqrt() / x.len() as f64
    }

    fn tone(freq: f64, n: usize) -> Vec<i16> {
        (0..n)
            .map(|i| {
                (16_000.0 * (2.0 * std::f64::consts::PI * freq * i as f64 / 37_800.0).sin()) as i16
            })
            .collect()
    }

    /// The kernel is a low-pass ahead of the 7/6 step, so it suppresses the
    /// image a 37.8 kHz tone leaves in the 44.1 kHz output. A 15 kHz tone's
    /// image folds to 21.3 kHz; the zig-zag converter leaves it far below the
    /// image a two-point linear interpolator leaves, and passes a 1 kHz tone
    /// at the rows' DC level.
    #[test]
    fn suppresses_the_image_linear_interpolation_leaves() {
        let src = tone(15_000.0, 37_800);
        let mut z = ZigZagResampler::new();
        let mut zz = Vec::new();
        for &s in &src {
            z.push((s, s));
            while let Some((l, _)) = z.pop() {
                zz.push(l as f64);
            }
        }
        let lin: Vec<f64> = (0..44_100)
            .map(|k| {
                let pos = k as f64 * 37_800.0 / 44_100.0;
                let i = pos as usize;
                let a = pos - i as f64;
                let s0 = src[i.min(src.len() - 1)] as f64;
                let s1 = src[(i + 1).min(src.len() - 1)] as f64;
                s0 + (s1 - s0) * a
            })
            .collect();
        let zz = &zz[2_000..];
        let lin = &lin[2_000..];
        let image_zz =
            tone_amplitude(zz, 21_300.0, 44_100.0) / tone_amplitude(zz, 15_000.0, 44_100.0);
        let image_lin =
            tone_amplitude(lin, 21_300.0, 44_100.0) / tone_amplitude(lin, 15_000.0, 44_100.0);
        assert!(
            image_zz < image_lin / 4.0,
            "zig-zag image {image_zz}, linear {image_lin}"
        );

        let src = tone(1_000.0, 37_800);
        let mut z = ZigZagResampler::new();
        let mut out = Vec::new();
        for &s in &src {
            z.push((s, s));
            while let Some((l, _)) = z.pop() {
                out.push(l as f64);
            }
        }
        let pass = tone_amplitude(&out[2_000..], 1_000.0, 44_100.0) / 16_000.0;
        assert!((pass - 0.906).abs() < 0.02, "1 kHz gain {pass}");
    }
}

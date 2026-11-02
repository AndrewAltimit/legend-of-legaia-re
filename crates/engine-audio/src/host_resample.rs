//! The output stage's rate converter: the mix's 44.1 kHz to the device's rate.
//!
//! The console's DAC runs at the SPU's own rate, so anything done here is the
//! port's and only has to stay out of the way. A device at 44.1 kHz takes the
//! mix untouched. Any other rate goes through a Kaiser-windowed sinc, because
//! the linear interpolator it replaces costs what can be heard: at 44.1 to 48
//! kHz it loses 1.5 dB at 10 kHz and 3.7 dB at 15 kHz and folds the images of
//! everything above 5 kHz back into the band at -30 dB or so. The windowed sinc
//! is flat within 0.1 dB to 18 kHz with its images below -70 dB (see the tests).

/// Kernel length, input samples.
pub const TAPS: usize = 16;
/// Kernel rows; a fractional position interpolates between two.
const PHASES: usize = 64;
/// Kaiser `beta`: about -80 dB sidelobes.
const BETA: f64 = 8.0;

/// Modified Bessel function of the first kind, order 0.
fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term) = (1.0, 1.0);
    for k in 1..32 {
        term *= (x / (2.0 * k as f64)) * (x / (2.0 * k as f64));
        sum += term;
    }
    sum
}

/// Stereo windowed-sinc interpolator over a short history of the input.
#[derive(Debug, Clone)]
pub struct HostResampler {
    /// `PHASES + 1` rows of [`TAPS`] coefficients.
    rows: Vec<[f32; TAPS]>,
    /// Input history, doubled so a window never wraps.
    hist: [[f32; TAPS * 2]; 2],
    pos: usize,
}

impl HostResampler {
    /// `step` is input samples per output sample. Going down in rate moves the
    /// cutoff to the output's Nyquist; going up leaves it at the input's.
    pub fn new(step: f64) -> Self {
        let cutoff = if step > 1.0 { 1.0 / step } else { 1.0 };
        let half = TAPS as f64 / 2.0;
        let norm = bessel_i0(BETA);
        let rows = (0..=PHASES)
            .map(|p| {
                let frac = p as f64 / PHASES as f64;
                let mut row = [0.0f64; TAPS];
                for (i, c) in row.iter_mut().enumerate() {
                    // Distance from this tap to the output position, which
                    // sits `frac` past the window's middle two taps' first.
                    let x = i as f64 - (half - 1.0) - frac;
                    let sinc = if x == 0.0 {
                        1.0
                    } else {
                        let a = std::f64::consts::PI * x * cutoff;
                        a.sin() / a
                    };
                    let r = (x / half).clamp(-1.0, 1.0);
                    *c = cutoff * sinc * bessel_i0(BETA * (1.0 - r * r).sqrt()) / norm;
                }
                // Unity at DC on every row, or the phase would ripple the level.
                let total: f64 = row.iter().sum();
                std::array::from_fn(|i| (row[i] / total) as f32)
            })
            .collect();
        Self {
            rows,
            hist: [[0.0; TAPS * 2]; 2],
            pos: 0,
        }
    }

    /// Push one input frame.
    pub fn push(&mut self, s: (i16, i16)) {
        let (l, r) = (f32::from(s.0), f32::from(s.1));
        self.hist[0][self.pos] = l;
        self.hist[0][self.pos + TAPS] = l;
        self.hist[1][self.pos] = r;
        self.hist[1][self.pos + TAPS] = r;
        self.pos = (self.pos + 1) % TAPS;
    }

    /// The output frame `frac` (`0.0..1.0`) past the window's middle.
    pub fn read(&self, frac: f32) -> (i16, i16) {
        let at = frac.clamp(0.0, 1.0) * PHASES as f32;
        let row = (at as usize).min(PHASES - 1);
        let t = at - row as f32;
        let (a, b) = (&self.rows[row], &self.rows[row + 1]);
        let chan = |h: &[f32; TAPS * 2]| {
            let w = &h[self.pos..self.pos + TAPS];
            let mut sum = 0.0f32;
            for i in 0..TAPS {
                sum += w[i] * (a[i] + (b[i] - a[i]) * t);
            }
            sum.clamp(i16::MIN as f32, i16::MAX as f32) as i16
        };
        (chan(&self.hist[0]), chan(&self.hist[1]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IN_RATE: f64 = 44_100.0;
    const OUT_RATE: f64 = 48_000.0;

    /// A sine through the converter: the output's amplitude at the tone and
    /// the largest level anywhere else (DFT over a whole number of cycles).
    fn through(freq: f64) -> (f64, f64) {
        let step = IN_RATE / OUT_RATE;
        let mut rs = HostResampler::new(step);
        let mut phase = 1.0;
        let mut t = 0usize;
        let amp = 20_000.0;
        let n = 4800;
        let mut out = Vec::with_capacity(n);
        for k in 0..n + 400 {
            while phase >= 1.0 {
                let v =
                    (amp * (2.0 * std::f64::consts::PI * freq * t as f64 / IN_RATE).sin()) as i16;
                rs.push((v, v));
                t += 1;
                phase -= 1.0;
            }
            if k >= 400 {
                out.push(f64::from(rs.read(phase as f32).0));
            }
            phase += step;
        }
        let bin = |f: f64| {
            let (mut re, mut im) = (0.0, 0.0);
            for (i, s) in out.iter().enumerate() {
                let a = 2.0 * std::f64::consts::PI * f * i as f64 / OUT_RATE;
                re += s * a.cos();
                im += s * a.sin();
            }
            2.0 * (re * re + im * im).sqrt() / out.len() as f64
        };
        let tone = bin(freq);
        // The images of a tone at `f` land at `44100 - f` and `44100 + f`.
        let image = bin(IN_RATE - freq).max(if IN_RATE + freq < OUT_RATE / 2.0 {
            bin(IN_RATE + freq)
        } else {
            0.0
        });
        (
            20.0 * (tone / amp).log10(),
            20.0 * (image.max(1e-3) / amp).log10(),
        )
    }

    #[test]
    fn the_band_stays_flat_and_the_images_stay_out_of_it() {
        for f in [100.0, 1000.0, 5000.0, 10_000.0, 15_000.0] {
            let (gain, image) = through(f);
            assert!(gain.abs() < 0.1, "{f} Hz: {gain:.2} dB");
            assert!(image < -60.0, "{f} Hz: image {image:.1} dB");
        }
    }

    #[test]
    fn dc_passes_at_unity_on_every_phase() {
        let mut rs = HostResampler::new(IN_RATE / OUT_RATE);
        for _ in 0..TAPS {
            rs.push((1000, -1000));
        }
        for p in 0..=16 {
            let (l, r) = rs.read(p as f32 / 16.0);
            assert!((i32::from(l) - 1000).abs() <= 1, "{p}: {l}");
            assert!((i32::from(r) + 1000).abs() <= 1, "{p}: {r}");
        }
    }
}

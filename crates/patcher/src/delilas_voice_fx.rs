//! Delilas arts-voice processing: the by-ear pitch/formant mapping
//! that re-voices the heroes' Tactical Arts shout banks (XA2/XA4/XA6)
//! toward the mapped sibling, plus the three-way mode the party swap
//! exposes for those banks.
//!
//! The DSP chain is a faithful Rust port of the local "voice lab"
//! dashboard the mapping was tuned on: WSOLA time-stretch, playback
//! resample, one spectral pass doing the cepstral formant warp and the
//! timbre transfer toward the sibling's reference clip, pitch-contour
//! bend, character layers (growl / sub-octave / detune / breath /
//! drive / carrier / attack graft) and the tone chain (tilt shelves,
//! peaking, HP/LP, gain, peak guard). [`DEFAULT_VOICE_MAP`] carries
//! the tuned per-cell parameters; every field is expressed so a future
//! re-tune is a table edit, not a code change.

use crate::delilas_party::Sibling;

/// What the arts shout banks carry after the party swap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArtsVoiceMode {
    /// Leave the retail Vahn / Noa / Gala shouts untouched.
    Original,
    /// Silence the arts shouts; the spliced SPU grunts remain the
    /// audible attack voice.
    Removed,
    /// Re-voice the retail shouts toward the mapped sibling with
    /// [`DEFAULT_VOICE_MAP`] (the tuned default).
    #[default]
    Adjusted,
}

impl std::str::FromStr for ArtsVoiceMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "original" => Ok(Self::Original),
            "removed" => Ok(Self::Removed),
            "adjusted" => Ok(Self::Adjusted),
            other => Err(format!(
                "unknown arts-voice mode {other:?} (expected original, removed or adjusted)"
            )),
        }
    }
}

impl std::fmt::Display for ArtsVoiceMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Original => "original",
            Self::Removed => "removed",
            Self::Adjusted => "adjusted",
        })
    }
}

/// One mapping cell's processing parameters (dashboard knob set).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoiceFx {
    /// Pitch shift in semitones.
    pub pitch: f32,
    /// true = WSOLA keeps duration under the pitch shift; false =
    /// classic resample (pitch and speed coupled).
    pub preserve: bool,
    /// Duration divisor (1.0 = retail length).
    pub speed: f32,
    /// EXTRA spectral-envelope warp in semitones (vocal-tract scale)
    /// on top of what the pitch resample already does.
    pub formant_st: f32,
    /// 0..1 morph of each frame's envelope toward the sibling
    /// reference clip's average envelope.
    pub timbre: f32,
    /// Pitch-contour bend: semitone offset at clip start / end
    /// (variable-rate resample between them).
    pub bend0: f32,
    pub bend1: f32,
    /// Peaking-EQ center / gain (dB), Q = 1.
    pub formant_hz: f32,
    pub formant_db: f32,
    /// High-pass / low-pass cutoffs (Hz).
    pub hp: f32,
    pub lp: f32,
    /// tanh soft-clip rasp, 0..1.
    pub drive: f32,
    /// AM growl depth 0..1 and rate (Hz).
    pub growl: f32,
    pub growl_hz: f32,
    /// Sub-octave layer mix 0..1.
    pub sub: f32,
    /// Envelope-following high-tilt noise mix 0..1.
    pub breath: f32,
    /// Detune-doubling spread in cents (0 = off).
    pub detune: f32,
    /// Milliseconds of the sibling reference clip's own onset grafted
    /// over the processed shout's start (0 = off).
    pub graft_ms: f32,
    /// 0..1 blend of the reference clip tiled under the shout with the
    /// shout's amplitude envelope imposed.
    pub carrier: f32,
    /// Spectral tilt in dB (+ = brighter; opposed shelves 400/3000 Hz).
    pub tilt: f32,
    /// Output gain.
    pub gain: f32,
}

/// Neutral cell: pass-through apart from the shared tone defaults.
pub const VOICE_FX_BASE: VoiceFx = VoiceFx {
    pitch: 0.0,
    preserve: true,
    speed: 1.0,
    formant_st: 0.0,
    timbre: 0.0,
    bend0: 0.0,
    bend1: 0.0,
    formant_hz: 1400.0,
    formant_db: 0.0,
    hp: 60.0,
    lp: 15000.0,
    drive: 0.0,
    growl: 0.0,
    growl_hz: 55.0,
    sub: 0.0,
    breath: 0.0,
    detune: 0.0,
    graft_ms: 0.0,
    carrier: 0.0,
    tilt: 0.0,
    gain: 1.0,
};

/// The tuned default mapping, `[hero slot 0..3][Lu, Gi, Che]`,
/// transcribed verbatim from the by-ear voice-lab session.
pub const DEFAULT_VOICE_MAP: [[VoiceFx; 3]; 3] = [
    // Vahn's shouts (XA2) toward ...
    [
        VoiceFx {
            pitch: 1.5,
            formant_st: -5.0,
            bend0: 6.5,
            bend1: 2.5,
            formant_hz: 2100.0,
            hp: 90.0,
            lp: 15500.0,
            ..VOICE_FX_BASE
        }, // Lu
        VoiceFx {
            pitch: -3.0,
            ..VOICE_FX_BASE
        }, // Gi
        VoiceFx {
            pitch: -6.0,
            ..VOICE_FX_BASE
        }, // Che
    ],
    // Noa's shouts (XA4) toward ...
    [
        VoiceFx {
            pitch: -2.5,
            ..VOICE_FX_BASE
        }, // Lu
        VoiceFx {
            pitch: -8.5,
            ..VOICE_FX_BASE
        }, // Gi
        VoiceFx {
            pitch: -11.0,
            formant_st: -5.0,
            timbre: 0.6,
            growl_hz: 50.0,
            ..VOICE_FX_BASE
        }, // Che
    ],
    // Gala's shouts (XA6) toward ...
    [
        VoiceFx {
            pitch: 5.5,
            timbre: 0.1,
            ..VOICE_FX_BASE
        }, // Lu
        VoiceFx {
            pitch: 5.0,
            formant_st: -6.0,
            ..VOICE_FX_BASE
        }, // Gi
        VoiceFx {
            pitch: -2.0,
            ..VOICE_FX_BASE
        }, // Che
    ],
];

/// The tuned cell for hero slot `hero` (0 = Vahn, 1 = Noa, 2 = Gala)
/// mapped to `sibling`.
pub fn voice_map(hero: usize, sibling: Sibling) -> &'static VoiceFx {
    let s = match sibling {
        Sibling::Lu => 0,
        Sibling::Gi => 1,
        Sibling::Che => 2,
    };
    &DEFAULT_VOICE_MAP[hero.min(2)][s]
}

// ---------------------------------------------------------------- DSP

const SPEC_N: usize = 1024;
const SPEC_H: usize = 256;
const SPEC_L: usize = 48;

fn hann(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64).cos())
        .collect()
}

/// In-place iterative radix-2 FFT; `inv` divides by N.
fn fft(re: &mut [f64], im: &mut [f64], inv: bool) {
    let n = re.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = if inv { 2.0 } else { -2.0 } * std::f64::consts::PI / len as f64;
        let (wr, wi) = (ang.cos(), ang.sin());
        let mut i = 0;
        while i < n {
            let (mut cr, mut ci) = (1.0f64, 0.0f64);
            for k in 0..len / 2 {
                let (ur, ui) = (re[i + k], im[i + k]);
                let vr = re[i + k + len / 2] * cr - im[i + k + len / 2] * ci;
                let vi = re[i + k + len / 2] * ci + im[i + k + len / 2] * cr;
                re[i + k] = ur + vr;
                im[i + k] = ui + vi;
                re[i + k + len / 2] = ur - vr;
                im[i + k + len / 2] = ui - vi;
                let ncr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = ncr;
            }
            i += len;
        }
        len <<= 1;
    }
    if inv {
        let s = 1.0 / n as f64;
        for i in 0..n {
            re[i] *= s;
            im[i] *= s;
        }
    }
}

/// Cepstral log-envelope of one magnitude spectrum (lifter keeps the
/// first [`SPEC_L`] quefrency bins).
fn frame_log_env(mag: &[f64], cr: &mut [f64], ci: &mut [f64], env: &mut [f64]) {
    let n = SPEC_N;
    for k in 0..n {
        cr[k] = (mag[k] + 1e-9).ln();
        ci[k] = 0.0;
    }
    fft(cr, ci, true);
    for k in SPEC_L + 1..n - SPEC_L {
        cr[k] = 0.0;
        ci[k] = 0.0;
    }
    fft(cr, ci, false);
    env.copy_from_slice(cr);
}

/// WSOLA time-stretch: `factor` > 1 lengthens; pitch preserved.
fn wsola(x: &[f32], factor: f64) -> Vec<f32> {
    if (factor - 1.0).abs() < 1e-3 || x.len() < SPEC_N * 2 {
        return x.to_vec();
    }
    let n = SPEC_N;
    let hs = n / 4;
    let ha = hs as f64 / factor;
    let seek = 220usize;
    let ov = 256usize;
    let out_len = (x.len() as f64 * factor).round() as usize;
    let mut y = vec![0.0f64; out_len + n];
    let mut norm = vec![0.0f64; out_len + n];
    let win = hann(n);
    let mut prev = 0usize;
    let mut k = 0usize;
    let mut p_out = 0usize;
    while p_out < out_len {
        let target = (k as f64 * ha).round() as usize;
        let mut best = target.min(x.len().saturating_sub(n));
        if k > 0 {
            let ref_start = prev + hs;
            let mut best_c = f64::NEG_INFINITY;
            let lo = target.saturating_sub(seek);
            let hi = (target + seek).min(x.len().saturating_sub(n));
            let mut s = lo;
            while s <= hi {
                let mut c = 0.0f64;
                let mut i = 0;
                while i < ov {
                    let a = x.get(ref_start + i).copied().unwrap_or(0.0) as f64;
                    let b = x.get(s + i).copied().unwrap_or(0.0) as f64;
                    c += a * b;
                    i += 8;
                }
                if c > best_c {
                    best_c = c;
                    best = s;
                }
                s += 4;
            }
        }
        for i in 0..n {
            let v = x.get(best + i).copied().unwrap_or(0.0) as f64;
            y[p_out + i] += v * win[i];
            norm[p_out + i] += win[i];
        }
        prev = best;
        k += 1;
        p_out += hs;
    }
    (0..out_len)
        .map(|i| {
            if norm[i] > 1e-4 {
                (y[i] / norm[i]) as f32
            } else {
                0.0
            }
        })
        .collect()
}

/// Linear-interp resample producing `len * factor` samples.
fn lin_resample(x: &[f32], factor: f64) -> Vec<f32> {
    let n = ((x.len() as f64 * factor).round() as usize).max(1);
    (0..n)
        .map(|i| {
            let s = i as f64 / factor;
            let k = s as usize;
            let fr = (s - k as f64) as f32;
            let a = x.get(k).copied().unwrap_or(0.0);
            let b = x.get(k + 1).copied().unwrap_or(0.0);
            a * (1.0 - fr) + b * fr
        })
        .collect()
}

/// One spectral pass doing the formant warp (`alpha` = vocal-tract
/// scale) and the timbre transfer toward `ref_env` (log-envelope
/// sampled at this signal's bin frequencies) by `strength`. The
/// timbre correction is zero-meaned so frame loudness survives.
fn spectral_pass(x: &[f32], alpha: f64, ref_env: Option<&[f64]>, strength: f64) -> Vec<f32> {
    let warp = (alpha - 1.0).abs() >= 0.005;
    let morph = ref_env.is_some() && strength > 0.01;
    if !warp && !morph {
        return x.to_vec();
    }
    let n = SPEC_N;
    let win = hann(n);
    let mut out = vec![0.0f64; x.len() + n];
    let mut norm = vec![0.0f64; x.len() + n];
    let (mut re, mut im) = (vec![0.0f64; n], vec![0.0f64; n]);
    let (mut cr, mut ci) = (vec![0.0f64; n], vec![0.0f64; n]);
    let mut mag = vec![0.0f64; n];
    let mut env = vec![0.0f64; n];
    let mut corr = vec![0.0f64; n];
    let mut p = 0usize;
    while p < x.len() {
        for i in 0..n {
            re[i] = x.get(p + i).copied().unwrap_or(0.0) as f64 * win[i];
            im[i] = 0.0;
        }
        fft(&mut re, &mut im, false);
        for k in 0..n {
            mag[k] = re[k].hypot(im[k]);
        }
        frame_log_env(&mag, &mut cr, &mut ci, &mut env);
        let mut mean = 0.0f64;
        for k in 0..=n / 2 {
            let mut c = 0.0f64;
            if warp {
                let s = k as f64 / alpha;
                let k0 = s as usize;
                let fr = s - k0 as f64;
                let e = if k0 >= n / 2 {
                    env[n / 2]
                } else {
                    env[k0] * (1.0 - fr) + env[k0 + 1] * fr
                };
                c += e - env[k];
            }
            if let Some(r) = ref_env
                && morph
            {
                c += strength * (r[k] - env[k]);
            }
            corr[k] = c;
            mean += c;
        }
        mean /= (n / 2 + 1) as f64;
        for k in 0..=n / 2 {
            let mut c = corr[k] - if morph { mean } else { 0.0 };
            c = c.clamp(-3.5, 3.5);
            corr[k] = c;
            if k > 0 && k < n / 2 {
                corr[n - k] = c;
            }
        }
        for k in 0..n {
            let m = mag[k] * corr[k].exp();
            let ph = im[k].atan2(re[k]);
            re[k] = m * ph.cos();
            im[k] = m * ph.sin();
        }
        fft(&mut re, &mut im, true);
        for i in 0..n {
            out[p + i] += re[i] * win[i];
            norm[p + i] += win[i] * win[i];
        }
        p += SPEC_H;
    }
    (0..x.len())
        .map(|i| {
            if norm[i] > 1e-6 {
                (out[i] / norm[i]) as f32
            } else {
                x[i]
            }
        })
        .collect()
}

/// Energy-weighted average log-envelope of a reference clip at its own
/// bin frequencies (`[0..=N/2]` meaningful).
fn ref_log_env_raw(pcm: &[i16]) -> Vec<f64> {
    let n = SPEC_N;
    let win = hann(n);
    let (mut re, mut im) = (vec![0.0f64; n], vec![0.0f64; n]);
    let (mut cr, mut ci) = (vec![0.0f64; n], vec![0.0f64; n]);
    let mut mag = vec![0.0f64; n];
    let mut env = vec![0.0f64; n];
    let mut acc = vec![0.0f64; n / 2 + 1];
    let mut wsum = 0.0f64;
    let mut p = 0usize;
    while p < pcm.len() {
        let mut en = 0.0f64;
        for i in 0..n {
            let v = pcm.get(p + i).copied().unwrap_or(0) as f64 / 32768.0 * win[i];
            re[i] = v;
            im[i] = 0.0;
            en += v * v;
        }
        if en >= 1e-4 {
            fft(&mut re, &mut im, false);
            for k in 0..n {
                mag[k] = re[k].hypot(im[k]);
            }
            frame_log_env(&mag, &mut cr, &mut ci, &mut env);
            let w = en.sqrt();
            for (a, e) in acc.iter_mut().zip(env.iter()) {
                *a += e * w;
            }
            wsum += w;
        }
        p += SPEC_H;
    }
    if wsum > 0.0 {
        for a in acc.iter_mut() {
            *a /= wsum;
        }
    }
    acc
}

/// Map a raw reference envelope onto this signal's bins: bin `k` at
/// sample rate `sr` sits at `k*sr/N` Hz.
fn map_ref_env(raw: &[f64], sr: u32, ref_sr: u32) -> Vec<f64> {
    let n = SPEC_N;
    (0..=n / 2)
        .map(|k| {
            let s = k as f64 * sr as f64 / ref_sr as f64;
            let k0 = s as usize;
            let fr = s - k0 as f64;
            if k0 >= n / 2 {
                raw[n / 2]
            } else {
                raw[k0] * (1.0 - fr) + raw[(k0 + 1).min(n / 2)] * fr
            }
        })
        .collect()
}

/// Variable-rate resample: semitone offset glides `s0` -> `s1` across
/// the clip (pitch-contour bend; locally couples pitch and time).
fn pitch_bend(x: &[f32], s0: f32, s1: f32) -> Vec<f32> {
    if s0.abs() < 0.05 && s1.abs() < 0.05 {
        return x.to_vec();
    }
    let mut out = Vec::with_capacity(x.len());
    let mut pos = 0.0f64;
    while pos < (x.len() - 1) as f64 {
        let t = pos / x.len() as f64;
        let f = ((s0 as f64 + (s1 as f64 - s0 as f64) * t) / 12.0).exp2();
        let k = pos as usize;
        let fr = (pos - k as f64) as f32;
        out.push(x[k] * (1.0 - fr) + x.get(k + 1).copied().unwrap_or(0.0) * fr);
        pos += f;
    }
    out
}

/// RBJ biquad, direct form 1.
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Biquad {
    fn new(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }
    fn highpass(f: f64, sr: f64, q: f64) -> Self {
        let w = 2.0 * std::f64::consts::PI * (f / sr).clamp(1e-4, 0.49);
        let (cw, sw) = (w.cos(), w.sin());
        let al = sw / (2.0 * q);
        Self::new(
            (1.0 + cw) / 2.0,
            -(1.0 + cw),
            (1.0 + cw) / 2.0,
            1.0 + al,
            -2.0 * cw,
            1.0 - al,
        )
    }
    fn lowpass(f: f64, sr: f64, q: f64) -> Self {
        let w = 2.0 * std::f64::consts::PI * (f / sr).clamp(1e-4, 0.49);
        let (cw, sw) = (w.cos(), w.sin());
        let al = sw / (2.0 * q);
        Self::new(
            (1.0 - cw) / 2.0,
            1.0 - cw,
            (1.0 - cw) / 2.0,
            1.0 + al,
            -2.0 * cw,
            1.0 - al,
        )
    }
    fn peaking(f: f64, sr: f64, q: f64, db: f64) -> Self {
        let a = 10.0f64.powf(db / 40.0);
        let w = 2.0 * std::f64::consts::PI * (f / sr).clamp(1e-4, 0.49);
        let (cw, sw) = (w.cos(), w.sin());
        let al = sw / (2.0 * q);
        Self::new(
            1.0 + al * a,
            -2.0 * cw,
            1.0 - al * a,
            1.0 + al / a,
            -2.0 * cw,
            1.0 - al / a,
        )
    }
    fn shelf(f: f64, sr: f64, db: f64, low: bool) -> Self {
        let a = 10.0f64.powf(db / 40.0);
        let w = 2.0 * std::f64::consts::PI * (f / sr).clamp(1e-4, 0.49);
        let (cw, sw) = (w.cos(), w.sin());
        let s = 1.0f64;
        let al = sw / 2.0 * ((a + 1.0 / a) * (1.0 / s - 1.0) + 2.0).sqrt();
        let two_sqrt_a_al = 2.0 * a.sqrt() * al;
        if low {
            Self::new(
                a * ((a + 1.0) - (a - 1.0) * cw + two_sqrt_a_al),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cw),
                a * ((a + 1.0) - (a - 1.0) * cw - two_sqrt_a_al),
                (a + 1.0) + (a - 1.0) * cw + two_sqrt_a_al,
                -2.0 * ((a - 1.0) + (a + 1.0) * cw),
                (a + 1.0) + (a - 1.0) * cw - two_sqrt_a_al,
            )
        } else {
            Self::new(
                a * ((a + 1.0) + (a - 1.0) * cw + two_sqrt_a_al),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cw),
                a * ((a + 1.0) + (a - 1.0) * cw - two_sqrt_a_al),
                (a + 1.0) - (a - 1.0) * cw + two_sqrt_a_al,
                2.0 * ((a - 1.0) - (a + 1.0) * cw),
                (a + 1.0) - (a - 1.0) * cw - two_sqrt_a_al,
            )
        }
    }
    fn run(&mut self, x: &mut [f32]) {
        for v in x.iter_mut() {
            let x0 = *v as f64;
            let y0 = self.b0 * x0 + self.b1 * self.x1 + self.b2 * self.x2
                - self.a1 * self.y1
                - self.a2 * self.y2;
            self.x2 = self.x1;
            self.x1 = x0;
            self.y2 = self.y1;
            self.y1 = y0;
            *v = y0 as f32;
        }
    }
}

/// Process one voiced clip (mono i16 at `rate`) through the full
/// dashboard chain. `reference` is the sibling's reference clip
/// (their longest grunt - the dashboard's default reference pick),
/// used by the timbre / carrier / graft stages. Deterministic: the
/// breath noise runs on a fixed-seed [`SplitMix64`](crate::rng::SplitMix64).
pub fn process_clip(
    pcm: &[i16],
    rate: u32,
    fx: &VoiceFx,
    reference: Option<(&[i16], u32)>,
) -> Vec<i16> {
    let x: Vec<f32> = pcm.iter().map(|&s| s as f32 / 32768.0).collect();
    let r = (fx.pitch as f64 / 12.0).exp2();
    let speed = if fx.speed > 0.0 { fx.speed as f64 } else { 1.0 };
    let (stretch, play) = if fx.preserve {
        (r / speed, r)
    } else {
        (1.0, r * speed)
    };
    let mut y = wsola(&x, stretch);
    y = lin_resample(&y, 1.0 / play);
    // spectral: formant warp + timbre transfer (post-resample domain,
    // so bin k sits at its final perceived frequency)
    let alpha = (fx.formant_st as f64 / 12.0).exp2();
    let ref_env = if fx.timbre > 0.01 {
        reference.map(|(rp, rr)| map_ref_env(&ref_log_env_raw(rp), rate, rr))
    } else {
        None
    };
    y = spectral_pass(&y, alpha, ref_env.as_deref(), fx.timbre as f64);
    y = pitch_bend(&y, fx.bend0, fx.bend1);
    if fx.sub > 0.01 {
        // -12st same-duration layer: WSOLA to half length, upsample x2
        let sub = lin_resample(&wsola(&y, 0.5), 2.0);
        let dry = 1.0 - fx.sub * 0.4;
        for (i, v) in y.iter_mut().enumerate() {
            *v = *v * dry + sub.get(i).copied().unwrap_or(0.0) * fx.sub * 0.8;
        }
    }
    if fx.growl > 0.01 {
        let w = 2.0 * std::f64::consts::PI * fx.growl_hz as f64 / rate as f64;
        for (i, v) in y.iter_mut().enumerate() {
            *v *= 1.0 - fx.growl * 0.5 * (0.5 + 0.5 * (w * i as f64).sin() as f32);
        }
    }
    if fx.detune > 1.0 {
        let c = (fx.detune as f64 / 1200.0).exp2();
        let up = lin_resample(&y, 1.0 / c);
        let dn = lin_resample(&y, c);
        for (i, v) in y.iter_mut().enumerate() {
            *v = *v * 0.6
                + up.get(i).copied().unwrap_or(0.0) * 0.35
                + dn.get(i).copied().unwrap_or(0.0) * 0.35;
        }
    }
    if fx.breath > 0.01 {
        let mut rng = crate::rng::SplitMix64::new(0x5EED_B4EA_7B0C_E501);
        let a = (-1.0f32 / (0.005 * rate as f32)).exp();
        let (mut env, mut n1) = (0.0f32, 0.0f32);
        for v in y.iter_mut() {
            let ab = v.abs();
            env = if ab > env { ab } else { env * a };
            let w = (rng.next_u64() >> 40) as f32 / (1u32 << 23) as f32 * 2.0 - 1.0;
            let hpn = w - n1;
            n1 = w * 0.7;
            *v = *v * (1.0 - fx.breath * 0.3) + hpn * env * fx.breath * 0.8;
        }
    }
    if fx.drive > 0.01 {
        let g = 1.0 + fx.drive * 6.0;
        let norm = (g as f64).tanh() as f32;
        for v in y.iter_mut() {
            *v = (*v * g).tanh() / norm;
        }
    }
    if let Some((rp, rr)) = reference {
        if fx.carrier > 0.01 {
            // reference tiled at native pitch, dynamics whitened, the
            // shout's amplitude envelope imposed, blended underneath
            let mut g: Vec<f32> = lin_resample(
                &rp.iter().map(|&s| s as f32 / 32768.0).collect::<Vec<_>>(),
                rate as f64 / rr as f64,
            );
            let mut s0 = 0usize;
            let mut s1 = g.len();
            while s0 < g.len() && g[s0].abs() < 0.01 {
                s0 += 1;
            }
            while s1 > s0 && g[s1 - 1].abs() < 0.01 {
                s1 -= 1;
            }
            g = g[s0..s1.max(s0 + 64).min(g.len())].to_vec();
            if !g.is_empty() {
                let a = (-1.0f32 / (0.008 * rate as f32)).exp();
                let (mut env_h, mut env_g) = (0.0f32, 0.0f32);
                let mut imposed = vec![0.0f32; y.len()];
                for i in 0..y.len() {
                    let gi = g[i % g.len()];
                    let (ah, ag) = (y[i].abs(), gi.abs());
                    env_h = if ah > env_h { ah } else { env_h * a };
                    env_g = if ag > env_g { ag } else { env_g * a };
                    imposed[i] = gi / (env_g + 0.02) * env_h;
                }
                for (v, im) in y.iter_mut().zip(imposed.iter()) {
                    *v = *v * (1.0 - fx.carrier * 0.8) + im * fx.carrier;
                }
            }
        }
        if fx.graft_ms > 5.0 {
            let g: Vec<f32> = lin_resample(
                &rp.iter().map(|&s| s as f32 / 32768.0).collect::<Vec<_>>(),
                rate as f64 / rr as f64,
            );
            let graft_n = ((fx.graft_ms as f64 / 1000.0 * rate as f64) as usize)
                .min(g.len())
                .min(y.len());
            if graft_n > 0 {
                let fade_n = (graft_n as f64 * 0.35) as usize + 32;
                let rms = |a: &[f32]| {
                    let n = a.len().clamp(1, 4096);
                    (a[..n].iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / n as f64)
                        .sqrt()
                        .max(1e-6)
                };
                let gg = (rms(&y) / rms(&g)).min(4.0) as f32;
                for i in 0..graft_n {
                    let t = if i > graft_n.saturating_sub(fade_n) {
                        (graft_n - i) as f32 / fade_n as f32
                    } else {
                        1.0
                    };
                    y[i] = g[i] * gg * t + y[i] * (1.0 - t);
                }
            }
        }
    }
    // tone chain at the final sample rate
    let sr = rate as f64;
    Biquad::highpass(fx.hp as f64, sr, 0.707).run(&mut y);
    Biquad::lowpass(fx.lp as f64, sr, 0.707).run(&mut y);
    if fx.formant_db.abs() > 0.05 {
        Biquad::peaking(fx.formant_hz as f64, sr, 1.0, fx.formant_db as f64).run(&mut y);
    }
    if fx.tilt.abs() > 0.05 {
        Biquad::shelf(400.0, sr, -fx.tilt as f64 / 2.0, true).run(&mut y);
        Biquad::shelf(3000.0, sr, fx.tilt as f64 / 2.0, false).run(&mut y);
    }
    let mut peak = 0.0f32;
    for v in y.iter_mut() {
        *v *= fx.gain;
        peak = peak.max(v.abs());
    }
    if peak > 0.98 {
        let s = 0.98 / peak;
        for v in y.iter_mut() {
            *v *= s;
        }
    }
    y.iter()
        .map(|&v| (v * 32767.0).clamp(-32768.0, 32767.0) as i16)
        .collect()
}

/// Silence-slice a channel into voiced clips (same parameters the
/// voice-lab extractor used: 512-sample RMS windows > 250, gaps under
/// 0.2 s merged, clips under 0.12 s dropped, 60 ms context pads).
fn slice_clips(pcm: &[i16], rate: u32) -> Vec<(usize, usize)> {
    let win = 512usize;
    let merge_w = (0.20 * rate as f64) as usize / win;
    let min_len = (0.12 * rate as f64) as usize;
    let pad = (0.06 * rate as f64) as usize;
    let mut active: Vec<bool> = pcm
        .chunks(win)
        .map(|c| {
            let ms = c.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / c.len() as f64;
            ms.sqrt() > 250.0
        })
        .collect();
    let mut i = 0;
    while i < active.len() {
        if !active[i] {
            let start = i;
            while i < active.len() && !active[i] {
                i += 1;
            }
            if start > 0 && i < active.len() && i - start <= merge_w {
                for a in active[start..i].iter_mut() {
                    *a = true;
                }
            }
        } else {
            i += 1;
        }
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < active.len() {
        if active[i] {
            let start = i;
            while i < active.len() && active[i] {
                i += 1;
            }
            let s = (start * win).saturating_sub(pad);
            let e = (i * win + pad).min(pcm.len());
            if e - s >= min_len {
                out.push((s, e));
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Process a whole XA channel: each voiced clip runs the chain and is
/// laid back at its ORIGINAL start offset (arts cue timing anchors on
/// clip starts), silence stays silence. A clip the bend shortened just
/// ends earlier; overlapping tails add-mix saturating.
pub fn process_channel(
    pcm: &[i16],
    rate: u32,
    fx: &VoiceFx,
    reference: Option<(&[i16], u32)>,
) -> Vec<i16> {
    let clips = slice_clips(pcm, rate);
    if clips.is_empty() {
        return pcm.to_vec();
    }
    let mut out = vec![0i16; pcm.len()];
    for (s, e) in clips {
        let y = process_clip(&pcm[s..e], rate, fx, reference);
        for (i, v) in y.iter().enumerate() {
            let Some(slot) = out.get_mut(s + i) else {
                break;
            };
            *slot = slot.saturating_add(*v);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f64, rate: u32, secs: f64) -> Vec<i16> {
        (0..(rate as f64 * secs) as usize)
            .map(|i| {
                ((2.0 * std::f64::consts::PI * hz * i as f64 / rate as f64).sin() * 12000.0) as i16
            })
            .collect()
    }

    #[test]
    fn mode_parses_and_defaults_to_adjusted() {
        assert_eq!(
            "original".parse::<ArtsVoiceMode>(),
            Ok(ArtsVoiceMode::Original)
        );
        assert_eq!(
            "Removed".parse::<ArtsVoiceMode>(),
            Ok(ArtsVoiceMode::Removed)
        );
        assert_eq!(
            "adjusted".parse::<ArtsVoiceMode>(),
            Ok(ArtsVoiceMode::Adjusted)
        );
        assert!("grunts".parse::<ArtsVoiceMode>().is_err());
        assert_eq!(ArtsVoiceMode::default(), ArtsVoiceMode::Adjusted);
    }

    #[test]
    fn voice_map_carries_the_tuned_cells() {
        // spot-check transcription against the by-ear session values
        let v_lu = voice_map(0, Sibling::Lu);
        assert_eq!(v_lu.pitch, 1.5);
        assert_eq!(v_lu.formant_st, -5.0);
        assert_eq!(v_lu.bend0, 6.5);
        assert_eq!(v_lu.hp, 90.0);
        let n_che = voice_map(1, Sibling::Che);
        assert_eq!(n_che.pitch, -11.0);
        assert_eq!(n_che.timbre, 0.6);
        assert_eq!(voice_map(2, Sibling::Gi).formant_st, -6.0);
        assert_eq!(voice_map(2, Sibling::Che).pitch, -2.0);
        // untouched knobs stay at base
        assert_eq!(voice_map(0, Sibling::Gi).carrier, 0.0);
        assert!(voice_map(1, Sibling::Lu).preserve);
    }

    #[test]
    fn preserve_mode_keeps_duration_and_shifts_pitch() {
        let rate = 37800u32;
        let x = tone(300.0, rate, 0.5);
        let fx = VoiceFx {
            pitch: -6.0,
            ..VOICE_FX_BASE
        };
        let y = process_clip(&x, rate, &fx, None);
        // duration preserved within WSOLA rounding
        let dd = (y.len() as f64 - x.len() as f64).abs() / x.len() as f64;
        assert!(dd < 0.05, "duration drifted {dd}");
        // dominant frequency halved-ish: count zero crossings
        let zc = |s: &[i16]| s.windows(2).filter(|w| (w[0] < 0) != (w[1] < 0)).count();
        let ratio = zc(&y) as f64 / zc(&x) as f64;
        let want = (-6.0f64 / 12.0).exp2();
        assert!((ratio - want).abs() < 0.08, "pitch ratio {ratio} vs {want}");
    }

    #[test]
    fn process_clip_is_deterministic_and_finite() {
        let rate = 37800u32;
        let x = tone(250.0, rate, 0.3);
        let refc = tone(180.0, 44100, 0.4);
        let fx = VoiceFx {
            pitch: -8.5,
            formant_st: -5.0,
            timbre: 0.6,
            bend0: 2.0,
            bend1: -1.0,
            breath: 0.3,
            drive: 0.2,
            sub: 0.3,
            growl: 0.4,
            detune: 15.0,
            carrier: 0.4,
            graft_ms: 80.0,
            tilt: 3.0,
            ..VOICE_FX_BASE
        };
        let a = process_clip(&x, rate, &fx, Some((&refc, 44100)));
        let b = process_clip(&x, rate, &fx, Some((&refc, 44100)));
        assert_eq!(a, b, "full chain must be deterministic");
        let peak = a.iter().map(|&s| (s as i32).abs()).max().unwrap();
        assert!(peak > 500, "output silent");
        assert!(peak <= 32200, "peak guard failed: {peak}");
    }

    #[test]
    fn channel_processing_keeps_clip_anchors() {
        let rate = 37800u32;
        // silence + clip + silence + clip
        let mut ch = vec![0i16; rate as usize / 2];
        ch.extend(tone(300.0, rate, 0.3));
        ch.extend(vec![0i16; rate as usize]);
        ch.extend(tone(280.0, rate, 0.25));
        ch.extend(vec![0i16; rate as usize / 4]);
        let fx = VoiceFx {
            pitch: 3.0,
            ..VOICE_FX_BASE
        };
        let out = process_channel(&ch, rate, &fx, None);
        assert_eq!(out.len(), ch.len());
        let energetic = |s: &[i16]| s.iter().any(|&v| v.unsigned_abs() > 400);
        // both clip windows still voiced, deep silence stays silent
        assert!(energetic(&out[rate as usize / 2..rate as usize]));
        let second_start = rate as usize / 2 + (rate as f64 * 0.3) as usize + rate as usize;
        assert!(energetic(
            &out[second_start..second_start + rate as usize / 8]
        ));
        assert!(!energetic(&out[..rate as usize / 4]));
    }
}

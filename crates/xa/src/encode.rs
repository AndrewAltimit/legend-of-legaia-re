//! XA-ADPCM **encoder** (mono, 4-bit): PCM to 128-byte sound groups the
//! retail streamer decodes - the write side of this crate's decoder.
//!
//! Per 28-sample sound unit the encoder brute-forces all four predictor
//! filters ([`crate::K0`]/[`crate::K1`]) crossed with shift ranges
//! `0..=12` and keeps the pair with the least squared reconstruction
//! error, mirroring the decoder's arithmetic exactly (f64 predictor
//! history, unclamped; only emitted samples round + clamp). Output
//! groups round-trip through [`crate::StreamingDecoder`] to the
//! reconstruction the encoder itself computed.

use crate::{K0, K1, SAMPLES_PER_UNIT, SOUND_GROUP_BYTES, UNITS_PER_GROUP_4BIT};

/// Rolling predictor history across units.
#[derive(Default, Clone, Copy)]
struct State {
    prev1: f64,
    prev2: f64,
}

/// Encode one 28-sample unit with a fixed `(filter, range)`; returns the
/// nibbles, the squared error, and the post-unit state.
fn try_unit(samples: &[i16], filter: usize, range: u32, s0: State) -> ([u8; 28], f64, State) {
    let (k0, k1) = (K0[filter], K1[filter]);
    let mut st = s0;
    let mut nibbles = [0u8; 28];
    let mut err = 0.0f64;
    let scale = (1i32 << (12 - range)) as f64;
    for (i, &raw) in samples.iter().enumerate() {
        let predicted = k0 * st.prev1 + k1 * st.prev2;
        let residual = raw as f64 - predicted;
        let q = (residual / scale).round().clamp(-8.0, 7.0) as i32;
        nibbles[i] = (q & 0xF) as u8;
        // Reconstruct exactly like the decoder: nibble sign-extended into
        // the top of a 16-bit word, arithmetic shift down by `range`.
        let top = (((q as i16) << 12) as i32) >> range;
        let recon = top as f64 + predicted;
        let clamped = recon.round().clamp(i16::MIN as f64, i16::MAX as f64);
        err += (clamped - raw as f64).powi(2);
        st.prev2 = st.prev1;
        st.prev1 = recon;
    }
    (nibbles, err, st)
}

/// Encode mono PCM into consecutive 4-bit XA sound groups (each
/// [`SOUND_GROUP_BYTES`] long, 8 units x 28 samples = 224 samples per
/// group). The tail pads with silence to a whole group.
pub fn encode_mono_4bit(pcm: &[i16]) -> Vec<u8> {
    let samples_per_group = UNITS_PER_GROUP_4BIT * SAMPLES_PER_UNIT;
    let groups = pcm.len().div_ceil(samples_per_group).max(1);
    let mut padded = pcm.to_vec();
    padded.resize(groups * samples_per_group, 0);

    let mut out = Vec::with_capacity(groups * SOUND_GROUP_BYTES);
    let mut state = State::default();
    for g in 0..groups {
        let mut group = [0u8; SOUND_GROUP_BYTES];
        for unit in 0..UNITS_PER_GROUP_4BIT {
            let base = g * samples_per_group + unit * SAMPLES_PER_UNIT;
            let samples = &padded[base..base + SAMPLES_PER_UNIT];
            let mut best: Option<([u8; 28], f64, State, u8)> = None;
            for filter in 0..4usize {
                for range in 0..=12u32 {
                    let (nibbles, err, st) = try_unit(samples, filter, range, state);
                    if best.as_ref().is_none_or(|(_, be, _, _)| err < *be) {
                        best = Some((nibbles, err, st, ((filter as u8) << 4) | range as u8));
                    }
                }
            }
            let (nibbles, _, st, param) = best.unwrap();
            state = st;
            // Param layout: units 0-3 at bytes 0..4 (copy at 4..8),
            // units 4-7 at bytes 8..12 (copy at 12..16).
            let po = if unit < 4 { unit } else { 4 + unit };
            group[po] = param;
            group[po + 4] = param;
            // Nibble layout: byte 16 + line*4 + unit/2; low nibble for
            // even units, high for odd.
            for (line, &n) in nibbles.iter().enumerate() {
                let b = &mut group[16 + line * 4 + unit / 2];
                if unit % 2 == 0 {
                    *b |= n & 0x0F;
                } else {
                    *b |= n << 4;
                }
            }
        }
        out.extend_from_slice(&group);
    }
    out
}

/// Encode mono PCM as DUAL-MONO 4-bit STEREO XA sound groups: the same
/// signal on both channels (even units = left, odd = right, 4 sample
/// pairs' worth per group = 112 stereo frames). For voicing a stereo
/// bank with a mono source.
pub fn encode_stereo_4bit_dualmono(pcm: &[i16]) -> Vec<u8> {
    let frames_per_group = (UNITS_PER_GROUP_4BIT / 2) * SAMPLES_PER_UNIT;
    let groups = pcm.len().div_ceil(frames_per_group).max(1);
    let mut padded = pcm.to_vec();
    padded.resize(groups * frames_per_group, 0);

    let mut out = Vec::with_capacity(groups * SOUND_GROUP_BYTES);
    let mut state = [State::default(); 2];
    for g in 0..groups {
        let mut group = [0u8; SOUND_GROUP_BYTES];
        for unit in 0..UNITS_PER_GROUP_4BIT {
            let ch = unit & 1;
            // Unit pair k covers frames [g*frames + k*28 ..][..28] on
            // both channels.
            let base = g * frames_per_group + (unit / 2) * SAMPLES_PER_UNIT;
            let samples = &padded[base..base + SAMPLES_PER_UNIT];
            let mut best: Option<([u8; 28], f64, State, u8)> = None;
            for filter in 0..4usize {
                for range in 0..=12u32 {
                    let (nibbles, err, st) = try_unit(samples, filter, range, state[ch]);
                    if best.as_ref().is_none_or(|(_, be, _, _)| err < *be) {
                        best = Some((nibbles, err, st, ((filter as u8) << 4) | range as u8));
                    }
                }
            }
            let (nibbles, _, st, param) = best.unwrap();
            state[ch] = st;
            let po = if unit < 4 { unit } else { 4 + unit };
            group[po] = param;
            group[po + 4] = param;
            for (line, &n) in nibbles.iter().enumerate() {
                let b = &mut group[16 + line * 4 + unit / 2];
                if unit % 2 == 0 {
                    *b |= n & 0x0F;
                } else {
                    *b |= n << 4;
                }
            }
        }
        out.extend_from_slice(&group);
    }
    out
}

/// Encode a true-stereo pair as 4-bit STEREO XA sound groups: left =
/// even units, right = odd units, 4 sample pairs' worth per group (112
/// stereo frames). `left`/`right` must be the same length; the tail
/// pads with silence to a whole group. This is
/// [`encode_stereo_4bit_dualmono`] generalised to independent channel
/// signals - for re-authoring a span of an existing stereo stream
/// in place.
pub fn encode_stereo_4bit(left: &[i16], right: &[i16]) -> Vec<u8> {
    assert_eq!(left.len(), right.len(), "stereo channels must match");
    let frames_per_group = (UNITS_PER_GROUP_4BIT / 2) * SAMPLES_PER_UNIT;
    let groups = left.len().div_ceil(frames_per_group).max(1);
    let mut l = left.to_vec();
    let mut r = right.to_vec();
    l.resize(groups * frames_per_group, 0);
    r.resize(groups * frames_per_group, 0);

    let mut out = Vec::with_capacity(groups * SOUND_GROUP_BYTES);
    let mut state = [State::default(); 2];
    for g in 0..groups {
        let mut group = [0u8; SOUND_GROUP_BYTES];
        for unit in 0..UNITS_PER_GROUP_4BIT {
            let ch = unit & 1;
            let base = g * frames_per_group + (unit / 2) * SAMPLES_PER_UNIT;
            let src = if ch == 0 { &l } else { &r };
            let samples = &src[base..base + SAMPLES_PER_UNIT];
            let mut best: Option<([u8; 28], f64, State, u8)> = None;
            for filter in 0..4usize {
                for range in 0..=12u32 {
                    let (nibbles, err, st) = try_unit(samples, filter, range, state[ch]);
                    if best.as_ref().is_none_or(|(_, be, _, _)| err < *be) {
                        best = Some((nibbles, err, st, ((filter as u8) << 4) | range as u8));
                    }
                }
            }
            let (nibbles, _, st, param) = best.unwrap();
            state[ch] = st;
            let po = if unit < 4 { unit } else { 4 + unit };
            group[po] = param;
            group[po + 4] = param;
            for (line, &n) in nibbles.iter().enumerate() {
                let b = &mut group[16 + line * 4 + unit / 2];
                if unit % 2 == 0 {
                    *b |= n & 0x0F;
                } else {
                    *b |= n << 4;
                }
            }
        }
        out.extend_from_slice(&group);
    }
    out
}

/// Naive linear-interpolation resampler (mono).
pub fn resample_linear(pcm: &[i16], from_hz: u32, to_hz: u32) -> Vec<i16> {
    if from_hz == to_hz || pcm.is_empty() {
        return pcm.to_vec();
    }
    let n_out = (pcm.len() as u64 * to_hz as u64 / from_hz as u64).max(1) as usize;
    (0..n_out)
        .map(|i| {
            let pos = i as f64 * from_hz as f64 / to_hz as f64;
            let i0 = pos.floor() as usize;
            let frac = pos - i0 as f64;
            let a = pcm[i0.min(pcm.len() - 1)] as f64;
            let b = pcm[(i0 + 1).min(pcm.len() - 1)] as f64;
            (a + (b - a) * frac).round() as i16
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BitsPerSample, Channels, DecodeOptions, StreamingDecoder};

    #[test]
    fn encoded_groups_decode_close_to_source() {
        // A synthetic voiced-ish signal: sum of two tones + decay.
        let n = 224 * 6;
        let pcm: Vec<i16> = (0..n)
            .map(|i| {
                let t = i as f64 / 18900.0;
                let env = (-t * 6.0).exp();
                ((6000.0 * (t * 2.0 * std::f64::consts::PI * 220.0).sin()
                    + 2500.0 * (t * 2.0 * std::f64::consts::PI * 880.0).sin())
                    * env) as i16
            })
            .collect();
        let groups = encode_mono_4bit(&pcm);
        assert_eq!(groups.len() % SOUND_GROUP_BYTES, 0);

        let mut dec = StreamingDecoder::new(DecodeOptions {
            channels: Channels::Mono,
            bits: BitsPerSample::Four,
            sample_rate: 18900,
        });
        let mut out = Vec::new();
        dec.feed(&groups, &mut out).expect("decode");
        assert!(out.len() >= pcm.len());

        // The 4-bit codec is lossy; require a high signal-to-noise ratio
        // rather than equality.
        let sig: f64 = pcm.iter().map(|&s| (s as f64).powi(2)).sum();
        let noise: f64 = pcm
            .iter()
            .zip(&out)
            .map(|(&a, &b)| (a as f64 - b as f64).powi(2))
            .sum();
        let snr_db = 10.0 * (sig / noise.max(1.0)).log10();
        assert!(snr_db > 25.0, "SNR {snr_db:.1} dB too low");
    }

    #[test]
    fn stereo_pair_round_trips_per_channel() {
        // Distinct L/R signals so a channel swap or a dual-mono fold
        // fails the assert.
        let n = 112 * 8;
        let left: Vec<i16> = (0..n)
            .map(|i| {
                let t = i as f64 / 37800.0;
                (9000.0 * (t * 2.0 * std::f64::consts::PI * 330.0).sin()) as i16
            })
            .collect();
        let right: Vec<i16> = (0..n)
            .map(|i| {
                let t = i as f64 / 37800.0;
                (7000.0 * (t * 2.0 * std::f64::consts::PI * 550.0).sin()) as i16
            })
            .collect();
        let groups = encode_stereo_4bit(&left, &right);
        assert_eq!(groups.len() % SOUND_GROUP_BYTES, 0);

        let mut dec = StreamingDecoder::new(DecodeOptions {
            channels: Channels::Stereo,
            bits: BitsPerSample::Four,
            sample_rate: 37800,
        });
        let mut out = Vec::new();
        dec.feed(&groups, &mut out).expect("decode");
        assert!(out.len() >= n * 2);

        let snr = |src: &[i16], step_off: usize| -> f64 {
            let sig: f64 = src.iter().map(|&s| (s as f64).powi(2)).sum();
            let noise: f64 = src
                .iter()
                .enumerate()
                .map(|(i, &a)| (a as f64 - out[i * 2 + step_off] as f64).powi(2))
                .sum();
            10.0 * (sig / noise.max(1.0)).log10()
        };
        assert!(snr(&left, 0) > 25.0, "left SNR too low");
        assert!(snr(&right, 1) > 25.0, "right SNR too low");
    }
}

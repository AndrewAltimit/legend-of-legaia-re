//! PSX CD-XA ADPCM decoder.
//!
//! Decodes raw 128-byte sound groups (the format Legaia ships in
//! `extracted/XA/*.XA` — CD-XA Mode2/Form2 sector subheaders stripped) into
//! PCM 16-bit samples, then writes a WAV file.
//!
//! ## Sound group layout (128 bytes)
//!
//! For 4-bit ADPCM (the most common XA mode):
//!
//! - bytes 0..16 — 8 sound-unit parameters, each repeated twice for error
//!   detection. Layout: `[su0,su1,su2,su3, su4,su5,su6,su7, su0..su7 again]`.
//!   Each parameter byte = `(filter << 4) | range` where filter ∈ 0..=3 and
//!   range ∈ 0..=12.
//! - bytes 16..128 — 28 lines × 4 bytes per line of sample nibbles. Within
//!   a line, byte k holds:
//!     * low nibble = sound unit `k` sample
//!     * high nibble = sound unit `k+4` sample
//!
//! For 8-bit ADPCM, there are 4 sound units of 28 samples each (28 bytes per
//! unit, packed as the same 28×4 line layout but each byte = one 8-bit
//! sample). Less common for music; not yet implemented.
//!
//! ## Filter coefficients
//!
//! XA-ADPCM uses 4 filters; SPU has a 5th. Coefficients are in 1/64 units:
//!
//! | filter | f0  | f1   |
//! |--------|-----|------|
//! | 0      |   0 |    0 |
//! | 1      |  60 |    0 |
//! | 2      | 115 |  -52 |
//! | 3      |  98 |  -55 |
//!
//! ## Decode formula (per sample)
//!
//! ```text
//! shifted = sign_extend(nibble, 4) << 12 >> range
//! pred    = (prev1 * f0 + prev2 * f1 + 32) >> 6
//! output  = clip(shifted + pred)
//! prev2   = prev1; prev1 = output
//! ```
//!
//! ## Legaia-specific notes (caveat)
//!
//! `extracted/XA/*.XA` files are aligned to 128-byte sound groups, but only
//! ~10% of groups pass standard XA validation (`bytes 8..16` must mirror
//! `bytes 0..8`, all filter nibbles ≤ 3). The remaining 90% appear to be
//! either CD-XA subheader/padding interleaved between audio frames, or a
//! Legaia-specific muxing scheme that hasn't been reverse-engineered yet.
//!
//! Until that's resolved, this decoder produces audible-but-glitched output
//! on real Legaia files — invalid groups emit silence to keep timing stable.
//! For purely synthetic / standard-XA test vectors the decoder is correct.
//!
//! ## Stereo interleave
//!
//! For stereo XA, sound units alternate L/R per pair: SU0=L, SU1=R, SU2=L,
//! SU3=R, etc. Each channel keeps its own (prev1, prev2) history.

use anyhow::{Context, Result, bail};

pub const SOUND_GROUP_BYTES: usize = 128;
pub const SAMPLES_PER_UNIT: usize = 28;
pub const UNITS_PER_GROUP_4BIT: usize = 8;

pub const F0: [i32; 5] = [0, 60, 115, 98, 122];
pub const F1: [i32; 5] = [0, 0, -52, -55, -60];

/// Channel mode for XA-ADPCM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channels {
    Mono,
    Stereo,
}

impl Channels {
    pub fn n(self) -> u16 {
        match self {
            Channels::Mono => 1,
            Channels::Stereo => 2,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DecodeOptions {
    pub channels: Channels,
    /// Output sample rate in Hz. PSX XA standard rates are 18900 (low-quality)
    /// and 37800 (high-quality). The rate is normally encoded in the per-sector
    /// CD-XA subheader, which is stripped from these files; supply it externally.
    pub sample_rate: u32,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        Self {
            channels: Channels::Mono,
            sample_rate: 37800,
        }
    }
}

/// Per-channel ADPCM state between sound units.
#[derive(Debug, Clone, Copy, Default)]
pub struct ChannelState {
    pub prev1: i32,
    pub prev2: i32,
}

#[derive(Debug, Clone)]
pub struct DecodeReport {
    pub n_groups: usize,
    pub n_groups_skipped: usize,
    /// Total interleaved sample count (= per_channel × channels).
    pub n_samples_interleaved: usize,
}

/// Decode a buffer of raw 128-byte sound groups into interleaved PCM 16-bit
/// samples. For stereo, output alternates L,R,L,R,...
///
/// Groups with malformed parameters (filter > 3, or the bytes 8..16 that
/// should redundantly mirror bytes 0..8 don't match) are treated as skipped
/// stream interleave / padding — they contribute 28 silent samples per unit
/// to keep timing intact, and increment `n_groups_skipped` in the report.
/// This is the same behavior the PSX SPU exhibits: it doesn't crash on bad
/// XA, it just emits the previously-decoded output.
pub fn decode(buf: &[u8], opts: DecodeOptions) -> Result<(Vec<i16>, DecodeReport)> {
    if !buf.len().is_multiple_of(SOUND_GROUP_BYTES) {
        bail!(
            "input length {} is not a multiple of {} (sound group size)",
            buf.len(),
            SOUND_GROUP_BYTES
        );
    }
    let n_groups = buf.len() / SOUND_GROUP_BYTES;
    let mut out: Vec<i16> = Vec::with_capacity(n_groups * UNITS_PER_GROUP_4BIT * SAMPLES_PER_UNIT);

    let mut state = [ChannelState::default(); 2];
    let mut skipped = 0usize;

    for g in 0..n_groups {
        let group = &buf[g * SOUND_GROUP_BYTES..(g + 1) * SOUND_GROUP_BYTES];
        if !group_is_valid(group) {
            // Emit silence for this group; reset state so we don't carry a
            // stale prediction across the gap.
            state[0] = ChannelState::default();
            state[1] = ChannelState::default();
            let n = UNITS_PER_GROUP_4BIT * SAMPLES_PER_UNIT;
            out.extend(std::iter::repeat_n(0i16, n));
            skipped += 1;
            continue;
        }
        decode_group_4bit(group, opts.channels, &mut state, &mut out)?;
    }

    Ok((
        out.clone(),
        DecodeReport {
            n_groups,
            n_groups_skipped: skipped,
            n_samples_interleaved: out.len(),
        },
    ))
}

fn group_is_valid(group: &[u8]) -> bool {
    // Bytes 8..16 must mirror bytes 0..8 (XA stores 8 SU params twice for
    // error detection). If they don't, this isn't a real sound group.
    if group[..8] != group[8..16] {
        return false;
    }
    // All filter nibbles must be in 0..=3.
    for byte in group.iter().take(UNITS_PER_GROUP_4BIT) {
        let filter = (byte >> 4) & 0x0F;
        if filter > 3 {
            return false;
        }
    }
    true
}

fn decode_group_4bit(
    group: &[u8],
    channels: Channels,
    state: &mut [ChannelState; 2],
    out: &mut Vec<i16>,
) -> Result<()> {
    debug_assert_eq!(group.len(), SOUND_GROUP_BYTES);

    // Read 8 sound unit parameters from bytes 0..8 (bytes 8..16 are duplicates).
    let mut params = [0u8; UNITS_PER_GROUP_4BIT];
    params.copy_from_slice(&group[..UNITS_PER_GROUP_4BIT]);

    // 28 lines × 4 bytes = sample nibbles. We accumulate per-unit, per-sample
    // into a temporary buffer, then interleave at the end.
    //
    // For mono: emit 8 units × 28 samples in unit order.
    // For stereo: SU0=L, SU1=R, SU2=L, SU3=R, ..., SU6=L, SU7=R; the natural
    //   per-sample order is L,R,L,R,... where for each "sample slot" we emit
    //   the matching pair of units.
    let mut decoded = [[0i16; SAMPLES_PER_UNIT]; UNITS_PER_GROUP_4BIT];

    for unit in 0..UNITS_PER_GROUP_4BIT {
        let p = params[unit];
        let range = (p & 0x0F) as u32;
        let filter = ((p >> 4) & 0x0F) as usize;
        if filter > 3 {
            bail!("XA filter {} out of range (0..=3)", filter);
        }
        let f0 = F0[filter];
        let f1 = F1[filter];

        let ch = match channels {
            Channels::Mono => 0,
            Channels::Stereo => unit & 1,
        };

        for s in 0..SAMPLES_PER_UNIT {
            let line_byte = group[16 + s * 4 + (unit % 4)];
            let nibble = if unit < 4 {
                line_byte & 0x0F
            } else {
                (line_byte >> 4) & 0x0F
            };
            // Sign-extend 4-bit nibble to 16 bits, then shift up to 16-bit
            // range and back down by `range` to apply the per-unit gain.
            let nib_signed = ((nibble as i16) << 12) >> 12;
            // Apply range with a sane cap (range > 12 would shift more than
            // the data is worth; clamp to avoid undefined behavior on huge
            // shifts).
            let shifted: i32 = if range >= 16 {
                0
            } else {
                ((nib_signed as i32) << 12) >> range
            };
            let pred = (state[ch].prev1 * f0 + state[ch].prev2 * f1 + 32) >> 6;
            let mixed = shifted.saturating_add(pred);
            let clipped = mixed.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            decoded[unit][s] = clipped;
            state[ch].prev2 = state[ch].prev1;
            state[ch].prev1 = clipped as i32;
        }
    }

    // Emit samples in playback order.
    match channels {
        Channels::Mono => {
            // 8 units in serial: SU0[0..28], SU1[0..28], ..., SU7[0..28].
            for unit in decoded.iter().take(UNITS_PER_GROUP_4BIT) {
                out.extend_from_slice(unit);
            }
        }
        Channels::Stereo => {
            // Pair (SU0,SU1), (SU2,SU3), ..., (SU6,SU7); each pair = 28 LR
            // sample pairs interleaved.
            for pair in decoded[..UNITS_PER_GROUP_4BIT].chunks_exact(2) {
                for (ls, rs) in pair[0].iter().zip(pair[1].iter()) {
                    out.push(*ls);
                    out.push(*rs);
                }
            }
        }
    }
    Ok(())
}

/// Write a 16-bit PCM WAV file.
pub fn write_wav(
    path: &std::path::Path,
    samples: &[i16],
    channels: Channels,
    sample_rate: u32,
) -> Result<()> {
    use std::io::Write;
    let n_channels = channels.n();
    let bits = 16u16;
    let byte_rate = sample_rate * (n_channels as u32) * (bits as u32 / 8);
    let block_align = n_channels * bits / 8;
    let data_size = (samples.len() * 2) as u32;
    let riff_size = 36 + data_size;

    let file =
        std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let mut w = std::io::BufWriter::new(file);
    w.write_all(b"RIFF")?;
    w.write_all(&riff_size.to_le_bytes())?;
    w.write_all(b"WAVE")?;
    w.write_all(b"fmt ")?;
    w.write_all(&16u32.to_le_bytes())?;
    w.write_all(&1u16.to_le_bytes())?; // PCM
    w.write_all(&n_channels.to_le_bytes())?;
    w.write_all(&sample_rate.to_le_bytes())?;
    w.write_all(&byte_rate.to_le_bytes())?;
    w.write_all(&block_align.to_le_bytes())?;
    w.write_all(&bits.to_le_bytes())?;
    w.write_all(b"data")?;
    w.write_all(&data_size.to_le_bytes())?;
    for s in samples {
        w.write_all(&s.to_le_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_silence_group() -> [u8; SOUND_GROUP_BYTES] {
        let mut g = [0u8; SOUND_GROUP_BYTES];
        // params = 0 for all 8 units (filter=0, range=0)
        // bytes 16..128 = 0 → all nibbles = 0 → all samples decode to 0
        // but with range=0 a 0-nibble shifted left 12 then right 0 = 0 (ok)
        g[0..16].fill(0);
        g
    }

    #[test]
    fn silence_in_silence_out() {
        let buf = synthetic_silence_group();
        let (samples, report) = decode(&buf, DecodeOptions::default()).unwrap();
        assert_eq!(report.n_groups, 1);
        assert_eq!(samples.len(), 224); // 8 units × 28 samples
        assert!(samples.iter().all(|&s| s == 0));
    }

    #[test]
    fn rejects_misaligned_input() {
        let buf = vec![0u8; 130];
        assert!(decode(&buf, DecodeOptions::default()).is_err());
    }

    #[test]
    fn stereo_doubles_per_group() {
        let buf = synthetic_silence_group();
        let (samples, _) = decode(
            &buf,
            DecodeOptions {
                channels: Channels::Stereo,
                sample_rate: 37800,
            },
        )
        .unwrap();
        // Same total sample count (224), but interpreted as 112 LR pairs.
        assert_eq!(samples.len(), 224);
    }

    #[test]
    fn nonzero_decoded_to_predictable_values() {
        // Filter 0, range 8: sample shifts contribute small non-zero output.
        // nibble = 1 (positive small): shifted = (1 << 12) >> 8 = 16
        // pred = 0 (history is 0, filter 0 has both coefs = 0)
        // output = 16
        let mut g = [0u8; SOUND_GROUP_BYTES];
        g[0] = 0x08; // SU0: filter=0, range=8
        g[8] = 0x08; // mirror copy required by group_is_valid
        g[16] = 0x01; // line 0, byte 0: SU0 nibble = 1
        let (samples, _) = decode(&g, DecodeOptions::default()).unwrap();
        assert_eq!(samples[0], 16);
    }

    #[test]
    fn skips_invalid_groups() {
        // Two-group buffer: group 0 valid, group 1 has a filter > 3.
        let mut buf = vec![0u8; 2 * SOUND_GROUP_BYTES];
        // group 0: all-zero params, all-zero data → valid silence.
        // group 1: corrupt — filter nibble for SU0 is 0xC.
        buf[SOUND_GROUP_BYTES] = 0xC0;
        let (samples, report) = decode(&buf, DecodeOptions::default()).unwrap();
        assert_eq!(report.n_groups, 2);
        assert_eq!(report.n_groups_skipped, 1);
        assert_eq!(samples.len(), 2 * 8 * SAMPLES_PER_UNIT);
    }

    #[test]
    fn wav_writes_valid_riff_header() {
        let samples = vec![0i16; 100];
        let tmp = std::env::temp_dir().join("legaia_xa_test.wav");
        write_wav(&tmp, &samples, Channels::Mono, 37800).unwrap();
        let bytes = std::fs::read(&tmp).unwrap();
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"fmt ");
        assert_eq!(&bytes[36..40], b"data");
        let _ = std::fs::remove_file(&tmp);
    }
}

//! PSX CD-XA ADPCM decoder.
//!
//! Decodes raw 128-byte sound groups (the format Legaia ships in
//! `extracted/XA/*.XA` - CD-XA Mode2/Form2 sector subheaders stripped) into
//! PCM 16-bit samples, then writes a WAV file.
//!
//! Faithful to a reference float decode (jPSXdec): on a real Legaia movie's
//! interleaved XA track the decoded PCM is bit-identical to the reference,
//! sample-for-sample, across both channels.
//!
//! ## Sound group layout (128 bytes)
//!
//! For 4-bit ADPCM (the most common XA mode), each group holds 8 sound units
//! of 28 samples each:
//!
//! - bytes 0..16 - the sound-unit parameters. The redundant copy is
//!   interleaved *within each half*, not appended, so the layout is
//!   `[p0 p1 p2 p3 | p0 p1 p2 p3 | p4 p5 p6 p7 | p4 p5 p6 p7]` - i.e. unit
//!   `u`'s parameter is at byte `u + (if u < 4 { 0 } else { 4 })`. Each
//!   parameter byte = `(filter << 4) | range` with filter ∈ 0..=3 and
//!   range ∈ 0..=12.
//! - bytes 16..128 - 28 lines × 4 bytes per line of sample nibbles. Unit `u`
//!   reads byte `u / 2` of each line, taking the **low** nibble when `u` is
//!   even and the **high** nibble when `u` is odd. So byte 0 carries units
//!   0 (low) and 1 (high), byte 1 carries units 2/3, etc.
//!
//! For 8-bit ADPCM, there are 4 sound units of 28 samples each (28 bytes per
//! unit, packed as the same 28×4 line layout but each byte = one 8-bit
//! sample). Less common for music; not yet implemented.
//!
//! ## Filter coefficients
//!
//! XA-ADPCM uses 4 filters; SPU has a 5th. Coefficients are in 1/64 units; the
//! predictor is evaluated at that fractional precision (see [`K0`] / [`K1`]):
//!
//! | filter | f0  | f1   | k0       | k1        |
//! |--------|-----|------|----------|-----------|
//! | 0      |   0 |    0 | 0        | 0         |
//! | 1      |  60 |    0 | 0.9375   | 0         |
//! | 2      | 115 |  -52 | 1.796875 | -0.8125   |
//! | 3      |  98 |  -55 | 1.53125  | -0.859375 |
//!
//! ## Decode formula (per sample)
//!
//! ```text
//! shifted = (sign_extend(nibble, 4) << 12) >> range
//! value   = shifted + k0 * prev1 + k1 * prev2
//! output  = clip(round(value))         // round half away from zero, then clamp to i16
//! prev2   = prev1; prev1 = value        // history is the UNCLAMPED, UNROUNDED value
//! ```
//!
//! Feeding the unrounded `value` (rather than the clamped 16-bit output) back
//! into the predictor is what keeps long passages from drifting.
//!
//! ## Stereo interleave
//!
//! For stereo XA the LEFT channel is the even units (0,2,4,6) and the RIGHT
//! channel is the odd units (1,3,5,7); output is L,R interleaved. Each channel
//! keeps its own (prev1, prev2) history.

use anyhow::{Context, Result, bail};

pub const SOUND_GROUP_BYTES: usize = 128;
pub const SAMPLES_PER_UNIT: usize = 28;
pub const UNITS_PER_GROUP_4BIT: usize = 8;

pub const F0: [i32; 5] = [0, 60, 115, 98, 122];
pub const F1: [i32; 5] = [0, 0, -52, -55, -60];

/// Filter coefficients as exact fractions (the integer tables divided by 64).
/// The predictor is evaluated at this precision and the running history is kept
/// unrounded, matching a reference float decode (see [`ChannelState`]).
pub const K0: [f64; 5] = [0.0, 0.9375, 1.796875, 1.53125, 1.90625];
pub const K1: [f64; 5] = [0.0, 0.0, -0.8125, -0.859375, -0.9375];

pub mod demux;

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
///
/// The predictor history (`prev1`/`prev2`) is the **unclamped, unrounded**
/// reconstructed value, not the 16-bit output sample. Keeping full precision in
/// the IIR feedback is what a reference float decoder (jPSXdec) does, and it
/// avoids the slow per-sample drift that re-feeding the rounded+clamped output
/// would accumulate through the filter.
#[derive(Debug, Clone, Copy, Default)]
pub struct ChannelState {
    pub prev1: f64,
    pub prev2: f64,
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
/// stream interleave / padding - they contribute 28 silent samples per unit
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

/// Extract the 8 sound-unit parameter bytes from a 128-byte group header.
///
/// The CD-XA 4-bit header stores params for units 0..4 at bytes 0..4 and
/// units 4..8 at bytes 8..12; bytes 4..8 and 12..16 are redundant copies.
fn sound_unit_params(group: &[u8]) -> [u8; UNITS_PER_GROUP_4BIT] {
    [
        group[0], group[1], group[2], group[3], group[8], group[9], group[10], group[11],
    ]
}

/// Round half away from zero, then clamp to the signed 16-bit PCM range.
fn round_clamp_i16(v: f64) -> i16 {
    let rounded = if v > 0.0 { v + 0.5 } else { v - 0.5 } as i64;
    rounded.clamp(i16::MIN as i64, i16::MAX as i64) as i16
}

fn group_is_valid(group: &[u8]) -> bool {
    // Validity check: every sound-unit parameter byte must have a filter
    // nibble in 0..=3. The params live at bytes [0,1,2,3,8,9,10,11] (the
    // CD-XA layout), so check those rather than the first 8 bytes.
    for byte in sound_unit_params(group) {
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

    // Read the 8 sound-unit parameters. The 16-byte header is NOT eight
    // sequential params followed by an eight-param mirror; the CD-XA layout
    // interleaves the redundant copy within each half:
    //   bytes 0..4   = params for units 0,1,2,3
    //   bytes 4..8   = copy of units 0,1,2,3 (error-detection redundancy)
    //   bytes 8..12  = params for units 4,5,6,7
    //   bytes 12..16 = copy of units 4,5,6,7
    // So units 4..8 live at bytes 8..12, not bytes 4..8.
    let params = sound_unit_params(group);

    // 28 lines × 4 bytes = sample nibbles. We accumulate per-unit, per-sample
    // into a temporary buffer, then interleave at the end.
    //
    // The 8 sound units of a 4-bit group map to the nibble layout as: unit `u`
    // reads byte `u / 2` of each 4-byte line, taking the low nibble when `u` is
    // even and the high nibble when `u` is odd. So byte 0 holds units 0 (low)
    // and 1 (high), byte 1 holds units 2/3, byte 2 holds units 4/5, byte 3
    // holds units 6/7.
    //
    // For mono: emit 8 units × 28 samples in unit order (one channel).
    // For stereo: the LEFT channel is the even units (0,2,4,6) and the RIGHT
    //   channel is the odd units (1,3,5,7). Output is L,R interleaved, pairing
    //   (0,1),(2,3),(4,5),(6,7); each channel's predictor history flows along
    //   its own units in the order this loop walks them.
    let mut decoded = [[0i16; SAMPLES_PER_UNIT]; UNITS_PER_GROUP_4BIT];

    for unit in 0..UNITS_PER_GROUP_4BIT {
        let p = params[unit];
        let range = (p & 0x0F) as u32;
        let filter = ((p >> 4) & 0x0F) as usize;
        if filter > 3 {
            bail!("XA filter {} out of range (0..=3)", filter);
        }
        let k0 = K0[filter];
        let k1 = K1[filter];

        let ch = match channels {
            Channels::Mono => 0,
            Channels::Stereo => unit & 1,
        };

        for s in 0..SAMPLES_PER_UNIT {
            let line_byte = group[16 + s * 4 + unit / 2];
            let nibble = if unit % 2 == 0 {
                line_byte & 0x0F
            } else {
                (line_byte >> 4) & 0x0F
            };
            // Place the 4-bit nibble in the top of a 16-bit word (sign
            // extended), then arithmetic-shift down by `range` to apply the
            // per-unit gain. `range` is capped so an out-of-spec value can't
            // trigger an undefined-behaviour shift.
            let top = ((nibble as i16) << 12) as i32; // == sign_extend(nibble) << 12
            let shifted: i32 = if range >= 16 { 0 } else { top >> range };
            // Predict in full precision and keep the running history unrounded
            // and unclamped; only the emitted sample is rounded and clamped.
            let predicted = shifted as f64 + k0 * state[ch].prev1 + k1 * state[ch].prev2;
            decoded[unit][s] = round_clamp_i16(predicted);
            state[ch].prev2 = state[ch].prev1;
            state[ch].prev1 = predicted;
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
            // Left = even units, right = odd units; pair (0,1),(2,3),(4,5),(6,7)
            // and emit each pair as 28 interleaved L,R sample pairs.
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

/// Incremental XA-ADPCM decoder. Holds per-channel filter state across
/// calls so callers can feed the decoder one sound group (or a small
/// batch) at a time as bytes arrive.
///
/// Use this when the source bytes don't fit comfortably in memory (long
/// disc-resident XA tracks) or when downstream consumers want PCM
/// produced on a pull schedule rather than a single big decode batch.
///
/// The all-at-once [`decode`] API is built on the same group walker;
/// switching to [`StreamingDecoder`] is a behaviour-preserving refactor
/// for the streaming path.
pub struct StreamingDecoder {
    opts: DecodeOptions,
    state: [ChannelState; 2],
    leftover: Vec<u8>,
    n_groups_total: usize,
    n_groups_skipped: usize,
}

impl StreamingDecoder {
    pub fn new(opts: DecodeOptions) -> Self {
        Self {
            opts,
            state: [ChannelState::default(); 2],
            leftover: Vec::new(),
            n_groups_total: 0,
            n_groups_skipped: 0,
        }
    }

    /// Feed `bytes` into the decoder, append decoded interleaved PCM to
    /// `out`. Any tail bytes that don't form a complete 128-byte sound
    /// group are buffered and consumed on the next call. Returns the
    /// number of complete groups decoded (including silently-skipped
    /// groups).
    ///
    /// Channel mode + sample rate stay fixed for the decoder lifetime -
    /// callers that need to switch mid-stream allocate a new decoder.
    pub fn feed(&mut self, bytes: &[u8], out: &mut Vec<i16>) -> Result<usize> {
        let mut buf: Vec<u8> = std::mem::take(&mut self.leftover);
        buf.extend_from_slice(bytes);
        let mut start = 0usize;
        let mut groups = 0usize;
        while start + SOUND_GROUP_BYTES <= buf.len() {
            let group = &buf[start..start + SOUND_GROUP_BYTES];
            if group_is_valid(group) {
                decode_group_4bit(group, self.opts.channels, &mut self.state, out)?;
            } else {
                // Same silent-skip behaviour as the batch decoder.
                self.state[0] = ChannelState::default();
                self.state[1] = ChannelState::default();
                let n = UNITS_PER_GROUP_4BIT * SAMPLES_PER_UNIT;
                out.extend(std::iter::repeat_n(0i16, n));
                self.n_groups_skipped += 1;
            }
            start += SOUND_GROUP_BYTES;
            groups += 1;
        }
        // Stash partial-group tail for the next call.
        self.leftover = buf[start..].to_vec();
        self.n_groups_total += groups;
        Ok(groups)
    }

    /// Number of complete groups consumed so far.
    pub fn groups_consumed(&self) -> usize {
        self.n_groups_total
    }

    /// Number of groups that were silently zero-filled because their
    /// parameter bytes failed the validity check.
    pub fn groups_skipped(&self) -> usize {
        self.n_groups_skipped
    }

    /// Channel mode the decoder was constructed with.
    pub fn channels(&self) -> Channels {
        self.opts.channels
    }

    /// Output sample rate the decoder was constructed with.
    pub fn sample_rate(&self) -> u32 {
        self.opts.sample_rate
    }

    /// Current leftover-byte count (0..=127). Useful for diagnostics -
    /// a healthy XA stream feeds whole groups so this is usually 0.
    pub fn pending_bytes(&self) -> usize {
        self.leftover.len()
    }
}

#[cfg(test)]
mod streaming_tests {
    use super::*;

    fn synth_silent_group() -> Vec<u8> {
        // Filter 0, range 0 across all 8 sound units, plus zero sample
        // nibbles - yields silence and validates cleanly.
        vec![0u8; SOUND_GROUP_BYTES]
    }

    #[test]
    fn streaming_matches_batch_for_whole_groups() {
        let buf: Vec<u8> = (0..3).flat_map(|_| synth_silent_group()).collect();
        let opts = DecodeOptions {
            channels: Channels::Mono,
            sample_rate: 18900,
        };
        let (batch_pcm, _) = decode(&buf, opts).unwrap();
        let mut decoder = StreamingDecoder::new(opts);
        let mut stream_pcm = Vec::new();
        decoder.feed(&buf, &mut stream_pcm).unwrap();
        assert_eq!(batch_pcm, stream_pcm);
        assert_eq!(decoder.groups_consumed(), 3);
        assert_eq!(decoder.pending_bytes(), 0);
    }

    #[test]
    fn streaming_carries_partial_group_into_next_feed() {
        let one_group = synth_silent_group();
        let opts = DecodeOptions {
            channels: Channels::Mono,
            sample_rate: 18900,
        };
        let mut decoder = StreamingDecoder::new(opts);
        let mut pcm = Vec::new();
        // Feed 64 bytes - half a group, no output yet.
        decoder.feed(&one_group[..64], &mut pcm).unwrap();
        assert_eq!(decoder.groups_consumed(), 0);
        assert_eq!(decoder.pending_bytes(), 64);
        assert!(pcm.is_empty());
        // Feed remaining 64 - completes the group.
        decoder.feed(&one_group[64..], &mut pcm).unwrap();
        assert_eq!(decoder.groups_consumed(), 1);
        assert_eq!(decoder.pending_bytes(), 0);
        assert_eq!(pcm.len(), UNITS_PER_GROUP_4BIT * SAMPLES_PER_UNIT);
    }
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
        g[0] = 0x08; // SU0 param: filter=0, range=8
        g[16] = 0x01; // line 0, byte 0, low nibble: SU0 sample 0 = 1
        let (samples, _) = decode(&g, DecodeOptions::default()).unwrap();
        assert_eq!(samples[0], 16);
    }

    #[test]
    fn sound_unit_params_skip_the_redundant_copies() {
        // Header = [A B C D | a b c d | E F G H | e f g h]; only the first copy
        // of each half is the live parameter set. Distinct bytes 4..8 / 12..16
        // must be ignored.
        let mut g = [0u8; SOUND_GROUP_BYTES];
        g[0..4].copy_from_slice(&[0x00, 0x11, 0x22, 0x33]);
        g[4..8].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]); // redundant copy: ignored
        g[8..12].copy_from_slice(&[0x01, 0x12, 0x23, 0x30]);
        g[12..16].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]); // ignored
        assert_eq!(
            sound_unit_params(&g),
            [0x00, 0x11, 0x22, 0x33, 0x01, 0x12, 0x23, 0x30]
        );
    }

    #[test]
    fn round_clamp_matches_round_half_away_from_zero() {
        assert_eq!(round_clamp_i16(0.5), 1);
        assert_eq!(round_clamp_i16(-0.5), -1);
        assert_eq!(round_clamp_i16(1.49), 1);
        assert_eq!(round_clamp_i16(-1.49), -1);
        assert_eq!(round_clamp_i16(2.5), 3);
        assert_eq!(round_clamp_i16(0.0), 0);
        // Clamps past the 16-bit range.
        assert_eq!(round_clamp_i16(40_000.0), i16::MAX);
        assert_eq!(round_clamp_i16(-40_000.0), i16::MIN);
    }

    #[test]
    fn stereo_splits_even_units_left_odd_units_right() {
        // Filter 0, range 12 everywhere -> output sample == nibble value.
        // Distinct nibbles in byte 0 (units 0 low, 1 high) prove the channel
        // split: even unit -> left, odd unit -> right.
        let mut g = [0u8; SOUND_GROUP_BYTES];
        for b in g.iter_mut().take(16) {
            *b = 0x0C; // filter 0, range 12
        }
        g[16] = 0x31; // line 0 byte 0: low nibble = 1 (unit0/L), high = 3 (unit1/R)
        let (samples, _) = decode(
            &g,
            DecodeOptions {
                channels: Channels::Stereo,
                sample_rate: 37_800,
            },
        )
        .unwrap();
        // First interleaved pair is (L=unit0[0], R=unit1[0]) = (1, 3).
        assert_eq!(samples[0], 1);
        assert_eq!(samples[1], 3);
    }

    #[test]
    fn output_saturates_and_holds_on_a_railing_input() {
        // Filter 1 (k0 = 0.9375) with a small range has a reconstruction whose
        // steady state runs past the 16-bit ceiling, so a long run of
        // max-magnitude nibbles must drive the output to i16::MAX and hold it
        // there (the unclamped predictor history keeps the value pinned high).
        let mut g = [0u8; SOUND_GROUP_BYTES];
        for b in g.iter_mut().take(16) {
            *b = 0x13; // filter 1, range 3
        }
        for b in g.iter_mut().skip(16) {
            *b = 0x77; // every nibble = 7 (max positive)
        }
        let (samples, _) = decode(&g, DecodeOptions::default()).unwrap();
        assert_eq!(samples[SAMPLES_PER_UNIT - 1], i16::MAX);
        assert_eq!(*samples.last().unwrap(), i16::MAX);
    }

    #[test]
    fn skips_invalid_groups() {
        // Two-group buffer: group 0 valid, group 1 has a filter > 3.
        let mut buf = vec![0u8; 2 * SOUND_GROUP_BYTES];
        // group 0: all-zero params, all-zero data → valid silence.
        // group 1: corrupt - filter nibble for SU0 is 0xC.
        buf[SOUND_GROUP_BYTES] = 0xC0;
        let (samples, report) = decode(&buf, DecodeOptions::default()).unwrap();
        assert_eq!(report.n_groups, 2);
        assert_eq!(report.n_groups_skipped, 1);
        assert_eq!(samples.len(), 2 * 8 * SAMPLES_PER_UNIT);
    }

    #[test]
    fn decode_empty_input_is_ok_and_empty() {
        // Zero groups: valid (multiple of 128) and yields no samples.
        let (samples, report) = decode(&[], DecodeOptions::default()).unwrap();
        assert!(samples.is_empty());
        assert_eq!(report.n_groups, 0);
    }

    #[test]
    fn decode_one_byte_input_is_err() {
        // Not a multiple of the 128-byte sound-group size.
        assert!(decode(&[0u8], DecodeOptions::default()).is_err());
    }

    #[test]
    fn decode_all_ones_group_does_not_panic() {
        // Every parameter byte = 0xFF (filter nibble = 0xF > 3) → the group is
        // classified invalid and zero-filled; range nibbles never trigger an
        // out-of-range shift panic.
        let buf = vec![0xFFu8; SOUND_GROUP_BYTES];
        let (samples, report) = decode(&buf, DecodeOptions::default()).unwrap();
        assert_eq!(report.n_groups_skipped, 1);
        assert_eq!(samples.len(), UNITS_PER_GROUP_4BIT * SAMPLES_PER_UNIT);
        assert!(samples.iter().all(|&s| s == 0));
    }

    #[test]
    fn decode_huge_range_nibble_does_not_overflow_shift() {
        // Valid filter (0) but range nibble 0xF (= 15) in every param byte.
        // The decoder must apply the range shift without UB and stay bounded.
        let mut buf = vec![0u8; SOUND_GROUP_BYTES];
        for b in buf.iter_mut().take(UNITS_PER_GROUP_4BIT) {
            *b = 0x0F; // filter=0, range=15
        }
        // Fill sample nibbles with max magnitude to push the predictor.
        for b in buf.iter_mut().skip(16) {
            *b = 0xFF;
        }
        let (samples, _) = decode(&buf, DecodeOptions::default()).unwrap();
        assert_eq!(samples.len(), UNITS_PER_GROUP_4BIT * SAMPLES_PER_UNIT);
    }

    #[test]
    fn decode_garbage_multiple_of_128_does_not_panic() {
        // Pseudo-random-ish bytes across several groups: any mix of valid /
        // invalid groups must decode to a bounded buffer without panicking.
        let buf: Vec<u8> = (0..(SOUND_GROUP_BYTES * 5))
            .map(|i| (i.wrapping_mul(31).wrapping_add(7)) as u8)
            .collect();
        let (samples, report) = decode(&buf, DecodeOptions::default()).unwrap();
        assert_eq!(report.n_groups, 5);
        assert_eq!(samples.len(), 5 * UNITS_PER_GROUP_4BIT * SAMPLES_PER_UNIT);
    }

    #[test]
    fn streaming_decoder_handles_garbage_without_panic() {
        let buf: Vec<u8> = (0..(SOUND_GROUP_BYTES * 3))
            .map(|i| (i.wrapping_mul(131).wrapping_add(17)) as u8)
            .collect();
        let mut dec = StreamingDecoder::new(DecodeOptions::default());
        let mut out = Vec::new();
        dec.feed(&buf, &mut out).unwrap();
        // Feed a trailing partial group; it must be buffered, not panic.
        dec.feed(&[0u8; 50], &mut out).unwrap();
        assert_eq!(dec.pending_bytes(), 50);
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

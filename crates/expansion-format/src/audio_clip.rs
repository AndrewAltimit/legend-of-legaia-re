//! Decoding a pack's own audio files: WAV (PCM or float) and Ogg Vorbis
//! under `assets/`, into interleaved 16-bit PCM the engine mixes.
//!
//! One decoder serves the validator, the native window and the browser page,
//! so a file that validates is a file both hosts play, sample for sample.
//! Ogg Vorbis goes through `lewton` (pure Rust); WAV is read here.
//!
//! Both containers can carry a loop of their own, which [`DecodedClip::loop_points`]
//! reports and a pack's `loop_start` / `loop_end` override:
//!
//! * WAV: the first loop of a `smpl` chunk (`dwStart`, `dwEnd` inclusive).
//! * Ogg: the `LOOPSTART` / `LOOPLENGTH` (or `LOOPEND`) comments, in sample
//!   frames, the convention game-music tools write.

use anyhow::{Context, Result, bail, ensure};

/// The longest clip a pack may carry, in seconds of audio. A clip is held
/// decoded while it plays, at four bytes a stereo frame.
pub const MAX_CLIP_SECONDS: u32 = 360;
/// The sample rates accepted, inclusive.
pub const RATE_RANGE: std::ops::RangeInclusive<u32> = 8_000..=96_000;

/// A decoded clip: interleaved signed 16-bit samples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedClip {
    pub sample_rate: u32,
    /// `1` (mono) or `2` (stereo).
    pub channels: u8,
    /// `frames * channels` samples, interleaved.
    pub samples: Vec<i16>,
    /// The loop the file itself carries, `[start, end)` in frames.
    pub loop_points: Option<(u32, u32)>,
}

impl DecodedClip {
    /// Sample frames (one sample per channel).
    pub fn frames(&self) -> u32 {
        (self.samples.len() / usize::from(self.channels.max(1))) as u32
    }

    /// Root-mean-square level of the whole clip, `0.0..=1.0` of full scale,
    /// every channel together.
    pub fn rms(&self) -> f32 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let sum: f64 = self
            .samples
            .iter()
            .map(|&s| {
                let v = f64::from(s) / 32768.0;
                v * v
            })
            .sum();
        (sum / self.samples.len() as f64).sqrt() as f32
    }

    /// The largest absolute sample, `0.0..=1.0` of full scale.
    pub fn peak(&self) -> f32 {
        self.samples
            .iter()
            .map(|&s| i32::from(s).unsigned_abs())
            .max()
            .map_or(0.0, |p| p as f32 / 32768.0)
    }
}

/// What a file is without decoding its audio: enough to check loop points
/// and lengths in the workbench.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipInfo {
    pub sample_rate: u32,
    pub channels: u8,
    pub frames: u32,
    pub loop_points: Option<(u32, u32)>,
}

impl ClipInfo {
    /// The clip's length in seconds.
    pub fn seconds(&self) -> f32 {
        self.frames as f32 / self.sample_rate.max(1) as f32
    }
}

/// The container a file's bytes are, by signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipFormat {
    Wav,
    Ogg,
}

impl ClipFormat {
    /// The format of `bytes`, by its signature.
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
            Some(Self::Wav)
        } else if bytes.starts_with(b"OggS") {
            Some(Self::Ogg)
        } else {
            None
        }
    }
}

/// Decode a WAV or Ogg Vorbis file.
pub fn decode(bytes: &[u8]) -> Result<DecodedClip> {
    let clip = match ClipFormat::sniff(bytes) {
        Some(ClipFormat::Wav) => decode_wav(bytes)?,
        Some(ClipFormat::Ogg) => decode_ogg(bytes)?,
        None => bail!("not a WAV (RIFF/WAVE) or Ogg file"),
    };
    check_shape(clip.sample_rate, clip.channels, clip.frames())?;
    Ok(clip)
}

/// Read a file's rate, channels, length and own loop without decoding the
/// audio (Ogg reads its headers and the last page's granule position).
pub fn probe(bytes: &[u8]) -> Result<ClipInfo> {
    let info = match ClipFormat::sniff(bytes) {
        Some(ClipFormat::Wav) => {
            let w = parse_wav(bytes)?;
            ClipInfo {
                sample_rate: w.rate,
                channels: w.channels,
                frames: (w.data.len() / w.frame_bytes()) as u32,
                loop_points: w.loop_points,
            }
        }
        Some(ClipFormat::Ogg) => {
            let reader = lewton::inside_ogg::OggStreamReader::new(std::io::Cursor::new(bytes))
                .map_err(|e| anyhow::anyhow!("Ogg Vorbis headers: {e}"))?;
            let channels = reader.ident_hdr.audio_channels;
            let rate = reader.ident_hdr.audio_sample_rate;
            let frames = last_granule(bytes).context("the Ogg stream has no final page")?;
            ClipInfo {
                sample_rate: rate,
                channels,
                frames: u32::try_from(frames).unwrap_or(u32::MAX),
                loop_points: comment_loop(&reader.comment_hdr.comment_list),
            }
        }
        None => bail!("not a WAV (RIFF/WAVE) or Ogg file"),
    };
    check_shape(info.sample_rate, info.channels, info.frames)?;
    Ok(info)
}

fn check_shape(rate: u32, channels: u8, frames: u32) -> Result<()> {
    ensure!(
        RATE_RANGE.contains(&rate),
        "sample rate {rate} Hz is outside {}..={} Hz",
        RATE_RANGE.start(),
        RATE_RANGE.end()
    );
    ensure!(
        channels == 1 || channels == 2,
        "{channels} channels; a clip is mono or stereo"
    );
    ensure!(frames > 0, "the clip holds no audio");
    ensure!(
        u64::from(frames) <= u64::from(MAX_CLIP_SECONDS) * u64::from(rate),
        "the clip runs {} s; the limit is {MAX_CLIP_SECONDS} s",
        frames / rate
    );
    Ok(())
}

/// The parsed `fmt ` / `data` / `smpl` chunks of a WAV file.
struct Wav<'a> {
    rate: u32,
    channels: u8,
    bits: u16,
    float: bool,
    data: &'a [u8],
    loop_points: Option<(u32, u32)>,
}

impl Wav<'_> {
    fn frame_bytes(&self) -> usize {
        usize::from(self.channels) * usize::from(self.bits / 8)
    }
}

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn parse_wav(bytes: &[u8]) -> Result<Wav<'_>> {
    let mut fmt = None;
    let mut data = None;
    let mut loop_points = None;
    let mut at = 12usize;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let len = u32_at(bytes, at + 4).unwrap_or(0) as usize;
        let body = at + 8;
        let end = body.saturating_add(len).min(bytes.len());
        let chunk = &bytes[body..end];
        match id {
            b"fmt " => fmt = Some(chunk),
            b"data" => data = Some(chunk),
            b"smpl" => {
                // `cSampleLoops` at +28, the first loop record at +36:
                // id, type, start, end (inclusive), fraction, play count.
                if u32_at(chunk, 28).unwrap_or(0) > 0
                    && let (Some(s), Some(e)) = (u32_at(chunk, 44), u32_at(chunk, 48))
                    && e >= s
                {
                    loop_points = Some((s, e.saturating_add(1)));
                }
            }
            _ => {}
        }
        // Chunks are word-aligned: an odd length carries a pad byte.
        at = body.saturating_add(len).saturating_add(len & 1);
    }
    let fmt = fmt.context("the WAV file has no `fmt ` chunk")?;
    let data = data.context("the WAV file has no `data` chunk")?;
    let tag = u16_at(fmt, 0).context("short `fmt ` chunk")?;
    let channels = u16_at(fmt, 2).context("short `fmt ` chunk")?;
    let rate = u32_at(fmt, 4).context("short `fmt ` chunk")?;
    let bits = u16_at(fmt, 14).context("short `fmt ` chunk")?;
    // WAVE_FORMAT_EXTENSIBLE carries the real tag as its sub-format GUID's
    // first two bytes.
    let tag = if tag == 0xFFFE {
        u16_at(fmt, 24).context("short extensible `fmt ` chunk")?
    } else {
        tag
    };
    let float = match (tag, bits) {
        (1, 8 | 16 | 24 | 32) => false,
        (3, 32) => true,
        _ => bail!("WAV encoding {tag} at {bits} bits; use 8/16/24/32-bit PCM or 32-bit float"),
    };
    ensure!(
        channels == 1 || channels == 2,
        "{channels} channels; a clip is mono or stereo"
    );
    Ok(Wav {
        rate,
        channels: channels as u8,
        bits,
        float,
        data,
        loop_points,
    })
}

fn decode_wav(bytes: &[u8]) -> Result<DecodedClip> {
    let w = parse_wav(bytes)?;
    let width = usize::from(w.bits / 8);
    let usable = w.data.len() / w.frame_bytes() * w.frame_bytes();
    let samples = w.data[..usable]
        .chunks_exact(width)
        .map(|s| match (width, w.float) {
            (1, _) => (i16::from(s[0]) - 128) << 8,
            (2, _) => i16::from_le_bytes([s[0], s[1]]),
            (3, _) => i16::from_le_bytes([s[1], s[2]]),
            (4, false) => i16::from_le_bytes([s[2], s[3]]),
            _ => {
                let f = f32::from_le_bytes([s[0], s[1], s[2], s[3]]);
                (f.clamp(-1.0, 1.0) * 32767.0).round() as i16
            }
        })
        .collect();
    Ok(DecodedClip {
        sample_rate: w.rate,
        channels: w.channels,
        samples,
        loop_points: w.loop_points,
    })
}

fn decode_ogg(bytes: &[u8]) -> Result<DecodedClip> {
    let mut reader = lewton::inside_ogg::OggStreamReader::new(std::io::Cursor::new(bytes))
        .map_err(|e| anyhow::anyhow!("Ogg Vorbis headers: {e}"))?;
    let channels = reader.ident_hdr.audio_channels;
    let rate = reader.ident_hdr.audio_sample_rate;
    ensure!(
        channels == 1 || channels == 2,
        "{channels} channels; a clip is mono or stereo"
    );
    ensure!(
        RATE_RANGE.contains(&rate),
        "sample rate {rate} Hz is outside {}..={} Hz",
        RATE_RANGE.start(),
        RATE_RANGE.end()
    );
    let limit = MAX_CLIP_SECONDS as usize * rate as usize * usize::from(channels);
    let mut samples = Vec::new();
    while let Some(packet) = reader
        .read_dec_packet_itl()
        .map_err(|e| anyhow::anyhow!("Ogg Vorbis audio: {e}"))?
    {
        samples.extend_from_slice(&packet);
        ensure!(
            samples.len() <= limit,
            "the clip runs past {MAX_CLIP_SECONDS} s"
        );
    }
    // The last page's granule position is the stream's true length; the
    // final packet decodes whole, so cut its padding off.
    if let Some(end) = last_granule(bytes) {
        let keep = usize::try_from(end)
            .unwrap_or(usize::MAX)
            .saturating_mul(usize::from(channels));
        samples.truncate(keep);
    }
    let loop_points = comment_loop(&reader.comment_hdr.comment_list);
    Ok(DecodedClip {
        sample_rate: rate,
        channels,
        samples,
        loop_points,
    })
}

/// The loop the `LOOPSTART` / `LOOPLENGTH` / `LOOPEND` comments name.
fn comment_loop(comments: &[(String, String)]) -> Option<(u32, u32)> {
    let get = |key: &str| {
        comments
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .and_then(|(_, v)| v.trim().parse::<u32>().ok())
    };
    let start = get("LOOPSTART")?;
    let end = get("LOOPEND").or_else(|| Some(start.saturating_add(get("LOOPLENGTH")?)))?;
    (end > start).then_some((start, end))
}

/// The granule position (total sample frames) of the stream's last page.
fn last_granule(bytes: &[u8]) -> Option<u64> {
    // Scan back for the last page header: `OggS`, version 0.
    let mut at = bytes.len().checked_sub(27)?;
    loop {
        if &bytes[at..at + 4] == b"OggS" && bytes[at + 4] == 0 {
            let g = u64::from_le_bytes(bytes.get(at + 6..at + 14)?.try_into().ok()?);
            if g != u64::MAX {
                return Some(g);
            }
        }
        at = at.checked_sub(1)?;
    }
}

/// Encode interleaved 16-bit PCM as a WAV file, with a `smpl` loop when
/// `loop_points` is set: what the procedural asset scripts and the tests
/// write.
pub fn encode_wav(
    sample_rate: u32,
    channels: u8,
    samples: &[i16],
    loop_points: Option<(u32, u32)>,
) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(samples.len() * 2 + 128);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&u16::from(channels).to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    let block = u32::from(channels) * 2;
    out.extend_from_slice(&(sample_rate * block).to_le_bytes());
    out.extend_from_slice(&(block as u16).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    if let Some((s, e)) = loop_points {
        out.extend_from_slice(b"smpl");
        out.extend_from_slice(&60u32.to_le_bytes());
        let mut smpl = [0u32; 15];
        smpl[2] = 1_000_000_000 / sample_rate.max(1);
        smpl[4] = 60;
        smpl[7] = 1;
        smpl[11] = s;
        smpl[12] = e.saturating_sub(1);
        for w in smpl {
            out.extend_from_slice(&w.to_le_bytes());
        }
    }
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    let riff = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&riff.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frames: usize) -> Vec<i16> {
        (0..frames)
            .flat_map(|i| {
                let v = ((i as f32 * 0.05).sin() * 8000.0) as i16;
                [v, -v]
            })
            .collect()
    }

    #[test]
    fn wav_round_trips_with_its_loop() {
        let pcm = tone(4410);
        let bytes = encode_wav(22050, 2, &pcm, Some((100, 4000)));
        let clip = decode(&bytes).unwrap();
        assert_eq!(clip.sample_rate, 22050);
        assert_eq!(clip.channels, 2);
        assert_eq!(clip.samples, pcm);
        assert_eq!(clip.loop_points, Some((100, 4000)));
        let info = probe(&bytes).unwrap();
        assert_eq!(info.frames, 4410);
        assert_eq!(info.loop_points, Some((100, 4000)));
        assert!(clip.rms() > 0.1 && clip.peak() <= 8000.0 / 32768.0 + 1e-4);
    }

    #[test]
    fn wav_widths_and_float_decode_to_16_bit() {
        let mut b = encode_wav(8000, 1, &[0x1234, -2], None);
        // Rewrite as 8-bit: tag 1, bits 8, the two data bytes as unsigned.
        b[34] = 8;
        let data_at = b.len() - 4;
        b[data_at - 4..data_at].copy_from_slice(&2u32.to_le_bytes());
        b.truncate(data_at + 2);
        b[data_at] = 0x92;
        b[data_at + 1] = 0x7F;
        let c = decode(&b).unwrap();
        assert_eq!(c.samples, vec![0x1200, -0x100]);
    }

    #[test]
    fn rejects_what_is_not_a_clip() {
        assert!(decode(b"not audio").is_err());
        let silent = encode_wav(44100, 2, &[], None);
        assert!(decode(&silent).is_err(), "an empty clip");
        let fast = encode_wav(192_000, 1, &[1, 2], None);
        assert!(probe(&fast).is_err(), "rate out of range");
    }

    #[test]
    fn ogg_comment_loops() {
        let c = |k: &str, v: &str| (k.to_string(), v.to_string());
        assert_eq!(
            comment_loop(&[c("LOOPSTART", "10"), c("LOOPLENGTH", "90")]),
            Some((10, 100))
        );
        assert_eq!(
            comment_loop(&[c("loopstart", "5"), c("LOOPEND", "50")]),
            Some((5, 50))
        );
        assert_eq!(comment_loop(&[c("LOOPSTART", "5")]), None);
    }

    #[test]
    fn garbage_ogg_is_an_error_not_a_panic() {
        let mut b = b"OggS".to_vec();
        b.extend_from_slice(&[0; 200]);
        assert!(decode(&b).is_err());
        assert!(probe(&b).is_err());
    }
}

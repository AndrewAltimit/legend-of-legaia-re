//! One-pass demux of an interleaved STR stream into its video frames, its
//! playback timing and its XA soundtrack - the kernel every movie host opens
//! a movie through.
//!
//! Legaia's `MOV/MV*.STR` files interleave Mode 2 Form 1 MDEC video sectors
//! (magic `0x0160`) with the cutscene's XA track (Mode 2 Form 2). Both tracks
//! are pulled from the same raw 2352-byte sector stream so their alignment is
//! the disc's own.
//!
//! The demuxer keeps each assembled frame as its **bitstream**, not as pixels:
//! the browser decodes one frame per request, the native window decodes them
//! all up front - both from the same frame list. Three rules every host gets
//! from here instead of re-deriving:
//!
//! - **Frame indexing is by assembled frame.** A frame that later fails to
//!   decode keeps its slot (a host shows the previous picture), so the
//!   timeline and the recovered frame rate do not depend on decode success.
//! - **Frame rate** is the mean sectors-per-frame over the whole window at the
//!   2x CD rate - every sector counts, audio included, exactly as
//!   [`crate::str_sector::analyze_str_timing`] measures a Form-1 extract.
//! - **The soundtrack** is the dominant `(file_no, ch_no)` channel, decoded at
//!   the subheader's own sample width (4- or 8-bit) and rate.

use crate::str_sector::{CD_SECTORS_PER_SEC_2X, SECTOR_DATA_BYTES, StrFrameAssembler, StrTiming};
use std::collections::BTreeMap;

/// Raw CD sector size the demuxer is fed.
pub const RAW_SECTOR_BYTES: usize = 2352;
const SUBHEADER_OFFSET: usize = legaia_xa::demux::SUBHEADER_OFFSET;
const USER_DATA_OFFSET: usize = legaia_xa::demux::USER_DATA_OFFSET;
const AUDIO_BYTES_PER_SECTOR: usize = legaia_xa::demux::AUDIO_BYTES_PER_SECTOR;

/// One assembled, not yet decoded, video frame.
#[derive(Debug, Clone)]
pub struct StrFrameBits {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// The sector header's frame number.
    pub frame_number: u32,
    /// The concatenated Iki bitstream ([`crate::MdecDecoder::decode_frame`]).
    pub bitstream: Vec<u8>,
}

impl StrFrameBits {
    /// Decode to row-major RGBA8; `None` on a bitstream error.
    pub fn decode_rgba(&self) -> Option<Vec<u8>> {
        crate::MdecDecoder::new(self.width, self.height)
            .decode_frame(&self.bitstream)
            .ok()
    }
}

/// The decoded soundtrack of an STR window.
#[derive(Debug, Clone)]
pub struct StrAudio {
    /// Interleaved PCM (L,R,L,R,... for stereo).
    pub pcm: Vec<i16>,
    /// Output sample rate in Hz.
    pub sample_rate: u32,
    /// Mono or stereo.
    pub channels: legaia_xa::Channels,
    /// Source XA file number.
    pub file_no: u8,
    /// Source XA channel number.
    pub ch_no: u8,
}

impl StrAudio {
    /// Whether the track is stereo.
    pub fn stereo(&self) -> bool {
        matches!(self.channels, legaia_xa::Channels::Stereo)
    }

    /// Playback length in seconds.
    pub fn duration_secs(&self) -> f64 {
        let frames = self.pcm.len() / self.channels.n() as usize;
        if self.sample_rate == 0 {
            0.0
        } else {
            frames as f64 / self.sample_rate as f64
        }
    }
}

/// A demuxed STR window.
#[derive(Debug, Clone, Default)]
pub struct StrAv {
    /// Every assembled video frame, in stream order.
    pub frames: Vec<StrFrameBits>,
    /// Sector stride and frame rate.
    pub timing: StrTiming,
    /// The dominant XA channel, decoded; `None` when the window carries no
    /// decodable track.
    pub audio: Option<StrAudio>,
}

impl StrAv {
    /// `(width, height)` of the first frame; `(0, 0)` with no frames.
    pub fn size(&self) -> (u32, u32) {
        self.frames.first().map_or((0, 0), |f| (f.width, f.height))
    }

    /// Seconds per frame, falling back to the retail 15 fps on a degenerate
    /// stream ([`StrTiming::frame_period`]).
    pub fn frame_period_secs(&self) -> f64 {
        self.timing.frame_period().as_secs_f64()
    }
}

struct AudioAcc {
    sample_rate: u32,
    stereo: bool,
    bits_per_sample: u8,
    audio: Vec<u8>,
}

/// Incremental demuxer: feed raw 2352-byte sectors in disc order, then
/// [`Self::finish`].
#[derive(Default)]
pub struct StrAvDemuxer {
    asm: StrFrameAssembler,
    frames: Vec<StrFrameBits>,
    audio: BTreeMap<(u8, u8), AudioAcc>,
    sectors: usize,
}

impl StrAvDemuxer {
    /// A fresh demuxer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Route one raw sector: a Form-2 audio sector to its channel buffer,
    /// anything else through the frame assembler (which skips non-video
    /// user data). A short sector still counts toward the stride.
    pub fn push_raw_sector(&mut self, raw: &[u8]) {
        self.sectors += 1;
        if raw.len() < USER_DATA_OFFSET + SECTOR_DATA_BYTES {
            return;
        }
        let mut sub = [0u8; 8];
        sub.copy_from_slice(&raw[SUBHEADER_OFFSET..SUBHEADER_OFFSET + 8]);
        let (sh, ok) = legaia_xa::demux::parse_subheader(&sub);
        if ok && sh.is_audio() && sh.is_form2() {
            let end = USER_DATA_OFFSET + AUDIO_BYTES_PER_SECTOR;
            if end <= raw.len() {
                self.audio
                    .entry((sh.file_no, sh.ch_no))
                    .or_insert_with(|| AudioAcc {
                        sample_rate: sh.sample_rate(),
                        stereo: sh.is_stereo(),
                        bits_per_sample: sh.bits_per_sample(),
                        audio: Vec::new(),
                    })
                    .audio
                    .extend_from_slice(&raw[USER_DATA_OFFSET..end]);
            }
            return;
        }
        let user = &raw[USER_DATA_OFFSET..USER_DATA_OFFSET + SECTOR_DATA_BYTES];
        // A malformed video header drops that sector, not the movie.
        if let Ok(Some((hdr, bs))) = self.asm.push_sector(user) {
            self.frames.push(StrFrameBits {
                width: hdr.width as u32,
                height: hdr.height as u32,
                frame_number: hdr.frame_number,
                bitstream: bs,
            });
        }
    }

    /// Close the window: recover the timing and decode the soundtrack.
    pub fn finish(self) -> StrAv {
        let n = self.frames.len();
        let spf = if n == 0 {
            0.0
        } else {
            self.sectors as f64 / n as f64
        };
        let timing = StrTiming {
            sector_count: self.sectors,
            frame_count: n,
            sectors_per_frame: spf,
            fps: if n == 0 {
                0.0
            } else {
                CD_SECTORS_PER_SEC_2X / spf
            },
        };
        let audio = self
            .audio
            .into_iter()
            .max_by_key(|(_, acc)| acc.audio.len())
            .and_then(|((file_no, ch_no), acc)| {
                let bits = match acc.bits_per_sample {
                    4 => legaia_xa::BitsPerSample::Four,
                    8 => legaia_xa::BitsPerSample::Eight,
                    _ => return None,
                };
                let channels = if acc.stereo {
                    legaia_xa::Channels::Stereo
                } else {
                    legaia_xa::Channels::Mono
                };
                let opts = legaia_xa::DecodeOptions {
                    channels,
                    sample_rate: acc.sample_rate,
                    bits,
                };
                let (pcm, _) = legaia_xa::decode(&acc.audio, opts).ok()?;
                Some(StrAudio {
                    pcm,
                    sample_rate: acc.sample_rate,
                    channels,
                    file_no,
                    ch_no,
                })
            });
        StrAv {
            frames: self.frames,
            timing,
            audio,
        }
    }
}

/// Demux a contiguous buffer of raw 2352-byte sectors (a trailing partial
/// sector is ignored).
pub fn demux_str_av(raw_sectors: &[u8]) -> StrAv {
    let mut d = StrAvDemuxer::new();
    for raw in raw_sectors.as_chunks::<RAW_SECTOR_BYTES>().0 {
        d.push_raw_sector(raw);
    }
    d.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video_sector(frame: u32, chunk: u16, chunks: u16) -> Vec<u8> {
        let mut raw = vec![0u8; RAW_SECTOR_BYTES];
        // Form-1 data subheader (not audio).
        raw[SUBHEADER_OFFSET + 2] = 0x08;
        raw[SUBHEADER_OFFSET + 6] = 0x08;
        let u = &mut raw[USER_DATA_OFFSET..];
        u[0..2].copy_from_slice(&0x0160u16.to_le_bytes());
        u[2..4].copy_from_slice(&0x8001u16.to_le_bytes());
        u[4..6].copy_from_slice(&chunk.to_le_bytes());
        u[6..8].copy_from_slice(&chunks.to_le_bytes());
        u[8..12].copy_from_slice(&frame.to_le_bytes());
        u[12..16].copy_from_slice(&64u32.to_le_bytes());
        u[16..18].copy_from_slice(&16u16.to_le_bytes());
        u[18..20].copy_from_slice(&16u16.to_le_bytes());
        raw
    }

    fn audio_sector(bits8: bool) -> Vec<u8> {
        let mut raw = vec![0u8; RAW_SECTOR_BYTES];
        // submode: audio (0x04) + form2 (0x20); coding: stereo, 37.8k, 4/8-bit.
        let coding = 0x01 | if bits8 { 0x10 } else { 0 };
        for base in [SUBHEADER_OFFSET, SUBHEADER_OFFSET + 4] {
            raw[base] = 1;
            raw[base + 1] = 0;
            raw[base + 2] = 0x24;
            raw[base + 3] = coding;
        }
        raw
    }

    #[test]
    fn frames_rate_and_audio_from_one_pass() {
        let mut buf = Vec::new();
        for f in 0..3u32 {
            buf.extend(video_sector(f, 0, 1));
            for _ in 0..9 {
                buf.extend(audio_sector(false));
            }
        }
        let av = demux_str_av(&buf);
        assert_eq!(av.frames.len(), 3);
        assert_eq!(av.timing.sector_count, 30);
        assert!((av.timing.fps - 15.0).abs() < 1e-9);
        let a = av.audio.expect("track");
        assert!(a.stereo());
        assert_eq!(a.sample_rate, 37_800);
    }

    #[test]
    fn every_assembled_frame_counts_toward_the_timeline() {
        // Frames are counted on assembly, never on decode success.
        let mut buf = Vec::new();
        for f in 0..4u32 {
            buf.extend(video_sector(f, 0, 1));
        }
        let av = demux_str_av(&buf);
        assert_eq!(av.frames.len(), 4);
        assert!((av.timing.fps - 150.0).abs() < 1e-9);
    }

    #[test]
    fn eight_bit_track_is_decoded_at_its_own_width() {
        let mut buf = Vec::new();
        buf.extend(video_sector(0, 0, 1));
        buf.extend(audio_sector(true));
        let av = demux_str_av(&buf);
        let a = av.audio.expect("8-bit track decodes");
        // 18 groups x 4 units x 28 samples, stereo -> 1008 sample frames.
        assert_eq!(a.pcm.len() / 2, 18 * 4 * 28 / 2);
    }

    #[test]
    fn empty_window_falls_back_to_retail_rate() {
        let av = demux_str_av(&[]);
        assert!(av.frames.is_empty() && av.audio.is_none());
        assert!((av.frame_period_secs() - 1.0 / 15.0).abs() < 1e-9);
    }
}

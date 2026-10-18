//! Streamed media on the disc: the CD-XA audio files (`XA/*.XA`, voice and
//! streamed music) and the STR movies (`MOV/*.STR`).
//!
//! Both are read as raw 2352-byte Mode 2 sectors, which only a disc image
//! has: a Form 1 copy in an extracted folder has lost the Form 2 audio and
//! the per-sector subheaders (`crates/xa/README.md`). XA files are
//! demultiplexed per `(file, channel)` with `legaia_xa::demux` and decoded
//! with `legaia_xa::decode`; movies go through `legaia_mdec`'s
//! `StrAvDemuxer` (frame bitstreams, the sector-stride frame rate and the
//! dominant XA track) and decode one frame at a time with the Iki decoder.

use std::path::Path;

use anyhow::{Context, Result};
use legaia_iso::raw::{RawDisc, USER_DATA_SIZE};
use legaia_mdec::str_av::{StrAudio, StrAv, StrAvDemuxer};
use legaia_xa::demux::ChannelStream;
use legaia_xa::{BitsPerSample, Channels, DecodeOptions};

use crate::library::IsoFile;

/// What kind of streamed file an ISO path names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Xa,
    Str,
}

/// The kind of `path`, by extension.
pub fn kind_of(path: &str) -> Option<MediaKind> {
    let p = path.to_ascii_uppercase();
    if p.ends_with(".XA") {
        Some(MediaKind::Xa)
    } else if p.ends_with(".STR") {
        Some(MediaKind::Str)
    } else {
        None
    }
}

/// Sort key that orders `XA/XA2.XA` before `XA/XA10.XA`: the path with
/// its digit run read as a number.
pub fn natural_key(path: &str) -> (String, u64) {
    let digits: String = path.chars().filter(char::is_ascii_digit).collect();
    let stem: String = path.chars().filter(|c| !c.is_ascii_digit()).collect();
    (stem, digits.parse().unwrap_or(0))
}

/// Sectors a file of `size` bytes spans (directory sizes count 2048-byte
/// Form 1 sectors, also for the Form 2 streams).
fn sectors(f: &IsoFile) -> u32 {
    f.size.div_ceil(USER_DATA_SIZE as u32)
}

/// One decoded XA channel.
pub struct XaTrack {
    pub file_no: u8,
    pub ch_no: u8,
    /// Interleaved PCM (L, R for stereo).
    pub pcm: Vec<i16>,
    pub sample_rate: u32,
    pub channels: Channels,
}

impl XaTrack {
    pub fn duration_secs(&self) -> f64 {
        let frames = self.pcm.len() / self.channels.n() as usize;
        frames as f64 / self.sample_rate.max(1) as f64
    }
}

/// Demultiplex the XA file `f` of disc image `bin` into its channels.
pub fn xa_channels(bin: &Path, f: &IsoFile) -> Result<Vec<ChannelStream>> {
    let mut disc = RawDisc::open(bin)?;
    legaia_xa::demux::demux_disc_range(&mut disc, f.lba, sectors(f))
        .with_context(|| format!("demultiplexing {}", f.path))
}

/// Decode one demultiplexed channel at its own subheader rate, layout and
/// sample width.
pub fn decode_channel(ch: &ChannelStream) -> Result<XaTrack> {
    let channels = if ch.stereo {
        Channels::Stereo
    } else {
        Channels::Mono
    };
    let bits = if ch.bits_per_sample == 8 {
        BitsPerSample::Eight
    } else {
        BitsPerSample::Four
    };
    let (pcm, _) = legaia_xa::decode(
        &ch.audio,
        DecodeOptions {
            channels,
            sample_rate: ch.sample_rate,
            bits,
        },
    )?;
    Ok(XaTrack {
        file_no: ch.file_no,
        ch_no: ch.ch_no,
        pcm,
        sample_rate: ch.sample_rate,
        channels,
    })
}

/// Demultiplex the STR movie `f` of disc image `bin`: every frame's
/// bitstream (not yet decoded), its frame rate and its soundtrack.
pub fn str_movie(bin: &Path, f: &IsoFile) -> Result<StrAv> {
    let mut disc = RawDisc::open(bin)?;
    let mut demux = StrAvDemuxer::new();
    for s in 0..sectors(f) {
        let raw = disc
            .read_raw_sector(f.lba + s)
            .with_context(|| format!("reading sector {} of {}", f.lba + s, f.path))?;
        demux.push_raw_sector(&raw);
    }
    Ok(demux.finish())
}

/// A movie soundtrack from `secs` in: the interleaved PCM from the sample
/// frame at that time (rounded down to a whole L/R pair), empty past the
/// end. What a movie resumed from a paused frame plays.
pub fn soundtrack_from(audio: &StrAudio, secs: f64) -> Vec<i16> {
    let ch = audio.channels.n() as usize;
    let frame = (secs.max(0.0) * audio.sample_rate as f64) as usize;
    audio.pcm.get(frame * ch..).unwrap_or(&[]).to_vec()
}

/// Write `pcm` as a 16-bit WAV.
pub fn write_wav(path: &Path, pcm: &[i16], channels: Channels, rate: u32) -> Result<()> {
    legaia_xa::write_wav(path, pcm, channels, rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resumed_soundtrack_starts_on_a_whole_sample_frame() {
        // 10 stereo frames at 10 Hz: frame k is (2k, 2k+1).
        let audio = StrAudio {
            pcm: (0..20).collect(),
            sample_rate: 10,
            channels: Channels::Stereo,
            file_no: 1,
            ch_no: 0,
        };
        assert_eq!(soundtrack_from(&audio, 0.0), audio.pcm);
        // 0.35 s = frame 3.5 -> frame 3, starting on its left sample.
        assert_eq!(soundtrack_from(&audio, 0.35)[..2], [6, 7]);
        assert_eq!(soundtrack_from(&audio, 0.35).len(), 14);
        assert!(soundtrack_from(&audio, 5.0).is_empty());
    }
}

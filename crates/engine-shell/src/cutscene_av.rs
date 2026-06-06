//! Combined STR (MDEC video) + interleaved XA audio decoding for cutscene
//! playback, plus the shared audio-driven playback clock.
//!
//! The Legaia `MOV/MV*.STR` files are CD-XA streams that **interleave** the
//! MDEC video sectors (Mode 2 Form 1, magic `0x0160`) with the cutscene's XA
//! audio track (Mode 2 Form 2, all on file/channel `(1, 0)`, stereo 37.8 kHz
//! 4-bit). The Form-1 extract written to `extracted/MOV/*.STR` keeps the video
//! sectors intact but truncates each Form-2 audio sector from 2324 bytes to
//! 2048, corrupting the audio - so faithful A/V playback reads the raw 2352-
//! byte sectors straight off the disc image, where both tracks are present.
//!
//! [`decode_str_av_from_disc`] does exactly that in a single pass: it routes
//! audio sectors to a per-`(file_no, ch_no)` buffer (à la [`legaia_xa::demux`])
//! and the remaining sectors' 2048-byte user data through the
//! [`StrFrameAssembler`], decoding the dominant audio channel to PCM and the
//! video to RGBA frames. The interleaving (and thus the A/V alignment) is
//! preserved because both tracks are pulled from the same sector stream.
//!
//! Once the audio is playing, the video clock is driven off the audio cursor
//! ([`due_video_frame`]): the visible frame is `audio_position / frame_period`,
//! so the picture stays locked to the soundtrack instead of free-running on a
//! separate wall-clock timer (which drifts against the hardware audio rate).
//! When there is no audio track the same function falls back to a wall-clock
//! position, matching the prior video-only behaviour.

use anyhow::{Context, Result};
use legaia_iso::raw::{RawDisc, SECTOR_SIZE};
use legaia_mdec::str_sector::{StrFrameAssembler, StrTiming, analyze_str_timing};
use legaia_mdec::{MdecDecoder, VideoFrame};
use std::collections::BTreeMap;
use std::path::Path;

/// Subheader offset within a raw 2352-byte CD sector (shared with the XA demux).
const SUBHEADER_OFFSET: usize = legaia_xa::demux::SUBHEADER_OFFSET;
/// User-data offset within a raw sector (Form 1 video data starts here).
const USER_DATA_OFFSET: usize = legaia_xa::demux::USER_DATA_OFFSET;
/// Form-1 video user-data length.
const VIDEO_USER_DATA: usize = legaia_iso::raw::USER_DATA_SIZE;
/// Audio bytes per Form-2 sector (18 sound groups x 128).
const AUDIO_BYTES_PER_SECTOR: usize = legaia_xa::demux::AUDIO_BYTES_PER_SECTOR;

/// The decoded XA audio track interleaved in a cutscene STR stream, ready to
/// hand to [`legaia_engine_audio::AudioOut::play_xa`].
#[derive(Debug, Clone)]
pub struct CutsceneAudio {
    /// Decoded interleaved PCM (stereo = L,R,L,R,...; mono = duplicated at mix).
    pub pcm: Vec<i16>,
    pub sample_rate: u32,
    pub channels: legaia_xa::Channels,
    /// Source `(file_no, ch_no)` the audio came from (informational).
    pub file_no: u8,
    pub ch_no: u8,
}

impl CutsceneAudio {
    /// Total playback duration in seconds.
    pub fn duration_secs(&self) -> f64 {
        let frames = self.pcm.len() / self.channels.n() as usize;
        if self.sample_rate == 0 {
            0.0
        } else {
            frames as f64 / self.sample_rate as f64
        }
    }
}

/// Result of decoding a cutscene STR straight off the disc image.
pub struct CutsceneAv {
    pub frames: Vec<VideoFrame>,
    pub timing: StrTiming,
    /// `None` when the stream carries no decodable (4-bit) audio track.
    pub audio: Option<CutsceneAudio>,
}

/// Decode an interleaved STR stream (`sector_count` raw 2352-byte sectors
/// starting at `lba`) from a disc image into RGBA video frames, the detected
/// playback timing, and the demuxed XA audio track.
///
/// One pass over the sectors: Form-2 audio sectors are accumulated per
/// `(file_no, ch_no)` and the rest is fed to the [`StrFrameAssembler`] as
/// Form-1 video user data. The dominant audio channel (most sectors) is
/// decoded to PCM; 8-bit audio (unsupported by the group decoder) is dropped
/// with a warning rather than mis-decoded.
pub fn decode_str_av_from_disc(
    disc_path: &Path,
    lba: u32,
    sector_count: u32,
) -> Result<CutsceneAv> {
    let mut disc =
        RawDisc::open(disc_path).with_context(|| format!("open disc {}", disc_path.display()))?;

    let mut asm = StrFrameAssembler::new();
    let mut frames: Vec<VideoFrame> = Vec::new();
    // Track total Form-1-shaped sectors so timing matches the Form-1 view
    // (analyze_str_timing measures mean sectors-per-frame over the extract).
    let mut audio_by_key: BTreeMap<(u8, u8), AudioChannelAcc> = BTreeMap::new();

    for s in 0..sector_count {
        let raw = disc
            .read_raw_sector(lba + s)
            .with_context(|| format!("read STR sector {} (lba {})", s, lba + s))?;
        let mut sub = [0u8; 8];
        sub.copy_from_slice(&raw[SUBHEADER_OFFSET..SUBHEADER_OFFSET + 8]);
        let (sh, ok) = legaia_xa::demux::parse_subheader(&sub);
        if ok && sh.is_audio() && sh.is_form2() {
            let end = USER_DATA_OFFSET + AUDIO_BYTES_PER_SECTOR;
            if end <= SECTOR_SIZE {
                let acc = audio_by_key
                    .entry((sh.file_no, sh.ch_no))
                    .or_insert_with(|| AudioChannelAcc {
                        sample_rate: sh.sample_rate(),
                        stereo: sh.is_stereo(),
                        bits_per_sample: sh.bits_per_sample(),
                        audio: Vec::new(),
                    });
                acc.audio.extend_from_slice(&raw[USER_DATA_OFFSET..end]);
            }
            continue;
        }
        // Everything else: treat as a Form-1 video sector. The assembler
        // magic-checks (0x0160) and silently skips non-video user data.
        let video_user = &raw[USER_DATA_OFFSET..USER_DATA_OFFSET + VIDEO_USER_DATA];
        if let Some((hdr, bs)) = asm.push_sector(video_user)? {
            let dec = MdecDecoder::new(hdr.width as u32, hdr.height as u32);
            match dec.decode_frame(&bs) {
                Ok(rgba) => frames.push(VideoFrame {
                    rgba,
                    width: hdr.width as u32,
                    height: hdr.height as u32,
                    frame_number: hdr.frame_number,
                }),
                Err(e) => log::warn!("STR frame {}: decode error: {e}", hdr.frame_number),
            }
        }
    }

    // Recover timing from the sector stride exactly like the Form-1 path:
    // total sectors / video frame count at the 2x CD rate.
    let timing = StrTiming {
        sector_count: sector_count as usize,
        frame_count: frames.len(),
        sectors_per_frame: if frames.is_empty() {
            0.0
        } else {
            sector_count as f64 / frames.len() as f64
        },
        fps: if frames.is_empty() {
            0.0
        } else {
            legaia_mdec::str_sector::CD_SECTORS_PER_SEC_2X
                / (sector_count as f64 / frames.len() as f64)
        },
    };

    // Decode the dominant audio channel (the cutscene's single track).
    let audio = audio_by_key
        .into_iter()
        .max_by_key(|(_, acc)| acc.audio.len())
        .and_then(|((file_no, ch_no), acc)| {
            let bits = match acc.bits_per_sample {
                4 => legaia_xa::BitsPerSample::Four,
                8 => legaia_xa::BitsPerSample::Eight,
                other => {
                    log::warn!(
                        "STR audio f{file_no} c{ch_no}: {other}-bit not supported by the group \
                         decoder; dropping audio track"
                    );
                    return None;
                }
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
            match legaia_xa::decode(&acc.audio, opts) {
                Ok((pcm, _)) => Some(CutsceneAudio {
                    pcm,
                    sample_rate: acc.sample_rate,
                    channels,
                    file_no,
                    ch_no,
                }),
                Err(e) => {
                    log::warn!(
                        "STR audio f{file_no} c{ch_no}: decode failed ({e}); dropping track"
                    );
                    None
                }
            }
        });

    Ok(CutsceneAv {
        frames,
        timing,
        audio,
    })
}

struct AudioChannelAcc {
    sample_rate: u32,
    stereo: bool,
    bits_per_sample: u8,
    audio: Vec<u8>,
}

/// Decode a raw STR file (concatenated 2048-byte Form-1 user-data sectors,
/// i.e. the `extracted/MOV/*.STR` shape) into video frames + timing, with no
/// audio (the extract truncates the interleaved Form-2 audio sectors). Mirrors
/// the historical video-only path so callers without a disc image still play.
pub fn decode_str_video_only(str_path: &Path) -> Result<(Vec<VideoFrame>, StrTiming)> {
    let data = std::fs::read(str_path).with_context(|| format!("read {}", str_path.display()))?;
    let timing = analyze_str_timing(&data);
    let n_sectors = data.len() / VIDEO_USER_DATA;
    let mut asm = StrFrameAssembler::new();
    let mut frames = Vec::new();
    for i in 0..n_sectors {
        let sector = &data[i * VIDEO_USER_DATA..(i + 1) * VIDEO_USER_DATA];
        if let Some((hdr, bs)) = asm.push_sector(sector)? {
            let dec = MdecDecoder::new(hdr.width as u32, hdr.height as u32);
            match dec.decode_frame(&bs) {
                Ok(rgba) => frames.push(VideoFrame {
                    rgba,
                    width: hdr.width as u32,
                    height: hdr.height as u32,
                    frame_number: hdr.frame_number,
                }),
                Err(e) => log::warn!("frame {}: decode error: {e}", hdr.frame_number),
            }
        }
    }
    Ok((frames, timing))
}

/// The video frame index due at the current playback position.
///
/// When `audio_secs` is `Some` (an XA track is playing), the video clock is
/// the audio cursor: `floor(audio_secs / frame_period_secs)`. This is the
/// A/V-sync path - the picture advances exactly as far as the soundtrack has
/// played, so the two never drift. When `audio_secs` is `None` (silent stream
/// or audio disabled) the function falls back to the wall-clock position,
/// preserving the prior video-only pacing.
///
/// The result is **not** clamped to the frame count; callers detect end of
/// stream by comparing against `frames.len()`.
pub fn due_video_frame(
    audio_secs: Option<f64>,
    wall_elapsed_secs: f64,
    frame_period_secs: f64,
) -> usize {
    if frame_period_secs <= 0.0 {
        return 0;
    }
    let pos = audio_secs.unwrap_or(wall_elapsed_secs).max(0.0);
    (pos / frame_period_secs) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_frame_uses_audio_cursor_when_present() {
        // 15 fps -> 1/15 s per frame. Audio at 0.20 s -> frame 3.
        let fp = 1.0 / 15.0;
        // Wall clock is deliberately far ahead to prove audio wins.
        assert_eq!(due_video_frame(Some(0.20), 9.99, fp), 3);
        assert_eq!(due_video_frame(Some(0.0), 9.99, fp), 0);
        // Just under the 4th frame boundary stays on frame 3.
        assert_eq!(due_video_frame(Some(4.0 * fp - 1e-6), 0.0, fp), 3);
        // Exactly on the boundary advances.
        assert_eq!(due_video_frame(Some(4.0 * fp + 1e-9), 0.0, fp), 4);
    }

    #[test]
    fn due_frame_falls_back_to_wall_clock_without_audio() {
        let fp = 1.0 / 15.0;
        assert_eq!(due_video_frame(None, 0.20, fp), 3);
        assert_eq!(due_video_frame(None, 0.0, fp), 0);
    }

    #[test]
    fn due_frame_is_monotonic_in_position() {
        let fp = 1.0 / 15.0;
        let mut last = 0usize;
        for i in 0..100 {
            let secs = i as f64 * 0.01;
            let f = due_video_frame(Some(secs), 0.0, fp);
            assert!(f >= last, "frame went backwards at {secs}");
            last = f;
        }
    }

    #[test]
    fn due_frame_handles_degenerate_period() {
        assert_eq!(due_video_frame(Some(1.0), 1.0, 0.0), 0);
        assert_eq!(due_video_frame(None, 1.0, -1.0), 0);
    }

    #[test]
    fn due_frame_clamps_negative_position() {
        let fp = 1.0 / 15.0;
        assert_eq!(due_video_frame(Some(-5.0), 0.0, fp), 0);
        assert_eq!(due_video_frame(None, -5.0, fp), 0);
    }

    #[test]
    fn cutscene_audio_duration() {
        let a = CutsceneAudio {
            pcm: vec![0i16; 37_800 * 2], // 1 s of stereo at 37.8 kHz
            sample_rate: 37_800,
            channels: legaia_xa::Channels::Stereo,
            file_no: 1,
            ch_no: 0,
        };
        assert!((a.duration_secs() - 1.0).abs() < 1e-9);
    }
}

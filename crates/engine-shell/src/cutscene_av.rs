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
//! [`decode_str_av_from_disc`] does exactly that in a single pass through the
//! shared [`legaia_mdec::str_av`] demuxer (the browser play page's kernel
//! too), decoding the dominant audio channel to PCM and the video to RGBA
//! frames. The interleaving (and thus the A/V alignment) is
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
use legaia_mdec::VideoFrame;
use legaia_mdec::str_sector::{StrFrameAssembler, StrTiming, analyze_str_timing};
use std::path::Path;

/// Form-1 video user-data length.
const VIDEO_USER_DATA: usize = legaia_iso::raw::USER_DATA_SIZE;

/// The decoded XA audio track interleaved in a cutscene STR stream, ready to
/// hand to [`legaia_engine_audio::AudioOut::play_xa`] - the shared demux
/// kernel's [`legaia_mdec::str_av::StrAudio`].
pub type CutsceneAudio = legaia_mdec::str_av::StrAudio;

/// Result of decoding a cutscene STR straight off the disc image.
pub struct CutsceneAv {
    pub frames: Vec<VideoFrame>,
    pub timing: StrTiming,
    /// `None` when the stream carries no decodable audio track.
    pub audio: Option<CutsceneAudio>,
}

/// Decode an interleaved STR stream (`sector_count` raw 2352-byte sectors
/// starting at `lba`) from a disc image into RGBA video frames, the detected
/// playback timing, and the demuxed XA audio track.
///
/// The demux is [`legaia_mdec::str_av::StrAvDemuxer`] - the same kernel the
/// browser play page opens a movie through - so both hosts agree on the
/// frame list, the frame rate and the soundtrack. This host decodes every
/// frame up front; a frame that fails to decode repeats the previous picture
/// instead of dropping out of the timeline (the page keeps its previous
/// picture the same way).
pub fn decode_str_av_from_disc(
    disc_path: &Path,
    lba: u32,
    sector_count: u32,
) -> Result<CutsceneAv> {
    let mut disc =
        RawDisc::open(disc_path).with_context(|| format!("open disc {}", disc_path.display()))?;
    let mut demux = legaia_mdec::str_av::StrAvDemuxer::new();
    for s in 0..sector_count {
        let raw = disc
            .read_raw_sector(lba + s)
            .with_context(|| format!("read STR sector {} (lba {})", s, lba + s))?;
        demux.push_raw_sector(&raw[..SECTOR_SIZE]);
    }
    let av = demux.finish();
    Ok(CutsceneAv {
        frames: decode_frames(&av.frames),
        timing: av.timing,
        audio: av.audio,
    })
}

/// Decode a demuxed frame list, keeping one output frame per assembled
/// frame: an undecodable frame repeats its predecessor (black for the first).
pub fn decode_frames(frames: &[legaia_mdec::str_av::StrFrameBits]) -> Vec<VideoFrame> {
    let mut out: Vec<VideoFrame> = Vec::with_capacity(frames.len());
    for f in frames {
        let rgba = match f.decode_rgba() {
            Some(rgba) => rgba,
            None => {
                log::warn!(
                    "STR frame {}: decode error; holding the previous picture",
                    f.frame_number
                );
                out.last()
                    .filter(|p| p.width == f.width && p.height == f.height)
                    .map(|p| p.rgba.clone())
                    .unwrap_or_else(|| vec![0u8; (f.width * f.height * 4) as usize])
            }
        };
        out.push(VideoFrame {
            rgba,
            width: f.width,
            height: f.height,
            frame_number: f.frame_number,
        });
    }
    out
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
    let mut bits = Vec::new();
    for i in 0..n_sectors {
        let sector = &data[i * VIDEO_USER_DATA..(i + 1) * VIDEO_USER_DATA];
        if let Ok(Some((hdr, bs))) = asm.push_sector(sector) {
            bits.push(legaia_mdec::str_av::StrFrameBits {
                width: hdr.width as u32,
                height: hdr.height as u32,
                frame_number: hdr.frame_number,
                bitstream: bs,
            });
        }
    }
    let frames = decode_frames(&bits);
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
    legaia_engine_core::cutscene::movie_frame_at(
        audio_secs.unwrap_or(wall_elapsed_secs),
        frame_period_secs,
    )
}

/// Narrow a whole-`MVn.STR`-file sector span to the segment one `fmv_id`
/// plays. The seek itself lives with the dispatch table it reads -
/// [`legaia_asset::fmv_dispatch::fmv_segment_window`] - so the native window
/// and the browser play page open a movie off the same arithmetic; this is
/// the same call under the name the window's boot chain already uses.
pub fn fmv_segment_window(
    entry: Option<&legaia_asset::fmv_dispatch::FmvEntry>,
    file_lba: u32,
    file_sectors: u32,
) -> (u32, u32) {
    legaia_asset::fmv_dispatch::fmv_segment_window(entry, file_lba, file_sectors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_window_delegates_to_the_shared_seek() {
        // MV3.STR fmv_id 4: frames 0x1a5..0x27b -> skip 420 frames, play 215.
        // The arithmetic is pinned in `legaia_asset::fmv_dispatch`; this only
        // proves the native name still reaches it.
        let e = legaia_asset::fmv_dispatch::FmvEntry {
            fmv_id: 4,
            path: "\\MOV\\MV3.STR;1".to_string(),
            scale_flag: 0,
            start_frame: 0x1a5,
            end_frame: 0x27b,
            width: 320,
            height: 240,
        };
        assert_eq!(
            fmv_segment_window(Some(&e), 1000, 6800),
            legaia_asset::fmv_dispatch::fmv_segment_window(Some(&e), 1000, 6800)
        );
        assert_eq!(fmv_segment_window(None, 1000, 6800), (1000, 6800));
    }

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

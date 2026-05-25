//! Internal audio/video decode oracle - a clean-room self-consistency check
//! on real disc data (no external reference decoder).
//!
//! Disc-gated per the `LEGAIA_DISC_BIN` skip-pass convention: when the var is
//! unset (or doesn't point at a file) the test prints a skip notice and passes,
//! so CI stays disc-free.
//!
//! ## What it checks
//!
//! **STR / MDEC** - for every `MOV/*.STR` on the disc:
//!   - the detected frame rate is the canonical PSX FMV 15 fps (every Legaia
//!     movie is exactly 10 sectors/frame), and `sectors_per_frame` is a whole
//!     number,
//!   - every assembled frame reports the same, sane (w, h),
//!   - the MDEC decoder turns each sampled frame's bitstream into exactly
//!     `w*h*4` RGBA bytes with no error (decode stability).
//!
//! **XA** - for every demuxed channel on the disc: a real CD-XA rate, a whole
//! number of sound groups, and that the 4-bit decoder yields the expected
//! interleaved sample count with zero silently-skipped groups (the clean-stream
//! invariant the demux path guarantees).

use std::path::PathBuf;

use legaia_iso::iso9660;
use legaia_iso::raw::RawDisc;
use legaia_mdec::MdecDecoder;
use legaia_mdec::str_sector::{StrFrameAssembler, analyze_str_timing};

fn disc_bin() -> Option<PathBuf> {
    let p = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from)?;
    if p.is_file() { Some(p) } else { None }
}

/// Read all 2048-byte user-data sectors of an ISO9660 file into one buffer.
fn read_file_sectors(disc: &mut RawDisc, lba: u32, size: u32) -> Vec<u8> {
    let count = size.div_ceil(legaia_iso::raw::USER_DATA_SIZE as u32);
    let mut buf = Vec::new();
    disc.read_user_data(lba, count, &mut buf)
        .expect("read STR sectors");
    buf
}

#[test]
fn str_movies_decode_stably_at_15fps() {
    let Some(bin) = disc_bin() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let mut disc = RawDisc::open(&bin).expect("open disc");
    let vol = iso9660::read_volume(&mut disc).expect("read volume");
    let files = iso9660::walk_files(&mut disc, &vol.root).expect("walk files");
    let strs: Vec<_> = files
        .into_iter()
        .filter(|(p, _)| p.to_ascii_uppercase().ends_with(".STR"))
        .collect();
    assert!(!strs.is_empty(), "expected at least one .STR on the disc");

    // Decoding every frame of every movie is thousands of MDEC inversions;
    // sample the first N per movie for the decode-stability check while still
    // validating timing + all frame headers (both cheap).
    const DECODE_SAMPLE: usize = 24;

    for (path, rec) in &strs {
        let data = read_file_sectors(&mut disc, rec.lba, rec.size);
        let timing = analyze_str_timing(&data);
        assert!(timing.frame_count > 0, "{path}: no video frames");
        assert!(
            (timing.fps - 15.0).abs() < 1.0,
            "{path}: fps {:.3} not ~15 (sectors/frame {:.3})",
            timing.fps,
            timing.sectors_per_frame
        );
        assert!(
            (timing.sectors_per_frame - timing.sectors_per_frame.round()).abs() < 1e-6,
            "{path}: sectors/frame {:.4} is not whole",
            timing.sectors_per_frame
        );

        let n_sectors = data.len() / 2048;
        let mut asm = StrFrameAssembler::new();
        let mut frame_idx = 0usize;
        let mut dim: Option<(u16, u16)> = None;
        for i in 0..n_sectors {
            let sector = &data[i * 2048..(i + 1) * 2048];
            if let Some((hdr, bs)) = asm.push_sector(sector).expect("assemble") {
                // All frames in one movie share dimensions.
                match dim {
                    None => {
                        assert!(
                            hdr.width > 0
                                && hdr.height > 0
                                && hdr.width <= 640
                                && hdr.height <= 480,
                            "{path}: implausible frame size {}x{}",
                            hdr.width,
                            hdr.height
                        );
                        dim = Some((hdr.width, hdr.height));
                    }
                    Some((w, h)) => assert_eq!(
                        (hdr.width, hdr.height),
                        (w, h),
                        "{path}: frame {} changed dimensions",
                        hdr.frame_number
                    ),
                }
                if frame_idx < DECODE_SAMPLE {
                    let dec = MdecDecoder::new(hdr.width as u32, hdr.height as u32);
                    let rgba = dec.decode_frame(&bs).unwrap_or_else(|e| {
                        panic!("{path}: frame {} decode: {e}", hdr.frame_number)
                    });
                    assert_eq!(
                        rgba.len(),
                        hdr.width as usize * hdr.height as usize * 4,
                        "{path}: frame {} wrong RGBA length",
                        hdr.frame_number
                    );
                }
                frame_idx += 1;
            }
        }
        assert_eq!(
            frame_idx, timing.frame_count,
            "{path}: assembled frame count disagrees with timing"
        );
        eprintln!(
            "[ok] {path}: {} frames {:?} @ {:.2} fps",
            timing.frame_count,
            dim.unwrap(),
            timing.fps
        );
    }
}

/// MDEC decode correctness regression: the decoder is pixel-deterministic, so a
/// content fingerprint of a real decoded frame pins the exact output. This
/// guards against silent decode regressions (e.g. a bitstream desync that the
/// stability check above can't see, since it only validates byte length). The
/// hash is of this decoder's own output - a derived checksum, not redistributed
/// content - in keeping with the disc-gated, CI-skipping convention.
#[test]
fn str_mdec_decode_is_pixel_stable() {
    let Some(bin) = disc_bin() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let mut disc = RawDisc::open(&bin).expect("open disc");
    let vol = iso9660::read_volume(&mut disc).expect("read volume");
    let files = iso9660::walk_files(&mut disc, &vol.root).expect("walk files");
    let (path, rec) = files
        .into_iter()
        .filter(|(p, _)| p.to_ascii_uppercase().ends_with(".STR"))
        .min_by_key(|(_, r)| r.size)
        .expect("at least one .STR");
    let data = read_file_sectors(&mut disc, rec.lba, rec.size);

    // Decode the first frame.
    let mut asm = StrFrameAssembler::new();
    let mut frame0: Option<(u16, u16, Vec<u8>)> = None;
    for i in 0..data.len() / 2048 {
        if let Some((hdr, bs)) = asm.push_sector(&data[i * 2048..(i + 1) * 2048]).unwrap() {
            let rgba = MdecDecoder::new(hdr.width as u32, hdr.height as u32)
                .decode_frame(&bs)
                .expect("decode frame 0");
            frame0 = Some((hdr.width, hdr.height, rgba));
            break;
        }
    }
    let (w, h, rgba) = frame0.expect("at least one frame");

    // Not blank / not a single flat colour (would indicate a dead decode).
    let first = &rgba[0..3];
    let varied = rgba.chunks_exact(4).any(|p| p[0..3] != *first);
    assert!(varied, "{path}: decoded frame 0 is a single flat colour");

    // FNV-1a over the RGBA pins the exact pixels.
    let mut hash = 0xcbf29ce484222325u64;
    for &b in &rgba {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    eprintln!("[ok] {path}: frame0 {w}x{h} fnv1a={hash:#018x}");
    assert_eq!(
        hash, 0xdd296c4b26aff925,
        "{path}: frame0 fingerprint changed (decoder output differs)"
    );
}

/// A/V sync: decoding a cutscene STR straight off the disc recovers BOTH the
/// MDEC video frames AND the interleaved XA audio track, and the audio-cursor
/// playback clock advances the video monotonically and ends on the last frame.
///
/// Targets the smallest movie to keep the full-frame MDEC decode bounded. The
/// determinism-safe clock relationship (`due_video_frame` over the decoded
/// audio duration) is also exercised here against real timing - the CI-only
/// half of that contract lives in the `cutscene_av` unit tests.
#[test]
fn str_av_decode_recovers_synced_audio_and_video() {
    use legaia_engine_shell::cutscene_av::{decode_str_av_from_disc, due_video_frame};

    let Some(bin) = disc_bin() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let mut disc = RawDisc::open(&bin).expect("open disc");
    let vol = iso9660::read_volume(&mut disc).expect("read volume");
    let files = iso9660::walk_files(&mut disc, &vol.root).expect("walk files");
    let smallest = files
        .into_iter()
        .filter(|(p, _)| p.to_ascii_uppercase().ends_with(".STR"))
        .min_by_key(|(_, rec)| rec.size)
        .expect("at least one .STR");
    let (path, rec) = smallest;
    let count = rec.size.div_ceil(legaia_iso::raw::USER_DATA_SIZE as u32);

    let av = decode_str_av_from_disc(&bin, rec.lba, count).expect("decode AV");
    assert!(!av.frames.is_empty(), "{path}: no video frames");
    assert!(
        (av.timing.fps - 15.0).abs() < 1.0,
        "{path}: fps {:.3} not ~15",
        av.timing.fps
    );

    // Every Legaia movie interleaves one stereo 37.8 kHz 4-bit XA track.
    let audio = av.audio.as_ref().expect("expected an interleaved XA track");
    assert_eq!(audio.sample_rate, 37_800, "{path}: unexpected XA rate");
    assert!(
        matches!(audio.channels, legaia_xa::Channels::Stereo),
        "{path}: expected stereo XA"
    );
    assert!(!audio.pcm.is_empty(), "{path}: empty audio PCM");
    let audio_dur = audio.duration_secs();
    let video_dur = av.frames.len() as f64 * av.timing.frame_period().as_secs_f64();
    // The interleaved tracks span the same movie; allow generous slack for
    // lead-in / trailing audio padding, but they must be the same ballpark
    // (not e.g. a 2x stereo-as-mono mis-decode, which would halve audio time).
    assert!(
        audio_dur > video_dur * 0.5 && audio_dur < video_dur * 2.0,
        "{path}: audio {audio_dur:.2}s vs video {video_dur:.2}s out of sync range"
    );

    // The audio-cursor clock sweeps the full frame range monotonically and
    // lands exactly on the last frame at the end of the audio.
    let fp = av.timing.frame_period().as_secs_f64();
    let mut last = 0usize;
    let steps = 200usize;
    for i in 0..=steps {
        let secs = audio_dur * i as f64 / steps as f64;
        let f = due_video_frame(Some(secs), 0.0, fp);
        assert!(f >= last, "{path}: video clock went backwards");
        last = f;
    }
    let final_frame = due_video_frame(Some(video_dur), 0.0, fp);
    assert!(
        final_frame >= av.frames.len() - 1,
        "{path}: clock at video end ({final_frame}) didn't reach last frame ({})",
        av.frames.len() - 1
    );

    eprintln!(
        "[ok] {path}: {} frames @ {:.2} fps, XA {:.1}s stereo {} Hz (video {:.1}s)",
        av.frames.len(),
        av.timing.fps,
        audio_dur,
        audio.sample_rate,
        video_dur
    );
}

#[test]
fn xa_channels_decode_to_expected_sample_counts() {
    let Some(bin) = disc_bin() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let files = legaia_xa::demux::demux_disc_all(&bin).expect("demux all XA");
    let mut channels = 0usize;
    for f in &files {
        for s in &f.streams {
            assert!(
                s.sample_rate == 18_900 || s.sample_rate == 37_800,
                "{} f{} c{}: bad rate {}",
                f.path,
                s.file_no,
                s.ch_no,
                s.sample_rate
            );
            assert!(
                s.audio.len().is_multiple_of(legaia_xa::SOUND_GROUP_BYTES),
                "{} f{} c{}: audio not whole groups",
                f.path,
                s.file_no,
                s.ch_no
            );
            if s.bits_per_sample != 4 {
                continue; // 8-bit unsupported by the group decoder; skip-and-warn path
            }
            let opts = legaia_xa::DecodeOptions {
                channels: if s.stereo {
                    legaia_xa::Channels::Stereo
                } else {
                    legaia_xa::Channels::Mono
                },
                sample_rate: s.sample_rate,
            };
            let (pcm, report) = legaia_xa::decode(&s.audio, opts).expect("decode XA");
            // 4-bit: 8 sound units x 28 samples per group, interleaved across
            // all channels of this (mono/stereo) stream.
            let expected = report.n_groups * 8 * 28;
            assert_eq!(
                pcm.len(),
                expected,
                "{} f{} c{}: {} samples, expected {}",
                f.path,
                s.file_no,
                s.ch_no,
                pcm.len(),
                expected
            );
            assert_eq!(
                report.n_groups_skipped, 0,
                "{} f{} c{}: {} groups silently skipped (demux should yield a clean stream)",
                f.path, s.file_no, s.ch_no, report.n_groups_skipped
            );
            channels += 1;
        }
    }
    assert!(channels > 0, "expected at least one decodable XA channel");
    eprintln!("[ok] {channels} XA channels decoded with no skipped groups");
}

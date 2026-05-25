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

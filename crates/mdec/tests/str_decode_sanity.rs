//! Disc-gated picture-sanity oracle for real `MOV/MV*.STR` frames.
//!
//! The other STR oracles check that frames *arrive* (count, order, segment
//! window, byte-for-byte demux agreement) and that a decode returns the right
//! number of bytes. None of them looks at the picture, which is how `mdec
//! decode-str` came to write garbled frames: it decoded every frame at the
//! dispatch slot's 320x240 while every retail movie is 320x224, and an Iki
//! frame decoded at the wrong height still yields a full-length RGBA buffer -
//! its column-major macroblocks just land in the wrong places.
//!
//! This file plays real frames through [`StrPlayer`] opened with a retail-shaped
//! 320x240 slot, decodes each at the size the pumped frame reports, and scores
//! the picture: macroblock-seam discontinuity against in-block gradient, and
//! frame-to-frame continuity. A mis-sized decode fails both. It also runs the
//! `mdec decode-str` binary over a slice of a real movie and checks the PNGs it
//! writes.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset - no Sony bytes here,
//! and the CLI test's scratch files live in the system temp directory only for
//! the duration of the test.

use legaia_mdec::MdecDecoder;
use legaia_mdec::str_player::{Bitstream, FmvSlot, PumpIdle, StrPlayer};

/// Locate a movie's extent on the disc, if the env var points at one.
fn open_movie(name: &str) -> Option<(legaia_iso::raw::RawDisc, u32, u32)> {
    let path = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let path = legaia_iso::raw::resolve_disc_path(std::path::Path::new(&path)).ok()?;
    let mut disc = legaia_iso::raw::RawDisc::open(&path).ok()?;
    let volume = legaia_iso::iso9660::read_volume(&mut disc).ok()?;
    let files = legaia_iso::iso9660::walk_files(&mut disc, &volume.root).ok()?;
    let (_, rec) = files
        .into_iter()
        .find(|(p, _)| p.to_ascii_uppercase().contains(name))?;
    Some((disc, rec.lba, rec.size))
}

/// The slot shape every retail dispatch record carries: 24-bit, 320x240.
fn retail_slot(start_frame: u32) -> FmvSlot {
    FmvSlot {
        colour: true,
        start_frame,
        end_frame: 0,
        fb_x: 0,
        fb_y: 0,
        width: 320,
        height: 240,
    }
}

/// One pumped frame: its header size and bitstream.
struct Pumped {
    number: u32,
    width: u32,
    height: u32,
    bitstream: Vec<u8>,
}

/// Pump `count` consecutive frames starting at `start_frame`.
fn pump_frames(name: &str, start_frame: u32, count: usize) -> Option<Vec<Pumped>> {
    let (mut disc, lba, size) = open_movie(name)?;
    let mut player = StrPlayer::open(retail_slot(start_frame), Bitstream::Iki);
    let seek = player.seek_sector_offset().max(0) as u32;
    let total = size.div_ceil(2048);
    let mut out = Vec::new();
    let mut i = seek;
    while i < total && out.len() < count {
        let sector = disc.read_sector(lba + i).ok()?;
        i += 1;
        player.deliver_sector(&sector);
        loop {
            match player.next_frame() {
                Ok(f) => out.push(Pumped {
                    number: f.frame_number,
                    width: f.width,
                    height: f.height,
                    bitstream: f.bitstream,
                }),
                Err(PumpIdle::NeedSectors) => break,
                Err(PumpIdle::Finished) => return Some(out),
            }
            if out.len() >= count {
                break;
            }
        }
    }
    Some(out)
}

/// BT.601 luma of every pixel.
fn luma(rgba: &[u8]) -> Vec<f64> {
    rgba.as_chunks::<4>()
        .0
        .iter()
        .map(|p| 0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64)
        .collect()
}

/// Mean horizontal luma step across 16-pixel macroblock seams, divided by the
/// mean step between in-block neighbours (columns not on an 8-pixel edge).
///
/// A correct decode is close to 1 - seams look like any other column. A
/// frame decoded at the wrong height smears macroblocks across columns, so
/// the seams carry hard edges and the ratio climbs well above it.
fn seam_ratio(rgba: &[u8], w: usize, h: usize) -> f64 {
    let l = luma(rgba);
    let (mut seam, mut seam_n, mut inner, mut inner_n) = (0.0, 0usize, 0.0, 0usize);
    for y in 0..h {
        for x in 1..w {
            let d = (l[y * w + x] - l[y * w + x - 1]).abs();
            if x % 16 == 0 {
                seam += d;
                seam_n += 1;
            } else if x % 8 != 0 {
                inner += d;
                inner_n += 1;
            }
        }
    }
    let inner = inner / inner_n as f64;
    (seam / seam_n as f64) / inner.max(0.25)
}

/// Mean absolute luma difference between two same-sized frames.
fn frame_delta(a: &[u8], b: &[u8]) -> f64 {
    let (la, lb) = (luma(a), luma(b));
    la.iter().zip(&lb).map(|(x, y)| (x - y).abs()).sum::<f64>() / la.len() as f64
}

#[test]
fn real_movie_frames_decode_to_a_coherent_picture() {
    // MV1 (the intro) at a bright beat, and an MV3 segment past its first
    // boundary, so the check is not one movie's luck.
    for (name, start) in [("MV1.STR", 900u32), ("MV3.STR", 400)] {
        let Some(frames) = pump_frames(name, start, 4) else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset or {name} not found");
            return;
        };
        assert_eq!(frames.len(), 4, "{name}: pumped too few frames");

        let mut decoded = Vec::new();
        for f in &frames {
            // The pumped size is the sector header's, not the slot's.
            assert_eq!(
                (f.width, f.height),
                (320, 224),
                "{name} frame {}: pumped size must come from the sector header",
                f.number
            );
            let rgba = MdecDecoder::new(f.width, f.height)
                .decode_frame(&f.bitstream)
                .unwrap_or_else(|e| panic!("{name} frame {}: {e}", f.number));
            let ratio = seam_ratio(&rgba, 320, 224);
            eprintln!("{name} frame {}: seam ratio {ratio:.2}", f.number);
            assert!(
                ratio < 2.5,
                "{name} frame {}: macroblock seams dominate (ratio {ratio:.2}) - garbled",
                f.number
            );

            // The defect this guards: the same bitstream at the slot's 240
            // height is measurably blockier.
            let wrong = MdecDecoder::new(320, 240)
                .decode_frame(&f.bitstream)
                .expect("a 240-line decode still returns a buffer");
            let wrong_ratio = seam_ratio(&wrong, 320, 240);
            eprintln!(
                "{name} frame {}: 240-line seam ratio {wrong_ratio:.2}",
                f.number
            );
            assert!(
                wrong_ratio > 2.5,
                "{name} frame {}: the metric cannot tell a mis-sized decode apart \
                 ({wrong_ratio:.2} vs {ratio:.2})",
                f.number
            );
            decoded.push(rgba);
        }

        // Consecutive frames of a 15 fps movie differ by a little, not by a
        // scrambled picture's worth.
        for pair in decoded.windows(2) {
            let d = frame_delta(&pair[0], &pair[1]);
            eprintln!("{name}: frame-to-frame delta {d:.2}");
            assert!(d < 24.0, "{name}: consecutive frames differ by {d:.2}");
        }
    }
}

#[test]
fn mdec_decode_str_cli_writes_header_sized_coherent_frames() {
    let Some(exe) = option_env!("CARGO_BIN_EXE_mdec") else {
        eprintln!("[skip] mdec binary not built (cli feature off)");
        return;
    };
    let Some((mut disc, lba, size)) = open_movie("MV1.STR") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or MV1.STR not found");
        return;
    };
    // The CLI reads the 2048-byte-sector form `legaia-extract` writes to
    // extracted/MOV/. A slice around frame 900 is enough; it never leaves the
    // temp directory.
    let dir = std::env::temp_dir().join(format!("legaia-mdec-cli-{}", std::process::id()));
    let frames_dir = dir.join("frames");
    std::fs::create_dir_all(&frames_dir).unwrap();
    let first = 8990u32;
    let count = 60u32.min(size.div_ceil(2048) - first);
    let mut bytes = Vec::with_capacity(count as usize * 2048);
    for i in 0..count {
        bytes.extend_from_slice(&disc.read_sector(lba + first + i).unwrap());
    }
    let str_path = dir.join("slice.str");
    std::fs::write(&str_path, &bytes).unwrap();

    let status = std::process::Command::new(exe)
        .arg("decode-str")
        .arg(&str_path)
        .arg("-o")
        .arg(&frames_dir)
        .args(["--max-frames", "3"])
        .status()
        .expect("run mdec");
    assert!(status.success(), "mdec decode-str failed");

    let mut pngs: Vec<_> = std::fs::read_dir(&frames_dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "png"))
        .collect();
    pngs.sort();
    assert_eq!(pngs.len(), 3, "expected three frames, got {pngs:?}");
    for p in &pngs {
        let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(p).unwrap()));
        let mut reader = dec.read_info().unwrap();
        let mut buf = vec![0u8; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!(
            (info.width, info.height),
            (320, 224),
            "{}: CLI wrote a slot-sized frame",
            p.display()
        );
        let ratio = seam_ratio(&buf[..info.buffer_size()], 320, 224);
        assert!(
            ratio < 2.5,
            "{}: garbled (seam ratio {ratio:.2})",
            p.display()
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

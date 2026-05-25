//! XA-ADPCM decode-correctness regression, disc-gated per the `LEGAIA_DISC_BIN`
//! skip-pass convention (when the var is unset the test prints a notice and
//! passes, so CI stays disc-free).
//!
//! The 4-bit XA decoder is deterministic, so a content fingerprint of a real
//! demuxed cutscene track pins the exact decoded PCM. Each fingerprint here was
//! cross-checked sample-for-sample against an external reference decode
//! (jPSXdec's lossless WAV export of the same track): every interleaved sample
//! matched, so these hashes also certify bit-for-bit reference parity. We commit
//! only the hashes of our own decoder's output - derived checksums, never the
//! reference audio or any disc bytes.
//!
//! The cross-check itself is reproducible: point `LEGAIA_XA_REF_DIR` at a
//! directory holding the jPSXdec exports (`MVn.STR[0.0].wav`, never committed -
//! they are decoded Sony media) and the test additionally diffs every channel
//! sample against the reference. Without that var it only pins our own
//! fingerprints. The whole-corpus sweep (not just MV1) catches any movie that
//! exercises a code path the first one doesn't - a mono channel, a different
//! rate, a stream that ends mid-group.
//!
//! Companion to the MDEC `str_mdec_decode_is_pixel_stable` video oracle.

use std::path::{Path, PathBuf};

use legaia_iso::iso9660;
use legaia_iso::raw::RawDisc;
use legaia_xa::{Channels, DecodeOptions, decode, demux};

fn disc_bin() -> Option<PathBuf> {
    let p = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from)?;
    if p.is_file() { Some(p) } else { None }
}

fn fnv1a(pcm: &[i16]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for s in pcm {
        for b in s.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// One movie's pinned decode shape: stem (for the on-disc lookup and the
/// optional `MVn.STR[0.0].wav` reference name), interleaved sample count, and
/// the FNV-1a of the decoded PCM.
struct MoviePin {
    stem: &'static str,
    samples: usize,
    fnv1a: u64,
}

/// Every cutscene movie interleaves a single stereo 37.8 kHz 4-bit XA track on
/// file/channel (1,0). MV1 is the track an external reference was first decoded
/// from; the decoder fix was layout/precision (not MV1-specific), so the rest
/// are bit-exact too - pinned here and cross-checked against jPSXdec when
/// `LEGAIA_XA_REF_DIR` is set.
const MOVIES: &[MoviePin] = &[
    MoviePin {
        stem: "MV1",
        samples: 6_777_792,
        fnv1a: 0xe642a9059d458401,
    },
    MoviePin {
        stem: "MV2",
        samples: 1_253_952,
        fnv1a: 0x545f9d1815d19322,
    },
    MoviePin {
        stem: "MV3",
        samples: 4_455_360,
        fnv1a: 0xc88dfbc6f804f66a,
    },
    MoviePin {
        stem: "MV4",
        samples: 1_733_760,
        fnv1a: 0x33f37356838950b3,
    },
    MoviePin {
        stem: "MV5",
        samples: 3_294_144,
        fnv1a: 0x3b4e6eb7e31d1041,
    },
    MoviePin {
        stem: "MV6",
        samples: 3_370_752,
        fnv1a: 0x561f67066612d13b,
    },
];

/// Read a 16-bit PCM WAV into interleaved samples. Scans the RIFF chunk list for
/// `fmt ` (to confirm 16-bit PCM) and `data` rather than assuming a fixed
/// 44-byte header, so jPSXdec's exact chunk layout doesn't matter.
fn read_wav_i16(path: &Path) -> Vec<i16> {
    let bytes = std::fs::read(path).expect("read reference WAV");
    assert_eq!(&bytes[0..4], b"RIFF", "{}: not a RIFF file", path.display());
    assert_eq!(
        &bytes[8..12],
        b"WAVE",
        "{}: not a WAVE file",
        path.display()
    );
    let mut pos = 12;
    let mut bits = 0u16;
    let mut data: Option<&[u8]> = None;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = pos + 8;
        match id {
            b"fmt " => bits = u16::from_le_bytes(bytes[body + 14..body + 16].try_into().unwrap()),
            b"data" => data = Some(&bytes[body..(body + size).min(bytes.len())]),
            _ => {}
        }
        // Chunks are word-aligned (a padding byte follows an odd size).
        pos = body + size + (size & 1);
    }
    assert_eq!(bits, 16, "{}: expected 16-bit PCM", path.display());
    let data = data.unwrap_or_else(|| panic!("{}: no data chunk", path.display()));
    data.chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect()
}

#[test]
fn xa_pcm_matches_reference() {
    let Some(bin) = disc_bin() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let ref_dir = std::env::var_os("LEGAIA_XA_REF_DIR").map(PathBuf::from);
    if ref_dir.is_none() {
        eprintln!(
            "[note] LEGAIA_XA_REF_DIR unset; pinning own fingerprints only \
             (set it to a dir of jPSXdec MVn.STR[0.0].wav exports for the full diff)"
        );
    }

    let mut disc = RawDisc::open(&bin).expect("open disc");
    let vol = iso9660::read_volume(&mut disc).expect("read volume");
    let files = iso9660::walk_files(&mut disc, &vol.root).expect("walk files");

    // Collect (stem, samples, hash) for every movie, then assert against the
    // pins after the loop so one run surfaces all six fingerprints (a fail-fast
    // assert inside the loop would hide the later movies' hashes).
    let mut observed: Vec<(&str, usize, u64)> = Vec::new();
    for movie in MOVIES {
        let needle = format!("{}.STR", movie.stem);
        let (path, rec) = files
            .iter()
            .find(|(p, _)| p.to_ascii_uppercase().ends_with(&needle))
            .unwrap_or_else(|| panic!("{} present on the disc", needle));
        let count = rec.size.div_ceil(legaia_iso::raw::USER_DATA_SIZE as u32);
        let streams = demux::demux_disc_range(&mut disc, rec.lba, count).expect("demux movie STR");

        // The cutscene's audio is the dominant (most sectors) channel.
        let stream = streams
            .iter()
            .max_by_key(|s| s.audio.len())
            .expect("at least one demuxed channel");
        assert_eq!(stream.sample_rate, 37_800, "{path}: unexpected XA rate");
        assert!(stream.stereo, "{path}: expected a stereo track");
        assert_eq!(stream.bits_per_sample, 4, "{path}: expected 4-bit XA");

        let (pcm, report) = decode(
            &stream.audio,
            DecodeOptions {
                channels: Channels::Stereo,
                sample_rate: stream.sample_rate,
            },
        )
        .expect("decode XA");
        assert_eq!(
            report.n_groups_skipped, 0,
            "{path}: demuxed track should be a clean stream"
        );

        let hash = fnv1a(&pcm);
        eprintln!("[ok] {path}: {} samples fnv1a={hash:#018x}", pcm.len());

        // Optional reference cross-check: sample-for-sample against jPSXdec.
        if let Some(dir) = &ref_dir {
            let wav = dir.join(format!("{}.STR[0.0].wav", movie.stem));
            let reference = read_wav_i16(&wav);
            assert_eq!(
                pcm.len(),
                reference.len(),
                "{path}: decoded sample count != reference {}",
                wav.display()
            );
            if let Some(i) = pcm.iter().zip(&reference).position(|(a, b)| a != b) {
                panic!(
                    "{path}: sample {i} differs (ours={}, ref={}) vs {}",
                    pcm[i],
                    reference[i],
                    wav.display()
                );
            }
            eprintln!(
                "[ok] {path}: bit-exact vs {} ({} samples)",
                wav.display(),
                pcm.len()
            );
        }

        observed.push((movie.stem, pcm.len(), hash));
    }

    // Regression pins: sample count + content fingerprint per movie.
    let mut mismatch = false;
    for (movie, &(stem, samples, hash)) in MOVIES.iter().zip(&observed) {
        debug_assert_eq!(movie.stem, stem);
        if samples != movie.samples || hash != movie.fnv1a {
            mismatch = true;
            eprintln!(
                "[MISMATCH] {stem}: got samples={samples} fnv1a={hash:#018x}, \
                 pinned samples={} fnv1a={:#018x}",
                movie.samples, movie.fnv1a
            );
        }
    }
    assert!(
        !mismatch,
        "one or more movie decode fingerprints changed (see [MISMATCH] lines above)"
    );
}

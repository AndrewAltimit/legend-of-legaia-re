//! XA-ADPCM decode-correctness regression, disc-gated per the `LEGAIA_DISC_BIN`
//! skip-pass convention (when the var is unset the test prints a notice and
//! passes, so CI stays disc-free).
//!
//! The 4-bit XA decoder is deterministic, so a content fingerprint of a real
//! demuxed cutscene track pins the exact decoded PCM. The fingerprint was
//! cross-checked sample-for-sample against an external reference decode
//! (jPSXdec's lossless WAV export of the same track): every one of the
//! 6,777,792 interleaved samples matched, so this hash also certifies bit-for-
//! bit reference parity. We commit only the hash of our own decoder's output -
//! a derived checksum, never the reference audio or any disc bytes.
//!
//! Companion to the MDEC `str_mdec_decode_is_pixel_stable` video oracle.

use std::path::PathBuf;

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

#[test]
fn xa_pcm_matches_reference() {
    let Some(bin) = disc_bin() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let mut disc = RawDisc::open(&bin).expect("open disc");
    let vol = iso9660::read_volume(&mut disc).expect("read volume");
    let files = iso9660::walk_files(&mut disc, &vol.root).expect("walk files");

    // The first cutscene movie interleaves a single stereo 37.8 kHz 4-bit XA
    // track on file/channel (1,0); it is the track the reference was decoded
    // from.
    let (path, rec) = files
        .iter()
        .find(|(p, _)| p.to_ascii_uppercase().ends_with("MV1.STR"))
        .expect("MV1.STR present on the disc");
    let count = rec.size.div_ceil(legaia_iso::raw::USER_DATA_SIZE as u32);
    let streams = demux::demux_disc_range(&mut disc, rec.lba, count).expect("demux MV1.STR");

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

    // Sample count and content both pinned. The count equals the reference
    // WAV's interleaved-sample count exactly.
    assert_eq!(
        pcm.len(),
        6_777_792,
        "{path}: decoded interleaved sample count changed"
    );
    let hash = fnv1a(&pcm);
    eprintln!("[ok] {path}: {} samples fnv1a={hash:#018x}", pcm.len());
    assert_eq!(
        hash, 0xe642a9059d458401,
        "{path}: decoded-PCM fingerprint changed (XA decoder output differs)"
    );
}

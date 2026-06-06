//! Parity check: the in-memory XA demux + decode used by site/audio.html
//! must produce the same PCM bytes as the proven path-based path in
//! `legaia_xa::demux::demux_file` + `legaia_xa::decode`.
//!
//! Skips when `LEGAIA_DISC_BIN` isn't set. Disc-gated.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::audio::{decode_xa_in_memory, enumerate_xa_files};
use legaia_xa::{Channels, DecodeOptions};
use std::env;
use std::fs;
use std::path::PathBuf;

#[test]
fn in_memory_xa_decode_matches_path_based_decode_for_mv1() {
    let Some(path) = env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping XA parity test");
        return;
    };
    let bin_path = PathBuf::from(&path);
    let disc = fs::read(&bin_path).expect("disc image");

    let xa_files = enumerate_xa_files(&disc);
    let mv1 = xa_files
        .iter()
        .find(|f| f.path.ends_with("MV1.STR"))
        .expect("MV1.STR on disc");
    eprintln!("[parity] MV1.STR @ lba={} size={}", mv1.lba, mv1.size);

    // (A) Path-based reference: same code the native `xa demux-disc` CLI runs.
    let streams_ref =
        legaia_xa::demux::demux_file(&bin_path, mv1.lba, mv1.size).expect("demux_file");
    assert!(
        !streams_ref.is_empty(),
        "reference path produced no streams"
    );
    let ref0 = &streams_ref[0];
    let opts = DecodeOptions {
        channels: if ref0.stereo {
            Channels::Stereo
        } else {
            Channels::Mono
        },
        sample_rate: ref0.sample_rate,
        bits: legaia_xa::BitsPerSample::Four,
    };
    let (pcm_ref, _) = legaia_xa::decode(&ref0.audio, opts).expect("ref decode");

    // (B) In-memory path (what the WASM site path uses).
    let streams_mine = decode_xa_in_memory(&disc, mv1.lba, mv1.size);
    assert!(
        !streams_mine.is_empty(),
        "in-memory path produced no streams"
    );
    let mine0 = &streams_mine[0];

    eprintln!(
        "[parity] ref: file={} ch={} rate={} stereo={} samples={}",
        ref0.file_no,
        ref0.ch_no,
        ref0.sample_rate,
        ref0.stereo,
        pcm_ref.len()
    );
    eprintln!(
        "[parity] mine: file={} ch={} rate={} stereo={} samples={}",
        mine0.file_no,
        mine0.ch_no,
        mine0.sample_rate,
        mine0.stereo,
        mine0.pcm.len()
    );

    assert_eq!(mine0.file_no, ref0.file_no, "file_no mismatch");
    assert_eq!(mine0.ch_no, ref0.ch_no, "ch_no mismatch");
    assert_eq!(mine0.sample_rate, ref0.sample_rate, "sample_rate mismatch");
    assert_eq!(mine0.stereo, ref0.stereo, "stereo mismatch");
    assert_eq!(mine0.pcm.len(), pcm_ref.len(), "sample count mismatch");
    // i16 PCM comparison; first divergence wins.
    for (i, (a, b)) in pcm_ref.iter().zip(mine0.pcm.iter()).enumerate() {
        if a != b {
            panic!("PCM divergence at sample {i}: ref={a} mine={b}");
        }
    }
}

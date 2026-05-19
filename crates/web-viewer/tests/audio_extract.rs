//! Disc-gated smoke test for the audio extraction surface exposed to the
//! site/audio.html WASM page. Verifies that the three enumerators surface
//! the expected magnitudes against a real disc image and that a single
//! VAG sample + a single XA stream both round-trip through the in-memory
//! decoders.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset - same gating pattern
//! as `crates/iso/tests/disc_pipeline.rs`.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::audio::{
    decode_vag_sample, decode_xa_in_memory, enumerate_bgm_pairs, enumerate_vabs, enumerate_xa_files,
};
use legaia_web_viewer::disc::{extract_prot_dat, parse_prot_toc};
use std::env;
use std::fs;

#[test]
fn enumerate_audio_against_real_disc() {
    let Some(path) = env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping audio extraction test");
        return;
    };
    let disc = fs::read(&path).expect("disc image");
    let prot = extract_prot_dat(&disc).expect("PROT.DAT extraction");
    let entries = parse_prot_toc(&prot).expect("PROT TOC parse");

    let vabs = enumerate_vabs(&prot, &entries);
    eprintln!("[audio] {} VAB banks across PROT entries", vabs.len());
    assert!(
        vabs.len() >= 200,
        "expected >= 200 VAB banks across the corpus, got {}",
        vabs.len()
    );

    let pairs = enumerate_bgm_pairs(&prot, &entries);
    eprintln!("[audio] {} BGM pairs (VAB+SEQ)", pairs.len());
    assert!(
        !pairs.is_empty(),
        "expected at least one BGM pair (music_01 cluster)"
    );

    let xa_files = enumerate_xa_files(&disc);
    eprintln!(
        "[audio] {} XA files on disc: {:?}",
        xa_files.len(),
        xa_files.iter().map(|f| &f.path).collect::<Vec<_>>()
    );
    assert!(
        xa_files.iter().any(|f| f.path.contains("MV")),
        "expected at least one MV*.STR file on the disc"
    );

    // Decode the first non-empty VAB sample to verify the chain.
    let vab = vabs.iter().find(|v| v.sample_count > 0).expect("any VAB");
    let pcm = decode_vag_sample(&prot, &entries, vab.prot_index, vab.vab_offset, 0)
        .expect("decode first VAG sample of first VAB");
    assert!(
        !pcm.is_empty(),
        "first VAB sample decoded to zero PCM samples"
    );
    let max_abs = pcm
        .iter()
        .map(|s| s.unsigned_abs() as u32)
        .max()
        .unwrap_or(0);
    eprintln!(
        "[audio] first VAB sample of PROT {}: {} mono samples, max |amp| = {}",
        vab.prot_index,
        pcm.len(),
        max_abs
    );
    assert!(
        max_abs > 0,
        "VAB sample decoded but is entirely silent (would never produce audible output)"
    );

    // Demux + decode the first XA file. MV1.STR is short enough to keep
    // this test fast (well under a second on release builds).
    let first_xa = xa_files
        .iter()
        .find(|f| f.path.ends_with("MV1.STR"))
        .or_else(|| xa_files.first())
        .expect("any XA file");
    let streams = decode_xa_in_memory(&disc, first_xa.lba, first_xa.size);
    assert!(
        !streams.is_empty(),
        "{} produced no audio channels",
        first_xa.path
    );
    let total: usize = streams.iter().map(|s| s.pcm.len()).sum();
    assert!(
        total > 0,
        "{} decoded to zero PCM samples across all channels",
        first_xa.path
    );
    eprintln!(
        "[audio] {} -> {} channel(s), {} interleaved samples",
        first_xa.path,
        streams.len(),
        total
    );
}

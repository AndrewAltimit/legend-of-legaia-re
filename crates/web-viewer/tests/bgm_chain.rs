//! Smoke test for the BGM playback chain that powers `site/audio.html`.
//!
//! Mirrors the construction sequence inside `LegaiaAudio::start_bgm` but
//! drives the SPU directly (no `WebAudioOut`, since that path is wasm32-only)
//! and asserts the chain produces non-silent output for the first real
//! music_01 BGM pair on disc. If this test goes silent, the in-browser
//! page will too - independent of any JS / autoplay-policy issue.
//!
//! Skips when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_audio::sequencer::Sequencer;
use legaia_engine_audio::spu::ram::SpuAllocator;
use legaia_engine_audio::{Spu, VabBank, render_bgm_to_pcm};
use legaia_seq::Seq;
use legaia_vab::parse as parse_vab;
use legaia_web_viewer::audio::enumerate_bgm_pairs;
use legaia_web_viewer::disc::{extract_prot_dat, parse_prot_toc};
use std::env;
use std::fs;

#[test]
fn first_bgm_pair_renders_non_silent_pcm() {
    let Some(path) = env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping BGM chain test");
        return;
    };
    let disc = fs::read(&path).expect("disc image");
    let prot = extract_prot_dat(&disc).expect("PROT.DAT");
    let entries = parse_prot_toc(&prot).expect("PROT TOC");

    let pairs = enumerate_bgm_pairs(&prot, &entries);
    let pair = pairs.first().expect("at least one BGM pair");
    eprintln!(
        "[bgm-chain] first pair: PROT {} vab=0x{:X} seq=0x{:X} {} progs / {} samples / {} BPM",
        pair.prot_index,
        pair.vab_offset,
        pair.seq_offset,
        pair.program_count,
        pair.sample_count,
        pair.bpm,
    );

    let e = entries
        .iter()
        .find(|x| x.index == pair.prot_index)
        .expect("entry");
    let off = e.byte_offset as usize;
    let end = (e.byte_offset + e.size_bytes) as usize;
    let buf = &prot[off..end];

    let vab_report = parse_vab(buf, pair.vab_offset as usize).expect("VAB parse");
    let seq = Seq::parse(&buf[pair.seq_offset as usize..]).expect("SEQ parse");

    let mut spu = Spu::new();
    let mut alloc = SpuAllocator::new(0x1000, 0x40_000);
    let bank = VabBank::upload(
        &mut spu,
        &mut alloc,
        &vab_report,
        &buf[pair.vab_offset as usize..],
    );
    let mut sequencer = Sequencer::new(seq, bank);

    // Drive the sequencer for ~2 seconds of game-time. Most BGM SEQs
    // sit on a rest for a beat or two before NoteOn fires, so we need
    // longer than the existing real_bgm_chain test's 200 ms window.
    for _ in 0..400 {
        sequencer.tick_us(&mut spu, 5_000.0);
    }

    // Sample 1 second of SPU output at the internal 44.1 kHz rate.
    let mut max_abs: i32 = 0;
    let mut nonzero_frames = 0usize;
    for _ in 0..44_100 {
        let (l, r) = spu.tick();
        let a = (l as i32).abs().max((r as i32).abs());
        if a != 0 {
            nonzero_frames += 1;
        }
        max_abs = max_abs.max(a);
    }
    eprintln!(
        "[bgm-chain] post-render: 1s window, max |sample| = {max_abs}, nonzero frames = {nonzero_frames}"
    );
    assert!(
        max_abs > 0,
        "BGM chain rendered 1 second of silence - SPU never received a NoteOn or every voice is muted"
    );
    assert!(
        nonzero_frames > 100,
        "BGM chain produced only {nonzero_frames} non-zero frames in 44100 - too sparse to be audible"
    );
}

/// Verify `render_bgm_to_pcm` (the WASM site's BGM path) advances the
/// sequencer at retail speed. Renders 5 seconds of audio and asserts
/// the output is exactly `5 * 44100 = 220500` stereo frames - 1 wall
/// second's worth of audio per second of sequencer time.
///
/// If this test ever produces a different sample count, the WASM-side
/// "BGM plays too fast / too slow" symptom would be reproduced offline
/// too, and the bug is in the engine-audio render loop rather than in
/// the browser's `ScriptProcessorNode` callback pacing.
#[test]
fn offline_bgm_render_matches_wall_clock_at_44100hz() {
    let Some(path) = env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping offline-render test");
        return;
    };
    let disc = fs::read(&path).expect("disc image");
    let prot = extract_prot_dat(&disc).expect("PROT.DAT");
    let entries = parse_prot_toc(&prot).expect("PROT TOC");

    let pairs = enumerate_bgm_pairs(&prot, &entries);
    let pair = pairs.first().expect("at least one BGM pair");
    let e = entries
        .iter()
        .find(|x| x.index == pair.prot_index)
        .expect("entry");
    let off = e.byte_offset as usize;
    let end = (e.byte_offset + e.size_bytes) as usize;
    let buf = &prot[off..end];

    let vab_report = parse_vab(buf, pair.vab_offset as usize).expect("VAB parse");
    let seq = Seq::parse(&buf[pair.seq_offset as usize..]).expect("SEQ parse");
    let mut spu = Spu::new();
    let mut alloc = SpuAllocator::new(0x1000, 0x40_000);
    let bank = VabBank::upload(
        &mut spu,
        &mut alloc,
        &vab_report,
        &buf[pair.vab_offset as usize..],
    );
    let mut sequencer = Sequencer::new(seq, bank);

    let duration_samples = 5 * 44_100;
    let pcm = render_bgm_to_pcm(&mut sequencer, &mut spu, duration_samples);

    assert_eq!(
        pcm.len(),
        duration_samples * 2,
        "render_bgm_to_pcm sample count drift: expected {} interleaved samples, got {}",
        duration_samples * 2,
        pcm.len()
    );

    let max_abs = pcm
        .iter()
        .map(|s| s.unsigned_abs() as u32)
        .max()
        .unwrap_or(0);
    let nonzero = pcm.iter().filter(|&&s| s != 0).count();
    eprintln!(
        "[offline-render] PROT {} bpm={} ppqn={}: 5.00s → {} interleaved samples, max |amp| = {}, nonzero = {}",
        pair.prot_index,
        pair.bpm,
        pair.ppqn,
        pcm.len(),
        max_abs,
        nonzero
    );
    assert!(
        max_abs > 0,
        "offline-rendered BGM is entirely silent over 5 seconds"
    );
    assert!(
        nonzero > 1000,
        "offline-rendered BGM has only {nonzero} non-zero samples in 5s of audio"
    );
}

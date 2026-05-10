//! End-to-end smoke test: build a synthetic VAB byte stream, parse it via
//! `legaia-vab`, upload it into a `Spu` via `engine-audio`'s `VabBank`, and
//! verify a key-on path actually drives the voice's envelope into Attack.
//!
//! Synthesises ONE program with ONE tone covering the whole keyboard, one
//! VAG body of one block. No Sony bytes - pure constructed test data.

use legaia_engine_audio::spu::ram::SpuAllocator;
use legaia_engine_audio::{Spu, VabBank};
use legaia_vab::{
    PROGRAMS_TABLE_SIZE, TONE_SIZE, TONES_PER_PROGRAM, VAB_HEADER_SIZE, VAB_MAGIC, VAG_BLOCK_BYTES,
    VAG_TABLE_ENTRIES, parse,
};

/// Build a one-program, one-tone, one-sample VAB blob.
fn build_synth_vab() -> Vec<u8> {
    let ps = 1usize; // programs in use
    let ts = 1usize; // tones in use
    let vs = 1usize; // samples in use

    let prog_off = VAB_HEADER_SIZE;
    let tone_off = prog_off + PROGRAMS_TABLE_SIZE;
    let table_off = tone_off + TONE_SIZE * TONES_PER_PROGRAM * ps;
    let vag_bodies_off = table_off + 2 * VAG_TABLE_ENTRIES;
    let vag_size = VAG_BLOCK_BYTES * 2; // 2 ADPCM blocks
    let total = vag_bodies_off + vag_size;

    let mut buf = vec![0u8; total];

    // Header. Layout from `parse_header`:
    //   0..4   magic 'VABp'
    //   4..8   version (legal: <= 10)
    //   8..12  vab_id
    //   12..16 fsize
    //   18..20 ps
    //   20..22 ts
    //   22..24 vs
    //   24     mvol
    //   25     pan
    //   26     attr1
    //   27     attr2
    buf[0..4].copy_from_slice(&VAB_MAGIC.to_le_bytes());
    buf[4..8].copy_from_slice(&7u32.to_le_bytes());
    buf[8..12].copy_from_slice(&1u32.to_le_bytes());
    buf[12..16].copy_from_slice(&(total as u32).to_le_bytes());
    buf[18..20].copy_from_slice(&(ps as u16).to_le_bytes());
    buf[20..22].copy_from_slice(&(ts as u16).to_le_bytes());
    buf[22..24].copy_from_slice(&(vs as u16).to_le_bytes());
    buf[24] = 127; // master vol
    buf[25] = 64; // pan center

    // Program 0: one tone in use.
    buf[prog_off] = 1; // tones in use

    // Tone 0 (program 0, tone slot 0). Field offsets per legaia_vab::parse:
    //   0 prior, 1 mode, 2 vol, 3 pan, 4 center, 5 shift, 6 min, 7 max,
    //   16..18 adsr1, 18..20 adsr2, 22..24 vag (1-based)
    let p = tone_off;
    buf[p + 2] = 100; // vol
    buf[p + 3] = 64; // pan
    buf[p + 4] = 60; // center note
    buf[p + 6] = 0; // min
    buf[p + 7] = 127; // max
    buf[p + 22..p + 24].copy_from_slice(&1i16.to_le_bytes()); // vag = 1 (sample 0)

    // VAG offset table: entry 0 = master shift, entry 1+ = sample size
    // in 8-byte units. vag_size = 32 -> 4 units.
    buf[table_off] = 0; // master shift
    let units = (vag_size / 8) as u16;
    buf[table_off + 2..table_off + 4].copy_from_slice(&units.to_le_bytes());

    // VAG body: a silence block + a terminator block.
    let body_off = vag_bodies_off;
    // Block 0: filter=0 shift=0 flag=0 (no end)
    // Block 1: end-marker (flag = 0x01)
    buf[body_off + VAG_BLOCK_BYTES + 1] = 0x01;

    buf
}

#[test]
fn synth_vab_uploads_and_plays() {
    let blob = build_synth_vab();
    let report = parse(&blob, 0).expect("synth VAB parses");
    assert_eq!(report.programs.len(), 128);
    assert_eq!(report.tones.len(), 1);
    assert_eq!(report.vag_samples.len(), 1);
    assert_eq!(report.tones[0][0].center, 60);

    let mut spu = Spu::new();
    let mut alloc = SpuAllocator::new(0x1000, 0x10_0000);
    let bank = VabBank::upload(&mut spu, &mut alloc, &report, &blob);
    assert_eq!(bank.programs.len(), 1);
    assert_eq!(bank.samples.len(), 1);
    assert!(bank.samples[0].is_some());

    // Voice 0 starts off; play_note transitions it into Attack.
    assert!(spu.voices[0].is_off());
    let ok = bank.play_note(&mut spu, 0, 0, 60, 100);
    assert!(ok, "play_note at center key should succeed");
    assert!(!spu.voices[0].is_off());
}

#[test]
fn synth_vab_pitches_correctly_for_octave_step() {
    let blob = build_synth_vab();
    let report = parse(&blob, 0).expect("synth VAB parses");
    let mut spu = Spu::new();
    let mut alloc = SpuAllocator::new(0x1000, 0x10_0000);
    let bank = VabBank::upload(&mut spu, &mut alloc, &report, &blob);

    // Note 60 at center -> base pitch = 0x800.
    bank.play_note(&mut spu, 0, 0, 60, 100);
    let p_center = spu.voices[0].pitch;
    spu.voices[0].adsr.phase = legaia_engine_audio::Phase::Off; // reset for next test

    // Note 72 (one octave up) -> pitch should be 2× base = 0x1000.
    bank.play_note(&mut spu, 0, 0, 72, 100);
    let p_octave = spu.voices[0].pitch;

    assert_eq!(p_center, 0x800);
    assert!(
        (p_octave as i32 - 0x1000).abs() < 4,
        "octave-up pitch {p_octave:#x} should be ~0x1000"
    );
}

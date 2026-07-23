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

/// Build a bank whose used programs are SPARSE in slot space: slots 1 and 3
/// used (`ps = 2`), slots 0 and 2 empty. The file's two packed tone pages
/// belong to slots 1 and 3 by rank - the mapping retail builds at VAB open
/// (`FUN_80068d94`) and the shape most real Legaia banks have (43 of the 77
/// music banks author non-contiguous program sets).
///
/// Page 0 (slot 1) plays VAG 1, page 1 (slot 3) plays VAG 2, so a wrong
/// page resolution is visible in the keyed voice's start address.
fn build_sparse_vab() -> Vec<u8> {
    let ps = 2usize;
    let vag_size = VAG_BLOCK_BYTES * 2;

    let prog_off = VAB_HEADER_SIZE;
    let tone_off = prog_off + PROGRAMS_TABLE_SIZE;
    let table_off = tone_off + TONE_SIZE * TONES_PER_PROGRAM * ps;
    let vag_bodies_off = table_off + 2 * VAG_TABLE_ENTRIES;
    let total = vag_bodies_off + vag_size * 2;

    let mut buf = vec![0u8; total];
    buf[0..4].copy_from_slice(&VAB_MAGIC.to_le_bytes());
    buf[4..8].copy_from_slice(&7u32.to_le_bytes());
    buf[8..12].copy_from_slice(&1u32.to_le_bytes());
    buf[12..16].copy_from_slice(&(total as u32).to_le_bytes());
    buf[18..20].copy_from_slice(&(ps as u16).to_le_bytes());
    buf[20..22].copy_from_slice(&2u16.to_le_bytes()); // ts
    buf[22..24].copy_from_slice(&2u16.to_le_bytes()); // vs
    buf[24] = 127; // master vol
    buf[25] = 64;

    // Slots 1 and 3 used; 0 and 2 empty (tones byte stays 0).
    buf[prog_off + 16] = 1; // slot 1: 1 tone
    buf[prog_off + 16 + 1] = 127; // slot 1 mvol
    buf[prog_off + 16 + 4] = 64; // slot 1 mpan
    buf[prog_off + 48] = 1; // slot 3: 1 tone
    buf[prog_off + 48 + 1] = 127;
    buf[prog_off + 48 + 4] = 64;

    // Packed page 0 -> VAG 1, page 1 -> VAG 2. Full-keyboard windows.
    for (page, vag) in [(0usize, 1i16), (1, 2)] {
        let p = tone_off + page * TONE_SIZE * TONES_PER_PROGRAM;
        buf[p + 2] = 100; // vol
        buf[p + 3] = 64; // pan
        buf[p + 4] = 60; // center
        buf[p + 7] = 127; // max
        buf[p + 22..p + 24].copy_from_slice(&vag.to_le_bytes());
    }

    // VAG table: two samples of vag_size each.
    let units = (vag_size / 8) as u16;
    buf[table_off + 2..table_off + 4].copy_from_slice(&units.to_le_bytes());
    buf[table_off + 4..table_off + 6].copy_from_slice(&units.to_le_bytes());
    // Bodies: end-marker in the second block of each.
    buf[vag_bodies_off + VAG_BLOCK_BYTES + 1] = 0x01;
    buf[vag_bodies_off + vag_size + VAG_BLOCK_BYTES + 1] = 0x01;

    buf
}

/// Program numbers resolve by rank among used slots, not by packed-page
/// index: on the sparse bank, program 1 keys VAG 1, program 3 keys VAG 2,
/// and the unused slots 0 and 2 key nothing.
#[test]
fn sparse_bank_programs_resolve_by_used_slot_rank() {
    let blob = build_sparse_vab();
    let report = parse(&blob, 0).expect("sparse VAB parses");
    assert_eq!(report.tones.len(), 2, "two packed pages");

    let mut spu = Spu::new();
    let mut alloc = SpuAllocator::new(0x1000, 0x10_0000);
    let bank = VabBank::upload(&mut spu, &mut alloc, &report, &blob);

    // Slot space: len = last used slot + 1.
    assert_eq!(bank.programs.len(), 4);
    let vag1_addr = bank.samples[0].expect("VAG 1 uploaded").addr;
    let vag2_addr = bank.samples[1].expect("VAG 2 uploaded").addr;

    // Program 1 (used) -> packed page 0 -> VAG 1.
    assert!(bank.play_note(&mut spu, 0, 1, 60, 100));
    assert_eq!(spu.voices[0].start_addr, vag1_addr);
    // Program 3 (used) -> packed page 1 -> VAG 2. Under the old packed indexing
    // this program number was out of range and the note silently dropped.
    assert!(bank.play_note(&mut spu, 1, 3, 60, 100));
    assert_eq!(spu.voices[1].start_addr, vag2_addr);
    // Unused slots alias onto the page the NEXT used slot gets (retail's +8
    // used-counter), so they resolve rather than fall silent: slot 0 -> page 0
    // (VAG 1, same as program 1), slot 2 -> page 1 (VAG 2, same as program 3).
    assert!(bank.play_note(&mut spu, 2, 0, 60, 100));
    assert_eq!(spu.voices[2].start_addr, vag1_addr);
    assert!(bank.play_note(&mut spu, 3, 2, 60, 100));
    assert_eq!(spu.voices[3].start_addr, vag2_addr);
    assert_eq!(bank.tone_prior(0, 60), bank.tone_prior(1, 60));
    assert_eq!(bank.tone_prior(2, 60), bank.tone_prior(3, 60));
    assert!(bank.tone_prior(1, 60).is_some());
}

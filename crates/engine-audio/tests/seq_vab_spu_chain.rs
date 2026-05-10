//! End-to-end smoke test: drive a synthetic SEQ through `Sequencer` against
//! a synthetic VAB uploaded to a `Spu`, render PCM samples, and verify the
//! SPU produced non-silent output once the sequencer fired the NoteOn.
//!
//! Together with `vab_smoke` (VAB upload + key-on) this proves the full
//! audio chain (SEQ → Sequencer → VabBank → Spu → i16 PCM) is wired without
//! requiring cpal or a real disc image.

use legaia_engine_audio::sequencer::Sequencer;
use legaia_engine_audio::spu::ram::SpuAllocator;
use legaia_engine_audio::{Spu, VabBank};
use legaia_seq::{SEQ_MAGIC, Seq};
use legaia_vab::{
    PROGRAMS_TABLE_SIZE, TONE_SIZE, TONES_PER_PROGRAM, VAB_HEADER_SIZE, VAB_MAGIC, VAG_BLOCK_BYTES,
    VAG_TABLE_ENTRIES, parse,
};

/// Synthesise a one-program / one-tone / one-VAG VAB whose sample is a 16-bit
/// constant-amplitude block (to make non-silence detection robust).
fn build_vab() -> Vec<u8> {
    let ps = 1usize;
    let ts = 1usize;
    let vs = 1usize;

    let prog_off = VAB_HEADER_SIZE;
    let tone_off = prog_off + PROGRAMS_TABLE_SIZE;
    let table_off = tone_off + TONE_SIZE * TONES_PER_PROGRAM * ps;
    let vag_bodies_off = table_off + 2 * VAG_TABLE_ENTRIES;
    let vag_size = VAG_BLOCK_BYTES * 4;
    let total = vag_bodies_off + vag_size;
    let mut buf = vec![0u8; total];

    // VAB header - same fields as the vab_smoke fixture.
    buf[0..4].copy_from_slice(&VAB_MAGIC.to_le_bytes());
    buf[4..8].copy_from_slice(&7u32.to_le_bytes());
    buf[8..12].copy_from_slice(&1u32.to_le_bytes());
    buf[12..16].copy_from_slice(&(total as u32).to_le_bytes());
    buf[18..20].copy_from_slice(&(ps as u16).to_le_bytes());
    buf[20..22].copy_from_slice(&(ts as u16).to_le_bytes());
    buf[22..24].copy_from_slice(&(vs as u16).to_le_bytes());
    buf[24] = 127;
    buf[25] = 64;

    // Program 0: 1 tone, master_vol 127.
    buf[prog_off] = 1;
    buf[prog_off + 1] = 127;

    // Tone 0..15 of program 0; we populate slot 0.
    let t0 = tone_off;
    buf[t0] = 0; // priority
    buf[t0 + 1] = 0; // mode
    buf[t0 + 2] = 127; // vol
    buf[t0 + 3] = 64; // pan
    buf[t0 + 4] = 60; // center key
    buf[t0 + 5] = 0; // shift
    buf[t0 + 6] = 0; // min_key
    buf[t0 + 7] = 127; // max_key
    buf[t0 + 8] = 0; // vibW
    buf[t0 + 9] = 0; // vibT
    buf[t0 + 10] = 0; // porW
    buf[t0 + 11] = 0; // porT
    buf[t0 + 12] = 0; // pbmin
    buf[t0 + 13] = 0; // pbmax
    buf[t0 + 14] = 0; // unused
    buf[t0 + 15] = 0; // unused
    // adsr1, adsr2 - give a quick attack and a long hold so the voice is
    // audibly playing on the next render cycle.
    buf[t0 + 16..t0 + 18].copy_from_slice(&0x80FFu16.to_le_bytes());
    buf[t0 + 18..t0 + 20].copy_from_slice(&0x5FC0u16.to_le_bytes());
    buf[t0 + 20..t0 + 22].copy_from_slice(&0u16.to_le_bytes()); // parent program
    buf[t0 + 22..t0 + 24].copy_from_slice(&1u16.to_le_bytes()); // VAG index 1 (1-based)
    // VAG offsets table: entry 0 is 0 (sentinel), entry 1 is the size in 8-byte units.
    buf[table_off..table_off + 2].copy_from_slice(&0u16.to_le_bytes());
    let body_size_units = (vag_size / 8) as u16;
    buf[table_off + 2..table_off + 4].copy_from_slice(&body_size_units.to_le_bytes());

    // VAG body - write 4 ADPCM blocks of constant amplitude. Each block is
    // 16 bytes: header byte (filter | shift), flag byte, then 14 nibble
    // bytes. We use shift=0 + filter=0 so each nibble decodes to its raw
    // signed-4 value × 1; nibbles 0x77 → +7 (positive max).
    for blk in 0..4 {
        let off = vag_bodies_off + blk * VAG_BLOCK_BYTES;
        buf[off] = 0x00; // shift=0, filter=0
        buf[off + 1] = if blk == 3 { 0x05 } else { 0x00 }; // flag (5 = end+sustain on last)
        for i in 0..14 {
            buf[off + 2 + i] = 0x77;
        }
    }
    buf
}

/// Synthesise a simple SEQ that does ProgramChange(0) then NoteOn(60).
/// Identical event shape to the synth in `sequencer::tests`.
fn build_seq() -> Seq {
    let mut buf = Vec::new();
    buf.extend_from_slice(&SEQ_MAGIC);
    buf.extend_from_slice(&[0x00, 0x01]);
    buf.extend_from_slice(&[0x01, 0xE0]); // ppqn 480
    buf.extend_from_slice(&[0x07, 0xA1, 0x20]); // tempo 500000us/qn (120 BPM)
    buf.push(0x04);
    buf.push(0x02);
    // delta 0, ProgramChange ch0 prog 0
    buf.push(0x00);
    buf.push(0xC0);
    buf.push(0x00);
    // delta 0, NoteOn key 60 vel 100
    buf.push(0x00);
    buf.push(0x90);
    buf.push(60);
    buf.push(100);
    // delta 480, NoteOff
    buf.push(0x83);
    buf.push(0x60);
    buf.push(60);
    buf.push(0);
    // delta 0, end-of-track
    buf.push(0x00);
    buf.push(0xFF);
    buf.push(0x2F);
    buf.push(0x00);
    Seq::parse(&buf).unwrap()
}

#[test]
fn end_to_end_seq_to_pcm_chain_produces_non_silent_output() {
    let vab_bytes = build_vab();
    let report = parse(&vab_bytes, 0).expect("parse VAB");

    let mut spu = Spu::new();
    let mut alloc = SpuAllocator::new(0x1000, 0x40_000);
    let bank = VabBank::upload(&mut spu, &mut alloc, &report, &vab_bytes);
    assert!(
        !bank.programs.is_empty() && !bank.samples.is_empty(),
        "VabBank should have populated programs + samples"
    );

    let seq = build_seq();
    let mut sequencer = Sequencer::new(seq, bank);

    // Tick the sequencer enough that the ProgramChange + NoteOn fire.
    sequencer.tick_us(&mut spu, 0.0);
    sequencer.tick_us(&mut spu, 1_000.0);

    // Render one frame's worth of PCM (44.1 kHz mono). Look for any sample
    // whose absolute value exceeds a threshold - the synth VAG's positive
    // amplitude should clearly clear silence. Use ~512 samples since SPU
    // mixing has some startup latency from envelope attack.
    let mut max_abs: i32 = 0;
    for _ in 0..512 {
        let (l, r) = spu.tick();
        max_abs = max_abs.max((l as i32).abs()).max((r as i32).abs());
    }
    assert!(
        max_abs > 8,
        "SPU produced silent output (max |sample| = {max_abs}) - chain isn't wired"
    );
}

#[test]
fn end_to_end_chain_silences_after_eot_when_not_looped() {
    let vab_bytes = build_vab();
    let report = parse(&vab_bytes, 0).expect("parse VAB");
    let mut spu = Spu::new();
    let mut alloc = SpuAllocator::new(0x1000, 0x40_000);
    let bank = VabBank::upload(&mut spu, &mut alloc, &report, &vab_bytes);

    let mut sequencer = Sequencer::new(build_seq(), bank);
    sequencer.tick_us(&mut spu, 10_000_000.0); // drain whole track
    assert!(sequencer.is_finished());

    // Tick a long stretch to let the voice's release ramp down; the SPU
    // should be silent (or nearly so) after release completes.
    let mut max_abs_late: i32 = 0;
    for _ in 0..44_100 * 2 {
        let (l, r) = spu.tick();
        max_abs_late = max_abs_late.max((l as i32).abs()).max((r as i32).abs());
    }
    // Allow a tiny noise floor - voices can hold residual envelope state
    // briefly after key-off.
    assert!(
        max_abs_late < 1024,
        "SPU still producing audible output after EOT (max |sample| = {max_abs_late})"
    );
}

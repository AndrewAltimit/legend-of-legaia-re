//! BGM-director-shaped audio pipeline: a director that owns a
//! [`Sequencer`] + [`Spu`] and drives PCM output when the engine-core
//! `route_bgm_events` path calls into it. This is the integration shape an
//! engine consumer will use; the test proves the piece compiles and
//! generates non-silent audio without any disc data.
//!
//! The `BgmDirector` trait lives in `engine-core::scene`, but this test
//! lives in `engine-audio` so we don't need to pull engine-core's larger
//! dep graph. We replicate the trait's shape locally — the public surface
//! is the five methods (`start`, `queue`, `pause`, `resume`, `stop`) so a
//! parallel struct here matches by name.

use legaia_engine_audio::sequencer::Sequencer;
use legaia_engine_audio::spu::ram::SpuAllocator;
use legaia_engine_audio::{Spu, VabBank};
use legaia_seq::{SEQ_MAGIC, Seq};
use legaia_vab::{
    PROGRAMS_TABLE_SIZE, TONE_SIZE, TONES_PER_PROGRAM, VAB_HEADER_SIZE, VAB_MAGIC, VAG_BLOCK_BYTES,
    VAG_TABLE_ENTRIES, parse,
};

/// Build a one-program / one-tone / one-VAG VAB with a constant-amplitude
/// 4-block sample. Same shape as the existing `seq_vab_spu_chain` fixture
/// — kept here so this test stands alone.
fn build_vab() -> Vec<u8> {
    let prog_off = VAB_HEADER_SIZE;
    let tone_off = prog_off + PROGRAMS_TABLE_SIZE;
    let table_off = tone_off + TONE_SIZE * TONES_PER_PROGRAM;
    let vag_bodies_off = table_off + 2 * VAG_TABLE_ENTRIES;
    let vag_size = VAG_BLOCK_BYTES * 4;
    let total = vag_bodies_off + vag_size;
    let mut buf = vec![0u8; total];
    buf[0..4].copy_from_slice(&VAB_MAGIC.to_le_bytes());
    buf[4..8].copy_from_slice(&7u32.to_le_bytes());
    buf[8..12].copy_from_slice(&1u32.to_le_bytes());
    buf[12..16].copy_from_slice(&(total as u32).to_le_bytes());
    buf[18..20].copy_from_slice(&1u16.to_le_bytes());
    buf[20..22].copy_from_slice(&1u16.to_le_bytes());
    buf[22..24].copy_from_slice(&1u16.to_le_bytes());
    buf[24] = 127;
    buf[25] = 64;
    buf[prog_off] = 1;
    buf[prog_off + 1] = 127;
    let t0 = tone_off;
    buf[t0 + 2] = 127; // vol
    buf[t0 + 3] = 64; // pan
    buf[t0 + 4] = 60; // center key
    buf[t0 + 7] = 127; // max_key
    buf[t0 + 16..t0 + 18].copy_from_slice(&0x80FFu16.to_le_bytes());
    buf[t0 + 18..t0 + 20].copy_from_slice(&0x5FC0u16.to_le_bytes());
    buf[t0 + 22..t0 + 24].copy_from_slice(&1u16.to_le_bytes());
    let body_size_units = (vag_size / 8) as u16;
    buf[table_off + 2..table_off + 4].copy_from_slice(&body_size_units.to_le_bytes());
    for blk in 0..4 {
        let off = vag_bodies_off + blk * VAG_BLOCK_BYTES;
        for i in 0..14 {
            buf[off + 2 + i] = 0x77;
        }
    }
    buf
}

/// Build a one-NoteOn SEQ that fires a 60-key note at delta 0.
fn build_seq_bytes() -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&SEQ_MAGIC);
    buf.extend_from_slice(&[0x00, 0x01]);
    buf.extend_from_slice(&[0x01, 0xE0]);
    buf.extend_from_slice(&[0x07, 0xA1, 0x20]);
    buf.push(0x04);
    buf.push(0x02);
    buf.extend_from_slice(&[0x00, 0xC0, 0x00]); // ProgramChange
    buf.extend_from_slice(&[0x00, 0x90, 60, 100]); // NoteOn
    buf.extend_from_slice(&[0x83, 0x60, 60, 0]); // NoteOff at delta 480
    buf.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]); // EOT
    buf
}

/// Engine-side audio device: holds a `Spu`, builds a fresh `Sequencer` per
/// `start` call from raw SEQ bytes, mixes both into a render loop. This is
/// the shape we want a real engine to wear — the consumer of
/// `engine-core::scene::BgmDirector::start` should look like this.
///
/// `bank` must have been uploaded into [`Self::spu`] — the test fixture
/// uploads via [`AudioDevice::install_bank_into_self`] so the same SPU
/// instance owns both the sample memory and the playback voices.
pub struct AudioDevice {
    spu: Spu,
    sequencer: Option<Sequencer>,
    paused: bool,
    bank: Option<VabBank>,
}

impl Default for AudioDevice {
    fn default() -> Self {
        Self {
            spu: Spu::new(),
            sequencer: None,
            paused: false,
            bank: None,
        }
    }
}

impl AudioDevice {
    pub fn new() -> Self {
        Self::default()
    }

    /// Upload a VAB into this device's SPU and stash the resulting bank.
    /// `engine-core::SceneHost::scene_vab_bytes` produces the same
    /// `(report, bytes)` pair an engine would feed here.
    pub fn install_bank(&mut self, report: &legaia_vab::VabReport, bytes: &[u8]) {
        let mut alloc = SpuAllocator::new(0x1000, 0x40_000);
        let bank = VabBank::upload(&mut self.spu, &mut alloc, report, bytes);
        self.bank = Some(bank);
    }

    pub fn start(&mut self, _id: u16, seq_bytes: &[u8]) {
        let Ok(seq) = Seq::parse(seq_bytes) else {
            return;
        };
        let Some(bank) = self.bank.as_ref() else {
            return;
        };
        self.sequencer = Some(Sequencer::new(seq, bank.clone()));
        self.paused = false;
    }

    pub fn pause(&mut self) {
        self.paused = true;
    }

    pub fn resume(&mut self) {
        self.paused = false;
    }

    pub fn stop(&mut self) {
        if let Some(seq) = self.sequencer.as_mut() {
            seq.stop(&mut self.spu);
        }
        self.sequencer = None;
        self.paused = false;
    }

    /// Render `n` samples of stereo output, mirroring what cpal would
    /// pull during a real frame. Returns `(max |left|, max |right|)`.
    pub fn render_samples(&mut self, n: usize, dt_us: f64) -> (i32, i32) {
        if let (Some(seq), false) = (self.sequencer.as_mut(), self.paused) {
            seq.tick_us(&mut self.spu, dt_us);
        }
        let mut max_l = 0i32;
        let mut max_r = 0i32;
        for _ in 0..n {
            let (l, r) = self.spu.tick();
            max_l = max_l.max((l as i32).abs());
            max_r = max_r.max((r as i32).abs());
        }
        (max_l, max_r)
    }
}

#[test]
fn director_start_resolves_seq_and_renders_audio() {
    let vab_bytes = build_vab();
    let report = parse(&vab_bytes, 0).expect("parse VAB");

    let mut device = AudioDevice::new();
    device.install_bank(&report, &vab_bytes);
    let seq_bytes = build_seq_bytes();
    // First tick: no audio (no sequencer attached).
    let (l0, r0) = device.render_samples(64, 1_000.0);
    assert_eq!(l0, 0, "no sequencer → no audio");
    assert_eq!(r0, 0);

    // Engine emits a BGM-start event ⇒ director.start fires.
    device.start(42, &seq_bytes);
    let (l1, r1) = device.render_samples(2048, 5_000.0);
    assert!(
        l1 > 8 || r1 > 8,
        "sequencer should produce audible output, got L={l1} R={r1}"
    );

    // pause / resume gate. Pausing should freeze the playhead but the SPU
    // still produces release-tail output briefly; we only verify the
    // pause flag is honoured by checking that no further sequencer ticks
    // happen.
    device.pause();
    let ph_before = device
        .sequencer
        .as_ref()
        .map(|s| s.playhead_ticks())
        .unwrap_or(0);
    device.render_samples(2048, 5_000.0);
    let ph_after = device
        .sequencer
        .as_ref()
        .map(|s| s.playhead_ticks())
        .unwrap_or(0);
    assert_eq!(
        ph_before, ph_after,
        "sequencer playhead should not advance while paused"
    );
    device.resume();

    // Stop clears the sequencer.
    device.stop();
    assert!(device.sequencer.is_none());
}

#[test]
fn director_start_with_invalid_seq_bytes_silently_drops_request() {
    let vab_bytes = build_vab();
    let report = parse(&vab_bytes, 0).expect("parse VAB");
    let mut device = AudioDevice::new();
    device.install_bank(&report, &vab_bytes);
    // Bogus bytes — not a SEQ. Director should silently drop the request
    // (the engine-side BGM lookup also returns None for invalid IDs, so
    // robustness here matches the rest of the chain).
    device.start(42, b"this is not a SEQ");
    assert!(device.sequencer.is_none());
}

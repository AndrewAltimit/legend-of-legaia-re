//! Tick-driven sequencer: drives a [`crate::Spu`] from a parsed
//! [`legaia_seq::Seq`] + a loaded [`crate::vab_bind::VabBank`].
//!
//! Models the public surface of Sony's libsnd SsAPI sequencer
//! (`SsSeqOpen`/`SsSeqPlay`/`SsSeqClose`/`SsSeqSetVol`) without copying any
//! Sony bytes. The mapping it implements:
//!
//! - One sequencer drives **one** SEQ + **one** VAB bank.
//! - Channels 0..=15 map to **logical voice slots**; the sequencer assigns
//!   physical SPU voices on key-on by linear scan over the 24 voices, picks
//!   the first idle one, and remembers the `(channel, key) → voice` binding
//!   so the matching key-off can shut it down.
//! - Tempo is stored as microseconds-per-quarter-note; the SsAPI runtime
//!   rounds to a tick clock at `ppqn` ticks per quarter note. We keep a
//!   sub-tick accumulator (`tick_accum_us`) to advance forward by exact
//!   real-time deltas (`tick(dt_us)`).
//! - Tempo events from the SEQ override the running tempo at the event
//!   time (matching libsnd's mid-stream `0xFF 0x51`).
//!
//! Tests use synthetic SEQs + a stubbed `VabBank` shape so no Sony bytes
//! are ever instantiated.

use crate::Spu;
use crate::vab_bind::VabBank;
use legaia_seq::{ChannelMessage, EventBody, MetaMessage, Seq};

/// Number of MIDI channels (the SsAPI dispatcher iterates 0..=15).
pub const CHANNELS: usize = 16;

/// libsnd default initial tempo when the SEQ header is broken: 120 BPM.
pub const DEFAULT_TEMPO_US_PER_QN: u32 = 500_000;

/// Per-channel state carried across events.
#[derive(Debug, Clone, Copy)]
struct ChannelState {
    /// Current program (set by `ProgramChange`).
    program: u8,
    /// Per-channel volume (CC 7), 0..=127.
    volume: u8,
    /// Per-channel pan (CC 10), 0..=127.
    pan: u8,
    /// Last program-change tick. Diagnostic only.
    _last_pc_tick: u64,
}

impl Default for ChannelState {
    fn default() -> Self {
        Self {
            program: 0,
            volume: 127,
            pan: 64,
            _last_pc_tick: 0,
        }
    }
}

/// Active note tracker: `(channel, key, voice)` so a NoteOff can find the
/// voice that was started for the matching NoteOn.
#[derive(Debug, Clone, Copy)]
struct ActiveNote {
    channel: u8,
    key: u8,
    voice: u8,
}

/// Sequencer state machine. One per playing SEQ.
pub struct Sequencer {
    seq: Seq,
    bank: VabBank,

    /// Index of the *next* event to fire (events[next..]).
    next: usize,
    /// Microseconds accumulated since the last fired event. We keep us
    /// (not ticks) so a mid-stream `SetTempo` correctly applies to the
    /// *future* gap rather than re-pricing already-elapsed real-time.
    tick_accum_us: f64,
    /// Microseconds per PPQN tick. Recomputed on tempo change.
    us_per_tick: f64,
    /// Current tempo (us/qn).
    tempo_us_per_qn: u32,
    /// Absolute tick offset of the playhead (sum of fired event deltas).
    abs_tick: u64,
    /// Has end-of-track been reached.
    finished: bool,
    /// If non-zero, restart at this event index when EOT is reached.
    /// 0 means "loop to beginning"; `usize::MAX` means "no looping".
    loop_to: usize,

    channels: [ChannelState; CHANNELS],
    /// Active note → voice mappings. Linear search; bounded by SPU voice
    /// count (24), so this stays cheap.
    active: Vec<ActiveNote>,
    /// Master sequencer volume (libsnd `mvol`), 0..=127.
    master_vol: u8,
}

impl Sequencer {
    /// Build a sequencer over a parsed SEQ + an uploaded bank. Resets every
    /// channel to default state.
    pub fn new(seq: Seq, bank: VabBank) -> Self {
        let tempo = if seq.header.tempo_us_per_qn == 0 {
            DEFAULT_TEMPO_US_PER_QN
        } else {
            seq.header.tempo_us_per_qn
        };
        let ppqn = seq.header.ppqn.max(1);
        let us_per_tick = tempo as f64 / ppqn as f64;
        Self {
            seq,
            bank,
            next: 0,
            tick_accum_us: 0.0,
            us_per_tick,
            tempo_us_per_qn: tempo,
            abs_tick: 0,
            finished: false,
            loop_to: usize::MAX,
            channels: [ChannelState::default(); CHANNELS],
            active: Vec::new(),
            master_vol: 127,
        }
    }

    /// Set the master sequencer volume (libsnd `SsSeqSetVol`). 0..=127.
    pub fn set_master_vol(&mut self, v: u8) {
        self.master_vol = v.min(127);
    }

    /// Enable looping: rewinds the event index to `to` when the SEQ hits
    /// end-of-track. Pass `usize::MAX` to disable (the default).
    pub fn set_loop_to(&mut self, to: usize) {
        self.loop_to = to;
    }

    /// Has the sequence completed (no looping)?
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Current playhead in PPQN ticks (since start, or since the last loop
    /// rewind).
    pub fn playhead_ticks(&self) -> u64 {
        self.abs_tick
    }

    /// Current tempo in BPM.
    pub fn bpm(&self) -> f32 {
        if self.tempo_us_per_qn == 0 {
            0.0
        } else {
            60_000_000.0 / self.tempo_us_per_qn as f32
        }
    }

    /// Active note count (diagnostic).
    pub fn active_notes(&self) -> usize {
        self.active.len()
    }

    /// Advance the sequencer by `dt_us` microseconds, firing any events
    /// whose accumulated delta has elapsed.
    pub fn tick_us(&mut self, spu: &mut Spu, dt_us: f64) {
        if self.finished {
            return;
        }
        self.tick_accum_us += dt_us;
        loop {
            let Some(event) = self.seq.events.get(self.next) else {
                self.finished = true;
                if self.loop_to != usize::MAX && self.loop_to < self.seq.events.len() {
                    self.rewind_to(self.loop_to, spu);
                }
                return;
            };
            // delta_us recomputed every event so a mid-stream SetTempo
            // correctly affects the gap to the *next* event, not the one
            // that fires it.
            let delta_us = event.delta as f64 * self.us_per_tick;
            // f64(event.delta * us_per_tick) doesn't always round-trip
            // exactly; allow 1 microsecond of slack so a perfectly-matched
            // accumulation fires deterministically.
            const US_EPS: f64 = 1.0;
            if self.tick_accum_us + US_EPS < delta_us {
                return;
            }
            self.tick_accum_us -= delta_us;
            self.abs_tick += event.delta as u64;
            self.fire(spu, self.next);
            self.next += 1;
            // EOT branch: if this event was end-of-track and looping is on,
            // rewind. Otherwise mark finished and bail.
            if matches!(
                self.seq
                    .events
                    .get(self.next.wrapping_sub(1))
                    .map(|e| &e.body),
                Some(EventBody::Meta(MetaMessage::EndOfTrack))
            ) {
                if self.loop_to != usize::MAX && self.loop_to < self.seq.events.len() {
                    self.rewind_to(self.loop_to, spu);
                } else {
                    self.finished = true;
                    return;
                }
            }
        }
    }

    /// Convenience wrapper: advance by milliseconds.
    pub fn tick_ms(&mut self, spu: &mut Spu, dt_ms: f64) {
        self.tick_us(spu, dt_ms * 1000.0);
    }

    /// Key-off every active note and reset the playhead to `to`. Called by
    /// the loop logic and exposed publicly so engines can implement
    /// gameplay-driven loop points.
    pub fn rewind_to(&mut self, to: usize, spu: &mut Spu) {
        self.silence_all(spu);
        self.next = to;
        self.tick_accum_us = 0.0;
        self.finished = false;
        // Recompute abs_tick from the rewound position.
        self.abs_tick = self.seq.events[..to.min(self.seq.events.len())]
            .iter()
            .map(|e| e.delta as u64)
            .sum();
    }

    /// Force-stop: key-off every voice and freeze the sequencer. The
    /// sequencer is not destroyed; call `rewind_to(0, spu)` to restart.
    pub fn stop(&mut self, spu: &mut Spu) {
        self.silence_all(spu);
        self.finished = true;
    }

    fn silence_all(&mut self, spu: &mut Spu) {
        for note in self.active.drain(..) {
            if (note.voice as usize) < spu.voices.len() {
                spu.voices[note.voice as usize].key_off();
            }
        }
    }

    fn fire(&mut self, spu: &mut Spu, idx: usize) {
        let event = &self.seq.events[idx];
        match &event.body {
            EventBody::Channel { channel, message } => {
                let ch = (*channel as usize) % CHANNELS;
                self.fire_channel(spu, ch, *message);
            }
            EventBody::Meta(MetaMessage::SetTempo { us_per_qn }) => {
                self.tempo_us_per_qn = (*us_per_qn).max(1);
                let ppqn = self.seq.header.ppqn.max(1);
                self.us_per_tick = self.tempo_us_per_qn as f64 / ppqn as f64;
            }
            EventBody::Meta(_) => {
                // Non-tempo meta events are inert for playback.
            }
        }
    }

    fn fire_channel(&mut self, spu: &mut Spu, ch: usize, msg: ChannelMessage) {
        match msg {
            ChannelMessage::ProgramChange { program } => {
                self.channels[ch].program = program;
            }
            ChannelMessage::ControlChange { control, value } => match control {
                0x07 => self.channels[ch].volume = value,
                0x0A => self.channels[ch].pan = value,
                _ => {}
            },
            ChannelMessage::NoteOn { key, velocity } => {
                if velocity == 0 {
                    self.note_off(spu, ch as u8, key);
                } else {
                    self.note_on(spu, ch as u8, key, velocity);
                }
            }
            ChannelMessage::NoteOff { key, .. } => {
                self.note_off(spu, ch as u8, key);
            }
            // PolyAftertouch / ChannelAftertouch / PitchBend not yet wired.
            _ => {}
        }
    }

    fn note_on(&mut self, spu: &mut Spu, channel: u8, key: u8, velocity: u8) {
        // Drop the prior instance of this (channel, key) if it exists -
        // libsnd silently restarts the voice.
        self.note_off(spu, channel, key);
        let Some(voice) = self.alloc_voice(spu) else {
            log::trace!(
                "sequencer: no free voice for ch{} key{} (active={})",
                channel,
                key,
                self.active.len()
            );
            return;
        };
        let cs = self.channels[channel as usize];
        // Combine master, channel, and event velocity so the bank's
        // play_note math sees a single 0..=127 effective velocity. The
        // bank further multiplies by program & tone vol.
        let combined = ((self.master_vol as u32 * cs.volume as u32 * velocity as u32) / (127 * 127))
            .min(127) as u8;
        let ok = self
            .bank
            .play_note(spu, voice as usize, cs.program as usize, key, combined);
        if ok {
            self.active.push(ActiveNote {
                channel,
                key,
                voice,
            });
        }
    }

    fn note_off(&mut self, spu: &mut Spu, channel: u8, key: u8) {
        let mut idx = 0;
        while idx < self.active.len() {
            let n = self.active[idx];
            if n.channel == channel && n.key == key {
                if (n.voice as usize) < spu.voices.len() {
                    spu.voices[n.voice as usize].key_off();
                }
                self.active.swap_remove(idx);
            } else {
                idx += 1;
            }
        }
    }

    fn alloc_voice(&self, spu: &Spu) -> Option<u8> {
        // Skip voices currently bound to active notes; the SPU itself can
        // advertise a voice as "off" between key-off and tail-fade, but we
        // don't want to steal one we still own.
        let occupied: u32 = self
            .active
            .iter()
            .map(|n| 1u32 << n.voice as u32)
            .fold(0, |a, b| a | b);
        for (i, voice) in spu.voices.iter().enumerate() {
            if occupied & (1 << i) == 0 && voice.is_off() {
                return Some(i as u8);
            }
        }
        // No idle voice available. Try stealing the oldest active note.
        self.active.first().map(|n| n.voice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_seq::SEQ_MAGIC;

    fn synthetic_seq() -> Seq {
        // ppqn=480, tempo 500000 us/qn (120 BPM) so 1 quarter = 500_000 us.
        let mut buf = Vec::new();
        buf.extend_from_slice(&SEQ_MAGIC);
        buf.extend_from_slice(&[0x00, 0x01]);
        buf.extend_from_slice(&[0x01, 0xE0]);
        buf.extend_from_slice(&[0x07, 0xA1, 0x20]);
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
        // delta 480 (1 quarter = 500000 us), NoteOff via NoteOn vel=0
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

    fn empty_bank() -> VabBank {
        VabBank {
            master_vol: 127,
            samples: vec![],
            programs: vec![],
        }
    }

    #[test]
    fn fires_program_change_immediately() {
        let mut spu = Spu::new();
        let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
        seq.tick_us(&mut spu, 0.0);
        // First two events have delta 0 and fire on the first tick (even
        // with dt=0, the loop drains zero-delta events). Channel 0 program
        // should be 0 and there should be no active note (bank is empty so
        // play_note returns false).
        assert_eq!(seq.channels[0].program, 0);
        assert_eq!(seq.active_notes(), 0);
        // Third event has delta 480 - not yet fired.
        assert_eq!(seq.next, 2);
    }

    #[test]
    fn advances_tick_count_after_quarter_note() {
        let mut spu = Spu::new();
        let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
        seq.tick_us(&mut spu, 500_000.0);
        // After 1 quarter-note: 3rd event (NoteOff, +480) should have fired.
        assert!(seq.next >= 3);
    }

    #[test]
    fn finishes_at_end_of_track() {
        let mut spu = Spu::new();
        let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
        // Drain all events with one big tick (well past total length).
        seq.tick_us(&mut spu, 10_000_000.0);
        assert!(seq.is_finished());
    }

    #[test]
    fn loop_rewinds_at_eot() {
        let mut spu = Spu::new();
        let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
        seq.set_loop_to(0);
        seq.tick_us(&mut spu, 10_000_000.0);
        // Looping should have prevented the finished flag.
        assert!(!seq.is_finished());
        // And re-fired at least one channel event.
        assert!(seq.playhead_ticks() < seq.seq.total_ticks() * 2);
    }

    #[test]
    fn tempo_event_changes_us_per_tick() {
        // Build a SEQ that starts at 120 BPM then jumps to 60 BPM after 0
        // ticks: the second event should be twice as long in real time.
        let mut buf = Vec::new();
        buf.extend_from_slice(&SEQ_MAGIC);
        buf.extend_from_slice(&[0x00, 0x01]);
        buf.extend_from_slice(&[0x01, 0xE0]);
        // initial tempo 500_000 (120 BPM)
        buf.extend_from_slice(&[0x07, 0xA1, 0x20]);
        buf.push(0x04);
        buf.push(0x02);
        // delta 0, set-tempo to 1_000_000 us/qn (60 BPM)
        buf.push(0x00);
        buf.push(0xFF);
        buf.push(0x51);
        buf.push(0x03);
        buf.extend_from_slice(&[0x0F, 0x42, 0x40]); // 1_000_000 BE
        // delta 480, end-of-track
        buf.push(0x83);
        buf.push(0x60);
        buf.push(0xFF);
        buf.push(0x2F);
        buf.push(0x00);
        let seq = Seq::parse(&buf).unwrap();
        let mut s = Sequencer::new(seq, empty_bank());
        let mut spu = Spu::new();
        // 1 quarter at the original tempo wouldn't reach EOT, but the
        // post-tempo-change tick rate should mean 1 quarter requires 1s.
        s.tick_us(&mut spu, 500_000.0);
        assert!(!s.is_finished()); // halfway through
        s.tick_us(&mut spu, 500_000.0);
        assert!(s.is_finished());
    }
}

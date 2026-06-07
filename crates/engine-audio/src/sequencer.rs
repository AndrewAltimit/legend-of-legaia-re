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
//!   advances a tick clock at `ppqn` ticks per quarter note.
//! - Timing uses an **exact integer accumulator** clocked in SPU samples
//!   (44.1 kHz) - no floating-point per-tick rate, so there is no drift on
//!   long tracks and playback is bit-deterministic. The accumulator counts
//!   in units of `sample × ppqn × 1_000_000`; an event whose delta is
//!   `d` ticks fires once the accumulator reaches `d × tempo_us × 44100`
//!   (the algebraic rearrangement of `elapsed_seconds ≥ d × tempo_us /
//!   (ppqn × 1e6)`), which keeps every term an integer.
//! - Tempo events from the SEQ override the running tempo at the event
//!   time (matching libsnd's mid-stream `0xFF 0x51`).
//! - Loop points are read **from the stream**: PSX SEQ encodes them as
//!   NRPN-style control changes on status `0xB0` - controller 99 (`0x63`)
//!   value 20 = Loop Start, value 30 = Loop Forever. When a Loop Start fires
//!   the sequencer remembers the position; a later Loop Forever (or an
//!   end-of-track that follows a Loop Start) rewinds to that marker rather
//!   than to the beginning. An external [`Sequencer::set_loop_to`] remains as
//!   a fallback for the handful of retail tracks that carry no markers.
//!
//! Tests use synthetic SEQs + a stubbed `VabBank` shape so no Sony bytes
//! are ever instantiated.

use crate::Spu;
use crate::spu::voice::SPU_INTERNAL_RATE;
use crate::vab_bind::VabBank;
use legaia_seq::{ChannelMessage, EventBody, MetaMessage, Seq};

/// Number of MIDI channels (the SsAPI dispatcher iterates 0..=15).
pub const CHANNELS: usize = 16;

/// libsnd default initial tempo when the SEQ header is broken: 120 BPM.
pub const DEFAULT_TEMPO_US_PER_QN: u32 = 500_000;

/// NRPN-style controller that carries SEQ loop markers (controller 99).
const CC_LOOP_MARKER: u8 = 0x63;
/// `CC_LOOP_MARKER` value marking a Loop Start point.
const LOOP_START_VALUE: u8 = 20;
/// `CC_LOOP_MARKER` value marking Loop Forever (jump to the last Loop Start).
const LOOP_FOREVER_VALUE: u8 = 30;

/// Center value of the 14-bit pitch-bend wheel - no bend.
const PITCH_BEND_CENTER: u16 = 0x2000;

/// Convert a 14-bit pitch-bend wheel value into a pitch multiplier (`1.0` at
/// center) using the sounding tone's own bend range (semitones): `up` at
/// full-up wheel, `down` at full-down. The range is the VAB tone's
/// `pbmax`/`pbmin` (disc-sourced, per [`VabBank::pitch_bend_range`]), so a
/// tone with a `(0, 0)` range does not respond to the wheel at all - exactly
/// as libsnd applies the per-tone range rather than a global constant. Shared
/// by NoteOn (bend a fresh note) and the `0xEn` handler (re-bend sounding
/// notes).
fn pitch_bend_factor(bend: u16, down: u8, up: u8) -> f64 {
    let norm = (bend as f64 - PITCH_BEND_CENTER as f64) / PITCH_BEND_CENTER as f64;
    let range = if norm >= 0.0 { up } else { down } as f64;
    let semitones = norm * range;
    2f64.powf(semitones / 12.0)
}

/// Scale a base SPU pitch register by a bend factor, clamped to the valid
/// `1..=0x3FFF` register range (the same clamp `compute_pitch` applies).
fn bend_pitch(base: u16, factor: f64) -> u16 {
    ((base as f64 * factor).round() as i64).clamp(1, 0x3FFF) as u16
}

/// Center value of the 7-bit channel-pan controller (CC10) - no pan.
const PAN_CENTER: u8 = 0x40;

/// Apply a channel pan (CC10, `0..=127`) to a voice's `(left, right)` volume
/// pair using libsnd's pan law: a pan left of center (`< 0x40`) attenuates
/// the **right** side by `pan/0x3f`; a pan right of center attenuates the
/// **left** side by `(0x7f - pan)/0x3f`. This is the per-pan-source stage the
/// libsnd voice-volume builder applies on top of the tone's own L/R (it runs
/// this same attenuation once per pan source - channel / sequence / tone).
/// The engine bakes the tone pan into the base L/R via `pan_split`, so this
/// adds only the channel pan that `play_note` does not see.
fn apply_channel_pan(left: i16, right: i16, pan: u8) -> (i16, i16) {
    let pan = pan.min(0x7f) as i32;
    if pan < 0x40 {
        (left, (right as i32 * pan / 0x3f) as i16)
    } else {
        ((left as i32 * (0x7f - pan) / 0x3f) as i16, right)
    }
}

/// Per-channel state carried across events.
#[derive(Debug, Clone, Copy)]
struct ChannelState {
    /// Current program (set by `ProgramChange`).
    program: u8,
    /// Per-channel volume (CC 7), 0..=127.
    volume: u8,
    /// Per-channel pan (CC 10), 0..=127.
    pan: u8,
    /// Current 14-bit pitch-bend wheel value (`0..=0x3FFF`, center `0x2000`).
    /// Set by `0xEn` events; applied to every note sounding on the channel
    /// and to subsequent NoteOns. The retail score uses this (see the
    /// `real_seq_expressive_events` corpus sweep); aftertouch is never used.
    pitch_bend: u16,
    /// Last program-change tick. Diagnostic only.
    _last_pc_tick: u64,
}

impl Default for ChannelState {
    fn default() -> Self {
        Self {
            program: 0,
            volume: 127,
            pan: 64,
            pitch_bend: PITCH_BEND_CENTER,
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
    /// The voice's SPU pitch register at NoteOn, before this channel's
    /// pitch-bend was folded in. Re-applying a new bend multiplies this
    /// base so repeated bends don't compound rounding error.
    base_pitch: u16,
    /// The sounding tone's pitch-bend range `(pbmin, pbmax)` in semitones,
    /// captured at NoteOn so a later `0xEn` event scales by the note's own
    /// range (a `(0, 0)` tone never bends).
    bend_range: (u8, u8),
    /// The voice's `(left, right)` volume with the tone pan baked in but the
    /// channel pan (CC10) NOT yet applied. A later CC10 change re-pans from
    /// this base so repeated pans don't compound, mirroring `base_pitch`.
    base_vol: (i16, i16),
}

/// Sequencer state machine. One per playing SEQ.
pub struct Sequencer {
    seq: Seq,
    bank: VabBank,

    /// Index of the *next* event to fire (events[next..]).
    next: usize,
    /// Integer time accumulated since the last fired event, in units of
    /// `sample × ppqn × 1_000_000`. We accumulate elapsed time (not ticks)
    /// so a mid-stream `SetTempo` correctly applies to the *future* gap
    /// rather than re-pricing already-elapsed real-time. See [`Self::fire_at`]
    /// for the exact-integer fire threshold.
    accum: u64,
    /// Accumulator increment per SPU sample: `ppqn × 1_000_000`. Constant for
    /// the life of the sequencer (ppqn is fixed by the header).
    accum_per_sample: u64,
    /// Fractional-sample carry for the `tick_us(f64)` entry point so repeated
    /// non-integer microsecond deltas convert to whole samples without drift.
    /// The sample-clocked [`Self::tick_sample`] path never touches this.
    sample_carry: f64,
    /// Current tempo (us/qn).
    tempo_us_per_qn: u32,
    /// Absolute tick offset of the playhead (sum of fired event deltas).
    abs_tick: u64,
    /// Has end-of-track been reached.
    finished: bool,
    /// External loop fallback: restart at this event index when EOT is
    /// reached *and* no in-stream Loop Start marker has been seen. 0 means
    /// "loop to beginning"; `usize::MAX` means "no looping". In-stream markers
    /// (see [`Self::loop_start`]) take precedence over this.
    loop_to: usize,
    /// Event index recorded when an in-stream Loop Start marker (CC 99 value
    /// 20) fires - the position a Loop Forever marker or a following EOT
    /// rewinds to. It points at the event *after* the marker, so a rewind
    /// neither re-fires the marker nor re-applies its delta. `None` until the
    /// first Loop Start is seen; persists across loop rewinds so repeated
    /// loops keep returning to the same bar.
    loop_start: Option<usize>,
    /// Set when a Loop Forever marker (CC 99 value 30) fires; the advance loop
    /// consumes it, rewinds, and clears it.
    pending_loop_forever: bool,

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
        Self {
            seq,
            bank,
            next: 0,
            accum: 0,
            accum_per_sample: ppqn as u64 * 1_000_000,
            sample_carry: 0.0,
            tempo_us_per_qn: tempo,
            abs_tick: 0,
            finished: false,
            loop_to: usize::MAX,
            loop_start: None,
            pending_loop_forever: false,
            channels: [ChannelState::default(); CHANNELS],
            active: Vec::new(),
            master_vol: 127,
        }
    }

    /// Set the master sequencer volume (libsnd `SsSeqSetVol`). 0..=127.
    pub fn set_master_vol(&mut self, v: u8) {
        self.master_vol = v.min(127);
    }

    /// Set the external loop fallback: rewinds the event index to `to` when
    /// the SEQ hits end-of-track *and* the stream carried no Loop Start marker.
    /// Pass `usize::MAX` to disable (the default). In-stream loop markers (CC
    /// 99 value 20/30) take precedence over this fallback.
    pub fn set_loop_to(&mut self, to: usize) {
        self.loop_to = to;
    }

    /// Where a loop should rewind to: the in-stream Loop Start marker if one
    /// has fired, else the external [`Self::set_loop_to`] fallback, else
    /// `None` (no looping). Bounds-checked against the event count.
    fn loop_target(&self) -> Option<usize> {
        if let Some(start) = self.loop_start
            && start < self.seq.events.len()
        {
            return Some(start);
        }
        if self.loop_to != usize::MAX && self.loop_to < self.seq.events.len() {
            return Some(self.loop_to);
        }
        None
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

    /// Advance the sequencer by exactly one SPU sample (1 / 44100 s). This is
    /// the production playback clock: callers ticking the SPU sample-by-sample
    /// call this once per `Spu::tick`, so the music timebase is locked to the
    /// audio clock with no floating-point drift.
    pub fn tick_sample(&mut self, spu: &mut Spu) {
        self.advance_samples(spu, 1);
    }

    /// Advance the sequencer by `dt_us` microseconds, firing any events whose
    /// accumulated delta has elapsed. Kept for callers that drive the
    /// sequencer off a wall-clock / per-frame delta (parity oracles, tests);
    /// the microsecond delta is converted to whole SPU samples with a
    /// fractional carry so repeated non-integer deltas don't drift.
    pub fn tick_us(&mut self, spu: &mut Spu, dt_us: f64) {
        if self.finished {
            return;
        }
        // samples = dt_us * 44100 / 1e6, with the leftover fraction carried
        // to the next call so the conversion is drift-free over a long track.
        let exact = dt_us.max(0.0) * SPU_INTERNAL_RATE as f64 / 1_000_000.0 + self.sample_carry;
        let whole = exact.floor();
        self.sample_carry = exact - whole;
        let samples = whole.max(0.0) as u64;
        // Always advance (even by zero samples) so a leading run of zero-delta
        // events drains on the first tick, matching the old behaviour.
        self.advance_samples(spu, samples);
    }

    /// Convenience wrapper: advance by milliseconds.
    pub fn tick_ms(&mut self, spu: &mut Spu, dt_ms: f64) {
        self.tick_us(spu, dt_ms * 1000.0);
    }

    /// Core integer-clocked advance. Adds `samples` SPU samples worth of time
    /// to the accumulator, then fires every event that has come due.
    fn advance_samples(&mut self, spu: &mut Spu, samples: u64) {
        if self.finished {
            return;
        }
        self.accum = self
            .accum
            .saturating_add(self.accum_per_sample.saturating_mul(samples));
        loop {
            let Some(event) = self.seq.events.get(self.next) else {
                self.finished = true;
                if let Some(target) = self.loop_target() {
                    self.rewind_to(target, spu);
                }
                return;
            };
            // Fire threshold (exact integer): the event fires once the
            // accumulator (sample × ppqn × 1e6) reaches `delta × tempo_us ×
            // 44100`. Recomputed every event so a mid-stream SetTempo affects
            // the gap to the *next* event, not the one that fired it.
            let threshold = (event.delta as u64)
                .saturating_mul(self.tempo_us_per_qn as u64)
                .saturating_mul(SPU_INTERNAL_RATE as u64);
            if self.accum < threshold {
                return;
            }
            self.accum -= threshold;
            self.abs_tick += event.delta as u64;
            self.fire(spu, self.next);
            self.next += 1;
            // Loop Forever branch: an in-stream CC 99 value 30 marker just
            // fired. Rewind to the last Loop Start (or the track beginning if
            // none was seen) and resume - this takes precedence over EOT.
            if self.pending_loop_forever {
                self.pending_loop_forever = false;
                let target = self.loop_start.unwrap_or(0).min(self.seq.events.len());
                self.rewind_to(target, spu);
                continue;
            }
            // EOT branch: if this event was end-of-track and a loop point is
            // available (an in-stream Loop Start or the external fallback),
            // rewind to it. Otherwise mark finished and bail.
            if matches!(
                self.seq
                    .events
                    .get(self.next.wrapping_sub(1))
                    .map(|e| &e.body),
                Some(EventBody::Meta(MetaMessage::EndOfTrack))
            ) {
                if let Some(target) = self.loop_target() {
                    self.rewind_to(target, spu);
                } else {
                    self.finished = true;
                    return;
                }
            }
        }
    }

    /// Key-off every active note and reset the playhead to `to`. Called by
    /// the loop logic and exposed publicly so engines can implement
    /// gameplay-driven loop points.
    pub fn rewind_to(&mut self, to: usize, spu: &mut Spu) {
        self.silence_all(spu);
        self.next = to;
        self.accum = 0;
        self.sample_carry = 0.0;
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
                // Tempo only re-prices the gap to *future* events; the
                // accumulator is in tempo-independent units (sample × ppqn ×
                // 1e6), so no rescaling of the carried remainder is needed.
                self.tempo_us_per_qn = (*us_per_qn).max(1);
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
                0x0A => {
                    self.channels[ch].pan = value;
                    // Re-pan every voice sounding on this channel from its
                    // pan-free base so successive CC10 events don't compound.
                    for note in self.active.iter().filter(|n| n.channel as usize == ch) {
                        if let Some(v) = spu.voices.get_mut(note.voice as usize) {
                            let (l, r) = apply_channel_pan(note.base_vol.0, note.base_vol.1, value);
                            v.vol_left = l;
                            v.vol_right = r;
                        }
                    }
                }
                CC_LOOP_MARKER => match value {
                    LOOP_START_VALUE => {
                        // Record the position *after* this marker. `self.next`
                        // is still this event's index here (the advance loop
                        // increments it after `fire` returns), so the event
                        // following the marker is `next + 1`. Rewinding there
                        // skips re-firing the marker and re-applying its delta.
                        self.loop_start = Some(self.next + 1);
                    }
                    LOOP_FOREVER_VALUE => {
                        self.pending_loop_forever = true;
                    }
                    _ => {}
                },
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
            ChannelMessage::PitchBend { value } => {
                self.channels[ch].pitch_bend = value;
                for note in self.active.iter().filter(|n| n.channel as usize == ch) {
                    if let Some(v) = spu.voices.get_mut(note.voice as usize) {
                        let (down, up) = note.bend_range;
                        v.pitch = bend_pitch(note.base_pitch, pitch_bend_factor(value, down, up));
                    }
                }
            }
            // Aftertouch (channel + poly) is recognized but unused by the
            // retail score (corpus sweep), so there is nothing to drive.
            ChannelMessage::PolyAftertouch { .. } | ChannelMessage::ChannelAftertouch { .. } => {}
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
            // play_note set the voice's base pitch; remember it (and the
            // tone's disc-sourced bend range) so later bends re-scale the
            // unbent value, then fold in any bend already held on this channel.
            let base_pitch = spu
                .voices
                .get(voice as usize)
                .map(|v| v.pitch)
                .unwrap_or(0x1000);
            let bend_range = self.bank.pitch_bend_range(cs.program as usize, key);
            if cs.pitch_bend != PITCH_BEND_CENTER
                && let Some(v) = spu.voices.get_mut(voice as usize)
            {
                let (down, up) = bend_range;
                v.pitch = bend_pitch(base_pitch, pitch_bend_factor(cs.pitch_bend, down, up));
            }
            // play_note baked the tone pan into the voice L/R; capture that as
            // the channel-pan-free base, then apply the channel pan (CC10) on
            // top - the stage play_note does not see.
            let base_vol = spu
                .voices
                .get(voice as usize)
                .map(|v| (v.vol_left, v.vol_right))
                .unwrap_or((0x3FFF, 0x3FFF));
            if cs.pan != PAN_CENTER
                && let Some(v) = spu.voices.get_mut(voice as usize)
            {
                let (l, r) = apply_channel_pan(base_vol.0, base_vol.1, cs.pan);
                v.vol_left = l;
                v.vol_right = r;
            }
            self.active.push(ActiveNote {
                channel,
                key,
                voice,
                base_pitch,
                bend_range,
                base_vol,
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

    /// The integer sample-clock fires an event at exactly the right SPU
    /// sample: at 120 BPM / ppqn 480, one quarter note (delta 480) is 0.5 s =
    /// 22050 samples. The event must fire on sample 22050, not 22049, with no
    /// floating-point slack.
    #[test]
    fn sample_clock_fires_quarter_note_at_exact_sample() {
        // ppqn 480, tempo 500000 (120 BPM). Events: ProgramChange (delta 0),
        // NoteOn (delta 480), EOT (delta 0).
        let mut buf = Vec::new();
        buf.extend_from_slice(&SEQ_MAGIC);
        buf.extend_from_slice(&[0x00, 0x01]);
        buf.extend_from_slice(&[0x01, 0xE0]); // ppqn 480
        buf.extend_from_slice(&[0x07, 0xA1, 0x20]); // 500000 us/qn
        buf.push(0x04);
        buf.push(0x02);
        buf.push(0x00); // delta 0
        buf.push(0xC0);
        buf.push(0x00); // ProgramChange
        buf.push(0x83); // delta 480
        buf.push(0x60);
        buf.push(0x90);
        buf.push(60);
        buf.push(100); // NoteOn
        buf.push(0x00);
        buf.push(0xFF);
        buf.push(0x2F); // EOT
        let seq = Seq::parse(&buf).unwrap();
        let mut s = Sequencer::new(seq, empty_bank());
        let mut spu = Spu::new();

        // Drain the leading zero-delta ProgramChange.
        s.tick_sample(&mut spu);
        assert_eq!(s.next, 1, "ProgramChange fires immediately");

        // 22050 samples == exactly one quarter at 120 BPM. We already consumed
        // one sample, so 22048 more leaves us one short; the 22049th tips it.
        for _ in 0..22_048 {
            s.tick_sample(&mut spu);
        }
        assert_eq!(s.next, 1, "NoteOn must not fire one sample early");
        s.tick_sample(&mut spu);
        assert!(s.next >= 2, "NoteOn fires on the exact 22050th sample");
    }

    /// A track with a mid-stream Loop Start marker (CC 99 value 20) must
    /// rewind to that marker - not to event 0 - when Loop Forever (CC 99
    /// value 30) fires, and the integer sample-clock must stay exact across
    /// the rewind so the looped body re-fires on the same sample offset.
    #[test]
    fn loop_forever_rewinds_to_in_stream_marker_not_zero() {
        // ppqn 480, tempo 500000 (120 BPM) => one quarter (delta 480) is
        // exactly 22050 SPU samples at 44.1 kHz.
        let mut buf = Vec::new();
        buf.extend_from_slice(&SEQ_MAGIC);
        buf.extend_from_slice(&[0x00, 0x01]); // version 1 (PsyQ shape)
        buf.extend_from_slice(&[0x01, 0xE0]); // ppqn 480
        buf.extend_from_slice(&[0x07, 0xA1, 0x20]); // 500000 us/qn
        buf.push(0x04);
        buf.push(0x02);
        // idx 0: delta 0, ProgramChange ch0 prog 0
        buf.push(0x00);
        buf.push(0xC0);
        buf.push(0x00);
        // idx 1: delta 480, NoteOn ch0 key 60 vel 100
        buf.push(0x83);
        buf.push(0x60);
        buf.push(0x90);
        buf.push(60);
        buf.push(100);
        // idx 2: delta 0, Loop Start (CC 0xB0 ch0 controller 99 value 20).
        // Records the rewind target as idx 3 (the event after this marker).
        buf.push(0x00);
        buf.push(0xB0);
        buf.push(99);
        buf.push(20);
        // idx 3: delta 480, NoteOn ch0 key 64 vel 100 (the loop body start)
        buf.push(0x83);
        buf.push(0x60);
        buf.push(0x90);
        buf.push(64);
        buf.push(100);
        // idx 4: delta 480, Loop Forever (CC 99 value 30) -> rewind to idx 3
        buf.push(0x83);
        buf.push(0x60);
        buf.push(0xB0);
        buf.push(99);
        buf.push(30);
        // idx 5: delta 0, end-of-track (never reached while looping)
        buf.push(0x00);
        buf.push(0xFF);
        buf.push(0x2F);
        let seq = Seq::parse(&buf).unwrap();
        let mut s = Sequencer::new(seq, empty_bank());
        let mut spu = Spu::new();

        // Loop Forever (idx 4) fires at sample 66150 (= 3 quarters). After it
        // fires the sequencer rewinds to the recorded Loop Start target.
        for _ in 0..66_150 {
            s.tick_sample(&mut spu);
        }
        assert!(!s.is_finished(), "Loop Forever must not finish the track");
        assert_eq!(
            s.loop_start,
            Some(3),
            "Loop Start records the event after the marker"
        );
        assert_eq!(
            s.next, 3,
            "Loop Forever rewinds to the in-stream marker (idx 3), not to 0"
        );

        // Exactness across the rewind: the looped NoteOn at idx 3 has delta
        // 480 = 22050 samples. It must not re-fire one sample early.
        for _ in 0..22_049 {
            s.tick_sample(&mut spu);
        }
        assert_eq!(s.next, 3, "looped NoteOn must not fire early after rewind");
        s.tick_sample(&mut spu);
        assert!(
            s.next >= 4,
            "looped NoteOn fires on the exact sample after rewind"
        );

        // And it keeps looping: drive well past another Loop Forever and the
        // playhead is still parked at the marker, never finished.
        for _ in 0..66_150 {
            s.tick_sample(&mut spu);
        }
        assert!(!s.is_finished());
        assert_eq!(s.next, 3, "repeated loops keep returning to the marker");
    }

    /// When the stream carries a Loop Start marker, end-of-track rewinds to it
    /// even if no external [`Sequencer::set_loop_to`] fallback was set.
    #[test]
    fn eot_rewinds_to_marker_without_external_loop() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&SEQ_MAGIC);
        buf.extend_from_slice(&[0x00, 0x01]);
        buf.extend_from_slice(&[0x01, 0xE0]);
        buf.extend_from_slice(&[0x07, 0xA1, 0x20]);
        buf.push(0x04);
        buf.push(0x02);
        // idx 0: delta 0, Loop Start
        buf.push(0x00);
        buf.push(0xB0);
        buf.push(99);
        buf.push(20);
        // idx 1: delta 480, NoteOn
        buf.push(0x83);
        buf.push(0x60);
        buf.push(0x90);
        buf.push(60);
        buf.push(100);
        // idx 2: delta 0, end-of-track
        buf.push(0x00);
        buf.push(0xFF);
        buf.push(0x2F);
        let seq = Seq::parse(&buf).unwrap();
        let mut s = Sequencer::new(seq, empty_bank());
        // Note: set_loop_to is NOT called - looping is driven purely by the
        // in-stream marker.
        let mut spu = Spu::new();
        for _ in 0..40_000 {
            s.tick_sample(&mut spu);
        }
        assert!(
            !s.is_finished(),
            "in-stream marker loops without set_loop_to"
        );
        // Loop Start recorded idx 1; EOT rewound there rather than finishing.
        assert_eq!(s.loop_start, Some(1));
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
        // delta 0, set-tempo to 1_000_000 us/qn (60 BPM). PSX SEQ meta has no
        // MIDI length byte: the 3 tempo bytes follow 0x51 directly.
        buf.push(0x00);
        buf.push(0xFF);
        buf.push(0x51);
        buf.extend_from_slice(&[0x0F, 0x42, 0x40]); // 1_000_000 BE
        // delta 480, end-of-track
        buf.push(0x83);
        buf.push(0x60);
        buf.push(0xFF);
        buf.push(0x2F);
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

    #[test]
    fn pitch_bend_factor_center_is_unity() {
        // Center wheel = no bend, whatever the tone range.
        assert!((pitch_bend_factor(PITCH_BEND_CENTER, 2, 2) - 1.0).abs() < 1e-9);
        // With a ±2-semitone tone range, full up bends sharp, full down flat.
        assert!(pitch_bend_factor(0x3FFF, 2, 2) > 1.0);
        assert!(pitch_bend_factor(0x0000, 2, 2) < 1.0);
        // A tone with a (0, 0) range never responds to the wheel.
        assert!((pitch_bend_factor(0x3FFF, 0, 0) - 1.0).abs() < 1e-9);
        assert!((pitch_bend_factor(0x0000, 0, 0) - 1.0).abs() < 1e-9);
        // Asymmetric range: full up uses `up`, full down uses `down`.
        let up = pitch_bend_factor(0x3FFF, 1, 12);
        let down = pitch_bend_factor(0x0000, 1, 12);
        assert!(up > 1.5); // ~+1 octave
        assert!(down > 0.94 && down < 1.0); // ~-1 semitone
    }

    #[test]
    fn bend_pitch_clamps_to_register_range() {
        // A large base × a sharp bend saturates at 0x3FFF, never overflows.
        assert_eq!(
            bend_pitch(0x3FFF, pitch_bend_factor(0x3FFF, 12, 12)),
            0x3FFF
        );
        // A bend never drives the register below 1.
        assert!(bend_pitch(1, pitch_bend_factor(0x0000, 12, 12)) >= 1);
        // Center leaves a mid-range pitch untouched.
        assert_eq!(
            bend_pitch(0x1000, pitch_bend_factor(PITCH_BEND_CENTER, 2, 2)),
            0x1000
        );
    }

    #[test]
    fn pitch_bend_event_repitches_sounding_voice() {
        let mut spu = Spu::new();
        let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
        // Seed a sounding note on channel 0, voice 0 at a known base pitch.
        let base = 0x1000u16;
        spu.voices[0].pitch = base;
        seq.active.push(ActiveNote {
            channel: 0,
            key: 60,
            voice: 0,
            base_pitch: base,
            bend_range: (2, 2),
            base_vol: (0x3FFF, 0x3FFF),
        });

        // Bend sharp: the voice's live pitch register rises, the channel
        // state records the wheel, and the stored base is untouched.
        seq.fire_channel(&mut spu, 0, ChannelMessage::PitchBend { value: 0x3FFF });
        assert_eq!(seq.channels[0].pitch_bend, 0x3FFF);
        assert!(spu.voices[0].pitch > base);
        assert_eq!(seq.active[0].base_pitch, base);

        // Return to center: the voice snaps back to exactly the base pitch
        // (re-bending the base, not the already-bent value).
        seq.fire_channel(
            &mut spu,
            0,
            ChannelMessage::PitchBend {
                value: PITCH_BEND_CENTER,
            },
        );
        assert_eq!(spu.voices[0].pitch, base);
    }

    #[test]
    fn apply_channel_pan_attenuates_opposite_side() {
        // Center pan leaves both sides untouched.
        assert_eq!(
            apply_channel_pan(0x3000, 0x3000, PAN_CENTER),
            (0x3000, 0x3000)
        );
        // Hard left silences the right, leaves the left.
        assert_eq!(apply_channel_pan(0x3000, 0x3000, 0), (0x3000, 0));
        // Hard right silences the left, leaves the right.
        assert_eq!(apply_channel_pan(0x3000, 0x3000, 0x7f), (0, 0x3000));
        // Partial left attenuates the right but not below zero / not the left.
        let (l, r) = apply_channel_pan(0x3000, 0x3000, 0x20);
        assert_eq!(l, 0x3000);
        assert!(r > 0 && r < 0x3000);
    }

    #[test]
    fn cc10_pan_repans_sounding_voice_from_base() {
        let mut spu = Spu::new();
        let mut seq = Sequencer::new(synthetic_seq(), empty_bank());
        let base = (0x3000i16, 0x3000i16);
        spu.voices[0].vol_left = base.0;
        spu.voices[0].vol_right = base.1;
        seq.active.push(ActiveNote {
            channel: 0,
            key: 60,
            voice: 0,
            base_pitch: 0x1000,
            bend_range: (0, 0),
            base_vol: base,
        });

        // Pan hard left: the right side is silenced, left untouched, channel
        // state updated.
        seq.fire_channel(
            &mut spu,
            0,
            ChannelMessage::ControlChange {
                control: 0x0A,
                value: 0,
            },
        );
        assert_eq!(seq.channels[0].pan, 0);
        assert_eq!(spu.voices[0].vol_left, base.0);
        assert_eq!(spu.voices[0].vol_right, 0);

        // Returning to center restores the full base (re-pans the base, not
        // the already-panned value).
        seq.fire_channel(
            &mut spu,
            0,
            ChannelMessage::ControlChange {
                control: 0x0A,
                value: PAN_CENTER,
            },
        );
        assert_eq!(
            (spu.voices[0].vol_left, spu.voices[0].vol_right),
            (base.0, base.1)
        );
    }
}

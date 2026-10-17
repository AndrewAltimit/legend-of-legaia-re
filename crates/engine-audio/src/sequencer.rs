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

use crate::spu::voice::SPU_INTERNAL_RATE;
use crate::vab_bind::VabBank;
use legaia_seq::Seq;

mod channel_math;
mod clock;
mod events;
mod transport;
mod voices;

use channel_math::*;

/// Number of MIDI channels (the SsAPI dispatcher iterates 0..=15).
pub const CHANNELS: usize = 16;

/// libsnd default initial tempo when the SEQ header is broken: 120 BPM.
pub const DEFAULT_TEMPO_US_PER_QN: u32 = 500_000;

/// [`crate::seq_calc::retail_effective_us_per_qn`], falling back to the
/// exact tempo where retail's slow mode would apply.
fn retail_play_tempo(us_per_qn: u32, ppqn: u16) -> u32 {
    crate::seq_calc::retail_effective_us_per_qn(us_per_qn, ppqn)
        .unwrap_or(us_per_qn)
        .max(1)
}

/// [`retail_play_tempo`] for the header tempo, which the SEQ open rounds to
/// the nearest BPM where the tempo meta handler truncates
/// ([`crate::seq_calc::header_bpm`]).
fn retail_header_play_tempo(us_per_qn: u32, ppqn: u16) -> u32 {
    crate::seq_calc::retail_effective_header_us_per_qn(us_per_qn, ppqn)
        .unwrap_or(us_per_qn)
        .max(1)
}

/// NRPN-style controller that carries SEQ loop markers (controller 99).
const CC_LOOP_MARKER: u8 = 0x63;
/// `CC_LOOP_MARKER` value marking a Loop Start point.
const LOOP_START_VALUE: u8 = 20;
/// `CC_LOOP_MARKER` value marking Loop Forever (jump to the last Loop Start).
const LOOP_FOREVER_VALUE: u8 = 30;

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
            pan: PAN_CENTER,
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
    /// The note's raw velocity, program and layer tone: a CC7 or CC10
    /// re-derives the voice volume from these through the whole retail chain
    /// ([`Sequencer::note_volume`]), as libsnd's re-key does, rather than
    /// rescaling an already-rounded value.
    velocity: u8,
    program: usize,
    tone: usize,
    /// The voice's [`crate::spu::voice::Voice::key_on_count`] right after
    /// this note keyed it. A different count means an SFX cue re-keyed the
    /// voice under the note; the note then no longer owns it. For a note
    /// still [`Self::pending`], the count before its key-on.
    key_on_stamp: u32,
    /// `Some(id)` while the note's key-on waits for the next flush
    /// ([`Sequencer::flush_key_ons`]); `None` once it has keyed the voice.
    pending: Option<u32>,
}

/// One layer of a note-on staged for the next flush: the voice is chosen and
/// reserved, the key-on itself is not yet written.
#[derive(Debug, Clone, Copy)]
struct PendingKey {
    id: u32,
    voice: u8,
    channel: u8,
    key: u8,
    velocity: u8,
    program: usize,
    tone: usize,
}

/// The sequence volume the game gives every BGM sequence: `FUN_8002614C`
/// passes its argument to `FUN_80064890` (`SsSeqSetVol`, both sides) for each
/// sound-source record, and its callers pass `(DAT_8007B6EC << 15) >> 16` -
/// `215` in 90 of the 98 catalogued states, so `107`. The mednafen library's
/// field and battle states read `107` in the playing sequence's `+0x58` /
/// `+0x5A`.
pub const RETAIL_BGM_SEQ_VOL: u8 = 107;

/// SPU samples between two retail key-on flushes: libsnd's `SsSeqCalc` runs
/// once per vsync, processes every event due, then flushes the staged
/// key-off and key-on masks (`FUN_80065BAC`).
pub const FLUSH_SAMPLES: u64 = (SPU_INTERNAL_RATE / 60) as u64;

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
    /// The quarter-note length events are actually spaced at: the retail
    /// driver's integer per-vsync tick budget
    /// ([`crate::seq_calc::retail_effective_us_per_qn`]) unless
    /// [`Self::set_exact_tempo`] asked for the score's exact tempo.
    play_us_per_qn: u32,
    /// Play the score's exact tempo instead of retail's quantised one.
    exact_tempo: bool,
    /// The tempo is still the header's (no tempo meta has fired), which the
    /// SEQ open rounds to the nearest BPM rather than truncating.
    header_tempo: bool,
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
    /// [`Self::set_master_vol`] changed the volume since the last flush: the
    /// flush re-volumes every sounding note ([`Self::remix_all`]).
    master_vol_dirty: bool,
    /// Monotonic count of loop rewinds. Bumped on every [`Self::rewind_to`],
    /// so an outside observer can detect a loop-back even when the playhead
    /// tick peaks and resets within a single sample (a marker-less EOT whose
    /// last event is zero-delta). Read via [`Self::loop_count`].
    loop_count: u64,
    /// Per-voice allocation **priority** - the VAB tone `prior` byte the note
    /// occupying the voice was keyed on with. Retail keeps this in the
    /// driver's per-voice record block (`0x801CDB50`, stride `0x36`, field
    /// `+0x1A`); the allocation scan may only steal a voice whose priority is
    /// `<=` the incoming note's. Written on every successful allocation.
    voice_prio: [u8; crate::spu::NUM_VOICES],
    /// Per-voice allocation **age** counter (retail per-voice record `+0x02`):
    /// every successful allocation increments every voice's age by one and
    /// zeroes the winner's, so the largest age marks the least-recently
    /// allocated voice. Used as the final steal tie-break. u16 with wrapping
    /// add, matching the retail halfword.
    voice_age: [u16; crate::spu::NUM_VOICES],
    /// Samples advanced since the last key-on flush, `0..FLUSH_SAMPLES`.
    flush_pos: u64,
    /// Note layers allocated since the last flush, keyed at the next one.
    pending: Vec<PendingKey>,
    /// Id source for [`PendingKey::id`].
    next_pending_id: u32,
    /// The retail pump's `+0x90`: tenths of a tick still to wait before
    /// `events[next]` fires. `None` until the pump first runs, or after an
    /// outside [`Self::rewind_to`] - then it is seeded from that event's own
    /// delta, as the stream read after a seek does.
    pump_wait: Option<i64>,
}

#[cfg(test)]
mod tests;

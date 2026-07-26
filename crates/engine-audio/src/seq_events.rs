//! The SsAPI **transport + stream tier** - the half of `SsSeqCalc`'s fan-out
//! that moves a channel's stream cursor, sitting directly on top of
//! [`crate::seq_calc`]'s per-frame dispatch.
//!
//! [`crate::seq_calc`] ports the frame loop and the three *envelope* kernels
//! (the two volume slides and the tempo slide) plus the track-end handler. What
//! it stops short of is everything that reads a byte: the start / stop arms of
//! its own dispatch table, the delta-time pump those arms gate, and the event
//! decoder the pump drives. Those are here, so the pair covers `SsSeqCalc`
//! entirely.
//!
//! Every kernel below is a pure function over a [`SeqChannel`] plus the SEQ
//! body it is playing, so a divergence between retail and
//! [`crate::sequencer`] localises to one kernel.
//!
//! Sources: `ghidra/scripts/funcs/{800638d8,8006418c,80063974,800639a0,
//! 80063cec,80064090}.txt` (disassembly).
//!
//! # The delta-time scale is tenths of a tick, on both sides
//!
//! `FUN_80061C68`, the varint reader, multiplies its decoded value by **10**
//! before returning it and before accumulating it into `+0x88`. That is the
//! same `* 10` that appears in [`crate::seq_calc::tick_budget`], so the pump's
//! `pending_wait >= tick_budget` comparison is tenths against tenths. The unit
//! reading was an inference from one formula; the two agreeing across
//! independent routines is what makes it a measurement.
//!
//! # The decoder is only half of a stream walk
//!
//! `FUN_80063CEC` does **not** consume a whole event. Of the five status
//! classes it recognises, only `0x9n` reads its operands *and* the trailing
//! delta-time; `0xBn` / `0xCn` read one operand byte, `0xEn` skips one without
//! reading it, and a meta event reads only its kind byte. The rest of each
//! event is consumed by the **installed handler** the decoder tail-calls.
//!
//! That is a correction of an earlier reading, and running the decoder alone
//! over a real SEQ body is what falsified it: every program change came back
//! paired with a phantom running-status program change whose operand was `0`,
//! because the trailing delta byte was being re-decoded as the next status.
//! The five handlers all live at `0x80060A1C..0x80061BF8` and are installed as
//! a 17-entry vector at `0x801CD220` by `FUN_80026234`; four of the five end in
//! `jal 0x80061C68` / `sw v0, 0x90(s0)` - they read the delta themselves.
//!
//! So the format is the conventional `[status][operands][delta]`, and a walker
//! needs both halves: [`decode_event`] then [`run_handler_tail`].
//!
//! REF: FUN_80061C68 - `_SsSeqGetVar`, the varint reader. Reproduced as the
//! private `read_delta` below rather than tagged, because the catalogue already
//! files the retail entry as libsnd plumbing.
//! REF: FUN_80026234 - installs the handler vector at `0x801CD220`.
//! REF: FUN_80061B24 (`+0x00`, note) / FUN_80061BF8 (`+0x04`, program change) /
//! FUN_8006166C (`+0x08`, pitch bend) / FUN_80061954 (`+0x0C`, meta) /
//! FUN_8006171C (`+0x10`, control change) - the installed handlers. All five
//! are catalogued as libsnd and replaced by `crate::sequencer`, so only the
//! *stream advance* each performs is modelled here, in [`run_handler_tail`].
//! REF: FUN_800684CC - the voice sweep [`stop_channel`]'s retail caller runs
//! first: it walks the voice pool and releases every voice whose owner
//! halfword matches the `(slot | channel << 8)` handle. The engine's voice pool
//! owns that decision, so the port models the record edit only.
//! REF: FUN_80063AA8 - the track-end handler, ported as
//! [`crate::seq_calc::track_end`]. Both of this module's calls into it pass a
//! third argument (`byte[loop_marker + 1]`, and the literal `0x2F`) that the
//! callee overwrites in its own prologue before any read - the port drops it,
//! as the two-argument reading is what the disassembly supports.
//! REF: FUN_8006206C - the slide-arming routine, which declines to arm while
//! [`flag::BUSY`] or [`flag::REWIND`] is set.
//!
//! # Port deviations, both bounds
//!
//! Retail dereferences the cursor unchecked and its pump can spin forever on a
//! stream that never supplies a wait. Both are reproduced as *outcomes* rather
//! than as behaviour: an out-of-range read yields [`SeqEvent::Overrun`] and a
//! run longer than [`MAX_EVENTS_PER_FRAME`] yields
//! [`PumpOutcome::Runaway`]. Neither can occur on a well-formed stream, so a
//! trace that reports one is reporting a defect in its input.

use crate::seq_calc::{SeqChannel, TrackEnd, flag, track_end};

/// Largest number of events [`pump_delta_time`] will decode in one frame before
/// declaring the stream runaway. Retail has no such bound.
pub const MAX_EVENTS_PER_FRAME: usize = 4096;

/// The stop arm of `SsSeqCalc`'s dispatch table.
///
/// Clears the playing byte and its own [`flag::STOP`] bit. Retail runs the
/// voice sweep `FUN_800684CC` over the channel's handle *first*, then edits the
/// record; the sweep is the engine voice pool's own business, so only the
/// record edit is modelled.
// PORT: FUN_800638d8
pub fn stop_channel(ch: &mut SeqChannel) {
    ch.playing = 0;
    ch.flags &= !flag::STOP;
}

/// The start arm of `SsSeqCalc`'s dispatch table.
///
/// Sets the playing byte and clears its own [`flag::START`] bit. It does *not*
/// touch the cursor - a start resumes where the channel already stood, and it
/// is [`restart_channel`] that rewinds.
// PORT: FUN_8006418c
pub fn start_channel(ch: &mut SeqChannel) {
    ch.playing = 1;
    ch.flags &= !flag::START;
}

/// The channel restart the track-end handler issues for a chained successor
/// ([`TrackEnd::Finished`]`::chain`).
///
/// Arms the successor for exactly one pass: repeat target `1`, repeat count
/// `0`, every transport bit cleared, the cursor rewound to `+0x04` - **the
/// track start, never the alternate loop point**, unlike the track-end handler
/// - and finally [`flag::PLAY`] set.
///
/// The five clears are five separate load / mask / store trios in retail, one
/// per bit, which is why the surviving bits are exactly the ones not listed.
// PORT: FUN_80064090
// NOT WIRED: reached in retail only from the track-end handler's chain arm, and
// the chain names a *different* `(slot, channel)`. `crate::seq_calc::track_end`
// is ported and reports it as data (`TrackEnd::Finished { chain }`), but no
// host owns a second record to apply this to: `note-trace --seq-calc` seeds one
// channel because a `music_01` entry carries one SEQ, and `crate::Sequencer`
// plays one SEQ per BGM slot with no retail slot table behind it. What has to
// exist first is a multi-slot channel table, not a call site - and until a
// chained retail SEQ is found on the disc there is nothing to point one at.
pub fn restart_channel(ch: &mut SeqChannel) {
    ch.repeat_target = 1;
    ch.repeat_count = 0;
    ch.flags &= !(flag::BUSY | flag::START | flag::STOP | flag::REWIND | flag::FINISHED);
    ch.playing = 1;
    ch.cursor = ch.start;
    ch.flags |= flag::PLAY;
}

/// One decoded SEQ event, in place of the five installed handler pointers
/// retail dispatches through (`_DAT_801CD220` note, `_DAT_801CD224` program
/// change, `_DAT_801CD228` pitch bend, `_DAT_801CD22C` meta, `_DAT_801CD230`
/// control change).
///
/// Reporting the class rather than calling one of five pointers is what lets
/// the decoder be ported without the vector decoded first. It does **not**
/// mean the handler is irrelevant to the stream - see [`run_handler_tail`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeqEvent {
    /// `0x9n` - note event (a release is a note event with velocity `0`). The
    /// delta-time that follows the operands has already been consumed into
    /// [`SeqChannel::pending_wait`].
    Note {
        /// Key number.
        note: u8,
        /// Velocity.
        velocity: u8,
        /// The delta-time just consumed, already scaled by the reader's `* 10`.
        delta: u32,
    },
    /// `0xBn` - control change. Retail reads **one** operand byte.
    ControlChange(u8),
    /// `0xCn` - program change.
    ProgramChange(u8),
    /// `0xEn` - pitch bend. Retail advances one operand byte and never reads
    /// it, so there is no value to report.
    PitchBend,
    /// `0xFF nn` with `nn != 0x2F` - a meta event, reported by kind byte.
    Meta(u8),
    /// `0xFF 0x2F` - end of track. Carries what
    /// [`crate::seq_calc::track_end`] decided.
    EndOfTrack(TrackEnd),
    /// The loop-marker guard fired: the cursor stood on `+0x10` with both
    /// [`flag::PLAY`] and [`flag::LOOP_ALT`] set, so the status byte was never
    /// classified and the track-end handler ran instead.
    LoopMarker(TrackEnd),
    /// A status byte matching no arm. Retail consumes it and returns, which is
    /// how a stream desynchronises silently.
    Unknown(u8),
    /// The cursor left the stream. A port bound; retail reads on.
    Overrun,
}

impl SeqEvent {
    /// Whether this event ends the pump's inner loop by itself, regardless of
    /// what it left in [`SeqChannel::pending_wait`].
    pub fn halts_pump(self) -> bool {
        matches!(self, Self::Overrun)
    }
}

/// `FUN_80061C68` - the MIDI varint delta-time reader, with retail's `* 10`
/// scale and its `+0x88` accumulation.
///
/// A leading `0x00` byte is the one case that returns without accumulating:
/// retail branches straight to the epilogue with `v0 = 0`, leaving `+0x88`
/// untouched.
///
/// Public because a SEQ body opens with a delta-time that no *event* owns: the
/// SEQ open (`FUN_80062410` at `80062620`) reads it before the first
/// `SsSeqCalc` frame, and a host seeding a channel by hand has to do the same
/// or the pump decodes that byte as a status.
pub fn read_delta(ch: &mut SeqChannel, stream: &[u8]) -> Option<u32> {
    let first = *stream.get(ch.cursor as usize)?;
    ch.cursor = ch.cursor.wrapping_add(1);
    if first == 0 {
        return Some(0);
    }
    let mut value = u32::from(first);
    if first & 0x80 != 0 {
        value = u32::from(first & 0x7F);
        loop {
            let b = *stream.get(ch.cursor as usize)?;
            ch.cursor = ch.cursor.wrapping_add(1);
            value = (value << 7).wrapping_add(u32::from(b & 0x7F));
            if b & 0x80 == 0 {
                break;
            }
        }
    }
    let scaled = value.wrapping_mul(10);
    ch.tick_accum = ch.tick_accum.wrapping_add(scaled);
    Some(scaled)
}

/// Decode one event off the channel's stream, advancing the cursor.
///
/// The dispatch is MIDI-shaped with running status: a byte with bit `0x80` set
/// latches [`SeqChannel::running_status`] (the high nibble) and
/// [`SeqChannel::midi_channel`] (the low nibble) before it is classified; a
/// byte without it is an operand for whatever the running status already is.
/// The `0xFn` class latches `0xFF` rather than `0xF0`, so a running meta event
/// re-enters through the `0xFF` arm.
///
/// The loop-marker guard runs before any classification: with both
/// [`flag::PLAY`] and [`flag::LOOP_ALT`] set, a cursor standing exactly on
/// [`SeqChannel::loop_marker`] ends the track instead of reading an event.
///
/// Retail's return value (`-1` marker, `1` end-of-track, `0` otherwise) is
/// **discarded by its only caller** - the pump re-reads `+0x90` instead - so
/// the port reports the event rather than the code.
// PORT: FUN_80063cec
pub fn decode_event(ch: &mut SeqChannel, stream: &[u8]) -> SeqEvent {
    let at = ch.cursor;
    let Some(&status) = stream.get(at as usize) else {
        return SeqEvent::Overrun;
    };
    ch.cursor = at.wrapping_add(1);

    if ch.flags & (flag::PLAY | flag::LOOP_ALT) == (flag::PLAY | flag::LOOP_ALT)
        && at == ch.loop_marker
    {
        return SeqEvent::LoopMarker(track_end(ch));
    }

    let fresh = status & 0x80 != 0;
    let kind = if fresh {
        // The channel nibble is latched before the switch, so an unrecognised
        // status still overwrites it. The *running status* is latched inside
        // each arm instead, so an unrecognised one leaves the previous class
        // standing - and the next operand byte is decoded as that class.
        ch.midi_channel = status & 0x0F;
        let high = status & 0xF0;
        // `0xFn` latches `0xFF`, not `0xF0`.
        if high == 0xF0 { 0xFF } else { high }
    } else {
        ch.running_status
    };

    match kind {
        0x90 => {
            if fresh {
                ch.running_status = 0x90;
            }
            let (note, velocity) = if fresh {
                let Some(n) = read_byte(ch, stream) else {
                    return SeqEvent::Overrun;
                };
                let Some(v) = read_byte(ch, stream) else {
                    return SeqEvent::Overrun;
                };
                (n, v)
            } else {
                let Some(v) = read_byte(ch, stream) else {
                    return SeqEvent::Overrun;
                };
                (status, v)
            };
            let Some(delta) = read_delta(ch, stream) else {
                return SeqEvent::Overrun;
            };
            ch.pending_wait = delta;
            SeqEvent::Note {
                note,
                velocity,
                delta,
            }
        }
        0xB0 => {
            if fresh {
                ch.running_status = 0xB0;
            }
            match operand(ch, stream, fresh, status) {
                Some(v) => SeqEvent::ControlChange(v),
                None => SeqEvent::Overrun,
            }
        }
        0xC0 => {
            if fresh {
                ch.running_status = 0xC0;
            }
            match operand(ch, stream, fresh, status) {
                Some(v) => SeqEvent::ProgramChange(v),
                None => SeqEvent::Overrun,
            }
        }
        0xE0 => {
            // The one arm that advances without reading: retail does
            // `*piVar7 = *piVar7 + 1` on the fresh path and nothing at all on
            // the running path, where the operand was already consumed as the
            // status byte.
            if fresh {
                ch.running_status = 0xE0;
                ch.cursor = ch.cursor.wrapping_add(1);
            }
            SeqEvent::PitchBend
        }
        0xFF => {
            if fresh {
                ch.running_status = 0xFF;
            }
            let Some(meta) = operand(ch, stream, fresh, status) else {
                return SeqEvent::Overrun;
            };
            if meta == 0x2F {
                SeqEvent::EndOfTrack(track_end(ch))
            } else {
                SeqEvent::Meta(meta)
            }
        }
        _ => SeqEvent::Unknown(status),
    }
}

/// How many further stream bytes the *installed handler* for an event reads
/// before it reads that event's trailing delta-time.
///
/// From each handler's own disassembly:
///
/// | class | handler | operand bytes it reads | reads the delta |
/// |---|---|---|---|
/// | `0x9n` | `FUN_80061B24` | 0 | no - the decoder already did |
/// | `0xBn` | `FUN_8006171C` | 1 (the controller **value**) | yes |
/// | `0xCn` | `FUN_80061BF8` | 0 | yes |
/// | `0xEn` | `FUN_8006166C` | 1 | yes |
/// | `0xFF` | `FUN_80061954` | 3 (a big-endian tempo, `60000000 / v`) | yes |
///
/// `None` is "no handler runs": end-of-track, the loop marker, an unrecognised
/// status, and an overrun all return from the decoder without a dispatch.
pub fn handler_operand_bytes(event: SeqEvent) -> Option<usize> {
    match event {
        SeqEvent::Note { .. } => Some(0),
        SeqEvent::ControlChange(_) => Some(1),
        SeqEvent::ProgramChange(_) => Some(0),
        SeqEvent::PitchBend => Some(1),
        SeqEvent::Meta(_) => Some(3),
        SeqEvent::EndOfTrack(_)
        | SeqEvent::LoopMarker(_)
        | SeqEvent::Unknown(_)
        | SeqEvent::Overrun => None,
    }
}

/// Advance the cursor past everything the installed handler consumes: its own
/// operands ([`handler_operand_bytes`]) and then the trailing delta-time, which
/// it stores into [`SeqChannel::pending_wait`] exactly as the decoder's note arm
/// does.
///
/// Returns the delta the handler read, `Some(0)` when the class has a handler
/// that does not read one (the note arm), and `None` when no handler runs or
/// the stream ran out. Calling this after every [`decode_event`] is what makes
/// the pair a complete stream walk.
///
/// This is deliberately **not** a port of the five handlers - all of them are
/// catalogued as libsnd and replaced by [`crate::sequencer`]. It models only
/// the stream advance, which is the part the decoder's own correctness depends
/// on.
pub fn run_handler_tail(ch: &mut SeqChannel, stream: &[u8], event: SeqEvent) -> Option<u32> {
    let operands = handler_operand_bytes(event)?;
    for _ in 0..operands {
        read_byte(ch, stream)?;
    }
    if matches!(event, SeqEvent::Note { .. }) {
        // `FUN_80061B24` is the one handler with no `jal 0x80061C68`.
        return Some(0);
    }
    let delta = read_delta(ch, stream)?;
    ch.pending_wait = delta;
    Some(delta)
}

fn read_byte(ch: &mut SeqChannel, stream: &[u8]) -> Option<u8> {
    let b = *stream.get(ch.cursor as usize)?;
    ch.cursor = ch.cursor.wrapping_add(1);
    Some(b)
}

/// The single-operand arms: a fresh status reads the operand, a running status
/// already has it in the status byte itself.
fn operand(ch: &mut SeqChannel, stream: &[u8], fresh: bool, status: u8) -> Option<u8> {
    if fresh {
        read_byte(ch, stream)
    } else {
        Some(status)
    }
}

/// What one pump call did with the frame's tick budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PumpOutcome {
    /// The wait is longer than a frame and the sub-frame divider absorbed this
    /// frame: nothing was spent and no event decoded.
    Divided,
    /// The wait is longer than a frame and the budget was subtracted from it.
    /// No event decoded.
    Waited,
    /// The wait completed inside this frame; the events decoded while spending
    /// the budget, in order.
    Ran(Vec<SeqEvent>),
    /// The pump returned without touching anything, which retail does when the
    /// budget is spent but `pending_wait` still exceeds it.
    Idle,
    /// The inner loop ran past [`MAX_EVENTS_PER_FRAME`] without a wait. A port
    /// bound; retail spins.
    Runaway(Vec<SeqEvent>),
}

/// The delta-time pump - `SsSeqCalc`'s [`crate::seq_calc::SeqCall::Pump`] arm,
/// and the gate every volume / tempo slide sits nested inside.
///
/// Three shapes, selected by `pending_wait - tick_budget`:
///
/// * **positive**, so the wait outlives the frame. The `+0x52` sub-frame
///   divider decides how: above zero it is decremented and *nothing else
///   happens* (the frame is swallowed whole); at zero it is reloaded from the
///   budget and the wait drops by exactly `1`; below zero the whole budget is
///   subtracted. So a negative divider is the ordinary "spend the budget" mode
///   and a non-negative one is a slow mode that stretches one tick over
///   `budget + 1` frames.
/// * **non-positive**, so the wait completes this frame: decode events until
///   the accumulated waits reach the budget, then carry the overshoot back into
///   `pending_wait`. The inner loop re-reads the budget every iteration, so an
///   event that retunes the tempo takes effect within the same frame.
/// * the guarded `budget < pending_wait` early return, unreachable on the
///   arithmetic above but faithfully preserved.
///
/// `FUN_80063974` is the entry `SsSeqCalc` actually calls: eleven instructions
/// that sign-extend both `i16` arguments and tail-call `FUN_800639A0`. The
/// widening has no meaning in a typed port, so both addresses tag this one
/// function.
// PORT: FUN_80063974
// PORT: FUN_800639a0
pub fn pump_delta_time(ch: &mut SeqChannel, stream: &[u8]) -> PumpOutcome {
    let budget = ch.tick_budget;
    let wait = ch.pending_wait;
    let short = (wait as i32).wrapping_sub(i32::from(budget));

    if short > 0 {
        let divider = ch.sub_frame;
        if divider > 0 {
            ch.sub_frame = divider.wrapping_sub(1);
            return PumpOutcome::Divided;
        }
        if divider == 0 {
            ch.sub_frame = budget;
            ch.pending_wait = ch.pending_wait.wrapping_sub(1);
            return PumpOutcome::Divided;
        }
        ch.pending_wait = short as u32;
        return PumpOutcome::Waited;
    }

    if i32::from(budget) < wait as i32 {
        return PumpOutcome::Idle;
    }

    let mut events = Vec::new();
    let mut accum = wait as i32;
    loop {
        if events.len() >= MAX_EVENTS_PER_FRAME {
            return PumpOutcome::Runaway(events);
        }
        let ev = decode_event(ch, stream);
        events.push(ev);
        if ev.halts_pump() {
            return PumpOutcome::Ran(events);
        }
        // Retail's decoder tail-calls the installed handler, so the pump
        // observes a fully-consumed event. Modelling the decoder alone here
        // would leave each event's trailing delta in the stream to be
        // re-decoded as the next status byte.
        if handler_operand_bytes(ev).is_some() && run_handler_tail(ch, stream, ev).is_none() {
            events.push(SeqEvent::Overrun);
            return PumpOutcome::Ran(events);
        }
        // Retail re-reads `+0x90` here and loops on zero without adding.
        if ch.pending_wait == 0 {
            continue;
        }
        accum = accum.wrapping_add(ch.pending_wait as i32);
        if accum < i32::from(ch.tick_budget) {
            continue;
        }
        ch.pending_wait = accum.wrapping_sub(i32::from(ch.tick_budget)) as u32;
        return PumpOutcome::Ran(events);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch() -> SeqChannel {
        SeqChannel {
            flags: flag::PLAY,
            tick_budget: 100,
            ..Default::default()
        }
    }

    // ---- transport arms -------------------------------------------------

    #[test]
    fn stop_clears_the_playing_byte_and_only_its_own_bit() {
        let mut c = ch();
        c.playing = 1;
        c.flags = flag::PLAY | flag::STOP | flag::VOL_UP;
        stop_channel(&mut c);
        assert_eq!(c.playing, 0);
        assert_eq!(c.flags, flag::PLAY | flag::VOL_UP);
    }

    #[test]
    fn start_sets_the_playing_byte_and_leaves_the_cursor_alone() {
        let mut c = ch();
        c.cursor = 0x40;
        c.start = 0;
        c.flags = flag::START;
        start_channel(&mut c);
        assert_eq!(c.playing, 1);
        assert_eq!(c.flags, 0);
        assert_eq!(c.cursor, 0x40, "a start resumes; only a restart rewinds");
    }

    #[test]
    fn restart_rewinds_to_the_track_start_not_the_alt_loop_point() {
        let mut c = ch();
        c.start = 0x10;
        c.alt_start = 0x80;
        c.cursor = 0x200;
        c.flags = flag::LOOP_ALT | flag::FINISHED | flag::REWIND;
        restart_channel(&mut c);
        assert_eq!(c.cursor, 0x10);
        // `LOOP_ALT` is not in the clear list, so it survives - and the restart
        // still ignores it.
        assert_eq!(c.flags, flag::LOOP_ALT | flag::PLAY);
    }

    #[test]
    fn restart_arms_exactly_one_pass() {
        let mut c = ch();
        c.repeat_target = 7;
        c.repeat_count = 3;
        restart_channel(&mut c);
        assert_eq!(c.repeat_target, 1);
        assert_eq!(c.repeat_count, 0);
        assert_eq!(c.playing, 1);
    }

    #[test]
    fn restart_clears_the_busy_bit_the_slide_armer_refuses_to_run_over() {
        let mut c = ch();
        c.flags = flag::BUSY;
        restart_channel(&mut c);
        assert_eq!(c.flags & flag::BUSY, 0);
    }

    // ---- the varint reader ----------------------------------------------

    #[test]
    fn a_leading_zero_delta_does_not_touch_the_accumulator() {
        let mut c = ch();
        c.tick_accum = 500;
        assert_eq!(read_delta(&mut c, &[0x00]), Some(0));
        assert_eq!(c.tick_accum, 500);
        assert_eq!(c.cursor, 1);
    }

    #[test]
    fn delta_times_are_scaled_by_ten_like_the_tick_budget() {
        let mut c = ch();
        assert_eq!(read_delta(&mut c, &[0x30]), Some(480));
        assert_eq!(c.tick_accum, 480);
    }

    #[test]
    fn multi_byte_varints_use_the_seven_bit_midi_encoding() {
        let mut c = ch();
        // 0x81 0x00 = 128.
        assert_eq!(read_delta(&mut c, &[0x81, 0x00]), Some(1280));
        assert_eq!(c.cursor, 2);
    }

    // ---- the event decoder ----------------------------------------------

    #[test]
    fn a_note_event_reads_two_operands_then_its_delta() {
        let mut c = ch();
        let ev = decode_event(&mut c, &[0x92, 0x40, 0x60, 0x18]);
        assert_eq!(
            ev,
            SeqEvent::Note {
                note: 0x40,
                velocity: 0x60,
                delta: 240
            }
        );
        assert_eq!(c.midi_channel, 2);
        assert_eq!(c.running_status, 0x90);
        assert_eq!(c.pending_wait, 240);
        assert_eq!(c.cursor, 4);
    }

    #[test]
    fn running_status_reuses_the_latched_class_and_the_byte_as_note() {
        let mut c = ch();
        decode_event(&mut c, &[0x92, 0x40, 0x60, 0x00, 0x43, 0x70, 0x00]);
        let ev = decode_event(&mut c, &[0x92, 0x40, 0x60, 0x00, 0x43, 0x70, 0x00]);
        assert_eq!(
            ev,
            SeqEvent::Note {
                note: 0x43,
                velocity: 0x70,
                delta: 0
            }
        );
    }

    #[test]
    fn control_change_reads_only_the_controller_number() {
        let mut c = ch();
        let ev = decode_event(&mut c, &[0xB3, 0x07, 0x55, 0x06]);
        assert_eq!(ev, SeqEvent::ControlChange(0x07));
        assert_eq!(c.cursor, 2, "the value byte belongs to the handler");
        assert_eq!(c.pending_wait, 0);
    }

    // ---- the handler tail ------------------------------------------------

    #[test]
    fn the_handler_tail_finishes_a_control_change() {
        let stream = [0xB3u8, 0x07, 0x55, 0x06, 0x90];
        let mut c = ch();
        let ev = decode_event(&mut c, &stream);
        assert_eq!(run_handler_tail(&mut c, &stream, ev), Some(60));
        assert_eq!(c.cursor, 4, "controller, value, delta");
        assert_eq!(c.pending_wait, 60);
    }

    #[test]
    fn a_program_change_handler_reads_only_the_delta() {
        let stream = [0xC0u8, 0x03, 0x00, 0xC1];
        let mut c = ch();
        let ev = decode_event(&mut c, &stream);
        assert_eq!(ev, SeqEvent::ProgramChange(0x03));
        assert_eq!(run_handler_tail(&mut c, &stream, ev), Some(0));
        assert_eq!(
            c.cursor, 3,
            "status, program, delta - the next byte is the next status"
        );
    }

    #[test]
    fn a_meta_handler_eats_three_tempo_bytes_and_the_delta() {
        let stream = [0xFFu8, 0x51, 0x07, 0xA1, 0x20, 0x00];
        let mut c = ch();
        let ev = decode_event(&mut c, &stream);
        assert_eq!(ev, SeqEvent::Meta(0x51));
        assert_eq!(run_handler_tail(&mut c, &stream, ev), Some(0));
        assert_eq!(c.cursor, 6);
    }

    #[test]
    fn a_note_handler_reads_nothing_because_the_decoder_read_the_delta() {
        let stream = [0x90u8, 0x40, 0x60, 0x06];
        let mut c = ch();
        let ev = decode_event(&mut c, &stream);
        assert_eq!(c.cursor, 4);
        assert_eq!(run_handler_tail(&mut c, &stream, ev), Some(0));
        assert_eq!(c.cursor, 4, "no further advance");
    }

    #[test]
    fn end_of_track_dispatches_no_handler() {
        let stream = [0xFFu8, 0x2F];
        let mut c = ch();
        let ev = decode_event(&mut c, &stream);
        assert_eq!(handler_operand_bytes(ev), None);
        assert_eq!(run_handler_tail(&mut c, &stream, ev), None);
    }

    #[test]
    fn program_change_reads_one_operand() {
        let mut c = ch();
        assert_eq!(
            decode_event(&mut c, &[0xC1, 0x22]),
            SeqEvent::ProgramChange(0x22)
        );
        assert_eq!(c.cursor, 2);
    }

    #[test]
    fn pitch_bend_consumes_one_byte_it_never_reads() {
        let mut c = ch();
        assert_eq!(decode_event(&mut c, &[0xE0, 0x7F]), SeqEvent::PitchBend);
        assert_eq!(c.cursor, 2);
    }

    #[test]
    fn a_running_pitch_bend_consumes_nothing_further() {
        let mut c = ch();
        decode_event(&mut c, &[0xE0, 0x7F, 0x11]);
        let before = c.cursor;
        assert_eq!(
            decode_event(&mut c, &[0xE0, 0x7F, 0x11]),
            SeqEvent::PitchBend
        );
        assert_eq!(c.cursor, before + 1, "only the status byte itself");
    }

    #[test]
    fn the_f0_class_latches_ff_so_a_running_meta_re_enters_the_meta_arm() {
        let mut c = ch();
        assert_eq!(decode_event(&mut c, &[0xF0, 0x51]), SeqEvent::Meta(0x51));
        assert_eq!(c.running_status, 0xFF);
        let ev = decode_event(&mut c, &[0xF0, 0x51, 0x51]);
        assert_eq!(ev, SeqEvent::Meta(0x51));
    }

    #[test]
    fn meta_2f_runs_the_track_end_handler() {
        let mut c = ch();
        c.start = 0;
        c.cursor = 0;
        let ev = decode_event(&mut c, &[0xFF, 0x2F]);
        assert!(matches!(ev, SeqEvent::EndOfTrack(TrackEnd::LoopForever)));
        assert_eq!(c.cursor, 0, "looped back to the track start");
    }

    #[test]
    fn an_unknown_status_is_consumed_and_reported() {
        let mut c = ch();
        assert_eq!(decode_event(&mut c, &[0xA0, 0x11]), SeqEvent::Unknown(0xA0));
        assert_eq!(c.cursor, 1);
    }

    #[test]
    fn an_unknown_status_still_overwrites_the_channel_nibble() {
        // The `+0x17` store is before the switch and the `+0x16` store is
        // inside each arm, so an unmatched status moves one and not the other.
        let mut c = ch();
        c.running_status = 0xB0;
        c.midi_channel = 1;
        assert_eq!(decode_event(&mut c, &[0xA5]), SeqEvent::Unknown(0xA5));
        assert_eq!(c.midi_channel, 5);
        assert_eq!(c.running_status, 0xB0, "the previous class still stands");
    }

    #[test]
    fn the_loop_marker_guard_needs_both_bits_and_the_exact_cursor() {
        let stream = [0x92u8, 0x40, 0x60, 0x00];
        // Right cursor, only `PLAY`: the guard does not fire.
        let mut c = ch();
        c.loop_marker = 0;
        assert!(matches!(
            decode_event(&mut c, &stream),
            SeqEvent::Note { .. }
        ));
        // Both bits, right cursor: it does.
        let mut c = ch();
        c.flags = flag::PLAY | flag::LOOP_ALT;
        c.loop_marker = 0;
        c.alt_start = 0;
        assert!(matches!(
            decode_event(&mut c, &stream),
            SeqEvent::LoopMarker(_)
        ));
        // Both bits, wrong cursor: it does not.
        let mut c = ch();
        c.flags = flag::PLAY | flag::LOOP_ALT;
        c.loop_marker = 3;
        assert!(matches!(
            decode_event(&mut c, &stream),
            SeqEvent::Note { .. }
        ));
    }

    #[test]
    fn an_empty_stream_overruns_rather_than_panicking() {
        let mut c = ch();
        assert_eq!(decode_event(&mut c, &[]), SeqEvent::Overrun);
    }

    // ---- the pump --------------------------------------------------------

    #[test]
    fn a_negative_divider_spends_the_whole_budget() {
        let mut c = ch();
        c.sub_frame = -1;
        c.pending_wait = 250;
        assert_eq!(pump_delta_time(&mut c, &[]), PumpOutcome::Waited);
        assert_eq!(c.pending_wait, 150);
    }

    #[test]
    fn a_positive_divider_swallows_the_frame_whole() {
        let mut c = ch();
        c.sub_frame = 3;
        c.pending_wait = 250;
        assert_eq!(pump_delta_time(&mut c, &[]), PumpOutcome::Divided);
        assert_eq!(c.pending_wait, 250, "not one tick spent");
        assert_eq!(c.sub_frame, 2);
    }

    #[test]
    fn a_spent_divider_reloads_from_the_budget_and_spends_one_tick() {
        let mut c = ch();
        c.sub_frame = 0;
        c.pending_wait = 250;
        assert_eq!(pump_delta_time(&mut c, &[]), PumpOutcome::Divided);
        assert_eq!(c.pending_wait, 249);
        assert_eq!(c.sub_frame, 100, "reloaded from the tick budget");
    }

    #[test]
    fn a_completed_wait_decodes_until_the_budget_is_reached() {
        // Two notes, each carrying a 60-tick delta (`0x06 * 10`), against a
        // 100-tick budget: the first leaves 60 < 100 so the loop runs again,
        // the second reaches 120 and carries 20 back.
        let stream = [0x90u8, 0x40, 0x60, 0x06, 0x41, 0x60, 0x06];
        let mut c = ch();
        c.pending_wait = 0;
        let out = pump_delta_time(&mut c, &stream);
        let PumpOutcome::Ran(events) = out else {
            panic!("expected Ran, got {out:?}");
        };
        assert_eq!(events.len(), 2);
        assert_eq!(c.pending_wait, 20);
    }

    #[test]
    fn the_inner_loop_re_reads_the_budget_every_iteration() {
        // A stream whose first event retunes the budget mid-frame. Modelled by
        // decoding one note (delta 60) and then shrinking the budget: the
        // accumulator is 60, and against a budget of 50 the loop stops at once.
        let stream = [0x90u8, 0x40, 0x60, 0x06, 0x41, 0x60, 0x06];
        let mut c = ch();
        c.tick_budget = 50;
        c.pending_wait = 0;
        let out = pump_delta_time(&mut c, &stream);
        let PumpOutcome::Ran(events) = out else {
            panic!("expected Ran, got {out:?}");
        };
        assert_eq!(events.len(), 1);
        assert_eq!(c.pending_wait, 10);
    }

    #[test]
    fn zero_delta_events_chain_without_ending_the_frame() {
        // Two program changes with zero deltas, then a note: neither of the
        // first two supplies a wait, so all three are decoded in one pass.
        let stream = [0xC0u8, 0x03, 0x00, 0x04, 0x00, 0x90, 0x40, 0x60, 0x0A];
        let mut c = ch();
        c.pending_wait = 0;
        let out = pump_delta_time(&mut c, &stream);
        let PumpOutcome::Ran(events) = out else {
            panic!("expected Ran, got {out:?}");
        };
        assert_eq!(
            events,
            vec![
                SeqEvent::ProgramChange(0x03),
                SeqEvent::ProgramChange(0x04),
                SeqEvent::Note {
                    note: 0x40,
                    velocity: 0x60,
                    delta: 100
                },
            ]
        );
    }

    #[test]
    fn the_pump_runs_the_handler_tail_so_a_delta_is_not_re_read_as_a_status() {
        // The regression the disc trace found: without the handler tail the
        // `0x00` delta after each program change came back as a second,
        // phantom running-status program change.
        let stream = [0xC0u8, 0x03, 0x00, 0x04, 0x00, 0x90, 0x40, 0x60, 0x0A];
        let mut c = ch();
        c.pending_wait = 0;
        let PumpOutcome::Ran(events) = pump_delta_time(&mut c, &stream) else {
            panic!("expected Ran");
        };
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, SeqEvent::ProgramChange(_)))
                .count(),
            2
        );
    }

    #[test]
    fn a_stream_that_never_waits_is_declared_runaway_not_hung() {
        // All-zero bytes decode as running control changes whose handler reads
        // a zero value and a zero delta, so nothing ever supplies a wait.
        let stream = vec![0u8; MAX_EVENTS_PER_FRAME * 4];
        let mut c = ch();
        c.running_status = 0xB0;
        c.pending_wait = 0;
        assert!(matches!(
            pump_delta_time(&mut c, &stream),
            PumpOutcome::Runaway(_)
        ));
    }

    #[test]
    fn a_stream_that_runs_out_stops_at_the_overrun() {
        let mut c = ch();
        c.pending_wait = 0;
        let out = pump_delta_time(&mut c, &[0xB0, 0x07]);
        let PumpOutcome::Ran(events) = out else {
            panic!("expected Ran, got {out:?}");
        };
        assert_eq!(events.last(), Some(&SeqEvent::Overrun));
    }
}

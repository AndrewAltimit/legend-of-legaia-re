//! The SsAPI **per-frame calc tier** - retail's `SsSeqCalc` dispatch and the
//! three per-channel kernels it fans out to.
//!
//! PORT: FUN_80062f98 - the per-frame sequencer top.
//! PORT: FUN_800649b0 - the tempo slide + tick-step recompute.
//! PORT: FUN_8006320c - the ascending volume slide.
//! PORT: FUN_8006352c - the descending volume slide.
//! PORT: FUN_80063aa8 - the track-end / loop-repeat handler.
//!
//! [`crate::Sequencer`] is the engine's clean-room replacement for this tier
//! and drives *playback* on its own integer-SPU-sample clock, so nothing on the
//! audio output path calls these kernels. Their host is the differential:
//! `note-trace --seq-calc` seeds one channel record off a real `music_01` SEQ
//! and runs this dispatch over it frame by frame, so a divergence between
//! retail and `sequencer.rs` localises to one kernel instead of being argued
//! about. Each is a pure function over a [`SeqChannel`], which is what makes
//! that possible. Wiring the tier into playback would mean `Sequencer` adopting
//! the retail channel record wholesale.
//!
//! REF: FUN_80063974 / FUN_800639a0 - the delta-time pump (the bit-`0x1` arm),
//! the `SeqCall::Pump` this module dispatches. Ported next door as
//! [`crate::seq_events::pump_delta_time`].
//! REF: FUN_80063cec - the SEQ event decoder the pump drives; ported as
//! [`crate::seq_events::decode_event`].
//! REF: FUN_800638d8 / FUN_8006418c - the `SeqCall::Stop` / `SeqCall::Start`
//! arms, ported as [`crate::seq_events::stop_channel`] /
//! [`crate::seq_events::start_channel`].
//! REF: FUN_80064090 - the chained-channel restart, ported as
//! [`crate::seq_events::restart_channel`].
//! REF: FUN_800683d8 - the channel-volume getter the slides read and cache.
//! REF: FUN_80067e9c - `_SsSeqNoteOn`, which commits a slide's new volume pair.
//! REF: FUN_800684cc - the note kill a finished track issues.
//! REF: FUN_800641ec - `SsSeqRewind`, the bit-`0x4` arm.
//! REF: FUN_80065bac - the voice flush `SsSeqCalc` runs once per frame.
//!
//! # Why this is ported rather than ignored
//!
//! The address band is PsyQ libsnd, and the catalogue already ignores the tier
//! *below* this one (`FUN_80066308` and friends) on the grounds that
//! `sequencer.rs` replaces it. That line is drawn at "below the SsAPI surface" -
//! and `SsSeqCalc` **is** the surface. Every kernel here changes what a player
//! hears: the tempo slide is the one place wall-clock tempo becomes an integer
//! tick step, the two volume slides are audible envelopes, and the track-end
//! handler decides whether and where BGM loops. None of it is plumbing the
//! engine can leave unstated.
//!
//! # The channel record
//!
//! One record per `(slot, channel)`, at `_DAT_801CD2C0[slot] + channel * 0xB0`.
//! Slot count is the `i16` at `0x801CDB40`, channel count the `i16` at
//! `0x801CDB42`, and the per-slot enable bitmap is `0x801CD2B8`.
//!
//! # Where this stops
//!
//! At the envelope kernels. Everything in `SsSeqCalc`'s fan-out that reads a
//! *stream byte* - the start / stop arms, the delta-time pump and the event
//! decoder - lives in [`crate::seq_events`], which completes the tier.
//!
//! An earlier reading held that `FUN_80063CEC` could not be ported until the
//! five installed handler pointers it dispatches through were decoded. That is
//! **false**, and the disassembly is what settles it: no arm's cursor
//! arithmetic depends on which routine a pointer holds, so the handler table is
//! a return value, exactly as `run` is a closure here.
//!
//! Sources: `ghidra/scripts/funcs/{80062f98,800649b0,8006320c,8006352c,80063aa8}.txt`
//! (disassembly).

/// Per-channel record stride in the retail slot table.
pub const CHANNEL_STRIDE: usize = 0xB0;

/// `+0x98` flag bits, in the order `SsSeqCalc` tests them.
pub mod flag {
    /// Channel is playing - gates the pump and, nested inside it, every slide.
    pub const PLAY: u32 = 0x1;
    /// Stop requested.
    pub const STOP: u32 = 0x2;
    /// Rewind requested. Its handler additionally **zeroes the whole flag
    /// word**.
    pub const REWIND: u32 = 0x4;
    /// Start requested.
    pub const START: u32 = 0x8;
    /// Ascending volume slide active.
    pub const VOL_UP: u32 = 0x10;
    /// Descending volume slide active.
    pub const VOL_DOWN: u32 = 0x20;
    /// Tempo slide, arm A. Dispatches to the same handler as [`TEMPO_B`].
    pub const TEMPO_A: u32 = 0x40;
    /// Tempo slide, arm B.
    pub const TEMPO_B: u32 = 0x80;
    /// Tested alongside [`REWIND`] by the slide-arming routine
    /// (`FUN_8006206C` at `800620b4`), which declines to arm a volume slide
    /// while either is set, and cleared by
    /// [`crate::seq_events::restart_channel`]. What *sets* it is not pinned.
    pub const BUSY: u32 = 0x100;
    /// Set by the track-end handler on the final repeat.
    pub const FINISHED: u32 = 0x200;
    /// Selects the alternate loop point (`+0x0C`) over the track start
    /// (`+0x04`).
    pub const LOOP_ALT: u32 = 0x400;
}

/// The retail channel record, field-for-field over the offsets the five kernels
/// touch. Offsets are in the field docs so a capture can be diffed against it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SeqChannel {
    /// `+0x00` stream cursor.
    pub cursor: u32,
    /// `+0x04` track start.
    pub start: u32,
    /// `+0x08` loop cursor the track-end handler republishes.
    pub loop_cursor: u32,
    /// `+0x0C` alternate loop point, selected by [`flag::LOOP_ALT`].
    pub alt_start: u32,
    /// `+0x10` loop-marker cursor. With both [`flag::PLAY`] and
    /// [`flag::LOOP_ALT`] set, the event decoder ends the track when the cursor
    /// stands here rather than reading a status byte.
    pub loop_marker: u32,
    /// `+0x14` playing byte.
    pub playing: u8,
    /// `+0x16` latched running status - the event class high nibble, except
    /// that the `0xFn` class latches `0xFF`.
    pub running_status: u8,
    /// `+0x17` low nibble of the last status byte: the MIDI channel.
    pub midi_channel: u8,
    /// `+0x1C` running-status low nibble.
    pub running_lo: u8,
    /// `+0x20` repeat target; `0` means loop forever.
    pub repeat_target: u8,
    /// `+0x21` repeats completed.
    pub repeat_count: u8,
    /// `+0x22` chained slot, or `0xFF` for none.
    pub chain_slot: u8,
    /// `+0x23` chained channel.
    pub chain_channel: u8,
    /// `+0x48` volume-slide span.
    pub slide_span: i16,
    /// `+0x4A` volume-slide level counter.
    pub slide_level: u16,
    /// `+0x4C` volume-slide step; its **sign selects the rate mode**.
    pub slide_step: i16,
    /// `+0x4E` tempo-slide step; same sign convention.
    pub tempo_step: i16,
    /// `+0x50` sequence resolution (ticks per quarter).
    pub resolution: i16,
    /// `+0x52` sub-frame divider the delta-time pump consults when a wait
    /// outlives the frame. Negative is the ordinary "spend the whole budget"
    /// mode; non-negative stretches one tick over `tick_budget + 1` frames.
    pub sub_frame: i16,
    /// `+0x54` per-frame tick budget the delta-time pump spends.
    pub tick_budget: i16,
    /// `+0x58` / `+0x5A` the **live** channel volume pair. `SsSepSetVol` /
    /// `SsSeqSetVol` write it and `FUN_800683D8` reads it back out; the slides
    /// see it only through that getter.
    pub vol: (u16, u16),
    /// `+0x5C` / `+0x5E` cached channel volume, refreshed by every slide tick -
    /// the destination `FUN_800683D8` is handed in the slide epilogue
    /// (`800634f8` passes `s0 + 0x5C` / `s0 + 0x5E`).
    pub vol_cache: (u16, u16),
    /// `+0x88` running tick accumulator.
    pub tick_accum: u32,
    /// `+0x90` pending delta-time wait.
    pub pending_wait: u32,
    /// `+0x94` current tempo.
    pub tempo: u32,
    /// `+0x98` flag word.
    pub flags: u32,
    /// `+0x9C` volume-slide total duration.
    pub slide_total: u32,
    /// `+0xA0` volume-slide remaining ticks.
    pub slide_remaining: i32,
    /// `+0xA8` tempo-slide remaining ticks.
    pub tempo_remaining: i32,
    /// `+0xAC` tempo-slide target.
    pub tempo_target: u32,
}

/// One handler `SsSeqCalc` fans out to, in dispatch order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeqCall {
    /// `FUN_80063974` - the delta-time pump. Gate for every slide below it.
    Pump,
    /// `FUN_8006320C` - ascending volume slide.
    VolUp,
    /// `FUN_8006352C` - descending volume slide.
    VolDown,
    /// `FUN_800649B0` - tempo slide. Reached from **both** tempo bits.
    Tempo,
    /// `FUN_800638D8` - stop.
    Stop,
    /// `FUN_8006418C` - start.
    Start,
    /// `FUN_800641EC` - `SsSeqRewind`; the flag word is zeroed after it.
    Rewind,
}

/// The `(bit, call)` table, in the exact order `SsSeqCalc` tests it. The first
/// four entries are **nested inside** the [`flag::PLAY`] arm, so a stopped
/// channel runs no slide.
pub const NESTED_DISPATCH: [(u32, SeqCall); 4] = [
    (flag::VOL_UP, SeqCall::VolUp),
    (flag::VOL_DOWN, SeqCall::VolDown),
    (flag::TEMPO_A, SeqCall::Tempo),
    (flag::TEMPO_B, SeqCall::Tempo),
];

/// The un-nested tail of the table.
pub const TAIL_DISPATCH: [(u32, SeqCall); 3] = [
    (flag::STOP, SeqCall::Stop),
    (flag::START, SeqCall::Start),
    (flag::REWIND, SeqCall::Rewind),
];

/// Dispatch one channel for one frame.
///
/// `run` executes a handler against the channel. The retail loop **re-loads the
/// flag word from memory before every single test**, so a handler that clears
/// its own bit is observed immediately by the next test - which is why the
/// tempo handler, called on [`flag::TEMPO_A`], can suppress its own
/// [`flag::TEMPO_B`] call in the same frame. This function reproduces that by
/// reading `ch.flags` afresh each time rather than snapshotting it.
///
/// Returns the calls made, in order.
pub fn dispatch_channel(
    ch: &mut SeqChannel,
    mut run: impl FnMut(SeqCall, &mut SeqChannel),
) -> Vec<SeqCall> {
    let mut made = Vec::new();
    if ch.flags & flag::PLAY != 0 {
        made.push(SeqCall::Pump);
        run(SeqCall::Pump, ch);
        for (bit, call) in NESTED_DISPATCH {
            if ch.flags & bit != 0 {
                made.push(call);
                run(call, ch);
            }
        }
    }
    for (bit, call) in TAIL_DISPATCH {
        if ch.flags & bit != 0 {
            made.push(call);
            run(call, ch);
            if call == SeqCall::Rewind {
                // The one arm that wipes the whole word.
                ch.flags = 0;
            }
        }
    }
    made
}

/// The globals `SsSeqCalc` walks.
#[derive(Debug, Default)]
pub struct SeqCalcState {
    /// `0x801CD2B4` - the re-entrancy latch.
    pub busy: bool,
    /// `0x801CD2B8` - per-slot enable bitmap. The retail shift is `sllv`, whose
    /// count is masked to 5 bits, so the bitmap is exactly 32 slots wide.
    pub slot_mask: u32,
    /// `0x801CDB40` - slot count, re-read every slot iteration.
    pub slot_count: i16,
    /// `0x801CDB42` - channel count, re-read every channel iteration.
    pub channel_count: i16,
}

/// `SsSeqCalc` itself: latch, flush the voices once, then walk every enabled
/// `(slot, channel)`.
///
/// Returns `None` when the latch was already held (retail's early return), else
/// the `(slot, channel, call)` triples in order.
pub fn seq_calc(
    state: &mut SeqCalcState,
    channels: &mut [Vec<SeqChannel>],
    mut run: impl FnMut(SeqCall, &mut SeqChannel),
) -> Option<Vec<(i16, i16, SeqCall)>> {
    if state.busy {
        return None;
    }
    state.busy = true;
    let mut trace = Vec::new();
    let mut slot: i16 = 0;
    while slot < state.slot_count {
        let enabled = state.slot_mask & (1u32 << (slot as u32 & 0x1F)) != 0;
        if enabled {
            let mut chan: i16 = 0;
            while chan < state.channel_count {
                if let Some(ch) = channels
                    .get_mut(slot as usize)
                    .and_then(|s| s.get_mut(chan as usize))
                {
                    for call in dispatch_channel(ch, &mut run) {
                        trace.push((slot, chan, call));
                    }
                }
                chan += 1;
            }
        }
        slot += 1;
    }
    state.busy = false;
    Some(trace)
}

/// Result of one tempo-slide tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TempoTick {
    /// `true` when the countdown expired or the slide reached its target, so
    /// both tempo flag bits were cleared.
    pub finished: bool,
    /// `true` when the tick returned early because it was not on a sub-step
    /// boundary - no tempo move, no flag change, and **no tick-step recompute**.
    pub skipped: bool,
}

/// Recompute the per-frame tick budget: `(resolution * tempo * 10) / (divisor *
/// 60)`, floored at `1`.
///
/// The multiply is signed on `resolution` and the divide is **unsigned**
/// (`divu`), so a negative tempo produces a huge quotient rather than a
/// negative one - the `i16` truncation is what the floor then catches.
///
/// `divisor` is the runtime word at `0x801CD2BC` and is deliberately a
/// parameter: the shape `(ticks/quarter * beats/minute * 10) / (divisor * 60)`
/// reads as "tenths of a tick per frame, with `divisor` the frame rate", but
/// that unit reading is an **inference** from the arithmetic, not a runtime
/// observation, so nothing here bakes a `60` in.
pub fn tick_budget(resolution: i16, tempo: u32, divisor: u32) -> i16 {
    let denom = divisor.wrapping_mul(60);
    if denom == 0 {
        // Retail traps (`break 0x1c00`); the engine floors instead.
        return 1;
    }
    let num = (i32::from(resolution) as u32)
        .wrapping_mul(tempo)
        .wrapping_mul(10);
    let q = (num / denom) as i16;
    if q <= 0 { 1 } else { q }
}

/// One tempo-slide tick (`FUN_800649B0`).
pub fn tempo_slide_tick(ch: &mut SeqChannel, divisor: u32) -> TempoTick {
    ch.tempo_remaining = ch.tempo_remaining.wrapping_sub(1);
    if ch.tempo_remaining < 0 {
        ch.flags &= !(flag::TEMPO_A | flag::TEMPO_B);
        return TempoTick {
            finished: true,
            skipped: false,
        };
    }

    let step = ch.tempo_step;
    if step > 0 {
        // Slow mode: one unit every `step` ticks.
        if ch.tempo_remaining % i32::from(step) != 0 {
            return TempoTick {
                finished: false,
                skipped: true,
            };
        }
        if ch.tempo_target < ch.tempo {
            ch.tempo = ch.tempo.wrapping_sub(1);
        } else if ch.tempo < ch.tempo_target {
            ch.tempo = ch.tempo.wrapping_add(1);
        }
    } else {
        // Fast mode: jump by |step| and clamp at the target. `step == 0` lands
        // here and moves nothing, but still recomputes the budget below.
        let s = i32::from(step);
        if ch.tempo_target < ch.tempo {
            let v = (ch.tempo as i32).wrapping_add(s) as u32;
            ch.tempo = if v < ch.tempo_target {
                ch.tempo_target
            } else {
                v
            };
        } else if ch.tempo < ch.tempo_target {
            let v = (ch.tempo as i32).wrapping_sub(s) as u32;
            ch.tempo = if ch.tempo_target < v {
                ch.tempo_target
            } else {
                v
            };
        }
    }

    ch.tick_budget = tick_budget(ch.resolution, ch.tempo, divisor);

    if ch.tempo_remaining != 0 && ch.tempo != ch.tempo_target {
        return TempoTick {
            finished: false,
            skipped: false,
        };
    }
    ch.flags &= !(flag::TEMPO_A | flag::TEMPO_B);
    TempoTick {
        finished: true,
        skipped: false,
    }
}

/// Which way a volume slide runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlideDir {
    /// `FUN_8006320C` - toward `(0x7F, 0x7F)`.
    Up,
    /// `FUN_8006352C` - toward `(0, 0)`.
    Down,
}

impl SlideDir {
    /// The flag bit this direction clears when it settles.
    ///
    /// Named `flag_bit` rather than `flag` on purpose. It was the only in-tree
    /// `fn flag`, and the reachability graph's receiver gate fires only where a
    /// name is *ambiguous* - so a unique name is never gated, and every
    /// `flag(..)` call on a **closure parameter** of that name elsewhere in the
    /// workspace (`FieldNpcAmbient::select_variant` takes one) resolved onto
    /// this method, reporting the whole module live and this file's `NOT WIRED`
    /// disclosures stale. See `docs/tooling/stale-not-wired-triage.md`.
    pub fn flag_bit(self) -> u32 {
        match self {
            Self::Up => flag::VOL_UP,
            Self::Down => flag::VOL_DOWN,
        }
    }
    /// The saturation endpoint.
    pub fn endpoint(self) -> (u16, u16) {
        match self {
            Self::Up => (0x7F, 0x7F),
            Self::Down => (0, 0),
        }
    }
}

/// What one volume-slide tick asks the caller to commit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SlideTick {
    /// The `(l, r)` pair to hand `_SsSeqNoteOn`, if any.
    pub commit: Option<(u16, u16)>,
    /// `true` when the commit is the saturated endpoint and the slide's flag
    /// bit was cleared with it.
    pub saturated: bool,
    /// `true` when the tick's tail cleared the slide flag (remaining hit zero
    /// or the level counter ran out).
    pub settled: bool,
}

/// One volume-slide tick (`FUN_8006320C` / `FUN_8006352C`).
///
/// `vol` is the live `(l, r)` pair read back through `FUN_800683D8`; the tick
/// always re-caches it into `ch.vol_cache` on the way out, which is the
/// epilogue both functions share.
pub fn volume_slide_tick(ch: &mut SeqChannel, dir: SlideDir, vol: (u16, u16)) -> SlideTick {
    let mut out = SlideTick::default();
    let cache = |ch: &mut SeqChannel, v: (u16, u16)| ch.vol_cache = v;

    ch.slide_remaining = ch.slide_remaining.wrapping_sub(1);
    if ch.slide_remaining < 0 {
        ch.flags &= !dir.flag_bit();
        out.settled = true;
        cache(ch, vol);
        return out;
    }

    let step = ch.slide_step;
    let mut committed = vol;

    if step > 0 {
        // Slow mode: +-1 every `step` ticks.
        if ch.slide_remaining % i32::from(step) != 0 {
            cache(ch, vol);
            return out;
        }
        ch.slide_level = ch.slide_level.wrapping_sub(1);
        if (ch.slide_level as i16) < 0 {
            out.commit = Some(dir.endpoint());
            out.saturated = true;
            ch.flags &= !dir.flag_bit();
            committed = dir.endpoint();
        } else if (ch.slide_level as i16) >= 1 {
            // Retail writes this compare as `(v + level) < (v + 1)`; the volume
            // terms cancel, so it is exactly `level < 1`.
            let moved = match dir {
                SlideDir::Up => (vol.0.wrapping_add(1), vol.1.wrapping_add(1)),
                SlideDir::Down => {
                    // The descending side additionally refuses to move a pair
                    // that is already at zero, and settles instead.
                    if vol.0 == 0 || vol.1 == 0 {
                        ch.flags &= !dir.flag_bit();
                        out.settled = true;
                        cache(ch, vol);
                        return out;
                    }
                    (vol.0.wrapping_sub(1), vol.1.wrapping_sub(1))
                }
            };
            out.commit = Some(moved);
            committed = moved;
        }
    } else if step < 0 {
        // Fast mode: |step| per tick, saturating.
        ch.slide_level = ch.slide_level.wrapping_add(step as u16);
        if (ch.slide_level as i16) < 0 {
            out.commit = Some(dir.endpoint());
            out.saturated = true;
            ch.flags &= !dir.flag_bit();
            committed = dir.endpoint();
        } else {
            let mag = i32::from(step).unsigned_abs() as u16;
            let reach = (vol.0.wrapping_add(mag), vol.1.wrapping_add(mag));
            let hit_end = match dir {
                SlideDir::Up => reach.0 >= 0x7F && reach.1 >= 0x7F,
                SlideDir::Down => {
                    (vol.0 as i32 + i32::from(step)) <= 0 && (vol.1 as i32 + i32::from(step)) <= 0
                }
            };
            if hit_end {
                out.commit = Some(dir.endpoint());
                out.saturated = true;
                ch.flags &= !dir.flag_bit();
                committed = dir.endpoint();
            }
            // Both arms then fall through to the incremental commit, gated on
            // the slide's own progress budget.
            let elapsed = ch.slide_total.wrapping_sub(ch.slide_remaining as u32) as i32;
            if elapsed.wrapping_mul(i32::from(mag)) < i32::from(ch.slide_span) {
                let inc = match dir {
                    SlideDir::Up => reach,
                    SlideDir::Down => {
                        if vol.0 == 0 || vol.1 == 0 {
                            (0, 0)
                        } else {
                            (
                                (vol.0 as i32 + i32::from(step)).max(0) as u16,
                                (vol.1 as i32 + i32::from(step)).max(0) as u16,
                            )
                        }
                    }
                };
                out.commit = Some(inc);
                committed = inc;
            }
        }
    } else {
        // step == 0: the `bgez` arm moves nothing at all.
        cache(ch, vol);
        return out;
    }

    // Shared tail: settle when the tick budget or the level counter is spent.
    if ch.slide_remaining == 0 || (ch.slide_level as i16) <= 0 {
        ch.flags &= !dir.flag_bit();
        out.settled = true;
    }
    cache(ch, committed);
    out
}

/// What the track-end handler decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackEnd {
    /// `+0x20 == 0`: loop forever. Cursor rewound, counters cleared.
    LoopForever,
    /// More repeats to go; cursor **and** the loop cursor rewound.
    Repeat {
        /// Repeats completed so far.
        count: u8,
    },
    /// Final repeat. Notes killed, flags rewritten, optionally chaining.
    Finished {
        /// The `(slot, channel)` restarted through `FUN_80064090`, when the
        /// chain byte is not `0xFF`.
        chain: Option<(u8, u8)>,
    },
}

/// The track-end / loop-repeat handler (`FUN_80063AA8`).
///
/// The loop point is `+0x0C` when [`flag::LOOP_ALT`] is set, else `+0x04`.
pub fn track_end(ch: &mut SeqChannel) -> TrackEnd {
    ch.repeat_count = ch.repeat_count.wrapping_add(1);
    let loop_point = if ch.flags & flag::LOOP_ALT != 0 {
        ch.alt_start
    } else {
        ch.start
    };

    if ch.repeat_target == 0 {
        ch.tick_accum = 0;
        ch.running_lo = 0;
        ch.pending_wait = 0;
        ch.cursor = loop_point;
        return TrackEnd::LoopForever;
    }

    if ch.repeat_count < ch.repeat_target {
        ch.tick_accum = 0;
        ch.running_lo = 0;
        ch.pending_wait = 0;
        ch.cursor = loop_point;
        ch.loop_cursor = loop_point;
        return TrackEnd::Repeat {
            count: ch.repeat_count,
        };
    }

    // Final repeat.
    ch.flags &= !(flag::PLAY | flag::START | flag::STOP);
    ch.flags |= flag::FINISHED | flag::REWIND;
    ch.playing = 0;
    ch.loop_cursor = loop_point;

    let chain = if ch.chain_slot != 0xFF {
        Some((ch.chain_slot, ch.chain_channel))
    } else {
        None
    };
    // The caller kills the channel's notes on both arms, then reloads the wait.
    ch.pending_wait = i32::from(ch.tick_budget) as u32;
    TrackEnd::Finished { chain }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(flags: u32) -> SeqChannel {
        SeqChannel {
            flags,
            ..Default::default()
        }
    }

    #[test]
    fn slides_are_nested_inside_the_play_bit() {
        // A stopped channel with every slide bit set runs no slide at all.
        let mut c = ch(flag::VOL_UP | flag::VOL_DOWN | flag::TEMPO_A | flag::TEMPO_B);
        let made = dispatch_channel(&mut c, |_, _| {});
        assert!(made.is_empty());

        let mut c = ch(flag::PLAY | flag::VOL_UP | flag::TEMPO_A);
        let made = dispatch_channel(&mut c, |_, _| {});
        assert_eq!(made, vec![SeqCall::Pump, SeqCall::VolUp, SeqCall::Tempo]);
    }

    #[test]
    fn dispatch_order_is_pump_slides_then_stop_start_rewind() {
        let mut c = ch(0x1FF);
        let made = dispatch_channel(&mut c, |_, _| {});
        assert_eq!(
            made,
            vec![
                SeqCall::Pump,
                SeqCall::VolUp,
                SeqCall::VolDown,
                SeqCall::Tempo,
                SeqCall::Tempo,
                SeqCall::Stop,
                SeqCall::Start,
                SeqCall::Rewind,
            ]
        );
    }

    #[test]
    fn the_flag_word_is_re_read_before_every_test() {
        // A handler that clears a later bit suppresses that later call - the
        // whole point of the re-read. Here the TEMPO_A call clears both tempo
        // bits, exactly as FUN_800649B0 does when its slide settles.
        let mut c = ch(flag::PLAY | flag::TEMPO_A | flag::TEMPO_B);
        let made = dispatch_channel(&mut c, |call, ch| {
            if call == SeqCall::Tempo {
                ch.flags &= !(flag::TEMPO_A | flag::TEMPO_B);
            }
        });
        assert_eq!(made, vec![SeqCall::Pump, SeqCall::Tempo]);

        // When it does not settle, both bits stay set and the handler runs
        // twice in one frame.
        let mut c = ch(flag::PLAY | flag::TEMPO_A | flag::TEMPO_B);
        let made = dispatch_channel(&mut c, |_, _| {});
        assert_eq!(made, vec![SeqCall::Pump, SeqCall::Tempo, SeqCall::Tempo]);
    }

    #[test]
    fn the_rewind_arm_zeroes_the_whole_flag_word() {
        let mut c = ch(flag::PLAY | flag::REWIND | flag::LOOP_ALT | flag::FINISHED);
        dispatch_channel(&mut c, |_, _| {});
        assert_eq!(c.flags, 0);
    }

    #[test]
    fn seq_calc_latch_blocks_re_entry() {
        let mut st = SeqCalcState {
            busy: true,
            slot_mask: 0xFF,
            slot_count: 2,
            channel_count: 2,
        };
        let mut chans = vec![vec![ch(flag::PLAY); 2]; 2];
        assert!(seq_calc(&mut st, &mut chans, |_, _| {}).is_none());
        // The latch is released on the way out of a real pass.
        st.busy = false;
        assert!(seq_calc(&mut st, &mut chans, |_, _| {}).is_some());
        assert!(!st.busy);
    }

    #[test]
    fn seq_calc_walks_only_enabled_slots() {
        let mut st = SeqCalcState {
            busy: false,
            slot_mask: 0b0100,
            slot_count: 4,
            channel_count: 1,
        };
        let mut chans = vec![vec![ch(flag::PLAY)]; 4];
        let trace = seq_calc(&mut st, &mut chans, |_, _| {}).unwrap();
        assert_eq!(trace, vec![(2, 0, SeqCall::Pump)]);
    }

    #[test]
    fn the_slot_bitmap_is_exactly_thirty_two_wide() {
        // `sllv` masks its count to 5 bits, so slot 32 would alias slot 0.
        let mut st = SeqCalcState {
            busy: false,
            slot_mask: 1,
            slot_count: 33,
            channel_count: 1,
        };
        let mut chans = vec![vec![ch(flag::PLAY)]; 33];
        let trace = seq_calc(&mut st, &mut chans, |_, _| {}).unwrap();
        assert_eq!(trace, vec![(0, 0, SeqCall::Pump), (32, 0, SeqCall::Pump)]);
    }

    #[test]
    fn tick_budget_matches_the_retail_formula() {
        // (resolution * tempo * 10) / (divisor * 60)
        let want: i32 = (480i32 * 120 * 10) / (60 * 60);
        assert_eq!(i32::from(tick_budget(480, 120, 60)), want);
        assert_eq!(tick_budget(480, 120, 60), 160);
    }

    #[test]
    fn tick_budget_floors_at_one() {
        // A tempo slow enough to round to zero still advances the pump.
        assert_eq!(tick_budget(1, 1, 60), 1);
        assert_eq!(tick_budget(0, 999, 60), 1);
    }

    #[test]
    fn tick_budget_scales_linearly_with_tempo() {
        let a = tick_budget(480, 60, 60);
        let b = tick_budget(480, 120, 60);
        assert_eq!(b, a * 2);
    }

    #[test]
    fn tempo_countdown_expiry_clears_both_tempo_bits() {
        let mut c = SeqChannel {
            flags: flag::TEMPO_A | flag::TEMPO_B | flag::PLAY,
            tempo_remaining: 0,
            ..Default::default()
        };
        let t = tempo_slide_tick(&mut c, 60);
        assert!(t.finished);
        assert_eq!(c.flags, flag::PLAY);
    }

    #[test]
    fn tempo_slow_mode_steps_one_unit_every_step_ticks() {
        let mut c = SeqChannel {
            flags: flag::TEMPO_A,
            tempo_remaining: 100,
            tempo_step: 4,
            tempo: 100,
            tempo_target: 120,
            resolution: 480,
            ..Default::default()
        };
        // 99 % 4 != 0 -> skipped entirely, budget untouched.
        let t = tempo_slide_tick(&mut c, 60);
        assert!(t.skipped);
        assert_eq!(c.tempo, 100);
        assert_eq!(c.tick_budget, 0);

        c.tempo_remaining = 101; // -> 100, 100 % 4 == 0
        let t = tempo_slide_tick(&mut c, 60);
        assert!(!t.skipped);
        assert_eq!(c.tempo, 101);
        assert_eq!(c.tick_budget, tick_budget(480, 101, 60));
    }

    #[test]
    fn tempo_fast_mode_jumps_and_clamps_at_the_target() {
        // step <= 0 is the fast arm: move by |step|, never past the target.
        let mut c = SeqChannel {
            flags: flag::TEMPO_A,
            tempo_remaining: 100,
            tempo_step: -30,
            tempo: 100,
            tempo_target: 120,
            resolution: 480,
            ..Default::default()
        };
        tempo_slide_tick(&mut c, 60);
        assert_eq!(c.tempo, 120, "clamped to the target, not 130");

        let mut c = SeqChannel {
            flags: flag::TEMPO_A,
            tempo_remaining: 100,
            tempo_step: -5,
            tempo: 100,
            tempo_target: 120,
            resolution: 480,
            ..Default::default()
        };
        tempo_slide_tick(&mut c, 60);
        assert_eq!(c.tempo, 105);
    }

    #[test]
    fn tempo_fast_mode_descends_too() {
        let mut c = SeqChannel {
            flags: flag::TEMPO_A,
            tempo_remaining: 100,
            tempo_step: -5,
            tempo: 100,
            tempo_target: 80,
            resolution: 480,
            ..Default::default()
        };
        tempo_slide_tick(&mut c, 60);
        assert_eq!(c.tempo, 95);
    }

    #[test]
    fn tempo_step_zero_freezes_the_tempo_but_still_recomputes_the_budget() {
        let mut c = SeqChannel {
            flags: flag::TEMPO_A,
            tempo_remaining: 100,
            tempo_step: 0,
            tempo: 90,
            tempo_target: 120,
            resolution: 480,
            ..Default::default()
        };
        let t = tempo_slide_tick(&mut c, 60);
        assert!(!t.skipped);
        assert_eq!(c.tempo, 90);
        assert_eq!(c.tick_budget, tick_budget(480, 90, 60));
    }

    #[test]
    fn tempo_settles_when_it_reaches_the_target() {
        let mut c = SeqChannel {
            flags: flag::TEMPO_A | flag::TEMPO_B,
            tempo_remaining: 100,
            tempo_step: 1,
            tempo: 119,
            tempo_target: 120,
            resolution: 480,
            ..Default::default()
        };
        let t = tempo_slide_tick(&mut c, 60);
        assert!(t.finished);
        assert_eq!(c.tempo, 120);
        assert_eq!(c.flags & (flag::TEMPO_A | flag::TEMPO_B), 0);
    }

    #[test]
    fn volume_slide_expiry_clears_its_own_bit_only() {
        for dir in [SlideDir::Up, SlideDir::Down] {
            let mut c = SeqChannel {
                flags: flag::VOL_UP | flag::VOL_DOWN | flag::PLAY,
                slide_remaining: 0,
                ..Default::default()
            };
            let t = volume_slide_tick(&mut c, dir, (40, 40));
            assert!(t.settled);
            assert_eq!(c.flags & dir.flag_bit(), 0);
            assert_ne!(c.flags & flag::PLAY, 0);
        }
    }

    #[test]
    fn volume_slow_mode_moves_one_unit_per_fire() {
        let mut c = SeqChannel {
            flags: flag::VOL_UP,
            slide_remaining: 9,
            slide_step: 4,
            slide_level: 10,
            ..Default::default()
        };
        // 8 % 4 == 0 -> fires.
        let t = volume_slide_tick(&mut c, SlideDir::Up, (40, 41));
        assert_eq!(t.commit, Some((41, 42)));

        let mut c = SeqChannel {
            flags: flag::VOL_DOWN,
            slide_remaining: 9,
            slide_step: 4,
            slide_level: 10,
            ..Default::default()
        };
        let t = volume_slide_tick(&mut c, SlideDir::Down, (40, 41));
        assert_eq!(t.commit, Some((39, 40)));
    }

    #[test]
    fn volume_slow_mode_skips_off_the_step_boundary() {
        let mut c = SeqChannel {
            flags: flag::VOL_UP,
            slide_remaining: 10,
            slide_step: 4,
            slide_level: 10,
            ..Default::default()
        };
        // 9 % 4 != 0
        let t = volume_slide_tick(&mut c, SlideDir::Up, (40, 40));
        assert_eq!(t.commit, None);
        assert_eq!(c.slide_level, 10, "the level counter does not move either");
    }

    #[test]
    fn a_spent_level_counter_saturates_and_clears_the_flag() {
        for (dir, end) in [(SlideDir::Up, (0x7F, 0x7F)), (SlideDir::Down, (0, 0))] {
            let mut c = SeqChannel {
                flags: flag::VOL_UP | flag::VOL_DOWN,
                slide_remaining: 9,
                slide_step: 4,
                slide_level: 0,
                ..Default::default()
            };
            let t = volume_slide_tick(&mut c, dir, (40, 40));
            assert!(t.saturated);
            assert_eq!(t.commit, Some(end));
            assert_eq!(c.flags & dir.flag_bit(), 0);
        }
    }

    #[test]
    fn descending_refuses_to_move_a_pair_already_at_zero() {
        let mut c = SeqChannel {
            flags: flag::VOL_DOWN,
            slide_remaining: 9,
            slide_step: 4,
            slide_level: 10,
            ..Default::default()
        };
        let t = volume_slide_tick(&mut c, SlideDir::Down, (0, 40));
        assert_eq!(t.commit, None);
        assert!(t.settled);
        assert_eq!(c.flags & flag::VOL_DOWN, 0);
    }

    #[test]
    fn volume_fast_mode_saturates_when_both_sides_reach_the_endpoint() {
        let mut c = SeqChannel {
            flags: flag::VOL_UP,
            slide_remaining: 9,
            slide_total: 100,
            slide_span: 0,
            slide_step: -0x40,
            slide_level: 0x400,
            ..Default::default()
        };
        let t = volume_slide_tick(&mut c, SlideDir::Up, (0x50, 0x50));
        assert!(t.saturated);
        assert_eq!(t.commit, Some((0x7F, 0x7F)));
    }

    #[test]
    fn volume_fast_mode_ramps_while_inside_the_span_budget() {
        let mut c = SeqChannel {
            flags: flag::VOL_UP,
            slide_remaining: 9,
            slide_total: 10,
            slide_span: 0x400,
            slide_step: -8,
            slide_level: 0x400,
            ..Default::default()
        };
        // elapsed = 10 - 8 = 2; 2*8 = 16 < 0x400 -> commits the ramp.
        let t = volume_slide_tick(&mut c, SlideDir::Up, (0x10, 0x11));
        assert_eq!(t.commit, Some((0x18, 0x19)));
    }

    #[test]
    fn volume_step_zero_does_nothing_at_all() {
        let mut c = SeqChannel {
            flags: flag::VOL_UP,
            slide_remaining: 9,
            slide_step: 0,
            slide_level: 10,
            ..Default::default()
        };
        let t = volume_slide_tick(&mut c, SlideDir::Up, (40, 40));
        assert_eq!(t, SlideTick::default());
        assert_ne!(c.flags & flag::VOL_UP, 0);
    }

    #[test]
    fn every_volume_tick_recaches_the_pair() {
        let mut c = SeqChannel {
            flags: flag::VOL_UP,
            slide_remaining: 9,
            slide_step: 0,
            ..Default::default()
        };
        volume_slide_tick(&mut c, SlideDir::Up, (0x33, 0x44));
        assert_eq!(c.vol_cache, (0x33, 0x44));
    }

    #[test]
    fn repeat_target_zero_loops_forever() {
        let mut c = SeqChannel {
            flags: flag::PLAY,
            repeat_target: 0,
            start: 0x100,
            alt_start: 0x200,
            cursor: 0x999,
            tick_accum: 7,
            pending_wait: 7,
            running_lo: 7,
            ..Default::default()
        };
        assert_eq!(track_end(&mut c), TrackEnd::LoopForever);
        assert_eq!(c.cursor, 0x100);
        assert_eq!((c.tick_accum, c.pending_wait, c.running_lo), (0, 0, 0));
        assert_ne!(c.flags & flag::PLAY, 0, "still playing");
    }

    #[test]
    fn the_alt_loop_point_is_selected_by_flag_0x400() {
        let mut c = SeqChannel {
            flags: flag::LOOP_ALT,
            repeat_target: 0,
            start: 0x100,
            alt_start: 0x200,
            ..Default::default()
        };
        track_end(&mut c);
        assert_eq!(c.cursor, 0x200);
    }

    #[test]
    fn an_intermediate_repeat_republishes_the_loop_cursor_too() {
        let mut c = SeqChannel {
            repeat_target: 3,
            repeat_count: 0,
            start: 0x100,
            cursor: 0x999,
            loop_cursor: 0x999,
            ..Default::default()
        };
        assert_eq!(track_end(&mut c), TrackEnd::Repeat { count: 1 });
        assert_eq!(c.cursor, 0x100);
        assert_eq!(c.loop_cursor, 0x100);
    }

    #[test]
    fn the_final_repeat_rewrites_the_flags_and_reloads_the_wait() {
        let mut c = SeqChannel {
            flags: flag::PLAY | flag::START | flag::STOP,
            repeat_target: 2,
            repeat_count: 1,
            chain_slot: 0xFF,
            tick_budget: 160,
            playing: 1,
            ..Default::default()
        };
        assert_eq!(track_end(&mut c), TrackEnd::Finished { chain: None });
        assert_eq!(c.flags & (flag::PLAY | flag::START | flag::STOP), 0);
        assert_ne!(c.flags & flag::FINISHED, 0);
        assert_ne!(c.flags & flag::REWIND, 0);
        assert_eq!(c.playing, 0);
        assert_eq!(c.pending_wait, 160);
    }

    #[test]
    fn a_non_ff_chain_byte_names_the_successor() {
        let mut c = SeqChannel {
            repeat_target: 1,
            repeat_count: 0,
            chain_slot: 3,
            chain_channel: 5,
            ..Default::default()
        };
        assert_eq!(
            track_end(&mut c),
            TrackEnd::Finished {
                chain: Some((3, 5))
            }
        );
    }

    #[test]
    fn the_finished_flag_does_not_survive_the_same_frames_rewind_arm() {
        // track_end sets FINISHED *and* REWIND; SsSeqCalc's bit-0x4 arm runs
        // later in the same frame and zeroes the word, so FINISHED is never
        // observable on a later frame.
        let mut c = SeqChannel {
            flags: flag::PLAY,
            repeat_target: 1,
            chain_slot: 0xFF,
            ..Default::default()
        };
        track_end(&mut c);
        assert_ne!(c.flags & flag::FINISHED, 0);
        dispatch_channel(&mut c, |_, _| {});
        assert_eq!(c.flags, 0);
    }
}

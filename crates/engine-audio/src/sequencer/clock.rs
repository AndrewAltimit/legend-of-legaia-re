//! The sample clock: tick entry points, the retail vsync pump and event advance.

use super::*;
use crate::Spu;
use crate::spu::voice::SPU_INTERNAL_RATE;
use legaia_seq::{EventBody, MetaMessage, Seq};

impl Sequencer {
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
    ///
    /// Key-ons are written at flush boundaries every [`FLUSH_SAMPLES`], the
    /// way retail's per-vsync `SsSeqCalc` stages them for its flush: the
    /// events of each flush period fire first, then the period's staged
    /// key-ons are written ([`Self::flush_key_ons`]).
    fn advance_samples(&mut self, spu: &mut Spu, samples: u64) {
        if samples == 0 {
            // A zero-length advance drains a leading run of zero-delta events
            // on the sample clock; the retail pump only runs at a vsync.
            if self.exact_tempo {
                self.advance_events(spu, 0);
            }
            return;
        }
        let mut remaining = samples;
        while remaining > 0 {
            let step = remaining.min(FLUSH_SAMPLES - self.flush_pos);
            if self.exact_tempo {
                self.advance_events(spu, step);
            }
            self.flush_pos += step;
            remaining -= step;
            if self.flush_pos >= FLUSH_SAMPLES {
                self.flush_pos = 0;
                if !self.exact_tempo {
                    self.pump_vsync(spu);
                }
                if self.master_vol_dirty {
                    self.master_vol_dirty = false;
                    self.remix_all(spu);
                }
                self.flush_key_ons(spu);
            }
        }
    }

    /// The tick budget the current tempo installs, in tenths of a tick per
    /// vsync (`+0x54`): the header's rounded BPM until a tempo meta fires,
    /// the meta's truncated one after. Floored at `1`; the slow mode
    /// (`+0x52 >= 0`, under one tick a vsync) is not reached by a shipped
    /// track and runs at that floor.
    fn retail_budget(&self) -> i64 {
        let bpm = if self.header_tempo {
            crate::seq_calc::header_bpm(self.tempo_us_per_qn)
        } else {
            (self.tempo_us_per_qn != 0).then(|| 60_000_000 / self.tempo_us_per_qn)
        }
        .unwrap_or(0);
        let (_, budget) = crate::seq_calc::tempo_set_budget(
            self.seq.header.ppqn.max(1) as i16,
            bpm,
            crate::seq_calc::RETAIL_TICK_DIVISOR,
        );
        i64::from(budget.max(1))
    }

    /// One `SsSeqCalc` of the delta-time pump (`FUN_800639A0`, ported
    /// standalone as [`crate::seq_events::pump_delta_time`]), driven over the
    /// parsed event list: a wait that outlives the vsync's budget is reduced
    /// by it and nothing fires; otherwise events fire until the waits
    /// accumulated since the vsync began reach the budget - which is re-read
    /// after every event, so a tempo meta retunes the vsync it lands in - and
    /// the overshoot carries into the next wait. Events therefore land on the
    /// vsync grid retail lands them on, including after a tempo change.
    fn pump_vsync(&mut self, spu: &mut Spu) {
        if self.finished {
            return;
        }
        let delta_of =
            |seq: &Seq, i: usize| seq.events.get(i).map_or(0, |e| i64::from(e.delta) * 10);
        let wait = self
            .pump_wait
            .unwrap_or_else(|| delta_of(&self.seq, self.next));
        let short = wait - self.retail_budget();
        if short > 0 {
            self.pump_wait = Some(short);
            return;
        }
        let mut accum = wait;
        // A port bound on one vsync's events; retail spins.
        for _ in 0..100_000 {
            let Some(event) = self.seq.events.get(self.next) else {
                self.finished = true;
                if let Some(target) = self.loop_target() {
                    self.rewind_to(target, spu);
                    self.pump_wait = Some(delta_of(&self.seq, self.next));
                }
                return;
            };
            self.abs_tick += u64::from(event.delta);
            let is_eot = matches!(event.body, EventBody::Meta(MetaMessage::EndOfTrack));
            self.fire(spu, self.next);
            self.next += 1;
            if self.pending_loop_forever {
                self.pending_loop_forever = false;
                let target = self.loop_start.unwrap_or(0).min(self.seq.events.len());
                self.rewind_to(target, spu);
            } else if is_eot {
                match self.loop_target() {
                    Some(target) => self.rewind_to(target, spu),
                    None => {
                        self.finished = true;
                        return;
                    }
                }
            }
            let d = delta_of(&self.seq, self.next);
            if d == 0 {
                continue;
            }
            accum += d;
            let budget = self.retail_budget();
            if accum < budget {
                continue;
            }
            self.pump_wait = Some(accum - budget);
            return;
        }
        self.pump_wait = Some(0);
    }

    /// Fire every event that has come due after `samples` more samples.
    fn advance_events(&mut self, spu: &mut Spu, samples: u64) {
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
                .saturating_mul(self.play_us_per_qn as u64)
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
}

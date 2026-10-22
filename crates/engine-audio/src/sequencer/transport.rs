//! Construction, transport control (loop, rewind, stop, pause) and the read-only status of a [`Sequencer`].

use super::*;
use crate::Spu;
use crate::vab_bind::VabBank;
use legaia_seq::Seq;

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
            play_us_per_qn: retail_header_play_tempo(tempo, ppqn),
            exact_tempo: false,
            header_tempo: true,
            abs_tick: 0,
            finished: false,
            loop_to: usize::MAX,
            loop_start: None,
            pending_loop_forever: false,
            channels: [ChannelState::default(); CHANNELS],
            active: Vec::new(),
            master_vol: 127,
            master_vol_dirty: false,
            loop_count: 0,
            voice_prio: [0; crate::spu::NUM_VOICES],
            voice_age: [0; crate::spu::NUM_VOICES],
            flush_pos: 0,
            pending: Vec::new(),
            next_pending_id: 0,
            pump_wait: None,
        }
    }

    /// Set the sequence volume (libsnd `SsSeqSetVol`, the channel record's
    /// `+0x58`/`+0x5A`), 0..=127: folded in per side after the key-on head
    /// chain ([`channel_mix`]). The game's BGM records run at
    /// [`RETAIL_BGM_SEQ_VOL`].
    ///
    /// A change re-volumes the notes already sounding, at the next flush:
    /// retail's set (`FUN_80064890` for a playing slot, and every step of the
    /// `SsSeqSetVol` slide `FUN_8006320C` / `FUN_8006352C`) runs
    /// `FUN_80067E9C`, which stores the new `+0x58` / `+0x5A` and then
    /// recomputes the left / right volume of every voice whose owner key is
    /// the sequence's, staging them for the vsync flush. A held note under a
    /// summon duck therefore follows the duck, not just the notes keyed after
    /// it.
    ///
    /// PORT: FUN_80067E9C (the per-voice re-apply loop, via
    /// [`Self::remix_all`] at the next flush)
    ///
    /// Partial: retail keeps independent left / right sequence volumes, each
    /// clamped to `0x7F` (`+0x58` / `+0x5A`, `0x80067ef0..0x80067f24`), and
    /// re-reads the channel pan from the per-voice record (`0x8006824c`);
    /// this keeps one value for both sides and the channel's current pan.
    pub fn set_master_vol(&mut self, v: u8) {
        let v = v.min(127);
        if v != self.master_vol {
            self.master_vol = v;
            self.master_vol_dirty = true;
        }
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
    pub(super) fn loop_target(&self) -> Option<usize> {
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

    /// Play the score's exact tempo (`true`) or the tempo retail's driver
    /// actually plays (`false`, the default). Retail does not clock a track
    /// in samples: each tempo set installs an integer budget of tenths of a
    /// tick per vsync (`FUN_80061954`, rounded to nearest), so a track runs
    /// up to a few percent off its written tempo - 80 BPM plays at 80.25,
    /// 23.67 BPM at 23.71. The exact mode is the score as written.
    pub fn set_exact_tempo(&mut self, exact: bool) {
        self.exact_tempo = exact;
        self.reprice_tempo();
    }

    pub(super) fn reprice_tempo(&mut self) {
        let ppqn = self.seq.header.ppqn.max(1);
        self.play_us_per_qn = if self.exact_tempo {
            self.tempo_us_per_qn
        } else if self.header_tempo {
            retail_header_play_tempo(self.tempo_us_per_qn, ppqn)
        } else {
            retail_play_tempo(self.tempo_us_per_qn, ppqn)
        };
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

    /// Monotonic count of loop rewinds so far. Increments each time the
    /// sequencer loops back (an in-stream marker, an EOT with a loop target,
    /// or an external [`Self::rewind_to`]). Lets a caller detect loop-backs
    /// robustly - unlike [`Self::playhead_ticks`], which can peak and reset
    /// within one sample on a zero-delta EOT and so hide the boundary.
    pub fn loop_count(&self) -> u64 {
        self.loop_count
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

    /// Key-off every active note and reset the playhead to `to`. Called by
    /// the loop logic and exposed publicly so engines can implement
    /// gameplay-driven loop points.
    pub fn rewind_to(&mut self, to: usize, spu: &mut Spu) {
        self.silence_all(spu);
        self.next = to;
        self.pump_wait = None;
        self.accum = 0;
        self.sample_carry = 0.0;
        self.finished = false;
        self.loop_count = self.loop_count.wrapping_add(1);
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

    /// Pause: key-off every sounding note and hold the playhead where it is,
    /// so the next tick after a resume fires the next event rather than
    /// replaying the intro.
    ///
    /// Retail's pause arm (`FUN_800628F0`, mode `0`) only raises the slot's
    /// flag `0x2`; the next per-tick service (`FUN_80062F98`) routes a
    /// flagged slot through `FUN_800638D8`, which kills the channel's notes
    /// and clears the flag. A gate that froze the clock but left the voices
    /// keyed on held whatever was sounding for as long as it stayed shut -
    /// the title theme's last note ringing under the whole attract movie.
    // REF: FUN_800638D8
    pub fn pause(&mut self, spu: &mut Spu) {
        self.silence_all(spu);
    }

    /// Forget every note whose voice another client has re-keyed since, so
    /// its later note-off, bend or CC does not reach the cue now sounding
    /// there and the allocator no longer counts the voice as the note's.
    pub(super) fn release_lost_voices(&mut self, spu: &Spu) {
        self.active.retain(|n| {
            spu.voices
                .get(n.voice as usize)
                .is_some_and(|v| v.key_on_count() == n.key_on_stamp)
        });
    }

    fn silence_all(&mut self, spu: &mut Spu) {
        self.release_lost_voices(spu);
        self.pending.clear();
        for note in self.active.drain(..) {
            if (note.voice as usize) < spu.voices.len() {
                spu.voices[note.voice as usize].key_off();
                spu.record_key_off(note.voice as usize);
            }
        }
    }
}

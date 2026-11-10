//! Note on / off, tone layering, the key-on flush, channel remix and SPU voice allocation.

use super::*;
use crate::Spu;

/// Output samples a flush's KON write trails a flush with one staged voice, for
/// `staged` voices with a register to write. See `docs/subsystems/audio.md`,
/// [the flush's own cost](../../../docs/subsystems/audio.md#the-flushs-own-cost).
// REF: FUN_80065BAC, FUN_8006C048
pub fn flush_key_on_lag(staged: u32) -> u8 {
    let q8 = staged.saturating_sub(1) * FLUSH_STAGED_VOICE_Q8;
    ((q8 + 0x80) >> 8).min(u32::from(u8::MAX)) as u8
}

/// Output samples, Q8, one staged voice costs a flush: `1.2`, fitted per flush
/// off six mednafen recordings (`1.15..=1.25`, scatter under a sample). A
/// PCSX-Redux cycle watch reads `1.34`, in cycles, with the same straight line.
pub(super) const FLUSH_STAGED_VOICE_Q8: u32 = 307;

/// Flushes the silent history at `0x801CE208` holds (`_DAT_801CE204 & 0xF`).
pub(super) const SILENT_RING_LEN: usize = 16;
/// How many of those words the release test ANDs: `0x80065CAC`'s loop stops one
/// short, so word `15` is written and never read.
const SILENT_RING_READ: usize = 15;
/// A key-on stores this in the record's envelope (`FUN_80065978`).
const KEYED_ENV: u16 = 0x7FFF;

impl Sequencer {
    pub(super) fn note_on(&mut self, spu: &mut Spu, channel: u8, key: u8, velocity: u8) {
        // A second note-on of a (channel, key) already sounding does NOT
        // release the first: `FUN_80061B24` hands a non-zero velocity straight
        // to `FUN_80066308`, which allocates fresh voices. A battle census
        // sees both of two same-tick `prog 0 / key 60` notes key their two
        // layers each (nine voices from five notes). The channel's next
        // note-off for that key releases every voice holding it.
        //
        // One voice per covering tone. A program may stack several tones over
        // one key (a layered instrument), and retail keys every one of them:
        // FUN_80068568 collects the tones whose `min..=max` covers the key,
        // and FUN_80066308 runs the allocation scan + key-on once per tone
        // (`0x800664F0..0x8006684C`), a failed scan dropping only that layer.
        let program = self.channels[channel as usize].program as usize;
        // The score's note stream, comparable line for line with the retail
        // key-on census's `note` rows (`autorun_keyon_census.lua`).
        log::trace!(
            "sequencer: note-on t={} ch={channel} prog={program} key={key} vel={velocity}",
            self.abs_tick
        );
        let layers = self.bank.layer_tones(program, key);
        if layers.is_empty() {
            log::trace!("sequencer: no tone for ch{channel} prog{program} key{key}");
            return;
        }
        for tone in layers {
            self.key_layer(spu, channel, key, velocity, program, tone);
        }
    }

    /// Allocate and key one layer (tone) of a note-on.
    fn key_layer(
        &mut self,
        spu: &mut Spu,
        channel: u8,
        key: u8,
        velocity: u8,
        program: usize,
        tone: usize,
    ) {
        // Resolve the layer's allocation priority (the VAB tone `prior` byte)
        // before scanning: retail stages the tone attrs first (FUN_80066308)
        // and only then runs the allocation scan against that priority.
        let Some(prio) = self.bank.tone_prior_at(program, tone) else {
            return;
        };
        // The tone can still fail to sound if its slot is empty (`vag <= 0`)
        // or its sample never uploaded (SPU RAM exhausted). Verify it can
        // actually fire BEFORE `alloc_voice`, which may key-off a sounding
        // voice: stealing a live note for one that then fails to play is a
        // net dropped note.
        if !self.bank.can_play_tone(program, tone) {
            log::trace!(
                "sequencer: tone {tone} not playable for ch{channel} prog{program} key{key}; not stealing"
            );
            return;
        }
        let Some(voice) = self.alloc_voice(spu, prio) else {
            self.note_dropped();
            log::trace!(
                "sequencer: no free voice for ch{} key{} (active={})",
                channel,
                key,
                self.active.len()
            );
            return;
        };
        // The voice is chosen and reserved now; the key-on is staged for the
        // next flush, as retail's note-on ORs the voice into the key-on
        // accumulator (`_DAT_801CDB48/4A`) that `FUN_80065BAC` writes once
        // per vsync. A note-off before that flush cancels it outright:
        // `FUN_80067480` clears the voice's staged key-on bit as it sets its
        // key-off bit (`0x80067500..0x80067544`), so a note shorter than one
        // flush period never sounds.
        let id = self.next_pending_id;
        self.next_pending_id = self.next_pending_id.wrapping_add(1);
        if voice == 0 {
            // The key-on stages the tone's ADSR into the shadow afresh
            // (`FUN_80065978`), over anything an earlier drop stored there.
            self.voice0_adsr2_zeroed = None;
        }
        let key_on_stamp = spu
            .voices
            .get(voice as usize)
            .map_or(0, |v| v.key_on_count());
        self.active.push(ActiveNote {
            channel,
            voice,
            velocity,
            program,
            tone,
            key_on_stamp,
            pending: Some(id),
        });
        self.voice_note[voice as usize] = Some(VoiceNote {
            program,
            tone,
            key,
            channel,
            velocity,
            pan: self.channels[channel as usize].pan,
            stamp: None,
            staged_wheel: None,
            staged_mix: None,
        });
        self.pending.push(PendingKey {
            id,
            voice,
            key,
            velocity,
            program,
            tone,
        });
    }

    /// A layer the allocation scan found no voice for. `FUN_80066B00`'s
    /// failure path still stores to the winner's record, `sh zero` at
    /// `rec + 0x2A` (`0x80066D74`) with the winner the out-of-range voice
    /// count: record `24` of the `0x36`-byte table at `0x801CDB50`, whose
    /// `+0x2A` is `0x801CE08A`, the ADSR2 word of voice `0`'s shadow
    /// registers at `0x801CE080`. A voice-0 key-on staged earlier in the same
    /// calc then goes out with ADSR2 `0` - its sustain rises at the fastest
    /// rate instead of decaying, and its release is instant; one staged later
    /// writes the shadow afresh.
    fn note_dropped(&mut self) {
        self.voice0_adsr2_zeroed = self
            .pending
            .iter()
            .rev()
            .find(|p| p.voice == 0)
            .map(|p| p.id);
    }

    /// Write every staged key-on - retail's per-vsync flush
    /// (`FUN_80065BAC` handing the key-on accumulator to `FUN_8006B854`).
    /// Layers whose note was released, stolen or re-keyed since they were
    /// staged are skipped. [`Self::tick_sample`] / [`Self::tick_us`] call
    /// this at every [`FLUSH_SAMPLES`] boundary.
    pub fn flush_key_ons(&mut self, spu: &mut Spu) {
        if self.pending.is_empty() {
            self.staged_voices = 0;
            self.last_flush_staged = 0;
            return;
        }
        self.release_lost_voices(spu);
        let mut keyed = 0u32;
        for p in std::mem::take(&mut self.pending) {
            let Some(idx) = self.active.iter().position(|n| n.pending == Some(p.id)) else {
                continue;
            };
            let voice = p.voice as usize;
            keyed |= 1 << voice;
            // Give the layer the master × velocity effective velocity only -
            // NOT the channel volume. The channel volume (CC7) and pan (CC10)
            // are applied afterward as dynamic channel-expression
            // (`channel_mix`) so a mid-note CC7/CC10 can re-derive the voice
            // volume from the same base.
            let combined = p.velocity;
            if !self
                .bank
                .play_note_layer(spu, voice, p.program, p.tone, p.key, combined)
            {
                self.active.swap_remove(idx);
                continue;
            }
            if voice == 0 && self.voice0_adsr2_zeroed == Some(p.id) {
                let v = &mut spu.voices[0];
                v.adsr_cfg = crate::spu::adsr::AdsrConfig::from_words(v.adsr_cfg.raw.0, 0);
            }
            self.voice_keys_seen[voice] = spu.voices[voice].key_on_count();
            // A key-on carries the note's own pitch: retail keeps no channel
            // wheel. Only a bend that reached this note since it was staged
            // shows, through the shadow register the flush hands over.
            let mut staged_mix = None;
            if let Some(n) = self.voice_note[voice].as_mut() {
                let wheel = n.staged_wheel.take();
                staged_mix = n.staged_mix.take();
                n.stamp = spu.voices.get(voice).map(|v| v.key_on_count());
                if let (Some(pitch), Some(v)) = (
                    wheel.and_then(|w| self.bank.bent_pitch(n.program, n.tone, n.key, w)),
                    spu.voices.get_mut(voice),
                ) {
                    v.pitch = pitch;
                }
            }
            // The voice volume through the full retail chain: CC7 folded into
            // the velocity, head, sequence volume, tone / program / channel
            // pans, square taper.
            if let Some((l, r)) = staged_mix.or_else(|| self.note_volume(&self.active[idx]))
                && let Some(v) = spu.voices.get_mut(voice)
            {
                v.vol_left = l;
                v.vol_right = r;
            }
            let note = &mut self.active[idx];
            note.key_on_stamp = spu.voices.get(voice).map_or(0, |v| v.key_on_count());
            note.pending = None;
        }
        self.voice0_adsr2_zeroed = None;
        // `FUN_80065BAC` writes the KON mask after its per-voice loop, and every
        // voice with a staged register costs a `FUN_8006C048` call first.
        let staged = (self.staged_voices | keyed).count_ones();
        self.staged_voices = 0;
        self.last_flush_staged = staged;
        let lag = flush_key_on_lag(staged);
        for (v, voice) in spu.voices.iter_mut().enumerate() {
            if keyed & (1 << v) != 0 {
                voice.delay_start(lag);
            }
        }
    }

    /// `0xEn`, `FUN_8006166C` -> `FUN_80067C1C`: re-pitch every voice whose
    /// driver record names this sequence and the channel's program, whichever
    /// channel keyed it and whether or not it was released. Nothing is stored:
    /// the next key-on on the channel plays at its own pitch.
    ///
    /// PORT: FUN_80067C1C
    pub(super) fn bend_program(&mut self, spu: &mut Spu, ch: usize, wheel: u8) {
        let program = self.channels[ch].program as usize;
        for (voice, slot) in self.voice_note.iter_mut().enumerate() {
            let Some(n) = slot.as_mut().filter(|n| n.program == program) else {
                continue;
            };
            let Some(stamp) = n.stamp else {
                n.staged_wheel = Some(wheel);
                continue;
            };
            let Some(v) = spu.voices.get_mut(voice) else {
                continue;
            };
            if v.key_on_count() == stamp
                && let Some(pitch) = self.bank.bent_pitch(n.program, n.tone, n.key, wheel)
            {
                v.pitch = pitch;
                self.staged_voices |= 1 << voice;
            }
        }
    }

    /// `0xB7` / `0xBA`, `FUN_80060EBC` / `FUN_80060F8C` -> `FUN_8006861C`: rebuild
    /// the volume of every voice whose record names this sequence and the
    /// channel's **program**, whichever channel keyed it and whether or not it
    /// has been released, from the record's own velocity and the event's channel
    /// volume and pan, each floored at `1`. So a pan move swings a tail still
    /// ringing, and a channel sharing a program moves with it. Returns whether a
    /// record matched.
    ///
    /// PORT: FUN_8006861C
    pub(super) fn remix_program(&mut self, spu: &mut Spu, ch: usize) -> bool {
        let cs = self.channels[ch];
        let program = cs.program as usize;
        let (vol, pan) = (cs.volume.max(1), cs.pan.max(1));
        let mut matched = false;
        for voice in 0..self.voice_note.len() {
            let Some(n) = self.voice_note[voice].filter(|n| n.program == program) else {
                continue;
            };
            let vel = effective_velocity(n.velocity, vol);
            let Some(head) = self
                .bank
                .seq_note_volume(n.program, n.tone, vel, self.master_vol)
            else {
                continue;
            };
            if self.stage_mix(spu, voice, channel_tail(head, pan)) {
                matched = true;
            }
        }
        matched
    }

    /// The per-voice loop of `FUN_80067E9C` that a sequence-volume change runs
    /// (see [`Self::set_master_vol`]): every record the sequence owns, released
    /// ones too, from its own velocity, its channel's volume as it stands and
    /// the channel pan **the note keyed with** (`+0x0A`, which a CC10 does not
    /// write).
    pub(super) fn remix_all(&mut self, spu: &mut Spu) {
        for voice in 0..self.voice_note.len() {
            let Some(n) = self.voice_note[voice] else {
                continue;
            };
            let vel = effective_velocity(n.velocity, self.channels[n.channel as usize].volume);
            let Some(head) = self
                .bank
                .seq_note_volume(n.program, n.tone, vel, self.master_vol)
            else {
                continue;
            };
            self.stage_mix(spu, voice, channel_tail(head, n.pan));
        }
    }

    /// Put a rebuilt volume on a voice the record still owns, or in the shadow
    /// registers of one whose key-on waits for the flush. `false` when a cue has
    /// taken the voice: its record names owner `0x21` now.
    fn stage_mix(&mut self, spu: &mut Spu, voice: usize, mix: (i16, i16)) -> bool {
        let Some(n) = self.voice_note[voice].as_mut() else {
            return false;
        };
        let Some(stamp) = n.stamp else {
            n.staged_mix = Some(mix);
            return true;
        };
        let Some(v) = spu
            .voices
            .get_mut(voice)
            .filter(|v| v.key_on_count() == stamp)
        else {
            return false;
        };
        v.vol_left = mix.0;
        v.vol_right = mix.1;
        self.staged_voices |= 1 << voice;
        true
    }

    /// A sounding note's voice volume, the sequencer path of `FUN_80067550`
    /// end to end: the channel's CC7 folded into the velocity
    /// ([`effective_velocity`]), the head chain, the sequence volume
    /// ([`Self::set_master_vol`]) and the tone / program pans
    /// ([`VabBank::seq_note_volume`]), then the channel pan and the square
    /// taper ([`channel_tail`]). `None` when the tone does not resolve.
    fn note_volume(&self, note: &ActiveNote) -> Option<(i16, i16)> {
        let cs = self.channels[note.channel as usize];
        let vel = effective_velocity(note.velocity, cs.volume);
        let head = self
            .bank
            .seq_note_volume(note.program, note.tone, vel, self.master_vol)?;
        Some(channel_tail(head, cs.pan))
    }

    /// `FUN_8006688C`: release every voice whose record names this sequence, the
    /// channel's program **as it stands now** and the key. The channel is not in
    /// the test. A note whose channel changed program since it keyed is not
    /// found, and holds until something steals its voice; a note on another
    /// channel sharing the program and key is released with this one.
    ///
    /// PORT: FUN_8006688C
    pub(super) fn note_off(&mut self, spu: &mut Spu, channel: u8, key: u8) -> u32 {
        let program = self.channels[channel as usize].program as usize;
        let mut released = 0;
        for voice in 0..self.voice_note.len() {
            let Some(n) = self.voice_note[voice] else {
                continue;
            };
            if n.program != program || n.key != key {
                continue;
            }
            let owned = n
                .stamp
                .is_none_or(|stamp| spu.voices[voice].key_on_count() == stamp);
            if !owned {
                continue;
            }
            // `FUN_80067480`: the held mark drops at once.
            self.voice_held &= !(1 << voice);
            let before = self.active.len();
            self.active.retain(|a| a.voice as usize != voice);
            if self.active.len() != before {
                spu.voices[voice].key_off();
                spu.record_key_off(voice);
                released |= 1 << voice;
            }
        }
        released
    }

    /// The top of `FUN_80065BAC`: read every voice's envelope into its record,
    /// write the new word of the silent history, and drop the held mark of a
    /// voice silent through all of it. A voice a cue keyed since the last look
    /// takes the marks `FUN_80065034` writes: held, age zero, no silent history.
    ///
    /// PORT: FUN_80065BAC (the envelope read and the silent history)
    pub(super) fn read_envelopes(&mut self, spu: &Spu) {
        // A new sequencer inherits a voice file another one keyed: the tails
        // still ringing there count by their envelopes and hold no mark, which
        // their key-off lowered.
        if !self.voices_seen {
            self.voices_seen = true;
            for (v, voice) in spu.voices.iter().enumerate() {
                self.voice_keys_seen[v] = voice.key_on_count();
                self.voice_env_next[v] = if voice.is_off() { 0 } else { voice.adsr.level };
            }
        }
        for (v, voice) in spu.voices.iter().enumerate() {
            let keys = voice.key_on_count();
            if keys != self.voice_keys_seen[v] {
                self.voice_keys_seen[v] = keys;
                self.voice_held |= 1 << v;
                self.voice_age[v] = 0;
                self.forget_silence(v);
            }
        }
        self.voice_env = self.voice_env_next;
        self.silent_at = (self.silent_at + 1) % SILENT_RING_LEN;
        let mut silent = 0;
        for (v, voice) in spu.voices.iter().enumerate() {
            self.voice_env_next[v] = if voice.is_off() { 0 } else { voice.adsr.level };
            if self.voice_env[v] == 0 {
                silent |= 1 << v;
            }
        }
        self.silent_ring[self.silent_at] = silent;
        let through = self.silent_ring[..SILENT_RING_READ]
            .iter()
            .fold(u32::MAX, |a, w| a & w);
        self.voice_held &= !through;
    }

    /// `FUN_80065978`: a key-on takes the voice out of every word of the history.
    pub(super) fn forget_silence(&mut self, voice: usize) {
        for word in &mut self.silent_ring {
            *word &= !(1 << voice);
        }
    }

    /// Retail voice-allocation scan.
    ///
    /// PORT: FUN_80066B00 - the sound driver's voice chooser (partial; see
    /// the end of this note), called per
    /// resolved tone by the note-trigger dispatcher (`FUN_80066308`) with the
    /// tone's staged `prior` byte as the request priority. Semantics recovered
    /// from the decompiled reference (no bytes copied):
    ///
    /// 1. Scan voices in **ascending order**. The first voice that is both
    ///    unreserved and has a fully-decayed envelope is taken immediately -
    ///    the scan stops there (first-idle, not last-idle).
    /// 2. While scanning, every voice also feeds a running steal-candidate
    ///    evaluation against a priority threshold that **starts at the
    ///    request's priority and tightens** to each lower priority seen:
    ///    - a voice with priority strictly below the threshold becomes the
    ///      candidate and lowers the threshold to its priority;
    ///    - a voice with priority equal to the threshold competes on the
    ///      **lowest current envelope level**, then (envelopes equal) on the
    ///      **largest allocation age** (least-recently allocated).
    ///
    ///    Net effect: the steal target is the minimum-priority voice among
    ///    those with priority `<=` the request, quietest-then-oldest within
    ///    that tier.
    /// 3. No idle voice and no candidate (every sounding voice outranks the
    ///    request): the note is **dropped** - retail returns the voice count
    ///    as an out-of-range sentinel and the dispatcher marks the note
    ///    failed. This differs from a naive always-steal policy.
    /// 4. On success every voice's age increments and the winner's resets to
    ///    zero; the winner adopts the request's priority.
    ///
    /// REF: FUN_80065978 (`_SsVoKeyOnDirect`, which consumes the chosen voice and
    /// clears its ring history). "Reserved" is [`Self::voice_held`], "envelope"
    /// [`Self::voice_env`], both kept by [`Self::read_envelopes`].
    ///
    /// Partial against the disassembly: two of the routine's tail stores are
    /// not carried - the `0x8006A7A4(0, 0xFFFFFF)` call when the winner's
    /// record flag is `2` (`0x80066d34..0x80066d4c`), and the unconditional
    /// `+0x2A` clear (`0x80066d74`), which on the drop path lands one record
    /// past the last voice.
    pub(super) fn alloc_voice(&mut self, spu: &mut Spu, req_prio: u8) -> Option<u8> {
        let n = spu.voices.len().min(crate::spu::NUM_VOICES);
        let (chosen, steal) = choose_voice(
            self.voice_held,
            &self.voice_env[..n],
            &self.voice_prio[..n],
            &self.voice_age[..n],
            req_prio,
        );
        // No idle voice and no steal candidate: every sounding voice outranks
        // the request, so the note is dropped (retail returns the out-of-range
        // voice count).
        let winner = chosen.or(steal)?;
        // The record is overwritten either way, so whatever note held the voice
        // loses it. A steal also keys off here; a free voice the silent history
        // let go of has nothing left to cut.
        let held = self.active.iter().any(|a| a.voice as usize == winner);
        self.active.retain(|a| a.voice as usize != winner);
        if chosen.is_none() || held {
            if let Some(v) = spu.voices.get_mut(winner) {
                v.key_off();
            }
            spu.record_key_off(winner);
        }
        // Post-allocation bookkeeping: age every voice, zero the winner's,
        // stamp the winner with the request's priority.
        for age in self.voice_age.iter_mut() {
            *age = age.wrapping_add(1);
        }
        self.voice_age[winner] = 0;
        self.voice_prio[winner] = req_prio;
        self.voice_held |= 1 << winner;
        self.voice_env[winner] = KEYED_ENV;
        self.forget_silence(winner);
        Some(winner as u8)
    }
}

/// The scan of `FUN_80066B00` over the voice records, as a pure function:
/// `(first idle voice, steal candidate)`. A voice is idle when it is not
/// `held` and its envelope reads `0`; the first one ends the scan. Every
/// voice the scan passes feeds the steal candidate: a priority below the
/// threshold (which starts at `req_prio` and drops to each such priority)
/// takes it, an equal one competes on the lower envelope, then on the larger
/// age, the challenger's age read signed and the incumbent's as it was
/// taken (zero-extended where a priority or envelope win took it).
///
/// PORT: FUN_80066B00 (the scan, `0x80066B3C..0x80066C80`)
pub fn choose_voice(
    held: u32,
    env: &[u16],
    prio: &[u8],
    age: &[u16],
    req_prio: u8,
) -> (Option<usize>, Option<usize>) {
    let mut threshold = req_prio as i32;
    let mut steal: Option<usize> = None;
    // Initial best-envelope sentinel is 0xFFFF, above any real ADSR level,
    // so the first voice on the threshold tier always seeds the candidate.
    let mut steal_env: u32 = 0xFFFF;
    let mut steal_age: i32 = 0;
    for v in 0..env.len() {
        let reserved = held & (1 << v) != 0;
        let e = u32::from(env[v]);
        if !reserved && e == 0 {
            // First idle voice: taken immediately, scan stops.
            return (Some(v), steal);
        }
        let pr = prio[v] as i32;
        if pr < threshold {
            threshold = pr;
            steal = Some(v);
            steal_env = e;
            steal_age = age[v] as i32;
        } else if pr == threshold {
            if e < steal_env {
                steal = Some(v);
                steal_env = e;
                steal_age = age[v] as i32;
            } else if e == steal_env {
                // Retail signedness quirk kept verbatim: the incumbent age
                // is zero-extended, the challenger sign-extended, compared
                // as i32 - an age >= 0x8000 can defend but never attack.
                let a = age[v] as i16 as i32;
                if steal_age < a {
                    steal = Some(v);
                    steal_age = a;
                }
            }
        }
    }
    (None, steal)
}

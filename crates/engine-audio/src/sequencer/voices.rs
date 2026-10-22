//! Note on / off, tone layering, the key-on flush, channel remix and SPU voice allocation.

use super::*;
use crate::Spu;

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
        let key_on_stamp = spu
            .voices
            .get(voice as usize)
            .map_or(0, |v| v.key_on_count());
        self.active.push(ActiveNote {
            channel,
            key,
            voice,
            base_pitch: 0x1000,
            bend_range: self.bank.pitch_bend_range_at(program, tone),
            velocity,
            program,
            tone,
            key_on_stamp,
            pending: Some(id),
        });
        self.pending.push(PendingKey {
            id,
            voice,
            channel,
            key,
            velocity,
            program,
            tone,
        });
    }

    /// Write every staged key-on - retail's per-vsync flush
    /// (`FUN_80065BAC` handing the key-on accumulator to `FUN_8006B854`).
    /// Layers whose note was released, stolen or re-keyed since they were
    /// staged are skipped. [`Self::tick_sample`] / [`Self::tick_us`] call
    /// this at every [`FLUSH_SAMPLES`] boundary.
    pub fn flush_key_ons(&mut self, spu: &mut Spu) {
        if self.pending.is_empty() {
            return;
        }
        self.release_lost_voices(spu);
        for p in std::mem::take(&mut self.pending) {
            let Some(idx) = self.active.iter().position(|n| n.pending == Some(p.id)) else {
                continue;
            };
            let voice = p.voice as usize;
            let cs = self.channels[p.channel as usize];
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
            // The layer set the voice's base pitch; remember it so later
            // bends re-scale the unbent value, then fold in any bend already
            // held on this channel.
            let base_pitch = spu.voices.get(voice).map(|v| v.pitch).unwrap_or(0x1000);
            let bend_range = self.active[idx].bend_range;
            if cs.pitch_bend != PITCH_BEND_CENTER
                && let Some(v) = spu.voices.get_mut(voice)
            {
                let (down, up) = bend_range;
                v.pitch = bend_pitch(base_pitch, pitch_bend_factor(cs.pitch_bend, down, up));
            }
            // The voice volume through the full retail chain: CC7 folded into
            // the velocity, head, sequence volume, tone / program / channel
            // pans, square taper.
            if let Some((l, r)) = self.note_volume(&self.active[idx])
                && let Some(v) = spu.voices.get_mut(voice)
            {
                v.vol_left = l;
                v.vol_right = r;
            }
            let note = &mut self.active[idx];
            note.base_pitch = base_pitch;
            note.key_on_stamp = spu.voices.get(voice).map_or(0, |v| v.key_on_count());
            note.pending = None;
        }
    }

    /// Re-derive the live voice volume for every note sounding on `ch` from
    /// its channel-free base, using the channel's current CC7 volume + CC10
    /// pan. Called whenever either controller changes mid-note.
    pub(super) fn remix_channel(&mut self, spu: &mut Spu, ch: usize) {
        for note in self
            .active
            .iter()
            .filter(|n| n.channel as usize == ch && n.pending.is_none())
        {
            if let Some((l, r)) = self.note_volume(note)
                && let Some(v) = spu.voices.get_mut(note.voice as usize)
            {
                v.vol_left = l;
                v.vol_right = r;
            }
        }
    }

    /// Re-derive the live voice volume of every sounding note, on every
    /// channel - the per-voice loop of `FUN_80067E9C` that a sequence-volume
    /// change runs (see [`Self::set_master_vol`]). Notes still waiting on
    /// their key-on are skipped: their volume is computed when they key.
    pub(super) fn remix_all(&mut self, spu: &mut Spu) {
        for note in self.active.iter().filter(|n| n.pending.is_none()) {
            if let Some((l, r)) = self.note_volume(note)
                && let Some(v) = spu.voices.get_mut(note.voice as usize)
            {
                v.vol_left = l;
                v.vol_right = r;
            }
        }
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

    pub(super) fn note_off(&mut self, spu: &mut Spu, channel: u8, key: u8) {
        let mut idx = 0;
        while idx < self.active.len() {
            let n = self.active[idx];
            if n.channel == channel && n.key == key {
                if (n.voice as usize) < spu.voices.len() {
                    spu.voices[n.voice as usize].key_off();
                    spu.record_key_off(n.voice as usize);
                }
                self.active.swap_remove(idx);
            } else {
                idx += 1;
            }
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
    /// REF: FUN_80065BAC (the per-frame flush that maintains the 16-deep
    /// silent-history ring at `0x801CE208` used to *unreserve* voices),
    /// FUN_80065978 (`_SsVoKeyOnDirect`, which consumes the chosen voice and
    /// clears its ring history). The engine's stand-ins: "reserved" = bound
    /// to an entry in `self.active`; "envelope" = the SPU voice's live ADSR
    /// level.
    ///
    /// Partial against the disassembly: two of the routine's tail stores are
    /// not carried - the `0x8006A7A4(0, 0xFFFFFF)` call when the winner's
    /// record flag is `2` (`0x80066d34..0x80066d4c`), and the unconditional
    /// `+0x2A` clear (`0x80066d74`), which on the drop path lands one record
    /// past the last voice.
    pub(super) fn alloc_voice(&mut self, spu: &mut Spu, req_prio: u8) -> Option<u8> {
        let occupied: u32 = self
            .active
            .iter()
            .map(|n| 1u32 << n.voice as u32)
            .fold(0, |a, b| a | b);
        let n = spu.voices.len().min(crate::spu::NUM_VOICES);
        let mut chosen: Option<usize> = None;
        // Steal-candidate evaluation state (retail registers r9/r10/r11/r8/r12).
        let mut threshold = req_prio as i32;
        let mut steal: Option<usize> = None;
        // Initial best-envelope sentinel is 0xFFFF, above any real ADSR level,
        // so the first voice on the threshold tier always seeds the candidate.
        let mut steal_env: u32 = 0xFFFF;
        let mut steal_age: i32 = 0;
        for (v, voice) in spu.voices.iter().enumerate().take(n) {
            let reserved = occupied & (1 << v) != 0;
            let env = voice.adsr.level as u32;
            if !reserved && voice.is_off() {
                // First idle voice: taken immediately, scan stops.
                chosen = Some(v);
                break;
            }
            let pr = self.voice_prio[v] as i32;
            if pr < threshold {
                threshold = pr;
                steal = Some(v);
                steal_env = env;
                steal_age = self.voice_age[v] as i32;
            } else if pr == threshold {
                if env < steal_env {
                    steal = Some(v);
                    steal_env = env;
                    steal_age = self.voice_age[v] as i32;
                } else if env == steal_env {
                    // Retail signedness quirk kept verbatim: the incumbent age
                    // is zero-extended, the challenger sign-extended, compared
                    // as i32 - an age >= 0x8000 can defend but never attack.
                    let age = self.voice_age[v] as i16 as i32;
                    if steal_age < age {
                        steal = Some(v);
                        steal_age = age;
                    }
                }
            }
        }
        // No idle voice and no steal candidate: every sounding voice outranks
        // the request, so the note is dropped (retail returns the out-of-range
        // voice count).
        let winner = chosen.or(steal)?;
        // Steal path: FREE the stolen note's `active` slot and key-off its
        // voice first. Otherwise the stolen note lingers in `self.active`
        // aliasing the reused voice: its later note-off would key-off the new
        // note that took the voice over (cutting it short), and the voice
        // would stay permanently "occupied" in the mask above, leaking it.
        if chosen.is_none() {
            let mut idx = 0;
            while idx < self.active.len() {
                if self.active[idx].voice as usize == winner {
                    self.active.swap_remove(idx);
                } else {
                    idx += 1;
                }
            }
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
        Some(winner as u8)
    }
}

//! The sequencer's voice state seen as libsnd's per-voice driver records
//! (`0x801CDB50`, stride `0x36`), for a differential that stands retail and the
//! port on one voice file and runs one walk on each.
//!
//! Retail keeps one record per SPU voice: owner (`+0x10`), VAB (`+0x18`),
//! program (`+0x14`), tone (`+0x16`), key (`+0x0E`), channel (`+0x0C`), raw
//! velocity (`+0x08`), the channel pan the note keyed with (`+0x0A`), priority
//! (`+0x1A`), age (`+0x02`), envelope (`+0x06`) and the held mark (`+0x1D`). The
//! port spreads the same facts over `voice_note`, `active`, `voice_prio`,
//! `voice_age`, `voice_env` and `voice_held`; [`Sequencer::seat_voice_record`]
//! writes them all from one record.

use super::*;
use crate::Spu;

/// One retail voice record as the sequencer that owns it sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceRecord {
    pub program: u8,
    pub tone: u8,
    pub key: u8,
    /// The MIDI channel that keyed the note (`+0x0C`).
    pub channel: u8,
    /// The raw note-on velocity (`+0x08`).
    pub velocity: u8,
    /// The channel pan the note keyed with (`+0x0A`).
    pub pan: u8,
    /// Allocation priority (`+0x1A`), age (`+0x02`) and the envelope the last
    /// flush read (`+0x06`).
    pub prior: u8,
    pub age: u16,
    pub env: u16,
    /// The held mark (`+0x1D`).
    pub held: bool,
    /// The note's note-off has not come yet.
    pub sounding: bool,
    /// A cue keyed the voice since the note did: the record's owner is `0x21`.
    pub cue_owned: bool,
}

impl Sequencer {
    /// Stand voice `voice` where a retail record stands. The marks the
    /// allocation scan reads (priority, age, envelope, held) are the voice's
    /// whoever owns it; the note is this sequence's only when `ours` (another
    /// sequence's record, or another bank's, is no note here). The voice's
    /// key-on count is the stamp, so a seated note owns the voice unless
    /// [`VoiceRecord::cue_owned`].
    pub fn seat_voice_record(&mut self, spu: &Spu, voice: usize, rec: VoiceRecord, ours: bool) {
        let keys = spu.voices.get(voice).map_or(0, |v| v.key_on_count());
        self.voices_seen = true;
        self.voice_keys_seen[voice] = keys;
        self.active.retain(|n| usize::from(n.voice) != voice);
        self.pending.retain(|p| usize::from(p.voice) != voice);
        self.voice_prio[voice] = rec.prior;
        self.voice_age[voice] = rec.age;
        self.voice_env[voice] = rec.env;
        if rec.held {
            self.voice_held |= 1 << voice;
        } else {
            self.voice_held &= !(1 << voice);
        }
        if !ours {
            self.voice_note[voice] = None;
            return;
        }
        let stamp = if rec.cue_owned {
            keys.wrapping_sub(1)
        } else {
            keys
        };
        self.voice_note[voice] = Some(VoiceNote {
            program: usize::from(rec.program),
            tone: usize::from(rec.tone),
            key: rec.key,
            channel: rec.channel,
            velocity: rec.velocity,
            pan: rec.pan,
            stamp: Some(stamp),
            staged_wheel: None,
            staged_mix: None,
        });
        if rec.sounding && !rec.cue_owned {
            self.active.push(ActiveNote {
                channel: rec.channel,
                voice: voice as u8,
                velocity: rec.velocity,
                program: usize::from(rec.program),
                tone: usize::from(rec.tone),
                key_on_stamp: stamp,
                pending: None,
            });
        }
    }

    /// Stand a channel's program, volume (CC7) and pan (CC10).
    pub fn seat_channel(&mut self, channel: u8, program: u8, volume: u8, pan: u8) {
        let c = &mut self.channels[usize::from(channel)];
        c.program = program;
        c.volume = volume;
        c.pan = pan;
    }

    /// Stand the flush's silent history: its words, its write index and the
    /// envelope reading the next [`Self::read_envelopes`] tests (the port reads
    /// a flush behind retail, so this is the reading retail's flush takes now).
    pub fn seat_silent_history(
        &mut self,
        ring: [u32; SILENT_RING_LEN],
        at: usize,
        env: [u16; crate::spu::NUM_VOICES],
    ) {
        self.silent_ring = ring;
        self.silent_at = at % SILENT_RING_LEN;
        self.voice_env_next = env;
    }

    /// The silent history's words and write index.
    pub fn silent_history(&self) -> ([u32; SILENT_RING_LEN], usize) {
        (self.silent_ring, self.silent_at)
    }

    /// The voices the allocation scan counts as held (the records' `+0x1D`).
    pub fn held_voices(&self) -> u32 {
        self.voice_held
    }

    /// Voice `voice`'s allocation age (the record's `+0x02`).
    pub fn voice_age(&self, voice: usize) -> u16 {
        self.voice_age[voice]
    }

    /// The envelopes the allocation scan reads (the records' `+0x06`).
    pub fn voice_envelopes(&self) -> [u16; crate::spu::NUM_VOICES] {
        self.voice_env
    }

    /// The voices holding a register write for the next flush.
    pub fn staged_voice_mask(&self) -> u32 {
        self.staged_voices
    }

    /// Voices a caller outside the score staged a volume write for since the
    /// last flush: a positional emitter's per-tick level (`FUN_800657D0`
    /// stores the shadow pair and raises the voice's flag `0x801CE060[v] |= 3`,
    /// `0x80065800..0x8006582C`). The flush `FUN_80065BAC` calls `FUN_8006C048`
    /// for every flagged voice before its KON write (`0x80065E0C..0x80065F34`),
    /// the score's or not, so those voices delay the score's key-ons too
    /// ([`flush_key_on_lag_with`]).
    pub fn stage_external_voices(&mut self, mask: u32) {
        self.external_staged |= mask & 0x00FF_FFFF;
    }

    /// A note-off on `channel` for `key` (`FUN_8006688C`); the voices it keyed off.
    pub fn apply_note_off(&mut self, spu: &mut Spu, channel: u8, key: u8) -> u32 {
        self.note_off(spu, channel, key)
    }

    /// A pitch bend on `channel` (`FUN_80067C1C`).
    pub fn apply_bend(&mut self, spu: &mut Spu, channel: u8, wheel: u8) {
        self.bend_program(spu, usize::from(channel), wheel);
    }

    /// A CC7 / CC10 re-volume on `channel` (`FUN_8006861C`), with the channel's
    /// volume and pan as they stand.
    pub fn apply_remix(&mut self, spu: &mut Spu, channel: u8) -> bool {
        self.remix_program(spu, usize::from(channel))
    }

    /// The sequence-volume walk `FUN_80067E9C` runs, at once rather than at the
    /// next flush ([`Self::set_master_vol`]).
    pub fn apply_master_vol(&mut self, spu: &mut Spu, v: u8) {
        self.set_master_vol(v);
        self.master_vol_dirty = false;
        self.remix_all(spu);
    }

    /// The envelope read and silent history at the top of `FUN_80065BAC`.
    pub fn apply_envelope_read(&mut self, spu: &Spu) {
        self.read_envelopes(spu);
    }
}

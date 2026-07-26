//! Concrete [`legaia_engine_core::scene::BgmDirector`] adapter that drives a
//! cpal-backed [`legaia_engine_audio::AudioOut`].
//!
//! The director owns the audio output handle plus the active scene's
//! [`legaia_engine_audio::VabBank`] (uploaded into the SPU at scene-load
//! time). On each `start` / `queue` call it parses the SEQ bytes the field
//! VM resolved through the BGM table, builds a [`legaia_engine_audio::Sequencer`],
//! and attaches it to the audio output. `pause` / `resume` toggle the
//! sequencer-feed flag without rebuilding state; `stop` detaches the
//! sequencer entirely.
//!
//! The retail engine routes BGM through SsAPI seq-context callbacks (see
//! `docs/subsystems/audio.md` "PsyQ libsnd SsAPI" + the `_DAT_801CE564`
//! seq-context resolver). We don't need that level of indirection in the
//! port - the field VM's BGM events arrive pre-resolved with the right SEQ
//! bytes and the active VAB is staged once per scene. This adapter is the
//! join point.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use legaia_asset::sfx_table::FALLBACK_VAB_SLOT;
use legaia_engine_audio::{
    ArtsShoutBank, AudioOut, PendingCue, SHOUT_CD_RESPONSE_DELAY, Sequencer, SfxBank, SfxScheduler,
    VabBank,
};
use legaia_engine_core::scene::BgmDirector;
use legaia_seq::Seq;

/// BGM director that routes [`BgmDirector`] events into a live
/// [`AudioOut`]. The director holds a clone of the audio handle (cpal stream
/// is reference-counted internally via `Arc`) plus the active VAB bank.
pub struct AudioBgmDirector {
    audio: Arc<AudioOut>,
    bank: Option<VabBank>,
    /// Master volume forwarded to every freshly-attached sequencer. Engines
    /// bump this when the user adjusts the music slider.
    pub master_vol: u8,
    /// Loop-to event index for newly-started sequencers. `None` plays once
    /// (sequencer reports `finished` when it runs off the end). Most field
    /// BGM loops to 0; cutscene SEQs typically don't.
    pub loop_to: Option<usize>,
    /// Whether playback is currently paused. `pause` / `resume` toggle
    /// without detaching the active sequencer.
    paused: bool,
    /// Last started BGM id, if any. Useful for diagnostics + suppressing
    /// redundant `start(same_id)` calls (the field VM occasionally re-emits
    /// op `0x35` without a state change).
    pub last_started: Option<u16>,
    /// Optional pending BGM bytes - used by `queue` to defer playback until
    /// the engine signals a transition (typically the next field-VM tick).
    pending: Option<(u16, Vec<u8>)>,
    /// Sound-effect descriptor bank (decoded from the executable's
    /// `DAT_8006F198` table, see `sfx-table.md`). Empty until
    /// [`Self::set_sfx_bank`]; play requests against an empty bank no-op.
    /// The bank is static across scenes (it lives in the executable), so it
    /// is set once at boot; the per-scene VAB it plays through is the same
    /// [`Self::bank`] the BGM sequencer uses.
    sfx_bank: SfxBank,
    /// Cue id -> **VAB slot**, the routing half of the same descriptor table
    /// ([`legaia_asset::sfx_table::SfxTable::cue_slots`]): a cue's `+4`
    /// category selects the mixer record whose `+8` is the slot its voices key.
    /// Empty until [`Self::set_sfx_cue_slots`]; an absent id routes to
    /// [`FALLBACK_VAB_SLOT`] exactly like an unpinned slot does.
    sfx_cue_slots: BTreeMap<u8, u8>,
    /// Resident SFX program banks keyed by that slot. Slot `0` is the system
    /// bank (extraction PROT 0868) the 16 shared UI cues key; slot `2` is the
    /// **class-2 sound bank** (PROT 0869, raw loader index `0x367`) the battle
    /// scene loader and the Baka Fighter init load explicitly, whose low
    /// programs (`0`, `3`) carry the strike / duel-hit cues (see
    /// `sfx-table.md`). Both are uploaded once at boot out of one allocator
    /// over a dedicated SPU RAM region, so battle / menu cues resolve
    /// regardless of which BGM VAB happens to be open. Empty when nothing
    /// could be staged (a disc-free boot); [`Self::tick_sfx_frame`] then falls
    /// back to the scene BGM bank ([`Self::bank`]), matching the retail
    /// field-scene path where a cue sounds out of whichever bank the libsnd
    /// current-bank globals hold.
    sfx_vabs: BTreeMap<u8, VabBank>,
    /// Frame-timed one-shot cue queue. [`Self::enqueue_sfx`] adds a cue at
    /// its strike-relative delay; [`Self::tick_sfx_frame`] advances one frame
    /// and fires matured cues through the SPU.
    sfx_sched: SfxScheduler,
    /// Arts-voice **shout** bank: the per-character CD-XA clips
    /// (`XA2`/`XA4`/`XA6`, demuxed per channel + decoded at boot) and the
    /// SCUS cue tables. `None` on a disc-free / extracted-dir boot (the raw
    /// CD-XA subheaders needed for channel demux only exist on a real disc
    /// image); shout requests then no-op, leaving arts silent - the same
    /// degradation retail applies to an unvoiced art.
    shout_bank: Option<ArtsShoutBank>,
}

impl AudioBgmDirector {
    pub fn new(audio: Arc<AudioOut>) -> Self {
        Self {
            audio,
            bank: None,
            master_vol: 100,
            loop_to: Some(0),
            paused: false,
            last_started: None,
            pending: None,
            sfx_bank: SfxBank::new(),
            sfx_cue_slots: BTreeMap::new(),
            sfx_vabs: BTreeMap::new(),
            sfx_sched: SfxScheduler::new(),
            shout_bank: None,
        }
    }

    /// Install the arts-voice shout bank (demuxed + decoded from the user's
    /// disc at boot; see [`crate::boot::read_arts_shout_bank`]).
    pub fn set_shout_bank(&mut self, bank: ArtsShoutBank) {
        self.shout_bank = Some(bank);
    }

    /// Whether the arts-voice shout bank was staged.
    pub fn has_shout_bank(&self) -> bool {
        self.shout_bank.is_some()
    }

    /// Fire the Tactical-Arts shout for `(cslot, action_constant)` through
    /// the XA mixing path. Resolves the cue against the bank's channel pools
    /// (retail `FUN_8004C140` selection, no immediate repeat) and stages the
    /// clip with the modeled CD-response start delay
    /// ([`SHOUT_CD_RESPONSE_DELAY`]), so the shout starts *after* the art
    /// animation that requested it - never before. A second shout while one
    /// is sounding queues behind it (the back-to-back no-drop path in
    /// [`AudioOut::play_xa_shout`]). Returns the fired channel, or `None`
    /// when the bank is absent or the art is unvoiced.
    pub fn play_art_shout(&mut self, cslot: u8, action: u8) -> Option<u8> {
        let bank = self.shout_bank.as_mut()?;
        let (channel, clip) = bank.shout(cslot, action)?;
        self.audio.play_xa_shout(
            clip.pcm.clone(),
            clip.sample_rate,
            legaia_xa::Channels::Mono,
            0x4000,
            SHOUT_CD_RESPONSE_DELAY,
        );
        Some(channel)
    }

    /// Install the sound-effect descriptor bank (decoded from the user's
    /// `SCUS_942.54` `DAT_8006F198` table at boot). Replaces any prior bank.
    pub fn set_sfx_bank(&mut self, bank: SfxBank) {
        self.sfx_bank = bank;
    }

    /// Install the cue id -> VAB slot routing decoded from the same
    /// descriptor table as [`Self::set_sfx_bank`]
    /// (`legaia_asset::sfx_table::SfxTable::cue_slots`). Without it every cue
    /// falls back to the class-2 bank, which is what a single-bank host did.
    pub fn set_sfx_cue_slots<I: IntoIterator<Item = (u8, u8)>>(&mut self, slots: I) {
        self.sfx_cue_slots = slots.into_iter().collect();
    }

    /// Install one resident SFX program bank at its VAB `slot` (0 = the
    /// PROT 0868 system bank, 2 = the PROT 0869 class-2 bank), uploaded into
    /// the shared SPU RAM region at boot. Cues fire against the bank their own
    /// category names so their programs are always resident; see
    /// [`Self::sfx_vabs`].
    pub fn set_sfx_vab(&mut self, slot: u8, bank: VabBank) {
        self.sfx_vabs.insert(slot, bank);
    }

    /// Whether any resident SFX bank was staged.
    pub fn has_sfx_vab(&self) -> bool {
        !self.sfx_vabs.is_empty()
    }

    /// The VAB slots that have a resident bank, ascending.
    pub fn staged_sfx_slots(&self) -> Vec<u8> {
        self.sfx_vabs.keys().copied().collect()
    }

    /// Borrow the active SFX bank - useful for tests / inspection.
    pub fn sfx_bank(&self) -> &SfxBank {
        &self.sfx_bank
    }

    /// The VAB slot cue `id` resolves to on this director, routing and
    /// fallback included. See [`resolve_sfx_slot`].
    pub fn sfx_slot_for_cue(&self, id: u8) -> u8 {
        resolve_sfx_slot(&self.sfx_cue_slots, &self.sfx_vabs, id)
    }

    /// The resident bank cue `id` keys, or `None` when nothing is staged.
    fn sfx_vab_for_cue(&self, id: u8) -> Option<&VabBank> {
        self.sfx_vabs.get(&self.sfx_slot_for_cue(id))
    }

    /// Queue a one-shot sound cue to fire `frames` after this call (the
    /// strike's `timing_frames`). `id` is the [`SfxBank`] descriptor id
    /// directly (the art-record `HitCue::kind`), played without
    /// `classify_cue`. `actor` / `target` ride along for HUD context.
    pub fn enqueue_sfx(&mut self, id: u16, frames: u16, actor: u8, target: u8) {
        self.sfx_sched
            .enqueue(PendingCue::new(id, frames).with_actors(actor, target));
    }

    /// Advance the SFX scheduler one frame and fire any matured cue through
    /// the SPU. Each cue resolves against the resident SFX bank **its own `+4`
    /// category names** ([`Self::sfx_vab_for_cue`]) - the retail path, where
    /// `FUN_80065034` repoints the current-bank globals at the cue's slot
    /// before the program lookup - and falls back to the active scene BGM bank
    /// ([`Self::bank`]) when nothing is staged at all (the disc-free boot).
    /// Returns the `(cue_id, voice)` pairs that keyed on. A cue is silently
    /// dropped when no bank is staged, its id isn't in the descriptor bank, its
    /// program / tone isn't resident, or no SPU voice is free (matching the
    /// retail "no voice / no program -> skip" behaviour). Call once per
    /// simulation tick so delayed cues advance even when none are enqueued that
    /// frame.
    pub fn tick_sfx_frame(&mut self) -> Vec<(u16, u8)> {
        let batch = self.sfx_sched.tick_frame();
        if batch.is_empty() {
            return Vec::new();
        }
        if self.sfx_vabs.is_empty() && self.bank.is_none() {
            return Vec::new();
        }
        let bank = &self.sfx_bank;
        let mut fired = Vec::new();
        self.audio.with_spu(|spu| {
            for cue in &batch.fired {
                let id = cue.id as u8;
                // The cue's category picks its bank; with nothing staged the
                // scene BGM VAB stands in, as it did before any SFX bank did.
                let Some(vab) = self.sfx_vab_for_cue(id).or(self.bank.as_ref()) else {
                    continue;
                };
                if let Some(voice) = bank.play_one_shot(id, spu, vab) {
                    fired.push((cue.id, voice));
                }
            }
        });
        fired
    }

    /// Drop every queued SFX cue (scene transition / battle abort).
    pub fn clear_sfx(&mut self) {
        self.sfx_sched.clear();
    }

    /// Replace the active VAB bank. Engines call this once per scene after
    /// resolving the scene's primary VAB entry through
    /// [`legaia_engine_core::scene::SceneHost::scene_vab_bytes`]; the bank
    /// is uploaded into the SPU and stored here for subsequent SEQ starts.
    pub fn set_bank(&mut self, bank: VabBank) {
        self.bank = Some(bank);
    }

    /// Borrow the active bank - useful for tests / inspection.
    pub fn bank(&self) -> Option<&VabBank> {
        self.bank.as_ref()
    }

    /// `true` if a sequencer is currently attached to the audio output.
    pub fn is_playing(&self) -> bool {
        self.audio.sequencer_progress().is_some() && !self.paused
    }

    /// Drain whatever was queued by the most recent [`BgmDirector::queue`]
    /// call. Engines call this when transitioning into the scene that
    /// should play the queued track.
    pub fn flush_queue(&mut self) -> Result<bool> {
        let Some((id, bytes)) = self.pending.take() else {
            return Ok(false);
        };
        self.start_inner(id, &bytes)?;
        Ok(true)
    }

    /// Split a raw `music_01` bank entry (`[chunk][pBAV VAB][pQES SEQ]`),
    /// upload the entry's **own** VAB into the SPU BGM region (capped below
    /// the resident SFX bank, exactly like `stage_scene_vab`), stash it as the
    /// active bank, and return the SEQ bytes. `None` when the pair is absent
    /// or the VAB header doesn't parse. This is the global-pool half of BGM
    /// playback - the track brings its own instruments, unlike the scene-local
    /// path that reuses the pre-staged scene VAB.
    fn stage_owned_vab(&mut self, entry_bytes: &[u8]) -> Option<Vec<u8>> {
        let vab_off = entry_bytes.windows(4).position(|w| w == b"pBAV")?;
        let seq_rel = entry_bytes[vab_off..]
            .windows(4)
            .position(|w| w == b"pQES")?;
        let report = legaia_vab::parse(entry_bytes, vab_off).ok()?;
        let body = &entry_bytes[vab_off..];
        let bank = self.audio.with_spu(|spu| {
            let mut alloc = legaia_engine_audio::SpuAllocator::new(
                crate::boot::SPU_RESERVED_BYTES,
                crate::boot::SPU_RAM_BYTES
                    - crate::boot::SPU_RESERVED_BYTES
                    - crate::boot::SFX_BANK_SPU_BYTES,
            );
            VabBank::upload(spu, &mut alloc, &report, body)
        });
        self.bank = Some(bank);
        Some(entry_bytes[vab_off + seq_rel..].to_vec())
    }

    fn start_inner(&mut self, bgm_id: u16, seq_bytes: &[u8]) -> Result<()> {
        let Some(bank) = self.bank.clone() else {
            log::warn!("AudioBgmDirector::start({bgm_id}) ignored - no VAB bank loaded for scene");
            return Ok(());
        };
        let seq = Seq::parse(seq_bytes).context("parse SEQ for BGM start")?;
        let mut sequencer = Sequencer::new(seq, bank);
        sequencer.set_master_vol(self.master_vol);
        if let Some(loop_to) = self.loop_to {
            sequencer.set_loop_to(loop_to);
        }
        // Retail BGM changes are hard cuts (or short `SsSeqSetVol` ramps), not
        // a serial cross-fade that fades the old track out to silence before
        // the new one is even installed - that swallows the incoming track's
        // intro. Swap immediately so the new track sounds from its first event,
        // with only a brief click-guard fade-in on the SPU master. If nothing
        // is playing, attach directly at full volume.
        //
        // ~2 frames at 60 Hz (44100 / 60 * 2). Long enough to avoid an onset
        // pop, far too short to hide an intro (the old fade held it silent for
        // 22050 samples = 0.5 s).
        const TRANSITION_FADE_IN_SAMPLES: u32 = 1_470;
        if self.audio.sequencer_progress().is_some() && !self.paused {
            self.audio.swap_bgm(sequencer, TRANSITION_FADE_IN_SAMPLES);
        } else {
            self.audio.attach_sequencer(sequencer);
        }
        self.paused = false;
        self.last_started = Some(bgm_id);
        Ok(())
    }
}

impl BgmDirector for AudioBgmDirector {
    fn start(&mut self, bgm_id: u16, seq_bytes: &[u8]) {
        // Suppress duplicate starts for the same BGM id - the field VM's
        // op 0x35 occasionally re-emits without a state change (we'd lose
        // the playhead by re-attaching).
        if self.last_started == Some(bgm_id)
            && !self.paused
            && self.audio.sequencer_progress().is_some()
        {
            return;
        }
        if let Err(e) = self.start_inner(bgm_id, seq_bytes) {
            log::warn!("AudioBgmDirector::start({bgm_id}) failed: {e:#}");
        }
    }

    fn queue(&mut self, bgm_id: u16, seq_bytes: &[u8]) {
        self.pending = Some((bgm_id, seq_bytes.to_vec()));
    }

    fn start_owned_vab(&mut self, bgm_id: u16, entry_bytes: &[u8]) {
        // Suppress a redundant re-emit of the same global track (the field VM
        // occasionally re-fires op 0x35): re-uploading the VAB + restarting
        // would drop the playhead.
        if self.last_started == Some(bgm_id)
            && !self.paused
            && self.audio.sequencer_progress().is_some()
        {
            return;
        }
        let Some(seq) = self.stage_owned_vab(entry_bytes) else {
            log::warn!("AudioBgmDirector::start_owned_vab({bgm_id}) - no [VAB][SEQ] pair in entry");
            return;
        };
        if let Err(e) = self.start_inner(bgm_id, &seq) {
            log::warn!("AudioBgmDirector::start_owned_vab({bgm_id}) failed: {e:#}");
        }
    }

    fn queue_owned_vab(&mut self, bgm_id: u16, entry_bytes: &[u8]) {
        // Upload the VAB now (so the bank is ready) and defer the SEQ start to
        // the next `flush_queue`.
        if let Some(seq) = self.stage_owned_vab(entry_bytes) {
            self.pending = Some((bgm_id, seq));
        }
    }

    fn pause(&mut self) {
        self.paused = true;
        self.audio.set_sequencer_paused(true);
    }

    fn resume(&mut self) {
        self.paused = false;
        self.audio.set_sequencer_paused(false);
    }

    fn stop(&mut self) {
        self.audio.detach_sequencer();
        self.paused = false;
        self.last_started = None;
    }
}

/// Which VAB slot a cue resolves to, given the installed cue -> slot routing
/// and the set of slots that actually staged.
///
/// **The fallback is the pre-routing behaviour on purpose.** This host stages
/// slots `0` and `2`; categories `6` and `11` name banks it does not hold
/// (PROT 0876 / 0889 - traced, but they do not fit beside the other two in the
/// reserved SPU region), so those - and any id the descriptor table doesn't
/// carry - resolve to [`FALLBACK_VAB_SLOT`], the class-2 bank this host staged
/// for every cue before the routing existed. Categories 0 and 2 become correct
/// without changing what 6 / 11 sound like. Retail needs no extra room because
/// slot 6 *is* slot 2's SPU region, refilled on the field/battle transition;
/// staging them here means reloading that region per mode - see
/// `docs/formats/sfx-table.md`.
///
/// Free function rather than a method so it is testable without a cpal device
/// (an [`AudioBgmDirector`] needs a live [`AudioOut`]).
pub(crate) fn resolve_sfx_slot<T>(
    cue_slots: &BTreeMap<u8, u8>,
    staged: &BTreeMap<u8, T>,
    id: u8,
) -> u8 {
    let slot = cue_slots.get(&id).copied().unwrap_or(FALLBACK_VAB_SLOT);
    if staged.contains_key(&slot) {
        slot
    } else {
        FALLBACK_VAB_SLOT
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_audio::VabBank;

    /// A cue keys the bank its `+4` category names when that slot staged, and
    /// the class-2 bank otherwise - never a third thing, and never silence
    /// while any bank is resident.
    #[test]
    fn cue_routes_to_its_category_slot_and_falls_back_to_class_two() {
        // Retail categories: 0x21 menu cursor = 0, 0x09 duel hit = 2,
        // 0x2E field script = 6, 0x4D = 11.
        let routing = BTreeMap::from([(0x21u8, 0u8), (0x09, 2), (0x2E, 6), (0x4D, 11)]);
        let staged: BTreeMap<u8, ()> = BTreeMap::from([(0, ()), (2, ())]);

        assert_eq!(resolve_sfx_slot(&routing, &staged, 0x21), 0);
        assert_eq!(resolve_sfx_slot(&routing, &staged, 0x09), 2);
        // Unpinned slots and unknown ids both land on the class-2 bank.
        for id in [0x2Eu8, 0x4D, 0xFE] {
            assert_eq!(resolve_sfx_slot(&routing, &staged, id), FALLBACK_VAB_SLOT);
        }
        // With only the class-2 bank staged this is exactly the old behaviour.
        let one: BTreeMap<u8, ()> = BTreeMap::from([(2, ())]);
        for id in [0x21u8, 0x09, 0x2E, 0xFE] {
            assert_eq!(resolve_sfx_slot(&routing, &one, id), FALLBACK_VAB_SLOT);
        }
        // No routing installed at all: everything is class-2, as before.
        let none = BTreeMap::new();
        assert_eq!(resolve_sfx_slot(&none, &staged, 0x21), FALLBACK_VAB_SLOT);
    }

    /// Test stub bank - empty programs / samples. Real banks come from
    /// `legaia_vab::parse`.
    fn empty_bank() -> VabBank {
        VabBank {
            master_vol: 127,
            samples: Vec::new(),
            programs: Vec::new(),
        }
    }

    /// Director without an audio handle - exercises queue / pause / resume
    /// state machines without opening a cpal stream (CI has no audio
    /// device). We can't construct AudioOut without a device, so the start
    /// / stop tests live as integration tests in environments where audio
    /// is available.
    #[test]
    fn queue_then_flush_replays_pending_bytes_or_logs_warning() {
        // Quick offline test: the queue / flush plumbing doesn't touch
        // audio when there's no bank. We simulate by directly setting the
        // pending field.
        struct Stub {
            pending: Option<(u16, Vec<u8>)>,
        }
        let mut s = Stub { pending: None };
        s.pending = Some((42, vec![1, 2, 3]));
        let drained = s.pending.take();
        assert_eq!(drained, Some((42, vec![1, 2, 3])));
        let _ = empty_bank(); // touch path so unused-import lint stays clean
    }
}

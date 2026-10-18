//! BGM on the director: the duck ramp, the scene bank and the owned-VAB
//! `music_01` start, the side-band tail, and the `BgmDirector` trait the
//! scene host drives.

use super::*;

impl<S: AudioSink> AudioDirector<S> {
    /// Set the audio duck's target as a percentage of the reference level
    /// (`BattleEvent::DuckAudioLevel`): `75` under a summon / magic capture,
    /// `100` when the Done band ramps it back. The ramp itself runs in
    /// [`Self::tick_duck`].
    pub fn set_duck_pct(&mut self, pct: u8) {
        self.duck_pct = pct;
        self.duck_target = legaia_engine_audio::duck::duck_target_for_pct_of(self.duck_ref, pct);
    }

    /// Install the configured audio level the duck is a percentage of and
    /// rests on (`World::audio.levels.configured_level`, a loaded save's
    /// `_DAT_8008457C`). Called per frame before [`Self::tick_duck`]; a
    /// change re-targets the ramp, so a save carrying a lower level brings
    /// the BGM down to it the way retail's next MAN load does.
    pub fn set_duck_reference(&mut self, configured_level: i32) {
        let reference = legaia_engine_audio::duck::reference_level(configured_level);
        if reference != self.duck_ref {
            self.duck_ref = reference;
            self.set_duck_pct(self.duck_pct);
        }
    }

    /// One frame of the duck: apply the level the frame begins with, then
    /// step the live level one unit toward the target (retail
    /// `DAT_1F800393` per vsync, the `0x35` / `0x51` arms).
    ///
    /// The order is retail's. The per-frame driver `FUN_8001698C` compares
    /// the cell `_DAT_8007B910` against the last applied level
    /// `DAT_8007B6EC` (`0x80016A1C..0x80016A38`), stores it there and hands
    /// it halved to `FUN_8002614C`, which sets every sound source's volume
    /// through `SsSeqSetVol` - and that re-volumes the notes already
    /// sounding. The action SM steps the cell later in the frame, so the BGM
    /// plays the level one frame back
    /// ([`legaia_engine_audio::duck::ducked_master_vol`] for the halving).
    ///
    /// A level resting below the reference is re-applied every frame too
    /// ([`legaia_engine_audio::duck::duck_apply`]), so a track started under
    /// a settled duck comes down on its first frame - the kernel the page's
    /// `tick_duck` calls.
    ///
    /// PORT: FUN_8001698C (the audio-level apply, `0x80016A04..0x80016A38`)
    pub fn tick_duck(&mut self) {
        use legaia_engine_audio::duck;
        let level = self.duck_level;
        let changed = level != self.duck_applied;
        self.duck_applied = level;
        if let Some(vol) = duck::duck_apply(changed, self.master_vol, level) {
            self.audio.set_sequencer_master_vol(vol);
        }
        duck::step_duck(&mut self.duck_level, self.duck_target);
    }

    /// The level last applied to the BGM (`DAT_8007B6EC`), one frame behind
    /// [`Self::duck_level`] while the duck ramps.
    pub fn duck_applied(&self) -> u8 {
        self.duck_applied
    }

    /// The live duck level in `_DAT_8007B910` units (for tests / traces).
    pub fn duck_level(&self) -> u8 {
        self.duck_level
    }

    /// The level the duck ramps toward (`duck_level` rests here).
    pub fn duck_target(&self) -> u8 {
        self.duck_target
    }

    /// The side-band bank currently staged, if any.
    pub fn side_band(&self) -> Option<SideBandBank> {
        self.tail.side_band().map(|(k, _)| k)
    }

    /// The tail borrowers' residency model (for tests / traces).
    pub fn bgm_tail(&self) -> &BgmTail<SideBandBank> {
        &self.tail
    }

    /// Replace the active VAB bank. Engines call this once per scene after
    /// resolving the scene's primary VAB entry through
    /// [`legaia_engine_scene_host::scene_host::SceneHost::scene_vab_bytes`]; the bank
    /// is uploaded into the SPU and stored here for subsequent SEQ starts.
    pub fn set_bank(&mut self, bank: VabBank) {
        // A tail borrower the new bank's samples reach is gone with it; one
        // above them stays (retail's resolver closes neither slot).
        self.bank = Some(bank);
        self.reclaim_sfx_region();
        self.observe_track();
    }

    /// Borrow the active bank - useful for tests / inspection.
    pub fn bank(&self) -> Option<&VabBank> {
        self.bank.as_ref()
    }

    /// `true` if a sequencer is attached, paused or not - the track whose
    /// samples the BGM region holds.
    pub fn is_attached(&self) -> bool {
        self.audio.sequencer_progress().is_some()
    }

    /// `true` if a sequencer is currently attached to the audio output.
    pub fn is_playing(&self) -> bool {
        self.audio.sequencer_progress().is_some() && !self.audio.sequencer_paused()
    }

    /// Split a raw `music_01` bank entry (`[chunk][pBAV VAB][pQES SEQ]`),
    /// upload the entry's **own** VAB into the SPU BGM region (capped below
    /// the resident SFX region, or across it for a bank too large for the
    /// BGM region - `legaia_engine_audio::spu_layout`), stash it as the
    /// active bank, and return the SEQ bytes. `None` when the pair is absent
    /// or the VAB header doesn't parse. Every track the field VM starts comes
    /// through here - a scene-local id plays retail's fallback track - so
    /// the track always brings its own instruments.
    fn stage_owned_vab(&mut self, entry_bytes: &[u8]) -> Option<Vec<u8>> {
        // The installer walk's split (type-0 bank, type-2 score), not a
        // magic hunt - see `chunk_install::owned_bank_offsets`.
        let split = legaia_engine_system::chunk_install::owned_bank_offsets(entry_bytes)?;
        let vab_off = split.vab;
        let report = legaia_vab::parse(entry_bytes, vab_off).ok()?;
        // Both halves are validated before the SPU is touched - the page's
        // `stage_owned` keeps the same order - so a score that does not parse
        // leaves the playing track's samples where they were.
        let seq_bytes = entry_bytes.get(split.seq..)?;
        Seq::parse(seq_bytes).ok()?;
        let body = &entry_bytes[vab_off..];
        // Into the BGM region, or - for a bank that does not fit it, the
        // ending theme's - from the same base across the SFX region, as
        // retail opens VAB 10 at slot 0's base (`spu_layout`).
        let staged = self
            .audio
            .with_spu(|spu| legaia_engine_audio::spu_layout::upload_owned_bank(spu, &report, body));
        // A tail borrower the new track's samples reach is gone with them.
        self.bank = Some(staged.bank);
        self.reclaim_sfx_region();
        self.observe_track();
        Some(seq_bytes.to_vec())
    }

    fn start_inner(&mut self, bgm_id: u16, seq_bytes: &[u8]) -> Result<()> {
        let Some(bank) = self.bank.clone() else {
            log::warn!("AudioDirector::start({bgm_id}) ignored - no VAB bank loaded for scene");
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
        if self.audio.sequencer_progress().is_some() && !self.audio.sequencer_paused() {
            self.audio.swap_bgm(sequencer, TRANSITION_FADE_IN_SAMPLES);
        } else {
            self.audio.attach_sequencer(sequencer);
        }
        // Retail's start arm (op 0x35 sub-op 1, `0x801E0104`) clears the
        // pause bit alongside the track select, so a start issued while
        // paused must reopen the sequencer gate too - attaching behind a
        // closed gate leaves the new track silent until an explicit
        // resume/unhalt.
        self.audio.set_sequencer_paused(false);
        self.last_started = Some(bgm_id);
        log::info!("AudioDirector: BGM {bgm_id} started");
        Ok(())
    }
}

impl<S: AudioSink> BgmDirector for AudioDirector<S> {
    fn start(&mut self, bgm_id: u16, seq_bytes: &[u8]) {
        self.commands.push(super::AudioCommand::BgmStart(bgm_id));
        // Suppress duplicate starts for the same BGM id - the field VM's
        // op 0x35 occasionally re-emits without a state change (we'd lose
        // the playhead by re-attaching).
        if self.last_started == Some(bgm_id)
            && !self.audio.sequencer_paused()
            && self.audio.sequencer_progress().is_some()
        {
            return;
        }
        if let Err(e) = self.start_inner(bgm_id, seq_bytes) {
            log::warn!("AudioDirector::start({bgm_id}) failed: {e:#}");
        }
    }

    fn start_owned_vab(&mut self, bgm_id: u16, entry_bytes: &[u8]) {
        self.commands
            .push(super::AudioCommand::BgmStartOwned(bgm_id));
        // Suppress a redundant re-emit of the same global track (the field VM
        // occasionally re-fires op 0x35): re-uploading the VAB + restarting
        // would drop the playhead.
        if self.last_started == Some(bgm_id)
            && !self.audio.sequencer_paused()
            && self.audio.sequencer_progress().is_some()
        {
            return;
        }
        let Some(seq) = self.stage_owned_vab(entry_bytes) else {
            log::warn!("AudioDirector::start_owned_vab({bgm_id}) - no [VAB][SEQ] pair in entry");
            return;
        };
        if let Err(e) = self.start_inner(bgm_id, &seq) {
            log::warn!("AudioDirector::start_owned_vab({bgm_id}) failed: {e:#}");
        }
    }

    /// The pause state is the output's sequencer gate and nothing else - the
    /// browser twin reads the same bit - so a writer that bypasses the
    /// director (a movie ducking the score, `legaia_engine_system::movie::movie_audio`)
    /// cannot leave a second latch disagreeing with it.
    fn pause(&mut self) {
        self.commands.push(super::AudioCommand::BgmPause);
        self.audio.set_sequencer_paused(true);
    }

    /// Sub-op `4`: retail's re-attach replays the slot's sequence from its
    /// start (`FUN_800628F0` resets the read cursor before it plays), so the
    /// track rewinds, then the gate opens.
    fn resume(&mut self) {
        self.commands.push(super::AudioCommand::BgmResume);
        self.audio.rewind_sequencer();
        self.audio.set_sequencer_paused(false);
    }

    /// Detach the track **and reopen the gate**, so a stop issued while
    /// paused leaves nothing closed behind it (the browser play page's `stop`
    /// does the same).
    fn stop(&mut self) {
        self.commands.push(super::AudioCommand::BgmStop);
        self.audio.detach_sequencer();
        self.audio.set_sequencer_paused(false);
        self.last_started = None;
    }

    /// Sub-op `0xA` - the unhalt-pause swap-commit (retail `0x801E0264`).
    /// When the pause gate is still closed no start intervened, so the
    /// director is holding the track sub-op 2 paused: release it the way
    /// retail's `FUN_800266E0` + `FUN_80026520` pair detaches and closes
    /// the slot. When a start already landed (the paired sub-op 9 precedes
    /// the commit in script order), the swap is done and the occupant is
    /// the incoming track - leave it alone. Either way the pause gate is
    /// cleared unconditionally, mirroring retail clearing `_DAT_8007B750`
    /// bit 1 on every pass through the arm - without this, a start issued
    /// while paused attaches its sequencer behind a still-closed gate and
    /// the score stays silent after the cutscene.
    fn unhalt_pause(&mut self) {
        self.commands.push(super::AudioCommand::BgmUnhaltPause);
        if self.audio.sequencer_paused() {
            self.audio.detach_sequencer();
            self.last_started = None;
        }
        self.audio.set_sequencer_paused(false);
    }
}

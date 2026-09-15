//! The play page's BGM director - the browser twin of the native
//! `AudioBgmDirector` (`crates/engine-shell/src/bgm.rs`). Implements
//! `legaia_engine_core::scene::BgmDirector` over the live `WebAudioOut`, so
//! the field VM's op-`0x35` events start / pause / stop the same tracks on
//! both hosts.

#[cfg(target_arch = "wasm32")]
use legaia_engine_audio::WebAudioOut;

/// A [`legaia_engine_core::scene::BgmDirector`] that routes the field VM's
/// op-`0x35` music events into a [`WebAudioOut`]: the browser twin of the
/// native `AudioBgmDirector`. Borrows the runtime's audio handle plus its
/// scene-local BGM bank + dedupe latch for the duration of one routing pass.
///
/// Scene-local starts (`bgm_id < 2000`) play their SEQ through the pre-staged
/// scene bank (`bank`); global-pool tracks (`>= 2000`) carry their own
/// `[chunk][pBAV VAB][pQES SEQ]` and upload it before playing - the path most
/// real Legaia music takes. Both loop to the start and land through
/// [`Self::play`]'s immediate swap.
#[cfg(target_arch = "wasm32")]
pub(crate) struct WebBgmDirector<'a> {
    pub(crate) out: &'a WebAudioOut,
    pub(crate) bank: &'a mut Option<legaia_engine_audio::VabBank>,
    pub(crate) last_started: &'a mut Option<u16>,
}

#[cfg(target_arch = "wasm32")]
impl WebBgmDirector<'_> {
    /// Install a freshly-built, looping sequencer and let it sound from its
    /// own first event, behind a click-guard ramp of ~2 frames at the SPU's
    /// 44.1 kHz rate - the same `swap_bgm` call, with the same constant, the
    /// native `AudioBgmDirector::start_inner` makes.
    ///
    /// Not `crossfade_to`. That one is a serial fade: with a track already
    /// playing it parks the incoming sequencer in `pending_seq` and rolls the
    /// outgoing one down to silence first, so the new track has not begun a
    /// fade-length after the script asked for it. Retail BGM changes are hard
    /// cuts, and a cutscene sting is mostly intro - half a second of the old
    /// track fading is the whole hook gone.
    fn play(&mut self, bgm_id: u16, seq: legaia_seq::Seq, bank: legaia_engine_audio::VabBank) {
        // ~2 frames at 60 Hz (44100 / 60 * 2): long enough to guard an onset
        // pop, far too short to hide an intro.
        const TRANSITION_FADE_IN_SAMPLES: u32 = 1_470;
        let mut sequencer = legaia_engine_audio::sequencer::Sequencer::new(seq, bank);
        sequencer.set_loop_to(0);
        self.out.swap_bgm(sequencer, TRANSITION_FADE_IN_SAMPLES);
        // Retail's start arm (op 0x35 sub-op 1, `0x801E0104`) clears the
        // pause bit alongside the track select - a start issued while the
        // gate is closed must reopen it, or the new track sits silent.
        self.out.set_sequencer_paused(false);
        *self.last_started = Some(bgm_id);
    }

    /// Split a global-pool `music_01` entry (`[chunk][pBAV VAB][pQES SEQ]`),
    /// upload its own VAB into the SPU BGM region, stash it as the active bank,
    /// and return the parsed SEQ. `None` when the pair is absent or a header
    /// doesn't parse. Mirrors the native `AudioBgmDirector::stage_owned_vab`.
    fn stage_owned(
        &mut self,
        entry_bytes: &[u8],
    ) -> Option<(legaia_seq::Seq, legaia_engine_audio::VabBank)> {
        let vab_off = entry_bytes.windows(4).position(|w| w == b"pBAV")?;
        let seq_rel = entry_bytes[vab_off..]
            .windows(4)
            .position(|w| w == b"pQES")?;
        let report = legaia_vab::parse(entry_bytes, vab_off).ok()?;
        let body = &entry_bytes[vab_off..];
        let bank = self.out.with_spu(|spu| {
            // Cap the BGM region below the resident class-2 SFX bank at the
            // top of SPU RAM, the way the native boot's `stage_scene_vab`
            // does, so a BGM upload never stomps the SFX samples
            // ([`crate::play_sfx`]).
            let mut alloc = legaia_engine_audio::spu::ram::SpuAllocator::new(
                crate::play_sfx::SPU_RESERVED_BYTES,
                legaia_engine_audio::spu::ram::SPU_RAM_BYTES as u32
                    - crate::play_sfx::SPU_RESERVED_BYTES
                    - crate::play_sfx::SFX_BANK_SPU_BYTES,
            );
            legaia_engine_audio::VabBank::upload(spu, &mut alloc, &report, body)
        });
        let seq = legaia_seq::Seq::parse(&entry_bytes[vab_off + seq_rel..]).ok()?;
        *self.bank = Some(bank.clone());
        Some((seq, bank))
    }
}

#[cfg(target_arch = "wasm32")]
impl legaia_engine_core::scene::BgmDirector for WebBgmDirector<'_> {
    fn start(&mut self, bgm_id: u16, seq_bytes: &[u8]) {
        if *self.last_started == Some(bgm_id) {
            return;
        }
        let Some(bank) = self.bank.clone() else {
            crate::console_log("play BGM: scene-local start with no scene VAB staged");
            return;
        };
        match legaia_seq::Seq::parse(seq_bytes) {
            Ok(seq) => self.play(bgm_id, seq, bank),
            Err(e) => crate::console_log(&format!("play BGM: SEQ parse failed: {e}")),
        }
    }

    fn start_owned_vab(&mut self, bgm_id: u16, entry_bytes: &[u8]) {
        if *self.last_started == Some(bgm_id) {
            return;
        }
        match self.stage_owned(entry_bytes) {
            Some((seq, bank)) => self.play(bgm_id, seq, bank),
            None => crate::console_log("play BGM: global entry has no [VAB][SEQ] pair"),
        }
    }

    fn pause(&mut self) {
        self.out.set_sequencer_paused(true);
    }

    fn resume(&mut self) {
        self.out.set_sequencer_paused(false);
    }

    fn stop(&mut self) {
        self.out.detach_sequencer();
        *self.last_started = None;
    }

    /// Sub-op `0xA` - the unhalt-pause swap-commit (retail `0x801E0264`):
    /// if the gate is still closed no start intervened, so the paused
    /// track is released the way retail's `FUN_800266E0` + `FUN_80026520`
    /// pair detaches and closes the slot; the gate is then reopened
    /// unconditionally (retail clears `_DAT_8007B750` bit 1 on every pass
    /// through the arm). The browser twin of
    /// `AudioBgmDirector::unhalt_pause`.
    fn unhalt_pause(&mut self) {
        if self.out.sequencer_paused() {
            self.out.detach_sequencer();
            *self.last_started = None;
        }
        self.out.set_sequencer_paused(false);
    }
}

//! The play page's BGM director - the browser twin of the native
//! `AudioBgmDirector` (`crates/engine-shell/src/bgm.rs`). Implements
//! `legaia_engine_core::scene::BgmDirector` over the live `WebAudioOut`, so
//! the field VM's op-`0x35` events start / pause / stop the same tracks on
//! both hosts.
//!
//! The parts of the native director's *policy* that this twin has to carry
//! are pulled out as plain constants / functions above the `wasm32`-only
//! body, so they can be asserted off-wasm - the director itself needs a
//! `WebAudioOut`, which only exists in a browser.

use crate::runtime::LegaiaRuntime;
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
use legaia_engine_audio::WebAudioOut;

/// Master volume forwarded to every freshly-built sequencer (`SsSeqSetVol`
/// units, `0..=127`) - the native director's `master_vol` seed. The
/// sequencer's own default is `127`, and the browser twin used to leave it
/// there, which made every track ~27% hotter than the same track under the
/// native window relative to the SFX and XA voices it mixes with. The
/// battle audio duck scales *this* value ([`crate::play_sfx`]), so the two
/// hosts must share it or the duck lands at different depths.
pub(crate) const BGM_MASTER_VOL: u8 = 100;

/// Loop-to event index for newly-started sequencers: the native director's
/// `loop_to` field. `Some(0)` loops every track to its start; `None` would
/// play once. The native field is seeded `Some(0)` and **no code path ever
/// sets it to `None`** (`grep -rn "loop_to" crates/engine-shell/src`), so the
/// effective native policy is "always loop", and that is what the twin
/// mirrors. A future one-shot policy (cutscene stings) belongs in both
/// hosts at once.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) const BGM_LOOP_TO: Option<usize> = Some(0);

/// Whether a `start(bgm_id)` re-emit must be dropped to keep the playhead of
/// the track already sounding - the native director's duplicate-start guard
/// (`AudioBgmDirector::start` / `start_owned_vab`), in full:
///
/// ```text
/// last_started == Some(bgm_id) && !paused && sequencer_live
/// ```
///
/// Only the *same id, still sounding, not paused* case is a duplicate. The
/// browser twin used to test the id alone, and every other combination is
/// a track that has to start again: the field VM re-emits its op-`0x35`
/// start with the same id when a scene's music returns after a battle, a
/// cutscene pause or a stop, and an id-only guard turned each of those into
/// silence - the field BGM never came back on the page.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn restart_suppressed(
    last_started: Option<u16>,
    bgm_id: u16,
    paused: bool,
    sequencer_live: bool,
) -> bool {
    last_started == Some(bgm_id) && !paused && sequencer_live
}

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
///
/// The pause latch is the audio output's own sequencer gate
/// ([`WebAudioOut::sequencer_paused`]) rather than a second bool: the
/// native director keeps its own `paused` mirror of the gate it sets, and
/// here the gate is the only pause state there is.
#[cfg(target_arch = "wasm32")]
pub(crate) struct WebBgmDirector<'a> {
    pub(crate) out: &'a WebAudioOut,
    pub(crate) bank: &'a mut Option<legaia_engine_audio::VabBank>,
    pub(crate) last_started: &'a mut Option<u16>,
}

#[cfg(target_arch = "wasm32")]
impl WebBgmDirector<'_> {
    /// The native guard, evaluated against this output's live state.
    fn suppressed(&self, bgm_id: u16) -> bool {
        restart_suppressed(
            *self.last_started,
            bgm_id,
            self.out.sequencer_paused(),
            self.out.sequencer_progress().is_some(),
        )
    }

    /// Install a freshly-built sequencer at [`BGM_MASTER_VOL`], looping per
    /// [`BGM_LOOP_TO`], and let it sound from its own first event - the
    /// native `AudioBgmDirector::start_inner`, call for call.
    ///
    /// With a track already sounding this is `swap_bgm` behind a click-guard
    /// ramp of ~2 frames at the SPU's 44.1 kHz rate, the same call with the
    /// same constant the native director makes. Not `crossfade_to`: that one
    /// is a serial fade that parks the incoming sequencer in `pending_seq`
    /// and rolls the outgoing one down to silence first, so the new track
    /// has not begun a fade-length after the script asked for it. Retail BGM
    /// changes are hard cuts, and a cutscene sting is mostly intro. With
    /// nothing sounding (or the gate closed) the sequencer is attached
    /// directly at full volume, exactly as native does.
    fn play(&mut self, bgm_id: u16, seq: legaia_seq::Seq, bank: legaia_engine_audio::VabBank) {
        // ~2 frames at 60 Hz (44100 / 60 * 2): long enough to guard an onset
        // pop, far too short to hide an intro.
        const TRANSITION_FADE_IN_SAMPLES: u32 = 1_470;
        let mut sequencer = legaia_engine_audio::sequencer::Sequencer::new(seq, bank);
        sequencer.set_master_vol(BGM_MASTER_VOL);
        if let Some(loop_to) = BGM_LOOP_TO {
            sequencer.set_loop_to(loop_to);
        }
        if self.out.sequencer_progress().is_some() && !self.out.sequencer_paused() {
            self.out.swap_bgm(sequencer, TRANSITION_FADE_IN_SAMPLES);
        } else {
            self.out.attach_sequencer(sequencer);
        }
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
    ///
    /// The upload re-owns the whole BGM region, transient tail included: the
    /// level-up reward bank the SFX channel may have parked behind the
    /// previous track ([`crate::play_sfx`]) is stale from here, and the SFX
    /// channel re-checks its base against the live bank before keying it.
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
        if self.suppressed(bgm_id) {
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
        if self.suppressed(bgm_id) {
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

    /// Detach the track **and reopen the gate**, as the native `stop` does:
    /// a stop issued while paused must not leave the gate closed for the
    /// next start, which would attach its sequencer behind it and sound
    /// nothing until an unrelated resume.
    fn stop(&mut self) {
        self.out.detach_sequencer();
        self.out.set_sequencer_paused(false);
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

/// The title -> load hand-off, as one call.
///
/// The native window runs `bgm.stop()` followed by
/// `BootSession::restore_field_bgm()` the moment a title-screen save-select
/// commits: the title theme has to let go of the score, and the loaded save's
/// own op-`0x35` track (`World::audio.current_bgm`) has to come back, because
/// the field VM will not re-emit a start for music that was already playing
/// when the save was written.
///
/// The browser play page did neither, so a load from the title left the title
/// theme running underneath the loaded scene - for as long as that scene's
/// script went without a music event, which in a town is the whole visit.
///
/// Returns whether a track is sounding afterwards; `false` covers both "audio
/// is down" and "the save carried no global-pool track", and in the second
/// case the stop still ran, which is the native behaviour too (silence, not a
/// stale theme).
#[wasm_bindgen]
impl LegaiaRuntime {
    pub fn play_bgm_title_handoff(&mut self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            use legaia_engine_core::scene::BgmDirector;
            let Some(out) = self.audio_out.as_ref() else {
                return false;
            };
            let mut director = WebBgmDirector {
                out,
                bank: &mut self.bgm_bank,
                last_started: &mut self.bgm_last_started,
            };
            director.stop();
            let Some(host) = self.scene_host.as_ref() else {
                return false;
            };
            let Some(id) = host.world.audio.current_bgm else {
                return false;
            };
            let Ok(Some(entry)) = host.music_bank_entry_bytes(id) else {
                return false;
            };
            director.start_owned_vab(id, &entry);
            self.bgm_last_started == Some(id)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The guard is the native triple, not the id alone. Every row here is a
    /// case the id-only guard got wrong except the first.
    #[test]
    fn only_a_same_id_live_unpaused_start_is_a_duplicate() {
        // Same track, still sounding, not paused: keep the playhead.
        assert!(restart_suppressed(Some(2019), 2019, false, true));
        // Same id but the sequencer is gone (stopped, detached, or run off
        // its end): the field VM's re-emit must restart it.
        assert!(!restart_suppressed(Some(2019), 2019, false, false));
        // Same id while paused: a start clears the pause bit and restarts.
        assert!(!restart_suppressed(Some(2019), 2019, true, true));
        assert!(!restart_suppressed(Some(2019), 2019, true, false));
        // Different id or nothing started yet: never suppressed.
        assert!(!restart_suppressed(Some(2019), 2026, false, true));
        assert!(!restart_suppressed(None, 2019, false, true));
    }

    /// The two policy constants the native director seeds. `master_vol`
    /// is what the battle duck scales, so it must be the same number on both
    /// hosts; `loop_to` is `Some(0)` because nothing native ever clears it.
    #[test]
    fn policy_constants_match_the_native_director_seed() {
        assert_eq!(BGM_MASTER_VOL, 100);
        assert_eq!(BGM_LOOP_TO, Some(0));
    }
}

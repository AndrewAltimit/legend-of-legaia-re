//! The minigames page's live SPU: one `WebAudioOut`, per-slot VAB staging and
//! the id-keyed and explicit-attribute cue paths.

use super::*;

// ---------------------------------------------------------------------------
// The page's live SPU
// ---------------------------------------------------------------------------
//
// The standalone minigames page used to hold no `Spu` at all. Its music is a
// whole track rendered offline to interleaved PCM (`music01_bgm_render`) and
// its cues are per-cue PCM decoded out of a VAB (`muscle_sfx_pcm`,
// `slot_sfx_pcm`), both handed to the page as buffers - so there was no voice
// to key and no mixer running to key it into.
//
// That covers a cue that names itself by id. It cannot cover the Muscle Dome's
// between-leg tally, which keys a voice per drained lane and names no id:
// `FUN_801D1288` resolves a whole `(voice, VAB id, program, tone, note, fine,
// vol_l, vol_r)` set, ported as `other_game_overlay::arena_voice_cue` ->
// `VoiceAttrCue`, and the native window drives it through
// `AudioDirector::key_on_voice_attr`. The catalog path
// (`SfxBank::play_one_shot`) could never sound it.
//
// So this block is the page's `play_sfx`: one `WebAudioOut` the entry point
// owns, a VAB region at the top of SPU RAM staged per descriptor slot, and the
// two firing paths the native window has - the id-keyed one and the explicit
// attr one. The offline BGM render stays the music source: it is an
// `AudioBufferSourceNode` on the page's own context, which mixes with this one
// at the device rather than inside the SPU, and moving a whole rendered track
// back through the sequencer would be a second BGM path rather than a shared
// one.
#[wasm_bindgen]
impl LegaiaMinigames {
    /// Open this page's live SPU. Must be called from a **user gesture** -
    /// browser autoplay policy refuses an `AudioContext` otherwise, which is
    /// the same rule `LegaiaViewer::start_bgm` follows.
    ///
    /// Idempotent: a second call is a no-op and still answers `true`. Answers
    /// `false` off wasm32, where there is no `WebAudioOut` to build.
    pub fn minigame_audio_open(&mut self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            if self.audio_out.is_some() {
                return true;
            }
            match legaia_engine_audio::WebAudioOut::new() {
                Ok(out) => {
                    self.audio_out = Some(out);
                    // A dedicated region at the top of SPU RAM, out of one
                    // allocator so the slots this page stages pack rather than
                    // overlap. No sequencer runs on this SPU, so everything
                    // below the 4 KB reserved head is free.
                    self.sfx_alloc = Some(legaia_engine_audio::spu::ram::SpuAllocator::new(
                        0x1000,
                        legaia_engine_audio::spu::ram::SPU_RAM_BYTES as u32 - 0x1000,
                    ));
                    true
                }
                Err(e) => {
                    console_log(&format!("minigame audio: WebAudioOut failed: {e:?}"));
                    false
                }
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        false
    }

    /// Is the live SPU up?
    pub fn minigame_audio_ready(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            self.audio_out.is_some()
        }
        #[cfg(not(target_arch = "wasm32"))]
        false
    }

    /// Resume the SPU's `AudioContext`. Browsers construct it `suspended` even
    /// inside a gesture handler on some engines, so the page calls this right
    /// after [`Self::minigame_audio_open`].
    #[cfg(target_arch = "wasm32")]
    pub fn minigame_audio_resume(&mut self) -> js_sys::Promise {
        match self.audio_out.as_ref() {
            Some(out) => out.resume(),
            None => js_sys::Promise::resolve(&JsValue::UNDEFINED),
        }
    }

    /// Master gain gate on the live SPU (the page's sound toggle).
    pub fn minigame_audio_set_muted(&mut self, muted: bool) {
        #[cfg(target_arch = "wasm32")]
        if let Some(out) = self.audio_out.as_ref() {
            out.set_muted(muted);
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = muted;
    }

    /// Fire one **catalog** cue by descriptor id - the id-keyed path, the
    /// browser twin of the cues the native window's `window/minigames.rs`
    /// hands `bgm.enqueue_sfx` (the Baka exchange hit
    /// `baka_fighter::BAKA_CUE_HIT`, the dome's own UI blips).
    ///
    /// Returns `true` when a voice keyed. `false` covers every honest miss,
    /// and they are worth keeping apart from a bug:
    ///
    /// * no live SPU (the page never took a gesture);
    /// * no `SCUS_942.54` (a raw `PROT.DAT` load - the descriptor table lives
    ///   in the executable, so no cue can resolve);
    /// * an id outside the static table's byte-wide id space, which is a
    ///   **runtime**-bank row rather than a static one (`bse.dat`, loaded per
    ///   battle - see `docs/formats/bse-dat.md`). The dance's
    ///   `COUNTIN_INTRO_CUE` is `0x200` and this page stages no runtime bank,
    ///   so it answers `false` rather than keying static row `0` by
    ///   truncation - the width trap the play page's scheduler was caught by
    ///   once already.
    pub fn minigame_sfx_cue(&mut self, id: u16) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            let Ok(id8) = u8::try_from(id) else {
                return false;
            };
            let Some(slot) = self.sfx_slot_for_cue(id8) else {
                return false;
            };
            if !self.stage_sfx_slot(slot) {
                return false;
            }
            let (Some(bank), Some(vab), Some(out)) = (
                self.sfx_bank.as_ref(),
                self.sfx_vabs.get(&slot),
                self.audio_out.as_ref(),
            ) else {
                return false;
            };
            out.with_spu(|spu| bank.play_one_shot(id8, spu, vab).is_some())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = id;
            false
        }
    }

    /// Key the Muscle Dome tally's **voice-attr** cues for a screen tick.
    ///
    /// The dome page replays `ScoreTallyRamp` from the INTERVAL screen's own
    /// tick rather than stepping it once per frame (`muscle_hub_quads_json`),
    /// so this replays the same way and keys only the steps it has not keyed
    /// yet - `muscle_tally_voiced_steps` is the high-water mark, cleared by
    /// [`Self::muscle_tally_voice_reset`] when a new INTERVAL screen opens.
    ///
    /// Returns how many voices keyed. The attr set is the shared kernel's:
    /// `arena_voice_cue` resolves the whole tuple and advances the rotating
    /// voice counter, exactly as the native window's INTERVAL arm does.
    pub fn muscle_tally_voice(&mut self, round: i32) -> u32 {
        #[cfg(target_arch = "wasm32")]
        {
            let Some(run) = self.muscle_run.as_ref() else {
                return 0;
            };
            let volume_word =
                legaia_engine_menus::new_game::GAME_STATE_COLD_RESET.voice_volume as u32;
            let (mut ramp, _) = run.tally_roll();
            let target = round.max(0);
            let from = self.muscle_tally_voiced_steps;
            let mut attrs = Vec::new();
            for step_idx in 0..target {
                let step = ramp.tick(1, self.muscle_tally_boost_at(step_idx), volume_word);
                if step_idx >= from {
                    // Re-resolve through the kernel rather than reusing the
                    // replayed step's own cue: the replay restarts the ramp's
                    // internal counter every call, so its voice slot would be
                    // 0 every time and every lane would key into one voice.
                    // The rotating counter belongs to the host, as retail's
                    // free-running `DAT_801D1AE4` does.
                    for _ in &step.cues {
                        attrs.push(
                            legaia_engine_minigames::other_game_overlay::arena_voice_cue(
                                &mut self.muscle_tally_cue_counter,
                                volume_word,
                            ),
                        );
                    }
                }
                if !step.rolling {
                    break;
                }
            }
            self.muscle_tally_voiced_steps = target;
            let mut keyed = 0u32;
            for cue in attrs {
                let attr = legaia_engine_audio::VoiceAttr::from_cue_words(
                    cue.voice,
                    cue.vab_program_tone,
                    cue.note_and_fine,
                    cue.volume,
                );
                // Retail's `a1` is a VAB **id**; the native director resolves
                // it as an SFX slot with the live BGM bank as the fallback.
                // This page has no resident BGM bank (its music is an offline
                // render), so the slot is the whole of the resolution.
                let slot = u8::try_from(attr.vab_id).unwrap_or(0);
                if !self.stage_sfx_slot(slot) {
                    continue;
                }
                let (Some(vab), Some(out)) = (self.sfx_vabs.get(&slot), self.audio_out.as_ref())
                else {
                    continue;
                };
                if out.with_spu(|spu| legaia_engine_audio::key_on_voice_attr(&attr, spu, vab)) {
                    keyed += 1;
                }
            }
            keyed
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = round;
            0
        }
    }

    /// Forget how far the tally has been voiced and lower its fast-forward
    /// latch - called when a fresh INTERVAL screen opens, so the next screen
    /// keys its lanes again and rolls at the slow rate (the arena init's
    /// clear of `DAT_801D1AB4`, `0x801CEB1C`).
    pub fn muscle_tally_voice_reset(&mut self) {
        self.muscle_tally_voiced_steps = 0;
        self.muscle_tally_boost_from = None;
    }
}

impl LegaiaMinigames {
    /// The VAB slot a static descriptor id keys against
    /// (`sfx_table::slot_for_category` over the `+4` category byte), or `None`
    /// without an executable to read the table out of. Builds the audio-layer
    /// bank on the way past, once.
    #[cfg(target_arch = "wasm32")]
    fn sfx_slot_for_cue(&mut self, id: u8) -> Option<u8> {
        let table = legaia_game_tables::sfx_table::SfxTable::from_scus(self.scus.as_ref()?)?;
        if self.sfx_bank.is_none() {
            self.sfx_bank = Some(legaia_engine_audio::SfxBank::from_descriptors(
                table
                    .active()
                    .map(|(i, d)| (i, d.program, d.tone, d.note, d.flags)),
            ));
        }
        Some(table.get(id)?.vab_slot())
    }

    /// Upload the program bank for descriptor `slot` into the live SPU, once.
    /// `false` when there is no SPU, no such slot, or the entry carries no
    /// VAB.
    #[cfg(target_arch = "wasm32")]
    fn stage_sfx_slot(&mut self, slot: u8) -> bool {
        if self.sfx_vabs.contains_key(&slot) {
            return true;
        }
        let Some(prot_index) = legaia_game_tables::sfx_table::prot_index_for_slot(slot) else {
            return false;
        };
        let Some(entry) = entry_bytes(&self.prot, &self.entries, prot_index) else {
            return false;
        };
        let Some(&off) = legaia_vab::find_vabs(entry).first() else {
            return false;
        };
        let Ok(report) = legaia_vab::parse(entry, off) else {
            return false;
        };
        let body = entry[off..].to_vec();
        let (Some(out), Some(alloc)) = (self.audio_out.as_ref(), self.sfx_alloc.as_mut()) else {
            return false;
        };
        let bank =
            out.with_spu(|spu| legaia_engine_audio::VabBank::upload(spu, alloc, &report, &body));
        self.sfx_vabs.insert(slot, bank);
        true
    }
}

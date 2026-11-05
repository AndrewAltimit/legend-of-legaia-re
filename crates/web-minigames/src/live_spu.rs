//! The minigames page's live SPU: the play hosts' `AudioDirector` over one
//! `WebAudioOut`, staged for whichever game the page has open.

use super::*;
use legaia_engine_audio::{AudioSink, VoiceAttr};
use legaia_engine_minigames::other_game_overlay::VoiceAttrCue;
use legaia_engine_mode::minigame_entry::MinigameSubId;
use legaia_engine_system::sound_state::SfxRingOp;

/// The output this page's director keys into: the live WebAudio output in a
/// browser, a headless sink off wasm (no device, same SPU and mixer).
#[cfg(target_arch = "wasm32")]
pub(crate) type MgSink = legaia_engine_audio::WebAudioOut;
/// The output this page's director keys into: the live WebAudio output in a
/// browser, a headless sink off wasm (no device, same SPU and mixer).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) type MgSink = legaia_engine_audio::TestAudioSink;

/// The page's audio director - the native window's and the play page's.
pub(crate) type MgDirector = legaia_engine_session::AudioDirector<MgSink>;

/// Build the page's output. Must run inside a user gesture in a browser.
fn open_sink() -> Result<MgSink, String> {
    #[cfg(target_arch = "wasm32")]
    {
        legaia_engine_audio::WebAudioOut::new().map_err(|e| e.to_string())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        Ok(legaia_engine_audio::TestAudioSink::new(
            legaia_engine_audio::SPU_INTERNAL_RATE,
        ))
    }
}

/// The minigame a page tab stands for: what picks the bank residency
/// ([`legaia_engine_mode::sfx_residency::SfxBankResidency`], through its mode)
/// and the runtime cue bundle, the way the door warp does on the play hosts.
pub(crate) fn tab_minigame(game: &str) -> Option<MinigameSubId> {
    Some(match game {
        "baka" => MinigameSubId::BakaFighter,
        "slot" => MinigameSubId::SlotMachine,
        "muscle" => MinigameSubId::MuscleDome,
        "dance" => MinigameSubId::Dance,
        "fishing" => MinigameSubId::Fishing,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// The page's live SPU
// ---------------------------------------------------------------------------
//
// One director, as on the play hosts: the cue descriptors, the retail ring,
// the voices the drainer picks, the per-mode bank residency. Every cue this
// page sounds keys a voice on it, so it carries the tone's ADSR, the retail
// volume and pan and the tone's reverb send, and a cue resolves exactly as the
// play hosts resolve it - including a closed slot being silent.
//
// The BGM stays an offline render on the page's own `AudioBufferSourceNode`
// (`music01_bgm_render`, the `MgBgm` loop), which mixes with this output at the
// device rather than inside the SPU. Moving a whole rendered track back through
// the sequencer would be a second BGM path rather than a shared one, and it
// means the BGM never loses a voice to a one-shot here.
#[wasm_bindgen]
impl LegaiaMinigames {
    /// Open this page's live SPU. Must be called from a **user gesture** -
    /// browser autoplay policy refuses an `AudioContext` otherwise, which is
    /// the same rule `LegaiaViewer::start_bgm` follows.
    ///
    /// Idempotent: a second call is a no-op and still answers `true`. Answers
    /// `false` off wasm32, where there is no `WebAudioOut` to build.
    pub fn minigame_audio_open(&mut self) -> bool {
        if self.audio.is_some() {
            return true;
        }
        match open_sink() {
            Ok(out) => {
                #[allow(clippy::arc_with_non_send_sync)]
                let d = MgDirector::new(std::sync::Arc::new(out));
                self.audio = Some(d);
                self.audio_staged = false;
                self.audio_sync();
                true
            }
            Err(_e) => {
                #[cfg(target_arch = "wasm32")]
                console_log(&format!("minigame audio: output failed: {_e}"));
                false
            }
        }
    }

    /// Is the live SPU up?
    pub fn minigame_audio_ready(&self) -> bool {
        self.audio.is_some()
    }

    /// The game whose tab the page just opened (`"dance"`, `"baka"`, `"slot"`,
    /// `"fishing"`, `"muscle"`; anything else is none). Leaving a game does
    /// what its mode exit does: nothing it keyed keeps sounding, its armed
    /// cues are dropped, and the CD input leaves the reverb send. The new
    /// game's banks are then staged the way its door warp stages them
    /// (`World::runtime_sfx_bundle`, `World::sync_sfx_residency`). Safe to call
    /// before the output opens: the open stages what the page last asked for.
    pub fn minigame_audio_enter(&mut self, game: &str) {
        let game = tab_minigame(game);
        if game == self.audio_game {
            return;
        }
        self.audio_game = game;
        if let Some(d) = self.audio.as_mut() {
            d.clear_sfx();
            let every_voice: Vec<u8> = (0..24).collect();
            d.stop_sfx_voices(&every_voice);
            d.audio().stop_xa();
            d.set_cd_reverb(false);
        }
        self.audio_sync();
    }

    /// Resume the SPU's `AudioContext`. Browsers construct it `suspended` even
    /// inside a gesture handler on some engines, so the page calls this right
    /// after [`Self::minigame_audio_open`].
    #[cfg(target_arch = "wasm32")]
    pub fn minigame_audio_resume(&mut self) -> js_sys::Promise {
        match self.audio.as_ref() {
            Some(d) => d.audio().resume(),
            None => js_sys::Promise::resolve(&JsValue::UNDEFINED),
        }
    }

    /// Master gain gate on the live SPU (the page's sound toggle).
    pub fn minigame_audio_set_muted(&mut self, muted: bool) {
        if let Some(d) = self.audio.as_ref() {
            d.audio().set_muted(muted);
        }
    }

    /// Fire one cue by id, the drainer's split: a static descriptor row below
    /// `0x100`, through its category's bank; a runtime row at `0x200` and up,
    /// through the open game's bundle and the bank the row's `+4` names. The
    /// band between them is a CD-XA voice on the retail dispatcher, which no
    /// cue on this page raises, and is declined as the play hosts decline it.
    ///
    /// Returns `true` when a voice keyed. `false` covers every honest miss:
    /// no live SPU (the page never took a gesture), no `SCUS_942.54` (a raw
    /// `PROT.DAT` load has no static table), a slot closed in this game, a row
    /// the open game's bundle does not carry.
    pub fn minigame_sfx_cue(&mut self, id: u16) -> bool {
        let Some(d) = self.audio.as_mut() else {
            return false;
        };
        let report = match id {
            0..0x100 => d.fire_now(id),
            0x200..0x8000 => d.fire_ring_now(id as i16),
            _ => return false,
        };
        !report.fired.is_empty()
    }

    /// Key a dance good-step **hit sting**: two voices together, directly and
    /// past the cue ring (`FUN_801d3d78`), the queue
    /// `World::route_dance_award_sounds` feeds on the play hosts. `r` is the
    /// resolved variant ([`Self::dance_take_award_sounds`] leaves the tier-2
    /// site's `rand() % 3` to the page). Returns how many voices keyed.
    pub fn dance_sting_key(&mut self, r: u16) -> u32 {
        let volume_word = legaia_engine_menus::new_game::GAME_STATE_COLD_RESET.voice_volume as u32;
        legaia_engine_minigames::dance::dance_hit_sting_cues(r, volume_word)
            .into_iter()
            .filter(|&cue| self.key_voice_cue(cue))
            .count() as u32
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
        let Some(run) = self.muscle_run.as_ref() else {
            return 0;
        };
        let volume_word = legaia_engine_menus::new_game::GAME_STATE_COLD_RESET.voice_volume as u32;
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
        let keyed = attrs
            .into_iter()
            .filter(|&cue| self.key_voice_cue(cue))
            .count() as u32;
        // The ring's armed "ka-ching" cues age on this screen's own tick.
        for _ in from..target {
            self.audio_frame(&[], &[], Vec::new());
        }
        keyed
    }

    /// Forget how far the tally has been voiced and lower its fast-forward
    /// latch - called when a fresh INTERVAL screen opens, so the next screen
    /// keys its lanes again and rolls at the slow rate (the arena init's
    /// clear of `DAT_801D1AB4`, `0x801CEB1C`). The screen's first frame also
    /// arms its four "ka-ching" ring cues, as the play hosts' `HubTimers` do
    /// (`0x801CFCAC..0x801CFCEC`).
    pub fn muscle_tally_voice_reset(&mut self) {
        self.muscle_tally_voiced_steps = 0;
        self.muscle_tally_boost_from = None;
        use legaia_engine_minigames::muscle_dome::{HUB_TALLY_CUE_STAGGER, HUB_TALLY_CUES};
        let ops: Vec<SfxRingOp> = HUB_TALLY_CUES
            .iter()
            .zip(HUB_TALLY_CUE_STAGGER)
            .enumerate()
            .map(|(slot, (&id, delay))| SfxRingOp::ArmSlot(slot as u8, id, i16::from(delay)))
            .collect();
        if let Some(d) = self.audio.as_mut() {
            d.apply_sfx_ring_ops(&ops, 1);
        }
    }
}

impl LegaiaMinigames {
    /// One game frame of the audio side, in the order the play hosts run it
    /// (`AudioDirector::route_world_sfx`, then the scheduler): the frame's ring
    /// stores, its voice releases, its direct key-ons, then the ring's age and
    /// drain. A no-op with no output up, so a muted page queues nothing.
    pub(crate) fn audio_frame(
        &mut self,
        ring: &[SfxRingOp],
        stops: &[u8],
        keys: Vec<VoiceAttrCue>,
    ) {
        let Some(d) = self.audio.as_mut() else {
            return;
        };
        d.apply_sfx_ring_ops(ring, 1);
        d.stop_sfx_voices(stops);
        for cue in keys {
            d.key_on_voice_attr(attr_of(cue));
        }
        d.tick_sfx_frame();
    }

    /// Key one direct voice cue. `false` with no output up or a slot closed.
    pub(crate) fn key_voice_cue(&mut self, cue: VoiceAttrCue) -> bool {
        self.audio
            .as_mut()
            .is_some_and(|d| d.key_on_voice_attr(attr_of(cue)))
    }

    /// Bring the director up to what the page has open: the descriptor tables
    /// and the slot-0 system bank once per disc, then the open game's runtime
    /// bundle, side bank and shared-region bank - the order
    /// `AudioDirector::route_world_sfx` syncs them in.
    pub(crate) fn audio_sync(&mut self) {
        let Some(d) = self.audio.as_mut() else {
            return;
        };
        let (prot, entries) = (&self.prot, &self.entries);
        let read = |e: u32| entry_bytes(prot, entries, e).map(<[u8]>::to_vec);
        if !self.audio_staged && !entries.is_empty() {
            self.audio_staged = true;
            if let Some(table) = self
                .scus
                .as_ref()
                .and_then(|s| legaia_game_tables::sfx_table::SfxTable::from_scus(s))
            {
                d.set_sfx_bank(legaia_engine_audio::SfxBank::from_descriptors(
                    table
                        .active()
                        .map(|(i, r)| (i, r.program, r.tone, r.note, r.flags)),
                ));
                d.set_sfx_cue_slots(table.cue_slots());
            }
            let slot0 = legaia_game_tables::sfx_table::SLOT0_SYSTEM_BANK_PROT_INDEX;
            let _staged = read(slot0).is_some_and(|bytes| d.stage_resident_slot0(bytes));
            #[cfg(target_arch = "wasm32")]
            if !_staged {
                console_log("minigame audio: slot-0 system bank (PROT 0868) did not stage");
            }
        }
        let Some(game) = self.audio_game else {
            return;
        };
        use legaia_minigame_formats::{dance_art, minigame_sfx};
        let bundle = match game {
            MinigameSubId::SlotMachine => read(minigame_sfx::SLOT_SFX_BANK_PROT_INDEX as u32),
            MinigameSubId::Dance => read(dance_art::DANCE_SFX_BANK_PROT_INDEX as u32),
            MinigameSubId::MuscleDome => read(minigame_sfx::ARENA_SFX_BUNDLE_PROT_INDEX as u32),
            // The fishing init loads no `efect.dat`: its runtime rows are the
            // venue scene's own.
            MinigameSubId::Fishing => self.fishing_sfx_bundle.clone(),
            _ => None,
        };
        let side_band = (game == MinigameSubId::MuscleDome)
            .then_some(legaia_engine_core::world::SideBandBank::ARENA);
        d.sync_field_sfx(&bundle.unwrap_or_default(), false, side_band, read);
        if let Some(mode) = game.scene_mode() {
            let want = self.audio_residency.sync(
                mode,
                "",
                None,
                legaia_engine_mode::sfx_residency::SharedRegionBank::CLASS2,
            );
            d.sync_shared_region(want, read);
        }
    }

    /// The disc under the page changed: the banks staged from the last one
    /// are stale, and the next sync stages this one's.
    pub(crate) fn audio_disc_changed(&mut self) {
        self.audio_staged = false;
        self.audio_residency = Default::default();
        // A fresh director: the old one's region layout names the last disc.
        if let Some(out) = self.audio.take().map(|d| d.audio().clone()) {
            out.with_spu(|spu| spu.key_off_mask(0x00FF_FFFF));
            self.audio = Some(MgDirector::new(out));
        }
        self.audio_sync();
    }

    /// A test's read of the page's output: `frames` frames of the mix, on the
    /// headless sink the page runs off wasm.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn audio_render_for_test(&self, frames: usize) -> Vec<(i16, i16)> {
        self.audio
            .as_ref()
            .map(|d| (0..frames).map(|_| d.audio().next_frame()).collect())
            .unwrap_or_default()
    }

    /// A test's read of the ring cues that have come due, oldest first.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn audio_ring_due_for_test(&self) -> Vec<i16> {
        self.audio
            .as_ref()
            .map(|d| {
                d.commands()
                    .recent()
                    .filter_map(|c| match *c {
                        legaia_engine_session::audio_director::AudioCommand::RingDue(id) => {
                            Some(id)
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A test's read of the voice file: which voices are keyed and live.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn audio_live_voices_for_test(&self) -> Vec<u8> {
        self.audio
            .as_ref()
            .map(|d| {
                d.audio().with_spu(|spu| {
                    spu.voices
                        .iter()
                        .enumerate()
                        .filter(|(_, v)| !v.is_off())
                        .map(|(i, _)| i as u8)
                        .collect()
                })
            })
            .unwrap_or_default()
    }
}

fn attr_of(cue: VoiceAttrCue) -> VoiceAttr {
    VoiceAttr::from_cue_words(
        cue.voice,
        cue.vab_program_tone,
        cue.note_and_fine,
        cue.volume,
    )
}

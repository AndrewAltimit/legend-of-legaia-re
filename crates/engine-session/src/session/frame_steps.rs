//! The per-tick steps both play hosts run around [`BootSession::tick`], held
//! here so each is written once.
//!
//! A play host's frame is the session tick plus a tail of steps the host
//! sequences itself, because some of them sit between two pieces of render
//! work only that host owns (a mesh upload, a VRAM re-read). The steps in
//! this file are the ones with no such host half: they read and write the
//! world and the audio director and nothing else, so the native window and
//! the browser play page call the same body and differ only in when their
//! frame reaches it.
//!
//! Each returns what a host still owes - a session to hold, a count for a
//! readout - rather than performing a host duty through a callback.

use legaia_engine_audio::AudioSink;
use legaia_engine_menus::game_over::GameOverSession;
use legaia_engine_mode::mode::SceneMode;

use super::BootSession;
use crate::audio_director::SfxFrameReport;

impl<S: AudioSink> BootSession<S> {
    /// The party-wipe probe: consume the world's `game_over` flag and hand
    /// back the return-to-title hold the host runs from here.
    ///
    /// `frame_owned` is the one term only the host can answer - whether a
    /// screen of its own (the boot UI, a hold already running) owns the frame.
    /// While it does the flag is left raised and nothing is returned.
    ///
    /// The flag is consumed on the take, so a host cannot start a second hold
    /// from one wipe however long it keeps ticking. The world stays parked on
    /// the frozen battle frame (`World::game_over_hold`) until the host's hold
    /// resolves and it calls `World::resolve_game_over_hold`.
    ///
    /// Retail's wipe arm has one exit store - mode 22 CARD INIT with the
    /// title context word set - so the hold has one destination and asks the
    /// player nothing.
    // REF: FUN_8003AEB0 (`0x8003B5D0` / `0x8003B5E0`, the wipe arm's stores)
    pub fn take_party_wipe(&mut self, frame_owned: bool) -> Option<GameOverSession> {
        if frame_owned || !self.host.world.game_over {
            return None;
        }
        self.host.world.game_over = false;
        log::info!("party wipe -> title screen");
        Some(GameOverSession::new())
    }

    /// The effect step of the frame tail: seat the pending move-FX and queue
    /// its sound cue on the director
    /// ([`crate::battle_fx::spawn_pending_move_fx`]), then advance the three
    /// move-VM effect scene-graphs one tick
    /// (`World::tick_effect_scene_graphs`: the Seru-magic summon, the battle
    /// move-FX, the field op-`0x34` stagers). Each half self-gates when
    /// nothing is live.
    ///
    /// A host seats the pending summon creature
    /// (`World::take_pending_summon_spawn`) before calling this, so the
    /// creature's first frame ticks on the tick that cast it - that seat is a
    /// mesh bind, which is the host's.
    ///
    /// Returns the ring value of the cue the spawned move carries, whether or
    /// not a director was attached to hear it.
    pub fn step_effect_scene_graphs(&mut self) -> Option<u16> {
        let ring = crate::battle_fx::spawn_pending_move_fx(&mut self.host.world);
        if let Some(ring_value) = ring {
            if let Some(bgm) = self.bgm.as_mut() {
                bgm.enqueue_sfx(ring_value, 0, 0, 0);
            }
            log::debug!("battle move-FX cue -> SFX {ring_value:#06x}");
        }
        self.host.world.tick_effect_scene_graphs();
        ring
    }

    /// Advance the Muscle Dome leg's round **time meter** one frame
    /// (`MuscleDomeSession::tick_time_meter`): it climbs through the
    /// selection phase and drains outside it. No-op with no leg open.
    ///
    /// Retail runs the meter from the arena's per-frame driver with the frame
    /// delta from scratchpad `0x1F800393`; the port ticks a fixed one per sim
    /// tick.
    pub fn step_muscle_time_meter(&mut self) {
        if let Some(s) = self.host.world.minigames.muscle_dome.as_mut() {
            s.tick_time_meter(1);
        }
    }

    /// Queue the minigame sessions' SFX cues (the fishing hub's and the point
    /// exchange's blips) on the director, to sound this tick. The dance's
    /// cues do not come this way: `World::tick_dance` stores them into the SFX
    /// ring, which [`BootSession::tick`] routes.
    ///
    /// The world's queue is drained with or without audio, so it cannot grow
    /// on a muted host. Returns how many cues it held.
    pub fn enqueue_minigame_sfx_cues(&mut self) -> u32 {
        let cues = self.host.world.drain_minigame_sfx_cues();
        if let Some(bgm) = self.bgm.as_mut() {
            for &cue in &cues {
                bgm.enqueue_sfx(cue, 0, 0, 0);
            }
        }
        cues.len() as u32
    }

    /// Queue the Baka Fighter duel's SFX cues (the exchange hit the rules
    /// kernel's damage step writes) on the director, to sound this tick. The
    /// duel has no actor or target slots, so the cue's HUD-context fields are
    /// zero. No-op outside the duel.
    ///
    /// The fight's buffer is drained with or without audio. Returns how many
    /// cues it held.
    pub fn enqueue_baka_sfx_cues(&mut self) -> u32 {
        if self.host.world.mode != SceneMode::BakaFighter {
            return 0;
        }
        let cues = self
            .host
            .world
            .minigames
            .baka_fighter
            .as_mut()
            .map(|f| f.take_cues())
            .unwrap_or_default();
        match self.bgm.as_mut() {
            Some(bgm) => {
                for &id in &cues {
                    bgm.enqueue_sfx(u16::from(id), 0, 0, 0);
                }
            }
            None => {
                for id in &cues {
                    log::debug!("baka SFX cue {id:#04x} (no audio)");
                }
            }
        }
        cues.len() as u32
    }

    /// One sim tick of the SFX scheduler while a menu-overlay screen (the
    /// pause menu, a shop, the prize exchange) freezes the field. The field's
    /// frame tail - and the audio frame it ends on - does not run under one,
    /// and this is the scheduler step the frame still owes: a cue queued by
    /// the screen sounds, and one already delayed when the screen opened keeps
    /// ageing and fires on time.
    ///
    /// Retail runs those screens at game mode `0x17`, whose per-frame handler
    /// still calls the cue drainer every frame. The duck ramp is not stepped:
    /// it belongs to the audio frame the field tail runs.
    ///
    /// `None` with no director attached.
    // REF: FUN_80025F74 (`jal FUN_80016B6C` at `0x80025F9C`)
    pub fn step_overlay_sfx(&mut self) -> Option<SfxFrameReport> {
        let report = self.bgm.as_mut()?.tick_sfx_frame();
        for (id, voice) in &report.fired {
            log::debug!("menu SFX cue {id:#04x} fired on voice {voice}");
        }
        Some(report)
    }
}

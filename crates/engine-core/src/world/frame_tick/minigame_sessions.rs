use super::*;

impl World {
    /// Enter the Noa dance (rhythm) minigame on `game`, suspending the current
    /// scene mode. The suspended mode is restored by [`World::exit_dance`] (and
    /// automatically once the song ends). Mirrors the pause-menu suspend/restore
    /// contract: the interrupted field/battle state stays intact underneath.
    ///
    /// Applies the dance stager's pad-latch clear
    /// ([`crate::dance::dance_scene_stage`]): retail zeroes `_DAT_8007B880` on
    /// the frame the hall is staged, so the confirm press that starts the
    /// minigame is not also read as its first judged note.
    pub fn enter_dance(&mut self, game: crate::dance::DanceGame) {
        // Don't stack a suspend: if the dance is already running, just swap the
        // game so a re-entry keeps the true return mode.
        if self.mode != SceneMode::Dance {
            self.minigames.dance_return_mode = self.mode;
        }
        // The pre-song count-in, and the how-to run's tutorial actor, are
        // staged here rather than in a host: retail runs `FUN_801cf470`'s
        // below-10 states before the beat clock, and owning that phase in the
        // world is what gives the **door-warp** entry a count-in on both
        // hosts. Previously only the native debug launcher ran one, from a
        // driver of its own.
        let long_song = game.song_len() == crate::dance::SONG_LEN_LONG;
        let how_to = game.mode() == crate::dance::DanceMode::HowTo;
        // State 1's flag writes, after it has read the mode off them: the
        // three one-shot mode requests are cleared (`0x801CF950..0x801CF964`;
        // the free-play flag stays) and the pass flag is raised for the
        // results state to clear on a loss (`0x801CF968`).
        // PORT: FUN_801cf470 (state 1's flag writes)
        for flag in [
            crate::dance::MODE_FLAG_FINALS,
            crate::dance::MODE_FLAG_QUALIFIER,
            crate::dance::MODE_FLAG_HOW_TO,
        ] {
            self.system_flag_clear(flag);
        }
        self.system_flag_set(crate::dance::WIN_FLAG);
        self.minigames.dance = Some(game);
        self.minigames.dance_last_judge = None;
        self.minigames.dance_countin = Some(crate::dance::CountIn::new());
        self.minigames.dance_countin_banner = None;
        self.minigames.dance_countin_go = None;
        // The dance overlay loads one of two mode-selected chart loops; the
        // exact mode -> song arm is unpinned, so it is approximated by song
        // length. Held until the count-in clears, which is when retail's
        // beat clock starts.
        self.minigames.dance_pending_bgm = Some(if long_song {
            crate::minigame_entry::DANCE_LONG_SONG_BGM_ID
        } else {
            crate::minigame_entry::DANCE_SHORT_SONG_BGM_ID
        });
        self.minigames.dance_tutorial = how_to.then(crate::dance_tutorial::DanceTutorial::new);
        self.minigames.dance_tutorial_frame = None;
        self.mode = SceneMode::Dance;
        if crate::dance::dance_scene_stage().clear_pad_latch {
            self.input.clear_edges();
        }
    }

    /// Drain the minigame SFX cue ids queued this frame (the fishing hub's
    /// and point exchange's blips; the dance stores its cues straight into
    /// the SFX ring instead, as retail does). Cosmetic - a
    /// host with no audio drops them, exactly as an unheard retail cue would
    /// be. Both hosts drain this queue, which is what keeps the two from
    /// growing separate cue paths.
    pub fn drain_minigame_sfx_cues(&mut self) -> Vec<u16> {
        std::mem::take(&mut self.minigames.pending_sfx)
    }

    /// Clear the dance minigame and return the final [`DanceGame`] so the host
    /// can read the score / pass result. Restores the interrupted mode if it is
    /// still `Dance` (a mid-song abort); when the song already auto-ended
    /// [`tick_dance`](Self::tick_dance) has restored the mode but left the game
    /// installed for one frame so the host can read it - this take clears it.
    ///
    /// **Nothing happens without a run to tear down.** Both hosts poll this
    /// from their frame path with the same `mode != Dance` test, which is
    /// true on every ordinary field frame, so an unguarded body ran the whole
    /// teardown - `restore_minigame_bgm` (self-gating, harmless) and the
    /// stager's PAD-LATCH CLEAR (not harmless) - sixty times a second. The
    /// clear forces `pad_prev = pad`, so every edge consumer that runs after
    /// the poll in its host's frame order saw no edges at all: on the browser
    /// play page, where the poll sits in `tick_minigame_ui` ahead of
    /// `tick_dev_menu`, the developer menu stopped taking input entirely.
    /// `enter_dance` is the only writer of `minigames.dance`, so "a run is
    /// installed, or the mode is still the hall" is exactly the set of frames
    /// with something to tear down.
    pub fn exit_dance(&mut self) -> Option<crate::dance::DanceGame> {
        if self.mode != SceneMode::Dance && self.minigames.dance.is_none() {
            return None;
        }
        if self.mode == SceneMode::Dance {
            self.mode = self.minigames.dance_return_mode;
        }
        self.minigames.dance_last_judge = None;
        self.minigames.dance_countin = None;
        self.minigames.dance_countin_banner = None;
        self.minigames.dance_countin_go = None;
        self.minigames.dance_pending_bgm = None;
        self.minigames.dance_tutorial = None;
        self.minigames.dance_tutorial_frame = None;
        // Give the hall its own music back when the chart loop displaced it.
        self.restore_minigame_bgm();
        // The stager runs on teardown as well as on entry, so the press that
        // leaves the hall does not carry into the restored field mode.
        if crate::dance::dance_scene_stage().clear_pad_latch {
            self.input.clear_edges();
        }
        self.minigames.dance.take()
    }

    /// Advance the dance minigame one frame: step the beat clock, judge this
    /// frame's directional presses, and end the run when the song finishes.
    ///
    /// The judged buttons are the retail ones: `FUN_801d1af4` reads the
    /// newly-pressed word `_DAT_8007B874` and tests the three face bits
    /// `0x80` / `0x20` / `0x10` = Square / Circle / Triangle, which is exactly
    /// [`crate::dance::DanceDir::pad_bit`]. This frame's pad edges are packed
    /// into that layout and the direction is picked by matching `pad_bit`, so
    /// the bit-to-direction binding lives in the ported kernel. Edge-triggered
    /// (`just_pressed`) so a held button scores at most one note per press.
    ///
    /// PORT: the dance overlay's per-frame driver (`FUN_801cf470` beat clock ->
    /// `FUN_801d1960` hit judge), one advance + one judged press pass per frame.
    pub(super) fn tick_dance(&mut self) {
        if self.minigames.dance.is_none() {
            // Mode is Dance but no game installed - drop back to a sane mode.
            self.mode = self.minigames.dance_return_mode;
            return;
        }
        // Every body's clip runs every frame, count-in included: the clip
        // driver ticks each actor whatever state the dance is in.
        if let Some(g) = self.minigames.dance.as_mut() {
            g.advance_body_clips(1);
        }
        // The pre-song count-in owns the frame while it runs: the beat clock
        // does not advance and no press is judged, which is retail's
        // below-10 state band. The song starts on the frame it clears.
        if let Some(mut ci) = self.minigames.dance_countin.take() {
            let step = ci.step();
            self.minigames.dance_countin_banner = step.banner;
            self.minigames.dance_countin_go = step.go;
            if let Some(cue) = step.cue {
                self.write_dance_ring_cue(crate::dance::STAGE_CUE_RING_SLOT, cue);
            }
            if step.done {
                self.minigames.dance_countin_banner = None;
                self.minigames.dance_countin_go = None;
                if let Some(bgm) = self.minigames.dance_pending_bgm.take() {
                    self.swap_to_minigame_bgm(bgm);
                }
            } else {
                self.minigames.dance_countin = Some(ci);
            }
            self.step_dance_tutorial();
            return;
        }
        let Some(game) = self.minigames.dance.as_mut() else {
            return;
        };
        game.advance(1);
        // Judge at most one directional press this frame (retail tests all
        // three bits in one pass, but a rhythm player presses one at a time);
        // the scan order is retail's - Triangle, then Square, then Circle.
        //
        // The engine's `PadButton` word and the packed word `FUN_8001822C`
        // builds hold the same 16 buttons with the two bytes swapped (see
        // `crate::retail_pad`: face/shoulder cluster low, dpad/system high),
        // so one rotate turns this frame's edges into the mask the retail
        // judge reads.
        use crate::dance::DanceDir;
        let pressed = (self.input.pad() & !self.input.pad_prev()).rotate_right(8);
        let dir = [DanceDir::C, DanceDir::A, DanceDir::B]
            .into_iter()
            .find(|d| pressed & d.pad_bit() != 0);
        if let Some(dir) = dir {
            self.minigames.dance_last_judge = Some(game.judge_press(dir));
        }
        let finish_cues = game.take_finish_cues();
        let award_sounds = game.take_award_sounds();
        let finished = game.finished();
        let clear_win = finished && game.results_clear_win_flag();
        // The countdown parts' move-VM op `0x1D` stores straight into ring
        // slot 3 (`DAT_8007B6DE`), as the award does.
        for cue in finish_cues {
            self.write_dance_ring_cue(crate::dance::AWARD_CUE_RING_SLOT, cue);
        }
        self.route_dance_award_sounds(&award_sounds);
        if finished {
            // Song finished: the results state grades the run into the pass
            // flag, then the interrupted mode is restored, leaving `dance` in
            // place so the host can read the final score before clearing.
            self.mode = self.minigames.dance_return_mode;
            if clear_win {
                self.system_flag_clear(crate::dance::WIN_FLAG);
            }
        }
        self.step_dance_tutorial();
    }

    /// Store one dance cue straight into a ring slot, the way every dance
    /// cue site does (`sh id, DAT_8007B6D8[slot]`, no cursor pair). Ids
    /// `>= 0x200` resolve through the dance's own descriptor bank
    /// ([`Self::runtime_sfx_bundle`]); the tutorial's `0x20` / `0x21` through
    /// the static table.
    pub(super) fn write_dance_ring_cue(&mut self, slot: u8, cue: u16) {
        self.audio
            .sfx_ring_ops
            .push(crate::world::SfxRingOp::WriteSlot(slot, cue as i16));
    }

    /// Hand the human's award sounds to the audio side: cues into ring slot
    /// 3, stings as two directly keyed voices (`FUN_801d3d78` ->
    /// `FUN_80065034`) on the queue both hosts drain. The tier-2 sting's
    /// variant is `rand() % 3` off the one world stream, as retail's
    /// `jal 0x80056798` at `0x801D2138`.
    pub(crate) fn route_dance_award_sounds(&mut self, sounds: &[crate::dance::DanceAwardSound]) {
        use crate::dance::DanceAwardSound;
        for &s in sounds {
            match s {
                DanceAwardSound::Cue(cue) => {
                    self.write_dance_ring_cue(crate::dance::AWARD_CUE_RING_SLOT, cue)
                }
                DanceAwardSound::Sting { r, random } => {
                    let r = if random {
                        (self.next_rand() % u32::from(crate::dance::STING_RANDOM_VARIANTS)) as u16
                    } else {
                        r
                    };
                    let vol = crate::other_game_overlay::cue_volume(
                        self.audio.levels.voice_volume as u32,
                    );
                    for v in crate::dance::dance_hit_sting_voices(r) {
                        self.audio
                            .sfx_voice_keys
                            .push(crate::other_game_overlay::VoiceAttrCue {
                                voice: u32::from(v.voice),
                                vab_program_tone: (
                                    i32::from(v.level),
                                    i32::from(v.program),
                                    i32::from(v.tone),
                                ),
                                note_and_fine: (i32::from(v.note), 0x40),
                                volume: (vol, vol),
                            });
                    }
                }
            }
        }
    }

    /// Run the Disco King how-to tutorial actor for one frame beside the live
    /// session, on the retail pad-word layout (the same rotate the judge
    /// applies). No-op unless the installed run is a
    /// [`crate::dance::DanceMode::HowTo`] one.
    ///
    /// The handler runs during the count-in too - retail's actor ticks
    /// independently of the dance states, and its opening prompt is what the
    /// player answers before the song.
    // REF: FUN_801D0750 (the actor handler; the per-state kernels are
    //      `crate::dance_tutorial`)
    pub(super) fn step_dance_tutorial(&mut self) {
        if self.minigames.dance_tutorial.is_none() {
            self.minigames.dance_tutorial_frame = None;
            return;
        }
        let (score, feedback_frames, combo_hit) = self
            .minigames
            .dance
            .as_ref()
            .map(|g| {
                (
                    g.score() as i32,
                    g.feedback_frames() as i32,
                    matches!(g.triangle_feedback(), Some(true)),
                )
            })
            .unwrap_or((0, 0, false));
        let pressed = (self.input.pad() & !self.input.pad_prev()).rotate_right(8);
        let Some(tut) = self.minigames.dance_tutorial.as_mut() else {
            return;
        };
        let frame = tut.step(pressed, score, feedback_frames, combo_hit, 1);
        if let Some(cue) = frame.cue {
            // `FUN_801d0750` stores its cursor / confirm blips straight into
            // ring slot 0 (`sh id, 0x8007B6D8`).
            self.write_dance_ring_cue(crate::dance::STAGE_CUE_RING_SLOT, cue);
        }
        if frame.done {
            self.minigames.dance_tutorial = None;
            self.minigames.dance_tutorial_frame = None;
        } else {
            self.minigames.dance_tutorial_frame = Some(frame);
        }
    }

    /// Enter the fishing minigame on `session`, suspending the current scene
    /// mode (restored by [`World::exit_fishing`]). Like the dance / pause-menu
    /// suspend contract, the interrupted field state stays intact underneath.
    ///
    /// Takes a ready session; [`World::enter_fishing_session`] is the entry
    /// that builds one from the persistent save-block words, which is what
    /// every player-facing and debug path uses.
    pub fn enter_fishing(&mut self, session: crate::fishing::PondSession) {
        if self.mode != SceneMode::Fishing {
            self.minigames.fishing_return_mode = self.mode;
        }
        self.minigames.fishing = Some(session);
        self.minigames.fishing_events.clear();
        self.mode = SceneMode::Fishing;
    }

    /// The persistent fishing words (`_DAT_8008444C..0x8008446C`) as the
    /// world holds them between sessions.
    pub fn fishing_persist(&self) -> crate::fishing::FishingPersist {
        let m = &self.minigames;
        crate::fishing::FishingPersist {
            lure: m.fishing_lure,
            rod: m.fishing_rod as i32,
            casts: m.fishing_casts,
            record: crate::fishing::FishingRecord {
                points: m.fishing_points,
                best_points: m.fishing_best_points,
                best_fish: m.fishing_best_fish as usize,
            },
            purchased_mask: m.fishing_prizes_purchased,
        }
    }

    /// Bank a session's persistent words back into the world - retail keeps
    /// them in the live save window, so they outlast the overlay.
    pub(super) fn bank_fishing_persist(&mut self, p: crate::fishing::FishingPersist) {
        let m = &mut self.minigames;
        m.fishing_lure = p.lure;
        m.fishing_rod = p.rod.max(0) as u32;
        m.fishing_casts = p.casts;
        m.fishing_points = p.record.points;
        m.fishing_best_points = p.record.best_points;
        m.fishing_best_fish = p.record.best_fish as u32;
        m.fishing_prizes_purchased = p.purchased_mask;
    }

    /// Open a fishing session over the decoded overlay `tables` at `venue`
    /// (`0` Buma, `1` Vidna - `DAT_801d90d0`), seeded from the persistent
    /// save-block words, and enter it.
    ///
    /// The bring-up's two ownership scans run first, as in retail: the rod
    /// scan ([`World::resolve_fishing_entry_rod`], `FUN_801CF070`) and the
    /// lure gate ([`crate::fishing::select_owned_rod`] over the lure family,
    /// `FUN_801d712c`), each writing its corrected index back. `venue_map`
    /// gives the cast lure a world to land in (the venue scene's `.MAP`).
    ///
    /// The BIOS-rand stream is seeded off the frame counter, so a repeat
    /// entry varies while a replayed pad stream stays deterministic.
    pub fn enter_fishing_session(
        &mut self,
        tables: &crate::fishing::FishingTables,
        venue: usize,
        venue_map: Option<crate::fishing::PondVenue>,
    ) {
        use crate::minigame_entry::FISHING_SEED_SALT;
        self.resolve_fishing_entry_rod();
        let bag = &self.party.inventory;
        let mut lure = self.minigames.fishing_lure;
        crate::fishing::select_owned_rod(&mut lure, |id| {
            i32::from(bag.get(&(id as u8)).copied().unwrap_or(0))
        });
        self.minigames.fishing_lure = lure;
        let seed = FISHING_SEED_SALT ^ self.frame as u32;
        let mut session =
            crate::fishing::PondSession::from_tables(tables, venue, self.fishing_persist(), seed);
        if let Some(v) = venue_map {
            session.attach_venue(v);
        }
        self.enter_fishing(session);
    }

    /// Re-point the persistent rod cell at a rod the party actually holds, and
    /// return it as a session's `rod_stat`.
    ///
    /// This is the fishing bring-up's own scan
    /// ([`crate::fishing::entry_rod_index`], retail `FUN_801CF070` at
    /// `0x801cf35c..0x801cf39c`) run against the live bag: it keeps
    /// [`crate::world::MinigameState::fishing_rod`] when that rod is held,
    /// otherwise steps forward with wrap, and lands on `0` for a party holding
    /// no rod at all - so a stale save index never divides the tension gauge.
    /// The result is written back, exactly as retail leaves it in
    /// `_DAT_80084454`, which is the same cell the persistent HUD's rod row
    /// reads.
    ///
    /// Every session entry runs it, through [`World::enter_fishing_session`]:
    /// the mode-24 door warp and both play hosts' debug launchers alike.
    pub fn resolve_fishing_entry_rod(&mut self) -> i32 {
        let bag = &self.party.inventory;
        let rod = crate::fishing::entry_rod_index(self.minigames.fishing_rod, |id| {
            i32::from(bag.get(&(id as u8)).copied().unwrap_or(0))
        });
        self.minigames.fishing_rod = rod;
        rod as i32
    }

    /// Leave the fishing minigame and restore the interrupted mode, returning
    /// the session so the host can read the final [`FishingRecord`]. Every
    /// persistent word the session carries - the point record, the cast
    /// counter, lure, rod and the one-time prize mask - is banked back into
    /// [`crate::world::MinigameState`] (retail writes those cells in place;
    /// the next session seeds from them). No-op when fishing isn't active.
    ///
    /// [`FishingRecord`]: crate::fishing::FishingRecord
    pub fn exit_fishing(&mut self) -> Option<crate::fishing::PondSession> {
        if self.mode == SceneMode::Fishing {
            self.mode = self.minigames.fishing_return_mode;
        }
        let session = self.minigames.fishing.take();
        self.minigames.fishing_exchange = None;
        self.minigames.fishing_events.clear();
        if let Some(s) = &session {
            self.bank_fishing_persist(s.persist());
        }
        session
    }

    /// Open the fishing point-exchange (prize shop) list on `exchange`.
    /// The host renders [`crate::world::MinigameState::fishing_exchange`] and commits buys through
    /// [`World::fishing_exchange_buy`].
    pub fn open_fishing_exchange(&mut self, mut exchange: crate::fishing::PrizeExchange) {
        // Row 0 hides until strictly affordable - floor the cursor to the
        // first visible row for the current point pool.
        exchange.cursor = exchange
            .cursor
            .max(exchange.first_visible(self.minigames.fishing_points));
        self.minigames.fishing_exchange = Some(exchange);
    }

    /// Close the point-exchange list.
    pub fn close_fishing_exchange(&mut self) {
        self.minigames.fishing_exchange = None;
    }

    /// Commit a point-exchange purchase of `qty` units of `row`
    /// (`FUN_801d06c8`'s Yes arm): validates through
    /// [`crate::fishing::PrizeExchange::buy`] against the persistent pool /
    /// purchased mask / live inventory count, then deducts
    /// [`crate::world::MinigameState::fishing_points`], latches the one-time bit, and grants the
    /// item into [`crate::world::PartyState::inventory`]. While a fishing session is live its
    /// record is synced to the reduced pool so the on-screen point total
    /// matches. `None` when no exchange is open or the buy doesn't validate.
    pub fn fishing_exchange_buy(
        &mut self,
        row: usize,
        qty: u32,
    ) -> Option<crate::fishing::PrizePurchase> {
        let ex = self.minigames.fishing_exchange.as_ref()?;
        let item_id = ex.rows.get(row)?.item_id;
        let owned = *self.party.inventory.get(&item_id).unwrap_or(&0) as u32;
        let purchase = ex.buy(
            row,
            qty,
            self.minigames.fishing_points,
            owned,
            self.minigames.fishing_prizes_purchased,
        )?;
        self.minigames.fishing_points -= purchase.cost as i32;
        if let Some(bit) = purchase.latched_bit {
            self.minigames.fishing_prizes_purchased |= 1 << bit;
        }
        let count = self.party.inventory.entry(purchase.item_id).or_insert(0);
        *count = count.saturating_add(purchase.qty.min(255) as u8);
        if let Some(s) = &mut self.minigames.fishing {
            s.record.points = self.minigames.fishing_points;
            s.purchased_mask = self.minigames.fishing_prizes_purchased;
        }
        Some(purchase)
    }

    /// Advance the fishing minigame one frame, reading this frame's pad into
    /// the session's [`PondInput`](crate::fishing::PondInput):
    ///
    /// - **Cast / confirm** is the [`Circle`] edge (retail packed bit
    ///   `0x20`): it starts the wind-up at the idle shore, locks the power
    ///   meter, and dismisses a resolved fight.
    /// - **Reel** is the held [`Cross`] (`0x40`, reel A) / [`Square`]
    ///   (`0x80`, reel B) pair, rebuilt into the retail held word
    ///   `_DAT_8007b850` and classified by the ported decoder inside the
    ///   session, so holding both resolves to reel A as retail does.
    /// - **Strike credit**: the pre-hook band check's pad nudge, counted by
    ///   [`crate::fishing_actors::bite_pad_nudge`] - one per fresh D-pad
    ///   left, D-pad right, and reel press, the reel pair counting once
    ///   ([`PondInput::from_engine_pad`](crate::fishing::PondInput::from_engine_pad)).
    ///
    /// The frame's [`PondEvent`](crate::fishing::PondEvent)s land in
    /// [`crate::world::MinigameState::fishing_events`] for the hosts' banner
    /// one-shots, and the two sound-bearing ones queue their cues here.
    ///
    /// [`Circle`]: input::PadButton::Circle
    /// [`Cross`]: input::PadButton::Cross
    /// [`Square`]: input::PadButton::Square
    ///
    /// PORT: FUN_801cf3bc (the fishing overlay's driver, reached through its
    /// actor-template tick word). What this covers: the run-loop states
    /// `0xc` idle / `0xd` wind-up / `0x14` power oscillator / `0x1e..0x22`
    /// lure flight, then the pre-hook band roll and the hooked fight, which
    /// retail runs from the fish actor's handler `FUN_801d26cc` into
    /// `FUN_801d4004` rather than from the mode switch. Not covered here:
    /// the rod/type select (state `0`), the fade ramps, the no-lure end
    /// screen (`0x96`) and the exit fade (`200`) - the hosts' Start-to-leave
    /// affordance replaces the last - and the driver tail's HUD, banner
    /// timers and sub-screen, which the hosts compose. The point-exchange
    /// branch (`0x64..0x7a`) is [`World::fishing_exchange_input`]. The
    /// casting-meter step is not byte-pinned; `FISHING_CAST_STEP` is the
    /// host rate.
    pub(super) fn tick_fishing(&mut self) {
        use crate::fishing::{PondEvent, PondInput};
        /// Per-frame casting-meter step (see the method note - not byte-pinned).
        const FISHING_CAST_STEP: i32 = 0x80;
        /// The lure-landing cue and the ring slot it is stored into.
        const LURE_DOWN_CUE: i16 = 0x204;
        const LURE_DOWN_CUE_SLOT: u8 = 2;
        /// Packed spread argument the strike splash fans its three parts by.
        /// Direct form (bit [`crate::fishing_chrome::SPLASH_SUB_BLOCK_BIT`]
        /// clear); the value is the play window's, carried over unchanged.
        const SPLASH_SPREAD: i32 = 0x40;
        if self.minigames.fishing.is_none() {
            // Mode is Fishing but no session installed - drop back to a sane mode.
            self.mode = self.minigames.fishing_return_mode;
            return;
        }
        // The venue's hub menu (Triangle / Select on the idle shore) owns the
        // frame while it is up, the exchange list it opens included.
        if self.tick_fishing_hub() {
            self.minigames.fishing_events.clear();
            return;
        }
        // The point-exchange sub-screen owns the pad while it is open, as
        // retail's shop branch owns the mode switch.
        if self.minigames.fishing_exchange.is_some() {
            self.minigames.fishing_events.clear();
            return;
        }
        let pond_input = PondInput::from_engine_pad(self.input.pad(), self.input.pad_prev());
        let (events, lure_down) = match self.minigames.fishing.as_mut() {
            Some(s) => {
                let casts = s.persist().casts;
                s.tick(pond_input, 1, FISHING_CAST_STEP);
                for _ in 0..s.take_rod_creaks() {
                    // The hooked rod's creak, `0x201` into ring slot 1.
                    self.audio
                        .sfx_ring_ops
                        .push(crate::world::SfxRingOp::WriteSlot(1, 0x201));
                }
                (s.take_events(), s.persist().casts != casts)
            }
            None => (Vec::new(), false),
        };
        // The lure landing: the arm of the lure tick that bumps the
        // persistent cast counter stores cue `0x204` straight into ring
        // slot 2 (`sh v0,-0x4924(v1)` at `0x801D2950`, `FUN_801d26cc`). A
        // runtime-bank id, resolved through the venue scene's own bundle -
        // the fishing init loads no `efect.dat` of its own.
        if lure_down {
            self.audio
                .sfx_ring_ops
                .push(crate::world::SfxRingOp::WriteSlot(
                    LURE_DOWN_CUE_SLOT,
                    LURE_DOWN_CUE,
                ));
        }
        for e in &events {
            // The cadence-match strike splash spawns its three parts into
            // the shared effect pool. The producer is the session's own
            // event, not a venue actor, so every host that ticks the world
            // gets the burst.
            if matches!(e, PondEvent::Splash) {
                let parts = crate::fishing_chrome::splash_burst(
                    crate::fishing_actors::SCREEN_CENTRE.0,
                    crate::fishing_actors::SCREEN_CENTRE.1,
                    crate::minigame_fx::SPLASH_SPRITE_ID,
                    SPLASH_SPREAD,
                );
                self.minigames.fx.spawn_splash(&parts);
            }
            // The hook and catch cues - one kernel shared with the minigames
            // page. The hook cue used to live on the native window's line
            // actor, which made the strike audible on one surface; the cue
            // queue is drained by every host.
            self.minigames
                .pending_sfx
                .extend(super::minigame_state::pond_event_cues(e));
        }
        self.minigames.fishing_events = events;
    }

    /// Enter the casino slot-machine minigame on `machine`, suspending the
    /// current scene mode (restored by [`World::exit_slot_machine`]). Like
    /// the dance / fishing / pause-menu suspend contract, the interrupted
    /// field state stays intact underneath.
    pub fn enter_slot_machine(&mut self, machine: crate::slot_machine::SlotMachine) {
        if self.mode != SceneMode::SlotMachine {
            self.minigames.slot_return_mode = self.mode;
        }
        self.minigames.slot_machine = Some(machine);
        self.mode = SceneMode::SlotMachine;
    }

    /// Leave the slot machine and restore the interrupted mode, committing
    /// the session's final balance into the casino coin bank
    /// ([`crate::world::MinigameState::casino_coins`] - the retail state-100 assignment
    /// `_DAT_800845A4 = DAT_801d4114`). Returns the session so the host can
    /// read the final state. No-op when the machine isn't active.
    pub fn exit_slot_machine(&mut self) -> Option<crate::slot_machine::SlotMachine> {
        if self.mode == SceneMode::SlotMachine {
            self.mode = self.minigames.slot_return_mode;
        }
        let mut machine = self.minigames.slot_machine.take();
        if let Some(m) = machine.as_mut() {
            self.minigames.casino_coins = m.cash_out().max(0) as u32;
            // Leaving mid-spin must not strand the reel motor.
            self.audio
                .sfx_voice_stops
                .push(crate::slot_machine::SPIN_VOICE);
        }
        machine
    }

    /// The five mode-24 minigame scene modes, in `sub_id` order.
    pub const MINIGAME_MODES: [SceneMode; 5] = [
        SceneMode::Fishing,
        SceneMode::SlotMachine,
        SceneMode::BakaFighter,
        SceneMode::MuscleDome,
        SceneMode::Dance,
    ];

    /// Whether the world is inside one of the five mode-24 minigames.
    pub fn in_minigame(&self) -> bool {
        Self::MINIGAME_MODES.contains(&self.mode)
    }

    /// **Engine affordance, not a retail port: Start leaves any minigame.**
    ///
    /// Retail's five minigames each quit through their own overlay's SM - the
    /// slot cabinet's exit menu row, the duel's decided-match confirm, the
    /// arena's give-up arm - and every one of those is a *different* control
    /// in a *different* overlay. Some are ported (the cabinet's cash-out quit
    /// row through `SlotMachine::cash_out_input`, the duel cabinet's PAY OUT
    /// choice), but each is reachable only from its own game's screens and
    /// not every game's exit is, so without one shared exit an entered
    /// minigame can still strand the player in a mode with the BGM running.
    ///
    /// That is the invariant [`crate::scene::SceneHost::drain_minigame_warp`]
    /// already states for its *failure* arms - "a script that armed a warp must
    /// never be left in a mode with no exit" - and it has to hold for the
    /// successful ones too, on every host, or a reachable minigame is a
    /// softlock. Each game's own `exit_*` runs, so the bookkeeping (cash-out,
    /// leg report, point bank) is the same as the deliberate exit; a door-warp
    /// entry additionally closes its round trip through
    /// [`Self::minigame_return_warp`].
    pub(super) fn poll_minigame_escape(&mut self) {
        if !self.in_minigame() || !self.input.just_pressed(input::PadButton::Start) {
            return;
        }
        match self.mode {
            SceneMode::Fishing => {
                self.exit_fishing();
            }
            SceneMode::SlotMachine => {
                self.exit_slot_machine();
            }
            // The attract card reads Start itself (its `0x844` edge is
            // Start, Cross or L1), so there Start begins rather than quits;
            // from the player select on it quits as everywhere else.
            SceneMode::BakaFighter
                if self
                    .minigames
                    .baka_fighter
                    .as_ref()
                    .is_some_and(|f| f.cabinet().state() == crate::baka_cabinet::ST_ATTRACT) =>
            {
                return;
            }
            SceneMode::BakaFighter => {
                self.exit_baka_fighter();
            }
            SceneMode::MuscleDome => {
                self.leave_muscle_dome();
            }
            SceneMode::Dance => {
                self.exit_dance();
            }
            _ => unreachable!("guarded by in_minigame"),
        }
        self.close_minigame_round_trip();
    }

    /// Close the mode-24 round trip after a minigame exit, when the entry
    /// came through the door warp (a backed-up scene name is armed):
    /// [`Self::minigame_return_warp`] restores the departure label, banks the
    /// session winnings and drops back to the field. A no-op for a session a
    /// debug launcher opened (nothing armed), and after `exit_baka_fighter`,
    /// which runs its own return warp.
    ///
    /// Every exit path calls it after its `exit_*`: the Start escape here,
    /// the native window's minigame hotkeys and the browser page's fishing
    /// button. The hotkeys and the button used to call the bare `exit_*`, so
    /// leaving a door-entered session that way kept the scene backup armed
    /// and never banked the winnings - only Start closed the trip.
    pub fn close_minigame_round_trip(&mut self) {
        if self.minigames.scene_backup.is_some() {
            self.minigame_return_warp();
        }
    }

    /// Arm the mode-24 minigame door-warp: back up the active scene name and
    /// zero the session-winnings accumulator, so [`Self::minigame_return_warp`]
    /// can round-trip back to the departure scene.
    ///
    /// Mirrors the two retail halves of the entry: the field-VM `0x3E` warp
    /// arm zeroes the winnings accumulator `_DAT_80084440`, and the mode-24
    /// OTHER-INIT entry `FUN_80025980` copies the active scene name
    /// `0x80084548` into the backup at `0x8007BAE8` before the minigame
    /// overlay clobbers the field.
    // REF: FUN_80025980 (scene-name backup half), FUN_801DE840 case 0x3E
    //      (winnings-accumulator zero half)
    pub fn arm_minigame_warp(&mut self) {
        self.minigames.scene_backup = Some(self.active_scene_label.clone());
        self.minigames.winnings = 0;
    }

    /// Request the mode-24 door-warp into `sub_id` from outside a script -
    /// the developer launchers (the native window's minigame hotkeys, the
    /// browser page's `play_mg_debug_warp`). Arms the round trip exactly as
    /// the op-`0x3E` arm does and leaves the `sub_id` for the scene host's
    /// next tick to drain (`SceneHost::drain_minigame_warp`), so a launcher
    /// enters the same session, with the same BGM swap and the same return
    /// warp, as walking through the casino door.
    ///
    /// A launcher into the Muscle Dome also lists the hub's announcer lines
    /// for the prestage drain here, at the request: a host that drains the
    /// list before its next tick (the browser page's launcher does) gets the
    /// lead the door scene's own list gives a walked door
    /// ([`crate::world::field_xa::scene_minigame_door_xa_prestage`]).
    pub fn request_minigame_warp(&mut self, sub_id: u8) {
        self.arm_minigame_warp();
        self.minigames.pending_warp = Some(sub_id);
        if crate::minigame_entry::MinigameSubId::from_sub_id(sub_id)
            == Some(crate::minigame_entry::MinigameSubId::MuscleDome)
        {
            self.queue_xa_prestage(crate::muscle_ringside::hub_xa_prestage());
        }
    }

    /// Mode-24 minigame exit / return-warp: restore the backed-up scene name
    /// into [`Self::active_scene_label`], commit the session winnings into
    /// the casino coin bank (`casino_coins += minigame_winnings`, saturating
    /// at the retail `9_999_999` cap), and drop back to [`SceneMode::Field`]
    /// (retail latches `_DAT_8007B83C = 2`, mode 2 MAIN INIT, whose
    /// per-scene initializer reloads the restored scene; the engine keeps
    /// the field state resident underneath its minigame sessions, so
    /// restoring the label + mode completes the same round trip without a
    /// reload).
    ///
    /// Distinct from the slot overlay's cash-out ([`Self::exit_slot_machine`],
    /// an *assignment* into the bank): this commit is a delta-add of the
    /// accumulator (`_DAT_800845A4 += _DAT_80084440`).
    ///
    /// The winnings commit runs even when no warp is armed (retail's add is
    /// unconditional); only the name restore needs the backup.
    // PORT: FUN_80026018
    pub fn minigame_return_warp(&mut self) {
        self.minigames.casino_coins = self
            .minigames
            .casino_coins
            .saturating_add(self.minigames.winnings)
            .min(9_999_999);
        if let Some(name) = self.minigames.scene_backup.take() {
            self.active_scene_label = name;
        }
        // Give the departure scene its own track back when the minigame's
        // overlay init took the score over (dance / Baka / dome). No-op for
        // the two slots that never displaced it.
        self.restore_minigame_bgm();
        self.mode = SceneMode::Field;
    }

    /// Advance the slot machine one frame, reading this frame's pad:
    ///
    /// - **Idle**: a [`Cross`](input::PadButton::Cross) press charges the
    ///   flat bet (3 coins, 1 in feature modes) and spins - all five
    ///   paylines play on every spin.
    /// - **Spinning**: the spin-up timer runs down on its own.
    /// - **Stopping**: the three reels have a stop button each, read off the
    ///   edge word `_DAT_8007B874` the way the reel SM does it
    ///   (`0x801CF70C..0x801CF7E0`): Square (`0x80`) stops reel 0, Cross
    ///   (`0x40`) reel 1, Circle (`0x20`) reel 2. The three tests are
    ///   independent - each gated on its own reel still running - so one
    ///   frame can stop several reels.
    /// - **Payout**: a [`Cross`] press collects the win into the balance.
    ///
    /// [`Cross`]: input::PadButton::Cross
    ///
    /// PORT: the slot overlay's per-frame driver (`FUN_801cf0d8` reel SM;
    /// the confirmed kernels live in [`crate::slot_machine`]).
    pub(super) fn tick_slot_machine(&mut self) {
        use crate::slot_machine::SlotFrameOutcome;
        let packed = crate::slot_machine::packed_edges(self.input.pad(), self.input.pad_prev());
        let Some(m) = self.minigames.slot_machine.as_mut() else {
            // Mode is SlotMachine but no session installed - drop back.
            self.mode = self.minigames.slot_return_mode;
            return;
        };
        // The cabinet's whole frame (`SlotMachine::frame`), shared with the
        // standalone minigames page.
        match m.frame(packed) {
            SlotFrameOutcome::CashedOut => {
                // State 100's tail: the bank commit and the return warp
                // (`FUN_80026018`), the same pair the Start escape runs.
                self.route_slot_sounds();
                self.exit_slot_machine();
                self.close_minigame_round_trip();
                return;
            }
            // Committed: restore the interrupted mode (the host reads the
            // session out via [`World::exit_slot_machine`]).
            SlotFrameOutcome::Committed => self.mode = self.minigames.slot_return_mode,
            SlotFrameOutcome::Stepped => {}
        }
        self.route_slot_sounds();
    }

    /// Hand the machine's sound writes to the audio side: its ring stores
    /// as [`SfxRingOp::WriteSlot`] (resolved against
    /// [`World::runtime_sfx_bundle`]), its motor voice through the direct
    /// key / release queues both hosts drain.
    ///
    /// [`SfxRingOp::WriteSlot`]: crate::world::SfxRingOp::WriteSlot
    pub(super) fn route_slot_sounds(&mut self) {
        let Some(m) = self.minigames.slot_machine.as_mut() else {
            return;
        };
        let sounds = m.take_sounds();
        for (slot, id) in sounds.ring {
            self.audio
                .sfx_ring_ops
                .push(crate::world::SfxRingOp::WriteSlot(slot, id));
        }
        self.audio.sfx_voice_keys.extend(sounds.voice_on);
        self.audio.sfx_voice_stops.extend(sounds.voice_off);
    }

    /// Enter the Baka Fighter duel on `fight`, suspending the current scene
    /// mode (restored by [`World::exit_baka_fighter`]). Like the dance /
    /// fishing / slot / pause-menu suspend contract, the interrupted field
    /// state stays intact underneath.
    pub fn enter_baka_fighter(&mut self, fight: crate::baka_fighter::BakaFight) {
        if self.mode != SceneMode::BakaFighter {
            self.minigames.baka_return_mode = self.mode;
        }
        // Retail reaches the duel through the mode-24 door warp: the field-VM
        // `0x3E` arm zeroes the winnings accumulator `_DAT_80084440` and the
        // mode-24 OTHER-INIT `FUN_80025980` backs up the active scene name.
        // Only a field entry goes through that warp; an engine-only entry from
        // another mode keeps the plain suspend/restore contract.
        if self.minigames.baka_return_mode == SceneMode::Field {
            self.arm_minigame_warp();
        }
        self.minigames.baka_fighter = Some(fight);
        self.mode = SceneMode::BakaFighter;
        self.queue_baka_xa_prestage();
    }

    /// Leave the Baka Fighter duel through the mode-24 return warp
    /// ([`Self::minigame_return_warp`], retail `FUN_80026018`): the winnings
    /// accumulator is banked into [`crate::world::MinigameState::casino_coins`], the backed-up scene
    /// name is restored and the mode drops back to the field.
    ///
    /// On a decided match with a player win, whatever prize the end-of-match
    /// tally has not yet drained is added to the accumulator first - retail's
    /// tally (`FUN_801D239C` at `0x801D28A8..0x801D28BC`) drains
    /// `DAT_801DBEE8` into `_DAT_80084440` a step at a time while the result
    /// screen is up, so leaving early has to bank the remainder for the total
    /// paid to match the prize either way.
    ///
    /// Returns the fight so the host can read the final state. No-op when no
    /// duel is active.
    pub fn exit_baka_fighter(&mut self) -> Option<crate::baka_fighter::BakaFight> {
        let fight = self.minigames.baka_fighter.take();
        if let Some(f) = fight.as_ref()
            && f.winner() == Some(0)
        {
            let owed = f.tally_gold_remaining().max(0) as u32;
            self.minigames.winnings = self.minigames.winnings.saturating_add(owed);
        }
        let return_mode = self.minigames.baka_return_mode;
        if self.mode == SceneMode::BakaFighter {
            // The warp's own mode write is retail's mode-2 (field) latch. An
            // engine-only entry from another mode restores that mode instead,
            // keeping the suspend contract the other minigames use.
            self.minigame_return_warp();
            if return_mode != SceneMode::Field {
                self.mode = return_mode;
            }
        }
        fight
    }

    /// Advance the Baka Fighter duel one frame, reading this frame's pad:
    ///
    /// - [`Square`](input::PadButton::Square) / [`Circle`](input::PadButton::Circle)
    ///   / [`Cross`](input::PadButton::Cross) commit attack types 1 / 2 / 3
    ///   for the player slot - retail's slot-0 read of the edge word
    ///   `_DAT_8007B874` (`andi 0x80` -> type 1 at `0x801D43B4`, `0x20` ->
    ///   type 2 at `0x801D43CC`, `0x40` -> type 3 at `0x801D43E4`, in
    ///   Legaia's packed pad layout). The three tests run in that order and
    ///   each overwrites the last, so on a frame with several edges Cross
    ///   wins, then Circle.
    /// - [`Triangle`](input::PadButton::Triangle) commits the chargeable
    ///   special (type 4) - a port enhancement: retail has no button for
    ///   type 4, which is its auto-finisher (`0x801D4550`).
    /// - The CPU slot picks through the ported `FUN_801d487c` roll inside
    ///   [`crate::baka_fighter::BakaFight::tick`].
    /// - When the match is decided, the frame's packed pad edge goes to the
    ///   cabinet (`FUN_801CF388`, [`crate::baka_cabinet::BakaCabinet`]),
    ///   which runs the rest of the **ladder** inside this one mode-24 visit,
    ///   as retail does: a win reaches the tally and then the "NEXT GAME /
    ///   PAY OUT" choice (Left / Right, confirm Cross); NEXT GAME seats the
    ///   next rung's opponent through the cabinet's install state, PAY OUT
    ///   and the all-clear run the exit state. A loss runs "GAME OVER",
    ///   whose first frame zeroes the prize accumulator (`sw zero,0x300(s2)`
    ///   = `_DAT_80084440` at `0x801D1288`), then the exit. The exit's fade
    ///   ending is where the return warp ([`World::exit_baka_fighter`])
    ///   banks what is left into the coin bank.
    ///
    /// The ladder therefore never outlives the visit: every exit of the
    /// cabinet runs through state `0x1F4`, and there is no save point inside
    /// mode 24, so it has no save representation to carry - the only
    /// persistent result is the coin bank.
    ///
    /// PORT: the Baka Fighter per-frame drive (`FUN_801d3f44` player input →
    /// type commit; `FUN_801d3468` resolution SM via `BakaFight::tick`).
    pub(super) fn tick_baka_fighter(&mut self) {
        if self.minigames.baka_fighter.is_none() {
            // Mode is BakaFighter but no fight installed - drop back.
            self.mode = self.minigames.baka_return_mode;
            return;
        }
        // The cabinet's whole frame (`BakaFight::frame`), shared with the
        // standalone minigames page: the front end, the duel's throw, and
        // after a match the tally / NEXT GAME / PAY OUT sheet. Each drained
        // tally step banks into the mode-24 winnings accumulator exactly as
        // retail's `FUN_801D239C` adds it into `_DAT_80084440` - the coin
        // prize, not party gold (`0x8008459C`); the exit warp
        // ([`Self::minigame_return_warp`]) then pays the accumulator into the
        // casino coin bank.
        let edge = crate::dev_menu::retail_packed(self.input.pad() & !self.input.pad_prev());
        let held = crate::dev_menu::retail_packed(self.input.pad());
        let pot = self.minigames.winnings;
        let out = match self.minigames.baka_fighter.as_mut() {
            Some(f) => f.frame(edge, held, pot),
            None => return,
        };
        if out.paid > 0 {
            self.minigames.winnings = self.minigames.winnings.saturating_add(out.paid);
        }
        if out.forfeit {
            self.minigames.winnings = 0;
        }
        if out.exit {
            self.exit_baka_fighter();
            return;
        }
        self.queue_baka_xa_prestage();
    }

    /// The duel chrome's announcer lines not yet listed, onto the prestage
    /// list both hosts drain ([`Self::queue_xa_prestage`]).
    pub(super) fn queue_baka_xa_prestage(&mut self) {
        let lines = self
            .minigames
            .baka_fighter
            .as_mut()
            .map(|f| f.take_xa_prestage())
            .unwrap_or_default();
        self.queue_xa_prestage(lines);
    }

    /// Enter the Muscle Dome contest on `session`, suspending the current
    /// scene mode (restored by [`World::exit_muscle_dome`]). Same suspend
    /// contract as the other minigames / the pause menu.
    ///
    /// A contest that has just been opened also runs its **start restore**
    /// here: the arena refills the lead fighter's HP / MP / SP to their
    /// maxima, and on every course above Beginner strips the four gear slots
    /// first. Retail does this in the arena entry's first-entry arm, so it
    /// fires once per contest and not at a leg boundary
    /// ([`crate::muscle_dome::DomeContest::take_start_restore`]).
    ///
    /// PORT: FUN_801d0ed8 (the apply site; the body is
    /// `muscle_dome::apply_contest_start_restore`)
    pub fn enter_muscle_dome(&mut self, session: crate::muscle_dome::MuscleDomeSession) {
        if self.mode != SceneMode::MuscleDome {
            self.minigames.muscle_return_mode = self.mode;
        }
        if let Some(restore) = self
            .minigames
            .muscle_contest
            .as_mut()
            .and_then(|c| c.take_start_restore())
            && let Some(rec) = self.party.roster.members.first_mut()
        {
            crate::muscle_dome::apply_contest_start_restore(rec, restore);
        }
        let mut session = session;
        session.arm_intro();
        self.minigames.muscle_dome = Some(session);
        self.minigames.muscle_hub_between_legs = false;
        self.minigames.muscle_playback_frames = 0;
        self.mode = SceneMode::MuscleDome;
        // The hub's announcer lines, staged ahead of the first visit's arms.
        self.queue_xa_prestage(crate::muscle_ringside::hub_xa_prestage());
    }

    /// Leave the arena **at the player's request** - the escape both hosts
    /// share (`Start` through [`Self::poll_minigame_escape`], and the native
    /// window's `M` hotkey).
    ///
    /// Leaving is not only [`Self::exit_muscle_dome`]: the leg has to be
    /// reported to the open contest, or the ladder carries on as if the leg
    /// never happened. A leg left undecided is the arena's run / give-up path
    /// ([`crate::muscle_dome::LEG_OUTCOME_RAN`], retail's `_DAT_80084448 = 4`
    /// arm), which ends the contest and voids the tally; a decided leg
    /// reports its own result, exactly as the Won / Lost confirm does. A
    /// contest that has run out then settles on the spot.
    ///
    /// Only the native hotkey used to report; the shared escape exited
    /// without a report, so on the browser play page a left leg kept the
    /// contest open with its tally intact.
    pub fn leave_muscle_dome(&mut self) -> Option<crate::muscle_dome::MuscleDomeSession> {
        use crate::muscle_dome::{LEG_OUTCOME_RAN, LegReport, MusclePhase};
        if self.minigames.muscle_hub_between_legs && self.minigames.muscle_dome.is_none() {
            // Between legs: the last leg is already reported, so leaving
            // here is giving up the next one - the run / give-up arm, which
            // ends the contest and voids the tally.
            self.leave_muscle_arena();
            self.report_muscle_leg(LegReport {
                survived: true,
                outcome: LEG_OUTCOME_RAN,
                turns_taken: 0,
            });
            self.settle_muscle_contest();
            return None;
        }
        let s = self.exit_muscle_dome()?;
        let phase = s.phase();
        let decided = matches!(phase, MusclePhase::Won | MusclePhase::Lost);
        self.report_muscle_leg(LegReport {
            survived: phase != MusclePhase::Lost,
            outcome: if decided { 0 } else { LEG_OUTCOME_RAN },
            turns_taken: s.turn(),
        });
        self.settle_muscle_contest();
        Some(s)
    }

    /// Leave the Muscle Dome and restore the interrupted mode.
    ///
    /// **A leg pays nothing.** The finished leg is reported to the open
    /// contest ([`World::report_muscle_leg`]), which is what decides whether
    /// the ladder carries on and what the run is eventually worth; a contest
    /// that has reached its end settles here and pays into the coin bank
    /// ([`World::settle_muscle_contest`]).
    ///
    /// It used to credit a Seru capture on a won leg, keyed off the victory
    /// caption's spell id. That was a misattribution: the caption table the
    /// id indexes is the shared battle-family cast-caption table, reached by
    /// any cast in any battle overlay, and the arena grants no Seru at all.
    ///
    /// Returns the session so the host can read the final state.
    pub fn exit_muscle_dome(&mut self) -> Option<crate::muscle_dome::MuscleDomeSession> {
        self.leave_muscle_arena();
        self.end_muscle_leg()
    }

    /// Hand the frame back from the arena: the interrupted mode and the
    /// venue's own music, and no between-legs hub left open.
    pub(super) fn leave_muscle_arena(&mut self) {
        self.minigames.muscle_hub_between_legs = false;
        if self.mode == SceneMode::MuscleDome {
            self.mode = self.minigames.muscle_return_mode;
        }
        // Give the venue its own music back when the arena's battle theme
        // displaced it (no-op when it did not).
        self.restore_minigame_bgm();
    }

    /// The battle end of one leg, which leaves the mode alone: take the
    /// session, write the fighter's HP back and pick the ringside still.
    pub(super) fn end_muscle_leg(&mut self) -> Option<crate::muscle_dome::MuscleDomeSession> {
        let session = self.minigames.muscle_dome.take();
        // The battle end writes the fighter's HP back into the lead record
        // (`+0x106`), and the background read that follows streams one of the
        // two ringside stills into VRAM, picked off that record: live HP
        // `+0x106` against the base maximum `+0x11C`, not the effective
        // `+0x104` (`FUN_801F6B24`, `0x801F6B8C..0x801F6BAC`).
        if let Some(s) = session.as_ref() {
            let fought = s.hp(0).clamp(0, i32::from(u16::MAX)) as u16;
            let (hp_cur, hp_max) = match self.party.roster.members.first_mut() {
                Some(rec) => {
                    let mut hms = rec.hp_mp_sp();
                    // A hand-built record with no maximum has nothing to
                    // write back into; the pick reads the fight's HP.
                    if hms.hp_max > 0 {
                        hms.hp_cur = fought.min(hms.hp_max);
                        rec.set_hp_mp_sp(hms);
                    } else {
                        hms.hp_cur = fought;
                    }
                    (hms.hp_cur, rec.record_stats().hp_max)
                }
                None => (fought, 0),
            };
            self.minigames.muscle_ringside_still =
                Some(crate::muscle_ringside::still_prot_index(hp_cur, hp_max));
        }
        session
    }

    /// Report the finished leg to the open contest and step the between-leg
    /// hub, applying the HP the recovery lanes hand back to the lead fighter.
    ///
    /// This is the arena's own re-entry: the ladder advances one leg, the new
    /// `(course, round)` decodes out of the sub-id word, and the hub decides
    /// between staging another leg and settling. Returns the contest state it
    /// landed in, or `None` when no contest is open.
    ///
    /// PORT: FUN_801cea6c (contest re-entry) / FUN_801cf870 states 0x0A..0x0C
    pub fn report_muscle_leg(
        &mut self,
        report: crate::muscle_dome::LegReport,
    ) -> Option<crate::muscle_dome::ContestState> {
        use crate::muscle_dome::ContestState;
        let flags = self.muscle_contest_flags();
        let hp_max = self
            .party
            .roster
            .members
            .first()
            .map(|r| r.hp_mp_sp().hp_max)
            .filter(|&hp| hp > 0)
            .unwrap_or(500);
        let contest = self.minigames.muscle_contest.as_mut()?;
        contest.finish_leg(report, hp_max, &flags);
        // The three recovery lanes drain, then the restore state hands the
        // total back to the fighter - a dome contest costs no permanent HP.
        while matches!(
            contest.state(),
            ContestState::LegScore | ContestState::Tally | ContestState::Restore
        ) {
            let restoring = contest.state() == ContestState::Restore;
            contest.advance();
            if restoring {
                // The retail store is the game-state window's `+0x6CC` /
                // `+0x6CE` pair, which is the lead party record's own
                // `+0x104` / `+0x106` HP pair (`0x80084708 - 0x80084140 =
                // 0x5C8`).
                let mut hms = match self.party.roster.members.first() {
                    Some(r) => r.hp_mp_sp(),
                    None => break,
                };
                hms.hp_cur = self
                    .minigames
                    .muscle_contest
                    .as_mut()?
                    .take_hp_restore(hms.hp_cur, hp_max);
                if let Some(rec) = self.party.roster.members.first_mut() {
                    rec.set_hp_mp_sp(hms);
                }
                return Some(self.minigames.muscle_contest.as_ref()?.state());
            }
        }
        Some(contest.state())
    }

    /// The story-flag reads the contest rules need, sampled off the system
    /// flag bank.
    pub fn muscle_contest_flags(&self) -> crate::muscle_dome::ContestFlags {
        use crate::muscle_dome as md;
        let mut flags = md::ContestFlags::default();
        for (i, &(id, _)) in md::COURSE_UNLOCK_FLAGS.iter().enumerate() {
            flags.course_unlock[i] = self.system_flag_test(id);
        }
        for (i, &(_, id)) in md::MASTER_LENGTH_GATES.iter().enumerate() {
            flags.master_gates[i] = self.system_flag_test(id);
        }
        flags.prize_awarded = self.system_flag_test(md::CONTEST_PRIZE_FLAG);
        flags
    }

    /// The **special-battle word** `0x8007BAC0` a dome leg opens on, as
    /// retail's arena entry seeds it: `1` with no course unlocked, then
    /// `0x101` / `0x111` / `0x321` for story flags `0x536` / `0x537` /
    /// `0x538`, the last set flag winning.
    ///
    /// The word is what crosses the ring's Item chip out on every unlocked
    /// course and the Ra-Seru chip on Master
    /// ([`crate::muscle_dome::SPECIAL_ITEM_FORBIDDEN`] /
    /// [`crate::muscle_dome::SPECIAL_MAGIC_FORBIDDEN`]). Both dome entry
    /// paths hand it to
    /// [`crate::muscle_dome::MuscleDomeSession::set_special_word`].
    ///
    /// An arena re-entered with the word already non-zero only advances the
    /// ladder cursor (`0x801CEC00`); the port keeps the cursor on
    /// [`crate::muscle_dome::DomeContest`] instead, so every leg re-derives
    /// the word from the same flags and the high bits are stable across the
    /// contest - which is the property retail's low-byte-only restamp has.
    ///
    /// REF: FUN_801cea6c (`0x801CEB44..0x801CEBC8`; this samples the flag
    /// bank for [`crate::muscle_dome::contest_entry_word`], which is the port)
    pub fn dome_special_word(&self) -> u32 {
        crate::muscle_dome::contest_entry_word(&self.muscle_contest_flags())
    }

    /// The special-battle word `_DAT_8007BAC0` as the battle's `!= 0` readers
    /// see it: the open arena leg's word ORed with the regular battle's half
    /// ([`crate::world::BattleState::special_word`], whose `0x200` battle init
    /// and the formation roll raise for monster `0xAF` and the Rim Elm
    /// ambush). Retail has one word; the port keeps the two halves apart
    /// because the arena session owns its own.
    ///
    /// Every reader that tests the whole word reads this, and each is a
    /// "no spoils / no escape hatch" gate - the arena restriction bits and the
    /// Ra-Seru bit suppress the same things:
    ///
    /// | Reader | Site | Suppresses |
    /// |---|---|---|
    /// | `FUN_8004E568` | `0x8004F0AC` | the gold award (zeroed after the Golden Book bonus) |
    /// | `FUN_8004E568` | `0x8004F274` | the per-member EXP (`s6 = 0`) |
    /// | `FUN_8004E568` | `0x8004F480` | the victory drop roll's seat loop |
    /// | `FUN_8004AD80` | `0x8004B48C` | the steal attack |
    /// | `FUN_801E91E8` | `0x801E9224` | the Seru absorb (reports "already known") |
    /// | `FUN_801DDB30` | `0x801DE450` | the summon spell-XP accrual |
    /// | `FUN_801E9FD4` | `0x801EA994` | a monster flee the roll granted |
    ///
    /// REF: FUN_8004E568, FUN_8004AD80, FUN_801E91E8, FUN_801DDB30, FUN_801E9FD4
    pub fn special_battle_word(&self) -> u32 {
        self.minigames
            .muscle_dome
            .as_ref()
            .map_or(0, |s| s.special_word())
            | self.battle.special_word
    }

    /// Settle the open contest: pay the tally into the casino coin bank,
    /// apply the flags the settlement names, and hand over the one-shot
    /// Master-course prize when it is due.
    ///
    /// The tally is the contest's whole reward. Returns the settlement, or
    /// `None` when no contest is open or it has not reached its end.
    ///
    /// PORT: FUN_801d0f60 / FUN_80026018 (the coin credit)
    pub fn settle_muscle_contest(&mut self) -> Option<crate::muscle_dome::ContestSettlement> {
        use crate::muscle_dome as md;
        let flags = self.muscle_contest_flags();
        let contest = self.minigames.muscle_contest.as_mut()?;
        if !contest.over() {
            return None;
        }
        let out = contest.settle(&flags);
        self.minigames.muscle_contest = None;
        self.minigames.muscle_settlement = Some(out);
        self.minigames.casino_coins =
            md::credit_casino_coins(self.minigames.casino_coins, out.score);
        if out.set_continue_flag {
            self.system_flag_set(md::CONTEST_CONTINUE_FLAG);
        } else {
            self.system_flag_clear(md::CONTEST_CONTINUE_FLAG);
        }
        if out.set_gave_up_flag {
            self.system_flag_set(md::CONTEST_GAVE_UP_FLAG);
        } else {
            self.system_flag_clear(md::CONTEST_GAVE_UP_FLAG);
        }
        if let Some(id) = out.set_ran_first_flag {
            self.system_flag_set(id);
        }
        if out.award_prize {
            self.system_flag_set(md::CONTEST_PRIZE_FLAG);
            let slot = self
                .party
                .inventory
                .entry(md::CONTEST_PRIZE_ITEM_ID)
                .or_insert(0);
            *slot = slot.saturating_add(1).min(legaia_save::STACK_CAP);
        }
        Some(out)
    }

    /// Advance the Muscle Dome one frame, reading this frame's pad:
    ///
    /// - **Select**: [`Left`](input::PadButton::Left) /
    ///   [`Right`](input::PadButton::Right) / [`Up`](input::PadButton::Up) /
    ///   [`Down`](input::PadButton::Down) commit the four dealt directions
    ///   (the retail direction bits, in the `ctx+0x1114..+0x1120` slot
    ///   order); [`Cross`](input::PadButton::Cross) confirms the queue. The
    ///   opponent commits through the shared selection logic when the player
    ///   confirms.
    /// - **Resolve**: each side's whole queued string plays out through the
    ///   session's installed [`DomeDamageModel`] - the *shared* retail damage
    ///   kernel (move-power record → predamage roll → element affinity →
    ///   finisher, on the contest's PsyQ `rand()` stream), the same one the
    ///   browser host resolves with. A session with no model installed
    ///   resolves to no damage rather than to invented constants.
    /// - **TurnOver / decided**: the next turn is taken automatically (retail
    ///   confirms nothing at a turn boundary), and [`Cross`] closes a
    ///   finished leg: it is reported to the contest, and a survived leg
    ///   with the course not exhausted stays in the arena for the hub's
    ///   INTERVAL / ROUND screens ([`World::muscle_hub_between_legs`]) while
    ///   every other leg settles and hands the field back. A leg finishes on a KO and on nothing else:
    ///   turns are counted, never budgeted. Retail agrees - the arena hands
    ///   the round to an ordinary battle (`FUN_801D1510` sets game mode
    ///   `0x14`), and the only battle-end signal comes from the `0x5A`
    ///   end-of-action KO scans.
    ///
    /// [`DomeDamageModel`]: crate::muscle_dome::DomeDamageModel
    ///
    /// `FUN_801D0748` is the **battle overlay's** round / flow SM (context
    /// pointer `_DAT_8007BD24`, phase byte `ctx+6`), not a dome-specific
    /// controller: its 2781 instructions form none of the dome's own tables
    /// (`0x801F4B8C` and friends appear nowhere in it). It is reached here
    /// because a dome leg *is* an ordinary battle, so the reuse is the retail
    /// chain rather than a shape match.
    ///
    /// PORT: FUN_801d0748 (that round driver's phase loop: pick / commit /
    /// resolve), with the presentation left to the host.
    pub(super) fn tick_muscle_dome(&mut self) {
        use crate::muscle_dome::MusclePhase;
        let Some(phase) = self.minigames.muscle_dome.as_ref().map(|s| s.phase()) else {
            // Between legs the arena hub owns the frame (its INTERVAL, the
            // ringside still and the ROUND card) until
            // [`Self::begin_next_muscle_leg`] stages the next fight.
            if !self.minigames.muscle_hub_between_legs {
                self.leave_muscle_arena();
            }
            return;
        };
        // The battle-open hold runs once the hub's own screens (the first
        // visit, the leg-open ROUND card) have handed the leg over - retail
        // starts the battle only past the hub's arm `0x16`.
        if !self.minigames.muscle_hub.covers_leg() {
            // `ctx[+0x6D6] -= 0x1F800393` per battle pass, one a vsync tick.
            let step = u16::from(super::battle::BATTLE_PASS_STEP_PER_TICK);
            if let Some(s) = self.minigames.muscle_dome.as_mut() {
                s.tick_intro(step);
            }
        }
        let confirm = self.input.just_pressed(input::PadButton::Cross);
        match phase {
            MusclePhase::Select => {
                // The whole selection - the command ring, the Auto |
                // Command prompt, the direction entry and its review, the
                // Ra-Seru list and the Begin | Reselect confirm - is the
                // session's command flow, which runs the battle's own
                // command and entry sessions (`muscle_dome::DomeMenu`).
                let pad = crate::muscle_dome::DomeSelectPad {
                    left: self.input.just_pressed(input::PadButton::Left),
                    right: self.input.just_pressed(input::PadButton::Right),
                    up: self.input.just_pressed(input::PadButton::Up),
                    down: self.input.just_pressed(input::PadButton::Down),
                    confirm,
                    cancel: self.input.just_pressed(input::PadButton::Circle),
                    triangle: self.input.just_pressed(input::PadButton::Triangle),
                    select_attack: self.toggles.select_attack,
                };
                let ev = self
                    .minigames
                    .muscle_dome
                    .as_mut()
                    .map(|s| s.select_input(pad));
                if ev == Some(crate::muscle_dome::DomeMenuEvent::Run) {
                    // Run on the round prompt: the leg is reported as ran,
                    // which settles the contest as a give-up - the shared
                    // escape path.
                    let _ = self.leave_muscle_dome();
                }
            }
            MusclePhase::Resolve => {
                if let Some(s) = self.minigames.muscle_dome.as_mut() {
                    // With no disc tables staged this closes the turn without
                    // damage rather than substituting invented numbers - and
                    // rather than parking the leg in `Resolve` forever. The
                    // damage rolls draw on the world stream (retail's one
                    // `rand()` seed).
                    s.resolve_turn_on_stream(&mut self.rng_state);
                    // The turn's plays now animate (the dome surface plays
                    // out `turn_timeline`: the closing walk, the swings, the
                    // done tails); the leg holds at `TurnOver` for that long,
                    // as retail's action phases do before the round driver
                    // re-enters the command cluster.
                    self.minigames.muscle_playback_frames =
                        crate::muscle_dome_scene::turn_playback_ticks(
                            s.last_turn_plays(),
                            s.last_turn_closes_in(),
                        );
                }
            }
            MusclePhase::TurnOver => {
                // Retail's turn boundary is automatic, not confirmed: the
                // battle-action SM writes `ctx[6] = 0x14` at `0x801E67F0` and
                // the round driver re-enters its own command cluster with no
                // press. The arena hub - the only thing that draws an
                // INTERVAL screen - runs in arena mode `0x18` and is not
                // executing during a leg, so a confirm gate here was a silent
                // one-press stall with nothing on screen to explain it.
                // REF: FUN_801e295c (turn-top arm)
                if self.minigames.muscle_playback_frames > 0 {
                    self.minigames.muscle_playback_frames -= 1;
                } else if let Some(s) = self.minigames.muscle_dome.as_mut() {
                    s.next_turn();
                }
            }
            MusclePhase::Won | MusclePhase::Lost => {
                if confirm {
                    let report = crate::muscle_dome::LegReport {
                        survived: phase == MusclePhase::Won,
                        outcome: 0,
                        turns_taken: self.minigames.muscle_dome.as_ref().map_or(0, |s| s.turn()),
                    };
                    self.end_muscle_leg();
                    self.report_muscle_leg(report);
                    // A contest that has run out settles on the spot: the
                    // payout is the contest's, not the leg's.
                    self.settle_muscle_contest();
                    // A survived leg with the course not exhausted re-enters
                    // the arena hub (state `0x0A`), which plays the INTERVAL
                    // tally and the ROUND card and then starts the next fight
                    // itself (`FUN_801D1510` past arm `0x16`): the player
                    // never leaves the arena between legs. Every other leg
                    // settles and hands the field back.
                    let continues = crate::muscle_dome::leg_boundary_raises_interval(
                        self.minigames.muscle_contest.as_ref().map(|c| c.state()),
                    );
                    if continues {
                        self.minigames.muscle_hub_between_legs = true;
                    } else {
                        self.leave_muscle_arena();
                    }
                }
            }
        }
    }

    /// Ticks left of the resolved turn's playback: while non-zero the leg
    /// sits at `TurnOver` and the turn's plays animate, so the hosts show the
    /// turn's damage rather than the next command cluster. Retail's round
    /// driver reaches its command phase only after the action phases
    /// `0xFE` / `0xFF` have played every queued action; the port times that
    /// span with the dome surface's own replay cadence
    /// ([`crate::muscle_dome_scene::turn_playback_ticks`]).
    pub fn muscle_playback_frames(&self) -> u32 {
        self.minigames.muscle_playback_frames
    }

    /// The play the dome surface is replaying this tick and its attacker's
    /// running damage total: `(attacker slot, total)`. The surface plays the
    /// turn out on [`crate::muscle_dome_scene::turn_timeline`]; the total
    /// sums that attacker's landed damage up to and including the current
    /// play, which is the tally retail's play-out counts up ("TOTAL n").
    /// `None` outside a playback.
    pub fn muscle_playback_tally(&self) -> Option<(usize, i32)> {
        let left = self.minigames.muscle_playback_frames;
        if left == 0 {
            return None;
        }
        let s = self.minigames.muscle_dome.as_ref()?;
        let plays = s.last_turn_plays();
        let closes_in = s.last_turn_closes_in();
        let total = crate::muscle_dome_scene::turn_playback_ticks(plays, closes_in);
        let beat = crate::muscle_dome_scene::beat_at(plays, closes_in, total.saturating_sub(left))?;
        let i = beat.play;
        let attacker = plays[i].attacker.min(1);
        if beat.kind == crate::muscle_dome_scene::BeatKind::Approach {
            // The walk in is the acting side's action; nothing has landed.
            return Some((attacker, 0));
        }
        let sum = plays[..=i]
            .iter()
            .filter(|p| p.attacker.min(1) == attacker)
            .map(|p| p.damage.max(0))
            .sum();
        Some((attacker, sum))
    }

    /// Whether the arena hub is between two legs of an open contest: the
    /// mode is still [`SceneMode::MuscleDome`], no leg is open, and the hub's
    /// INTERVAL / ROUND screens own the frame.
    pub fn muscle_hub_between_legs(&self) -> bool {
        self.minigames.muscle_hub_between_legs
            && self.minigames.muscle_dome.is_none()
            && self.mode == SceneMode::MuscleDome
    }

    /// Advance the arena hub's screen timers one tick
    /// ([`crate::muscle_ringside::HubTimers`], `FUN_801CF870`'s screen arms),
    /// off this tick's leg / contest edges and pad. Runs every tick in every
    /// mode - the INTERVAL screen plays after the leg has closed - and is
    /// called by the shared scene host right after [`Self::tick`], so the
    /// two play hosts and a headless harness all run one hub. The hub's
    /// hand-off past arm `0x16` stages the next fight here
    /// ([`Self::begin_next_muscle_leg`]); the sounds it fires queue for the
    /// host ([`Self::take_muscle_hub_sounds`]).
    pub fn tick_muscle_hub(&mut self) {
        let pad = self.input.retail_pad().pressed as u16;
        // `_DAT_80084580`, the voice/SFX volume each tally cue halves.
        let volume_word = self.audio.levels.voice_volume as u32;
        let mut timers = std::mem::take(&mut self.minigames.muscle_hub);
        let frame = timers.tick(self, pad, volume_word);
        self.minigames.muscle_hub = timers;
        if frame.next_leg {
            self.begin_next_muscle_leg();
        }
        // The INTERVAL arm's tally cues go through the SFX ring like every
        // other cue: the drainer resolves them against the arena bundle.
        for &(slot, id, delay) in &frame.ring_cues {
            self.audio
                .sfx_ring_ops
                .push(crate::world::SfxRingOp::ArmSlot(slot, id, delay));
        }
        let sounds = &mut self.minigames.muscle_hub_sounds;
        if frame.xa.is_some() {
            sounds.xa = frame.xa;
        }
        sounds.voice_cues.extend(frame.voice_cues);
        // A host that never drains (a headless harness) keeps only the
        // latest roll's keys.
        let excess = sounds.voice_cues.len().saturating_sub(32);
        sounds.voice_cues.drain(..excess);
    }

    /// Drain the CD-XA line and the tally voice keys the hub fired since the
    /// last drain, for the host to sound.
    pub fn take_muscle_hub_sounds(&mut self) -> crate::muscle_ringside::HubTimersFrame {
        std::mem::take(&mut self.minigames.muscle_hub_sounds)
    }

    /// Stage the contest's next fight once the between-legs hub has played
    /// out - the hub's hand-off past arm `0x16` (`FUN_801D1510`). The fight
    /// opens through the same mode-24 drain the arena door uses, without
    /// re-arming the round trip (the departure scene stays backed up from
    /// the door). A no-op when the hub is not between legs.
    ///
    /// [`Self::tick_muscle_hub`] calls it on [`crate::muscle_ringside::HubTimersFrame::next_leg`].
    pub fn begin_next_muscle_leg(&mut self) {
        if !self.muscle_hub_between_legs() {
            return;
        }
        // The flag stays up until the leg is installed
        // ([`Self::enter_muscle_dome`]), so the arena keeps the frame on the
        // ticks before the scene host drains the request.
        self.minigames.pending_warp =
            Some(crate::minigame_entry::MinigameSubId::MuscleDome.sub_id());
    }
}

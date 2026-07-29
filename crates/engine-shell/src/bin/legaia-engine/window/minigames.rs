//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;

impl PlayWindowApp {
    // The mode-24 minigame door warp (`World::arm_minigame_warp` /
    // `World::minigame_return_warp`, retail `FUN_80025980` / `FUN_80026018`)
    // is not driven from these entry points - it runs one layer down, inside
    // `World::enter_baka_fighter` / `World::exit_baka_fighter`, where the
    // producer it depends on lives.
    //
    // `FUN_80026018` banks the mode-24 winnings accumulator `_DAT_80084440`
    // into the casino coin bank `_DAT_800845A4` (`0x80026050..0x80026078`,
    // clamped at 9,999,999). What fills that accumulator is the Baka Fighter
    // end-of-match tally: `FUN_801D239C` at `0x801d2894..0x801d28bc` adds each
    // drained step into `0x80084440` - the coin prize, not party gold
    // (`0x8008459C`). The engine's duel tick pays the same drain into
    // `World::minigame_winnings`, so the warp's commit has something to bank.
    // REF: FUN_80026018 (coin-bank commit), FUN_801d239c (the producer)

    /// Drive the fishing HUD's one-shot banner animations for this frame.
    ///
    /// Seeds a timer on each session phase edge (cast lock = strike + hook,
    /// resolve = reel-in or miss, recast = the auxiliary banner), then services
    /// every timer through the retail driver-tail loop
    /// ([`BannerTimer::service`](legaia_engine_render::BannerTimer::service))
    /// and caches this frame's draws for the HUD builder, which is `&self` and
    /// cannot advance them itself.
    ///
    /// The frame step is the engine's fixed one tick per frame (retail reads
    /// `DAT_1f800393`, its frame-rate compensation word).
    pub(super) fn tick_fishing_banners(&mut self) {
        use legaia_engine_core::fishing::{FightOutcome, FishingPhase};
        let Some(session) = self.session.host.world.fishing.as_ref() else {
            // Left the minigame: drop any half-run banner with the session.
            self.fishing_banners = Default::default();
            self.fishing_banner_draws.clear();
            self.fishing_prev_phase = None;
            return;
        };
        let phase = session.phase();
        let outcome = session.last_outcome();
        match (self.fishing_prev_phase, phase) {
            (Some(FishingPhase::Casting), FishingPhase::Fighting) => {
                self.fishing_banners.on_hook();
            }
            (Some(FishingPhase::Fighting), FishingPhase::Done) => match outcome {
                Some(FightOutcome::Landed { .. }) => self.fishing_banners.on_landed(),
                Some(FightOutcome::Snapped) => self.fishing_banners.on_snapped(),
                _ => {}
            },
            (Some(FishingPhase::Done), FishingPhase::Casting) => {
                self.fishing_banners.on_recast();
            }
            _ => {}
        }
        self.fishing_prev_phase = Some(phase);
        self.fishing_banner_draws = self.fishing_banners.service_frame(1);
    }

    /// Settle the open Muscle Dome contest if it has reached its end, paying
    /// the tally into the casino coin bank and awarding the one-shot
    /// Master-course prize when it is due.
    ///
    /// Called wherever a leg can close: the pad path in `tick_muscle_dome`
    /// and the window's own `M` abort. A contest that is still mid-ladder
    /// settles nothing.
    pub(super) fn settle_muscle_contest_if_over(&mut self) {
        let Some(out) = self.session.host.world.settle_muscle_contest() else {
            return;
        };
        log::info!(
            "muscle: contest settled - {} coins paid, bank now {}{}",
            out.score,
            self.session.host.world.casino_coins,
            if out.award_prize {
                " (War God Icon awarded)"
            } else {
                ""
            },
        );
    }

    /// Advance the Muscle Dome contest's round **time meter** one frame.
    ///
    /// Retail runs the meter from the arena's per-frame driver with the frame
    /// delta from scratchpad `0x1F800393`; the engine ticks a fixed one per
    /// frame. No-op outside a contest.
    pub(super) fn tick_muscle_time_meter(&mut self) {
        if let Some(s) = self.session.host.world.muscle_dome.as_mut() {
            s.tick_time_meter(1);
        }
    }

    /// Drain the Baka Fighter duel's queued SFX cues and enqueue them into the
    /// BGM director's SFX scheduler, so the punch/exchange hit (`BAKA_CUE_HIT`
    /// = `0x09`, written by the rules kernel's damage step) actually sounds in
    /// the live engine. Mirrors the battle strike-SFX path
    /// (`drain_and_log_battle_events` → `enqueue_sfx`); the director's
    /// per-frame `tick_sfx_frame` (driven from `drain_and_log_battle_events`)
    /// fires the enqueued cues against the resident class-2 SFX bank the same
    /// frame. The cues carry no gameplay state, so nothing here affects
    /// determinism. No-op outside the duel / when no audio is attached; the
    /// fight's cue buffer is drained regardless so it never accumulates.
    pub(super) fn drain_baka_sfx_cues(&mut self) {
        if self.session.host.world.mode != SceneMode::BakaFighter {
            return;
        }
        let cues: Vec<u8> = self
            .session
            .host
            .world
            .baka_fighter
            .as_mut()
            .map(|f| f.take_cues())
            .unwrap_or_default();
        if cues.is_empty() {
            return;
        }
        if let Some(bgm) = self.session.bgm.as_mut() {
            // Fire on the same frame (strike-relative delay 0); the duel has no
            // actor/target slots, so pass 0/0 for the HUD-context fields.
            for id in &cues {
                bgm.enqueue_sfx(*id as u16, 0, 0, 0);
            }
        } else {
            for id in &cues {
                log::debug!("baka SFX cue {id:#04x} (no audio)");
            }
        }
    }

    /// The monster stat archive (PROT 867) bytes, decoded + cached on first
    /// use. `None` if no disc is attached or the entry can't be read.
    pub(super) fn monster_archive_bytes(&mut self) -> Option<std::sync::Arc<Vec<u8>>> {
        if self.monster_archive.is_none() {
            const MONSTER_ARCHIVE_PROT_ENTRY: u32 = 867;
            match self
                .session
                .host
                .index
                .entry_bytes_extended(MONSTER_ARCHIVE_PROT_ENTRY)
            {
                Ok(b) => self.monster_archive = Some(std::sync::Arc::new(b)),
                Err(e) => {
                    log::warn!("play-window: monster archive (PROT 867) load skipped: {e:#}");
                    return None;
                }
            }
        }
        self.monster_archive.clone()
    }

    /// Load the Noa dance overlay (PROT 0980), decode its baked step chart, and
    /// arm a dance run on the qualifier floor. Returns `false` (and logs) when
    /// no disc is attached or the chart can't decode.
    pub(super) fn start_dance_minigame(&mut self, long_song: bool) -> bool {
        self.start_dance_minigame_mode(legaia_engine_core::dance::DanceMode::Qualifier, long_song)
    }

    /// Load the dance overlay and arm a run on `mode`'s floor, holding the
    /// parsed game pending behind the pre-song **count-in** phase (retail
    /// `FUN_801cf470` runs its below-10 states - the `1 2 3 READY... GO!`
    /// banner - before the beat clock starts; `tick_dance_countin` plays that
    /// envelope out and only then enters the dance + starts the song).
    ///
    /// Mirrors the disc-gated `dance_minigame_real` test's overlay path: read
    /// the raw PROT entry, lift it to its statically-recovered loaded form via
    /// [`static_overlay::as_loaded`], then parse through
    /// [`DanceGame::from_overlay_for_mode`].
    pub(super) fn start_dance_minigame_mode(
        &mut self,
        mode: legaia_engine_core::dance::DanceMode,
        long_song: bool,
    ) -> bool {
        use legaia_asset::static_overlay;
        let Some(rec) = static_overlay::overlay_map()
            .by_prot_index(legaia_asset::dance_chart::DANCE_OVERLAY_PROT_INDEX as u32)
        else {
            log::warn!("dance: overlay 0980 absent from the static-overlay map");
            return false;
        };
        let raw = match self.session.host.index.entry_bytes_extended(rec.prot_index) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("dance: PROT {} read failed: {e:#}", rec.prot_index);
                return false;
            }
        };
        let loaded = match static_overlay::as_loaded(&raw, rec) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("dance: as_loaded failed: {e:#}");
                return false;
            }
        };
        match legaia_engine_core::dance::DanceGame::from_overlay_for_mode(&loaded, mode, long_song)
        {
            Some(game) => {
                self.dance_countin = Some((game, 0, false));
                self.dance_fx_score = 0;
                true
            }
            None => {
                log::warn!("dance: step-chart parse failed");
                false
            }
        }
    }

    /// Advance the pre-song count-in banner one frame
    /// ([`legaia_engine_core::dance::dance_countin_banner_envelope`]): fire
    /// the once-only intro cue on the hold-segment entry, cache the envelope
    /// for the HUD, and enter the pending dance (+ start the song) when the
    /// slide-out finishes.
    pub(super) fn tick_dance_countin(&mut self) {
        use legaia_engine_core::dance;
        // The banner's whole timeline: slide-in (0x1e) + hold (to 0x5a) +
        // slide-out (0x1e more).
        const COUNTIN_END: i32 = 0x5a + 0x1e;
        let Some((_, t, cue_fired)) = self.dance_countin.as_mut() else {
            self.dance_countin_draw = None;
            return;
        };
        let env = dance::dance_countin_banner_envelope(*t);
        if env.hold && !*cue_fired {
            *cue_fired = true;
            if let Some(bgm) = self.session.bgm.as_mut() {
                bgm.enqueue_sfx(dance::COUNTIN_INTRO_CUE, 0, 0, 0);
            }
        }
        self.dance_countin_draw = Some(env);
        *t += 1;
        if *t >= COUNTIN_END {
            let (game, _, _) = self.dance_countin.take().expect("count-in armed");
            self.dance_countin_draw = None;
            let long = game.song_len() == dance::SONG_LEN_LONG;
            self.session.host.world.enter_dance(game);
            // The dance overlay loads one of two mode-selected chart loops
            // (global BGM 2058/2064 = extraction 1048/1054). The exact
            // mode->song arm is unpinned; approximate it by song length.
            self.session
                .start_global_bgm(if long { 2064 } else { 2058 });
        }
    }

    /// The dance side-channel frame: spawn the sequence-clear banner + stars
    /// into the effect pool on the human's scoring judge
    /// ([`legaia_engine_core::dance::good_banner_spawn`]), and run the Disco
    /// King tutorial actor beside a how-to run.
    pub(super) fn tick_dance_side(&mut self) {
        use legaia_engine_core::dance::{self, Judge};
        let in_dance = self.session.host.world.mode == SceneMode::Dance;
        if !in_dance && self.dance_countin.is_none() {
            self.dance_tutorial = None;
            self.dance_tutorial_frame = None;
            self.dance_fx_score = 0;
            return;
        }
        let (score, feedback_frames, combo_hit) = self
            .session
            .host
            .world
            .dance
            .as_ref()
            .map(|g| {
                (
                    g.score(),
                    g.feedback_frames() as i32,
                    matches!(g.triangle_feedback(), Some(true)),
                )
            })
            .unwrap_or((0, 0, false));
        // Sequence-clear banner on the score edge of a scoring judge.
        if in_dance && score > self.dance_fx_score {
            if let Some(Judge::Sequence { weight }) = self.session.host.world.dance_last_judge {
                self.minigame_fx
                    .spawn_good_banner(&dance::good_banner_spawn(weight.min(0xFFFF) as u16));
            }
            self.dance_fx_score = score;
        }
        // The tutorial actor runs one handler call per frame, on the retail
        // pad-word layout (the same rotate `World::tick_dance` applies).
        if let Some(tut) = self.dance_tutorial.as_mut() {
            let pressed = (self.pad & !self.prev_pad).rotate_right(8);
            let frame = tut.step(pressed, score as i32, feedback_frames, combo_hit, 1);
            if let Some(cue) = frame.cue
                && let Some(bgm) = self.session.bgm.as_mut()
            {
                bgm.enqueue_sfx(cue, 0, 0, 0);
            }
            if frame.done {
                self.dance_tutorial = None;
                self.dance_tutorial_frame = None;
            } else {
                self.dance_tutorial_frame = Some(frame);
            }
        }
    }

    /// The venue scene's `.MAP` extended footprint - the engine's
    /// `_DAT_1F8003EC` floor buffer (tile records at `+0`, height/wall grid
    /// at `+0x4000`, cell grid at `+0x8000`). `None` when the current scene
    /// carries no field map.
    fn venue_floor_bytes(&self) -> Option<Vec<u8>> {
        let scene = self.session.host.scene.as_ref()?;
        let idx = scene.field_map_index(&self.session.host.index)?;
        self.session.host.index.entry_bytes_extended(idx).ok()
    }

    /// The fishing venue's actor-side frame: the free-swimming fish wander
    /// (idle/cast), the venue floor solve for its height, the retail camera
    /// publish, the reeling-line actor across hook -> fight -> celebration,
    /// and the sub-screen idle sway.
    pub(super) fn tick_fishing_actors(&mut self) {
        use legaia_engine_core::fishing::{FightOutcome, FishingPhase};
        use legaia_engine_core::fishing_actors as fa;
        use legaia_engine_core::fishing_chrome as fc;
        if self.session.host.world.mode != SceneMode::Fishing {
            self.fish_wander = None;
            self.fish_line = None;
            self.fishing_floor = None;
            self.fishing_sway_offset = (0, 0);
            return;
        }
        let Some(phase) = self.session.host.world.fishing.as_ref().map(|s| s.phase()) else {
            return;
        };
        // One-time venue arm: the wander actor, the floor buffer, and the
        // venue camera reset (through the engine camera's retail global
        // trios; axis 4 = `TR.y` deliberately untouched, as retail leaves
        // `_DAT_800840BC` alone).
        if self.fish_wander.is_none() {
            self.fish_wander = Some(fa::FishWander::new(0x400, 0, 0x400));
            self.fishing_floor = self.venue_floor_bytes();
            let reset = fc::venue_camera_reset();
            let g = &mut self.session.camera.globals.0;
            g[0] = reset.rot[0] as i32;
            g[1] = reset.rot[1] as i32;
            g[2] = reset.rot[2] as i32;
            g[3] = reset.tr_x;
            g[5] = reset.tr_z;
        }
        // The wander runs while the cast is idle (retail's MODE_IDLE_CAST
        // fishing-SM state); the D-pad steers the fish.
        if phase == FishingPhase::Casting {
            let held = self.pad.rotate_right(8);
            let mut rng = self.minigame_rng;
            let rolled = self.fish_wander.as_mut().and_then(|w| {
                w.tick(held, || {
                    let mut x = rng;
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    rng = x;
                    x
                })
            });
            self.minigame_rng = rng;
            if rolled.is_some()
                && let Some(w) = self.fish_wander.as_ref()
                && let Some(r) = fc::ripple_spawn(w.x, w.z, 0)
            {
                self.minigame_fx.spawn_ripple(&r);
            }
        }
        // Settle the actor onto the venue floor (the `.MAP` height grid
        // through the shared ground solver) and publish its camera.
        if let (Some(w), Some(buf)) = (self.fish_wander.as_mut(), self.fishing_floor.as_ref()) {
            let ramp = legaia_engine_core::minigame_floor::height_ramp();
            let grid = legaia_engine_core::minigame_floor::FloorGrid::new(buf);
            let t = fc::float_actor_tick(grid, w.x, w.z, 0, &ramp);
            w.y = t.y;
        }
        if let Some(w) = self.fish_wander.as_ref() {
            let cam = w.camera();
            let g = &mut self.session.camera.globals.0;
            g[1] = cam.yaw as i32;
            g[4] = cam.pitch_term;
            g[6] = cam.translation.0;
            g[7] = cam.translation.1;
            g[8] = cam.translation.2;
        }
        // The line actor: armed on the hook edge, landed on the catch edge.
        // `fishing_prev_phase` still holds last frame's phase here (the
        // banner tick that refreshes it runs after this method).
        let outcome = self
            .session
            .host
            .world
            .fishing
            .as_ref()
            .and_then(|s| s.last_outcome());
        match (self.fishing_prev_phase, phase) {
            (Some(FishingPhase::Casting), FishingPhase::Fighting) => {
                self.minigame_fx.spawn_splash(&fc::splash_burst(
                    fa::SCREEN_CENTRE.0,
                    fa::SCREEN_CENTRE.1,
                    0,
                    0x40,
                ));
                self.fish_line = Some(fa::LineActorSim::hooked());
            }
            (Some(FishingPhase::Fighting), FishingPhase::Done) => {
                if let (Some(line), Some(FightOutcome::Landed { points })) =
                    (self.fish_line.as_mut(), outcome)
                {
                    line.land(points);
                } else {
                    self.fish_line = None;
                }
            }
            _ => {}
        }
        if let Some(mut line) = self.fish_line.take() {
            let f = line.tick(1);
            let origin = self
                .fish_wander
                .as_ref()
                .map(|w| (w.x, w.z))
                .unwrap_or((0, 0));
            let mut cues: Vec<u8> = Vec::new();
            if let Some(cue) = f.cue {
                cues.push(cue);
            }
            for b in &f.bursts {
                self.minigame_fx.spawn_burst(b, origin);
                if let Some(cue) = b.cue {
                    cues.push(cue);
                }
            }
            if let Some(bgm) = self.session.bgm.as_mut() {
                for cue in cues {
                    bgm.enqueue_sfx(cue as u16, 0, 0, 0);
                }
            }
            if !f.done {
                self.fish_line = Some(line);
            }
        }
        // Sub-screen idle sway while the point-exchange list is up.
        if self.session.host.world.fishing_exchange.is_some() {
            let (v, next) = fc::sway_vector(sway_sine_table(), self.fishing_sway_angle, 1);
            self.fishing_sway_angle = next;
            self.fishing_sway_offset = (v.x, v.y);
        } else {
            self.fishing_sway_offset = (0, 0);
        }
    }

    /// Consume the Baka Fighter round-chrome frame the duel produced this
    /// tick (`BakaFight` owns the [`BakaChrome`] runner and steps it inside
    /// `tick_with_input`; the round banners start at its own round ends) and
    /// resolve each glyph draw's `u` stamp against the overlay's parsed
    /// widget table ([`legaia_engine_core::baka_fighter_chrome::glyph_u`]).
    ///
    /// [`BakaChrome`]: legaia_engine_core::baka_fighter_chrome::BakaChrome
    pub(super) fn tick_baka_chrome(&mut self) {
        use legaia_engine_core::baka_fighter_chrome as bc;
        if self.session.host.world.mode != SceneMode::BakaFighter {
            self.baka_chrome_frame.clear();
            return;
        }
        let Some(f) = self.session.host.world.baka_fighter.as_ref() else {
            return;
        };
        let frame = f.chrome_frame();
        if let Some(xa) = frame.xa {
            log::debug!(
                "baka chrome: announcer XA clip {} chan {} ({} frames)",
                xa.clip,
                xa.chan,
                xa.dur
            );
        }
        // Resolve the draws: a glyph-carrying draw pages the glyph strip by
        // stamping `u = glyph_u(index)` into widget 5's record - performed
        // here against the parsed table, exactly where retail's emitter does
        // the byte store.
        let widgets = self.baka_hud_widgets.as_deref();
        self.baka_chrome_frame = frame
            .draws
            .iter()
            .map(|d| {
                // The stamped cell rect: widget 5's record with its `u`
                // paged to the glyph index. The page's texels are not
                // uploaded; the rect is the future atlas source.
                let stamped = d.glyph.and_then(|idx| {
                    let u = bc::glyph_u(idx);
                    widgets
                        .and_then(|t| t.get(bc::GLYPH_WIDGET as usize))
                        .map(|w| (u, w.v, w.w, w.h))
                });
                (*d, stamped)
            })
            .collect();
    }

    /// Per-frame driver for every minigame side-channel this window hosts:
    /// the dance count-in + tutorial + effect spawns, the fishing venue
    /// actors, the Baka round chrome, and the shared effect pool's ageing.
    pub(super) fn tick_minigame_extras(&mut self) {
        self.tick_dance_countin();
        self.tick_dance_side();
        self.tick_fishing_actors();
        self.tick_baka_chrome();
        self.minigame_fx.tick();
    }

    /// Load the fishing overlay (PROT 0972), decode its per-species table, and
    /// start a fishing session in the world (suspending the current scene).
    /// Returns `false` (and logs) when no disc is attached or the table can't
    /// decode. Mirrors [`Self::start_dance_minigame`]'s overlay path.
    ///
    /// The rod stat + persistent record start at defaults (the save-block
    /// fishing record isn't loaded into this dev entry point).
    pub(super) fn start_fishing_minigame(&mut self) -> bool {
        use legaia_asset::static_overlay;
        let Some(rec) = static_overlay::overlay_map()
            .by_prot_index(legaia_asset::fishing_species::FISHING_OVERLAY_PROT_INDEX as u32)
        else {
            log::warn!("fishing: overlay 0972 absent from the static-overlay map");
            return false;
        };
        let raw = match self.session.host.index.entry_bytes_extended(rec.prot_index) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("fishing: PROT {} read failed: {e:#}", rec.prot_index);
                return false;
            }
        };
        let loaded = match static_overlay::as_loaded(&raw, rec) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("fishing: as_loaded failed: {e:#}");
                return false;
            }
        };
        let Some(species) = legaia_asset::fishing_species::parse(&loaded) else {
            log::warn!("fishing: species-table parse failed");
            return false;
        };
        // Decode the two point-exchange venue pages alongside the species
        // table, naming rows from the SCUS item table when it's readable
        // (P toggles the prize list while fishing).
        self.fishing_prize_venues = legaia_asset::fishing_exchange::parse(&loaded).map(|ex| {
            use legaia_engine_core::Vfs;
            let scus = if let Some(root) = self.extracted_root.as_deref() {
                legaia_engine_core::DirVfs::new(root)
                    .ok()
                    .and_then(|v| v.read("SCUS_942.54").ok())
            } else if let Some(disc) = self.disc_path.as_deref() {
                legaia_engine_core::DiscVfs::open(disc)
                    .ok()
                    .and_then(|v| v.read("SCUS_942.54").ok())
            } else {
                None
            };
            let names = scus
                .as_deref()
                .and_then(legaia_asset::item_names::ItemNameTable::from_scus);
            [0usize, 1].map(|venue| {
                legaia_engine_core::fishing::PrizeExchange::from_asset(
                    venue,
                    &ex.venues[venue],
                    names.as_ref(),
                )
            })
        });
        // Default rod stat for the dev entry point; the record resumes the
        // world's persistent point pool (banked back on exit).
        const DEV_ROD_STAT: i32 = 4;
        let record = legaia_engine_core::fishing::FishingRecord {
            points: self.session.host.world.fishing_points,
            ..Default::default()
        };
        let session =
            legaia_engine_core::fishing::FishingSession::new(species, DEV_ROD_STAT, record);
        self.session.host.world.enter_fishing(session);
        true
    }

    /// Load the slot-machine overlay (PROT 0975), decode its payout table, and
    /// start a slot session in the world (suspending the current scene).
    /// Returns `false` (and logs) when no disc is attached or the table can't
    /// decode. Mirrors [`Self::start_dance_minigame`]'s overlay path.
    ///
    /// The playing balance seeds from the world's casino coin bank
    /// (`World::casino_coins`, the retail `_DAT_800845A4`); a thin bank first
    /// goes through the casino's **coin-exchange counter**
    /// ([`Self::buy_casino_coins`]) and only falls back to a fronted dev stake
    /// when the party cannot pay. The final balance commits back to the bank on
    /// exit (`World::exit_slot_machine`).
    pub(super) fn start_slot_minigame(&mut self) -> bool {
        use legaia_asset::static_overlay;
        let Some(rec) = static_overlay::overlay_map()
            .by_prot_index(legaia_asset::slot_payout::SLOT_OVERLAY_PROT_INDEX as u32)
        else {
            log::warn!("slots: overlay 0975 absent from the static-overlay map");
            return false;
        };
        let raw = match self.session.host.index.entry_bytes_extended(rec.prot_index) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("slots: PROT {} read failed: {e:#}", rec.prot_index);
                return false;
            }
        };
        let loaded = match static_overlay::as_loaded(&raw, rec) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("slots: as_loaded failed: {e:#}");
                return false;
            }
        };
        let Some(payouts) = legaia_asset::slot_payout::parse(&loaded) else {
            log::warn!("slots: payout-table parse failed");
            return false;
        };
        // The retail entry path arrives through the casino with coins already
        // banked; when the bank can't cover a spin, buy them at the exchange
        // counter first, and only front a dev stake if the party can't pay.
        const DEV_STAKE: i32 = 100;
        let bank = self.session.host.world.casino_coins as i32;
        let balance = if bank >= legaia_engine_core::slot_machine::MIN_SPIN_BALANCE {
            bank
        } else if let Some(bought) = self.buy_casino_coins(DEV_STAKE) {
            bought
        } else {
            log::info!("slots: coin bank {bank} too thin - fronting a {DEV_STAKE}-coin dev stake");
            DEV_STAKE
        };
        // Seed from the frame counter: deterministic across a replayed pad
        // stream (retail reseeds from BIOS rand at machine init).
        let seed = 0x5107_5EED ^ self.session.host.world.frame as u32;
        let machine = legaia_engine_core::slot_machine::SlotMachine::new(payouts, seed, balance);
        self.session.host.world.enter_slot_machine(machine);
        true
    }

    /// Buy `coins` at the casino's coin-exchange counter, debiting party gold
    /// and crediting the coin bank. Returns the new bank balance, or `None`
    /// when the counter refuses the sale (party can't pay, or the counter is
    /// out of coins) - in which case nothing is debited.
    ///
    /// The counter arithmetic is the ported one: the requested count is laid
    /// out least-significant-digit-first the way the screen's entry field
    /// stores it, and [`coin_exchange_quote`] resolves the total cost and both
    /// gates (`gold >= cost`, `stock >= coins`) exactly as `FUN_801E6F70`
    /// does before it recolours the total.
    ///
    /// [`coin_exchange_quote`]: legaia_engine_core::slot_machine::coin_exchange_quote
    ///
    /// The counter's remaining stock is retail's `_DAT_8007BB90`, a global the
    /// port has no producer for; this host stands in the full bank cap, so the
    /// stock gate only ever bites on an absurd request.
    fn buy_casino_coins(&mut self, coins: i32) -> Option<i32> {
        use legaia_engine_core::slot_machine::{
            BALANCE_CAP, COIN_ENTRY_DIGITS, coin_exchange_quote,
        };
        // The entry field is COIN_ENTRY_DIGITS single-digit cells, units first.
        let mut digits = [0u8; COIN_ENTRY_DIGITS];
        let mut n = coins.max(0);
        for d in digits.iter_mut() {
            *d = (n % 10) as u8;
            n /= 10;
        }
        let gold = self.session.host.world.money;
        let quote = coin_exchange_quote(&digits, gold, BALANCE_CAP);
        if !quote.is_valid() {
            log::info!(
                "slots: coin counter refused {} coins ({} gold, have {gold}; in stock: {})",
                quote.coins,
                quote.cost,
                quote.in_stock
            );
            return None;
        }
        self.session.host.world.money = gold - quote.cost;
        let bank = self.session.host.world.casino_coins as i32 + quote.coins;
        self.session.host.world.casino_coins = bank.max(0) as u32;
        log::info!(
            "slots: bought {} coins for {} gold at the exchange counter (bank {bank})",
            quote.coins,
            quote.cost
        );
        Some(bank)
    }

    /// Load the Baka Fighter overlay (PROT 0976), parse the roster + action
    /// tables, and enter a best-of-3 duel: the player fights as roster
    /// fighter 0 against a ladder opponent picked from the roster (rotating
    /// with the frame counter so repeat entries vary). Returns `false` (with
    /// a log line) when the overlay or tables don't resolve.
    pub(super) fn start_baka_minigame(&mut self) -> bool {
        use legaia_asset::static_overlay;
        let Some(rec) = static_overlay::overlay_map()
            .by_prot_index(legaia_asset::baka_opponents::BAKA_OVERLAY_PROT_INDEX as u32)
        else {
            log::warn!("baka: overlay 0976 absent from the static-overlay map");
            return false;
        };
        let raw = match self.session.host.index.entry_bytes_extended(rec.prot_index) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("baka: PROT {} read failed: {e:#}", rec.prot_index);
                return false;
            }
        };
        let loaded = match static_overlay::as_loaded(&raw, rec) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("baka: as_loaded failed: {e:#}");
                return false;
            }
        };
        let Some(opponents) = legaia_asset::baka_opponents::parse(&loaded) else {
            log::warn!("baka: roster-table parse failed");
            return false;
        };
        // The HUD widget table, for the round chrome's glyph-strip paging.
        self.baka_hud_widgets = legaia_asset::baka_opponents::parse_baka_hud(&loaded);
        let Some(actions) = legaia_asset::baka_opponents::parse_actions(&loaded) else {
            log::warn!("baka: action-table parse failed");
            return false;
        };
        // Rotate the ladder opponent with the frame counter (1..=16; roster 0
        // is the player-side default). Seed like the slot machine: frame-
        // derived, deterministic across a replayed pad stream.
        let frame = self.session.host.world.frame as u32;
        let opponent = 1 + (frame as usize % (opponents.len().saturating_sub(1).max(1)));
        let seed = 0xBA4A_F19A ^ frame;
        let Some(fight) = legaia_engine_core::baka_fighter::BakaFight::from_tables(
            &opponents, &actions, 0, opponent, seed,
        ) else {
            log::warn!("baka: fight construction failed (roster 0 vs {opponent})");
            return false;
        };
        log::info!(
            "baka: round 1 vs roster fighter {opponent} (gold prize {})",
            fight.gold_reward()
        );
        self.session.host.world.enter_baka_fighter(fight);
        // The duel overlay init (FUN_801CF00C) loads its own track: global
        // BGM 2053 = music_01 slot 53, the boss overture.
        self.session.start_global_bgm(2053);
        true
    }

    /// Load the Muscle Dome direction tables from the battle overlay (PROT
    /// 0898) and enter a contest (fought to a KO - a dome round is an
    /// ordinary battle and is not turn-limited). The player's per-direction AP
    /// costs come from their own player battle file's equipped-section swing
    /// records (`+0x74`, the same bytes the Arts gauge reads), and the
    /// player's HP / budget pool come from the lead party record's live
    /// fields (`+0x104` max HP, `+0x110` AGL - the `+0x14e` / `+0x154` battle
    /// actor fields retail copies them into). The opponent has no actor
    /// record here: it fights the same direction deck at the flat favored
    /// cost, from documented stand-in HP / budget constants. Returns `false`
    /// (with a log line) when the tables don't resolve.
    pub(super) fn start_muscle_minigame(&mut self) -> bool {
        use legaia_asset::muscle_dome as md;
        use legaia_asset::static_overlay;
        use legaia_engine_core::muscle_dome::{MuscleCard, MuscleDomeSession};
        let Some(rec) =
            static_overlay::overlay_map().by_prot_index(md::MUSCLE_OVERLAY_PROT_INDEX as u32)
        else {
            log::warn!("muscle: battle overlay 0898 absent from the static-overlay map");
            return false;
        };
        let raw = match self.session.host.index.entry_bytes_extended(rec.prot_index) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("muscle: PROT {} read failed: {e:#}", rec.prot_index);
                return false;
            }
        };
        let loaded = match static_overlay::as_loaded(&raw, rec) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("muscle: as_loaded failed: {e:#}");
                return false;
            }
        };
        let Some(commands) = md::hand_command_ids(&loaded) else {
            log::warn!("muscle: hand command-id table failed its structural check");
            return false;
        };
        // Player card costs: the lead character's equipped-section swing
        // records, keyed by runtime slot = the command id.
        const FAVORED_COST: u16 = 0x1E;
        let mut player_costs = [FAVORED_COST; 4];
        if let Some(costs) = self.lead_swing_costs() {
            for (i, &cmd) in commands.iter().enumerate() {
                if let Some(&c) = costs.get((cmd - 0x0C) as usize)
                    && c > 0
                {
                    player_costs[i] = c as u16;
                }
            }
        } else {
            log::info!("muscle: lead swing costs unavailable - flat favored costs");
        }
        let card = |cmd: u8, cost: u16| MuscleCard {
            command_id: cmd,
            cost,
        };
        let player_hand = std::array::from_fn(|i| card(commands[i], player_costs[i]));
        let opp_hand = std::array::from_fn(|i| card(commands[i], FAVORED_COST));
        // The opponent is the *real* one: PROT 0977's course ladder names a
        // monster id per (course, round) and `FUN_801D1510` stores it into
        // formation slot 0, so the arena's foe is an ordinary battle monster
        // with an ordinary PROT 867 record.
        //
        // Which `(course, round)` is staged is the **contest's** to say, not
        // this launcher's: the course comes from the arena's own story-flag
        // unlock seeds and the round walks the ladder as legs are cleared.
        // Opening the contest here is the arena entry retail runs when the
        // sub-id word is still zero.
        const STANDIN_BUDGET: u16 = 120;
        const STANDIN_HP: i32 = 400;
        let arena_raw = self
            .session
            .host
            .index
            .entry_bytes_extended(legaia_engine_core::muscle_dome::ARENA_OVERLAY_PROT_INDEX as u32)
            .ok();
        let ladder = arena_raw
            .as_deref()
            .and_then(legaia_engine_core::muscle_dome::parse_course_ladder);
        if self.session.host.world.muscle_contest.is_none() {
            let flags = self.session.host.world.muscle_contest_flags();
            self.session.host.world.muscle_contest = arena_raw.as_deref().and_then(|raw| {
                legaia_engine_core::muscle_dome::DomeContest::from_overlay(raw, &flags)
            });
        }
        let (course, round) = self
            .session
            .host
            .world
            .muscle_contest
            .as_ref()
            .map_or((0usize, 0u32), |c| (c.course(), c.round()));
        let opponent_round = ladder.as_ref().and_then(|l| {
            let rounds = &l.get(course)?.rounds;
            let n = (round as usize).min(rounds.len().saturating_sub(1));
            Some((n, *rounds.get(n)?))
        });
        let opponent_record = opponent_round.and_then(|(_, r)| {
            let archive = self.monster_archive_bytes()?;
            legaia_asset::monster_archive::record(&archive, r.monster_id as u16).ok()?
        });
        let lead = self.session.host.world.roster.members.first();
        let player_hp = lead
            .map(|r| r.hp_mp_sp().hp_max as i32)
            .filter(|&hp| hp > 0)
            .unwrap_or(500);
        let player_budget = lead
            .map(|r| r.live_stats().agl)
            .filter(|&agl| agl > 0)
            .unwrap_or(STANDIN_BUDGET);
        // Resolve through the *retail* damage kernel, the same one the
        // browser host uses: the move-power table, its id -> index map and
        // the element-affinity matrix all come off this raw PROT 0898 entry.
        // The player's stat profile is the lead party record's live window
        // (`+0x110..+0x11B`); the opponent's is its own monster record's
        // battle-entry profile, the same `battle_stats()` the battle loader
        // stages. The constants below survive only as the fallback for a
        // disc whose ladder or archive does not decode.
        const STANDIN_OPPONENT: legaia_engine_core::muscle_dome::DomeCombatant =
            legaia_engine_core::muscle_dome::DomeCombatant {
                hp_max: STANDIN_HP as u16,
                int: 40,
                udf: 30,
                ldf: 30,
                element: 0,
            };
        let opponent = opponent_record
            .as_ref()
            .map(|r| {
                let bs = r.battle_stats();
                legaia_engine_core::muscle_dome::DomeCombatant {
                    hp_max: r.hp,
                    int: bs[4],
                    udf: bs[2],
                    ldf: bs[3],
                    element: r.element,
                }
            })
            .unwrap_or(STANDIN_OPPONENT);
        let opponent_hp = opponent_record
            .as_ref()
            .map(|r| r.hp as i32)
            .filter(|&hp| hp > 0)
            .unwrap_or(STANDIN_HP);
        let opponent_budget = opponent_record
            .as_ref()
            .map(|r| r.battle_stats()[0])
            .filter(|&agl| agl > 0)
            .unwrap_or(STANDIN_BUDGET);
        let player_profile = lead
            .map(|r| {
                let live = r.live_stats();
                legaia_engine_core::muscle_dome::DomeCombatant {
                    hp_max: player_hp.clamp(0, u16::MAX as i32) as u16,
                    int: live.int,
                    udf: live.udf,
                    ldf: live.ldf,
                    element: 0,
                }
            })
            .unwrap_or(STANDIN_OPPONENT);
        // The victory caption's Seru index. It names a *string*, not a prize:
        // a contest pays casino coins, and nothing in the arena grants a
        // Seru. See `legaia_engine_core::muscle_dome::reward_spell_id`.
        const CAPTION_SERU_INDEX: u8 = 1;
        let mut session = MuscleDomeSession::new(
            player_hand,
            opp_hand,
            [player_budget, opponent_budget],
            [player_hp, opponent_hp],
            CAPTION_SERU_INDEX,
        );
        let seed = 0x4D55_5343 ^ self.session.host.world.frame as u32;
        match legaia_engine_core::muscle_dome::DomeDamageModel::from_battle_overlay(
            &raw,
            [player_profile, opponent],
            [player_hp, opponent_hp],
            seed,
        ) {
            Some(model) => session.install_damage_model(model),
            None => log::warn!(
                "muscle: PROT 0898 move-power table did not decode - the contest will \
                 resolve without damage"
            ),
        }
        match opponent_round {
            Some((n, r)) => log::info!(
                "muscle: course {course} round {} vs monster {:#04x} ({} HP), tally {}, \
                 deck {commands:02x?}, player costs {player_costs:?}, player {player_hp} HP \
                 on a {player_budget} AP pool",
                n + 1,
                r.monster_id,
                opponent_hp,
                self.session
                    .host
                    .world
                    .muscle_contest
                    .as_ref()
                    .map_or(0, |c| c.tally()),
            ),
            None => log::warn!(
                "muscle: PROT 0977 course ladder did not decode - fighting the \
                 disclosed stand-in opponent instead"
            ),
        }
        self.session.host.world.enter_muscle_dome(session);
        // The arena loads no track of its own - it reuses the battle engine,
        // so it plays a battle theme. Use the standard random-battle theme
        // (global BGM 2026 = music_01 slot 26, M26B1); see
        // docs/subsystems/minigame-muscle-dome.md.
        self.session.start_global_bgm(2026);
        true
    }
}

/// The 4096-step sine table the sub-screen sway samples. Retail reads the
/// shared table through `*_DAT_8007B81C` (runtime data the port does not
/// stage); the host synthesizes an equivalent once.
fn sway_sine_table() -> &'static [i16] {
    use std::sync::OnceLock;
    static TABLE: OnceLock<Vec<i16>> = OnceLock::new();
    TABLE.get_or_init(|| {
        (0..legaia_engine_core::fishing_chrome::SINE_TURN)
            .map(|i| {
                let f = (i as f64) * std::f64::consts::TAU
                    / legaia_engine_core::fishing_chrome::SINE_TURN as f64;
                (f.sin() * 4096.0).round() as i16
            })
            .collect()
    })
}

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
    // `World::minigames.winnings`, so the warp's commit has something to bank.
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
        let Some(session) = self.session.host.world.minigames.fishing.as_ref() else {
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
            self.session.host.world.minigames.casino_coins,
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
        if let Some(s) = self.session.host.world.minigames.muscle_dome.as_mut() {
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
            .minigames
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

    /// Load the dance overlay and arm a run on `mode`'s floor. The world
    /// stages the pre-song **count-in** (retail `FUN_801cf470` runs its
    /// below-10 states - the `1 2 3 READY... GO!` banner - before the beat
    /// clock starts) and starts the song when it clears.
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
                // `World::enter_dance` arms the count-in, the how-to tutorial
                // actor and the pending song id; the world's dance tick holds
                // the beat clock off until the banner clears. This window used
                // to hold the parsed game pending behind a count-in driver of
                // its own, which is why the door-warp entry - the one a player
                // reaches - had no count-in on either host.
                self.session.host.world.enter_dance(game);
                true
            }
            None => {
                log::warn!("dance: step-chart parse failed");
                false
            }
        }
    }

    /// Fire the minigame sessions' queued SFX cues (the dance count-in's
    /// intro cue, the how-to tutorial's cursor / confirm cues) into the BGM
    /// director's scheduler.
    ///
    /// The count-in itself is no longer driven here: `World::tick_dance`
    /// plays the banner envelope out and starts the song, so the browser play
    /// page gets the same phase from the same kernel. This host only sounds
    /// what that phase queued.
    pub(super) fn drain_minigame_sfx_cues(&mut self) {
        let cues = self.session.host.world.drain_minigame_sfx_cues();
        if cues.is_empty() {
            return;
        }
        let Some(bgm) = self.session.bgm.as_mut() else {
            return;
        };
        for cue in cues {
            bgm.enqueue_sfx(cue, 0, 0, 0);
        }
    }

    /// Put the dance hall's own HUD texture page into the VRAM this window is
    /// drawing with for as long as a dance runs, and take it back on the way
    /// out.
    ///
    /// Retail has the page because the dance **is** the hall scene: its whole
    /// texture set is resident before the minigame starts. The port hosts the
    /// session over whichever scene the player walked in from, so the 4bpp
    /// page the widget table names holds that scene's texels and every HUD
    /// quad sampled nothing. The fix is residency, not a draw call - which is
    /// why the count-in banner had been text on both hosts with its geometry
    /// fully pinned.
    ///
    /// Only the rects the run's own widget table names are staged
    /// ([`DanceGame::hud_vram_rects`](legaia_engine_core::dance::DanceGame::hud_vram_rects)),
    /// because the suspended scene keeps rendering behind the HUD and the rest
    /// of that pack targets the columns a field texture pack occupies. The
    /// pristine copy is kept so the field VRAM is exact on exit rather than
    /// re-derived - the same shape the battle path uses for its own throwaway
    /// injection.
    fn stage_dance_hud_art(&mut self) {
        let in_dance = self.session.host.world.mode == SceneMode::Dance;
        if !in_dance {
            // Leaving edge: the field VRAM goes back byte for byte.
            if let Some(clean) = self.dance_vram_restore.take() {
                self.cpu_vram_base = Some(clean);
                self.upload_cpu_vram();
            }
            self.session.host.world.minigames.dance_hud_art_staged = false;
            return;
        }
        if self.dance_vram_restore.is_some() {
            return;
        }
        let Some(rects) = self
            .session
            .host
            .world
            .minigames
            .dance
            .as_ref()
            .map(|g| g.hud_vram_rects())
        else {
            return;
        };
        let Some(base) = self.cpu_vram_base.as_ref() else {
            return;
        };
        let clean = base.clone();
        let mut staged = base.clone();
        let n = legaia_engine_core::dance::stage_dance_hud_vram(
            &self.session.host.index,
            &rects,
            &mut staged,
        );
        if n == 0 {
            // No page, no residency claim: the hosts fall back to the
            // placeholder letterforms together.
            log::warn!("play-window: dance HUD page not staged (PROT entry absent or unpacked 0)");
            return;
        }
        log::info!(
            "play-window: dance HUD page staged ({n} TIM(s), {} rect(s))",
            rects.len()
        );
        self.cpu_vram_base = Some(staged);
        self.upload_cpu_vram();
        self.dance_vram_restore = Some(clean);
        self.session.host.world.minigames.dance_hud_art_staged = true;
    }

    /// Re-upload `cpu_vram_base` to the GPU. Silent when the renderer is not
    /// up yet (a headless tick), which is the same guard every other VRAM
    /// mutation in this window carries.
    fn upload_cpu_vram(&mut self) {
        if let (Some(r), Some(base)) = (self.win.renderer.as_ref(), self.cpu_vram_base.as_ref()) {
            match r.upload_vram(base) {
                Ok(v) => self.uploaded_vram = Some(v),
                Err(e) => log::error!("play-window: dance VRAM upload: {e:#}"),
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

    /// The venue map's `+0x10000` region block - the table the lure's water
    /// class walks (`FUN_800180EC`'s input).
    fn venue_region_block(&self) -> Option<Vec<u8>> {
        let scene = self.session.host.scene.as_ref()?;
        scene
            .field_map_region_block(&self.session.host.index)
            .ok()
            .flatten()
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
            self.fishing_regions = None;
            self.fish_lure = None;
            self.fishing_sway_offset = (0, 0);
            return;
        }
        let Some(phase) = self
            .session
            .host
            .world
            .minigames
            .fishing
            .as_ref()
            .map(|s| s.phase())
        else {
            return;
        };
        // One-time venue arm: the wander actor, the floor buffer, and the
        // venue camera reset (through the engine camera's retail global
        // trios; axis 4 = `TR.y` deliberately untouched, as retail leaves
        // `_DAT_800840BC` alone).
        if self.fish_wander.is_none() {
            self.fish_wander = Some(fa::FishWander::new(0x400, 0, 0x400));
            self.fishing_floor = self.venue_floor_bytes();
            self.fishing_regions = self.venue_region_block();
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
                self.session.host.world.minigames.fx.spawn_ripple(&r);
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
            .minigames
            .fishing
            .as_ref()
            .and_then(|s| s.last_outcome());
        match (self.fishing_prev_phase, phase) {
            (Some(FishingPhase::Casting), FishingPhase::Fighting) => {
                // The strike splash is spawned by `World::tick_fishing` off
                // the session's own phase edge, so every host gets it.
                self.fish_line = Some(fa::LineActorSim::hooked());
                // The cast lands: the lure spawns a fixed radius ahead of the
                // venue anchor along the angler's facing, the same
                // subtraction retail runs in the fishing SM's cast arm.
                let facing = self.fish_wander.as_ref().map(|w| w.facing).unwrap_or(0);
                let (ax, az) = fa::VENUE_ANCHOR;
                self.session.host.world.minigames.fishing_casts += 1;
                self.fish_lure =
                    fa::LureActor::cast(ax, az, facing, 1).map(|l| (l, Default::default()));
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
        // The lure's own frame while the line is out: the walk-grid drift and
        // the water class of the tile it sits over.
        if let (Some((lure, probe)), Some(buf)) =
            (self.fish_lure.as_mut(), self.fishing_floor.as_ref())
        {
            let region = self
                .fishing_regions
                .as_deref()
                .and_then(legaia_engine_core::field_regions::RegionTable::parse);
            let casts = self.session.host.world.minigames.fishing_casts;
            *probe = lure.probe(buf, region.as_ref(), casts, 1);
        }
        if let Some(mut line) = self.fish_line.take() {
            let f = line.tick(1);
            // Retail's celebration bursts ride the line actor, which sits on
            // the lure - not on the free-swimming fish the venue also draws.
            let origin = self
                .fish_lure
                .as_ref()
                .map(|(l, _)| (l.x(), l.z))
                .or_else(|| self.fish_wander.as_ref().map(|w| (w.x, w.z)))
                .unwrap_or((0, 0));
            // The bursts' *visuals* are this actor's: they hang off the lure,
            // which only this host simulates. Their **cues** are not - the
            // hook cue and the celebration tiers are queued by
            // `World::tick_fishing` off the session's own phase edges, where
            // all three hosts drain them (`drain_minigame_sfx_cues`). Firing
            // them here as well would play each one twice on this host alone.
            for b in &f.bursts {
                self.session.host.world.minigames.fx.spawn_burst(b, origin);
            }
            if !f.done {
                self.fish_line = Some(line);
            }
        }
        // Sub-screen idle sway while the point-exchange list is up.
        if self.session.host.world.minigames.fishing_exchange.is_some() {
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
        let Some(f) = self.session.host.world.minigames.baka_fighter.as_ref() else {
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
    /// the minigame cue queue, the fishing venue actors, the Baka round
    /// chrome and the Muscle Dome hub-screen timers. The effect pool itself
    /// is aged by `World::tick`, where every host reaches it.
    pub(super) fn tick_minigame_extras(&mut self) {
        self.drain_minigame_sfx_cues();
        self.stage_dance_hud_art();
        self.tick_fishing_actors();
        self.tick_baka_chrome();
        self.tick_muscle_hub();
    }

    /// Advance the Muscle Dome hub-screen timers one frame, off the world's
    /// own leg / contest edges (so both the pad path and the `M` abort arm
    /// them):
    ///
    /// * a leg opening on a **fresh** contest arms the "Welcome to the
    ///   Muscle Dome!" intro card, then the ROUND banner;
    /// * a leg opening mid-ladder arms the ROUND banner alone;
    /// * a leg **closing** arms the between-legs INTERVAL + score-tally
    ///   screen, but only when the shared cadence rule
    ///   ([`legaia_engine_core::muscle_dome::leg_boundary_raises_interval`])
    ///   says retail's hub reaches state `0x0A` - a survived leg with the
    ///   course not yet exhausted. A lost, run-from or final leg settles
    ///   instead and raises nothing.
    ///
    /// A **turn** boundary arms nothing here, and cannot: the leg stays open
    /// across turns, so the closing edge never fires. Retail agrees - the turn
    /// boundary is `ctx[6] = 0x14` inside the battle SM, with the arena hub not
    /// running ([`MusclePhase::ends_turn`]).
    ///
    /// Each screen runs retail's own fade / hold envelope
    /// ([`legaia_engine_core::muscle_dome::HubScreen`]) rather than a frame
    /// count this host picked: fade in at the arm's rate, hold at the
    /// measured literal, fade out - and the two card holds end early on a
    /// pad press, the way `FUN_801CF870`'s `& 0xF4` test lets them.
    ///
    /// [`MusclePhase::ends_turn`]: legaia_engine_core::muscle_dome::MusclePhase::ends_turn
    pub(super) fn tick_muscle_hub(&mut self) {
        use legaia_engine_core::muscle_dome::HubScreen;
        // A dome leg the player WALKED into (the mode-24 door warp, drained
        // by the shared scene host) carries no contest: the warp arm opens
        // the leg and deliberately stages no `(course, round)`, because a
        // door warp does not carry one. The debug launcher below opened the
        // contest and loaded the hub page itself, so until now the window
        // showed a contest line and a hub screen only for a fight started
        // from a hotkey. Do what the browser play page does on every entry.
        if self.session.host.world.minigames.muscle_dome.is_some() {
            self.open_muscle_contest();
            self.load_muscle_hub_assets();
        }
        // The pad edges the skippable holds read (retail's `DAT_801D1A9C`
        // snapshot of `_DAT_8007B874 | _DAT_8007B938`).
        let pad = self.session.host.world.input.retail_pad().pressed as u16;
        // `_DAT_80084580`, the voice/SFX volume setting each tally cue halves.
        // The engine holds no live mirror of that word, so this is its cold
        // reset - the value a freshly booted game keys the cue at.
        let volume_word = legaia_engine_core::new_game::GAME_STATE_COLD_RESET.voice_volume as u32;
        let world = &self.session.host.world;
        let leg_open = world.minigames.muscle_dome.is_some();
        let contest_open = world.minigames.muscle_contest.is_some();
        if leg_open && !self.muscle_prev_leg_open {
            let round = world
                .minigames
                .muscle_contest
                .as_ref()
                .map_or(1, |c| c.round() as i32 + 1);
            // The card runs once per leg: a re-entered hub already played it
            // over the still, so the leg that opens after it does not.
            let raise = legaia_engine_core::muscle_ringside::leg_open_raises_round_card(
                self.muscle_card_round.take(),
                round,
            );
            if contest_open && !self.muscle_prev_contest_open {
                // A fresh contest opens on the hub's first visit, whose own
                // arms end in the ROUND card (`FirstVisitHub`).
                self.muscle_first_visit =
                    Some(legaia_engine_core::muscle_ringside::FirstVisitHub::new());
            } else if raise {
                // Retail's ROUND card is arms 0x15 / 0x16 - the opponent-card
                // envelope.
                self.muscle_round_banner = Some((round, HubScreen::opponent_card()));
            }
            self.muscle_interval = None;
            self.muscle_backdrop = None;
        }
        if !leg_open && self.muscle_prev_leg_open && self.muscle_prev_contest_open {
            // The leg boundary the arena hub sees. Whether it shows the tally
            // screen is the shared rule's call, not this host's.
            let raises = legaia_engine_core::muscle_dome::leg_boundary_raises_interval(
                world.minigames.muscle_contest.as_ref().map(|c| c.state()),
            );
            // The tally roll is data-dependent; its four lanes step one row
            // per tick after the shared lead-in, and the last cue lands on
            // the staggered vsync countdown, so the roll cannot be shorter
            // than that stagger.
            let roll = legaia_engine_core::muscle_dome::HUB_TALLY_ROLL_LEAD_TICKS
                + *legaia_engine_core::muscle_dome::HUB_TALLY_CUE_STAGGER
                    .last()
                    .unwrap_or(&0) as i32;
            self.muscle_interval = raises.then(|| HubScreen::interval(roll));
            // A hub re-entered after a leg draws the still that leg's end
            // left resident as its backdrop (retail's `_DAT_801D1AE0` arm).
            self.muscle_backdrop = world
                .minigames
                .muscle_ringside_still
                .filter(|_| raises)
                .map(legaia_engine_core::muscle_ringside::HubBackdrop::reentry);
            // Arm the tally roll with the screen: the contest is already
            // settled, so the roll only decides what the six rows read while
            // the screen is up, and it ends on the settled values.
            self.muscle_tally = raises
                .then(|| {
                    world
                        .minigames
                        .muscle_contest
                        .as_ref()
                        .map(|c| c.tally_roll())
                })
                .flatten();
            self.muscle_first_visit = None;
            self.muscle_round_banner = None;
        }
        // Retail runs one arm at a time: the first visit's arms walk intro,
        // title, course card and ROUND card in turn.
        if let Some(hub) = self.muscle_first_visit.as_mut() {
            hub.tick(1, pad);
            if hub.done() {
                self.muscle_first_visit = None;
            }
        } else if let Some((_, banner)) = self.muscle_round_banner.as_mut() {
            banner.tick(1, pad);
            if banner.done() {
                self.muscle_round_banner = None;
            }
        }
        // The backdrop rides the INTERVAL screen's arms, then runs its own
        // return + ROUND-card arms once the screen has gone.
        if let Some(backdrop) = self.muscle_backdrop.as_mut() {
            let lane0_full = self.muscle_tally.as_ref().is_some_and(|(ramp, _)| {
                ramp.fade[0] >= legaia_engine_core::other_game_overlay::LANE_FADE_FULL
            });
            backdrop.tick(1, pad, self.muscle_interval.map(|i| i.stage()), lane0_full);
            if backdrop.card_brightness().is_some() {
                self.muscle_card_round = Some(
                    self.session
                        .host
                        .world
                        .minigames
                        .muscle_contest
                        .as_ref()
                        .map_or(1, |c| c.round() as i32 + 1),
                );
            }
            if backdrop.done() {
                self.muscle_backdrop = None;
            }
        }
        if let Some(interval) = self.muscle_interval.as_mut() {
            interval.tick(1, pad);
            // The tally rolls on the same clock. `boost` is retail's bypass
            // flag `DAT_801D1AB4`, which this host never raises, and the
            // volume word is the voice-volume setting the cue halves.
            if let Some((ramp, tally)) = self.muscle_tally.as_mut() {
                let step = ramp.tick(1, false, volume_word);
                *tally += step.tally_gain;
                // Each drained lane keys a voice directly, with no cue id in
                // sight (`FUN_801D1288` builds the whole attr set). The
                // director's explicit key-on is the only path that takes it.
                let cues = step.cues.clone();
                if let Some(bgm) = self.session.bgm.as_mut() {
                    for cue in cues {
                        bgm.key_on_voice_attr(legaia_engine_audio::VoiceAttr::from_cue_words(
                            cue.voice,
                            cue.vab_program_tone,
                            cue.note_and_fine,
                            cue.volume,
                        ));
                    }
                }
            }
            if interval.done() {
                self.muscle_interval = None;
                self.muscle_tally = None;
            }
        }
        self.muscle_prev_leg_open = leg_open;
        self.muscle_prev_contest_open = contest_open;
    }

    /// Load the Muscle Dome hub-screen assets once: the two hub page TIMs
    /// (the LZS payload of the dome's own data file, extraction 1220 /
    /// `other6.lzs` slot 0 - the pages retail uploads at VRAM
    /// (320,0)/(320,256)) baked to RGBA per referenced 16-colour sub-palette
    /// and stacked into one sprite atlas, plus the PROT 0977 sprite
    /// descriptor table the shared emitters place every hub screen from.
    /// No-op when already loaded; logs and leaves `muscle_hub` empty when the
    /// disc or renderer is absent.
    /// Stage the dome contest (`(course, round)` off the arena overlay and
    /// the party's story flags) unless one is already open.
    ///
    /// The mode-24 door warp opens a leg without one on purpose - the warp
    /// operand names an overlay, not a ladder position - so whichever host
    /// runs the dome has to do this. The browser play page has always done it
    /// on entry; this window used to do it only inside its `M` launcher.
    pub(super) fn open_muscle_contest(&mut self) {
        if self.session.host.world.minigames.muscle_contest.is_some() {
            return;
        }
        let Ok(raw) =
            self.session.host.index.entry_bytes_extended(
                legaia_engine_core::muscle_dome::ARENA_OVERLAY_PROT_INDEX as u32,
            )
        else {
            return;
        };
        let flags = self.session.host.world.muscle_contest_flags();
        self.session.host.world.minigames.muscle_contest =
            legaia_engine_core::muscle_dome::DomeContest::from_overlay(&raw, &flags);
    }

    pub(super) fn load_muscle_hub_assets(&mut self) {
        use legaia_engine_render::other_game_hud as hud;
        if self.muscle_hub.is_some() {
            return;
        }
        /// PROT entry (extraction space) of the dome data container:
        /// LZS section 0 carries the two hub-page TIMs back to back.
        const HUB_CONTAINER_PROT_INDEX: u32 = 1220;
        let Some(renderer) = self.win.renderer.as_ref() else {
            return;
        };
        let container = match self
            .session
            .host
            .index
            .entry_bytes_extended(HUB_CONTAINER_PROT_INDEX)
        {
            Ok(b) => b,
            Err(e) => {
                log::warn!("muscle hub: PROT {HUB_CONTAINER_PROT_INDEX} read failed: {e:#}");
                return;
            }
        };
        let Some((tim0, tim1)) =
            legaia_lzs::decompress_container(&container)
                .ok()
                .and_then(|sections| {
                    // Section 0 = `[12-byte header][TIM][TIM]`.
                    let blob = sections.into_iter().next()?;
                    let t0 = legaia_tim::parse(blob.get(0xC..)?).ok()?;
                    let t1 = legaia_tim::parse(blob.get(0xC + t0.byte_extent()..)?).ok()?;
                    Some((t0, t1))
                })
        else {
            log::warn!("muscle hub: page TIMs did not decode from the dome container");
            return;
        };
        let arena_raw =
            match self.session.host.index.entry_bytes_extended(
                legaia_engine_core::muscle_dome::ARENA_OVERLAY_PROT_INDEX as u32,
            ) {
                Ok(b) => b,
                Err(e) => {
                    log::warn!("muscle hub: PROT 0977 read failed: {e:#}");
                    return;
                }
            };
        let table = hud::parse_sprite_table(&arena_raw);
        if table.is_empty() {
            log::warn!("muscle hub: PROT 0977 sprite table did not parse");
            return;
        }
        // Every (page, sub-palette) pair the hub draws can reach: each
        // record's own CLUT, its variant-2 sibling (`clut + 1` - the emitter
        // bump), and the digit record's four tally palettes.
        let mut wanted = std::collections::BTreeSet::new();
        for (i, rec) in table.iter().enumerate() {
            let sheet = u8::from(rec.tpage & 0x10 != 0);
            let pal = (rec.clut & 0x3F) as u8;
            wanted.insert((sheet, pal));
            wanted.insert((sheet, pal + 1));
            if i == hud::DIGIT_SPRITE_INDEX {
                for p in 0..4u8 {
                    wanted.insert((sheet, pal + p));
                }
            }
        }
        let tims = [&tim0, &tim1];
        let atlas_w = tims.iter().map(|t| t.pixel_width()).max().unwrap_or(0) as u32;
        if atlas_w == 0 {
            return;
        }
        // The two ringside stills share the atlas: each is a 320-wide sheet
        // (the VRAM region `(384, 0)` the battle end's loader fills).
        let atlas_w = atlas_w.max(legaia_asset::ringside_still::WIDTH as u32);
        let mut blocks: Vec<(u8, u8, u32)> = Vec::new();
        let mut rgba: Vec<u8> = Vec::new();
        let mut atlas_h = 0u32;
        for (sheet, pal) in wanted {
            let tim = tims[sheet as usize];
            // A palette past the sheet's CLUT bank simply isn't baked; the
            // draw that would sample it is skipped at build time.
            let Ok(px) = legaia_tim::decode_rgba8(tim, pal as usize) else {
                continue;
            };
            let (tw, th) = (tim.pixel_width() as u32, tim.pixel_height() as u32);
            for row in 0..th as usize {
                let src = &px[row * tw as usize * 4..(row + 1) * tw as usize * 4];
                rgba.extend_from_slice(src);
                rgba.resize(rgba.len() + ((atlas_w - tw) * 4) as usize, 0);
            }
            blocks.push((sheet, pal, atlas_h));
            atlas_h += th;
        }
        if blocks.is_empty() {
            log::warn!("muscle hub: no page/palette block decoded");
            return;
        }
        // A small white block: the texel the untextured backdrop shade draws
        // with.
        let white_y = atlas_h;
        for _ in 0..4 {
            rgba.extend(std::iter::repeat_n(0xFF, 4 * 4));
            rgba.resize(rgba.len() + ((atlas_w - 4) * 4) as usize, 0);
        }
        atlas_h += 4;
        let mut stills: Vec<(u32, u32)> = Vec::new();
        for variant in 0..2u32 {
            let index = legaia_asset::ringside_still::PROT_INDEX_DEFAULT + variant;
            let Some(sheet) = self
                .session
                .host
                .index
                .entry_bytes_extended(index)
                .ok()
                .and_then(|b| legaia_engine_render::ringside_backdrop::still_sheet_rgba(&b))
            else {
                log::warn!("muscle hub: ringside still {index} did not decode");
                continue;
            };
            let sw = legaia_asset::ringside_still::WIDTH as u32;
            for row in sheet.chunks_exact(sw as usize * 4) {
                rgba.extend_from_slice(row);
                rgba.resize(rgba.len() + ((atlas_w - sw) * 4) as usize, 0);
            }
            stills.push((variant, atlas_h));
            atlas_h += legaia_asset::ringside_still::HEIGHT as u32;
        }
        match renderer.upload_sprite_atlas(&rgba, atlas_w, atlas_h) {
            Ok(atlas) => {
                log::info!(
                    "muscle hub: atlas uploaded ({atlas_w}x{atlas_h}, {} page/palette blocks)",
                    blocks.len()
                );
                self.muscle_hub = Some(MuscleHubAssets {
                    blocks,
                    table,
                    atlas,
                    stills,
                    white_y,
                });
            }
            Err(e) => log::warn!("muscle hub: atlas upload skipped: {e:#}"),
        }
    }

    /// The Muscle Dome hub screens as retail-placed sprite draws, through the
    /// shared `engine-ui` emitters both hosts draw with
    /// ([`legaia_engine_render::other_game_hud::hub_screen_quads`] /
    /// [`legaia_engine_render::other_game_hud::score_tally_quads`] - the browser
    /// dome page reaches the same functions via
    /// `minigames_muscle::muscle_hub_quads_json`): the hub's first visit
    /// (`ringside_backdrop::first_visit_hub_draw` - wall, shade, intro strip,
    /// title zoom, course card, ROUND card) over a fresh contest's first leg,
    /// the INTERVAL heading + six-row score tally between legs. Every quad's extent and screen seat come out of the
    /// PROT 0977 descriptor table and recovered draw lists; the host places
    /// nothing itself.
    ///
    /// The tally's six values are the contest's own rows - the four
    /// `LegScoreRows` lanes, then the running tally and the coin bank they
    /// settle into - the same model row set the browser page feeds the same
    /// builder.
    ///
    /// Two disclosed stand-ins: the packet's vertical two-stop colour
    /// gradient flattens to the stops' mean (the sprite pipeline is
    /// one-colour), and semi-transparent packets draw with ordinary alpha
    /// blending.
    pub(super) fn muscle_hub_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_render::SpriteDraw> {
        use legaia_engine_render::other_game_hud as hud;
        let Some(assets) = self.muscle_hub.as_ref() else {
            return Vec::new();
        };
        let world = &self.session.host.world;
        let in_dome = world.mode == SceneMode::MuscleDome;
        // The retail emitters mutate the shared table (variant write-back),
        // so run them over a per-frame copy of the pristine parse.
        let mut table = assets.table.clone();
        // The brightness argument is the screen's own fade counter, which
        // clamps at `HUB_FADE_FULL` (0x80) - the emitter's neutral, since it
        // scales each stored channel by `c * brightness / 256`. The host used
        // to pass 0x100, which drew every hub screen at twice retail's
        // brightness.
        let mut quads: Vec<hud::HudQuad> = Vec::new();
        // The first visit's shade and the screens drawn over it: the shade
        // sits between the wall tiles (`quads`) and these.
        let mut shade: Option<legaia_engine_render::ringside_backdrop::BackdropShade> = None;
        let mut front: Vec<hud::HudQuad> = Vec::new();
        if in_dome {
            // A first visit's frame: the brick wall + shade (the backdrop
            // emitter's latch-0 arm) behind the arm's screens, all composed
            // by the shared kernel the play page draws with.
            if let Some(hub) = self.muscle_first_visit {
                let f = hub.frame();
                let levels = legaia_engine_render::ringside_backdrop::FirstVisitLevels {
                    backdrop: f.backdrop,
                    intro: f.intro,
                    title_scale: f.title_scale,
                    course_card: f.course_card,
                    round_card: f.round_card,
                };
                let (course, round) = world
                    .minigames
                    .muscle_contest
                    .as_ref()
                    .map_or((0, 1), |c| (c.course() as i32, c.round() as i32 + 1));
                let d = legaia_engine_render::ringside_backdrop::first_visit_hub_draw(
                    &mut table, &levels, course, round,
                );
                quads.extend(d.tiles);
                shade = d.shade;
                front.extend(d.hud);
            } else if let Some((round, banner)) = self.muscle_round_banner {
                quads.extend(hud::hub_screen_quads(
                    &mut table,
                    &hud::round_banner_draws(round),
                    banner.brightness(),
                ));
            }
        } else if let Some(interval) = self.muscle_interval {
            let bright = interval.brightness();
            quads.extend(hud::hub_screen_quads(
                &mut table,
                hud::HUB_INTERVAL_HEADING,
                bright,
            ));
            // The six rows are the roll's own, not the settled totals: three
            // recovery lanes counting down, the HP they count into, the score
            // lane counting down and the coin tally counting up. With no roll
            // armed the screen draws the settled values, which is where the
            // roll ends anyway.
            let (values, row_bright) = match self.muscle_tally.as_ref() {
                Some((ramp, tally)) => (ramp.row_values(*tally), ramp.row_brightness(bright)),
                None => {
                    let (rows, tally) = world
                        .minigames
                        .muscle_contest
                        .as_ref()
                        .map_or((Default::default(), 0), |c| (c.rows(), c.tally()));
                    ([0, 0, 0, rows.hp_restore(), 0, tally], [bright; 6])
                }
            };
            quads.extend(hud::score_tally_quads(&mut table, values, row_bright));
        }
        // The re-entered hub's ROUND card (arms 0x15 / 0x16) over the still.
        if !in_dome
            && self.muscle_interval.is_none()
            && let Some(card) = self.muscle_backdrop.and_then(|b| b.card_brightness())
        {
            let round = world
                .minigames
                .muscle_contest
                .as_ref()
                .map_or(1, |c| c.round() as i32 + 1);
            quads.extend(hud::hub_screen_quads(
                &mut table,
                &hud::round_banner_draws(round),
                card,
            ));
        }
        let mut out: Vec<legaia_engine_render::SpriteDraw> = Vec::new();
        // The backdrop goes first: retail links the still's two packets at
        // the ordering table's far end (`OT + 0xFA0`), behind every sprite.
        if !in_dome
            && let Some(b) = self.muscle_backdrop.filter(|b| b.visible())
            && let Some(&(_, still_y)) = assets.stills.iter().find(|(v, _)| *v == b.variant())
        {
            use legaia_engine_render::ringside_backdrop as rb;
            for q in rb::ringside_still_quads(b.level()) {
                let Some(d) = rb::StillDraw::from_quad(&q) else {
                    continue;
                };
                // A flat packet colour: texture modulation `texel * c / 128`.
                let c = f32::from(d.level) / 128.0;
                out.push(legaia_engine_render::SpriteDraw {
                    dst: d.dst,
                    src: (d.src.0, still_y + d.src.1, d.src.2, d.src.3),
                    color: [c, c, c, 1.0],
                });
            }
        }
        if quads.is_empty() && out.is_empty() && front.is_empty() {
            return Vec::new();
        }
        let shade_at = quads.len();
        quads.extend(front);
        for (i, q) in quads.iter().enumerate() {
            if i == shade_at
                && let Some(sh) = shade
            {
                out.extend(shade_band_draws(&sh, assets.white_y));
            }
            let sheet = u8::from(q.tpage & 0x10 != 0);
            let pal = (q.clut & 0x3F) as u8;
            let Some(&(_, _, block_y)) = assets
                .blocks
                .iter()
                .find(|(s, p, _)| *s == sheet && *p == pal)
            else {
                continue;
            };
            let dw = (q.xy[1].0 as i32 - q.xy[0].0 as i32 + 1).max(0) as u32;
            let dh = (q.xy[2].1 as i32 - q.xy[0].1 as i32 + 1).max(0) as u32;
            let sw = (q.uv[1].0 as i32 - q.uv[0].0 as i32 + 1).max(0) as u32;
            let sh = (q.uv[2].1 as i32 - q.uv[0].1 as i32 + 1).max(0) as u32;
            if dw == 0 || dh == 0 || sw == 0 || sh == 0 {
                continue;
            }
            // PSX texture modulation is `texel * c / 128`; the per-vertex
            // colours are a vertical two-stop gradient, flattened here to
            // the stops' mean.
            let tint = |k: usize| (q.rgb[0][k] as f32 + q.rgb[2][k] as f32) / 2.0 / 128.0;
            // The retail display is the 320x240 frame and the GPU clips to
            // it; the first visit's wall tiles run past it (three 128-wide
            // columns, two 128-high rows), so clip here - the texel window
            // shrinks with the same ratio - or the stage transform carries
            // the overhang onto the window beside the frame.
            let (dx, dy) = (q.xy[0].0 as i64, q.xy[0].1 as i64);
            let (x0, x1) = (dx.max(0), (dx + dw as i64).min(320));
            let (y0, y1) = (dy.max(0), (dy + dh as i64).min(240));
            if x1 <= x0 || y1 <= y0 {
                continue;
            }
            let sx = q.uv[0].0 as i64 + (x0 - dx) * sw as i64 / dw as i64;
            let sy = q.uv[0].1 as i64 + (y0 - dy) * sh as i64 / dh as i64;
            let csw = ((x1 - x0) * sw as i64 / dw as i64).max(1) as u32;
            let csh = ((y1 - y0) * sh as i64 / dh as i64).max(1) as u32;
            out.push(legaia_engine_render::SpriteDraw {
                dst: (x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32),
                src: (sx as u32, block_y + sy as u32, csw, csh),
                color: [tint(0), tint(1), tint(2), 1.0],
            });
        }
        if shade_at >= quads.len()
            && let Some(sh) = shade
        {
            out.extend(shade_band_draws(&sh, assets.white_y));
        }
        // The quads sit in the retail 320x240 frame; map them through the
        // same stage transform every minigame chrome layer uses.
        let (stage_origin, stage_scale) = self.save_select_stage(surface_w, surface_h);
        legaia_engine_render::scale_stage_text_draws(&mut out, stage_origin, stage_scale);
        out
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
            points: self.session.host.world.minigames.fishing_points,
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
    /// (`World::minigames.casino_coins`, the retail `_DAT_800845A4`); a thin bank first
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
        let bank = self.session.host.world.minigames.casino_coins as i32;
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
        let gold = self.session.host.world.party.money;
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
        self.session.host.world.party.money = gold - quote.cost;
        let bank = self.session.host.world.minigames.casino_coins as i32 + quote.coins;
        self.session.host.world.minigames.casino_coins = bank.max(0) as u32;
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
        // The duel overlay init (FUN_801CF00C) loads its own track, through
        // the same constant the door-warp entry uses - the piecewise bank map
        // makes a hand-written id easy to get two slots wrong.
        self.session
            .start_global_bgm(legaia_engine_core::minigame_entry::BAKA_FIGHTER_BGM_ID);
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
        if self.session.host.world.minigames.muscle_contest.is_none() {
            let flags = self.session.host.world.muscle_contest_flags();
            self.session.host.world.minigames.muscle_contest =
                arena_raw.as_deref().and_then(|raw| {
                    legaia_engine_core::muscle_dome::DomeContest::from_overlay(raw, &flags)
                });
        }
        let (course, round) = self
            .session
            .host
            .world
            .minigames
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
        let lead = self.session.host.world.party.roster.members.first();
        // The fighter enters at the lead record's live HP (`+0x106`), as the
        // arena door does - the battle end writes the fight's HP back there
        // and the ringside pick reads it.
        let player_hp_max = lead
            .map(|r| r.hp_mp_sp().hp_max as i32)
            .filter(|&hp| hp > 0)
            .unwrap_or(500);
        let player_hp = lead
            .map(|r| r.hp_mp_sp().hp_cur as i32)
            .filter(|&hp| hp > 0)
            .unwrap_or(player_hp_max);
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
                    hp_max: player_hp_max.clamp(0, u16::MAX as i32) as u16,
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
        // The lead's normal-art catalog, so a recognised art in the queue
        // resolves as an art rather than as the swings it consumed. The
        // shared filter is the same one the arena-door warp uses, and the
        // lead is the roster slot whose swing costs were read above.
        {
            let catalog = legaia_engine_core::muscle_dome::art_catalog_for(
                &self.session.host.world.tables.art_records,
                legaia_art::Character::Vahn,
            );
            if !catalog.is_empty() {
                session.install_art_catalog(0, catalog);
            }
        }
        // The special-battle word the arena's own entry seeds from the three
        // course-unlock story flags (`FUN_801CEA6C`, `0x801CEB88..0x801CEBC8`).
        // The earlier "no dome round raises either restriction bit" note was
        // wrong about the arena's **entry**: the ladder never raises one, but
        // the entry seed does - every unlocked course forbids the Item chip
        // and Master forbids the Ra-Seru chip too.
        let special = self.session.host.world.dome_special_word();
        session.set_special_word(special);
        // The Ra-Seru (magic) command class, through the same shared door the
        // arena-door warp installs it with.
        if let Some(magic) =
            legaia_engine_core::muscle_dome::magic_loadout_for(&self.session.host.world, 0, special)
        {
            session.install_magic(0, magic);
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
                    .minigames
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
        // The hub-screen art (intro card / ROUND banner / INTERVAL + tally),
        // drawn through the shared `other_game_hud` emitters.
        self.load_muscle_hub_assets();
        // The arena loads no track of its own - it reuses the battle engine,
        // so it plays a battle theme. Same constant the door-warp entry uses
        // (`MinigameSubId::bgm_id`); see
        // docs/subsystems/minigame-muscle-dome.md.
        self.session
            .start_global_bgm(legaia_engine_core::music_labels::BATTLE_THEME_1_BGM_ID);
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

/// Rows the backdrop shade's vertical Gouraud ramp is cut into for the
/// sprite pipeline, which carries one colour per draw.
const SHADE_BANDS: u32 = 16;

/// The first visit's backdrop shade (`FUN_801D1610`, a subtractive `B - F`
/// Gouraud quad, `0x64` at the top fading to `0` at the bottom) as sprite
/// draws over the atlas's white block.
///
/// Disclosed stand-in: the sprite pipeline blends with ordinary alpha, so
/// the ramp is cut into [`SHADE_BANDS`] flat bands of black at alpha
/// `f / 255` - "scale the background by `1 - f/255`" in place of retail's
/// "subtract `f`". The browser play page draws the same bands.
fn shade_band_draws(
    sh: &legaia_engine_render::ringside_backdrop::BackdropShade,
    white_y: u32,
) -> Vec<legaia_engine_render::SpriteDraw> {
    let (x0, y0) = (i32::from(sh.xy[0].0), i32::from(sh.xy[0].1));
    let w = (i32::from(sh.xy[1].0) - x0).max(0) as u32;
    let h = (i32::from(sh.xy[2].1) - y0).max(0);
    let top = f32::from(sh.rgb[0][0]);
    let bottom = f32::from(sh.rgb[2][0]);
    let mut out = Vec::new();
    for b in 0..SHADE_BANDS as i32 {
        let by0 = y0 + h * b / SHADE_BANDS as i32;
        let by1 = y0 + h * (b + 1) / SHADE_BANDS as i32;
        let t = (b as f32 + 0.5) / SHADE_BANDS as f32;
        let f = top + (bottom - top) * t;
        if by1 <= by0 || f <= 0.0 {
            continue;
        }
        out.push(legaia_engine_render::SpriteDraw {
            dst: (x0, by0, w, (by1 - by0) as u32),
            src: (0, white_y, 1, 1),
            color: [0.0, 0.0, 0.0, f / 255.0],
        });
    }
    out
}

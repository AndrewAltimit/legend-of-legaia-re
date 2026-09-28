//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;

/// The Baka duel surface on the GPU: the duel VRAM (re-uploaded when the
/// engine's surface generation moves - a rung seats a new opponent) and this
/// frame's posed meshes, the textured and untextured halves of the one
/// buffer set `BakaDuelScene` builds.
pub(super) struct BakaDuelGpu {
    pub(super) generation: u32,
    pub(super) vram: UploadedVram,
    pub(super) textured: Option<UploadedVramMesh>,
    pub(super) untextured: Option<UploadedColorMesh>,
    /// `DuelCamera::vp_raw` for this frame's aspect.
    pub(super) mvp: Mat4,
}

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
    /// Seeds a timer on each of the session's events this tick
    /// (`World::minigames.fishing_events`: cadence splash, hook, landed,
    /// snapped, recast), then services every timer through the retail
    /// driver-tail loop
    /// ([`BannerTimer::service`](legaia_engine_render::BannerTimer::service))
    /// and caches this frame's draws for the HUD builder, which is `&self` and
    /// cannot advance them itself.
    ///
    /// The frame step is the engine's fixed one tick per frame (retail reads
    /// `DAT_1f800393`, its frame-rate compensation word).
    pub(super) fn tick_fishing_banners(&mut self) {
        use legaia_engine_core::fishing::PondEvent;
        let world = &self.session.host.world;
        if world.minigames.fishing.is_none() {
            // Left the minigame: drop any half-run banner with the session.
            self.fishing_banners = Default::default();
            self.fishing_banner_draws.clear();
            return;
        }
        for e in &world.minigames.fishing_events {
            match e {
                PondEvent::Splash => self.fishing_banners.splash.start(),
                PondEvent::Hooked(_) => self.fishing_banners.on_hook(),
                PondEvent::Landed(_) => self.fishing_banners.on_landed(),
                PondEvent::Snapped => self.fishing_banners.on_snapped(),
                PondEvent::Recast => self.fishing_banners.on_recast(),
            }
        }
        self.fishing_banner_draws = self.fishing_banners.service_frame(1);
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

    /// The fishing venue's actor-side frame: the free-swimming fish wander
    /// and its retarget ripple, the venue floor solve, the retail camera
    /// publish, the reeling-line actor and its catch bursts, and the
    /// sub-screen idle sway.
    ///
    /// All of it is the shared engine kernel
    /// ([`legaia_engine_core::fishing_venue::tick_fishing_venue_on_host`]),
    /// whose actors live on `World::minigames.fishing_venue`; the browser play
    /// page runs the same call. This host only applies the camera writes to
    /// its own session camera.
    pub(super) fn tick_fishing_actors(&mut self) {
        let writes =
            legaia_engine_core::fishing_venue::tick_fishing_venue_on_host(&mut self.session.host);
        writes.apply(&mut self.session.camera);
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
        let frame = f.chrome_frame().clone();
        // The announcer line the chrome fired (`FUN_8003D53C`), through the
        // XA path the battle clips use - the play page plays the same one.
        if let Some(xa) = frame.xa
            && let Some(bgm) = self.session.bgm.as_mut()
        {
            let fired = bgm.play_xa_clip(u32::from(xa.clip), u32::from(xa.chan), u32::from(xa.dur));
            log::debug!(
                "baka chrome: announcer XA clip {} chan {} ({}) -> {}",
                xa.clip,
                xa.chan,
                xa.dur,
                if fired { "playing" } else { "not staged" }
            );
        }
        // The overlay's HUD widget table, for the glyph-strip paging below.
        // Parsed on the first duel frame whatever opened the duel - the door
        // warp and the `B` launcher enter through the same scene-host arm,
        // which keeps no render tables.
        if self.baka_hud_widgets.is_none() {
            self.baka_hud_widgets = self.baka_overlay_hud_widgets();
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

    /// PROT 0976's HUD widget table in its loaded form, or `None` when the
    /// overlay does not resolve.
    fn baka_overlay_hud_widgets(&self) -> Option<Vec<legaia_asset::baka_opponents::BakaHudWidget>> {
        use legaia_asset::static_overlay;
        let rec = static_overlay::overlay_map()
            .by_prot_index(legaia_asset::baka_opponents::BAKA_OVERLAY_PROT_INDEX as u32)?;
        let raw = self
            .session
            .host
            .index
            .entry_bytes_extended(rec.prot_index)
            .ok()?;
        let loaded = static_overlay::as_loaded(&raw, rec).ok()?;
        legaia_asset::baka_opponents::parse_baka_hud(&loaded)
    }

    /// The slot machine's five paylines as screen primitives: the ported
    /// payline pass (`FUN_801D3380`) with its projection
    /// (`SlotMachine::payline_segments`), turned into one-pixel flat quads by
    /// the shared `ui_slot_paylines` builder. Both browser pages stroke the
    /// same projected segments. Empty outside the slot machine or when the
    /// overlay's payline table did not decode.
    pub(super) fn slot_payline_screen_prims(
        &self,
    ) -> Vec<legaia_engine_render::screen_overlay::ScreenPrim> {
        use legaia_engine_render::ui_slot_paylines as usp;
        if self.session.host.world.mode != SceneMode::SlotMachine {
            return Vec::new();
        }
        let Some(m) = self.session.host.world.minigames.slot_machine.as_ref() else {
            return Vec::new();
        };
        let segments: Vec<usp::PaylineSegment> = m
            .payline_segments()
            .iter()
            .map(|l| usp::PaylineSegment {
                a: [l.a.0, l.a.1],
                b: [l.b.0, l.b.1],
                rgb: [l.prim.color.0, l.prim.color.1, l.prim.color.2],
                semi: l.prim.code & 0x02 != 0,
            })
            .collect();
        usp::payline_screen_prims(&segments)
    }

    /// Pose the Baka duel's 3D surface for this frame and put it on the GPU.
    ///
    /// The pose, the arena camera and the buffers are the engine's
    /// (`legaia_engine_core::baka_duel_scene::BakaDuelSurface::frame`, the
    /// call the browser play page makes too); this host uploads the duel
    /// VRAM on a generation change and the posed meshes every frame, and
    /// the redraw's duel branch draws them under `DuelCamera::vp_raw`.
    /// Drops the GPU copy whenever no duel is on screen.
    pub(super) fn refresh_baka_duel_gpu(&mut self) {
        let live = self.session.host.world.mode == SceneMode::BakaFighter;
        let index = self.session.host.index.clone();
        let read = |i: usize| index.entry_bytes(i as u32).ok().map(|b| b.to_vec());
        let fight = if live {
            self.session.host.world.minigames.baka_fighter.as_ref()
        } else {
            None
        };
        let generation_before = self.baka_surface.generation();
        if self.baka_surface.frame(read, fight).is_none() {
            self.baka_gpu = None;
            return;
        }
        let (Some(r), Some(scene)) = (self.win.renderer.as_ref(), self.baka_surface.scene()) else {
            return;
        };
        let generation = self.baka_surface.generation();
        let (sw, sh) = r.surface_size();
        let (_, aspect) = super::geometry::scene_viewport_for(sw, sh);
        let mvp = Mat4::from_cols_array(
            &fight
                .map(|f| f.duel_camera().vp_raw(aspect))
                .unwrap_or(Mat4::IDENTITY.to_cols_array()),
        );
        let normals = vec![[0.0f32; 3]; scene.positions.len()];
        let textured = r
            .upload_vram_mesh(
                &scene.positions,
                &scene.uvs,
                &scene.cba_tsb,
                &normals,
                &scene.colors,
                &scene.textured_indices,
            )
            .map_err(|e| log::warn!("baka duel: textured upload failed: {e:#}"))
            .ok();
        let fill: Vec<[u8; 3]> = scene
            .flat_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| [c[0], c[1], c[2]])
            .collect();
        let untextured = (!scene.untextured_indices.is_empty())
            .then(|| {
                r.upload_color_mesh(&scene.positions, &fill, &scene.untextured_indices)
                    .map_err(|e| log::warn!("baka duel: untextured upload failed: {e:#}"))
                    .ok()
            })
            .flatten();
        let stale = generation != generation_before
            || self
                .baka_gpu
                .as_ref()
                .is_none_or(|g| g.generation != generation);
        let vram = if stale {
            match self.baka_surface.vram().map(|v| r.upload_vram(&v)) {
                Some(Ok(v)) => v,
                Some(Err(e)) => {
                    log::warn!("baka duel: vram upload failed: {e:#}");
                    return;
                }
                None => return,
            }
        } else {
            match self.baka_gpu.take() {
                Some(g) => g.vram,
                None => return,
            }
        };
        self.baka_gpu = Some(BakaDuelGpu {
            generation,
            vram,
            textured,
            untextured,
            mvp,
        });
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
        // A dome leg the player WALKED into (the mode-24 door warp, drained
        // by the shared scene host) already carries its contest: the warp
        // arm opens it off the unlock flags (`DomeContest::from_overlay`)
        // and seats the lead fighter's costs, so `open_muscle_contest` is a
        // no-op there. It stays as the guard for a leg opened any other way;
        // the hub page's assets are this window's own and load here on every
        // entry, as the browser play page does.
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
        // The screen timers are one engine kernel on both play hosts; this
        // window only sounds what they fired.
        let frame = self
            .muscle_timers
            .tick(&self.session.host.world, pad, volume_word);
        // The first visit's two announcer lines (`FUN_8003D53C` at arms 0 and
        // 0x15), through the same XA path the battle clips use.
        if let Some(c) = frame.xa
            && let Some(bgm) = self.session.bgm.as_mut()
        {
            let fired = bgm.play_xa_clip(
                u32::from(c.clip),
                u32::from(c.channel),
                u32::from(c.duration_sectors),
            );
            log::debug!(
                "dome hub XA clip slot {} ch {} dur {} -> {}",
                c.clip,
                c.channel,
                c.duration_sectors,
                if fired { "playing" } else { "not staged" }
            );
        }
        // Each drained tally lane keys a voice directly, with no cue id in
        // sight (`FUN_801D1288` builds the whole attr set); the director's
        // explicit key-on is the only path that takes it.
        if let Some(bgm) = self.session.bgm.as_mut() {
            for cue in frame.voice_cues {
                bgm.key_on_voice_attr(legaia_engine_audio::VoiceAttr::from_cue_words(
                    cue.voice,
                    cue.vab_program_tone,
                    cue.note_and_fine,
                    cue.volume,
                ));
            }
        }
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
    /// The shared mode-24 door warp now opens the contest itself (the warp
    /// operand names an overlay, not a ladder position, so it stages the
    /// rung off the unlock flags), which makes this a no-op on a door entry;
    /// it covers the `M` launcher and any leg opened without the warp.
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
                    palette_stp: legaia_engine_render::ringside_backdrop::HubPaletteStp::from_tims(
                        &tim0, &tim1,
                    ),
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
    /// Semi-transparent packets ride the returned [`OverlayBlendSpan`]s: the
    /// first visit's shade through the subtractive (`ABR 2`) sprite
    /// pipeline, between the wall tiles and the screens drawn over them, and
    /// a hub quad through its tpage's ABR wherever its palette carries STP
    /// (`HubPaletteStp::quad_abr` - an STP-free palette draws opaque, as
    /// retail's GPU draws it).
    ///
    /// One disclosed stand-in: a packet's vertical two-stop colour gradient
    /// flattens to the stops' mean (the sprite pipeline is one-colour).
    ///
    /// [`OverlayBlendSpan`]: legaia_engine_render::OverlayBlendSpan
    pub(super) fn muscle_hub_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> (
        Vec<legaia_engine_render::SpriteDraw>,
        Vec<legaia_engine_render::OverlayBlendSpan>,
    ) {
        use legaia_engine_render::other_game_hud as hud;
        let Some(assets) = self.muscle_hub.as_ref() else {
            return (Vec::new(), Vec::new());
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
            if let Some(hub) = self.muscle_timers.first_visit {
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
            } else if let Some((round, banner)) = self.muscle_timers.round_banner {
                quads.extend(hud::hub_screen_quads(
                    &mut table,
                    &hud::round_banner_draws(round),
                    banner.brightness(),
                ));
            }
        } else if let Some(interval) = self.muscle_timers.interval {
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
            let (values, row_bright) = match self.muscle_timers.tally.as_ref() {
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
            && self.muscle_timers.interval.is_none()
            && let Some(card) = self
                .muscle_timers
                .backdrop
                .and_then(|b| b.card_brightness())
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
            && let Some(b) = self.muscle_timers.backdrop.filter(|b| b.visible())
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
            return (Vec::new(), Vec::new());
        }
        let shade_at = quads.len();
        quads.extend(front);
        let mut blend = Vec::new();
        let push_shade =
            |out: &mut Vec<legaia_engine_render::SpriteDraw>,
             blend: &mut Vec<legaia_engine_render::OverlayBlendSpan>,
             sh: &legaia_engine_render::ringside_backdrop::BackdropShade| {
                let rows = shade_row_draws(sh, assets.white_y);
                blend.push(legaia_engine_render::OverlayBlendSpan {
                    start: out.len() as u32,
                    count: rows.len() as u32,
                    abr: sh.abr,
                });
                out.extend(rows);
            };
        for (i, q) in quads.iter().enumerate() {
            if i == shade_at
                && let Some(sh) = shade
            {
                push_shade(&mut out, &mut blend, &sh);
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
            // A semi packet blends only through STP texels: the hub's
            // variant-2 palettes (and sheet 1's) carry none, so those
            // packets draw opaque whatever their ABR.
            let semi_abr = assets.palette_stp.quad_abr(q);
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
            if let Some(abr) = semi_abr {
                blend.push(legaia_engine_render::OverlayBlendSpan {
                    start: out.len() as u32,
                    count: 1,
                    abr,
                });
            }
            out.push(legaia_engine_render::SpriteDraw {
                dst: (x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32),
                src: (sx as u32, block_y + sy as u32, csw, csh),
                color: [tint(0), tint(1), tint(2), 1.0],
            });
        }
        if shade_at >= quads.len()
            && let Some(sh) = shade
        {
            push_shade(&mut out, &mut blend, &sh);
        }
        // The quads sit in the retail 320x240 frame; map them through the
        // same stage transform every minigame chrome layer uses.
        let (stage_origin, stage_scale) = self.save_select_stage(surface_w, surface_h);
        legaia_engine_render::scale_stage_text_draws(&mut out, stage_origin, stage_scale);
        (out, blend)
    }

    /// Load the fishing overlay (PROT 0972) and start a fishing session in the
    /// world (suspending the current scene) through the same engine entry the
    /// mode-24 door warp takes (`SceneHost::enter_fishing_from_overlay`): the
    /// species / spawn / cadence tables, the bring-up's rod and lure ownership
    /// scans, the persistent save-block words and the venue the cast lure
    /// lands in. Returns `false` (and logs) when no disc is attached or the
    /// tables can't decode. Mirrors [`Self::start_dance_minigame`]'s overlay
    /// path.
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
        if !self.session.host.enter_fishing_from_overlay(&loaded) {
            log::warn!("fishing: species / spawn / cadence tables did not decode");
            return false;
        }
        true
    }
}

/// The first visit's backdrop shade (`FUN_801D1610`, a subtractive `B - F`
/// Gouraud quad, `0x64` at the top fading to `0` at the bottom) as sprite
/// draws over the atlas's white block, for the caller to put in an `ABR 2`
/// [`legaia_engine_render::OverlayBlendSpan`].
///
/// The sprite pipeline carries one colour per draw, and the quad's colour
/// only varies with `y`, so one draw per display row reproduces the ramp:
/// row `y` subtracts `top + (bottom - top) * y / h`, the Gouraud value at
/// that row. A row that subtracts nothing is skipped.
fn shade_row_draws(
    sh: &legaia_engine_render::ringside_backdrop::BackdropShade,
    white_y: u32,
) -> Vec<legaia_engine_render::SpriteDraw> {
    let (x0, y0) = (i32::from(sh.xy[0].0), i32::from(sh.xy[0].1));
    let w = (i32::from(sh.xy[1].0) - x0).max(0) as u32;
    let h = (i32::from(sh.xy[2].1) - y0).max(0);
    let top = i32::from(sh.rgb[0][0]);
    let bottom = i32::from(sh.rgb[2][0]);
    let mut out = Vec::new();
    for row in 0..h {
        let f = top + (bottom - top) * row / h;
        if f <= 0 {
            continue;
        }
        let f = f as f32 / 255.0;
        out.push(legaia_engine_render::SpriteDraw {
            dst: (x0, y0 + row, w, 1),
            src: (0, white_y, 1, 1),
            color: [f, f, f, 1.0],
        });
    }
    out
}

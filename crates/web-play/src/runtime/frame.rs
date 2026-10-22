//! `tick_frame` and the per-tick steps it runs: field VRAM effects, NPC clip
//! playback, BGM and field-event routing, world effects and the play clock.

use super::*;

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Advance the engine one frame. Returns `""` normally, or the label of the
    /// scene the engine just walked into (a door / warp) - the page rebuilds its
    /// render state whenever the return is non-empty.
    pub fn tick_frame(&mut self) -> Result<String, JsValue> {
        // A card Load / save import parks its save for the resume the page
        // lands in the same turn (`play_resume_save` consumes it). A park
        // nothing consumed must not re-apply the old save over the live party
        // at some later entry: the resume point has passed once the world
        // ticks, so the park does not outlive this frame. The native Load
        // lands and loads in one call (`BootSession::resume_save`) and parks
        // nothing.
        self.pending_card_resume = None;
        // The same for a score hand-off armed alongside it: with no entry to
        // run it, it runs now, over the scene still open.
        self.run_pending_bgm_handoff();
        // The audio director exists before the session ticks (off wasm it is
        // built on first use; on wasm once audio is up), so the session's
        // field SFX routing has it to route into.
        let _ = self.audio_director();
        let Some(session) = self.scene_host.session_mut() else {
            self.world.tick();
            return Ok(String::new());
        };
        // The naming prompt is modal: the field is frozen under it, and the
        // page's overlay steps it (`play_name_entry`). A catch-up tick that
        // lands while it is up runs nothing, so no stray pad edge reaches
        // the prompt through the session's own name-entry arm.
        if session.host.world.name_entry_active() {
            return Ok(String::new());
        }
        // A movie owns the frame. The native window freezes every world tick
        // under one (`run_ticks = 0` while its decoder handle is live), and so
        // does this host: under a movie the session does not tick. The FMV
        // service below still runs: it is what advances the picture and ends
        // the cutscene.
        let movie_held = self.fmv.armed_for().is_some();
        let mut options_adopted = false;
        let event = if movie_held {
            SceneTickEvent::Stepped
        } else {
            // The native session's frame (`BootSession::tick`): the mode
            // seat's frame, the camera's half before the world tick (with the
            // page's own azimuth), the world tick, the tick's BGM events, the
            // camera's half after it, the SFX queue dropped on a door, the
            // field SFX routing (CD-XA lines included), and the mode word
            // adopted. The page owns the pause menu and the per-tick queue
            // drains, which the session was told at install.
            session.camera_azimuth_override = self.camera_azimuth_override.take();
            // The options' simulation knobs and the camera distance,
            // re-asserted ahead of the tick as the native window does in its
            // `resolve_tick_pad`: one session step on both hosts. A save
            // load's option words land first, as the native window takes
            // them (retail restores them with the card).
            options_adopted =
                session.adopt_loaded_options(&mut self.options_state, &mut self.options_pins);
            session.apply_option_knobs(&self.options_state, self.precise_movement_override);
            session
                .tick()
                .map_err(|e| JsValue::from_str(&format!("tick: {e:#}")))?
        };
        // FMV beats: the movie path lives in [`crate::play_fmv`]; it hands
        // back the scene label when the post-movie hand-off entered one (the
        // session's hand-off, which resets the camera globals and drops the
        // SFX queue as a door does).
        if options_adopted {
            self.persist_and_apply_options();
        }
        let fmv_handoff_scene = self.service_cutscene_fmv();
        // ...and the rest of the frame tail is the world's, so it freezes
        // with the scene tick. A movie that ends this frame falls through, as
        // it does natively (the finish is drained before the window counts
        // its ticks).
        if movie_held && fmv_handoff_scene.is_empty() && self.fmv.armed_for().is_some() {
            return Ok(String::new());
        }
        // A scene swap - a door or the post-movie hand-off - restarts the
        // between-beat cutscene glide; the camera globals' reset is the
        // session's.
        if matches!(event, SceneTickEvent::SceneEntered { .. }) || !fmv_handoff_scene.is_empty() {
            self.cutscene_glide.reset();
        }
        // Advance the world's play clock off the page's wall clock, the same
        // delta-against-a-high-water-mark the native window runs.
        self.tick_play_clock();
        // A field-VM op-0x49 sub-0 merchant (or sub-7 prize counter) armed a
        // screen this tick: hand it to the menu runtime so the page can open
        // it, where the native window polls - after the session tick, ahead
        // of the effect step and of the tick's audio frame, which sounds the
        // opening blip. The field VM stays suspended (op-0x49 Armed) until
        // `play_shop_input` sees the session end and calls
        // `finish_field_shop`.
        self.poll_field_shop();
        // Effect scene-graphs, ticked exactly where the native window ticks
        // them: drain the two production spawn requests (a player Seru-magic
        // cast, and a non-summon move whose power record carries a spawnable
        // effect list), then advance the summon / move-FX / field-FX
        // scene-graphs through the move VM. All three self-gate to a no-op
        // when nothing is live, and all three are host-agnostic simulation -
        // leaving them unticked in the browser meant a cast spawned an effect
        // that then never moved.
        self.tick_world_effects();
        // Fishing HUD one-shot banners ride the sim clock, not the page's
        // animation frame, so a heavy scene does not slow them down.
        self.tick_fishing_banners();
        // Field VRAM effects: CLUT-walk shimmer + ambient palette cyclers +
        // scripted CLUT fx, drained against the scene VRAM; the page re-reads
        // `field_vram_bytes` when `field_vram_take_dirty` reports a change.
        self.step_field_vram_fx();
        // Battle presentation: encounter-banner arming on the Field -> Battle
        // edge, battle-event fold, HUD row refresh, popup aging
        // ([`crate::play_battle`]). Cheap no-op outside battle.
        self.tick_battle_presentation();
        // In-world minigame presentation (casino / dance / arena sessions
        // the scene host installed): the draw-side state the page reads.
        self.tick_minigame_ui();
        // Muscle Dome: the round time meter, the session's step the native
        // window names the same way.
        self.tick_muscle_time_meter();
        // Sound-effect channel: step the footstep cadence, advance the delay
        // scheduler, key whatever matured.
        //
        // **After** the two queue drains above, not before them. The audio
        // frame is one session step on both hosts
        // (`BootSession::step_audio_frame`), which the native window runs
        // from its own `tick_sfx` right after its battle-event drain, so a
        // cue whose `timing_frames` is `0` - every strike impact, every
        // minigame blip - sounds on the tick that raised it. Advancing the
        // scheduler first made this host's copy of that cue wait a whole
        // frame, which is not a delay anyone can hear on its own but puts
        // the impact one frame off the animation it is supposed to land on.
        self.tick_sfx();
        // The field-to-battle intro emitter: armed while the encounter
        // session sits in `Transition`, dropped when it leaves; caches this
        // frame's screen-prim geometry for the page's pass. Cheap no-op
        // outside a transition.
        self.tick_battle_intro();
        // Party wipe: raise the game-over panel on the `World::game_over`
        // edge, the same probe the native window's redraw loop runs.
        self.poll_game_over();
        // Field party-status HUD countdown, ticked where the native window
        // ticks it (`FUN_801D0D38`); the draw pass reads the decision back.
        self.tick_field_party_hud();
        self.tick_passive_hud();
        // Developer menu (the visitor's explicit opt-in): ticked exactly
        // where the native window's redraw loop ticks its own, off the same
        // world pad words. A no-op while the opt-in is off.
        self.tick_dev_menu();
        // Drain every field-VM event the BGM router handed back (and, with
        // audio off, the BGM ones too) - the browser twin of the native
        // `drain_and_route_field_events`. `World::pending_field_events` is
        // only ever emptied by a consumer, and this host used to leave the
        // non-BGM events on it forever: a session's queue grew with every
        // camera beat, item grant and dialog open.
        self.drain_and_route_field_events();
        // The FMV hand-off loaded a scene without going through the field
        // VM's transition op, so it produces no `SceneEntered` event - the
        // page still has to rebuild, or it draws the old scene's meshes over
        // the new world. (Its SFX queue was already dropped above, before
        // this tick's ring ops were replayed.)
        //
        // A door (`SceneEntered`) rebuilds the same way. `town01` keeps its
        // establishing-sweep timeline: the page draws the name-entry overlay
        // its pinned op-0x49 opens (`crate::play_name_entry`), so the
        // suspended script has a surface to resume from. No bank is staged
        // and the dedupe latch is kept, so a track that carries across the
        // transition keeps its playhead and its samples (the native
        // `after_scene_swap` does the same).
        let entered = if !fmv_handoff_scene.is_empty() {
            fmv_handoff_scene
        } else if let SceneTickEvent::SceneEntered { name } = event {
            name
        } else {
            String::new()
        };
        if !entered.is_empty() {
            self.rebuild_render_state()?;
        }
        // The rest of the tick's tail runs on an entry tick too, as it does
        // in the native window's loop, which rebuilds and carries on. This
        // host used to return straight after the rebuild, so the entry
        // tick's rig change, merchant arm and NPC clip step were skipped.
        //
        // A `CC F8 50` re-staged the player's model this tick: rebuild the
        // rig from the new mesh, the native window's twin
        // (`rebind_live_npc_models` there drains the same signal).
        if let Some(h) = self.scene_host.host_mut()
            && h.world.take_player_rig_change()
        {
            legaia_engine_session::scene_actors::bind_lead_rig(h, self.locomotion_anm.as_ref());
            self.build_player_rig();
        }
        self.drive_npc_clips();
        Ok(entered)
    }
}

impl LegaiaRuntime {
    /// Drain this sim tick's VRAM-mutating field effects against the host's
    /// scene VRAM - the browser twin of the native play-window's
    /// `apply_world_clut_fx` + water-CLUT animator: the type-6 CLUT-walk
    /// shimmer, the scripted `MoveImage` stamps, the ambient move-VM tree
    /// (jou's pulsating-flesh palette cyclers + lightning), and the CLUT-cell
    /// one-shots. Battle-guarded like the native path: while a battle is up
    /// the page's GPU texture holds the battle VRAM and a field re-upload
    /// would clobber it.
    fn step_field_vram_fx(&mut self) {
        let Some(host) = self.scene_host.host_mut() else {
            return;
        };
        if host.world.mode == SceneMode::Battle {
            return;
        }
        let Some(res) = host.resources.as_mut() else {
            return;
        };
        let mut dirty = false;
        if let Some(anim) = self.field_vram_anim.as_mut() {
            // Live divisor, not the value this scene was rebuilt at - the
            // native animator reads `clock.frame_step` off the world on
            // every frame.
            anim.set_frame_step(host.world.clock.frame_step);
            dirty |= anim.tick(1, &mut res.vram);
        }
        // The scripted VRAM effects (op-`0x43` stamps + rect copies, the
        // ambient move-VM tree, CLUT-cell one-shots and blend fades) through
        // the shared frame-tail kernel the native window's
        // `apply_world_clut_fx` calls; the page presents one framebuffer
        // page, so the back-buffer bias is off.
        dirty |= host.world.step_field_vram_effects(&mut res.vram, false);
        self.field_vram_dirty |= dirty;
    }

    /// One sim tick of the actor layer's clip playback
    /// ([`crate::field_actors::FieldActors::drive`]).
    fn drive_npc_clips(&mut self) {
        let Some(host) = self.scene_host.host_mut() else {
            return;
        };
        let banks = crate::field_actors::ActorBanks {
            scene_anm: self.scene_anm.as_ref(),
            locomotion_anm: self.locomotion_anm.as_ref(),
        };
        self.actors.drive(host, banks);
    }

    /// Drain this tick's field-VM BGM events into the page's director.
    /// Runs only while a director exists (on wasm: audio is up); until then
    /// the events are dropped by `drain_and_route_field_events`, as an
    /// unheard retail op-`0x35` would be, and the late-audio start
    /// (`start_current_bgm_on_late_audio`) brings the scene's track up.
    fn route_bgm(&mut self) {
        if self.audio_director().is_none() {
            return;
        }
        let Some((host, d)) = self.scene_host.host_director_mut() else {
            return;
        };
        if let Err(e) = host.route_bgm_events(d) {
            crate::console_log(&format!("play BGM: route failed: {e:#}"));
        }
    }
}

impl LegaiaRuntime {
    /// Consume this tick's remaining field-VM events - the browser twin of
    /// the native window's `drain_and_route_field_events`
    /// (`window/boot_cutscene.rs`). BGM events are normally consumed by
    /// `route_bgm` first; while audio is down (no `WebAudioOut` yet) they
    /// come through here and are dropped, exactly as an unheard retail
    /// op-`0x35` would be. `ActorSpawned` is noted for the page's dynamic
    /// mesh upload; everything else is presentation this host reads off the
    /// world's own state instead (the cutscene camera params, the dialog box).
    pub(crate) fn drain_and_route_field_events(&mut self) {
        // Route any BGM event the frame's own steps raised since the routing
        // pass after the scene tick (a battle-presentation or minigame swap,
        // a dance song ending) before the drain below drops whatever is left.
        // The native `drain_and_route_field_events` opens with the same pass.
        self.route_bgm();
        // Only the session arm of `tick_frame` reaches this step: the
        // disc-free scaffold world returns before it.
        let Some(host) = self.scene_host.host_mut() else {
            return;
        };
        // The one drain both hosts run (`World::drain_spawned_mesh_slots`).
        host.world
            .drain_spawned_mesh_slots(&mut self.pending_dynamic_mesh_slots);
    }
}

/// Milliseconds since a fixed origin, from whatever clock this target has.
/// Only ever consumed as a delta ([`LegaiaRuntime::tick_play_clock`]), so the
/// origin itself is irrelevant.
fn wall_clock_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0)
    }
}

impl LegaiaRuntime {
    /// The tick's effect step, the native window's `tick_world_effects` leg
    /// for leg: seat a cast's summon creature, then the session's move-FX
    /// spawn + scene-graph tick (`BootSession::step_effect_scene_graphs`).
    ///
    /// * `take_pending_summon_spawn` -> [`Self::spawn_summon_creature_web`]:
    ///   a player Seru-magic cast's namesake `battle_data` creature, seated
    ///   and drawn through the enemy animation pipeline
    ///   ([`crate::play_battle_fx`]). The seat is a mesh bind, which is this
    ///   host's, and it runs first so the creature's first frame ticks on the
    ///   tick that cast it.
    /// * the session step: `take_pending_move_fx_spawn` ->
    ///   [`World::spawn_move_fx`](legaia_engine_core::world::World::spawn_move_fx),
    ///   the spawned move's sound cue through the retail dispatch decode
    ///   (`classify_cue` = `FUN_8004FCC8`) onto the director's scheduler, then
    ///   `tick_summon` / `tick_move_fx` / `tick_field_fx` at the retail
    ///   `0x0400` anim-speed step. Each self-gates when nothing is live.
    fn tick_world_effects(&mut self) {
        let summon = match self.scene_host.host_mut() {
            Some(host) => host.world.take_pending_summon_spawn(),
            None => return,
        };
        if let Some((spell_id, _origin)) = summon {
            self.spawn_summon_creature_web(spell_id);
        }
        let cue = self
            .scene_host
            .session_mut()
            .and_then(|s| s.step_effect_scene_graphs());
        // Counted for the page's readout whether or not a director heard it.
        if cue.is_some() {
            self.sfx.queued += 1;
        }
    }

    /// Advance the world's play clock off the page's wall clock - the browser
    /// twin of the native window's `tick_play_clock`, called once per
    /// [`LegaiaRuntime::tick_frame`].
    ///
    /// Whole seconds only, and by delta rather than absolutely, so a loaded
    /// save keeps its accumulated total. The page used to substitute
    /// `world.frame / 60` at the one place the clock was *drawn*, which left
    /// [`legaia_engine_vm::frame_clock::FrameClock::play_time_seconds`] frozen at
    /// whatever a load put there - so the H:MM:SS box reset on every page
    /// load, ignored a loaded save's hours, and, worse, a save written from
    /// the browser recorded the *loaded* play time rather than the played one.
    ///
    /// The origin and high-water mark are the world's
    /// ([`legaia_engine_core::world::World::tick_play_clock`], the kernel the
    /// native window calls too), so New Game restarts both. This page used to
    /// keep them itself and reset only the origin on New Game, which froze
    /// play time after a second New Game until the wall clock caught up with
    /// the old mark.
    pub(crate) fn tick_play_clock(&mut self) {
        let now_secs = wall_clock_ms() / 1000.0;
        if let Some(host) = self.scene_host.host_mut() {
            host.world.tick_play_clock(now_secs);
        }
    }
}

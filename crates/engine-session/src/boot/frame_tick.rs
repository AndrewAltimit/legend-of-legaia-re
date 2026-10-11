//! One session frame: `BootSession::tick` and the steps around it (fog render
//! tick, global and field BGM, field SFX routing, the post-swap fix-ups, the
//! FMV hand-off, the world frame tail and the name-entry frame).

use super::*;

impl<S: AudioSink> BootSession<S> {
    /// The fog pool's render step the play hosts run from their draw pass
    /// (`World::fog_render_step` through the field follow frame), for a
    /// headless driver that renders nothing but must keep the world in step
    /// with a host that does.
    ///
    /// The step is not presentation-only: it ages the pool and writes the
    /// live count (`_DAT_8007BCA8`) and the depth view the ambient emitter's
    /// fog spawner reads on the next tick (`FogPool::spawn`'s cap and
    /// overworld depth test), and each spawn that passes draws the world
    /// `rand()` stream. A driver that skips it runs a fogged scene on a
    /// different stream from the window. Same guard as the hosts: nothing
    /// runs outside game mode 3 or while the script gate is clear. The
    /// centre is only the frame resolver's no-camera fallback.
    pub fn fog_render_tick(&mut self) {
        let world = &mut self.host.world;
        if !legaia_engine_core::world::World::fog_mode(world.mode) || !world.fog.gate {
            return;
        }
        let frame = legaia_engine_core::camera_view::resolve_field_camera(
            world,
            &self.camera,
            None,
            [0.0, 0.0],
        );
        if let Some(view) = frame.field_view() {
            let _ = world.fog_render_step(&view);
        }
    }

    /// Start a **global-pool** `music_01` track (`bgm_id >= 2000`) through the
    /// BGM director: resolve the bank entry, upload its own VAB, and play its
    /// SEQ. This is how a minigame (or any caller with a disc-pinned track id)
    /// starts music that doesn't live in the current scene's sound bank -
    /// the dance overlay's chart loops, the Baka Fighter overture, the Muscle
    /// Dome battle theme. Returns `false` when audio is off, the id isn't a
    /// bank slot, or the entry doesn't decode. The slot machine + fishing
    /// deliberately don't call this: retail inherits the host scene's BGM.
    pub fn start_global_bgm(&mut self, bgm_id: u16) -> bool {
        // A scene-local id loads retail's fallback track, as the op-0x35
        // route does, so a card load whose save carries one restores a track
        // rather than silence.
        let bank_id = legaia_engine_core::scene_host::bgm_bank_id(bgm_id);
        let Ok(Some(entry)) = self.host.music_bank_entry_bytes(bank_id) else {
            return false;
        };
        let Some(bgm) = self.bgm.as_mut() else {
            return false;
        };
        bgm.start_owned_vab(bgm_id, &entry);
        true
    }

    /// Restart the field scene's BGM after a minigame that took over the
    /// director with its own global track (dance / Baka Fighter / Muscle
    /// Dome). Re-plays whatever op-`0x35` track the scene had running
    /// ([`legaia_engine_core::world::AudioState::current_bgm`](legaia_engine_core::world::AudioState::current_bgm)),
    /// re-uploading its VAB. No-op when the scene had no track or it isn't a
    /// global-pool id. The slot machine + fishing don't need this: they never
    /// replaced the director's bank.
    pub fn restore_field_bgm(&mut self) {
        if let Some(id) = self.host.world.audio.current_bgm {
            self.start_global_bgm(id);
        }
    }

    /// One per-frame step: tick the world, route field-VM camera + BGM
    /// events, advance the camera follow, return the [`SceneTickEvent`] for
    /// engines that want to react to scene transitions.
    /// Hand this tick's SFX ring producer calls (field-VM op `0x36` sub
    /// `0`/`4`, the ambient motion VM's op `0x09`) to the director's retail
    /// ring, and keep the director's field-side SFX sources - the scene's
    /// runtime descriptor rows and the side-band bank - in step with the
    /// world. The director's own per-frame [`AudioBgmDirector::tick_sfx_frame`]
    /// then plays whatever came due. With no audio the calls are dropped, as
    /// every other cue is.
    fn route_field_sfx(&mut self) {
        if self.host_stages_field_xa {
            // The host's own XA lane drains both queues; the rest of the
            // routing is the director's.
            if let Some(bgm) = self.bgm.as_mut() {
                bgm.route_world_sfx(&mut self.host.world, &self.host.index);
            } else {
                let w = &mut self.host.world;
                let _ = (
                    w.take_sfx_ring_ops(),
                    w.take_sfx_voice_stops(),
                    w.take_sfx_voice_keys(),
                );
            }
            return;
        }
        // The field's CD-XA one-shots (op `0x36`'s XA arm, the scripted-scene
        // voice leg) - drained every tick so none outlives its frame.
        let field_xa = self.host.world.drain_field_xa_cues();
        // The scene's CD-XA prestage list is for a host that decodes clips
        // asynchronously (the browser page stages it); this director reads a
        // clip's span synchronously on first use, so the list is dropped.
        let _ = self.host.world.drain_field_xa_prestage();
        let Some(bgm) = self.bgm.as_mut() else {
            // No audio: the producer queues are dropped, as every cue is.
            let w = &mut self.host.world;
            let _ = (
                w.take_sfx_ring_ops(),
                w.take_sfx_voice_stops(),
                w.take_sfx_voice_keys(),
            );
            return;
        };
        for xa in &field_xa {
            let fired = bgm
                .play_xa_clip(xa.clip, xa.channel, xa.duration_sectors)
                .is_some();
            log::debug!(
                "field XA clip slot {} ch {} dur {} -> {}",
                xa.clip,
                xa.channel,
                xa.duration_sectors,
                if fired { "playing" } else { "not staged" }
            );
        }
        bgm.route_world_sfx(&mut self.host.world, &self.host.index);
    }

    /// The session-side half of a scene swap under the host: the camera
    /// globals reset and the SFX queue dropped (no bank is staged).
    /// A door (`SceneTickEvent::SceneEntered`) and the post-FMV hand-off
    /// ([`Self::apply_pending_fmv_handoff`]) both swap the scene, and only the
    /// first used to reach this - so a movie that handed off into a new scene
    /// kept the trigger scene's camera shot and VAB bank.
    fn after_scene_swap(&mut self) {
        // The camera globals' reset (`FUN_80025C24`) is not here: a door's
        // runs inside `frame_step::camera_after_world_tick`, the tick order
        // both hosts share, and the post-FMV hand-off runs it itself.
        if let Some(bgm) = self.bgm.as_mut() {
            // New scene -> drop any SFX cues queued against the previous
            // one. No bank is staged: retail's field init loads no scene
            // bank (its only bank loads are slot 6 and the ending arm), and
            // the BGM slot changes only with the track, so a track carried
            // across the door keeps its samples.
            bgm.clear_sfx();
            // Nothing is flushed here. Op-`0x35` sub-op 9 - the op a cutscene
            // changes music with - is a *start* behind a load barrier, not a
            // queue for the next scene; it plays the moment
            // `route_bgm_events` hands it over. Deferring it to this point is
            // what left every Biron Monastery cutscene silent and then
            // started its score over the next scene.
        }
    }

    /// Run retail's post-FMV control transfer
    /// ([`SceneHost::apply_pending_fmv_handoff`]) and, when it entered a new
    /// scene, the same session-side swap a door runs. Every engine-shell host
    /// calls this rather than the bare host kernel; the render-side rebuild
    /// stays with the caller, keyed on [`FmvHandoffOutcome::Entered`].
    ///
    /// [`FmvHandoffOutcome::Entered`]: legaia_engine_core::scene_host::FmvHandoffOutcome::Entered
    pub fn apply_pending_fmv_handoff(
        &mut self,
    ) -> Option<legaia_engine_core::scene_host::FmvHandoffOutcome> {
        let outcome = self.host.apply_pending_fmv_handoff()?;
        if matches!(
            outcome,
            legaia_engine_core::scene_host::FmvHandoffOutcome::Entered { .. }
        ) {
            // Field entry resets the camera globals (`FUN_80025C24`) and
            // kills any mover in flight, so the movie's trigger scene cannot
            // leak its shot into the next one - as on a door.
            self.camera.reset_globals_for_scene_entry();
            self.after_scene_swap();
        }
        Some(outcome)
    }

    /// The world-side half of the per-tick tail both play hosts run after the
    /// scene tick ([`legaia_engine_core::world::World::step_world_frame_tail`]
    /// plus [`legaia_engine_core::world::World::step_field_vram_effects`] over
    /// the scene's own VRAM image, as the browser page steps it). What the
    /// tail hands back for drawing is dropped - a headless session draws
    /// nothing - except the spawned move's sound cue, which goes to the
    /// director when one is attached, the way both hosts route it.
    fn run_world_frame_tail(&mut self) {
        // The summon spawn request's seat: both play hosts bind the creature
        // mesh and seat it here, and a directed module's walk arm reads the
        // seat, so a headless run seats it unrendered.
        self.host.world.seat_summon_creature_unrendered();
        let tail = self.host.world.step_world_frame_tail(None, None, |_| None);
        if let (Some(cue), Some(bgm)) = (tail.move_fx_cue, self.bgm.as_mut())
            && let legaia_engine_audio::CueDispatch::Ring { ring_value, .. } =
                legaia_engine_audio::classify_cue(u32::from(cue))
        {
            bgm.enqueue_sfx(ring_value, 0, 0, 0);
        }
        if let Some(res) = self.host.resources.as_mut() {
            let _ = self
                .host
                .world
                .step_field_vram_effects(&mut res.vram, false);
        }
    }

    /// One frame under the naming prompt: the edge drives the entry SM
    /// (`World::step_name_entry_frame`, which also passes the display frame)
    /// and the camera's half still runs, because retail's field frame keeps
    /// stepping the camera mover around the prompt actor - the opening's
    /// last glide lands under it. `false` when no prompt is open. The native
    /// window's per-tick arm calls this too.
    pub fn step_name_entry_frame(&mut self, edge: u16) -> bool {
        if !self.host.world.step_name_entry_frame(edge) {
            return false;
        }
        legaia_engine_core::frame_step::camera_after_world_tick(
            &mut self.camera,
            &mut self.host.world,
            false,
        );
        true
    }

    pub fn tick(&mut self) -> Result<SceneTickEvent> {
        // The hosts' per-tick queue duty, for a caller that does not perform
        // it: what the previous tick queued and nobody took is gone before
        // this one runs, as it is in both play hosts.
        if !self.host_drains_queues {
            std::mem::take(&mut self.queue_marks).drop_stale(&mut self.host.world);
        }
        // The naming prompt (field-VM op `0x49`, the opening's pc `0x02C6`) is
        // modal: the field is frozen under it and every pad edge drives the
        // entry SM. The edge is the one the caller's `set_pad` just made.
        // Before this arm only the two window hosts routed the prompt, so any
        // driver that ticks the session - every headless run - parked on it
        // forever. The native window still takes its own arm first (it also
        // skips its per-frame tail); both call the same kernel.
        self.step_pause_wipe();
        let input = &self.host.world.input;
        let edge = input.pad() & !input.pad_prev();
        if self.step_name_entry_frame(edge) {
            return Ok(SceneTickEvent::Stepped);
        }
        // The mode table's outer level, once per frame, ahead of everything
        // else - retail's `main` (`FUN_80015E90`, `0x8001615C..0x8001620C`)
        // takes any pending mode-change edge before it dispatches the new
        // mode's handler. The edge is not bookkeeping: it swallows the pad
        // edges (`gp+0x538` / `gp+0x55C`) so the button that caused the
        // transition is not re-delivered to the mode it opened.
        let mode_frame = self.mode_seat.frame(&mut self.host.world);
        if let Some(edge) = mode_frame.edge {
            log::debug!(
                "mode {:?} -> {:?} ({})",
                edge.from,
                edge.to,
                self.mode_seat.mode_name()
            );
        }
        // Pause menu (retail CARD pair, game_mode 0x17). Drive the Start edge
        // through the shared predicate rather than a local mode test: retail's
        // accept is a leg of the locomotion controller `FUN_801D01B0`, so the
        // menu opens wherever that controller walks the player - towns,
        // fields *and* the three kingdom overworlds, which are ordinary
        // `game_mode 0x03` field-run scenes in retail even though the port
        // models them as `SceneMode::WorldMap`. Spelling `SceneMode::Field`
        // here is what previously made the Save row - legal only on those
        // overworlds - unreachable by pad anywhere in the port.
        //
        // The windowed host never reaches this auto-path (it handles the
        // Start edge itself and skips `tick` while its boot-UI owns the
        // frame), so the two hosts can't double-drive the session.
        //
        // REF: FUN_801D01B0 (`0x801D0250`, the menu-open accept)
        // A script's op-`0x49` save point / ready check is a menu-button press
        // of its own (`World::scripted_menu_open_pending`): it opens the menu
        // with no Start edge and past the engagement gate the Start path
        // keeps, exactly once per arm.
        let menu_opened_this_tick = !self.host_owns_pause_menu
            && matches!(
                self.press_field_menu(self.host.world.input.just_pressed(PadButton::Start)),
                PauseMenuPress::Opened { .. }
            );
        if !self.host_owns_pause_menu && !menu_opened_this_tick && self.field_menu.is_some() {
            let close = self.tick_field_menu();
            if close {
                self.close_field_menu();
            }
        }
        // The camera's half before the world tick (shared with the browser
        // page, `frame_step::camera_before_world_tick`): snap a free-roam
        // camera back to the follow default - a cutscene's op-0x45 events
        // leave it Cinematic at the shot's yaw, and the stale yaw would rotate
        // the d-pad remap ~180deg off the on-screen camera - then publish the
        // compass azimuth (scripted yaw + the user's drag-orbit + the host
        // framing bias) the field controller reads THIS tick.
        legaia_engine_core::frame_step::camera_before_world_tick(
            &mut self.camera,
            &mut self.host.world,
            self.camera_azimuth_override.take(),
        );
        let event = self.host.tick()?;
        if let Some(bgm) = self.bgm.as_mut() {
            // SceneHost::route_bgm_events drains the world's pending BGM
            // events and dispatches into the director.
            let _ = self.host.route_bgm_events(bgm)?;
        }
        // The camera's half after it: route this tick's op-0x45 events,
        // advance the globals, and reset them on a scene entry
        // (`frame_step::camera_after_world_tick`).
        let scene_entered = matches!(event, SceneTickEvent::SceneEntered { .. });
        legaia_engine_core::frame_step::camera_after_world_tick(
            &mut self.camera,
            &mut self.host.world,
            scene_entered,
        );
        if scene_entered {
            self.after_scene_swap();
        }
        self.route_field_sfx();
        if !self.host_drains_queues {
            // The world-side half of the hosts' frame tail - effect
            // scene-graphs, move-FX / effect-script spawns, ANIMATE cues and
            // the scripted VRAM effects - which the play window runs itself.
            self.run_world_frame_tail();
        }
        // Reconcile the word with wherever the scene sessions left the world.
        // The seat owns the word; the sessions own the scene, and this is the
        // one join between them (see `ModeSeat`'s "what owns what").
        self.mode_seat.adopt_world_mode(&self.host.world);
        if !self.host_drains_queues {
            // What this tick queued stays readable until the next one starts.
            self.queue_marks = HostQueueMarks::record(&self.host.world);
        }
        self.frames += 1;
        Ok(event)
    }
}

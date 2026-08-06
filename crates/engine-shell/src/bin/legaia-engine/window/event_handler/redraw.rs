//! `RedrawRequested` window-event handler (per-frame tick + render),
//! extracted from `event_handler.rs` (mechanical split; behavior-preserving).

use super::super::*;

/// What the frame presents: the scene alone, or the scene with the
/// field-to-battle transition's screen primitives composited over it.
///
/// A function rather than an inline expression because the screenshot
/// harness has to capture the *same* target it presents - capturing a bare
/// `Scene` there drops every transition style from the PNGs.
fn present_target<'a>(
    scene: &'a RenderScene<'a>,
    prims: &'a [legaia_engine_render::screen_overlay::ScreenPrim],
) -> RenderTarget<'a> {
    if prims.is_empty() {
        RenderTarget::Scene(scene)
    } else {
        RenderTarget::SceneWithScreenPrims { scene, prims }
    }
}

impl PlayWindowApp {
    pub(super) fn handle_redraw(&mut self) {
        // Opt-in frame profiler (`LEGAIA_PROFILE=1`; see
        // `legaia_engine_render::profile`). Free when off - each call is a
        // cached-bool branch. The stage marks below carve the frame into
        // tick / pose / drawlist / acquire / uniforms / encode / submit /
        // present.
        legaia_engine_render::profile::begin_frame();
        let dt = self.win.advance_tick(100);
        // Drain up to 4 ticks per render frame so we never spiral
        // but can still catch up from minor vsync jitter.
        let ticks = self.win.drain_ticks(dt, 4);
        // In-flow windowed cutscene: when the field VM's FMV-trigger
        // op flips the world into SceneMode::Cutscene and the STR has
        // decoded, suspend world ticks and play the video in-window.
        // Once its frames drain, resume the field (`finish_cutscene`).
        if self
            .cutscene
            .as_ref()
            .is_some_and(|c| c.idx >= c.frames.len())
        {
            // Stop the cutscene audio and resume the scene sequencer
            // (BGM was paused while the movie played).
            if let Some(out) = self.session.audio.as_ref() {
                out.stop_xa();
                out.set_sequencer_paused(false);
            }
            self.session.host.world.finish_cutscene();
            self.cutscene = None;
        }
        let run_ticks = if self.cutscene.is_some() { 0 } else { ticks };
        for _ in 0..run_ticks {
            self.tick_no += 1;
            // Scripted keyboard harness (`--key-script`): deliver this tick's
            // keys through the real keyboard arms, press then release, before
            // the pad injection below. Order matters both ways round: the key
            // arms run first so a minigame entry is open for the rest of the
            // tick, and the pad write lands after so a key that also binds to
            // a pad button cannot leave a bit latched into `set_pad`.
            let scripted_keys = self
                .screenshot
                .as_ref()
                .map(|sc| {
                    sc.key_script
                        .get(&self.tick_no)
                        .cloned()
                        .unwrap_or_default()
                })
                .unwrap_or_default();
            for code in scripted_keys {
                self.handle_key(code, ElementState::Pressed);
                self.handle_key(code, ElementState::Released);
            }
            // Screenshot harness: inject the scripted one-tick pad edge for
            // this tick (overriding keyboard). Ticks with no script entry get
            // a neutral pad so the previous press releases (edge resets).
            let scripted_pad = self
                .screenshot
                .as_ref()
                .map(|sc| sc.pad_script.get(&self.tick_no).copied().unwrap_or(0));
            if let Some(pad) = scripted_pad {
                self.pad = pad;
            }
            // Party wipe: the world raises `game_over` when a battle
            // resolves to `BattleEndCause::PartyWipe`. Consume the flag and
            // start the return-to-title hand-off, which owns the frame from
            // here (the arm below skips the scene tick while any boot UI is
            // active). Retail's wipe arm stores mode 22 CARD INIT with the
            // title context word set, so the destination is the title screen
            // and nothing is asked of the player on the way.
            if self.session.host.world.game_over && !self.boot_ui.is_active() {
                self.session.host.world.game_over = false;
                self.boot_ui =
                    BootUiState::GameOver(legaia_engine_core::game_over::GameOverSession::new());
                log::info!("play-window: party wipe -> title screen");
            }
            // When the boot UI is active, route input there and skip
            // the scene tick - the player hasn't entered the world
            // yet (or has paused into save-select).
            if self.boot_ui.is_active() {
                let _ = self.tick_boot_ui();
                self.prev_pad = self.pad;
                continue;
            }
            // Start in field opens the pause menu. Edge-detect so a
            // held key doesn't auto-reopen.
            let pressed_edge = self.pad & !self.prev_pad;
            // Name-entry overlay is modal: while it's open the field is
            // frozen and every pad edge routes into the entry SM (one
            // cell / glyph per press). Mirrors the opening `town01`
            // naming prompt, which suspends the field VM.
            if self.session.host.world.name_entry_active() {
                let p = pressed_edge;
                let input = legaia_engine_core::name_entry::NameEntryInput {
                    up: p & 0x0010 != 0,
                    down: p & 0x0040 != 0,
                    left: p & 0x0080 != 0,
                    right: p & 0x0020 != 0,
                    confirm: p & 0x4000 != 0, // Cross
                    cancel: p & 0x1000 != 0,  // Triangle
                };
                self.session.host.world.step_name_entry(input);
                // Keep the frame counter advancing so the caret blinks.
                self.session.host.world.frame = self.session.host.world.frame.wrapping_add(1);
                self.prev_pad = self.pad;
                continue;
            }
            // Prologue intro-skip (retail FUN_801D1344): while the opening
            // chain plays with the trigger bit armed, a confirm press
            // (Cross) skips the WHOLE remaining opening to `town01` -
            // available mid-narration too (the crawl is timer-driven; retail
            // has no per-line skip).
            if let Some(target) = self
                .session
                .host
                .world
                .take_prologue_handoff(pressed_edge & 0x4000 != 0)
            {
                match self.session.enter_field_live(target, &self.field_live_opts) {
                    Ok(mode) => {
                        log::info!("prologue handoff: entered '{target}' (mode={mode:?})");
                        // `enter_field_scene` installs `town01`'s opening
                        // cutscene timeline (gated on the prologue hand-off):
                        // the establishing camera + Vahn's scripted walk-out
                        // play, and the name-entry overlay opens when the
                        // timeline reaches its pinned op-`0x49` STATE_RESUME
                        // (P2[3] body `0x02c6`) - the faithful in-script
                        // trigger, not a blind host call at the hand-off.
                        //
                        // The host swapped scenes (opdeene -> town01):
                        // rebuild the render-side scene state so Rim Elm's
                        // geometry replaces the prologue's.
                        self.rebuild_scene_render_state();
                    }
                    Err(e) => {
                        log::warn!("prologue handoff: enter '{target}' failed ({e:#})")
                    }
                }
                self.prev_pad = self.pad;
                continue;
            }
            // While the opening narration crawl / title card is on screen the
            // pad is frozen (the timeline owns the scene) but the world keeps
            // ticking so the crawl advances and the timeline's terminal
            // SceneChange can fire (rebuilding render state on a swap).
            if self.session.host.world.cutscene_narration_active()
                || self.session.host.world.cutscene_card.is_some()
            {
                self.session.host.world.set_pad(0);
                match self.session.tick() {
                    Ok(legaia_engine_core::scene::SceneTickEvent::SceneEntered { name }) => {
                        log::info!("opening chain: entered '{name}'");
                        self.rebuild_scene_render_state();
                    }
                    Ok(_) => {}
                    Err(e) => log::error!("session tick (narration): {e:#}"),
                }
                self.prev_pad = self.pad;
                continue;
            }
            // Start opens the pause menu wherever retail's locomotion
            // controller runs, which is the field **and the overworld**.
            // The guard used to be `!menu_runtime.is_open()` alone, so Start
            // mid-battle opened the menu and froze the fight - the boot-UI
            // arm above skips the scene tick, so nothing advanced until the
            // player backed out.
            //
            // It then spelled the mode test out locally as
            // `mode == SceneMode::Field`, on the premise that "on the world
            // map the controller has its own" Start handler. That premise is
            // false: `FUN_801E76D4` is the top-view debug renderer, and the
            // overworld runs the ordinary `FUN_801D01B0` chain. A local copy
            // of the test is exactly how the overworld lost the pause menu,
            // so this asks the engine instead
            // ([`World::field_menu_open_allowed`]) and every host that opens
            // the menu asks the same question.
            if pressed_edge & 0x0008 != 0
                && self.session.host.world.field_menu_open_allowed()
                && !self.menu_runtime.is_open()
            {
                // Start: open the BootSession-hosted pause menu (the
                // retail CARD pair, game_mode 0x17 - the world holds
                // SceneMode::Menu while it is open) and route the
                // window's input + draws to it via the boot-UI arm.
                //
                // The open can be REFUSED - `open_field_menu` declines while
                // a dialogue engagement owns the player, as retail's
                // engaged-bit branch does. Only take the boot-UI arm when a
                // session actually exists, or the window would route input
                // and draws to a menu that is not there while the scene tick
                // stayed skipped.
                self.session.open_field_menu();
                if self.session.field_menu_is_open() {
                    self.boot_ui = BootUiState::FieldMenu { sub: None };
                    self.prev_pad = self.pad;
                    continue;
                }
            }
            // Route this frame's pad into the engine before the
            // tick so World::tick's mode dispatch (world-map
            // controller, field-VM dialog-advance poll) sees real
            // input. Edge detection lives in World.input. While a
            // menu-runtime overlay (shop / inn) is up the pad drives
            // the menu, not the field, so feed the field a neutral pad
            // (the player must not walk while shopping).
            let field_pad = if self.menu_runtime.is_open() {
                0
            } else {
                self.pad
            };
            // Re-assert the precise-movement toggle each tick: scene / New
            // Game transitions can reseed world state, and the toggle is
            // host policy (options file + `R` key), not world state.
            self.session.host.world.precise_movement = self.options_state.precise_movement;
            // Field Move default (pause-menu Walk / Run) + the run button
            // that inverts it. Re-asserted per tick for the same reason as
            // precise movement: it is host policy over reseeded world state.
            self.session.host.world.field_move_run_default =
                self.options_state.field_move == legaia_engine_core::options::FieldMoveOpt::Run;
            // Photosensitivity guard over the ambient palette cyclers -
            // host policy like the two above (default ON; see
            // `OptionsState::reduce_flashing`).
            self.session.host.world.reduce_flashing = self.options_state.reduce_flashing;
            // `set_pad` also latches the run button off the same word, so
            // there is nothing host-side to keep in sync.
            self.session.host.world.set_pad(field_pad);
            match self.session.tick() {
                // Door transition: the host loaded a new scene under
                // the window (field-VM op 0x3E/0x3F or a walk-touch
                // door). Rebuild the render-side scene state so the
                // new scene's geometry/VRAM replace the old one's -
                // without this the world model swaps under the OLD
                // scene's meshes.
                Ok(legaia_engine_core::scene::SceneTickEvent::SceneEntered { name }) => {
                    log::info!("play-window: scene transition -> '{name}'");
                    self.rebuild_scene_render_state();
                }
                Ok(_) => {}
                Err(e) => log::error!("session tick: {e:#}"),
            }
            // Placed-prop animation: advance every posed prop's clip and post
            // the player's contact edges, so walking into a Rim Elm house door
            // resumes its bind script and swings it open (retail's per-actor
            // anim tick `FUN_800204F8`, driven by the body-contact script
            // resume `FUN_801D5B5C`).
            self.tick_field_prop_anims();
            // Baka Fighter duel: drain the exchange-hit SFX cue the rules
            // kernel queued this tick and enqueue it into the SFX scheduler
            // (the per-frame `tick_sfx_frame` below fires it against the
            // resident class-2 sound bank).
            self.drain_baka_sfx_cues();
            // Minigame side-channels: the dance count-in + tutorial + effect
            // spawns, the fishing venue actors (wander / line / floor solve /
            // camera publish / sway), the Baka round chrome, and the shared
            // effect pool. Runs BEFORE tick_fishing_banners so
            // `fishing_prev_phase` still holds last frame's phase for its own
            // edge detection.
            self.tick_minigame_extras();
            // Fishing: advance the HUD's one-shot banner animations (hook /
            // reel-in / miss / auxiliary / strike splash) and cache their
            // draws - the retail driver tail's own per-frame timer loop.
            self.tick_fishing_banners();
            // Muscle Dome: run the round time meter (climbs through the
            // selection phase, drains outside it).
            self.tick_muscle_time_meter();
            // Opt-in synthetic tile board (`LEGAIA_TILE_BOARD_DEMO=1`): no
            // retail scene script installs one, so this is the visual
            // trigger for the per-cell tile-actor draw pass.
            self.maybe_install_demo_tile_board();
            // Advance the world's play clock off the window's wall clock.
            // `World::advance_play_time` is written to be driven "from the
            // frame loop's wall-clock delta" and no host was driving it, so
            // every consumer of `play_time_seconds` - the save screen's
            // play-time column, the seru-trade gate, the dev Records page -
            // read a value that only ever changed when a save was loaded.
            // Whole seconds only, and by delta rather than absolutely, so a
            // loaded save keeps its accumulated total.
            self.tick_play_clock();
            // Opt-in developer menu (`LEGAIA_DEV_MENU=1`): retail reaches its
            // dev tools from debug branches a player cannot; this is the
            // engine's equivalent entry point.
            self.tick_dev_menu();
            // Dance minigame auto-end: `tick_dance` restores the scene
            // mode when the song timer runs out but leaves the game
            // installed for one frame. Detect that (mode no longer
            // Dance while a game is still present), log the final grade,
            // and clear it.
            if self.session.host.world.mode != SceneMode::Dance
                && let Some(g) = self.session.host.world.exit_dance()
            {
                log::info!(
                    "dance: song finished - score {} (pass={})",
                    g.score(),
                    g.passed()
                );
                self.session.restore_field_bgm();
            }
            // A field-VM shop op (`0x49` sub-0 inline shop record) opened
            // a priced gold shop this tick: hand the player into its buy
            // list. The field VM is suspended (op-0x49 Armed) until the
            // player leaves, at which point `finish_field_shop` (below)
            // lets it resume past the merchant op.
            if let Some(shop) = self.session.host.world.take_pending_field_shop() {
                // Open the top-level Buy / Sell / Trade picker (Trade row
                // present only when the disc enabled seru trading). Names
                // for the trade rows come from the boot SCUS.
                if self.session.host.world.seru_trade_enabled() {
                    self.ensure_seru_names();
                }
                self.menu_runtime.open_shop_menu(shop);
            }
            // Production cast-band trigger: a player Seru-magic cast
            // (spell id 0x81..=0x8b) requests a summon spawn. The
            // faithful render is the namesake battle_data creature drawn
            // through the enemy animation pipeline (the summon reuses
            // that creature's mesh + per-object TRS animation), so spawn
            // it as a battle creature rather than the move-VM scene-graph
            // stand-in (`summon::summon_creature_id`).
            if let Some((spell_id, _origin)) = self.session.host.world.take_pending_summon_spawn() {
                self.spawn_summon_creature(spell_id);
            }
            // Production move-FX trigger: a non-summon spell cast or
            // enemy special whose move-power record carries a spawnable
            // effect list requests its `0x801f6324` scene-graph spawn at
            // the target's battle position. Seat it through the same
            // move-VM path the `H` debug key and field FX use.
            if let Some((move_id, origin)) = self.session.host.world.take_pending_move_fx_spawn()
                && self.session.host.world.spawn_move_fx(move_id, origin)
            {
                // Route the move's sound cue through the retail dispatch
                // decode (`classify_cue` = FUN_8004FCC8) and PLAY it: a
                // Ring cue's `ring_value` is the SfxBank descriptor id
                // (docs/formats/sfx-table.md), enqueued into the same
                // per-frame SFX scheduler the art-strike cues ride
                // (`AudioBgmDirector::enqueue_sfx` -> `tick_sfx_frame`,
                // which resolves the cue's own `+4` category bank).
                // Voice cues (`id >= 0x100`) are streamed XA triggers
                // with no engine lane yet - logged, not dropped silently.
                if let Some(cue) = self.session.host.world.take_pending_move_fx_cue() {
                    match legaia_engine_audio::classify_cue(cue as u32) {
                        legaia_engine_audio::CueDispatch::Ring { ring_value, .. } => {
                            if let Some(bgm) = self.session.bgm.as_mut() {
                                bgm.enqueue_sfx(ring_value, 0, 0, 0);
                                log::debug!(
                                    "battle move-FX cue {cue:#04x} enqueued as SFX {ring_value:#04x}"
                                );
                            } else {
                                log::debug!(
                                    "battle move-FX cue {cue:#04x} -> SFX {ring_value:#04x} (no audio)"
                                );
                            }
                        }
                        dispatch @ legaia_engine_audio::CueDispatch::Voice { .. } => {
                            log::debug!(
                                "battle move-FX cue {cue:#04x} -> {dispatch:?} (voice lane unmodeled)"
                            );
                        }
                    }
                }
            }
            // Advance an active Seru-magic summon scene-graph (the cast
            // above, or the `G` debug spawn) through the move VM.
            self.session.host.world.tick_summon(0x0400);
            // Advance an active battle move-FX scene-graph (the `H` debug
            // spawn) through the same move VM.
            self.session.host.world.tick_move_fx(0x0400);
            // Advance any field move-VM scene-graph effects spawned by the
            // field-VM op 0x34 sub-3 ("Play 3D animation") - the per-scene
            // prescript stagers (`FUN_800252EC` → `FUN_80021DF4`). No-op
            // when none are live (off the field / no trigger fired).
            self.session.host.world.tick_field_fx(0x0400);
            // In battle, re-stamp the party's eye/mouth face frames
            // from the playing clips' facial tracks (the retail
            // per-frame facial animator). The clips themselves are
            // advanced by `World::tick`'s Battle arm, which every host
            // reaches - ticking them again here would run them at 2x.
            if self.session.host.world.mode == SceneMode::Battle {
                self.tick_battle_face_stamps();
                self.tick_battle_status_clut();
            }
            // World-map ocean shimmer: cycle the 13-frame CLUT animation
            // (self-gates to None off the world map).
            self.advance_ocean_animation();
            // Scripted CLUT-cell effects (field-VM 4C 61 one-shots +
            // cross-fades): drain the world's banked game ticks against the
            // CPU VRAM (self-gates when none are live).
            self.apply_world_clut_fx();
            // Catch any path that re-uploaded VRAM over the battle
            // texture this frame (and restore it).
            self.check_battle_vram_residency();
            if self.menu_runtime.is_open() {
                let p = self.pad;
                let input = MenuInput {
                    cross: p & 0x4000 != 0,
                    circle: p & 0x2000 != 0,
                    triangle: p & 0x1000 != 0,
                    square: p & 0x8000 != 0,
                    up: p & 0x0010 != 0,
                    down: p & 0x0040 != 0,
                    left: p & 0x0080 != 0,
                    right: p & 0x0020 != 0,
                };
                self.menu_runtime.tick(&mut self.session.host.world, input);
            }
            // A field-VM-triggered shop the player has now closed: tell
            // the world so the suspended op-0x49 resumes (Armed -> Done)
            // and the field VM advances past the merchant op next tick.
            if self.session.host.world.field_shop_open && !self.menu_runtime.is_open() {
                self.session.host.world.finish_field_shop();
            }
            self.prev_pad = self.pad;
            // Record-mode: advance the log's frame counter so
            // `meta.frames` reflects the recorded duration even
            // when the user closes mid-run with no pad transitions.
            if let Some(log) = self.record_log.as_mut() {
                log.observe_frame(self.session.frames);
            }
            // Drain whatever battle events the SM fired this tick,
            // fold their gameplay-state side into the world (HP /
            // status), and ring them into the HUD log.
            self.drain_and_log_battle_events();
            // Route field events: ActorSpawned events whose actor
            // carries a `tmd_ref` queue a render-pass mesh upload
            // so spawn-record actors appear in the scene.
            self.drain_and_route_field_events();
            // Mirror the world's dialog request into a rendered,
            // typed-out panel (opened from the scene MES, dropped when
            // the world dismisses the box).
            self.sync_dialog_panel();
        }
        legaia_engine_render::profile::mark("tick");
        // A tick this frame may have flipped the world into
        // SceneMode::Cutscene (field-VM FMV-trigger op). Start
        // windowed STR playback if so; a cut/missing slot drains the
        // trigger as a no-op (mirrors the headless `play` loop).
        if self.cutscene.is_none() {
            self.try_start_windowed_cutscene();
        }
        // While a cutscene plays, the window shows the video and the
        // scene render is skipped entirely.
        if self.cutscene.is_some() {
            self.render_windowed_cutscene();
            self.win.request_redraw();
            return;
        }
        // On a Field<->Battle transition, upload/drop monster meshes
        // and swap the VRAM. Must run before the render borrows
        // `uploaded_vram` below (this method may re-upload it).
        self.sync_battle_render();
        // Step the phase-scripted battle camera (dialogue close-up / far
        // menu framing + idle orbit / submenu close-up, with the measured
        // glides between). After `sync_battle_render` so battle entry sees
        // `battle_stage_mesh`; before the render borrow reads the pose.
        self.tick_battle_camera();
        // Ease the in-engine cutscene camera between Camera Configure
        // beats. Done here (outside the renderer borrow below) so the
        // interpolator can take `&mut self`; while no cutscene timeline
        // owns the scene the interp is reset so the next opening shot
        // snaps in rather than sweeping from a stale pose.
        // The cutscene camera also owns the WORLD-MAP frame while the opening
        // chain's map01 leg runs its timeline: retail's Rim Elm fly-in is
        // three op-0x45 beats in map01's opening record (snap to the high
        // aerial shot, then the `45 0B .. apply 900` ease-out descent), driven
        // through the same camera globals as the field cutscenes. Gate on a
        // staged param so a world-map beat record WITHOUT camera beats (the
        // Drake mist-wall force-walk bands) keeps the ordinary walk camera.
        let cutscene_cam = if self.session.host.world.cutscene_timeline_active()
            && (self.session.host.world.mode != SceneMode::WorldMap
                || !self.session.host.world.camera_state.params.is_empty())
        {
            let (focus, pitch, yaw, roll, h, tr_eye) = self.cutscene_view();
            // Glide pacing from the op-`0x45` `apply_trigger` (retail
            // `FUN_801DE084` → `FUN_801DB510`): a Configure with `apply == 0`
            // commits its camera targets IMMEDIATELY (snap cut), while
            // `apply > 0` stages them and the per-frame mover glides the live
            // globals there over exactly `apply` frames - the mover law
            // (curve per mode nibble, 1 apply unit = 1 sim tick) is
            // capture-pinned; see `CutsceneCameraInterp`. opdeene's beats mix
            // both: the entry shot snaps (`apply 0`), the mid-prologue grove
            // drift glides (`apply 840`, paired with a 760-frame WaitFrames),
            // and the crater-rim tableau dolly glides (`apply 480`) WHILE the
            // narration text scrolls - the "3D keeps playing under the
            // crawl" retail behaviour. The interp arms glides PER COMPONENT
            // on target change (see `CutsceneCameraInterp::glide`), so the
            // H-only re-poke one frame after the tableau beat cannot snap
            // the in-flight dolly (the earlier whole-tuple ease-rate model
            // did exactly that, tele-porting the eye into the crater-rim
            // geometry - the "opening shot buried in a gold wall" report).
            //
            // Advanced in RETAIL DISPLAY-FRAME time, not render-frame or
            // sim-tick time. Retail's mover (`FUN_801DC0BC`) accumulates the
            // frame-skip factor `DAT_1F800393` into its progress once per
            // logic tick, which credits exactly one unit per display frame -
            // so `apply` is a duration in display frames and the glide must
            // span that many of them. The sim clock runs at 100 Hz, so
            // stepping per sim tick ran every glide 1.67x fast; diff the
            // world's retail-frame counter instead. Min 1 step so an idle
            // frame still refreshes the held pose.
            // Snap beats drained this frame commit first (retail order: the
            // mover snaps to an `apply 0` beat, then glides from there when
            // a same-tick follow-up beat re-stages - the map01 fly-in pair).
            self.replay_camera_snap_beats();
            let apply = self.session.host.world.camera_state.apply_trigger;
            let mode = self.session.host.world.camera_state.mode;
            let now = self.session.host.world.field_frames;
            let steps = u32::try_from(now.saturating_sub(self.cutscene_cam_frames))
                .unwrap_or(u32::MAX)
                .max(1);
            self.cutscene_cam_frames = now;
            let out = self.cutscene_cam_interp.glide(
                focus,
                pitch,
                yaw,
                roll,
                h,
                tr_eye,
                u32::from(apply),
                mode,
                steps,
            );
            if std::env::var_os("LEGAIA_DIAG_CUTCAM").is_some() {
                let w = &self.session.host.world;
                eprintln!(
                    "DIAG cutcam: frame {} apply {} target focus={focus:?} pitch={pitch:.3} \
                     yaw={yaw:.3} roll={roll:.3} h={h} tr_eye={tr_eye:?} | eased focus={:?} \
                     pitch={:.3} yaw={:.3} roll={:.3} h={} tr_eye={:?} | params={:?}",
                    w.frame, apply, out.0, out.1, out.2, out.3, out.4, out.5, w.camera_state.params
                );
            }
            Some(out)
        } else {
            self.cutscene_cam_interp.reset();
            self.pending_camera_snaps.clear();
            None
        };
        // VDF vertex morphs (jou's flesh-ground pulse, rikuroa's generator
        // sacs): rebuild the pack meshes whose morph deltas moved this frame
        // (collected outside the renderer borrow; uploaded inside it below).
        let field_morph_rebuilds = self.take_field_morph_rebuilds();
        // Field-to-battle intro: advance the transition emitter and take both
        // it and its screen-space primitives out of `self`, before the
        // renderer borrow below - the same borrow-window pattern as the morph
        // rebuilds above. Both are empty whenever no transition is running,
        // and the emitter is put back after the render. See `window::battle`.
        let (mut battle_intro, battle_intro_prims) = self.take_battle_intro_frame();
        if let (Some(r), Some(vram), Some(atlas)) = (
            self.win.renderer.as_ref(),
            self.uploaded_vram.as_ref(),
            self.font_atlas.as_ref(),
        ) {
            let (w, h) = r.surface_size();
            let aspect = w as f32 / h.max(1) as f32;
            // Upload (or drop) the opdeene "It was the Seru." caption sprite
            // atlas to track World state. The caption image is present only
            // while opdeene is loaded and never changes, so upload it once on
            // first sight and drop it when the scene clears it (scene change).
            // Disjoint fields: `r` borrows `win.renderer`, the image lives under
            // `session`, the cache is `caption_atlas`.
            if self.caption_atlas.is_none()
                && let Some(cap) = self.session.host.world.cutscene_caption.as_ref()
            {
                match r.upload_sprite_atlas(&cap.rgba, cap.width, cap.height) {
                    Ok(atlas) => self.caption_atlas = Some((atlas, cap.width, cap.height)),
                    Err(e) => log::warn!("caption atlas upload: {e:#}"),
                }
            } else if self.caption_atlas.is_some()
                && self.session.host.world.cutscene_caption.is_none()
            {
                self.caption_atlas = None;
            }
            // Full-scene colour grade: the opening prologue cutscene
            // (`opdeene`, "It was the Seru.") renders its whole 3D scene in
            // warm gold sepia (dim ambient + gold far-colour depth cue in
            // retail); every other scene, incl. the Rim Elm hand-off, is
            // natural colour. Staged every frame so it clears on transition.
            //
            // The scripted screen-fade tint (op `0x4C 0x12` global tint -
            // the scene-entry fade-from-black) composes into the
            // same multiply: with grade strength `s` and gold `G`, the shaded
            // pixel is `rgb*((1-s) + G*s)`, so a full-strength grade of
            // `gold' = tint*((1-s) + G*s)` is exactly `tint * graded`. The
            // depth-cue far colour is tinted the same way below, so both
            // branches of the shader's cue mix carry the tint and the product
            // distributes to the final pixel. `None` tint stages the previous
            // identity values - the byte-identical untouched path. The text /
            // narration overlay is a separate shader and stays bright, which
            // is retail's look (the creation crawl scrolls over the fades).
            let tint = self.session.host.world.scene_screen_tint();
            match (self.session.host.world.scene_color_grade(), tint) {
                // Prologue grade: staged as the renderer's PALETTE-COLLAPSE
                // mode - the retail mechanism's true altitude (the scene's
                // uploaded CLUTs + TMD colour words are rewritten to the gold
                // law at load; the engine's shaders apply the identical law
                // per texel / packet colour). `set_color_grade` carries the
                // gold coefficients for the packet collapse; the screen tint
                // rides the palette slot so the ground's neutral modulation
                // still fades. The view-depth cue ramp is inert in this mode
                // (retail's prologue nodes hold `IR0 = 0`).
                (Some(g), t) => {
                    r.set_color_grade(g.gold, g.strength);
                    r.set_palette_grade(t.unwrap_or([1.0; 3]), true);
                }
                (None, Some(t)) => {
                    r.set_color_grade(t, 1.0);
                    r.set_palette_grade([1.0; 3], false);
                }
                (None, None) => {
                    r.set_color_grade([1.0, 1.0, 1.0], 0.0);
                    r.set_palette_grade([1.0; 3], false);
                }
            }
            // The grade's second half: the per-render-node DPCS far-colour
            // pull (gold far colour + depth-graded IR0 in retail), staged as
            // a view-depth IR0 ramp. Cleared every non-prologue frame, so
            // interactive scenes render the identity (ramp-off) path. The
            // screen-fade tint multiplies the far colour too (see above), so
            // a fade-to-black reaches full black on far-cued geometry.
            match self.session.host.world.scene_depth_cue() {
                Some(c) => {
                    let far = match tint {
                        Some(t) => [c.far[0] * t[0], c.far[1] * t[1], c.far[2] * t[2]],
                        None => c.far,
                    };
                    r.set_depth_cue_ramp(far, c.near_z, c.far_z, c.max_ir0)
                }
                None => r.clear_depth_cue_ramp(),
            }
            // Retail GTE NCLIP winding rejection, scoped to the in-engine
            // cutscene camera: the opdeene prologue's crater-rim tableau shot
            // sits INSIDE the scene's closed cave-wall backdrop mesh, and
            // retail's per-prim NCLIP is what discards the shell's near wall
            // (otherwise it renders over the whole tableau - the "wall of
            // gold burying the camera" report). The field frame draws raw
            // retail vertices under a camera-side Y-flip, which mirrors the
            // projected winding, so retail's front faces arrive CW - mode 2
            // (discard front-facing = discard CCW under the pipelines'
            // default Ccw front-face) keeps them. Off outside the cutscene
            // camera (free-roam field / battle / world map keep both-sided
            // draws; their per-pass winding parities differ). The world-map
            // fly-in leg keeps both-sided draws too - the continent terrain's
            // winding parity is the world-map pass's, not the field pass's,
            // so the field-tuned cull would eat the ground tiles.
            let in_world_map_now = self.session.host.world.mode == SceneMode::WorldMap;
            let nclip_mode = u32::from(cutscene_cam.is_some() && !in_world_map_now) * 2;
            r.set_backface_cull(nclip_mode);
            if std::env::var_os("LEGAIA_DIAG_NOSEMI").is_some() {
                r.set_semi_blend(false);
            }
            // World-map mode frames the loaded map with the
            // controller-driven camera (azimuth / zoom / pan); an active
            // in-engine cutscene (opdeene opening prologue) frames the
            // cutscene's executed op-0x45 camera target; every other mode
            // uses the orbit camera.
            let in_world_map = self.session.host.world.mode == SceneMode::WorldMap;
            let cam = self.compute_scene_camera(aspect, in_world_map, cutscene_cam);
            // Stage the derived scene point lights (the dynamic-lighting
            // enhancement's candle / wall-light layer) with this frame's
            // camera so the renderer can recover world space from the
            // per-draw MVPs. Field free-roam only - battle / world map /
            // boot UI clear them so the layer never lights the wrong
            // coordinate space. Inert (zero staged count, no shadow pass)
            // while dynamic lighting or the shadow sub-toggle is off.
            if !self.boot_ui.is_active()
                && !in_world_map
                && self.session.host.world.mode == SceneMode::Field
                && !self.scene_point_lights.is_empty()
            {
                // Per-frame selection: a scene can carry dozens of candle
                // props but only 8 lights shade at once, so pick the ones
                // nearest the player (falling back to the origin when no
                // player actor is seated).
                let w = &self.session.host.world;
                let focus = w
                    .player_actor_slot
                    .and_then(|s| w.actors.get(s as usize))
                    .map(|a| {
                        [
                            a.move_state.world_x as f32,
                            a.move_state.world_y as f32,
                            a.move_state.world_z as f32,
                        ]
                    })
                    .unwrap_or([0.0; 3]);
                let picked = legaia_engine_render::scene_lights::nearest_lights(
                    &self.scene_point_lights,
                    focus,
                );
                r.set_scene_lights(&picked, cam);
            } else {
                r.clear_scene_lights();
            }
            // Camera-occlusion fade (the see-through-walls enhancement),
            // two per-frame halves:
            //
            // 1. The **visibility gate**: ray-cast a 5-point eye->player
            //    cross against the static scene triangles
            //    (`field_occluders`) and arm the fade ONLY when every
            //    sample is blocked - a partially visible character gets
            //    no fade at all (geometry merely near the corridor, e.g.
            //    an upper-tier floor beside a pit, must not dither while
            //    the player is plainly on screen). The eye is the follow
            //    camera's analytic position (`field_follow_camera_eye`),
            //    so the gate runs in field free-roam under the follow
            //    camera only - cutscene framing is authored, the debug
            //    orbit is a dev vantage, and battle / world map / boot UI
            //    frame their own subjects.
            // 2. The **strength ramp**: ease toward the gate verdict a
            //    quarter of the gap per frame (`OCCL_STRENGTH_EASE`,
            //    mirrored by the browser play page's ramp) so the
            //    screen-door dissolves in/out instead of popping while
            //    the gate flips at cover edges.
            //
            // The staged focus is the player's body centre: the floor
            // tier under the actor (the same sampler the follow camera
            // anchors to) lifted half a character height (~130-unit mesh;
            // field world is retail Y-down, so up is negative).
            const OCCL_STRENGTH_EASE: f32 = 0.25;
            let occl_focus = (self.occlusion_fade
                && !self.boot_ui.is_active()
                && !in_world_map
                && self.session.host.world.mode == SceneMode::Field
                && cutscene_cam.is_none()
                && !self.field_debug_camera)
                .then(|| {
                    let w = &self.session.host.world;
                    w.player_actor_slot
                        .and_then(|s| w.actors.get(s as usize))
                        .map(|a| (a.move_state.world_x, a.move_state.world_z))
                })
                .flatten();
            let mut occl_staged = false;
            if let Some((wx, wz)) = occl_focus {
                const HALF_CHAR_HEIGHT: f32 = 65.0;
                let floor_y = self
                    .session
                    .host
                    .world
                    .sample_field_floor_height(wx as i32, wz as i32);
                let centre = [wx as f32, floor_y as f32 - HALF_CHAR_HEIGHT, wz as f32];
                let fully_hidden = self
                    .field_follow_camera_eye()
                    .map(|eye| {
                        if std::env::var_os("LEGAIA_OCCL_DEBUG").is_some() {
                            let hits = self.field_occluders.sample_hits(eye.to_array(), centre);
                            // A correct eye is the centre of projection: its
                            // clip w under the very camera matrix the draws
                            // use must be ~0.
                            let eye_w = (cam * Vec4::new(eye.x, eye.y, eye.z, 1.0)).w;
                            log::info!(
                                "occl-gate: hits {:?} eye {:?} (clip w {:.2}) centre {:?}",
                                hits,
                                eye.to_array(),
                                eye_w,
                                centre
                            );
                            if hits.iter().all(|h| *h)
                                && let Some((tri, res_tmd)) =
                                    self.field_occluders.first_hit(eye.to_array(), centre)
                            {
                                log::info!(
                                    "occl-gate: centre blocked by tri {tri:?} res_tmd {res_tmd}"
                                );
                            }
                        }
                        self.field_occluders.fully_occluded(eye.to_array(), centre)
                    })
                    .unwrap_or(false);
                let target = if fully_hidden { 1.0 } else { 0.0 };
                let mut s = self.occl_fade_strength.get();
                s += (target - s) * OCCL_STRENGTH_EASE;
                if (s - target).abs() < 0.01 {
                    s = target;
                }
                self.occl_fade_strength.set(s);
                if s > 0.01 {
                    let clip = cam * Vec4::new(centre[0], centre[1], centre[2], 1.0);
                    if std::env::var_os("LEGAIA_OCCL_DEBUG").is_some() {
                        log::info!("occl-gate: staging focus clip {:?} strength {s:.2}", clip);
                    }
                    // The fade circle is authored in world units, so the
                    // renderer needs this camera's vertical projection
                    // scale to size it in pixels at the focus depth.
                    let scale_y = legaia_engine_render::occlusion_fade::view_proj_scale_y(
                        &cam.to_cols_array(),
                    );
                    r.set_occlusion_focus(clip.to_array(), s, scale_y);
                    occl_staged = true;
                }
            } else {
                self.occl_fade_strength.set(0.0);
            }
            if !occl_staged {
                r.clear_occlusion_focus();
            }
            // Drain queued spawn slots: build a VRAM mesh from each
            // actor's `tmd_ref` (global-pool TMD that the field-VM
            // 0x4C 0xD8 host hook installed) and append it to
            // `self.meshes` / `self.scene_tmd_data`, then bind
            // `actor.tmd_binding` to the new mesh index so the
            // draws iteration below picks it up. Idempotent: if
            // the actor already has a binding (e.g. an earlier
            // pass already uploaded), the spawn is skipped.
            // Tile-board tile actors: the board install spawns them through
            // `World::spawn_field_actor` directly (no `ActorSpawned` event),
            // so no drain entry ever queues their template meshes. Scan the
            // board draw list and queue each resolved-template slot once per
            // install; an earlier board in the same scene may have left the
            // slot in `drained_spawn_slots` with its binding since cleared by
            // the despawn, so drop it from the drained set to let the drain
            // below re-upload. Cleared on teardown (empty draw list) so a
            // later board's re-used slots re-queue.
            {
                let world = &self.session.host.world;
                if world.tile_board_draw_list.is_empty() {
                    self.tile_slots_queued.clear();
                } else {
                    for slot in
                        legaia_engine_shell::tile_board_draws::tile_actor_slots_needing_mesh(world)
                    {
                        if self.tile_slots_queued.insert(slot) {
                            self.drained_spawn_slots.remove(&slot);
                            self.pending_dynamic_mesh_slots.push(slot);
                        }
                    }
                }
            }
            let pending = std::mem::take(&mut self.pending_dynamic_mesh_slots);
            for slot in pending {
                let actor = match self.session.host.world.actors.get(slot as usize) {
                    Some(a) => a,
                    None => continue,
                };
                // Idempotence is tracked per-slot, NOT by "already has
                // a binding": `upload_assets` naively pre-binds every
                // actor K -> scene TMD slot K, and the player's spawn
                // (its `tmd_ref` = the real character mesh from the
                // global pool) must override that placeholder or the
                // player renders as whatever scene mesh happened to
                // share its slot index (usually invisible).
                if self.drained_spawn_slots.contains(&slot) {
                    continue;
                }
                let Some(gtmd) = actor.tmd_ref.as_ref().map(std::sync::Arc::clone) else {
                    continue;
                };
                let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(&gtmd.tmd, &gtmd.raw);
                if vmesh.indices.is_empty() {
                    log::warn!("play-window: spawn slot {slot} has TMD with 0 indices; skipping");
                    continue;
                }
                match r.upload_vram_mesh(
                    &vmesh.positions,
                    &vmesh.uvs,
                    &vmesh.cba_tsb,
                    &vmesh.normals,
                    &vmesh.colors,
                    &vmesh.indices,
                ) {
                    Ok(m) => {
                        let new_idx = self.meshes.len();
                        self.meshes.push(m);
                        self.scene_tmd_data
                            .push((gtmd.tmd.clone(), gtmd.raw.clone()));
                        self.session.host.world.actors[slot as usize].tmd_binding = Some(new_idx);
                        self.drained_spawn_slots.insert(slot);
                        log::info!("play-window: spawn slot {slot} -> mesh slot {new_idx}");
                    }
                    Err(e) => log::warn!("spawn mesh upload: {e:#}"),
                }
            }
            // For each active actor with a tmd_binding and a current
            // pose_frame, regenerate and re-upload the posed mesh.
            // posed_overrides[i] replaces meshes[i] when present.
            let (posed_overrides, player_color_posed) = self.build_posed_actor_overrides(r);
            // Arts after-image ghosts: the actor's mesh at rate-scheduled
            // historical poses, flat-coloured additive (retail FUN_80049348;
            // kernel `engine-core::battle_afterimage`). Empty outside a
            // battle / outside a SpecialStarter dash.
            let battle_ghost_uploads = self.build_battle_ghost_uploads(r);
            legaia_engine_render::profile::mark("pose:actor");
            // Placed props posed at their live clip frame: the ones resting on
            // frame 0 keep the baked rest mesh, the ones mid-swing get rebuilt.
            let (posed_prop_baked_v, posed_prop_baked_c, posed_prop_live_v, posed_prop_live_c) =
                self.posed_prop_frame_draws(r);
            legaia_engine_render::profile::mark("pose:prop");
            // VDF vertex morphs: upload this frame's rebuilt morph meshes;
            // the draw loops below substitute them for the static uploads
            // (`field_morph_live` - the `FUN_8001C604` render substitution).
            for (mesh_idx, vmesh) in &field_morph_rebuilds {
                if let Ok(m) = r.upload_vram_mesh(
                    &vmesh.positions,
                    &vmesh.uvs,
                    &vmesh.cba_tsb,
                    &vmesh.normals,
                    &vmesh.colors,
                    &vmesh.indices,
                ) {
                    self.field_morph_live.insert(*mesh_idx, m);
                }
            }

            // Field-NPC clip playback: advance each placed NPC's looping ANM
            // clip and draw its posed mesh halves.
            //
            // The playhead advances in SIM-TICK time, not render-frame time:
            // each redraw shows the clip's current frame and then advances the
            // playhead by `run_ticks` (the number of 60 Hz sim ticks this
            // redraw drained). Ticking once per *redraw* (what this did
            // before) tied the animation rate to the display's refresh rate -
            // on a 144 Hz monitor every NPC animated 2.4x too fast, and any
            // screenshot oracle over an animated field scene was only
            // reproducible while the engine held exactly 60 fps. At a steady
            // 60 Hz (1 tick per redraw) the emitted frame sequence is
            // unchanged. The player and prop clips already advance inside
            // `World::tick`; this brings the NPC clips onto the same clock.
            //
            // The skinned mesh for a `(slot, clip frame)` is a **constant** -
            // the clip is a short loop over a fixed pose set - so it is skinned
            // and uploaded on the first visit to that frame and memoised in
            // `npc_pose_cache` thereafter. Rebuilding it every render frame
            // re-derived the same vertex bytes and allocated fresh GPU buffers
            // for them, which dominated the frame: the CPU re-pose plus its
            // upload was ~70% of the field frame in a populated town.
            //
            // The rest-pose meshes in `field_npc_draws` stay as the fallback
            // for NPCs whose clip or upload is unavailable.
            //
            // `npc_frames` records which frame each slot is showing *this*
            // render, so the draw pass below can look its mesh up in the cache.
            let mut npc_frames: Vec<(u8, usize)> = Vec::new();
            if self.session.host.world.mode == SceneMode::Field {
                // Channel op-0x4B ANIMATE cues re-target the NPC's clip
                // player before this frame's tick: the cue's anim id names
                // a bundle record the same way the placement anim byte does
                // (`record = id - 1`), against whichever bundle the
                // placement originally resolved through. This is what makes
                // the prologue-vignette actors *perform* their scripted
                // beats instead of looping the placement clip.
                // Timeline `A2 F8 <move_id>` ExecMove cues against the
                // PLAYER: resolve scene-ANM-bundle record `move_id - 1`
                // (the same `id - 1` record space as the NPC cues; pinned
                // live - town01's post-naming ExecMove 48/49 land the
                // retail player anim pointer on scene records 47/48) and
                // queue it as a one-shot over the idle/walk pair. Cues
                // whose record doesn't resolve (e.g. the low walk-move
                // ids the locomotion controller already covers) drop out
                // harmlessly.
                let move_cues = std::mem::take(&mut self.session.host.world.field_player_move_cues);
                if !move_cues.is_empty()
                    && let Some(bundle) = self.npc_anim_bundles.0.as_ref()
                {
                    for id in move_cues {
                        // Moves 1/2 are the locomotion walk moves - the
                        // live trace keeps the retail anim pointer on the
                        // PROT 0874 walk/idle clips across them, and the
                        // engine's movement controller already animates
                        // those - so only higher ids re-target the clip.
                        if id <= 2 {
                            continue;
                        }
                        if let Some(clip) =
                            legaia_engine_core::field_anim::FieldClipPlayer::from_record(
                                bundle,
                                id as usize - 1,
                            )
                            && let Some(anim) = self.session.host.world.field_player_anim.as_mut()
                        {
                            anim.push_scripted(clip);
                        }
                    }
                }
                let cues: Vec<_> = self
                    .session
                    .host
                    .world
                    .field_npc_anim_cues
                    .drain()
                    .collect();
                for (slot, (_count, base_id, _frames)) in cues {
                    if !self.npc_anim_srcs.contains_key(&slot) {
                        continue;
                    }
                    let special = self.npc_bundle_special.get(&slot).copied().unwrap_or(false);
                    let bundle = if special {
                        self.npc_anim_bundles.1.as_ref()
                    } else {
                        self.npc_anim_bundles.0.as_ref()
                    };
                    let (Some(b), Some(rec_idx)) = (bundle, (base_id as usize).checked_sub(1))
                    else {
                        continue;
                    };
                    if let Some(player) =
                        legaia_engine_core::field_anim::FieldClipPlayer::from_record(b, rec_idx)
                    {
                        self.npc_clip_players.insert(slot, player);
                        // The incoming clip restarts at frame 0 and reuses the
                        // same low frame indices, so the outgoing clip's memo
                        // entries for this slot would alias it. Drop them.
                        self.npc_pose_cache.retain(|(s, _), _| *s != slot);
                        self.npc_pose_verify.retain(|(s, _), _| *s != slot);
                    }
                }
                let verify = std::env::var_os("LEGAIA_POSE_CACHE_VERIFY").is_some();
                let cache = &mut self.npc_pose_cache;
                let verify_poses = &mut self.npc_pose_verify;
                let srcs = &self.npc_anim_srcs;
                for (slot, player) in self.npc_clip_players.iter_mut() {
                    let Some((tmd, raw)) = srcs.get(slot) else {
                        continue;
                    };
                    // `frame()` is the frame this redraw shows; take it as the
                    // cache key and read its pose WITHOUT moving the playhead,
                    // then advance by the sim ticks this redraw ran (0 on a
                    // pure-refresh frame, so a 144 Hz display holds each frame
                    // for the same wall-clock time a 60 Hz one does).
                    let key = (*slot, player.frame());
                    let pose = player.current_pose();
                    player.advance(run_ticks);
                    npc_frames.push(key);
                    if cache.contains_key(&key) {
                        // `LEGAIA_POSE_CACHE_VERIFY=1`: the pose behind a hit
                        // must be the pose the entry was built from, or the key
                        // is aliasing and the NPC would draw someone else's
                        // frame.
                        if verify
                            && let Some(want) = verify_poses.get(&key)
                            && *want != pose.bone_outputs
                        {
                            log::error!(
                                "pose-cache MISMATCH at slot {} frame {}: cached pose != live pose",
                                key.0,
                                key.1
                            );
                        }
                        continue;
                    }
                    if verify {
                        verify_poses.insert(key, pose.bone_outputs.clone());
                    }
                    let vmesh =
                        legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(tmd, raw, &pose.bone_outputs);
                    let cmesh =
                        legaia_tmd::mesh::tmd_to_color_mesh_posed_rot(tmd, raw, &pose.bone_outputs);
                    let vm = if vmesh.indices.is_empty() {
                        None
                    } else {
                        r.upload_vram_mesh(
                            &vmesh.positions,
                            &vmesh.uvs,
                            &vmesh.cba_tsb,
                            &vmesh.normals,
                            &vmesh.colors,
                            &vmesh.indices,
                        )
                        .ok()
                    };
                    let cm = if cmesh.is_empty() {
                        None
                    } else {
                        r.upload_color_mesh_blended(
                            &cmesh.positions,
                            &cmesh.colors,
                            &cmesh.indices,
                            &cmesh.blend,
                        )
                        .ok()
                    };
                    if vm.is_some() || cm.is_some() {
                        cache.insert(key, (vm, cm));
                    }
                }
            }
            // Re-borrow the memo immutably: `slot -> this frame's posed halves`.
            let npc_posed: std::collections::HashMap<u8, &NpcPosedHalves> = npc_frames
                .iter()
                .filter_map(|k| self.npc_pose_cache.get(k).map(|m| (k.0, m)))
                .collect();
            // Everything above this mark is per-frame skinning: CPU mesh
            // re-pose + GPU re-upload for the player, the animated props and
            // every placed NPC.
            legaia_engine_render::profile::mark("pose");
            // Iterate every actor that has a `tmd_binding`. Scene-init
            // actors (slots 0..N from `init_scene_animations`) have
            // their bindings set but aren't necessarily `.active` -
            // the original draws iteration walked meshes directly,
            // so we preserve that behaviour by not gating on
            // `.active` here. Dynamically spawned actors set both
            // `.active` and a binding to their freshly uploaded
            // mesh slot (beyond `scene_tmd_data.len()`) via the
            // spawn pass above.
            //
            // Suppress 3D draws while the boot UI is active so the
            // last-loaded scene (e.g. a town) doesn't show through
            // behind publisher logos / title / save-select. The one
            // exception is the party-wipe hand-off: retail holds the
            // final battle frame while mode 22 CARD INIT streams the
            // menu overlay, so the GameOver hold keeps drawing the
            // (frozen, untick'd) battle scene underneath.
            let game_over_hold = matches!(self.boot_ui, BootUiState::GameOver(_));
            let mut draws: Vec<SceneDraw<'_>> = Vec::new();
            // Untextured (F*/G*) field props, drawn on the colour
            // pipeline alongside the textured `draws`.
            let mut color_draws: Vec<ColorSceneDraw<'_>> = Vec::new();
            if self.boot_ui.is_active() && !game_over_hold {
                // Boot UI is fullscreen - suppress 3D draws.
            } else if in_world_map {
                // World-map continent = two layers, both in the shared
                // player / entity-marker world frame:
                //
                // 1. The **ground** is a heightfield surface
                //    (`ground_heightfield`) built from the walk
                //    `.MAP` floor grid (`Scene::walk_heightfield`,
                //    elevation per `FUN_80019278`). It draws with a
                //    provisional uniform ground texel: per-tile
                //    texturing has no clean source - the record `+0x14`
                //    byte is terrain-type metadata, not an atlas
                //    selector (no draw path reads it; see
                //    docs/subsystems/world-map.md "Open (texturing)").
                // 2. The sparse **placed landmarks** (trees / mountains
                //    / castle) are slot-1 pack meshes positioned per
                //    occupied tile (`world_map_terrain_draws`, the
                //    `flags & 0x4` set resolved via record[+0x10]+prefix).
                //
                // The earlier per-cell pack-mesh sweep that stamped a
                // mesh on every `0x1000` cell was wrong (it flooded the
                // map with pool-5; see docs/subsystems/world-map.md).
                // No Y-flip on the heightfield: its baked `-lut`
                // corner heights are already in the same frame as the
                // landmark placements' un-flipped translation (see the
                // field-branch note below).
                if let Some(hf_mesh) = self.ground_heightfield.as_ref() {
                    draws.push(SceneDraw {
                        mesh: hf_mesh,
                        mvp: cam,
                        cue: None,
                    });
                }
                for (mesh_idx, model) in self.world_map_terrain_draws.iter() {
                    if let Some(mesh) = self.meshes.get(*mesh_idx) {
                        draws.push(SceneDraw {
                            mesh,
                            mvp: cam * *model,
                            cue: None,
                        });
                    }
                }
                // Last-resort fallback: nothing resolved at all -> draw
                // the whole pack at pack-local coords so the map isn't
                // blank.
                if self.ground_heightfield.is_none() && self.world_map_terrain_draws.is_empty() {
                    for mesh in &self.meshes {
                        draws.push(SceneDraw {
                            mesh,
                            mvp: cam,
                            cue: None,
                        });
                    }
                }
            } else {
                let in_battle = self.session.host.world.mode == SceneMode::Battle;
                if in_battle {
                    // Battle backdrop: the scene's `scene_tmd_stream`
                    // dome (PROT 88 for the overworld map01 battle) -
                    // sky hemisphere + mountain arc + grass - drawn at its
                    // **raw world coordinates** under the exact retail
                    // orbit camera (`retail_battle_mvp`). `model = F`
                    // (plain Y-flip): the camera bakes in `F`, so
                    // `cam * F` recovers the raw PSX vertex the retail
                    // transform expects.
                    //
                    // World-fixed, and **one draw but two copies** - do not
                    // read the single `draws.push` below as "one instance".
                    // Retail sets the dome up as a background **actor**
                    // (`FUN_800513F0`: `tmd_register` -> `DAT_8007C018[]`
                    // + `FUN_80020de0` actor_alloc + `FUN_80020f88` link)
                    // rendered by the normal actor path `FUN_80048A08`, and
                    // it renders it **twice**: a second copy under either a
                    // per-stage `Ry(180)` half-turn or a mirror-X selected by
                    // the SCUS table `DAT_80078B50`
                    // (`legaia_asset::battle_backdrop::SecondCopy`). The
                    // stage TMD is a HALF arena, not a full surround
                    // (map01's dome is a front half, verts `Z in [-1260,
                    // +12155]`; town01's arena is authored entirely at
                    // `X >= 0`, open side facing -X - the sea horizon in
                    // the retail Tetsu close-up), which is why the second
                    // copy exists at all.
                    //
                    // The port applies that copy at **mesh build** time, not
                    // here: `battle_stage_meshes` pre-appends it with
                    // `VramMesh::append_scaled` (winding flipped for the
                    // mirror), so the page and the window both upload one
                    // mesh and this loop pushes one draw. An older comment
                    // here said "the mirror draw is removed (one instance,
                    // like retail)" - that described a *withdrawn* draw-time
                    // second instance and was wrong about retail besides.
                    // See `project_battle_backdrop_is_prot88_dome` and
                    // `docs/subsystems/battle.md` § the second copy.
                    //
                    // **Stage scale.** The stage rides the same
                    // [`BATTLE_WORLD_SCALE`] base matrix the actors do
                    // (`0x8007BF10 = 16384*I`, composed per drawn object by
                    // `FUN_80048A08` - and the dome is registered as an
                    // ordinary background *actor*, so it goes through that
                    // same path). Drawing it at raw 1x while the actors ride
                    // 4x put the two classes in different worlds: the phase
                    // camera's translation trio is authored in the scaled
                    // stage space, so against 1x geometry the eye orbited at
                    // four times the intended radius and swung *through* the
                    // arena shell - the frame filling with one magnified
                    // wall - and every actor stood 3x its seat distance away
                    // from the ground cell it was supposed to be on. One
                    // scale for every battle draw class is what makes the
                    // arena a backdrop and the grid a floor.
                    if let Some(stage_idx) = self.battle_stage_mesh
                        && let Some(mesh) = self.meshes.get(stage_idx)
                    {
                        let flip = Self::battle_stage_model();
                        // Half-arena stage in the scaled battle stage space.
                        draws.push(SceneDraw {
                            mesh,
                            mvp: cam * flip,
                            cue: None,
                        });
                    }
                    // ...and the shell's untextured `F*`/`G*` half on the
                    // colour pipeline, at the identical transform. Retail
                    // walks one primitive list, so these panels (sky band,
                    // painted wall faces, flat water) belong to the same
                    // backdrop draw; without them the shell has holes.
                    if let Some(cidx) = self.battle_stage_color_mesh
                        && let Some(cmesh) = self.color_meshes.get(cidx)
                    {
                        color_draws.push(ColorSceneDraw {
                            mesh: cmesh,
                            mvp: cam * Self::battle_stage_model(),
                        });
                    }
                } else {
                    // Debug layer filter (`LEGAIA_DIAG_LAYERS=hf,tiles,
                    // ctiles,place,cplace,npc`): when set, only the
                    // named field layers draw - the render-side sibling
                    // of `LEGAIA_DIAG_PLACE` for bisecting which layer
                    // a visual defect lives in.
                    let layer_filter = std::env::var("LEGAIA_DIAG_LAYERS").ok();
                    let layer_on = |name: &str| {
                        layer_filter
                            .as_deref()
                            .is_none_or(|f| f.split(',').any(|s| s == name))
                    };
                    // Bulk ground FIRST: the `.MAP` floor-grid
                    // heightfield (the `0x1000` ground layer - most
                    // town floor cells have NO pack mesh, so without
                    // this surface they render as holes).
                    //
                    // NO model flip: the heightfield bakes its corner
                    // elevation as `-lut[nib]` - the same retail
                    // Y-down world height the placements/actors put in
                    // their translations - and the field camera's
                    // FIELD_WORLD_FLIP provides the single net Y
                    // negation, so elevated tiers (e.g. the tier -192
                    // cliff-top town core) render ABOVE sea-level
                    // tier-0 cells, matching retail. Pipelines don't
                    // cull, so winding is immaterial.
                    if layer_on("hf")
                        && let Some(hf_mesh) = self.ground_heightfield.as_ref()
                    {
                        draws.push(SceneDraw {
                            mesh: hf_mesh,
                            mvp: cam,
                            cue: None,
                        });
                    }
                    // Then the terrain / decor tile layer (drawn under
                    // the buildings): the `CELL_VISIBLE` field-map tiles
                    // (stone plaza, paths, riverbank).
                    if layer_on("tiles") {
                        for (mesh_idx, model) in &self.field_terrain_draws {
                            let mesh = self
                                .field_morph_live
                                .get(mesh_idx)
                                .or_else(|| self.meshes.get(*mesh_idx));
                            if let Some(mesh) = mesh {
                                draws.push(SceneDraw {
                                    mesh,
                                    mvp: cam * *model,
                                    cue: None,
                                });
                            }
                        }
                    }
                    // Untextured ground tiles (vertex-colour meshes the
                    // textured bridge has no entry for) - without these
                    // the floor shows holes where a tile's mesh carries
                    // no textured prims.
                    if layer_on("ctiles") {
                        for (mesh_idx, model) in &self.field_terrain_color_draws {
                            if let Some(mesh) = self.color_meshes.get(*mesh_idx) {
                                color_draws.push(ColorSceneDraw {
                                    mesh,
                                    mvp: cam * *model,
                                });
                            }
                        }
                    }
                    // Static environment geometry: draw each placed
                    // building / terrain mesh at its world transform
                    // (resolved at scene load in
                    // `resolve_field_placement_draws`).
                    if layer_on("place") {
                        // Diag bisect: `LEGAIA_DIAG_PLACE_RANGE=a..b` draws only
                        // placement-draw slots [a, b).
                        let place_range =
                            std::env::var("LEGAIA_DIAG_PLACE_RANGE").ok().and_then(|s| {
                                let (a, b) = s.split_once("..")?;
                                Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?))
                            });
                        for (di, (mesh_idx, model)) in self.field_placement_draws.iter().enumerate()
                        {
                            if let Some((a, b)) = place_range
                                && !(a..b).contains(&di)
                            {
                                continue;
                            }
                            let mesh = self
                                .field_morph_live
                                .get(mesh_idx)
                                .or_else(|| self.meshes.get(*mesh_idx));
                            if let Some(mesh) = mesh {
                                draws.push(SceneDraw {
                                    mesh,
                                    mvp: cam * *model,
                                    cue: None,
                                });
                            }
                        }
                        // Posed props (house doors, cupboards, the windmill):
                        // the ones resting on frame 0 replay their baked rest
                        // mesh; the ones whose clip is running were re-posed
                        // above, so the door draws mid-swing.
                        for (mesh_idx, model) in &posed_prop_baked_v {
                            if let Some(mesh) = self.meshes.get(*mesh_idx) {
                                draws.push(SceneDraw {
                                    mesh,
                                    mvp: cam * *model,
                                    cue: None,
                                });
                            }
                        }
                        for (mesh, model) in &posed_prop_live_v {
                            draws.push(SceneDraw {
                                mesh,
                                mvp: cam * *model,
                                cue: None,
                            });
                        }
                    }
                    // Untextured props (the F*/G* meshes the VRAM path
                    // drops) on the colour pipeline, same transforms.
                    if layer_on("cplace") {
                        for (mesh_idx, model) in &self.field_placement_color_draws {
                            if let Some(mesh) = self.color_meshes.get(*mesh_idx) {
                                color_draws.push(ColorSceneDraw {
                                    mesh,
                                    mvp: cam * *model,
                                });
                            }
                        }
                        for (mesh_idx, model) in &posed_prop_baked_c {
                            if let Some(mesh) = self.color_meshes.get(*mesh_idx) {
                                color_draws.push(ColorSceneDraw {
                                    mesh,
                                    mvp: cam * *model,
                                });
                            }
                        }
                        for (mesh, model) in &posed_prop_live_c {
                            color_draws.push(ColorSceneDraw {
                                mesh,
                                mvp: cam * *model,
                            });
                        }
                    }
                    // Occlusion-fade draw watermark: every draw pushed so
                    // far is scene ENVIRONMENT (terrain, placements, posed
                    // props, their colour halves) - fadeable. Everything
                    // after this point is an ACTOR (the player's two
                    // halves, NPCs, tile actors, spawned meshes), which the
                    // fade must never dissolve - the depth margin only has
                    // to guard geometry AT the focus depth now, so
                    // occluders hugging the character still open up.
                    r.set_occlusion_env_draws(draws.len(), color_draws.len());
                    // The player's untextured mesh half (pants /
                    // sleeves), following the actor's live transform.
                    // Prefer this frame's posed rebuild (idle/walk
                    // playback); fall back to the static rest pose.
                    if let Some((cidx, slot)) = self.player_color_draw
                        && let Some(mesh) = player_color_posed
                            .as_ref()
                            .or_else(|| self.color_meshes.get(cidx))
                    {
                        color_draws.push(ColorSceneDraw {
                            mesh,
                            mvp: cam * self.actor_model(slot),
                        });
                    }
                    // Field NPCs + animated props at their live
                    // positions (motion-VM walkers update
                    // `field_npc_positions`; everyone else stands at
                    // the spawn tile), floor-snapped like the player.
                    let w = &self.session.host.world;
                    for d in self.field_npc_draws.iter().filter(|_| layer_on("npc")) {
                        let (x, z) = w
                            .field_npc_positions
                            .get(&d.slot)
                            .copied()
                            .unwrap_or(d.spawn);
                        // Story-parked actor (spawn-prologue `MoveTo` to the
                        // off-map hide box, or a cutscene hide): not drawn -
                        // retail parks despawned actors at the far-corner
                        // sentinel tile precisely so they never render.
                        let hide = legaia_engine_core::world::FIELD_OFFMAP_HIDE_XZ;
                        if x == hide && z == hide {
                            continue;
                        }
                        // Zero render scale (`actor[+0x72] = 0`, written by
                        // the trigger records' spawn prologue): retail
                        // collapses the actor's mesh to a point - the
                        // invisible interaction markers (orange diamond +
                        // blue cone dev gizmo). Skip the draw entirely.
                        if w.field_npc_render_scale(d.slot as usize) == Some(0) {
                            continue;
                        }
                        let y = w.sample_field_floor_height(x as i32, z as i32) as f32;
                        // Raw retail-convention transform (no model
                        // flip): the field camera's FIELD_WORLD_FLIP
                        // provides the single net Y negation. Walkers
                        // face their travel heading (12-bit, `0` =
                        // Z+, same convention + half-turn compose as
                        // the player's `render_26`); never-walked
                        // NPCs read their spawn-prologue heading
                        // seeded into `field_npc_headings` (facing-0
                        // / prologue-less records render at
                        // identity).
                        let rot = match w.field_npc_headings.get(&d.slot) {
                            Some(&h) => Mat4::from_rotation_y(
                                std::f32::consts::PI + (h as f32) / 4096.0 * std::f32::consts::TAU,
                            ),
                            None => Mat4::IDENTITY,
                        };
                        let model = Mat4::from_translation(Vec3::new(x as f32, y, z as f32)) * rot;
                        let posed = npc_posed.get(&d.slot);
                        match (posed.and_then(|p| p.0.as_ref()), d.mesh_idx) {
                            (Some(mesh), _) => draws.push(SceneDraw {
                                mesh,
                                mvp: cam * model,
                                cue: None,
                            }),
                            (None, Some(mi)) => {
                                if let Some(mesh) = self.meshes.get(mi) {
                                    draws.push(SceneDraw {
                                        mesh,
                                        mvp: cam * model,
                                        cue: None,
                                    });
                                }
                            }
                            (None, None) => {}
                        }
                        match (posed.and_then(|p| p.1.as_ref()), d.color_idx) {
                            (Some(mesh), _) => color_draws.push(ColorSceneDraw {
                                mesh,
                                mvp: cam * model,
                            }),
                            (None, Some(ci)) => {
                                if let Some(mesh) = self.color_meshes.get(ci) {
                                    color_draws.push(ColorSceneDraw {
                                        mesh,
                                        mvp: cam * model,
                                    });
                                }
                            }
                            (None, None) => {}
                        }
                    }
                    // Tile-board tile actors: one mesh instance per drawable
                    // cell in this frame's deferred draw list (retail
                    // `overlay_0897_801e0f3c` - a cell value's shared actor
                    // draws at EVERY cell holding that value, not just its
                    // own last-repositioned transform). Only slots the spawn
                    // drain above uploaded draw (`drained_spawn_slots`): a
                    // slot still wearing `upload_assets`' naive pre-bind
                    // would render an unrelated scene mesh, and unresolved
                    // templates (no `tmd_ref`) never upload - both degrade
                    // to "no draw".
                    for d in legaia_engine_shell::tile_board_draws::tile_board_actor_draws(w) {
                        if !self.drained_spawn_slots.contains(&d.slot) {
                            continue;
                        }
                        let Some(tmd_idx) =
                            w.actors.get(d.slot as usize).and_then(|a| a.tmd_binding)
                        else {
                            continue;
                        };
                        if let Some(mesh) = self.meshes.get(tmd_idx) {
                            // Raw retail-convention transform, like the NPC
                            // draws: the field camera's FIELD_WORLD_FLIP
                            // provides the single net Y negation.
                            let model = Mat4::from_translation(Vec3::new(
                                d.world[0], d.world[1], d.world[2],
                            ));
                            draws.push(SceneDraw {
                                mesh,
                                mvp: cam * model,
                                cue: None,
                            });
                        }
                    }
                }
                // Actors ride the same battle rotation as the dome +
                // grid but with the retail 4x world-scale base matrix
                // (`0x8007BF10 = 16384*I`) composed under it - the
                // `FUN_80048A08` per-actor camera composition. The
                // uniform scale commutes with the per-model Y-flip, so
                // composing it on the camera side scales both the mesh
                // and the actor's stage translation, exactly like
                // retail. Outside a stage-dome battle the synthetic
                // AABB-framing camera stays unscaled (it frames the
                // raw actor coordinates).
                let actor_cam = if in_battle && self.battle_stage_mesh.is_some() {
                    cam * Mat4::from_scale(Vec3::splat(BATTLE_WORLD_SCALE))
                } else {
                    cam
                };
                // Flat tiled ground grid (retail's func_0x801d02c0 grass)
                // under the actors, on the same battle camera so the
                // party stands on it and the foreground reads as grass
                // instead of the bare clear colour. `cam` bakes in the
                // Y-flip, so `* flip` recovers the raw PSX y=0 plane.
                // The per-draw cue is the emitter's own DPCS depth cue:
                // `IR0 = SZ >> 2` per vertex (unsaturated - `max_ir0`
                // rides past 1.0 exactly like retail's bare `mtc2`),
                // blending toward the battle's staged far colour, so the
                // floor washes out with distance the way retail's does.
                // The grid rides [`BATTLE_WORLD_SCALE`] like every other
                // battle draw class (see the stage-scale note on the
                // backdrop draw above): the party stands ON its own grid
                // cell only if the cell and the actor's stage translation
                // are lifted by the same factor. The DPCS ramp is a
                // VIEW-depth window, so it is lifted with the geometry -
                // otherwise the whole ramp would collapse into the near
                // field and the floor would read as fully fogged.
                if in_battle
                    && let Some(gi) = self.battle_ground_mesh
                    && let Some(gmesh) = self.meshes.get(gi)
                {
                    use legaia_engine_vm::battle_ground_grid as grid;
                    let flip = Self::battle_stage_model();
                    draws.push(SceneDraw {
                        mesh: gmesh,
                        mvp: cam * flip,
                        cue: self
                            .battle_ground_cue_far
                            .map(|far| legaia_engine_render::DrawCue {
                                far,
                                near_z: 0.0,
                                far_z: grid::grid_cue_far_z() * BATTLE_WORLD_SCALE,
                                max_ir0: grid::grid_cue_max_ir0(),
                            }),
                    });
                }
                for (i, actor) in self.session.host.world.actors.iter().enumerate() {
                    let Some(tmd_idx) = actor.tmd_binding else {
                        continue;
                    };
                    // In a stage-dome battle, draw only the ACTIVE battle
                    // actors (party + monsters). The scene-init actors
                    // (bound but inactive, parked at the origin) would
                    // otherwise pile their meshes at world (0,0,0) - the
                    // "duplicate Vahn" + scattered scene geometry.
                    if in_battle && self.battle_stage_mesh.is_some() && !actor.active {
                        continue;
                    }
                    // Board-owned tile actors draw once per cell through the
                    // deferred tile-board pass above; their own transform
                    // only holds the LAST repositioned cell (and a slot the
                    // drain hasn't uploaded still wears the naive pre-bind).
                    // The player (tile table slot 0) stays on this path.
                    if legaia_engine_shell::tile_board_draws::is_tile_actor_slot(
                        &self.session.host.world,
                        i,
                    ) {
                        continue;
                    }
                    // The `opdeene` prologue cutscene is an abstract vignette
                    // sequence (the "It was the Seru" Genesis-tree imagery)
                    // driven by the per-actor field channels, NOT by a
                    // controllable lead. `enter_field_scene` still installs the
                    // free-roam player (slot 0) at the generic field cold-spawn,
                    // so without this it stands in the shot as a stray mesh.
                    // Scene-gated on `opdeene` so `town01`'s opening cutscene -
                    // where the timeline scripts the lead actor (Vahn walking
                    // out of his house) - keeps drawing him.
                    if i == 0
                        && self.session.host.world.active_scene_label
                            == legaia_asset::new_game::OPENING_CUTSCENE_SCENE
                    {
                        continue;
                    }
                    let mesh = posed_overrides
                        .get(tmd_idx)
                        .and_then(|o| o.as_ref())
                        .or_else(|| self.meshes.get(tmd_idx));
                    if let Some(mesh) = mesh {
                        // Target-select cursor: while the command picker
                        // points at an enemy row, the ported FUN_801DA6B4
                        // (`engine-vm::battle_action::target_cursor_highlight`)
                        // stamps three render words across the monster slots -
                        // `render_flag` 5 on the pointed-at monster / 200 on
                        // the rest, the bright/dim colour words, and the q12
                        // `render_blend` (0x1000 = cursor up, 0 = cursor
                        // down). The blend word is the render packet's
                        // `+0x78` tint weight (`FUN_8004A908`), NOT a mesh
                        // scale; the tint rides the per-draw GTE depth-cue
                        // seam (a saturated `DrawCue` ramp = a flat blend
                        // toward the cue colour) - the pointed-at monster
                        // pulses bright, the others dim. The pulse phase is
                        // the host tick.
                        let model = self.actor_model(i);
                        let mut cue = None;
                        if in_battle {
                            use legaia_engine_vm::battle_action as ba;
                            let b = &actor.battle;
                            match b.render_flag {
                                ba::CURSOR_FLAG_SELECTED => {
                                    let pulse = 0.30 + 0.20 * (self.tick_no as f32 * 0.25).sin();
                                    log::trace!(
                                        "target cursor: actor {i} SELECTED pulse {pulse:.2}"
                                    );
                                    cue = Some(legaia_engine_render::DrawCue {
                                        far: [1.0, 1.0, 1.0],
                                        near_z: -1.0,
                                        far_z: 0.0,
                                        max_ir0: pulse,
                                    });
                                }
                                ba::CURSOR_FLAG_DIMMED => {
                                    log::trace!("target cursor: actor {i} DIMMED");
                                    cue = Some(legaia_engine_render::DrawCue {
                                        far: [0.0, 0.0, 0.0],
                                        near_z: -1.0,
                                        far_z: 0.0,
                                        max_ir0: 0.55,
                                    });
                                }
                                _ => {}
                            }
                            // Per-clip impact tint (`FUN_8004CE2C` pass 2
                            // via `engine-vm::battle_impact_fx`; the world
                            // tick owns the writes + decay): while the
                            // actor's +0x21F selector is armed, the packed
                            // +0x04 colour word rides the same saturated
                            // DrawCue seam. The same arm renders the
                            // item/spirit cue-group flash (`FUN_801E22C8`
                            // stamps colour + a `0x2000` blend the tick
                            // drains), weighting the strength by the live
                            // blend so the flash fades out. Overrides the
                            // cursor arms (the impact target is retail's
                            // tint owner) and loses to the hit flash below.
                            {
                                use legaia_engine_vm::battle_impact_fx as ifx;
                                let blend_armed = b.render_flag == 0 && b.render_blend != 0;
                                if (b.impact_state != 0 || blend_armed)
                                    && b.render_color != ifx::IMPACT_NEUTRAL_STATE
                                    && b.render_color != 0
                                {
                                    let strength = if b.impact_state != 0 {
                                        ifx::IMPACT_TINT_CUE_STRENGTH
                                    } else {
                                        ifx::IMPACT_TINT_CUE_STRENGTH
                                            * (b.render_blend.min(0x1000) as f32 / 4096.0)
                                    };
                                    let c = ifx::unpack_actor_state_rgb(b.render_color);
                                    cue = Some(legaia_engine_render::DrawCue {
                                        far: [
                                            f32::from(c[0]) / 255.0,
                                            f32::from(c[1]) / 255.0,
                                            f32::from(c[2]) / 255.0,
                                        ],
                                        near_z: -1.0,
                                        far_z: 0.0,
                                        max_ir0: strength,
                                    });
                                }
                            }
                            // Hit reaction: the struck actor ramps toward
                            // white for a few frames after damage lands.
                            // Retail's own recolour is the per-actor colour
                            // word `FUN_8004A908` steps through the actor
                            // OTZ/colour setup; the port has no per-actor
                            // colour word, so the flash rides the same
                            // saturated `DrawCue` seam the target cursor
                            // uses - a flat blend toward the cue colour,
                            // decaying over `HIT_FLASH_FRAMES`. It composes
                            // over the cursor tint rather than beside it:
                            // a struck monster that is also the pointed-at
                            // one flashes, then falls back to its pulse.
                            if let Some(age) = self.battle_hit_age(i) {
                                let k = 1.0 - f32::from(age) / f32::from(HIT_FLASH_FRAMES.max(1));
                                cue = Some(legaia_engine_render::DrawCue {
                                    far: [1.0, 1.0, 1.0],
                                    near_z: -1.0,
                                    far_z: 0.0,
                                    max_ir0: 0.8 * k,
                                });
                            }
                        }
                        draws.push(SceneDraw {
                            mesh,
                            mvp: actor_cam * model,
                            cue,
                        });
                    }
                }
                // Arts after-image ghosts trail the live bodies: additive
                // flat-colour copies at the rate-scheduled historical poses.
                // Retail pushes each ghost 0x50 OT buckets deeper than the
                // live body (FUN_80048A08), so under painter ordering the
                // body covers the coincident screen area REGARDLESS of true
                // depth - including the attack camera's behind-the-attacker
                // framings, where the trailing ghost is genuinely nearer the
                // camera than the body. A depth buffer cannot express that
                // with any compare function (the blend pass's GreaterEqual
                // passed every coincident fragment and washed the whole mesh
                // additive - the "monster glows yellow" defect), so each
                // ghost is scaled uniformly ABOUT THE EYE until it sits past
                // the body's distance: every vertex slides along its own
                // camera ray (screen silhouette unchanged) while the body's
                // opaque depth wins wherever they overlap.
                if in_battle {
                    use legaia_engine_core::battle_afterimage as ai;
                    let eye = ai::camera_eye_from_vp(&actor_cam.to_cols_array());
                    for (mesh, model, gpos, bpos) in &battle_ghost_uploads {
                        let push = match eye {
                            Some(e) => {
                                let k = ai::ghost_eye_push_scale(
                                    e,
                                    *gpos,
                                    *bpos,
                                    ai::GHOST_EYE_PUSH_MARGIN,
                                );
                                let ev = Vec3::from(e);
                                Mat4::from_translation(ev)
                                    * Mat4::from_scale(Vec3::splat(k))
                                    * Mat4::from_translation(-ev)
                            }
                            None => Mat4::IDENTITY,
                        };
                        color_draws.push(ColorSceneDraw {
                            mesh,
                            mvp: actor_cam * push * *model,
                        });
                    }
                }
            }
            let mut hud = self.build_hud(w, h);
            // Post-battle spoils panel. The XP / gold / drops a victory
            // credits used to land with no on-screen acknowledgement at all
            // (`World::last_battle_rewards` had no reader outside its own
            // declaration); this is the shared `engine-ui` builder both hosts
            // draw. Suppressed while a boot-UI panel owns the frame.
            if !self.boot_ui.is_active() {
                hud.extend(self.battle_spoils_draws(w, h));
                hud.extend(self.encounter_hint_draws(w, h));
            }
            let overlay = TextOverlay { atlas, draws: &hud };

            // Boot-phase sprite overlay: alternates between the
            // publisher-logos atlas (during PublisherLogos) and
            // the title-screen atlas (during Title). PROKION/SCEA
            // are vertically-packed sprite atlases -
            // `publisher_logo_sprite_draws` unfolds them into N
            // side-by-side strips; Contrail/WARNING + the title
            // TIM produce a single quad each.
            let logo_draw_vec = self.publisher_logo_sprite_draws(w, h);
            let title_draw_vec = self.title_screen_sprite_draws(w, h);
            let menu_glyph_draw_vec = self.title_menu_glyph_sprite_draws(w, h);
            // Muscle Dome hub screens (intro card / ROUND banner / INTERVAL +
            // score tally), placed by the shared `other_game_hud` emitters.
            // Rides sprite slot 1: the boot-UI overlays that own it are all
            // inactive while a dome leg or its between-legs beat is up.
            let muscle_hub_draw_vec = self.muscle_hub_sprite_draws(w, h);
            // Slot-2 chrome samples the resident system-UI atlas.
            // Save-select pills/panel and the field-menu window
            // frame are mutually-exclusive boot states, so both
            // share this one vec (the field-menu frame draws
            // behind its text, which is emitted in the text layer).
            let mut save_chrome_draw_vec = self.save_select_chrome_sprite_draws(w, h);
            save_chrome_draw_vec.extend(self.field_menu_chrome_sprite_draws(w, h));
            // Dialog-window chrome (gradient fill + gold frame + hand
            // cursors) shares the system-UI atlas slot; a dialog box
            // and the boot/menu chrome are mutually exclusive states.
            save_chrome_draw_vec.extend(self.dialog_chrome_sprite_draws(w, h));
            // Name-entry window chrome (grid + name-field filigree
            // windows + hand cursor) shares the same atlas slot.
            save_chrome_draw_vec.extend(self.name_entry_chrome_sprite_draws(w, h));
            // Battle HUD chrome (party-strip + plaque lozenges and the
            // gold HP / green MP label cells) comes out of the same
            // atlas; battle and the boot/menu chrome never coexist. Drawn
            // first of the three battle surfaces so the prompt box and the
            // command chips below layer over the readout, not under it.
            save_chrome_draw_vec.extend(self.battle_chrome_sprite_draws(w, h));
            // Sparring-tutorial prompt box: the same window skin, framed at
            // the rect the retail emitter registers the prompt with. In
            // battle, so it cannot coexist with the boot/menu chrome above.
            save_chrome_draw_vec.extend(self.battle_tutorial_chrome_sprite_draws(w, h));
            // Arts command-input chrome (direction chips + D-pad, the
            // pennant input bar, the AP plate). Also system-UI-atlas
            // sampled, and mutually exclusive with every state above -
            // it only draws inside a battle. Coexists with the prompt box
            // above: retail shows the drill's instruction window over the
            // chips it is describing.
            save_chrome_draw_vec.extend(self.arts_input_chrome_sprite_draws(w, h));
            let logo_overlay = self.publisher_logos.as_ref().map(|p| TextOverlay {
                atlas: &p.atlas,
                draws: &logo_draw_vec,
            });
            let title_overlay = self.title_screen.as_ref().map(|t| TextOverlay {
                atlas: &t.atlas,
                draws: &title_draw_vec,
            });
            let menu_glyph_overlay = self.menu_glyphs.as_ref().map(|m| TextOverlay {
                atlas: &m.atlas,
                draws: &menu_glyph_draw_vec,
            });
            let save_chrome_overlay = self.save_menu.as_ref().map(|sm| TextOverlay {
                atlas: &sm.atlas,
                draws: &save_chrome_draw_vec,
            });
            let muscle_hub_overlay = self.muscle_hub.as_ref().map(|m| TextOverlay {
                atlas: &m.atlas,
                draws: &muscle_hub_draw_vec,
            });
            // Opening-cutscene "It was the Seru." caption: the opdeene baked TIM
            // (`World::cutscene_caption`) blitted centered and faded
            // (`cutscene_caption_alpha`) over the gap between the two narration
            // crawls. One textured quad sampling the caption atlas - the
            // background palette entry is transparent, so only the white text
            // draws over the scene; alpha 0 emits nothing. Scaled by `h/240` to
            // preserve the PSX 320x240 framing (retail centers it horizontally,
            // mid-screen ~y110 over the villager tableau).
            let caption_draw_vec: Vec<legaia_engine_render::SpriteDraw> = {
                let alpha = self.session.host.world.cutscene_caption_alpha;
                match self.caption_atlas.as_ref() {
                    Some((_, cw, ch)) if alpha > 0.001 => {
                        let scale = h as f32 / 240.0;
                        let dw = (*cw as f32 * scale).round().max(0.0) as u32;
                        let dh = (*ch as f32 * scale).round().max(0.0) as u32;
                        let dx = (w as i32 - dw as i32) / 2;
                        let dy = ((110.0 / 240.0) * h as f32).round() as i32 - dh as i32 / 2;
                        vec![legaia_engine_render::SpriteDraw {
                            dst: (dx, dy, dw, dh),
                            src: (0, 0, *cw, *ch),
                            color: [1.0, 1.0, 1.0, alpha.clamp(0.0, 1.0)],
                        }]
                    }
                    _ => Vec::new(),
                }
            };
            let caption_overlay = self
                .caption_atlas
                .as_ref()
                .map(|(atlas, _, _)| TextOverlay {
                    atlas,
                    draws: &caption_draw_vec,
                });

            // Force a pure-black background during boot UI so the
            // logos / title / save-select panels read on PSX-style
            // black instead of the default dark-blue clear. In a
            // stage-dome battle clear to a sky blue so the gaps the
            // front-half dome leaves open read as sky (like retail)
            // rather than the bare grey clear.
            let scene_clear = if self.boot_ui.is_active() && !game_over_hold {
                Some([0.0, 0.0, 0.0, 1.0])
            } else if self.session.host.world.mode == SceneMode::Battle
                && self.battle_stage_mesh.is_some()
            {
                Some([0.32, 0.46, 0.66, 1.0])
            } else {
                None
            };

            // Slot 1: logos OR title-art bands (title still
            // emits during SaveSelect, dimmed). Slot 2: either
            // the save-menu chrome (panel + slot pills) when
            // SaveSelect is active, or the menu-glyph atlas
            // (deprecated no-disc title-menu fallback) otherwise.
            // The opdeene caption takes slot 1 when active: during the opening
            // cutscene the boot-UI logo / title overlays are inactive (their
            // draw vecs empty), so there is no contention.
            let sprites_slot_1 = if !caption_draw_vec.is_empty() {
                caption_overlay.as_ref()
            } else if !logo_draw_vec.is_empty() {
                logo_overlay.as_ref()
            } else if !title_draw_vec.is_empty() {
                title_overlay.as_ref()
            } else if !muscle_hub_draw_vec.is_empty() {
                muscle_hub_overlay.as_ref()
            } else {
                None
            };
            let sprites_slot_2 = if !save_chrome_draw_vec.is_empty() {
                save_chrome_overlay.as_ref()
            } else if !menu_glyph_draw_vec.is_empty() {
                menu_glyph_overlay.as_ref()
            } else {
                None
            };
            // FX model matrices pair with the active render frame:
            // battle cameras carry no world negation (keep the
            // per-model Y-flip); the field cameras compose
            // FIELD_WORLD_FLIP (draw raw PSX Y-down vertices).
            let fx_in_battle = self.session.host.world.mode == SceneMode::Battle;
            let fx_model_flip = if fx_in_battle {
                Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0))
            } else {
                Mat4::IDENTITY
            };
            // Battle FX ride the actor camera composition (the retail
            // 4x world-scale base under the shared rotation) so
            // effects land on the scaled actor stage; field FX use
            // the field camera as-is.
            // The uniform scale `fx_cam` composes on top of `cam`. Effect
            // billboards need it separately from the matrix: retail forms a
            // sprite quad's corners in VIEW space, after the camera matrix has
            // scaled the centre (`FUN_800195a8` - the `MVMVA` transforms the
            // centre, the corner adds follow it, and the matrix is reset to
            // identity before the projection), so the half-extents must not go
            // through the 4x a second time. See
            // `legaia_engine_vm::effect_billboard`.
            let fx_scale = if fx_in_battle && self.battle_stage_mesh.is_some() {
                BATTLE_WORLD_SCALE
            } else {
                1.0
            };
            let fx_cam = if fx_scale != 1.0 {
                cam * Mat4::from_scale(Vec3::splat(fx_scale))
            } else {
                cam
            };
            // Effect-pool billboards: bridge live effect child sprites
            // into the renderer as faithful camera-facing quads sized
            // and UV-addressed from the effect bundle's inline atlas
            // (`World::active_effect_sprites`). Each draws two ways: a
            // textured quad sampling the scene VRAM at the sprite's
            // atlas page/clut/uv (the retail FUN_801E0088 pass-2 path -
            // in battle the flame atlas + CLUT rows are resident via the
            // battle-entry blit, see `effect_billboard_mesh`), plus a
            // tinted outline through the Lines pipeline so the spawn
            // reads even where a sprite samples unloaded texels. See
            // docs/subsystems/effect-vm.md. The billboards ride `fx_cam`
            // like every other battle FX layer: in a stage-dome battle
            // the pool positions are actor-stage coordinates, so drawing
            // them under the unscaled `cam` landed each quad 4x too
            // small at the wrong stage position. The camera-facing basis
            // derives from the same matrix, so the quads face the camera
            // that actually draws them.
            let (effect_billboard, effect_lines) =
                self.build_effect_billboards(r, fx_cam, fx_scale);
            self.diag_effect_billboards(fx_cam);
            let effect_billboard = if std::env::var_os("LEGAIA_DIAG_NOFX").is_some() {
                None
            } else {
                effect_billboard
            };
            if let Some(mesh) = effect_billboard.as_ref() {
                draws.push(SceneDraw {
                    mesh,
                    mvp: fx_cam,
                    cue: None,
                });
            }
            // World-map overlay lines: a kind-coded upright marker for
            // each placed entity (portal / NPC / encounter zone, from
            // `World::world_map_entity_markers`) plus the player marker
            // (`World::world_map_player_marker`) - the player's own mesh
            // isn't drawn in world-map mode. Both build into one Lines
            // mesh, routed through the same overlay slot as the effect
            // outlines (mutually exclusive: no effects spawn on the
            // world map). Without this the installed entities + player
            // carry positions but never appear on screen.
            let world_map_entity_lines = self.build_world_map_overlay_lines(r, in_world_map);
            // Effect 3D models (`etmd.dat`): spell effects like Tail
            // Fire are small Gouraud-shaded `etmd` meshes textured by
            // the resident `etim` texels, not billboards. Build a
            // per-frame VRAM mesh + transform for each live effect that
            // has a model assigned (same per-frame model-matrix
            // convention as `actor_model`). Held in a local Vec so the
            // meshes outlive the render borrow.
            let effect_model_draws = self.build_effect_model_draws(r, fx_model_flip, in_world_map);
            for (mesh, model) in &effect_model_draws {
                draws.push(SceneDraw {
                    mesh,
                    mvp: fx_cam * *model,
                    cue: None,
                });
            }

            // Active Seru-magic summon scene-graph (debug-spawned via
            // `G`): one textured mesh per move-VM-driven part, posed by
            // the part's interpreted transform (world pos + rotation
            // banks). The animation computation is faithful (move VM);
            // the transform composition is the open PROT 0900 piece.
            let summon_part_draws =
                self.build_summon_and_move_fx_part_draws(r, fx_model_flip, in_world_map);
            for (mesh, model) in &summon_part_draws {
                draws.push(SceneDraw {
                    mesh,
                    mvp: fx_cam * *model,
                    cue: None,
                });
            }
            // Field move-VM effect parts (op 0x34 sub-3 stagers): resolve
            // each mesh part against the SCENE's TMD pack - `env_tmds` =
            // `res.tmds` filtered to the scene_asset_table bundle entry,
            // the same source the field-placement renderer + the
            // asset-viewer use - NOT the battle `global_tmd_pool`. Retail
            // resolves a field stager's mesh as `DAT_8007C018[model_sel +
            // DAT_8007B6F8]`, where `DAT_8007B6F8 = 5` is the character-mesh
            // prefix and `DAT_8007C018[5..]` is exactly this scene pack; so
            // the part's relative `model_sel` (spawn base 0, surfaced as
            // `model_index`) indexes `env_tmds` directly, mirroring how a
            // placement's `pack_index` does.
            let field_fx_draws = self.build_field_fx_part_draws(r, fx_model_flip, in_world_map);
            for (mesh, model) in &field_fx_draws {
                draws.push(SceneDraw {
                    mesh,
                    mvp: fx_cam * *model,
                    cue: None,
                });
            }
            // Screen-effect widget overlays (the PROT-0900 mask /
            // sprite / panel / letterbox family, field-VM op 0x43):
            // composite the world's published per-frame draw list
            // above the 3D scene under an orthographic screen-space
            // MVP (PSX 320x240 frame). Solid border/band quads ride
            // the untextured colour pipeline; panel + sprite quads
            // ride the VRAM pipeline (clut/texpage sampled like any
            // retail prim). Depth layers mirror the retail OT slots:
            // sprites (+0xc) in front, panels (+0x10), mask/bands
            // (+0x1c) behind. The letterbox gradient feather strips
            // are subtractive-blend draws the engine doesn't model
            // yet and are skipped.
            let (screen_fx_solid, screen_fx_tex) = self.build_screen_fx_meshes(r);
            let screen_fx_mvp = Mat4::orthographic_rh(0.0, 320.0, 240.0, 0.0, 0.0, 1.0);
            if let Some(m) = &screen_fx_tex {
                draws.push(SceneDraw {
                    mesh: m,
                    mvp: screen_fx_mvp,
                    cue: None,
                });
            }
            if let Some(m) = &screen_fx_solid {
                color_draws.push(ColorSceneDraw {
                    mesh: m,
                    mvp: screen_fx_mvp,
                });
            }
            // Floating value readout: the numeral a landed hit throws.
            //
            // Retail's own art and geometry - 24x24 cells off the battle
            // effect atlas (texture page `0x27`, CLUT `0x7703`), thrown over
            // the STRUCK actor, growing about a fixed horizontal centre and
            // rising to screen row 32. Laid out by
            // `engine-vm::battle_value_readout::value_cells`, whose module
            // header carries the packet + VRAM provenance. Drawn here rather
            // than in the HUD builder because the seat needs the target's
            // projected screen position, which only a host holding the camera
            // has; it rides the same screen-space ortho MVP as the widget
            // overlays above, so the numerals sit in front of the fight.
            let value_readout = self.battle_value_readout_mesh(r, fx_cam);
            if let Some(m) = &value_readout {
                draws.push(SceneDraw {
                    mesh: m,
                    mvp: screen_fx_mvp,
                    cue: None,
                });
            }
            // The scripted screen fade (op 0x4C 0x12) is NOT drawn as a wash
            // mesh here: it is a multiply tint staged into the colour grade +
            // depth-cue far colour (see the grade staging above), matching
            // the retail mechanism - the 3D scene darkens while the narration
            // overlay keeps scrolling bright.
            legaia_engine_render::profile::draw_counts(draws.len(), color_draws.len());
            let scene = RenderScene {
                vram,
                draws: &draws,
                color_draws: &color_draws,
                // Effect outlines share the billboards' `fx_cam` (the two
                // sources are mutually exclusive, and off the battle stage
                // `fx_cam == cam`, so the world-map markers are unaffected).
                overlay_lines: world_map_entity_lines
                    .as_ref()
                    .or(effect_lines.as_ref())
                    .map(|m| (m, fx_cam)),
                overlay_sprites: sprites_slot_1,
                overlay_sprites_2: sprites_slot_2,
                overlay_text: Some(&overlay),
                clear_color: scene_clear,
            };
            legaia_engine_render::profile::mark("drawlist");
            // On the frame the transition arms, land the field frame in the
            // software VRAM the intro strips texture themselves with, and draw
            // the rest of this frame against that page. Retail gets it for
            // free - on the console the framebuffer *is* VRAM - so the port
            // re-renders this scene offscreen and blits the readback in.
            if let Some(v) = Self::capture_battle_intro_frame(
                battle_intro.as_mut(),
                r,
                &scene,
                self.cpu_vram_base.as_ref(),
            ) {
                // Keep the captured page GPU-resident for the whole
                // transition: the capture is a one-shot, but every
                // transition frame's primitives sample it (the curtain
                // strips, and the tile shatter's pages + shade page).
                self.battle_intro_vram = Some(v);
            }
            let scene = match (battle_intro.as_ref(), self.battle_intro_vram.as_ref()) {
                (Some(_), Some(v)) => RenderScene { vram: v, ..scene },
                _ => scene,
            };
            // The intro's primitives composite *over* the scene in one frame.
            // `RenderTarget::ScreenOverlay` cannot do it: that is a whole-frame
            // mode which clears and draws nothing but quads, so it could never
            // carry a transition strip over a field scene.
            //
            // The target is built *before* the screenshot harness so a capture
            // sees the frame that is presented. Capturing `Scene(&scene)` here
            // instead would silently drop every transition style from the PNGs
            // - the harness's own blind spot, not the emitter's.
            // Screen-space primitives composited over the scene this
            // frame: the field-to-battle intro's styles, plus the in-battle
            // weapon-trail bands (mutually exclusive in practice - the
            // trail only draws once a swing clip plays inside the battle).
            let mut screen_prims = battle_intro_prims;
            screen_prims.extend(self.weapon_trail_screen_prims(r));
            let target = |scene| present_target(scene, &screen_prims);
            // Periodic sweep (`--screenshot-every`): capture a frame every N
            // ticks into the sweep dir (named for the tick), keep running,
            // and exit after the capture at/past `--screenshot-last-tick`.
            // Redraws can drain up to 4 ticks, so the cadence is tracked via
            // `sweep_next_tick` rather than a modulo on the tick counter.
            if let Some(sw) = self.screenshot.as_ref().and_then(|sc| sc.sweep.as_ref())
                && self.tick_no >= self.sweep_next_tick
            {
                let path = sw.dir.join(format!("tick_{:05}.png", self.tick_no));
                let last_tick = sw.last_tick;
                self.sweep_next_tick = self.tick_no + sw.every;
                match r.capture_rgba(target(&scene)) {
                    Ok(img) => match write_capture_png(&path, &img) {
                        Ok(()) => {
                            println!(
                                "[ok] screenshot {} ({}x{}) at tick {}",
                                path.display(),
                                img.width,
                                img.height,
                                self.tick_no
                            );
                        }
                        Err(e) => {
                            eprintln!("screenshot write failed: {e:#}");
                            std::process::exit(1);
                        }
                    },
                    Err(e) => {
                        eprintln!("screenshot capture failed: {e:#}");
                        std::process::exit(1);
                    }
                }
                if last_tick.is_some_and(|lt| self.tick_no >= lt) {
                    std::process::exit(0);
                }
            }
            // Screenshot harness: at the target tick, read the frame back
            // offscreen and exit instead of presenting to the window.
            let capture_due = self
                .screenshot
                .as_ref()
                .is_some_and(|sc| sc.path.is_some() && self.tick_no >= sc.capture_tick);
            if capture_due {
                let path = self
                    .screenshot
                    .as_ref()
                    .and_then(|sc| sc.path.clone())
                    .unwrap();
                match r.capture_rgba(target(&scene)) {
                    Ok(img) => match write_capture_png(&path, &img) {
                        Ok(()) => {
                            println!(
                                "[ok] screenshot {} ({}x{}) at tick {}",
                                path.display(),
                                img.width,
                                img.height,
                                self.tick_no
                            );
                            std::process::exit(0);
                        }
                        Err(e) => {
                            eprintln!("screenshot write failed: {e:#}");
                            std::process::exit(1);
                        }
                    },
                    Err(e) => {
                        eprintln!("screenshot capture failed: {e:#}");
                        std::process::exit(1);
                    }
                }
            }
            if let Err(e) = r.render(target(&scene)) {
                log::error!("render: {e:#}");
            }
        }
        // The transition emitter was taken out of `self` for the render
        // borrow; put it back so its working set survives to the next frame.
        self.battle_intro = battle_intro;
        legaia_engine_render::profile::end_frame();
        self.win.request_redraw();
    }
}

/// Where a landed hit's numeral starts, above the struck actor's own origin,
/// in stage pixels. Retail's start seat is captured only once
/// (`battle_melee_hit_spark`, top edge 49 with the actor mid-frame), so the
/// lift is engine-chosen; the row it rises **to** is pinned.
const VALUE_READOUT_ACTOR_LIFT: i32 = 26;

/// Frames the struck actor stays flashed after a hit lands.
pub(super) const HIT_FLASH_FRAMES: u16 = 10;

impl PlayWindowApp {
    /// Screen-space stage position (retail 320x240) an actor's origin
    /// projects to under `cam`, or `None` when it is behind the camera.
    pub(super) fn actor_stage_point(&self, slot: usize, cam: Mat4) -> Option<(i32, i32)> {
        let a = self.session.host.world.actors.get(slot)?;
        let w = Vec3::new(
            a.move_state.world_x as f32,
            a.move_state.world_y as f32,
            a.move_state.world_z as f32,
        );
        let clip = cam * w.extend(1.0);
        if clip.w <= 0.01 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some((
            ((ndc.x * 0.5 + 0.5) * 320.0) as i32,
            ((0.5 - ndc.y * 0.5) * 240.0) as i32,
        ))
    }

    /// `LEGAIA_DIAG_FX=1` instrument: report where each live effect billboard
    /// actually lands, in the frame's own FX camera.
    ///
    /// "The spawn fires and nothing appears" has three distinguishable causes
    /// and this separates them in one line per sprite: a clip `w <= 0` (the
    /// quad is behind the eye), NDC outside `[-1, 1]` (projected off-screen),
    /// or an in-frame NDC with texel coordinates that name a page nothing
    /// uploaded (the quad draws, but every texel discards). Off by default -
    /// the log is per-frame per-sprite.
    fn diag_effect_billboards(&self, cam: Mat4) {
        if std::env::var_os("LEGAIA_DIAG_FX").is_none() {
            return;
        }
        let sprites = self.session.host.world.active_effect_sprites();
        if sprites.is_empty() {
            return;
        }
        for slot in 0..4usize {
            if let Some(a) = self.session.host.world.actors.get(slot) {
                log::info!(
                    "DIAG fx actor {slot}: world ({},{},{}) screen {:?}",
                    a.move_state.world_x,
                    a.move_state.world_y,
                    a.move_state.world_z,
                    self.actor_stage_point(slot, cam)
                );
            }
        }
        for s in sprites.iter().take(4) {
            let p = cam * glam::Vec4::new(s.world_pos[0], s.world_pos[1], s.world_pos[2], 1.0);
            let ndc = if p.w.abs() > 1e-6 {
                [p.x / p.w, p.y / p.w, p.z / p.w]
            } else {
                [f32::NAN; 3]
            };
            let u0 = s.uv[0] as u8;
            let v0 = s.uv[1] as u8;
            let u1 = u0.saturating_add(s.uv_size[0].saturating_sub(1) as u8);
            let v1 = v0.saturating_add(s.uv_size[1].saturating_sub(1) as u8);
            let texels = self
                .battle_vram
                .as_ref()
                .or(self.cpu_vram_base.as_ref())
                .map(|v| {
                    v.prim_texture_status(s.clut, s.page, &[(u0, v0), (u1, v0), (u0, v1), (u1, v1)])
                });
            // Resolve the sprite's own texel rect through its CLUT, exactly
            // as the fragment shader would: 4bpp indices out of the texture
            // page, index 0 discarded, everything else a BGR555 word.
            let mut opaque = 0usize;
            let mut total = 0usize;
            let mut sample = 0u16;
            if let Some(v) = self.battle_vram.as_ref().or(self.cpu_vram_base.as_ref()) {
                let px = ((s.page & 0xF) * 64) as usize;
                let py = (((s.page >> 4) & 1) * 256) as usize;
                let cx = ((s.clut & 0x3F) * 16) as usize;
                let cy = ((s.clut >> 6) & 0x1FF) as usize;
                for dv in 0..s.uv_size[1] as usize {
                    for du in 0..s.uv_size[0] as usize {
                        let u = u0 as usize + du;
                        let word = v.pixel(px + (u >> 2), py + v0 as usize + dv);
                        let idx = ((word >> (4 * (u & 3))) & 0xF) as usize;
                        total += 1;
                        if idx != 0 {
                            opaque += 1;
                            if sample == 0 {
                                sample = v.pixel(cx + idx, cy);
                            }
                        }
                    }
                }
            }
            log::info!(
                "DIAG fx: n={} pos {:?} size {:?} page {:#04x} clut {:#06x} uv {:?}+{:?} \
                 bright {:#04x} clip.w {:.1} ndc [{:.3} {:.3} {:.3}] texels {:?} \
                 opaque {opaque}/{total} sample {sample:#06x}",
                sprites.len(),
                s.world_pos,
                s.size,
                s.page,
                s.clut,
                s.uv,
                s.uv_size,
                s.brightness,
                p.w,
                ndc[0],
                ndc[1],
                ndc[2],
                texels,
            );
        }
    }

    /// How recently actor `slot` was struck, in frames, or `None` if the
    /// flash window has passed. Derived from the popup queue, which is what
    /// the per-strike FX drain already fills.
    pub(super) fn battle_hit_age(&self, slot: usize) -> Option<u16> {
        self.battle_hud
            .popups
            .iter()
            .filter(|p| usize::from(p.slot) == slot && !p.is_heal)
            .map(|p| p.frames_total.saturating_sub(p.frames_remaining))
            .filter(|&age| age < HIT_FLASH_FRAMES)
            .min()
    }

    /// The frame's floating value readout, as one screen-space VRAM-textured
    /// mesh in stage coordinates.
    ///
    /// One run of digit cells per live popup, seated over the struck actor
    /// (`actor_stage_point`) and laid out by
    /// `legaia_engine_vm::battle_value_readout::value_cells`. Returns `None`
    /// outside battle, with no popups, or before the battle VRAM (which is
    /// what makes the effect atlas's digit page resident) has been uploaded.
    pub(super) fn battle_value_readout_mesh(
        &self,
        r: &legaia_engine_render::Renderer,
        cam: Mat4,
    ) -> Option<legaia_engine_render::UploadedVramMesh> {
        use legaia_engine_vm::battle_value_readout as vr;
        if self.session.host.world.mode != SceneMode::Battle {
            return None;
        }
        if self.battle_vram.is_none() || self.battle_hud.popups.is_empty() {
            return None;
        }
        let mut pos: Vec<[f32; 3]> = Vec::new();
        let mut uvs: Vec<[u8; 2]> = Vec::new();
        let mut cba_tsb: Vec<[u16; 2]> = Vec::new();
        let mut idx: Vec<u32> = Vec::new();
        // One numeral per actor, the newest. Retail's readout is a per-slot
        // **value window** (`_DAT_801F6980`, four halfwords, one per slot), so
        // a second hit on the same actor replaces the figure rather than
        // stacking beside it - and stacking is not cosmetic here: two runs
        // centred on the same point interleave into an unreadable third
        // number (a 9 landing inside a 10 reads as "190").
        let mut newest: Vec<&legaia_engine_core::battle_hud::DamagePopup> = Vec::new();
        for p in &self.battle_hud.popups {
            if p.status.is_some() {
                // Status applications have no numeral on the sheet.
                continue;
            }
            match newest.iter_mut().find(|q| q.slot == p.slot) {
                Some(q) if q.frames_remaining >= p.frames_remaining => {}
                Some(q) => *q = p,
                None => newest.push(p),
            }
        }
        for p in newest {
            let Some((ax, ay)) = self.actor_stage_point(usize::from(p.slot), cam) else {
                continue;
            };
            let age = p.frames_total.saturating_sub(p.frames_remaining);
            let cells = vr::value_cells(p.amount, ax, ay - VALUE_READOUT_ACTOR_LIFT, age);
            for c in &cells {
                let base = pos.len() as u32;
                let (x0, y0) = (c.x as f32, c.y as f32);
                let (x1, y1) = (x0 + c.w as f32, y0 + c.h as f32);
                pos.push([x0, y0, 0.0]);
                pos.push([x1, y0, 0.0]);
                pos.push([x0, y1, 0.0]);
                pos.push([x1, y1, 0.0]);
                let u1 = c.u.saturating_add(c.cell - 1);
                let v1 = c.v.saturating_add(c.cell - 1);
                uvs.extend_from_slice(&[[c.u, c.v], [u1, c.v], [c.u, v1], [u1, v1]]);
                cba_tsb.extend(std::iter::repeat_n([vr::GLYPH_CLUT, vr::GLYPH_TPAGE], 4));
                idx.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
            }
        }
        if idx.is_empty() {
            return None;
        }
        let normals = vec![[0.0f32; 3]; pos.len()];
        // Retail's quads carry the neutral colour word `0x808080`: the sheet
        // is already the gold ramp, so a tint here would double-apply it.
        let colors = vec![[legaia_tmd::legaia_prims::MODULATION_NEUTRAL; 3]; pos.len()];
        match r.upload_vram_mesh(&pos, &uvs, &cba_tsb, &normals, &colors, &idx) {
            Ok(m) => Some(m),
            Err(e) => {
                log::warn!("battle value readout mesh upload: {e:#}");
                None
            }
        }
    }
}

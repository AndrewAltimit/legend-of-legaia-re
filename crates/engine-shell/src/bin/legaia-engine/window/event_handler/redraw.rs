//! `RedrawRequested` window-event handler (per-frame tick + render),
//! extracted from `event_handler.rs` (mechanical split; behavior-preserving).

use super::super::*;

impl PlayWindowApp {
    pub(super) fn handle_redraw(&mut self) {
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
            // Opening-cutscene narration plays first. While its
            // subtitle pages are on screen the field is held and a
            // confirm press (Cross) skips to the next page; the
            // per-page timer (World::tick) auto-advances otherwise.
            // Only once the narration completes does a confirm reach
            // the hand-off gate below - so the prologue narration
            // precedes the Rim Elm transition, mirroring retail order.
            if self.session.host.world.cutscene_narration_active() {
                if pressed_edge & 0x4000 != 0 {
                    self.session.host.world.skip_cutscene_narration();
                }
                // Freeze player movement (pad held at 0) but keep the
                // world ticking so the narration's per-page timer
                // advances and the scene still renders.
                self.session.host.world.set_pad(0);
                if let Err(e) = self.session.tick() {
                    log::error!("session tick (narration): {e:#}");
                }
                self.prev_pad = self.pad;
                continue;
            }
            // Prologue cutscene -> Rim Elm handoff. While in `opdeene`
            // with the trigger armed, a confirm press (Cross) hands off
            // to `town01`, mirroring FUN_801D1344's flag + pad gate.
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
            if pressed_edge & 0x0008 != 0 && !self.menu_runtime.is_open() {
                // Start: open the BootSession-hosted pause menu (the
                // retail CARD pair, game_mode 0x17 - the world holds
                // SceneMode::Menu while it is open) and route the
                // window's input + draws to it via the boot-UI arm.
                self.session.open_field_menu();
                self.boot_ui = BootUiState::FieldMenu { sub: None };
                self.prev_pad = self.pad;
                continue;
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
                // Route the move's sound cue the same way the field-FX /
                // debug path does (classify only; move-FX cue playback is
                // not yet wired to the SFX ring).
                if let Some(cue) = self.session.host.world.take_pending_move_fx_cue() {
                    let dispatch = legaia_engine_audio::classify_cue(cue as u32);
                    log::debug!("battle move-FX cue {cue:#04x} -> {dispatch:?}");
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
            // In battle, advance each monster actor's per-object idle
            // animation into its `pose_frame` (the render pass below
            // deforms the mesh via the rigid `posed_rot` builder).
            if self.session.host.world.mode == SceneMode::Battle {
                self.session.host.world.tick_battle_animations();
                // ...and re-stamp the party's eye/mouth face frames
                // from the playing clips' facial tracks (the retail
                // per-frame facial animator).
                self.tick_battle_face_stamps();
            }
            // World-map ocean shimmer: cycle the 13-frame CLUT animation
            // (self-gates to None off the world map).
            self.advance_ocean_animation();
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
        // Ease the in-engine cutscene camera between Camera Configure
        // beats. Done here (outside the renderer borrow below) so the
        // interpolator can take `&mut self`; while no cutscene timeline
        // owns the scene the interp is reset so the next opening shot
        // snaps in rather than sweeping from a stale pose.
        let cutscene_cam = if self.session.host.world.mode != SceneMode::WorldMap
            && self.session.host.world.cutscene_timeline_active()
        {
            let (look_at, pitch, yaw, fov) = self.cutscene_view();
            // ~0.15/frame ease => a few-frame blend at the redraw cadence.
            Some(
                self.cutscene_cam_interp
                    .approach(look_at, pitch, yaw, fov, 0.15),
            )
        } else {
            self.cutscene_cam_interp.reset();
            None
        };
        if let (Some(r), Some(vram), Some(atlas)) = (
            self.win.renderer.as_ref(),
            self.uploaded_vram.as_ref(),
            self.font_atlas.as_ref(),
        ) {
            let (w, h) = r.surface_size();
            let aspect = w as f32 / h.max(1) as f32;
            // World-map mode frames the loaded map with the
            // controller-driven camera (azimuth / zoom / pan); an active
            // in-engine cutscene (opdeene opening prologue) frames the
            // cutscene's executed op-0x45 camera target; every other mode
            // uses the orbit camera.
            let in_world_map = self.session.host.world.mode == SceneMode::WorldMap;
            let cam = if in_world_map {
                let world = &self.session.host.world;
                let (az, zoom, px, pz, walk_mode) = world
                    .world_map_ctrl
                    .as_ref()
                    .map(|c| (c.azimuth, c.zoom, c.camera_x, c.camera_z, !c.is_top_view()))
                    .unwrap_or((0, 0, 0, 0, true));
                // In walk mode the camera follows the player: pan so the
                // framing centre tracks the player's world position
                // (the AABB-relative offset world_map_camera_mvp adds to
                // its centre). Top-view debug keeps the controller scroll.
                let (pan_x, pan_z) = if walk_mode {
                    let center = [
                        (self.scene_aabb.0[0] + self.scene_aabb.1[0]) * 0.5,
                        (self.scene_aabb.0[2] + self.scene_aabb.1[2]) * 0.5,
                    ];
                    world
                        .player_actor_slot
                        .and_then(|s| world.actors.get(s as usize))
                        .map(|a| {
                            (
                                (a.move_state.world_x as f32 - center[0]) as i32,
                                (a.move_state.world_z as f32 - center[1]) as i32,
                            )
                        })
                        .unwrap_or((px, pz))
                } else {
                    (px, pz)
                };
                // Walk view frames a fixed WORLD-space radius around the
                // player rather than the (small, object-local) kingdom-
                // pack AABB - the continent terrain now draws at world
                // tile coordinates (`field_placement_draws`), so the
                // pack-AABB radius would frame only the one tile under
                // the player. Keep the box centred at the pack-AABB
                // centre (the pan re-centres it on the player) and widen
                // it; top-view keeps the full-pack framing for the
                // overhead continent sweep.
                let (cam_lo, cam_hi) = if walk_mode {
                    // Frame a wide world-space radius around the player so
                    // the overworld reads at retail's overhead scale (the
                    // walk camera also sits steeper - see
                    // `walk_view_camera_mvp`).
                    const WALK_HALF: f32 = 4200.0;
                    let cx = (self.scene_aabb.0[0] + self.scene_aabb.1[0]) * 0.5;
                    let cz = (self.scene_aabb.0[2] + self.scene_aabb.1[2]) * 0.5;
                    (
                        [cx - WALK_HALF, self.scene_aabb.0[1], cz - WALK_HALF],
                        [cx + WALK_HALF, self.scene_aabb.1[1], cz + WALK_HALF],
                    )
                } else {
                    (self.scene_aabb.0, self.scene_aabb.1)
                };
                if walk_mode {
                    // The RETAIL walk-view camera, pinned from the two
                    // overworld resident savestates' RAM (sebucus /
                    // karisto): `screen = H * (R*(6*(v - player)) + TR)
                    // / Ez` with `H = 368` (`0x8007B6F4`), a **6.0x
                    // uniform world scale** (base matrix `0x8007BF10` =
                    // `24576 * I`), R from the `0x8007B790` trio
                    // (pitch-only at azimuth 0; the controller azimuth
                    // feeds ry), focus = the player's world X/Z
                    // (`0x80089118/20` hold its negation, Y = 0), and
                    // TR from `0x800840B8`. The two saves pin two zoom
                    // states - (pitch 360, TR (0,536,9139)) and
                    // (pitch 476, TR (0,406,11041)); the controller
                    // zoom slides along that pinned axis (negative =
                    // pull back), anchored at the closer state.
                    // `FIELD_WORLD_FLIP` cancels `psx_camera_mvp`'s
                    // internal pre-flip, so the whole composition runs
                    // on raw retail Y-down world coordinates and the
                    // overworld elevation renders retail-correct.
                    let world = &self.session.host.world;
                    let player = world
                        .player_actor_slot
                        .and_then(|s| world.actors.get(s as usize))
                        .map(|a| {
                            Vec3::new(
                                a.move_state.world_x as f32,
                                0.0,
                                a.move_state.world_z as f32,
                            )
                        })
                        .unwrap_or_else(|| {
                            Vec3::new(
                                (self.scene_aabb.0[0] + self.scene_aabb.1[0]) * 0.5,
                                0.0,
                                (self.scene_aabb.0[2] + self.scene_aabb.1[2]) * 0.5,
                            )
                        });
                    // Zoom: t=0 -> the sebucus pin, t=1 -> the karisto
                    // pin. Controller zoom is positive-in, so negative
                    // values pull back along the pinned axis.
                    let t = ((-zoom) as f32 / 64.0).clamp(0.0, 1.0);
                    let pitch_units = 360.0 + t * (476.0 - 360.0);
                    let tr = Vec3::new(
                        0.0,
                        536.0 + t * (406.0 - 536.0),
                        9139.0 + t * (11041.0 - 9139.0),
                    );
                    let to_rad = |units: f32| units / 4096.0 * std::f32::consts::TAU;
                    Self::psx_camera_mvp(
                        to_rad(pitch_units),
                        to_rad(az as f32),
                        368.0,
                        tr,
                        Vec3::ZERO,
                        aspect,
                    ) * FIELD_WORLD_FLIP
                        * Mat4::from_scale(Vec3::splat(WORLD_MAP_WORLD_SCALE))
                        * Mat4::from_translation(-player)
                } else {
                    // Top-view debug camera keeps its synthetic framing
                    // but composes the same single world Y-negation so
                    // the (now unflipped) world-map draws render
                    // upright under it.
                    legaia_engine_render::window::world_map_camera_mvp(
                        cam_lo, cam_hi, az, zoom, pan_x, pan_z, aspect,
                    ) * FIELD_WORLD_FLIP
                }
            } else if let Some((look_at, pitch, yaw, fov)) = cutscene_cam {
                // Field world frame: retail Y-down coordinates go
                // through ONE world Y-negation in the camera (the
                // `FIELD_WORLD_FLIP` post-multiply) and the field
                // draws use UN-flipped model matrices, so elevation
                // renders retail-correct (up = retail-negative-Y =
                // screen-up). The look-at is a raw retail world point,
                // so negate its Y into the flipped frame.
                legaia_engine_render::window::cutscene_camera_mvp(
                    [look_at[0], -look_at[1], look_at[2]],
                    pitch,
                    yaw,
                    fov,
                    self.scene_aabb.0,
                    self.scene_aabb.1,
                    aspect,
                ) * FIELD_WORLD_FLIP
            } else if self.session.host.world.mode == SceneMode::Battle {
                if self.battle_stage_mesh.is_some() {
                    // Stage-dome battle: low front-facing shot into the
                    // dome (grass foreground, mountains on the horizon).
                    self.battle_dome_camera_mvp(aspect)
                } else {
                    // No stage: frame the animated enemies (the battle
                    // actors live at the world origin).
                    self.battle_camera_mvp(aspect)
                }
            } else if self.field_debug_camera {
                // Wide debug orbit vantage (`C` toggles), in the same
                // one-world-negation field frame as the follow camera.
                self.camera_mvp(aspect) * FIELD_WORLD_FLIP
            } else {
                // Field: the retail follow camera (savestate-pinned
                // pitch/yaw/H, player-anchored) when a player actor
                // exists; the fixed debug orbit vantage otherwise.
                // `FIELD_WORLD_FLIP` cancels `psx_camera_mvp`'s
                // internal pre-flip, making the follow composition
                // exactly the retail GTE model on raw Y-down world
                // coordinates (elevation renders retail-correct).
                self.field_follow_camera_mvp(aspect)
                    .unwrap_or_else(|| self.camera_mvp(aspect))
                    * FIELD_WORLD_FLIP
            };
            // Drain queued spawn slots: build a VRAM mesh from each
            // actor's `tmd_ref` (global-pool TMD that the field-VM
            // 0x4C 0xD8 host hook installed) and append it to
            // `self.meshes` / `self.scene_tmd_data`, then bind
            // `actor.tmd_binding` to the new mesh index so the
            // draws iteration below picks it up. Idempotent: if
            // the actor already has a binding (e.g. an earlier
            // pass already uploaded), the spawn is skipped.
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
            let mut posed_overrides: Vec<Option<UploadedVramMesh>> =
                (0..self.scene_tmd_data.len()).map(|_| None).collect();
            // The player's untextured colour half follows the same
            // per-frame pose - rebuilt below alongside the textured
            // override, drawn instead of the static rest-pose colour
            // mesh at the draw site.
            let mut player_color_posed: Option<UploadedColorMesh> = None;
            let player_slot = self.session.host.world.player_actor_slot;
            for (ai, actor) in self.session.host.world.actors.iter().enumerate() {
                if !actor.active {
                    continue;
                }
                let (Some(tmd_idx), Some(pose)) = (actor.tmd_binding, &actor.pose_frame) else {
                    continue;
                };
                let Some((tmd, raw)) = self.scene_tmd_data.get(tmd_idx) else {
                    continue;
                };
                // Battle actors and the field player carry per-object
                // rigid-transform clips (rotation matters), so use the
                // full `R·v + T` builder; other field actors keep the
                // translation-only ANM path unchanged.
                let is_field_player = player_slot == Some(ai as u8)
                    && self.player_color_draw.map(|(_, s)| s) == Some(ai);
                let mut vmesh = if actor.battle_animation.is_some() || player_slot == Some(ai as u8)
                {
                    legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(tmd, raw, &pose.bone_outputs)
                } else {
                    legaia_tmd::mesh::tmd_to_vram_mesh_posed(tmd, raw, &pose.bone_outputs)
                };
                if is_field_player {
                    let cmesh =
                        legaia_tmd::mesh::tmd_to_color_mesh_posed_rot(tmd, raw, &pose.bone_outputs);
                    if !cmesh.is_empty() {
                        match r.upload_color_mesh_blended(
                            &cmesh.positions,
                            &cmesh.colors,
                            &cmesh.indices,
                            &cmesh.blend,
                        ) {
                            Ok(m) => player_color_posed = Some(m),
                            Err(e) => {
                                log::warn!("posed player colour upload: {e:#}")
                            }
                        }
                    }
                }
                // The posed mesh is rebuilt from the raw TMD, so its
                // CBA/TSB are the nominal on-disc defaults. Re-apply the
                // per-slot relocation `battle_render_mesh` did for the
                // rest mesh, or the animated monster samples the wrong
                // VRAM page and renders white.
                if let Some(slot) = actor.battle_tex_slot {
                    for ct in &mut vmesh.cba_tsb {
                        ct[0] = legaia_asset::monster_archive::relocate_cba(ct[0], slot);
                        ct[1] = legaia_asset::monster_archive::relocate_tsb(ct[1], slot);
                    }
                }
                if vmesh.indices.is_empty() {
                    continue;
                }
                if std::env::var_os("LEGAIA_DIAG_POSE").is_some() {
                    let (lo, hi) = vmesh.aabb();
                    log::info!(
                        "DIAG pose: tmd {tmd_idx} verts {} aabb {lo:?}..{hi:?} \
                         bones[0..2]={:?}",
                        vmesh.positions.len(),
                        &pose.bone_outputs[..pose.bone_outputs.len().min(2)]
                    );
                }
                match r.upload_vram_mesh(
                    &vmesh.positions,
                    &vmesh.uvs,
                    &vmesh.cba_tsb,
                    &vmesh.normals,
                    &vmesh.indices,
                ) {
                    Ok(m) => posed_overrides[tmd_idx] = Some(m),
                    Err(e) => log::warn!("posed mesh upload: {e:#}"),
                }
            }

            // Field-NPC clip playback: advance each placed NPC's
            // looping ANM clip and re-upload its posed mesh halves
            // (the same per-frame rebuild path the player's idle /
            // walk pair uses). The rest-pose meshes in
            // `field_npc_draws` stay as the fallback for NPCs whose
            // clip or upload is unavailable this frame.
            let mut npc_posed: std::collections::HashMap<
                u8,
                (Option<UploadedVramMesh>, Option<UploadedColorMesh>),
            > = std::collections::HashMap::new();
            if self.session.host.world.mode == SceneMode::Field {
                for (slot, player) in self.npc_clip_players.iter_mut() {
                    let Some((tmd, raw)) = self.npc_anim_srcs.get(slot) else {
                        continue;
                    };
                    let pose = player.tick();
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
                        npc_posed.insert(*slot, (vm, cm));
                    }
                }
            }
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
            // behind publisher logos / title / save-select.
            let mut draws: Vec<SceneDraw<'_>> = Vec::new();
            // Untextured (F*/G*) field props, drawn on the colour
            // pipeline alongside the textured `draws`.
            let mut color_draws: Vec<ColorSceneDraw<'_>> = Vec::new();
            if self.boot_ui.is_active() {
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
                    });
                }
                for (mesh_idx, model) in self.world_map_terrain_draws.iter() {
                    if let Some(mesh) = self.meshes.get(*mesh_idx) {
                        draws.push(SceneDraw {
                            mesh,
                            mvp: cam * *model,
                        });
                    }
                }
                // Last-resort fallback: nothing resolved at all -> draw
                // the whole pack at pack-local coords so the map isn't
                // blank.
                if self.ground_heightfield.is_none() && self.world_map_terrain_draws.is_empty() {
                    for mesh in &self.meshes {
                        draws.push(SceneDraw { mesh, mvp: cam });
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
                    // Drawn ONCE, world-fixed - matching retail, which
                    // sets the dome up as a background **actor**
                    // (`FUN_800513F0`: `tmd_register` -> `DAT_8007C018[]`
                    // + `FUN_80020de0` actor_alloc + `FUN_80020f88` link)
                    // rendered by the normal actor path `FUN_80048A08`.
                    // The dome is a FRONT half (verts `Z in [-1260,
                    // +12155]`), so it is NOT a full surround: as the
                    // camera orbits, different portions of the front arc
                    // come into view and the rest of the horizon is open
                    // sky/grass. The retail captures bear this out -
                    // mountains cover only 44–81% of the horizon columns
                    // depending on angle (NOT a ring). Earlier engine
                    // builds added a 180° mirror to "complete" the
                    // surround; that over-fills what retail leaves as a
                    // gap, so it is removed. See
                    // `project_battle_backdrop_is_prot88_dome`.
                    if let Some(stage_idx) = self.battle_stage_mesh
                        && let Some(mesh) = self.meshes.get(stage_idx)
                    {
                        let flip = Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
                        // Front half-dome at raw world coords.
                        draws.push(SceneDraw {
                            mesh,
                            mvp: cam * flip,
                        });
                        // The dome geometry is a FRONT half (Z>=0), so a
                        // single instance leaves the back of the horizon
                        // open. Draw a 180-deg-Y mirror so the mountain
                        // ring + sky complete the full circle around the
                        // actors. (Retail's dome is a front half with
                        // partial coverage; the mirror reads fuller.)
                        let back = flip * Mat4::from_rotation_y(std::f32::consts::PI);
                        draws.push(SceneDraw {
                            mesh,
                            mvp: cam * back,
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
                        });
                    }
                    // Then the terrain / decor tile layer (drawn under
                    // the buildings): the `CELL_VISIBLE` field-map tiles
                    // (stone plaza, paths, riverbank).
                    if layer_on("tiles") {
                        for (mesh_idx, model) in &self.field_terrain_draws {
                            if let Some(mesh) = self.meshes.get(*mesh_idx) {
                                draws.push(SceneDraw {
                                    mesh,
                                    mvp: cam * *model,
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
                        for (mesh_idx, model) in &self.field_placement_draws {
                            if let Some(mesh) = self.meshes.get(*mesh_idx) {
                                draws.push(SceneDraw {
                                    mesh,
                                    mvp: cam * *model,
                                });
                            }
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
                    }
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
                        let y = w.sample_field_floor_height(x as i32, z as i32) as f32;
                        // Raw retail-convention transform (no model
                        // flip): the field camera's FIELD_WORLD_FLIP
                        // provides the single net Y negation. Walkers
                        // face their travel heading (12-bit, `0` =
                        // Z+, same convention + half-turn compose as
                        // the player's `render_26`); NPCs that never
                        // walked stay unrotated (no facing byte in
                        // the placement record).
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
                            }),
                            (None, Some(mi)) => {
                                if let Some(mesh) = self.meshes.get(mi) {
                                    draws.push(SceneDraw {
                                        mesh,
                                        mvp: cam * model,
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
                if in_battle
                    && let Some(gi) = self.battle_ground_mesh
                    && let Some(gmesh) = self.meshes.get(gi)
                {
                    let flip = Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
                    draws.push(SceneDraw {
                        mesh: gmesh,
                        mvp: cam * flip,
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
                    let mesh = posed_overrides
                        .get(tmd_idx)
                        .and_then(|o| o.as_ref())
                        .or_else(|| self.meshes.get(tmd_idx));
                    if let Some(mesh) = mesh {
                        draws.push(SceneDraw {
                            mesh,
                            mvp: actor_cam * self.actor_model(i),
                        });
                    }
                }
            }
            let hud = self.build_hud(w, h);
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
            // Slot-2 chrome samples the resident system-UI atlas.
            // Save-select pills/panel and the field-menu window
            // frame are mutually-exclusive boot states, so both
            // share this one vec (the field-menu frame draws
            // behind its text, which is emitted in the text layer).
            let mut save_chrome_draw_vec = self.save_select_chrome_sprite_draws(w, h);
            save_chrome_draw_vec.extend(self.field_menu_chrome_sprite_draws(w, h));
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

            // Force a pure-black background during boot UI so the
            // logos / title / save-select panels read on PSX-style
            // black instead of the default dark-blue clear. In a
            // stage-dome battle clear to a sky blue so the gaps the
            // front-half dome leaves open read as sky (like retail)
            // rather than the bare grey clear.
            let scene_clear = if self.boot_ui.is_active() {
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
            let sprites_slot_1 = if !logo_draw_vec.is_empty() {
                logo_overlay.as_ref()
            } else if !title_draw_vec.is_empty() {
                title_overlay.as_ref()
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
            // Effect-pool billboards: bridge live effect child sprites
            // into the renderer as faithful camera-facing quads sized
            // and UV-addressed from the effect bundle's inline atlas
            // (`World::active_effect_sprites`). Each draws two ways: a
            // textured quad sampling the scene VRAM at the sprite's
            // atlas page/clut/uv (the retail FUN_801E0088 pass-2 path -
            // invisible while the texel-source upload is unpinned, real
            // once it lands), plus a tinted outline through the Lines
            // pipeline so the spawn is visible now. See
            // docs/subsystems/effect-vm.md.
            let (effect_billboard, effect_lines) = if self.boot_ui.is_active() {
                (None, None)
            } else {
                let sprites = self.session.host.world.active_effect_sprites();
                if sprites.is_empty() {
                    (None, None)
                } else {
                    // Camera right/up in world space (clip-space basis
                    // dirs mapped back through the inverse MVP).
                    let inv = cam.inverse();
                    let right = inv.transform_vector3(Vec3::X).normalize_or_zero();
                    let up = inv.transform_vector3(Vec3::Y).normalize_or_zero();
                    let mesh = effect_billboard_mesh(r, &sprites, right, up);
                    let (pos, col, idx) = effect_sprite_line_geometry(&sprites, right, up);
                    let lines = match r.upload_lines(&pos, &col, &idx) {
                        Ok(m) => Some(m),
                        Err(e) => {
                            log::warn!("effect outline lines upload: {e:#}");
                            None
                        }
                    };
                    (mesh, lines)
                }
            };
            if let Some(mesh) = effect_billboard.as_ref() {
                draws.push(SceneDraw { mesh, mvp: cam });
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
            let world_map_entity_lines = if in_world_map && !self.boot_ui.is_active() {
                let markers = self.session.host.world.world_map_entity_markers();
                let mut pos: Vec<[f32; 3]> = Vec::new();
                let mut col: Vec<[u8; 4]> = Vec::new();
                let mut idx: Vec<u32> = Vec::new();
                if !markers.is_empty() {
                    let (p, c, i) = world_map_entity_line_geometry(
                        &markers,
                        self.scene_aabb.0,
                        self.scene_aabb.1,
                    );
                    pos = p;
                    col = c;
                    idx = i;
                }
                if let Some(player) = self.session.host.world.world_map_player_marker() {
                    let (p, c, i) = world_map_player_line_geometry(
                        &player,
                        self.scene_aabb.0,
                        self.scene_aabb.1,
                    );
                    let base = pos.len() as u32;
                    pos.extend(p);
                    col.extend(c);
                    idx.extend(i.into_iter().map(|v| v + base));
                }
                // Merge the env-gated slot-4 inspection wireframe (built
                // once at scene load) into the same overlay-lines buffer.
                if let Some((p, c, i)) = self.world_map_slot4_lines.as_ref() {
                    let base = pos.len() as u32;
                    pos.extend_from_slice(p);
                    col.extend_from_slice(c);
                    idx.extend(i.iter().map(|v| v + base));
                }
                if idx.is_empty() {
                    None
                } else {
                    match r.upload_lines(&pos, &col, &idx) {
                        Ok(m) => Some(m),
                        Err(e) => {
                            log::warn!("world-map overlay marker lines upload: {e:#}");
                            None
                        }
                    }
                }
            } else {
                None
            };
            // Effect 3D models (`etmd.dat`): spell effects like Tail
            // Fire are small Gouraud-shaded `etmd` meshes textured by
            // the resident `etim` texels, not billboards. Build a
            // per-frame VRAM mesh + transform for each live effect that
            // has a model assigned (same per-frame model-matrix
            // convention as `actor_model`). Held in a local Vec so the
            // meshes outlive the render borrow.
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
            let fx_cam = if fx_in_battle && self.battle_stage_mesh.is_some() {
                cam * Mat4::from_scale(Vec3::splat(BATTLE_WORLD_SCALE))
            } else {
                cam
            };
            let mut effect_model_draws: Vec<(UploadedVramMesh, Mat4)> = Vec::new();
            if !self.boot_ui.is_active() && !in_world_map {
                for em in self.session.host.world.active_effect_models() {
                    let Some(gtmd) = self
                        .session
                        .host
                        .world
                        .global_tmd(em.tmd_index as i16)
                        .map(std::sync::Arc::clone)
                    else {
                        continue;
                    };
                    let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(&gtmd.tmd, &gtmd.raw);
                    if vmesh.indices.is_empty() {
                        continue;
                    }
                    match r.upload_vram_mesh(
                        &vmesh.positions,
                        &vmesh.uvs,
                        &vmesh.cba_tsb,
                        &vmesh.normals,
                        &vmesh.indices,
                    ) {
                        Ok(m) => {
                            let model =
                                Mat4::from_translation(Vec3::from(em.world_pos)) * fx_model_flip;
                            effect_model_draws.push((m, model));
                        }
                        Err(e) => log::warn!("effect model mesh upload: {e:#}"),
                    }
                }
            }
            for (mesh, model) in &effect_model_draws {
                draws.push(SceneDraw {
                    mesh,
                    mvp: fx_cam * *model,
                });
            }

            // Active Seru-magic summon scene-graph (debug-spawned via
            // `G`): one textured mesh per move-VM-driven part, posed by
            // the part's interpreted transform (world pos + rotation
            // banks). The animation computation is faithful (move VM);
            // the transform composition is the open PROT 0900 piece.
            let mut summon_part_draws: Vec<(UploadedVramMesh, Mat4)> = Vec::new();
            if !self.boot_ui.is_active() && !in_world_map {
                // Summon parts and battle move-FX parts render identically
                // (move-VM scene-graph parts resolving into the battle
                // `global_tmd_pool` = PROT 0871 effect library). FIELD
                // move-VM effects are drawn separately below: their meshes
                // live in the SCENE's TMD pack, not the battle pool.
                let part_draws = self
                    .session
                    .host
                    .world
                    .active_summon_part_draws()
                    .into_iter()
                    .chain(self.session.host.world.active_move_fx_part_draws());
                for sp in part_draws {
                    let Some(gtmd) = self
                        .session
                        .host
                        .world
                        .global_tmd(sp.model_index as i16)
                        .map(std::sync::Arc::clone)
                    else {
                        continue;
                    };
                    let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(&gtmd.tmd, &gtmd.raw);
                    if vmesh.indices.is_empty() {
                        continue;
                    }
                    match r.upload_vram_mesh(
                        &vmesh.positions,
                        &vmesh.uvs,
                        &vmesh.cba_tsb,
                        &vmesh.normals,
                        &vmesh.indices,
                    ) {
                        Ok(m) => {
                            let model = Mat4::from_translation(Vec3::from(sp.world_pos))
                                * Mat4::from_rotation_y(sp.rot[1])
                                * Mat4::from_rotation_x(sp.rot[0])
                                * Mat4::from_rotation_z(sp.rot[2])
                                * fx_model_flip;
                            summon_part_draws.push((m, model));
                        }
                        Err(e) => log::warn!("summon/move-FX part mesh upload: {e:#}"),
                    }
                }
            }
            for (mesh, model) in &summon_part_draws {
                draws.push(SceneDraw {
                    mesh,
                    mvp: fx_cam * *model,
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
            let mut field_fx_draws: Vec<(UploadedVramMesh, Mat4)> = Vec::new();
            if !self.boot_ui.is_active() && !in_world_map {
                for fp in self.session.host.world.active_field_fx_part_draws() {
                    // `model_index` = the stager record's relative
                    // `model_sel` (spawn base 0) → the scene TMD pack
                    // (`field_stager_tmds`, the env_tmds / asset-viewer
                    // source = DAT_8007C018[5 + model_sel]).
                    let Some((tmd, raw)) = self.field_stager_tmds.get(fp.model_index) else {
                        continue;
                    };
                    let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(tmd, raw);
                    if vmesh.indices.is_empty() {
                        continue;
                    }
                    match r.upload_vram_mesh(
                        &vmesh.positions,
                        &vmesh.uvs,
                        &vmesh.cba_tsb,
                        &vmesh.normals,
                        &vmesh.indices,
                    ) {
                        Ok(m) => {
                            let model = Mat4::from_translation(Vec3::from(fp.world_pos))
                                * Mat4::from_rotation_y(fp.rot[1])
                                * Mat4::from_rotation_x(fp.rot[0])
                                * Mat4::from_rotation_z(fp.rot[2])
                                * fx_model_flip;
                            field_fx_draws.push((m, model));
                        }
                        Err(e) => log::warn!("field-FX part mesh upload: {e:#}"),
                    }
                }
            }
            for (mesh, model) in &field_fx_draws {
                draws.push(SceneDraw {
                    mesh,
                    mvp: fx_cam * *model,
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
            let mut screen_fx_solid = None;
            let mut screen_fx_tex = None;
            let fx_frame = &self.session.host.world.screen_fx_frame;
            if !fx_frame.is_empty() {
                let quad = |pos: &mut Vec<[f32; 3]>,
                            idx: &mut Vec<u32>,
                            l: f32,
                            t: f32,
                            rr: f32,
                            b: f32,
                            z: f32| {
                    let base = pos.len() as u32;
                    pos.push([l, t, z]);
                    pos.push([rr, t, z]);
                    pos.push([l, b, z]);
                    pos.push([rr, b, z]);
                    idx.extend_from_slice(&[
                        base,
                        base + 1,
                        base + 2,
                        base + 1,
                        base + 3,
                        base + 2,
                    ]);
                };
                // Solid quads (mask borders + letterbox bands): flat black.
                let mut pos = Vec::new();
                let mut idx = Vec::new();
                for q in &fx_frame.solid_quads {
                    if q.right <= q.left || q.bottom <= q.top {
                        continue;
                    }
                    quad(
                        &mut pos,
                        &mut idx,
                        q.left as f32,
                        q.top as f32,
                        q.right as f32,
                        q.bottom as f32,
                        -0.002,
                    );
                }
                if !idx.is_empty() {
                    let colors = vec![[0u8, 0, 0]; pos.len()];
                    match r.upload_color_mesh(&pos, &colors, &idx) {
                        Ok(m) => screen_fx_solid = Some(m),
                        Err(e) => log::warn!("screen-fx solid mesh upload: {e:#}"),
                    }
                }
                // Textured quads: panels (15bpp direct pages) + sprites
                // (clut-indexed), one shared mesh with per-kind depth.
                let mut pos = Vec::new();
                let mut uvs: Vec<[u8; 2]> = Vec::new();
                let mut cba_tsb: Vec<[u16; 2]> = Vec::new();
                let mut idx = Vec::new();
                for p in &fx_frame.panels {
                    if p.right <= p.left || p.bottom <= p.top {
                        continue;
                    }
                    let base = pos.len();
                    quad(
                        &mut pos,
                        &mut idx,
                        p.left as f32,
                        p.top as f32,
                        p.right as f32,
                        p.bottom as f32,
                        -0.001,
                    );
                    uvs.extend_from_slice(&[
                        [p.u0, p.v0],
                        [p.u1, p.v0],
                        [p.u0, p.v1],
                        [p.u1, p.v1],
                    ]);
                    cba_tsb.extend(std::iter::repeat_n([0u16, p.texpage], pos.len() - base));
                }
                for s in &fx_frame.sprites {
                    if s.w <= 0 || s.h <= 0 {
                        continue;
                    }
                    let base = pos.len();
                    quad(
                        &mut pos,
                        &mut idx,
                        s.x as f32,
                        s.y as f32,
                        (s.x + s.w) as f32,
                        (s.y + s.h) as f32,
                        0.0,
                    );
                    let u1 = (s.u as i32 + s.w as i32 - 1).min(255) as u8;
                    let v1 = (s.v as i32 + s.h as i32 - 1).min(255) as u8;
                    uvs.extend_from_slice(&[[s.u, s.v], [u1, s.v], [s.u, v1], [u1, v1]]);
                    cba_tsb.extend(std::iter::repeat_n(
                        [s.clut as u16, s.texpage as u16],
                        pos.len() - base,
                    ));
                }
                if !idx.is_empty() {
                    let normals = vec![[0.0f32; 3]; pos.len()];
                    match r.upload_vram_mesh(&pos, &uvs, &cba_tsb, &normals, &idx) {
                        Ok(m) => screen_fx_tex = Some(m),
                        Err(e) => log::warn!("screen-fx textured mesh upload: {e:#}"),
                    }
                }
            }
            let screen_fx_mvp = Mat4::orthographic_rh(0.0, 320.0, 240.0, 0.0, 0.0, 1.0);
            if let Some(m) = &screen_fx_tex {
                draws.push(SceneDraw {
                    mesh: m,
                    mvp: screen_fx_mvp,
                });
            }
            if let Some(m) = &screen_fx_solid {
                color_draws.push(ColorSceneDraw {
                    mesh: m,
                    mvp: screen_fx_mvp,
                });
            }
            // Field colour fade (op 0x34 sub-0, e.g. the opening white
            // flash): a full-screen wash of the fade colour. The PSX
            // pipeline blends per-ABR-mode (no free alpha) and the
            // retail fade-actor draw handler isn't dumped, so this is
            // an approximation - a 50%-average (ABR 0) wash drawn while
            // the ramp still has coverage; it lifts when the fade
            // completes (`World::color_fade` drops). Held here so it
            // outlives `screen_fx_solid`'s borrow.
            let mut color_fade_mesh = None;
            if let Some(cf) = &self.session.host.world.color_fade
                && cf.coverage() > 0.15
            {
                let rgb = cf.rgb();
                let pos = vec![
                    [0.0f32, 0.0, -0.003],
                    [320.0, 0.0, -0.003],
                    [0.0, 240.0, -0.003],
                    [320.0, 240.0, -0.003],
                ];
                let colors = vec![rgb; 4];
                let idx = [0u32, 1, 2, 1, 3, 2];
                let blend = vec![legaia_engine_render::psx_blend::pack_blend_word(true, 0); 4];
                match r.upload_color_mesh_blended(&pos, &colors, &idx, &blend) {
                    Ok(m) => color_fade_mesh = Some(m),
                    Err(e) => log::warn!("color-fade wash upload: {e:#}"),
                }
            }
            if let Some(m) = &color_fade_mesh {
                color_draws.push(ColorSceneDraw {
                    mesh: m,
                    mvp: screen_fx_mvp,
                });
            }
            let scene = RenderScene {
                vram,
                draws: &draws,
                color_draws: &color_draws,
                overlay_lines: world_map_entity_lines
                    .as_ref()
                    .or(effect_lines.as_ref())
                    .map(|m| (m, cam)),
                overlay_sprites: sprites_slot_1,
                overlay_sprites_2: sprites_slot_2,
                overlay_text: Some(&overlay),
                clear_color: scene_clear,
            };
            if let Err(e) = r.render(RenderTarget::Scene(&scene)) {
                log::error!("render: {e:#}");
            }
        }
        self.win.request_redraw();
    }
}

//! The redraw's render-state staging, mesh-upload drains and per-frame
//! skinning - the steps of the render block that run before the draw lists
//! borrow the uploaded meshes - split out of `handle_redraw`.
//!
//! Each step re-borrows the renderer itself: the render block only enters
//! when the renderer, the scene VRAM and the font atlas are all present.

use super::super::*;
use super::redraw_passes::CutsceneCam;

/// What [`PlayWindowApp::stage_frame_render_state`] resolved for the frame.
pub(super) struct FrameView {
    /// Surface size in pixels.
    pub w: u32,
    pub h: u32,
    /// The scene camera (view-projection) this frame draws with.
    pub cam: Mat4,
    /// The frame is the world map's.
    pub in_world_map: bool,
}

/// The frame's skinned and re-posed meshes, owned by the frame.
pub(super) struct PosedFrame {
    pub posed_overrides: Vec<Option<UploadedVramMesh>>,
    pub player_color_posed: Option<UploadedColorMesh>,
    pub battle_ghost_uploads: Vec<(UploadedColorMesh, Mat4, [f32; 3], [f32; 3])>,
    pub posed_prop_baked_v: Vec<(usize, Mat4, Option<usize>)>,
    pub posed_prop_baked_c: Vec<(usize, Mat4, Option<usize>)>,
    pub posed_prop_live_v: Vec<(UploadedVramMesh, Mat4, Option<usize>)>,
    pub posed_prop_live_c: Vec<(UploadedColorMesh, Mat4, Option<usize>)>,
    /// `(slot, pose key)` of each placed NPC's clip frame this redraw shows,
    /// the key into `npc_pose_cache`.
    pub npc_frames: Vec<(u8, usize)>,
}

impl PlayWindowApp {
    /// Stage the renderer's per-frame state: the stage viewport, the caption
    /// atlas, the colour grade and depth cue, the NCLIP / curvature /
    /// near-reject knobs, the scene camera, the volumetric fog, the lighting
    /// mood and scene lights, and the camera-occlusion fade.
    pub(super) fn stage_frame_render_state(
        &mut self,
        cutscene_cam: Option<CutsceneCam>,
    ) -> FrameView {
        let r = self
            .win
            .renderer
            .as_ref()
            .expect("render gate: renderer present");
        let (w, h) = r.surface_size();
        // The 3D pass draws into the 2D stage rect at the stage's 4:3,
        // so it lines up with every stage-anchored draw at any window
        // size (`scene_viewport_for`); the browser canvas is a stage.
        let (scene_viewport, aspect) = scene_viewport_for(w, h);
        r.set_scene_viewport(scene_viewport);
        // Upload (or drop) the opdeene "It was the Seru." caption sprite
        // atlas to track World state. The caption image is present only
        // while opdeene is loaded and never changes, so upload it once on
        // first sight and drop it when the scene clears it (scene change).
        // Disjoint fields: `r` borrows `win.renderer`, the image lives under
        // `session`, the cache is `caption_atlas`.
        if self.caption_atlas.is_none()
            && let Some(cap) = self.session.host.world.cutscene.caption.as_ref()
        {
            match r.upload_sprite_atlas(&cap.rgba, cap.width, cap.height) {
                Ok(atlas) => self.caption_atlas = Some((atlas, cap.width, cap.height)),
                Err(e) => log::warn!("caption atlas upload: {e:#}"),
            }
        } else if self.caption_atlas.is_some() && self.session.host.world.cutscene.caption.is_none()
        {
            self.caption_atlas = None;
        }
        // Full-scene colour grade: the opening prologue cutscene
        // (`opdeene`, "It was the Seru.") renders its whole 3D scene in
        // warm gold sepia (dim ambient + gold far-colour depth cue in
        // retail); every other scene, incl. the Rim Elm hand-off, is
        // natural colour. Staged every frame so it clears on transition.
        //
        // The op `0x4C 0x12` word (`_DAT_8007BCB8..BA`) is NOT a frame
        // multiply: its one reader disc-wide is the fog particle update
        // `FUN_8003F3FC` (see `docs/subsystems/field-ambient-fx.md`), so
        // the scene's own pixels never take it - `retona_field_card_boot`
        // holds the word at 27 over a full-brightness frame. The fog
        // sheets take it through `World::fog_render_step`.
        match self.session.host.world.scene_color_grade() {
            // Prologue grade: staged as the renderer's PALETTE-COLLAPSE
            // mode - the retail mechanism's true altitude (the scene's
            // uploaded CLUTs are rewritten to the gold law and the
            // resident TMD colour words by the two `4C E6` HSV ops; the
            // engine's shaders apply the identical laws per texel /
            // packet colour, `prologue_sepia_word`). The view-depth cue
            // ramp is inert in this mode (retail's prologue nodes hold
            // `IR0 = 0`).
            Some(g) => {
                r.set_color_grade(g.gold, g.strength);
                r.set_palette_grade([1.0; 3], true);
            }
            None => {
                r.set_color_grade([1.0, 1.0, 1.0], 0.0);
                r.set_palette_grade([1.0; 3], false);
            }
        }
        // The grade's second half: the per-render-node DPCS far-colour
        // pull (gold far colour + depth-graded IR0 in retail), staged as
        // a view-depth IR0 ramp. Cleared every non-prologue frame, so
        // interactive scenes render the identity (ramp-off) path.
        match self.session.host.world.scene_depth_cue() {
            Some(c) => r.set_depth_cue_ramp(c.far, c.near_z, c.far_z, c.max_ir0),
            None => r.clear_depth_cue_ramp(),
        }
        // Retail GTE NCLIP winding rejection over the whole field pass
        // (`camera_view::nclip_cull_mode`): retail culls the back faces
        // of every field mesh, which is what hides a sky dome's outer
        // shell (korout, retona) and the opdeene prologue shot's near
        // cave wall. The field frame draws raw retail vertices under a
        // camera-side Y-flip, which mirrors the projected winding, so
        // retail's front faces arrive CW - mode 2 (discard front-facing
        // = discard CCW under the pipelines' default Ccw front-face)
        // keeps them. The world map, battle and the minigame venues keep
        // both-sided draws (their per-pass winding parities differ).
        let nclip_mode = legaia_engine_core::camera_view::nclip_cull_mode(
            cutscene_cam.is_some(),
            self.session.host.world.mode,
        );
        r.set_backface_cull(nclip_mode);
        // The overworld's per-vertex screen-Y bend (`FUN_800271A8`'s
        // table, applied by retail's overworld prim leaves), scaled for
        // this frame's camera - the same kernel the browser play page
        // stages `u_curve` from.
        r.set_overworld_curvature(self.overworld_curve_scale(cutscene_cam));
        // Retail's per-primitive near reject (`camera_view::prim_near_cut`):
        // a primitive whose mean corner depth sits near or behind the eye
        // is not drawn, where a per-pixel clip would paint it across the
        // frame. Only under a retail camera - the field debug orbit and
        // the stage-less battle framing are vantages retail never had.
        let retail_camera = match self.session.host.world.mode {
            SceneMode::Battle => self.battle_stage_mesh.is_some(),
            _ => !self.field_debug_camera,
        } && std::env::var_os("LEGAIA_DIAG_NO_PRIM_NEAR").is_none();
        r.set_prim_near_reject(
            legaia_engine_core::camera_view::prim_near_cut(
                self.session.host.world.mode,
                retail_camera,
            ),
            legaia_engine_core::camera_view::prim_gpu_span_h(
                self.session.host.world.mode,
                retail_camera,
                self.session.camera.globals.0[9] as f32,
            ),
        );
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
        // The volumetric ground-fog enhancement (`engine-core::fog_volume`,
        // `F9` / `--no-volumetric-fog`): the engine's bank for this tick,
        // drawn after the 3D scene and before the HUD. Its space is the
        // field's retail Y-down world (`cam` already carries the field
        // frame's Y negation) or the raw battle stage (the stage model's
        // scale + Y-flip). Staged every frame; `None` stages nothing.
        self.stage_fog_volume(r, cam, in_world_map);
        // Enhanced lighting's mood: the persisted time of day over the
        // loaded scene (`scene_lighting::TimeOfDay::mood` - the same call
        // the browser play page makes). Cheap; staged every frame so a
        // scene change or an `F8` cycle lands on the next frame.
        let mood = self.lighting_mood();
        r.set_lighting_mood(mood);
        // Stage the derived scene point lights (the dynamic-lighting
        // enhancement's candle / wall-light layer) with this frame's
        // camera so the renderer can recover world space from the
        // per-draw MVPs. Field free-roam only - battle / world map /
        // boot UI clear them so the layer never lights the wrong
        // coordinate space. Inert (zero staged count, no shadow pass)
        // while dynamic lighting or the shadow sub-toggle is off.
        // A menu-overlay screen that owns the frame (a shop, the casino
        // prize counter) draws no field, so it stages no field light
        // either: the halos are screen sprites and would otherwise glow
        // through the black behind the windows.
        if !self.boot_ui.is_active()
            && !self.menu_runtime.covers_field()
            && !in_world_map
            && self.session.host.world.mode == SceneMode::Field
            && !(self.scene_point_lights.is_empty() && self.scene_prop_lights.is_empty())
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
            // The static lights plus every prop's set at the actor's
            // live position (the same anchor the NPC draw uses).
            let mut all = self.scene_point_lights.clone();
            all.extend(legaia_engine_render::scene_lighting::place_prop_lights(
                &self.scene_prop_lights,
                |slot, spawn| w.field_npc_live_anchor(slot, spawn),
            ));
            let picked = legaia_engine_render::scene_lights::nearest_lights(&all, focus);
            r.set_scene_lights(&picked, cam);
            // Halos + soft light shafts around the picked lights (the
            // bloom stand-in), scaled by the mood's glow.
            r.set_glow_sprites(&legaia_engine_render::scene_lighting::glow_sprites(
                &picked, &mood,
            ));
        } else {
            r.clear_scene_lights();
            r.set_glow_sprites(&[]);
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
        // The world half of the gate is the shared kernel
        // (`field_occlusion::fade_armed`: field mode, no scripted shot);
        // what stays here is genuinely this host's - its master toggle,
        // a boot / pause panel owning the screen, and the `F3` debug
        // vantage. The browser play page reads the same split.
        let occl_focus = (self.occlusion_fade
            && !self.boot_ui.is_active()
            && !self.field_debug_camera
            && legaia_engine_core::field_occlusion::fade_armed(
                &self.session.host.world,
                cutscene_cam.is_some(),
            ))
        .then(|| legaia_engine_core::field_occlusion::player_body_centre(&self.session.host.world))
        .flatten();
        let mut occl_staged = false;
        if let Some(centre) = occl_focus {
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
                let scale_y =
                    legaia_engine_render::occlusion_fade::view_proj_scale_y(&cam.to_cols_array());
                // The floor point under the character anchors the
                // feet-line rule: nothing below it on screen fades.
                let feet =
                    legaia_engine_core::field_occlusion::player_feet(&self.session.host.world)
                        .unwrap_or(centre);
                let feet_clip = cam * Vec4::new(feet[0], feet[1], feet[2], 1.0);
                r.set_occlusion_focus(clip.to_array(), feet_clip.to_array(), s, scale_y);
                occl_staged = true;
            }
        } else {
            self.occl_fade_strength.set(0.0);
        }
        if !occl_staged {
            r.clear_occlusion_focus();
        }
        FrameView {
            w,
            h,
            cam,
            in_world_map,
        }
    }

    /// Upload the meshes queued for this frame: the tile-board actors, the
    /// spawn-record actors' `tmd_ref` meshes and the morph-weight actors'
    /// re-blended meshes.
    pub(super) fn drain_frame_mesh_uploads(&mut self) {
        let r = self
            .win
            .renderer
            .as_ref()
            .expect("render gate: renderer present");
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
            if world.board.draw_list.is_empty() {
                self.tile_slots_queued.clear();
            } else {
                for slot in crate::tile_board_draws::tile_actor_slots_needing_mesh(world) {
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
        // Morph-weight actors (the same `0x4C 0xD8` allocator, seated by
        // `World::spawn_morph_weight_actor`): re-pose and re-upload each
        // one every frame. Retail re-blends in the handler call itself
        // (`FUN_8002174C` restores the `+0x90` rest pose and re-applies
        // the deltas at the live `+0x6E` weight before every draw), and
        // the envelope is a ping-pong ramp that moves on every frame, so
        // there is no frame where a cached upload would still be current.
        // The blend is the engine's - `World::morph_weight_posed_tmd` is
        // the one kernel, shared with the browser play page.
        for (slot, _weight) in self.session.host.world.morph_weight_actor_weights() {
            let Some(mesh_idx) = self
                .session
                .host
                .world
                .actors
                .get(slot as usize)
                .and_then(|a| a.tmd_binding)
            else {
                continue;
            };
            let Some((posed, raw, _)) = self
                .session
                .host
                .world
                .morph_weight_posed_tmd(slot as usize)
            else {
                continue;
            };
            let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(&posed, &raw);
            if vmesh.indices.is_empty() {
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
                    if let Some(entry) = self.meshes.get_mut(mesh_idx) {
                        *entry = m;
                    }
                }
                Err(e) => log::warn!("morph mesh upload: {e:#}"),
            }
        }
    }

    /// Per-frame skinning: the posed actors, the arts ghosts, the posed
    /// props, the VDF / NPC morph uploads and the placed NPCs' clip poses
    /// (memoised in `npc_pose_cache`).
    pub(super) fn pose_frame_meshes(
        &mut self,
        field_morph_rebuilds: Vec<(usize, legaia_tmd::mesh::VramMesh)>,
        npc_morph_rebuilds: Vec<(
            u8,
            Option<(legaia_tmd::mesh::VramMesh, legaia_tmd::mesh::ColorMesh)>,
        )>,
        field_tail_ticks: u32,
    ) -> PosedFrame {
        let r = self
            .win
            .renderer
            .as_ref()
            .expect("render gate: renderer present");
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
                if let Some(d) = self.draw_census.as_mut() {
                    d.morph.insert(
                        *mesh_idx,
                        legaia_engine_core::draw_census::MeshCensus::from_mesh(
                            &vmesh.positions,
                            &vmesh.cba_tsb,
                            &vmesh.colors,
                            &vmesh.indices,
                        ),
                    );
                }
            }
        }
        for (slot, halves) in npc_morph_rebuilds {
            let Some((vmesh, cmesh)) = halves else {
                self.npc_morph_static.remove(&slot);
                continue;
            };
            let vm = (!vmesh.indices.is_empty())
                .then(|| {
                    r.upload_vram_mesh(
                        &vmesh.positions,
                        &vmesh.uvs,
                        &vmesh.cba_tsb,
                        &vmesh.normals,
                        &vmesh.colors,
                        &vmesh.indices,
                    )
                    .ok()
                })
                .flatten();
            let cm = (!cmesh.is_empty())
                .then(|| {
                    r.upload_color_mesh_blended(
                        &cmesh.positions,
                        &cmesh.colors,
                        &cmesh.indices,
                        &cmesh.blend,
                    )
                    .ok()
                })
                .flatten();
            self.npc_morph_static.insert(slot, (vm, cm));
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
        if self.session.host.world.field_npc_clips_advance() {
            // (The `A2` / `4C 51` cue drain that re-targets these
            // players runs per sim tick in the loop above -
            // `Self::drain_anim_cues`.)
            let verify = std::env::var_os("LEGAIA_POSE_CACHE_VERIFY").is_some();
            let cache = &mut self.npc_pose_cache;
            let verify_poses = &mut self.npc_pose_verify;
            let srcs = &self.npc_anim_srcs;
            let world = &self.session.host.world;
            let tag_vram = self.cpu_vram_base.as_ref();
            for (slot, player) in self.npc_clip_players.iter_mut() {
                let Some((tmd, raw)) = srcs.get(slot) else {
                    continue;
                };
                // A slot the world drives poses off its own cursor (the
                // actor's `+0x62` word holds a chest lid shut / open);
                // anything else free-runs as before.
                let world_driven = world.sync_npc_clip(*slot, player);
                // `pose_key()` is the pose this redraw shows (`frame * 16`
                // plus the sub-frame a blend-gated clip poses in between);
                // take it as the cache key and read its pose WITHOUT moving
                // the playhead, then advance by the sim ticks this redraw
                // ran (0 on a pure-refresh frame, so a 144 Hz display holds
                // each frame for the same wall-clock time a 60 Hz one does).
                // A script's look rotation (`4C 45`) turns one object -
                // the head - on top of the keyframe, so it is part of
                // the pose the cache keys: its angles ride the key's
                // high bits.
                let look = world.actor_look(legaia_engine_core::actor_look::LookKey::Npc(*slot));
                let look_bits = look.map_or(0usize, |l| {
                    let a = l.angles.map(|v| v as u16 as usize);
                    (1 << 63)
                        | ((l.object as u16 as usize & 0xFF) << 52)
                        | ((a[0] & 0xFFF) << 40)
                        | ((a[1] & 0xFFF) << 28)
                        | ((a[2] & 0xFFF) << 16)
                });
                let key = (*slot, player.pose_key() | look_bits);
                let mut pose = player.current_pose();
                if let Some(l) = look {
                    legaia_engine_core::actor_look::apply_look(&mut pose.bone_outputs, l);
                }
                if !world_driven {
                    player.advance(field_tail_ticks);
                }
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
                // An op-`0x4B` morph re-stages the mesh before the
                // skin (`FUN_8001C604` runs per group ahead of the
                // bone transform); a morph change drops the slot's
                // cache entries (`take_npc_morph_rebuilds`).
                let morphed = world.npc_morphed_tmd(*slot, tmd);
                let tmd = morphed.as_ref().unwrap_or(tmd);
                // The retail count-equality contract: an actor draws as
                // many objects as its clip has bones. A slot bound at
                // upload was already cut; one whose first clip came from
                // a later cue is cut here.
                let cut;
                let tmd = if tmd.objects.len() > pose.bone_outputs.len() {
                    let mut t = tmd.clone();
                    t.objects.truncate(pose.bone_outputs.len());
                    cut = t;
                    &cut
                } else {
                    tmd
                };
                let mut vmesh =
                    legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(tmd, raw, &pose.bone_outputs);
                let mut cmesh =
                    legaia_tmd::mesh::tmd_to_color_mesh_posed_rot(tmd, raw, &pose.bone_outputs);
                // Enhanced lighting's emissive tags, as the spawn build
                // set them (a re-pose would otherwise drop them).
                if let Some(v) = tag_vram {
                    legaia_engine_render::scene_lighting::tag_emissive_meshes(
                        raw, &mut vmesh, &mut cmesh, v,
                    );
                }
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
        PosedFrame {
            posed_overrides,
            player_color_posed,
            battle_ghost_uploads,
            posed_prop_baked_v,
            posed_prop_baked_c,
            posed_prop_live_v,
            posed_prop_live_c,
            npc_frames,
        }
    }
}

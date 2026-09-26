//! Per-frame render-pass builders for `handle_redraw`, extracted from
//! `redraw.rs` (mechanical split; behavior-preserving). Each method moves a
//! self-contained, read-only render-pass block verbatim out of the monolithic
//! redraw handler and returns the owned GPU resources / matrices it produced.

use super::super::*;

/// Decoded cutscene camera inputs to the retail PSX GTE model:
/// `(focus, pitch_radians, yaw_radians, roll_radians, h, tr_eye)`. See
/// [`PlayWindowApp::cutscene_view`].
pub(in crate::window) type CutsceneCam = ([f32; 3], f32, f32, f32, f32, [f32; 3]);

impl PlayWindowApp {
    /// This frame's scene camera, raw retail Y-down world frame.
    ///
    /// **Which camera owns the frame is the shared resolver's answer**
    /// (`camera_view::resolve_field_camera`), the same call the browser play
    /// page makes: a scripted op-`0x45` shot wins over every mode-derived
    /// camera (the world map included - map01's opening leg is the retail
    /// Rim Elm aerial fly-in; the caller only passes `cutscene_cam` in
    /// world-map mode when the running timeline actually staged camera
    /// params), then the two world-map vantages, then the field follow
    /// camera, and `HostDebugOrbit` when there is no player to follow. The
    /// matrix is `camera_view::frame_vp` for every one of those arms.
    ///
    /// Two arms stay host-side, and neither is a second answer to the
    /// resolver's question:
    ///
    /// - **Battle.** The resolver covers walkable scenes only; the battle
    ///   camera is its own kernel (`battle_cam_script::battle_vp`, stepped by
    ///   `window::battle_cam` against the battle phase model), and the page
    ///   runs that kernel too. Folding it in would mean a `FieldCameraFrame`
    ///   variant carrying the battle phase state, which buys no sharing the
    ///   kernel does not already give.
    /// - **The `F3` debug orbit and `HostDebugOrbit`.** Both are this host's
    ///   own vantage (`camera_mvp`); the page has its own orbit with the same
    ///   three knobs.
    ///
    /// `FIELD_WORLD_FLIP` cancels the resolver's Y-up frame, so the whole
    /// composition runs on raw retail Y-down world coordinates.
    pub(in crate::window) fn compute_scene_camera(
        &self,
        aspect: f32,
        in_world_map: bool,
        cutscene_cam: Option<CutsceneCam>,
    ) -> Mat4 {
        let world = &self.session.host.world;
        if cutscene_cam.is_none() && world.mode == SceneMode::Battle {
            // Stage-dome battle: the retail phase-scripted camera; with no
            // stage, the animated enemies. One selector, shared with the FX
            // passes.
            return self.battle_scene_mvp(aspect);
        }
        if cutscene_cam.is_none() && !in_world_map && self.field_debug_camera {
            // Wide debug orbit vantage (`F3` toggles), in the same
            // one-world-negation field frame as the follow camera.
            return self.camera_mvp(aspect) * FIELD_WORLD_FLIP;
        }
        let cutscene = cutscene_cam.map(|(focus, pitch, yaw, roll, h, tr_eye)| {
            legaia_engine_vm::psx_camera::FieldCameraView {
                focus,
                pitch,
                yaw,
                roll,
                h,
                tr_eye,
            }
        });
        let frame = legaia_engine_core::camera_view::resolve_field_camera(
            world,
            &self.session.camera,
            cutscene,
            [
                (self.scene_aabb.0[0] + self.scene_aabb.1[0]) * 0.5,
                (self.scene_aabb.0[2] + self.scene_aabb.1[2]) * 0.5,
            ],
        );
        let vp = legaia_engine_core::camera_view::frame_vp(
            &frame,
            (self.scene_aabb.0, self.scene_aabb.1),
            aspect,
        )
        .map(|m| Mat4::from_cols_array(&m))
        .unwrap_or_else(|| self.camera_mvp(aspect));
        vp * FIELD_WORLD_FLIP
    }

    /// The overworld curvature's `clip.w`-to-`SZ` factor for this frame's
    /// scene pass (`overworld_curvature::frame_curve_scale`), over the same
    /// frame [`Self::compute_scene_camera`] resolves - `0.0` off the kingdom
    /// overworld or under a camera with no retail eye. The browser play page
    /// stages its `u_curve` from the same kernel.
    pub(in crate::window) fn overworld_curve_scale(
        &self,
        cutscene_cam: Option<CutsceneCam>,
    ) -> f32 {
        let world = &self.session.host.world;
        if !world.overworld_bit() {
            return 0.0;
        }
        let cutscene = cutscene_cam.map(|(focus, pitch, yaw, roll, h, tr_eye)| {
            legaia_engine_vm::psx_camera::FieldCameraView {
                focus,
                pitch,
                yaw,
                roll,
                h,
                tr_eye,
            }
        });
        let frame = legaia_engine_core::camera_view::resolve_field_camera(
            world,
            &self.session.camera,
            cutscene,
            [
                (self.scene_aabb.0[0] + self.scene_aabb.1[0]) * 0.5,
                (self.scene_aabb.0[2] + self.scene_aabb.1[2]) * 0.5,
            ],
        );
        legaia_engine_core::overworld_curvature::frame_curve_scale(true, &frame)
    }

    pub(super) fn build_posed_actor_overrides(
        &self,
        r: &legaia_engine_render::Renderer,
    ) -> (Vec<Option<UploadedVramMesh>>, Option<UploadedColorMesh>) {
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
            let is_field_player =
                player_slot == Some(ai as u8) && self.player_color_draw.map(|(_, s)| s) == Some(ai);
            let mut vmesh = if actor.battle_animation.is_some() || player_slot == Some(ai as u8) {
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
            // Rot limb dimming (`FUN_80048A08`), the browser page's twin in
            // `web-viewer::play_battle_limb_dim`.
            if actor.battle_animation.is_some() {
                self.session
                    .host
                    .world
                    .dim_posed_battle_mesh(ai, tmd, raw, &mut vmesh.colors);
            }
            if std::env::var_os("LEGAIA_DIAG_POSE").is_some() {
                let (lo, hi) = vmesh.aabb();
                log::info!(
                    "DIAG pose: actor {ai} tmd {tmd_idx} verts {} anim {:?} \
                         world ({},{},{}) aabb {lo:?}..{hi:?} bones[0..2]={:?}",
                    vmesh.positions.len(),
                    actor
                        .battle_animation
                        .as_ref()
                        .map(|p| (p.action_id(), p.current_frame())),
                    actor.move_state.world_x,
                    actor.move_state.world_y,
                    actor.move_state.world_z,
                    &pose.bone_outputs[..pose.bone_outputs.len().min(2)]
                );
            }
            match r.upload_vram_mesh(
                &vmesh.positions,
                &vmesh.uvs,
                &vmesh.cba_tsb,
                &vmesh.normals,
                &vmesh.colors,
                &vmesh.indices,
            ) {
                Ok(m) => posed_overrides[tmd_idx] = Some(m),
                Err(e) => log::warn!("posed mesh upload: {e:#}"),
            }
        }
        (posed_overrides, player_color_posed)
    }

    /// `view_scale` is the uniform scale `cam` composes ahead of the
    /// projection (`BATTLE_WORLD_SCALE` on the battle stage, `1.0` elsewhere).
    /// It sizes the quads: retail adds the half-extents in view space, so they
    /// must not go through that scale - see `effect_sprite_corners`.
    pub(super) fn build_effect_billboards(
        &self,
        r: &legaia_engine_render::Renderer,
        cam: Mat4,
        view_scale: f32,
    ) -> (
        Option<UploadedVramMesh>,
        Option<legaia_engine_render::UploadedLines>,
    ) {
        if self.boot_ui.is_active() {
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
                let mesh = effect_billboard_mesh(r, &sprites, right, up, view_scale);
                // The wireframe outline is a **diagnostic**, off by default
                // (`LEGAIA_DIAG_FX=1`). It exists to make a spawn readable
                // when its texels are not resident, and it predates the
                // battle-entry flame-atlas blit; with the atlas resident the
                // textured quad draws, and leaving the outline on stamps a
                // bright rectangle over every effect in normal play.
                let lines = if std::env::var_os("LEGAIA_DIAG_FX").is_some() {
                    let (pos, col, idx) =
                        effect_sprite_line_geometry(&sprites, right, up, view_scale);
                    match r.upload_lines(&pos, &col, &idx) {
                        Ok(m) => Some(m),
                        Err(e) => {
                            log::warn!("effect outline lines upload: {e:#}");
                            None
                        }
                    }
                } else {
                    None
                };
                (mesh, lines)
            }
        }
    }

    /// The env-gated slot-4 inspection wireframe (`LEGAIA_WORLDMAP_SLOT4`),
    /// built once at scene load, as this frame's overlay lines. A diagnostic,
    /// not a render path: the entity and player markers left this line pass
    /// for the shared screen-prim kernel ([`Self::world_map_marker_prims`]).
    pub(super) fn build_world_map_overlay_lines(
        &self,
        r: &legaia_engine_render::Renderer,
        in_world_map: bool,
    ) -> Option<legaia_engine_render::UploadedLines> {
        if !in_world_map || self.boot_ui.is_active() {
            return None;
        }
        let (p, c, i) = self.world_map_slot4_lines.as_ref()?;
        if i.is_empty() {
            return None;
        }
        match r.upload_lines(p, c, i) {
            Ok(m) => Some(m),
            Err(e) => {
                log::warn!("world-map slot-4 inspection lines upload: {e:#}");
                None
            }
        }
    }

    /// The overworld's entity and player markers as screen primitives,
    /// through the kernel the browser play page calls
    /// (`legaia_engine_core::world_map_markers`) and the shared
    /// `world_map_marker_prim` wrapper. The camera is the resolver's
    /// non-scripted frame; the kernel itself draws nothing under a scripted
    /// shot (the map01 fly-in shows the bare continent).
    ///
    /// The player's marker is a stand-in: it only draws while the party
    /// leader's real mesh could NOT be uploaded (the world-map draw branch
    /// renders that mesh whenever the upload succeeded, and drawing both
    /// would stamp a post through the character).
    pub(super) fn world_map_marker_prims(
        &self,
    ) -> Vec<legaia_engine_render::screen_overlay::ScreenPrim> {
        let world = &self.session.host.world;
        if world.mode != SceneMode::WorldMap || self.boot_ui.is_active() {
            return Vec::new();
        }
        let aabb = (self.scene_aabb.0, self.scene_aabb.1);
        let frame = legaia_engine_core::camera_view::resolve_field_camera(
            world,
            &self.session.camera,
            None,
            [(aabb.0[0] + aabb.1[0]) * 0.5, (aabb.0[2] + aabb.1[2]) * 0.5],
        );
        let player_mesh_drawn = world
            .player_actor_slot
            .is_some_and(|pslot| self.drained_spawn_slots.contains(&pslot));
        legaia_engine_core::world_map_markers::marker_quads(world, &frame, aabb, !player_mesh_drawn)
            .iter()
            .map(|q| {
                legaia_engine_render::screen_overlay::world_map_marker_prim(q.xy, q.rgba, q.depth)
            })
            .collect()
    }

    pub(super) fn build_effect_model_draws(
        &self,
        r: &legaia_engine_render::Renderer,
        fx_model_flip: Mat4,
        in_world_map: bool,
    ) -> Vec<(UploadedVramMesh, Mat4)> {
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
                    &vmesh.colors,
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
        effect_model_draws
    }

    pub(super) fn build_summon_and_move_fx_part_draws(
        &self,
        r: &legaia_engine_render::Renderer,
        fx_model_flip: Mat4,
        in_world_map: bool,
    ) -> Vec<(UploadedVramMesh, Mat4)> {
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
                    &vmesh.colors,
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
        summon_part_draws
    }

    pub(super) fn build_field_fx_part_draws(
        &self,
        r: &legaia_engine_render::Renderer,
        fx_model_flip: Mat4,
        in_world_map: bool,
    ) -> Vec<(UploadedVramMesh, Mat4)> {
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
                    &vmesh.colors,
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
        field_fx_draws
    }

    /// This frame's move-FX afterimage streak, as screen-space quads on the
    /// retail 320x240 stage.
    ///
    /// The pass retail runs once per move-FX frame: project the launch point
    /// the action effect script's terminator staged
    /// (`legaia_engine_core::action_effect_script::MoveFxStreak`), fan a
    /// camera-facing billboard out at the staged half-width, and emit the
    /// jittered semi-transparent `POLY_FT4` the ported `FUN_801E1AB0` builds.
    ///
    /// Gated on the move-FX scene being live: the trail texpage is
    /// scene-scoped (`World::spawn_move_fx` sets it, `tick_move_fx` drops it
    /// when the scene drains), so the streak lasts exactly as long as the
    /// effect it trails. Empty outside a move.
    // PORT: FUN_801E1AB0 (per-frame emission; the packet + the projection
    // live in `legaia_engine_render::{afterimage, streak_pass}`)
    fn move_fx_streak_quads(
        &self,
        r: &legaia_engine_render::Renderer,
    ) -> Vec<legaia_engine_render::afterimage::AfterimageQuad> {
        use legaia_engine_render::streak_pass::{
            StreakSource, clip_ribbon_quads, streak_quads_scheduled,
        };
        let world = &self.session.host.world;
        let (w, h) = r.surface_size();
        let mvp = self.battle_scene_mvp(w as f32 / h.max(1) as f32);
        // The clip-tag ribbon (`FUN_8004CE2C` tag `0x67` ->
        // `FUN_801E1D98(&target[+0x3C], 0xC)`) is independent of the move-FX
        // scene: it rides a physical art's clip, not a staged effect.
        let mut clip = world
            .battle
            .clip_ribbon
            .map(|c| {
                let seat = c.seat.map(f32::from);
                clip_ribbon_quads(seat, c.trail_id, &mvp, world.clock.display_frames as u32)
            })
            .unwrap_or_default();
        let trail = world.active_move_fx_trail_texpage();
        if trail.is_none() {
            return clip;
        }
        let block = world.move_fx_streak();
        let Some(src) = StreakSource::from_block(block.launch, block.half_width(), trail) else {
            return clip;
        };
        // The retail emitter schedule keys on the counter word and on the
        // acting side: party = afterimage shrinking toward the ribbon,
        // monster = ribbon throughout (`FUN_801E09F8`).
        let party = world.battle_ctx.active_actor < 3;
        // The schedule's own clock is the world's display-frame counter, the
        // same one the browser play page feeds it. It used to be this
        // window's redraw counter, which advances on frames the simulation
        // did not take - so the emitter phase was a property of how fast the
        // host happened to be drawing.
        let frame = world.clock.display_frames as u32;
        let mut quads = streak_quads_scheduled(&src, &mvp, frame, block.counter_word, party);
        quads.append(&mut clip);
        log::debug!(
            "move-FX streak: launch {:?} counter {:#x} half-width {} -> {} quad(s)",
            block.launch,
            block.counter_word,
            block.half_width(),
            quads.len()
        );
        quads
    }

    /// This frame's weapon-trail bands, as screen-space gouraud quads on
    /// the retail 320x240 stage - the projection seat of the swept
    /// `POLY_G4` trail. The trigger + sweep come from
    /// `World::battle_weapon_trail_draws` (retail `FUN_8005112C` /
    /// `FUN_80048310`); the band packets from
    /// `legaia_engine_ui::battle_trail::weapon_trail_prims`
    /// (`FUN_800485BC`). Placement is the live body's model law (party
    /// battle arm: translation + the Y-flip), so the bands land exactly on
    /// the weapon as drawn.
    // REF: FUN_800485BC (packet + band order live in
    // `legaia_engine_ui::battle_trail`; this is the per-host projection)
    /// The world's live full-screen fade as one flat quad
    /// ([`legaia_engine_core::world::World::screen_fade_draw`] through
    /// `fade_prim`, the kernel both hosts composite fades with): `None` while
    /// no fade is up or its start delay is still running.
    pub(super) fn screen_fade_screen_prim(
        &self,
    ) -> Option<legaia_engine_render::screen_overlay::ScreenPrim> {
        let (rgb, abr, ot) = self.session.host.world.screen_fade_draw()?;
        Some(legaia_engine_render::screen_overlay::fade_prim(
            rgb, abr, ot,
        ))
    }

    /// This frame's field fog sheets (`legaia_engine_core::fog_particles`):
    /// the pool's render step (`FUN_8003F348` / `FUN_8003F3FC`, run from
    /// retail's field render pass) through the follow camera this frame
    /// draws the field with, wrapped by the shared `fog_puff_prim` so the
    /// blend class and vertex order are the browser play page's too. Empty
    /// outside game mode 3 (a field scene or the kingdom overworld) or while
    /// the script gate (`_DAT_8007B854`) is clear - the same two tests the
    /// pass makes at `0x80026EA4..0x80026EC4`.
    ///
    /// The camera resolves with no cutscene view, on both hosts alike, so a
    /// scripted camera beat projects the fog through the follow pose: a
    /// shared simplification, not a per-host one.
    pub(super) fn take_field_fog_prims(
        &mut self,
    ) -> Vec<legaia_engine_render::screen_overlay::ScreenPrim> {
        use legaia_engine_core::camera_view::resolve_field_camera;
        use legaia_engine_render::screen_overlay::fog_puff_prim;
        let world = &self.session.host.world;
        if !legaia_engine_core::world::World::fog_mode(world.mode) || !world.fog.gate {
            return Vec::new();
        }
        let center = [
            (self.scene_aabb.0[0] + self.scene_aabb.1[0]) * 0.5,
            (self.scene_aabb.0[2] + self.scene_aabb.1[2]) * 0.5,
        ];
        let frame = resolve_field_camera(world, &self.session.camera, None, center);
        // The field follow pose, or the overworld walk pose in the field
        // frame - retail's overworld is a game-mode-3 scene and draws the
        // same pool through the same pass.
        let Some(view) = frame.field_view() else {
            return Vec::new();
        };
        self.session
            .host
            .world
            .fog_render_step(&view)
            .iter()
            .map(|q| fog_puff_prim(q.xy, q.uv, q.clut, q.tpage, q.rgb, q.ot_index, q.depth))
            .collect()
    }

    /// This frame's actor drop shadows (`legaia_engine_core::drop_shadow`,
    /// retail's `FUN_8001C394` blob): `World::field_drop_shadows` through the
    /// same follow camera as the fog sheets, wrapped by the shared
    /// `drop_shadow_prim` so the blend class and vertex order are the browser
    /// play page's too. Each cell is depth-tested against the scene already
    /// drawn, which keeps it under the actor standing on it and over the
    /// ground - retail's `+0xA0` OT bias and far-bucket ground, in depth-buffer
    /// terms. Empty outside game mode 3.
    pub(super) fn field_drop_shadow_prims(
        &self,
    ) -> Vec<legaia_engine_render::screen_overlay::ScreenPrim> {
        use legaia_engine_core::camera_view::resolve_field_camera;
        use legaia_engine_render::screen_overlay::drop_shadow_prim;
        let world = &self.session.host.world;
        if !legaia_engine_core::world::World::fog_mode(world.mode) {
            return Vec::new();
        }
        let center = [
            (self.scene_aabb.0[0] + self.scene_aabb.1[0]) * 0.5,
            (self.scene_aabb.0[2] + self.scene_aabb.1[2]) * 0.5,
        ];
        let frame = resolve_field_camera(world, &self.session.camera, None, center);
        let Some(view) = frame.field_view() else {
            return Vec::new();
        };
        world
            .field_drop_shadows(&view)
            .iter()
            .map(|q| drop_shadow_prim(q.xy, q.uv, q.clut, q.tpage, q.rgb, q.ot_index, q.depth))
            .collect()
    }

    /// This frame's move-VM strip spans: every extension sub-op `0x2C`
    /// execution the world captured since the last draw (the scanline strip
    /// emitter `FUN_801D31B0`), projected through the same follow camera as
    /// the fog sheets by the shared `move_strip_prims` kernel the browser
    /// play page draws them with. Drained every frame, drawn only in a field
    /// scene - the extension dispatcher exists only in the field overlay.
    pub(super) fn take_move_strip_prims(
        &mut self,
    ) -> Vec<legaia_engine_render::screen_overlay::ScreenPrim> {
        use legaia_engine_core::camera_view::{FieldCameraFrame, resolve_field_camera};
        let requests = self.session.host.world.move_vm.take_strip_requests();
        let world = &self.session.host.world;
        if requests.is_empty() || world.mode != SceneMode::Field {
            return Vec::new();
        }
        let center = [
            (self.scene_aabb.0[0] + self.scene_aabb.1[0]) * 0.5,
            (self.scene_aabb.0[2] + self.scene_aabb.1[2]) * 0.5,
        ];
        let frame = resolve_field_camera(world, &self.session.camera, None, center);
        let (FieldCameraFrame::Follow(view) | FieldCameraFrame::Cutscene(view)) = frame else {
            return Vec::new();
        };
        legaia_engine_render::move_strip::move_strip_prims(&requests, &view)
    }

    /// This frame's attached lights - the field VM's op `0x34` sub-1 light
    /// pools (`World::field_light_draws`, the `FUN_801E4470` draw feeding
    /// `FUN_801E3984`) - through the same follow camera as the fog sheets and
    /// the shared `light_pool_prims` wrapper the browser play page uses.
    /// Empty outside a field scene or with no light live.
    pub(super) fn field_light_screen_prims(
        &self,
    ) -> Vec<legaia_engine_render::screen_overlay::ScreenPrim> {
        use legaia_engine_core::camera_view::{FieldCameraFrame, resolve_field_camera};
        let world = &self.session.host.world;
        if world.mode != SceneMode::Field || world.script_actors.lights.is_empty() {
            return Vec::new();
        }
        let center = [
            (self.scene_aabb.0[0] + self.scene_aabb.1[0]) * 0.5,
            (self.scene_aabb.0[2] + self.scene_aabb.1[2]) * 0.5,
        ];
        let frame = resolve_field_camera(world, &self.session.camera, None, center);
        let (FieldCameraFrame::Follow(view) | FieldCameraFrame::Cutscene(view)) = frame else {
            return Vec::new();
        };
        world
            .field_light_draws(&view)
            .iter()
            .flat_map(|d| legaia_engine_render::screen_overlay::light_pool_prims(d.abr, &d.polys))
            .collect()
    }

    pub(super) fn weapon_trail_screen_prims(
        &self,
        r: &legaia_engine_render::Renderer,
    ) -> Vec<legaia_engine_render::screen_overlay::ScreenPrim> {
        use legaia_engine_render::battle_trail as bt;
        use legaia_engine_vm::battle_trail::TRAIL_POINTS;
        let world = &self.session.host.world;
        if world.mode != SceneMode::Battle {
            return Vec::new();
        }
        let draws = world.battle_weapon_trail_draws();
        if draws.is_empty() {
            return Vec::new();
        }
        let (w, h) = r.surface_size();
        let mvp = self.battle_scene_mvp(w as f32 / h.max(1) as f32);
        let mut out = Vec::new();
        for d in draws {
            let Some(actor) = world.actors.get(d.actor_slot as usize) else {
                continue;
            };
            let pos = [
                f32::from(actor.move_state.world_x),
                f32::from(actor.move_state.world_y),
                f32::from(actor.move_state.world_z),
            ];
            let mut steps = Vec::with_capacity(d.steps.len());
            'draw: for s in &d.steps {
                let mut pts = [(0i16, 0i16); TRAIL_POINTS];
                for (i, t) in s.iter().enumerate() {
                    // The live body's battle model: translation *
                    // scale(1, -1, 1) about the actor origin (the party
                    // arm of `actor_model`; retail anchors every sweep
                    // step at the CURRENT slot base - `FUN_800485BC`
                    // reads `ctx[+0x34]/[+0x38]` fresh per band).
                    let p = [
                        pos[0] + f32::from(t[0]),
                        pos[1] - f32::from(t[1]),
                        pos[2] + f32::from(t[2]),
                    ];
                    match bt::project_stage_point(&mvp, p) {
                        Some(xy) => pts[i] = xy,
                        // Truncate the sweep at the near plane rather
                        // than smearing a wrapped vertex.
                        None => break 'draw,
                    }
                }
                steps.push(pts);
            }
            let prims = bt::weapon_trail_prims(&steps, d.rgb, bt::WEAPON_TRAIL_OT);
            if !prims.is_empty() {
                log::debug!(
                    "weapon trail: actor {} {} step(s) -> {} band quad(s)",
                    d.actor_slot,
                    steps.len(),
                    prims.len()
                );
            }
            out.extend(prims);
        }
        out
    }

    /// Build this frame's arts after-image ghost meshes - the render half of
    /// the retail walk `FUN_80049348` (`legaia_engine_core::battle_afterimage`
    /// holds the schedule/gate/colour kernel; `World::battle_ghost_draws`
    /// binds it to the live pose history). Each ghost is the actor's full
    /// mesh at a historical pose, drawn **flat-coloured and additive** - the
    /// retail draw wrapper `FUN_80043390` decodes the ghost colour word's
    /// mode byte `0x85` into the GP0 ABE bit + ABR mode 1 (B + F) with the
    /// GTE far colour as the flat RGB - so it uploads on the colour-mesh
    /// pipeline with every prim's blend word forced semi-transparent
    /// additive.
    // REF: FUN_80049348 - the retail ghost walk this pass draws for.
    pub(super) fn build_battle_ghost_uploads(
        &self,
        r: &legaia_engine_render::Renderer,
    ) -> Vec<(UploadedColorMesh, Mat4, [f32; 3], [f32; 3])> {
        use legaia_engine_render::psx_blend::pack_blend_word;
        let world = &self.session.host.world;
        let mut out = Vec::new();
        if world.mode != SceneMode::Battle {
            return out;
        }
        // A/B diagnostic: drop the whole ghost pass so a suspect additive
        // wash can be attributed to (or exonerated from) this pass alone.
        if std::env::var_os("LEGAIA_DIAG_NO_GHOSTS").is_some() {
            return out;
        }
        for g in world.battle_ghost_draws() {
            let i = g.actor_slot as usize;
            let Some(actor) = world.actors.get(i) else {
                continue;
            };
            let Some(tmd_idx) = actor.tmd_binding else {
                continue;
            };
            let Some((tmd, raw)) = self.scene_tmd_data.get(tmd_idx) else {
                continue;
            };
            let vmesh =
                legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(tmd, raw, &g.pose.bone_outputs);
            if vmesh.indices.is_empty() {
                continue;
            }
            let colors = vec![g.color; vmesh.positions.len()];
            let blend = vec![pack_blend_word(true, 1); vmesh.positions.len()];
            match r.upload_color_mesh_blended(&vmesh.positions, &colors, &vmesh.indices, &blend) {
                Ok(m) => {
                    // Same placement law as the live battle body
                    // (`actor_model`'s battle arm), at the ghost's own
                    // historical position.
                    let rot = if actor.battle_monster_id.is_some() {
                        Mat4::from_rotation_y(std::f32::consts::PI)
                    } else {
                        Mat4::IDENTITY
                    };
                    let gpos = [g.pos[0] as f32, g.pos[1] as f32, g.pos[2] as f32];
                    let model = Mat4::from_translation(Vec3::from(gpos))
                        * rot
                        * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
                    // The live body's current position, for the render-side
                    // eye push that parks the ghost behind it.
                    let bpos = [
                        f32::from(actor.move_state.world_x),
                        f32::from(actor.move_state.world_y),
                        f32::from(actor.move_state.world_z),
                    ];
                    log::debug!(
                        "arts ghost: actor {} at {:?} colour {:?}",
                        g.actor_slot,
                        g.pos,
                        g.color
                    );
                    out.push((m, model, gpos, bpos));
                }
                Err(e) => log::warn!("ghost mesh upload: {e:#}"),
            }
        }
        out
    }

    /// Build this frame's screen-effect widget meshes (PROT-0900 family) plus
    /// the move-FX afterimage streak.
    ///
    /// The widget geometry - culling, UVs, colours and the retail ordering-table
    /// slot each kind links at - comes out of the shared
    /// [`legaia_engine_core::screen_fx::ScreenFxFrame::draw_quads`] kernel, so
    /// this host and the browser play page cannot disagree about it. What is
    /// host-local is only the mesh upload and the depth each OT slot maps to.
    ///
    /// The ordering is load-bearing and was wrong while the two flat families
    /// shared one batch: retail links the mask's borders at OT `+0x1c` (farthest)
    /// and the letterbox's bands at `+0x4` (nearest, in front of the sprites), so
    /// a letterbox band drawn with the mask sits behind every sprite the same
    /// scene spawns. The feather strips were not drawn at all.
    pub(super) fn build_screen_fx_meshes(
        &self,
        r: &legaia_engine_render::Renderer,
    ) -> (Option<UploadedColorMesh>, Option<UploadedVramMesh>) {
        use legaia_engine_core::screen_fx::ScreenFxQuad;

        let mut screen_fx_solid = None;
        let mut screen_fx_tex = None;
        let streak = self.move_fx_streak_quads(r);
        let fx_quads = self.session.host.world.presentation.fx_frame.draw_quads();
        if fx_quads.is_empty() && streak.is_empty() {
            return (None, None);
        }
        // Retail OT slot -> ortho depth. Larger slot = farther, and the pass
        // draws through `Mat4::orthographic_rh(0, 320, 240, 0, 0.0, 1.0)`, whose
        // depth is `-z`; the scale keeps every slot inside the near/far range.
        let ot_depth = |ot: u32| -(ot as f32) / 1024.0;

        // --- flat quads ----------------------------------------------------
        let mut pos: Vec<[f32; 3]> = Vec::new();
        let mut colors: Vec<[u8; 3]> = Vec::new();
        let mut idx: Vec<u32> = Vec::new();
        for q in &fx_quads {
            let ScreenFxQuad::Flat {
                xy,
                rgba,
                gouraud,
                ot,
                ..
            } = q
            else {
                continue;
            };
            let base = pos.len() as u32;
            let z = ot_depth(*ot);
            for (i, (x, y)) in xy.iter().enumerate() {
                pos.push([*x as f32, *y as f32, z]);
                let c = gouraud.map_or(*rgba, |g| g[i]);
                colors.push([c[0], c[1], c[2]]);
            }
            idx.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
        }
        if !idx.is_empty() {
            match r.upload_color_mesh(&pos, &colors, &idx) {
                Ok(m) => screen_fx_solid = Some(m),
                Err(e) => log::warn!("screen-fx solid mesh upload: {e:#}"),
            }
        }

        // --- textured quads (panels + sprites) + the afterimage streak ------
        let mut pos: Vec<[f32; 3]> = Vec::new();
        let mut uvs: Vec<[u8; 2]> = Vec::new();
        let mut cba_tsb: Vec<[u16; 2]> = Vec::new();
        let mut idx: Vec<u32> = Vec::new();
        for q in &fx_quads {
            let ScreenFxQuad::Textured {
                xy,
                uv,
                clut,
                tpage,
                ot,
                ..
            } = q
            else {
                continue;
            };
            let base = pos.len() as u32;
            let z = ot_depth(*ot);
            for ((x, y), (u, v)) in xy.iter().zip(uv) {
                pos.push([*x as f32, *y as f32, z]);
                uvs.push([*u, *v]);
                cba_tsb.push([*clut, *tpage]);
            }
            idx.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
        }
        // Move-FX afterimage streak. Unlike the widget quads these are not
        // axis-aligned rects - the packet carries four independent corners in
        // the retail `xy0..xy3` order (TL, TR, BL, BR) - so they are pushed
        // vertex-by-vertex. Depth `0.0` puts them in front of every widget:
        // retail links each streak packet at the projected billboard's own OT
        // bucket (inside the scene), which this screen-space batch cannot
        // express, so the engine draws them over the actors instead.
        for q in &streak {
            let base = pos.len() as u32;
            for (x, y) in q.xy {
                pos.push([x as f32, y as f32, 0.0]);
            }
            idx.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
            for (u, v) in q.uv {
                uvs.push([u, v]);
            }
            cba_tsb.extend(std::iter::repeat_n([q.clut, q.tpage], q.xy.len()));
        }
        if !idx.is_empty() {
            let normals = vec![[0.0f32; 3]; pos.len()];
            // Screen-FX sprites are engine-synthesised: no baked colour word,
            // so the neutral colour draws the texel unchanged.
            let colors = vec![[legaia_tmd::legaia_prims::MODULATION_NEUTRAL; 3]; pos.len()];
            match r.upload_vram_mesh(&pos, &uvs, &cba_tsb, &normals, &colors, &idx) {
                Ok(m) => screen_fx_tex = Some(m),
                Err(e) => log::warn!("screen-fx textured mesh upload: {e:#}"),
            }
        }
        (screen_fx_solid, screen_fx_tex)
    }
}

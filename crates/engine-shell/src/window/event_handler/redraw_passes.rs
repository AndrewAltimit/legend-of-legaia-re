//! Per-frame render-pass builders for `handle_redraw`, extracted from
//! `redraw.rs` (mechanical split; behavior-preserving). Each method moves a
//! self-contained, read-only render-pass block verbatim out of the monolithic
//! redraw handler and returns the owned GPU resources / matrices it produced.

use super::super::*;

/// The 320x240 stage's aspect: every stage-projected battle pass's camera.
const STAGE_ASPECT: f32 =
    legaia_engine_render::BOOT_UI_STAGE_W as f32 / legaia_engine_render::BOOT_UI_STAGE_H as f32;

/// Decoded cutscene camera inputs to the retail PSX GTE model:
/// `(focus, pitch_radians, yaw_radians, roll_radians, h, tr_eye)`. See
/// [`PlayWindowApp::cutscene_view`].
pub(in crate::window) type CutsceneCam = ([f32; 3], f32, f32, f32, f32, [f32; 3]);

impl PlayWindowApp {
    /// Stage this frame's volumetric ground-fog bank on the renderer, or
    /// clear it - the browser play page's `_drawFogVolume` twin.
    ///
    /// The bank is the engine's (`World::fog_volume_frame`, `None` with the
    /// toggle down or outside a field scene / battle); this host owns only
    /// the matrix of the bank's space. A field bank is in retail Y-down world
    /// units, which `cam` already maps (it carries the field frame's Y
    /// negation); a battle bank is in raw stage units, which every battle
    /// draw maps through `cam * battle_stage_model()`. The boot UI, the world
    /// map and the in-world minigame venues (their own VRAM and camera) draw
    /// none; a scripted shot and the `F3` debug vantage do - the bank is world
    /// geometry, and `cam` is that frame's own matrix.
    /// The mood enhanced lighting lights this frame under: the persisted
    /// time of day over the loaded scene and its held scripted grade
    /// (`scene_lighting::TimeOfDay::mood_graded` - the same call the browser
    /// play page makes).
    pub(in crate::window) fn lighting_mood(
        &self,
    ) -> legaia_engine_render::scene_lighting::LightingMood {
        use legaia_engine_render::scene_lighting::TimeOfDay;
        let scene_name = self
            .session
            .host
            .scene
            .as_ref()
            .map(|s| s.name.as_str())
            .unwrap_or("");
        TimeOfDay::from_name(&self.options_state.lighting_time_of_day)
            .unwrap_or_default()
            .mood_graded(scene_name, self.session.host.world.held_scene_grade())
    }

    pub(in crate::window) fn stage_fog_volume(
        &self,
        r: &legaia_engine_render::Renderer,
        cam: Mat4,
        in_world_map: bool,
    ) {
        use legaia_engine_core::fog_volume::{FOG_LAYERS, FogSpace, MESH_DIM, SIM_DIM};
        let world = &self.session.host.world;
        let venue = self.baka_gpu.is_some()
            || self.muscle_gpu.is_some()
            || self.fishing_gpu.is_some()
            || self.dance_venue_gpu.is_some()
            || self.slot_gpu.is_some();
        // A menu-overlay screen that owns the frame (a shop, the casino prize
        // counter) draws no field: the 3D pass is skipped and the frame
        // clears black, so the field's mist bank must not draw over the black
        // either. It did, as a grey haze behind every shop window in a misty
        // scene; the browser page's black backdrop sits over its GL canvas.
        let frame = if self.boot_ui.is_active()
            || self.menu_runtime.covers_field()
            || in_world_map
            || venue
        {
            None
        } else {
            world.fog_volume_frame()
        };
        let Some(f) = frame else {
            r.set_fog_volume(None);
            return;
        };
        let world_to_clip = match f.space {
            FogSpace::Field => cam,
            FogSpace::Battle => cam * Self::battle_stage_model(),
        };
        let positions = f.mesh_positions();
        r.set_fog_volume(Some(&legaia_engine_render::FogVolumeDraw {
            world_to_clip,
            sim_origin: f.sim_origin,
            sim_cell: f.sim_cell,
            sim_dim: SIM_DIM as u32,
            density: &f.density,
            mesh_origin: f.mesh_origin,
            mesh_cell: f.mesh_cell,
            mesh_dim: MESH_DIM as u32,
            mesh_positions: &positions,
            ground_gen: f.ground_gen,
            // Enhanced lighting: the mist sits in the scene's mood light.
            color: if self.dynamic_lighting {
                legaia_engine_render::scene_lighting::fog_tint(f.color, &self.lighting_mood())
            } else {
                f.color
            },
            opacity: f.opacity,
            height: f.height,
            layers: FOG_LAYERS,
            drift: f.drift,
            shader_constants: f.space.shader_constants(),
            soft_distance: f.space.soft_distance(),
        }));
    }

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

    /// The retail camera this frame's move-VM part draws resolve their
    /// `+0x52` camera-relative bits against
    /// (`legaia_engine_render::gte::camera_relative_model_prefix`, the port of
    /// `FUN_8001CF50`'s placement), picked by the same selection
    /// [`Self::compute_scene_camera`] makes: the stage-dome battle camera, or
    /// the resolver's frame in the field frame. `None` where this host draws
    /// through a vantage of its own (the stage-less battle orbit, the `F3`
    /// debug orbit, `HostDebugOrbit`, the overworld top view): with no retail
    /// rotation there is nothing to undo, and the parts draw as composed.
    /// The browser play page makes the same selection (`play_battle_fx.rs`).
    pub(in crate::window) fn part_camera_pose(
        &self,
        in_world_map: bool,
        cutscene_cam: Option<CutsceneCam>,
    ) -> Option<legaia_engine_render::gte::PartCameraPose> {
        use legaia_engine_render::gte::PartCameraPose;
        let world = &self.session.host.world;
        if cutscene_cam.is_none() && world.mode == SceneMode::Battle {
            return self
                .battle_stage_mesh
                .is_some()
                .then(|| PartCameraPose::from_battle(&world.battle_cam_pose()));
        }
        if cutscene_cam.is_none() && !in_world_map && self.field_debug_camera {
            return None;
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
        frame
            .field_view()
            .map(|v| PartCameraPose::from_field_view(&v))
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

    /// Battle body `ai`'s draw plan under the camera the battle actor pass
    /// projects with - the same call `redraw.rs` makes for the body's cue.
    fn body_draw_plan(&self, ai: usize) -> Option<legaia_engine_core::world::BattleActorDrawPlan> {
        let world = &self.session.host.world;
        if world.mode != SceneMode::Battle {
            return None;
        }
        let pose = self
            .battle_stage_mesh
            .is_some()
            .then(|| world.battle_cam_pose());
        world.battle_actor_draw_plan(
            ai,
            pose.as_ref(),
            BATTLE_WORLD_SCALE,
            self.battle_stage_outdoor,
        )
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
            let Some(tmd_idx) = actor.tmd_binding else {
                continue;
            };
            let Some(pose) = &actor.pose_frame else {
                // A battle body drawn off its rest mesh still takes its
                // colour word's blend (the capture / defeat fade, the
                // near-camera ghost): retail's dispatcher ORs it into every
                // packet whatever the pose. Only a blended body needs the
                // re-upload; an opaque one draws the resident rest mesh.
                if let Some(plan) = self.body_draw_plan(ai)
                    && plan.semi_mode().is_some()
                    && let Some(rest) = self.battle_rest_vmesh.get(&tmd_idx)
                {
                    let mut cba_tsb = rest.cba_tsb.clone();
                    plan.apply_body_blend(&mut cba_tsb);
                    match r.upload_vram_mesh(
                        &rest.positions,
                        &rest.uvs,
                        &cba_tsb,
                        &rest.normals,
                        &rest.colors,
                        &rest.indices,
                    ) {
                        Ok(m) => posed_overrides[tmd_idx] = Some(m),
                        Err(e) => log::warn!("blended rest mesh upload: {e:#}"),
                    }
                }
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
            // A script's look rotation (`4C 45`, `World::actor_look`) turns
            // one object of the field player - the head - on top of its
            // keyframe; the browser page folds the same kernel in.
            let looked;
            let pose = match (player_slot == Some(ai as u8) && actor.battle_animation.is_none())
                .then(|| {
                    self.session
                        .host
                        .world
                        .actor_look(legaia_engine_core::actor_look::LookKey::Player)
                })
                .flatten()
            {
                Some(look) => {
                    let mut p = pose.clone();
                    legaia_engine_core::actor_look::apply_look(&mut p.bone_outputs, look);
                    looked = p;
                    &looked
                }
                None => pose,
            };
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
            // The body's whole-mesh semi-transparency (the near-camera ghost
            // pass `FUN_8004DC68`, the capture / defeat fade): the colour
            // word's ABE + ABR onto every prim's TSB, as `FUN_80043390` ORs
            // them into each packet. The browser play page's twin is
            // `web-viewer::play_battle_body_blend`.
            if let Some(plan) = self.body_draw_plan(ai) {
                plan.apply_body_blend(&mut vmesh.cba_tsb);
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
            let mut sprites = self.session.host.world.active_effect_sprites();
            // The battle camera consumes Y-up input (its trailing flip
            // cancels the per-model one a billboard does not have), so a
            // battle billboard is built around the flipped centre; the field
            // cameras compose the world flip themselves. The play page makes
            // the same call.
            if self.session.host.world.mode == SceneMode::Battle {
                for s in &mut sprites {
                    s.world_pos = legaia_engine_render::effect_billboard::battle_billboard_centre(
                        s.world_pos,
                    );
                }
            }
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
        part_cam: Option<&legaia_engine_render::gte::PartCameraPose>,
    ) -> Vec<(UploadedVramMesh, Mat4)> {
        let mut summon_part_draws: Vec<(UploadedVramMesh, Mat4)> = Vec::new();
        // A part's placement (`gte::part_model_place`, the browser page's
        // too): `T(F world_pos)`, or - for a `+0x52 & 0x780` node - the
        // camera-relative prefix `FUN_8001CF50` resolves to under this host's
        // full-camera view. Battle models carry the per-model Y-flip, which is
        // the frame flip `F` both the prefix and the translation take.
        let frame_flip = self.session.host.world.mode == SceneMode::Battle;
        let place = |flags: u16, pos: [f32; 3]| -> Mat4 {
            Mat4::from_cols_array(&legaia_engine_render::gte::part_model_place(
                flags, pos, part_cam, frame_flip,
            ))
        };
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
                // The part's morphed or rest mesh with its render scale and
                // colour word applied (`World::part_draw_vram_mesh` - the
                // Spirit aura's cones grow, spin and fade through it); the
                // browser play page draws the same kernel.
                let Some(vmesh) = self.session.host.world.part_draw_vram_mesh(&sp) else {
                    continue;
                };
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
                        let model = place(sp.flags_52, sp.world_pos)
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
        // Draw-kind-4 nodes - effect ribbons (move-VM op `0x42`) and
        // `0x4000` sprite-arm quads (op `0x23`): transform nodes whose model
        // is the emitter's own per-frame output, composed like a part, in
        // battle, on the field and on the overworld. Retail's per-actor
        // dispatcher `FUN_8001ADA4` makes no mode test on its case-4 arm
        // (`0x8001B060..0x8001B160`), and the `map01` overworld state
        // `keikoku_chest_preload` holds seven live kind-4 sprite-arm nodes
        // on list `_DAT_8007C350`. The browser play page draws the same list
        // (`play_battle_fx.rs`, both FX frames).
        if !self.boot_ui.is_active() {
            for rb in self.session.host.world.active_effect_kind4_draws() {
                let v = &rb.mesh;
                match r.upload_vram_mesh(
                    &v.positions,
                    &v.uvs,
                    &v.cba_tsb,
                    &v.normals,
                    &v.colors,
                    &v.indices,
                ) {
                    Ok(m) => {
                        let model = place(rb.flags_52, rb.world_pos)
                            * Mat4::from_rotation_y(rb.rot[1])
                            * Mat4::from_rotation_x(rb.rot[0])
                            * Mat4::from_rotation_z(rb.rot[2])
                            * fx_model_flip;
                        summon_part_draws.push((m, model));
                    }
                    Err(e) => log::warn!("draw-kind-4 mesh upload: {e:#}"),
                }
            }
            // Battle ground shadows (`FUN_80048A08`'s disc, built by the same
            // default-arm builder `FUN_80028158`): one per drawn body, judged
            // by the plan the battle actor pass draws with. The browser play
            // page draws the same list (`play_battle_fx.rs`).
            let world = &self.session.host.world;
            for rb in world.battle_ground_shadows(|i| self.body_draw_plan(i)) {
                let v = &rb.mesh;
                match r.upload_vram_mesh(
                    &v.positions,
                    &v.uvs,
                    &v.cba_tsb,
                    &v.normals,
                    &v.colors,
                    &v.indices,
                ) {
                    Ok(m) => {
                        let model =
                            Mat4::from_translation(Vec3::from(rb.world_pos)) * fx_model_flip;
                        summon_part_draws.push((m, model));
                    }
                    Err(e) => log::warn!("battle ground shadow upload: {e:#}"),
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
        part_cam: Option<&legaia_engine_render::gte::PartCameraPose>,
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
                        // Field frame: the models compose on the raw
                        // retail frame, so no flip to conjugate by.
                        let place = legaia_engine_render::gte::camera_relative_model_prefix(
                            fp.flags_52,
                            fp.world_pos,
                            part_cam,
                            false,
                        )
                        .map(|m| Mat4::from_cols_array(&m))
                        .unwrap_or_else(|| Mat4::from_translation(Vec3::from(fp.world_pos)));
                        let model = place
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
    fn move_fx_streak_quads(&self) -> Vec<legaia_engine_render::afterimage::AfterimageQuad> {
        use legaia_engine_render::streak_pass::{
            StreakSource, clip_ribbon_quads, streak_quads_scheduled,
        };
        let world = &self.session.host.world;
        // The quads project into the 320x240 stage (`project_stage_point`),
        // so the camera is the stage's 4:3 - the aspect the scene pass draws
        // at inside its stage viewport (`scene_viewport_for`) and the page's
        // `battle_vp` hard-codes. The surface's own aspect put the trail and
        // the streak off the bodies on any window that is not 4:3 (the
        // runner's 960x699 among them).
        let mvp = self.battle_scene_mvp(STAGE_ASPECT);
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
    /// The world's live full-screen fades as flat quads
    /// ([`legaia_engine_core::world::World::screen_fade_draws`] through
    /// `fade_prim`, the kernel both hosts composite fades with): empty while
    /// no fade is up or every start delay is still running.
    pub(super) fn screen_fade_screen_prims(
        &self,
    ) -> Vec<legaia_engine_render::screen_overlay::ScreenPrim> {
        self.session
            .host
            .world
            .screen_fade_draws()
            .into_iter()
            .map(|(rgb, abr, ot)| legaia_engine_render::screen_overlay::fade_prim(rgb, abr, ot))
            .collect()
    }

    /// PROT 0948's Cross Beam packets while arm 3 runs
    /// ([`legaia_engine_core::world::World::cross_beam_draw`] through
    /// `legaia_engine_ui::cast_beam::cross_beam_prims`, the kernel the browser
    /// play page draws them with). Empty on every other frame.
    pub(super) fn cross_beam_screen_prims(
        &self,
    ) -> Vec<legaia_engine_render::screen_overlay::ScreenPrim> {
        self.session
            .host
            .world
            .cross_beam_draw()
            .map(legaia_engine_render::cast_beam::cross_beam_prims)
            .unwrap_or_default()
    }

    /// PROT 0904's (Theeder) beam packets this frame
    /// ([`legaia_engine_core::world::World::theeder_draw`] through
    /// `legaia_engine_ui::cast_theeder::theeder_prims`, the kernel the browser
    /// play page draws them with), projected with the battle camera at the
    /// stage's 4:3 - the weapon trail's projection. Retail points are Y-down;
    /// this host's battle space is Y-up, so Y is negated on the way in.
    pub(super) fn theeder_screen_prims(
        &self,
    ) -> Vec<legaia_engine_render::screen_overlay::ScreenPrim> {
        use legaia_engine_render::battle_trail as bt;
        let world = &self.session.host.world;
        let Some(packet) = world.theeder_draw() else {
            return Vec::new();
        };
        let mvp = self.battle_scene_mvp(STAGE_ASPECT);
        legaia_engine_render::cast_theeder::theeder_prims(&packet, world.theeder_trail(), |p| {
            bt::project_stage_point(&mvp, [f32::from(p[0]), -f32::from(p[1]), f32::from(p[2])])
        })
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
    /// execution of the most recent tick (the scanline strip emitter
    /// `FUN_801D31B0`, `MoveVmGlobals::strip_frame`), projected through the
    /// same follow camera as the fog sheets by the shared `move_strip_prims`
    /// kernel the browser play page draws them with. Drawn only in a field
    /// scene - the extension dispatcher exists only in the field overlay.
    pub(super) fn take_move_strip_prims(
        &mut self,
    ) -> Vec<legaia_engine_render::screen_overlay::ScreenPrim> {
        use legaia_engine_core::camera_view::{FieldCameraFrame, resolve_field_camera};
        let world = &self.session.host.world;
        // The latest tick's set, not drained: an idle redraw draws it again
        // (`MoveVmGlobals::strip_frame`).
        let requests = world.move_vm.strip_frame();
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
        legaia_engine_render::move_strip::move_strip_prims(requests, &view)
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
        // The quads project into the 320x240 stage (`project_stage_point`),
        // so the camera is the stage's 4:3 - the aspect the scene pass draws
        // at inside its stage viewport (`scene_viewport_for`) and the page's
        // `battle_vp` hard-codes. The surface's own aspect put the trail and
        // the streak off the bodies on any window that is not 4:3 (the
        // runner's 960x699 among them).
        let mvp = self.battle_scene_mvp(STAGE_ASPECT);
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

    /// This frame's PROT-0900 screen-effect widgets (iris mask, scripted
    /// sprites, image panel, letterbox bands and feathers) as screen
    /// primitives, for the pass the field fog sheets and attached lights sort
    /// in - the browser play page's `screen_fx_prims` twin, a variant-for-variant
    /// re-wrap of the shared [`legaia_engine_core::screen_fx::ScreenFxFrame::draw_quads`]
    /// kernel.
    ///
    /// Retail links the mask's black borders at OT `+0x1C` and a fog sheet at
    /// `SZ >> 5`, so the sheets sort **under** the mask. Drawn as scene meshes,
    /// the widgets landed before every screen primitive and the ending's fog
    /// haze painted over its black credits card.
    pub(super) fn screen_fx_screen_prims(
        &self,
    ) -> Vec<legaia_engine_render::screen_overlay::ScreenPrim> {
        use legaia_engine_core::screen_fx::ScreenFxQuad;
        use legaia_engine_render::screen_overlay::{FlatQuad, ScreenPrim, ScreenQuad};
        self.session
            .host
            .world
            .presentation
            .fx_frame
            .draw_quads()
            .into_iter()
            .map(|q| match q {
                ScreenFxQuad::Flat {
                    xy,
                    rgba,
                    gouraud,
                    semi_transparent,
                    abr_mode,
                    ot,
                } => ScreenPrim::Flat(FlatQuad {
                    xy,
                    color: rgba,
                    gouraud,
                    semi_transparent,
                    abr_mode,
                    ot_index: ot,
                    depth: None,
                }),
                ScreenFxQuad::Textured {
                    xy,
                    uv,
                    clut,
                    tpage,
                    color,
                    semi_transparent,
                    ot,
                } => ScreenPrim::Textured(ScreenQuad {
                    xy,
                    uv,
                    clut,
                    tpage,
                    color,
                    gouraud: None,
                    semi_transparent,
                    ot_index: ot,
                    depth: None,
                }),
            })
            .collect()
    }

    /// Build this frame's move-FX afterimage streak mesh. The PROT-0900
    /// screen-effect widgets it used to batch with ride the screen-prim pass
    /// ([`Self::screen_fx_screen_prims`]), where they sort against the fog
    /// sheets by their retail OT slots.
    pub(super) fn build_screen_fx_meshes(
        &self,
        r: &legaia_engine_render::Renderer,
    ) -> Option<UploadedVramMesh> {
        let mut screen_fx_tex = None;
        let streak = self.move_fx_streak_quads();
        if streak.is_empty() {
            return None;
        }
        let mut pos: Vec<[f32; 3]> = Vec::new();
        let mut uvs: Vec<[u8; 2]> = Vec::new();
        let mut cba_tsb: Vec<[u16; 2]> = Vec::new();
        let mut idx: Vec<u32> = Vec::new();
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
        screen_fx_tex
    }
}

//! Per-frame render-pass builders for `handle_redraw`, extracted from
//! `redraw.rs` (mechanical split; behavior-preserving). Each method moves a
//! self-contained, read-only render-pass block verbatim out of the monolithic
//! redraw handler and returns the owned GPU resources / matrices it produced.

use super::super::*;

/// Decoded cutscene camera inputs to the retail PSX GTE model:
/// `(focus, pitch_radians, yaw_radians, roll_radians, h, tr_eye)`. See
/// [`PlayWindowApp::cutscene_view`].
pub(in crate::window) type CutsceneCam = ([f32; 3], f32, f32, f32, f32, [f32; 3]);

/// The scripted shot as the resolver's view type.
fn cutscene_view(cam: CutsceneCam) -> legaia_engine_camera::psx_camera::FieldCameraView {
    let (focus, pitch, yaw, roll, h, tr_eye) = cam;
    legaia_engine_camera::psx_camera::FieldCameraView {
        focus,
        pitch,
        yaw,
        roll,
        h,
        tr_eye,
    }
}

impl PlayWindowApp {
    /// The world, scene camera and scene box this frame's projections read
    /// (`legaia_engine_world_views::screen_passes::HostView`, the seam the
    /// browser play page reads through too).
    pub(in crate::window) fn host_view(
        &self,
    ) -> legaia_engine_world_views::screen_passes::HostView<'_> {
        legaia_engine_world_views::screen_passes::HostView::new(
            &self.session.host.world,
            &self.session.camera,
            self.scene_aabb,
        )
    }

    /// The mood enhanced lighting lights this frame under
    /// (`legaia_engine_screens::field_frame::lighting_mood`, the answer the
    /// browser play page reads too).
    pub(in crate::window) fn lighting_mood(
        &self,
    ) -> legaia_render_kernels::scene_lighting::LightingMood {
        legaia_engine_screens::field_frame::lighting_mood(
            Some(&self.session.host),
            &self.options_state.lighting_time_of_day,
        )
    }

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
    pub(in crate::window) fn stage_fog_volume(
        &self,
        r: &legaia_engine_render::Renderer,
        cam: Mat4,
        in_world_map: bool,
    ) {
        use legaia_render_kernels::fog_volume::{FOG_LAYERS, FogSpace, MESH_DIM, SIM_DIM};
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
            color: legaia_engine_screens::field_frame::fog_volume_color(
                f.color,
                self.dynamic_lighting,
                &self.lighting_mood(),
            ),
            opacity: f.opacity,
            height: f.height,
            layers: FOG_LAYERS,
            drift: f.drift,
            shader_constants: f.space.shader_constants(),
            soft_distance: f.space.soft_distance(),
            spill_phase: f.spill_phase(),
            spill_constants: legaia_render_kernels::fog_volume::FOG_SPILL_CONSTANTS,
            haze_constants: legaia_render_kernels::fog_volume::FOG_HAZE_CONSTANTS,
            ground: f
                .ground_tex
                .as_ref()
                .map(|g| legaia_engine_render::FogGroundTexture {
                    uv_rect: f.ground_uv_rect(),
                    w: g.w,
                    h: g.h,
                    texels: g.texels,
                    ground_gen: g.ground_gen,
                }),
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
    /// matrix is `camera_frame::frame_vp` for every one of those arms.
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
        let frame = self.host_view().frame(cutscene_cam.map(cutscene_view));
        let vp = legaia_engine_field::camera_frame::frame_vp(
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
    /// (`legaia_render_kernels::gte::camera_relative_model_prefix`, the port of
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
    ) -> Option<legaia_render_kernels::gte::PartCameraPose> {
        use legaia_render_kernels::gte::PartCameraPose;
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
        self.host_view()
            .part_camera_pose(cutscene_cam.map(cutscene_view))
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
        self.host_view()
            .curve_scale(cutscene_cam.map(cutscene_view))
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

    /// Battle body `ai`'s body-to-view map under the same camera, for the
    /// shatter renderer's sweep rows (`World::battle_shatter_view`).
    fn body_shatter_view(&self, ai: usize) -> [[f32; 3]; 3] {
        let world = &self.session.host.world;
        let pose = self
            .battle_stage_mesh
            .is_some()
            .then(|| world.battle_cam_pose());
        world.battle_shatter_view(ai, pose.as_ref(), BATTLE_WORLD_SCALE)
    }

    /// Battle body `ai`'s mesh broken by the shatter renderer
    /// (`legaia_render_kernels::shatter`, the port of `FUN_80029888` /
    /// `FUN_80029DD8`): the TMD's local triangles through the kernel, then
    /// each object's live pose, then the same per-body passes the posed
    /// rebuild takes (Rot limb dimming before the split, the texture-slot
    /// relocation and the colour word's blend after). The browser play
    /// page runs the same kernel (`play_battle_actor_shatter`).
    fn shattered_battle_mesh(
        &self,
        r: &legaia_engine_render::Renderer,
        ai: usize,
        actor: &legaia_engine_core::world::Actor,
        tmd_idx: usize,
        input: &legaia_render_kernels::shatter::ShatterInput,
    ) -> Option<UploadedVramMesh> {
        let (tmd, raw) = self.scene_tmd_data.get(tmd_idx)?;
        let masked = self
            .session
            .host
            .world
            .battle_live_drawn_objects(ai)
            .map(|d| legaia_engine_battle::battle_attach_swap::mask_tmd(tmd, &d));
        let tmd = masked.as_ref().unwrap_or(tmd);
        let (mut local, object_ids) = legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(tmd, raw);
        if actor.battle_animation.is_some() {
            self.session
                .host
                .world
                .dim_posed_battle_mesh(ai, tmd, raw, &mut local.colors);
        }
        let bones = actor
            .pose_frame
            .as_ref()
            .map_or(&[][..], |p| p.bone_outputs.as_slice());
        let rotate = actor.battle_animation.is_some()
            || self.session.host.world.player_actor_slot == Some(ai as u8);
        let view = self.body_shatter_view(ai);
        let mut vmesh = legaia_render_kernels::shatter::shatter_vram_mesh(
            &local,
            &object_ids,
            input,
            bones,
            rotate,
            // A sweep row measures each object in its pose: Freed's
            // re-forming body (PROT 0912 arm 7's row 1) comes together from
            // the floor up.
            |o| legaia_render_kernels::shatter::bone_sweep_frame(bones, o, rotate, &view),
        );
        if let Some(slot) = actor.battle_tex_slot {
            for ct in &mut vmesh.cba_tsb {
                ct[0] = legaia_actor_formats::monster_archive::relocate_cba(ct[0], slot);
                ct[1] = legaia_actor_formats::monster_archive::relocate_tsb(ct[1], slot);
            }
        }
        if let Some(plan) = self.body_draw_plan(ai) {
            plan.apply_body_blend(&mut vmesh.cba_tsb);
        }
        if vmesh.indices.is_empty() {
            return None;
        }
        r.upload_vram_mesh(
            &vmesh.positions,
            &vmesh.uvs,
            &vmesh.cba_tsb,
            &vmesh.normals,
            &vmesh.colors,
            &vmesh.indices,
        )
        .map_err(|e| log::warn!("shattered mesh upload: {e:#}"))
        .ok()
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
            // A body whose `+0x1BA` shatter word is up draws through the
            // shatter renderer (`FUN_80029888`): its triangles come apart,
            // so the mesh is rebuilt de-indexed this frame, posed or not.
            if let Some(input) = self.body_draw_plan(ai).and_then(|p| p.shatter) {
                if let Some(m) = self.shattered_battle_mesh(r, ai, actor, tmd_idx, &input) {
                    posed_overrides[tmd_idx] = Some(m);
                }
                continue;
            }
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
                        .actor_look(legaia_engine_field::actor_look::LookKey::Player)
                })
                .flatten()
            {
                Some(look) => {
                    let mut p = pose.clone();
                    legaia_engine_field::actor_look::apply_look(&mut p.bone_outputs, look);
                    looked = p;
                    &looked
                }
                None => pose,
            };
            // The playing clip's morph lanes (`FUN_800495C8`, blended before
            // the draw and restored after it): the posed mesh is built from
            // a blended clone, the rest TMD stays untouched.
            let morphed = self.session.host.world.battle_morphed_tmd(ai, tmd);
            let tmd = morphed.as_ref().unwrap_or(tmd);
            // A party seat draws the objects its per-channel model table
            // names for the playing stream's channels (`FUN_80048A08` after
            // the mesh-variant swap `FUN_8004CCD4`); the browser page's twin
            // is `play_battle_actor_hidden`.
            let masked = self
                .session
                .host
                .world
                .battle_live_drawn_objects(ai)
                .map(|d| legaia_engine_battle::battle_attach_swap::mask_tmd(tmd, &d));
            let tmd = masked.as_ref().unwrap_or(tmd);
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
                    ct[0] = legaia_actor_formats::monster_archive::relocate_cba(ct[0], slot);
                    ct[1] = legaia_actor_formats::monster_archive::relocate_tsb(ct[1], slot);
                }
            }
            if vmesh.indices.is_empty() {
                continue;
            }
            // Rot limb dimming (`FUN_80048A08`), the browser page's twin in
            // `web-play::play_battle_limb_dim`.
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
            // `web-play::play_battle_body_blend`.
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
            // The billboard list both hosts draw
            // (`legaia_engine_world_views::effect_draws`): centres in the
            // camera's frame (flipped in battle), half-extents out of the
            // view scale, texel corners mirrored, the TSB semi bit set.
            let sprites = legaia_engine_world_views::effect_draws::effect_billboards(
                &self.session.host.world,
                view_scale,
            );
            if sprites.is_empty() {
                (None, None)
            } else {
                // Camera right/up in world space (clip-space basis
                // dirs mapped back through the inverse MVP).
                let inv = cam.inverse();
                let right = inv.transform_vector3(Vec3::X).normalize_or_zero();
                let up = inv.transform_vector3(Vec3::Y).normalize_or_zero();
                let mesh = effect_billboard_mesh(r, &sprites, right, up);
                // The wireframe outline is a **diagnostic**, off by default
                // (`LEGAIA_DIAG_FX=1`). It exists to make a spawn readable
                // when its texels are not resident, and it predates the
                // battle-entry flame-atlas blit; with the atlas resident the
                // textured quad draws, and leaving the outline on stamps a
                // bright rectangle over every effect in normal play.
                let lines = if std::env::var_os("LEGAIA_DIAG_FX").is_some() {
                    let (pos, col, idx) = effect_sprite_line_geometry(&sprites, right, up);
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
    /// (`legaia_engine_world_views::world_map_markers`) and the shared
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
    ) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
        if self.boot_ui.is_active() {
            return Vec::new();
        }
        let player_mesh_drawn = self
            .session
            .host
            .world
            .player_actor_slot
            .is_some_and(|pslot| self.drained_spawn_slots.contains(&pslot));
        self.host_view().world_map_marker_prims(!player_mesh_drawn)
    }

    /// The overworld sky band (`FUN_801F73E4`) as screen primitives, through
    /// the gate (`legaia_engine_world_views::world_map_sky`) and wrapper
    /// (`screen_prim::sky_band_prims`) the browser play page calls. Each
    /// quad is depth-tested at the far plane, so it fills only what the
    /// terrain left clear - retail's farthest-bucket draw order.
    pub(super) fn world_map_sky_prims(
        &self,
    ) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
        if self.boot_ui.is_active() {
            return Vec::new();
        }
        self.host_view().world_map_sky_prims()
    }

    /// The live effect layer's 3D draws, uploaded: the `etmd` effect
    /// models, then the summon / move-FX parts followed by the draw-kind-4
    /// nodes and the battle ground shadows, then the field stager parts.
    ///
    /// Which draws exist and where each sits is the list both hosts draw
    /// (`legaia_engine_world_views::effect_draws::effect_model_draws`: the
    /// overworld gate, the battle frame flip, the `+0x52` camera-relative
    /// placement against `part_cam`); this window resolves each to a mesh
    /// upload. A part draws `World::part_draw_vram_mesh` - its morphed or
    /// rest mesh with its render scale and colour word applied (the Spirit
    /// aura's cones grow, spin and fade through it). A field stager part's
    /// `model_index` is the stager record's relative `model_sel` (spawn
    /// base 0) into the scene TMD pack (`field_stager_tmds`,
    /// `DAT_8007C018[5 + model_sel]`), not the battle effect library.
    #[allow(clippy::type_complexity)]
    pub(super) fn build_effect_draws(
        &self,
        r: &legaia_engine_render::Renderer,
        part_cam: Option<&legaia_render_kernels::gte::PartCameraPose>,
    ) -> (
        Vec<(UploadedVramMesh, Mat4)>,
        Vec<(UploadedVramMesh, Mat4)>,
        Vec<(UploadedVramMesh, Mat4)>,
    ) {
        if self.boot_ui.is_active() {
            return (Vec::new(), Vec::new(), Vec::new());
        }
        let world = &self.session.host.world;
        let lists =
            legaia_engine_world_views::effect_draws::effect_model_draws(world, part_cam, |i| {
                self.body_draw_plan(i)
            });
        let upload = |vmesh: &legaia_tmd::mesh::VramMesh, what: &str| {
            if vmesh.indices.is_empty() {
                return None;
            }
            match r.upload_vram_mesh(
                &vmesh.positions,
                &vmesh.uvs,
                &vmesh.cba_tsb,
                &vmesh.normals,
                &vmesh.colors,
                &vmesh.indices,
            ) {
                Ok(m) => Some(m),
                Err(e) => {
                    log::warn!("{what} mesh upload: {e:#}");
                    None
                }
            }
        };
        let effect_model_draws = lists
            .effect_models
            .iter()
            .filter_map(|em| {
                let gtmd = world.global_tmd(em.index as i16)?;
                let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(&gtmd.tmd, &gtmd.raw);
                Some((
                    upload(&vmesh, "effect model")?,
                    Mat4::from_cols_array(&em.model),
                ))
            })
            .collect();
        let mut part_draws: Vec<(UploadedVramMesh, Mat4)> = lists
            .parts
            .iter()
            .filter_map(|p| {
                let vmesh = world.part_draw_vram_mesh(&p.draw)?;
                Some((
                    upload(&vmesh, "summon/move-FX part")?,
                    Mat4::from_cols_array(&p.model),
                ))
            })
            .collect();
        for (n, what) in lists
            .kind4
            .iter()
            .map(|n| (n, "draw-kind-4"))
            .chain(lists.shadows.iter().map(|n| (n, "battle ground shadow")))
        {
            if let Some(m) = upload(&n.mesh, what) {
                part_draws.push((m, Mat4::from_cols_array(&n.model)));
            }
        }
        let field_fx_draws = lists
            .field_parts
            .iter()
            .filter_map(|fp| {
                let (tmd, raw) = self.field_stager_tmds.get(fp.index)?;
                let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(tmd, raw);
                Some((
                    upload(&vmesh, "field-FX part")?,
                    Mat4::from_cols_array(&fp.model),
                ))
            })
            .collect();
        (effect_model_draws, part_draws, field_fx_draws)
    }

    /// This frame's field fog sheets (`legaia_engine_field::fog_particles`):
    /// the quads the last sim tick's render step left
    /// (`BootSession::step_field_fog`, `FUN_8003F348` / `FUN_8003F3FC`, run
    /// from retail's field render pass once per frame), wrapped by the shared
    /// `fog_puff_prim` so the blend class and vertex order are the browser
    /// play page's too. Empty outside game mode 3 (a field scene or the
    /// kingdom overworld) or while the script gate (`_DAT_8007B854`) is clear -
    /// the same two tests the pass makes at `0x80026EA4..0x80026EC4`.
    ///
    /// A read, not a step: the step is per sim tick
    /// (`tick_frame_tail_effects`), so a redraw that ran two ticks or none
    /// draws what the last tick left, as the page does. The one exception is
    /// the capture harness's frame: with `LEGAIA_SEAT_FOG` due it installs
    /// the retail state's records and re-runs the frame's step over them
    /// (`BootSession::seat_field_fog`), through the camera this frame draws.
    pub(super) fn take_field_fog_prims(
        &mut self,
    ) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
        if let Some(sc) = self.screenshot.as_ref()
            && sc.seat_fog_due.take()
            && let Some(fog) = sc.seat_fog.take()
        {
            let centre = self.host_view().centre();
            self.session.seat_field_fog(&fog, centre);
        }
        self.host_view().fog_prims()
    }

    /// This frame's actor drop shadows (retail's `FUN_8001C394` blob), the
    /// shared pass both hosts draw
    /// (`legaia_engine_world_views::screen_passes::HostView::drop_shadow_prims`).
    /// Each cell is depth-tested against the scene already drawn, which keeps
    /// it under the actor standing on it and over the ground - retail's
    /// `+0xA0` OT bias and far-bucket ground, in depth-buffer terms.
    pub(super) fn field_drop_shadow_prims(
        &self,
    ) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
        self.host_view().drop_shadow_prims()
    }

    /// This frame's move-VM strip spans (`FUN_801D31B0`), the shared pass
    /// both hosts draw
    /// (`legaia_engine_world_views::screen_passes::HostView::move_strip_prims`).
    pub(super) fn take_move_strip_prims(
        &mut self,
    ) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
        self.host_view().move_strip_prims()
    }

    /// This frame's attached lights (op `0x34` sub-1, `FUN_801E3984`), the
    /// shared pass both hosts draw
    /// (`legaia_engine_world_views::screen_passes::HostView::light_prims`).
    pub(super) fn field_light_screen_prims(
        &self,
    ) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
        self.host_view().light_prims()
    }

    /// Build this frame's arts after-image ghost meshes - the render half of
    /// the retail walk `FUN_80049348` (`legaia_engine_battle::battle_afterimage`
    /// holds the schedule/gate/colour kernel; `World::battle_ghost_draws`
    /// binds it to the live pose history). Each ghost is the actor's full
    /// mesh at a historical pose, drawn through the body's own textured
    /// path: the walk only rewrites the render node's colour word
    /// (`+0x74 = rgb | 0x85000000`) and weight (`+0x78 = 0x1000`) and calls
    /// the ordinary object draw `FUN_80048A08`. The dispatcher
    /// `FUN_80043390` turns the mode byte into the ABE bit, ABR 1 (B + F)
    /// and prim bank 2 (`0xA0`), whose handlers still emit textured packets
    /// (`0x24` `POLY_FT3` at `0x800458E0` for kind 18) with the DPCS output
    /// as the packet colour. At weight `0x1000` that output is the far
    /// colour, so a ghost is the body's texture modulated by the ghost
    /// colour (`texel * rgb / 128`), added over the frame - not a flat fill.
    /// So it uploads on the textured pipeline with every prim's TSB raised
    /// to semi-transparent additive; the draw site stages the colour as a
    /// full-weight cue.
    // REF: FUN_80049348 - the retail ghost walk this pass draws for.
    pub(super) fn build_battle_ghost_uploads(
        &self,
        r: &legaia_engine_render::Renderer,
    ) -> Vec<super::redraw_stage::BattleGhostUpload> {
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
            // The ghost's own re-run of the mesh-variant swap
            // (`FUN_80049348` -> `FUN_8004CCD4`).
            let masked = g
                .drawn
                .as_deref()
                .map(|d| legaia_engine_battle::battle_attach_swap::mask_tmd(tmd, d));
            let tmd = masked.as_ref().unwrap_or(tmd);
            let mut vmesh =
                legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(tmd, raw, &g.pose.bone_outputs);
            if vmesh.indices.is_empty() {
                continue;
            }
            // The live body's per-slot texture relocation
            // (`build_posed_actor_overrides`), then the ghost word's blend.
            if let Some(slot) = actor.battle_tex_slot {
                for ct in &mut vmesh.cba_tsb {
                    ct[0] = legaia_actor_formats::monster_archive::relocate_cba(ct[0], slot);
                    ct[1] = legaia_actor_formats::monster_archive::relocate_tsb(ct[1], slot);
                }
            }
            legaia_engine_battle::battle_body_blend::apply_body_blend(
                &mut vmesh.cba_tsb,
                Some(legaia_engine_battle::battle_afterimage::GHOST_ABR),
            );
            match r.upload_vram_mesh(
                &vmesh.positions,
                &vmesh.uvs,
                &vmesh.cba_tsb,
                &vmesh.normals,
                &vmesh.colors,
                &vmesh.indices,
            ) {
                Ok(mesh) => {
                    // Same placement law as the live battle body
                    // (`actor_model`'s battle arm), at the ghost's own
                    // historical position.
                    // The ring holds no rotation: the walk draws every ghost
                    // through the live node's rotation trio (`FUN_80049348`
                    // swaps only the position, cursor and clip record), so
                    // a ghost turns by the body's live facing `+0x46` as
                    // the body does (`actor_model`'s battle arm). A fixed
                    // half-turn swung Cort's Guilty Cross ghosts `0xED`
                    // off his `0x8ED` facing, and their claws out past the
                    // body's.
                    let rot = Mat4::from_rotation_y(
                        f32::from(actor.battle.facing_angle & 0xFFF) / 4096.0
                            * std::f32::consts::TAU,
                    );
                    // In the placement frame (`T(F pos)`, the live body's
                    // law): the eye the draw pushes about is read off the
                    // camera in that frame too.
                    let gpos = legaia_engine_battle::battle_afterimage::placement_pos([
                        g.pos[0] as f32,
                        g.pos[1] as f32,
                        g.pos[2] as f32,
                    ]);
                    let model = Mat4::from_translation(Vec3::from(gpos))
                        * rot
                        * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
                    // The live body's current position, for the render-side
                    // eye push that parks the ghost behind it.
                    let bpos = legaia_engine_battle::battle_afterimage::placement_pos([
                        f32::from(actor.move_state.world_x),
                        f32::from(actor.move_state.world_y),
                        f32::from(actor.move_state.world_z),
                    ]);
                    log::debug!(
                        "arts ghost: actor {} at {:?} colour {:?}",
                        g.actor_slot,
                        g.pos,
                        g.color
                    );
                    out.push(super::redraw_stage::BattleGhostUpload {
                        mesh,
                        model,
                        ghost_pos: gpos,
                        body_pos: bpos,
                        color: g.color,
                        nclip: g.nclip_mode(),
                    });
                }
                Err(e) => log::warn!("ghost mesh upload: {e:#}"),
            }
        }
        out
    }
}

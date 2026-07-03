//! Per-frame render-pass builders for `handle_redraw`, extracted from
//! `redraw.rs` (mechanical split; behavior-preserving). Each method moves a
//! self-contained, read-only render-pass block verbatim out of the monolithic
//! redraw handler and returns the owned GPU resources / matrices it produced.

use super::super::*;

impl PlayWindowApp {
    pub(super) fn compute_scene_camera(
        &self,
        aspect: f32,
        in_world_map: bool,
        cutscene_cam: Option<([f32; 3], f32, f32, f32)>,
    ) -> Mat4 {
        if in_world_map {
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
        }
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
        (posed_overrides, player_color_posed)
    }

    pub(super) fn build_effect_billboards(
        &self,
        r: &legaia_engine_render::Renderer,
        cam: Mat4,
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
        }
    }

    pub(super) fn build_world_map_overlay_lines(
        &self,
        r: &legaia_engine_render::Renderer,
        in_world_map: bool,
    ) -> Option<legaia_engine_render::UploadedLines> {
        if in_world_map && !self.boot_ui.is_active() {
            let markers = self.session.host.world.world_map_entity_markers();
            let mut pos: Vec<[f32; 3]> = Vec::new();
            let mut col: Vec<[u8; 4]> = Vec::new();
            let mut idx: Vec<u32> = Vec::new();
            if !markers.is_empty() {
                let (p, c, i) =
                    world_map_entity_line_geometry(&markers, self.scene_aabb.0, self.scene_aabb.1);
                pos = p;
                col = c;
                idx = i;
            }
            if let Some(player) = self.session.host.world.world_map_player_marker() {
                let (p, c, i) =
                    world_map_player_line_geometry(&player, self.scene_aabb.0, self.scene_aabb.1);
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
        }
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

    pub(super) fn build_screen_fx_meshes(
        &self,
        r: &legaia_engine_render::Renderer,
    ) -> (Option<UploadedColorMesh>, Option<UploadedVramMesh>) {
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
                idx.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
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
                uvs.extend_from_slice(&[[p.u0, p.v0], [p.u1, p.v0], [p.u0, p.v1], [p.u1, p.v1]]);
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
        (screen_fx_solid, screen_fx_tex)
    }

    pub(super) fn build_color_fade_mesh(
        &self,
        r: &legaia_engine_render::Renderer,
    ) -> Option<UploadedColorMesh> {
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
        color_fade_mesh
    }
}

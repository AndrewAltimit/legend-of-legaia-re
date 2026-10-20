//! Per-frame mesh rebuilds for animated field geometry: morphing placements
//! and NPCs, and the posed-prop frame draws with the keys they are cached
//! under.

use super::*;

impl PlayWindowApp {
    /// Rebuild + re-upload the env-pack meshes whose VDF morph deltas moved
    /// this frame (the world's dirty set: retail op-`0x0A` ambient morph
    /// parts + the scene-entry pulse). The rebuilt mesh replaces the static
    /// upload in every draw of that mesh (`field_morph_live` substitution
    /// in the redraw loops) - the native side of the `FUN_8001C604` render
    /// substitution: staged vertices for the draw, authored rest pose
    /// untouched.
    ///
    /// REF: FUN_8001C604
    pub(in crate::window) fn take_field_morph_rebuilds(
        &mut self,
    ) -> Vec<(usize, legaia_tmd::mesh::VramMesh)> {
        let dirty = self.session.host.world.take_morph_dirty_slots();
        if dirty.is_empty() {
            return Vec::new();
        }
        let mut slots: Vec<usize> = dirty.iter().map(|&(s, _)| s).collect();
        slots.sort_unstable();
        slots.dedup();
        let Some(vram) = self.cpu_vram_base.as_ref() else {
            return Vec::new();
        };
        let mut rebuilds = Vec::new();
        for slot in slots {
            let Some(Some(mesh_idx)) = self.field_pack_mesh_idx.get(slot).copied() else {
                continue;
            };
            // The env-pack TMD the slot's mesh was uploaded from. A scene
            // whose pack is not the bundle's own (`rikuroa` streams it)
            // has no stager list entry for the slot; the upload's own
            // source is the same TMD either way.
            let Some((tmd, raw)) = self
                .field_stager_tmds
                .get(slot)
                .or_else(|| self.scene_tmd_data.get(mesh_idx))
            else {
                continue;
            };
            // Stage the deltas onto a cloned TMD (rest pose untouched).
            let mut morphed = tmd.clone();
            let mut any = false;
            for (group, obj) in morphed.objects.iter_mut().enumerate() {
                let Some(deltas) = self.session.host.world.current_morph_deltas(
                    slot,
                    group as u32,
                    obj.vertices.len(),
                ) else {
                    continue;
                };
                for (v, d) in obj.vertices.iter_mut().zip(deltas.iter()) {
                    v.x = v.x.wrapping_add(d[0]);
                    v.y = v.y.wrapping_add(d[1]);
                    v.z = v.z.wrapping_add(d[2]);
                }
                any = true;
            }
            if !any {
                continue;
            }
            let mut vmesh =
                legaia_tmd::mesh::tmd_to_vram_mesh_filtered(&morphed, raw, |cba, tsb, uvs| {
                    vram.prim_has_texture_data(cba, tsb, uvs)
                });
            if vmesh.indices.is_empty() {
                continue;
            }
            // Keep enhanced lighting's emissive tags through the morph
            // (the Genesis Tree is a VDF-morphed terrain mesh).
            legaia_render_kernels::scene_lighting::tag_emissive_vram_mesh(raw, &mut vmesh, vram);
            rebuilds.push((mesh_idx, vmesh));
        }
        rebuilds
    }

    /// Field-VM op-`0x4B` morphs on placed NPCs (`World::npc_morphed_tmd`),
    /// collected outside the renderer borrow. Drains the world's NPC dirty
    /// set: a clip-driven slot's pose-cache entries are dropped, so the pose
    /// pass re-skins it from the morphed mesh; a clip-less slot's static mesh
    /// is re-staged here (`Some` = morphed halves to upload, `None` = the
    /// lanes went dead, draw the rest upload again). A slot whose lanes are
    /// live but has no re-staged mesh yet (a scene re-upload cleared it) is
    /// re-staged too, so a HOLD-ed morph survives `upload_assets`.
    ///
    /// REF: FUN_8001C604
    #[allow(clippy::type_complexity)]
    pub(in crate::window) fn take_npc_morph_rebuilds(
        &mut self,
    ) -> Vec<(
        u8,
        Option<(legaia_tmd::mesh::VramMesh, legaia_tmd::mesh::ColorMesh)>,
    )> {
        let world = &mut self.session.host.world;
        let mut slots: std::collections::BTreeSet<u8> =
            world.take_npc_morph_dirty().into_iter().collect();
        for &s in &slots {
            self.npc_pose_cache.retain(|(k, _), _| *k != s);
            self.npc_pose_verify.retain(|(k, _), _| *k != s);
        }
        let world = &self.session.host.world;
        for &s in self.npc_rest_srcs.keys() {
            if world.npc_morph_live(s) != self.npc_morph_static.contains_key(&s) {
                slots.insert(s);
            }
        }
        let mut out = Vec::new();
        for s in slots {
            let Some((tmd, raw, pose)) = self.npc_rest_srcs.get(&s) else {
                continue;
            };
            let Some(morphed) = world.npc_morphed_tmd(s, tmd) else {
                out.push((s, None));
                continue;
            };
            let (mut vmesh, mut cmesh) = match pose {
                Some(p) => (
                    legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(&morphed, raw, p),
                    legaia_tmd::mesh::tmd_to_color_mesh_posed_rot(&morphed, raw, p),
                ),
                None => (
                    legaia_tmd::mesh::tmd_to_vram_mesh(&morphed, raw),
                    legaia_tmd::mesh::tmd_to_color_mesh(&morphed, raw),
                ),
            };
            if let Some(v) = self.cpu_vram_base.as_ref() {
                legaia_render_kernels::scene_lighting::tag_emissive_meshes(
                    raw, &mut vmesh, &mut cmesh, v,
                );
            }
            out.push((s, Some((vmesh, cmesh))));
        }
        out
    }

    /// Build this frame's posed-prop draws. A prop resting on frame 0 replays
    /// its baked rest mesh (the cheap path - and where every prop sits until it
    /// is touched); one whose clip has moved is re-posed from the raw TMD at its
    /// live cursor, blended between keyframes when its clip carries the blend
    /// gate, so the door is drawn mid-swing.
    ///
    /// Returns `(baked_vram, baked_color, live_vram, live_color)` as
    /// `(mesh index / uploaded mesh, model)` lists for the caller's draw pass.
    #[allow(clippy::type_complexity)]
    pub(in crate::window) fn posed_prop_frame_draws(
        &self,
        r: &legaia_engine_render::Renderer,
    ) -> (
        Vec<(usize, Mat4, Option<usize>)>,
        Vec<(usize, Mat4, Option<usize>)>,
        Vec<(UploadedVramMesh, Mat4, Option<usize>)>,
        Vec<(UploadedColorMesh, Mat4, Option<usize>)>,
    ) {
        // A script can move a posed prop's actor (`A3` seat, a `4C 42` lift)
        // like any placed object's; retail draws it at the actor
        // (`World::object_draw_displacements`). The bind record also keys
        // the prop's draw tint for the caller.
        let moves = self.session.host.world.object_draw_displacements();
        let turns = self.session.host.world.object_draw_turn_matrices();
        let mut baked_v = Vec::new();
        let mut baked_c = Vec::new();
        let mut live_v = Vec::new();
        let mut live_c = Vec::new();
        let Some(bundle) = self.npc_anim_bundles.0.as_ref() else {
            return (baked_v, baked_c, live_v, live_c);
        };
        // The placed-object actor cull (`field_view_window::placed_actor_visible`),
        // the same gate the static placed pass asks.
        let view_cells = legaia_engine_world_views::field_view_window::field_view_cells(
            &self.session.host.world,
            legaia_engine_scene::field_view_window::framing_is_retail(&self.session.camera)
                && !self.field_debug_camera,
        );
        for (pi, p) in self.field_posed_props.iter().enumerate() {
            let record = self
                .session
                .host
                .world
                .props
                .bank
                .props
                .get(&p.anchor)
                .map(|s| s.record);
            let base = match record.and_then(|r| turns.get(&r)) {
                Some(t) => Mat4::from_cols_array(
                    &legaia_engine_scene::field_env::turn_placed_model(&p.model.to_cols_array(), t),
                ),
                None => p.model,
            };
            let model = match record.and_then(|r| moves.get(&r)) {
                Some(d) => {
                    Mat4::from_translation(Vec3::new(d[0] as f32, d[1] as f32, d[2] as f32)) * base
                }
                None => base,
            };
            if !legaia_engine_world_views::field_view_window::placed_actor_visible(
                &self.session.host.world,
                view_cells.as_ref(),
                model.w_axis.x as i32,
                model.w_axis.z as i32,
                p.cull_radius,
            ) {
                continue;
            }
            let key = self
                .session
                .host
                .world
                .props
                .bank
                .pose_key(p.anchor)
                .unwrap_or_default();
            if key.is_rest() {
                if let Some(i) = p.baked.vram {
                    // The copy shaded at this prop's rotation, when its mesh
                    // carries light-source rows.
                    let i = match self.field_lit.posed.get(pi).copied().flatten() {
                        Some(v) => v | LIT_VARIANT_TAG,
                        None => i,
                    };
                    baked_v.push((i, model, record));
                }
                if let Some(i) = p.baked.color {
                    baked_c.push((i, model, record));
                }
                continue;
            }
            // Off the rest pose: rebuild. `FUN_8001B964` poses object `b` of the
            // mesh by part `b` of the clip through the frame blender
            // `FUN_8001BE80` at the actor's live cursor (`actor+0x68`), which is
            // exactly the `R*v + T` builder the battle / player pose path
            // already uses. The transforms come from the engine's shared
            // kernel, the one the page's prop re-pose calls too.
            let Some((tmd, raw)) = p.baked.tmd.and_then(|i| self.field_posed_tmds.get(i)) else {
                continue;
            };
            let Some(offsets) = legaia_engine_scene::field_env::prop_bone_offsets(
                bundle,
                p.anim_id,
                key,
                tmd.objects.len(),
            ) else {
                continue;
            };
            if p.baked.vram.is_some() {
                let (mut vmesh, posed_lit) =
                    legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot_lit(tmd, raw, &offsets);
                if let Some(light) = self.field_lit.light
                    && self
                        .field_lit
                        .sources
                        .contains_key(&p.baked.vram.unwrap_or(usize::MAX))
                    && std::env::var_os("LEGAIA_DIAG_NO_LIT_ROWS").is_none()
                {
                    let m3: [[f32; 3]; 3] =
                        std::array::from_fn(|row| std::array::from_fn(|col| p.model.col(col)[row]));
                    legaia_engine_field::field_lit_mesh::shade_lit_rows(
                        &mut vmesh.colors,
                        &posed_lit,
                        &light,
                        &legaia_engine_field::field_lit_mesh::rotation_from_matrix(m3),
                    );
                }
                if let Some(v) = self.cpu_vram_base.as_ref() {
                    legaia_render_kernels::scene_lighting::tag_emissive_vram_mesh(
                        raw, &mut vmesh, v,
                    );
                }
                if !vmesh.indices.is_empty()
                    && let Ok(m) = r.upload_vram_mesh(
                        &vmesh.positions,
                        &vmesh.uvs,
                        &vmesh.cba_tsb,
                        &vmesh.normals,
                        &vmesh.colors,
                        &vmesh.indices,
                    )
                {
                    live_v.push((m, model, record));
                }
            }
            if p.baked.color.is_some() {
                let mut cmesh = legaia_tmd::mesh::tmd_to_color_mesh_posed_rot(tmd, raw, &offsets);
                legaia_render_kernels::scene_lighting::tag_emissive_color_mesh(raw, &mut cmesh);
                if !cmesh.is_empty()
                    && let Ok(m) = r.upload_color_mesh_blended(
                        &cmesh.positions,
                        &cmesh.colors,
                        &cmesh.indices,
                        &cmesh.blend,
                    )
                {
                    live_c.push((m, model, record));
                }
            }
        }
        (baked_v, baked_c, live_v, live_c)
    }

    /// The distinct `(res.tmds index, anim id)` pairs the scene's **placed**
    /// objects need a posed rest mesh for: every bound placement whose bind
    /// names a nonzero anim id. `upload_assets` bakes frame 0 of each.
    pub(in crate::window) fn posed_placement_keys(&self, res: &SceneResources) -> Vec<(usize, u8)> {
        let Some(scene) = self.session.host.scene.as_ref() else {
            return Vec::new();
        };
        let (Ok(Some(placements)), Ok(Some(binds))) = (
            scene.field_object_placements(&self.session.host.index),
            scene.field_object_binds(&self.session.host.index),
        ) else {
            return Vec::new();
        };
        let env_tmds = legaia_engine_scene::field_env::env_pack_tmd_indices(scene, res);
        let floor_lut = scene
            .field_floor_height_lut(&self.session.host.index)
            .ok()
            .flatten();
        let (draws, _) = legaia_engine_scene::field_env::resolve_placed_env_draws(
            &env_tmds,
            &placements,
            floor_lut,
            Some(&binds),
        );
        let mut keys: Vec<(usize, u8)> = draws
            .iter()
            .filter(|d| d.anim_id != 0)
            .map(|d| (d.res_tmd, d.anim_id))
            .collect();
        keys.sort_unstable();
        keys.dedup();
        keys
    }
}

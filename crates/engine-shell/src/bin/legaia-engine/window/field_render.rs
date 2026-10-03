//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;

use legaia_engine_core::field_env::{FloorAnchor, FloorWave};

/// One baked placed-object draw list: the `(mesh, model)` draws, the floor
/// rungs each draw's Y came from, and which placed-object sweep owns each draw
/// ([`legaia_engine_core::field_env::placed_window_key`]), and each draw's
/// grid cell + cull radius for the visible-tile crop
/// ([`legaia_engine_core::field_view_window::CellKey`]), all parallel.
pub(super) type PlacedDrawList = (
    Vec<(usize, Mat4)>,
    Vec<FloorAnchor>,
    Vec<Option<legaia_engine_core::field_env::PlacedWindowKey>>,
    Vec<legaia_engine_core::field_view_window::CellKey>,
);

/// The live **floor-height ladder** patch for the field draw lists.
///
/// The scene's sixteen-rung elevation ladder is not static: field-VM op `0x4C`
/// nibble-9 sub-`0xE` rewrites all sixteen rungs and sub-`0..2` sets one rung
/// oscillating every frame through `FUN_801DDE34` -> `FUN_801DA930`, which is
/// the travelling wave under `jou`'s organic Seru interior (57 sites; `concnow`
/// carries 34, and `4C 90` occurs 180 times across 19 scenes). Retail's per-cell
/// terrain emitters re-read the scratchpad ladder at `0x1F80035C` every frame,
/// so the drawn ground moves with it.
///
/// The window bakes its `(mesh, model)` lists once per scene, so this keeps the
/// per-draw ladder rungs beside them and folds the difference into each
/// matrix's Y whenever the live ladder moves. Nothing happens - not even an
/// iteration - while the live ladder equals the one the lists were baked
/// against, which is every frame of every scene whose script leaves it alone.
#[derive(Default)]
pub(super) struct FieldFloorWave {
    /// MAN-frame ladder the four lists' `world_y` were resolved against.
    base: Option<[i16; 16]>,
    /// MAN-frame ladder currently folded into those matrices.
    applied: [i16; 16],
    /// Anchors parallel to `field_terrain_draws`.
    pub(super) terrain: Vec<FloorAnchor>,
    /// Anchors parallel to `field_terrain_color_draws`.
    pub(super) terrain_color: Vec<FloorAnchor>,
    /// Anchors parallel to `field_placement_draws`.
    pub(super) placement: Vec<FloorAnchor>,
    /// Anchors parallel to `field_placement_color_draws`.
    pub(super) placement_color: Vec<FloorAnchor>,
}

impl FieldFloorWave {
    /// Adopt one scene's baked ladder (`Scene::field_floor_height_lut`, MAN
    /// frame) and the four lists' per-draw rungs. Called once per scene load,
    /// after the four lists resolve.
    pub(super) fn install(
        base: Option<[i16; 16]>,
        terrain: Vec<FloorAnchor>,
        terrain_color: Vec<FloorAnchor>,
        placement: Vec<FloorAnchor>,
        placement_color: Vec<FloorAnchor>,
    ) -> Self {
        FieldFloorWave {
            base,
            applied: base.unwrap_or([0i16; 16]),
            terrain,
            terrain_color,
            placement,
            placement_color,
        }
    }

    /// Fold the world's live ladder into the four lists' Y translations.
    ///
    /// `world_lut` is `World::terrain.floor_height_lut` - the runtime
    /// **scratchpad** frame, the negation of the MAN frame held here.
    /// Returns the number of draw matrices moved this frame.
    pub(super) fn apply(
        &mut self,
        world_lut: &[i16; 16],
        lists: [&mut Vec<(usize, Mat4)>; 4],
    ) -> usize {
        if self.base.is_none() {
            return 0;
        }
        let live = world_lut.map(i16::wrapping_neg);
        if live == self.applied {
            return 0;
        }
        // The step is `live - applied`, not `live - base`: the matrices already
        // carry whatever the last frame folded in.
        let Some(step) = FloorWave::between(self.applied, live) else {
            return 0;
        };
        self.applied = live;
        let anchors = [
            &self.terrain,
            &self.terrain_color,
            &self.placement,
            &self.placement_color,
        ];
        let mut moved = 0;
        for (list, anchors) in lists.into_iter().zip(anchors) {
            for ((_, model), floor) in list.iter_mut().zip(anchors) {
                let dy = step.offset(floor);
                if dy != 0 {
                    model.w_axis.y += dy as f32;
                    moved += 1;
                }
            }
        }
        moved
    }
}

impl PlayWindowApp {
    /// Keep the cropped ground in step with this frame's visible-tile cell
    /// rectangle: re-upload the ground with the index list
    /// `field_ground::crop_indices` keeps whenever
    /// [`legaia_engine_core::field_view_window::ViewCells::stamp`] moves, and
    /// drop it when the crop is off. The browser play page re-uploads its
    /// ground indices through the same kernel on the same stamp.
    ///
    /// Re-resolve the walk ground through the world's **live** floor-height
    /// ladder and re-upload it when the ladder has moved. Retail's ground
    /// pass (PROT 0900 `FUN_801F6D48`) takes each cell's corner tiers through
    /// the ladder every frame, so a scene whose script animates it (op `0x4C`
    /// nibble 9: `jouina`'s pulsing path, `concnow`'s flesh pits) deforms the
    /// ground per vertex - the shape the floor sampler already walks the
    /// player on. Shared kernel `field_ground::live_render_positions`; the
    /// browser play page re-uploads through it too. Drops the cropped upload
    /// so [`Self::sync_ground_crop`] rebuilds it from the moved positions.
    pub(super) fn sync_ground_wave(&mut self) {
        if self.session.host.world.mode == SceneMode::WorldMap {
            return;
        }
        let live = self.session.host.world.terrain.floor_height_lut;
        let Some(src) = self.ground_src.as_mut() else {
            return;
        };
        if src.lut_applied == Some(live)
            || (src.lut_applied.is_none()
                && src.vmesh.positions
                    == legaia_engine_core::field_ground::live_render_positions(&src.hf, &live))
        {
            src.lut_applied = Some(live);
            return;
        }
        src.lut_applied = Some(live);
        src.vmesh.positions =
            legaia_engine_core::field_ground::live_render_positions(&src.hf, &live);
        src.flat_refs =
            legaia_engine_core::overworld_draw_order::ground_flat_refs(&src.vmesh.positions);
        let Some(r) = self.win.renderer.as_ref() else {
            return;
        };
        let v = &src.vmesh;
        match r.upload_vram_mesh_with_flat_refs(
            &v.positions,
            &v.uvs,
            &v.cba_tsb,
            &v.normals,
            &v.colors,
            &v.indices,
            &src.flat_refs,
        ) {
            Ok(m) => self.ground_heightfield = Some(m),
            Err(e) => log::warn!("live ground re-upload skipped: {e:#}"),
        }
        self.ground_crop = None;
    }

    pub(super) fn sync_ground_crop(
        &mut self,
        cells: Option<&legaia_engine_core::field_view_window::ViewCells>,
    ) {
        let Some(cells) = cells else {
            self.ground_crop = None;
            return;
        };
        let stamp = cells.stamp();
        if self.ground_crop.as_ref().is_some_and(|(s, _)| *s == stamp) {
            return;
        }
        let (Some(src), Some(r)) = (self.ground_src.as_ref(), self.win.renderer.as_ref()) else {
            self.ground_crop = None;
            return;
        };
        let v = &src.vmesh;
        let indices =
            legaia_engine_core::field_ground::crop_indices(&v.positions, &v.indices, Some(cells));
        // An empty crop is a real answer (no ground cell in the rectangle),
        // kept as `(stamp, None)` so the full ground does not stand in for it.
        if indices.is_empty() {
            self.ground_crop = Some((stamp, None));
            return;
        }
        self.ground_crop = r
            .upload_vram_mesh_with_flat_refs(
                &v.positions,
                &v.uvs,
                &v.cba_tsb,
                &v.normals,
                &v.colors,
                &indices,
                &src.flat_refs,
            )
            .map_err(|e| log::warn!("cropped ground upload skipped: {e:#}"))
            .ok()
            .map(|m| (stamp, Some(m)));
    }

    /// Resolve the field static-geometry placement draws for the current
    /// scene: each placed environment object's scene-pack mesh paired with a
    /// world model matrix. Built from the field map's object table
    /// (`Scene::field_object_placements`) and the scene_asset_table TMD pack;
    /// the per-object pack index resolves via `legaia_asset::field_objects`.
    ///
    /// `tmd_src_index[j]` is the `res.tmds` index of uploaded mesh `j` (meshes
    /// skip empty-prim TMDs, so this bridges back). Returns empty for scenes
    /// with no field map / no bundle (e.g. battle or world-map blocks).
    ///
    /// World Y is left at the ground plane for now; the per-tile floor-height
    /// LUT (MAN header) is a separate refinement.
    pub(super) fn resolve_field_placement_draws(
        &self,
        res: &SceneResources,
        tmd_src_index: &[usize],
        posed: &PosedPlacementMeshes,
        textured: bool,
    ) -> PlacedDrawList {
        let Some(scene) = self.session.host.scene.as_ref() else {
            return Default::default();
        };
        let placements = match scene.field_object_placements(&self.session.host.index) {
            Ok(Some(p)) if !p.is_empty() => p,
            _ => return Default::default(),
        };
        let binds = scene
            .field_object_binds(&self.session.host.index)
            .ok()
            .flatten();
        // Field frame: raw retail-convention transforms (the camera's
        // FIELD_WORLD_FLIP provides the single net Y negation).
        self.resolve_placement_draws(
            res,
            tmd_src_index,
            &placements,
            false,
            binds.as_ref(),
            Some((posed, textured)),
        )
    }

    /// The scene's **posed placed props**, one entry per placement (not per
    /// `(mesh, anim)` pair).
    ///
    /// A `.MAP` placed object whose object bind names an animation is a
    /// multi-object prop posed by that clip, and the clip is what makes a Rim
    /// Elm house door swing: the bind record's script holds the prop on frame 0
    /// at spawn (`0x4C 0x35`) and its resumable body clears the hold bit when
    /// the touch / interact dispatch runs the record through the field VM. The
    /// **live animation bank lives on the world**
    /// (`World::props.bank`, installed at field entry and ticked by
    /// `World::tick_prop_interactions`) - the draw pass here only reads each
    /// prop's current frame.
    ///
    /// Props whose clip does not resolve (no ANM bundle, a bone-count mismatch)
    /// yield no entry, and `resolve_placement_draws` then falls back to the raw
    /// unposed mesh for them exactly as before.
    pub(super) fn resolve_posed_props(
        &self,
        res: &SceneResources,
        posed: &PosedPlacementMeshes,
        bundle: Option<&legaia_asset::player_anm::PlayerAnmBundle>,
    ) -> Vec<PosedPropDraw> {
        use legaia_engine_core::field_env;
        let Some(scene) = self.session.host.scene.as_ref() else {
            return Vec::new();
        };
        if bundle.is_none() {
            return Vec::new();
        }
        let (Ok(Some(placements)), Ok(Some(binds))) = (
            scene.field_object_placements(&self.session.host.index),
            scene.field_object_binds(&self.session.host.index),
        ) else {
            return Vec::new();
        };
        let env_tmds = field_env::env_pack_tmd_indices(scene, res);
        let floor_lut = scene
            .field_floor_height_lut(&self.session.host.index)
            .ok()
            .flatten();
        let (mut draws, _) =
            field_env::resolve_placed_env_draws(&env_tmds, &placements, floor_lut, Some(&binds));
        // Same story-hidden gate as the static placement pass: a parked
        // prop's clip must not draw either.
        field_env::retain_visible_placed_draws(
            &mut draws,
            &binds,
            &self.session.host.world.hidden_object_records(),
        );

        let scales = field_env::placed_render_scales(
            &draws,
            Some(&binds),
            &self.session.host.world.object_render_scales(),
        );
        let bank = &self.session.host.world.props.bank;
        let mut props = Vec::new();
        for (d, &scale) in draws.iter().zip(&scales) {
            if d.anim_id == 0 || !bank.props.contains_key(&d.anchor) {
                continue;
            }
            let Some(&baked) = posed.get(&(d.res_tmd, d.anim_id)) else {
                continue; // no baked pose - the unposed fallback draws it
            };
            // Same coplanar lift as the static placement pass (see
            // `resolve_placement_draws`), so a posed prop stays consistent
            // with the tile it rests on.
            let off = self
                .coplanar_env_offsets
                .get(d)
                .copied()
                .unwrap_or([0.0; 3]);
            let t = Mat4::from_translation(Vec3::new(
                d.world_x as f32 + off[0],
                d.world_y as f32 + off[1],
                d.world_z as f32 + off[2],
            ));
            let rot =
                legaia_engine_render::battle_intro::placement_rotation(d.rot_x, d.rot_y, d.rot_z)
                    * Mat4::from_scale(Vec3::splat(scale));
            props.push(PosedPropDraw {
                anchor: d.anchor,
                anim_id: d.anim_id,
                model: t * rot,
                baked,
            });
        }
        log::info!(
            "play-window: {} posed placed props ({} of them animate on touch/interact)",
            props.len(),
            bank.props.values().filter(|p| p.program.animates()).count(),
        );
        props
    }

    /// Rebuild the cross-draw coplanar-offset map for the current scene: the
    /// combined terrain + placed [`legaia_engine_core::field_env::EnvDraw`]
    /// lists (field scenes) or the walk landmark + decoration lists (world
    /// map), run through `legaia_engine_core::coplanar_draws`. Overlapping
    /// same-plane draws otherwise z-fight - retail painter-orders them
    /// through the ordering table, the port's depth buffer ties per pixel.
    ///
    /// Resolves the layers with the same inputs the draw resolvers use so
    /// the map's `EnvDraw` keys match theirs by identity.
    /// Resolve the scene's complete STATIC env draw list (terrain tiles +
    /// placed objects; world-map scenes use the walk layers) - the shared
    /// input of the coplanar-lift pass and the occlusion-fade gate's
    /// occluder set.
    pub(super) fn static_env_draws(
        &self,
        res: &SceneResources,
    ) -> Vec<legaia_engine_core::field_env::EnvDraw> {
        use legaia_engine_core::field_env;
        let Some(scene) = self.session.host.scene.as_ref() else {
            return Vec::new();
        };
        let index = &self.session.host.index;
        let env_tmds = field_env::env_pack_tmd_indices(scene, res);
        if env_tmds.is_empty() {
            return Vec::new();
        }
        let floor_lut = scene.field_floor_height_lut(index).ok().flatten();
        let mut draws: Vec<field_env::EnvDraw> = Vec::new();
        if legaia_engine_core::scene::is_world_map_scene(&scene.name) {
            let mut tiles = scene
                .walk_object_placements(index)
                .ok()
                .flatten()
                .unwrap_or_default();
            if let Ok(Some(deco)) = scene.walk_decoration_placements(index) {
                tiles.extend(deco);
            }
            let (d, _) = field_env::resolve_env_draws(&env_tmds, &tiles, floor_lut);
            draws.extend(d);
        } else {
            if let Ok(Some(tiles)) = scene.field_terrain_tiles(index) {
                let tiles: Vec<_> = tiles
                    .into_iter()
                    .filter(|p| p.flags & legaia_asset::field_objects::FLAG_PLACED == 0)
                    .collect();
                let (d, _) = field_env::resolve_env_draws(&env_tmds, &tiles, floor_lut);
                draws.extend(d);
            }
            if let Ok(Some(placements)) = scene.field_object_placements(index) {
                let binds = scene.field_object_binds(index).ok().flatten();
                let (d, _) = field_env::resolve_placed_env_draws(
                    &env_tmds,
                    &placements,
                    floor_lut,
                    binds.as_ref(),
                );
                draws.extend(d);
            }
        }
        draws
    }

    pub(super) fn compute_coplanar_env_offsets(
        &self,
        res: &SceneResources,
    ) -> std::collections::HashMap<legaia_engine_core::field_env::EnvDraw, [f32; 3]> {
        use legaia_engine_core::coplanar_draws;
        let draws = self.static_env_draws(res);
        if draws.is_empty() {
            return Default::default();
        }
        let planes = coplanar_draws::draw_plane_summaries(&draws, res);
        let offs = coplanar_draws::coplanar_draw_offsets(&draws, &planes);
        if !offs.is_empty() {
            log::info!(
                "play-window: {} coplanar draw lifts (of {} env draws)",
                offs.len(),
                draws.len()
            );
        }
        offs
    }

    /// The scene's **world-space** bounding box, for the world map's top-view
    /// debug camera.
    ///
    /// Built off the same static draw list the occluder set and the coplanar
    /// pass use, through the shared kernel
    /// `engine_core::field_env::env_draws_world_aabb` the browser play page
    /// also calls. It used to be the union of the uploaded meshes' *local*
    /// vertex extents: every env-pack mesh is authored about its own origin,
    /// so that box sat near `(0,0,0)` while the geometry it was meant to
    /// frame sits at placement coordinates, and the top view framed the origin
    /// corner with the map off to one side.
    ///
    /// `None` leaves the previous box in place - a scene with no static env
    /// geometry has nothing to frame.
    pub(super) fn scene_world_aabb(&self, res: &SceneResources) -> Option<([f32; 3], [f32; 3])> {
        let draws = self.static_env_draws(res);
        let ground = self
            .session
            .host
            .scene
            .as_ref()
            .and_then(|sc| sc.walk_heightfield(&self.session.host.index).ok().flatten());
        legaia_engine_core::field_env::env_draws_world_aabb(&[&draws], res, ground.as_ref())
    }

    /// Build the occlusion-fade visibility gate's world-space occluder set
    /// from the same static draw list the render layers use (see
    /// `legaia_engine_core::field_occlusion`). Rebuilt per scene load.
    pub(super) fn build_field_occluders(
        &self,
        res: &SceneResources,
    ) -> legaia_engine_core::field_occlusion::FieldOccluders {
        let draws = self.static_env_draws(res);
        let occ = legaia_engine_core::field_occlusion::FieldOccluders::build(&[&draws], res);
        if !occ.is_empty() {
            log::info!(
                "play-window: occlusion-fade gate armed with {} static triangles",
                occ.triangle_count()
            );
        }
        occ
    }

    /// Rebuild + re-upload the env-pack meshes whose VDF morph deltas moved
    /// this frame (the world's dirty set: retail op-`0x0A` ambient morph
    /// parts + the scene-entry pulse). The rebuilt mesh replaces the static
    /// upload in every draw of that mesh (`field_morph_live` substitution
    /// in the redraw loops) - the native side of the `FUN_8001C604` render
    /// substitution: staged vertices for the draw, authored rest pose
    /// untouched.
    ///
    /// REF: FUN_8001C604
    pub(super) fn take_field_morph_rebuilds(&mut self) -> Vec<(usize, legaia_tmd::mesh::VramMesh)> {
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
            let vmesh =
                legaia_tmd::mesh::tmd_to_vram_mesh_filtered(&morphed, raw, |cba, tsb, uvs| {
                    vram.prim_has_texture_data(cba, tsb, uvs)
                });
            if vmesh.indices.is_empty() {
                continue;
            }
            rebuilds.push((mesh_idx, vmesh));
        }
        rebuilds
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
    pub(super) fn posed_prop_frame_draws(
        &self,
        r: &legaia_engine_render::Renderer,
    ) -> (
        Vec<(usize, Mat4)>,
        Vec<(usize, Mat4)>,
        Vec<(UploadedVramMesh, Mat4)>,
        Vec<(UploadedColorMesh, Mat4)>,
    ) {
        let mut baked_v = Vec::new();
        let mut baked_c = Vec::new();
        let mut live_v = Vec::new();
        let mut live_c = Vec::new();
        let Some(bundle) = self.npc_anim_bundles.0.as_ref() else {
            return (baked_v, baked_c, live_v, live_c);
        };
        for p in &self.field_posed_props {
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
                    baked_v.push((i, p.model));
                }
                if let Some(i) = p.baked.color {
                    baked_c.push((i, p.model));
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
            let Some(offsets) = legaia_engine_core::field_env::prop_bone_offsets(
                bundle,
                p.anim_id,
                key,
                tmd.objects.len(),
            ) else {
                continue;
            };
            if p.baked.vram.is_some() {
                let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(tmd, raw, &offsets);
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
                    live_v.push((m, p.model));
                }
            }
            if p.baked.color.is_some() {
                let cmesh = legaia_tmd::mesh::tmd_to_color_mesh_posed_rot(tmd, raw, &offsets);
                if !cmesh.is_empty()
                    && let Ok(m) = r.upload_color_mesh_blended(
                        &cmesh.positions,
                        &cmesh.colors,
                        &cmesh.indices,
                        &cmesh.blend,
                    )
                {
                    live_c.push((m, p.model));
                }
            }
        }
        (baked_v, baked_c, live_v, live_c)
    }

    /// The distinct `(res.tmds index, anim id)` pairs the scene's **placed**
    /// objects need a posed rest mesh for: every bound placement whose bind
    /// names a nonzero anim id. `upload_assets` bakes frame 0 of each.
    pub(super) fn posed_placement_keys(&self, res: &SceneResources) -> Vec<(usize, u8)> {
        let Some(scene) = self.session.host.scene.as_ref() else {
            return Vec::new();
        };
        let (Ok(Some(placements)), Ok(Some(binds))) = (
            scene.field_object_placements(&self.session.host.index),
            scene.field_object_binds(&self.session.host.index),
        ) else {
            return Vec::new();
        };
        let env_tmds = legaia_engine_core::field_env::env_pack_tmd_indices(scene, res);
        let floor_lut = scene
            .field_floor_height_lut(&self.session.host.index)
            .ok()
            .flatten();
        let (draws, _) = legaia_engine_core::field_env::resolve_placed_env_draws(
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

    /// Resolve the field scene's **terrain / ground** tiles (the `CELL_VISIBLE`
    /// sweep in `Scene::field_terrain_tiles`) to `(mesh, model)` draws, the same
    /// way `resolve_field_placement_draws` resolves the placed objects. This is
    /// the town's floor / ground layer; without it a field scene renders its
    /// buildings floating over the bare clear colour.
    ///
    /// Records carrying the *placed* flag are excluded: they are already drawn
    /// by `resolve_field_placement_draws`, from the same record and at the same
    /// transform, so a visible cell pointing at one would stamp a second, and
    /// the second copy would be the **unposed** one (the placement layer poses
    /// its multi-object props). Keeping the two layers disjoint is the same rule
    /// `field_objects::parse_walk_decorations` applies on the world map.
    pub(super) fn resolve_field_terrain_draws(
        &self,
        res: &SceneResources,
        tmd_src_index: &[usize],
    ) -> (
        Vec<(usize, Mat4)>,
        Vec<legaia_engine_core::field_env::FloorAnchor>,
        Vec<legaia_engine_core::field_view_window::CellKey>,
    ) {
        let Some(scene) = self.session.host.scene.as_ref() else {
            return Default::default();
        };
        let tiles: Vec<legaia_asset::field_objects::Placement> =
            match scene.field_terrain_tiles(&self.session.host.index) {
                Ok(Some(t)) => t
                    .into_iter()
                    .filter(|p| p.flags & legaia_asset::field_objects::FLAG_PLACED == 0)
                    .collect(),
                _ => return Default::default(),
            };
        if tiles.is_empty() {
            return Default::default();
        }
        // Field frame: raw retail-convention transforms (see above).
        let (draws, floors, _, cells) =
            self.resolve_placement_draws(res, tmd_src_index, &tiles, false, None, None);
        (draws, floors, cells)
    }

    /// World-map continent terrain draws: the dense visible-tile set
    /// (`Scene::field_terrain_tiles`, the `FUN_801F69D8` overhead sweep) rather
    /// than the placed-flag interactive objects. Tiles whose pack index falls
    /// outside the loaded slot-1 landmark pack (they reference the wider global
    /// TMD pool, not yet loaded for the world map) resolve to no mesh and are
    /// skipped by `resolve_placement_draws`.
    pub(super) fn resolve_world_map_terrain_draws(
        &self,
        res: &SceneResources,
        tmd_src_index: &[usize],
    ) -> (Vec<(usize, Mat4)>, usize) {
        let Some(scene) = self.session.host.scene.as_ref() else {
            return (Vec::new(), 0);
        };
        // Free-roam walk view: read the *walk* `.MAP` (`Scene::walk_field_map_
        // index`, the `block_start - 2` entry the runtime resolves through
        // `toc[idx+2]`) and sweep its `0x1000`-gated continent (`walk_terrain_
        // tiles`), then the placed-flag landmarks from the same `.MAP`. The
        // earlier path read the within-block decoy entry with the overhead
        // `0x2000` gate, which for the kingdoms resolved a different map and
        // produced the sparse mesh scatter.
        // Two sparse pack-mesh layers on top of the heightfield ground:
        // the placed landmarks (FUN_8003A55C, flags & 0x4) and the
        // decoration layer (walk-visible cells with a nonzero record[+0x10]
        // and no placed flag - the crossed-quad trees, mountain groups, and
        // props). The bulk continent ground is NOT per-cell pack meshes (the
        // old `walk_terrain_tiles` sweep floods 97% of cells with pool-5
        // because their record[+0x10] is 0); it is the heightfield surface
        // built separately in `upload_assets` (`Scene::walk_heightfield`).
        // See docs/subsystems/world-map.md.
        //
        // The two layers resolve separately so the caller knows where the
        // decorations start: retail's decoration sweep (`FUN_801F69D8`)
        // depth-cues each decoration by its origin's depth
        // (`legaia_engine_core::overworld_ground_cue::decoration_draw_cue`),
        // the landmarks it skips take no cue from it.
        let landmarks = match scene.walk_object_placements(&self.session.host.index) {
            Ok(Some(t)) => t,
            _ => Vec::new(),
        };
        let deco = match scene.walk_decoration_placements(&self.session.host.index) {
            Ok(Some(t)) => t,
            _ => Vec::new(),
        };
        // World-map frame: raw retail-convention transforms - both world-map
        // cameras compose FIELD_WORLD_FLIP (the walk view through the pinned
        // retail composition), so the draws are unflipped like the field's.
        let mut draws = if landmarks.is_empty() {
            Vec::new()
        } else {
            self.resolve_placement_draws(res, tmd_src_index, &landmarks, false, None, None)
                .0
        };
        let deco_start = draws.len();
        if !deco.is_empty() {
            draws.extend(
                self.resolve_placement_draws(res, tmd_src_index, &deco, false, None, None)
                    .0,
            );
        }
        (draws, deco_start)
    }

    /// Resolve the world-map water/CLUT-cell animation for the active scene.
    ///
    /// Retail path: the kingdom bundle's slot 5 (the type-byte `0x06` slot of
    /// PROT 0085 / 0244 / 0391) is the CLUT-walk animation table - eight
    /// independent 16x1 `MoveImage` walkers (ocean head + shoreline/terrain
    /// shimmer cells), parsed by [`legaia_asset::clut_walk`]. Parsing it also
    /// parks the walkers' VRAM source strips into the CPU VRAM (see
    /// [`Self::park_clut_walk_strips`]): the strips ship in the bundle's
    /// slot-0 TIM_LIST as raw CLUT-block records without the TIM magic, so
    /// the scene TIM pre-pass skips them.
    ///
    /// Fallback: the legacy single-cell 13-frame ocean-head cycle
    /// ([`legaia_asset::ocean::find_ocean_assets`]), used ONLY when no scene
    /// entry yields a parseable slot-5 table. The fallback exists for
    /// modified / damaged bundles (every retail kingdom ships slot 5): it
    /// keeps the most visible water shimmer alive rather than freezing the
    /// sea, at the cost of the seven non-ocean shimmer cells.
    // REF: FUN_8001f05c - asset-type dispatch case 6 installs the decoded
    // slot-5 table at DAT_8007B7C8; FUN_801d6704 (field init) spawns one
    // walker actor per entry via FUN_80024cfc.
    pub(super) fn resolve_ocean_anim(&mut self) -> Option<WaterAnim> {
        // Resolve + park through the one engine kernel the browser page and
        // the field-scene viewer also call (`ClutWalkAnim::install`): the
        // scene bundle's type-6 table on a field scene, the kingdom's slot-5
        // table on an overworld, the Drake complement rows, and the legacy
        // ocean-head fallback.
        let scene = self.session.host.scene.as_ref()?;
        let base = self.cpu_vram_base.as_mut()?;
        let install = legaia_engine_core::clut_walk_anim::ClutWalkAnim::install(
            scene,
            &self.session.host.index,
            base,
        )?;
        if install.ocean_fallback {
            log::warn!(
                "play-window: no slot-5 CLUT-walk table in the kingdom bundle; \
                 falling back to the legacy ocean-head cycle"
            );
        }
        for (x, y) in &install.missing_cells {
            log::warn!(
                "play-window: CLUT-walk source cell ({x}, {y}) has no VRAM data - \
                 the walker will copy blank entries (strip residency gap)"
            );
        }
        Some(WaterAnim {
            anim: install.anim,
            vsyncs_to_game_tick: 0,
        })
    }

    /// Advance the world-map water/CLUT-cell animation one sim tick, in
    /// retail vsync units: only the sim ticks that map to a retail vsync
    /// (`World::clock.display_frame_step`) advance the clock, and a retail *game
    /// tick* lands every `World::clock.frame_step` vsyncs (the adaptive
    /// `DAT_1F800393` factor `FUN_80016B6C` writes - `3` on the overworld,
    /// `2` in towns).
    ///
    /// Table path (retail): each slot-5 entry is an independent walker.
    /// Per game tick every accumulator banks `dt` vsyncs; when one crosses
    /// its current frame's `hold_vsyncs` it emits a 16x1 VRAM->VRAM
    /// `MoveImage` from the parked source strip onto the entry's
    /// destination cell, **resets the accumulator to zero** (NOT
    /// subtract-remainder: live captures show strictly constant intervals -
    /// hold 8 at dt 3 fires every 9 vsyncs with zero jitter, which only a
    /// reset produces), and advances the frame index with wrap-around. The
    /// real interval is therefore `ceil(hold / dt) * dt` vsyncs.
    ///
    /// The CPU VRAM is re-uploaded only when at least one copy fired, so
    /// the whole-VRAM upload runs a few times a second, not every frame.
    // REF: FUN_8001ada4 - stepped by `legaia_engine_core::clut_walk_anim`.
    pub(super) fn advance_ocean_animation(&mut self) {
        // While a battle is up the GPU texture holds the BATTLE VRAM (party
        // band + palettes + monster pages); the water cells aren't visible
        // under the battle stage, and re-uploading the field snapshot here
        // would clobber that texture so every battle mesh samples field
        // bytes (white speckle on the party band). Hold the shimmer until
        // the field VRAM is restored at battle exit.
        if self.session.host.world.mode == SceneMode::Battle {
            return;
        }
        // Only the sim ticks that map to a retail vsync advance the clock
        // (the 100 Hz sim carries ~60 vsyncs/s; see `World::clock.display_frame_step`).
        if self.session.host.world.clock.display_frame_step == 0 {
            return;
        }
        let dt = u32::from(self.session.host.world.clock.frame_step.max(1));
        let (Some(w), Some(base)) = (self.ocean_anim.as_mut(), self.cpu_vram_base.as_mut()) else {
            return;
        };
        // A retail game tick lands every `dt` vsyncs; the stepper banks `dt`
        // on each (`legaia_engine_core::clut_walk_anim`).
        w.vsyncs_to_game_tick += 1;
        if w.vsyncs_to_game_tick < dt {
            return;
        }
        w.vsyncs_to_game_tick = 0;
        if !w.anim.game_tick(dt, base) {
            return;
        }
        if let Some(r) = self.win.renderer.as_ref() {
            match r.upload_vram(base) {
                Ok(v) => self.uploaded_vram = Some(v),
                Err(e) => log::error!("play-window: water CLUT re-upload: {e:#}"),
            }
        }
    }

    /// Apply the world's scripted VRAM effects - the field-VM `0x4C` n6
    /// sub-`0x60` `MoveImage` stamps (`World::apply_script_vram_moves`, e.g.
    /// the town01 opening's Noa face-frame stamps) and the sub-`0x61`
    /// CLUT-cell one-shots + cross-fades (`World::step_clut_fx`) - against
    /// the CPU VRAM and re-upload when anything changed. `World::tick` banks
    /// the retail game ticks (every `frame_step` vsyncs); this drains them
    /// once per sim tick. Battle-guarded for the same reason as
    /// [`Self::advance_ocean_animation`]: while a battle is up the GPU
    /// texture holds the battle VRAM and a field re-upload would clobber it.
    pub(super) fn apply_world_clut_fx(&mut self) {
        // The shared frame-tail kernel runs every step every tick (each
        // drains its own tick backlog and self-gates) and holds the battle
        // guard. This used to skip the whole call while four effect lists
        // were empty - a test that left out the `4C DB` blend fades, so a
        // lone blend fade never stepped, and that banked the idle ticks a
        // later effect then consumed at once.
        let Some(base) = self.cpu_vram_base.as_mut() else {
            return;
        };
        if !self.session.host.world.step_field_vram_effects(base, false) {
            return;
        }
        if let Some(r) = self.win.renderer.as_ref() {
            match r.upload_vram(base) {
                Ok(v) => self.uploaded_vram = Some(v),
                Err(e) => log::error!("play-window: scripted CLUT-fx re-upload: {e:#}"),
            }
        }
    }

    /// Shared placement -> world-transform resolver for both the field static-
    /// object layer and the world-map continent terrain. Maps each placement's
    /// scene-pack mesh index through the uploaded-mesh bridge and builds its
    /// world model matrix.
    ///
    /// `binds` (the placed layer only) applies retail's spawn gate: an object
    /// with no bind at its anchor tile is skipped, exactly as `FUN_8003A55C`
    /// skips the tile. `posed` then swaps in the baked frame-0 rest mesh for any
    /// bind that names an animation - the multi-object props, whose TMD objects
    /// are that clip's bones and are nonsense without its transform. The `bool`
    /// selects which uploaded-mesh list the caller is bridging (textured vs
    /// colour), since a posed prop has one slot in each.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn resolve_placement_draws(
        &self,
        res: &SceneResources,
        tmd_src_index: &[usize],
        placements: &[legaia_asset::field_objects::Placement],
        flip_y: bool,
        binds: Option<
            &std::collections::HashMap<(u8, u8), legaia_engine_core::field_env::ObjectBind>,
        >,
        posed: Option<(&PosedPlacementMeshes, bool)>,
    ) -> PlacedDrawList {
        let Some(scene) = self.session.host.scene.as_ref() else {
            return Default::default();
        };
        if placements.is_empty() {
            return Default::default();
        }
        // Per-tile floor-height LUT (MAN header). World Y for a placed object
        // is `-lut[tile_floor_nibble] + y_off`; without it the town renders on
        // a flat plane (Rim Elm is on a cliff with real elevation changes).
        let floor_lut = scene
            .field_floor_height_lut(&self.session.host.index)
            .ok()
            .flatten();
        // The environment meshes are the scene's geometry-pack TMDs, in scan
        // order; `pack_index` indexes that subset of `res.tmds`. Pack
        // selection (the per-entry TMD-count vote) + the placement -> draw
        // resolution live in the shared kernel `engine_core::field_env`, so
        // the web viewer's assembled scene view resolves the exact same
        // draws; this method only adds the uploaded-mesh bridge and the
        // render-frame model matrix.
        let env_tmds = legaia_engine_core::field_env::env_pack_tmd_indices(scene, res);
        if env_tmds.is_empty() {
            return Default::default();
        }
        let (mut env_draws, dropped) = legaia_engine_core::field_env::resolve_placed_env_draws(
            &env_tmds, placements, floor_lut, binds,
        );
        // Story-hidden placed objects: a bind record's spawn prologue that
        // parked its actor at the hide box (flag-gated scenery like town01's
        // gate rocks) draws nothing, exactly as retail draws the actor rather
        // than the raw `.MAP` table.
        if let Some(binds) = binds {
            legaia_engine_core::field_env::retain_visible_placed_draws(
                &mut env_draws,
                binds,
                &self.session.host.world.hidden_object_records(),
            );
        }
        // Render scale: a bind record's prologue can leave the actor's
        // `+0x72` at a non-unit value (town01's horizon backdrop draws at a
        // quarter), and retail's case-5 draw folds it into the model matrix.
        let scales = legaia_engine_core::field_env::placed_render_scales(
            &env_draws,
            binds,
            &self.session.host.world.object_render_scales(),
        );
        let diag = std::env::var_os("LEGAIA_DIAG_PLACE").is_some();
        if diag {
            for d in &dropped {
                match d {
                    legaia_engine_core::field_env::EnvDrawDrop::NoPackIndex {
                        world_x,
                        world_z,
                    } => {
                        log::info!("DIAG place drop: no pack_index at ({world_x}, {world_z})");
                    }
                    legaia_engine_core::field_env::EnvDrawDrop::SlotOutOfRange {
                        pack_index,
                        world_x,
                        world_z,
                    } => {
                        log::info!(
                            "DIAG place drop: pack_index {} out of range ({} env tmds) at ({}, {})",
                            pack_index,
                            env_tmds.len(),
                            world_x,
                            world_z
                        );
                    }
                    legaia_engine_core::field_env::EnvDrawDrop::Unbound {
                        anchor,
                        world_x,
                        world_z,
                    } => {
                        log::info!(
                            "DIAG place drop: anchor tile {anchor:?} has no object bind \
                             (retail never spawns it) at ({world_x}, {world_z})"
                        );
                    }
                }
            }
        }
        // res.tmds index -> uploaded-mesh index (None where the mesh was
        // dropped for having no renderable prims).
        let mut res_to_mesh: Vec<Option<usize>> = vec![None; res.tmds.len()];
        for (mesh_idx, &src) in tmd_src_index.iter().enumerate() {
            if let Some(slot) = res_to_mesh.get_mut(src) {
                *slot = Some(mesh_idx);
            }
        }
        let mut draws = Vec::new();
        // Parallel to `draws`: which ladder rungs each draw's Y came from, so
        // the per-frame floor wave (`FieldFloorWave`) can move the drawn ground
        // when a script sets a rung oscillating.
        let mut floors = Vec::new();
        // Parallel to `draws`: which placed-object sweep owns each draw
        // (`Some` = the sub-area window sweep's, gated per frame on the
        // world's windowed static-object list by `field_env::placed_draw_live`).
        let mut window_keys = Vec::new();
        // Parallel to `draws`: the grid cell + cull radius the visible-tile
        // crop (`field_view_window::terrain_draw_visible`) tests per frame.
        let mut cell_keys = Vec::new();
        for (d, &scale) in env_draws.iter().zip(&scales) {
            // A bind with an anim id means the prop's TMD objects are that
            // clip's bones, and the clip is live (a house door swings open on
            // contact). Those props are drawn from `field_posed_props`, which
            // owns one entry per placement and re-poses it at its own frame -
            // so hand them over rather than emitting a static instance here.
            // When the pose is unavailable (no scene ANM bundle, or a
            // bone-count mismatch) `upload_assets` has already logged it and we
            // fall back to the unposed mesh rather than losing the object.
            let posed_idx = match (d.anim_id, posed) {
                (0, _) | (_, None) => None,
                (anim, Some((table, _))) => table.get(&(d.res_tmd, anim)).map(|_| ()),
            };
            let mesh_idx = match posed_idx {
                Some(()) => continue, // owned by the posed-prop pass
                None => match res_to_mesh[d.res_tmd] {
                    Some(idx) => idx,
                    None => {
                        if diag {
                            log::info!(
                                "DIAG place drop: pack {} (res {}) not in this mesh bridge \
                                 at ({}, {})",
                                d.env_slot,
                                d.res_tmd,
                                d.world_x,
                                d.world_z
                            );
                        }
                        continue;
                    }
                },
            };
            // PSX field coords (same retail Y-down convention as actor
            // positions). `flip_y` selects the render-frame pairing: the
            // world-map cameras carry no world negation, so their draws keep
            // the per-model flip; the FIELD frame draws raw vertices and the
            // camera's FIELD_WORLD_FLIP provides the single net negation
            // (elevation renders retail-correct).
            //
            // `coplanar_env_offsets` lifts draws whose surfaces coincide with
            // a larger/earlier draw's plane (sub-2-unit, toward the visible
            // side) so overlapping tiles resolve deterministically instead of
            // z-fighting.
            let off = self
                .coplanar_env_offsets
                .get(d)
                .copied()
                .unwrap_or([0.0; 3]);
            let t = Mat4::from_translation(Vec3::new(
                d.world_x as f32 + off[0],
                d.world_y as f32 + off[1],
                d.world_z as f32 + off[2],
            ));
            // All three authored angles from the object record
            // (`+0x08`/`+0x0A`/`+0x0C`), composed in retail's `Rx * Ry * Rz`
            // order. Yaw alone (bridge quarter-turns, tree variety) covers
            // most placements, but a minority across the disc carry a real
            // tilt, and dropping it also displaces any mesh authored off its
            // own origin - the rotation is about the origin, not the
            // geometry's centre.
            //
            // Both branches below build `rot` in the RETAIL frame: the field
            // pairing draws raw vertices and lets the camera's
            // FIELD_WORLD_FLIP supply the single net negation, and the
            // world-map pairing applies the per-model flip on the left of
            // `rot`. So the same matrix is correct for both.
            let rot =
                legaia_engine_render::battle_intro::placement_rotation(d.rot_x, d.rot_y, d.rot_z)
                    * Mat4::from_scale(Vec3::splat(scale));
            let model = if flip_y {
                t * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0)) * rot
            } else {
                t * rot
            };
            if diag {
                log::info!(
                    "DIAG place keep: pack {} (res {} -> mesh {}) at ({}, {}, {}) rot {}",
                    d.env_slot,
                    d.res_tmd,
                    mesh_idx,
                    d.world_x,
                    d.world_y,
                    d.world_z,
                    d.rot_y & 0x0FFF
                );
            }
            draws.push((mesh_idx, model));
            floors.push(d.floor);
            window_keys.push(legaia_engine_core::field_env::placed_window_key(d, binds));
            cell_keys.push(legaia_engine_core::field_view_window::CellKey::of_draw(d));
        }
        log::info!(
            "play-window: {} field placement draws ({} placements, {} env meshes)",
            draws.len(),
            placements.len(),
            env_tmds.len(),
        );
        (draws, floors, window_keys, cell_keys)
    }

    /// Debug-install a synthetic tile board (`LEGAIA_TILE_BOARD_DEMO=1`) so
    /// the per-cell tile-actor draw pass can be exercised visually: no
    /// retail scene MAN carries an op-0x49 sub-5 install (the census in
    /// `tests/tile_board_draw_live.rs` pins that), so without this the
    /// board renderer has no reachable on-screen trigger. Builds the same
    /// 14-byte `[0x49, sub-op 5, header]` window the field VM would hand
    /// `op49_menu_request`, centred a few tiles off the player so the
    /// follow camera frames it, with the tile templates pointed at the
    /// resident global-pool head (the effect-model library at `3..`).
    /// One-shot per scene: a no-op while a board is up or armed.
    pub(super) fn maybe_install_demo_tile_board(&mut self) {
        if std::env::var_os("LEGAIA_TILE_BOARD_DEMO").is_none() {
            return;
        }
        let world = &mut self.session.host.world;
        if world.install_demo_tile_board() {
            log::info!(
                "play-window: demo tile board installed ({} draw-list cells)",
                world.board.draw_list.len()
            );
        }
    }

    /// Retail's post-FMV control transfer, through the session (which also
    /// resets the camera globals and drops the queued SFX cues on a scene
    /// swap - no scene bank is staged, as retail's field init loads none),
    /// plus the render-side rebuild when the hand-off entered a new scene.
    /// The hand-off loads its scene outside the field VM's transition op, so
    /// no `SceneEntered` event follows it - the window used to only log the
    /// outcome and draw the trigger scene's meshes over the new world, which
    /// the browser play page already rebuilt (`runtime.rs`, the
    /// `fmv_handoff_scene` arm of `tick_frame`).
    pub(super) fn apply_fmv_handoff(&mut self) {
        if let Some(outcome) = self.session.apply_pending_fmv_handoff() {
            log::info!("cutscene: {outcome}");
            if matches!(
                outcome,
                legaia_engine_core::scene::FmvHandoffOutcome::Entered { .. }
            ) {
                self.rebuild_scene_render_state();
            }
        }
    }

    /// Rebuild the window's render-side scene state after the host swapped
    /// scenes under it (a door transition: `SceneTickEvent::SceneEntered`).
    /// Rebuilds [`SceneResources`] for the newly loaded scene and re-runs
    /// [`Self::upload_assets`], which replaces the VRAM, mesh list, actor
    /// bindings, player mesh + locomotion clips, NPC/prop draws, terrain and
    /// placement draw lists wholesale. Soft-fails (logs, keeps the stale
    /// scene render) so a bad destination never crashes the window loop.
    pub(super) fn rebuild_scene_render_state(&mut self) {
        // Every caller swapped the scene. Retail's field entry
        // (`FUN_80025C24`) rewrites the camera globals and kills the mover, so
        // no glide pose survives the door - the next scene's first scripted
        // shot snaps in, as on the browser page (`CutsceneGlide::reset`).
        self.cutscene_glide.reset();
        match build_window_scene_resources(&self.session) {
            Ok(res) => {
                // Spawn-slot drain state is per-scene (the new scene's field
                // VM re-issues its own actor spawns; `upload_assets` re-seats
                // the player's slot).
                self.drained_spawn_slots.clear();
                self.scene_res = Some(res);
                self.upload_assets();
            }
            Err(e) => log::warn!("play-window: scene-resource rebuild failed: {e:#}"),
        }
    }
}

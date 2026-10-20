//! Field placement draw resolution: placed-object and posed-prop draws, the
//! static environment list with its coplanar offsets, the scene AABB and the
//! camera-occlusion occluder set.

use super::*;

/// [`PlayWindowApp::static_env_layers`]: terrain draws, placed-object
/// draws, the scene's object binds.
type StaticEnvLayers = (
    Vec<legaia_engine_scene::field_env::EnvDraw>,
    Vec<legaia_engine_scene::field_env::EnvDraw>,
    Option<std::collections::HashMap<(u8, u8), legaia_engine_scene::field_env::ObjectBind>>,
);

impl PlayWindowApp {
    /// Resolve the field static-geometry placement draws for the current
    /// scene: each placed environment object's scene-pack mesh paired with a
    /// world model matrix. Built from the field map's object table
    /// (`Scene::field_object_placements`) and the scene_asset_table TMD pack;
    /// the per-object pack index resolves via `legaia_scene_formats::field_objects`.
    ///
    /// `tmd_src_index[j]` is the `res.tmds` index of uploaded mesh `j` (meshes
    /// skip empty-prim TMDs, so this bridges back). Returns empty for scenes
    /// with no field map / no bundle (e.g. battle or world-map blocks).
    ///
    /// World Y is left at the ground plane for now; the per-tile floor-height
    /// LUT (MAN header) is a separate refinement.
    pub(in crate::window) fn resolve_field_placement_draws(
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
            None,
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
    pub(in crate::window) fn resolve_posed_props(
        &self,
        res: &SceneResources,
        posed: &PosedPlacementMeshes,
        bundle: Option<&legaia_actor_formats::player_anm::PlayerAnmBundle>,
    ) -> Vec<PosedPropDraw> {
        use legaia_engine_scene::field_env;
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
        self.follow_floor_placed_draws(&mut draws, &binds);

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
                legaia_render_kernels::battle_intro::placement_rotation(d.rot_x, d.rot_y, d.rot_z)
                    * Mat4::from_scale(Vec3::splat(scale));
            props.push(PosedPropDraw {
                anchor: d.anchor,
                anim_id: d.anim_id,
                model: t * rot,
                baked,
                cull_radius: d.cull_radius,
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
    /// combined terrain + placed [`legaia_engine_scene::field_env::EnvDraw`]
    /// lists (field scenes) or the walk landmark + decoration lists (world
    /// map), run through `legaia_engine_scene::coplanar_draws`. Overlapping
    /// same-plane draws otherwise z-fight - retail painter-orders them
    /// through the ordering table, the port's depth buffer ties per pixel.
    ///
    /// Resolves the layers with the same inputs the draw resolvers use so
    /// the map's `EnvDraw` keys match theirs by identity.
    /// Resolve the scene's complete STATIC env draw list (terrain tiles +
    /// placed objects; world-map scenes use the walk layers) - the shared
    /// input of the coplanar-lift pass and the occlusion-fade gate's
    /// occluder set.
    /// Seat the placed draws whose bind record follows the floor on the
    /// floor sample under them (`field_env::follow_floor_placed_draws` over
    /// `World::object_floor_follow_y`) - the same pass the browser play page
    /// runs in `build_field_render`.
    pub(super) fn follow_floor_placed_draws(
        &self,
        draws: &mut [legaia_engine_scene::field_env::EnvDraw],
        binds: &std::collections::HashMap<(u8, u8), legaia_engine_scene::field_env::ObjectBind>,
    ) {
        let world = &self.session.host.world;
        let follow = world.object_floor_follow_records();
        if follow.is_empty() {
            return;
        }
        legaia_engine_scene::field_env::follow_floor_placed_draws(draws, binds, |r, x, z| {
            world.object_floor_follow_y(&follow, r, x, z)
        });
    }

    pub(in crate::window) fn static_env_draws(
        &self,
        res: &SceneResources,
    ) -> Vec<legaia_engine_scene::field_env::EnvDraw> {
        let (mut draws, placements, _) = self.static_env_layers(res);
        draws.extend(placements);
        draws
    }

    /// [`Self::static_env_draws`] as its two layers - terrain (or the
    /// overworld's landmarks and decorations) and the field's placed
    /// objects - with the scene's object binds.
    fn static_env_layers(&self, res: &SceneResources) -> StaticEnvLayers {
        use legaia_engine_scene::field_env;
        let Some(scene) = self.session.host.scene.as_ref() else {
            return Default::default();
        };
        let index = &self.session.host.index;
        let env_tmds = field_env::env_pack_tmd_indices(scene, res);
        if env_tmds.is_empty() {
            return Default::default();
        }
        let floor_lut = scene.field_floor_height_lut(index).ok().flatten();
        let mut draws: Vec<field_env::EnvDraw> = Vec::new();
        let mut placed: Vec<field_env::EnvDraw> = Vec::new();
        let mut scene_binds = None;
        if legaia_engine_scene::scene::is_world_map_scene(&scene.name) {
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
                    .filter(|p| p.flags & legaia_scene_formats::field_objects::FLAG_PLACED == 0)
                    .collect();
                let (d, _) = field_env::resolve_env_draws(&env_tmds, &tiles, floor_lut);
                draws.extend(d);
            }
            if let Ok(Some(placements)) = scene.field_object_placements(index) {
                let binds = scene.field_object_binds(index).ok().flatten();
                let (mut d, _) = field_env::resolve_placed_env_draws(
                    &env_tmds,
                    &placements,
                    floor_lut,
                    binds.as_ref(),
                );
                if let Some(binds) = binds.as_ref() {
                    self.follow_floor_placed_draws(&mut d, binds);
                }
                placed.extend(d);
                scene_binds = binds;
            }
        }
        (draws, placed, scene_binds)
    }

    pub(in crate::window) fn compute_coplanar_env_offsets(
        &self,
        res: &SceneResources,
    ) -> std::collections::HashMap<legaia_engine_scene::field_env::EnvDraw, [f32; 3]> {
        use legaia_engine_scene::coplanar_draws;
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
    /// `engine_scene::field_env::env_draws_world_aabb` the browser play page
    /// also calls. It used to be the union of the uploaded meshes' *local*
    /// vertex extents: every env-pack mesh is authored about its own origin,
    /// so that box sat near `(0,0,0)` while the geometry it was meant to
    /// frame sits at placement coordinates, and the top view framed the origin
    /// corner with the map off to one side.
    ///
    /// `None` leaves the previous box in place - a scene with no static env
    /// geometry has nothing to frame.
    pub(in crate::window) fn scene_world_aabb(
        &self,
        res: &SceneResources,
    ) -> Option<([f32; 3], [f32; 3])> {
        let draws = self.static_env_draws(res);
        let ground = self
            .session
            .host
            .scene
            .as_ref()
            .and_then(|sc| sc.walk_heightfield(&self.session.host.index).ok().flatten());
        legaia_engine_scene::field_env::env_draws_world_aabb(&[&draws], res, ground.as_ref())
    }

    /// Build the occlusion-fade visibility gate's world-space occluder set
    /// from the same static draw list the render layers use (see
    /// `legaia_engine_scene::field_occlusion`). Rebuilt per scene load.
    pub(in crate::window) fn build_field_occluders(
        &self,
        res: &SceneResources,
    ) -> legaia_engine_scene::field_occlusion::FieldOccluders {
        let draws = self.static_env_draws(res);
        let occ = legaia_engine_scene::field_occlusion::FieldOccluders::build(&[&draws], res);
        if !occ.is_empty() {
            log::info!(
                "play-window: occlusion-fade gate armed with {} static triangles",
                occ.triangle_count()
            );
        }
        occ
    }

    /// Build the volumetric fog's ground coverage field from the same static
    /// draw list and hand it to the world's bank
    /// (`legaia_engine_scene::fog_ground`) - the browser play page makes the
    /// same call at its scene load. Rebuilt per scene load.
    pub(in crate::window) fn install_fog_ground(&mut self, res: &SceneResources) {
        use legaia_engine_scene::field_env;
        let started = std::time::Instant::now();
        // The bank lies on what is drawn: the story-hidden placements gone
        // (the gate `resolve_placement_draws` runs), each placement at its
        // render scale - the same layers the browser play page hands in.
        let (terrain, mut placements, binds) = self.static_env_layers(res);
        let world = &self.session.host.world;
        if let Some(binds) = binds.as_ref() {
            field_env::retain_visible_placed_draws(
                &mut placements,
                binds,
                &world.hidden_object_records(),
            );
        }
        let scales = field_env::placed_render_scales(
            &placements,
            binds.as_ref(),
            &world.object_render_scales(),
        );
        let host = &mut self.session.host;
        let field = host.scene.as_ref().and_then(|scene| {
            legaia_engine_scene::fog_ground::build_for_scene(
                &terrain,
                &placements,
                &scales,
                res,
                scene,
                &host.index,
            )
        });
        let label = host
            .scene
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_default();
        if let Some(f) = &field {
            log::info!(
                "play-window: fog ground field {}x{} cells, {} on the floor ({:.1} ms)",
                f.w,
                f.h,
                f.covered_count(),
                started.elapsed().as_secs_f64() * 1000.0
            );
        }
        host.world.fog_volume.set_ground_field(&label, field);
    }
}

impl PlayWindowApp {
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
    ///
    /// `record_binds` names each draw's bind record (the list's last member)
    /// without applying any of the `binds` gates - the overworld landmarks,
    /// which draw unposed and unscaled but whose actors a script can tint.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::window) fn resolve_placement_draws(
        &self,
        res: &SceneResources,
        tmd_src_index: &[usize],
        placements: &[legaia_scene_formats::field_objects::Placement],
        flip_y: bool,
        binds: Option<
            &std::collections::HashMap<(u8, u8), legaia_engine_scene::field_env::ObjectBind>,
        >,
        posed: Option<(&PosedPlacementMeshes, bool)>,
        record_binds: Option<
            &std::collections::HashMap<(u8, u8), legaia_engine_scene::field_env::ObjectBind>,
        >,
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
        // resolution live in the shared kernel `engine_scene::field_env`, so
        // the web viewer's assembled scene view resolves the exact same
        // draws; this method only adds the uploaded-mesh bridge and the
        // render-frame model matrix.
        let env_tmds = legaia_engine_scene::field_env::env_pack_tmd_indices(scene, res);
        if env_tmds.is_empty() {
            return Default::default();
        }
        let (mut env_draws, dropped) = legaia_engine_scene::field_env::resolve_placed_env_draws(
            &env_tmds, placements, floor_lut, binds,
        );
        // Story-hidden placed objects: a bind record's spawn prologue that
        // parked its actor at the hide box (flag-gated scenery like town01's
        // gate rocks) draws nothing, exactly as retail draws the actor rather
        // than the raw `.MAP` table.
        if let Some(binds) = binds {
            legaia_engine_scene::field_env::retain_visible_placed_draws(
                &mut env_draws,
                binds,
                &self.session.host.world.hidden_object_records(),
            );
            self.follow_floor_placed_draws(&mut env_draws, binds);
        }
        // Render scale: a bind record's prologue can leave the actor's
        // `+0x72` at a non-unit value (town01's horizon backdrop draws at a
        // quarter), and retail's case-5 draw folds it into the model matrix.
        let scales = legaia_engine_scene::field_env::placed_render_scales(
            &env_draws,
            binds,
            &self.session.host.world.object_render_scales(),
        );
        let diag = std::env::var_os("LEGAIA_DIAG_PLACE").is_some();
        if diag {
            for d in &dropped {
                match d {
                    legaia_engine_scene::field_env::EnvDrawDrop::NoPackIndex {
                        world_x,
                        world_z,
                    } => {
                        log::info!("DIAG place drop: no pack_index at ({world_x}, {world_z})");
                    }
                    legaia_engine_scene::field_env::EnvDrawDrop::SlotOutOfRange {
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
                    legaia_engine_scene::field_env::EnvDrawDrop::Unbound {
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
        // world's windowed static-object list by `legaia_engine_world_views::scene_staging::placed_draw_live`).
        let mut window_keys = Vec::new();
        // Parallel to `draws`: the grid cell + cull radius the visible-tile
        // crop (`field_view_window::terrain_draw_visible`) tests per frame.
        let mut cell_keys = Vec::new();
        // Parallel to `draws`: the bind record whose actor a script can move
        // (`World::object_draw_displacements`, folded in per frame).
        let mut records = Vec::new();
        // Parallel to `draws`: a camera-facing cell's flags and rotation
        // (`EnvDraw::view_skip`), re-placed against the camera every frame.
        let mut facing = Vec::new();
        let bind_records =
            legaia_engine_scene::field_env::placed_bind_records(&env_draws, binds.or(record_binds));
        for ((d, &scale), &record) in env_draws.iter().zip(&scales).zip(&bind_records) {
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
                legaia_render_kernels::battle_intro::placement_rotation(d.rot_x, d.rot_y, d.rot_z)
                    * Mat4::from_scale(Vec3::splat(scale));
            let model = if flip_y {
                t * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0)) * rot
            } else {
                t * rot
            };
            if diag {
                log::info!(
                    "DIAG place keep: pack {} (res {} -> mesh {}) at ({}, {}, {}) rot {} bind {:?}",
                    d.env_slot,
                    d.res_tmd,
                    mesh_idx,
                    d.world_x,
                    d.world_y,
                    d.world_z,
                    d.rot_y & 0x0FFF,
                    record
                );
            }
            draws.push((mesh_idx, model));
            floors.push(d.floor);
            window_keys.push(legaia_engine_scene::field_env::placed_window_key(d, binds));
            cell_keys.push(legaia_engine_scene::field_view_window::CellKey::of_draw(d));
            records.push(record);
            facing.push((d.view_skip != 0 && !flip_y).then_some((d.view_skip, rot)));
        }
        log::info!(
            "play-window: {} field placement draws ({} placements, {} env meshes)",
            draws.len(),
            placements.len(),
            env_tmds.len(),
        );
        (draws, floors, window_keys, cell_keys, records, facing)
    }
}

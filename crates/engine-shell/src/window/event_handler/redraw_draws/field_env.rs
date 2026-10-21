//! The field environment half of the scene draw lists: the static
//! environment, the terrain tiles and the placed objects.

use super::*;

impl<'m> DrawCtx<'_, 'm> {
    /// The field's environment layers (ground, terrain tiles, placements,
    /// posed props, their colour halves), the player's colour half, the
    /// placed NPCs and the tile-board actors.
    pub(super) fn push_field_env(&self, out: &mut DrawLists<'m>, cues: &FrameCues) {
        let r = self.r;
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
        self.push_field_terrain(out, &layer_on);
        self.push_field_placements(out, cues, &layer_on);
        // Occlusion-fade draw watermark: every draw pushed so
        // far is scene ENVIRONMENT (terrain, placements, posed
        // props, their colour halves) - fadeable. Everything
        // after this point is an ACTOR (the player's two
        // halves, NPCs, tile actors, spawned meshes), which the
        // fade must never dissolve - the depth margin only has
        // to guard geometry AT the focus depth now, so
        // occluders hugging the character still open up.
        r.set_occlusion_env_draws(out.draws.len(), out.color_draws.len());
        self.push_field_npcs(out, cues, &layer_on);
    }

    /// The field ground (or its visible-tile crop) and the terrain tiles,
    /// textured and untextured, with camera-facing decoration cells.
    pub(super) fn push_field_terrain(
        &self,
        out: &mut DrawLists<'m>,
        layer_on: &dyn Fn(&str) -> bool,
    ) {
        let app = self.app;
        let store = &self.store;
        let cam = self.cam;
        let cutscene_cam = self.cutscene_cam;
        let view_cells = self.view_cells;
        let in_world_map = self.in_world_map;
        let DrawLists {
            draws, color_draws, ..
        } = out;
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
        // Under the visible-tile crop the cropped copy draws
        // instead (`sync_ground_crop`).
        let ground = match (view_cells, store.ground_crop) {
            (Some(_), Some((_, m))) => m.as_ref(),
            _ => store.ground_heightfield.as_ref(),
        };
        if layer_on("hf")
            && let Some(hf_mesh) = ground
        {
            draws.push(SceneDraw {
                mesh: hf_mesh,
                mvp: cam,
                cue: None,
            });
        }
        // A camera-facing terrain / decoration cell (record
        // flags `+0x12 & 0x380` - `rugi`'s candle glows, the
        // `vell` forest trees) is rebuilt against this frame's
        // camera: retail's decoration pass `FUN_801F7088` drops
        // the flagged axes from the camera rotation before the
        // cell's own. Shared kernel
        // `gte::decoration_cell_basis`; the browser play page
        // takes the same basis through `field_terrain_facing`.
        let facing_cam = app.part_camera_pose(in_world_map, cutscene_cam);
        let facing_model =
            |model: &Mat4, f: Option<&Option<super::super::field_render::CameraFacing>>| -> Mat4 {
                let Some(Some((skip, rot))) = f else {
                    return *model;
                };
                let Some(k) =
                    legaia_render_kernels::gte::decoration_cell_basis(*skip, facing_cam.as_ref())
                else {
                    return *model;
                };
                let basis = Mat4::from_mat3(glam::Mat3::from_cols_array_2d(&k).transpose());
                Mat4::from_translation(model.w_axis.truncate()) * basis * *rot
            };
        // Then the terrain / decor tile layer (drawn under
        // the buildings): the `CELL_VISIBLE` field-map tiles
        // (stone plaza, paths, riverbank).
        if layer_on("tiles") {
            for (di, (mesh_idx, model)) in app.field_terrain_draws.iter().enumerate() {
                if !legaia_engine_scene::field_view_window::terrain_draw_visible(
                    view_cells.as_ref(),
                    app.field_terrain_cell_keys
                        .get(di)
                        .copied()
                        .unwrap_or_default(),
                ) {
                    continue;
                }
                // A lit mesh draws its copy shaded at this
                // draw's rotation (`field_lit_mesh`).
                let mesh = store.field_morph_live.get(mesh_idx).or_else(|| {
                    store
                        .field_lit
                        .terrain
                        .get(di)
                        .copied()
                        .flatten()
                        .and_then(|v| store.field_lit.meshes.get(v))
                        .or_else(|| store.meshes.get(*mesh_idx))
                });
                if let Some(mesh) = mesh {
                    draws.push(SceneDraw {
                        mesh,
                        mvp: cam * facing_model(model, app.field_terrain_facing.get(di)),
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
            for (di, (mesh_idx, model)) in app.field_terrain_color_draws.iter().enumerate() {
                if !legaia_engine_scene::field_view_window::terrain_draw_visible(
                    view_cells.as_ref(),
                    app.field_terrain_color_cell_keys
                        .get(di)
                        .copied()
                        .unwrap_or_default(),
                ) {
                    continue;
                }
                if let Some(mesh) = store.color_meshes.get(*mesh_idx) {
                    color_draws.push(ColorSceneDraw {
                        mesh,
                        mvp: cam * facing_model(model, app.field_terrain_color_facing.get(di)),
                        cue: None,
                    });
                }
            }
        }
    }

    /// The placed static objects and posed props, textured and untextured,
    /// at their live transforms, parked / culled / tinted / clipped.
    pub(super) fn push_field_placements(
        &self,
        out: &mut DrawLists<'m>,
        cues: &FrameCues,
        layer_on: &dyn Fn(&str) -> bool,
    ) {
        let app = self.app;
        let store = &self.store;
        let cam = self.cam;
        let view_cells = self.view_cells;
        let effect_clip = |key: legaia_engine_field::object_tint::ActorTintKey, model: &Mat4| {
            cues.effect_clip(key, model)
        };
        let posed_prop_baked_v = &store.posed.posed_prop_baked_v;
        let posed_prop_baked_c = &store.posed.posed_prop_baked_c;
        let posed_prop_live_v = &store.posed.posed_prop_live_v;
        let posed_prop_live_c = &store.posed.posed_prop_live_c;
        let DrawLists {
            draws,
            color_draws,
            clip_marks,
            color_clip_marks,
            ..
        } = out;
        // Static environment geometry: draw each placed
        // building / terrain mesh at its world transform
        // (resolved at scene load in
        // `resolve_field_placement_draws`).
        // Retail's placed-object near reject
        // (`field_env::placed_origin_near_culled`): an object
        // whose origin sits within 160 units of the eye, or
        // behind it, is not drawn. Judged from retail's vantage
        // only (`field_view_window::retail_vantage`): under a
        // swung / tilted / zoomed follow camera, a wider distance
        // preset or the `F3` debug orbit, a ground slab's origin
        // can sit behind the lens while the slab is in shot.
        // A placed object a script has moved (`A3` seat, `4C 42`
        // lift under the actor's `0x20000000` height law) draws at
        // its actor's live position: retail's case-5 draw reads
        // the actor, not the `.MAP` record. The shared kernel is
        // `World::object_draw_displacements`; the browser play
        // page takes the composed model (`field_placement_live_models`).
        let object_moves = app.session.host.world.object_draw_displacements();
        // ... and at its actor's live angles: a script that turns
        // the object (op `0x38`, `4C 48`) turns the drawn mesh
        // about its origin (`World::object_draw_turn_matrices`,
        // folded into the same `field_placement_live_models` on the page).
        let object_turns = app.session.host.world.object_draw_turn_matrices();
        // A placed object a script parks at the hide box after
        // the scene built its draw lists (`rugi`'s entry script
        // runs `A3 06 7F 7F` on the stone block the opened wall
        // leaves behind) stops drawing that frame: retail's
        // case-5 draw reads the actor, which now stands off the
        // map. The build-time pass drops the records parked by
        // then; this is the same set, read per frame. Shared
        // kernel `World::hidden_object_records`; the browser play
        // page reads it through `field_placement_parked`.
        let object_parked = app.session.host.world.hidden_object_records();
        let parked = |record: Option<usize>| record.is_some_and(|r| object_parked.contains(&r));
        // Turn, then move: the shared `field_env::live_placed_model` the
        // overworld's landmarks go through too.
        let object_moved = |model: &Mat4, record: Option<usize>| -> Mat4 {
            Mat4::from_cols_array(&legaia_engine_scene::field_env::live_placed_model(
                &model.to_cols_array(),
                record,
                &object_turns,
                &object_moves,
            ))
        };
        // A placed object whose actor carries a draw tint
        // (op `4C 81`: `+0x74` colour, `+0x78` blend - chitei2's
        // hologram panels go black once the generator is down)
        // draws with that pair as a constant per-draw cue, the
        // far colour / `IR0` retail's case-5 draw stages. Shared
        // table `World::object_draw_tints`; the browser play page
        // reads the same one (`field_placement_tints`).
        let object_tints = app.session.host.world.object_draw_tints();
        // A motion stream's model swap (op `0x0E`), per placed draw: the
        // shared first-placement-of-the-record rule (`placed_model_swaps`).
        let object_swaps = legaia_engine_scene::field_env::placed_model_swaps(
            &app.field_placement_records,
            app.session.host.world.object_live_models(),
        );
        let object_color_swaps = legaia_engine_scene::field_env::placed_model_swaps(
            &app.field_placement_color_records,
            app.session.host.world.object_live_models(),
        );
        let object_cue = |record: Option<usize>| {
            let &(colour, blend) = object_tints.get(&record?)?;
            tint_draw_cue((colour, blend))
        };
        let retail_vantage = app.retail_vantage();
        let place_near_culled = |mvp: &Mat4| {
            retail_vantage
                && legaia_engine_scene::field_env::placed_origin_near_culled(mvp.w_axis.w)
        };
        if layer_on("place") {
            // Diag bisect: `LEGAIA_DIAG_PLACE_RANGE=a..b` draws only
            // placement-draw slots [a, b).
            let place_range = std::env::var("LEGAIA_DIAG_PLACE_RANGE").ok().and_then(|s| {
                let (a, b) = s.split_once("..")?;
                Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?))
            });
            // The sub-area window sweep's placements are gated
            // on the world's windowed static-object list (a
            // no-op unless retail windowing is on).
            let static_window = &app.session.host.world.terrain.static_window;
            for (di, (mesh_idx, model)) in app.field_placement_draws.iter().enumerate() {
                let record = app.field_placement_records.get(di).copied().flatten();
                if parked(record) {
                    continue;
                }
                let model = &object_moved(model, record);
                if let Some((a, b)) = place_range
                    && !(a..b).contains(&di)
                {
                    continue;
                }
                if !legaia_engine_world_views::scene_staging::placed_draw_live(
                    app.field_placement_window_keys
                        .get(di)
                        .and_then(Option::as_ref),
                    static_window,
                ) || !legaia_engine_world_views::field_view_window::placed_actor_visible(
                    &app.session.host.world,
                    view_cells.as_ref(),
                    model.w_axis.x as i32,
                    model.w_axis.z as i32,
                    app.field_placement_cell_keys
                        .get(di)
                        .map_or(0, |k| k.cull_radius),
                ) {
                    continue;
                }
                // A motion stream's model swap (op `0x0E`) draws
                // the record's object with the swapped-in mesh.
                let swapped = object_swaps
                    .get(di)
                    .copied()
                    .flatten()
                    .and_then(|id| app.field_pack_meshes.get(id).copied().flatten())
                    .and_then(|m| store.meshes.get(m));
                let mesh = swapped
                    .or_else(|| store.field_morph_live.get(mesh_idx))
                    .or_else(|| {
                        store
                            .field_lit
                            .placement
                            .get(di)
                            .copied()
                            .flatten()
                            .and_then(|v| store.field_lit.meshes.get(v))
                            .or_else(|| store.meshes.get(*mesh_idx))
                    });
                let mvp = cam * *model;
                if place_near_culled(&mvp) {
                    continue;
                }
                if let Some(mesh) = mesh {
                    if let Some(c) = object_key(record).and_then(|k| effect_clip(k, model)) {
                        clip_marks.push((draws.len(), c));
                    }
                    draws.push(SceneDraw {
                        mesh,
                        mvp,
                        cue: object_cue(record),
                    });
                }
            }
            // Posed props (house doors, cupboards, the windmill):
            // the ones resting on frame 0 replay their baked rest
            // mesh; the ones whose clip is running were re-posed
            // above, so the door draws mid-swing.
            for (mesh_idx, model, record) in posed_prop_baked_v {
                if parked(*record) {
                    continue;
                }
                let mvp = cam * *model;
                if place_near_culled(&mvp) {
                    continue;
                }
                // A lit prop's shaded copy (`LIT_VARIANT_TAG`).
                // Field-level borrows, not a `&app` helper.
                let tag = super::super::field_render::LIT_VARIANT_TAG;
                let baked = if *mesh_idx & tag != 0 {
                    store.field_lit.meshes.get(*mesh_idx & !tag)
                } else {
                    store.meshes.get(*mesh_idx)
                };
                if let Some(mesh) = baked {
                    if let Some(c) = object_key(*record).and_then(|k| effect_clip(k, model)) {
                        clip_marks.push((draws.len(), c));
                    }
                    draws.push(SceneDraw {
                        mesh,
                        mvp,
                        cue: object_cue(*record),
                    });
                }
            }
            for (mesh, model, record) in posed_prop_live_v {
                if parked(*record) {
                    continue;
                }
                let mvp = cam * *model;
                if place_near_culled(&mvp) {
                    continue;
                }
                if let Some(c) = object_key(*record).and_then(|k| effect_clip(k, model)) {
                    clip_marks.push((draws.len(), c));
                }
                draws.push(SceneDraw {
                    mesh,
                    mvp,
                    cue: object_cue(*record),
                });
            }
        }
        // Untextured props (the F*/G* meshes the VRAM path
        // drops) on the colour pipeline, same transforms.
        if layer_on("cplace") {
            let static_window = &app.session.host.world.terrain.static_window;
            for (di, (mesh_idx, model)) in app.field_placement_color_draws.iter().enumerate() {
                let record = app.field_placement_color_records.get(di).copied().flatten();
                if parked(record) {
                    continue;
                }
                let model = &object_moved(model, record);
                if !legaia_engine_world_views::scene_staging::placed_draw_live(
                    app.field_placement_color_window_keys
                        .get(di)
                        .and_then(Option::as_ref),
                    static_window,
                ) || !legaia_engine_world_views::field_view_window::placed_actor_visible(
                    &app.session.host.world,
                    view_cells.as_ref(),
                    model.w_axis.x as i32,
                    model.w_axis.z as i32,
                    app.field_placement_color_cell_keys
                        .get(di)
                        .map_or(0, |k| k.cull_radius),
                ) {
                    continue;
                }
                let mvp = cam * *model;
                if place_near_culled(&mvp) {
                    continue;
                }
                let color_idx = object_color_swaps
                    .get(di)
                    .copied()
                    .flatten()
                    .and_then(|id| app.field_pack_color_meshes.get(id).copied().flatten())
                    .unwrap_or(*mesh_idx);
                if let Some(mesh) = store.color_meshes.get(color_idx) {
                    if let Some(c) = object_key(record).and_then(|k| effect_clip(k, model)) {
                        color_clip_marks.push((color_draws.len(), c));
                    }
                    color_draws.push(ColorSceneDraw {
                        mesh,
                        mvp,
                        cue: object_cue(record),
                    });
                }
            }
            for (mesh_idx, model, record) in posed_prop_baked_c {
                if parked(*record) {
                    continue;
                }
                let mvp = cam * *model;
                if place_near_culled(&mvp) {
                    continue;
                }
                if let Some(mesh) = store.color_meshes.get(*mesh_idx) {
                    if let Some(c) = object_key(*record).and_then(|k| effect_clip(k, model)) {
                        color_clip_marks.push((color_draws.len(), c));
                    }
                    color_draws.push(ColorSceneDraw {
                        mesh,
                        mvp,
                        cue: object_cue(*record),
                    });
                }
            }
            for (mesh, model, record) in posed_prop_live_c {
                if parked(*record) {
                    continue;
                }
                let mvp = cam * *model;
                if place_near_culled(&mvp) {
                    continue;
                }
                if let Some(c) = object_key(*record).and_then(|k| effect_clip(k, model)) {
                    color_clip_marks.push((color_draws.len(), c));
                }
                color_draws.push(ColorSceneDraw {
                    mesh,
                    mvp,
                    cue: object_cue(*record),
                });
            }
        }
    }
}

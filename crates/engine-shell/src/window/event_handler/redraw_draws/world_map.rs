//! The world-map half of the scene draw lists: the overworld terrain, ocean,
//! entrances and the map-mode actors.

use super::*;

impl<'m> DrawCtx<'_, 'm> {
    /// The world map: the ground heightfield, the placed landmarks (live
    /// transforms, model swaps, tints, decoration cues) and the party leader.
    pub(super) fn push_world_map(&self, out: &mut DrawLists<'m>, cues: &FrameCues) {
        let app = self.app;
        let store = &self.store;
        let cam = self.cam;
        let cutscene_cam = self.cutscene_cam;
        let player_tint_cue = cues.player_tint_cue;
        let effect_clip = |key: legaia_engine_field::object_tint::ActorTintKey, model: &Mat4| {
            cues.effect_clip(key, model)
        };
        let posed_overrides = &store.posed.posed_overrides;
        let player_color_posed = &store.posed.player_color_posed;
        let DrawLists {
            draws,
            color_draws,
            clip_marks,
            color_clip_marks,
            ..
        } = out;
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
        if let Some(hf_mesh) = store.ground_heightfield.as_ref() {
            draws.push(SceneDraw {
                mesh: hf_mesh,
                mvp: cam,
                cue: None,
            });
        }
        // Retail's decoration sweep (`FUN_801F69D8`) hazes each
        // decoration toward `0xD0` by one `IR0` taken from its
        // origin's camera depth; the landmarks ahead of
        // `world_map_deco_start` carry no such cue.
        // `LEGAIA_DIAG_NO_DECO_CUE` drops it, for before/after frames.
        let deco_curve = if std::env::var_os("LEGAIA_DIAG_NO_DECO_CUE").is_some() {
            0.0
        } else {
            app.overworld_curve_scale(cutscene_cam)
        };
        let deco_cue = |mvp: Mat4| {
            legaia_engine_field::overworld_ground_cue::decoration_draw_cue(mvp.w_axis.w, deco_curve)
                .map(|c| legaia_engine_render::DrawCue {
                    far: c.far,
                    near_z: -1.0,
                    far_z: 0.0,
                    max_ir0: c.ir0,
                })
        };
        let (deco_start, color_deco_start) = app.world_map_deco_start;
        // A landmark whose actor carries an op-`4C 81` draw tint
        // (map02's Jeremi walls, map03's bridge spans) draws with it,
        // as the field's placed objects do; the browser page reads the
        // same `World::object_draw_tints` (`field_placement_tints`).
        let object_tints = app.session.host.world.object_draw_tints();
        let landmark_cue = |record: Option<&Option<usize>>| {
            let &(colour, blend) = object_tints.get(&(*record?)?)?;
            tint_draw_cue((colour, blend))
        };
        // A landmark is an actor: retail's case-5 draw reads its live
        // position and mesh, so a script's `A3 <id>` seat and
        // `CC <id> 50` model swap move and re-skin it (the credits
        // walk re-skins Rim Elm, `map01` record 8). The kernels are the
        // field branch's - `World::object_draw_displacements`,
        // `World::object_live_models` on a record's first draw - and
        // the browser page takes the same two through its
        // `field_placement_live_models` / `field_placement_models`
        // exports.
        let wm_moves = app.session.host.world.object_draw_displacements();
        let wm_turns = app.session.host.world.object_draw_turn_matrices();
        let wm_models = app.session.host.world.object_live_models();
        let wm_swaps = legaia_engine_scene::field_env::placed_model_swaps(
            &app.world_map_terrain_records,
            wm_models,
        );
        let wm_color_swaps = legaia_engine_scene::field_env::placed_model_swaps(
            &app.world_map_terrain_color_records,
            wm_models,
        );
        let wm_live =
            |records: &[Option<usize>], swaps: &[Option<usize>], i: usize, model: &Mat4| {
                let record = records.get(i).copied().flatten();
                let moved =
                    Mat4::from_cols_array(&legaia_engine_scene::field_env::live_placed_model(
                        &model.to_cols_array(),
                        record,
                        &wm_turns,
                        &wm_moves,
                    ));
                (moved, swaps.get(i).copied().flatten())
            };
        for (i, (mesh_idx, model)) in app.world_map_terrain_draws.iter().enumerate() {
            let (model, swap) = if i < deco_start {
                wm_live(&app.world_map_terrain_records, &wm_swaps, i, model)
            } else {
                (*model, None)
            };
            let mesh_idx = match swap {
                Some(id) => match app.field_pack_meshes.get(id).copied().flatten() {
                    Some(m) => m,
                    None => continue,
                },
                None => *mesh_idx,
            };
            if let Some(mesh) = store.meshes.get(mesh_idx) {
                let mvp = cam * model;
                draws.push(SceneDraw {
                    mesh,
                    mvp,
                    cue: if i >= deco_start {
                        deco_cue(mvp)
                    } else {
                        landmark_cue(app.world_map_terrain_records.get(i))
                    },
                });
            }
        }
        // The untextured half of the same stamps (hut roofs,
        // colour-only landmarks) on the colour pipeline - the
        // field branch's pairing, which this branch lacked.
        for (i, (mesh_idx, model)) in app.world_map_terrain_color_draws.iter().enumerate() {
            let (model, swap) = if i < color_deco_start {
                wm_live(
                    &app.world_map_terrain_color_records,
                    &wm_color_swaps,
                    i,
                    model,
                )
            } else {
                (*model, None)
            };
            let mesh_idx = match swap {
                Some(id) => match app.field_pack_color_meshes.get(id).copied().flatten() {
                    Some(m) => m,
                    None => continue,
                },
                None => *mesh_idx,
            };
            if let Some(mesh) = store.color_meshes.get(mesh_idx) {
                let mvp = cam * model;
                color_draws.push(ColorSceneDraw {
                    mesh,
                    mvp,
                    cue: if i >= color_deco_start {
                        deco_cue(mvp)
                    } else {
                        landmark_cue(app.world_map_terrain_color_records.get(i))
                    },
                });
            }
        }
        // Last-resort fallback: nothing resolved at all -> draw
        // the whole pack at pack-local coords so the map isn't
        // blank.
        if store.ground_heightfield.is_none() && app.world_map_terrain_draws.is_empty() {
            for mesh in store.meshes {
                draws.push(SceneDraw {
                    mesh,
                    mvp: cam,
                    cue: None,
                });
            }
        }
        // The party leader's field figure at the player's live
        // transform - retail draws the overworld walker with the
        // same PROT 0874 mesh as the field. Both world-map cameras
        // compose FIELD_WORLD_FLIP, so the un-flipped `actor_model`
        // (translation * heading yaw) is the correct frame, same as
        // the field branch. Hidden while a cutscene timeline owns
        // the map (the opening fly-in shows the bare continent) -
        // the same gate the marker overlay uses.
        if !app.session.host.world.cutscene_timeline_active() {
            let w = &app.session.host.world;
            let player = w.player_actor_slot.and_then(|pslot| {
                // Only a successfully uploaded player mesh draws: the
                // naive pre-bind (actor K -> scene TMD K) would show
                // an unrelated scene mesh as "the player".
                if !app.drained_spawn_slots.contains(&pslot) {
                    return None;
                }
                let slot = pslot as usize;
                let tmd_idx = w.actors.get(slot)?.tmd_binding?;
                Some((slot, tmd_idx))
            });
            if let Some((slot, tmd_idx)) = player {
                let mesh = posed_overrides
                    .get(tmd_idx)
                    .and_then(|o| o.as_ref())
                    .or_else(|| store.meshes.get(tmd_idx));
                if let Some(mesh) = mesh {
                    if let Some(c) = effect_clip(
                        legaia_engine_field::object_tint::ActorTintKey::Player,
                        &app.actor_model(slot),
                    ) {
                        clip_marks.push((draws.len(), c));
                    }
                    draws.push(SceneDraw {
                        mesh,
                        mvp: cam * app.actor_model(slot),
                        cue: player_tint_cue,
                    });
                }
                // The untextured colour half (pants / sleeves), same
                // pairing as the field branch.
                if let Some((cidx, cslot)) = app.player_color_draw
                    && let Some(cmesh) = player_color_posed
                        .as_ref()
                        .or_else(|| store.color_meshes.get(cidx))
                {
                    if let Some(c) = effect_clip(
                        legaia_engine_field::object_tint::ActorTintKey::Player,
                        &app.actor_model(cslot),
                    ) {
                        color_clip_marks.push((color_draws.len(), c));
                    }
                    color_draws.push(ColorSceneDraw {
                        mesh: cmesh,
                        mvp: cam * app.actor_model(cslot),
                        cue: player_tint_cue,
                    });
                }
            }
        }
    }
}

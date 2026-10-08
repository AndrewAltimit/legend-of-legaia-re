//! The redraw's scene draw lists: every textured and untextured 3D draw
//! the frame submits, by mode (minigame surfaces, the dance venue, the world
//! map, the battle stage, the field environment, the actors, the arts
//! ghosts) - split out of `handle_redraw`.
//!
//! The lists borrow only the uploaded-mesh fields named in [`MeshStore`], so
//! the caller can keep mutating the rest of the window (the battle-intro
//! VRAM capture, the framebuffer grab, the screenshot sweep) while they live.

use super::super::*;
use super::redraw_passes::CutsceneCam;
use super::redraw_stage::PosedFrame;

// An op-`4C 81` draw tint (`+0x74` colour / `+0x78` blend) as the
// constant per-draw cue retail's actor draw stages (far colour +
// `IR0`, `FUN_8001ADA4` -> `FUN_80043390`). One shape for placed
// objects, NPCs and the player.
fn tint_draw_cue((colour, blend): (u32, u16)) -> Option<legaia_engine_render::DrawCue> {
    let (far, max_ir0) = legaia_engine_core::world::tint_cue(colour, blend);
    Some(legaia_engine_render::DrawCue {
        far,
        near_z: -1.0,
        far_z: 0.0,
        max_ir0,
    })
}

/// The uploaded meshes (and this frame's posed meshes) the draw lists
/// borrow, one reference per window field so the borrow stays field-precise.
pub(super) struct MeshStore<'m> {
    pub meshes: &'m Vec<UploadedVramMesh>,
    pub color_meshes: &'m Vec<UploadedColorMesh>,
    pub field_lit: &'m FieldLitMeshes,
    pub field_morph_live: &'m std::collections::HashMap<usize, UploadedVramMesh>,
    pub ground_heightfield: &'m Option<UploadedVramMesh>,
    pub ground_crop: &'m Option<(u32, Option<UploadedVramMesh>)>,
    pub baka_gpu: &'m Option<minigames::BakaDuelGpu>,
    pub muscle_gpu: &'m Option<minigames::BakaDuelGpu>,
    pub fishing_gpu: &'m Option<minigames::BakaDuelGpu>,
    pub dance_venue_gpu: &'m Option<minigames::DanceVenueGpu>,
    pub dance_cast_gpu: &'m Option<minigames::DanceCastGpu>,
    pub npc_morph_static: &'m std::collections::HashMap<u8, NpcPosedHalves>,
    /// `slot -> this frame's posed halves` out of `npc_pose_cache`.
    pub npc_posed: &'m std::collections::HashMap<u8, &'m NpcPosedHalves>,
    pub posed: &'m PosedFrame,
}

/// The frame's draw lists and their per-draw marks.
pub(super) struct DrawLists<'m> {
    pub draws: Vec<SceneDraw<'m>>,
    /// Untextured (F*/G*) draws, on the colour pipeline.
    pub color_draws: Vec<ColorSceneDraw<'m>>,
    /// Object-effect clips by textured draw index.
    pub clip_marks: Vec<(usize, legaia_engine_render::DrawClip)>,
    /// Per-draw NCLIP words by textured draw index.
    pub nclip_marks: Vec<(usize, u32)>,
    /// Object-effect clips by colour draw index.
    pub color_clip_marks: Vec<(usize, legaia_engine_render::DrawClip)>,
}

/// The per-draw cues every mode's pass shares: the player's op-`4C 81` tint
/// and the object-effect clips.
pub(super) struct FrameCues {
    player_tint_cue: Option<legaia_engine_render::DrawCue>,
    effect_clips: Vec<(
        legaia_engine_core::world::ActorTintKey,
        legaia_engine_core::object_effect::EffectClip,
        f32,
    )>,
}

impl FrameCues {
    /// The object-effect clip a draw keyed `key` at `model` carries.
    fn effect_clip(
        &self,
        key: legaia_engine_core::world::ActorTintKey,
        model: &Mat4,
    ) -> Option<legaia_engine_render::DrawClip> {
        let effect_clips = &self.effect_clips;
        let (_, clip, scale) = effect_clips.iter().find(|(k, _, _)| *k == key)?;
        let rows = [
            model.row(0).to_array(),
            model.row(1).to_array(),
            model.row(2).to_array(),
        ];
        let mc = clip.in_mesh_space(rows, *scale);
        Some(legaia_engine_render::DrawClip {
            m: mc.m,
            lo: mc.lo,
            hi: mc.hi,
        })
    }
}

/// The key a placed object's record draws under.
fn object_key(record: Option<usize>) -> Option<legaia_engine_core::world::ActorTintKey> {
    record.map(|r| legaia_engine_core::world::ActorTintKey::Object(r as u16))
}

/// Everything one frame's scene draw lists are built from.
pub(super) struct DrawCtx<'a, 'm> {
    pub app: &'a PlayWindowApp,
    pub store: MeshStore<'m>,
    pub r: &'a legaia_engine_render::Renderer,
    pub cam: Mat4,
    pub cutscene_cam: Option<CutsceneCam>,
    pub view_cells: &'a Option<legaia_engine_core::field_view_window::ViewCells>,
    pub in_world_map: bool,
    /// The party-wipe hold keeps drawing the frozen battle under the boot UI.
    pub game_over_hold: bool,
}

impl<'m> DrawCtx<'_, 'm> {
    /// Build the frame's draw lists for whichever mode owns the 3D frame.
    pub(super) fn build_scene_draws(&self) -> DrawLists<'m> {
        let app = self.app;
        let store = &self.store;
        let cam = self.cam;
        let in_world_map = self.in_world_map;
        let game_over_hold = self.game_over_hold;
        let player_tint_cue = app
            .session
            .host
            .world
            .player_draw_tint()
            .and_then(tint_draw_cue);
        let mut out = DrawLists {
            draws: Vec::new(),
            color_draws: Vec::new(),
            clip_marks: Vec::new(),
            nclip_marks: Vec::new(),
            color_clip_marks: Vec::new(),
        };
        // Object-effect clips (a raised `+0x42`, field-VM `4C C2`): the
        // draw indices that carry one, staged on the renderer below.
        // Shared kernel `World::object_effect_mesh_clip`; the browser
        // page asks the same one (`field_placement_effect_clips`).

        // Per-draw NCLIP words (`Renderer::set_draw_nclip`): the battle
        // bodies' single-sided rule over the battle pass's both-sided one.

        let effect_clips = app.session.host.world.object_effect_clips();
        let cues = FrameCues {
            player_tint_cue,
            effect_clips,
        };
        // Untextured (F*/G*) field props, drawn on the colour
        // pipeline alongside the textured `draws`.
        if (app.boot_ui.is_active() && !game_over_hold) || app.menu_runtime.covers_field() {
            // A shop is a menu-overlay session: the field overlay is
            // swapped out and the screen behind the windows is black.
            // Boot UI is fullscreen - suppress 3D draws.
        } else if let Some(g) = store
            .baka_gpu
            .as_ref()
            .or(store.muscle_gpu.as_ref())
            .or(store.fishing_gpu.as_ref())
        {
            let DrawLists {
                draws, color_draws, ..
            } = &mut out;
            // The Baka duel owns the 3D frame: the engine-posed fighters,
            // ghosts, walls and floor under the arena camera. The Muscle
            // Dome's arena surface draws the same way: the shell, the
            // ground grid, the fighter and the monster under the dome
            // camera.
            if let Some(m) = g.textured.as_ref() {
                draws.push(SceneDraw {
                    mesh: m,
                    mvp: g.mvp,
                    cue: None,
                });
            }
            if let Some(m) = g.untextured.as_ref() {
                color_draws.push(ColorSceneDraw {
                    mesh: m,
                    mvp: g.mvp,
                    cue: None,
                });
            }
        } else if app.slot_gpu.is_some() {
            // The slot machine: the overlay's whole frame is its own
            // cabinet scene, drawn as screen primitives below - the
            // walked-in casino floor is not on screen.
        } else if app.session.host.world.muscle_hub_between_legs() {
            // The arena hub between two legs: retail runs it in arena
            // mode `0x18` with no 3D scene - the ringside still and the
            // INTERVAL / ROUND screens are the whole frame.
        } else if let Some(g) = store.dance_venue_gpu.as_ref() {
            let DrawLists {
                draws, color_draws, ..
            } = &mut out;
            // The dance venue owns the 3D frame: the `other7` hall the
            // dance entry loads, under the venue camera the frame
            // resolves through (`FieldCameraFrame::Venue`). The walked-in
            // scene's actors and geometry are not drawn - retail's dance
            // is a scene of its own.
            // The hall, baked in raw world coordinates and cut to the
            // triangles the PSX GPU draws from this eye
            // (`refresh_dance_venue_view`).
            if let Some(mesh) = g.textured_gpu.as_ref() {
                draws.push(SceneDraw {
                    mesh,
                    mvp: cam,
                    cue: None,
                });
            }
            if let Some(mesh) = g.untextured_gpu.as_ref() {
                color_draws.push(ColorSceneDraw {
                    mesh,
                    mvp: cam,
                    cue: None,
                });
            }
            // The floor's bodies, posed in raw world coordinates by the
            // engine surface (`refresh_dance_cast_gpu`).
            if let Some(c) = store.dance_cast_gpu.as_ref() {
                if let Some(mesh) = c.textured.as_ref() {
                    draws.push(SceneDraw {
                        mesh,
                        mvp: cam,
                        cue: None,
                    });
                }
                if let Some(mesh) = c.untextured.as_ref() {
                    color_draws.push(ColorSceneDraw {
                        mesh,
                        mvp: cam,
                        cue: None,
                    });
                }
            }
        } else if in_world_map {
            self.push_world_map(&mut out, &cues);
        } else {
            let in_battle = app.session.host.world.mode == SceneMode::Battle;
            if in_battle {
                self.push_battle_backdrop(&mut out);
            } else {
                self.push_field_env(&mut out, &cues);
            }
            // Actors ride the same battle rotation as the dome +
            // grid but with the retail 4x world-scale base matrix
            // (`0x8007BF10 = 16384*I`) composed under it - the
            // `FUN_80048A08` per-actor camera composition. The
            // uniform scale commutes with the per-model Y-flip, so
            // composing it on the camera side scales both the mesh
            // and the actor's stage translation, exactly like
            // retail. Outside a stage-dome battle the synthetic
            // AABB-framing camera stays unscaled (it frames the
            // raw actor coordinates).
            let actor_cam = if in_battle && app.battle_stage_mesh.is_some() {
                cam * Mat4::from_scale(Vec3::splat(BATTLE_WORLD_SCALE))
            } else {
                cam
            };
            self.push_actors(&mut out, &cues, in_battle, actor_cam);
            // Arts after-image ghosts trail the live bodies: additive
            // flat-colour copies at the rate-scheduled historical poses.
            // Retail pushes each ghost 0x50 OT buckets deeper than the
            // live body (FUN_80048A08), so under painter ordering the
            // body covers the coincident screen area REGARDLESS of true
            // depth - including the attack camera's behind-the-attacker
            // framings, where the trailing ghost is genuinely nearer the
            // camera than the body. A depth buffer cannot express that
            // with any compare function (the blend pass's GreaterEqual
            // passed every coincident fragment and washed the whole mesh
            // additive - the "monster glows yellow" defect), so each
            // ghost is scaled uniformly ABOUT THE EYE until it sits past
            // the body's distance: every vertex slides along its own
            // camera ray (screen silhouette unchanged) while the body's
            // opaque depth wins wherever they overlap.
            if in_battle {
                self.push_battle_ghosts(&mut out, actor_cam);
            }
        }
        out
    }

    /// The world map: the ground heightfield, the placed landmarks (live
    /// transforms, model swaps, tints, decoration cues) and the party leader.
    fn push_world_map(&self, out: &mut DrawLists<'m>, cues: &FrameCues) {
        let app = self.app;
        let store = &self.store;
        let cam = self.cam;
        let cutscene_cam = self.cutscene_cam;
        let player_tint_cue = cues.player_tint_cue;
        let effect_clip = |key: legaia_engine_core::world::ActorTintKey, model: &Mat4| {
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
            legaia_engine_core::overworld_ground_cue::decoration_draw_cue(mvp.w_axis.w, deco_curve)
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
        // the browser page folds the same two tables into its
        // overworld placements (`field_placement_moves` /
        // `field_placement_models`).
        let wm_moves = app.session.host.world.object_draw_displacements();
        let wm_turns = app.session.host.world.object_draw_turn_matrices();
        let wm_models = app.session.host.world.object_live_models();
        let wm_swaps = legaia_engine_core::field_env::placed_model_swaps(
            &app.world_map_terrain_records,
            wm_models,
        );
        let wm_color_swaps = legaia_engine_core::field_env::placed_model_swaps(
            &app.world_map_terrain_color_records,
            wm_models,
        );
        let wm_live =
            |records: &[Option<usize>], swaps: &[Option<usize>], i: usize, model: &Mat4| {
                let record = records.get(i).copied().flatten();
                let moved =
                    Mat4::from_cols_array(&legaia_engine_core::field_env::live_placed_model(
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
                        legaia_engine_core::world::ActorTintKey::Player,
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
                        legaia_engine_core::world::ActorTintKey::Player,
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

    /// The battle stage backdrop (the dome pair) and its untextured half.
    fn push_battle_backdrop(&self, out: &mut DrawLists<'m>) {
        let app = self.app;
        let store = &self.store;
        let cam = self.cam;
        let DrawLists {
            draws, color_draws, ..
        } = out;
        // Battle backdrop: the scene's `scene_tmd_stream`
        // dome (PROT 88 for the overworld map01 battle) -
        // sky hemisphere + mountain arc + grass - drawn at its
        // **raw world coordinates** under the exact retail
        // orbit camera (`retail_battle_mvp`). `model = F`
        // (plain Y-flip): the camera bakes in `F`, so
        // `cam * F` recovers the raw PSX vertex the retail
        // transform expects.
        //
        // World-fixed, and **one draw but two copies** - do not
        // read the single `draws.push` below as "one instance".
        // Retail sets the dome up as a background **actor**
        // (`FUN_800513F0`: `tmd_register` -> `DAT_8007C018[]`
        // + `FUN_80020de0` actor_alloc + `FUN_80020f88` link)
        // rendered by the normal actor path `FUN_80048A08`, and
        // it renders it **twice**: a second copy under either a
        // per-stage `Ry(180)` half-turn or a mirror-X selected by
        // the SCUS table `DAT_80078B50`
        // (`legaia_asset::battle_backdrop::SecondCopy`). The
        // stage TMD is a HALF arena, not a full surround
        // (map01's dome is a front half, verts `Z in [-1260,
        // +12155]`; town01's arena is authored entirely at
        // `X >= 0`, open side facing -X - the sea horizon in
        // the retail Tetsu close-up), which is why the second
        // copy exists at all.
        //
        // The port applies that copy at **mesh build** time, not
        // here: `battle_stage_meshes` pre-appends it with
        // `VramMesh::append_scaled` (winding flipped for the
        // mirror), so the page and the window both upload one
        // mesh and this loop pushes one draw. An older comment
        // here said "the mirror draw is removed (one instance,
        // like retail)" - that described a *withdrawn* draw-time
        // second instance and was wrong about retail besides.
        // See `project_battle_backdrop_is_prot88_dome` and
        // `docs/subsystems/battle.md` § the second copy.
        //
        // **Stage scale.** The stage rides the same
        // [`BATTLE_WORLD_SCALE`] base matrix the actors do
        // (`0x8007BF10 = 16384*I`, composed per drawn object by
        // `FUN_80048A08` - and the dome is registered as an
        // ordinary background *actor*, so it goes through that
        // same path). Drawing it at raw 1x while the actors ride
        // 4x put the two classes in different worlds: the phase
        // camera's translation trio is authored in the scaled
        // stage space, so against 1x geometry the eye orbited at
        // four times the intended radius and swung *through* the
        // arena shell - the frame filling with one magnified
        // wall - and every actor stood 3x its seat distance away
        // from the ground cell it was supposed to be on. One
        // scale for every battle draw class is what makes the
        // arena a backdrop and the grid a floor.
        // The backdrop pair's own depth cue (`FUN_80050120`'s
        // `+0x78` ramp): pulled toward black through a summon
        // close-up, and off the draw entirely at full weight -
        // a cast module that wants the stage gone (PROT 0903's
        // fire tunnel) drives it there.
        let backdrop_cue = app.session.host.world.battle_backdrop_cue();
        let stage_cue = backdrop_cue
            .filter(|&w| w > 0.0)
            .map(|w| legaia_engine_render::DrawCue {
                far: [0.0; 3],
                near_z: -1.0,
                far_z: 0.0,
                max_ir0: w,
            });
        if backdrop_cue.is_some()
            && let Some(stage_idx) = app.battle_stage_mesh
            && let Some(mesh) = store.meshes.get(stage_idx)
        {
            let flip = PlayWindowApp::battle_stage_model();
            // Half-arena stage in the scaled battle stage space.
            draws.push(SceneDraw {
                mesh,
                mvp: cam * flip,
                cue: stage_cue,
            });
        }
        // ...and the shell's untextured `F*`/`G*` half on the
        // colour pipeline, at the identical transform. Retail
        // walks one primitive list, so these panels (sky band,
        // painted wall faces, flat water) belong to the same
        // backdrop draw; without them the shell has holes.
        if backdrop_cue.is_some()
            && let Some(cidx) = app.battle_stage_color_mesh
            && let Some(cmesh) = store.color_meshes.get(cidx)
        {
            color_draws.push(ColorSceneDraw {
                mesh: cmesh,
                mvp: cam * PlayWindowApp::battle_stage_model(),
                cue: stage_cue,
            });
        }
    }

    /// The field's environment layers (ground, terrain tiles, placements,
    /// posed props, their colour halves), the player's colour half, the
    /// placed NPCs and the tile-board actors.
    fn push_field_env(&self, out: &mut DrawLists<'m>, cues: &FrameCues) {
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
    fn push_field_terrain(&self, out: &mut DrawLists<'m>, layer_on: &dyn Fn(&str) -> bool) {
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
            |model: &Mat4, f: Option<&Option<super::field_render::CameraFacing>>| -> Mat4 {
                let Some(Some((skip, rot))) = f else {
                    return *model;
                };
                let Some(k) =
                    legaia_engine_render::gte::decoration_cell_basis(*skip, facing_cam.as_ref())
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
                if !legaia_engine_core::field_view_window::terrain_draw_visible(
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
                if !legaia_engine_core::field_view_window::terrain_draw_visible(
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
    fn push_field_placements(
        &self,
        out: &mut DrawLists<'m>,
        cues: &FrameCues,
        layer_on: &dyn Fn(&str) -> bool,
    ) {
        let app = self.app;
        let store = &self.store;
        let cam = self.cam;
        let view_cells = self.view_cells;
        let effect_clip = |key: legaia_engine_core::world::ActorTintKey, model: &Mat4| {
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
        // behind it, is not drawn. Judged under the retail
        // camera only - the `F3` debug orbit frames from a
        // vantage retail never had.
        // A placed object a script has moved (`A3` seat, `4C 42`
        // lift under the actor's `0x20000000` height law) draws at
        // its actor's live position: retail's case-5 draw reads
        // the actor, not the `.MAP` record. The shared kernel is
        // `World::object_draw_displacements`; the browser play
        // page folds the same table in (`field_placement_moves`).
        let object_moves = app.session.host.world.object_draw_displacements();
        // ... and at its actor's live angles: a script that turns
        // the object (op `0x38`, `4C 48`) turns the drawn mesh
        // about its origin (`World::object_draw_turn_matrices`,
        // the browser page's `field_placement_turns`).
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
            Mat4::from_cols_array(&legaia_engine_core::field_env::live_placed_model(
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
        let object_swaps = legaia_engine_core::field_env::placed_model_swaps(
            &app.field_placement_records,
            app.session.host.world.object_live_models(),
        );
        let object_color_swaps = legaia_engine_core::field_env::placed_model_swaps(
            &app.field_placement_color_records,
            app.session.host.world.object_live_models(),
        );
        let object_cue = |record: Option<usize>| {
            let &(colour, blend) = object_tints.get(&record?)?;
            tint_draw_cue((colour, blend))
        };
        let place_near_culled = |mvp: &Mat4| {
            !app.field_debug_camera
                && legaia_engine_core::field_env::placed_origin_near_culled(mvp.w_axis.w)
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
                if !legaia_engine_core::field_env::placed_draw_live(
                    app.field_placement_window_keys
                        .get(di)
                        .and_then(Option::as_ref),
                    static_window,
                ) || !legaia_engine_core::field_view_window::placed_actor_visible(
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
                let tag = super::field_render::LIT_VARIANT_TAG;
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
                if !legaia_engine_core::field_env::placed_draw_live(
                    app.field_placement_color_window_keys
                        .get(di)
                        .and_then(Option::as_ref),
                    static_window,
                ) || !legaia_engine_core::field_view_window::placed_actor_visible(
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

    /// The field actors: the player's colour half, the placed NPCs and the
    /// tile-board actors.
    fn push_field_npcs(
        &self,
        out: &mut DrawLists<'m>,
        cues: &FrameCues,
        layer_on: &dyn Fn(&str) -> bool,
    ) {
        let app = self.app;
        let store = &self.store;
        let cam = self.cam;
        let player_tint_cue = cues.player_tint_cue;
        let effect_clip = |key: legaia_engine_core::world::ActorTintKey, model: &Mat4| {
            cues.effect_clip(key, model)
        };
        let player_color_posed = &store.posed.player_color_posed;
        let npc_posed = store.npc_posed;
        let DrawLists {
            draws,
            color_draws,
            clip_marks,
            color_clip_marks,
            ..
        } = out;
        // The player's untextured mesh half (pants /
        // sleeves), following the actor's live transform.
        // Prefer this frame's posed rebuild (idle/walk
        // playback); fall back to the static rest pose.
        if let Some((cidx, slot)) = app.player_color_draw
            && let Some(mesh) = player_color_posed
                .as_ref()
                .or_else(|| store.color_meshes.get(cidx))
        {
            if let Some(c) = effect_clip(
                legaia_engine_core::world::ActorTintKey::Player,
                &app.actor_model(slot),
            ) {
                color_clip_marks.push((color_draws.len(), c));
            }
            color_draws.push(ColorSceneDraw {
                mesh,
                mvp: cam * app.actor_model(slot),
                cue: player_tint_cue,
            });
        }
        // Field NPCs + animated props at their live
        // positions (motion-VM walkers update
        // `field_npc_positions`; everyone else stands at
        // the spawn tile), floor-snapped like the player.
        let w = &app.session.host.world;
        for d in app.field_npc_draws.iter().filter(|_| layer_on("npc")) {
            // The NPC's whole draw pose is the engine's
            // (`World::field_npc_draw_pose`, which the browser play page
            // reads through `play_npc_draw_poses`): `None` for an actor
            // parked in the off-map hide box or at zero render scale
            // (`actor[+0x72] = 0`, the invisible interaction markers);
            // otherwise its live position at the floor / scripted-arc
            // height, its composed yaw, its authored tilt and its render
            // scale. Raw retail-convention transform (no model flip): the
            // field camera's FIELD_WORLD_FLIP provides the single net Y
            // negation.
            let Some(pose) = w.field_npc_draw_pose(d.slot, d.spawn) else {
                continue;
            };
            // A tilted slot takes the full `Rx * Ry * Rz` composer the
            // placed-object pass uses (`placement_rotation`); an untilted
            // one keeps the yaw-only matrix.
            let rot = if pose.tilted() {
                legaia_engine_render::battle_intro::placement_rotation(
                    pose.pitch, pose.yaw, pose.roll,
                )
            } else {
                Mat4::from_rotation_y(f32::from(pose.yaw) / 4096.0 * std::f32::consts::TAU)
            };
            // Retail folds the render scale in after the rotation
            // (`ScaleMatrix` on the rotation, never the translation).
            let model = Mat4::from_translation(Vec3::from(pose.pos))
                * rot
                * Mat4::from_scale(Vec3::splat(pose.scale));
            // The actor's op-`4C 81` draw tint (`+0x74` /
            // `+0x78`), staged as a constant per-draw cue on
            // both mesh halves - the browser page reads the same
            // `World::field_npc_draw_tint` (`play_npc_tints`).
            let cue = w
                .field_npc_draw_tint(d.slot as usize)
                .and_then(tint_draw_cue);
            let npc_clip = effect_clip(
                legaia_engine_core::world::ActorTintKey::Npc(d.slot as usize),
                &model,
            );
            if let Some(c) = npc_clip {
                // Both mesh halves push below; mark the slot each
                // lands in.
                clip_marks.push((draws.len(), c));
                color_clip_marks.push((color_draws.len(), c));
            }
            // A clip-less NPC's op-`0x4B` morph re-stages
            // its static mesh (`npc_morph_static`).
            let posed = npc_posed
                .get(&d.slot)
                .copied()
                .or_else(|| store.npc_morph_static.get(&d.slot));
            match (posed.and_then(|p| p.0.as_ref()), d.mesh_idx) {
                (Some(mesh), _) => draws.push(SceneDraw {
                    mesh,
                    mvp: cam * model,
                    cue,
                }),
                (None, Some(mi)) => {
                    if let Some(mesh) = store.meshes.get(mi) {
                        draws.push(SceneDraw {
                            mesh,
                            mvp: cam * model,
                            cue,
                        });
                    }
                }
                (None, None) => {}
            }
            match (posed.and_then(|p| p.1.as_ref()), d.color_idx) {
                (Some(mesh), _) => color_draws.push(ColorSceneDraw {
                    mesh,
                    mvp: cam * model,
                    cue,
                }),
                (None, Some(ci)) => {
                    if let Some(mesh) = store.color_meshes.get(ci) {
                        color_draws.push(ColorSceneDraw {
                            mesh,
                            mvp: cam * model,
                            cue,
                        });
                    }
                }
                (None, None) => {}
            }
        }
        // Tile-board tile actors: one mesh instance per drawable
        // cell in this frame's deferred draw list (retail
        // `overlay_0897_801e0f3c` - a cell value's shared actor
        // draws at EVERY cell holding that value, not just its
        // own last-repositioned transform). Only slots the spawn
        // drain above uploaded draw (`drained_spawn_slots`): a
        // slot still wearing `upload_assets`' naive pre-bind
        // would render an unrelated scene mesh, and unresolved
        // templates (no `tmd_ref`) never upload - both degrade
        // to "no draw".
        for d in crate::tile_board_draws::tile_board_actor_draws(w) {
            if !app.drained_spawn_slots.contains(&d.slot) {
                continue;
            }
            let Some(tmd_idx) = w.actors.get(d.slot as usize).and_then(|a| a.tmd_binding) else {
                continue;
            };
            if let Some(mesh) = store.meshes.get(tmd_idx) {
                // Raw retail-convention transform, like the NPC
                // draws: the field camera's FIELD_WORLD_FLIP
                // provides the single net Y negation.
                // The board's fade scales each tile about
                // its own origin (the tile actor's `+0x72`).
                let model = Mat4::from_translation(Vec3::new(d.world[0], d.world[1], d.world[2]))
                    * Mat4::from_scale(Vec3::splat(d.scale));
                draws.push(SceneDraw {
                    mesh,
                    mvp: cam * model,
                    cue: None,
                });
            }
        }
    }

    /// The battle ground grid and every drawn actor (battle bodies with their
    /// cursor / tint cues and NCLIP words, the field player).
    fn push_actors(
        &self,
        out: &mut DrawLists<'m>,
        cues: &FrameCues,
        in_battle: bool,
        actor_cam: Mat4,
    ) {
        let app = self.app;
        let store = &self.store;
        let cam = self.cam;
        let player_tint_cue = cues.player_tint_cue;
        let effect_clip = |key: legaia_engine_core::world::ActorTintKey, model: &Mat4| {
            cues.effect_clip(key, model)
        };
        let posed_overrides = &store.posed.posed_overrides;
        let DrawLists {
            draws,
            clip_marks,
            nclip_marks,
            ..
        } = out;
        // Flat tiled ground grid (retail's func_0x801d02c0 grass)
        // under the actors, on the same battle camera so the
        // party stands on it and the foreground reads as grass
        // instead of the bare clear colour. `cam` bakes in the
        // Y-flip, so `* flip` recovers the raw PSX y=0 plane.
        // The per-draw cue is the emitter's own DPCS depth cue:
        // `IR0 = SZ >> 2` per vertex (unsaturated - `max_ir0`
        // rides past 1.0 exactly like retail's bare `mtc2`),
        // blending toward the battle's staged far colour, so the
        // floor washes out with distance the way retail's does.
        // The grid rides [`BATTLE_WORLD_SCALE`] like every other
        // battle draw class (see the stage-scale note on the
        // backdrop draw above): the party stands ON its own grid
        // cell only if the cell and the actor's stage translation
        // are lifted by the same factor. The DPCS ramp is NOT
        // lifted: it is keyed on the vertex's view depth `SZ`, and
        // the camera translation trio is already in view units, so
        // the fragment depth is retail's `SZ` unscaled (see
        // docs/subsystems/battle.md, the grid's near colour and cue
        // depth, for the capture that pins it).
        if in_battle
            && let Some(gi) = app.battle_ground_mesh
            && let Some(gmesh) = store.meshes.get(gi)
        {
            use legaia_engine_vm::battle_ground_grid as grid;
            let flip = PlayWindowApp::battle_stage_model();
            draws.push(SceneDraw {
                mesh: gmesh,
                mvp: cam * flip,
                cue: app
                    .battle_ground_cue_far
                    .map(|far| legaia_engine_render::DrawCue {
                        far,
                        near_z: 0.0,
                        far_z: grid::grid_cue_far_z(),
                        max_ir0: grid::grid_cue_max_ir0(),
                    }),
            });
        }
        // The camera the battle bodies' tint pass judges depth under
        // (`World::battle_actor_draw_plan`): the phase-scripted dome
        // camera this pass projects with, or none outside a
        // stage-dome battle (the body is then judged at retail's
        // parked depth).
        let battle_pose = (in_battle && app.battle_stage_mesh.is_some())
            .then(|| app.session.host.world.battle_cam_pose());
        for (i, actor) in app.session.host.world.actors.iter().enumerate() {
            let Some(tmd_idx) = actor.tmd_binding else {
                continue;
            };
            // Draw only ACTIVE (spawned) actors -
            // `World::actor_slot_drawn`. The never-spawned slots
            // `init_scene_animations` pre-binds would otherwise draw
            // every scene-pack mesh at world (0,0,0): in a stage-dome
            // battle the "duplicate Vahn", in the field uru's sky /
            // cliff pack smeared across the whole frame. The browser
            // play page never drew them (it draws the player and the
            // NPC catalog, not `world.actors`).
            if !app
                .session
                .host
                .world
                .actor_slot_drawn(i, in_battle && app.battle_stage_mesh.is_none())
            {
                continue;
            }
            // The summon band's hide (`+0x21C = 0xFF` with the prim
            // word zeroed, `0x801E4B30..0x801E4B6C`): every party
            // seat and living monster is off screen while the
            // creature performs, restored at `0x36`.
            if in_battle
                && actor.battle.render_flag
                    == legaia_engine_vm::battle_target_group::RENDER_FLAG_HIDDEN
            {
                continue;
            }
            // Retail's per-body battle draw (`FUN_800480D8` over the
            // tint pass `FUN_8004A908`): a body whose colour word
            // comes out zero is not drawn unless the lone-monster
            // grey gate stamps it, and one nearer than view depth
            // `0xA1` is rejected by the render dispatcher.
            let battle_plan = if in_battle {
                app.session.host.world.battle_actor_draw_plan(
                    i,
                    battle_pose.as_ref(),
                    BATTLE_WORLD_SCALE,
                    app.battle_stage_outdoor,
                )
            } else {
                None
            };
            if battle_plan.is_some_and(|p| !p.drawn) {
                continue;
            }
            // Board-owned tile actors draw once per cell through the
            // deferred tile-board pass above; their own transform
            // only holds the LAST repositioned cell (and a slot the
            // drain hasn't uploaded still wears the naive pre-bind).
            // The player (tile table slot 0) stays on this path.
            if crate::tile_board_draws::is_tile_actor_slot(&app.session.host.world, i) {
                continue;
            }
            // The `opdeene` prologue cutscene is an abstract vignette
            // sequence (the "It was the Seru" Genesis-tree imagery)
            // driven by the per-actor field channels, NOT by a
            // controllable lead. `enter_field_scene` still installs the
            // free-roam player (slot 0) at the generic field cold-spawn,
            // so without this it stands in the shot as a stray mesh.
            // Scene-gated on `opdeene` so `town01`'s opening cutscene -
            // where the timeline scripts the lead actor (Vahn walking
            // out of his house) - keeps drawing him.
            if i == 0
                && app.session.host.world.active_scene_label
                    == legaia_asset::new_game::OPENING_CUTSCENE_SCENE
            {
                continue;
            }
            let mesh = posed_overrides
                .get(tmd_idx)
                .and_then(|o| o.as_ref())
                .or_else(|| store.meshes.get(tmd_idx));
            if let Some(mesh) = mesh {
                // Target-select cursor: while the command picker
                // points at an enemy row, the ported FUN_801DA6B4
                // (`engine-vm::battle_action::target_cursor_highlight`)
                // stamps three render words across the monster slots -
                // `render_flag` 5 on the pointed-at monster / 200 on
                // the rest, the bright/dim colour words, and the q12
                // `render_blend` (0x1000 = cursor up, 0 = cursor
                // down). The blend word is the render packet's
                // `+0x78` tint weight (`FUN_8004A908`), NOT a mesh
                // scale; the tint rides the per-draw GTE depth-cue
                // seam (a saturated `DrawCue` ramp = a flat blend
                // toward the cue colour) - the pointed-at monster
                // pulses bright, the others dim.
                //
                // The pulse phase is the **world display-frame**
                // clock, not this window's redraw counter: the
                // browser play page runs the same formula off
                // `World::clock.display_frames`, and a redraw
                // counter advances on frames the simulation did not
                // take (a movie, a paused world, a resize storm), so
                // the two hosts' cursors drifted apart the moment
                // either host's redraw rate left its tick rate.
                let model = app.actor_model(i);
                // Outside battle the player's op-`4C 81` draw tint
                // rides the same per-draw cue seam.
                let mut cue =
                    if !in_battle && app.session.host.world.player_actor_slot == Some(i as u8) {
                        player_tint_cue
                    } else {
                        None
                    };
                if in_battle {
                    use legaia_engine_vm::battle_action as ba;
                    let b = &actor.battle;
                    // The cursor's cue is the engine's
                    // (`battle_action::cursor_cue`), shared with the
                    // page's `play_battle_actor_cursor`.
                    if let Some((far, max_ir0)) =
                        ba::cursor_cue(b.render_flag, app.session.host.world.clock.display_frames)
                    {
                        log::trace!(
                            "target cursor: actor {i} flag {} ir0 {max_ir0:.2}",
                            b.render_flag
                        );
                        cue = Some(legaia_engine_render::DrawCue {
                            far,
                            near_z: -1.0,
                            far_z: 0.0,
                            max_ir0,
                        });
                    }
                    // Retail's one tint seam: `FUN_8004A908` packs
                    // the actor's `+0x04` lanes (`>> 2`) into the
                    // render node's `+0x74` and copies the `+0x0C`
                    // blend into `+0x78` whenever it is non-zero
                    // (`0x8004AA24..0x8004AA70`); the draw pass
                    // `FUN_80048A08` stages those as the GTE far
                    // colour + IR0 (`gp[0x9D8]` / `gp[0x9DC]`,
                    // `0x80048BEC..0x80048C00`). So the prim's
                    // modulation colour becomes `baked + (tint -
                    // baked) * blend / 0x1000` and the GPU still
                    // multiplies the texel through it - a hit reads
                    // as the actor's own texture pushed toward the
                    // element colour, never a flat silhouette
                    // (retail `battle_gimard_tail_fire_a`: Vahn at
                    // `(0xC7,0x38,0x38)` x `0x1000` is red 160..248
                    // over his texture). Every writer rides this
                    // one rule - the impact triple (`FUN_801EC3E4` /
                    // `FUN_801E09F8` / the clip-`0x18` arms), the
                    // item/spirit cue-group flash, and the
                    // presentation SM's colour arms
                    // (`FUN_80050120`). `DrawCue.far` is display
                    // units and the shader's far term is
                    // `texel * far * 255 / 128` - retail's own
                    // `texel * colour / 128`. The cursor arms above
                    // keep their own cue (their retail look is the
                    // same rule; that thread is not this one's).
                    //
                    // `render_flag == 2` (the capture / defeat fade,
                    // SM arm 2) also ORs `0x81000000` into the node's
                    // mode word, so the fading actor draws ABE|ABR1
                    // additive and black = gone. The override builder
                    // applies that word's blend to every prim of the
                    // posed mesh and the rest mesh alike
                    // (`BattleActorDrawPlan::apply_body_blend`,
                    // `redraw_passes.rs`), so the fade takes its cue
                    // whenever its word raises ABE
                    // (`BattleActorDrawPlan::tint_cue_applies`, the
                    // page's gate too), never as an opaque black
                    // silhouette. Once its
                    // lanes reach zero the draw plan above skips the
                    // body (`FUN_800480D8`'s word-zero arm), as it
                    // does the summon hide. The two cursor flags keep
                    // their own cue.
                    //
                    // The cue is the whole tint pass, not only its
                    // blend arm: with no blend running retail still
                    // stages the lanes as the far colour, weighted by
                    // view depth, and a body past half its radius
                    // (in `/16` depth units) takes the depth-cue arm
                    // - a darker copy of its colour, or a brighter
                    // one on the outdoor stages - plus the status
                    // colours. `World::battle_actor_draw_plan`.
                    if let Some(p) = battle_plan
                        && p.tint_cue_applies(b.render_flag)
                    {
                        cue = Some(legaia_engine_render::DrawCue {
                            far: p.cue_far(),
                            near_z: -1.0,
                            far_z: 0.0,
                            max_ir0: p.cue_ir0(),
                        });
                    }
                }
                // The player's object-effect clip (`CC F8 C2`), off
                // the battle stage.
                if !in_battle
                    && app.session.host.world.player_actor_slot == Some(i as u8)
                    && let Some(c) =
                        effect_clip(legaia_engine_core::world::ActorTintKey::Player, &model)
                {
                    clip_marks.push((draws.len(), c));
                }
                // Retail draws a battle body single-sided unless its
                // colour word carries the double-sided bit
                // (`BattleActorDrawPlan::nclip_mode`, the page's
                // placement `nclip` too); the battle pass itself
                // stays both-sided.
                if let Some(p) = battle_plan {
                    nclip_marks.push((draws.len(), p.nclip_mode()));
                }
                draws.push(SceneDraw {
                    mesh,
                    mvp: actor_cam * model,
                    cue,
                });
            }
        }
    }

    /// The arts after-image ghosts, pushed past their bodies about the eye.
    fn push_battle_ghosts(&self, out: &mut DrawLists<'m>, actor_cam: Mat4) {
        let store = &self.store;
        let battle_ghost_uploads = &store.posed.battle_ghost_uploads;
        let DrawLists { color_draws, .. } = out;
        use legaia_engine_core::battle_afterimage as ai;
        let eye = ai::camera_eye_from_vp(&actor_cam.to_cols_array());
        for (mesh, model, gpos, bpos) in battle_ghost_uploads {
            let push = match eye {
                Some(e) => {
                    let k = ai::ghost_eye_push_scale(e, *gpos, *bpos, ai::GHOST_EYE_PUSH_MARGIN);
                    let ev = Vec3::from(e);
                    Mat4::from_translation(ev)
                        * Mat4::from_scale(Vec3::splat(k))
                        * Mat4::from_translation(-ev)
                }
                None => Mat4::IDENTITY,
            };
            color_draws.push(ColorSceneDraw {
                mesh,
                mvp: actor_cam * push * *model,
                cue: None,
            });
        }
    }
}

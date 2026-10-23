//! The actor half of the scene draw lists: field NPCs, and the player /
//! party / battle combatants.

use super::*;

impl<'m> DrawCtx<'_, 'm> {
    /// The field actors: the player's colour half, the placed NPCs and the
    /// tile-board actors.
    pub(super) fn push_field_npcs(
        &self,
        out: &mut DrawLists<'m>,
        cues: &FrameCues,
        layer_on: &dyn Fn(&str) -> bool,
    ) {
        let app = self.app;
        let store = &self.store;
        let cam = self.cam;
        let player_tint_cue = cues.player_tint_cue;
        let effect_clip = |key: legaia_engine_field::object_tint::ActorTintKey, model: &Mat4| {
            cues.effect_clip(key, model)
        };
        let player_color_posed = &store.posed.player_color_posed;
        let npc_posed = store.npc_posed;
        let DrawLists {
            draws,
            color_draws,
            clip_marks,
            color_clip_marks,
            nclip_marks,
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
                legaia_engine_field::object_tint::ActorTintKey::Player,
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
            // Retail's actor cull (`FUN_801D79E8`) under the visible-tile
            // crop, the gate the placed-object pass asks too.
            if !legaia_engine_world_views::field_view_window::npc_actor_visible(
                w,
                self.view_cells.as_ref(),
                d.slot,
                d.spawn,
            ) {
                continue;
            }
            // A tilted slot takes the full `Rx * Ry * Rz` composer the
            // placed-object pass uses (`placement_rotation`); an untilted
            // one keeps the yaw-only matrix.
            let rot = if pose.tilted() {
                legaia_render_kernels::battle_intro::placement_rotation(
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
                legaia_engine_field::object_tint::ActorTintKey::Npc(d.slot as usize),
                &model,
            );
            if let Some(c) = npc_clip {
                // Both mesh halves push below; mark the slot each
                // lands in.
                clip_marks.push((draws.len(), c));
                color_clip_marks.push((color_draws.len(), c));
            }
            // An NPC the op-`43 07` deformation is breaking up draws its
            // shards (`PosedFrame::npc_shattered`), both sides: the shatter
            // renderer issues no NCLIP. A clip-less NPC's op-`0x4B` morph
            // re-stages its static mesh (`npc_morph_static`).
            let shattered = store.posed.npc_shattered.get(&d.slot);
            if shattered.is_some() {
                nclip_marks.push((draws.len(), legaia_render_kernels::shatter::SHATTER_NCLIP));
            }
            let posed = shattered
                .or_else(|| npc_posed.get(&d.slot).copied())
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
        for d in legaia_engine_world_views::tile_board::tile_board_actor_draws(w) {
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
    pub(super) fn push_actors(
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
        let effect_clip = |key: legaia_engine_field::object_tint::ActorTintKey, model: &Mat4| {
            cues.effect_clip(key, model)
        };
        let posed_overrides = &store.posed.posed_overrides;
        let DrawLists {
            draws,
            clip_marks,
            nclip_marks,
            color_draws,
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
            && app.session.host.world.battle_ground_grid_drawn()
            && let Some(gi) = app.battle_ground_mesh
            && let Some(gmesh) = store.meshes.get(gi)
        {
            use legaia_engine_battle_vm::battle_ground_grid as grid;
            let flip = PlayWindowApp::battle_stage_model();
            draws.push(SceneDraw {
                mesh: gmesh,
                // Retail links every grid quad into one far ordering-table
                // entry (`0x3FEC >> otshift`, `0x801D02D4..0x801D02E0`): the
                // floor never hides what is drawn after it.
                mvp: legaia_engine_render::pin_to_far_bucket(cam * flip),
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
        // The backdrop's drawn slot 1 - on a stage that keeps object 1,
        // the horizon mist ribbon - once per backdrop copy, each under the
        // frame's slot angle (`backdrop_slot_1_basis`, the per-object
        // `FUN_8004629C` turn of `FUN_8001ADA4`'s backdrop arm). Same stage
        // model and depth cue as the shell.
        if in_battle
            && let Some(w) = app.session.host.world.battle_backdrop_cue()
            && let Some((_, second)) = app.battle_stage_shell.as_ref()
        {
            let yaw = app.session.host.world.battle_backdrop_slot_1_yaw();
            let cue = (w > 0.0).then_some(legaia_engine_render::DrawCue {
                far: [0.0; 3],
                near_z: -1.0,
                far_z: 0.0,
                max_ir0: w,
            });
            for copy_b in [false, true] {
                let rows = legaia_engine_scene_host::scene_host::backdrop_slot_1_basis(
                    *second, yaw, copy_b,
                );
                let basis = Mat4::from_mat3(glam::Mat3::from_cols_array_2d(&rows).transpose());
                let mvp = cam * PlayWindowApp::battle_stage_model() * basis;
                if let Some(mesh) = app.battle_stage_spun_mesh.and_then(|i| store.meshes.get(i)) {
                    // Copy A culls its back faces, the mirrored copy B
                    // draws both sides (`backdrop_copy_nclip`).
                    nclip_marks.push((
                        draws.len(),
                        legaia_engine_scene_host::scene_host::backdrop_copy_nclip(*second, copy_b),
                    ));
                    draws.push(SceneDraw { mesh, mvp, cue });
                }
                if let Some(mesh) = app
                    .battle_stage_spun_color_mesh
                    .and_then(|i| store.color_meshes.get(i))
                {
                    color_draws.push(ColorSceneDraw { mesh, mvp, cue });
                }
            }
        }
        // The camera the battle bodies' tint pass judges depth under
        // (`World::battle_actor_draw_plan`): the phase-scripted dome
        // camera this pass projects with in every battle.
        let battle_pose = in_battle.then(|| app.session.host.world.battle_cam_pose());
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
            if !app.session.host.world.actor_slot_drawn(i) {
                continue;
            }
            // The summon band's hide (`+0x21C = 0xFF` with the prim
            // word zeroed, `0x801E4B30..0x801E4B6C`): every party
            // seat and living monster is off screen while the
            // creature performs, restored at `0x36`. The zero word is
            // the hide, not the byte (`World::battle_body_hidden`).
            if in_battle && app.session.host.world.battle_body_hidden(i) {
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
            if legaia_engine_world_views::tile_board::is_tile_actor_slot(&app.session.host.world, i)
            {
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
            if app.session.host.world.actor_hidden_by_scene(i) {
                continue;
            }
            let mesh = posed_overrides
                .get(tmd_idx)
                .and_then(|o| o.as_ref())
                .or_else(|| store.meshes.get(tmd_idx));
            if let Some(mesh) = mesh {
                // Target-select cursor: while the command picker
                // points at an enemy row, the ported FUN_801DA6B4
                // (`engine-battle-vm::battle_action::target_cursor_highlight`)
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
                    let b = &actor.battle;
                    // The cursor's cue is the engine's
                    // (`battle_action::cursor_cue`), shared with the
                    // page's `play_battle_actor_cursor`.

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
                    //
                    // Tint word first, else the cursor pulse: the one
                    // precedence `BattleActorDrawPlan::body_cue` the
                    // page's `play_battle_actor_cursor` stages too.
                    if let Some((far, max_ir0)) =
                        legaia_engine_core::world::BattleActorDrawPlan::body_cue(
                            battle_plan.as_ref(),
                            b.render_flag,
                            app.session.host.world.clock.display_frames,
                        )
                    {
                        cue = Some(legaia_engine_render::DrawCue {
                            far,
                            near_z: -1.0,
                            far_z: 0.0,
                            max_ir0,
                        });
                    }
                }
                // The player's object-effect clip (`CC F8 C2`), off
                // the battle stage.
                if !in_battle
                    && app.session.host.world.player_actor_slot == Some(i as u8)
                    && let Some(c) = effect_clip(
                        legaia_engine_field::object_tint::ActorTintKey::Player,
                        &model,
                    )
                {
                    clip_marks.push((draws.len(), c));
                }
                // A body the defeat fade is sinking draws clipped at the
                // stage floor (its node's `+0x42`, object-effect row 0 as
                // battle setup seeds it) - `World::battle_actor_floor_clip`,
                // the page's `play_battle_actor_floor_clips` too.
                if in_battle && let Some(c) = app.session.host.world.battle_actor_floor_clip(i) {
                    clip_marks.push((
                        draws.len(),
                        legaia_engine_render::DrawClip {
                            m: c.m,
                            lo: c.lo,
                            hi: c.hi,
                        },
                    ));
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
}

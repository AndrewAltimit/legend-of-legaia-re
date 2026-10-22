//! The battle half of the scene draw lists: the twice-drawn backdrop shell
//! and the arts after-image ghosts.

use super::*;

impl<'m> DrawCtx<'_, 'm> {
    /// The battle stage backdrop (the dome pair) and its untextured half.
    pub(super) fn push_battle_backdrop(&self, out: &mut DrawLists<'m>) {
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
        // (`legaia_scene_formats::battle_backdrop::SecondCopy`). The
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
}

impl<'m> DrawCtx<'_, 'm> {
    /// The arts after-image ghosts, pushed past their bodies about the eye.
    pub(super) fn push_battle_ghosts(&self, out: &mut DrawLists<'m>, actor_cam: Mat4) {
        let store = &self.store;
        let battle_ghost_uploads = &store.posed.battle_ghost_uploads;
        let DrawLists {
            draws, nclip_marks, ..
        } = out;
        use legaia_engine_battle::battle_afterimage as ai;
        let eye = ai::camera_eye_from_vp(&actor_cam.to_cols_array());
        for g in battle_ghost_uploads {
            let push = match eye {
                Some(e) => {
                    let k = ai::ghost_eye_push_scale(
                        e,
                        g.ghost_pos,
                        g.body_pos,
                        ai::GHOST_EYE_PUSH_MARGIN,
                    );
                    let ev = Vec3::from(e);
                    Mat4::from_translation(ev)
                        * Mat4::from_scale(Vec3::splat(k))
                        * Mat4::from_translation(-ev)
                }
                None => Mat4::IDENTITY,
            };
            // Single-sided, as the live body: the walk's draw word carries no
            // double-sided bit (`BattleGhostDraw::nclip_mode`).
            nclip_marks.push((draws.len(), g.nclip));
            // The walk's `+0x78 = 0x1000` (`0x800493D8`): the DPCS output is
            // the far colour whatever the prim's baked colour, so the cue
            // is a constant full weight toward the ghost colour.
            draws.push(SceneDraw {
                mesh: &g.mesh,
                mvp: actor_cam * push * g.model,
                cue: Some(legaia_engine_render::DrawCue {
                    far: ai::ghost_far_colour(g.color),
                    near_z: -1.0,
                    far_z: 0.0,
                    max_ir0: 1.0,
                }),
            });
        }
    }
}

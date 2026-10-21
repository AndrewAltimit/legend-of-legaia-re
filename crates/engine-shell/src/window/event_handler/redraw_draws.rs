//! The redraw's scene draw lists: every textured and untextured 3D draw
//! the frame submits, by mode (minigame surfaces, the dance venue, the world
//! map, the battle stage, the field environment, the actors, the arts
//! ghosts).
//!
//! The lists borrow only the uploaded-mesh fields named in [`MeshStore`], so
//! the caller can keep mutating the rest of the window (the battle-intro
//! VRAM capture, the framebuffer grab, the screenshot sweep) while they live.

use super::super::*;
use super::redraw_passes::CutsceneCam;
use super::redraw_stage::PosedFrame;

#[path = "redraw_draws/actors.rs"]
mod actors;
#[path = "redraw_draws/battle.rs"]
mod battle;
#[path = "redraw_draws/field_env.rs"]
mod field_env;
#[path = "redraw_draws/world_map.rs"]
mod world_map;

// An op-`4C 81` draw tint (`+0x74` colour / `+0x78` blend) as the
// constant per-draw cue retail's actor draw stages (far colour +
// `IR0`, `FUN_8001ADA4` -> `FUN_80043390`). One shape for placed
// objects, NPCs and the player.
fn tint_draw_cue((colour, blend): (u32, u16)) -> Option<legaia_engine_render::DrawCue> {
    let (far, max_ir0) = legaia_engine_field::object_tint::tint_cue(colour, blend);
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
        legaia_engine_field::object_tint::ActorTintKey,
        legaia_engine_effects::object_effect::EffectClip,
        f32,
    )>,
}

impl FrameCues {
    /// The object-effect clip a draw keyed `key` at `model` carries.
    fn effect_clip(
        &self,
        key: legaia_engine_field::object_tint::ActorTintKey,
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
fn object_key(record: Option<usize>) -> Option<legaia_engine_field::object_tint::ActorTintKey> {
    record.map(|r| legaia_engine_field::object_tint::ActorTintKey::Object(r as u16))
}

/// Everything one frame's scene draw lists are built from.
pub(super) struct DrawCtx<'a, 'm> {
    pub app: &'a PlayWindowApp,
    pub store: MeshStore<'m>,
    pub r: &'a legaia_engine_render::Renderer,
    pub cam: Mat4,
    pub cutscene_cam: Option<CutsceneCam>,
    pub view_cells: &'a Option<legaia_engine_scene::field_view_window::ViewCells>,
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
            // retail - stage or no stage: the scale is the battle
            // mode's base matrix, not the backdrop's.
            let actor_cam = if in_battle {
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
}

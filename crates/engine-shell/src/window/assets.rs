//! Scene asset upload: [`PlayWindowApp::upload_assets`] and the stages it
//! runs, from the VRAM image through the scene's meshes and draw lists to
//! the actor bindings and the scene's derived lights.

use super::*;

#[path = "assets/actor_binding.rs"]
mod actor_binding;
#[path = "assets/npc_models.rs"]
mod npc_models;
#[path = "assets/scene_draws.rs"]
mod scene_draws;
#[path = "assets/scene_geometry.rs"]
mod scene_geometry;
#[path = "assets/ui_atlases.rs"]
mod ui_atlases;

use scene_draws::FieldDraws;
use scene_geometry::SceneGeometry;

/// Kingdom slot-4 inspection overlay (env-gated): the decoded
/// translation path of every animated part in the world-map actor
/// clip bank, NOT a mesh wireframe (slot 4 is an ANM container).
/// Built once at scene load; the per-frame world-map render merges
/// it into the overlay-lines buffer.
fn world_map_slot4_lines(res: &SceneResources) -> Option<LineGeometry> {
    if std::env::var_os("LEGAIA_WORLDMAP_SLOT4").is_some() {
        res.world_map_slot4.as_ref().and_then(|slot| {
            let (p, c, i) = world_map_slot4_line_geometry(slot);
            if i.is_empty() {
                None
            } else {
                log::info!(
                    "play-window: world-map slot-4 clip bank {} clips, {} translation-path segments",
                    slot.bodies.len(),
                    i.len() / 2
                );
                Some((p, c, i))
            }
        })
    } else {
        None
    }
}

/// Emitter samples for the scene's point lights (dynamic-lighting
/// enhancement): every *instance* of an emitter-carrying mesh -
/// candle flames, wall lights - contributes its world-space samples.
/// The static placement + terrain layers accumulate here; the MAN
/// NPC/prop layer (where most interior candle sconces actually live)
/// adds its own, and the combined set clusters into at most
/// `MAX_SCENE_LIGHTS` lights at the end of the upload.
fn static_layer_light_samples(
    geo: &SceneGeometry,
    draws: &FieldDraws,
) -> Vec<legaia_render_kernels::scene_lighting::EmitterSample> {
    let SceneGeometry {
        tmd_src_index,
        color_tmd_src_index,
        tmd_vram_emitters,
        tmd_color_emitters,
        ..
    } = geo;
    let (field_placement_draws, field_terrain_draws) = (&draws.placement.0, &draws.terrain.0);
    let (field_placement_color_draws, field_terrain_color_draws) =
        (&draws.placement_color.0, &draws.terrain_color.0);
    let mut light_samples: Vec<legaia_render_kernels::scene_lighting::EmitterSample> = Vec::new();
    for (mesh_idx, model) in field_placement_draws.iter().chain(field_terrain_draws) {
        if let Some(&src) = tmd_src_index.get(*mesh_idx)
            && let Some(em) = tmd_vram_emitters.get(src)
            && !em.is_empty()
        {
            light_samples.extend(legaia_render_kernels::scene_lighting::transform_samples(
                em, model,
            ));
        }
    }
    for (mesh_idx, model) in field_placement_color_draws
        .iter()
        .chain(field_terrain_color_draws)
    {
        if let Some(&src) = color_tmd_src_index.get(*mesh_idx)
            && let Some(em) = tmd_color_emitters.get(src)
            && !em.is_empty()
        {
            light_samples.extend(legaia_render_kernels::scene_lighting::transform_samples(
                em, model,
            ));
        }
    }
    light_samples
}

impl PlayWindowApp {
    /// Upload the pending scene's assets and bind them to the world. The
    /// stages run in this order; each later one reads what the earlier ones
    /// uploaded or resolved.
    pub(super) fn upload_assets(&mut self) {
        let Some(res) = self.scene_res.take() else {
            return;
        };
        // The entered scene's actors, bound to the world once per scene by
        // the step both hosts run (`scene_actors::bind_scene_actors`): the
        // lead's locomotion clips, the NPC clip cursors, the floor snap.
        // Ahead of the renderer gate, so a window with no renderer binds the
        // same world; the draw half below only reads the result.
        if std::mem::take(&mut self.scene_actors_pending) {
            self.adopt_scene_actor_binding();
        }
        // Pose source for the scene's animated actors + placed static objects.
        let scene_bundle = self.find_scene_anm_bundle();
        let Some(r) = self.win.renderer.as_ref() else {
            // No renderer yet (the window opens later, or the headless
            // driver never opens one): keep the resources for the upload,
            // but run the half of it that is world state rather than GPU
            // state now. The scripted VRAM effects step against
            // `cpu_vram_base` and are not presentation-only (a CLUT-cell
            // fade releases a halted context, the ambient move-VM tree draws
            // the world `rand()` stream), so a window with no base image ran
            // a different simulation from one with a renderer.
            self.cpu_vram_base = Some(res.vram.clone());
            if let Some(b) = self.scene_world_aabb(&res) {
                self.scene_aabb = b;
            }
            self.scene_res = Some(res);
            return;
        };

        // GPU uploads: the VRAM image, the font, then the scene's meshes
        // over shared accumulators.
        let vram_opt = r
            .upload_vram(&res.vram)
            .map_err(|e| log::error!("VRAM upload: {e:#}"))
            .ok();
        let font_opt = r
            .upload_font(&self.font)
            .map_err(|e| log::error!("font upload: {e:#}"))
            .ok();
        let mut geo = SceneGeometry::new(res.tmds.len(), self.draw_census.is_some());
        self.upload_env_meshes(r, &res, &mut geo);
        self.upload_posed_placement_meshes(r, &res, scene_bundle.as_ref(), &mut geo);
        self.upload_ground_heightfield(r, &mut geo);
        let world_map_slot4_lines = world_map_slot4_lines(&res);

        // Draw lists. Cross-draw coplanar offsets are computed over the
        // combined terrain + placed EnvDraw lists BEFORE the resolvers run,
        // so every resolver (textured / colour / posed bridges) finds its
        // draw's offset by identity lookup and the layers stay mutually
        // consistent.
        self.coplanar_env_offsets = self.compute_coplanar_env_offsets(&res);
        // Occlusion-fade visibility gate: the static scene's world-space
        // triangle set, ray-cast per field frame (eye -> player samples) so
        // the see-through fade arms only when the character is completely
        // hidden. Same static draw list as the coplanar pass above.
        self.field_occluders = self.build_field_occluders(&res);
        // Volumetric fog: where the ground is, so the bank lies on it and
        // spills off its rims (an enhancement; nothing reads it back).
        self.install_fog_ground(&res);
        let draws = self.resolve_field_draws(&res, &geo, scene_bundle.as_ref());
        let light_samples = static_layer_light_samples(&geo, &draws);
        let world_map = self.resolve_world_map_draws(&res, &geo);

        // Commit to the window.
        self.install_field_pack_tables(&res, &geo.tmd_src_index);
        // Keep a clean CPU copy of the scene VRAM so a battle can inject
        // monster textures into a throwaway clone and restore this on exit.
        self.cpu_vram_base = Some(res.vram.clone());
        if let Some(v) = vram_opt {
            self.uploaded_vram = Some(v);
        }
        if let Some(a) = font_opt {
            self.font_atlas = Some(a);
        }
        self.upload_ui_atlases();
        self.commit_scene_geometry(&res, geo, draws, world_map, world_map_slot4_lines);

        // Actors' meshes. The live mesh overrides and the player's rig mesh
        // (the lead's field form, or the model a `CC F8 50` re-staged the
        // player onto - `SceneHost::player_rig_mesh`) are resolved first.
        // What each actor is bound to is the world's, settled above.
        let live_npc_meshes = self.live_npc_mesh_overrides();
        let player_rig = self.session.host.player_rig_mesh();
        self.upload_player_rig(player_rig);
        let prop_light_sets = self.upload_field_npcs(&res, scene_bundle, &live_npc_meshes);
        self.install_scene_lights(&light_samples, prop_light_sets);
        // Kept for the next upload: a mid-scene model re-bind
        // (`rebind_live_npc_models`) and a renderer that re-opens re-upload
        // from it.
        self.scene_res = Some(res);
        log::info!(
            "play-window: {} meshes uploaded, VRAM {}",
            self.meshes.len(),
            if self.uploaded_vram.is_some() {
                "ready"
            } else {
                "failed"
            }
        );
    }
}

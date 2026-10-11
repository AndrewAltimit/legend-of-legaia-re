//! Resolving the scene's draw lists against the uploaded geometry, and
//! committing geometry and lists to the window.

use super::*;
use crate::window::field_render::{PlacedDrawList, TerrainDrawList};

/// The field draw lists [`PlayWindowApp::resolve_field_draws`] resolves:
/// each list on the textured bridge and again on the colour-mesh bridge.
pub(super) struct FieldDraws {
    pub(super) posed_props: Vec<PosedPropDraw>,
    pub(super) placement: PlacedDrawList,
    pub(super) placement_color: PlacedDrawList,
    pub(super) terrain: TerrainDrawList,
    pub(super) terrain_color: TerrainDrawList,
}

/// One world-map terrain stamp list: the draws, where the decoration run
/// starts, and each draw's record.
pub(super) type WorldMapDrawList = (Vec<(usize, Mat4)>, usize, Vec<Option<usize>>);

/// The world-map terrain stamps on both bridges.
pub(super) struct WorldMapDraws {
    pub(super) terrain: WorldMapDrawList,
    pub(super) terrain_color: WorldMapDrawList,
}

impl PlayWindowApp {
    /// Resolve the field static-geometry draws: each placed environment
    /// object and terrain tile -> its scene-pack mesh -> a world transform.
    /// Built here (not per-frame) because the placement table + pack are
    /// fixed for the scene; the field draw branch just replays the lists.
    pub(super) fn resolve_field_draws(
        &self,
        res: &SceneResources,
        geo: &SceneGeometry,
        scene_bundle: Option<&legaia_actor_formats::player_anm::PlayerAnmBundle>,
    ) -> FieldDraws {
        // Posed placed props (house doors, cupboards, the windmill): one draw
        // per placement + its live clip state, so each keeps its own cursor.
        // Resolved before the placement lists, which hand these props over.
        let posed_props = self.resolve_posed_props(res, &geo.posed_placement_meshes, scene_bundle);
        // Each resolver also hands back the floor-ladder rungs every draw's Y
        // came from, parallel to the draw list, so the live ladder can be
        // folded back in per frame (`FieldFloorWave` - the op-`0x4C` nibble-9
        // floor wave).
        let placement = self.resolve_field_placement_draws(
            res,
            &geo.tmd_src_index,
            &geo.posed_placement_meshes,
            true,
        );
        // Same resolver, but bridged through the colour-mesh list: the untextured
        // props' placement transforms map to `color_meshes` indices.
        let placement_color = self.resolve_field_placement_draws(
            res,
            &geo.color_tmd_src_index,
            &geo.posed_placement_meshes,
            false,
        );
        let terrain = self.resolve_field_terrain_draws(res, &geo.tmd_src_index);
        // Untextured ground tiles resolve through the colour-mesh bridge (the
        // textured bridge has no entry for them - they'd render as floor holes).
        let terrain_color = self.resolve_field_terrain_draws(res, &geo.color_tmd_src_index);
        log::info!(
            "play-window: {} field terrain draws (ground layer, +{} colour tiles)",
            terrain.0.len(),
            terrain_color.0.len()
        );
        FieldDraws {
            posed_props,
            placement,
            placement_color,
            terrain,
            terrain_color,
        }
    }

    /// The world-map terrain stamps, textured and then bridged through the
    /// colour-mesh list: the landmark pack's untextured F*/G* prims (hut
    /// roofs, colour-only landmarks) draw on the colour pipeline, as the
    /// field terrain's do.
    pub(super) fn resolve_world_map_draws(
        &self,
        res: &SceneResources,
        geo: &SceneGeometry,
    ) -> WorldMapDraws {
        let terrain = self.resolve_world_map_terrain_draws(res, &geo.tmd_src_index);
        let terrain_color = self.resolve_world_map_terrain_draws(res, &geo.color_tmd_src_index);
        WorldMapDraws {
            terrain,
            terrain_color,
        }
    }

    /// The scene-pack tables the field effect paths look meshes up through:
    /// the move-VM stager TMD list and the env-pack-slot -> uploaded-mesh
    /// bridge of the VDF morph substitution.
    pub(super) fn install_field_pack_tables(
        &mut self,
        res: &SceneResources,
        tmd_src_index: &[usize],
    ) {
        // Field move-VM stager scene-pack TMD list: `env_tmds` (res.tmds @ the
        // scene_asset_table bundle entry, scan order) = retail `DAT_8007C018[5..]`,
        // indexed by a field stager's relative `model_sel`. Kept as its own clone
        // because `res` is consumed below; the field-FX render path looks meshes up
        // here instead of the battle `global_tmd_pool`.
        let field_stager_tmds: Vec<(legaia_tmd::Tmd, Vec<u8>)> = self
            .session
            .host
            .scene
            .as_ref()
            .and_then(|scene| {
                legaia_engine_scene::scene_bundle::find_bundle(scene).map(|b| b.entry_idx())
            })
            .map(|bundle_entry| {
                res.tmds
                    .iter()
                    .filter(|t| t.entry_idx == bundle_entry)
                    .map(|t| (t.tmd.clone(), t.raw.clone()))
                    .collect()
            })
            .unwrap_or_default();
        self.field_stager_tmds = field_stager_tmds;
        // Env-pack-slot -> uploaded-mesh bridge for the VDF morph
        // substitution (`redraw`'s `refresh_field_morph_meshes`).
        self.field_pack_mesh_idx = self
            .session
            .host
            .scene
            .as_ref()
            .map(|scene| {
                legaia_engine_scene::field_env::env_pack_tmd_indices(scene, res)
                    .into_iter()
                    .map(|ti| tmd_src_index.iter().position(|&x| x == ti))
                    .collect()
            })
            .unwrap_or_default();
        self.field_morph_live.clear();
    }

    /// Commit the uploaded geometry and the resolved lists to the window,
    /// then derive what hangs off them: the pack-slot bridges, the floor
    /// wave, the lit meshes, the ocean animation and the scene box.
    pub(super) fn commit_scene_geometry(
        &mut self,
        res: &SceneResources,
        geo: SceneGeometry,
        draws: FieldDraws,
        world_map: WorldMapDraws,
        world_map_slot4_lines: Option<LineGeometry>,
    ) {
        let SceneGeometry {
            meshes,
            census,
            tmd_data,
            tmd_src_index,
            color_meshes,
            color_tmd_src_index,
            lit_sources,
            posed_tmds,
            world_map_hf,
            ground_src,
            ground_census,
            ..
        } = geo;
        let mut diag_census_out = (census, ground_census);
        let FieldDraws {
            posed_props,
            placement:
                (
                    field_placement_draws,
                    floor_placement,
                    placement_window_keys,
                    placement_cell_keys,
                    placement_records,
                    _,
                ),
            placement_color:
                (
                    field_placement_color_draws,
                    floor_placement_color,
                    placement_color_window_keys,
                    placement_color_cell_keys,
                    placement_color_records,
                    _,
                ),
            terrain: (field_terrain_draws, floor_terrain, terrain_cell_keys, terrain_facing),
            terrain_color:
                (
                    field_terrain_color_draws,
                    floor_terrain_color,
                    terrain_color_cell_keys,
                    terrain_color_facing,
                ),
        } = draws;
        let WorldMapDraws {
            terrain: (world_map_terrain_draws, deco_start, world_map_terrain_records),
            terrain_color:
                (world_map_terrain_color_draws, color_deco_start, world_map_terrain_color_records),
        } = world_map;
        self.meshes = meshes;
        if let Some(d) = self.draw_census.as_mut() {
            *d = super::DrawCensusDiag {
                meshes: std::mem::take(&mut diag_census_out.0),
                ground: diag_census_out.1.take(),
                ..Default::default()
            };
        }
        self.scene_tmd_data = tmd_data;
        self.field_terrain_draws = field_terrain_draws;
        self.field_lit.sources = lit_sources;
        self.field_lit.light = None;
        self.field_terrain_color_draws = field_terrain_color_draws;
        self.field_terrain_cell_keys = terrain_cell_keys;
        self.field_terrain_color_cell_keys = terrain_color_cell_keys;
        self.field_terrain_facing = terrain_facing;
        self.field_terrain_color_facing = terrain_color_facing;
        self.field_placement_draws = field_placement_draws;
        self.color_meshes = color_meshes;
        self.field_placement_color_draws = field_placement_color_draws;
        self.field_placement_window_keys = placement_window_keys;
        self.field_placement_color_window_keys = placement_color_window_keys;
        self.field_placement_cell_keys = placement_cell_keys;
        self.field_placement_color_cell_keys = placement_color_cell_keys;
        self.field_placement_records = placement_records;
        self.field_placement_color_records = placement_color_records;
        // Pack slot -> uploaded mesh, for the placed objects a motion
        // stream re-binds (`World::object_live_models`).
        let env_tmds = self
            .session
            .host
            .scene
            .as_ref()
            .map(|s| legaia_engine_scene::field_env::env_pack_tmd_indices(s, res))
            .unwrap_or_default();
        let bridge = |src: &[usize]| -> Vec<Option<usize>> {
            env_tmds
                .iter()
                .map(|&r| src.iter().position(|&s| s == r))
                .collect()
        };
        self.field_pack_meshes = bridge(&tmd_src_index);
        self.field_pack_color_meshes = bridge(&color_tmd_src_index);
        // The ladder those four lists were baked against, plus their per-draw
        // rungs: `handle_redraw` folds any later movement into the matrices.
        let floor_base = self.session.host.scene.as_ref().and_then(|s| {
            s.field_floor_height_lut(&self.session.host.index)
                .ok()
                .flatten()
        });
        self.field_floor_wave = field_render::FieldFloorWave::install(
            floor_base,
            floor_terrain,
            floor_terrain_color,
            floor_placement,
            floor_placement_color,
        );
        // The posed props own their own draws (one per placement, so each keeps
        // its own clip cursor) and their own live animation state.
        self.field_posed_tmds = posed_tmds;
        self.field_posed_props = posed_props;
        self.rebuild_field_lit_meshes();
        self.world_map_terrain_draws = world_map_terrain_draws;
        self.world_map_terrain_color_draws = world_map_terrain_color_draws;
        self.world_map_terrain_records = world_map_terrain_records;
        self.world_map_terrain_color_records = world_map_terrain_color_records;
        self.world_map_deco_start = (deco_start, color_deco_start);
        self.ground_heightfield = world_map_hf;
        self.ground_src = ground_src;
        self.ground_crop = None;
        self.world_map_slot4_lines = world_map_slot4_lines;
        // World-map ocean: recover the 13-frame CLUT animation for the kingdom
        // (the ocean texture + base CLUT are already uploaded by the slot-0 TIM
        // pass). `None` off the world map, so the per-tick advance self-gates.
        self.ocean_anim = self.resolve_ocean_anim();
        // The top-view debug camera's framing box. World-space, through the
        // shared `field_env::env_draws_world_aabb` kernel the browser play
        // page calls: the placement transform is what turns an env-pack mesh's
        // origin-centred extent into the box that actually contains the map.
        if let Some(b) = self.scene_world_aabb(res) {
            self.scene_aabb = b;
        }
    }
}

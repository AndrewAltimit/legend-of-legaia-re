//! Assembled **full-scene** exports: load a CDNAME field/town scene through
//! the engine's real scene loaders and surface everything the WebGL
//! assembled view needs - the environment mesh pack, the `.MAP` placement /
//! terrain-tile draws, the walk-ground heightfield, and the field VRAM.
//!
//! This is the browser twin of the play-window's static field layer: the
//! same [`legaia_engine_core::scene_resources::SceneResources`] build (field
//! VRAM pre-pass + LZS-packed env TMD scan), the same
//! [`legaia_engine_core::field_env`] pack vote + placement resolution, the
//! same floor-height-LUT world Y. A `scene_asset_table` entry viewed alone
//! shows one object-local mesh at the origin; this path shows the map those
//! meshes assemble into.

use super::*;
use legaia_engine_core::field_env::{self, EnvDraw};
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
};
use std::sync::Arc;

/// A fully-assembled field scene held by [`LegaiaViewer`] between
/// `set_scene_field` and the per-mesh accessors. Built by
/// [`build_field_scene`] (public so the disc-gated integration tests can
/// exercise the assembly without a browser canvas).
pub struct FieldScenePack {
    /// CDNAME label the scene was loaded as (status line).
    pub name: String,
    /// Engine scene resources: field-mode VRAM + every parsed scene TMD.
    pub res: SceneResources,
    /// Environment-pack subset of `res.tmds` (pack-index order) - the index
    /// space the placement records select from.
    pub env_tmds: Vec<usize>,
    /// Placed-object draws (`flags & 0x4`; buildings / props / landmarks).
    pub placements: Vec<EnvDraw>,
    /// Bulk terrain-tile draws (`CELL_VISIBLE`; ground / decor tiles).
    /// Empty for world-map scenes (their ground is the heightfield).
    pub terrain: Vec<EnvDraw>,
    /// Walk-ground heightfield surface (`None` when the scene has no
    /// resolvable `.MAP` floor grid / floor LUT).
    pub ground: Option<legaia_asset::field_objects::WalkHeightfield>,
    /// Currently-selected env-pack slot + its built mesh, cached so the
    /// positions/uvs/cba_tsb/indices accessors don't rebuild per call.
    pub cur: Option<(usize, legaia_tmd::mesh::VramMesh)>,
}

/// Assemble a CDNAME scene's full static map: field-mode
/// [`SceneResources`] (VRAM + env TMD pack) + the `.MAP` placement /
/// terrain-tile draws resolved through [`field_env`] + the walk-ground
/// heightfield. The engine-parity core of [`LegaiaViewer::set_scene_field`].
pub fn build_field_scene(index: &ProtIndex, name: &str) -> Result<FieldScenePack, String> {
    let scene = Scene::load(index, name).map_err(|e| format!("{e:#}"))?;

    // The shared blocks the retail field engine keeps resident across
    // scene transitions (player TMD + shared UI atlas) - included so the
    // VRAM matches the engine's field build; the env-pack vote filters
    // them out of the mesh selection.
    let mut shared_scenes: Vec<Scene> = Vec::new();
    for n in FIELD_SHARED_BLOCKS {
        if let Ok(s) = Scene::load(index, n) {
            shared_scenes.push(s);
        }
    }
    let shared_refs: Vec<&Scene> = shared_scenes.iter().collect();
    let is_world_map = legaia_engine_core::scene::is_world_map_scene(name);
    let kind = if is_world_map {
        SceneLoadKind::WorldMap
    } else {
        SceneLoadKind::Field
    };
    let (res, _stats) = SceneResources::build_targeted_with_options(
        &scene,
        &shared_refs,
        BuildOptions {
            kind,
            // Retail's field loader DMA-uploads every scene TIM; the
            // render-targeted subset drops ~75% of the env pack's prims.
            upload_all_tims: true,
        },
    )
    .map_err(|e| format!("{e:#}"))?;

    let env_tmds = field_env::env_pack_tmd_indices(&scene, &res);
    let floor_lut = scene.field_floor_height_lut(index).ok().flatten();
    // World-map scenes draw the sparse walk-frame landmarks; field/town
    // scenes draw the placed objects + the bulk terrain-tile layer
    // (mirrors the play-window's resolve_field_* / resolve_world_map_*
    // split in `engine-shell`).
    let (placement_records, terrain_records) = if is_world_map {
        (
            scene
                .walk_object_placements(index)
                .ok()
                .flatten()
                .unwrap_or_default(),
            Vec::new(),
        )
    } else {
        (
            scene
                .field_object_placements(index)
                .ok()
                .flatten()
                .unwrap_or_default(),
            scene
                .field_terrain_tiles(index)
                .ok()
                .flatten()
                .unwrap_or_default(),
        )
    };
    let (placements, _) = field_env::resolve_env_draws(&env_tmds, &placement_records, floor_lut);
    let (terrain, _) = field_env::resolve_env_draws(&env_tmds, &terrain_records, floor_lut);
    let ground = scene
        .walk_heightfield(index)
        .ok()
        .flatten()
        .filter(|h| !h.indices.is_empty());

    Ok(FieldScenePack {
        name: name.to_string(),
        res,
        env_tmds,
        placements,
        terrain,
        ground,
        cur: None,
    })
}

impl LegaiaViewer {
    /// Build (and cache) the engine-core [`ProtIndex`] over the loaded disc.
    /// After `load_disc`, `self.disc` holds the extracted PROT.DAT bytes and
    /// `self.cdname_text` the CDNAME.TXT captured from the full image (raw
    /// PROT.DAT loads have no CDNAME - scene names then can't resolve and
    /// `set_scene_field` errors).
    fn ensure_prot_index(&mut self) -> Result<Arc<ProtIndex>, String> {
        if let Some(ix) = &self.prot_index {
            return Ok(ix.clone());
        }
        let prot_bytes = if crate::disc::is_mode2_2352_disc(&self.disc) {
            extract_prot_dat(&self.disc)
                .ok_or_else(|| "PROT.DAT not found in disc image".to_string())?
        } else {
            self.disc.clone()
        };
        let ix = ProtIndex::from_bytes(prot_bytes, self.cdname_text.as_deref())
            .map_err(|e| format!("PROT index: {e:#}"))?;
        let ix = Arc::new(ix);
        self.prot_index = Some(ix.clone());
        Ok(ix)
    }
}

#[wasm_bindgen]
impl LegaiaViewer {
    /// Load a CDNAME scene (e.g. `"town01"`, `"korb3"`) as an **assembled
    /// full map**: field-mode VRAM + the environment mesh pack + the `.MAP`
    /// placement / terrain draws + the walk-ground heightfield. Returns the
    /// environment pack's TMD count (the `field_scene_mesh` slot space).
    ///
    /// Requires a full disc image (CDNAME.TXT resolves the scene block).
    /// World-map scenes (`map01..03`) load their walk-frame landmark
    /// placements; every other field scene loads the placed-object +
    /// terrain-tile layers.
    pub fn set_scene_field(&mut self, name: &str) -> Result<u32, JsValue> {
        self.field_scene = None;
        let index = self
            .ensure_prot_index()
            .map_err(|e| JsValue::from_str(&format!("set_scene_field({name}): {e}")))?;
        let pack = build_field_scene(&index, name)
            .map_err(|e| JsValue::from_str(&format!("set_scene_field({name}): {e}")))?;
        let count = pack.env_tmds.len() as u32;
        console_log(&format!(
            "field scene {name}: {} env meshes, {} placements, {} terrain tiles, {} ground quads",
            count,
            pack.placements.len(),
            pack.terrain.len(),
            pack.ground.as_ref().map(|h| h.quad_count()).unwrap_or(0),
        ));
        self.field_scene = Some(pack);
        Ok(count)
    }

    /// Number of TMDs in the loaded field scene's environment pack. 0 when
    /// no field scene is loaded.
    pub fn field_scene_pack_count(&self) -> u32 {
        self.field_scene
            .as_ref()
            .map(|f| f.env_tmds.len() as u32)
            .unwrap_or(0)
    }

    /// One-line JSON status for the UI:
    /// `{"name", "pack_count", "placements", "terrain", "ground_quads"}`.
    pub fn field_scene_status_json(&self) -> String {
        match &self.field_scene {
            Some(f) => format!(
                r#"{{"name":"{}","pack_count":{},"placements":{},"terrain":{},"ground_quads":{}}}"#,
                f.name.replace('"', ""),
                f.env_tmds.len(),
                f.placements.len(),
                f.terrain.len(),
                f.ground.as_ref().map(|h| h.quad_count()).unwrap_or(0),
            ),
            None => "null".to_string(),
        }
    }

    /// Select the active environment-pack slot and build its mesh (textured
    /// prims whose pages/CLUTs are resident in the field VRAM; matches the
    /// engine's per-prim filter). Returns the slot, or an error when out of
    /// range. Subsequent `field_scene_mesh_*` calls read the built mesh.
    pub fn field_scene_mesh(&mut self, slot: u32) -> Result<u32, JsValue> {
        let f = self
            .field_scene
            .as_mut()
            .ok_or_else(|| JsValue::from_str("field_scene_mesh: no field scene loaded"))?;
        let s = slot as usize;
        let Some(&res_idx) = f.env_tmds.get(s) else {
            return Err(JsValue::from_str(&format!(
                "field_scene_mesh: slot {s} >= count {}",
                f.env_tmds.len()
            )));
        };
        if f.cur.as_ref().map(|(cs, _)| *cs) != Some(s) {
            let mesh = f.res.tmds[res_idx].build_filtered_vram_mesh(&f.res.vram);
            f.cur = Some((s, mesh));
        }
        Ok(slot)
    }

    pub fn field_scene_mesh_positions(&self) -> Vec<f32> {
        let Some((_, mesh)) = self.field_scene.as_ref().and_then(|f| f.cur.as_ref()) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.positions.len() * 3);
        for p in &mesh.positions {
            out.extend_from_slice(p);
        }
        out
    }

    pub fn field_scene_mesh_uvs(&self) -> Vec<u8> {
        let Some((_, mesh)) = self.field_scene.as_ref().and_then(|f| f.cur.as_ref()) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.uvs.len() * 2);
        for uv in &mesh.uvs {
            out.extend_from_slice(uv);
        }
        out
    }

    pub fn field_scene_mesh_cba_tsb(&self) -> Vec<u16> {
        let Some((_, mesh)) = self.field_scene.as_ref().and_then(|f| f.cur.as_ref()) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.cba_tsb.len() * 2);
        for ct in &mesh.cba_tsb {
            out.extend_from_slice(ct);
        }
        out
    }

    pub fn field_scene_mesh_indices(&self) -> Vec<u32> {
        self.field_scene
            .as_ref()
            .and_then(|f| f.cur.as_ref())
            .map(|(_, m)| m.indices.clone())
            .unwrap_or_default()
    }

    /// Field-mode VRAM bytes (1 MB) shared by every env-pack mesh + the
    /// ground heightfield. Empty when no field scene is loaded.
    pub fn field_scene_vram_bytes(&self) -> Vec<u8> {
        self.field_scene
            .as_ref()
            .map(|f| f.res.vram.as_bytes().to_vec())
            .unwrap_or_default()
    }

    /// Per-placement env-pack slot, one `u32` per placed object. Feed each
    /// into [`Self::field_scene_mesh`] and draw at the matching
    /// [`Self::field_scene_placement_positions`] entry.
    pub fn field_scene_placement_slots(&self) -> Vec<u32> {
        self.field_scene
            .as_ref()
            .map(|f| f.placements.iter().map(|d| d.env_slot as u32).collect())
            .unwrap_or_default()
    }

    /// Per-placement world positions `[x, y, z, ...]` (flattened), same
    /// pre-Y-flip world frame as the ground heightfield (draw with the shared
    /// `(1, -1, 1)` model flip at scale 1).
    pub fn field_scene_placement_positions(&self) -> Vec<f32> {
        let Some(f) = self.field_scene.as_ref() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(f.placements.len() * 3);
        for d in &f.placements {
            out.push(d.world_x as f32);
            out.push(d.world_y as f32);
            out.push(d.world_z as f32);
        }
        out
    }

    /// Per-terrain-tile env-pack slot (the dense `CELL_VISIBLE` decor layer).
    pub fn field_scene_terrain_slots(&self) -> Vec<u32> {
        self.field_scene
            .as_ref()
            .map(|f| f.terrain.iter().map(|d| d.env_slot as u32).collect())
            .unwrap_or_default()
    }

    /// Per-terrain-tile world positions `[x, y, z, ...]` (flattened).
    pub fn field_scene_terrain_positions(&self) -> Vec<f32> {
        let Some(f) = self.field_scene.as_ref() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(f.terrain.len() * 3);
        for d in &f.terrain {
            out.push(d.world_x as f32);
            out.push(d.world_y as f32);
            out.push(d.world_z as f32);
        }
        out
    }

    /// Ground-heightfield accessors (same layout as the kingdom
    /// `walk_ground_*` family; empty when the scene has no resolvable floor
    /// grid).
    pub fn field_scene_ground_positions(&self) -> Vec<f32> {
        let Some(hf) = self.field_scene.as_ref().and_then(|f| f.ground.as_ref()) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(hf.positions.len() * 3);
        for p in &hf.positions {
            out.extend_from_slice(p);
        }
        out
    }

    pub fn field_scene_ground_uvs(&self) -> Vec<u8> {
        let Some(hf) = self.field_scene.as_ref().and_then(|f| f.ground.as_ref()) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(hf.uvs.len() * 2);
        for uv in &hf.uvs {
            out.extend_from_slice(uv);
        }
        out
    }

    pub fn field_scene_ground_cba_tsb(&self) -> Vec<u16> {
        let Some(hf) = self.field_scene.as_ref().and_then(|f| f.ground.as_ref()) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(hf.cba_tsb.len() * 2);
        for ct in &hf.cba_tsb {
            out.extend_from_slice(ct);
        }
        out
    }

    pub fn field_scene_ground_indices(&self) -> Vec<u32> {
        self.field_scene
            .as_ref()
            .and_then(|f| f.ground.as_ref())
            .map(|hf| hf.indices.clone())
            .unwrap_or_default()
    }

    pub fn field_scene_ground_quad_count(&self) -> u32 {
        self.field_scene
            .as_ref()
            .and_then(|f| f.ground.as_ref())
            .map(|hf| hf.quad_count() as u32)
            .unwrap_or(0)
    }
}

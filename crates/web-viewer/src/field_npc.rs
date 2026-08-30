//! Field-**NPC** catalog: every actor a scene's MAN places, with the mesh it
//! is drawn from.
//!
//! An NPC is not a separate asset class. It is a TMD in the scene's own TMD
//! pool selected by a **MAN partition-1 placement record**: the record's model
//! byte indexes the scene TMD list, its anim byte names a record in the scene's
//! ANM bundle, and its tile bytes give the spawn. So the catalog is the
//! placement list, resolved against the [`FieldScenePack`] the field-scene
//! loader already builds (see `docs/subsystems/script-vm.md` § placement header
//! and `docs/formats/anm.md` § per-scene bundle).
//!
//! This is the browser twin of the play-window's field-NPC pass (`window/
//! assets.rs`): the same `classify_placements` walk, the same off-map skip, the
//! same `res.tmds[model_index]` resolution.
//!
//! **The pose is not cosmetic.** A character-shaped TMD ships its vertices in
//! *object-local* space - each object's coordinates are relative to its own
//! joint. Drawn raw, the parts pile up on the origin. The assembled figure is
//! `v_world = R_bone . v_object_local + T_bone` with `(R, T)` from frame 0 of
//! the placement's ANM record. The mesh accessors here therefore ship
//! per-vertex `object_ids` alongside the geometry, and the page composes the
//! pose from the existing `player_anm_record_pose_frames` accessor - the same
//! animator the characters page runs.

use super::*;
use crate::field_scene::FieldScenePack;
use legaia_engine_core::scene::ProtIndex;

/// One catalogued placement - the shared kernel's record
/// ([`legaia_engine_core::npc_catalog::NpcEntry`]), re-exported so the
/// viewer's accessors keep their old path.
pub use legaia_engine_core::npc_catalog::NpcEntry;

/// The NPC catalog for one loaded field scene. Built by
/// [`LegaiaViewer::set_scene_npcs`]; the meshes resolve against the
/// [`FieldScenePack`] in `self.field_scene` (same VRAM, same TMD pool).
pub struct FieldNpcPack {
    /// CDNAME label the catalog was built for.
    pub scene: String,
    /// Renderable placements, in MAN partition-1 order.
    pub entries: Vec<NpcEntry>,
    /// PROT entry index of the scene's ANM bundle, for
    /// `player_anm_record_pose_frames`. `None` when the scene ships no bundle
    /// (its actors then have no clip and draw in TMD-local rest).
    pub anm_prot: Option<u32>,
    /// Party / savepoint placements (`model_index >= 0xF0`), which draw from
    /// the global head pool + the PROT 0874 locomotion bundle rather than the
    /// scene's - excluded from the catalog (the characters page owns them) but
    /// counted so the page can say so.
    pub special_count: u32,
    /// Multi-object actors the scene gives no way to assemble: their TMD has
    /// several objects (so its vertices are object-local and need a bone pose)
    /// but the placement names no clip, or the scene ships no ANM bundle at all
    /// (Mt. Rikuroa's story actors are the case that exists). Drawn raw they
    /// would be a pile of parts on the origin, so they're left out - and
    /// counted, so the page can say how many rather than silently hide them.
    pub unposable_count: u32,
    /// Currently-built mesh, keyed by catalog position:
    /// `(catalog_idx, mesh, object_ids, flat_rgba)`.
    #[allow(clippy::type_complexity)]
    pub cur: Option<(usize, legaia_tmd::mesh::VramMesh, Vec<u32>, Vec<u8>)>,
}

impl LegaiaViewer {
    /// Resolve a catalog entry's TMD out of the loaded scene's TMD pool.
    fn npc_tmd(&self, catalog_idx: usize) -> Option<(legaia_tmd::Tmd, Vec<u8>)> {
        let npcs = self.field_npcs.as_ref()?;
        let f = self.field_scene.as_ref()?;
        let e = npcs.entries.get(catalog_idx)?;
        let t = f.res.tmds.get(e.placement.model_index as usize)?;
        Some((t.tmd.clone(), t.raw.clone()))
    }
}

/// Catalog every NPC / actor the scene's MAN places, resolved against an
/// already-built [`FieldScenePack`] (its `res.tmds` is the model-byte index
/// space, its VRAM is what the meshes sample). The engine-parity core of
/// [`LegaiaViewer::set_scene_npcs`] - public so the disc-gated integration
/// test can exercise the catalog without a browser canvas.
pub fn build_npc_catalog(
    index: &ProtIndex,
    name: &str,
    pack: &FieldScenePack,
) -> Result<FieldNpcPack, String> {
    build_npc_catalog_res(index, name, &pack.res)
}

/// [`build_npc_catalog`] against an already-built
/// [`SceneResources`](legaia_engine_core::scene_resources::SceneResources) - the
/// form the play page uses, whose resources come from the running
/// [`SceneHost`](legaia_engine_core::scene::SceneHost) rather than a viewer-side
/// scene build. `res.tmds` is the model-byte index space either way.
pub fn build_npc_catalog_res(
    index: &ProtIndex,
    name: &str,
    res: &legaia_engine_core::scene_resources::SceneResources,
) -> Result<FieldNpcPack, String> {
    build_npc_catalog_impl(index, name, res, None)
}

/// The **play page's** catalog: every placement the native play-window draws.
///
/// Differs from [`build_npc_catalog_res`] (the NPC browser page's curated
/// list) in two ways, both matching the play-window's field-NPC pass
/// (`engine-shell` `window/assets.rs`):
///
/// - **global-pool specials are included**: a `model_index >= 0xF0` placement
///   (save point / party head) resolves against `global_pool[model - 0xF0]` -
///   the world's PROT 0874 §0 pool that `enter_field_scene` seeds - and its
///   clip against the locomotion bundle. Skipping them left the save crystal
///   (and story party members) missing from the browser scene.
/// - **clipless multi-object actors are included**: retail draw kind 5 draws
///   every TMD object with the actor's single transform (raw object-local
///   vertices), and the native window does the same, so the play page must
///   too rather than withholding them.
pub fn build_npc_catalog_play(
    index: &ProtIndex,
    name: &str,
    res: &legaia_engine_core::scene_resources::SceneResources,
    global_pool: &[Option<std::sync::Arc<legaia_engine_core::world::GlobalTmd>>],
) -> Result<FieldNpcPack, String> {
    build_npc_catalog_impl(index, name, res, Some(global_pool))
}

/// Shared walk behind the two catalog builders - delegates to the kernel
/// (`engine-core::npc_catalog::catalog_scene_npcs`) and wraps the result
/// with the viewer's mesh cache.
fn build_npc_catalog_impl(
    index: &ProtIndex,
    name: &str,
    res: &legaia_engine_core::scene_resources::SceneResources,
    play_pool: Option<&[Option<std::sync::Arc<legaia_engine_core::world::GlobalTmd>>]>,
) -> Result<FieldNpcPack, String> {
    let legaia_engine_core::npc_catalog::NpcCatalog {
        scene,
        entries,
        anm_prot,
        special_count,
        unposable_count,
    } = legaia_engine_core::npc_catalog::catalog_scene_npcs(index, name, res, play_pool)?;
    Ok(FieldNpcPack {
        scene,
        entries,
        anm_prot,
        special_count,
        unposable_count,
        cur: None,
    })
}

#[wasm_bindgen]
impl LegaiaViewer {
    /// Load a CDNAME scene and catalog every NPC / actor its MAN places.
    /// Loads the field scene first when it isn't already resident (so
    /// `field_scene_vram_bytes` is the VRAM these meshes sample). Returns the
    /// number of catalogued placements.
    pub fn set_scene_npcs(&mut self, name: &str) -> Result<u32, JsValue> {
        self.field_npcs = None;
        if self.field_scene.as_ref().map(|f| f.name.as_str()) != Some(name) {
            self.set_scene_field(name)?;
        }
        let index = self
            .ensure_prot_index()
            .map_err(|e| JsValue::from_str(&format!("set_scene_npcs({name}): {e}")))?;
        let pack = self
            .field_scene
            .as_ref()
            .ok_or_else(|| JsValue::from_str("set_scene_npcs: field scene missing"))?;
        let npcs = build_npc_catalog(&index, name, pack)
            .map_err(|e| JsValue::from_str(&format!("set_scene_npcs({name}): {e}")))?;
        console_log(&format!(
            "npc catalog {name}: {} actors ({} party/savepoint, {} unposable), anm bundle {:?}",
            npcs.entries.len(),
            npcs.special_count,
            npcs.unposable_count,
            npcs.anm_prot,
        ));
        let count = npcs.entries.len() as u32;
        self.field_npcs = Some(npcs);
        Ok(count)
    }

    /// The loaded scene's NPC catalog as JSON. Shape:
    /// ```text
    /// {
    ///   "scene": "town01",
    ///   "anm_prot": 4,            // null when the scene ships no ANM bundle
    ///   "special_count": 3,       // party / savepoint heads (not listed)
    ///   "unposable_count": 0,     // multi-object actors with no pose source
    ///   "npcs": [
    ///     { "i": 0,               // catalog index -> field_npc_mesh(i)
    ///       "slot": 7,            // MAN partition-1 record index
    ///       "model": 42,          // scene TMD-pool index (the mesh identity)
    ///       "anim": 9,            // ANM record index + 1; 0 = no clip
    ///       "nobj": 12,           // TMD object count
    ///       "kind": "talk",       // talk | door | prop
    ///       "target_map": null,
    ///       "dialog": "Hey, Vahn!",
    ///       "conditional": false, // true = script-gated spawn (parked off-map)
    ///       "x": 1088, "z": 2624  // spawn, world units
    ///     }, ...
    ///   ]
    /// }
    /// ```
    /// `null` when no catalog is loaded.
    pub fn field_npc_catalog_json(&self) -> String {
        let Some(n) = self.field_npcs.as_ref() else {
            return "null".to_string();
        };
        let npcs: Vec<serde_json::Value> = n
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                serde_json::json!({
                    "i": i,
                    "slot": e.placement.index,
                    "model": e.placement.model_index,
                    "anim": e.placement.anim_id,
                    "nobj": e.nobj,
                    "kind": e.kind,
                    "target_map": e.target_map,
                    "dialog": e.dialog,
                    "conditional": e.conditional,
                    "x": e.placement.world_x,
                    "z": e.placement.world_z,
                })
            })
            .collect();
        serde_json::json!({
            "scene": n.scene,
            "anm_prot": n.anm_prot,
            "special_count": n.special_count,
            "unposable_count": n.unposable_count,
            "npcs": npcs,
        })
        .to_string()
    }

    /// Build (and cache) one catalogued NPC's mesh. The **field-hybrid** build:
    /// textured prims that sample the scene VRAM plus the untextured
    /// flat/gouraud prims that carry per-vertex RGB, in one vertex stream with
    /// parallel per-vertex object ids - so the page can both render the
    /// colour-only body parts and compose the ANM pose. Returns the catalog
    /// index.
    pub fn field_npc_mesh(&mut self, catalog_idx: u32) -> Result<u32, JsValue> {
        let i = catalog_idx as usize;
        if self
            .field_npcs
            .as_ref()
            .and_then(|n| n.cur.as_ref())
            .map(|c| c.0)
            == Some(i)
        {
            return Ok(catalog_idx);
        }
        let (tmd, raw) = self
            .npc_tmd(i)
            .ok_or_else(|| JsValue::from_str(&format!("field_npc_mesh: no catalog entry {i}")))?;
        let (mesh, object_ids, shading) =
            legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&tmd, &raw);
        // Flag rides in the alpha byte (255 = textured, sample VRAM and
        // modulate by the RGB; 0 = fill with the RGB) - the
        // `u_use_flat_colors` / `a_flat_rgba` convention the shared renderer
        // already implements.
        let flat = crate::packet_color::hybrid(&mesh, &shading);
        let n = self
            .field_npcs
            .as_mut()
            .ok_or_else(|| JsValue::from_str("field_npc_mesh: no catalog loaded"))?;
        n.cur = Some((i, mesh, object_ids, flat));
        Ok(catalog_idx)
    }

    fn npc_cur(&self) -> Option<&(usize, legaia_tmd::mesh::VramMesh, Vec<u32>, Vec<u8>)> {
        self.field_npcs.as_ref()?.cur.as_ref()
    }

    pub fn field_npc_mesh_positions(&self) -> Vec<f32> {
        let Some((_, m, _, _)) = self.npc_cur() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(m.positions.len() * 3);
        for p in &m.positions {
            out.extend_from_slice(p);
        }
        out
    }

    pub fn field_npc_mesh_uvs(&self) -> Vec<u8> {
        let Some((_, m, _, _)) = self.npc_cur() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(m.uvs.len() * 2);
        for uv in &m.uvs {
            out.extend_from_slice(uv);
        }
        out
    }

    pub fn field_npc_mesh_cba_tsb(&self) -> Vec<u16> {
        let Some((_, m, _, _)) = self.npc_cur() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(m.cba_tsb.len() * 2);
        for ct in &m.cba_tsb {
            out.extend_from_slice(ct);
        }
        out
    }

    pub fn field_npc_mesh_indices(&self) -> Vec<u32> {
        self.npc_cur()
            .map(|(_, m, _, _)| m.indices.clone())
            .unwrap_or_default()
    }

    /// Per-vertex TMD object index, parallel to the positions - the bone each
    /// vertex belongs to. The page's animator keys the per-frame
    /// `R . v + T` on this.
    pub fn field_npc_mesh_object_ids(&self) -> Vec<u32> {
        self.npc_cur()
            .map(|(_, _, o, _)| o.clone())
            .unwrap_or_default()
    }

    /// Per-vertex `[r, g, b, textured_flag]` for the hybrid render.
    pub fn field_npc_mesh_flat_rgba(&self) -> Vec<u8> {
        self.npc_cur()
            .map(|(_, _, _, f)| f.clone())
            .unwrap_or_default()
    }

    /// Bounding sphere `[cx, cy, cz, r]` of the built mesh, for camera framing.
    pub fn field_npc_mesh_bounds(&self) -> Vec<f32> {
        let Some((_, m, _, _)) = self.npc_cur() else {
            return vec![0.0; 4];
        };
        centroid_bounds(&m.positions)
    }
}

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
use legaia_asset::man_section::ActorPlacement;
use legaia_engine_core::man_field_scripts::{PlacementKind, classify_placements};
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::FIELD_OFFMAP_HIDE_XZ;

/// One catalogued placement: the MAN record plus what its script implies.
pub struct NpcEntry {
    pub placement: ActorPlacement,
    /// `"talk"` (carries inline dialog / an interact op), `"door"` (warps to
    /// another scene), or `"prop"` (decorative / script-only).
    pub kind: &'static str,
    /// Field-VM map id for a `door`.
    pub target_map: Option<u8>,
    /// First line of the actor's inline dialog block, when it has one - the
    /// only human-readable label retail gives an NPC.
    pub dialog: Option<String>,
    /// Object count of the resolved TMD (the mesh's bone count ceiling).
    pub nobj: u32,
    /// Parked at the off-map hide box: a **conditional spawn** the scene only
    /// places once a script says so (a story NPC who isn't in town yet). Its
    /// model and clip are fully resolvable - retail just isn't drawing it at
    /// scene load - so the catalog lists it, flagged.
    pub conditional: bool,
    /// `model_index >= 0xF0`: a **global-pool special** (party head / save
    /// point). Its mesh comes from the world's global TMD pool (slot
    /// `model_index - 0xF0`) and its clip from the PROT 0874 locomotion
    /// bundle, not the scene's. Only the play catalog
    /// ([`build_npc_catalog_play`]) lists these.
    pub special: bool,
}

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

/// Decode the first line of an inline dialog block into a display label.
/// Glyph bytes are ASCII-compatible from `0x20`, so the printable run is the
/// text; control bytes (line breaks, the `0x1F` segment lead) end the line.
fn dialog_label(inline: &[u8]) -> Option<String> {
    let segs = legaia_engine_core::dialog::decode_inline_segments(inline);
    let first = segs.into_iter().next()?;
    let line: String = first
        .iter()
        .take_while(|&&b| b != 0x00)
        .filter(|&&b| (0x20..=0x7E).contains(&b))
        .map(|&b| b as char)
        .collect();
    let line = line.trim();
    if line.len() < 2 {
        return None;
    }
    Some(line.chars().take(64).collect())
}

/// Locate the scene's ANM bundle the way the play-window does: the type-0x05
/// section of one of the scene's PROT slots. The descriptor-count seed varies
/// per scene (town01 resolves at 3; the prologue scenes only at >= 5), so try
/// the spread and take the first hit.
fn scene_anm_prot(scene: &Scene) -> Option<u32> {
    scene.entries.iter().find_map(|e| {
        let found = [3usize, 5, 6, 7]
            .into_iter()
            .any(|desc| !legaia_asset::player_anm::find_in_entry(&e.bytes, desc).is_empty());
        found.then_some(e.idx)
    })
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

/// Shared walk behind the two catalog builders. `play_pool` is `Some` for the
/// play-page build (specials resolve against it, clipless multi-object actors
/// stay in), `None` for the NPC browser page's curated build.
fn build_npc_catalog_impl(
    index: &ProtIndex,
    name: &str,
    res: &legaia_engine_core::scene_resources::SceneResources,
    play_pool: Option<&[Option<std::sync::Arc<legaia_engine_core::world::GlobalTmd>>]>,
) -> Result<FieldNpcPack, String> {
    let scene = Scene::load(index, name).map_err(|e| format!("{e:#}"))?;
    let man = scene
        .field_man_payload(index)
        .map_err(|e| format!("MAN: {e:#}"))?
        .ok_or_else(|| format!("{name}: scene has no MAN"))?;
    let mf = legaia_asset::man_section::parse(&man).map_err(|e| format!("MAN parse: {e:#}"))?;

    let anm_prot = scene_anm_prot(&scene);
    let mut entries = Vec::new();
    let mut special_count = 0u32;
    let mut unposable_count = 0u32;
    for (p, kind) in classify_placements(&mf, &man) {
        let nobj = if p.special_model {
            // Party / savepoint heads come from the global pool, not the
            // scene's. The browser NPC page routes them to the characters
            // page; the play page draws them like the native window does.
            let Some(pool) = play_pool else {
                special_count += 1;
                continue;
            };
            let Some(g) = pool
                .get((p.model_index - 0xF0) as usize)
                .and_then(|s| s.as_ref())
            else {
                continue; // no pool mesh - the native window skips it too
            };
            special_count += 1;
            g.tmd.objects.len() as u32
        } else {
            let Some(t) = res.tmds.get(p.model_index as usize) else {
                continue;
            };
            t.tmd.objects.len() as u32
        };
        // A multi-object TMD's vertices are object-local: without a bone pose
        // it draws as a pile of parts on the origin. The curated NPC page
        // withholds those; the play page keeps them (retail draw kind 5 draws
        // them raw, and so does the native play-window).
        if !p.special_model && nobj > 1 && (p.anim_id == 0 || anm_prot.is_none()) {
            unposable_count += 1;
            if play_pool.is_none() {
                continue;
            }
        }
        let (label, target_map, dialog) = match &kind {
            PlacementKind::Portal { target_map } => ("door", Some(*target_map), None),
            PlacementKind::Npc { dialog_inline, .. } => (
                "talk",
                None,
                dialog_inline.as_deref().and_then(dialog_label),
            ),
            PlacementKind::Plain => ("prop", None, None),
        };
        // The off-map hide box marks a spawn retail withholds until a script
        // places it - the actor is real and fully resolvable, so the catalog
        // lists it with a flag rather than dropping it the way the field
        // renderer does.
        let conditional = p.world_x == FIELD_OFFMAP_HIDE_XZ && p.world_z == FIELD_OFFMAP_HIDE_XZ;
        let special = p.special_model;
        entries.push(NpcEntry {
            nobj,
            placement: p,
            kind: label,
            target_map,
            dialog,
            conditional,
            special,
        });
    }

    Ok(FieldNpcPack {
        scene: name.to_string(),
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
        // Flag rides in the alpha byte (255 = textured, sample VRAM; 0 = use
        // the vertex colour) - the `u_use_flat_colors` / `a_flat_rgba`
        // convention the shared renderer already implements.
        let mut flat = Vec::with_capacity(shading.colors.len() * 4);
        for (c, &t) in shading.colors.iter().zip(shading.textured.iter()) {
            flat.extend_from_slice(&[c[0], c[1], c[2], if t != 0 { 255 } else { 0 }]);
        }
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

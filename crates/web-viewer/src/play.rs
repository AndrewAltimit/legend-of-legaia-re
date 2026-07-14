//! Browser **play** surface: the render half of [`crate::runtime::LegaiaRuntime`].
//!
//! [`crate::runtime`] owns the simulation (a real
//! [`legaia_engine_core::scene::SceneHost`]: field VM, locomotion + collision,
//! NPC motion, dialogue). This module is what the page draws: the assembled
//! static map, the posed player, and the scene's posed NPCs - all resolved
//! against the **same** [`legaia_engine_core::scene_resources::SceneResources`]
//! the host already built at `enter_field_scene`, so nothing is decoded twice
//! and the picture is of the world the engine is actually simulating.
//!
//! The pieces are the ones the other pages already use:
//!
//! - **Map** - `field_env` pack vote + `.MAP` placement / terrain-tile
//!   resolution + the walk-ground heightfield (the browser twin of the
//!   play-window's static field layer; [`crate::field_scene`] does the same for
//!   the static viewer, off its own resource build).
//! - **NPCs** - the MAN partition-1 placement catalog ([`crate::field_npc`]),
//!   drawn at the world's **live** NPC positions / headings so an NPC walking
//!   its authored route walks on screen.
//! - **Player** - the lead's field mesh out of the global TMD pool (PROT 0874
//!   §0), posed each frame from the world's live `pose_frame` (the idle / walk
//!   locomotion clips, PROT 0874 §1).
//!
//! Character meshes ship their vertices in **object-local** space, so every
//! actor draw is `v_world = R_bone . v_object_local + T_bone`, composed here in
//! Rust off the engine's own pose (identical math to
//! [`legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot`]) rather than re-derived in
//! JS.

use super::*;
use crate::runtime::LegaiaRuntime;
use legaia_engine_core::field_env::{self, EnvDraw};
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::scene_resources::SceneResources;

/// PSX 12-bit angle -> radians.
const A2R: f32 = std::f32::consts::TAU / 4096.0;

/// The assembled static map for the scene the host is currently running.
/// Derived from the host's own [`SceneResources`] - no second resource build.
pub struct FieldRender {
    /// Environment-pack subset of `res.tmds` (pack-index order) - the index
    /// space the placement records select from.
    pub env_tmds: Vec<usize>,
    /// Placed-object draws (buildings / props / landmarks).
    pub placements: Vec<EnvDraw>,
    /// Bulk terrain-tile draws (ground / decor tiles).
    pub terrain: Vec<EnvDraw>,
    /// Walk-ground heightfield surface, when the scene has a resolvable floor.
    pub ground: Option<legaia_asset::field_objects::WalkHeightfield>,
    /// Cached built env mesh: `(slot, mesh, flat_rgba)`.
    #[allow(clippy::type_complexity)]
    pub cur: Option<(usize, legaia_tmd::mesh::VramMesh, Vec<u8>)>,
}

/// The lead party member's field-form actor: the object-local mesh, its
/// per-vertex bone ids, and the scratch buffer each frame's pose writes into.
pub(crate) struct PlayerRig {
    /// Object-local hybrid mesh (textured prims + the untextured flat / gouraud
    /// prims that carry per-vertex RGB), built once.
    pub base: legaia_tmd::mesh::VramMesh,
    /// Per-vertex TMD object index - the bone each vertex hangs from.
    pub object_ids: Vec<u32>,
    /// Per-vertex `[r, g, b, textured_flag]` for the hybrid shader.
    pub flat: Vec<u8>,
    /// Posed positions, rewritten by each [`LegaiaRuntime::player_mesh_positions`].
    pub posed: Vec<f32>,
}

/// The scene's NPC catalog (placements + resolved meshes).
pub(crate) struct NpcRender {
    pub pack: crate::field_npc::FieldNpcPack,
}

/// Compose `Rz . Ry . Rx . v + T` for every vertex, keyed by its bone
/// (`object_ids`). A vertex whose bone the pose doesn't cover keeps its
/// object-local position - a single-object model needs no pose at all, since
/// its local space *is* its model space.
///
/// Same composition as [`legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot`] (the
/// retail per-object assembly `FUN_8004998C`), applied in place so an animated
/// actor re-poses without rebuilding its geometry.
/// REF: FUN_8004998C
fn pose_into(
    out: &mut Vec<f32>,
    base: &[[f32; 3]],
    object_ids: &[u32],
    bones: &[([i16; 3], [i16; 3])],
) {
    out.clear();
    out.reserve(base.len() * 3);
    let trig: Vec<([f32; 3], [f32; 6])> = bones
        .iter()
        .map(|(t, r)| {
            let (sx, cx) = (r[0] as f32 * A2R).sin_cos();
            let (sy, cy) = (r[1] as f32 * A2R).sin_cos();
            let (sz, cz) = (r[2] as f32 * A2R).sin_cos();
            (
                [t[0] as f32, t[1] as f32, t[2] as f32],
                [cx, sx, cy, sy, cz, sz],
            )
        })
        .collect();
    for (v, p) in base.iter().enumerate() {
        let bone = object_ids.get(v).and_then(|&o| trig.get(o as usize));
        let Some((tr, [cx, sx, cy, sy, cz, sz])) = bone else {
            out.extend_from_slice(p);
            continue;
        };
        let (mut x, mut y, mut z) = (p[0], p[1], p[2]);
        let (ny, nz) = (y * cx - z * sx, y * sx + z * cx);
        y = ny;
        z = nz;
        let (nx, nz2) = (x * cy + z * sy, -x * sy + z * cy);
        x = nx;
        z = nz2;
        let (nx2, ny2) = (x * cz - y * sz, x * sz + y * cz);
        x = nx2;
        y = ny2;
        out.push(x + tr[0]);
        out.push(y + tr[1]);
        out.push(z + tr[2]);
    }
}

/// Resolve the scene's env-pack + placement / terrain / ground layers from the
/// resources the host already built. The engine-parity core of the play page's
/// static map.
pub fn build_field_render(
    index: &ProtIndex,
    scene: &Scene,
    res: &SceneResources,
    is_world_map: bool,
) -> FieldRender {
    let env_tmds = field_env::env_pack_tmd_indices(scene, res);
    let floor_lut = scene.field_floor_height_lut(index).ok().flatten();
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
    FieldRender {
        env_tmds,
        placements,
        terrain,
        ground,
        cur: None,
    }
}

impl LegaiaRuntime {
    /// The host's scene resources (built by `enter_field_scene`).
    fn res(&self) -> Option<&SceneResources> {
        self.scene_host.as_ref()?.resources.as_ref()
    }

    fn field_cur(&self) -> Option<&(usize, legaia_tmd::mesh::VramMesh, Vec<u8>)> {
        self.field.as_ref()?.cur.as_ref()
    }

    fn npc_cur(&self) -> Option<&(usize, legaia_tmd::mesh::VramMesh, Vec<u32>, Vec<u8>)> {
        self.npcs.as_ref()?.pack.cur.as_ref()
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    // ----------------------------------------------------------------- map

    /// Field VRAM (1 MB) - the image every mesh below samples. The engine's own
    /// scene VRAM, not a viewer-side rebuild.
    pub fn field_vram_bytes(&self) -> Vec<u8> {
        self.res()
            .map(|r| r.vram.as_bytes().to_vec())
            .unwrap_or_default()
    }

    /// `{"pack_count", "placements", "terrain", "ground_quads"}` for the status
    /// line; `null` before a scene is entered.
    pub fn field_status_json(&self) -> String {
        match self.field.as_ref() {
            Some(f) => format!(
                r#"{{"pack_count":{},"placements":{},"terrain":{},"ground_quads":{}}}"#,
                f.env_tmds.len(),
                f.placements.len(),
                f.terrain.len(),
                f.ground.as_ref().map(|h| h.quad_count()).unwrap_or(0),
            ),
            None => "null".to_string(),
        }
    }

    /// Select + build environment-pack slot `slot`; subsequent `field_mesh_*`
    /// reads return that mesh.
    pub fn field_mesh(&mut self, slot: u32) -> Result<u32, JsValue> {
        let s = slot as usize;
        let res_idx = {
            let f = self
                .field
                .as_ref()
                .ok_or_else(|| JsValue::from_str("field_mesh: no scene"))?;
            if f.cur.as_ref().map(|(cs, _, _)| *cs) == Some(s) {
                return Ok(slot);
            }
            *f.env_tmds
                .get(s)
                .ok_or_else(|| JsValue::from_str(&format!("field_mesh: slot {s} out of range")))?
        };
        let built = {
            let res = self
                .res()
                .ok_or_else(|| JsValue::from_str("field_mesh: no resources"))?;
            let rtmd = res
                .tmds
                .get(res_idx)
                .ok_or_else(|| JsValue::from_str("field_mesh: tmd missing"))?;
            crate::field_scene::build_hybrid_env_mesh(rtmd, &res.vram)
        };
        if let Some(f) = self.field.as_mut() {
            f.cur = Some((s, built.0, built.1));
        }
        Ok(slot)
    }

    pub fn field_mesh_positions(&self) -> Vec<f32> {
        let Some((_, m, _)) = self.field_cur() else {
            return Vec::new();
        };
        m.positions.iter().flatten().copied().collect()
    }

    pub fn field_mesh_uvs(&self) -> Vec<u8> {
        let Some((_, m, _)) = self.field_cur() else {
            return Vec::new();
        };
        m.uvs.iter().flatten().copied().collect()
    }

    pub fn field_mesh_cba_tsb(&self) -> Vec<u16> {
        let Some((_, m, _)) = self.field_cur() else {
            return Vec::new();
        };
        m.cba_tsb.iter().flatten().copied().collect()
    }

    pub fn field_mesh_indices(&self) -> Vec<u32> {
        self.field_cur()
            .map(|(_, m, _)| m.indices.clone())
            .unwrap_or_default()
    }

    pub fn field_mesh_flat_rgba(&self) -> Vec<u8> {
        self.field_cur()
            .map(|(_, _, f)| f.clone())
            .unwrap_or_default()
    }

    /// Per-placement env-pack slot (parallel to
    /// [`Self::field_placement_positions`] / [`Self::field_placement_rot_y`]).
    pub fn field_placement_slots(&self) -> Vec<u32> {
        self.field
            .as_ref()
            .map(|f| f.placements.iter().map(|d| d.env_slot as u32).collect())
            .unwrap_or_default()
    }

    pub fn field_placement_positions(&self) -> Vec<f32> {
        self.field
            .as_ref()
            .map(|f| env_positions(&f.placements))
            .unwrap_or_default()
    }

    pub fn field_placement_rot_y(&self) -> Vec<u16> {
        self.field
            .as_ref()
            .map(|f| f.placements.iter().map(|d| d.rot_y).collect())
            .unwrap_or_default()
    }

    pub fn field_terrain_slots(&self) -> Vec<u32> {
        self.field
            .as_ref()
            .map(|f| f.terrain.iter().map(|d| d.env_slot as u32).collect())
            .unwrap_or_default()
    }

    pub fn field_terrain_positions(&self) -> Vec<f32> {
        self.field
            .as_ref()
            .map(|f| env_positions(&f.terrain))
            .unwrap_or_default()
    }

    pub fn field_terrain_rot_y(&self) -> Vec<u16> {
        self.field
            .as_ref()
            .map(|f| f.terrain.iter().map(|d| d.rot_y).collect())
            .unwrap_or_default()
    }

    pub fn field_ground_positions(&self) -> Vec<f32> {
        let Some(hf) = self.field.as_ref().and_then(|f| f.ground.as_ref()) else {
            return Vec::new();
        };
        hf.positions.iter().flatten().copied().collect()
    }

    pub fn field_ground_uvs(&self) -> Vec<u8> {
        let Some(hf) = self.field.as_ref().and_then(|f| f.ground.as_ref()) else {
            return Vec::new();
        };
        hf.uvs.iter().flatten().copied().collect()
    }

    pub fn field_ground_cba_tsb(&self) -> Vec<u16> {
        let Some(hf) = self.field.as_ref().and_then(|f| f.ground.as_ref()) else {
            return Vec::new();
        };
        hf.cba_tsb.iter().flatten().copied().collect()
    }

    pub fn field_ground_indices(&self) -> Vec<u32> {
        self.field
            .as_ref()
            .and_then(|f| f.ground.as_ref())
            .map(|hf| hf.indices.clone())
            .unwrap_or_default()
    }

    pub fn field_ground_quad_count(&self) -> u32 {
        self.field
            .as_ref()
            .and_then(|f| f.ground.as_ref())
            .map(|hf| hf.quad_count() as u32)
            .unwrap_or(0)
    }

    // -------------------------------------------------------------- player

    /// `true` when the lead's field mesh resolved out of the global TMD pool.
    pub fn player_has_mesh(&self) -> bool {
        self.player.is_some()
    }

    /// Player mesh geometry (object-local; pair with
    /// [`Self::player_mesh_positions`], which poses it).
    pub fn player_mesh_indices(&self) -> Vec<u32> {
        self.player
            .as_ref()
            .map(|p| p.base.indices.clone())
            .unwrap_or_default()
    }

    pub fn player_mesh_uvs(&self) -> Vec<u8> {
        self.player
            .as_ref()
            .map(|p| p.base.uvs.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    pub fn player_mesh_cba_tsb(&self) -> Vec<u16> {
        self.player
            .as_ref()
            .map(|p| p.base.cba_tsb.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    pub fn player_mesh_flat_rgba(&self) -> Vec<u8> {
        self.player
            .as_ref()
            .map(|p| p.flat.clone())
            .unwrap_or_default()
    }

    /// The player's vertices **posed at the current frame**: the world's live
    /// `pose_frame` (idle clip standing, walk clip moving), composed per bone.
    /// Falls back to the object-local rest geometry when no clip is installed -
    /// which is what a lead outside the Vahn / Noa / Gala trio gets, since the
    /// locomotion bundle only banks those three.
    pub fn player_mesh_positions(&mut self) -> Vec<f32> {
        let pose: Option<Vec<([i16; 3], [i16; 3])>> = self
            .scene_host
            .as_ref()
            .and_then(|h| {
                let slot = h.world.player_actor_slot? as usize;
                h.world.actors.get(slot)
            })
            .and_then(|a| a.pose_frame.as_ref())
            .map(|p| p.bone_outputs.clone());
        let Some(p) = self.player.as_mut() else {
            return Vec::new();
        };
        match pose {
            Some(bones) if !bones.is_empty() => {
                // Disjoint field borrows: the scratch buffer and the source
                // geometry are different fields of the rig.
                let PlayerRig {
                    base,
                    object_ids,
                    posed,
                    ..
                } = p;
                pose_into(posed, &base.positions, object_ids, &bones);
                posed.clone()
            }
            _ => p.base.positions.iter().flatten().copied().collect(),
        }
    }

    /// `[world_x, world_y, world_z, facing_units]` for the player actor.
    /// `facing_units` is the engine heading (`render_26`, PSX 12-bit; `0` =
    /// travelling `+Z`); the world coords are the raw retail frame (`+Y` down).
    pub fn player_transform(&self) -> Vec<f32> {
        let Some(a) = self.scene_host.as_ref().and_then(|h| {
            let slot = h.world.player_actor_slot? as usize;
            h.world.actors.get(slot)
        }) else {
            return vec![0.0; 4];
        };
        vec![
            a.move_state.world_x as f32,
            a.move_state.world_y as f32,
            a.move_state.world_z as f32,
            a.move_state.render_26 as f32,
        ]
    }

    // ---------------------------------------------------------------- NPCs

    /// The scene's NPC / actor catalog. Shape:
    /// `{"anm_prot": 4, "npcs": [{"i", "slot", "model", "anim", "nobj",
    /// "kind", "target_map", "dialog", "conditional", "x", "z"}, ...]}`.
    /// `null` before a scene is entered.
    pub fn play_npc_catalog_json(&self) -> String {
        let Some(n) = self.npcs.as_ref() else {
            return "null".to_string();
        };
        let npcs: Vec<serde_json::Value> = n
            .pack
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
            "anm_prot": n.pack.anm_prot,
            "npcs": npcs,
        })
        .to_string()
    }

    /// Build catalog entry `i`'s mesh (hybrid: textured + vertex-colour prims,
    /// with per-vertex bone ids). Returns `i`.
    pub fn play_npc_mesh(&mut self, i: u32) -> Result<u32, JsValue> {
        let idx = i as usize;
        let (tmd, raw) = {
            let n = self
                .npcs
                .as_ref()
                .ok_or_else(|| JsValue::from_str("play_npc_mesh: no catalog"))?;
            if n.pack.cur.as_ref().map(|c| c.0) == Some(idx) {
                return Ok(i);
            }
            let e = n
                .pack
                .entries
                .get(idx)
                .ok_or_else(|| JsValue::from_str(&format!("play_npc_mesh: no entry {idx}")))?;
            let model = e.placement.model_index as usize;
            let res = self
                .res()
                .ok_or_else(|| JsValue::from_str("play_npc_mesh: no resources"))?;
            let t = res
                .tmds
                .get(model)
                .ok_or_else(|| JsValue::from_str("play_npc_mesh: model out of range"))?;
            (t.tmd.clone(), t.raw.clone())
        };
        let (mesh, object_ids, shading) =
            legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&tmd, &raw);
        let mut flat = Vec::with_capacity(shading.colors.len() * 4);
        for (c, &t) in shading.colors.iter().zip(shading.textured.iter()) {
            flat.extend_from_slice(&[c[0], c[1], c[2], if t != 0 { 255 } else { 0 }]);
        }
        if let Some(n) = self.npcs.as_mut() {
            n.pack.cur = Some((idx, mesh, object_ids, flat));
        }
        Ok(i)
    }

    pub fn play_npc_mesh_positions(&self) -> Vec<f32> {
        let Some((_, m, _, _)) = self.npc_cur() else {
            return Vec::new();
        };
        m.positions.iter().flatten().copied().collect()
    }

    pub fn play_npc_mesh_uvs(&self) -> Vec<u8> {
        let Some((_, m, _, _)) = self.npc_cur() else {
            return Vec::new();
        };
        m.uvs.iter().flatten().copied().collect()
    }

    pub fn play_npc_mesh_cba_tsb(&self) -> Vec<u16> {
        let Some((_, m, _, _)) = self.npc_cur() else {
            return Vec::new();
        };
        m.cba_tsb.iter().flatten().copied().collect()
    }

    pub fn play_npc_mesh_indices(&self) -> Vec<u32> {
        self.npc_cur()
            .map(|(_, m, _, _)| m.indices.clone())
            .unwrap_or_default()
    }

    /// Per-vertex TMD object index for the built NPC mesh - the bone each
    /// vertex hangs from. The page's animator keys its per-frame `R . v + T`
    /// on this.
    pub fn play_npc_mesh_object_ids(&self) -> Vec<u32> {
        self.npc_cur()
            .map(|(_, _, o, _)| o.clone())
            .unwrap_or_default()
    }

    pub fn play_npc_mesh_flat_rgba(&self) -> Vec<u8> {
        self.npc_cur()
            .map(|(_, _, _, f)| f.clone())
            .unwrap_or_default()
    }

    /// Catalog entry `i`'s clip, decoded to the pose format the JS animator
    /// consumes: `6` entries per bone per frame (`[tx, ty, tz, rx, ry, rz]`,
    /// absolute). Empty when the placement names no clip or the scene ships no
    /// ANM bundle. An NPC's clip is its placement `anim_id - 1` in the scene's
    /// own ANM bundle (`docs/formats/anm.md` § per-scene bundle).
    pub fn play_npc_pose_frames(&self, i: u32) -> Vec<i32> {
        let Some(n) = self.npcs.as_ref() else {
            return Vec::new();
        };
        let Some(e) = n.pack.entries.get(i as usize) else {
            return Vec::new();
        };
        let Some(prot) = n.pack.anm_prot else {
            return Vec::new();
        };
        if e.placement.anim_id == 0 {
            return Vec::new();
        }
        self.anm_pose_frames(prot, (e.placement.anim_id - 1) as u32, e.nobj)
    }

    /// `[frame_count, bone_count]` of catalog entry `i`'s clip; `[0, 0]` when
    /// it has none.
    pub fn play_npc_pose_dims(&self, i: u32) -> Vec<u32> {
        let Some(n) = self.npcs.as_ref() else {
            return vec![0, 0];
        };
        let Some(e) = n.pack.entries.get(i as usize) else {
            return vec![0, 0];
        };
        let parts = e.nobj.max(1);
        let f = self.play_npc_pose_frames(i);
        if f.is_empty() {
            return vec![0, 0];
        }
        vec![f.len() as u32 / (parts * 6), parts]
    }

    /// Live world state of every catalogued NPC, flattened
    /// `[x, y, z, facing_units, ...]` in catalog order. Positions come from the
    /// **world** (`field_npc_positions`), so an NPC walking its MAN-authored
    /// route walks on screen; the MAN placement anchor is the fallback for one
    /// that has never moved. `y` is the floor height under the NPC.
    pub fn play_npc_transforms(&self) -> Vec<f32> {
        let (Some(n), Some(h)) = (self.npcs.as_ref(), self.scene_host.as_ref()) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(n.pack.entries.len() * 4);
        for e in &n.pack.entries {
            let slot = e.placement.index as u8;
            let (x, z) = h
                .world
                .field_npc_positions
                .get(&slot)
                .copied()
                .unwrap_or((e.placement.world_x, e.placement.world_z));
            let facing = h.world.field_npc_headings.get(&slot).copied().unwrap_or(0) as f32;
            let y = h.world.sample_field_floor_height(x as i32, z as i32) as f32;
            out.extend_from_slice(&[x as f32, y, z as f32, facing]);
        }
        out
    }
}

/// Flatten `EnvDraw` world positions to `[x, y, z, ...]`.
fn env_positions(draws: &[EnvDraw]) -> Vec<f32> {
    let mut out = Vec::with_capacity(draws.len() * 3);
    for d in draws {
        out.push(d.world_x as f32);
        out.push(d.world_y as f32);
        out.push(d.world_z as f32);
    }
    out
}

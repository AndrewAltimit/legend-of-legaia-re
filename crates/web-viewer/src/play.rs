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
    /// Placed-object draws (buildings / props / landmarks). A nonzero
    /// [`EnvDraw::anim_id`] means the object's bind names a clip: its TMD
    /// objects are that clip's bones and the mesh must be **posed** from
    /// frame 0 of scene ANM record `anim_id - 1` (see
    /// [`field_env::resolve_placed_env_draws`]).
    pub placements: Vec<EnvDraw>,
    /// Bulk terrain-tile draws (ground / decor tiles). `FLAG_PLACED` records
    /// are excluded - they are already drawn, posed, by the placement layer
    /// (the native window's `resolve_field_terrain_draws` rule).
    pub terrain: Vec<EnvDraw>,
    /// Walk-ground heightfield surface, when the scene has a resolvable floor.
    pub ground: Option<legaia_asset::field_objects::WalkHeightfield>,
    /// Cached built env mesh: `((slot, anim_id), mesh, flat_rgba)`.
    /// `anim_id != 0` is the frame-0 posed variant of the slot's mesh.
    #[allow(clippy::type_complexity)]
    pub cur: Option<((usize, u8), legaia_tmd::mesh::VramMesh, Vec<u8>)>,
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

/// Live clip playback for one placed NPC - the browser twin of the native
/// play-window's `npc_clip_players` map: a [`FieldClipPlayer`] per placement
/// slot, advanced in **sim-tick** time (one [`LegaiaRuntime::tick_frame`] =
/// one 60 Hz tick) so the clip plays at the retail cadence regardless of the
/// display refresh rate, and re-targeted by channel op-`0x4B` ANIMATE cues
/// (drained from `World::field_npc_anim_cues`) so scripted actors perform
/// their beats instead of looping the placement clip.
///
/// [`FieldClipPlayer`]: legaia_engine_core::field_anim::FieldClipPlayer
pub(crate) struct NpcClip {
    pub player: legaia_engine_core::field_anim::FieldClipPlayer,
    /// Bumped on every ANIMATE-cue re-target, so the page knows the pose
    /// stream behind the frame index changed and must be re-read.
    pub generation: u32,
    /// Clip resolves from the PROT 0874 locomotion bundle (global-pool
    /// special) rather than the scene's own ANM bundle.
    pub special: bool,
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
/// static map - the same resolver calls the native play-window makes:
///
/// - the **placed** layer goes through [`field_env::resolve_placed_env_draws`]
///   with the scene's object binds, so every multi-object prop carries the
///   clip that poses it (unposed, a cupboard's doors float inside the cabinet
///   and a windmill's sails heap on its hub);
/// - the **terrain** sweep excludes `FLAG_PLACED` records - those are already
///   drawn (posed) by the placement layer, and the second copy would be the
///   unposed one (the native `resolve_field_terrain_draws` rule).
pub fn build_field_render(
    index: &ProtIndex,
    scene: &Scene,
    res: &SceneResources,
    is_world_map: bool,
) -> FieldRender {
    let env_tmds = field_env::env_pack_tmd_indices(scene, res);
    let floor_lut = scene.field_floor_height_lut(index).ok().flatten();
    let (placement_records, terrain_records, binds) = if is_world_map {
        (
            scene
                .walk_object_placements(index)
                .ok()
                .flatten()
                .unwrap_or_default(),
            Vec::new(),
            None,
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
                .unwrap_or_default()
                .into_iter()
                .filter(|p| p.flags & legaia_asset::field_objects::FLAG_PLACED == 0)
                .collect(),
            scene.field_object_binds(index).ok().flatten(),
        )
    };
    let (placements, _) = field_env::resolve_placed_env_draws(
        &env_tmds,
        &placement_records,
        floor_lut,
        binds.as_ref(),
    );
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

    fn field_cur(&self) -> Option<&((usize, u8), legaia_tmd::mesh::VramMesh, Vec<u8>)> {
        self.field.as_ref()?.cur.as_ref()
    }

    fn npc_cur(&self) -> Option<&(usize, legaia_tmd::mesh::VramMesh, Vec<u32>, Vec<u8>)> {
        self.npcs.as_ref()?.pack.cur.as_ref()
    }

    /// Bone count of the clip a catalogued NPC placement names (`anim_id - 1`
    /// in the scene bundle, or the locomotion bundle for a global-pool
    /// special). `None` when the placement has no clip or the bundle is
    /// unavailable - the mesh then keeps its full object table.
    fn npc_clip_bone_count(&self, anim_id: u8, special: bool) -> Option<usize> {
        let bundle = if special {
            self.locomotion_anm.as_ref()?
        } else {
            self.scene_anm.as_ref()?
        };
        let rec_idx = (anim_id as usize).checked_sub(1)?;
        let rec = bundle.record(rec_idx).ok()?;
        (rec.bone_count > 0).then_some(rec.bone_count as usize)
    }

    /// Frame-0 bone transforms of scene ANM record `anim_id - 1`, under
    /// retail's count-equality contract (`FUN_8001B964` refuses to draw a
    /// posed prop whose mesh chain and clip disagree on the part count).
    /// `None` = draw the raw unposed mesh instead.
    fn frame0_bone_offsets(
        &self,
        anim_id: u8,
        res_idx: usize,
    ) -> Option<Vec<([i16; 3], [i16; 3])>> {
        self.frame_bone_offsets(anim_id, res_idx, 0)
    }

    /// Bone transforms of scene ANM record `anim_id - 1` at clip frame `frame`,
    /// under retail's count-equality contract (see [`Self::frame0_bone_offsets`]).
    /// Frame `0` is the rest pose; a live prop's cursor (`PropAnim::frame`)
    /// advances it, which is what makes the Rim Elm windmill's sails turn.
    /// `None` = the clip / mesh disagree on the part count, so pose nothing.
    fn frame_bone_offsets(
        &self,
        anim_id: u8,
        res_idx: usize,
        frame: usize,
    ) -> Option<Vec<([i16; 3], [i16; 3])>> {
        let bundle = self.scene_anm.as_ref()?;
        let rec_idx = (anim_id as usize).checked_sub(1)?;
        let rec = bundle.record(rec_idx).ok()?;
        let bones = rec.bone_count as usize;
        let objects = self.res()?.tmds.get(res_idx)?.tmd.objects.len();
        if bones != objects {
            crate::console_log(&format!(
                "play: ANM record {rec_idx} has {bones} bones but env mesh (res {res_idx}) \
                 has {objects} objects - not posing"
            ));
            return None;
        }
        let f = frame.min((rec.frame_count as usize).saturating_sub(1));
        Some(
            (0..bones)
                .map(|b| match bundle.bone_transform(rec_idx, f, b) {
                    Some(t) => (
                        [t.t_x as i16, t.t_y as i16, t.t_z as i16],
                        [t.r_x as i16, t.r_y as i16, t.r_z as i16],
                    ),
                    None => ([0; 3], [0; 3]),
                })
                .collect(),
        )
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
        self.field_mesh_posed(slot, 0)
    }

    /// Select + build environment-pack slot `slot` **posed at frame 0** of
    /// scene ANM record `anim_id - 1` - the rest state of a placed prop whose
    /// object bind names a clip (cupboard doors closed on the cabinet's front
    /// face, the windmill's sails on their hub). Falls back to the raw
    /// object-local mesh when the pose can't resolve (no scene bundle, or the
    /// clip's bone count doesn't match the mesh's object count - retail's
    /// count-equality contract, `FUN_8001B964`), exactly as the native
    /// play-window falls back to its unposed instance. `anim_id == 0` is the
    /// plain unposed build ([`Self::field_mesh`]).
    pub fn field_mesh_posed(&mut self, slot: u32, anim_id: u32) -> Result<u32, JsValue> {
        let s = slot as usize;
        let anim = anim_id.min(u8::MAX as u32) as u8;
        let res_idx = {
            let f = self
                .field
                .as_ref()
                .ok_or_else(|| JsValue::from_str("field_mesh: no scene"))?;
            if f.cur.as_ref().map(|(key, _, _)| *key) == Some((s, anim)) {
                return Ok(slot);
            }
            *f.env_tmds
                .get(s)
                .ok_or_else(|| JsValue::from_str(&format!("field_mesh: slot {s} out of range")))?
        };
        let offsets: Option<Vec<([i16; 3], [i16; 3])>> = if anim == 0 {
            None
        } else {
            self.frame0_bone_offsets(anim, res_idx)
        };
        let built = {
            let res = self
                .res()
                .ok_or_else(|| JsValue::from_str("field_mesh: no resources"))?;
            let rtmd = res
                .tmds
                .get(res_idx)
                .ok_or_else(|| JsValue::from_str("field_mesh: tmd missing"))?;
            match &offsets {
                Some(o) => crate::field_scene::build_hybrid_env_mesh_posed(rtmd, o),
                None => crate::field_scene::build_hybrid_env_mesh(rtmd, &res.vram),
            }
        };
        if let Some(f) = self.field.as_mut() {
            f.cur = Some(((s, anim), built.0, built.1));
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

    /// Per-placement object-bind animation id (parallel to
    /// [`Self::field_placement_slots`]). `0` = unposed; nonzero = draw the
    /// slot's mesh through [`Self::field_mesh_posed`] with this id, or the
    /// prop's multi-object parts heap on the origin.
    pub fn field_placement_anim_ids(&self) -> Vec<u32> {
        self.field
            .as_ref()
            .map(|f| f.placements.iter().map(|d| d.anim_id as u32).collect())
            .unwrap_or_default()
    }

    /// Live clip frame of each placement (parallel to
    /// [`Self::field_placement_slots`]): `-1` for a static prop (no anim, or
    /// no live prop-bank entry), else the prop's current cursor frame
    /// (`PropAnimBank::frame`, the `actor+0x68 >> 4` the draw walker poses
    /// from). The world advances every prop's cursor each field tick
    /// (`tick_prop_interactions` -> `PropAnimBank::tick_anims`, retail's
    /// `FUN_800204F8`), so an animated prop - the windmill sails, a swinging
    /// door mid-swing - reports a changing frame, and the page re-poses it.
    pub fn field_placement_frames(&self) -> Vec<i32> {
        let (Some(f), Some(h)) = (self.field.as_ref(), self.scene_host.as_ref()) else {
            return Vec::new();
        };
        f.placements
            .iter()
            .map(|d| {
                if d.anim_id == 0 {
                    return -1;
                }
                h.world
                    .field_prop_bank
                    .frame(d.anchor)
                    .map(|fr| fr as i32)
                    .unwrap_or(-1)
            })
            .collect()
    }

    /// Positions of environment-pack slot `slot` **posed at clip frame
    /// `frame`** of scene ANM record `anim_id - 1` - the per-frame re-pose the
    /// draw walker (`FUN_8001B964`) does off a placed prop's live cursor.
    /// Same vertex order as [`Self::field_mesh_posed`]'s frame-0 build (the two
    /// differ only in the per-object transform), so the page can upload the
    /// mesh once and rewrite just its positions each frame. Empty when the pose
    /// can't resolve (no bundle / bone-count mismatch) - the caller then leaves
    /// the prop at its rest pose.
    pub fn field_mesh_posed_frame_positions(
        &self,
        slot: u32,
        anim_id: u32,
        frame: u32,
    ) -> Vec<f32> {
        let s = slot as usize;
        let anim = anim_id.min(u8::MAX as u32) as u8;
        let Some(f) = self.field.as_ref() else {
            return Vec::new();
        };
        let Some(&res_idx) = f.env_tmds.get(s) else {
            return Vec::new();
        };
        let Some(offsets) = self.frame_bone_offsets(anim, res_idx, frame as usize) else {
            return Vec::new();
        };
        let Some(res) = self.res() else {
            return Vec::new();
        };
        let Some(rtmd) = res.tmds.get(res_idx) else {
            return Vec::new();
        };
        let (mesh, _flat) = crate::field_scene::build_hybrid_env_mesh_posed(rtmd, &offsets);
        mesh.positions.iter().flatten().copied().collect()
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
                    "special": e.special,
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
    ///
    /// Mirrors the native window's field-NPC bind: a special
    /// (`model >= 0xF0`) resolves out of the world's **global TMD pool**
    /// rather than the scene's, and when the placement names a clip the TMD's
    /// object table is truncated to the clip's bone count (the objects past it
    /// are equipment-swap templates the clip never poses - drawn, they'd
    /// litter the actor's feet with raw parts).
    pub fn play_npc_mesh(&mut self, i: u32) -> Result<u32, JsValue> {
        let idx = i as usize;
        let (mut tmd, raw, anim_id, special) = {
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
            let (tmd, raw) = if e.special {
                let slot = (e.placement.model_index - 0xF0) as usize;
                let g = self
                    .scene_host
                    .as_ref()
                    .and_then(|h| h.world.global_tmd_pool.get(slot))
                    .and_then(|s| s.as_ref())
                    .ok_or_else(|| JsValue::from_str("play_npc_mesh: no global-pool mesh"))?;
                (g.tmd.clone(), g.raw.clone())
            } else {
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
            (tmd, raw, e.placement.anim_id, e.special)
        };
        if let Some(bones) = self.npc_clip_bone_count(anim_id, special) {
            tmd.objects.truncate(bones);
        }
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
    /// absolute). Empty when the placement names no clip or its bundle is
    /// unavailable. An NPC's clip is its placement `anim_id - 1` in the
    /// scene's own ANM bundle (`docs/formats/anm.md` § per-scene bundle); a
    /// global-pool special's indexes the PROT 0874 locomotion bundle instead
    /// (the native window's bundle split).
    pub fn play_npc_pose_frames(&self, i: u32) -> Vec<i32> {
        let Some(n) = self.npcs.as_ref() else {
            return Vec::new();
        };
        let Some(e) = n.pack.entries.get(i as usize) else {
            return Vec::new();
        };
        let bundle = if e.special {
            self.locomotion_anm.as_ref()
        } else {
            self.scene_anm.as_ref()
        };
        let (Some(b), Some(rec_idx)) = (bundle, (e.placement.anim_id as usize).checked_sub(1))
        else {
            return Vec::new();
        };
        bundle_pose_frames(b, rec_idx)
    }

    /// `[frame_count, bone_count]` of catalog entry `i`'s clip; `[0, 0]` when
    /// it has none. `bone_count` is the clip's own count - the stride of
    /// [`Self::play_npc_pose_frames`], and the count
    /// [`Self::play_npc_mesh`] truncated the object table to.
    pub fn play_npc_pose_dims(&self, i: u32) -> Vec<u32> {
        let Some(n) = self.npcs.as_ref() else {
            return vec![0, 0];
        };
        let Some(e) = n.pack.entries.get(i as usize) else {
            return vec![0, 0];
        };
        let bundle = if e.special {
            self.locomotion_anm.as_ref()
        } else {
            self.scene_anm.as_ref()
        };
        let (Some(b), Some(rec_idx)) = (bundle, (e.placement.anim_id as usize).checked_sub(1))
        else {
            return vec![0, 0];
        };
        match b.record(rec_idx) {
            Ok(r) if r.bone_count > 0 => vec![r.frame_count as u32, r.bone_count as u32],
            _ => vec![0, 0],
        }
    }

    /// The off-map hide-box coordinate (`FIELD_OFFMAP_HIDE_XZ`). Retail parks
    /// despawned / story-hidden actors at this far-corner sentinel tile
    /// precisely so they never render; the page must skip drawing any NPC
    /// whose **live** position is this tile on both axes, exactly as the
    /// native play-window's draw pass does.
    pub fn field_offmap_hide_xz(&self) -> i32 {
        legaia_engine_core::world::FIELD_OFFMAP_HIDE_XZ as i32
    }

    /// Live clip-playback state of every catalogued NPC, flattened
    /// `[frame, generation, ...]` pairs in catalog order; `[-1, -1]` for an
    /// entry with no live clip player. `frame` is the clip frame this render
    /// should show ([`legaia_engine_core::field_anim::FieldClipPlayer::frame`],
    /// advanced once per drained sim tick - the native window's sim-tick anim
    /// contract); `generation` bumps when an ANIMATE cue re-targets the clip,
    /// telling the page to re-read the pose behind the index.
    pub fn play_npc_clip_states(&self) -> Vec<i32> {
        let Some(n) = self.npcs.as_ref() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(n.pack.entries.len() * 2);
        for e in &n.pack.entries {
            match self.npc_clips.get(&(e.placement.index as u8)) {
                Some(c) => {
                    out.push(c.player.frame() as i32);
                    out.push(c.generation as i32);
                }
                None => {
                    out.push(-1);
                    out.push(-1);
                }
            }
        }
        out
    }

    /// Current pose of catalog entry `i`'s **live** clip: 6 `i32` per bone
    /// (`[tx, ty, tz, rx, ry, rz]`, absolute), read WITHOUT advancing the
    /// playhead ([`FieldClipPlayer::current_pose`] - the playhead moves only
    /// in [`LegaiaRuntime::tick_frame`]). Unlike
    /// [`Self::play_npc_pose_frames`] this follows ANIMATE-cue re-targets, so
    /// a scripted actor's performed clip is what comes back. Empty when the
    /// entry has no live clip player.
    ///
    /// [`FieldClipPlayer::current_pose`]: legaia_engine_core::field_anim::FieldClipPlayer::current_pose
    pub fn play_npc_live_bones(&self, i: u32) -> Vec<i32> {
        let Some(n) = self.npcs.as_ref() else {
            return Vec::new();
        };
        let Some(e) = n.pack.entries.get(i as usize) else {
            return Vec::new();
        };
        let Some(c) = self.npc_clips.get(&(e.placement.index as u8)) else {
            return Vec::new();
        };
        let pose = c.player.current_pose();
        let mut out = Vec::with_capacity(pose.bone_outputs.len() * 6);
        for (t, r) in pose.bone_outputs {
            out.extend([
                t[0] as i32,
                t[1] as i32,
                t[2] as i32,
                r[0] as i32,
                r[1] as i32,
                r[2] as i32,
            ]);
        }
        out
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
            // A slot with no seeded heading renders at **identity** in the
            // native window (`redraw.rs`: `None => Mat4::IDENTITY`). The page
            // composes `rot = -(facing + 2048)` (the half-turn the walker
            // convention carries), so identity is `facing = 2048`, not `0` -
            // `0` would draw every prologue-less NPC turned half a revolution.
            let facing = h
                .world
                .field_npc_headings
                .get(&slot)
                .copied()
                .unwrap_or(2048) as f32;
            let y = h.world.sample_field_floor_height(x as i32, z as i32) as f32;
            out.extend_from_slice(&[x as f32, y, z as f32, facing]);
        }
        out
    }
}

/// Decode ANM bundle record `rec_idx` into the flat pose stream the JS
/// animator consumes: `6` `i32` per bone per frame
/// (`[tx, ty, tz, rx, ry, rz]`, absolute), stride = the record's own bone
/// count. Empty when the record doesn't decode.
fn bundle_pose_frames(
    bundle: &legaia_asset::player_anm::PlayerAnmBundle,
    rec_idx: usize,
) -> Vec<i32> {
    let Ok(rec) = bundle.record(rec_idx) else {
        return Vec::new();
    };
    let bones = rec.bone_count as usize;
    let frames = rec.frame_count as usize;
    let mut out = Vec::with_capacity(frames * bones * 6);
    for f in 0..frames {
        for b in 0..bones {
            let Some(t) = bundle.bone_transform(rec_idx, f, b) else {
                return Vec::new();
            };
            out.extend_from_slice(&[t.t_x, t.t_y, t.t_z, t.r_x, t.r_y, t.r_z]);
        }
    }
    out
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

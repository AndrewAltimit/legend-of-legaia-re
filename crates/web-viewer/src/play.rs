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
use legaia_engine_core::field_env::{self, EnvDraw, PropPoseKey};
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
    /// Which placed-object sweep owns each placement draw (parallel to
    /// [`Self::placements`]): `Some` for the sub-area window sweep's, which
    /// [`LegaiaRuntime::field_placement_live`] gates on the world's windowed
    /// static-object list through [`field_env::placed_draw_live`] - the kernel
    /// the native play-window asks per frame.
    pub window_keys: Vec<Option<field_env::PlacedWindowKey>>,
    /// Per-placement bind record (parallel to [`Self::placements`], `None` =
    /// unbound): the key [`LegaiaRuntime::field_placement_moves`] looks a
    /// script's live object displacement up by.
    pub placement_records: Vec<Option<usize>>,
    /// Bulk terrain-tile draws (ground / decor tiles). `FLAG_PLACED` records
    /// are excluded - they are already drawn, posed, by the placement layer
    /// (the native window's `resolve_field_terrain_draws` rule).
    pub terrain: Vec<EnvDraw>,
    /// Walk-ground heightfield surface, when the scene has a resolvable floor.
    pub ground: Option<legaia_asset::field_objects::WalkHeightfield>,
    /// The scene's MAN-frame floor-height ladder, i.e. the one the two draw
    /// lists' `world_y` were baked against. Kept so the live ladder - which
    /// the field VM animates per frame (op `0x4C` nibble-9) - can be folded
    /// back in through [`field_env::FloorWave`] without re-walking the map.
    pub floor_lut: Option<[i16; 16]>,
    /// The live (scratchpad-frame) ladder [`Self::ground`]'s drawn positions
    /// were last re-resolved against by
    /// [`LegaiaRuntime::field_ground_live_positions`]; `None` until the first
    /// re-resolve, and always `None` on the overworld, whose ground does not
    /// follow the field ladder.
    pub ground_lut_applied: Option<[i16; 16]>,
    /// Whether this is a kingdom overworld (its ground is not re-resolved).
    pub is_world_map: bool,
    /// Cached built env mesh: `((slot, anim_id), mesh, flat_rgba)`.
    /// `anim_id != 0` is the frame-0 posed variant of the slot's mesh.
    #[allow(clippy::type_complexity)]
    pub cur: Option<((usize, u8), legaia_tmd::mesh::VramMesh, Vec<u8>)>,
    /// The occlusion fade's visibility gate: the static scene triangles
    /// (terrain + placements, retail Y-down world), ray-cast per frame by
    /// [`LegaiaRuntime::field_player_occluded`] - the same
    /// `engine_core::field_occlusion` kernel the native play-window runs.
    pub occluders: legaia_engine_core::field_occlusion::FieldOccluders,
    /// Cross-draw coplanar lifts (`engine_core::coplanar_draws`), applied to
    /// each draw's translation by the position accessors - the same map the
    /// native play-window (`coplanar_env_offsets`) and the field-scene viewer
    /// compute. Without it every placement/terrain pair that meets on one
    /// world plane (a floor slab against its room's wall-strip aprons)
    /// z-fights view-angle-dependently on this page alone.
    pub coplanar_offsets: std::collections::HashMap<EnvDraw, [f32; 3]>,
    /// Per-placement uniform render scale (parallel to [`Self::placements`]):
    /// the bind record's `actor[+0x72]` as a factor, `1.0` for the ordinary
    /// unit-scale object - [`field_env::placed_render_scales`], the kernel the
    /// native play-window folds into the same draws' model matrices.
    pub placement_scales: Vec<f32>,
    /// Where the overworld's **decoration** layer starts in
    /// [`Self::placements`] (the landmarks come first); `placements.len()`
    /// on every other scene. The draws from here on carry retail's
    /// per-object decoration depth cue
    /// ([`legaia_engine_core::overworld_ground_cue::decoration_draw_cue`]),
    /// the native window's `world_map_deco_start` twin.
    pub decoration_start: usize,
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
/// display refresh rate, and re-targeted by channel clip cues (`A2` ExecMove, `4C 51`)
/// (drained from `World::npcs.anim_cues`) so scripted actors perform
/// their beats instead of looping the placement clip.
///
/// [`FieldClipPlayer`]: legaia_engine_core::field_anim::FieldClipPlayer
pub(crate) struct NpcClip {
    pub player: legaia_engine_core::field_anim::FieldClipPlayer,
    /// Bumped on every ANIMATE-cue re-target, so the page knows the pose
    /// stream behind the frame index changed and must be re-read.
    pub generation: u32,
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

/// The overworld's placed landmarks minus the story-hidden ones (bind prologue
/// parked the actor at the hide box), the same filter the native window's
/// `resolve_world_map_terrain_draws` applies.
fn world_map_landmarks(
    index: &ProtIndex,
    scene: &Scene,
    hidden_records: &std::collections::HashSet<usize>,
) -> Vec<legaia_asset::field_objects::Placement> {
    let mut landmarks = scene
        .walk_object_placements(index)
        .ok()
        .flatten()
        .unwrap_or_default();
    if let Ok(Some(binds)) = scene.field_object_binds(index) {
        field_env::retain_visible_landmark_placements(&mut landmarks, &binds, hidden_records);
    }
    landmarks
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
    hidden_records: &std::collections::HashSet<usize>,
    render_scales: &std::collections::HashMap<usize, u16>,
    floor_follow: &dyn Fn(usize, i32, i32) -> Option<i32>,
) -> FieldRender {
    let env_tmds = field_env::env_pack_tmd_indices(scene, res);
    let floor_lut = scene.field_floor_height_lut(index).ok().flatten();
    let (placement_records, terrain_records, binds) = if is_world_map {
        // Overworld: the walk-object placements plus the decoration sweep
        // (trees, mountain groups) the native window's world-map branch
        // appends (`field_render.rs`); the page used to draw the first
        // layer only, so every kingdom lost its forests and ranges.
        let mut tiles = world_map_landmarks(index, scene, hidden_records);
        if let Ok(Some(deco)) = scene.walk_decoration_placements(index) {
            tiles.extend(deco);
        }
        (tiles, Vec::new(), None)
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
    let (mut placements, _) = field_env::resolve_placed_env_draws(
        &env_tmds,
        &placement_records,
        floor_lut,
        binds.as_ref(),
    );
    // The decorations follow the landmarks in `placement_records`; binds are
    // `None` on the overworld, so the landmark count is what they resolve to
    // on their own (and the retain below never runs there).
    let decoration_start = if is_world_map {
        let landmarks = world_map_landmarks(index, scene, hidden_records);
        field_env::resolve_placed_env_draws(&env_tmds, &landmarks, floor_lut, None)
            .0
            .len()
    } else {
        placements.len()
    };
    // Story-hidden placed objects (bind prologue parked at the hide box -
    // town01's flag-gated gate rocks): same gate the native shell applies in
    // `resolve_placement_draws`.
    if let Some(binds) = binds.as_ref() {
        field_env::retain_visible_placed_draws(&mut placements, binds, hidden_records);
        // A bind record carrying the actor tick's floor-follow law draws on
        // the floor sample under it, not at its `.MAP` lift - the same pass
        // the native shell runs (`follow_floor_placed_draws`).
        field_env::follow_floor_placed_draws(&mut placements, binds, floor_follow);
    }
    let placement_scales =
        field_env::placed_render_scales(&placements, binds.as_ref(), render_scales);
    // The overworld resolves without binds (its landmarks draw unposed and
    // unscaled), but a landmark still has an actor a kingdom MAN can tint -
    // so its record comes off the binds all the same; the decorations that
    // follow have none. The native window's `resolve_world_map_terrain_draws`
    // keys its landmark cues the same way.
    let placement_records = if is_world_map {
        let wm_binds = scene.field_object_binds(index).ok().flatten();
        let mut r = field_env::placed_bind_records(
            &placements[..decoration_start.min(placements.len())],
            wm_binds.as_ref(),
        );
        r.resize(placements.len(), None);
        r
    } else {
        field_env::placed_bind_records(&placements, binds.as_ref())
    };
    let window_keys = placements
        .iter()
        .map(|d| field_env::placed_window_key(d, binds.as_ref()))
        .collect();
    let (terrain, _) = field_env::resolve_env_draws(&env_tmds, &terrain_records, floor_lut);
    let ground = scene
        .walk_heightfield(index)
        .ok()
        .flatten()
        .filter(|h| !h.indices.is_empty());
    let occluders =
        legaia_engine_core::field_occlusion::FieldOccluders::build(&[&terrain, &placements], res);
    // Cross-draw coplanar lifts over the combined layers (terrain first, then
    // placements - the same concatenation the native shell and the field-scene
    // viewer rank, so all three hosts lift the same draws).
    let mut combined: Vec<EnvDraw> = Vec::with_capacity(terrain.len() + placements.len());
    combined.extend_from_slice(&terrain);
    combined.extend_from_slice(&placements);
    let planes = legaia_engine_core::coplanar_draws::draw_plane_summaries(&combined, res);
    let coplanar_offsets =
        legaia_engine_core::coplanar_draws::coplanar_draw_offsets(&combined, &planes);
    FieldRender {
        env_tmds,
        placements,
        window_keys,
        placement_records,
        terrain,
        ground,
        floor_lut,
        ground_lut_applied: None,
        is_world_map,
        cur: None,
        occluders,
        coplanar_offsets,
        placement_scales,
        decoration_start,
    }
}

impl LegaiaRuntime {
    /// The host's scene resources (built by `enter_field_scene`).
    pub(crate) fn res(&self) -> Option<&SceneResources> {
        self.scene_host.as_ref()?.resources.as_ref()
    }

    fn field_cur(&self) -> Option<&((usize, u8), legaia_tmd::mesh::VramMesh, Vec<u8>)> {
        self.field.as_ref()?.cur.as_ref()
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
        self.frame_bone_offsets(anim_id, res_idx, PropPoseKey::REST)
    }

    /// Bone transforms of scene ANM record `anim_id - 1` posed at `key`, under
    /// retail's count-equality contract (see [`Self::frame0_bone_offsets`]).
    /// [`PropPoseKey::REST`] is the rest pose; a live prop's cursor
    /// (`PropAnim::pose_key`) advances it, which is what makes the Rim Elm
    /// windmill's sails turn. The transforms come from the engine's shared
    /// kernel `field_env::prop_bone_offsets` - the frame blender's port, the
    /// same call the native play-window's prop re-pose makes.
    /// `None` = the clip / mesh disagree on the part count, so pose nothing.
    fn frame_bone_offsets(
        &self,
        anim_id: u8,
        res_idx: usize,
        key: PropPoseKey,
    ) -> Option<Vec<([i16; 3], [i16; 3])>> {
        let bundle = self.scene_anm.as_ref()?;
        let objects = self.res()?.tmds.get(res_idx)?.tmd.objects.len();
        field_env::posed_prop_offsets(bundle, anim_id, key, objects)
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    // ----------------------------------------------------------------- map

    /// Field VRAM (1 MB) - the image every mesh below samples. The engine's own
    /// scene VRAM, not a viewer-side rebuild.
    ///
    /// While a field-to-battle transition holds a landed capture, this is the
    /// emitter's **cloned** page instead - the scene VRAM with the captured
    /// field frame (and, for the curtain, its per-frame intermediate) blitted
    /// into the texture-page rects the style bodies sample. The pristine page
    /// comes back the moment the transition drops, via the same dirty flag.
    pub fn field_vram_bytes(&self) -> Vec<u8> {
        if let Some(page) = self.battle_intro.as_ref().and_then(|i| i.captured_vram()) {
            return page.as_bytes().to_vec();
        }
        self.res()
            .map(|r| r.vram.as_bytes().to_vec())
            .unwrap_or_default()
    }

    /// True when a field VRAM effect (water CLUT-walk shimmer, jou's ambient
    /// palette cyclers / lightning, scripted CLUT fx) changed texels since
    /// the last call - the page then re-uploads [`Self::field_vram_bytes`].
    /// Reading clears the flag.
    pub fn field_vram_take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.field_vram_dirty)
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
            let (mut mesh, flat) = match &offsets {
                Some(o) => crate::field_scene::build_hybrid_env_mesh_posed(rtmd, o),
                None => crate::field_scene::build_hybrid_env_mesh(rtmd, &res.vram),
            };
            // Enhanced lighting's emissive tags (TSB bit 13) - the native
            // window's env-mesh rule; the shaders read them only while the
            // enhancement is on.
            legaia_engine_ui::scene_lighting::tag_emissive_hybrid(
                &rtmd.raw, &mut mesh, &flat, &res.vram,
            );
            (mesh, flat)
        };
        if let Some(f) = self.field.as_mut() {
            f.cur = Some(((s, anim), built.0, built.1));
        }
        Ok(slot)
    }

    /// Select + build environment-pack slot `slot` (unposed) with its
    /// **light-source rows** shaded for a draw at the record angles
    /// `(rot_x, rot_y, rot_z)` under the world's live field light - the
    /// `legaia_engine_core::field_lit_mesh` kernel the native window shades
    /// its env draws with. Returns whether the mesh has lit rows at all; for a
    /// mesh without any, the build is the plain [`Self::field_mesh`] one and
    /// the page keeps sharing that upload across draws.
    pub fn field_mesh_lit(
        &mut self,
        slot: u32,
        rot_x: u32,
        rot_y: u32,
        rot_z: u32,
    ) -> Result<bool, JsValue> {
        let s = slot as usize;
        let light = self
            .scene_host
            .as_ref()
            .map(|h| h.world.presentation.field_light)
            .ok_or_else(|| JsValue::from_str("field_mesh_lit: no scene"))?;
        let res_idx = *self
            .field
            .as_ref()
            .ok_or_else(|| JsValue::from_str("field_mesh_lit: no scene"))?
            .env_tmds
            .get(s)
            .ok_or_else(|| JsValue::from_str(&format!("field_mesh_lit: slot {s} out of range")))?;
        let (mesh, flat, lit) = {
            let res = self
                .res()
                .ok_or_else(|| JsValue::from_str("field_mesh_lit: no resources"))?;
            let rtmd = res
                .tmds
                .get(res_idx)
                .ok_or_else(|| JsValue::from_str("field_mesh_lit: tmd missing"))?;
            let (mut mesh, mut flat, lit) =
                legaia_engine_core::scene_assembly::build_hybrid_env_mesh_lit(rtmd, &res.vram);
            if legaia_engine_core::field_lit_mesh::has_lit_rows(&lit) {
                let rot = legaia_engine_core::field_lit_mesh::draw_rotation(
                    rot_x as u16,
                    rot_y as u16,
                    rot_z as u16,
                );
                legaia_engine_core::field_lit_mesh::shade_lit_rows_rgba(
                    &mut mesh.colors,
                    &mut flat,
                    &lit,
                    &light,
                    &rot,
                );
            }
            legaia_engine_ui::scene_lighting::tag_emissive_hybrid(
                &rtmd.raw, &mut mesh, &flat, &res.vram,
            );
            (mesh, flat, lit)
        };
        let has_lit = legaia_engine_core::field_lit_mesh::has_lit_rows(&lit);
        if let Some(f) = self.field.as_mut() {
            // Not the plain build's cache key: a later `field_mesh(slot)`
            // must rebuild the unshaded stream.
            f.cur = Some(((usize::MAX, 0), mesh, flat));
        }
        Ok(has_lit)
    }

    /// Whether env-pack slot `slot` carries light-source rows - the page's
    /// "needs a per-rotation shaded copy" test.
    pub fn field_mesh_has_lit_rows(&self, slot: u32) -> bool {
        let Some(res_idx) = self
            .field
            .as_ref()
            .and_then(|f| f.env_tmds.get(slot as usize).copied())
        else {
            return false;
        };
        let Some(res) = self.res() else {
            return false;
        };
        res.tmds.get(res_idx).is_some_and(|rtmd| {
            let (_, lit) = rtmd.build_filtered_vram_mesh_lit_vertices(&res.vram);
            legaia_engine_core::field_lit_mesh::has_lit_rows(&lit)
        })
    }

    /// The live field light as one comparable key (angles + back colour) -
    /// the page re-shades its lit env copies when it changes (op `4C 8A`).
    pub fn field_light_key(&self) -> String {
        self.scene_host
            .as_ref()
            .map(|h| {
                let l = h.world.presentation.field_light;
                format!(
                    "{},{},{},{},{},{}",
                    l.angles[0], l.angles[1], l.angles[2], l.back[0], l.back[1], l.back[2]
                )
            })
            .unwrap_or_default()
    }

    pub fn field_mesh_positions(&self) -> Vec<f32> {
        let Some((_, m, _)) = self.field_cur() else {
            return Vec::new();
        };
        m.positions.iter().flatten().copied().collect()
    }

    /// Drain the env-pack slots whose VDF morph deltas changed since the
    /// last call (the engine world's ambient morph parts + the scene-entry
    /// pulse, ticked by the sim's ambient drain). The page re-uploads each
    /// slot's positions from [`Self::field_morph_positions`] - the play
    /// page's side of the `FUN_8001C604` render substitution.
    pub fn field_morph_slots(&mut self) -> Vec<u32> {
        let Some(host) = self.scene_host.as_mut() else {
            return Vec::new();
        };
        let mut slots: Vec<u32> = host
            .world
            .take_morph_dirty_slots()
            .into_iter()
            .map(|(s, _)| s as u32)
            .collect();
        slots.sort_unstable();
        slots.dedup();
        slots
    }

    /// Morphed vertex positions for env-pack slot `slot`: the plain
    /// (`anim 0`) hybrid mesh build with the live VDF deltas staged onto
    /// the TMD group vertices. Stream order matches the uploaded rest-pose
    /// mesh (the prim walk is position-independent); empty when no morph
    /// targets the slot.
    pub fn field_morph_positions(&mut self, slot: u32) -> Vec<f32> {
        let s = slot as usize;
        let Some(res_idx) = self.field.as_ref().and_then(|f| f.env_tmds.get(s).copied()) else {
            return Vec::new();
        };
        let Some(host) = self.scene_host.as_ref() else {
            return Vec::new();
        };
        let Some(res) = host.resources.as_ref() else {
            return Vec::new();
        };
        let Some(rtmd) = res.tmds.get(res_idx) else {
            return Vec::new();
        };
        let Some(m) = host.world.morphed_env_tmd(s, rtmd) else {
            return Vec::new();
        };
        let (mesh, _) = crate::field_scene::build_hybrid_env_mesh(&m, &res.vram);
        mesh.positions.iter().flatten().copied().collect()
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
            .map(|f| env_positions(&f.placements, &f.coplanar_offsets))
            .unwrap_or_default()
    }

    pub fn field_placement_rot_y(&self) -> Vec<u16> {
        self.field
            .as_ref()
            .map(|f| f.placements.iter().map(|d| d.rot_y).collect())
            .unwrap_or_default()
    }

    /// Per-placement uniform render scale (parallel to
    /// [`Self::field_placement_slots`]): the bind record's `actor[+0x72]` as a
    /// factor, applied after the rotation (`T * R * S`). `1.0` for nearly
    /// every placement; town01's horizon backdrop draws at `0.25`.
    /// Index of the first overworld **decoration** draw in the placement
    /// list (the landmarks come first): the page stages retail's per-object
    /// decoration depth cue on the draws from here on. Equal to the list's
    /// length off the overworld.
    pub fn field_decoration_start(&self) -> u32 {
        self.field
            .as_ref()
            .map(|f| f.decoration_start as u32)
            .unwrap_or(0)
    }

    pub fn field_placement_scales(&self) -> Vec<f32> {
        self.field
            .as_ref()
            .map(|f| f.placement_scales.clone())
            .unwrap_or_default()
    }

    /// Per-placement authored pitch (object record `+0x08`), parallel to
    /// [`Self::field_placement_rot_y`]. Composed with yaw and roll in
    /// retail's `Rx * Ry * Rz` order (`FUN_80026988`).
    pub fn field_placement_rot_x(&self) -> Vec<u16> {
        self.field
            .as_ref()
            .map(|f| f.placements.iter().map(|d| d.rot_x).collect())
            .unwrap_or_default()
    }

    /// Per-placement authored roll (object record `+0x0C`). See
    /// [`Self::field_placement_rot_x`].
    pub fn field_placement_rot_z(&self) -> Vec<u16> {
        self.field
            .as_ref()
            .map(|f| f.placements.iter().map(|d| d.rot_z).collect())
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

    /// Per-placement **scripted displacement** (parallel to
    /// [`Self::field_placement_slots`]), flattened `[dx, dy, dz]` in retail
    /// world units (Y-down): how far a script has moved the placed object's
    /// actor from its bind seat - an `A3` seat, an op-`4C 42` lift under the
    /// actor's `0x20000000` height law (`chitei2`'s falling boulder). Retail
    /// draws a placed object at its actor, so the page adds this to each
    /// draw's translation. **Empty** while no placement has moved, which is
    /// nearly every frame of nearly every scene. The same
    /// `World::object_draw_displacements` table the native play-window folds
    /// into its placed draws.
    pub fn field_placement_moves(&self) -> Vec<f32> {
        let (Some(f), Some(h)) = (self.field.as_ref(), self.scene_host.as_ref()) else {
            return Vec::new();
        };
        let moves = h.world.object_draw_displacements();
        if moves.is_empty() {
            return Vec::new();
        }
        let per = field_env::placed_draw_displacements(&f.placement_records, &moves);
        if per.iter().all(|d| *d == [0; 3]) {
            return Vec::new();
        }
        per.into_iter().flatten().map(|v| v as f32).collect()
    }

    /// Per-placement **draw tint** (parallel to
    /// [`Self::field_placement_slots`]), flattened `[r, g, b, ir0]`: the far
    /// colour (display `0..1`) and `IR0` (`1.0 = 0x1000`) of a constant
    /// per-draw depth cue, from the placed object's actor `+0x74` / `+0x78`
    /// (op `4C 81` - chitei2's hologram panels go black once the generator
    /// is down). `ir0 == 0` = untinted. **Empty** while no placement is
    /// tinted. The same `World::object_draw_tints` table the native
    /// play-window stages per placed draw.
    pub fn field_placement_tints(&self) -> Vec<f32> {
        let (Some(f), Some(h)) = (self.field.as_ref(), self.scene_host.as_ref()) else {
            return Vec::new();
        };
        let tints = h.world.object_draw_tints();
        if tints.is_empty() {
            return Vec::new();
        }
        let mut any = false;
        let mut out = Vec::with_capacity(f.placement_records.len() * 4);
        for r in &f.placement_records {
            match r.and_then(|r| tints.get(&r)) {
                Some(&(colour, blend)) => {
                    let (far, ir0) = legaia_engine_core::world::tint_cue(colour, blend);
                    out.extend_from_slice(&[far[0], far[1], far[2], ir0]);
                    any = true;
                }
                None => out.extend_from_slice(&[0.0; 4]),
            }
        }
        if any { out } else { Vec::new() }
    }

    /// Per-placement **live** mask (parallel to [`Self::field_placement_slots`]):
    /// `1` = draw it this frame, `0` = the placement is a sub-area window
    /// sweep's whose actor is not on the world's windowed static-object list.
    /// Every entry is `1` unless retail windowing is on
    /// ([`Self::set_retail_static_window`]). The same
    /// [`field_env::placed_draw_live`] kernel the native play-window's
    /// placed-object pass asks per draw.
    pub fn field_placement_live(&self) -> Vec<u8> {
        let (Some(f), Some(h)) = (self.field.as_ref(), self.scene_host.as_ref()) else {
            return Vec::new();
        };
        let window = &h.world.terrain.static_window;
        f.window_keys
            .iter()
            .map(|k| u8::from(field_env::placed_draw_live(k.as_ref(), window)))
            .collect()
    }

    /// Per-placement **actor-cull** mask (parallel to
    /// [`Self::field_placement_slots`]): `1` = the placed object's actor is
    /// culled this frame (outside the region box or the visible tile window
    /// widened by its record's cull radius - retail's `FUN_801D79E8`, which
    /// the actor draw walk honours), so the page skips it. **Empty** while
    /// the visible-tile crop does not apply (no crop = draw every placement).
    /// The same `field_view_window::placed_actor_visible` kernel the native
    /// play-window's placed-object pass asks per draw.
    pub fn field_placement_culled(&self, debug_camera: bool) -> Vec<u8> {
        let (Some(f), Some(h)) = (self.field.as_ref(), self.scene_host.as_ref()) else {
            return Vec::new();
        };
        let Some(cells) = self.field_view_cells_now(debug_camera) else {
            return Vec::new();
        };
        let moves = h.world.object_draw_displacements();
        let per = field_env::placed_draw_displacements(&f.placement_records, &moves);
        f.placements
            .iter()
            .zip(per)
            .map(|(d, m)| {
                u8::from(
                    !legaia_engine_core::field_view_window::placed_actor_visible(
                        &h.world,
                        Some(&cells),
                        d.world_x + m[0],
                        d.world_z + m[2],
                        d.cull_radius,
                    ),
                )
            })
            .collect()
    }

    /// A stamp that moves whenever [`Self::field_placement_culled`] can: the
    /// published cull view (focus, region box, window) and the crop's own
    /// stamp; `0` while no crop applies. The page re-reads the mask only when
    /// it moves.
    pub fn field_placement_cull_stamp(&self, debug_camera: bool) -> u32 {
        let Some(cells) = self.field_view_cells_now(debug_camera) else {
            return 0;
        };
        let Some(view) = self
            .scene_host
            .as_ref()
            .and_then(|h| h.world.npcs.cull_view)
        else {
            return 0;
        };
        let mut h: u32 = cells.stamp();
        for v in [
            view.focus_stored[0],
            view.focus_stored[1],
            i32::from_le_bytes(view.attr_box),
            i32::from_le_bytes(view.window.map(|b| b as u8)),
        ] {
            for b in v.to_le_bytes() {
                h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
            }
        }
        h.max(1)
    }

    /// Whether retail's placed-object near reject drops a placed draw whose
    /// origin sits at clip `w` = `origin_view_depth` under this frame's
    /// retail camera - the same `field_env::placed_origin_near_culled` kernel
    /// the native play-window's placed-object pass asks per draw.
    pub fn field_placed_near_culled(&self, origin_view_depth: f32) -> bool {
        field_env::placed_origin_near_culled(origin_view_depth)
    }

    /// A stamp that changes whenever [`Self::field_placement_live`] can: the
    /// windowed list's rebuild generation, with the retail-windowing flag in
    /// bit 0. The page re-reads the mask only when this moves.
    pub fn field_static_window_stamp(&self) -> u32 {
        self.scene_host.as_ref().map_or(0, |h| {
            let w = &h.world.terrain.static_window;
            (w.generation << 1) | u32::from(w.retail_windowing)
        })
    }

    /// Turn retail static-object windowing on / off (the
    /// `retail_static_window` option, persisted like the native config).
    pub fn set_retail_static_window(&mut self, on: bool) {
        if self.options_state.retail_static_window != on {
            self.options_state.retail_static_window = on;
            self.persist_and_apply_options();
        }
    }

    /// Whether retail static-object windowing is on.
    pub fn retail_static_window(&self) -> bool {
        self.options_state.retail_static_window
    }

    /// This frame's visible-tile cell rectangle through the shared
    /// `field_view_window::field_view_cells` kernel the native play-window
    /// asks: `None` = draw the map whole. `debug_camera` is the page's `F3`
    /// vantage, which (like the native window's) lifts the crop.
    fn field_view_cells_now(
        &self,
        debug_camera: bool,
    ) -> Option<legaia_engine_core::field_view_window::ViewCells> {
        let h = self.scene_host.as_ref()?;
        legaia_engine_core::field_view_window::field_view_cells(
            &h.world,
            legaia_engine_core::field_view_window::framing_is_retail(&self.camera) && !debug_camera,
        )
    }

    /// A stamp that moves whenever the visible-tile crop can change what
    /// [`Self::field_terrain_live`] and [`Self::field_ground_indices_cropped`]
    /// return; `0` while no crop applies. The page re-reads both only when it
    /// moves - the native window re-uploads its ground on the same stamp.
    pub fn field_view_window_stamp(&self, debug_camera: bool) -> u32 {
        self.field_view_cells_now(debug_camera)
            .map_or(0, |c| c.stamp())
    }

    /// Per-terrain-draw **live** mask (parallel to [`Self::field_terrain_slots`]):
    /// `1` = inside this frame's visible-tile crop
    /// (`field_view_window::terrain_draw_visible`, the gate the native
    /// window's terrain pass asks per draw). All `1` while no crop applies.
    pub fn field_terrain_live(&self, debug_camera: bool) -> Vec<u8> {
        let Some(f) = self.field.as_ref() else {
            return Vec::new();
        };
        let cells = self.field_view_cells_now(debug_camera);
        f.terrain
            .iter()
            .map(|d| {
                u8::from(legaia_engine_core::field_view_window::terrain_draw_visible(
                    cells.as_ref(),
                    legaia_engine_core::field_view_window::CellKey::of_draw(d),
                ))
            })
            .collect()
    }

    /// [`Self::field_ground_indices`] cropped to this frame's visible cells
    /// (`field_ground::crop_indices`, the kernel the native window re-uploads
    /// its ground through). The whole list while no crop applies.
    pub fn field_ground_indices_cropped(&self, debug_camera: bool) -> Vec<u32> {
        let Some(hf) = self.field.as_ref().and_then(|f| f.ground.as_ref()) else {
            return Vec::new();
        };
        let cells = self.field_view_cells_now(debug_camera);
        legaia_engine_core::field_ground::crop_indices(
            &hf.positions,
            &legaia_engine_core::field_ground::render_indices(hf),
            cells.as_ref(),
        )
    }

    /// Turn retail's visible-tile crop on / off (the `retail_view_window`
    /// option, persisted like the native config). It only applies at retail
    /// framing either way.
    pub fn set_retail_view_window(&mut self, on: bool) {
        if self.options_state.retail_view_window != on {
            self.options_state.retail_view_window = on;
            self.persist_and_apply_options();
        }
    }

    /// Whether retail's visible-tile crop is on.
    pub fn retail_view_window(&self) -> bool {
        self.options_state.retail_view_window
    }

    /// Live pose key of each placement (parallel to
    /// [`Self::field_placement_slots`]): `-1` for a static prop (no anim, or
    /// no live prop-bank entry), else the prop's
    /// [`PropPoseKey::to_i32`] (`PropAnimBank::pose_key` - the `actor+0x68`
    /// cursor the frame blender poses from, plus the clamp bit; `0` is the
    /// rest pose). The world advances every prop's cursor each field tick
    /// (`tick_prop_interactions` -> `PropAnimBank::tick_anims`, retail's
    /// `FUN_800204F8`), so an animated prop - the windmill sails, a swinging
    /// door mid-swing - reports a changing key, and the page re-poses it by
    /// handing the key back to [`Self::field_mesh_posed_frame_positions`].
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
                    .props
                    .bank
                    .pose_key(d.anchor)
                    .map(PropPoseKey::to_i32)
                    .unwrap_or(-1)
            })
            .collect()
    }

    /// Positions of environment-pack slot `slot` **posed at pose key
    /// `frame`** ([`Self::field_placement_frames`]' value) of scene ANM record
    /// `anim_id - 1` - the per-frame re-pose the draw walker (`FUN_8001B964`)
    /// does off a placed prop's live cursor.
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
        let key = PropPoseKey::from_i32(frame as i32);
        let Some(offsets) = self.frame_bone_offsets(anim, res_idx, key) else {
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
            .map(|f| env_positions(&f.terrain, &f.coplanar_offsets))
            .unwrap_or_default()
    }

    /// Per-draw **floor-wave** Y offsets for the static map, terrain draws
    /// first then placement draws - the two lists `field_terrain_positions()`
    /// and `field_placement_positions()` return, concatenated in that order.
    ///
    /// The scene's sixteen-rung floor-height ladder is script-animated: op
    /// `0x4C` nibble-9 sub-`0..2` sets a rung oscillating every frame
    /// (`FUN_801DDE34` -> `FUN_801DA930`), which is the travelling wave under
    /// `jou`'s organic interior floor. The page bakes its draw positions once
    /// per scene, so this is what moves the drawn ground with the walk
    /// heightfield instead of leaving it at the disc-static tier.
    ///
    /// **Empty** whenever the live ladder equals the baked one, which is every
    /// frame of every scene whose script leaves it alone - the page skips the
    /// whole pass on an empty return.
    pub fn field_floor_wave_offsets(&self) -> Vec<f32> {
        let Some(f) = self.field.as_ref() else {
            return Vec::new();
        };
        let Some(h) = self.scene_host.as_ref() else {
            return Vec::new();
        };
        // The terrain / decoration cells follow the live rungs; a placed
        // object stands on the ladder its actor was spawned against
        // (`World::placed_floor_offsets`).
        let wave = field_env::FloorWave::from_scene_and_world(
            f.floor_lut,
            &h.world.terrain.floor_height_lut,
        );
        let placed = h.world.placed_floor_offsets(
            f.floor_lut,
            f.placements.iter().map(|d| &d.floor),
            &f.window_keys,
        );
        if wave.is_none() && placed.iter().all(|&o| o == 0) {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(f.terrain.len() + f.placements.len());
        out.extend(
            f.terrain
                .iter()
                .map(|d| wave.map_or(0, |w| w.offset(&d.floor)) as f32),
        );
        out.extend(placed.into_iter().map(|o| o as f32));
        out
    }

    pub fn field_terrain_rot_y(&self) -> Vec<u16> {
        self.field
            .as_ref()
            .map(|f| f.terrain.iter().map(|d| d.rot_y).collect())
            .unwrap_or_default()
    }

    /// The walk-ground heightfield's drawn positions, flattened - through the
    /// shared [`legaia_engine_core::field_ground::render_positions`] kernel
    /// (the `GROUND_SINK`) the native window's ground mesh also runs.
    pub fn field_ground_positions(&self) -> Vec<f32> {
        let Some(hf) = self.field.as_ref().and_then(|f| f.ground.as_ref()) else {
            return Vec::new();
        };
        legaia_engine_core::field_ground::render_positions(hf)
            .into_iter()
            .flatten()
            .collect()
    }

    /// The walk-ground heightfield's drawn positions re-resolved through the
    /// world's **live** floor-height ladder
    /// ([`legaia_engine_core::field_ground::live_render_positions`]), when it
    /// has moved since the last call - empty when nothing changed, so the
    /// page re-uploads only on a frame the ladder actually moved. Field VM op
    /// `0x4C` nibble 9 animates the ladder (`jouina`'s pulsing path,
    /// `concnow`'s flesh pits), and the floor sampler the player stands on
    /// reads the same ladder, so the drawn ground and the collision surface
    /// stay one shape. The native window re-uploads through the same kernel.
    pub fn field_ground_live_positions(&mut self) -> Vec<f32> {
        let Some(live) = self
            .scene_host
            .as_ref()
            .map(|h| h.world.terrain.floor_height_lut)
        else {
            return Vec::new();
        };
        let Some(f) = self.field.as_mut() else {
            return Vec::new();
        };
        let Some(hf) = f.ground.as_ref() else {
            return Vec::new();
        };
        if f.is_world_map || f.ground_lut_applied == Some(live) {
            return Vec::new();
        }
        f.ground_lut_applied = Some(live);
        legaia_engine_core::field_ground::live_render_positions(hf, &live)
            .into_iter()
            .flatten()
            .collect()
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

    /// The walk-ground heightfield's drawn triangles, through the shared
    /// [`legaia_engine_core::field_ground::render_indices`] kernel: reversed
    /// onto the scene TMDs' winding, so the cutscene camera's NCLIP pass keeps
    /// the ground on this page as it does in the native window.
    pub fn field_ground_indices(&self) -> Vec<u32> {
        self.field
            .as_ref()
            .and_then(|f| f.ground.as_ref())
            .map(legaia_engine_core::field_ground::render_indices)
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
    ///
    /// The vertices come back at the player's render scale
    /// (`World::player_render_scale`, `+0x72` - `0xC00` on a kingdom map),
    /// the native window's actor-matrix scale: the page translates them to
    /// the player's position, so scaling about the local origin is the same
    /// composition.
    pub fn player_mesh_positions(&mut self) -> Vec<f32> {
        let scale = self
            .scene_host
            .as_ref()
            .map_or(1.0, |h| h.world.player_render_scale());
        let mut out = self.player_mesh_positions_unscaled();
        if scale != 1.0 {
            for v in &mut out {
                *v *= scale;
            }
        }
        out
    }

    fn player_mesh_positions_unscaled(&mut self) -> Vec<f32> {
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

    /// Camera-occlusion fade **visibility gate**: is the player completely
    /// hidden from `eye` by the static scene geometry? Ray-casts the
    /// 5-point body cross of `engine_core::field_occlusion` (the same
    /// kernel the native play-window runs) against the scene's occluder
    /// set. `eye_*` are RAW retail Y-down world coordinates - the page's
    /// draw frame is Y-up, so `play-app.js` negates its `_eye()` Y on the
    /// way in. `false` when no scene / player / occluders exist, so a
    /// partially visible character (or an unprovable one) never fades.
    pub fn field_player_occluded(&self, eye_x: f32, eye_y: f32, eye_z: f32) -> bool {
        let Some(f) = self.field.as_ref() else {
            return false;
        };
        let Some(h) = self.scene_host.as_ref() else {
            return false;
        };
        // Body centre: the one kernel the native gate samples
        // (`field_occlusion::player_body_centre` - the floor tier under the
        // actor lifted half a character height). This page used to rebuild
        // it here with its own copy of the half-height constant.
        let Some(centre) = legaia_engine_core::field_occlusion::player_body_centre(&h.world) else {
            return false;
        };
        f.occluders.fully_occluded([eye_x, eye_y, eye_z], centre)
    }

    // ---------------------------------------------------------------- NPCs
    //
    // Every answer below is `crate::field_actors::FieldActors`'s, the one
    // actor layer the map viewer runs too.

    fn actor_banks(&self) -> crate::field_actors::ActorBanks<'_> {
        crate::field_actors::ActorBanks {
            scene_anm: self.scene_anm.as_ref(),
            locomotion_anm: self.locomotion_anm.as_ref(),
        }
    }

    /// The scene's NPC / actor catalog. Shape:
    /// `{"anm_prot": 4, "npcs": [{"i", "slot", "model", "anim", "nobj",
    /// "kind", "target_map", "dialog", "conditional", "special", "x", "z"},
    /// ...]}`. `null` before a scene is entered.
    pub fn play_npc_catalog_json(&self) -> String {
        self.actors.catalog_json()
    }

    /// The object count catalog entry `i`'s mesh is cut to right now (the
    /// live clip's bone count), or `-1` for an uncut mesh. The page re-builds
    /// an NPC's mesh when this moves.
    pub fn play_npc_mesh_cut(&self, i: u32) -> i32 {
        let Some(h) = self.scene_host.as_ref() else {
            return -1;
        };
        self.actors.mesh_cut(h, self.actor_banks(), i)
    }

    /// Per catalog entry, the generation of its op-`0x4B` VDF morph (`-1` =
    /// never armed); the page re-reads [`Self::play_npc_morph_base`] when it
    /// moves.
    pub fn play_npc_morph_states(&self) -> Vec<i32> {
        self.actors.morph_states()
    }

    /// Catalog entry `i`'s object-local base positions with its live morph
    /// staged (`World::npc_morphed_tmd`), in [`Self::play_npc_mesh_positions`]'
    /// vertex order.
    pub fn play_npc_morph_base(&self, i: u32) -> Vec<f32> {
        let Some(h) = self.scene_host.as_ref() else {
            return Vec::new();
        };
        self.actors.morph_base(h, self.actor_banks(), i)
    }

    /// Build catalog entry `i`'s mesh (hybrid: textured + vertex-colour prims,
    /// with per-vertex bone ids). Returns `i`. A special (`model >= 0xF0`)
    /// resolves out of the world's global TMD pool, and a clip-bound mesh is
    /// cut to the clip's bone count, as the native window's field-NPC bind.
    pub fn play_npc_mesh(&mut self, i: u32) -> Result<u32, JsValue> {
        let Some(h) = self.scene_host.as_ref() else {
            return Err(JsValue::from_str("play_npc_mesh: no scene"));
        };
        let banks = crate::field_actors::ActorBanks {
            scene_anm: self.scene_anm.as_ref(),
            locomotion_anm: self.locomotion_anm.as_ref(),
        };
        self.actors
            .build_mesh(h, banks, i)
            .map_err(|e| JsValue::from_str(&e))?;
        Ok(i)
    }

    /// The live model id the scripted-motion VM's op `0x0E` re-bound catalog
    /// entry `i`'s actor to, or `-1` while it still draws its spawn mesh. The
    /// page re-uploads the mesh when the answer moves.
    pub fn play_npc_live_model(&self, i: u32) -> i32 {
        self.scene_host
            .as_ref()
            .map_or(-1, |h| self.actors.live_model(h, i))
    }

    pub fn play_npc_mesh_positions(&self) -> Vec<f32> {
        self.actors.mesh_positions()
    }

    pub fn play_npc_mesh_uvs(&self) -> Vec<u8> {
        self.actors.mesh_uvs()
    }

    pub fn play_npc_mesh_cba_tsb(&self) -> Vec<u16> {
        self.actors.mesh_cba_tsb()
    }

    pub fn play_npc_mesh_indices(&self) -> Vec<u32> {
        self.actors.mesh_indices()
    }

    /// Per-vertex TMD object index for the built NPC mesh - the bone each
    /// vertex hangs from.
    pub fn play_npc_mesh_object_ids(&self) -> Vec<u32> {
        self.actors.mesh_object_ids()
    }

    pub fn play_npc_mesh_flat_rgba(&self) -> Vec<u8> {
        self.actors.mesh_flat_rgba()
    }

    /// Catalog entry `i`'s spawn clip, 6 `i32` per bone per frame
    /// (`[tx, ty, tz, rx, ry, rz]`, absolute); empty with no clip.
    pub fn play_npc_pose_frames(&self, i: u32) -> Vec<i32> {
        let Some(h) = self.scene_host.as_ref() else {
            return Vec::new();
        };
        self.actors.pose_frames(h, self.actor_banks(), i)
    }

    /// `[frame_count, bone_count]` of catalog entry `i`'s clip; `[0, 0]` when
    /// it has none.
    pub fn play_npc_pose_dims(&self, i: u32) -> Vec<u32> {
        let Some(h) = self.scene_host.as_ref() else {
            return vec![0, 0];
        };
        self.actors.pose_dims(h, self.actor_banks(), i)
    }

    /// The off-map hide-box coordinate (`FIELD_OFFMAP_HIDE_XZ`): the page
    /// skips drawing any NPC whose live position is this tile on both axes,
    /// exactly as the native play-window's draw pass does.
    pub fn field_offmap_hide_xz(&self) -> i32 {
        legaia_engine_core::world::FIELD_OFFMAP_HIDE_XZ as i32
    }

    /// Live clip-playback state of every catalogued NPC, `[pose, generation,
    /// ...]` (`[-1, -1]` with no live clip player).
    pub fn play_npc_clip_states(&self) -> Vec<i32> {
        self.actors.clip_states()
    }

    /// Current pose of catalog entry `i`'s live clip, 6 `i32` per bone, read
    /// without advancing the playhead (it moves only in
    /// [`LegaiaRuntime::tick_frame`]).
    pub fn play_npc_live_bones(&self, i: u32) -> Vec<i32> {
        self.actors.live_bones(i)
    }

    /// Live world state of every catalogued NPC, `[x, y, z, facing_units,
    /// ...]` in catalog order (world positions, the render-scale-zero hide,
    /// the floor / scripted-arc height).
    pub fn play_npc_transforms(&self) -> Vec<f32> {
        self.scene_host
            .as_ref()
            .map(|h| self.actors.transforms(h))
            .unwrap_or_default()
    }

    /// Live `(pitch, roll)` of every catalogued NPC, `[pitch, roll, ...]`
    /// (`World::field_npc_tilt`; `(0, 0)` for an untilted actor).
    pub fn play_npc_tilts(&self) -> Vec<f32> {
        self.scene_host
            .as_ref()
            .map(|h| self.actors.tilts(h))
            .unwrap_or_default()
    }

    /// Per catalogued NPC, the op-`4C 81` **draw tint** as a constant
    /// per-draw cue, `[r, g, b, ir0, ...]` (far colour in display `0..1`,
    /// `ir0` in `1.0 = 0x1000` units; `ir0 == 0` = untinted). **Empty** while
    /// no NPC is tinted. The same `World::field_npc_draw_tint` the native
    /// play-window stages on its NPC draws.
    pub fn play_npc_tints(&self) -> Vec<f32> {
        self.scene_host
            .as_ref()
            .map(|h| self.actors.tints(h))
            .unwrap_or_default()
    }

    /// The player's op-`4C 81` draw tint, `[r, g, b, ir0]`, or empty while
    /// the player draws untinted (`World::player_draw_tint`, the native
    /// window's player cue).
    pub fn play_player_tint(&self) -> Vec<f32> {
        self.scene_host
            .as_ref()
            .and_then(|h| h.world.player_draw_tint())
            .map(|(colour, blend)| {
                let (far, ir0) = legaia_engine_core::world::tint_cue(colour, blend);
                vec![far[0], far[1], far[2], ir0]
            })
            .unwrap_or_default()
    }
}

/// Flatten `EnvDraw` world positions to `[x, y, z, ...]`.
/// Flatten draw translations, adding each draw's cross-draw coplanar lift
/// (see [`FieldRender::coplanar_offsets`]) - the same per-draw offset the
/// field-scene viewer's `field_scene_*_positions` accessors apply.
fn env_positions(
    draws: &[EnvDraw],
    offsets: &std::collections::HashMap<EnvDraw, [f32; 3]>,
) -> Vec<f32> {
    let mut out = Vec::with_capacity(draws.len() * 3);
    for d in draws {
        let off = offsets.get(d).copied().unwrap_or([0.0; 3]);
        out.push(d.world_x as f32 + off[0]);
        out.push(d.world_y as f32 + off[1]);
        out.push(d.world_z as f32 + off[2]);
    }
    out
}

/// A script-spawned actor's mesh staged for the page's upload: the hybrid
/// textured + colour mesh, its per-vertex bone ids and the packet-colour
/// stream ([`crate::packet_color::hybrid`]).
pub(crate) struct StagedActorMesh {
    pub mesh: legaia_tmd::mesh::VramMesh,
    pub object_ids: Vec<u32>,
    pub flat: Vec<u8>,
}

/// Script-spawned actors (field-VM `0x4C 0xD8`, `FieldEvent::ActorSpawned`)
/// - the browser twin of the native window's `pending_dynamic_mesh_slots`
/// drain in its redraw pass. An actor a scene script spawns with a TMD
/// reference is not in the MAN placement catalog the NPC layer draws from,
/// so the page used to give it no geometry at all. The page drains the
/// slots once per frame, uploads each slot's mesh at its rest pose, and
/// draws them from the live actor transforms.
#[wasm_bindgen]
impl LegaiaRuntime {
    /// Drain the actor slots that gained a TMD reference since the last
    /// call. Upload each through [`Self::play_dynamic_actor_mesh`].
    pub fn play_take_dynamic_mesh_slots(&mut self) -> Vec<u32> {
        let pending = std::mem::take(&mut self.pending_dynamic_mesh_slots);
        pending
            .into_iter()
            .filter(|s| !self.dynamic_mesh_slots.contains(s))
            .map(u32::from)
            .collect()
    }

    /// Actor slots carrying a live morph-weight blend (the same `0x4C 0xD8`
    /// allocator, seated by `World::spawn_morph_weight_actor`). The page
    /// re-stages each one through [`Self::play_dynamic_actor_mesh`] every
    /// frame, exactly as the native window re-uploads it in its redraw pass:
    /// retail re-blends inside the handler call, and the envelope is a
    /// ping-pong ramp that moves on every frame, so no upload stays current.
    pub fn play_morph_weight_slots(&self) -> Vec<u32> {
        self.scene_host
            .as_ref()
            .map(|h| {
                h.world
                    .morph_weight_actor_weights()
                    .into_iter()
                    .map(|(slot, _)| u32::from(slot))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Stage actor `slot`'s spawned mesh (its `tmd_ref` from the global
    /// pool) for the `play_dynamic_mesh_*` reads. `false` when the slot
    /// carries no drawable mesh.
    ///
    /// A morph-weight actor stages its **blended** mesh instead - through
    /// `World::morph_weight_posed_tmd`, the one engine-side kernel the
    /// native window poses from too, so the blend itself lives on neither
    /// host.
    pub fn play_dynamic_actor_mesh(&mut self, slot: u32) -> bool {
        let Ok(slot) = u8::try_from(slot) else {
            return false;
        };
        let posed = self
            .scene_host
            .as_ref()
            .and_then(|h| h.world.morph_weight_posed_tmd(slot as usize));
        let Some(gtmd) = self
            .scene_host
            .as_ref()
            .and_then(|h| h.world.actors.get(slot as usize))
            .and_then(|a| a.tmd_ref.as_ref().map(std::sync::Arc::clone))
        else {
            return false;
        };
        let (mesh, object_ids, shading) = match posed.as_ref() {
            Some((tmd, raw, _)) => legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(tmd, raw),
            None => legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&gtmd.tmd, &gtmd.raw),
        };
        if mesh.indices.is_empty() {
            return false;
        }
        let flat = crate::packet_color::hybrid(&mesh, &shading);
        self.dynamic_mesh_cur = Some(StagedActorMesh {
            mesh,
            object_ids,
            flat,
        });
        if !self.dynamic_mesh_slots.contains(&slot) {
            self.dynamic_mesh_slots.push(slot);
        }
        true
    }

    pub fn play_dynamic_mesh_positions(&self) -> Vec<f32> {
        self.dynamic_mesh_cur
            .as_ref()
            .map(|m| m.mesh.positions.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    pub fn play_dynamic_mesh_uvs(&self) -> Vec<u8> {
        self.dynamic_mesh_cur
            .as_ref()
            .map(|m| m.mesh.uvs.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    pub fn play_dynamic_mesh_cba_tsb(&self) -> Vec<u16> {
        self.dynamic_mesh_cur
            .as_ref()
            .map(|m| m.mesh.cba_tsb.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    pub fn play_dynamic_mesh_indices(&self) -> Vec<u32> {
        self.dynamic_mesh_cur
            .as_ref()
            .map(|m| m.mesh.indices.clone())
            .unwrap_or_default()
    }

    pub fn play_dynamic_mesh_flat_rgba(&self) -> Vec<u8> {
        self.dynamic_mesh_cur
            .as_ref()
            .map(|m| m.flat.clone())
            .unwrap_or_default()
    }

    /// Per-vertex TMD object index of the staged dynamic mesh.
    pub fn play_dynamic_mesh_object_ids(&self) -> Vec<u32> {
        self.dynamic_mesh_cur
            .as_ref()
            .map(|m| m.object_ids.clone())
            .unwrap_or_default()
    }

    /// Live transforms of every uploaded dynamic actor, `6 x f32` per entry:
    /// `[slot, x, y, z, facing_12bit, active]` in retail world units (the
    /// page negates Y like every other actor draw). `active` is `0` for a
    /// despawned / hidden slot - skip the draw. Heading follows the NPC
    /// heading map when the slot has one, else identity (`facing = 2048`,
    /// the native `None => Mat4::IDENTITY` arm).
    pub fn play_dynamic_actor_transforms(&self) -> Vec<f32> {
        let Some(h) = self.scene_host.as_ref() else {
            return Vec::new();
        };
        let hide = legaia_engine_core::world::FIELD_OFFMAP_HIDE_XZ;
        let mut out = Vec::with_capacity(self.dynamic_mesh_slots.len() * 6);
        for &slot in &self.dynamic_mesh_slots {
            let Some(a) = h.world.actors.get(slot as usize) else {
                continue;
            };
            let (x, y, z) = (
                a.move_state.world_x,
                a.move_state.world_y,
                a.move_state.world_z,
            );
            let active = a.active && a.tmd_ref.is_some() && !(x == hide && z == hide);
            let facing = h.world.npcs.headings.get(&slot).copied().unwrap_or(2048) as f32;
            out.extend_from_slice(&[
                f32::from(slot),
                x as f32,
                y as f32,
                z as f32,
                facing,
                if active { 1.0 } else { 0.0 },
            ]);
        }
        out
    }
}

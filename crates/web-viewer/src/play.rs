//! Browser **play** surface: the render half of [`crate::runtime::LegaiaRuntime`].
//!
//! [`crate::runtime`] owns the simulation (a real
//! [`legaia_engine_scene_host::scene_host::SceneHost`]: field VM, locomotion + collision,
//! NPC motion, dialogue). This module is what the page draws: the assembled
//! static map, the posed player, and the scene's posed NPCs - all resolved
//! against the **same** [`legaia_engine_scene::scene_resources::SceneResources`]
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
use legaia_engine_scene::field_env::{self, EnvDraw, PropPoseKey};
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_engine_scene::scene_resources::SceneResources;

mod dynamic_actor_mesh;
mod field_mesh;
mod npc_exports;
mod placements;
mod player_mesh;
mod terrain_ground;

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
    /// static-object list through [`legaia_engine_world_views::scene_staging::placed_draw_live`] - the kernel
    /// the native play-window asks per frame.
    pub window_keys: Vec<Option<field_env::PlacedWindowKey>>,
    /// Per-placement bind record (parallel to [`Self::placements`], `None` =
    /// unbound): the key [`LegaiaRuntime::field_placement_live_models`] looks
    /// a script's live object turn and displacement up by.
    pub placement_records: Vec<Option<usize>>,
    /// Bulk terrain-tile draws (ground / decor tiles). `FLAG_PLACED` records
    /// are excluded - they are already drawn, posed, by the placement layer
    /// (the native window's `resolve_field_terrain_draws` rule).
    pub terrain: Vec<EnvDraw>,
    /// Walk-ground heightfield surface, when the scene has a resolvable floor.
    pub ground: Option<legaia_scene_formats::field_objects::WalkHeightfield>,
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
    pub occluders: legaia_engine_scene::field_occlusion::FieldOccluders,
    /// Cross-draw coplanar lifts (`engine_scene::coplanar_draws`), applied to
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
    /// ([`legaia_engine_field::overworld_ground_cue::decoration_draw_cue`]),
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
/// [`FieldClipPlayer`]: legaia_engine_field::field_anim::FieldClipPlayer
pub(crate) struct NpcClip {
    pub player: legaia_engine_field::field_anim::FieldClipPlayer,
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
) -> Vec<legaia_scene_formats::field_objects::Placement> {
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
                .filter(|p| p.flags & legaia_scene_formats::field_objects::FLAG_PLACED == 0)
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
        legaia_engine_scene::field_occlusion::FieldOccluders::build(&[&terrain, &placements], res);
    // Cross-draw coplanar lifts over the combined layers (terrain first, then
    // placements - the same concatenation the native shell and the field-scene
    // viewer rank, so all three hosts lift the same draws).
    let mut combined: Vec<EnvDraw> = Vec::with_capacity(terrain.len() + placements.len());
    combined.extend_from_slice(&terrain);
    combined.extend_from_slice(&placements);
    let planes = legaia_engine_scene::coplanar_draws::draw_plane_summaries(&combined, res);
    let coplanar_offsets =
        legaia_engine_scene::coplanar_draws::coplanar_draw_offsets(&combined, &planes);
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
        self.scene_host.host()?.resources.as_ref()
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

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
use legaia_engine_core::field_env::EnvDraw;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::scene_resources::SceneResources;
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
    /// For world-map scenes this is the whole sparse mesh layer: the walk
    /// `.MAP`'s placed landmarks followed by its decoration cells (trees /
    /// mountain groups / props), concatenated in that order to match the
    /// native play-window's `resolve_world_map_terrain_draws`.
    pub placements: Vec<EnvDraw>,
    /// Bulk terrain-tile draws (`CELL_VISIBLE`; ground / decor tiles).
    /// Empty for world-map scenes (their ground is the heightfield).
    pub terrain: Vec<EnvDraw>,
    /// Walk-ground heightfield surface (`None` when the scene has no
    /// resolvable `.MAP` floor grid / floor LUT).
    pub ground: Option<legaia_asset::field_objects::WalkHeightfield>,
    /// Cross-draw coplanar lifts (`legaia_engine_core::coplanar_draws`) for
    /// the combined terrain + placement lists, applied by the position
    /// exporters so overlapping same-plane tiles resolve deterministically
    /// instead of z-fighting (mirrors the native play-window).
    pub coplanar_offsets: std::collections::HashMap<EnvDraw, [f32; 3]>,
    /// Currently-selected env-pack slot + its built mesh + the parallel
    /// per-vertex flat-colour array (see [`build_hybrid_env_mesh`]), cached
    /// so the positions/uvs/cba_tsb/indices accessors don't rebuild per call.
    pub cur: Option<(usize, legaia_tmd::mesh::VramMesh, Vec<u8>)>,
    /// Live VRAM animation state ([`LegaiaViewer::field_scene_anim_init`]):
    /// the bundle's type-6 CLUT-walk table + the scene-entry ambient move-VM
    /// tree. `None` until initialised (or when the scene has neither).
    pub anim: Option<FieldSceneAnim>,
}

/// Per-scene VRAM animation runner for the browser viewer - the two field
/// mechanisms that mutate VRAM frame-over-frame (see
/// `docs/subsystems/field-ambient-fx.md`):
///
/// 1. the bundle type-6 **CLUT-walk table** (`legaia_asset::clut_walk`,
///    `FUN_8001ada4` case 0xB - water / waterfall shimmer, 12 carriers), and
/// 2. the **ambient move-VM effect tree** (`World::spawn_ambient_record` -
///    jou's pulsating-flesh palette cyclers + lightning).
///
/// Both run on the retail game-tick clock: a game tick every
/// [`Self::frame_step`] vsyncs (`DAT_1F800393`; 2 in towns, 3 on the
/// overworld).
pub struct FieldSceneAnim {
    /// Parsed walker table + per-entry `(accumulator, frame_index)` state.
    walker: Option<(legaia_asset::clut_walk::ClutWalkTable, Vec<(u32, usize)>)>,
    /// Ambient move-VM world (only the effect subsystem is used).
    ambient: Option<Box<legaia_engine_core::world::World>>,
    /// Vsyncs per game tick (retail `DAT_1F800393`).
    frame_step: u8,
    /// Vsyncs banked toward the next game tick.
    vsync_accum: u8,
}

/// Hybrid env-mesh builders - hoisted to the shared assembly kernel
/// (`engine-core::scene_assembly`) so the browser pages and the native
/// `export-glb` path bake identical meshes; re-exported under the old path
/// for the crate's other pages.
pub use legaia_engine_core::scene_assembly::{build_hybrid_env_mesh, build_hybrid_env_mesh_posed};

/// Assemble a CDNAME scene's full static map: field-mode
/// [`SceneResources`] (VRAM + env TMD pack) + the `.MAP` placement /
/// terrain-tile draws resolved through [`field_env`] + the walk-ground
/// heightfield. The engine-parity core of [`LegaiaViewer::set_scene_field`].
pub fn build_field_scene(index: &ProtIndex, name: &str) -> Result<FieldScenePack, String> {
    let legaia_engine_core::scene_assembly::AssembledScene {
        name,
        res,
        env_tmds,
        placements,
        terrain,
        ground,
        coplanar_offsets,
    } = legaia_engine_core::scene_assembly::assemble_field_scene(index, name)?;
    Ok(FieldScenePack {
        name,
        res,
        env_tmds,
        placements,
        terrain,
        ground,
        coplanar_offsets,
        cur: None,
        anim: None,
    })
}

/// Build the VRAM animation state for a loaded field scene: parse the
/// bundle's type-6 walker table (parking its source strips into the pack's
/// VRAM) and stage the scene's ambient move-VM effect tree. Returns `None`
/// when the scene has neither animation source.
pub fn build_field_scene_anim(
    index: &ProtIndex,
    pack: &mut FieldScenePack,
) -> Option<FieldSceneAnim> {
    let scene = Scene::load(index, &pack.name).ok()?;
    let is_world_map = legaia_engine_core::scene::is_world_map_scene(&pack.name);
    let frame_step: u8 = if is_world_map { 3 } else { 2 };

    // Walker table (any scene bundle; kingdom bundles resolve through the
    // same by-type path).
    let mut walker = None;
    for entry in &scene.entries {
        let Ok(table) = legaia_asset::clut_walk::from_scene_bundle(&entry.bytes) else {
            continue;
        };
        for s in legaia_asset::clut_walk::scene_park_strips(&entry.bytes) {
            pack.res.vram.write_block(s.fb_x, s.fb_y, s.w, s.h, &s.data);
        }
        let state = vec![(legaia_asset::clut_walk::ACCUMULATOR_SEED, 0usize); table.entries.len()];
        walker = Some((table, state));
        break;
    }

    // Ambient move-VM tree: prescript stagers + the MAN P1 effect-script
    // installs (field scenes only; the overworld has no ambient tree).
    let mut ambient: Option<Box<legaia_engine_core::world::World>> = None;
    if !is_world_map && let Some(scripts) = scene.find_event_scripts() {
        let stager_bytes = scripts.bytes.to_vec();
        if let Ok(Some(man_bytes)) = scene.field_man_payload(index)
            && let Ok(man_file) = legaia_asset::man_section::parse(&man_bytes)
        {
            // Same census the native scene host uses (retail `FUN_8003A1E4`'s
            // placement spawn-prologue slice) - keep the two in step, or the
            // viewer and the play page animate different scenes.
            let installs = legaia_engine_core::man_field_scripts::scene_entry_ambient_installs(
                &man_file, &man_bytes,
            );
            if !installs.is_empty() {
                let mut world = Box::new(legaia_engine_core::world::World::default());
                world.frame_step = frame_step;
                world.install_field_stagers(&stager_bytes);
                // VDF buffer before the spawn: flag-gated installer records
                // resolve morph lanes at spawn-run.
                world.set_vdf_buffer(legaia_engine_core::scene_bundle::find_vdf_buffer(&scene));
                for arg in installs {
                    world.spawn_ambient_record(arg as usize + 1, [0, 0, 0]);
                }
                // Scene-entry VDF pulse (enhancement, `engine-core::vdf_pulse`):
                // jou's flesh-ground morph pack moves at plain entry; scenes
                // whose stager table arms morphs itself keep retail behaviour
                // (the installer self-guards).
                let pack_objects: Vec<Vec<usize>> = pack
                    .env_tmds
                    .iter()
                    .map(|&ti| {
                        pack.res.tmds[ti]
                            .tmd
                            .objects
                            .iter()
                            .map(|o| o.vertices.len())
                            .collect()
                    })
                    .collect();
                world.install_entry_vdf_pulse(&pack_objects);
                if !world.ambient_fx.is_empty() {
                    ambient = Some(world);
                }
            }
        }
    }

    if walker.is_none() && ambient.is_none() {
        return None;
    }
    Some(FieldSceneAnim {
        walker,
        ambient,
        frame_step,
        vsync_accum: 0,
    })
}

impl FieldSceneAnim {
    /// Walker-only animation state for the **play** runtime
    /// ([`crate::runtime::LegaiaRuntime`]): there the live scene host's own
    /// `World` carries the ambient move-VM tree (spawned at scene entry and
    /// drained per sim tick), so only the CLUT-walk half runs here.
    pub(crate) fn walker_only(
        table: legaia_asset::clut_walk::ClutWalkTable,
        frame_step: u8,
    ) -> FieldSceneAnim {
        let state = vec![(legaia_asset::clut_walk::ACCUMULATOR_SEED, 0usize); table.entries.len()];
        FieldSceneAnim {
            walker: Some((table, state)),
            ambient: None,
            frame_step,
            vsync_accum: 0,
        }
    }

    /// Advance `vsyncs` retail vsyncs and apply any due VRAM writes to
    /// `vram`. Returns `true` when texels changed.
    pub fn tick(&mut self, vsyncs: u32, vram: &mut legaia_tim::Vram) -> bool {
        let dt = u32::from(self.frame_step.max(1));
        let mut game_ticks = 0u32;
        for _ in 0..vsyncs.min(64) {
            self.vsync_accum += 1;
            if u32::from(self.vsync_accum) >= dt {
                self.vsync_accum = 0;
                game_ticks += 1;
            }
        }
        if game_ticks == 0 {
            return false;
        }
        let mut wrote = false;
        // Walker entries: acc += dt per game tick; on crossing the frame's
        // hold, MoveImage the 16x1 source cell in and reset (the retail
        // FUN_8001ada4 case-0xB law, same as the play-window animator).
        if let Some((table, state)) = self.walker.as_mut() {
            for _ in 0..game_ticks {
                for (entry, (acc, idx)) in table.entries.iter().zip(state.iter_mut()) {
                    *acc += dt;
                    let frame = &entry.frames[*idx];
                    if *acc < u32::from(frame.hold_vsyncs) {
                        continue;
                    }
                    *acc = 0;
                    vram.move_image(
                        frame.src_x,
                        frame.src_y,
                        legaia_asset::clut_walk::COPY_WIDTH,
                        1,
                        entry.dest_x,
                        entry.dest_y,
                    );
                    *idx = (*idx + 1) % entry.frames.len();
                    wrote = true;
                }
            }
        }
        // Ambient move-VM tree: bank the game ticks and drain against VRAM.
        if let Some(world) = self.ambient.as_mut() {
            world.ambient_pending_game_ticks += game_ticks;
            if world.step_ambient_fx(vram) {
                wrote = true;
            }
        }
        wrote
    }

    /// One-line status for the UI: walker entry count + live ambient parts.
    pub fn status(&self) -> (usize, usize) {
        (
            self.walker
                .as_ref()
                .map(|(t, _)| t.entries.len())
                .unwrap_or(0),
            self.ambient
                .as_ref()
                .map(|w| w.ambient_fx.len())
                .unwrap_or(0),
        )
    }
}

impl LegaiaViewer {
    /// Build (and cache) the engine-core [`ProtIndex`] over the loaded disc.
    /// After `load_disc`, `self.disc` holds the extracted PROT.DAT bytes and
    /// `self.cdname_text` the CDNAME.TXT captured from the full image (raw
    /// PROT.DAT loads have no CDNAME - scene names then can't resolve and
    /// `set_scene_field` errors).
    pub(crate) fn ensure_prot_index(&mut self) -> Result<Arc<ProtIndex>, String> {
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
        self.field_npcs = None;
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

    /// Select the active environment-pack slot and build its mesh: the
    /// textured prims whose pages/CLUTs are resident in the field VRAM
    /// (matches the engine's per-prim filter) **plus** the untextured
    /// `F*`/`G*` vertex-colour prims, merged by [`build_hybrid_env_mesh`]
    /// (the engine-shell's colour-mesh pipeline sibling). Returns the slot,
    /// or an error when out of range. Subsequent `field_scene_mesh_*` calls
    /// read the built mesh.
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
        if f.cur.as_ref().map(|(cs, _, _)| *cs) != Some(s) {
            let (mesh, flat) = build_hybrid_env_mesh(&f.res.tmds[res_idx], &f.res.vram);
            f.cur = Some((s, mesh, flat));
        }
        Ok(slot)
    }

    pub fn field_scene_mesh_positions(&self) -> Vec<f32> {
        let Some((_, mesh, _)) = self.field_scene.as_ref().and_then(|f| f.cur.as_ref()) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.positions.len() * 3);
        for p in &mesh.positions {
            out.extend_from_slice(p);
        }
        out
    }

    pub fn field_scene_mesh_uvs(&self) -> Vec<u8> {
        let Some((_, mesh, _)) = self.field_scene.as_ref().and_then(|f| f.cur.as_ref()) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.uvs.len() * 2);
        for uv in &mesh.uvs {
            out.extend_from_slice(uv);
        }
        out
    }

    pub fn field_scene_mesh_cba_tsb(&self) -> Vec<u16> {
        let Some((_, mesh, _)) = self.field_scene.as_ref().and_then(|f| f.cur.as_ref()) else {
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
            .map(|(_, m, _)| m.indices.clone())
            .unwrap_or_default()
    }

    /// Per-vertex `[r, g, b, flag]` bytes for the current mesh's hybrid
    /// flat-colour render (`flag` 255 = textured vertex, sample VRAM; 0 =
    /// untextured vertex, use the RGB). **Empty** when the mesh carries no
    /// untextured prims - the JS side then skips binding the attribute and
    /// the draw behaves exactly like the pure-textured path.
    pub fn field_scene_mesh_flat_rgba(&self) -> Vec<u8> {
        self.field_scene
            .as_ref()
            .and_then(|f| f.cur.as_ref())
            .map(|(_, _, flat)| flat.clone())
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

    /// Initialise the loaded field scene's VRAM animation: the bundle's
    /// type-6 CLUT-walk table (water / waterfall shimmer) + the scene-entry
    /// ambient move-VM effect tree (jou's pulsating flesh / lightning).
    /// Returns a JSON status `{"walker_entries", "ambient_parts"}`; both
    /// zero when the scene has no animation sources. Call once after
    /// `set_scene_field`; then drive [`Self::field_scene_anim_tick`] per
    /// rendered frame and re-upload the VRAM texture when it returns `true`.
    pub fn field_scene_anim_init(&mut self) -> Result<String, JsValue> {
        let index = self
            .ensure_prot_index()
            .map_err(|e| JsValue::from_str(&format!("field_scene_anim_init: {e}")))?;
        let Some(pack) = self.field_scene.as_mut() else {
            return Err(JsValue::from_str("field_scene_anim_init: no field scene"));
        };
        pack.anim = build_field_scene_anim(&index, pack);
        let (walker, ambient) = pack.anim.as_ref().map(|a| a.status()).unwrap_or((0, 0));
        Ok(format!(
            r#"{{"walker_entries":{walker},"ambient_parts":{ambient}}}"#
        ))
    }

    /// Advance the field scene's VRAM animation by `vsyncs` retail vsyncs
    /// (pass 1 per 60 Hz rendered frame). Returns `true` when VRAM texels
    /// changed - re-upload [`Self::field_scene_vram_bytes`] to the GPU then.
    pub fn field_scene_anim_tick(&mut self, vsyncs: u32) -> bool {
        let Some(pack) = self.field_scene.as_mut() else {
            return false;
        };
        let Some(anim) = pack.anim.as_mut() else {
            return false;
        };
        anim.tick(vsyncs, &mut pack.res.vram)
    }

    /// Drain the environment-pack slots whose VDF morph deltas changed
    /// since the last call (retail-armed ambient morph parts + the
    /// scene-entry pulse). For each returned slot the page re-uploads that
    /// mesh's positions from [`Self::field_scene_morph_positions`] - the
    /// browser side of the `FUN_8001C604` render substitution.
    pub fn field_scene_morph_slots(&mut self) -> Vec<u32> {
        let Some(world) = self
            .field_scene
            .as_mut()
            .and_then(|p| p.anim.as_mut())
            .and_then(|a| a.ambient.as_mut())
        else {
            return Vec::new();
        };
        let mut slots: Vec<u32> = world
            .take_morph_dirty_slots()
            .into_iter()
            .map(|(s, _)| s as u32)
            .collect();
        slots.sort_unstable();
        slots.dedup();
        slots
    }

    /// The morphed vertex-position stream for environment-pack slot `slot`:
    /// the same hybrid mesh build as [`Self::field_scene_mesh`] with the
    /// live VDF deltas staged onto the TMD's group vertices
    /// (`ResolvedTmd::with_group_deltas` - the rest pose is never
    /// mutated). The prim walk is position-independent, so the stream
    /// aligns 1:1 with the uploaded mesh; the page swaps positions only.
    /// Empty when no morph targets the slot.
    pub fn field_scene_morph_positions(&mut self, slot: u32) -> Vec<f32> {
        let Some(pack) = self.field_scene.as_mut() else {
            return Vec::new();
        };
        let Some(world) = pack.anim.as_mut().and_then(|a| a.ambient.as_mut()) else {
            return Vec::new();
        };
        let s = slot as usize;
        let Some(&res_idx) = pack.env_tmds.get(s) else {
            return Vec::new();
        };
        let rtmd = &pack.res.tmds[res_idx];
        let mut morphed: Option<legaia_engine_core::scene_resources::ResolvedTmd> = None;
        for (group, obj) in rtmd.tmd.objects.iter().enumerate() {
            if let Some(deltas) = world.current_morph_deltas(s, group as u32, obj.vertices.len()) {
                let base = morphed.take().unwrap_or_else(|| rtmd.clone());
                morphed = Some(base.with_group_deltas(group as u32, &deltas));
            }
        }
        let Some(m) = morphed else {
            return Vec::new();
        };
        let (mesh, _) = build_hybrid_env_mesh(&m, &pack.res.vram);
        mesh.positions.iter().flatten().copied().collect()
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
            let off = f.coplanar_offsets.get(d).copied().unwrap_or([0.0; 3]);
            out.push(d.world_x as f32 + off[0]);
            out.push(d.world_y as f32 + off[1]);
            out.push(d.world_z as f32 + off[2]);
        }
        out
    }

    /// Per-placement authored yaw (object record `+0x0A`), PSX angle units
    /// (`4096` = full revolution), in placement order. Convert with
    /// `rotY = -(rot & 0xFFF) * Math.PI / 2048` for `placementModelScaled*`.
    pub fn field_scene_placement_rot_y(&self) -> Vec<u16> {
        self.field_scene
            .as_ref()
            .map(|f| f.placements.iter().map(|d| d.rot_y).collect())
            .unwrap_or_default()
    }

    /// Per-placement authored pitch (object record `+0x08`), PSX angle units,
    /// in placement order - the sibling of [`Self::field_scene_placement_rot_y`]
    /// on the X axis.
    ///
    /// A placement carrying a nonzero `rot_x` or `rot_z` cannot go through the
    /// yaw-only `placementModelScaled*` path: that builder's negated-yaw
    /// convention is a cancellation specific to `Ry`. The page composes
    /// retail's `Rx * Ry * Rz` instead (`placementModelEuler`), which is what
    /// `legaia_engine_render::battle_intro::placement_rotation` applies on the
    /// native host. Not a rarity to skip: across the field corpus ~6% of
    /// placements tilt, and some scenes tilt every one of theirs.
    pub fn field_scene_placement_rot_x(&self) -> Vec<u16> {
        self.field_scene
            .as_ref()
            .map(|f| f.placements.iter().map(|d| d.rot_x).collect())
            .unwrap_or_default()
    }

    /// Per-placement authored roll (object record `+0x0C`); see
    /// [`Self::field_scene_placement_rot_x`].
    pub fn field_scene_placement_rot_z(&self) -> Vec<u16> {
        self.field_scene
            .as_ref()
            .map(|f| f.placements.iter().map(|d| d.rot_z).collect())
            .unwrap_or_default()
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
            let off = f.coplanar_offsets.get(d).copied().unwrap_or([0.0; 3]);
            out.push(d.world_x as f32 + off[0]);
            out.push(d.world_y as f32 + off[1]);
            out.push(d.world_z as f32 + off[2]);
        }
        out
    }

    /// Per-terrain-tile authored yaw, same encoding as
    /// [`Self::field_scene_placement_rot_y`].
    pub fn field_scene_terrain_rot_y(&self) -> Vec<u16> {
        self.field_scene
            .as_ref()
            .map(|f| f.terrain.iter().map(|d| d.rot_y).collect())
            .unwrap_or_default()
    }

    /// Per-terrain-tile authored pitch / roll, same encoding as
    /// [`Self::field_scene_placement_rot_x`]. The native shell composes all
    /// three angles for the terrain layer too (one `static_env_draws` list
    /// feeds one `placement_rotation`), so the layer is exported here rather
    /// than assumed flat.
    pub fn field_scene_terrain_rot_x(&self) -> Vec<u16> {
        self.field_scene
            .as_ref()
            .map(|f| f.terrain.iter().map(|d| d.rot_x).collect())
            .unwrap_or_default()
    }

    /// See [`Self::field_scene_terrain_rot_x`].
    pub fn field_scene_terrain_rot_z(&self) -> Vec<u16> {
        self.field_scene
            .as_ref()
            .map(|f| f.terrain.iter().map(|d| d.rot_z).collect())
            .unwrap_or_default()
    }

    /// Ground-heightfield accessors (same layout as the kingdom
    /// `walk_ground_*` family; empty when the scene has no resolvable floor
    /// grid).
    pub fn field_scene_ground_positions(&self) -> Vec<f32> {
        let Some(hf) = self.field_scene.as_ref().and_then(|f| f.ground.as_ref()) else {
            return Vec::new();
        };
        // Ground sinks below the env pack's authored floor art (see
        // `coplanar_draws::GROUND_SINK`).
        let mut out = Vec::with_capacity(hf.positions.len() * 3);
        for p in &hf.positions {
            out.push(p[0]);
            out.push(p[1] + legaia_engine_core::coplanar_draws::GROUND_SINK);
            out.push(p[2]);
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

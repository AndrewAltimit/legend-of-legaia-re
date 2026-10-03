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
    /// The bundle's type-6 CLUT-walk animator
    /// ([`LegaiaViewer::field_scene_anim_init`]). `None` until initialised
    /// (or when the scene has none).
    pub anim: Option<FieldSceneAnim>,
    /// The scene running live and headless
    /// ([`legaia_engine_core::scene_live::LiveScene`]): its world drives the
    /// floor-height ladder, the placed-prop clips, the ambient move-VM tree
    /// and the scripted VRAM effects exactly as the play hosts' world does.
    /// `None` until [`LegaiaViewer::field_scene_anim_init`] (and for the
    /// overworld, which is not previewed live).
    pub live: Option<Box<legaia_engine_core::scene_live::LiveScene>>,
    /// The scene's ANM bundle - the clips placed props pose from.
    pub scene_anm: Option<legaia_asset::player_anm::PlayerAnmBundle>,
    /// The live ladder the ground's drawn positions were last re-resolved
    /// against ([`LegaiaViewer::field_scene_ground_live_positions`]).
    pub ground_lut_applied: Option<[i16; 16]>,
    /// Per-placement windowed-list identity
    /// ([`legaia_engine_core::field_env::placed_window_key`]): which spawn
    /// sweep owns each placed object, and so which ladder it stands on.
    pub window_keys: Vec<Option<legaia_engine_core::field_env::PlacedWindowKey>>,
}

/// The bundle type-6 **CLUT-walk table** runner (`legaia_asset::clut_walk`,
/// `FUN_8001ada4` case 0xB - water / waterfall shimmer, 12 carriers; see
/// `docs/subsystems/field-ambient-fx.md`), shared by the map viewer and the
/// play runtime. It runs on the retail game-tick clock: a game tick every
/// [`legaia_engine_core::world::FrameClock::frame_step`] vsyncs
/// (`DAT_1F800393`; 2 in towns, 3 on the overworld).
///
/// The other VRAM writers - the ambient move-VM tree and the scripted CLUT
/// effects - are the world's, stepped through
/// `World::step_field_vram_effects` by whichever host owns the world (the
/// play runtime's scene host, the map viewer's
/// [`legaia_engine_core::scene_live::LiveScene`]).
pub struct FieldSceneAnim {
    /// The CLUT-walk shimmer (the slot-5 / type-6 walker or the legacy
    /// ocean-head fallback), stepped by the one engine kernel the native
    /// window runs (`legaia_engine_core::clut_walk_anim`).
    clut: Option<legaia_engine_core::clut_walk_anim::ClutWalkAnim>,
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
        live: None,
        scene_anm: None,
        ground_lut_applied: None,
        window_keys: Vec::new(),
    })
}

/// Build the CLUT-walk animator for a loaded field scene: parse the bundle's
/// type-6 walker table, parking its source strips into the pack's VRAM.
/// `None` when the scene has no walker.
pub fn build_field_scene_anim(
    index: &ProtIndex,
    pack: &mut FieldScenePack,
) -> Option<FieldSceneAnim> {
    let scene = Scene::load(index, &pack.name).ok()?;
    let is_world_map = legaia_engine_core::scene::is_world_map_scene(&pack.name);
    let frame_step: u8 = if is_world_map { 3 } else { 2 };
    // Resolved and parked by the engine kernel the play hosts call
    // (`ClutWalkAnim::install`).
    let clut = legaia_engine_core::clut_walk_anim::ClutWalkAnim::install(
        &scene,
        index,
        &mut pack.res.vram,
    )?
    .anim;
    Some(FieldSceneAnim {
        clut: Some(clut),
        frame_step,
        vsync_accum: 0,
    })
}

/// Enter the pack's scene live and headless
/// ([`legaia_engine_core::scene_live::LiveScene`]) and resolve the scene ANM
/// bundle its placed props pose from. The live scene is `None` for the
/// overworld (not previewed live) or a scene the host refuses.
pub fn build_field_scene_live(index: Arc<ProtIndex>, pack: &mut FieldScenePack) {
    let scene = Scene::load(&index, &pack.name).ok();
    pack.scene_anm = scene
        .as_ref()
        .and_then(legaia_engine_core::npc_catalog::scene_anm_bundle);
    let binds = scene
        .as_ref()
        .and_then(|s| s.field_object_binds(&index).ok().flatten());
    pack.window_keys = pack
        .placements
        .iter()
        .map(|d| legaia_engine_core::field_env::placed_window_key(d, binds.as_ref()))
        .collect();
    pack.ground_lut_applied = None;
    pack.live = legaia_engine_core::scene_live::LiveScene::enter(index, &pack.name)
        .map_err(|e| console_log(&format!("field scene {}: not live: {e}", pack.name)))
        .ok()
        .map(Box::new);
}

impl FieldScenePack {
    /// Per-draw Y offsets (retail frame, +Y down) of the terrain draws then
    /// the placement draws under the live floor-height ladder
    /// ([`legaia_engine_core::field_env::FloorWave`]). Empty while the
    /// ladder sits where the scene shipped it.
    ///
    /// The terrain / decoration cells follow the live rungs; a placed object
    /// stands on the ladder its actor was spawned against
    /// (`World::placed_floor_offsets`) - the play page's split.
    pub fn floor_wave_offsets(&self) -> Vec<f32> {
        let Some(live) = self.live.as_ref() else {
            return Vec::new();
        };
        let wave = live.floor_wave();
        let placed = live.host.world.placed_floor_offsets(
            live.scene_floor_lut(),
            self.placements.iter().map(|d| &d.floor),
            &self.window_keys,
        );
        if wave.is_none() && placed.iter().all(|&o| o == 0) {
            return Vec::new();
        }
        self.terrain
            .iter()
            .map(|d| wave.map_or(0, |w| w.offset(&d.floor)) as f32)
            .chain(placed.into_iter().map(|o| o as f32))
            .collect()
    }

    /// The ground's drawn positions under the live ladder, flattened, when
    /// it moved since the last call (empty otherwise).
    pub fn ground_live_positions(&mut self) -> Vec<f32> {
        let Some(live) = self.live.as_ref().map(|l| l.live_floor_lut()) else {
            return Vec::new();
        };
        let Some(hf) = self.ground.as_ref() else {
            return Vec::new();
        };
        if self.ground_lut_applied == Some(live) {
            return Vec::new();
        }
        self.ground_lut_applied = Some(live);
        legaia_engine_core::field_ground::live_render_positions(hf, &live)
            .into_iter()
            .flatten()
            .collect()
    }

    /// Per-placement live pose key (`PropPoseKey::to_i32`), `-1` for a static
    /// prop.
    pub fn placement_frames(&self) -> Vec<i32> {
        let live = self.live.as_ref();
        self.placements
            .iter()
            .map(|d| {
                live.and_then(|l| l.prop_pose_key(d))
                    .map_or(-1, |k| k.to_i32())
            })
            .collect()
    }
}

/// One retail vsync of the viewer's whole animation: the live scene's world
/// tick, the CLUT walker, and the world's VRAM effects (ambient move-VM tree,
/// scripted CLUT fx) - in the play runtime's per-sim-tick order. Returns
/// `true` when VRAM texels changed.
pub fn tick_field_scene_vsync(pack: &mut FieldScenePack) -> bool {
    let mut wrote = false;
    if let Some(live) = pack.live.as_mut() {
        live.tick();
    }
    if let Some(anim) = pack.anim.as_mut() {
        if let Some(live) = pack.live.as_ref() {
            anim.set_frame_step(live.host.world.clock.frame_step);
        }
        wrote |= anim.tick(1, &mut pack.res.vram);
    }
    if let Some(live) = pack.live.as_mut()
        && live.is_live()
    {
        wrote |= live
            .host
            .world
            .step_field_vram_effects(&mut pack.res.vram, false);
    }
    wrote
}

impl FieldSceneAnim {
    /// CLUT-walk-only animation state for the **play** runtime
    /// ([`crate::runtime::LegaiaRuntime`]): there the live scene host's own
    /// `World` carries the ambient move-VM tree (spawned at scene entry and
    /// drained per sim tick), so only the CLUT-walk half runs here.
    pub(crate) fn clut_only(
        clut: legaia_engine_core::clut_walk_anim::ClutWalkAnim,
        frame_step: u8,
    ) -> FieldSceneAnim {
        FieldSceneAnim {
            clut: Some(clut),
            frame_step,
            vsync_accum: 0,
        }
    }

    /// Which halves of the animator are installed, as a bitmask: bit 0 the
    /// CLUT walker, bit 1 the legacy ocean-head cycle. Read by
    /// `LegaiaRuntime::play_field_anim_kind`.
    pub(crate) fn kind_code(&self) -> u32 {
        use legaia_engine_core::clut_walk_anim::ClutWalkAnim;
        match self.clut {
            Some(ClutWalkAnim::Walk { .. }) => 1,
            Some(ClutWalkAnim::Ocean { .. }) => 2,
            None => 0,
        }
    }

    /// Re-point the animator at the world's **live** vsyncs-per-game-tick
    /// (`World::clock.frame_step`, retail `DAT_1F800393`).
    ///
    /// The play runtime snapshotted this at scene rebuild while the native
    /// window reads it off the world every frame. Today the two agree (the
    /// scene loader installs the per-mode floor and only the un-wired
    /// adaptive resolver `World::resolve_frame_step` can raise it), so this
    /// is the shape of the drift rather than a visible one - but a snapshot
    /// is exactly what makes wiring that resolver change one host only.
    pub fn set_frame_step(&mut self, frame_step: u8) {
        self.frame_step = frame_step.max(1);
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
        // The CLUT-walk shimmer, one engine step per game tick.
        if let Some(clut) = self.clut.as_mut() {
            for _ in 0..game_ticks {
                wrote |= clut.game_tick(dt, vram);
            }
        }
        wrote
    }

    /// CLUT-walk entry count, for the UI status line.
    pub fn walker_entries(&self) -> usize {
        match self.clut.as_ref() {
            Some(legaia_engine_core::clut_walk_anim::ClutWalkAnim::Walk { table, .. }) => {
                table.entries.len()
            }
            _ => 0,
        }
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

    /// Start the loaded field scene's animation: the bundle's type-6
    /// CLUT-walk table (water / waterfall shimmer) and the scene itself,
    /// entered live and headless through the engine's scene host
    /// ([`legaia_engine_core::scene_live::LiveScene`]) - whose world runs the
    /// scene's scripts, so the floor-height ladder, placed-prop clips,
    /// ambient move-VM tree and scripted VRAM effects move as they do on the
    /// play hosts. Returns a JSON status
    /// `{"walker_entries", "ambient_parts", "live"}`. Call once after
    /// `set_scene_field`, before the first VRAM upload (the walker parks its
    /// source strips into VRAM); then drive [`Self::field_scene_anim_tick`].
    pub fn field_scene_anim_init(&mut self) -> Result<String, JsValue> {
        let index = self
            .ensure_prot_index()
            .map_err(|e| JsValue::from_str(&format!("field_scene_anim_init: {e}")))?;
        let Some(pack) = self.field_scene.as_mut() else {
            return Err(JsValue::from_str("field_scene_anim_init: no field scene"));
        };
        pack.anim = build_field_scene_anim(&index, pack);
        build_field_scene_live(index, pack);
        let walker = pack.anim.as_ref().map_or(0, |a| a.walker_entries());
        let ambient = pack
            .live
            .as_ref()
            .map_or(0, |l| l.host.world.ambient.fx.len());
        let live = pack.live.is_some();
        Ok(format!(
            r#"{{"walker_entries":{walker},"ambient_parts":{ambient},"live":{live}}}"#
        ))
    }

    /// Advance the field scene by `vsyncs` retail vsyncs (pass the vsyncs of
    /// wall clock elapsed; capped at 64): one live-world tick, CLUT-walk step
    /// and VRAM-effect drain each ([`tick_field_scene_vsync`]). Returns `true`
    /// when VRAM texels changed - re-upload [`Self::field_scene_vram_bytes`]
    /// to the GPU then.
    pub fn field_scene_anim_tick(&mut self, vsyncs: u32) -> bool {
        let Some(pack) = self.field_scene.as_mut() else {
            return false;
        };
        let mut wrote = false;
        for _ in 0..vsyncs.min(64) {
            wrote |= tick_field_scene_vsync(pack);
        }
        wrote
    }

    /// Whether the scene is running live (see [`Self::field_scene_anim_init`]).
    pub fn field_scene_is_live(&self) -> bool {
        self.field_scene
            .as_ref()
            .and_then(|p| p.live.as_ref())
            .is_some_and(|l| l.is_live())
    }

    /// Drain the environment-pack slots whose VDF morph deltas changed
    /// since the last call (the live world's ambient morph parts + the
    /// scene-entry pulse). For each returned slot the page re-uploads that
    /// mesh's positions from [`Self::field_scene_morph_positions`] - the
    /// browser side of the `FUN_8001C604` render substitution, the same
    /// drain the play page reads.
    pub fn field_scene_morph_slots(&mut self) -> Vec<u32> {
        let Some(live) = self.field_scene.as_mut().and_then(|p| p.live.as_mut()) else {
            return Vec::new();
        };
        let mut slots: Vec<u32> = live
            .host
            .world
            .take_morph_dirty_slots()
            .into_iter()
            .map(|(s, _)| s as u32)
            .collect();
        slots.sort_unstable();
        slots.dedup();
        slots
    }

    /// The morphed vertex-position stream for environment-pack slot `slot`
    /// (`World::morphed_env_tmd` through the same hybrid mesh build as
    /// [`Self::field_scene_mesh`]). The prim walk is position-independent,
    /// so the stream aligns 1:1 with the uploaded mesh; the page swaps
    /// positions only. Empty when no morph targets the slot.
    pub fn field_scene_morph_positions(&mut self, slot: u32) -> Vec<f32> {
        let Some(pack) = self.field_scene.as_ref() else {
            return Vec::new();
        };
        let Some(live) = pack.live.as_ref() else {
            return Vec::new();
        };
        let s = slot as usize;
        let Some(&res_idx) = pack.env_tmds.get(s) else {
            return Vec::new();
        };
        let Some(m) = live.host.world.morphed_env_tmd(s, &pack.res.tmds[res_idx]) else {
            return Vec::new();
        };
        let (mesh, _) = build_hybrid_env_mesh(&m, &pack.res.vram);
        mesh.positions.iter().flatten().copied().collect()
    }

    /// Per-draw Y offsets (retail frame, +Y down) of the terrain draws then
    /// the placement draws, under the live floor-height ladder
    /// ([`legaia_engine_core::field_env::FloorWave`], the kernel the play
    /// page's `field_floor_wave_offsets` calls). Empty while the ladder sits
    /// where the scene shipped it.
    pub fn field_scene_floor_wave_offsets(&self) -> Vec<f32> {
        self.field_scene
            .as_ref()
            .map(|p| p.floor_wave_offsets())
            .unwrap_or_default()
    }

    /// The walk-ground heightfield's drawn positions under the live ladder
    /// ([`legaia_engine_core::field_ground::live_render_positions`], the play
    /// hosts' kernel), when the ladder moved since the last call - empty
    /// otherwise, so the page re-uploads only on a frame it changed.
    pub fn field_scene_ground_live_positions(&mut self) -> Vec<f32> {
        self.field_scene
            .as_mut()
            .map(|p| p.ground_live_positions())
            .unwrap_or_default()
    }

    /// Per-placement object-bind animation id (parallel to
    /// [`Self::field_scene_placement_slots`]): `0` = a static mesh; nonzero =
    /// a prop posed by scene ANM record `id - 1`, built through
    /// [`Self::field_scene_mesh_posed`].
    pub fn field_scene_placement_anim_ids(&self) -> Vec<u32> {
        self.field_scene
            .as_ref()
            .map(|f| f.placements.iter().map(|d| d.anim_id as u32).collect())
            .unwrap_or_default()
    }

    /// Live pose key of each placement (parallel to
    /// [`Self::field_scene_placement_slots`]): `-1` for a static prop, else
    /// the prop bank's pose key (`PropPoseKey::to_i32`) - the same value the
    /// play page's `field_placement_frames` reports, from the same
    /// world-ticked cursor (the windmill's sails turn).
    pub fn field_scene_placement_frames(&self) -> Vec<i32> {
        self.field_scene
            .as_ref()
            .map(|p| p.placement_frames())
            .unwrap_or_default()
    }

    /// Select + build env-pack slot `slot` **posed at frame 0** of scene ANM
    /// record `anim_id - 1` - the rest state of a placed prop whose bind
    /// names a clip (a multi-object mesh whose parts are the clip's bones:
    /// windmill sails on their hub, cupboard doors on the cabinet). Falls
    /// back to the raw mesh when the pose can't resolve (retail's
    /// count-equality contract, `field_env::posed_prop_offsets`). Subsequent
    /// `field_scene_mesh_*` calls read the built mesh.
    pub fn field_scene_mesh_posed(&mut self, slot: u32, anim_id: u32) -> Result<u32, JsValue> {
        let f = self
            .field_scene
            .as_mut()
            .ok_or_else(|| JsValue::from_str("field_scene_mesh_posed: no field scene loaded"))?;
        let s = slot as usize;
        let Some(&res_idx) = f.env_tmds.get(s) else {
            return Err(JsValue::from_str(&format!(
                "field_scene_mesh_posed: slot {s} >= count {}",
                f.env_tmds.len()
            )));
        };
        let rtmd = &f.res.tmds[res_idx];
        let offsets = f.scene_anm.as_ref().and_then(|b| {
            legaia_engine_core::field_env::posed_prop_offsets(
                b,
                anim_id.min(u8::MAX as u32) as u8,
                legaia_engine_core::field_env::PropPoseKey::REST,
                rtmd.tmd.objects.len(),
            )
        });
        let (mesh, flat) = match &offsets {
            Some(o) => build_hybrid_env_mesh_posed(rtmd, o),
            None => build_hybrid_env_mesh(rtmd, &f.res.vram),
        };
        // Keyed on a slot no plain build uses, so the next plain
        // `field_scene_mesh(slot)` rebuilds.
        f.cur = Some((usize::MAX, mesh, flat));
        Ok(slot)
    }

    /// Positions of env-pack slot `slot` posed at pose key `frame`
    /// ([`Self::field_scene_placement_frames`]' value) of scene ANM record
    /// `anim_id - 1` - same vertex order as [`Self::field_scene_mesh_posed`],
    /// so the page rewrites positions only. Empty when the pose can't
    /// resolve.
    pub fn field_scene_mesh_posed_frame_positions(
        &self,
        slot: u32,
        anim_id: u32,
        frame: u32,
    ) -> Vec<f32> {
        let Some(f) = self.field_scene.as_ref() else {
            return Vec::new();
        };
        let Some(&res_idx) = f.env_tmds.get(slot as usize) else {
            return Vec::new();
        };
        let rtmd = &f.res.tmds[res_idx];
        let Some(offsets) = f.scene_anm.as_ref().and_then(|b| {
            legaia_engine_core::field_env::posed_prop_offsets(
                b,
                anim_id.min(u8::MAX as u32) as u8,
                legaia_engine_core::field_env::PropPoseKey::from_i32(frame as i32),
                rtmd.tmd.objects.len(),
            )
        }) else {
            return Vec::new();
        };
        let (mesh, _) = build_hybrid_env_mesh_posed(rtmd, &offsets);
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
        // Ground sinks below the env pack's authored floor art, through the
        // same `field_ground` render kernel the play page and the native
        // window build their ground from.
        legaia_engine_core::field_ground::render_positions(hf)
            .into_iter()
            .flatten()
            .collect()
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

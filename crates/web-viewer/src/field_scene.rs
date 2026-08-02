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

/// Build one env-pack mesh the way the native play-window renders it: the
/// VRAM-filtered **textured** prims plus the untextured `F*`/`G*`
/// **vertex-colour** prims (`legaia_tmd::mesh::tmd_to_color_mesh` - the props
/// whose prims carry per-vertex RGB instead of UVs, which the textured
/// builder drops; the engine-shell draws them on its colour-mesh pipeline).
/// Both halves are merged into one vertex stream for the WebGL renderer, with
/// a parallel `[r, g, b, flag]` byte array (`flag` 255 = textured, sample
/// VRAM and modulate by the RGB; 0 = untextured, fill with the RGB) matching
/// the `u_use_flat_colors` / `a_flat_rgba` convention the field-character
/// hybrid shader path already implements. Every vertex carries a colour -
/// the textured half's is the prim's packet word, retail's whole field
/// lighting model (`texel * colour / 128`).
pub fn build_hybrid_env_mesh(
    rtmd: &legaia_engine_core::scene_resources::ResolvedTmd,
    vram: &legaia_tim::Vram,
) -> (legaia_tmd::mesh::VramMesh, Vec<u8>) {
    let mut mesh = rtmd.build_filtered_vram_mesh(vram);
    let mut cmesh = legaia_tmd::mesh::tmd_to_color_mesh(&rtmd.tmd, &rtmd.raw);
    // Coplanar z-fight resolution over BOTH halves as one stream - the same
    // shared kernel the native play-window runs (`legaia_tmd::mesh::coplanar`):
    // flag double-sided pairs for the shader's facing discard, nudge distinct
    // coplanar decal layers toward their visible side. The merge below carries
    // the colour half's pair flag onto the merged CBA attribute.
    legaia_tmd::mesh::resolve_hybrid(&mut mesh, &mut cmesh);
    merge_hybrid_halves(mesh, &cmesh)
}

/// [`build_hybrid_env_mesh`] **posed** at one set of per-object rigid
/// transforms (frame 0 of a placed prop's object-bind clip): both halves go
/// through the `*_posed_rot` builders - the same `R . v + T` bake the native
/// play-window's posed-placement pass runs - and merge into the same hybrid
/// stream. Used for the `.MAP` placed objects whose bind names an animation;
/// their TMD objects are that clip's bones, and the raw object-local vertices
/// are nonsense without its transform.
pub fn build_hybrid_env_mesh_posed(
    rtmd: &legaia_engine_core::scene_resources::ResolvedTmd,
    offsets: &[([i16; 3], [i16; 3])],
) -> (legaia_tmd::mesh::VramMesh, Vec<u8>) {
    let mut mesh = legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(&rtmd.tmd, &rtmd.raw, offsets);
    let mut cmesh = legaia_tmd::mesh::tmd_to_color_mesh_posed_rot(&rtmd.tmd, &rtmd.raw, offsets);
    legaia_tmd::mesh::resolve_hybrid(&mut mesh, &mut cmesh);
    merge_hybrid_halves(mesh, &cmesh)
}

/// Merge the untextured vertex-colour half into the textured half's vertex
/// stream, producing the parallel `[r, g, b, flag]` array. Both halves carry a
/// colour: the untextured half's is its **fill**, the textured half's is its
/// **modulation** (`texel * colour / 128`), and the `flag` byte is which.
///
/// The colour half's per-vertex PSX blend word (ABE enable in bit 15 + ABR
/// mode in bits 5..=6, see [`legaia_tmd::mesh::ColorMesh::blend`]) is carried
/// into the TSB half of the merged `cba_tsb` attribute - the same packing the
/// textured prims ride ([`legaia_tmd::mesh::TSB_SEMI_TRANSPARENT_BIT`]).
/// Dropping it forced every untextured semi-transparent prim (water sheets,
/// window light shafts) to draw opaque in the WebGL viewers. The flat-colour
/// shader path never samples VRAM for these verts, so the nonzero TSB is
/// blend metadata only.
fn merge_hybrid_halves(
    mut mesh: legaia_tmd::mesh::VramMesh,
    cmesh: &legaia_tmd::mesh::ColorMesh,
) -> (legaia_tmd::mesh::VramMesh, Vec<u8>) {
    // The textured half's stream is its packet colours (the shader's
    // `texel * colour / 128` modulation), NOT white - a white stream would
    // brighten every textured surface by 255/128.
    if cmesh.is_empty() {
        let flat = crate::packet_color::textured(&mesh);
        return (mesh, flat);
    }
    let mut flat = crate::packet_color::textured(&mesh);
    flat.reserve(cmesh.positions.len() * 4);
    let base = mesh.positions.len() as u32;
    for ((p, c), blend) in cmesh
        .positions
        .iter()
        .zip(cmesh.colors.iter())
        .zip(cmesh.blend.iter())
    {
        mesh.positions.push(*p);
        mesh.uvs.push([0, 0]);
        // A colour vert flagged as one copy of a double-sided pair
        // (`resolve_hybrid`, blend bit 14) re-keys the flag onto the merged
        // CBA attribute's bit 15 - where the WebGL shader's facing discard
        // reads it for every vertex, textured or flat.
        let cba = if blend & legaia_tmd::mesh::BLEND_DOUBLE_SIDED_BIT != 0 {
            legaia_tmd::mesh::CBA_DOUBLE_SIDED_BIT
        } else {
            0
        };
        mesh.cba_tsb
            .push([cba, blend & !legaia_tmd::mesh::BLEND_DOUBLE_SIDED_BIT]);
        mesh.normals.push([0.0, 0.0, 0.0]);
        // Keep `VramMesh::colors` index-aligned with the positions: the
        // untextured half has no modulation word, so it takes the neutral one.
        mesh.colors.push([crate::packet_color::NEUTRAL; 3]);
        flat.extend_from_slice(&[c[0], c[1], c[2], 0]);
    }
    mesh.indices.extend(cmesh.indices.iter().map(|i| i + base));
    (mesh, flat)
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
    // Boot-resident system-UI bundle (raw PROT TOC entries 0/1): layers
    // under the scene build so the row-510 strip CLUT + (960,256)
    // menu-glyph atlas the env meshes sample (town01 slots 21/26/74,
    // rikuroa slots 50/51/63) are resident in the browser build too.
    let system_ui = index.system_ui_bundle().ok();
    let (res, _stats) = SceneResources::build_targeted_with_options(
        &scene,
        &shared_refs,
        BuildOptions {
            kind,
            // Retail's field loader DMA-uploads every scene TIM; the
            // render-targeted subset drops ~75% of the env pack's prims.
            upload_all_tims: true,
            system_ui: system_ui.as_deref(),
        },
    )
    .map_err(|e| format!("{e:#}"))?;

    let env_tmds = field_env::env_pack_tmd_indices(&scene, &res);
    let floor_lut = scene.field_floor_height_lut(index).ok().flatten();
    // World-map scenes draw two sparse walk-frame layers over the continent
    // heightfield; field/town scenes draw the placed objects + the bulk
    // terrain-tile layer (mirrors the play-window's resolve_field_* /
    // resolve_world_map_* split in `engine-shell`).
    let (placement_records, terrain_records) = if is_world_map {
        // The walk `.MAP` carries the placed-flag landmarks (`FUN_8003A55C`,
        // flags & 0x4) AND a **decoration** layer - walk-visible cells with a
        // nonzero record `+0x10` and no placed flag: the crossed-quad trees,
        // mountain groups and props (~300 cells on Drake alone). Both are one
        // un-posed layer, concatenated landmarks-then-decorations exactly as
        // `resolve_world_map_terrain_draws` in `engine-shell` does; dropping
        // the second half here left the browser's assembled world maps bare
        // next to the native window. The continent ground itself is NOT this
        // layer - it is the heightfield below.
        let mut tiles = scene
            .walk_object_placements(index)
            .ok()
            .flatten()
            .unwrap_or_default();
        if let Ok(Some(deco)) = scene.walk_decoration_placements(index) {
            tiles.extend(deco);
        }
        (tiles, Vec::new())
    } else {
        (
            scene
                .field_object_placements(index)
                .ok()
                .flatten()
                .unwrap_or_default(),
            // Retail's per-cell terrain emitters (`FUN_801F69D8` /
            // `FUN_801F7088`) skip records carrying the *placed* flag - those
            // are the placement sweep's actors, drawn (and posed) above. The
            // two layers also resolve Y differently (corner average vs single
            // nibble), so a duplicate here would land at a different height.
            scene
                .field_terrain_tiles(index)
                .ok()
                .flatten()
                .unwrap_or_default()
                .into_iter()
                .filter(|p| p.flags & legaia_asset::field_objects::FLAG_PLACED == 0)
                .collect(),
        )
    };
    let (placements, _) = field_env::resolve_env_draws(&env_tmds, &placement_records, floor_lut);
    let (terrain, _) = field_env::resolve_env_draws(&env_tmds, &terrain_records, floor_lut);
    let ground = scene
        .walk_heightfield(index)
        .ok()
        .flatten()
        .filter(|h| !h.indices.is_empty());

    // Cross-draw coplanar lifts over the combined layers (terrain first,
    // then placements - the same concatenation the native shell ranks).
    let mut combined: Vec<EnvDraw> = Vec::with_capacity(terrain.len() + placements.len());
    combined.extend_from_slice(&terrain);
    combined.extend_from_slice(&placements);
    let planes = legaia_engine_core::coplanar_draws::draw_plane_summaries(&combined, &res);
    let coplanar_offsets =
        legaia_engine_core::coplanar_draws::coplanar_draw_offsets(&combined, &planes);

    Ok(FieldScenePack {
        name: name.to_string(),
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

#[cfg(test)]
mod tests {
    use super::merge_hybrid_halves;
    use legaia_tmd::mesh::{ColorMesh, TSB_SEMI_TRANSPARENT_BIT, VramMesh};

    /// The colour half's blend words must survive the hybrid merge in the
    /// TSB half of `cba_tsb` - an untextured ABE prim (fountain water /
    /// window light shaft) that loses its blend word draws as an opaque
    /// grey blob in the WebGL viewers.
    #[test]
    fn hybrid_merge_keeps_colour_half_blend_words() {
        let mesh = VramMesh {
            positions: vec![[0.0; 3]; 3],
            uvs: vec![[0, 0]; 3],
            cba_tsb: vec![[7, 0x1234]; 3],
            indices: vec![0, 1, 2],
            normals: vec![[0.0; 3]; 3],
            colors: vec![[128; 3]; 3],
        };
        let semi = TSB_SEMI_TRANSPARENT_BIT; // ABE on, ABR 0
        let cmesh = ColorMesh {
            positions: vec![[1.0; 3]; 3],
            colors: vec![[10, 20, 30]; 3],
            indices: vec![0, 1, 2],
            blend: vec![semi; 3],
        };
        let (merged, flat) = merge_hybrid_halves(mesh, &cmesh);
        assert_eq!(merged.positions.len(), 6);
        assert_eq!(flat.len(), 24);
        // Textured prefix untouched, colour tail flagged 0 with its blend
        // word in the TSB half.
        assert_eq!(merged.cba_tsb[0], [7, 0x1234]);
        for v in 3..6 {
            assert_eq!(merged.cba_tsb[v], [0, semi], "vert {v}");
            assert_eq!(flat[v * 4 + 3], 0, "vert {v} flag");
        }
    }
}

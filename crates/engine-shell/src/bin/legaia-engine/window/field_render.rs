//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;

impl PlayWindowApp {
    /// Resolve the field static-geometry placement draws for the current
    /// scene: each placed environment object's scene-pack mesh paired with a
    /// world model matrix. Built from the field map's object table
    /// (`Scene::field_object_placements`) and the scene_asset_table TMD pack;
    /// the per-object pack index resolves via `legaia_asset::field_objects`.
    ///
    /// `tmd_src_index[j]` is the `res.tmds` index of uploaded mesh `j` (meshes
    /// skip empty-prim TMDs, so this bridges back). Returns empty for scenes
    /// with no field map / no bundle (e.g. battle or world-map blocks).
    ///
    /// World Y is left at the ground plane for now; the per-tile floor-height
    /// LUT (MAN header) is a separate refinement.
    pub(super) fn resolve_field_placement_draws(
        &self,
        res: &SceneResources,
        tmd_src_index: &[usize],
    ) -> Vec<(usize, Mat4)> {
        let Some(scene) = self.session.host.scene.as_ref() else {
            return Vec::new();
        };
        let placements = match scene.field_object_placements(&self.session.host.index) {
            Ok(Some(p)) if !p.is_empty() => p,
            _ => return Vec::new(),
        };
        // Field frame: raw retail-convention transforms (the camera's
        // FIELD_WORLD_FLIP provides the single net Y negation).
        self.resolve_placement_draws(res, tmd_src_index, &placements, false)
    }

    /// Resolve the field scene's **terrain / ground** tiles (the `CELL_VISIBLE`
    /// sweep in `Scene::field_terrain_tiles`) to `(mesh, model)` draws, the same
    /// way `resolve_field_placement_draws` resolves the placed objects. This is
    /// the town's floor / ground layer; without it a field scene renders its
    /// buildings floating over the bare clear colour.
    pub(super) fn resolve_field_terrain_draws(
        &self,
        res: &SceneResources,
        tmd_src_index: &[usize],
    ) -> Vec<(usize, Mat4)> {
        let Some(scene) = self.session.host.scene.as_ref() else {
            return Vec::new();
        };
        let tiles = match scene.field_terrain_tiles(&self.session.host.index) {
            Ok(Some(t)) if !t.is_empty() => t,
            _ => return Vec::new(),
        };
        // Field frame: raw retail-convention transforms (see above).
        self.resolve_placement_draws(res, tmd_src_index, &tiles, false)
    }

    /// World-map continent terrain draws: the dense visible-tile set
    /// (`Scene::field_terrain_tiles`, the `FUN_801F69D8` overhead sweep) rather
    /// than the placed-flag interactive objects. Tiles whose pack index falls
    /// outside the loaded slot-1 landmark pack (they reference the wider global
    /// TMD pool, not yet loaded for the world map) resolve to no mesh and are
    /// skipped by `resolve_placement_draws`.
    pub(super) fn resolve_world_map_terrain_draws(
        &self,
        res: &SceneResources,
        tmd_src_index: &[usize],
    ) -> Vec<(usize, Mat4)> {
        let Some(scene) = self.session.host.scene.as_ref() else {
            return Vec::new();
        };
        // Free-roam walk view: read the *walk* `.MAP` (`Scene::walk_field_map_
        // index`, the `block_start - 2` entry the runtime resolves through
        // `toc[idx+2]`) and sweep its `0x1000`-gated continent (`walk_terrain_
        // tiles`), then the placed-flag landmarks from the same `.MAP`. The
        // earlier path read the within-block decoy entry with the overhead
        // `0x2000` gate, which for the kingdoms resolved a different map and
        // produced the sparse mesh scatter.
        // Two sparse pack-mesh layers on top of the heightfield ground:
        // the placed landmarks (FUN_8003A55C, flags & 0x4) and the
        // decoration layer (walk-visible cells with a nonzero record[+0x10]
        // and no placed flag - the crossed-quad trees, mountain groups, and
        // props). The bulk continent ground is NOT per-cell pack meshes (the
        // old `walk_terrain_tiles` sweep floods 97% of cells with pool-5
        // because their record[+0x10] is 0); it is the heightfield surface
        // built separately in `upload_assets` (`Scene::walk_heightfield`).
        // See docs/subsystems/world-map.md.
        let mut tiles = match scene.walk_object_placements(&self.session.host.index) {
            Ok(Some(t)) => t,
            _ => Vec::new(),
        };
        if let Ok(Some(deco)) = scene.walk_decoration_placements(&self.session.host.index) {
            tiles.extend(deco);
        }
        if tiles.is_empty() {
            return Vec::new();
        }
        // World-map frame: raw retail-convention transforms - both world-map
        // cameras compose FIELD_WORLD_FLIP (the walk view through the pinned
        // retail composition), so the draws are unflipped like the field's.
        self.resolve_placement_draws(res, tmd_src_index, &tiles, false)
    }

    /// Resolve the world-map water/CLUT-cell animation for the active scene.
    ///
    /// Retail path: the kingdom bundle's slot 5 (the type-byte `0x06` slot of
    /// PROT 0085 / 0244 / 0391) is the CLUT-walk animation table - eight
    /// independent 16x1 `MoveImage` walkers (ocean head + shoreline/terrain
    /// shimmer cells), parsed by [`legaia_asset::clut_walk`]. Parsing it also
    /// parks the walkers' VRAM source strips into the CPU VRAM (see
    /// [`Self::park_clut_walk_strips`]): the strips ship in the bundle's
    /// slot-0 TIM_LIST as raw CLUT-block records without the TIM magic, so
    /// the scene TIM pre-pass skips them.
    ///
    /// Fallback: the legacy single-cell 13-frame ocean-head cycle
    /// ([`legaia_asset::ocean::find_ocean_assets`]), used ONLY when no scene
    /// entry yields a parseable slot-5 table. The fallback exists for
    /// modified / damaged bundles (every retail kingdom ships slot 5): it
    /// keeps the most visible water shimmer alive rather than freezing the
    /// sea, at the cost of the seven non-ocean shimmer cells.
    // REF: FUN_8001f05c - asset-type dispatch case 6 installs the decoded
    // slot-5 table at DAT_8007B7C8; FUN_801d6704 (field init) spawns one
    // walker actor per entry via FUN_80024cfc.
    pub(super) fn resolve_ocean_anim(&mut self) -> Option<WaterAnim> {
        let scene = self.session.host.scene.as_ref()?;
        if !legaia_engine_core::scene::is_world_map_scene(&scene.name) {
            return None;
        }
        let mut walk: Option<legaia_asset::clut_walk::ClutWalkTable> = None;
        let mut strips: Vec<legaia_asset::clut_walk::ParkStrip> = Vec::new();
        for entry in &scene.entries {
            let Ok(table) = legaia_asset::clut_walk::from_kingdom_entry(&entry.bytes) else {
                continue;
            };
            if let Ok(slot0) = legaia_asset::kingdom_bundle::decode_slot(&entry.bytes, 0) {
                strips = legaia_asset::clut_walk::park_strips(&slot0);
            }
            walk = Some(table);
            break;
        }
        if let Some(table) = walk {
            self.park_clut_walk_strips(&table, strips);
            let state =
                vec![(legaia_asset::clut_walk::ACCUMULATOR_SEED, 0usize); table.entries.len()];
            return Some(WaterAnim::Walk(ClutWalkAnim {
                table,
                state,
                vsyncs_to_game_tick: 0,
            }));
        }
        // Slot 5 absent/unparseable: legacy ocean-head fallback.
        for entry in &scene.entries {
            let Ok(slot0) = legaia_asset::kingdom_bundle::decode_slot(&entry.bytes, 0) else {
                continue;
            };
            if let Some(ocean) = legaia_asset::ocean::find_ocean_assets(&slot0)
                && ocean.animation_frames.len() >= 32
            {
                log::warn!(
                    "play-window: no slot-5 CLUT-walk table in the kingdom bundle; \
                     falling back to the legacy ocean-head cycle"
                );
                return Some(WaterAnim::Ocean(OceanAnim {
                    frames: ocean.animation_frames,
                    cur: 0,
                    vsyncs_to_game_tick: 0,
                    vsync_accum: 0,
                }));
            }
        }
        None
    }

    /// Park the CLUT-walk source strips into the CPU VRAM and verify every
    /// walk source cell is populated.
    ///
    /// Two layers, mirroring what the retail VRAM actually holds:
    ///
    /// 1. The scene bundle's own slot-0 CLUT-block records (`strips`) - the
    ///    retail loader `LoadImage`s these verbatim; the engine's TIM
    ///    pre-pass skips them because they carry no TIM magic.
    /// 2. The **Drake complement**: map02 / map03 park only rows
    ///    `{501, 503, 505}` while the (kingdom-invariant) walk table also
    ///    sources rows 498/502/504. Retail inherits those rows as VRAM
    ///    residue from the Drake kingdom's upload - map01 is always the
    ///    first world map, and resident Sebucus / Karisto captures hold
    ///    map01's record bytes on those rows byte-exact - so the engine
    ///    parks the byte-identical records straight from the Drake bundle
    ///    (PROT entry 85) for any source row the scene's own bundle
    ///    doesn't cover.
    ///
    /// Any source cell still unpopulated after both layers is a real
    /// residency gap: reported loudly, never papered over.
    fn park_clut_walk_strips(
        &mut self,
        table: &legaia_asset::clut_walk::ClutWalkTable,
        strips: Vec<legaia_asset::clut_walk::ParkStrip>,
    ) {
        /// Drake kingdom bundle (`map01`), the shared source of the
        /// kingdom-invariant strip rows.
        const DRAKE_KINGDOM_BUNDLE_ENTRY: u32 = 85;
        let Some(base) = self.cpu_vram_base.as_mut() else {
            return;
        };
        for s in &strips {
            base.write_block(s.fb_x, s.fb_y, s.w, s.h, &s.data);
        }
        let covered = |base: &legaia_tim::Vram| {
            let mut missing: Vec<(u16, u16)> = Vec::new();
            for e in &table.entries {
                for f in &e.frames {
                    if !base.region_has_data(
                        f.src_x as usize,
                        f.src_y as usize,
                        legaia_asset::clut_walk::COPY_WIDTH as usize,
                        1,
                    ) && !missing.contains(&(f.src_x, f.src_y))
                    {
                        missing.push((f.src_x, f.src_y));
                    }
                }
            }
            missing
        };
        let mut missing = covered(base);
        if !missing.is_empty() {
            if let Ok(drake) = self
                .session
                .host
                .index
                .entry_bytes_extended(DRAKE_KINGDOM_BUNDLE_ENTRY)
                && let Ok(slot0) = legaia_asset::kingdom_bundle::decode_slot(&drake, 0)
            {
                let missing_rows: Vec<u16> = missing.iter().map(|&(_, y)| y).collect();
                for s in legaia_asset::clut_walk::park_strips(&slot0) {
                    if missing_rows.contains(&s.fb_y) {
                        base.write_block(s.fb_x, s.fb_y, s.w, s.h, &s.data);
                    }
                }
            }
            missing = covered(base);
        }
        for (x, y) in &missing {
            log::warn!(
                "play-window: CLUT-walk source cell ({x}, {y}) has no VRAM data - \
                 the walker will copy blank entries (strip residency gap)"
            );
        }
    }

    /// Advance the world-map water/CLUT-cell animation one sim tick, in
    /// retail vsync units: only the sim ticks that map to a retail vsync
    /// (`World::field_frame_step`) advance the clock, and a retail *game
    /// tick* lands every `World::frame_step` vsyncs (the adaptive
    /// `DAT_1F800393` factor `FUN_80016B6C` writes - `3` on the overworld,
    /// `2` in towns).
    ///
    /// Table path (retail): each slot-5 entry is an independent walker.
    /// Per game tick every accumulator banks `dt` vsyncs; when one crosses
    /// its current frame's `hold_vsyncs` it emits a 16x1 VRAM->VRAM
    /// `MoveImage` from the parked source strip onto the entry's
    /// destination cell, **resets the accumulator to zero** (NOT
    /// subtract-remainder: live captures show strictly constant intervals -
    /// hold 8 at dt 3 fires every 9 vsyncs with zero jitter, which only a
    /// reset produces), and advances the frame index with wrap-around. The
    /// real interval is therefore `ceil(hold / dt) * dt` vsyncs.
    ///
    /// The CPU VRAM is re-uploaded only when at least one copy fired, so
    /// the whole-VRAM upload runs a few times a second, not every frame.
    // PORT: FUN_8001ada4 - the SCUS actor walker's case 0xB, the CLUT-walk
    // stepper (acc += DAT_1F800393; on acc >= hold: MoveImage 16x1, acc = 0,
    // frame++ wrapping).
    pub(super) fn advance_ocean_animation(&mut self) {
        // While a battle is up the GPU texture holds the BATTLE VRAM (party
        // band + palettes + monster pages); the water cells aren't visible
        // under the battle stage, and re-uploading the field snapshot here
        // would clobber that texture so every battle mesh samples field
        // bytes (white speckle on the party band). Hold the shimmer until
        // the field VRAM is restored at battle exit.
        if self.session.host.world.mode == SceneMode::Battle {
            return;
        }
        // Only the sim ticks that map to a retail vsync advance the clock
        // (the 100 Hz sim carries ~60 vsyncs/s; see `World::field_frame_step`).
        if self.session.host.world.field_frame_step == 0 {
            return;
        }
        let dt = u32::from(self.session.host.world.frame_step.max(1));
        // First pass under the animation borrow: bank the game tick and
        // collect the fired copies; second pass applies them to the VRAM.
        let mut copies: Vec<(u16, u16, u16, u16)> = Vec::new();
        let mut head_frame: Option<[u8; 32]> = None;
        {
            let Some(anim) = self.ocean_anim.as_mut() else {
                return;
            };
            match anim {
                WaterAnim::Walk(w) => {
                    // A retail game tick lands every `dt` vsyncs; on each
                    // one, every entry banks `dt` (the walker's
                    // `acc += DAT_1F800393` step - one shared clock, so the
                    // entries stay phase-locked).
                    w.vsyncs_to_game_tick += 1;
                    if w.vsyncs_to_game_tick < dt {
                        return;
                    }
                    w.vsyncs_to_game_tick = 0;
                    for (entry, (acc, idx)) in w.table.entries.iter().zip(w.state.iter_mut()) {
                        *acc += dt;
                        let frame = &entry.frames[*idx];
                        if *acc < u32::from(frame.hold_vsyncs) {
                            continue;
                        }
                        *acc = 0;
                        copies.push((frame.src_x, frame.src_y, entry.dest_x, entry.dest_y));
                        *idx = (*idx + 1) % entry.frames.len();
                    }
                    if copies.is_empty() {
                        return;
                    }
                }
                WaterAnim::Ocean(anim) => {
                    // Legacy fallback: single ocean-head cell, frame bytes
                    // written from the decoded table rather than copied
                    // from a parked strip.
                    anim.vsyncs_to_game_tick += 1;
                    if anim.vsyncs_to_game_tick < dt {
                        return;
                    }
                    anim.vsyncs_to_game_tick = 0;
                    anim.vsync_accum += dt;
                    if anim.vsync_accum < OCEAN_ANIM_VSYNCS_PER_FRAME {
                        return;
                    }
                    anim.vsync_accum = 0;
                    let nframes = anim.frames.len() / 32;
                    if nframes == 0 {
                        return;
                    }
                    anim.cur = (anim.cur + 1) % nframes;
                    let off = anim.cur * 32;
                    head_frame = Some(anim.frames[off..off + 32].try_into().unwrap());
                }
            }
        }
        let Some(base) = self.cpu_vram_base.as_mut() else {
            return;
        };
        for (src_x, src_y, dst_x, dst_y) in copies {
            // The retail 16x1 CLUT-cell MoveImage (libgpu FUN_80058490).
            base.move_image(
                src_x,
                src_y,
                legaia_asset::clut_walk::COPY_WIDTH,
                1,
                dst_x,
                dst_y,
            );
        }
        if let Some(frame) = head_frame {
            // Fallback: CLUT row at VRAM (0, 506), the ocean-head cell.
            base.write_clut_row(0, 506, &frame);
        }
        if let Some(r) = self.win.renderer.as_ref() {
            match r.upload_vram(base) {
                Ok(v) => self.uploaded_vram = Some(v),
                Err(e) => log::error!("play-window: water CLUT re-upload: {e:#}"),
            }
        }
    }

    /// Apply the world's scripted CLUT-cell effects (field-VM `0x4C` n6
    /// sub-`0x61` one-shots + cross-fades, `World::step_clut_fx`) against the
    /// CPU VRAM and re-upload when anything changed. `World::tick` banks the
    /// retail game ticks (every `frame_step` vsyncs); this drains them once
    /// per sim tick. Battle-guarded for the same reason as
    /// [`Self::advance_ocean_animation`]: while a battle is up the GPU
    /// texture holds the battle VRAM and a field re-upload would clobber it.
    pub(super) fn apply_world_clut_fx(&mut self) {
        if self.session.host.world.mode == SceneMode::Battle {
            return;
        }
        if self.session.host.world.clut_fx.is_empty() {
            return;
        }
        let Some(base) = self.cpu_vram_base.as_mut() else {
            return;
        };
        if !self.session.host.world.step_clut_fx(base) {
            return;
        }
        if let Some(r) = self.win.renderer.as_ref() {
            match r.upload_vram(base) {
                Ok(v) => self.uploaded_vram = Some(v),
                Err(e) => log::error!("play-window: scripted CLUT-fx re-upload: {e:#}"),
            }
        }
    }

    /// Shared placement -> world-transform resolver for both the field static-
    /// object layer and the world-map continent terrain. Maps each placement's
    /// scene-pack mesh index through the uploaded-mesh bridge and builds its
    /// world model matrix.
    pub(super) fn resolve_placement_draws(
        &self,
        res: &SceneResources,
        tmd_src_index: &[usize],
        placements: &[legaia_asset::field_objects::Placement],
        flip_y: bool,
    ) -> Vec<(usize, Mat4)> {
        let Some(scene) = self.session.host.scene.as_ref() else {
            return Vec::new();
        };
        if placements.is_empty() {
            return Vec::new();
        }
        // Per-tile floor-height LUT (MAN header). World Y for a placed object
        // is `-lut[tile_floor_nibble] + y_off`; without it the town renders on
        // a flat plane (Rim Elm is on a cliff with real elevation changes).
        let floor_lut = scene
            .field_floor_height_lut(&self.session.host.index)
            .ok()
            .flatten();
        // The environment meshes are the scene's geometry-pack TMDs, in scan
        // order; `pack_index` indexes that subset of `res.tmds`. Pack
        // selection (the per-entry TMD-count vote) + the placement -> draw
        // resolution live in the shared kernel `engine_core::field_env`, so
        // the web viewer's assembled scene view resolves the exact same
        // draws; this method only adds the uploaded-mesh bridge and the
        // render-frame model matrix.
        let env_tmds = legaia_engine_core::field_env::env_pack_tmd_indices(scene, res);
        if env_tmds.is_empty() {
            return Vec::new();
        }
        let (env_draws, dropped) =
            legaia_engine_core::field_env::resolve_env_draws(&env_tmds, placements, floor_lut);
        let diag = std::env::var_os("LEGAIA_DIAG_PLACE").is_some();
        if diag {
            for d in &dropped {
                match d {
                    legaia_engine_core::field_env::EnvDrawDrop::NoPackIndex {
                        world_x,
                        world_z,
                    } => {
                        log::info!("DIAG place drop: no pack_index at ({world_x}, {world_z})");
                    }
                    legaia_engine_core::field_env::EnvDrawDrop::SlotOutOfRange {
                        pack_index,
                        world_x,
                        world_z,
                    } => {
                        log::info!(
                            "DIAG place drop: pack_index {} out of range ({} env tmds) at ({}, {})",
                            pack_index,
                            env_tmds.len(),
                            world_x,
                            world_z
                        );
                    }
                }
            }
        }
        // res.tmds index -> uploaded-mesh index (None where the mesh was
        // dropped for having no renderable prims).
        let mut res_to_mesh: Vec<Option<usize>> = vec![None; res.tmds.len()];
        for (mesh_idx, &src) in tmd_src_index.iter().enumerate() {
            if let Some(slot) = res_to_mesh.get_mut(src) {
                *slot = Some(mesh_idx);
            }
        }
        let mut draws = Vec::new();
        for d in &env_draws {
            let Some(mesh_idx) = res_to_mesh[d.res_tmd] else {
                if diag {
                    log::info!(
                        "DIAG place drop: pack {} (res {}) not in this mesh bridge at ({}, {})",
                        d.env_slot,
                        d.res_tmd,
                        d.world_x,
                        d.world_z
                    );
                }
                continue;
            };
            // PSX field coords (same retail Y-down convention as actor
            // positions). `flip_y` selects the render-frame pairing: the
            // world-map cameras carry no world negation, so their draws keep
            // the per-model flip; the FIELD frame draws raw vertices and the
            // camera's FIELD_WORLD_FLIP provides the single net negation
            // (elevation renders retail-correct).
            let t = Mat4::from_translation(Vec3::new(
                d.world_x as f32,
                d.world_y as f32,
                d.world_z as f32,
            ));
            // Authored yaw from the object record's `+0x0A` (PSX 4096-per-rev;
            // bridge quarter-turns, tree variety). `Mat4::from_rotation_y`
            // reproduces retail's pure-Y `FUN_80026988` matrix exactly in the
            // retail frame, and rotation about Y commutes with the (1,-1,1)
            // flip, so it composes identically on both frame pairings.
            let rot = Mat4::from_rotation_y(
                f32::from(d.rot_y & 0x0FFF) * (std::f32::consts::TAU / 4096.0),
            );
            let model = if flip_y {
                t * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0)) * rot
            } else {
                t * rot
            };
            draws.push((mesh_idx, model));
        }
        log::info!(
            "play-window: {} field placement draws ({} placements, {} env meshes)",
            draws.len(),
            placements.len(),
            env_tmds.len(),
        );
        draws
    }

    /// Debug-install a synthetic tile board (`LEGAIA_TILE_BOARD_DEMO=1`) so
    /// the per-cell tile-actor draw pass can be exercised visually: no
    /// retail scene MAN carries an op-0x49 sub-5 install (the census in
    /// `tests/tile_board_draw_live.rs` pins that), so without this the
    /// board renderer has no reachable on-screen trigger. Builds the same
    /// 14-byte `[0x49, sub-op 5, header]` window the field VM would hand
    /// `op49_menu_request`, centred a few tiles off the player so the
    /// follow camera frames it, with the tile templates pointed at the
    /// resident global-pool head (the effect-model library at `3..`).
    /// One-shot per scene: a no-op while a board is up or armed.
    pub(super) fn maybe_install_demo_tile_board(&mut self) {
        if std::env::var_os("LEGAIA_TILE_BOARD_DEMO").is_none() {
            return;
        }
        let world = &mut self.session.host.world;
        if world.mode != SceneMode::Field || world.tile_board.is_some() || world.tile_board_armed {
            return;
        }
        let Some(pslot) = world.player_actor_slot else {
            return;
        };
        let (px, pz) = {
            let a = &world.actors[pslot as usize];
            (a.move_state.world_x as i32, a.move_state.world_z as i32)
        };
        // 7x7 board with the player's tile at its centre.
        let origin_x = ((px >> 7) - 3).clamp(0, 255) as u8;
        let origin_z = ((pz >> 7) - 3).clamp(0, 255) as u8;
        let instr: [u8; 14] = [
            0x49, 0x05, // op, sub-op
            origin_x, origin_z, // +1/+2 tile origin
            7, 7, // +3/+4 width x height
            5, // +5 draw radius
            0, // +6 mode flag (full-board draw)
            0, 0, 0, 0, // +7/+9 event-flag bases (unused by the demo)
            0, // +0xb player template (character-mesh head)
            3, // +0xc tile template base (effect-model library)
        ];
        if world.try_install_tile_board(&instr) {
            log::info!(
                "play-window: demo tile board installed at tile ({origin_x},{origin_z}) \
                 ({} draw-list cells)",
                world.tile_board_draw_list.len()
            );
        }
    }

    /// Rebuild the window's render-side scene state after the host swapped
    /// scenes under it (a door transition: `SceneTickEvent::SceneEntered`).
    /// Rebuilds [`SceneResources`] for the newly loaded scene and re-runs
    /// [`Self::upload_assets`], which replaces the VRAM, mesh list, actor
    /// bindings, player mesh + locomotion clips, NPC/prop draws, terrain and
    /// placement draw lists wholesale. Soft-fails (logs, keeps the stale
    /// scene render) so a bad destination never crashes the window loop.
    pub(super) fn rebuild_scene_render_state(&mut self) {
        match build_window_scene_resources(&self.session) {
            Ok(res) => {
                // Spawn-slot drain state is per-scene (the new scene's field
                // VM re-issues its own actor spawns; `upload_assets` re-seats
                // the player's slot).
                self.drained_spawn_slots.clear();
                self.scene_res = Some(res);
                self.upload_assets();
            }
            Err(e) => log::warn!("play-window: scene-resource rebuild failed: {e:#}"),
        }
    }
}

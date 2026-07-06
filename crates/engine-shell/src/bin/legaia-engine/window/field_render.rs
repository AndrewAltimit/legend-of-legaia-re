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
        // Only the sparse placed landmarks (FUN_8003A55C, flags & 0x4) resolve
        // to slot-1 pack meshes via record[+0x10]+prefix. The bulk continent
        // ground is NOT per-cell pack meshes (the old `walk_terrain_tiles`
        // sweep floods 97% of cells with pool-5 because their record[+0x10] is
        // 0); it is the heightfield surface built separately in `upload_assets`
        // (`Scene::walk_heightfield`). See docs/subsystems/world-map.md.
        let tiles = match scene.walk_object_placements(&self.session.host.index) {
            Ok(Some(t)) => t,
            _ => Vec::new(),
        };
        if tiles.is_empty() {
            return Vec::new();
        }
        // World-map frame: raw retail-convention transforms - both world-map
        // cameras compose FIELD_WORLD_FLIP (the walk view through the pinned
        // retail composition), so the draws are unflipped like the field's.
        self.resolve_placement_draws(res, tmd_src_index, &tiles, false)
    }

    /// Resolve the world-map ocean CLUT animation for the active scene: scan the
    /// scene's PROT entries for the kingdom bundle, decode its slot-0 TIM_LIST,
    /// and pull the ocean tile's 13-frame CLUT animation table
    /// ([`legaia_asset::ocean::find_ocean_assets`]). Returns `None` for a
    /// non-world-map scene or a bundle without the ocean tile. The ocean tile
    /// texture and its base CLUT are already uploaded into VRAM by the slot-0
    /// TIM pass; this only recovers the per-frame palette overrides.
    pub(super) fn resolve_ocean_anim(&self) -> Option<OceanAnim> {
        let scene = self.session.host.scene.as_ref()?;
        if !legaia_engine_core::scene::is_world_map_scene(&scene.name) {
            return None;
        }
        for entry in &scene.entries {
            let Ok(slot0) = legaia_asset::kingdom_bundle::decode_slot(&entry.bytes, 0) else {
                continue;
            };
            if let Some(ocean) = legaia_asset::ocean::find_ocean_assets(&slot0)
                && ocean.animation_frames.len() >= 32
            {
                return Some(OceanAnim {
                    frames: ocean.animation_frames,
                    cur: 0,
                    tick: 0,
                });
            }
        }
        None
    }

    /// Advance the world-map ocean CLUT animation one sim tick. When the frame
    /// cursor crosses [`OCEAN_ANIM_TICKS_PER_FRAME`], write the next frame's 16
    /// BGR555 entries into the CPU VRAM CLUT row at `(0, 506)` and re-upload the
    /// VRAM so the heightfield's water cells (which sample that CLUT) shimmer.
    /// No-op when no ocean animation is loaded. Cheap: the whole-VRAM re-upload
    /// fires only on a frame change (~10x/s), not every render frame.
    pub(super) fn advance_ocean_animation(&mut self) {
        // While a battle is up the GPU texture holds the BATTLE VRAM (party
        // band + palettes + monster pages); the ocean cells aren't visible
        // under the battle stage, and re-uploading the field snapshot here
        // would clobber that texture so every battle mesh samples field
        // bytes (white speckle on the party band). Hold the shimmer until
        // the field VRAM is restored at battle exit.
        if self.session.host.world.mode == SceneMode::Battle {
            return;
        }
        let frame: [u8; 32] = {
            let Some(anim) = self.ocean_anim.as_mut() else {
                return;
            };
            anim.tick += 1;
            if anim.tick < OCEAN_ANIM_TICKS_PER_FRAME {
                return;
            }
            anim.tick = 0;
            let nframes = anim.frames.len() / 32;
            if nframes == 0 {
                return;
            }
            anim.cur = (anim.cur + 1) % nframes;
            let off = anim.cur * 32;
            anim.frames[off..off + 32].try_into().unwrap()
        };
        let Some(base) = self.cpu_vram_base.as_mut() else {
            return;
        };
        // CLUT row at VRAM (0, 506) - the retail per-frame ocean DMA target.
        base.write_clut_row(0, 506, &frame);
        if let Some(r) = self.win.renderer.as_ref() {
            match r.upload_vram(base) {
                Ok(v) => self.uploaded_vram = Some(v),
                Err(e) => log::error!("play-window: ocean CLUT re-upload: {e:#}"),
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
        // order; `pack_index` indexes that subset of `res.tmds`.
        //
        // The geometry pack is the scene-owned PROT entry that actually
        // produced the most environment TMDs - NOT necessarily
        // `find_bundle`'s entry. For a single-entry town both agree (town01:
        // MAN + 114 TMDs in entry 4). But two scene shapes split them, in
        // opposite directions, so neither "the bundle entry" nor "the first
        // SceneAssetTable" is universally right:
        //   - the opening cutscene `opdeene` keeps its MAN in a
        //     `SceneScriptedAssetTable` (entry 748, what `find_bundle` returns)
        //     and its 72-TMD vignette geometry in a *separate* `SceneAssetTable`
        //     sibling (entry 749) - so keying on `find_bundle` found zero env
        //     meshes and the prologue rendered as a blank screen;
        //   - a world-map kingdom bundle keeps its geometry in the
        //     `SceneScriptedAssetTable` (`find_bundle`, entry 85) while a
        //     sibling `SceneAssetTable` (entry 86) holds an unrelated sub-area -
        //     so "prefer the SceneAssetTable" would break the overworld.
        // Selecting the scene-owned entry with the most parsed TMDs resolves
        // the geometry pack the placements index in every case (opdeene 749,
        // town01 4, map01 85), and the `scene_entry_ids` filter keeps shared
        // blocks (the player mesh) out of the vote.
        let scene_entry_ids: std::collections::HashSet<u32> =
            scene.entries.iter().map(|e| e.idx).collect();
        let mut entry_tmd_counts: std::collections::HashMap<u32, usize> =
            std::collections::HashMap::new();
        for t in &res.tmds {
            if scene_entry_ids.contains(&t.entry_idx) {
                *entry_tmd_counts.entry(t.entry_idx).or_default() += 1;
            }
        }
        // Highest TMD count wins; ties break to the lowest entry index so the
        // choice is deterministic (HashMap iteration order is not).
        let Some(env_entry) = entry_tmd_counts
            .into_iter()
            .max_by_key(|&(idx, n)| (n, std::cmp::Reverse(idx)))
            .map(|(idx, _)| idx)
        else {
            return Vec::new();
        };
        let env_tmds: Vec<usize> = res
            .tmds
            .iter()
            .enumerate()
            .filter(|(_, t)| t.entry_idx == env_entry)
            .map(|(i, _)| i)
            .collect();
        // res.tmds index -> uploaded-mesh index (None where the mesh was
        // dropped for having no renderable prims).
        let mut res_to_mesh: Vec<Option<usize>> = vec![None; res.tmds.len()];
        for (mesh_idx, &src) in tmd_src_index.iter().enumerate() {
            if let Some(slot) = res_to_mesh.get_mut(src) {
                *slot = Some(mesh_idx);
            }
        }
        let diag = std::env::var_os("LEGAIA_DIAG_PLACE").is_some();
        let mut draws = Vec::new();
        for p in placements {
            let Some(pack_index) = p.pack_index else {
                if diag {
                    log::info!(
                        "DIAG place drop: no pack_index at ({}, {})",
                        p.world_x,
                        p.world_z
                    );
                }
                continue;
            };
            let Some(&res_idx) = env_tmds.get(pack_index as usize) else {
                if diag {
                    log::info!(
                        "DIAG place drop: pack_index {} out of range ({} env tmds) at ({}, {})",
                        pack_index,
                        env_tmds.len(),
                        p.world_x,
                        p.world_z
                    );
                }
                continue;
            };
            let Some(mesh_idx) = res_to_mesh[res_idx] else {
                if diag {
                    log::info!(
                        "DIAG place drop: pack {} (res {}) not in this mesh bridge at ({}, {})",
                        pack_index,
                        res_idx,
                        p.world_x,
                        p.world_z
                    );
                }
                continue;
            };
            // World Y from the floor-height LUT (`-lut[nibble] + y_off`), or
            // the ground plane when the LUT / nibble is unavailable.
            let world_y = match (floor_lut, p.floor_nibble) {
                (Some(lut), Some(nib)) => -(lut[(nib & 0x0F) as usize] as i32) + p.y_off as i32,
                _ => 0,
            };
            // PSX field coords (same retail Y-down convention as actor
            // positions). `flip_y` selects the render-frame pairing: the
            // world-map cameras carry no world negation, so their draws keep
            // the per-model flip; the FIELD frame draws raw vertices and the
            // camera's FIELD_WORLD_FLIP provides the single net negation
            // (elevation renders retail-correct).
            let t = Mat4::from_translation(Vec3::new(
                p.world_x as f32,
                world_y as f32,
                p.world_z as f32,
            ));
            let model = if flip_y {
                t * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0))
            } else {
                t
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
        match build_window_scene_resources(
            &self.session,
            Some(self.scene_rebuild_extracted_root.as_path()),
        ) {
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

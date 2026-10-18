//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;

use legaia_engine_scene::field_env::{FloorAnchor, FloorWave};

#[path = "field_render/floor_wave.rs"]
mod floor_wave;
#[path = "field_render/placements.rs"]
mod placements;
#[path = "field_render/prop_animation.rs"]
mod prop_animation;
#[path = "field_render/terrain.rs"]
mod terrain;

pub(super) use floor_wave::*;

/// Tag on a [`PlayWindowApp::posed_prop_frame_draws`] baked index naming a
/// shaded light-source variant (`field_lit.meshes`) rather than `meshes`.
pub(super) const LIT_VARIANT_TAG: usize = 1 << 30;

/// One baked placed-object draw list: the `(mesh, model)` draws, the floor
/// rungs each draw's Y came from, and which placed-object sweep owns each draw
/// ([`legaia_engine_scene::field_env::placed_window_key`]), and each draw's
/// grid cell + cull radius for the visible-tile crop
/// ([`legaia_engine_scene::field_view_window::CellKey`]), and each draw's bind
/// record ([`legaia_engine_scene::field_env::placed_bind_records`] - the key of
/// its live scripted displacement), all parallel.
pub(super) type PlacedDrawList = (
    Vec<(usize, Mat4)>,
    Vec<FloorAnchor>,
    Vec<Option<legaia_engine_scene::field_env::PlacedWindowKey>>,
    Vec<legaia_engine_scene::field_view_window::CellKey>,
    Vec<Option<usize>>,
    Vec<Option<CameraFacing>>,
);

/// A camera-facing terrain / decoration cell
/// ([`legaia_engine_scene::field_env::EnvDraw::view_skip`]): the record's
/// `0x380` bits and the cell's own rotation-and-scale, kept so the frame can
/// rebuild the cell's model as `T(pos) * K * rot` against this frame's camera
/// (`legaia_render_kernels::gte::decoration_cell_basis`).
pub(super) type CameraFacing = (u16, Mat4);

/// [`PlayWindowApp::resolve_field_terrain_draws`]'s lists: the draws, the
/// ladder rungs each Y came from, the visible-tile crop keys and the
/// camera-facing cells, all parallel.
pub(super) type TerrainDrawList = (
    Vec<(usize, Mat4)>,
    Vec<FloorAnchor>,
    Vec<legaia_engine_scene::field_view_window::CellKey>,
    Vec<Option<CameraFacing>>,
);

impl PlayWindowApp {
    /// Shade the env draws' light-source rows under the world's live field
    /// light: one shaded copy of each lit mesh per draw rotation, retail's
    /// `L * Rot` fold (`legaia_engine_field::field_lit_mesh`). Runs after an
    /// asset upload and again whenever the live light changes (op `4C 8A`).
    pub(super) fn rebuild_field_lit_meshes(&mut self) {
        let light = self.session.host.world.presentation.field_light;
        let lit = &mut self.field_lit;
        lit.light = Some(light);
        lit.meshes.clear();
        lit.terrain.clear();
        lit.placement.clear();
        lit.posed.clear();
        let census_on = self.draw_census.is_some();
        let mut lit_census: Vec<legaia_engine_system::draw_census::MeshCensus> = Vec::new();
        // `LEGAIA_DIAG_NO_LIT_ROWS` leaves the lit rows at their neutral
        // texel, for before/after frames.
        if lit.sources.is_empty() || std::env::var_os("LEGAIA_DIAG_NO_LIT_ROWS").is_some() {
            return;
        }
        let Some(r) = self.win.renderer.as_ref() else {
            return;
        };
        let mut keys: std::collections::HashMap<(usize, [[i32; 3]; 3]), usize> =
            std::collections::HashMap::new();
        let sources = &lit.sources;
        let shaded = &mut lit.meshes;
        let mut variant = |mesh_idx: usize, model: &Mat4| -> Option<usize> {
            let (mesh, lit_vertices) = sources.get(&mesh_idx)?;
            let m3: [[f32; 3]; 3] =
                std::array::from_fn(|row| std::array::from_fn(|col| model.col(col)[row]));
            let rot = legaia_engine_field::field_lit_mesh::rotation_from_matrix(m3);
            if let Some(&v) = keys.get(&(mesh_idx, rot)) {
                return Some(v);
            }
            let mut colors = mesh.colors.clone();
            legaia_engine_field::field_lit_mesh::shade_lit_rows(
                &mut colors,
                lit_vertices,
                &light,
                &rot,
            );
            let up = r
                .upload_vram_mesh(
                    &mesh.positions,
                    &mesh.uvs,
                    &mesh.cba_tsb,
                    &mesh.normals,
                    &colors,
                    &mesh.indices,
                )
                .map_err(|e| log::warn!("lit env mesh upload skipped: {e:#}"))
                .ok()?;
            shaded.push(up);
            if census_on {
                lit_census.push(legaia_engine_system::draw_census::MeshCensus::from_mesh(
                    &mesh.positions,
                    &mesh.cba_tsb,
                    &colors,
                    &mesh.indices,
                ));
            }
            let v = shaded.len() - 1;
            keys.insert((mesh_idx, rot), v);
            Some(v)
        };
        let terrain: Vec<Option<usize>> = self
            .field_terrain_draws
            .iter()
            .map(|(m, model)| variant(*m, model))
            .collect();
        let placement: Vec<Option<usize>> = self
            .field_placement_draws
            .iter()
            .map(|(m, model)| variant(*m, model))
            .collect();
        let posed: Vec<Option<usize>> = self
            .field_posed_props
            .iter()
            .map(|p| p.baked.vram.and_then(|i| variant(i, &p.model)))
            .collect();
        lit.terrain = terrain;
        lit.placement = placement;
        lit.posed = posed;
        if let Some(d) = self.draw_census.as_mut() {
            d.lit = lit_census;
        }
    }

    /// Re-shade the env draws when the live field light no longer matches
    /// the one they were shaded under.
    pub(super) fn sync_field_lit_meshes(&mut self) {
        if !self.field_lit.sources.is_empty()
            && self.field_lit.light != Some(self.session.host.world.presentation.field_light)
        {
            self.rebuild_field_lit_meshes();
        }
    }
}

impl PlayWindowApp {
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
        if world.install_demo_tile_board() {
            log::info!(
                "play-window: demo tile board installed ({} draw-list cells)",
                world.board.draw_list.len()
            );
        }
    }

    /// Retail's post-FMV control transfer, through the session (which also
    /// resets the camera globals and drops the queued SFX cues on a scene
    /// swap - no scene bank is staged, as retail's field init loads none),
    /// plus the render-side rebuild when the hand-off entered a new scene.
    /// The hand-off loads its scene outside the field VM's transition op, so
    /// no `SceneEntered` event follows it - the window used to only log the
    /// outcome and draw the trigger scene's meshes over the new world, which
    /// the browser play page already rebuilt (`runtime.rs`, the
    /// `fmv_handoff_scene` arm of `tick_frame`).
    pub(super) fn apply_fmv_handoff(&mut self) {
        if let Some(outcome) = self.session.apply_pending_fmv_handoff() {
            log::info!("cutscene: {outcome}");
            if matches!(
                outcome,
                legaia_engine_scene_host::scene_host::FmvHandoffOutcome::Entered { .. }
            ) {
                self.rebuild_scene_render_state();
            }
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
        // Every caller swapped the scene. Retail's field entry
        // (`FUN_80025C24`) rewrites the camera globals and kills the mover, so
        // no glide pose survives the door - the next scene's first scripted
        // shot snaps in, as on the browser page (`CutsceneGlide::reset`).
        self.cutscene_glide.reset();
        match build_window_scene_resources(&self.session) {
            Ok(res) => {
                // Spawn-slot drain state is per-scene (the new scene's field
                // VM re-issues its own actor spawns; `upload_assets` re-seats
                // the player's slot).
                self.drained_spawn_slots.clear();
                self.scene_res = Some(res);
                self.scene_actors_pending = true;
                self.upload_assets();
            }
            Err(e) => log::warn!("play-window: scene-resource rebuild failed: {e:#}"),
        }
    }
}

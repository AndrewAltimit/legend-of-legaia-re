//! Field and world-map terrain draw lists, the ocean tile animation and the
//! world CLUT effects.

use super::*;

impl PlayWindowApp {
    /// Resolve the field scene's **terrain / ground** tiles (the `CELL_VISIBLE`
    /// sweep in `Scene::field_terrain_tiles`) to `(mesh, model)` draws, the same
    /// way `resolve_field_placement_draws` resolves the placed objects. This is
    /// the town's floor / ground layer; without it a field scene renders its
    /// buildings floating over the bare clear colour.
    ///
    /// Records carrying the *placed* flag are excluded: they are already drawn
    /// by `resolve_field_placement_draws`, from the same record and at the same
    /// transform, so a visible cell pointing at one would stamp a second, and
    /// the second copy would be the **unposed** one (the placement layer poses
    /// its multi-object props). Keeping the two layers disjoint is the same rule
    /// `field_objects::parse_walk_decorations` applies on the world map.
    pub(in crate::window) fn resolve_field_terrain_draws(
        &self,
        res: &SceneResources,
        tmd_src_index: &[usize],
    ) -> TerrainDrawList {
        let Some(scene) = self.session.host.scene.as_ref() else {
            return Default::default();
        };
        let tiles: Vec<legaia_scene_formats::field_objects::Placement> =
            match scene.field_terrain_tiles(&self.session.host.index) {
                Ok(Some(t)) => t
                    .into_iter()
                    .filter(|p| p.flags & legaia_scene_formats::field_objects::FLAG_PLACED == 0)
                    .collect(),
                _ => return Default::default(),
            };
        if tiles.is_empty() {
            return Default::default();
        }
        // Field frame: raw retail-convention transforms (see above).
        let (draws, floors, _, cells, _, facing) =
            self.resolve_placement_draws(res, tmd_src_index, &tiles, false, None, None, None);
        (draws, floors, cells, facing)
    }

    /// World-map continent terrain draws: the dense visible-tile set
    /// (`Scene::field_terrain_tiles`, the `FUN_801F69D8` overhead sweep) rather
    /// than the placed-flag interactive objects. Tiles whose pack index falls
    /// outside the loaded slot-1 landmark pack (they reference the wider global
    /// TMD pool, not yet loaded for the world map) resolve to no mesh and are
    /// skipped by `resolve_placement_draws`.
    pub(in crate::window) fn resolve_world_map_terrain_draws(
        &self,
        res: &SceneResources,
        tmd_src_index: &[usize],
    ) -> (Vec<(usize, Mat4)>, usize, Vec<Option<usize>>) {
        let Some(scene) = self.session.host.scene.as_ref() else {
            return (Vec::new(), 0, Vec::new());
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
        //
        // The two layers resolve separately so the caller knows where the
        // decorations start: retail's decoration sweep (`FUN_801F69D8`)
        // depth-cues each decoration by its origin's depth
        // (`legaia_engine_field::overworld_ground_cue::decoration_draw_cue`),
        // the landmarks it skips take no cue from it.
        let mut landmarks = match scene.walk_object_placements(&self.session.host.index) {
            Ok(Some(t)) => t,
            _ => Vec::new(),
        };
        // Story-hidden landmarks: a placed record whose bind prologue parked
        // its actor at the hide box draws nothing (map01's sea dome south of
        // Rim Elm, the river bridge's second stamp).
        let binds = scene
            .field_object_binds(&self.session.host.index)
            .ok()
            .flatten();
        if let Some(binds) = binds.as_ref() {
            legaia_engine_scene::field_env::retain_visible_landmark_placements(
                &mut landmarks,
                binds,
                &self.session.host.world.hidden_object_records(),
            );
        }
        let deco = match scene.walk_decoration_placements(&self.session.host.index) {
            Ok(Some(t)) => t,
            _ => Vec::new(),
        };
        // World-map frame: raw retail-convention transforms - both world-map
        // cameras compose FIELD_WORLD_FLIP (the walk view through the pinned
        // retail composition), so the draws are unflipped like the field's.
        // The landmarks keep their bind record (a kingdom MAN's object
        // records tint them - map02's walls around Jeremi draw half-dark), the
        // decorations have none.
        let (mut draws, mut records) = if landmarks.is_empty() {
            (Vec::new(), Vec::new())
        } else {
            let l = self.resolve_placement_draws(
                res,
                tmd_src_index,
                &landmarks,
                false,
                None,
                None,
                binds.as_ref(),
            );
            (l.0, l.4)
        };
        let deco_start = draws.len();
        if !deco.is_empty() {
            draws.extend(
                self.resolve_placement_draws(res, tmd_src_index, &deco, false, None, None, None)
                    .0,
            );
        }
        records.resize(draws.len(), None);
        (draws, deco_start, records)
    }

    /// Resolve the world-map water/CLUT-cell animation for the active scene.
    ///
    /// Retail path: the kingdom bundle's slot 5 (the type-byte `0x06` slot of
    /// PROT 0085 / 0244 / 0391) is the CLUT-walk animation table - eight
    /// independent 16x1 `MoveImage` walkers (ocean head + shoreline/terrain
    /// shimmer cells), parsed by [`legaia_scene_formats::clut_walk`]. Parsing it also
    /// parks the walkers' VRAM source strips into the CPU VRAM (see
    /// [`Self::park_clut_walk_strips`]): the strips ship in the bundle's
    /// slot-0 TIM_LIST as raw CLUT-block records without the TIM magic, so
    /// the scene TIM pre-pass skips them.
    ///
    /// Fallback: the legacy single-cell 13-frame ocean-head cycle
    /// ([`legaia_scene_formats::ocean::find_ocean_assets`]), used ONLY when no scene
    /// entry yields a parseable slot-5 table. The fallback exists for
    /// modified / damaged bundles (every retail kingdom ships slot 5): it
    /// keeps the most visible water shimmer alive rather than freezing the
    /// sea, at the cost of the seven non-ocean shimmer cells.
    // REF: FUN_8001f05c - asset-type dispatch case 6 installs the decoded
    // slot-5 table at DAT_8007B7C8; FUN_801d6704 (field init) spawns one
    // walker actor per entry via FUN_80024cfc.
    pub(in crate::window) fn resolve_ocean_anim(&mut self) -> Option<WaterAnim> {
        // Resolve + park through the one engine kernel the browser page and
        // the field-scene viewer also call (`ClutWalkAnim::install`): the
        // scene bundle's type-6 table on a field scene, the kingdom's slot-5
        // table on an overworld, the Drake complement rows, and the legacy
        // ocean-head fallback.
        let scene = self.session.host.scene.as_ref()?;
        let base = self.cpu_vram_base.as_mut()?;
        let install = legaia_engine_scene::clut_walk_anim::ClutWalkAnim::install(
            scene,
            &self.session.host.index,
            base,
        )?;
        if install.ocean_fallback {
            log::warn!(
                "play-window: no slot-5 CLUT-walk table in the kingdom bundle; \
                 falling back to the legacy ocean-head cycle"
            );
        }
        for (x, y) in &install.missing_cells {
            log::warn!(
                "play-window: CLUT-walk source cell ({x}, {y}) has no VRAM data - \
                 the walker will copy blank entries (strip residency gap)"
            );
        }
        Some(WaterAnim {
            anim: install.anim,
            vsyncs_to_game_tick: 0,
        })
    }

    /// Advance the world-map water/CLUT-cell animation one sim tick, in
    /// retail vsync units: only the sim ticks that map to a retail vsync
    /// (`World::clock.display_frame_step`) advance the clock, and a retail *game
    /// tick* lands every `World::clock.frame_step` vsyncs (the adaptive
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
    // REF: FUN_8001ada4 - stepped by `legaia_engine_scene::clut_walk_anim`.
    pub(in crate::window) fn advance_ocean_animation(&mut self) {
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
        // (the 100 Hz sim carries ~60 vsyncs/s; see `World::clock.display_frame_step`).
        if self.session.host.world.clock.display_frame_step == 0 {
            return;
        }
        let dt = u32::from(self.session.host.world.clock.frame_step.max(1));
        let (Some(w), Some(base)) = (self.ocean_anim.as_mut(), self.cpu_vram_base.as_mut()) else {
            return;
        };
        // A retail game tick lands every `dt` vsyncs; the stepper banks `dt`
        // on each (`legaia_engine_scene::clut_walk_anim`).
        w.vsyncs_to_game_tick += 1;
        if w.vsyncs_to_game_tick < dt {
            return;
        }
        w.vsyncs_to_game_tick = 0;
        if !w.anim.game_tick(dt, base) {
            return;
        }
        if let Some(r) = self.win.renderer.as_ref() {
            match r.upload_vram(base) {
                Ok(v) => self.uploaded_vram = Some(v),
                Err(e) => log::error!("play-window: water CLUT re-upload: {e:#}"),
            }
        }
    }

    /// Apply the world's scripted VRAM effects - the field-VM `0x4C` n6
    /// sub-`0x60` `MoveImage` stamps (`World::apply_script_vram_moves`, e.g.
    /// the town01 opening's Noa face-frame stamps) and the sub-`0x61`
    /// CLUT-cell one-shots + cross-fades (`World::step_clut_fx`) - against
    /// the CPU VRAM and re-upload when anything changed. `World::tick` banks
    /// the retail game ticks (every `frame_step` vsyncs); this drains them
    /// once per sim tick. Battle-guarded for the same reason as
    /// [`Self::advance_ocean_animation`]: while a battle is up the GPU
    /// texture holds the battle VRAM and a field re-upload would clobber it.
    pub(in crate::window) fn apply_world_clut_fx(&mut self) {
        // The shared frame-tail kernel runs every step every tick (each
        // drains its own tick backlog and self-gates) and holds the battle
        // guard. This used to skip the whole call while four effect lists
        // were empty - a test that left out the `4C DB` blend fades, so a
        // lone blend fade never stepped, and that banked the idle ticks a
        // later effect then consumed at once.
        let Some(base) = self.cpu_vram_base.as_mut() else {
            return;
        };
        if !self.session.host.world.step_field_vram_effects(base, false) {
            return;
        }
        if let Some(r) = self.win.renderer.as_ref() {
            match r.upload_vram(base) {
                Ok(v) => self.uploaded_vram = Some(v),
                Err(e) => log::error!("play-window: scripted CLUT-fx re-upload: {e:#}"),
            }
        }
    }
}

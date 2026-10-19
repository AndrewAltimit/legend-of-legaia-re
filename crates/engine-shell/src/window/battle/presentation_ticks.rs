//! The battle's per-frame presentation ticks: face tables and face stamps,
//! Seru names, the status and effect CLUT cycles, the stage shell, the ground
//! ambient and the VRAM residency check.

use super::*;

impl PlayWindowApp {
    /// Lazily read the static face-frame tables out of the boot source's
    /// `SCUS_942.54` (`legaia_actor_formats::face_anim::FaceFrameTables`). A failed
    /// attempt (disc-free run / unparsable executable) is remembered so the
    /// probe only runs once; facial animation is simply skipped then.
    pub(in crate::window) fn load_face_tables(&mut self) {
        if self.battle_face_tables.attempted() {
            return;
        }
        use legaia_engine_scene::vfs::Vfs;
        let scus = if let Some(root) = self.extracted_root.as_deref() {
            legaia_engine_scene::vfs::DirVfs::new(root)
                .ok()
                .and_then(|v| v.read("SCUS_942.54").ok())
        } else if let Some(disc) = self.disc_path.as_deref() {
            legaia_engine_scene::vfs::DiscVfs::open(disc)
                .ok()
                .and_then(|v| v.read("SCUS_942.54").ok())
        } else {
            None
        };
        if self.battle_face_tables.load(scus.as_deref()) {
            log::info!(
                "play-window: SCUS face-frame tables unavailable - battle faces stay neutral"
            );
        }
    }

    /// Read the spell/seru display-name table from the boot SCUS so the
    /// seru-trade overlay can label each offer. Called once at launch
    /// (`window/run.rs`); a disc-free run leaves it `None` (offers fall back
    /// to "Seru NN").
    pub(in crate::window) fn ensure_seru_names(&mut self) {
        if self.seru_names.is_some() {
            return;
        }
        use legaia_engine_scene::vfs::Vfs;
        let scus = if let Some(root) = self.extracted_root.as_deref() {
            legaia_engine_scene::vfs::DirVfs::new(root)
                .ok()
                .and_then(|v| v.read("SCUS_942.54").ok())
        } else if let Some(disc) = self.disc_path.as_deref() {
            legaia_engine_scene::vfs::DiscVfs::open(disc)
                .ok()
                .and_then(|v| v.read("SCUS_942.54").ok())
        } else {
            None
        };
        self.seru_names = scus
            .as_deref()
            .and_then(legaia_game_tables::spell_names::SpellNameTable::from_scus);
    }

    /// The battle VRAM's per-tick re-stamps through the steps the page runs
    /// too (`legaia_engine_session::battle_vram`): the party's facial
    /// animation, then the status CLUT recolour, the effect CLUT stages and
    /// the stage module's queued `MoveImage`s. One re-upload when any texel
    /// moved, which moves the expected resident generation along with it so
    /// [`Self::check_battle_vram_residency`] does not read the refresh as a
    /// clobber.
    pub(in crate::window) fn tick_battle_vram_restamps(&mut self) {
        use legaia_engine_session::battle_vram::{stamp_battle_faces, step_battle_vram_edits};
        let world = &mut self.session.host.world;
        let mut dirty = false;
        if let Some(vram) = self.battle_vram.as_mut() {
            dirty |= stamp_battle_faces(
                world,
                &self.battle_face_tables,
                &mut self.battle_faces,
                vram,
            );
        }
        dirty |= step_battle_vram_edits(
            world,
            &mut self.battle_hud.status_clut,
            self.battle_vram.as_mut(),
        );
        if dirty
            && let (Some(r), Some(vram)) = (self.win.renderer.as_ref(), self.battle_vram.as_ref())
        {
            match r.upload_vram(vram) {
                Ok(v) => {
                    self.battle_vram_generation = Some(v.generation());
                    self.uploaded_vram = Some(v);
                }
                Err(e) => log::error!("play-window: battle re-stamp VRAM re-upload: {e:#}"),
            }
        }
    }

    /// A changed backdrop object list mid-fight
    /// (`SceneHost::battle_stage_object_indices` - the evolved-Cort
    /// arrival's slot-0 rebind) re-builds the stage shell meshes in place.
    /// The same arrival's VRAM edit is [`Self::tick_battle_vram_restamps`]'s.
    pub(in crate::window) fn tick_battle_stage_shell(&mut self) {
        let (Some(idx), Some((built, second))) =
            (self.battle_stage_mesh, self.battle_stage_shell.clone())
        else {
            return;
        };
        let Some((tmd, raw)) = self.scene_tmd_data.get(idx).cloned() else {
            return;
        };
        let objects = self
            .session
            .host
            .battle_stage_object_indices(tmd.objects.len());
        if objects == built {
            return;
        }
        let Some(r) = self.win.renderer.as_ref() else {
            return;
        };
        // The split the entry build made (`SceneHost::battle_stage_layers`).
        // The one mid-fight edit, the slot-0 rebind, leaves slot 1 holding
        // the object it held, so the spun meshes stand; a list with no slot
        // 1 drops them.
        let layers = self.session.host.battle_stage_layers(tmd.objects.len());
        if layers.spun.is_empty() {
            self.battle_stage_spun_mesh = None;
            self.battle_stage_spun_color_mesh = None;
        }
        let (vmesh, cmesh) = stage_shell_meshes(&tmd, &raw, second, &layers.fixed);
        match r.upload_vram_mesh(
            &vmesh.positions,
            &vmesh.uvs,
            &vmesh.cba_tsb,
            &vmesh.normals,
            &vmesh.colors,
            &vmesh.indices,
        ) {
            Ok(m) => self.meshes[idx] = m,
            Err(e) => log::warn!("play-window: stage shell rebuild: {e:#}"),
        }
        let cm = (!cmesh.is_empty())
            .then(|| {
                r.upload_color_mesh_blended(
                    &cmesh.positions,
                    &cmesh.colors,
                    &cmesh.indices,
                    &cmesh.blend,
                )
                .ok()
            })
            .flatten();
        match (self.battle_stage_color_mesh, cm) {
            (Some(ci), Some(cm)) => self.color_meshes[ci] = cm,
            (Some(_), None) => self.battle_stage_color_mesh = None,
            (None, Some(cm)) => {
                self.battle_stage_color_mesh = Some(self.color_meshes.len());
                self.color_meshes.push(cm);
            }
            (None, None) => {}
        }
        log::info!("play-window: battle stage shell re-bound to objects {objects:?}");
        self.battle_stage_shell = Some((objects, second));
    }

    /// Follow the live battle ambient on the ground grid: the pre-cue vertex
    /// colour is the ambient `0x8007B7B0` (base `+ 0x404040`) and the cue's
    /// far colour `0x8007BB48` is derived from the same base, both of which a
    /// summon close-up ramps down (`World::battle_ambient_base`, docs:
    /// battle.md "A cast dims it"). The far colour is re-derived every frame;
    /// the grid mesh bakes the near colour into its vertices, so it is rebuilt
    /// only on the frames the ambient moves.
    pub(in crate::window) fn sync_battle_ground_ambient(&mut self) {
        use legaia_engine_battle_vm::battle_ground_grid as grid;
        let Some(gi) = self.battle_ground_mesh else {
            return;
        };
        if self.session.host.world.mode != SceneMode::Battle {
            return;
        }
        let base = self.session.host.world.battle_ambient_base();
        self.battle_ground_cue_far = Some(
            grid::grid_far_colour(base, self.battle_stage_outdoor).map(|c| f32::from(c) / 255.0),
        );
        let near = grid::battle_ambient_colour(base);
        if near == self.battle_ground_rgbc {
            return;
        }
        let Some(r) = self.win.renderer.as_ref() else {
            return;
        };
        let g = build_battle_ground_grid(near);
        match r.upload_vram_mesh(
            &g.positions,
            &g.uvs,
            &g.cba_tsb,
            &g.normals,
            &g.colors,
            &g.indices,
        ) {
            Ok(m) => {
                self.meshes[gi] = m;
                self.battle_ground_rgbc = near;
            }
            Err(e) => log::warn!("play-window: battle ground grid re-colour: {e:#}"),
        }
    }

    /// Residency guard: while a battle texture is expected to be GPU-resident,
    /// verify no other path re-uploaded VRAM over it this frame. The
    /// white-speckle party bug (a background CLUT animator re-uploading the
    /// field snapshot mid-battle) was invisible to every CPU-side VRAM oracle
    /// because they never check *which* upload the draw samples. On a
    /// violation: log loudly, fail debug builds, and self-heal by re-uploading
    /// the stashed battle VRAM.
    pub(in crate::window) fn check_battle_vram_residency(&mut self) {
        if self.session.host.world.mode != SceneMode::Battle {
            return;
        }
        let Some(expected) = self.battle_vram_generation else {
            return;
        };
        let current = self.uploaded_vram.as_ref().map(|v| v.generation());
        if current == Some(expected) {
            return;
        }
        log::error!(
            "play-window: battle VRAM clobbered mid-battle (expected upload \
             generation {expected}, GPU holds {current:?}); re-uploading the \
             battle texture"
        );
        debug_assert!(
            false,
            "battle VRAM residency violated: expected generation {expected}, found {current:?}"
        );
        if let (Some(r), Some(vram)) = (self.win.renderer.as_ref(), self.battle_vram.as_ref()) {
            match r.upload_vram(vram) {
                Ok(v) => {
                    self.battle_vram_generation = Some(v.generation());
                    self.uploaded_vram = Some(v);
                }
                Err(e) => log::error!("play-window: battle VRAM re-upload (heal): {e:#}"),
            }
        }
    }
}

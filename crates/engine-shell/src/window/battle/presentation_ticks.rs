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
        if self.face_tables_attempted {
            return;
        }
        self.face_tables_attempted = true;
        use legaia_engine_core::Vfs;
        let scus = if let Some(root) = self.extracted_root.as_deref() {
            legaia_engine_core::DirVfs::new(root)
                .ok()
                .and_then(|v| v.read("SCUS_942.54").ok())
        } else if let Some(disc) = self.disc_path.as_deref() {
            legaia_engine_core::DiscVfs::open(disc)
                .ok()
                .and_then(|v| v.read("SCUS_942.54").ok())
        } else {
            None
        };
        self.face_tables = scus
            .as_deref()
            .and_then(legaia_actor_formats::face_anim::FaceFrameTables::from_scus);
        self.art_mouth_tables = scus
            .as_deref()
            .and_then(legaia_actor_formats::face_anim::ArtMouthTables::from_scus);
        if self.face_tables.is_none() {
            log::info!(
                "play-window: SCUS face-frame tables unavailable - battle faces stay neutral"
            );
        }
    }

    /// Lazily read the spell/seru display-name table from the boot SCUS so the
    /// seru-trade overlay can label each offer. Cached after the first read;
    /// a disc-free run leaves it `None` (offers fall back to "Seru NN").
    pub(in crate::window) fn ensure_seru_names(&mut self) {
        if self.seru_names.is_some() {
            return;
        }
        use legaia_engine_core::Vfs;
        let scus = if let Some(root) = self.extracted_root.as_deref() {
            legaia_engine_core::DirVfs::new(root)
                .ok()
                .and_then(|v| v.read("SCUS_942.54").ok())
        } else if let Some(disc) = self.disc_path.as_deref() {
            legaia_engine_core::DiscVfs::open(disc)
                .ok()
                .and_then(|v| v.read("SCUS_942.54").ok())
        } else {
            None
        };
        self.seru_names = scus
            .as_deref()
            .and_then(legaia_game_tables::spell_names::SpellNameTable::from_scus);
    }

    /// Per-tick battle facial animation: re-stamp each registered party
    /// member's current eye + mouth face frame onto the band's live face
    /// rows, exactly like the retail per-frame animator. The playing clip's
    /// `action_id` selects the member's face tracks, its integer keyframe
    /// cursor is the frame counter, and the selected frames are VRAM-to-VRAM
    /// copies from the band's face-frame strip (`Vram::move_image`, the
    /// `MoveImage` stamp). During the victory window - the battle ended in
    /// a monster wipe while a member still plays a dynamic-art-slot clip
    /// (staged id `0x11..=0x18`, e.g. the killing art) - the mouth records
    /// come from the static `0x80077E80` override table and the animator
    /// clocks on the halved victory counter instead (the win-quote mouth
    /// flap). The GPU texture is only re-uploaded on a frame
    /// whose stamp set differs from the previous one (retail re-issues
    /// identical `MoveImage`s every frame; the visible result is the same).
    // PORT: FUN_80047430 (facial-animator dispatch): per visible party
    // node, call the stamp pass with (band slot, char index, cursor
    // keyframes, playing action entry), skipping char 3 (Terra) and bands
    // >= 3. The stamp-selection half is `FaceFrameTables::stamps_with_art_window`
    // (PORT: FUN_8004C7B4 in legaia_actor_formats::face_anim).
    pub(in crate::window) fn tick_battle_face_stamps(&mut self) {
        use legaia_actor_formats::face_anim::{ART_BAND_FIRST, ART_BAND_LAST, ArtMouthOverride};
        use legaia_engine_battle_vm::battle_action::BattleEndCause;
        if self.session.host.world.mode != SceneMode::Battle || self.battle_faces.is_empty() {
            return;
        }
        let Some(tables) = self.face_tables.as_ref() else {
            return;
        };
        let art_tables = self.art_mouth_tables.as_ref();
        let Some(vram) = self.battle_vram.as_mut() else {
            return;
        };
        // Retail gate 1 of the victory-window mouth override: the
        // battle-end signal `DAT_8007BD71 == 0xFE` raised by a monster
        // wipe (the SM `0x5A` arm; the engine mirror is the
        // `BattleActionHost::battle_end` latch). Gates 2/3 - the victory
        // sequencer's phase halfword `ctx+0x6CE != 0` and the celebration
        // flag `DAT_8007BD60 & 0x80` (which the party-wipe path clears) -
        // are the retail victory presentation's internal progress flags;
        // the engine has no victory sequencer, so "the won battle is
        // still on screen" stands in for them. Escapes also raise 0xFE
        // but never set the celebration flag, so they stay excluded.
        let victory_window =
            self.session.host.world.battle.end == Some(BattleEndCause::MonsterWipe);
        let mut changed = false;
        for mf in &mut self.battle_faces {
            let Some(actor) = self.session.host.world.actors.get(mf.actor_slot) else {
                continue;
            };
            // No player yet = the rest pose: behaves like clip frame 0 of
            // the (track-less) idle, i.e. the neutral face.
            let (action_id, frame) = actor
                .battle_animation
                .as_ref()
                .map(|p| (p.action_id(), p.current_frame()))
                .unwrap_or((0, 0));
            // Track source. Staged ids >= 0x10 are art-bank clips: retail
            // materializes bank record `id - 0x10` and installs its
            // embedded entry (record +0x24) as the action-table pointer
            // (FUN_8004AD80), so the animator reads THAT entry's tracks
            // (record +0xB0 eyes / +0xBC mouth). Below the art base, the
            // playing clip's action slot picks the record[0] / swing entry.
            let tracks = if action_id
                >= legaia_actor_formats::battle_char_assembly::ART_ANIM_ID_BASE
            {
                mf.art_tracks
                    .get(
                        (action_id - legaia_actor_formats::battle_char_assembly::ART_ANIM_ID_BASE)
                            as usize,
                    )
                    .and_then(|t| t.as_ref())
            } else {
                mf.tracks.get(action_id as usize).and_then(|t| t.as_ref())
            };
            // Retail gate 4: the member's last-staged anim id
            // (`actor[+0x1DB]`; the engine's art-bank clips carry it as
            // their `action_id`) sits in the dynamic-art-slot band
            // `0x11..=0x18`. Open the override window, clocking the
            // member's `gp+0x9EA` mirror from 0; closed, the counter
            // resets.
            let art_mouth = if victory_window
                && (ART_BAND_FIRST..=ART_BAND_LAST).contains(&action_id)
                && let Some(track) = art_tables.and_then(|t| t.track(mf.char_index, action_id))
            {
                let counter = mf.art_counter.unwrap_or(0);
                mf.art_counter = Some(counter.saturating_add(1));
                Some(ArtMouthOverride { track, counter })
            } else {
                mf.art_counter = None;
                None
            };
            // The retail mouth-neutral gate: character-record word `+0xF8`
            // bit 0x2000 = ability bitfield (`+0xF4`) bit 45 (passive 0x2D
            // Rage, Evil Medallion). Read it off the occupying character's
            // rebuilt ability bytes (byte 5 bit 0x20).
            let world = &self.session.host.world;
            let force_neutral_mouth = world
                .party
                .roster
                .members
                .get(world.party_roster_slot(mf.actor_slot))
                .map(|m| m.ability_bits()[5] & 0x20 != 0)
                .unwrap_or(false);
            let stamps = tables.stamps_with_art_window(
                mf.char_index,
                mf.actor_slot,
                tracks,
                frame,
                art_mouth,
                force_neutral_mouth,
            );
            if mf.last_stamps.as_deref() != Some(&stamps) {
                for s in &stamps {
                    vram.move_image(s.src_x, s.src_y, s.w, s.h, s.dst_x, s.dst_y);
                }
                mf.last_stamps = Some(stamps);
                changed = true;
            }
        }
        if !changed {
            return;
        }
        if let (Some(r), Some(vram)) = (self.win.renderer.as_ref(), self.battle_vram.as_ref()) {
            match r.upload_vram(vram) {
                Ok(v) => {
                    // A face re-stamp is a legitimate battle-VRAM refresh:
                    // move the expected resident generation along with it.
                    self.battle_vram_generation = Some(v.generation());
                    self.uploaded_vram = Some(v);
                }
                Err(e) => log::error!("play-window: face-stamp VRAM re-upload: {e:#}"),
            }
        }
    }

    /// Status CLUT recolour - the fourth pass of `FUN_8004CE2C`. An actor
    /// whose Stone latch fired this frame has its party CLUT row
    /// (`481 + slot`) restaged grey from the pristine palette copy
    /// ([`legaia_engine_battle::battle_status_clut`]); the latch itself is
    /// armed inside `BattleHud::sync_status`, which
    /// [`Self::drain_and_log_battle_events`] already runs once per slot per
    /// frame.
    ///
    /// Shares the mid-battle re-upload protocol with
    /// [`Self::tick_battle_face_stamps`]: mutate the stashed battle VRAM,
    /// re-upload, and move the expected resident generation with it so
    /// [`Self::check_battle_vram_residency`] does not read the refresh as a
    /// clobber.
    pub(in crate::window) fn tick_battle_status_clut(&mut self) {
        if !self.battle_hud.status_clut.armed() {
            return;
        }
        let Some(vram) = self.battle_vram.as_mut() else {
            return;
        };
        if !self.battle_hud.status_clut.step(vram) {
            return;
        }
        if let (Some(r), Some(vram)) = (self.win.renderer.as_ref(), self.battle_vram.as_ref()) {
            match r.upload_vram(vram) {
                Ok(v) => {
                    self.battle_vram_generation = Some(v.generation());
                    self.uploaded_vram = Some(v);
                }
                Err(e) => log::error!("play-window: status-CLUT VRAM re-upload: {e:#}"),
            }
        }
    }

    /// Effect **CLUT stages** - the palette arm of the action-effect script's
    /// table form (`FUN_801DEA50`, `0x801df0dc..0x801df134`). Each queued
    /// `0x801F6418` byte is a VRAM source x whose sixteen entries move onto
    /// `(224, 476)`, recolouring whatever the spawned move-FX prototype draws
    /// ([`legaia_engine_battle::battle_effect_clut`]).
    ///
    /// Shares the mid-battle re-upload protocol with
    /// [`Self::tick_battle_status_clut`]: mutate the stashed battle VRAM,
    /// re-upload, and move the expected resident generation with it so
    /// [`Self::check_battle_vram_residency`] does not read the refresh as a
    /// clobber. Drains unconditionally so the queue cannot accumulate across
    /// a battle when no renderer is up.
    pub(in crate::window) fn tick_battle_effect_clut(&mut self) {
        let stages = self.session.host.world.drain_battle_clut_stages();
        if stages.is_empty() {
            return;
        }
        let Some(vram) = self.battle_vram.as_mut() else {
            return;
        };
        let mut dirty = false;
        for x in stages {
            dirty |= legaia_engine_battle::battle_effect_clut::stage_effect_clut(vram, x);
        }
        if !dirty {
            return;
        }
        if let (Some(r), Some(vram)) = (self.win.renderer.as_ref(), self.battle_vram.as_ref()) {
            match r.upload_vram(vram) {
                Ok(v) => {
                    self.battle_vram_generation = Some(v.generation());
                    self.uploaded_vram = Some(v);
                }
                Err(e) => log::error!("play-window: effect-CLUT VRAM re-upload: {e:#}"),
            }
        }
    }

    /// Stage-module render edits, mid-fight: the battle `MoveImage`s a stage
    /// module queued (`World::apply_battle_vram_moves` - the evolved-Cort
    /// arrival blanks the ground tile) and a changed backdrop object list
    /// (`SceneHost::battle_stage_object_indices` - the same arrival's slot-0
    /// rebind), which re-builds the stage shell meshes in place.
    ///
    /// Shares the mid-battle re-upload protocol with
    /// [`Self::tick_battle_effect_clut`].
    pub(in crate::window) fn tick_battle_stage_shell(&mut self) {
        let moved = match self.battle_vram.as_mut() {
            Some(vram) => self.session.host.world.apply_battle_vram_moves(vram),
            None => {
                self.session.host.world.discard_battle_vram_edits();
                false
            }
        };
        if moved
            && let (Some(r), Some(vram)) = (self.win.renderer.as_ref(), self.battle_vram.as_ref())
        {
            match r.upload_vram(vram) {
                Ok(v) => {
                    self.battle_vram_generation = Some(v.generation());
                    self.uploaded_vram = Some(v);
                }
                Err(e) => log::error!("play-window: stage-module VRAM re-upload: {e:#}"),
            }
        }
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

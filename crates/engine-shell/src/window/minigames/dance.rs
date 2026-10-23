//! The dance minigame on the play window: entry, the HUD art staging, and
//! the venue and cast uploads with their per-frame view refresh.

use super::*;

impl PlayWindowApp {
    /// Load the dance overlay and arm a run on `mode`'s floor. The world
    /// stages the pre-song **count-in** (retail `FUN_801cf470` runs its
    /// below-10 states - the `1 2 3 READY... GO!` banner - before the beat
    /// clock starts) and starts the song when it clears.
    ///
    /// Mirrors the disc-gated `dance_minigame_real` test's overlay path: read
    /// the raw PROT entry, lift it to its statically-recovered loaded form via
    /// [`static_overlay::as_loaded`], then parse through
    /// [`DanceGame::from_overlay_for_mode`].
    pub(in crate::window) fn start_dance_minigame_mode(
        &mut self,
        mode: legaia_engine_minigames::dance::DanceMode,
        long_song: bool,
    ) -> bool {
        use legaia_overlay_images::static_overlay;
        let Some(rec) = static_overlay::overlay_map()
            .by_prot_index(legaia_minigame_formats::dance_chart::DANCE_OVERLAY_PROT_INDEX as u32)
        else {
            log::warn!("dance: overlay 0980 absent from the static-overlay map");
            return false;
        };
        let raw = match self.session.host.index.entry_bytes_extended(rec.prot_index) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("dance: PROT {} read failed: {e:#}", rec.prot_index);
                return false;
            }
        };
        let loaded = match static_overlay::as_loaded(&raw, rec) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("dance: as_loaded failed: {e:#}");
                return false;
            }
        };
        match legaia_engine_minigames::dance::DanceGame::from_overlay_for_mode(
            &loaded, mode, long_song,
        ) {
            Some(mut game) => {
                // The hall's choreography bank times each judge move.
                if let Some(bank) = legaia_engine_minigame_scenes::dance_venue::dance_clip_bank(
                    &self.session.host.index,
                ) {
                    game.attach_clip_bank(&bank);
                }
                self.session.host.stage_dance_sfx_bundle();
                // `World::enter_dance` arms the count-in, the how-to tutorial
                // actor and the pending song id; the world's dance tick holds
                // the beat clock off until the banner clears. This window used
                // to hold the parsed game pending behind a count-in driver of
                // its own, which is why the door-warp entry - the one a player
                // reaches - had no count-in on either host.
                self.session.host.world.enter_dance(game);
                true
            }
            None => {
                log::warn!("dance: step-chart parse failed");
                false
            }
        }
    }
}

impl PlayWindowApp {
    /// Put the dance hall's own HUD texture page into the VRAM this window is
    /// drawing with for as long as a dance runs, and take it back on the way
    /// out.
    ///
    /// Retail has the page because the dance **is** the hall scene: its whole
    /// texture set is resident before the minigame starts. The port hosts the
    /// session over whichever scene the player walked in from, so the 4bpp
    /// page the widget table names holds that scene's texels and every HUD
    /// quad sampled nothing. The fix is residency, not a draw call - which is
    /// why the count-in banner had been text on both hosts with its geometry
    /// fully pinned.
    ///
    /// Only the rects the run's own widget table names are staged
    /// ([`DanceGame::hud_vram_rects`](legaia_engine_minigames::dance::DanceGame::hud_vram_rects)),
    /// because the suspended scene keeps rendering behind the HUD and the rest
    /// of that pack targets the columns a field texture pack occupies. The
    /// pristine copy is kept so the field VRAM is exact on exit rather than
    /// re-derived - the same shape the battle path uses for its own throwaway
    /// injection.
    pub(super) fn stage_dance_hud_art(&mut self) {
        // The venue owns its own VRAM, HUD page included: nothing to stage
        // over the field, and the residency claim is the venue build's.
        if let Some(g) = self.dance_venue_gpu.as_ref() {
            self.session.host.world.minigames.dance_hud_art_staged = g.hud_staged;
            return;
        }
        let in_dance = self.session.host.world.mode == SceneMode::Dance;
        if !in_dance {
            // Leaving edge: the field VRAM goes back byte for byte.
            if let Some(clean) = self.dance_vram_restore.take() {
                self.cpu_vram_base = Some(clean);
                self.upload_cpu_vram();
            }
            self.session.host.world.minigames.dance_hud_art_staged = false;
            return;
        }
        if self.dance_vram_restore.is_some() {
            return;
        }
        let Some(rects) = self
            .session
            .host
            .world
            .minigames
            .dance
            .as_ref()
            .map(|g| g.hud_vram_rects())
        else {
            return;
        };
        let Some(base) = self.cpu_vram_base.as_ref() else {
            return;
        };
        let clean = base.clone();
        let mut staged = base.clone();
        let n = legaia_engine_minigame_scenes::dance_hud_art::stage_dance_hud_vram(
            &self.session.host.index,
            &rects,
            &mut staged,
        );
        if n == 0 {
            // No page, no residency claim: the hosts fall back to the
            // placeholder letterforms together.
            log::warn!("play-window: dance HUD page not staged (PROT entry absent or unpacked 0)");
            return;
        }
        log::info!(
            "play-window: dance HUD page staged ({n} TIM(s), {} rect(s))",
            rects.len()
        );
        self.cpu_vram_base = Some(staged);
        self.upload_cpu_vram();
        self.dance_vram_restore = Some(clean);
        self.session.host.world.minigames.dance_hud_art_staged = true;
    }

    /// The dance entry's venue, once a frame: stage or restore the entry's
    /// globals through the shared kernel
    /// ([`legaia_engine_minigame_scenes::dance_venue::sync_dance_venue`] - block base,
    /// the session camera's view window, the venue camera the frame resolves
    /// through), then keep the GPU copy of the venue in step with the staging:
    /// built from [`legaia_engine_minigame_scenes::dance_venue::DanceVenue::build`] on a
    /// new generation, dropped the frame the teardown restores. The walked-in
    /// scene's render state is never rebuilt, so leaving the hall shows it
    /// exactly as it was.
    pub(in crate::window) fn sync_dance_venue(&mut self) {
        // The auto-end and the venue globals are the session's step
        // (`BootSession::step_dance_tail`), which the browser play page
        // runs too; what follows is this window's GPU copy.
        if let (Some(edge), _) = self.session.step_dance_tail() {
            log::info!("dance venue: {edge:?}");
        }
        let Some(generation) = self
            .session
            .host
            .world
            .minigames
            .dance_venue
            .map(|s| s.generation)
        else {
            self.dance_venue_gpu = None;
            if self.dance_cast_surface.has_assets() {
                self.dance_cast_surface.set_assets(None);
            }
            return;
        };
        if self
            .dance_venue_gpu
            .as_ref()
            .is_some_and(|g| g.generation == generation)
            || self.dance_venue_failed == Some(generation)
        {
            return;
        }
        match self.build_dance_venue_gpu(generation) {
            Some(g) => {
                log::info!(
                    "dance venue: {} textured + {} untextured triangles, hud page {}",
                    g.textured.indices.len() / 3,
                    g.untextured.indices.len() / 3,
                    g.hud_staged
                );
                self.dance_cast_surface.set_assets(g.cast_assets.clone());
                self.dance_venue_gpu = Some(g);
            }
            None => {
                log::warn!("dance venue: build failed - the walked-in scene stays on screen");
                self.dance_venue_failed = Some(generation);
                self.dance_venue_gpu = None;
            }
        }
    }

    pub(super) fn build_dance_venue_gpu(&self, generation: u32) -> Option<DanceVenueGpu> {
        use legaia_engine_minigame_scenes::dance_venue::DanceVenue;
        use legaia_overlay_images::static_overlay;
        let r = self.win.renderer.as_ref()?;
        let index = &self.session.host.index;
        let overlay = static_overlay::overlay_map()
            .by_prot_index(legaia_minigame_formats::dance_chart::DANCE_OVERLAY_PROT_INDEX as u32)
            .and_then(|rec| {
                let raw = index.entry_bytes_extended(rec.prot_index).ok()?;
                static_overlay::as_loaded(&raw, rec).ok()
            });
        let venue = DanceVenue::build(index, overlay.as_deref())?;
        // The floor's bodies draw off this same venue (its scene TMD pool and
        // choreography bank) plus kind 0's resident mesh (PROT 0874).
        let cast_assets = overlay
            .as_deref()
            .and_then(legaia_minigame_formats::dance_cast::parse)
            .and_then(|cast| {
                let pack = index
                    .entry_bytes(legaia_actor_formats::character_pack::PROT_ENTRY_INDEX)
                    .ok();
                legaia_engine_minigame_scenes::dance_cast_scene::DanceCastAssets::from_venue(
                    &venue,
                    &cast,
                    pack.as_deref().map(|p| p.as_slice()),
                )
            })
            .map(std::sync::Arc::new);
        let res = &venue.resources;
        let vram = r
            .upload_vram(&res.vram)
            .map_err(|e| log::warn!("dance venue: vram upload: {e:#}"))
            .ok()?;
        // One mesh build per (mesh, clip): a bound prop is baked at frame 0
        // of its clip, the rest pose every host draws it in.
        let mut built: std::collections::HashMap<
            (usize, u8),
            (legaia_tmd::mesh::VramMesh, legaia_tmd::mesh::ColorMesh),
        > = std::collections::HashMap::new();
        let mut textured = VenueTexturedBake::default();
        let mut untextured = VenueColorBake::default();
        for vd in &venue.draws {
            let d = vd.draw;
            let Some(rtmd) = res.tmds.get(d.res_tmd) else {
                continue;
            };
            let (vmesh, cmesh) = built.entry((d.res_tmd, d.anim_id)).or_insert_with(|| {
                let offsets = (d.anim_id != 0)
                    .then(|| venue.frame0_bone_offsets(d.anim_id, rtmd.tmd.objects.len()))
                    .flatten();
                let (mut vmesh, mut cmesh) = match &offsets {
                    Some(o) => (
                        legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(&rtmd.tmd, &rtmd.raw, o),
                        legaia_tmd::mesh::tmd_to_color_mesh_posed_rot(&rtmd.tmd, &rtmd.raw, o),
                    ),
                    None => (
                        rtmd.build_filtered_vram_mesh_lit(&res.vram).0,
                        legaia_tmd::mesh::tmd_to_color_mesh(&rtmd.tmd, &rtmd.raw),
                    ),
                };
                legaia_tmd::mesh::resolve_hybrid(&mut vmesh, &mut cmesh);
                (vmesh, cmesh)
            });
            // Raw retail Y-down transform; the camera's FIELD_WORLD_FLIP is
            // the single net negation, as for every field placement. The
            // composition is `EnvDraw::place_point` (`Rx * Ry * Rz`, then the
            // world position and the coplanar lift) - the one kernel the
            // browser pages bake the same hall with.
            let base = textured.positions.len() as u32;
            textured
                .positions
                .extend(vmesh.positions.iter().map(|p| d.place_point(*p, vd.lift)));
            textured.normals.extend(
                vmesh
                    .normals
                    .iter()
                    .map(|n| Vec3::from(d.rotate(*n)).normalize_or_zero().to_array()),
            );
            textured.uvs.extend_from_slice(&vmesh.uvs);
            textured.cba_tsb.extend_from_slice(&vmesh.cba_tsb);
            textured.colors.extend_from_slice(&vmesh.colors);
            textured
                .indices
                .extend(vmesh.indices.iter().map(|i| i + base));
            let base = untextured.positions.len() as u32;
            untextured
                .positions
                .extend(cmesh.positions.iter().map(|p| d.place_point(*p, vd.lift)));
            untextured.colors.extend_from_slice(&cmesh.colors);
            untextured.blend.extend_from_slice(&cmesh.blend);
            untextured
                .indices
                .extend(cmesh.indices.iter().map(|i| i + base));
        }
        // The walk-ground heightfield, already in world space.
        if let Some(hf) = venue
            .scene
            .walk_heightfield(index)
            .ok()
            .flatten()
            .filter(|hf| !hf.indices.is_empty())
        {
            let v = super::super::geometry::heightfield_to_vram_mesh(&hf);
            let base = textured.positions.len() as u32;
            textured.positions.extend_from_slice(&v.positions);
            textured.normals.extend_from_slice(&v.normals);
            textured.uvs.extend_from_slice(&v.uvs);
            textured.cba_tsb.extend_from_slice(&v.cba_tsb);
            textured.colors.extend_from_slice(&v.colors);
            textured.indices.extend(v.indices.iter().map(|i| i + base));
        }
        Some(DanceVenueGpu {
            generation,
            vram,
            hud_staged: venue.hud_staged,
            textured,
            untextured,
            textured_gpu: None,
            textured_kept: None,
            untextured_gpu: None,
            untextured_kept: None,
            cast_assets,
        })
    }

    /// Put the hall on the GPU for this frame's venue camera: only the
    /// triangles the PSX GPU draws from this eye
    /// ([`legaia_engine_minigame_scenes::dance_venue::psx_gpu_visible_indices`], the
    /// kernel the browser play page filters its baked hall through too).
    /// Re-uploads only when the kept set changes.
    pub(in crate::window) fn refresh_dance_venue_view(&mut self) {
        use legaia_engine_minigame_scenes::dance_venue::psx_gpu_visible_indices;
        let Some(camera) = self
            .session
            .host
            .world
            .minigames
            .dance_venue
            .as_ref()
            .map(|s| s.camera)
        else {
            return;
        };
        let (Some(r), Some(g)) = (self.win.renderer.as_ref(), self.dance_venue_gpu.as_mut()) else {
            return;
        };
        let t = &g.textured;
        let kept = psx_gpu_visible_indices(&camera, &t.positions, [0.0; 3], &t.indices);
        if g.textured_kept.as_ref() != Some(&kept) {
            g.textured_gpu = (!kept.is_empty())
                .then(|| {
                    r.upload_vram_mesh(
                        &t.positions,
                        &t.uvs,
                        &t.cba_tsb,
                        &t.normals,
                        &t.colors,
                        &kept,
                    )
                    .map_err(|e| log::warn!("dance venue: textured upload failed: {e:#}"))
                    .ok()
                })
                .flatten();
            g.textured_kept = Some(kept);
        }
        let c = &g.untextured;
        let kept = psx_gpu_visible_indices(&camera, &c.positions, [0.0; 3], &c.indices);
        if g.untextured_kept.as_ref() != Some(&kept) {
            g.untextured_gpu = (!kept.is_empty())
                .then(|| {
                    r.upload_color_mesh_blended(&c.positions, &c.colors, &kept, &c.blend)
                        .map_err(|e| log::warn!("dance venue: untextured upload failed: {e:#}"))
                        .ok()
                })
                .flatten();
            g.untextured_kept = Some(kept);
        }
    }

    /// Pose the dance floor's bodies for this frame and put them on the GPU.
    ///
    /// The cast, the clips and the pose are the engine's
    /// ([`legaia_engine_minigame_scenes::dance_cast_scene::DanceCastSurface::frame`] over
    /// `World::minigames.dance`, the call the browser play page makes too);
    /// this host uploads the posed buffers and the redraw's venue branch
    /// draws them under the venue camera. Dropped whenever no venue is up.
    pub(in crate::window) fn refresh_dance_cast_gpu(&mut self) {
        let game = if self.dance_venue_gpu.is_some() {
            self.session.host.world.minigames.dance.as_ref()
        } else {
            None
        };
        let Some(scene) = self.dance_cast_surface.frame(game) else {
            self.dance_cast_gpu = None;
            return;
        };
        let Some(r) = self.win.renderer.as_ref() else {
            return;
        };
        let normals = vec![[0.0f32; 3]; scene.positions.len()];
        let textured = (!scene.textured_indices.is_empty())
            .then(|| {
                r.upload_vram_mesh(
                    &scene.positions,
                    &scene.uvs,
                    &scene.cba_tsb,
                    &normals,
                    &scene.colors,
                    &scene.textured_indices,
                )
                .map_err(|e| log::warn!("dance cast: textured upload failed: {e:#}"))
                .ok()
            })
            .flatten();
        let fill: Vec<[u8; 3]> = scene
            .flat_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| [c[0], c[1], c[2]])
            .collect();
        let untextured = (!scene.untextured_indices.is_empty())
            .then(|| {
                r.upload_color_mesh(&scene.positions, &fill, &scene.untextured_indices)
                    .map_err(|e| log::warn!("dance cast: untextured upload failed: {e:#}"))
                    .ok()
            })
            .flatten();
        self.dance_cast_gpu = Some(DanceCastGpu {
            textured,
            untextured,
        });
    }
}

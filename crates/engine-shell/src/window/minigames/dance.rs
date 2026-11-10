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
        match legaia_engine_minigames::dance::DanceGame::from_overlay_for_mode(&loaded, mode) {
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
            self.session.host.world.restore_dance_ambient();
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
            self.step_dance_venue_ambient();
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
                self.stage_dance_venue_ambient();
            }
            None => {
                log::warn!("dance venue: build failed - the walked-in scene stays on screen");
                self.dance_venue_failed = Some(generation);
                self.dance_venue_gpu = None;
            }
        }
    }

    /// Install the hall's ambient tree off the venue block's stager bundle.
    fn stage_dance_venue_ambient(&mut self) {
        let bytes = legaia_engine_minigame_scenes::dance_venue::venue_stager_bytes(
            &self.session.host.index,
        );
        match bytes {
            Some(bytes) => {
                if let (Some(field), Some(g)) =
                    (self.cpu_vram_base.as_ref(), self.dance_venue_gpu.as_mut())
                {
                    self.session
                        .host
                        .world
                        .carry_walked_in_cells(field, &mut g.cpu_vram);
                }
                self.session.host.world.stage_dance_ambient(&bytes);
            }
            None => log::warn!("dance venue: no stager bundle, the hall stays unlit"),
        }
    }

    /// Run the hall's ambient tree on the venue's VRAM and put what it wrote
    /// on the GPU.
    fn step_dance_venue_ambient(&mut self) {
        let Some(g) = self.dance_venue_gpu.as_mut() else {
            return;
        };
        if !self.session.host.world.step_dance_ambient(&mut g.cpu_vram) {
            return;
        }
        if let Some(r) = self.win.renderer.as_ref() {
            match r.upload_vram(&g.cpu_vram) {
                Ok(v) => g.vram = v,
                Err(e) => log::warn!("dance venue: vram upload: {e:#}"),
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
                    self.session.host.world.party_field_equip(),
                )
            })
            .map(std::sync::Arc::new);
        let res = &venue.resources;
        let vram = r
            .upload_vram(&res.vram)
            .map_err(|e| log::warn!("dance venue: vram upload: {e:#}"))
            .ok()?;
        let mut built = VenueMeshCache::new();
        let mut textured = VenueTexturedBake::default();
        let mut untextured = VenueColorBake::default();
        for vd in &venue.draws {
            append_venue_draw(&venue, &mut built, vd, &mut textured, &mut untextured);
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
        let markers = bake_venue_markers(&venue, &mut built);
        let props = venue
            .props
            .iter()
            .map(|vd| super::VenuePropGpu {
                venue_draw: *vd,
                baked: std::collections::HashMap::new(),
                shown: None,
                textured_gpu: None,
                untextured_gpu: None,
            })
            .collect();
        Some(DanceVenueGpu {
            generation,
            vram,
            cpu_vram: venue.resources.vram.clone(),
            hud_staged: venue.hud_staged,
            textured,
            untextured,
            textured_gpu: None,
            textured_kept: None,
            untextured_gpu: None,
            untextured_kept: None,
            markers,
            props,
            venue: Some(venue),
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
        // The step-marker tiles, on the mesh each one's flipbook staged.
        let game = self.session.host.world.minigames.dance.as_ref();
        for m in &mut g.markers {
            let bias = legaia_scene_formats::field_objects::FIELD_ACTOR_PACK_BIAS as i16;
            let slot = game
                .and_then(|d| d.step_marker_mesh(m.class))
                .map(|mesh| mesh - bias)
                .filter(|s| m.baked.contains_key(s))
                .unwrap_or(m.rest_slot);
            let Some((t, c)) = m.baked.get(&slot) else {
                continue;
            };
            let kept_t = psx_gpu_visible_indices(&camera, &t.positions, [0.0; 3], &t.indices);
            let kept_c = psx_gpu_visible_indices(&camera, &c.positions, [0.0; 3], &c.indices);
            if m.shown
                .as_ref()
                .is_some_and(|(s, kt, kc)| *s == slot && *kt == kept_t && *kc == kept_c)
            {
                continue;
            }
            m.textured_gpu = (!kept_t.is_empty())
                .then(|| {
                    r.upload_vram_mesh(
                        &t.positions,
                        &t.uvs,
                        &t.cba_tsb,
                        &t.normals,
                        &t.colors,
                        &kept_t,
                    )
                    .map_err(|e| log::warn!("dance marker: textured upload failed: {e:#}"))
                    .ok()
                })
                .flatten();
            m.untextured_gpu = (!kept_c.is_empty())
                .then(|| {
                    r.upload_color_mesh_blended(&c.positions, &c.colors, &kept_c, &c.blend)
                        .map_err(|e| log::warn!("dance marker: untextured upload failed: {e:#}"))
                        .ok()
                })
                .flatten();
            m.shown = Some((slot, kept_t, kept_c));
        }
        // The animated props, on the run's clip clock (frame `0` without a
        // run).
        let hall_props = game.map(|d| d.hall_props());
        let Some(venue) = g.venue.as_ref() else {
            return;
        };
        for p in &mut g.props {
            let key = legaia_engine_minigame_scenes::dance_venue::prop_pose_key(
                p.venue_draw.draw.anim_id,
                hall_props,
            );
            let (t, c) = p.baked.entry(key).or_insert_with(|| {
                let (mut t, mut c) = Default::default();
                append_venue_prop(venue, &p.venue_draw, key, &mut t, &mut c);
                (t, c)
            });
            let kept_t = psx_gpu_visible_indices(&camera, &t.positions, [0.0; 3], &t.indices);
            let kept_c = psx_gpu_visible_indices(&camera, &c.positions, [0.0; 3], &c.indices);
            if p.shown
                .as_ref()
                .is_some_and(|(s, kt, kc)| *s == key && *kt == kept_t && *kc == kept_c)
            {
                continue;
            }
            p.textured_gpu = (!kept_t.is_empty())
                .then(|| {
                    r.upload_vram_mesh(
                        &t.positions,
                        &t.uvs,
                        &t.cba_tsb,
                        &t.normals,
                        &t.colors,
                        &kept_t,
                    )
                    .map_err(|e| log::warn!("dance prop: textured upload failed: {e:#}"))
                    .ok()
                })
                .flatten();
            p.untextured_gpu = (!kept_c.is_empty())
                .then(|| {
                    r.upload_color_mesh_blended(&c.positions, &c.colors, &kept_c, &c.blend)
                        .map_err(|e| log::warn!("dance prop: untextured upload failed: {e:#}"))
                        .ok()
                })
                .flatten();
            p.shown = Some((key, kept_t, kept_c));
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
                r.upload_color_mesh_blended(
                    &scene.positions,
                    &fill,
                    &scene.untextured_indices,
                    &legaia_engine_render::psx_blend::blend_words_of_tsb(&scene.cba_tsb),
                )
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

/// One mesh build per `(mesh, clip)`: a bound prop is baked at frame 0 of its
/// clip, the rest pose every host draws it in.
type VenueMeshCache = std::collections::HashMap<
    (usize, u8),
    (legaia_tmd::mesh::VramMesh, legaia_tmd::mesh::ColorMesh),
>;

/// Bake one of the venue's draws into world space. Raw retail Y-down
/// transform; the camera's `FIELD_WORLD_FLIP` is the single net negation, as for
/// every field placement. The composition is `EnvDraw::place_point`
/// (`Rx * Ry * Rz`, then the world position and the coplanar lift) - the one
/// kernel the browser pages bake the same hall with.
fn append_venue_draw(
    venue: &legaia_engine_minigame_scenes::dance_venue::DanceVenue,
    built: &mut VenueMeshCache,
    vd: &legaia_engine_minigame_scenes::dance_venue::VenueDraw,
    textured: &mut VenueTexturedBake,
    untextured: &mut VenueColorBake,
) {
    let res = &venue.resources;
    let d = vd.draw;
    let Some(rtmd) = res.tmds.get(d.res_tmd) else {
        return;
    };
    let (vmesh, cmesh) = built.entry((d.res_tmd, d.anim_id)).or_insert_with(|| {
        let offsets = (d.anim_id != 0)
            .then(|| venue.frame0_bone_offsets(d.anim_id, rtmd.tmd.objects.len()))
            .flatten();
        venue_draw_meshes(res, rtmd, offsets.as_deref())
    });
    place_venue_meshes(vmesh, cmesh, vd, textured, untextured);
}

/// One draw's two halves, posed by `offsets` when it has them.
fn venue_draw_meshes(
    res: &legaia_engine_scene::scene_resources::SceneResources,
    rtmd: &legaia_engine_scene::scene_resources::ResolvedTmd,
    offsets: Option<&[([i16; 3], [i16; 3])]>,
) -> (legaia_tmd::mesh::VramMesh, legaia_tmd::mesh::ColorMesh) {
    let (mut vmesh, mut cmesh) = match offsets {
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
}

/// Bake one animated hall prop at its place, posed at `key`
/// (`DanceVenue::prop_offsets_at`).
fn append_venue_prop(
    venue: &legaia_engine_minigame_scenes::dance_venue::DanceVenue,
    vd: &legaia_engine_minigame_scenes::dance_venue::VenueDraw,
    key: legaia_engine_scene::field_env::PropPoseKey,
    textured: &mut VenueTexturedBake,
    untextured: &mut VenueColorBake,
) {
    let res = &venue.resources;
    let Some(rtmd) = res.tmds.get(vd.draw.res_tmd) else {
        return;
    };
    let offsets = venue.prop_offsets_at(&vd.draw, key);
    let (vmesh, cmesh) = venue_draw_meshes(res, rtmd, offsets.as_deref());
    place_venue_meshes(&vmesh, &cmesh, vd, textured, untextured);
}

/// Append one draw's posed halves at its place.
fn place_venue_meshes(
    vmesh: &legaia_tmd::mesh::VramMesh,
    cmesh: &legaia_tmd::mesh::ColorMesh,
    vd: &legaia_engine_minigame_scenes::dance_venue::VenueDraw,
    textured: &mut VenueTexturedBake,
    untextured: &mut VenueColorBake,
) {
    let d = vd.draw;
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

/// Bake each marker tile's meshes at its place: the record's own, and every
/// mesh of its flipbook row, a slot of the scene's environment pack.
fn bake_venue_markers(
    venue: &legaia_engine_minigame_scenes::dance_venue::DanceVenue,
    built: &mut VenueMeshCache,
) -> Vec<VenueMarkerGpu> {
    let env_tmds =
        legaia_engine_scene::field_env::env_pack_tmd_indices(&venue.scene, &venue.resources);
    let script = &venue.marker_script;
    venue
        .markers
        .iter()
        .map(|m| {
            let class = usize::from(m.class);
            let rest_slot = m.venue_draw.draw.env_slot as i16;
            let mut baked = std::collections::HashMap::new();
            let row = (0..script.steps(class)).filter_map(|i| script.entry(class, i).map(|e| e.0));
            for slot in row.chain([rest_slot]) {
                let Some(&res_tmd) = usize::try_from(slot).ok().and_then(|s| env_tmds.get(s))
                else {
                    continue;
                };
                baked.entry(slot).or_insert_with(|| {
                    let mut vd = m.venue_draw;
                    vd.draw.env_slot = slot as usize;
                    vd.draw.res_tmd = res_tmd;
                    let (mut t, mut c) = Default::default();
                    append_venue_draw(venue, built, &vd, &mut t, &mut c);
                    (t, c)
                });
            }
            VenueMarkerGpu {
                class,
                rest_slot,
                baked,
                shown: None,
                textured_gpu: None,
                untextured_gpu: None,
            }
        })
        .collect()
}

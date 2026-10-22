//! Fishing on the play window: the banner and venue-actor ticks, the HUD and
//! line prims, the venue upload and the minigame entry.

use super::*;

impl PlayWindowApp {
    /// Drive the fishing HUD's one-shot banner animations for this tick and
    /// cache the draws for the HUD builder, which is `&self` and cannot
    /// advance them itself. The step is the shared one
    /// (`legaia_engine_screens::fishing_banner_frame`): the session's events
    /// this tick seed the timers, each timer retires itself, and leaving the
    /// minigame drops any half-run banner with the session.
    pub(in crate::window) fn tick_fishing_banners(&mut self) {
        self.fishing_banner_draws = legaia_engine_screens::fishing_banner_frame(
            &self.session.host.world,
            &mut self.fishing_banners,
        );
    }
}

impl PlayWindowApp {
    /// The fishing venue's actor-side frame: the free-swimming fish wander
    /// and its retarget ripple, the venue floor solve, the retail camera
    /// publish, the reeling-line actor and its catch bursts, and the
    /// sub-screen idle sway.
    ///
    /// All of it is the shared engine kernel
    /// ([`legaia_engine_scene_host::fishing_venue::tick_fishing_venue_on_host`]),
    /// whose actors live on `World::minigames.fishing_venue`; the browser play
    /// page runs the same call. This host only applies the camera writes to
    /// its own session camera.
    pub(in crate::window) fn tick_fishing_actors(&mut self) {
        let writes = legaia_engine_scene_host::fishing_venue::tick_fishing_venue_on_host(
            &mut self.session.host,
        );
        writes.apply(&mut self.session.camera);
    }
}

impl PlayWindowApp {
    /// This frame's fishing HUD draw list
    /// (`legaia_engine_screens::fishing_hud_items`, the list the browser play
    /// page builds too), with this frame's live banners serviced in the
    /// redraw handler. The text consumer and the sprite pass read the same
    /// list. Empty outside a fishing session.
    pub(in crate::window) fn fishing_hud_items(&self) -> Vec<legaia_engine_ui::HudDraw> {
        let world = &self.session.host.world;
        if world.mode != SceneMode::Fishing {
            return Vec::new();
        }
        legaia_engine_screens::fishing_hud_items(world, &self.fishing_banner_draws)
    }

    /// The sprite half of the fishing HUD as screen primitives: every plate,
    /// digit, gauge cap and banner cut from the overlay's sprite table
    /// (`FUN_801D63B0`, through the shared `fishing::sprite` builder) out of
    /// the venue's HUD page, which the pond VRAM this window uploads for the
    /// session already holds. The browser play page makes the same call.
    /// Empty without a session or a decoded table.
    pub(in crate::window) fn fishing_hud_screen_prims(
        &self,
    ) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
        let Some(table) = self.session.host.world.minigames.fishing_sprites.as_deref() else {
            return Vec::new();
        };
        legaia_engine_ui::fishing::sprite::fishing_hud_sprite_prims(
            &self.fishing_hud_items(),
            table,
        )
    }

    /// The fishing rod and line as screen primitives
    /// (`legaia_engine_screens::fishing_line_screen_prims`, the call the
    /// browser play page makes too), the line projected through this
    /// window's session camera framed on the scene's centre.
    pub(in crate::window) fn fishing_line_screen_prims(
        &mut self,
    ) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
        let center = legaia_engine_world_views::screen_passes::scene_centre(self.scene_aabb);
        legaia_engine_screens::fishing_line_screen_prims(
            &mut self.session.host.world,
            &self.session.camera,
            center,
        )
    }
}

impl PlayWindowApp {
    /// Pose the fishing pond - the `other1` venue with the party on its
    /// shore seats (`legaia_engine_minigame_scenes::fishing_scene::FishingSurface`, the
    /// kernel the browser play page drives too) - and put it on the GPU. The
    /// redraw draws it in place of the walked-in field, which stays loaded
    /// underneath and is shown again unchanged when the session ends.
    pub(in crate::window) fn refresh_fishing_gpu(&mut self) {
        let world = &self.session.host.world;
        let live = world.mode == SceneMode::Fishing;
        let generation_before = self.fishing_surface.generation();
        if self
            .fishing_surface
            .frame(&self.session.host.index, &world.minigames, live)
            .is_none()
        {
            self.fishing_gpu = None;
            return;
        }
        let (Some(r), Some(scene)) = (self.win.renderer.as_ref(), self.fishing_surface.scene())
        else {
            return;
        };
        let generation = self.fishing_surface.generation();
        let (sw, sh) = r.surface_size();
        let (_, aspect) = super::super::geometry::scene_viewport_for(sw, sh);
        let mvp = Mat4::from_cols_array(&scene.vp_raw(aspect));
        let normals = vec![[0.0f32; 3]; scene.positions.len()];
        let textured = r
            .upload_vram_mesh(
                &scene.positions,
                &scene.uvs,
                &scene.cba_tsb,
                &normals,
                &scene.colors,
                &scene.textured_indices,
            )
            .map_err(|e| log::warn!("fishing pond: textured upload failed: {e:#}"))
            .ok();
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
                    .map_err(|e| log::warn!("fishing pond: untextured upload failed: {e:#}"))
                    .ok()
            })
            .flatten();
        let stale = generation != generation_before
            || self
                .fishing_gpu
                .as_ref()
                .is_none_or(|g| g.generation != generation);
        let vram = if stale {
            match self.fishing_surface.vram().map(|v| r.upload_vram(v)) {
                Some(Ok(v)) => v,
                Some(Err(e)) => {
                    log::warn!("fishing pond: vram upload failed: {e:#}");
                    return;
                }
                None => return,
            }
        } else {
            match self.fishing_gpu.take() {
                Some(g) => g.vram,
                None => return,
            }
        };
        self.fishing_gpu = Some(BakaDuelGpu {
            generation,
            vram,
            textured,
            untextured,
            mvp,
        });
    }
}

impl PlayWindowApp {
    /// Load the fishing overlay (PROT 0972) and start a fishing session in the
    /// world (suspending the current scene) through the same engine entry the
    /// mode-24 door warp takes (`SceneHost::enter_fishing_from_overlay`): the
    /// species / spawn / cadence tables, the bring-up's rod and lure ownership
    /// scans, the persistent save-block words and the venue the cast lure
    /// lands in. Returns `false` (and logs) when no disc is attached or the
    /// tables can't decode. Mirrors [`Self::start_dance_minigame_mode`]'s overlay
    /// path.
    pub(in crate::window) fn start_fishing_minigame(&mut self) -> bool {
        use legaia_overlay_images::static_overlay;
        let Some(rec) = static_overlay::overlay_map().by_prot_index(
            legaia_minigame_formats::fishing_species::FISHING_OVERLAY_PROT_INDEX as u32,
        ) else {
            log::warn!("fishing: overlay 0972 absent from the static-overlay map");
            return false;
        };
        let raw = match self.session.host.index.entry_bytes_extended(rec.prot_index) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("fishing: PROT {} read failed: {e:#}", rec.prot_index);
                return false;
            }
        };
        let loaded = match static_overlay::as_loaded(&raw, rec) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("fishing: as_loaded failed: {e:#}");
                return false;
            }
        };
        if !self.session.host.enter_fishing_from_overlay(&loaded) {
            log::warn!("fishing: species / spawn / cadence tables did not decode");
            return false;
        }
        true
    }
}

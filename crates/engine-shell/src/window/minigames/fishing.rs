//! Fishing on the play window: the banner and venue-actor ticks, the HUD and
//! line prims, the venue upload and the minigame entry.

use super::*;

impl PlayWindowApp {
    /// Drive the fishing HUD's one-shot banner animations for this frame.
    ///
    /// Seeds a timer on each of the session's events this tick
    /// (`World::minigames.fishing_events`: cadence splash, hook, landed,
    /// snapped, recast), then services every timer through the retail
    /// driver-tail loop
    /// ([`BannerTimer::service`](legaia_engine_render::BannerTimer::service))
    /// and caches this frame's draws for the HUD builder, which is `&self` and
    /// cannot advance them itself.
    ///
    /// The frame step is the engine's fixed one tick per frame (retail reads
    /// `DAT_1f800393`, its frame-rate compensation word).
    pub(in crate::window) fn tick_fishing_banners(&mut self) {
        use legaia_engine_fishing::fishing::PondEvent;
        let world = &self.session.host.world;
        if world.minigames.fishing.is_none() {
            // Left the minigame: drop any half-run banner with the session.
            self.fishing_banners = Default::default();
            self.fishing_banner_draws.clear();
            return;
        }
        for e in &world.minigames.fishing_events {
            match e {
                PondEvent::Splash => self.fishing_banners.splash.start(),
                PondEvent::Hooked(_) => self.fishing_banners.on_hook(),
                PondEvent::Landed(_) => self.fishing_banners.on_landed(),
                PondEvent::Snapped => self.fishing_banners.on_snapped(),
                PondEvent::Recast => self.fishing_banners.on_recast(),
            }
        }
        self.fishing_banner_draws = self.fishing_banners.service_frame(1);
    }
}

impl PlayWindowApp {
    /// The fishing venue's actor-side frame: the free-swimming fish wander
    /// and its retarget ripple, the venue floor solve, the retail camera
    /// publish, the reeling-line actor and its catch bursts, and the
    /// sub-screen idle sway.
    ///
    /// All of it is the shared engine kernel
    /// ([`legaia_engine_core::fishing_venue::tick_fishing_venue_on_host`]),
    /// whose actors live on `World::minigames.fishing_venue`; the browser play
    /// page runs the same call. This host only applies the camera writes to
    /// its own session camera.
    pub(in crate::window) fn tick_fishing_actors(&mut self) {
        let writes =
            legaia_engine_core::fishing_venue::tick_fishing_venue_on_host(&mut self.session.host);
        writes.apply(&mut self.session.camera);
    }
}

impl PlayWindowApp {
    /// This frame's fishing HUD draw list: the retail persistent rows
    /// (best-catch, capped point total, lure label, lures remaining) at their
    /// traced stage-pixel pens, the catch HUD while a cast is out, and the
    /// live one-shot banners. The text consumer and the sprite pass read the
    /// same list. Empty outside a fishing session.
    pub(in crate::window) fn fishing_hud_items(&self) -> Vec<legaia_engine_ui::HudDraw> {
        let world = &self.session.host.world;
        if world.mode != SceneMode::Fishing {
            return Vec::new();
        }
        let Some(s) = world.minigames.fishing.as_ref() else {
            return Vec::new();
        };
        // The lure index is the session's - the entry's ownership gate
        // already re-pointed it at an owned lure.
        let lure = s.lure;
        let lures_left = *world
            .party
            .inventory
            .get(&(legaia_engine_fishing::fishing::lure_item_id(lure) as u8))
            .unwrap_or(&0) as i32;
        let mut items = legaia_engine_ui::persistent_hud_draws(
            s.record.points,
            s.record.best_points,
            lure,
            lures_left,
        );
        // The catch HUD, drawn over the persistent rows while a cast is out
        // (`PondSession::catch_hud`). The cast line-projection term
        // `DAT_801d9178` has no engine analogue and stays zero.
        let c = s.catch_hud();
        if c.visible {
            items.extend(legaia_engine_ui::catch_hud_draws(
                &legaia_engine_ui::CatchHudState {
                    record: c.record,
                    line_extent: 0,
                    cast_power: c.cast_power,
                    depth: c.depth,
                    tension: c.tension,
                    gauges_visible: c.gauges_visible,
                },
            ));
        }
        // A landed catch's result plate (`FUN_801D5298`): rank, points in
        // the large digits, label and the rising species name.
        if let Some(r) = s.catch_result() {
            let name_len = world
                .minigames
                .fishing_captions
                .as_ref()
                .map_or(0, |t| t.species_name(r.species).len());
            items.extend(legaia_engine_ui::catch_result_draws(
                r.ramp,
                r.strength,
                r.points,
                r.species as u32,
                name_len,
            ));
        }
        // This frame's live banners, serviced in the redraw handler.
        items.extend(self.fishing_banner_draws.iter().copied());
        items
    }

    /// The sprite half of the fishing HUD as screen primitives: every plate,
    /// digit, gauge cap and banner cut from the overlay's sprite table
    /// (`FUN_801D63B0`, through the shared `ui_fishing_sprite` builder) out of
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

    /// The fishing rod and line as screen primitives: the rod model the rod
    /// actor posed this frame (`PondSession::rod_faces`, wrapped by the shared
    /// `ui_fishing_rod` builder), then the session's line for this frame
    /// (`legaia_engine_core::fishing_venue::fishing_line_frame`, the fish end
    /// projected through the follow camera the scene draws with), wrapped by
    /// the shared `ui_fishing_line` builder. The browser play page makes the
    /// same calls. Empty with no rod and no line out.
    pub(in crate::window) fn fishing_line_screen_prims(
        &mut self,
    ) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
        use legaia_engine_core::camera_view::resolve_field_camera;
        if self.session.host.world.mode != SceneMode::Fishing {
            return Vec::new();
        }
        let center = [
            (self.scene_aabb.0[0] + self.scene_aabb.1[0]) * 0.5,
            (self.scene_aabb.0[2] + self.scene_aabb.1[2]) * 0.5,
        ];
        let world = &mut self.session.host.world;
        // The pond draws under the venue camera; the line projects through it.
        let view = legaia_engine_core::fishing_venue::venue_view(&world.minigames).or_else(|| {
            resolve_field_camera(world, &self.session.camera, None, center).field_view()
        });
        // The rod model first: its actor runs ahead of the lure tick, so in a
        // shared bucket its packets are the earlier `AddPrim`s.
        let mut prims: Vec<_> = world
            .minigames
            .fishing
            .as_ref()
            .map(|p| p.rod_faces())
            .unwrap_or_default()
            .into_iter()
            .map(|f| legaia_engine_ui::fishing::rod::fishing_rod_prim(f.xy, f.rgb, f.ot))
            .collect();
        prims.extend(
            legaia_engine_core::fishing_venue::fishing_line_frame(
                &mut world.minigames,
                view.as_ref(),
            )
            .and_then(|l| {
                legaia_engine_ui::fishing::line::fishing_line_prim(
                    l.fish, l.rod, l.fish_rgb, l.rod_rgb, l.ot,
                )
            }),
        );
        prims
    }
}

impl PlayWindowApp {
    /// Pose the fishing pond - the `other1` venue with the party on its
    /// shore seats (`legaia_engine_core::fishing_scene::FishingSurface`, the
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
    /// tables can't decode. Mirrors [`Self::start_dance_minigame`]'s overlay
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

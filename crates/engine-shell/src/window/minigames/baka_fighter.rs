//! The Baka Fighter duel on the play window: its SFX cue drain, the chrome
//! tick, the HUD prims and widget table, and the duel surface upload.

use super::*;

impl PlayWindowApp {
    // The mode-24 minigame door warp (`World::arm_minigame_warp` /
    // `World::minigame_return_warp`, retail `FUN_80025980` / `FUN_80026018`)
    // is not driven from these entry points - it runs one layer down, inside
    // `World::enter_baka_fighter` / `World::exit_baka_fighter`, where the
    // producer it depends on lives.
    //
    // `FUN_80026018` banks the mode-24 winnings accumulator `_DAT_80084440`
    // into the casino coin bank `_DAT_800845A4` (`0x80026050..0x80026078`,
    // clamped at 9,999,999). What fills that accumulator is the Baka Fighter
    // end-of-match tally: `FUN_801D239C` at `0x801d2894..0x801d28bc` adds each
    // drained step into `0x80084440` - the coin prize, not party gold
    // (`0x8008459C`). The engine's duel tick pays the same drain into
    // `World::minigames.winnings`, so the warp's commit has something to bank.
    // REF: FUN_80026018 (coin-bank commit), FUN_801d239c (the producer)
}

impl PlayWindowApp {
    /// Consume the Baka Fighter round-chrome frame the duel produced this
    /// tick (`BakaFight` owns the [`BakaChrome`] runner and steps it inside
    /// `tick_with_input`; the round banners start at its own round ends) and
    /// resolve each glyph draw's `u` stamp against the overlay's parsed
    /// widget table ([`legaia_engine_minigames::baka_fighter_chrome::glyph_u`]).
    ///
    /// [`BakaChrome`]: legaia_engine_minigames::baka_fighter_chrome::BakaChrome
    pub(in crate::window) fn tick_baka_chrome(&mut self) {
        use legaia_engine_minigames::baka_fighter_chrome as bc;
        if self.session.host.world.mode != SceneMode::BakaFighter {
            self.baka_chrome_frame.clear();
            return;
        }
        let Some(f) = self.session.host.world.minigames.baka_fighter.as_ref() else {
            return;
        };
        let frame = f.chrome_frame().clone();
        // The announcer line the chrome fired (`FUN_8003D53C`): the session's
        // step, which the browser play page runs too.
        self.session.sound_baka_announcer();
        // The overlay's HUD widget table, for the glyph-strip paging below.
        // Parsed on the first duel frame whatever opened the duel - the door
        // warp and the `B` launcher enter through the same scene-host arm,
        // which keeps no render tables.
        if self.baka_hud_widgets.is_none() {
            self.baka_hud_widgets = self.baka_overlay_hud_widgets();
        }
        // Resolve the draws: a glyph-carrying draw pages the glyph strip by
        // stamping `u = glyph_u(index)` into widget 5's record - performed
        // here against the parsed table, exactly where retail's emitter does
        // the byte store.
        let widgets = self.baka_hud_widgets.as_deref();
        self.baka_chrome_frame = frame
            .draws
            .iter()
            .map(|d| {
                // The stamped cell rect: widget 5's record with its `u`
                // paged to the glyph index. The page's texels are not
                // uploaded; the rect is the future atlas source.
                let stamped = d.glyph.and_then(|idx| {
                    let u = bc::glyph_u(idx);
                    widgets
                        .and_then(|t| t.get(bc::GLYPH_WIDGET as usize))
                        .map(|w| (u, w.v, w.w, w.h))
                });
                (*d, stamped)
            })
            .collect();
    }

    /// Whether this frame draws the Baka cabinet / round-chrome widgets as
    /// retail quads: a duel is up, its VRAM is on the GPU, and that VRAM
    /// carries the PROT 1203 pages the widget table samples
    /// (`BakaDuelSurface::hud_art_resident`). `hud.rs` falls back to the
    /// text labels when it does not hold.
    pub(in crate::window) fn baka_hud_art_drawn(&self) -> bool {
        self.session.host.world.mode == SceneMode::BakaFighter
            && self.baka_gpu.is_some()
            && self.baka_surface.hud_art_resident()
    }

    /// The Baka Fighter screen layer as screen-space PSX primitives against
    /// the duel VRAM: the duel HUD (`FUN_801d2afc` - bars, pips, combo,
    /// attack icons, the STAGE strip) and the cabinet's and round chrome's
    /// widgets (`FUN_801d5ed0`), through the one shared builder
    /// (`minigame_hud::baka_screen_prims`) the browser play page calls too.
    /// Empty unless [`Self::baka_hud_art_drawn`].
    pub(in crate::window) fn baka_hud_prims(
        &self,
    ) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
        if !self.baka_hud_art_drawn() {
            return Vec::new();
        }
        let Some(f) = self.session.host.world.minigames.baka_fighter.as_ref() else {
            return Vec::new();
        };
        legaia_engine_screens::minigame_hud::baka_screen_prims(&self.baka_surface, f)
    }

    /// PROT 0976's HUD widget table in its loaded form, or `None` when the
    /// overlay does not resolve.
    pub(super) fn baka_overlay_hud_widgets(
        &self,
    ) -> Option<Vec<legaia_minigame_formats::baka_opponents::BakaHudWidget>> {
        use legaia_overlay_images::static_overlay;
        let rec = static_overlay::overlay_map().by_prot_index(
            legaia_minigame_formats::baka_opponents::BAKA_OVERLAY_PROT_INDEX as u32,
        )?;
        let raw = self
            .session
            .host
            .index
            .entry_bytes_extended(rec.prot_index)
            .ok()?;
        let loaded = static_overlay::as_loaded(&raw, rec).ok()?;
        legaia_minigame_formats::baka_opponents::parse_baka_hud(&loaded)
    }
}

impl PlayWindowApp {
    /// Pose the Baka duel's 3D surface for this frame and put it on the GPU.
    ///
    /// The pose, the arena camera and the buffers are the engine's
    /// (`legaia_engine_minigame_scenes::baka_duel_scene::BakaDuelSurface::frame`, the
    /// call the browser play page makes too); this host uploads the duel
    /// VRAM on a generation change and the posed meshes every frame, and
    /// the redraw's duel branch draws them under `DuelCamera::vp_raw`.
    /// Drops the GPU copy whenever no duel is on screen.
    pub(in crate::window) fn refresh_baka_duel_gpu(&mut self) {
        let live = self.session.host.world.mode == SceneMode::BakaFighter;
        let index = self.session.host.index.clone();
        let read = |i: usize| index.entry_bytes(i as u32).ok().map(|b| b.to_vec());
        let fight = if live {
            self.session.host.world.minigames.baka_fighter.as_ref()
        } else {
            None
        };
        let generation_before = self.baka_surface.generation();
        if self.baka_surface.frame(read, fight).is_none() {
            self.baka_gpu = None;
            return;
        }
        let (Some(r), Some(scene)) = (self.win.renderer.as_ref(), self.baka_surface.scene()) else {
            return;
        };
        let generation = self.baka_surface.generation();
        let (sw, sh) = r.surface_size();
        let (_, aspect) = super::super::geometry::scene_viewport_for(sw, sh);
        let mvp = Mat4::from_cols_array(
            &fight
                .map(|f| f.duel_camera().vp_raw(aspect))
                .unwrap_or(Mat4::IDENTITY.to_cols_array()),
        );
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
            .map_err(|e| log::warn!("baka duel: textured upload failed: {e:#}"))
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
                    .map_err(|e| log::warn!("baka duel: untextured upload failed: {e:#}"))
                    .ok()
            })
            .flatten();
        let stale = generation != generation_before
            || self
                .baka_gpu
                .as_ref()
                .is_none_or(|g| g.generation != generation);
        let vram = if stale {
            match self.baka_surface.vram().map(|v| r.upload_vram(&v)) {
                Some(Ok(v)) => v,
                Some(Err(e)) => {
                    log::warn!("baka duel: vram upload failed: {e:#}");
                    return;
                }
                None => return,
            }
        } else {
            match self.baka_gpu.take() {
                Some(g) => g.vram,
                None => return,
            }
        };
        self.baka_gpu = Some(BakaDuelGpu {
            generation,
            vram,
            textured,
            untextured,
            mvp,
        });
    }
}

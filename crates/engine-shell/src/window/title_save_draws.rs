//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;

impl PlayWindowApp {
    /// Build the per-quad [`legaia_engine_render::SpriteDraw`] list for
    /// the active publisher logo.
    ///
    /// Neither the layout nor the letterboxing is computed here: `LOGO_QUADS`
    /// carries retail's own per-logo rects in the 640x480 stage the boot pass
    /// runs in (`FUN_801CE9C0` selects it with `FUN_8001DAF8(0x400)`), and the
    /// stage-into-surface fit is the shared
    /// `legaia_engine_ui::ui_boot_logos::publisher_logo_sprite_draws` the
    /// browser play page's own logo stage draws through. This only resolves
    /// which logo is up. Returns an empty vec when boot-UI isn't
    /// `PublisherLogos` or the atlas wasn't uploaded.
    pub(super) fn publisher_logo_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_render::SpriteDraw> {
        use legaia_engine_core::publisher_logos::STAGE;
        use legaia_engine_render::ui_boot_logos::{LogoQuadView, publisher_logo_sprite_draws};

        let BootUiState::PublisherLogos(session) = &self.boot_ui else {
            return Vec::new();
        };
        let Some(assets) = self.publisher_logos.as_ref() else {
            return Vec::new();
        };
        let idx = session.current_logo();
        if idx >= legaia_engine_core::publisher_logos::LOGO_COUNT {
            return Vec::new();
        }
        let quads: Vec<LogoQuadView> = session
            .current_quads()
            .iter()
            .map(|q| LogoQuadView {
                src: q.src,
                dst: q.dst,
            })
            .collect();
        publisher_logo_sprite_draws(
            &quads,
            assets.rects[idx],
            STAGE,
            session.alpha(),
            surface_w,
            surface_h,
        )
    }

    /// Canonical PSX-framebuffer (320×240) stage origin + scale, shared
    /// by every boot-UI element (title art, save-select chrome, slot
    /// pills, cursor, menu glyphs). Every retail-pinned position is
    /// expressed in 320×240 framebuffer pixels, so this is the single
    /// stage transform that maps them to screen coords. Using the same
    /// stage for the title art AND the save-select panel ensures
    /// relative positions remain correct at any window resolution.
    pub(super) fn save_select_stage(&self, surface_w: u32, surface_h: u32) -> ((i32, i32), u32) {
        // Shared with the browser play page rather than mirrored - the two
        // copies of this arithmetic were a paired constant with no gate over
        // them.
        legaia_engine_render::pause_menu::stage_transform(surface_w, surface_h)
    }

    /// Build the [`legaia_engine_render::SpriteDraw`] list for the
    /// retail save-screen chrome (panel frame + slot pills). Anchored
    /// at the same 256×256 stage origin the title atlas uses so the
    /// chrome overlays the title art at retail-equivalent positions.
    ///
    /// Returns an empty vec when the save-menu atlas wasn't uploaded
    /// (e.g. running without a disc) or when the boot UI isn't in a
    /// SaveSelect / field-Save sub-state.
    pub(super) fn save_select_chrome_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_render::SpriteDraw> {
        if self.save_menu.is_none() {
            return Vec::new();
        }
        use legaia_engine_core::save_select::SaveSelectSession;
        // The save-select session (or field-menu Save sub-session) that
        // drives both pill chrome and any retail Load-mode overlays.
        let session: &SaveSelectSession = match &self.boot_ui {
            BootUiState::SaveSelect(s) => s,
            BootUiState::FieldMenu {
                sub: Some(active), ..
            } => {
                use legaia_engine_core::field_menu_dispatch::FieldMenuSubsession;
                if let FieldMenuSubsession::Save(s) = active {
                    s
                } else {
                    return Vec::new();
                }
            }
            _ => return Vec::new(),
        };
        self.save_select_overlay(session, surface_w, surface_h)
            .sprites
    }

    /// The save-select screen - both halves - through the shared
    /// composition: the engine's overlay sequence
    /// (`SaveScreenFlow::overlay_model`) borrowed into `engine-ui`'s view and
    /// composed by `save_select_overlay_draws`, the browser play page's calls
    /// too. The text half draws with or without the system-UI atlas; the
    /// sprite half needs it. Serves the boot Continue -> Load screen and the
    /// pause menu's Load / Save rows alike.
    pub(super) fn save_select_overlay(
        &self,
        s: &legaia_engine_core::save_select::SaveSelectSession,
        surface_w: u32,
        surface_h: u32,
    ) -> legaia_engine_render::SaveSelectOverlayDraws {
        let Some(m) = self.save_flow.overlay_model(s) else {
            return Default::default();
        };
        let rows: Vec<legaia_engine_render::SaveSelectRow<'_>> = s
            .slots()
            .iter()
            .map(|snap| legaia_engine_render::SaveSelectRow {
                label: &snap.label,
                present: snap.present,
                party_lv: snap.party_lv,
                play_time_seconds: snap.play_time_seconds,
                money: snap.money,
                location: &snap.location,
            })
            .collect();
        let cells: Vec<legaia_engine_render::SlotGridCell> = m
            .preview
            .iter()
            .flat_map(|p| p.cells.iter())
            .map(|c| legaia_engine_render::SlotGridCell {
                present: c.present,
                portrait_char_id: c.portrait_char_id,
            })
            .collect();
        let preview = m
            .preview
            .as_ref()
            .map(|p| legaia_engine_render::SaveSelectPreviewView {
                cells: &cells,
                cell: p.cell,
                info: p.info.map(|b| legaia_engine_render::SlotInfoView {
                    slot_no: b.slot.saturating_add(1),
                    location: &b.location,
                    play_time: &p.play_time,
                    leader_name: &b.leader_name,
                    leader_level: b.party_lv,
                    leader_hp: b.leader_hp,
                    leader_mp: b.leader_mp,
                    leader_char_id: b.leader_char_id,
                }),
                caption: p.caption,
                panel_y_offset: p.panel_y_offset,
            });
        let view = legaia_engine_render::SaveSelectOverlayView {
            title: m.title,
            rows: &rows,
            cursor: m.cursor,
            single_pill: m.single_pill,
            pills: &m.pills,
            pill_cursor: m.pill_cursor,
            slide_t: m.slide_t,
            info_t: m.info_t,
            now_checking: m.now_checking,
            banner: m.banner,
            preview,
            confirm: m.confirm,
        };
        let (stage_origin, stage_scale) = self.save_select_stage(surface_w, surface_h);
        legaia_engine_render::save_select_overlay_draws(
            &self.font,
            self.save_menu.as_ref().map(|a| &a.rects),
            &view,
            stage_origin,
            stage_scale,
        )
    }

    /// Sprite half of the field pause menu and its sub-screens: the 9-slice
    /// window frames, the carved title-tab plaques and the per-screen icon /
    /// cursor / gauge sprites.
    ///
    /// This is one half of [`Self::field_menu_sub_draws`]'s output - the
    /// composition emits texts and sprites together, and the native window
    /// splits them because its redraw loop batches the font atlas and the
    /// chrome atlas separately. The composition itself (which windows a
    /// screen frames, in what order, and where the modals land) is shared
    /// with the browser play page in `legaia_engine_ui::pause_menu`.
    ///
    /// Returns empty unless boot-UI is in a FieldMenu state and the atlas
    /// has been uploaded. The Save sub-session is excluded: it renders
    /// through [`Self::save_select_chrome_sprite_draws`], which also serves
    /// the boot Continue -> Load screen.
    pub(super) fn field_menu_chrome_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_render::SpriteDraw> {
        use legaia_engine_core::field_menu_dispatch::FieldMenuSubsession;
        if self.save_menu.is_none() {
            return Vec::new();
        }
        let BootUiState::FieldMenu { sub } = &self.boot_ui else {
            return Vec::new();
        };
        // The kind-0x0D entry pair closes every window before opening its
        // own, so nothing behind it is framed while it is up.
        if !self
            .context_locked_screen_draws(surface_w, surface_h)
            .is_empty()
        {
            return Vec::new();
        }
        match sub {
            Some(FieldMenuSubsession::Save(_)) => Vec::new(),
            Some(active) => {
                self.field_menu_sub_draws(active, surface_w, surface_h)
                    .sprites
            }
            None => self.field_menu_root_draws(surface_w, surface_h).sprites,
        }
    }

    /// Build the [`legaia_engine_render::SpriteDraw`] list for the
    /// title-screen quad. Composes the retail title screen by drawing
    /// per-band sub-rects of the PROT 0888 title TIM: orb + wordmark
    /// always, "PRESS START BUTTON" only during the PressStart phase,
    /// and the two copyright lines in every post-fade phase. The
    /// `<DEMO>` band and the small "NEW GAME CONTINUE" footer band are
    /// intentionally skipped - the former is a demo-build leftover
    /// retail never draws, the latter is replaced by larger
    /// font-rendered menu labels (see [`Self::boot_ui_draws`]).
    ///
    /// Each band is positioned at its source `y` within a centred,
    /// integer-scaled 256×256 stage. Returns an empty vec when
    /// boot-UI isn't `Title`, the atlas wasn't uploaded, or the title
    /// session has reached [`legaia_engine_core::title::TitlePhase::Done`].
    pub(super) fn title_screen_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_render::SpriteDraw> {
        // Everything below the composition is state, not geometry: which
        // bands draw and how bright ([`boot_title_band_state`]). The
        // composition itself is `legaia_engine_ui::title_band_sprites`,
        // shared with the browser play page so neither host can place a band
        // the other does not.
        let Some(state) = boot_title_band_state(&self.boot_ui) else {
            return Vec::new();
        };
        let Some(assets) = self.title_screen.as_ref() else {
            return Vec::new();
        };
        let (_atlas_x, _atlas_y, atlas_w, atlas_h) = assets.rect;
        if atlas_w == 0 || atlas_h == 0 {
            return Vec::new();
        }
        // Share the canonical PSX framebuffer (320x240) stage with
        // every other boot-UI element so the title art aligns with
        // the save-select panel, slot pills, and cursor - all of
        // which use retail-pinned framebuffer coords.
        let (stage_origin, scale) = self.save_select_stage(surface_w, surface_h);
        legaia_engine_render::title_band_sprites(state, stage_origin, scale)
    }

    /// **Deprecated path** kept as a no-disc fallback. The retail title
    /// menu now renders via `title_screen_sprite_draws` sampling the
    /// dedicated NEW GAME / CONTINUE sub-rects from the title TIM
    /// (PROT 0888 @ y=227..237). When the title atlas is present this
    /// method returns an empty vec so the title-TIM path is the
    /// single source of menu glyphs.
    ///
    /// Returns an empty vec when:
    /// - boot UI isn't [`BootUiState::Title`], or
    /// - the title session has already reached
    ///   [`legaia_engine_core::title::TitlePhase::Done`], or
    /// - the title-screen atlas IS uploaded (retail-faithful path
    ///   covers the menu rows itself), or
    /// - the menu-glyph atlas wasn't uploaded, or
    /// - the title phase isn't `MainMenu`.
    pub(super) fn title_menu_glyph_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_render::SpriteDraw> {
        let BootUiState::Title(session) = &self.boot_ui else {
            return Vec::new();
        };
        if self.menu_glyphs.is_none() {
            return Vec::new();
        }
        // When the title-screen atlas is loaded, the retail-faithful
        // path inside `title_screen_sprite_draws` already emits the
        // NEW GAME / CONTINUE rows from the title TIM itself - skip
        // the debug-atlas fallback to avoid double-rendering.
        if self.title_screen.is_some() {
            return Vec::new();
        }
        use legaia_engine_core::title::TitlePhase;
        // Same one selector as the font path and as the browser page: the
        // rows are drawn for whatever sub-mode word `title_draw_list` calls a
        // menu, and the session's phase only supplies the cursor row.
        let (menu_open, cursor) = match session.phase() {
            TitlePhase::MainMenu { cursor } => (true, cursor),
            _ => (false, 0),
        };
        let phase_id = legaia_engine_render::title_text_phase(session.retail_submode(), menu_open);
        if phase_id != 2 {
            return Vec::new();
        }
        // Anchor inside the same centred + integer-scaled 256×256
        // title stage that `title_screen_sprite_draws` uses. The menu
        // rows sit between the wordmark band (ends at src y=140) and
        // the copyright bands (start at src y=195) - the menu-glyph
        // cell is 14 px tall at 1× and we render at 2× the title-art
        // scale for retail-faithful sizing (~28 px atlas-pixels per
        // row, two rows + gutter = ~60 px in source).
        let atlas_w: u32 = 256;
        let atlas_h: u32 = 256;
        let title_scale = (surface_w / atlas_w.max(1))
            .min(surface_h / atlas_h.max(1))
            .clamp(1, 4);
        let title_scale_i32 = title_scale as i32;
        let stage_x0 = (surface_w as i32 - (atlas_w as i32) * title_scale_i32) / 2;
        let stage_y0 = (surface_h as i32 - (atlas_h as i32) * title_scale_i32) / 2;
        // Render menu glyphs at 2× the title-art scale so the letters
        // match the retail proportion (~28 px tall in framebuffer
        // pixels at 1×). "NEW GAME" is 8 cells × 8 px × 2 = 128 px at
        // 1× glyph_scale, then × title_scale for the on-screen size.
        let glyph_scale = title_scale;
        let menu_w_src = 8 * 8; // 8 chars × 8 px (1× glyph multiplier)
        // Centre horizontally inside the 256-wide title stage.
        let pen_src_x = (atlas_w as i32 - menu_w_src) / 2;
        let pen_src_y = 152;
        let pen = (
            stage_x0 + pen_src_x * title_scale_i32,
            stage_y0 + pen_src_y * title_scale_i32,
        );
        legaia_engine_render::title_menu_draws_for(
            phase_id,
            cursor,
            session.continue_enabled,
            pen,
            glyph_scale,
        )
    }
}

/// Which title-TIM bands the boot UI draws this frame, and how bright - the
/// state half of [`PlayWindowApp::title_screen_sprite_draws`].
///
/// Active during both the Title phases and the SaveSelect boot sub-state. The
/// SaveSelect arm is the retail backdrop, [`TitleBandState::backdrop`]: the
/// menu overlay's `FUN_801DD35C` calls the title-strip drawer `FUN_801E0418`
/// at `0x801E0260` and then the dimmed art `FUN_801E02A4` with one brightness
/// byte, only while `_DAT_8007BB00` (the came-from-the-title word) is set -
/// the `lw v0,-0x4500(v0)` / `beq v0,zero,0x801E0270` pair at `0x801E01D0`.
/// Retail pivots to pure black once a slot is confirmed (NowChecking /
/// SlotPreview): the dialog + portrait grid + info panel are composed against
/// black, never the title art. The browser play page makes the same two cuts
/// (`boot_title_backdrop_visible`).
///
/// [`TitleBandState::backdrop`]: legaia_engine_render::TitleBandState::backdrop
fn boot_title_band_state(boot_ui: &BootUiState) -> Option<legaia_engine_render::TitleBandState> {
    use legaia_engine_core::title::TitlePhase;
    match boot_ui {
        BootUiState::Title(session) => {
            let alpha = match session.phase() {
                TitlePhase::Done(_) => return None,
                TitlePhase::FadeIn { frames_remaining } => {
                    let total = session.fade_in_frames.max(1) as f32;
                    1.0 - (frames_remaining as f32 / total).clamp(0.0, 1.0)
                }
                _ => 1.0,
            };
            let mut st = legaia_engine_render::TitleBandState::card(alpha);
            st.press_start = matches!(session.phase(), TitlePhase::PressStart { .. });
            // Main-menu rows: selected row bright, unselected dim.
            if let TitlePhase::MainMenu { cursor } = session.phase() {
                st.menu = Some((cursor, true));
            }
            Some(st)
        }
        BootUiState::SaveSelect(s) => {
            if !s.phase().shows_title_backdrop() {
                return None;
            }
            Some(legaia_engine_render::TitleBandState::backdrop())
        }
        _ => None,
    }
}

#[cfg(test)]
mod title_backdrop_tests {
    use super::*;
    use legaia_engine_core::save_select::{SaveSelectMode, SaveSelectSession, SlotSnapshot};

    /// The native half of the save-select backdrop parity: the boot
    /// save-select resolves to the retail backdrop state, and that state
    /// composes as retail's title-strip stack (`FUN_801E0418`,
    /// `title_strip_sprites`) - the same five strips at the same dim the
    /// browser play page emits from `boot_title_backdrop_draws_json`
    /// (`web-viewer/tests/title_backdrop_parity.rs` pins that half off the
    /// disc).
    #[test]
    fn the_boot_save_select_draws_retails_title_strips() {
        let session = SaveSelectSession::new(SaveSelectMode::Load, vec![SlotSnapshot::empty(1)]);
        let state = boot_title_band_state(&BootUiState::SaveSelect(session))
            .expect("the save-select keeps the title behind it");
        assert_eq!(state, legaia_engine_render::TitleBandState::backdrop());
        let origin = (7, 11);
        let drawn = legaia_engine_render::title_band_sprites(state, origin, 3);
        // Brightness is TITLE_BACKDROP_LUM of the neutral 0x80, CONTINUE lit.
        let b = (legaia_engine_render::TITLE_BACKDROP_LUM * 128.0).round() as u8;
        let strips = legaia_engine_render::title_strip_sprites(true, b, 1.0, origin, 3);
        assert_eq!(
            drawn.len(),
            5,
            "wordmark, NEW GAME, CONTINUE, TM, copyright"
        );
        assert_eq!(
            format!("{drawn:?}"),
            format!("{strips:?}"),
            "the backdrop is the title-strip drawer's output, sprite for sprite"
        );
    }

    /// No boot UI, no backdrop: the strips are the save-select's, not the
    /// field's.
    #[test]
    fn no_boot_ui_draws_no_title_strips() {
        assert!(boot_title_band_state(&BootUiState::Inactive).is_none());
    }
}

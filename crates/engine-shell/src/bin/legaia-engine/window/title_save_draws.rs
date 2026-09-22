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
        let Some(assets) = self.save_menu.as_ref() else {
            return Vec::new();
        };
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
        let slot_count = session.slots().len().min(2);
        let cursor_row = (session.current_slot() as usize).min(1);
        // Retail draws every visible slot pill during Browsing and the
        // Confirm prompts, but hides the non-selected pills once a
        // slot has been confirmed for load (NowChecking + SlotPreview
        // both show only the picked pill). Build the pill slice
        // accordingly so the sprite chrome matches retail. AND retail
        // relocates that single visible pill up under the Load panel
        // (SAVE_SELECT_SLOT1_POS_LOAD_ACTIVE) during Load-active.
        // The relocation is animated - mode 2 of FUN_801E1C1C slides
        // the slot composite linearly from screen `(136, 96)` (=
        // param_3=0xa0 with `sVar6 -= 0x18` x-shift, param_4=0x60) to
        // `(24, 40)` over 16 frames, driven by `DAT_801ef194`. We
        // interpolate against `session.slide_anim_t()` so the engine
        // matches retail's slide-in.
        // Which pills, which cursor, which overlays - the shared decision
        // (`save_select::phase_layout`) both hosts read, so a phase cannot
        // mean two screens.
        let layout = legaia_engine_core::save_select::phase_layout(session.phase());
        let (pills, pill_anchor): (Vec<u8>, (i32, i32)) = if layout.single_pill {
            // Slide start = the pill's Browsing position (retail mode-2
            // start `(160, 96)` minus the `-0x18` x-shift = the Browsing
            // pill quad, i.e. the pill slides away from where it already
            // sat).
            let pos = session.interpolate(
                legaia_engine_render::SAVE_SELECT_SLOT1_POS,
                legaia_engine_render::SAVE_SELECT_SLOT1_POS_LOAD_ACTIVE,
            );
            (vec![session.current_slot()], pos)
        } else {
            (
                (0..slot_count as u8).collect(),
                legaia_engine_render::SAVE_SELECT_SLOT1_POS,
            )
        };
        let (stage_origin, stage_scale) = self.save_select_stage(surface_w, surface_h);
        let mut draws = legaia_engine_render::save_select_chrome_draws_for(
            &assets.rects,
            &pills,
            pill_anchor,
            stage_origin,
            stage_scale,
        );
        // Pointing-finger cursor sprite - retail's small white hand
        // pointing at the selected slot pill, byte-pinned to CLUT row
        // 7 of the system-UI TIM. Emit last so it draws on top of
        // the pills. Suppressed once a card is committed: the dialog
        // covers the pill row and the grid emits its own cursor on the
        // focused cell.
        if slot_count > 0 && layout.pill_cursor {
            draws.push(legaia_engine_render::save_select_cursor_draw_for(
                &assets.rects,
                cursor_row,
                stage_origin,
                stage_scale,
            ));
        }
        // Phase-specific overlays: SlotPreview shows the 5×3 grid + a
        // bottom info panel; NowChecking shows a centered dialog box
        // with the "Now checking. Do not remove MEMORY CARD" message.
        match session.phase() {
            // Every preview phase, the two confirms included - retail raises
            // the overwrite / delete prompt FROM the preview, so the block
            // grid and the info panel stay under the messagebox.
            _ if layout.preview => {
                // The grid is the picked PORT's fifteen blocks, focused by
                // the shared flow's cursor - NOT the pill row, which in a
                // two-stage rack lists the card ports instead.
                let (blocks, cell) = self.save_flow.preview(session);
                let cells: Vec<legaia_engine_render::SlotGridCell> = (0..15)
                    .map(|i| {
                        blocks
                            .get(i)
                            .map(|s| legaia_engine_render::SlotGridCell {
                                present: s.present,
                                portrait_char_id: if s.present {
                                    Some(slot_leader_char_id(s))
                                } else {
                                    None
                                },
                            })
                            .unwrap_or_default()
                    })
                    .collect();
                draws.extend(legaia_engine_render::slot_preview_grid_draws_for(
                    &assets.rects,
                    &cells,
                    cell,
                    stage_origin,
                    stage_scale,
                ));
                let info = build_slot_info_view(blocks, cell);
                let view = info.as_ref().map(|i| i.as_view());
                let panel_y_offset = info_panel_slide_offset(session);
                draws.extend(legaia_engine_render::slot_info_panel_draws_for(
                    &assets.rects,
                    view.as_ref(),
                    panel_y_offset,
                    stage_origin,
                    stage_scale,
                ));
            }
            _ if layout.now_checking => {
                // Slide the panel left-from-right alongside the text,
                // matching retail mode-0's `pos = (416, 112) -> (160,
                // 112)` interpolation.
                let pos_x = legaia_engine_core::save_select::interpolate_anim(
                    (legaia_engine_render::NOW_CHECKING_SLIDE_START_X, 0),
                    (legaia_engine_render::NOW_CHECKING_SLIDE_TARGET_X, 0),
                    session.slide_anim_t(),
                )
                .0;
                let slide_offset = (pos_x - legaia_engine_render::NOW_CHECKING_SLIDE_TARGET_X, 0);
                draws.extend(legaia_engine_render::now_checking_panel_draws_for(
                    &assets.rects,
                    stage_origin,
                    stage_scale,
                    slide_offset,
                ));
            }
            _ => {}
        }
        // Retail raises the confirm as its own centred messagebox pair
        // (prompt bar + stacked Yes/No box, mode 3 of FUN_801E1C1C),
        // sliding up from below the stage ON TOP of the preview. Text half
        // lives in `save_select_phase_text_draws`.
        if layout.confirm {
            draws.extend(legaia_engine_render::confirm_dialog_panel_draws_for(
                &assets.rects,
                confirm_dialog_slide_y(session),
                stage_origin,
                stage_scale,
            ));
        }
        draws
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
        // Active during both the Title phases and the SaveSelect boot
        // sub-state. SaveSelect dims the bands to ~45 % brightness so
        // the panel + slot pills layered on top read clearly. Retail
        // pivots to pure black once a slot is confirmed (NowChecking /
        // SlotPreview): the dialog + portrait grid + info panel are
        // composed against black, never the title art.
        let title_session: Option<&legaia_engine_core::title::TitleSession> = match &self.boot_ui {
            BootUiState::Title(s) => Some(s),
            BootUiState::SaveSelect(s) => {
                use legaia_engine_core::save_select::SelectPhase;
                if matches!(
                    s.phase(),
                    SelectPhase::NowChecking { .. } | SelectPhase::SlotPreview { .. }
                ) {
                    return Vec::new();
                }
                None
            }
            _ => return Vec::new(),
        };
        // Everything below the composition is state, not geometry: which
        // bands draw and how bright. The composition itself is
        // `legaia_engine_ui::title_band_sprites`, shared with the browser
        // play page so neither host can place a band the other does not.
        let state = match title_session {
            Some(session) => {
                if matches!(
                    session.phase(),
                    legaia_engine_core::title::TitlePhase::Done(_)
                ) {
                    return Vec::new();
                }
                use legaia_engine_core::title::TitlePhase;
                let alpha = match session.phase() {
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
                st
            }
            // SaveSelect: the retail backdrop - dimmed art with both rows
            // drawn cursor-less behind the slot pills.
            None => legaia_engine_render::TitleBandState::backdrop(),
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

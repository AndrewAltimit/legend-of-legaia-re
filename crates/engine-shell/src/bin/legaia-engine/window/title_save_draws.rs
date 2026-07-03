//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;

impl PlayWindowApp {
    /// Build the per-strip [`legaia_engine_render::SpriteDraw`] list for
    /// the active publisher logo.
    ///
    /// PROKION and SCEA are stored as vertically-packed sprite atlases
    /// (see [`legaia_engine_core::publisher_logos::STRIPS_PER_LOGO`]);
    /// retail unfolds them by drawing the `N` strips side-by-side. We
    /// compute one [`SpriteDraw`] per strip, all sharing the session's
    /// current alpha, then integer-scale + centre the unfolded layout.
    /// Returns an empty vec when boot-UI isn't `PublisherLogos` or the
    /// atlas wasn't uploaded.
    pub(super) fn publisher_logo_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_render::SpriteDraw> {
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
        let (sx, sy, sw, sh) = assets.rects[idx];
        if sw == 0 || sh == 0 {
            return Vec::new();
        }
        let (cols, rows) = legaia_engine_core::publisher_logos::STRIP_GRID[idx];
        let cols = cols.max(1);
        let rows = rows.max(1);
        let strips_total = cols * rows;
        let strip_h_src = sh / strips_total;
        if strip_h_src == 0 {
            return Vec::new();
        }
        let unfolded_w = sw * cols;
        let unfolded_h = strip_h_src * rows;
        // Integer-multiple up-scale that fits inside the surface, capped
        // at 4× to keep logos crisp at typical 960×720. `max(1)` falls
        // back to native size (and accepts clipping) for layouts wider
        // than the surface.
        let scale_w = surface_w / unfolded_w.max(1);
        let scale_h = surface_h / unfolded_h.max(1);
        let scale = scale_w.min(scale_h).clamp(1, 4);
        let strip_w_dst = sw * scale;
        let strip_h_dst = strip_h_src * scale;
        let dst_w_total = unfolded_w * scale;
        let dst_h_total = unfolded_h * scale;
        let dst_x0 = (surface_w as i32 - dst_w_total as i32) / 2;
        let dst_y0 = (surface_h as i32 - dst_h_total as i32) / 2;
        let alpha = session.alpha().clamp(0.0, 1.0);
        let color = [1.0, 1.0, 1.0, alpha];
        // Source strips are stored column-major: source strip index
        // `s = c * rows + r` lands at output (col c, row r).
        let mut out = Vec::with_capacity(strips_total as usize);
        for r in 0..rows {
            for c in 0..cols {
                let s = c * rows + r;
                let src_y = sy + s * strip_h_src;
                let dst_x = dst_x0 + (c * strip_w_dst) as i32;
                let dst_y = dst_y0 + (r * strip_h_dst) as i32;
                out.push(legaia_engine_render::SpriteDraw {
                    dst: (dst_x, dst_y, strip_w_dst, strip_h_dst),
                    src: (sx, src_y, sw, strip_h_src),
                    color,
                });
            }
        }
        out
    }

    /// Canonical PSX-framebuffer (320×240) stage origin + scale, shared
    /// by every boot-UI element (title art, save-select chrome, slot
    /// pills, cursor, menu glyphs). Every retail-pinned position is
    /// expressed in 320×240 framebuffer pixels, so this is the single
    /// stage transform that maps them to screen coords. Using the same
    /// stage for the title art AND the save-select panel ensures
    /// relative positions remain correct at any window resolution.
    pub(super) fn save_select_stage(&self, surface_w: u32, surface_h: u32) -> ((i32, i32), u32) {
        let stage_w = legaia_engine_render::BOOT_UI_STAGE_W;
        let stage_h = legaia_engine_render::BOOT_UI_STAGE_H;
        let scale = (surface_w / stage_w).min(surface_h / stage_h).clamp(1, 4);
        let sw = stage_w * scale;
        let sh = stage_h * scale;
        let x0 = (surface_w as i32 - sw as i32) / 2;
        let y0 = (surface_h as i32 - sh as i32) / 2;
        ((x0, y0), scale)
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
        use legaia_engine_core::save_select::{SaveSelectSession, SelectPhase};
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
        let (pills, pill_anchor): (Vec<u8>, (i32, i32)) = match session.phase() {
            SelectPhase::NowChecking { slot, .. } | SelectPhase::SlotPreview { slot } => {
                // Slide start: retail mode-2 start `(160, 96)` minus
                // the `-0x18` x-shift baked into the inline emit
                // before the GPU command -> top-left `(136, 96)`.
                const SLIDE_START_TOPLEFT: (i32, i32) = (136, 96);
                let pos = session.interpolate(
                    SLIDE_START_TOPLEFT,
                    legaia_engine_render::SAVE_SELECT_SLOT1_POS_LOAD_ACTIVE,
                );
                (vec![slot], pos)
            }
            _ => (
                (0..slot_count as u8).collect(),
                legaia_engine_render::SAVE_SELECT_SLOT1_POS,
            ),
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
        // the pills. Suppress during NowChecking (dialog covers the
        // pill row) and SlotPreview (the grid emits its own cursor
        // on the focused cell).
        let emit_pill_cursor = !matches!(
            session.phase(),
            SelectPhase::NowChecking { .. } | SelectPhase::SlotPreview { .. }
        );
        if slot_count > 0 && emit_pill_cursor {
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
            SelectPhase::SlotPreview { slot } => {
                // Build per-cell views from the session's slot
                // snapshots. Each cell maps to one memory-card block;
                // up to 15 cells (5×3 grid).
                let cells: Vec<legaia_engine_render::SlotGridCell> = (0..15)
                    .map(|i| {
                        session
                            .slots()
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
                    slot,
                    stage_origin,
                    stage_scale,
                ));
                let info = build_slot_info_view(session.slots(), slot);
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
            SelectPhase::NowChecking { .. } => {
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
        draws
    }

    /// Build the 9-slice window-frame [`legaia_engine_render::SpriteDraw`]s
    /// for the field pause menu and its sub-screens, sampling the same
    /// resident system-UI atlas the save chrome uses.
    ///
    /// Retail draws each menu's bordered windows as a separate pass before
    /// the content (`FUN_801D33D8` renders content only). Each screen's
    /// window set + rects come from the menu overlay's window-descriptor
    /// table (`legaia_asset::menu_windows`, RAM-matched against the
    /// catalogued menu-open captures); frames are emitted in retail draw
    /// order, so a later window's opaque interior occludes earlier ones
    /// (the equip main window covers the item-list window's lower span).
    /// Screens whose retail window sets are not capture-pinned (Items /
    /// Spells / Arts / the Equip picker) frame with
    /// [`MENU_SUBWINDOW_CONTENT`]. Returns empty unless boot-UI is in a
    /// FieldMenu state (and not the Save sub-session, which owns its own
    /// load-screen chrome) and the atlas has been uploaded.
    pub(super) fn field_menu_chrome_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_render::SpriteDraw> {
        use legaia_asset::menu_windows::{
            OPTIONS_SCREEN_WINDOWS, STATUS_SCREEN_WINDOWS, TOP_LEVEL_WINDOWS,
        };
        use legaia_engine_core::field_menu_dispatch::FieldMenuSubsession;
        let Some(assets) = self.save_menu.as_ref() else {
            return Vec::new();
        };
        let BootUiState::FieldMenu { sub } = &self.boot_ui else {
            return Vec::new();
        };
        // The Save sub-session renders through the save-select chrome
        // (`save_select_chrome_sprite_draws`); don't double-frame it.
        // Items / Spells / Arts and the Equip picker keep the generic
        // near-fullscreen frame: their retail window sets exist in the
        // descriptor table (equip = tab 2 + ids 21/22/23) but the engine
        // content layouts don't fill those windows yet.
        let ids: &[usize] = match sub {
            None => &TOP_LEVEL_WINDOWS,
            Some(FieldMenuSubsession::Save(_)) => return Vec::new(),
            Some(FieldMenuSubsession::Status(_)) => &STATUS_SCREEN_WINDOWS,
            Some(FieldMenuSubsession::Config(_)) => &OPTIONS_SCREEN_WINDOWS,
            Some(_) => &[],
        };
        let (stage_origin, stage_scale) = self.save_select_stage(surface_w, surface_h);
        let mut out = Vec::new();
        if ids.is_empty() {
            // Unpinned screen: one near-fullscreen frame.
            let (x, y, w, h) = MENU_SUBWINDOW_CONTENT;
            out.extend(legaia_engine_render::menu_window_chrome_draws_for(
                &assets.rects,
                (x - 6, y - 2, w + 12, h + 12),
                stage_origin,
                stage_scale,
            ));
        }
        for &id in ids {
            out.extend(legaia_engine_render::menu_window_chrome_draws_for(
                &assets.rects,
                self.menu_window_frame_rect(id),
                stage_origin,
                stage_scale,
            ));
        }
        out
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
        let (alpha, dim) = if let Some(session) = title_session {
            if matches!(
                session.phase(),
                legaia_engine_core::title::TitlePhase::Done(_)
            ) {
                return Vec::new();
            }
            let alpha = match session.phase() {
                legaia_engine_core::title::TitlePhase::FadeIn { frames_remaining } => {
                    let total = session.fade_in_frames.max(1) as f32;
                    1.0 - (frames_remaining as f32 / total).clamp(0.0, 1.0)
                }
                _ => 1.0,
            };
            (alpha, false)
        } else {
            (1.0, true)
        };
        let Some(assets) = self.title_screen.as_ref() else {
            return Vec::new();
        };
        let (_atlas_x, _atlas_y, atlas_w, atlas_h) = assets.rect;
        if atlas_w == 0 || atlas_h == 0 {
            return Vec::new();
        }
        // Share the canonical PSX framebuffer (320×240) stage with
        // every other boot-UI element so the title art aligns with
        // the save-select panel, slot pills, and cursor - all of
        // which use retail-pinned framebuffer coords. The title TIM's
        // bands are sampled at their natural src (sx, sy) but drawn
        // at dst (TITLE_ART_POS + sx, TITLE_ART_POS + sy), i.e.
        // offset by retail's title-quad top-left placement.
        let ((stage_x0, stage_y0), scale) = self.save_select_stage(surface_w, surface_h);
        let lum = if dim { 0.45 } else { 1.0 };
        let color = [lum, lum, lum, alpha];
        let emit_press_start = matches!(
            &self.boot_ui,
            BootUiState::Title(s)
                if matches!(s.phase(), legaia_engine_core::title::TitlePhase::PressStart { .. })
        );
        use legaia_asset::title_pak;
        // Each entry: (src_rect, dst_x_src, dst_y_src, tint). Most
        // bands draw at their own (src_x, src_y); the menu rows are
        // sampled from a packed single-row band and re-positioned so
        // "NEW GAME" sits at src_y=143 and "CONTINUE" at src_y=159
        // (matching the retail stacked layout, which puts these
        // ~14 px apart between the wordmark and the copyright lines).
        let scale_i32 = scale as i32;
        let mut out: Vec<legaia_engine_render::SpriteDraw> = Vec::new();
        // `dst_src_x/y` are coords inside the title TIM's source rect
        // (0..256, 0..256). We offset by TITLE_ART_POS so the result
        // lands at retail's framebuffer position.
        let title_pos_x = legaia_engine_render::TITLE_ART_POS.0;
        let title_pos_y = legaia_engine_render::TITLE_ART_POS.1;
        let push_band = |out: &mut Vec<legaia_engine_render::SpriteDraw>,
                         src: (u32, u32, u32, u32),
                         dst_src_x: i32,
                         dst_src_y: i32,
                         tint: [f32; 4]| {
            let (sx, sy, sw, sh) = src;
            out.push(legaia_engine_render::SpriteDraw {
                dst: (
                    stage_x0 + (title_pos_x + dst_src_x) * scale_i32,
                    stage_y0 + (title_pos_y + dst_src_y) * scale_i32,
                    sw * scale,
                    sh * scale,
                ),
                src: (sx, sy, sw, sh),
                color: tint,
            });
        };

        // Wordmark always.
        let wm = title_pak::TITLE_BAND_WORDMARK;
        push_band(&mut out, wm, wm.0 as i32, wm.1 as i32, color);

        // PressStart prompt only during that phase.
        if emit_press_start {
            let ps = title_pak::TITLE_BAND_PRESS_START;
            push_band(&mut out, ps, ps.0 as i32, ps.1 as i32, color);
        }

        // Main-menu rows (NEW GAME / CONTINUE) - drawn during MainMenu
        // (selected row bright, unselected dim) and also during
        // SaveSelect (both dim - they sit in the background behind the
        // slot pills and don't reflect a live cursor).
        let menu_state: Option<(u8, bool)> = match &self.boot_ui {
            BootUiState::Title(s) => match s.phase() {
                legaia_engine_core::title::TitlePhase::MainMenu { cursor } => Some((cursor, true)),
                _ => None,
            },
            BootUiState::SaveSelect(_) => Some((1, false)),
            _ => None,
        };
        if let Some((cursor, has_focus)) = menu_state {
            let row_white = color;
            let row_dim = [color[0] * 0.5, color[1] * 0.5, color[2] * 0.5, color[3]];
            let ng = title_pak::TITLE_BAND_MENU_NEW_GAME;
            let co = title_pak::TITLE_BAND_MENU_CONTINUE;
            // Center inside the title-art width so the rows sit on
            // the screen's horizontal center (fb_x=160) after the
            // TITLE_ART_POS.x=33 offset is applied by push_band.
            let title_art_w = legaia_engine_render::TITLE_ART_SIZE.0 as u32;
            let ng_x = ((title_art_w - ng.2) / 2) as i32;
            let co_x = ((title_art_w - co.2) / 2) as i32;
            // Sit the menu between wordmark (ends y~141) and copyrights (start y~195).
            let ng_y: i32 = 154;
            let co_y: i32 = ng_y + ng.3 as i32 + 4;
            let ng_tint = if has_focus && cursor == 0 {
                row_white
            } else {
                row_dim
            };
            let co_tint = if has_focus && cursor == 1 {
                row_white
            } else {
                row_dim
            };
            push_band(&mut out, ng, ng_x, ng_y, ng_tint);
            push_band(&mut out, co, co_x, co_y, co_tint);
        }

        // Copyright lines always (post-fade).
        let tm = title_pak::TITLE_BAND_TM_COPYRIGHT;
        push_band(&mut out, tm, tm.0 as i32, tm.1 as i32, color);
        let cc = title_pak::TITLE_BAND_C_COPYRIGHT;
        push_band(&mut out, cc, cc.0 as i32, cc.1 as i32, color);
        out
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
        let (phase_id, cursor) = match session.phase() {
            TitlePhase::MainMenu { cursor } => (2u8, cursor),
            _ => return Vec::new(),
        };
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

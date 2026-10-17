//! The pause menu's disc-sourced assets: the chrome atlas and its band
//! rects, the window table, the font, and the atlas buffers the page uploads.

use super::*;

impl PlayMenuAssets {
    /// Shared dialog-font atlas (reused by the boot title screen's text
    /// fallback).
    pub(crate) fn font_ref(&self) -> &legaia_font::Font {
        &self.font
    }

    /// The chrome atlas' band rects, when the gold chrome resolved from the
    /// disc (shared with the dialog reading box, [`crate::play_dialog`]).
    pub(crate) fn chrome_rects(&self) -> Option<&SaveMenuAtlasRects> {
        self.chrome.as_ref().map(|(_, r)| r)
    }

    /// A fully-opaque texel inside the baked chrome atlas
    /// ([`legaia_engine_ui::atlas_opaque_texel`]) - the surface the field
    /// party HUD's translucent plate draws on, so the plate can ride the
    /// sprite array beneath the readout it backs rather than the text array
    /// on top of it.
    pub(crate) fn chrome_solid_texel(&self) -> Option<(u32, u32, u32, u32)> {
        self.chrome
            .as_ref()
            .and_then(|(a, _)| legaia_engine_ui::atlas_opaque_texel(&a.rgba, a.width, a.height))
    }

    /// The battle HUD's nine status-element + eight element badge cells,
    /// projected out of the baked atlas.
    ///
    /// Which cells actually carry art is a property of **this bake** (three
    /// of the status badges need the row-511 CLUT extension the slice above
    /// is rooted at), so it travels with the atlas rather than being
    /// re-derived from the layout - the same rule the native window follows.
    pub(crate) fn battle_badges(
        &self,
    ) -> Option<legaia_engine_ui::battle_hud_chrome::BattleBadgeRects> {
        self.chrome
            .as_ref()
            .map(|(a, _)| legaia_engine_screens::field_screens::battle_badge_rects(a))
    }

    /// The disc-parsed menu-overlay window-descriptor table, when it parsed.
    ///
    /// [`Self::window_rect`] falls back to pinned rects for the screens whose
    /// layout is capture-verified, but the retail **painter** family
    /// ([`crate::play_shop`]) dispatches on each descriptor's `renderer_va`,
    /// which no fallback can invent - those windows draw only when the real
    /// table is present, exactly as they do in the native window.
    pub(crate) fn window_table(
        &self,
    ) -> Option<&legaia_game_tables::menu_windows::MenuWindowTable> {
        self.windows.as_ref()
    }

    /// Descriptor-rect resolver over the parsed table, with the pinned
    /// fallback + the frame-box maths shared with the native window
    /// ([`legaia_engine_ui::pause_menu::MenuRects`]).
    fn rects(&self) -> MenuRects<'_> {
        MenuRects::new(self.windows.as_ref())
    }

    /// The chrome atlas' band rects, when the gold chrome resolved.
    fn chrome_rects_opt(&self) -> Option<&SaveMenuAtlasRects> {
        self.chrome.as_ref().map(|(_, r)| r)
    }

    /// The shared pause-menu composition context for this frame.
    pub(super) fn menu_ctx(&self, origin: (i32, i32), scale: u32) -> PauseMenuCtx<'_> {
        PauseMenuCtx {
            font: &self.font,
            rects: self.rects(),
            chrome: self.chrome_rects_opt(),
            origin,
            scale,
        }
    }
}

impl LegaiaRuntime {
    /// Build the menu assets on demand (font is always available; chrome +
    /// window table need the loaded PROT). Returns `false` when there is no
    /// disc loaded yet. Crate-visible so the boot title screen can share the
    /// font atlas.
    pub(crate) fn ensure_menu_assets(&mut self) -> bool {
        if self.menu_assets.is_some() {
            return true;
        }
        // The real retail proportional dialog font decoded from the disc at
        // `load_disc` (byte-identical to what the native pause menu draws); the
        // built-in placeholder only stands in on a PROT.DAT-only load where the
        // font TIM / SCUS width table weren't available.
        let mut font = self
            .menu_font
            .clone()
            .unwrap_or_else(legaia_font::Font::placeholder);
        // Chrome atlas + window table off the loaded PROT, best-effort: a
        // PROT.DAT-only load may lack the overlay slices, in which case the
        // menu still renders its glyphs (no gold frame).
        let (chrome, windows) = match self.scene_host.host() {
            Some(host) => {
                let idx = &host.index;
                let panel = {
                    // Rooted ONE TIM EARLIER than the system-UI sheet, at the
                    // row-511 CLUT extension - the same base the native window
                    // uses. That TIM carries no pixels, only sub-palettes
                    // 16..18, and three of the nine status-element badges
                    // (Stone / Rage / Faint) decode with nothing else. A slice
                    // rooted at the sheet puts the extension *behind* its start
                    // where `build_atlas` cannot reach it, and those three
                    // cells silently bake blank.
                    let base =
                        legaia_engine_menus::save_menu_atlas::SYSTEM_UI_CLUT_EXT_TIM_OFFSET as u64;
                    let end = (legaia_ui_formats::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_OFFSET
                        + legaia_ui_formats::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_SIZE)
                        as u64;
                    idx.prot_dat_raw_bytes(base, (end - base) as usize)
                };
                let pill = idx
                    .entry_bytes_extended(legaia_ui_formats::title_pak::PROT_INDEX_OVERLAY as u32);
                // The battle HUD's 8x12 numeral cells, off the menu-glyph
                // TIM's sub-palette 13. Baked into the same atlas so the
                // HUD's sprite list stays one texture on this host too.
                let glyph_tim = idx
                    .prot_dat_raw_bytes(
                        legaia_ui_formats::menu_glyph_atlas::PROT_DAT_OFFSET,
                        legaia_ui_formats::menu_glyph_atlas::TIM_SIZE,
                    )
                    .ok();
                // The menus' bold fixed-width numerals and the list pager
                // pieces ride the font atlas as sprite cells - the native
                // window attaches the same cells
                // (`save_menu_atlas::menu_font_cells`).
                if let Ok(system_ui) = panel.as_ref() {
                    font = font.with_sprite_cells(
                        &legaia_engine_menus::save_menu_atlas::menu_font_cells(
                            system_ui,
                            glyph_tim.as_deref(),
                        ),
                    );
                }
                let chrome = match (panel, pill) {
                    (Ok(panel_bytes), Ok(pill_bytes)) => {
                        match build_atlas(&panel_bytes, &pill_bytes, glyph_tim.as_deref()) {
                            Ok(mut a) => {
                                // The red cross-out X off the battle effect
                                // page, baked into the same atlas the chips
                                // draw from (the native window bakes it too).
                                if let Ok(flame) = idx.entry_bytes_extended(
                                    legaia_engine_menus::save_menu_atlas::FLAME_ATLAS_PROT_ENTRY,
                                ) {
                                    legaia_engine_menus::save_menu_atlas::add_cross_out_mark(
                                        &mut a, &flame,
                                    );
                                }
                                let rects =
                                    legaia_engine_screens::field_screens::save_menu_rects(&a);
                                Some((a, rects))
                            }
                            Err(e) => {
                                crate::console_log(&format!("play menu: chrome atlas failed: {e}"));
                                None
                            }
                        }
                    }
                    _ => None,
                };
                let windows = idx
                    .entry_bytes_extended(
                        legaia_game_tables::menu_windows::MENU_OVERLAY_PROT_INDEX as u32,
                    )
                    .ok()
                    .and_then(|b| legaia_game_tables::menu_windows::parse(&b).ok());
                (chrome, windows)
            }
            None => (None, None),
        };
        self.menu_assets = Some(PlayMenuAssets {
            font,
            chrome,
            windows,
        });
        true
    }

    pub(super) fn menu_world(&self) -> Option<&legaia_engine_core::world::World> {
        self.scene_host.host().map(|h| &h.world)
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// `true` once the gold chrome atlas resolved from the disc; `false` means
    /// the menu renders glyphs only (PROT.DAT-only load).
    pub fn play_menu_has_chrome(&self) -> bool {
        self.menu_assets
            .as_ref()
            .map(|a| a.chrome.is_some())
            .unwrap_or(false)
    }

    /// The whitewashed font atlas (RGBA8) the text draws sample. Stable across
    /// the session; the page uploads it once.
    pub fn play_menu_font_rgba(&self) -> Vec<u8> {
        self.menu_assets
            .as_ref()
            .map(|a| a.font.atlas_rgba().to_vec())
            .unwrap_or_default()
    }

    /// `[width, height]` of the font atlas.
    pub fn play_menu_font_dims(&self) -> Vec<u32> {
        self.menu_assets
            .as_ref()
            .map(|a| {
                let (w, h) = a.font.atlas_dimensions();
                vec![w, h]
            })
            .unwrap_or_else(|| vec![0, 0])
    }

    /// The assembled menu-chrome atlas (RGBA8) the sprite draws sample. Empty
    /// when no chrome resolved.
    pub fn play_menu_chrome_rgba(&self) -> Vec<u8> {
        self.menu_assets
            .as_ref()
            .and_then(|a| a.chrome.as_ref())
            .map(|(atlas, _)| atlas.rgba.clone())
            .unwrap_or_default()
    }

    /// `[width, height]` of the chrome atlas; `[0, 0]` when none.
    pub fn play_menu_chrome_dims(&self) -> Vec<u32> {
        self.menu_assets
            .as_ref()
            .and_then(|a| a.chrome.as_ref())
            .map(|(atlas, _)| vec![atlas.width, atlas.height])
            .unwrap_or_else(|| vec![0, 0])
    }

    /// `true`: [`Self::play_menu_input`] fires the menu's blip itself, so
    /// the page must not fire one off the raw edge too. A cached bundle
    /// without this export leaves the page on its old raw-edge blip.
    pub fn play_menu_blips_inline(&self) -> bool {
        true
    }
}

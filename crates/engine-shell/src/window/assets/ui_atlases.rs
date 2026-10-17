//! The boot-decoded UI atlases, uploaded once the renderer is live: the
//! publisher logos, the title screen, the menu glyphs and the save-menu
//! sheet with its battle chrome.

use super::*;

impl PlayWindowApp {
    /// Upload each pending UI atlas, in the order the boot decoded them.
    pub(super) fn upload_ui_atlases(&mut self) {
        self.upload_publisher_logos_atlas();
        self.upload_title_screen_atlas();
        self.upload_menu_glyph_atlas();
        self.upload_save_menu_atlas();
    }

    /// Upload the publisher-logos atlas (if pre-decoded at boot) now
    /// that the renderer is live.
    fn upload_publisher_logos_atlas(&mut self) {
        if let (Some(atlas_data), Some(r)) = (
            self.pending_publisher_logos_atlas.take(),
            self.win.renderer.as_ref(),
        ) {
            match r.upload_sprite_atlas(&atlas_data.rgba, atlas_data.width, atlas_data.height) {
                Ok(atlas) => {
                    log::info!(
                        "play-window: publisher-logos atlas uploaded ({}x{})",
                        atlas_data.width,
                        atlas_data.height
                    );
                    self.publisher_logos = Some(PublisherLogosAssets {
                        rects: atlas_data.rects,
                        atlas,
                    });
                }
                Err(e) => log::warn!("publisher-logos atlas upload skipped: {e:#}"),
            }
        }
    }

    /// Upload the title-screen atlas the same way.
    fn upload_title_screen_atlas(&mut self) {
        if let (Some(atlas_data), Some(r)) = (
            self.pending_title_screen_atlas.take(),
            self.win.renderer.as_ref(),
        ) {
            match r.upload_sprite_atlas(&atlas_data.rgba, atlas_data.width, atlas_data.height) {
                Ok(atlas) => {
                    log::info!(
                        "play-window: title-screen atlas uploaded ({}x{})",
                        atlas_data.width,
                        atlas_data.height
                    );
                    self.title_screen = Some(TitleScreenAssets {
                        rect: atlas_data.rect,
                        atlas,
                    });
                }
                Err(e) => log::warn!("title-screen atlas upload skipped: {e:#}"),
            }
        }
    }

    /// Upload the menu-glyph atlas the same way.
    fn upload_menu_glyph_atlas(&mut self) {
        if let (Some(atlas_data), Some(r)) = (
            self.pending_menu_glyph_atlas.take(),
            self.win.renderer.as_ref(),
        ) {
            match r.upload_sprite_atlas(&atlas_data.rgba, atlas_data.width, atlas_data.height) {
                Ok(atlas) => {
                    log::info!(
                        "play-window: menu-glyph atlas uploaded ({}x{})",
                        atlas_data.width,
                        atlas_data.height
                    );
                    self.menu_glyphs = Some(MenuGlyphAssets { atlas });
                }
                Err(e) => log::warn!("menu-glyph atlas upload skipped: {e:#}"),
            }
        }
    }

    /// Upload the save-menu UI atlas (PROT 0899) the same way.
    fn upload_save_menu_atlas(&mut self) {
        if let (Some(atlas_data), Some(r)) = (
            self.pending_save_menu_atlas.take(),
            self.win.renderer.as_ref(),
        ) {
            match r.upload_sprite_atlas(&atlas_data.rgba, atlas_data.width, atlas_data.height) {
                Ok(atlas) => {
                    log::info!(
                        "play-window: save-menu atlas uploaded ({}x{})",
                        atlas_data.width,
                        atlas_data.height
                    );
                    let rects = legaia_engine_screens::field_screens::save_menu_rects(&atlas_data);
                    // The battle HUD's badge cells: which ones actually
                    // carry art is a property of *this* bake (three of the
                    // status badges need the row-511 extension palettes),
                    // so it travels with the atlas rather than being
                    // re-derived from the layout.
                    let badges =
                        legaia_engine_screens::field_screens::battle_badge_rects(&atlas_data);
                    // A fully-opaque texel inside this bake, so the field
                    // party HUD can lay its translucent plate down in the
                    // sprite half - under the labels and numerals that
                    // sample the same atlas, which the text half could not
                    // do (it always composites on top).
                    let solid = legaia_engine_ui::atlas_opaque_texel(
                        &atlas_data.rgba,
                        atlas_data.width,
                        atlas_data.height,
                    );
                    self.save_menu = Some(SaveMenuAssets {
                        rects,
                        atlas,
                        badges,
                        solid,
                    });
                }
                Err(e) => log::warn!("save-menu atlas upload skipped: {e:#}"),
            }
        }
    }
}

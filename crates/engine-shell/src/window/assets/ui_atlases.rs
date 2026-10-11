//! The boot-decoded UI atlases, uploaded once the renderer is live: the
//! publisher logos, the title screen, the menu glyphs and the save-menu
//! sheet with its battle chrome.

use super::*;

/// The save-menu sheet's sprite rects, as the menus and the battle HUD name
/// them.
fn save_menu_rects(
    atlas_data: &legaia_engine_menus::save_menu_atlas::SaveMenuAtlas,
) -> legaia_engine_ui::SaveMenuAtlasRects {
    legaia_engine_ui::SaveMenuAtlasRects {
        panel_tl: atlas_data.band_panel_tl(),
        panel_tr: atlas_data.band_panel_tr(),
        panel_bl: atlas_data.band_panel_bl(),
        panel_br: atlas_data.band_panel_br(),
        panel_top: atlas_data.band_panel_top(),
        panel_bot: atlas_data.band_panel_bot(),
        panel_left: atlas_data.band_panel_left(),
        panel_right: atlas_data.band_panel_right(),
        slot1: atlas_data.band_slot1(),
        slot2: atlas_data.band_slot2(),
        cursor: atlas_data.band_cursor(),
        panel_interior: atlas_data.band_panel_interior(),
        panel_filigree: atlas_data.band_panel_filigree(),
        label_lv: atlas_data.band_label_lv(),
        label_hp: atlas_data.band_label_hp(),
        label_mp: atlas_data.band_label_mp(),
        icon_money: atlas_data.band_icon_money(),
        label_time: atlas_data.band_label_time(),
        label_coin: atlas_data.band_label_coin(),
        gauge_cap: atlas_data.band_gauge_cap(),
        gauge_trough: atlas_data.band_gauge_trough(),
        gauge_box: atlas_data.band_gauge_box(),
        gauge_tip: atlas_data.band_gauge_tip(),
        gauge_digits: atlas_data.band_gauge_digits(),
        gauge_100: atlas_data.band_gauge_100(),
        gauge_fill: atlas_data.band_gauge_fill(),
        dialog_fill: atlas_data.band_dialog_fill(),
        icon_weapon: atlas_data.band_icon_weapon(),
        icon_helmet: atlas_data.band_icon_helmet(),
        icon_armor: atlas_data.band_icon_armor(),
        icon_boot: atlas_data.band_icon_boot(),
        icon_goods: atlas_data.band_icon_goods(),
        pager_left: atlas_data.band_pager_left(),
        pager_right: atlas_data.band_pager_right(),
        tab_cap_l: atlas_data.band_tab_cap_l(),
        tab_body: atlas_data.band_tab_body(),
        tab_cap_r: atlas_data.band_tab_cap_r(),
        atr_icons: atlas_data.band_atr_icons(),
        load_empty_frame: Some(atlas_data.band_load_empty_frame()),
        load_portrait_by_char: [
            atlas_data.band_load_portrait(0),
            atlas_data.band_load_portrait(1),
            atlas_data.band_load_portrait(2),
        ],
        battle: Some(legaia_engine_ui::BattleChromeRects {
            panel_bg: atlas_data.band_battle_panel_bg(),
            plate_cap_l: atlas_data.band_battle_plate_cap_l(),
            plate_body: atlas_data.band_battle_plate_body(),
            plate_cap_r: atlas_data.band_battle_plate_cap_r(),
            separator: atlas_data.band_battle_separator(),
            digits: atlas_data.band_hud_digits(),
            cross_out: atlas_data.band_cross_out(),
            rot_stamp: atlas_data.band_rot_stamp(),
            curse_plate: atlas_data.band_curse_plate(),
        }),
    }
}

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
                    let rects = save_menu_rects(&atlas_data);
                    // The battle HUD's badge cells: which ones actually
                    // carry art is a property of *this* bake (three of the
                    // status badges need the row-511 extension palettes),
                    // so it travels with the atlas rather than being
                    // re-derived from the layout.
                    let badges = legaia_engine_ui::battle_hud_chrome::BattleBadgeRects {
                        status: atlas_data.band_status_badges(),
                        element: atlas_data.band_element_badges(),
                    };
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

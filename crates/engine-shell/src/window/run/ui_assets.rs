//! The UI assets a play window launches with, decoded off the boot source
//! before the renderer exists: the dialog font, the publisher-logo, title,
//! menu-glyph and save-menu atlases, the menu window table, the mounted
//! memory card and the boot screen the window opens on.

use super::*;

/// Text metrics, best source first. `extracted/font/` (a `font-extract`
/// run) wins when present; otherwise the boot source's own disc bytes
/// (PROT.DAT font TIM + the SCUS width table) - both produce the same
/// atlas + advance table, so this is a source preference, not a fidelity
/// one. The fixed-width placeholder is the last resort: it advances every
/// glyph 9px where retail advances `widths[c] + 1` (4..10), which stretched
/// every string by roughly a third.
pub(super) fn load_dialog_font(session: &NativeSession, extracted_root: &Path) -> Font {
    Font::load_from_extracted(extracted_root)
        .map_err(|e| log::debug!("extracted/font not loaded ({e:#}); trying the disc"))
        .ok()
        // The `0xCE` escape sprites ride the atlas whichever source won.
        .map(|f| match session.escape_icons.as_ref() {
            Some(icons) => f.with_escape_icons(icons),
            None => f,
        })
        .or_else(|| session.dialog_font.clone())
        .unwrap_or_else(|| {
            log::warn!("dialog font unavailable; falling back to the placeholder font");
            Font::placeholder()
        })
}

/// Try to decode the publisher logos from PROT 0895 (init.pak) up
/// front. Falls back silently when the disc isn't loaded or the
/// entry doesn't parse - retail discs always have it.
pub(super) fn build_publisher_logos_atlas(
    session: &NativeSession,
) -> Option<legaia_engine_title::publisher_logos::LogosAtlas> {
    match session
        .host
        .index
        .entry_bytes(legaia_ui_formats::init_pak::PROT_INDEX as u32)
    {
        Ok(b) => match legaia_engine_title::publisher_logos::build_atlas_from_init_pak(&b) {
            Ok(a) => {
                log::info!(
                    "play-window: publisher-logos atlas built ({}x{}, {} logos)",
                    a.width,
                    a.height,
                    a.rects.len()
                );
                Some(a)
            }
            Err(e) => {
                log::warn!("play-window: publisher-logos build failed: {e:#}");
                None
            }
        },
        Err(e) => {
            log::warn!("play-window: PROT 0895 read failed: {e:#}");
            None
        }
    }
}

/// Try to decode the title-screen TIM from PROT 0888 (`sound_data2`
/// per CDNAME, actually carries title art) up front. Falls back
/// silently when the disc isn't loaded or the entry doesn't parse -
/// retail discs always have it.
pub(super) fn build_title_screen_atlas(
    session: &NativeSession,
) -> Option<legaia_engine_title::title_screen_atlas::TitleScreenAtlas> {
    match session
        .host
        .index
        .entry_bytes(legaia_ui_formats::title_pak::PROT_INDEX_PRIMARY as u32)
    {
        Ok(b) => match legaia_engine_title::title_screen_atlas::build_atlas_from_prot_888(
            &b,
            legaia_ui_formats::title_pak::TITLE_TIM_OFFSET,
        ) {
            Ok(a) => {
                log::info!(
                    "play-window: title-screen atlas built ({}x{})",
                    a.width,
                    a.height
                );
                Some(a)
            }
            Err(e) => {
                log::warn!("play-window: title-screen build failed: {e:#}");
                None
            }
        },
        Err(e) => {
            log::warn!("play-window: PROT 0888 read failed: {e:#}");
            None
        }
    }
}

/// Try to decode the menu-glyph atlas from the unindexed pre-init_data
/// gap in `PROT.DAT` (offset `0x11218`). Carries the small-caps font
/// retail samples for "NEW GAME" / "CONTINUE" menu rows. The
/// per-entry extractor never visits this gap, so we read PROT.DAT
/// raw bytes - see `legaia_ui_formats::menu_glyph_atlas`.
pub(super) fn build_menu_glyph_atlas(
    session: &NativeSession,
) -> Option<legaia_engine_menus::menu_glyph_atlas::MenuGlyphAtlas> {
    match session.host.index.prot_dat_raw_bytes(
        legaia_ui_formats::menu_glyph_atlas::PROT_DAT_OFFSET,
        legaia_ui_formats::menu_glyph_atlas::TIM_SIZE,
    ) {
        Ok(b) => match legaia_engine_menus::menu_glyph_atlas::build_atlas_from_prot_dat_slice(&b) {
            Ok(a) => {
                log::info!(
                    "play-window: menu-glyph atlas built ({}x{})",
                    a.width,
                    a.height
                );
                Some(a)
            }
            Err(e) => {
                log::warn!("play-window: menu-glyph build failed: {e:#}");
                None
            }
        },
        Err(e) => {
            log::warn!("play-window: PROT.DAT raw read failed: {e:#}");
            None
        }
    }
}

/// The battle HUD's 8x12 numeral cells come off the same menu-glyph TIM
/// as the small-caps rows above, through its sub-palette 13. Baked into
/// the save-menu atlas (below) rather than sampled from the menu-glyph
/// atlas so the HUD's sprite list stays one texture.
pub(super) fn read_menu_glyph_tim(session: &NativeSession) -> Option<Vec<u8>> {
    session
        .host
        .index
        .prot_dat_raw_bytes(
            legaia_ui_formats::menu_glyph_atlas::PROT_DAT_OFFSET,
            legaia_ui_formats::menu_glyph_atlas::TIM_SIZE,
        )
        .ok()
}

/// The menus' bold fixed-width numerals and the list pager pieces ride
/// the font atlas as sprite cells, so every menu number draws off the
/// same texture as its labels (`save_menu_atlas::menu_font_cells`; the
/// browser page attaches the same cells).
pub(super) fn attach_menu_font_cells(
    session: &NativeSession,
    font: Font,
    menu_glyph_tim_bytes: &Option<Vec<u8>>,
) -> Font {
    match session.host.index.prot_dat_raw_bytes(
        legaia_engine_menus::save_menu_atlas::SYSTEM_UI_CLUT_EXT_TIM_OFFSET as u64,
        legaia_ui_formats::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_OFFSET
            + legaia_ui_formats::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_SIZE
            - legaia_engine_menus::save_menu_atlas::SYSTEM_UI_CLUT_EXT_TIM_OFFSET,
    ) {
        Ok(system_ui) => {
            font.with_sprite_cells(&legaia_engine_menus::save_menu_atlas::menu_font_cells(
                &system_ui,
                menu_glyph_tim_bytes.as_deref(),
            ))
        }
        Err(_) => font,
    }
}

/// Decode the save-menu UI atlas from its two disc sources, plus the
/// optional menu-glyph TIM for the HUD numerals.
pub(super) fn build_save_menu_atlas(
    session: &NativeSession,
    menu_glyph_tim_bytes: &Option<Vec<u8>>,
) -> Option<legaia_engine_menus::save_menu_atlas::SaveMenuAtlas> {
    // Try to decode the save-menu UI atlas. Needs TWO disc sources:
    //   1. PROT 0899's extended footprint @ `OVERLAY_SAVE_MENU_TIM_OFFSET`
    //      carries the SLOT 1 / SLOT 2 pill sprites (CLUT 7).
    //   2. Raw PROT.DAT @ `OVERLAY_SYSTEM_UI_TIM_OFFSET = 0x018E0`
    //      carries the 9-slice panel chrome (CLUT row 2).
    // Plus the optional menu-glyph TIM above for the HUD numerals.
    // The atlas builder composites both into one 256x256 RGBA atlas;
    // see `crates/engine-menus/src/save_menu_atlas.rs`. The 9-slice
    // tile geometry was pinned via `scripts/pcsx-redux/scan_panel_prims.py`
    // against sstate9's RAM dump - every primitive's source u/v + CLUT
    // is byte-pinned to the retail render.
    match (
        session
            .host
            .index
            .entry_bytes_extended(legaia_ui_formats::title_pak::PROT_INDEX_OVERLAY as u32),
        // Pull a slice that covers BOTH the system-UI sheet (panel
        // chrome, cursor) AND the load-screen portrait + frame TIMs
        // (`OVERLAY_LOAD_PORTRAIT_TIM_OFFSET`..end of
        // `OVERLAY_LOAD_EMPTY_FRAME_TIM`). The slice starts at the
        // system-UI TIM header so existing offsets stay
        // slice-relative; `build_atlas` handles both shapes.
        {
            // Rooted one TIM earlier than the sheet, at the row-511 CLUT
            // extension: it carries sub-palettes 16..18, which three of the
            // nine status-element badges decode with. `build_atlas` splits
            // that leading TIM off and treats the remainder exactly as a
            // sheet-rooted slice.
            let base = legaia_engine_menus::save_menu_atlas::SYSTEM_UI_CLUT_EXT_TIM_OFFSET;
            let end = legaia_ui_formats::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_OFFSET
                + legaia_ui_formats::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_SIZE;
            session
                .host
                .index
                .prot_dat_raw_bytes(base as u64, end - base)
        },
    ) {
        (Ok(pill_bytes), Ok(panel_bytes)) => {
            match legaia_engine_menus::save_menu_atlas::build_atlas(
                &panel_bytes,
                &pill_bytes,
                menu_glyph_tim_bytes.as_deref(),
            ) {
                Ok(mut a) => {
                    // The red cross-out X lives on the battle effect page, not
                    // the system-UI sheet; bake it into the same atlas so it
                    // draws in the chip list (the browser page bakes it too).
                    if let Ok(flame) = session.host.index.entry_bytes_extended(
                        legaia_engine_menus::save_menu_atlas::FLAME_ATLAS_PROT_ENTRY,
                    ) {
                        legaia_engine_menus::save_menu_atlas::add_cross_out_mark(&mut a, &flame);
                    }
                    log::info!(
                        "play-window: save-menu atlas built ({}x{}) - 9-slice from PROT.DAT[0x018E0] + pills from PROT 0899",
                        a.width,
                        a.height
                    );
                    Some(a)
                }
                Err(e) => {
                    log::warn!("play-window: save-menu build failed: {e:#}");
                    None
                }
            }
        }
        (Err(e), _) => {
            log::warn!("play-window: PROT 0899 read failed: {e:#}");
            None
        }
        (_, Err(e)) => {
            log::warn!("play-window: PROT.DAT raw read failed: {e:#}");
            None
        }
    }
}

/// Parse the menu overlay's window-descriptor table (PROT 0899
/// @0x15F20): the retail window rects behind every pause-menu screen.
/// Falls back to the pinned mirror consts when unavailable.
pub(super) fn read_menu_window_table(
    session: &mut NativeSession,
) -> Option<legaia_game_tables::menu_windows::MenuWindowTable> {
    session
        .host
        .index
        // The table sits at file 0x15F20, past the entry's TOC size - read
        // the extended footprint (the same read the save-menu pill TIM uses).
        .entry_bytes_extended(legaia_game_tables::menu_windows::MENU_OVERLAY_PROT_INDEX as u32)
        .ok()
        .and_then(|b| {
            // The same overlay carries the Arrange display-order table
            // (FUN_801D64A8): install it so the Items screen's Arrange
            // command sorts by the retail rank rather than id order.
            session.host.world.install_menu_overlay_tables(&b);
            match legaia_game_tables::menu_windows::parse(&b) {
                Ok(t) => Some(t),
                Err(e) => {
                    log::warn!("play-window: menu window table parse failed: {e:#}");
                    None
                }
            }
        })
}

/// Port 2 of the save screen's rack, mounted once. A container the
/// detector does not recognise is a mistake worth naming, not an empty
/// port, so the mount failure logs instead of silently leaving `None`.
pub(super) fn mount_card(card: Option<&Path>) -> Option<MountedCard> {
    card.and_then(|p| match MountedCard::open(p) {
        Ok(c) => Some(c),
        Err(e) => {
            log::warn!("play-window: --card not mounted: {e:#}");
            None
        }
    })
}

/// The boot screen the window opens on.
///
/// Cold boot with no publisher logos on the disc goes straight to the
/// title, so the Continue-enable scan happens here instead of in the
/// mode-table hand-off - and it has to scan BOTH ports, exactly as that
/// hand-off does. Scanning port 1 alone greys the row out for a player whose
/// only save is on the memory-card image they mounted, which is the one
/// thing `--card` exists for.
pub(super) fn initial_boot_ui(
    boot_ui: bool,
    has_publisher_logos: bool,
    cards: [Option<&MountedCard>; 2],
    seat: &legaia_engine_mode::mode::ModeSeat,
) -> BootUiState {
    if boot_ui {
        // The session's one cold-boot rule (`FrontEnd::cold_boot`): the
        // logos when they built, else the title with the rack's Continue bit.
        BootUiState::FrontEnd(legaia_engine_session::FrontEnd::cold_boot(
            seat,
            has_publisher_logos,
            rack_has_save(cards),
        ))
    } else {
        BootUiState::Inactive
    }
}

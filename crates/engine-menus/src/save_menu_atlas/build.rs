//! `build_atlas`: the bake of the base atlas off the system-UI sheets, plus the menu font cell table.

use super::*;

/// Build a [`SaveMenuAtlas`] from raw `PROT.DAT` bytes (carries the
/// system-UI TIM at offset `0x018E0`) plus the trailing-overlay
/// bytes of PROT 0899 (carries the save-menu TIM with the slot pills).
///
/// The panel tiles are decoded from the system-UI TIM with CLUT row 2
/// - byte-equal to the retail VRAM contents at parked-on-load-screen
///   sstate9. The slot pills are decoded from PROT 0899 with CLUT 7 -
///   byte-equal as well.
///
/// `menu_glyph_tim` is the optional third source: the
/// [`legaia_ui_formats::menu_glyph_atlas`] TIM slice (`PROT.DAT[0x11218]`),
/// which carries the battle HUD's 8x12 numeral cells. Pass `None` and
/// [`ATLAS_RECT_HUD_DIGITS`] stays transparent - every consumer treats a
/// blank strip as "no digit art" and falls back to font glyphs, so a
/// caller that cannot reach the TIM still gets a usable atlas.
pub fn build_atlas(
    prot_dat_bytes: &[u8],
    prot_0899_bytes: &[u8],
    menu_glyph_tim: Option<&[u8]>,
) -> anyhow::Result<SaveMenuAtlas> {
    // A caller may root its slice one TIM earlier, at the row-511 CLUT
    // extension (`SYSTEM_UI_CLUT_EXT_TIM_OFFSET`), to bring the status
    // badges' sub-palettes 16..18 along. Split that off first so every
    // offset below still measures from the system-UI sheet.
    let (clut_ext_tim, prot_dat_bytes) = split_leading_clut_ext(prot_dat_bytes);
    // --- Slot pills from PROT 0899 ---
    let pill_tim = title_pak::extract_overlay_save_menu_tim(prot_0899_bytes)?;
    let pill_parsed = legaia_tim::parse(pill_tim.bytes)?;
    let pill_w = pill_parsed.pixel_width() as u32;
    let pill_h = pill_parsed.image.h as u32;
    if pill_w != ATLAS_WIDTH || pill_h != ATLAS_HEIGHT {
        anyhow::bail!(
            "save-menu TIM dims {}x{} != expected {}x{}",
            pill_w,
            pill_h,
            ATLAS_WIDTH,
            ATLAS_HEIGHT
        );
    }
    let pill_rgba = legaia_tim::decode_rgba8(&pill_parsed, PILL_CLUT)?;

    // --- Panel chrome from PROT.DAT[0x018E0] ---
    // `prot_dat_bytes` is a slice that already starts at the TIM
    // header (callers pull just this region via
    // `prot_dat_raw_bytes(OVERLAY_SYSTEM_UI_TIM_OFFSET, …)`), so use
    // the slice-relative parser to avoid double-applying the offset.
    let panel_tim = title_pak::extract_overlay_system_ui_tim_from_slice(prot_dat_bytes)?;
    let panel_parsed = legaia_tim::parse(panel_tim.bytes)?;
    let panel_src_w = panel_parsed.pixel_width() as u32;
    let panel_src_h = panel_parsed.image.h as u32;
    if panel_src_w != 256 || panel_src_h != 192 {
        anyhow::bail!(
            "system-UI TIM dims {}x{} != expected 256x192",
            panel_src_w,
            panel_src_h
        );
    }
    let panel_rgba = legaia_tim::decode_rgba8(&panel_parsed, PANEL_CLUT_ROW)?;
    // Cursor decoded with a different CLUT row of the same TIM.
    let cursor_rgba = legaia_tim::decode_rgba8(&panel_parsed, CURSOR_CLUT_ROW)?;

    // --- Compose into single 256x256 atlas ---
    let mut out = vec![0u8; (ATLAS_WIDTH * ATLAS_HEIGHT * 4) as usize];

    // Slot pills - copy from pill plane (256x256) at retail src coords.
    copy_rect(
        &mut out,
        ATLAS_WIDTH,
        &pill_rgba,
        pill_w,
        title_pak::OVERLAY_SAVE_MENU_BAND_SLOT1,
        title_pak::OVERLAY_SAVE_MENU_BAND_SLOT1,
    );
    copy_rect(
        &mut out,
        ATLAS_WIDTH,
        &pill_rgba,
        pill_w,
        title_pak::OVERLAY_SAVE_MENU_BAND_SLOT2,
        title_pak::OVERLAY_SAVE_MENU_BAND_SLOT2,
    );

    // The block badge and the write panel's progress tube - records 2 / 3 /
    // 4 of the save UI's sprite-record table (`0x801E5048`), same TIM.
    add_card_work_sprites(&mut out, &pill_parsed, pill_w)?;

    // Panel 9-slice tiles - copy from panel plane (256x192) into
    // atlas at the same source coords (160..192, 0..32). Those atlas
    // pixels are unused in the PROT 0899 layout, so the panel tiles
    // and pills coexist in a single 256x256 atlas.
    for tile in [
        title_pak::OVERLAY_SYSTEM_UI_PANEL_TL,
        title_pak::OVERLAY_SYSTEM_UI_PANEL_TR,
        title_pak::OVERLAY_SYSTEM_UI_PANEL_BL,
        title_pak::OVERLAY_SYSTEM_UI_PANEL_BR,
        title_pak::OVERLAY_SYSTEM_UI_PANEL_TOP,
        title_pak::OVERLAY_SYSTEM_UI_PANEL_BOT,
        title_pak::OVERLAY_SYSTEM_UI_PANEL_LEFT,
        title_pak::OVERLAY_SYSTEM_UI_PANEL_RIGHT,
    ] {
        copy_rect(&mut out, ATLAS_WIDTH, &panel_rgba, panel_src_w, tile, tile);
    }

    // The dark class-0 skin (widget record 0x43): the same nine cells one
    // skin row down, through sub-palette 3. The fill stays raw - the frame
    // builder shades it row by row, as `FUN_8002BDC4`'s vertex greys do.
    let dark_rgba = legaia_tim::decode_rgba8(&panel_parsed, CLASS0_DARK_CLUT_ROW)?;
    for cell in ATLAS_RECTS_CLASS0_DARK {
        copy_rect(&mut out, ATLAS_WIDTH, &dark_rgba, panel_src_w, cell, cell);
    }

    // Pointing-finger cursor - same TIM, different CLUT row. Source
    // rect (152, 64, 16, 16) is well outside both the panel-tile and
    // pill regions, so it slots in without overlap.
    copy_rect(
        &mut out,
        ATLAS_WIDTH,
        &cursor_rgba,
        panel_src_w,
        title_pak::OVERLAY_SYSTEM_UI_CURSOR,
        title_pak::OVERLAY_SYSTEM_UI_CURSOR,
    );

    // "Condition" pager triangles - same CLUT row 7 plane as the cursor
    // (the FUN_8002b994 sprite-table kinds 2/3, frame-0 UVs).
    for (src, dst) in [
        (
            title_pak::OVERLAY_SYSTEM_UI_PAGER_LEFT,
            ATLAS_RECT_PAGER_LEFT,
        ),
        (
            title_pak::OVERLAY_SYSTEM_UI_PAGER_RIGHT,
            ATLAS_RECT_PAGER_RIGHT,
        ),
    ] {
        copy_rect(&mut out, ATLAS_WIDTH, &cursor_rgba, panel_src_w, src, dst);
    }

    // The dialogue page mark's two frames - the cursor primitive's record 1,
    // same CLUT row 7 plane.
    for (uv, dst) in legaia_engine_dialog::cursor_sprite::CURSOR_RECORDS
        [legaia_engine_dialog::cursor_sprite::KIND_PAGE_MARK]
        .uv
        .iter()
        .zip(ATLAS_RECT_ADVANCE_ICON)
    {
        let src = (u32::from(uv.0), u32::from(uv.1), 16, 16);
        copy_rect(&mut out, ATLAS_WIDTH, &cursor_rgba, panel_src_w, src, dst);
    }

    // Tab-banner plaque pieces - CLUT row 12 (carved brown ramp; the
    // scan_panel_prims pin over the menu_status_town capture).
    let tab_rgba = legaia_tim::decode_rgba8(
        &panel_parsed,
        title_pak::OVERLAY_SYSTEM_UI_TAB_CLUT_ROW as usize,
    )?;
    for (src, dst) in [
        (title_pak::OVERLAY_SYSTEM_UI_TAB_CAP_L, ATLAS_RECT_TAB_CAP_L),
        (title_pak::OVERLAY_SYSTEM_UI_TAB_BODY, ATLAS_RECT_TAB_BODY),
        (title_pak::OVERLAY_SYSTEM_UI_TAB_CAP_R, ATLAS_RECT_TAB_CAP_R),
    ] {
        copy_rect(&mut out, ATLAS_WIDTH, &tab_rgba, panel_src_w, src, dst);
    }

    // Arts command-input chrome - three more sub-palettes of the same
    // system-UI plane. The input screen is shared by the battle Arts
    // command and the Muscle Dome's Attack (the dome runs it verbatim),
    // so both hosts sample these from one bake. Every piece lands at its
    // natural sheet coordinate except the direction-chip triple, whose
    // rows the portrait strip already owns - `arts_chip_atlas_rect`
    // re-seats those three. See `legaia_engine_ui::arts_input`.
    add_arts_input_sprites(&mut out, &panel_parsed, panel_src_w)?;

    // ATR element icons from the extension-strip TIM (best-effort: skip
    // silently when the caller's slice doesn't reach it, mirroring the
    // portrait handling below).
    add_atr_icon_sprites(&mut out, prot_dat_bytes)?;

    // Panel interior tile - pre-baked with the gouraud gray gradient
    // retail applies via the 0x3C textured-quad primitives. The
    // source region (128..160, 0..29) of CLUT row 2 carries the
    // marbled-blue stippled pattern; we multiply each pixel by a
    // vertical gradient (top = dark gray 64/255, bottom = lighter
    // gray 136/255) to match the per-vertex color modulation, then
    // copy into the atlas at the natural source coords so engines
    // sample via `band_panel_interior()`.
    bake_panel_interior_gradient(
        &mut out,
        &panel_rgba,
        panel_src_w,
        title_pak::OVERLAY_SYSTEM_UI_PANEL_INTERIOR,
        title_pak::OVERLAY_SYSTEM_UI_PANEL_INTERIOR_TOP_RGB,
        title_pak::OVERLAY_SYSTEM_UI_PANEL_INTERIOR_BOT_RGB,
    );

    // Raw (un-baked) copy of the same marbled-filigree region into a
    // free atlas slot, so the pause-menu chrome can tile it in 2D and
    // apply its own darkening colour (the save screen keeps the
    // gouraud-baked variant above; the field menu wants the plain
    // repeating damask).
    copy_rect(
        &mut out,
        ATLAS_WIDTH,
        &panel_rgba,
        panel_src_w,
        FILIGREE_SRC,
        ATLAS_RECT_FILIGREE,
    );

    // Status-panel stat labels (LV / HP / MP) - same sheet, CLUT row 1
    // (the `0x800732a4` icon-record palette; pixel-exact vs the golden
    // menu_status_town capture). The status page draws these as sprites
    // in place of ASCII glyphs.
    let label_rgba = legaia_tim::decode_rgba8(
        &panel_parsed,
        title_pak::OVERLAY_SYSTEM_UI_LABEL_CLUT_ROW as usize,
    )?;
    for (src, dst) in [
        (title_pak::OVERLAY_SYSTEM_UI_LABEL_LV, ATLAS_RECT_LABEL_LV),
        (title_pak::OVERLAY_SYSTEM_UI_LABEL_HP, ATLAS_RECT_LABEL_HP),
        (title_pak::OVERLAY_SYSTEM_UI_LABEL_MP, ATLAS_RECT_LABEL_MP),
        // Top-level pause menu money/time/coin box icons (UI-icon
        // codes 0x62 / 0x63 / 0x66) - same label CLUT row.
        (
            title_pak::OVERLAY_SYSTEM_UI_ICON_MONEY,
            ATLAS_RECT_ICON_MONEY,
        ),
        (
            title_pak::OVERLAY_SYSTEM_UI_LABEL_TIME,
            ATLAS_RECT_LABEL_TIME,
        ),
        (
            title_pak::OVERLAY_SYSTEM_UI_LABEL_COIN,
            ATLAS_RECT_LABEL_COIN,
        ),
    ] {
        copy_rect(&mut out, ATLAS_WIDTH, &label_rgba, panel_src_w, src, dst);
    }

    // Status-page AP gauge pieces + red value digits - same sheet, CLUT
    // row 4 (the status gauge palette; pixel-exact vs the golden
    // menu_status_town capture at the FUN_801D33D8 bar anchor).
    let gauge_rgba = legaia_tim::decode_rgba8(
        &panel_parsed,
        title_pak::OVERLAY_SYSTEM_UI_GAUGE_CLUT_ROW as usize,
    )?;
    for (src, dst) in [
        (title_pak::OVERLAY_SYSTEM_UI_GAUGE_CAP, ATLAS_RECT_GAUGE_CAP),
        (
            title_pak::OVERLAY_SYSTEM_UI_GAUGE_TROUGH,
            ATLAS_RECT_GAUGE_TROUGH,
        ),
        (title_pak::OVERLAY_SYSTEM_UI_GAUGE_BOX, ATLAS_RECT_GAUGE_BOX),
        (title_pak::OVERLAY_SYSTEM_UI_GAUGE_TIP, ATLAS_RECT_GAUGE_TIP),
        (
            title_pak::OVERLAY_SYSTEM_UI_GAUGE_DIGITS,
            ATLAS_RECT_GAUGE_DIGITS,
        ),
    ] {
        copy_rect(&mut out, ATLAS_WIDTH, &gauge_rgba, panel_src_w, src, dst);
    }

    // The "100" full-gauge glyph (ICO record 0x6B) decodes with the
    // label CLUT row, not the gauge row.
    copy_rect(
        &mut out,
        ATLAS_WIDTH,
        &label_rgba,
        panel_src_w,
        title_pak::OVERLAY_SYSTEM_UI_GAUGE_100,
        ATLAS_RECT_GAUGE_100,
    );

    // AP-gauge meter-fill gradient column - synthesized, not
    // TIM-sourced: retail's `FUN_8002c0b0` draws the fill as two
    // untextured gouraud quads (dark-red -> gold over rows 0..2, gold
    // -> dark-red over rows 3..5). Bake the per-row linear
    // interpolation of those endpoints so the engine can stretch one
    // sprite to the fill width.
    bake_gauge_fill_gradient(&mut out, ATLAS_RECT_GAUGE_FILL);

    // Dialog-window interior fill gradient column - synthesized, not
    // TIM-sourced: retail's `FUN_8002C69C` fills every dialog / menu
    // box with two stacked semi-transparent gouraud quads (dark navy
    // top -> brighter blue bottom, mode-0 blend applied twice =
    // 0.25*back + 0.75*gradient). Bake the per-row interpolation at
    // alpha 191 so one stretched alpha-blended sprite matches it.
    bake_dialog_fill_gradient(&mut out, ATLAS_RECT_DIALOG_FILL);

    // Equipment pictograms - same sheet, CLUT row 8 (the gold icon
    // ramp; the `0x800732a4` UV/CLUT table records for icon codes
    // 0x24/0x22/0x23/0x25/0x46).
    let icon_rgba = legaia_tim::decode_rgba8(
        &panel_parsed,
        title_pak::OVERLAY_SYSTEM_UI_ICON_CLUT_ROW as usize,
    )?;
    for (src, dst) in [
        (
            title_pak::OVERLAY_SYSTEM_UI_ICON_WEAPON,
            ATLAS_RECT_ICON_WEAPON,
        ),
        (
            title_pak::OVERLAY_SYSTEM_UI_ICON_HELMET,
            ATLAS_RECT_ICON_HELMET,
        ),
        (
            title_pak::OVERLAY_SYSTEM_UI_ICON_ARMOR,
            ATLAS_RECT_ICON_ARMOR,
        ),
        (title_pak::OVERLAY_SYSTEM_UI_ICON_BOOT, ATLAS_RECT_ICON_BOOT),
        (
            title_pak::OVERLAY_SYSTEM_UI_ICON_GOODS,
            ATLAS_RECT_ICON_GOODS,
        ),
    ] {
        copy_rect(&mut out, ATLAS_WIDTH, &icon_rgba, panel_src_w, src, dst);
    }

    // Load-screen slot-grid: empty-cell frame + 3 character portrait
    // TIMs. These live just past the system-UI sheet in the unindexed
    // pre-`init_data` gap of PROT.DAT, so the caller needs to pass the
    // full PROT.DAT buffer too (the system-UI-only slice can't reach
    // them).
    add_load_slot_grid_sprites(&mut out, prot_dat_bytes)?;

    // Battle-screen chrome - the roster panel plate, the blue plate 3-slice
    // and the `/` separator, three more sub-palettes of the same system-UI
    // plane. All at their natural sheet coordinates
    // (`engine_ui::battle_chrome`), which the atlas happens to leave free.
    add_battle_chrome_sprites(&mut out, &panel_parsed, panel_src_w)?;

    // The nine status-element badges the exclusive ladder selects, each on
    // its own row-511 sub-palette. Three of them reach past the sheet TIM's
    // own sixteen palettes into the extension block, so the returned mask
    // says which cells actually carry art.
    let status_badges_baked =
        add_status_badge_sprites(&mut out, &panel_parsed, panel_src_w, clut_ext_tim)?;

    // The eight plain element badges. Their texels live on the extension
    // strip below the sheet, not on the sheet, and their palettes come out
    // of two sibling CLUT TIMs - so this is a whole separate source chain.
    let element_badges_baked = add_element_badge_sprites(&mut out, prot_dat_bytes)?;

    // Battle-HUD numeral cells, off a different TIM entirely. Optional:
    // a caller without the menu-glyph slice leaves the strip blank and
    // the HUD falls back to font glyphs.
    let mut has_hud_digits = false;
    if let Some(tim) = menu_glyph_tim {
        add_hud_digit_strip(&mut out, tim)?;
        has_hud_digits = true;
    }

    Ok(SaveMenuAtlas {
        rgba: out,
        width: ATLAS_WIDTH,
        height: ATLAS_HEIGHT,
        has_hud_digits,
        status_badges_baked,
        element_badges_baked,
        has_cross_out: false,
        has_rot_stamp: false,
        has_curse_plate: false,
    })
}

/// Separator glyph cells [`menu_font_cells`] cuts (`FUN_8003C1F8` ids
/// `0..14`, through the `.` at `0xD`; the cells past it are blank).
pub const SEPARATOR_GLYPHS: u32 = 14;

/// The menu sprite cells retail draws in place of font glyphs, decoded off
/// the disc for [`legaia_font::Font::with_sprite_cells`]:
///
/// * the ten 8x12 numerals the fixed-width number primitive `FUN_80034B78`
///   blits (`uv = (d * 8, 208, 8, 12)` on the menu-glyph page, the same
///   cells the battle HUD reads - [`add_hud_digit_strip`]); every menu
///   price, count, stat and purse reads them, white-palette, tinted per ink;
/// * the fourteen 8x12 separator glyphs beside them on the same row
///   (`FUN_8003C1F8`, `uv = (0x50 + g * 8, 208)`): the `/` between two
///   counts and the stat-compare rise / fall arrows;
/// * the list kernel `FUN_80032A44`'s pager pieces off the system-UI sheet,
///   at their UI-icon records (`0x800732A4 + code * 12`, `+3` CLUT byte,
///   `+4..+7` U/V/W/H): the "PAGE" tag (code `0x76`, CLUT 1), the 6x8 page
///   digits (`0x7A..=0x83`, CLUT 8) and slash (`0x79`, CLUT 8), and the two
///   8x8 page triangles (`0x27` / `0x28`, CLUT 7).
///
/// `system_ui` is the sheet-rooted (or CLUT-extension-rooted) slice
/// [`build_atlas`] takes. A source that does not decode contributes no cells
/// and the menus keep their font glyphs.
pub fn menu_font_cells(
    system_ui: &[u8],
    menu_glyph_tim: Option<&[u8]>,
) -> Vec<legaia_font::SpriteCellSrc> {
    use legaia_font::{SpriteCellSrc, sprite_cell_ids as ids};
    fn cut(rgba: &[u8], src_w: u32, (x, y, w, h): (u32, u32, u32, u32)) -> Vec<u8> {
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for r in 0..h {
            let o = (((y + r) * src_w + x) * 4) as usize;
            out.extend_from_slice(&rgba[o..o + (w * 4) as usize]);
        }
        out
    }
    let mut cells = Vec::new();
    if let Some(tim) = menu_glyph_tim
        .and_then(|t| legaia_ui_formats::menu_glyph_atlas::extract_from_tim_slice(t).ok())
        .and_then(|t| legaia_tim::parse(t.bytes).ok())
    {
        let w = tim.pixel_width() as u32;
        if let Ok(rgba) =
            legaia_tim::decode_rgba8(&tim, title_pak::MENU_GLYPH_ATLAS_TEXT_CLUT_ROW as usize)
        {
            for d in 0..10u32 {
                cells.push(SpriteCellSrc {
                    id: ids::NUMERAL + d as u16,
                    w: 8,
                    h: 12,
                    rgba: cut(&rgba, w, (d * 8, 208, 8, 12)),
                });
            }
            // The separator glyphs `FUN_8003C1F8` draws: `uv = (g * 8 +
            // 0x50, 0xD0, 8, 12)` (`sll a0,a0,0x3` / `addiu a0,a0,0x50` and
            // `li v0,0xd0` at `0x8003C204..0x8003C218`).
            for g in 0..SEPARATOR_GLYPHS {
                cells.push(SpriteCellSrc {
                    id: ids::SEPARATOR + g as u16,
                    w: 8,
                    h: 12,
                    rgba: cut(&rgba, w, (0x50 + g * 8, 208, 8, 12)),
                });
            }
        }
    }
    let (_, sheet) = split_leading_clut_ext(system_ui);
    if let Some(tim) = title_pak::extract_overlay_system_ui_tim_from_slice(sheet)
        .ok()
        .and_then(|t| legaia_tim::parse(t.bytes).ok())
    {
        let w = tim.pixel_width() as u32;
        let mut push = |id: u16, clut: usize, rect: (u32, u32, u32, u32)| {
            if let Ok(rgba) = legaia_tim::decode_rgba8(&tim, clut) {
                cells.push(SpriteCellSrc {
                    id,
                    w: rect.2,
                    h: rect.3,
                    rgba: cut(&rgba, w, rect),
                });
            }
        };
        push(ids::PAGE_TAG, 1, (80, 136, 24, 8));
        push(ids::PAGE_SLASH, 8, (120, 136, 6, 7));
        for d in 0..10u32 {
            push(ids::PAGE_DIGIT + d as u16, 8, (64 + d * 6, 144, 6, 8));
        }
        push(ids::PAGE_PREV, 7, (96, 48, 8, 8));
        push(ids::PAGE_NEXT, 7, (96, 56, 8, 8));
    }
    cells
}

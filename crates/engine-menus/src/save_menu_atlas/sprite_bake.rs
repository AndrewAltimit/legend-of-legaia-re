//! Per-sheet sprite bakes into the atlas: status and element badges, battle chrome, HUD digits, arts input, attribute icons, memory-card and load-slot art.

use super::*;

/// Split a leading row-511 CLUT-extension TIM off the caller's PROT.DAT
/// slice, returning `(extension bytes, the rest)`.
///
/// Three shapes reach here and all three are answered by looking for that
/// TIM's own signature - a CLUT block at VRAM `(256, 511)`:
///
/// * a slice rooted at [`SYSTEM_UI_CLUT_EXT_TIM_OFFSET`] - the extension is
///   the first `SYSTEM_UI_CLUT_EXT_TIM_SIZE` bytes;
/// * the whole `PROT.DAT` - the extension is at its absolute offset and the
///   rest is returned unchanged, since every other offset here is absolute
///   in that case too;
/// * a slice rooted at the system-UI sheet - the extension is *behind* the
///   slice start and simply unreachable.
pub(super) fn split_leading_clut_ext(bytes: &[u8]) -> (Option<&[u8]>, &[u8]) {
    let looks_like_ext = |b: &[u8]| -> bool {
        b.len() >= SYSTEM_UI_CLUT_EXT_TIM_SIZE
            && b[..4] == [0x10, 0, 0, 0]
            && legaia_tim::parse(&b[..SYSTEM_UI_CLUT_EXT_TIM_SIZE])
                .ok()
                .and_then(|t| t.clut.map(|c| (c.fb_x, c.fb_y)))
                == Some((256, 511))
    };
    if looks_like_ext(bytes) {
        return (
            Some(&bytes[..SYSTEM_UI_CLUT_EXT_TIM_SIZE]),
            &bytes[SYSTEM_UI_CLUT_EXT_TIM_SIZE..],
        );
    }
    if bytes.len() > SYSTEM_UI_CLUT_EXT_TIM_OFFSET
        && looks_like_ext(&bytes[SYSTEM_UI_CLUT_EXT_TIM_OFFSET..])
    {
        let end = SYSTEM_UI_CLUT_EXT_TIM_OFFSET + SYSTEM_UI_CLUT_EXT_TIM_SIZE;
        return (Some(&bytes[SYSTEM_UI_CLUT_EXT_TIM_OFFSET..end]), bytes);
    }
    (None, bytes)
}

/// Stamp the nine **status-element badges** into the atlas, one row-511
/// sub-palette each, and return a bitmask of the cells that got art.
///
/// The art settles the ladder's per-bit assignment on its own: decoding the
/// two-column block gives `Venom` / `Toxic` / `Stone` / `Rot` / `Rage` /
/// `Numb` / `Sleep` / `Curse` / `Faint` in exactly `FUN_8002C2E4`'s order.
///
/// Six badges decode with the sheet TIM's own palettes. `Stone`, `Rage` and
/// `Faint` sit on sub-palettes 16 / 17 / 18, past that TIM's sixteen - they
/// take the CLUT-only extension TIM at [`SYSTEM_UI_CLUT_EXT_TIM_OFFSET`],
/// whose block uploads to VRAM `(256, 511)` and continues the same strip.
/// Without it those three cells stay blank and the HUD keeps its tag.
///
/// REF: FUN_8002c2e4
pub(super) fn add_status_badge_sprites(
    dst: &mut [u8],
    panel_parsed: &legaia_tim::Tim,
    src_w: u32,
    clut_ext_tim: Option<&[u8]>,
) -> anyhow::Result<u16> {
    // The extension TIM carries no pixels of its own - only the three
    // palettes - so it is grafted onto the sheet's image the same way the
    // ATR icons graft their row-500 block onto the extension strip.
    let ext_sheet = clut_ext_tim
        .and_then(|b| legaia_tim::parse(b).ok())
        .and_then(|ext| {
            ext.clut.map(|clut| {
                let mut sheet = panel_parsed.clone();
                sheet.clut = Some(clut);
                sheet
            })
        });

    let mut baked = 0u16;
    for (i, cell) in STATUS_BADGE_CELLS.iter().enumerate() {
        let (tim, pal) = if cell.subpalette < SYSTEM_UI_CLUT_EXT_FIRST_SUBPALETTE {
            (panel_parsed, cell.subpalette as usize)
        } else {
            match ext_sheet.as_ref() {
                Some(t) => (
                    t,
                    (cell.subpalette - SYSTEM_UI_CLUT_EXT_FIRST_SUBPALETTE) as usize,
                ),
                None => continue,
            }
        };
        let rgba = legaia_tim::decode_rgba8(tim, pal).map_err(|e| {
            anyhow::anyhow!("status badge {:#04x} decode failed: {e:?}", cell.sprite)
        })?;
        debug_assert_eq!(status_badge_index(cell.sprite), Some(i));
        copy_rect(
            dst,
            ATLAS_WIDTH,
            &rgba,
            src_w,
            (cell.sheet.0, cell.sheet.1, STATUS_BADGE_W, STATUS_BADGE_H),
            status_badge_atlas_rect(i),
        );
        baked |= 1 << i;
    }
    Ok(baked)
}

/// Stamp the eight **plain element badges** into the atlas and return a
/// bitmask of the cells that got art.
///
/// These are not on the system-UI sheet. Their texels are row `v = 192` of
/// the 256x32 extension strip that continues the page at VRAM `(896, 448)`
/// (`title_pak::OVERLAY_SYSTEM_UI_EXT_TIM_OFFSET`), i.e. strip-local
/// `v = 0`; and their palettes come from that strip's own CLUT block (VRAM
/// row 498, badges `0..=3`) and its row-499 sibling (badges `4..=7`). The
/// widget record's palette byte `0x40 + i` is what says so: the bit-6
/// decode walks a 4-wide block of CLUTs row-major, so the low two bits pick
/// the column inside a block and the next two pick the block.
pub(super) fn add_element_badge_sprites(
    dst: &mut [u8],
    prot_dat_bytes: &[u8],
) -> anyhow::Result<u8> {
    let base = title_pak::OVERLAY_SYSTEM_UI_TIM_OFFSET;
    let size = title_pak::OVERLAY_SYSTEM_UI_EXT_TIM_SIZE;
    let has_tim_magic = |off: usize| {
        prot_dat_bytes.len() >= off + 8 && prot_dat_bytes[off..off + 4] == [0x10, 0, 0, 0]
    };
    // Same full-PROT.DAT vs sheet-rooted-slice disambiguation the ATR
    // icons run; both source TIMs sit after the sheet, so a relative root
    // never underflows.
    let strip_abs = title_pak::OVERLAY_SYSTEM_UI_EXT_TIM_OFFSET;
    let row499_abs = ELEMENT_BADGE_ROW499_TIM_OFFSET;
    let (strip_off, row499_off) = if has_tim_magic(strip_abs - base) {
        (strip_abs - base, row499_abs - base)
    } else if has_tim_magic(strip_abs) {
        (strip_abs, row499_abs)
    } else {
        return Ok(0);
    };
    let end = |o: usize| (o + size).min(prot_dat_bytes.len());
    let Ok(strip) = legaia_tim::parse(&prot_dat_bytes[strip_off..end(strip_off)]) else {
        return Ok(0);
    };
    let strip_w = strip.pixel_width() as u32;
    let row499 = legaia_tim::parse(&prot_dat_bytes[row499_off..end(row499_off)])
        .ok()
        .and_then(|t| t.clut)
        .map(|clut| {
            let mut s = strip.clone();
            s.clut = Some(clut);
            s
        });

    let mut baked = 0u8;
    for i in 0..ELEMENT_BADGE_COUNT {
        let (tim, pal) = if i < 4 {
            (&strip, i)
        } else {
            match row499.as_ref() {
                Some(t) => (t, i - 4),
                None => continue,
            }
        };
        let Ok(rgba) = legaia_tim::decode_rgba8(tim, pal) else {
            continue;
        };
        copy_rect(
            dst,
            ATLAS_WIDTH,
            &rgba,
            strip_w,
            (
                ELEMENT_BADGE_SHEET_U0 + i as u32 * ELEMENT_BADGE_SHEET_PITCH,
                0,
                ELEMENT_BADGE_W,
                ELEMENT_BADGE_H,
            ),
            element_badge_atlas_rect(i),
        );
        baked |= 1 << i;
    }
    Ok(baked)
}

/// Stamp the battle screen's own skin into the atlas: the roster-panel
/// plate, the blue plate 3-slice and the `/` separator.
///
/// Three sub-palettes of the system-UI plane, all pinned in
/// `engine_ui::battle_chrome` off retail's display list - sub-palette 0 for
/// the panel, 4 for the blue plate row at `v = 0`, 5 for the separator.
/// The carved-gold plate row at `v = 64` is not repeated here: it is the
/// same art the field menu's tab banner already bakes.
pub(super) fn add_battle_chrome_sprites(
    dst: &mut [u8],
    panel_parsed: &legaia_tim::Tim,
    src_w: u32,
) -> anyhow::Result<()> {
    use title_pak::{
        OVERLAY_SYSTEM_UI_BATTLE_PANEL_BG, OVERLAY_SYSTEM_UI_BATTLE_PANEL_CLUT_ROW,
        OVERLAY_SYSTEM_UI_BATTLE_PLATE_BODY, OVERLAY_SYSTEM_UI_BATTLE_PLATE_CAP_L,
        OVERLAY_SYSTEM_UI_BATTLE_PLATE_CAP_R, OVERLAY_SYSTEM_UI_BATTLE_PLATE_CLUT_ROW,
        OVERLAY_SYSTEM_UI_BATTLE_SEPARATOR, OVERLAY_SYSTEM_UI_BATTLE_SEPARATOR_CLUT_ROW,
    };

    let panel_rgba = legaia_tim::decode_rgba8(
        panel_parsed,
        OVERLAY_SYSTEM_UI_BATTLE_PANEL_CLUT_ROW as usize,
    )?;
    copy_rect(
        dst,
        ATLAS_WIDTH,
        &panel_rgba,
        src_w,
        OVERLAY_SYSTEM_UI_BATTLE_PANEL_BG,
        ATLAS_RECT_BATTLE_PANEL_BG,
    );

    let plate_rgba = legaia_tim::decode_rgba8(
        panel_parsed,
        OVERLAY_SYSTEM_UI_BATTLE_PLATE_CLUT_ROW as usize,
    )?;
    for (src, dst_rect) in [
        (
            OVERLAY_SYSTEM_UI_BATTLE_PLATE_CAP_L,
            ATLAS_RECT_BATTLE_PLATE_CAP_L,
        ),
        (
            OVERLAY_SYSTEM_UI_BATTLE_PLATE_BODY,
            ATLAS_RECT_BATTLE_PLATE_BODY,
        ),
        (
            OVERLAY_SYSTEM_UI_BATTLE_PLATE_CAP_R,
            ATLAS_RECT_BATTLE_PLATE_CAP_R,
        ),
    ] {
        copy_rect(dst, ATLAS_WIDTH, &plate_rgba, src_w, src, dst_rect);
    }

    let sep_rgba = legaia_tim::decode_rgba8(
        panel_parsed,
        OVERLAY_SYSTEM_UI_BATTLE_SEPARATOR_CLUT_ROW as usize,
    )?;
    copy_rect(
        dst,
        ATLAS_WIDTH,
        &sep_rgba,
        src_w,
        OVERLAY_SYSTEM_UI_BATTLE_SEPARATOR,
        ATLAS_RECT_BATTLE_SEPARATOR,
    );
    Ok(())
}

/// Stamp the battle HUD's ten 8x12 numeral cells into
/// [`ATLAS_RECT_HUD_DIGITS`].
///
/// Source is the **menu-glyph atlas** TIM, not the system-UI sheet -
/// retail's HUD numerals come off VRAM page `(960, 256)` through CLUT
/// row 510 sub-palette 13, which is
/// [`title_pak::MENU_GLYPH_ATLAS_TEXT_CLUT_ROW`]. The row origin
/// (`u = digit * 8`, `v = 208`) is read out of the HUD's own `SPRT`
/// packets; see [`legaia_ui_formats::menu_glyph_atlas`].
pub(super) fn add_hud_digit_strip(dst: &mut [u8], menu_glyph_tim: &[u8]) -> anyhow::Result<()> {
    use legaia_ui_formats::menu_glyph_atlas as mga;

    let tim = mga::extract_from_tim_slice(menu_glyph_tim)?;
    let parsed = legaia_tim::parse(tim.bytes)?;
    let src_w = parsed.pixel_width() as u32;
    let rgba =
        legaia_tim::decode_rgba8(&parsed, title_pak::MENU_GLYPH_ATLAS_TEXT_CLUT_ROW as usize)?;
    copy_rect(
        dst,
        ATLAS_WIDTH,
        &rgba,
        src_w,
        mga::DIGITS_ROW,
        ATLAS_RECT_HUD_DIGITS,
    );
    Ok(())
}

/// Stamp the **arts command-input** pieces into the atlas from three
/// further sub-palettes of the system-UI plane: the direction chips and
/// input bar (sub-palette 6), the chip label strip / diamond ends /
/// pennant caps (5), and the D-pad glyph (7).
///
/// Placement is `legaia_engine_ui::arts_input::ArtsInputAtlasRects::BAKED`:
/// natural sheet coordinates everywhere except the chip triple, whose rows
/// the character-portrait strip already occupies at `(200..248, 96..112)`
/// and which therefore re-seats `OVERLAY_SYSTEM_UI_ARTS_CHIP_ATLAS_DY`
/// rows down. The label strip is copied as one `24 x (v_high + h)` column
/// so every word (including the sheet-read Arms / Ra-Seru ones a later
/// pass may want) comes along.
///
/// REF: FUN_801D0748
pub(super) fn add_arts_input_sprites(
    dst: &mut [u8],
    panel_parsed: &legaia_tim::Tim,
    src_w: u32,
) -> anyhow::Result<()> {
    use title_pak::{arts_chip_atlas_rect as chip_at, *};

    // Sub-palette 6: chip body + caps, and the input-bar tiles.
    let chip_rgba =
        legaia_tim::decode_rgba8(panel_parsed, OVERLAY_SYSTEM_UI_ARTS_CHIP_CLUT_ROW as usize)?;
    for src in [
        OVERLAY_SYSTEM_UI_ARTS_CHIP_BODY,
        OVERLAY_SYSTEM_UI_ARTS_CHIP_CAP_L,
        OVERLAY_SYSTEM_UI_ARTS_CHIP_CAP_R,
    ] {
        copy_rect(dst, ATLAS_WIDTH, &chip_rgba, src_w, src, chip_at(src));
    }
    for src in [
        OVERLAY_SYSTEM_UI_ARTS_BAR_END_L,
        OVERLAY_SYSTEM_UI_ARTS_BAR_BODY,
        OVERLAY_SYSTEM_UI_ARTS_BAR_ARROW,
    ] {
        copy_rect(dst, ATLAS_WIDTH, &chip_rgba, src_w, src, src);
    }

    // Sub-palette 5: the label-strip column plus the three 9x18 ends.
    let label_rgba =
        legaia_tim::decode_rgba8(panel_parsed, OVERLAY_SYSTEM_UI_ARTS_LABEL_CLUT_ROW as usize)?;
    let strip = (
        OVERLAY_SYSTEM_UI_ARTS_LABEL_U,
        0,
        OVERLAY_SYSTEM_UI_ARTS_LABEL_W,
        OVERLAY_SYSTEM_UI_ARTS_LABEL_V_HIGH + OVERLAY_SYSTEM_UI_ARTS_LABEL_H,
    );
    copy_rect(dst, ATLAS_WIDTH, &label_rgba, src_w, strip, strip);
    for src in [
        OVERLAY_SYSTEM_UI_ARTS_DIAMOND_L,
        OVERLAY_SYSTEM_UI_ARTS_DIAMOND_R,
        OVERLAY_SYSTEM_UI_ARTS_PENNANT_CAP_R,
    ] {
        copy_rect(dst, ATLAS_WIDTH, &label_rgba, src_w, src, src);
    }

    // Sub-palette 7 (the cursor's plane): the D-pad glyph.
    let dpad_rgba =
        legaia_tim::decode_rgba8(panel_parsed, OVERLAY_SYSTEM_UI_ARTS_DPAD_CLUT_ROW as usize)?;
    copy_rect(
        dst,
        ATLAS_WIDTH,
        &dpad_rgba,
        src_w,
        OVERLAY_SYSTEM_UI_ARTS_DPAD,
        OVERLAY_SYSTEM_UI_ARTS_DPAD,
    );
    Ok(())
}

/// Decode the three ATR element icons out of the system-UI **extension
/// strip** TIM (`PROT.DAT[0x10178]`, 256x32 4bpp at VRAM `(896, 448)` =
/// sheet V rows 192..224) and stamp them into the atlas at
/// [`ATLAS_RECT_ATR_ICONS`]. The icons' ICO records select the VRAM
/// row-500 CLUT, which is the CLUT block of the sibling palette-variant
/// TIM at `PROT.DAT[0x10028]` - so the strip's pixels decode with that
/// TIM's palettes (per-character index
/// `title_pak::OVERLAY_SYSTEM_UI_ATR_PALETTES`).
///
/// `prot_dat_bytes` handling mirrors [`add_load_slot_grid_sprites`]:
/// full PROT.DAT or the system-UI-rooted slice both work; a slice too
/// short to reach the strip skips silently.
pub(super) fn add_atr_icon_sprites(dst: &mut [u8], prot_dat_bytes: &[u8]) -> anyhow::Result<()> {
    let abs = title_pak::OVERLAY_SYSTEM_UI_EXT_TIM_OFFSET;
    let abs_clut = title_pak::OVERLAY_SYSTEM_UI_EXT_CLUT_TIM_OFFSET;
    let size = title_pak::OVERLAY_SYSTEM_UI_EXT_TIM_SIZE;
    // Disambiguate full-PROT.DAT vs system-UI-rooted slice by looking for
    // the TIM magic at the candidate offset (a length check alone can't
    // tell them apart: the disc-mode slice is longer than `abs + size`).
    let has_tim_magic = |off: usize| {
        prot_dat_bytes.len() >= off + size && prot_dat_bytes[off..off + 4] == [0x10, 0, 0, 0]
    };
    let base = title_pak::OVERLAY_SYSTEM_UI_TIM_OFFSET;
    let rel = abs - base;
    let (ext_off, clut_off) = if has_tim_magic(rel) {
        (rel, abs_clut - base)
    } else if has_tim_magic(abs) {
        (abs, abs_clut)
    } else {
        return Ok(()); // slice too short; atlas-without-ATR is OK
    };
    let mut strip = legaia_tim::parse(&prot_dat_bytes[ext_off..ext_off + size])
        .map_err(|e| anyhow::anyhow!("ext-strip TIM parse failed: {e:?}"))?;
    let clut_tim = legaia_tim::parse(&prot_dat_bytes[clut_off..clut_off + size])
        .map_err(|e| anyhow::anyhow!("ext-strip CLUT TIM parse failed: {e:?}"))?;
    // Swap in the row-500 CLUT block: the strip's own CLUT uploads to
    // row 498; the ATR ICO records point one TIM over.
    strip.clut = clut_tim.clut;
    let strip_w = strip.pixel_width() as u32;
    for (i, (src, dst_rect)) in title_pak::OVERLAY_SYSTEM_UI_ATR_ICONS
        .iter()
        .zip(ATLAS_RECT_ATR_ICONS.iter())
        .enumerate()
    {
        let pal = title_pak::OVERLAY_SYSTEM_UI_ATR_PALETTES[i];
        let rgba = legaia_tim::decode_rgba8(&strip, pal)
            .map_err(|e| anyhow::anyhow!("ATR icon {i} decode failed: {e:?}"))?;
        copy_rect(dst, ATLAS_WIDTH, &rgba, strip_w, *src, *dst_rect);
    }
    Ok(())
}

/// Decode the 3 portrait TIMs + the 32x32 empty-cell frame from
/// `PROT.DAT` (if the caller provided them) and stamp them into the
/// atlas at the documented `ATLAS_RECT_*` positions.
///
/// `prot_dat_bytes` may be:
///   * the full `PROT.DAT` buffer - portraits are loaded from
///     absolute offsets;
///   * the slice that already starts at the system-UI TIM header
///     (offset `OVERLAY_SYSTEM_UI_TIM_OFFSET = 0x018E0`) - portraits
///     are loaded from slice-relative offsets if the slice extends
///     far enough;
///   * any shorter slice - portrait / frame loading is skipped
///     silently (atlas just won't have those rects populated).
///
/// Each portrait TIM ships with its own CLUT (single row, 16 entries);
/// CLUT row 0 is the only meaningful row.
/// Bake the save UI's block badge (`No.` + the fifteen block numerals,
/// sub-palette 8) and the write panel's progress tube (sub-palette 0) out of
/// PROT 0899's save-menu TIM, plus the grey texel the progress fill
/// modulates. Placements are `title_pak::SAVE_MENU_ATLAS_*`.
pub(super) fn add_card_work_sprites(
    dst: &mut [u8],
    tim: &legaia_tim::Tim,
    src_w: u32,
) -> anyhow::Result<()> {
    let digit_rgba = legaia_tim::decode_rgba8(tim, title_pak::OVERLAY_SAVE_UI_REC_DIGIT_CLUT)?;
    let tube_rgba = legaia_tim::decode_rgba8(tim, title_pak::OVERLAY_SAVE_UI_REC_TUBE_CLUT)?;
    copy_rect(
        dst,
        ATLAS_WIDTH,
        &digit_rgba,
        src_w,
        title_pak::OVERLAY_SAVE_UI_REC_NO_LABEL,
        title_pak::SAVE_MENU_ATLAS_NO_LABEL,
    );
    let (u0, v0, w, h) = title_pak::OVERLAY_SAVE_UI_REC_BLOCK_DIGIT;
    for block in 0..title_pak::OVERLAY_SAVE_UI_BLOCK_DIGIT_COUNT {
        if let Some(cell) = title_pak::save_menu_atlas_block_digit(block) {
            copy_rect(
                dst,
                ATLAS_WIDTH,
                &digit_rgba,
                src_w,
                (u0 + block * w, v0, w, h),
                cell,
            );
        }
    }
    copy_rect(
        dst,
        ATLAS_WIDTH,
        &tube_rgba,
        src_w,
        title_pak::OVERLAY_SAVE_UI_REC_PROGRESS_TUBE,
        title_pak::SAVE_MENU_ATLAS_PROGRESS_TUBE,
    );
    let grey = title_pak::OVERLAY_SAVE_UI_PROGRESS_TEXEL_GREY;
    let stride = (ATLAS_WIDTH * 4) as usize;
    for ((x0, y0, tw, th), v) in [
        (title_pak::SAVE_MENU_ATLAS_PROGRESS_TEXEL, grey),
        (title_pak::SAVE_MENU_ATLAS_WHITE_TEXEL, 255),
    ] {
        for y in y0..y0 + th {
            for x in x0..x0 + tw {
                let off = y as usize * stride + x as usize * 4;
                dst[off..off + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
    }
    Ok(())
}

pub(super) fn add_load_slot_grid_sprites(
    dst: &mut [u8],
    prot_dat_bytes: &[u8],
) -> anyhow::Result<()> {
    // Pick the right base offset depending on whether we got the full
    // PROT.DAT (offset 0 = file start) or the system-UI-rooted slice
    // (offset 0 = system-UI TIM header).
    let portrait_base = if prot_dat_bytes.len()
        >= title_pak::OVERLAY_LOAD_PORTRAIT_TIM_OFFSET + title_pak::OVERLAY_LOAD_PORTRAIT_STRIDE
    {
        // Looks like a full PROT.DAT.
        title_pak::OVERLAY_LOAD_PORTRAIT_TIM_OFFSET
    } else {
        // System-UI-rooted slice - portraits live at
        // `(portrait_off - system_ui_off)` into the slice. If the
        // slice doesn't extend that far, skip portrait loading.
        let system_ui_off = title_pak::OVERLAY_SYSTEM_UI_TIM_OFFSET;
        let portrait_off = title_pak::OVERLAY_LOAD_PORTRAIT_TIM_OFFSET;
        match portrait_off.checked_sub(system_ui_off) {
            Some(p) if p + title_pak::OVERLAY_LOAD_PORTRAIT_STRIDE <= prot_dat_bytes.len() => p,
            _ => return Ok(()), // slice too short, atlas-without-portraits is OK
        }
    };
    let frame_base = portrait_base + title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_OFFSET
        - title_pak::OVERLAY_LOAD_PORTRAIT_TIM_OFFSET;

    // Each portrait: a 16x16 4bpp TIM at portrait_base + idx*stride.
    for idx in 0..title_pak::OVERLAY_LOAD_PORTRAIT_COUNT {
        let off = portrait_base + idx * title_pak::OVERLAY_LOAD_PORTRAIT_STRIDE;
        if off + title_pak::OVERLAY_LOAD_PORTRAIT_STRIDE > prot_dat_bytes.len() {
            // Slice exhausted mid-atlas - stop after the portraits we
            // could load.
            break;
        }
        let tim_bytes = &prot_dat_bytes[off..off + title_pak::OVERLAY_LOAD_PORTRAIT_STRIDE];
        let parsed = legaia_tim::parse(tim_bytes)
            .map_err(|e| anyhow::anyhow!("portrait {idx} parse failed: {e:?}"))?;
        let rgba = legaia_tim::decode_rgba8(&parsed, 0)
            .map_err(|e| anyhow::anyhow!("portrait {idx} decode failed: {e:?}"))?;
        let src_w = parsed.pixel_width() as u32;
        let src_h = parsed.image.h as u32;
        let dst_rect = (
            ATLAS_RECT_PORTRAIT_BASE_X + (idx as u32) * ATLAS_RECT_PORTRAIT_W,
            ATLAS_RECT_PORTRAIT_BASE_Y,
            ATLAS_RECT_PORTRAIT_W,
            ATLAS_RECT_PORTRAIT_H,
        );
        copy_rect(
            dst,
            ATLAS_WIDTH,
            &rgba,
            src_w,
            (0, 0, src_w, src_h),
            dst_rect,
        );
    }

    // Empty-cell frame: a 32x32 4bpp TIM. Skip silently if the slice
    // doesn't reach it.
    if frame_base + title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_SIZE > prot_dat_bytes.len() {
        return Ok(());
    }
    let frame_bytes =
        &prot_dat_bytes[frame_base..frame_base + title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_SIZE];
    let parsed = legaia_tim::parse(frame_bytes)
        .map_err(|e| anyhow::anyhow!("empty-frame parse failed: {e:?}"))?;
    let rgba = legaia_tim::decode_rgba8(&parsed, 0)
        .map_err(|e| anyhow::anyhow!("empty-frame decode failed: {e:?}"))?;
    let src_w = parsed.pixel_width() as u32;
    let src_h = parsed.image.h as u32;
    copy_rect(
        dst,
        ATLAS_WIDTH,
        &rgba,
        src_w,
        (0, 0, src_w, src_h),
        ATLAS_RECT_EMPTY_FRAME,
    );

    Ok(())
}

/// Copy a `(x, y, w, h)` rect from `src` (sized `src_w x src_h`,
/// implicit from the slice length) into `dst` (sized `dst_w x ?`).
/// `src_rect` and `dst_rect` may use different `(x, y)` origins - the
/// `(w, h)` values must match.
pub(super) fn copy_rect(
    dst: &mut [u8],
    dst_w: u32,
    src: &[u8],
    src_w: u32,
    src_rect: (u32, u32, u32, u32),
    dst_rect: (u32, u32, u32, u32),
) {
    debug_assert_eq!((src_rect.2, src_rect.3), (dst_rect.2, dst_rect.3));
    let (sx, sy, w, h) = src_rect;
    let (dx, dy, _, _) = dst_rect;
    let dst_stride = (dst_w * 4) as usize;
    let src_stride = (src_w * 4) as usize;
    for row in 0..h {
        let src_off = (sy + row) as usize * src_stride + sx as usize * 4;
        let dst_off = (dy + row) as usize * dst_stride + dx as usize * 4;
        let len = w as usize * 4;
        dst[dst_off..dst_off + len].copy_from_slice(&src[src_off..src_off + len]);
    }
}

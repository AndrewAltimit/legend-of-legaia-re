//! Save-menu sprite atlas - byte-perfect retail save/load-screen UI.
//!
//! Composes a single 256x256 RGBA atlas containing:
//!
//! - **9-slice panel tiles** decoded from the system-UI TIM at
//!   `PROT.DAT[0x018E0]` with CLUT row 2 (byte-pinned via
//!   `scripts/pcsx-redux/scan_panel_prims.py` against the PCSX-Redux
//!   sstate9 RAM dump; see [[project-load-screen-panel-source-pinned]]).
//!   The tiles sit at their natural source coordinates `(160..192, 0..32)`,
//!   so engines can sample them with the `OVERLAY_SYSTEM_UI_PANEL_*`
//!   constants exported by `legaia_asset::title_pak`.
//! - **SLOT 1 / SLOT 2 pills** decoded from PROT 0899's save-menu TIM
//!   with CLUT 7. The atlas keeps these at their existing source
//!   coordinates `(33, 97/113, 45, 15)` for backward compatibility.
//!
//! Retail draws the panel from **14 GP0_TEXTURED_SPRITE primitives**:
//! 4 corners (4×4 each), top + bottom edges (24×4 tiles repeated 3×
//! with a 1×4 remainder), and left + right edges (4×21). **No
//! interior fill is drawn** - the "marbled blue" look in retail is
//! the dimmed title art bleeding through the empty middle of the
//! 9-slice frame. Engines that need an opaque interior must draw it
//! themselves.

use legaia_asset::title_pak;

/// Atlas dimensions in source pixels. Matches the legacy PROT-0899
/// save-menu atlas dimensions so existing engine-render plumbing
/// keeps working; the panel tiles slot in at coordinates
/// `(160..192, 0..32)` which are free in the source PROT 0899 atlas
/// (those columns hold tiny memory-card icons not used at SaveSelect).
pub const ATLAS_WIDTH: u32 = 256;
pub const ATLAS_HEIGHT: u32 = 256;

/// CLUT row used to render the slot pills: bright blue body with
/// white text.
const PILL_CLUT: usize = 7;

/// Atlas placement of the **load-screen empty-cell frame** (32x32
/// sprite, hollow blue border). Chosen to sit inside unused atlas
/// columns at the bottom-right corner.
pub const ATLAS_RECT_EMPTY_FRAME: (u32, u32, u32, u32) = (200, 64, 32, 32);

/// Atlas placement of the status-panel **stat labels** (LV / HP / MP),
/// copied from CLUT row 2 of the system-UI TIM. Packed in a free strip
/// below the filigree tile. Each is 16x12.
pub const ATLAS_RECT_LABEL_LV: (u32, u32, u32, u32) = (40, 232, 16, 12);
pub const ATLAS_RECT_LABEL_HP: (u32, u32, u32, u32) = (60, 232, 16, 12);
pub const ATLAS_RECT_LABEL_MP: (u32, u32, u32, u32) = (80, 232, 16, 12);

/// Atlas placement of the **raw** (un-gradient-baked) marbled-blue
/// filigree interior tile (32x29). Copied verbatim from CLUT row 2 of
/// the system-UI TIM so the field-menu chrome can tile it in 2D as the
/// window interior (the pause menu fills every window with this navy
/// damask, darkened by a per-draw colour, rather than the save screen's
/// gouraud-gradient variant at [`ATLAS_RECT_FILIGREE`]'s baked sibling).
/// Sits in a free atlas region below the portraits.
pub const ATLAS_RECT_FILIGREE: (u32, u32, u32, u32) = (0, 200, 32, 29);

/// Atlas placement of the 3 character portrait TIMs (16x16 each).
/// Stacked horizontally just below the empty-frame rect; each portrait
/// occupies a 16x16 sub-region.
pub const ATLAS_RECT_PORTRAIT_W: u32 = 16;
pub const ATLAS_RECT_PORTRAIT_H: u32 = 16;
pub const ATLAS_RECT_PORTRAIT_BASE_X: u32 = 200;
pub const ATLAS_RECT_PORTRAIT_BASE_Y: u32 = 96;

/// CLUT row of the system-UI TIM that decodes the panel chrome.
/// Mirror of [`title_pak::OVERLAY_SYSTEM_UI_PANEL_CLUT_ROW`].
const PANEL_CLUT_ROW: usize = title_pak::OVERLAY_SYSTEM_UI_PANEL_CLUT_ROW as usize;

/// CLUT row of the system-UI TIM that decodes the pointing-finger
/// cursor. Mirror of [`title_pak::OVERLAY_SYSTEM_UI_CURSOR_CLUT_ROW`].
const CURSOR_CLUT_ROW: usize = title_pak::OVERLAY_SYSTEM_UI_CURSOR_CLUT_ROW as usize;

/// Pre-decoded save-menu atlas - RGBA8 pixels + the source rects
/// engines sample to compose the retail save/load screen.
///
/// Build once at boot from PROT.DAT + PROT 0899 bytes via
/// [`build_atlas`], hand to engine-render's `upload_sprite_atlas`,
/// then emit one sprite quad per 9-slice tile + one per slot pill
/// each frame the save-select UI is active.
#[derive(Debug, Clone)]
pub struct SaveMenuAtlas {
    /// RGBA8 pixel data, exactly `4 * width * height` bytes.
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl SaveMenuAtlas {
    /// Panel top-left corner tile (4x4).
    pub fn band_panel_tl(&self) -> (u32, u32, u32, u32) {
        title_pak::OVERLAY_SYSTEM_UI_PANEL_TL
    }
    /// Panel top-right corner tile (4x4).
    pub fn band_panel_tr(&self) -> (u32, u32, u32, u32) {
        title_pak::OVERLAY_SYSTEM_UI_PANEL_TR
    }
    /// Panel bottom-left corner tile (4x4).
    pub fn band_panel_bl(&self) -> (u32, u32, u32, u32) {
        title_pak::OVERLAY_SYSTEM_UI_PANEL_BL
    }
    /// Panel bottom-right corner tile (4x4).
    pub fn band_panel_br(&self) -> (u32, u32, u32, u32) {
        title_pak::OVERLAY_SYSTEM_UI_PANEL_BR
    }
    /// Panel top edge tile (24x4) - repeated horizontally between
    /// the top corners.
    pub fn band_panel_top(&self) -> (u32, u32, u32, u32) {
        title_pak::OVERLAY_SYSTEM_UI_PANEL_TOP
    }
    /// Panel bottom edge tile (24x4).
    pub fn band_panel_bot(&self) -> (u32, u32, u32, u32) {
        title_pak::OVERLAY_SYSTEM_UI_PANEL_BOT
    }
    /// Panel left edge tile (4x21).
    pub fn band_panel_left(&self) -> (u32, u32, u32, u32) {
        title_pak::OVERLAY_SYSTEM_UI_PANEL_LEFT
    }
    /// Panel right edge tile (4x21).
    pub fn band_panel_right(&self) -> (u32, u32, u32, u32) {
        title_pak::OVERLAY_SYSTEM_UI_PANEL_RIGHT
    }
    /// SLOT 1 pill rect (baked "SLOT 1" label).
    pub fn band_slot1(&self) -> (u32, u32, u32, u32) {
        title_pak::OVERLAY_SAVE_MENU_BAND_SLOT1
    }
    /// SLOT 2 pill rect (baked "SLOT 2" label).
    pub fn band_slot2(&self) -> (u32, u32, u32, u32) {
        title_pak::OVERLAY_SAVE_MENU_BAND_SLOT2
    }
    /// Pointing-finger cursor sprite (16x16, white ink + grey shadow).
    /// Lives in the same system-UI TIM as the panel chrome but uses
    /// CLUT row 7 instead of row 2.
    pub fn band_cursor(&self) -> (u32, u32, u32, u32) {
        title_pak::OVERLAY_SYSTEM_UI_CURSOR
    }
    /// Panel interior fill tile (32x29, gradient-baked). Used by the
    /// save/load screen, whose interior retail draws with a gouraud
    /// gradient.
    pub fn band_panel_interior(&self) -> (u32, u32, u32, u32) {
        title_pak::OVERLAY_SYSTEM_UI_PANEL_INTERIOR
    }
    /// Raw marbled-blue filigree interior tile (32x29), un-baked. The
    /// pause-menu windows tile this in 2D as their navy damask interior.
    pub fn band_panel_filigree(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_FILIGREE
    }
    /// Status-panel "LV" label sprite (16x12).
    pub fn band_label_lv(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_LABEL_LV
    }
    /// Status-panel "HP" label sprite (16x12).
    pub fn band_label_hp(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_LABEL_HP
    }
    /// Status-panel "MP" label sprite (16x12).
    pub fn band_label_mp(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_LABEL_MP
    }
    /// Empty-cell frame sprite for the load-screen slot grid (32x32,
    /// 20x20 hollow blue border centred in the sprite - outer 6 px
    /// margin is transparent).
    pub fn band_load_empty_frame(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_EMPTY_FRAME
    }
    /// Character portrait sub-rect for the load-screen slot grid
    /// (16x16). `char_id`: 0=Vahn, 1=Noa, 2=Gala. Returns `None` for
    /// characters whose portrait isn't in the on-disc atlas (party
    /// members past index 2 don't have load-screen icons; retail
    /// renders them as plain empty-frame slots).
    pub fn band_load_portrait(&self, char_id: u8) -> Option<(u32, u32, u32, u32)> {
        if (char_id as usize) < title_pak::OVERLAY_LOAD_PORTRAIT_COUNT {
            Some((
                ATLAS_RECT_PORTRAIT_BASE_X + (char_id as u32) * ATLAS_RECT_PORTRAIT_W,
                ATLAS_RECT_PORTRAIT_BASE_Y,
                ATLAS_RECT_PORTRAIT_W,
                ATLAS_RECT_PORTRAIT_H,
            ))
        } else {
            None
        }
    }
}

/// Build a [`SaveMenuAtlas`] from raw `PROT.DAT` bytes (carries the
/// system-UI TIM at offset `0x018E0`) plus the trailing-overlay
/// bytes of PROT 0899 (carries the save-menu TIM with the slot pills).
///
/// The panel tiles are decoded from the system-UI TIM with CLUT row 2
/// - byte-equal to the retail VRAM contents at parked-on-load-screen
///   sstate9. The slot pills are decoded from PROT 0899 with CLUT 7 -
///   byte-equal as well.
pub fn build_atlas(prot_dat_bytes: &[u8], prot_0899_bytes: &[u8]) -> anyhow::Result<SaveMenuAtlas> {
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
        title_pak::OVERLAY_SYSTEM_UI_PANEL_INTERIOR,
        ATLAS_RECT_FILIGREE,
    );

    // Status-panel stat labels (LV / HP / MP) from the same CLUT-row-2 sheet
    // - the status page draws these as sprites in place of ASCII glyphs.
    for (src, dst) in [
        (title_pak::OVERLAY_SYSTEM_UI_LABEL_LV, ATLAS_RECT_LABEL_LV),
        (title_pak::OVERLAY_SYSTEM_UI_LABEL_HP, ATLAS_RECT_LABEL_HP),
        (title_pak::OVERLAY_SYSTEM_UI_LABEL_MP, ATLAS_RECT_LABEL_MP),
    ] {
        copy_rect(&mut out, ATLAS_WIDTH, &panel_rgba, panel_src_w, src, dst);
    }

    // Load-screen slot-grid: empty-cell frame + 3 character portrait
    // TIMs. These live just past the system-UI sheet in the unindexed
    // pre-`init_data` gap of PROT.DAT, so the caller needs to pass the
    // full PROT.DAT buffer too (the system-UI-only slice can't reach
    // them).
    add_load_slot_grid_sprites(&mut out, prot_dat_bytes)?;

    Ok(SaveMenuAtlas {
        rgba: out,
        width: ATLAS_WIDTH,
        height: ATLAS_HEIGHT,
    })
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
fn add_load_slot_grid_sprites(dst: &mut [u8], prot_dat_bytes: &[u8]) -> anyhow::Result<()> {
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

/// Pre-bake the gouraud gray gradient retail applies to the panel
/// interior into the atlas. Reads `rect` from `src_rgba` (CLUT-row-2
/// pixels of the marbled-blue source region), multiplies each pixel
/// by a per-row linear gradient between `top_rgb` and `bot_rgb`
/// scaled to `[0, 1]`, and writes the result into `dst` at the same
/// rect coords.
///
/// PSX hardware does this as a per-vertex color modulation in the
/// 0x3C textured-quad primitive (top vertices have rgb 64,64,64;
/// bottom vertices have rgb 136,136,136), so the GPU interpolates
/// linearly across the quad. We bake the same linear interpolation
/// into the atlas so the engine can draw the result as a regular
/// SpriteDraw without needing per-vertex colors.
fn bake_panel_interior_gradient(
    dst: &mut [u8],
    src_rgba: &[u8],
    src_w: u32,
    rect: (u32, u32, u32, u32),
    top_rgb: (u8, u8, u8),
    bot_rgb: (u8, u8, u8),
) {
    let (x0, y0, w, h) = rect;
    let dst_stride = (ATLAS_WIDTH * 4) as usize;
    let src_stride = (src_w * 4) as usize;
    // Per-row gradient factor in 0..255 scaled (256 lerp).
    let lerp_chan = |a: u8, b: u8, t_num: u32, t_den: u32| -> u8 {
        // Avoid div-by-zero for single-row interiors.
        if t_den == 0 {
            return a;
        }
        let aa = a as u32;
        let bb = b as u32;
        ((aa * (t_den - t_num) + bb * t_num) / t_den) as u8
    };
    for row in 0..h {
        let t_num = row;
        let t_den = h.saturating_sub(1).max(1);
        let mod_r = lerp_chan(top_rgb.0, bot_rgb.0, t_num, t_den);
        let mod_g = lerp_chan(top_rgb.1, bot_rgb.1, t_num, t_den);
        let mod_b = lerp_chan(top_rgb.2, bot_rgb.2, t_num, t_den);
        let src_off = ((y0 + row) as usize) * src_stride + (x0 as usize) * 4;
        let dst_off = ((y0 + row) as usize) * dst_stride + (x0 as usize) * 4;
        for col in 0..w {
            let o = col as usize * 4;
            // PSX color modulation is `(tex * color) / 128` (i.e.
            // 0x80 = identity, 0xFF = ~2x). Mirror that semantic.
            let modulate = |tex: u8, color: u8| -> u8 {
                let prod = (tex as u32 * color as u32) / 128;
                prod.min(255) as u8
            };
            dst[dst_off + o] = modulate(src_rgba[src_off + o], mod_r);
            dst[dst_off + o + 1] = modulate(src_rgba[src_off + o + 1], mod_g);
            dst[dst_off + o + 2] = modulate(src_rgba[src_off + o + 2], mod_b);
            dst[dst_off + o + 3] = src_rgba[src_off + o + 3];
        }
    }
}

/// Copy a `(x, y, w, h)` rect from `src` (sized `src_w x src_h`,
/// implicit from the slice length) into `dst` (sized `dst_w x ?`).
/// `src_rect` and `dst_rect` may use different `(x, y)` origins - the
/// `(w, h)` values must match.
fn copy_rect(
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Disc-gated: build the real save-menu atlas from PROT.DAT +
    /// PROT 0899 and verify the panel-tile + pill regions contain
    /// opaque pixels with the right tonal range.
    #[test]
    fn builds_real_save_menu_atlas_when_disc_extracted() {
        let prot_dat_path = "../../extracted/PROT.DAT";
        let prot_899_path = "../../extracted/PROT/0899_xxx_dat.BIN";
        let prot_dat = match std::fs::read(prot_dat_path) {
            Ok(b) => b,
            Err(_) => {
                eprintln!("skip: {prot_dat_path} missing");
                return;
            }
        };
        let prot_899 = match std::fs::read(prot_899_path) {
            Ok(b) => b,
            Err(_) => {
                eprintln!("skip: {prot_899_path} missing");
                return;
            }
        };
        // build_atlas now expects a slice that already starts at the
        // system-UI TIM header (the disc-mode caller pulls just that
        // region via `prot_dat_raw_bytes(OVERLAY_SYSTEM_UI_TIM_OFFSET, …)`).
        let tim_off = legaia_asset::title_pak::OVERLAY_SYSTEM_UI_TIM_OFFSET;
        let tim_size = legaia_asset::title_pak::OVERLAY_SYSTEM_UI_TIM_SIZE;
        let system_ui_slice = &prot_dat[tim_off..tim_off + tim_size];
        let atlas = build_atlas(system_ui_slice, &prot_899).expect("build save-menu atlas");
        assert_eq!(atlas.width, ATLAS_WIDTH);
        assert_eq!(atlas.height, ATLAS_HEIGHT);
        assert_eq!(atlas.rgba.len(), (ATLAS_WIDTH * ATLAS_HEIGHT * 4) as usize);

        // The top-left corner tile must contain opaque gold-bronze
        // pixels (CLUT row 2 entries 7..15).
        let (tlx, tly, tlw, tlh) = atlas.band_panel_tl();
        let stride = (ATLAS_WIDTH * 4) as usize;
        let mut gold_hits = 0u32;
        for row in 0..tlh {
            for col in 0..tlw {
                let off = ((tly + row) as usize) * stride + ((tlx + col) as usize) * 4;
                let (r, g, b, a) = (
                    atlas.rgba[off],
                    atlas.rgba[off + 1],
                    atlas.rgba[off + 2],
                    atlas.rgba[off + 3],
                );
                // Gold-bronze tones have r > g > b with r in 60..210.
                if a == 255 && r >= 60 && r > g && g > b {
                    gold_hits += 1;
                }
            }
        }
        assert!(
            gold_hits >= 8,
            "panel top-left tile has too few gold-bronze pixels ({gold_hits})"
        );

        // Slot-1 pill band should have a saturated blue tone (CLUT 7).
        let (sx, sy, sw, sh) = atlas.band_slot1();
        let mut blue_hits = 0u32;
        for row in 0..sh {
            for col in 0..sw {
                let off = ((sy + row) as usize) * stride + ((sx + col) as usize) * 4;
                let (r, g, b, a) = (
                    atlas.rgba[off],
                    atlas.rgba[off + 1],
                    atlas.rgba[off + 2],
                    atlas.rgba[off + 3],
                );
                if a == 255 && b > 100 && r < 120 && g < 120 {
                    blue_hits += 1;
                }
            }
        }
        assert!(
            blue_hits > 30,
            "slot 1 pill has too few blue pixels ({blue_hits}) - CLUT may be off"
        );
    }
}

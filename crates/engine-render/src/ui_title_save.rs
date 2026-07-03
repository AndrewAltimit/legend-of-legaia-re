//! Title-screen + save/load-screen UI draw builders: title menu,
//! 9-slice window chrome, save-slot grid + info panel, and the
//! "Now checking" dialog. Extracted from the crate root.

use crate::*;

/// Build [`TextDraw`]s for the title screen.
///
/// Phase argument controls which UI is rendered:
/// - `phase` = 0: fade-in (no text - engines fade the screen to black);
/// - `phase` = 1: "Press START" prompt (centered roughly mid-screen);
/// - `phase` = 2: main menu (New Game / Continue / Options stacked).
///
/// `cursor` is ignored for phases 0/1 and selects the highlighted row
/// (0..=2) in phase 2. `continue_enabled = false` dims the Continue row.
/// `blink_on` toggles the prompt visibility on phase 1 every blink_period
/// frames; engines drive this from the title session's blink phase.
///
/// When the engine has uploaded the PROT 0888 title TIM atlas, pass
/// `atlas_present = true` to suppress the font-rendered "PRESS START"
/// prompt (phase 1) - the TIM's own "PRESS START BUTTON" band is drawn
/// in its place by the sprite layer. The menu rows (phase 2) are
/// still rendered via font because retail uses larger font glyphs
/// there too, not the tiny "NEW GAME CONTINUE" band at the bottom of
/// the TIM.
///
/// A natural anchor for a 320×240 surface is `pen = (96, 100)` - the
/// renderer offsets each line from this top-left.
pub fn title_draws_for(
    font: &legaia_font::Font,
    phase: u8,
    cursor: u8,
    continue_enabled: bool,
    blink_on: bool,
    atlas_present: bool,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 16;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let dim: [f32; 4] = [0.45, 0.45, 0.45, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];

    let mut out = Vec::new();

    match phase {
        0 => {}
        1 if blink_on && !atlas_present => {
            let l = font.layout_ascii("PRESS START");
            out.extend(text_draws_for(&l, pen, white));
        }
        1 => {}
        2 => {
            // Retail title menu carries only two rows; Options lives in
            // the in-game field menu. Color is the selection indicator
            // (selected = white, unselected = dim) - no arrow / cursor
            // mark in retail. The disabled-Continue row reads the same
            // as a non-highlighted row.
            let _ = (gold, continue_enabled);
            let rows = ["NEW GAME", "CONTINUE"];
            for (i, label) in rows.iter().enumerate() {
                let row_y = pen.1 + i as i32 * LINE_H;
                let selected = i as u8 == cursor;
                let color = if selected { white } else { dim };
                let l = font.layout_ascii(label);
                out.extend(text_draws_for(&l, (pen.0, row_y), color));
            }
        }
        _ => {}
    }
    out
}

/// Build [`SpriteDraw`]s for the title-screen main-menu rows ("NEW GAME"
/// / "CONTINUE") sampling the dedicated menu-glyph atlas from
/// `PROT.DAT` (see [`legaia_asset::menu_glyph_atlas`]).
///
/// Retail-faithful equivalent of phase 2 in [`title_draws_for`] - same
/// row labels and cursor / dim semantics, but each row is a horizontal
/// strip of sprite cells sampled from the menu-glyph atlas instead of
/// dialog-font glyphs. Selected row gets a gold tint; the Continue
/// row is dimmed when `continue_enabled = false`. Retail's title menu
/// only carries two rows (NEW GAME / CONTINUE); Options is reached via
/// the in-game field menu, not from the title.
///
/// `cell_scale` is an integer multiplier applied to source-pixel sizes
/// so engines can match the title-art's `play-window` stage scale
/// (mirrors the per-band SpriteDraw scaling). `pen` is the top-left
/// corner of the first row's first glyph in surface pixels.
///
/// Note: the menu-glyph atlas carries only uppercase letters and
/// digits - no cursor marks.
///
/// Returns an empty vec for any phase other than 2.
pub fn title_menu_draws_for(
    phase: u8,
    cursor: u8,
    continue_enabled: bool,
    pen: (i32, i32),
    cell_scale: u32,
) -> Vec<SpriteDraw> {
    if phase != 2 {
        return Vec::new();
    }
    // Retail uses color as the SELECTION INDICATOR: the highlighted row
    // is bright white and unselected rows are dim gray. There is no
    // arrow / cursor mark - the brightness IS the cursor. Disabled
    // (Continue with no save) reads the same as a non-highlighted row.
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let dim: [f32; 4] = [0.55, 0.55, 0.55, 1.0];

    use legaia_asset::menu_glyph_atlas as mga;
    let cell_w = mga::GLYPH_W as i32;
    let cell_h = mga::ALPHABET_GLYPH_H as i32;
    let scale = cell_scale.max(1) as i32;
    // One blank row of padding between rows so the small-caps glyphs
    // sit clearly apart (matches the retail menu vertical pitch).
    let line_h = cell_h + 2;

    let rows = ["NEW GAME", "CONTINUE"];
    let mut out = Vec::new();
    for (i, label) in rows.iter().enumerate() {
        let row_y = pen.1 + i as i32 * line_h * scale;
        let selected = i as u8 == cursor;
        let row_disabled = i == 1 && !continue_enabled;
        let _ = row_disabled; // disabled rows render the same as unselected
        let color = if selected { white } else { dim };
        let mut x = pen.0;
        for c in label.chars() {
            if let Some((sx, sy, sw, sh)) = mga::glyph_rect(c) {
                out.push(SpriteDraw {
                    dst: (x, row_y, sw * scale as u32, sh * scale as u32),
                    src: (sx, sy, sw, sh),
                    color,
                });
            }
            x += cell_w * scale;
        }
    }
    out
}

/// One slot row passed into [`save_select_draws_for`]. Plain-data view
/// so the renderer doesn't depend on `engine-core::save_select`.
pub struct SaveSelectRow<'a> {
    pub label: &'a str,
    pub present: bool,
    pub party_lv: u8,
    pub play_time_seconds: u32,
    pub money: u32,
    pub location: &'a str,
}

/// **Canonical PSX framebuffer stage for the boot UI**. All retail-
/// pinned positions (panel, pills, cursor, title art) are expressed
/// in 320×240 framebuffer coords; the boot-UI stage maps 1:1 to
/// this so everything stays in lockstep at any window resolution.
///
/// Engines compute `stage_scale = min(surface_w / 320, surface_h /
/// 240).clamp(1, 4)`, center the resulting `320*scale × 240*scale`
/// rectangle inside the surface, and use that as the `stage_origin`
/// for every boot-UI sprite emission.
pub const BOOT_UI_STAGE_W: u32 = 320;
/// Companion to [`BOOT_UI_STAGE_W`].
pub const BOOT_UI_STAGE_H: u32 = 240;

/// Retail PSX framebuffer position of the load-screen panel top-left.
/// Mirror of [`legaia_asset::title_pak::OVERLAY_SAVE_PANEL_RETAIL_DST`].
pub const SAVE_SELECT_PANEL_POS: (i32, i32) = (6, 4);
/// Total size of the load-screen panel in source pixels.
pub const SAVE_SELECT_PANEL_SIZE: (i32, i32) = (81, 29);
/// Retail PSX framebuffer position of the SLOT 1 pill **sprite top
/// edge**. Pinned via direct framebuffer-pixel inspection at sstate9
/// - the rounded pill outline starts at `fb_y=99` (transition pixels),
///   the saturated-blue body at `y=101`, sprite bottom at `y=112`. The
///   earlier `y=102` pin tracked the saturated-blue body, not the
///   sprite-top edge - drawing at that offset made the cursor finger
///   look too high relative to the pill chrome.
pub const SAVE_SELECT_SLOT1_POS: (i32, i32) = (137, 99);
/// Retail pin of the SLOT 1 pill sprite top-left **after the user
/// has committed to loading a slot** - once the load flow enters
/// `NowChecking` / `SlotPreview`, retail relocates the active pill
/// up under the Load panel. Pinned via the slide-in primitive
/// `FUN_801E1C1C` mode 2 in `overlay_save_ui_select_801dd35c.txt`:
/// the dispatcher calls `FUN_801e1c1c(2, DAT_801ef194, 0xa0, 0x60,
/// 0x30, 0x28)` - slide from `(160, 96)` to **target `(48, 40)`**.
/// Mode 2's GPU emit pre-shifts `sVar6 = param_3 - 0x18` so the
/// composite's top-left lands at `(24, 40)`. The earlier
/// screenshot-derived `(22, 41)` was ~2px off due to anti-aliased
/// sprite-edge sampling. Pairs with [`SAVE_SELECT_CURSOR_POS_LOAD_ACTIVE`].
pub const SAVE_SELECT_SLOT1_POS_LOAD_ACTIVE: (i32, i32) = (24, 40);
/// Vertical pitch between consecutive slot pill sprite tops in
/// framebuffer pixels. SLOT 1 sprite top at y=99, SLOT 2 at y=115.
pub const SAVE_SELECT_SLOT_PITCH_Y: i32 = 16;
/// Retail PSX framebuffer position of the pointing-finger cursor when
/// pointing at SLOT 1. Mirror of
/// [`legaia_asset::title_pak::OVERLAY_SAVE_CURSOR_RETAIL_DST`]. SLOT 2
/// shifts the cursor down by [`SAVE_SELECT_SLOT_PITCH_Y`].
pub const SAVE_SELECT_CURSOR_POS: (i32, i32) = (114, 100);
/// Pin of the pointing-finger cursor sprite top-left during the
/// Load-active states (NowChecking / SlotPreview), matching the
/// SLOT 1 pill at [`SAVE_SELECT_SLOT1_POS_LOAD_ACTIVE`]. Retail
/// hides the pill cursor while the dialog is up and the grid
/// emits its own cursor, so this constant is currently unused in
/// emission - kept here for parity with the Browsing pin in case
/// a future variant needs it.
pub const SAVE_SELECT_CURSOR_POS_LOAD_ACTIVE: (i32, i32) = (10, 41);
/// **DEPRECATED** - superseded by [`SAVE_SELECT_CURSOR_POS`]. Old
/// callers used this with `SAVE_SELECT_SLOT1_POS` to derive cursor
/// placement; new code should use [`SAVE_SELECT_CURSOR_POS`] directly.
pub const SAVE_SELECT_CURSOR_X_OFFSET: i32 = -14;

/// Retail PSX framebuffer position of the **left edge of the first
/// title glyph** drawn inside the load-screen panel. Pinned via
/// GPULog primitive scan at sstate9 (parked on the load screen):
/// retail emits four 14x15 textured-sprite primitives at dst
/// `(35, 13)`, `(42, 13)`, `(48, 13)`, `(55, 13)` for `L`, `o`, `a`,
/// `d`. Stage coords; the engine applies `stage_origin + pos *
/// stage_scale` when emitting screen-pixel draws.
///
/// Pairs with [`SAVE_SELECT_TITLE_COLOR`] (the bright-text CLUT entry
/// retail picks from the menu CLUT block at VRAM `(208, 510)`).
pub const SAVE_SELECT_TITLE_POS: (i32, i32) = (35, 13);

/// RGBA tint applied to the dialog-font stencil when rendering the
/// load-screen title word. Pinned to retail's framebuffer pixel
/// colour at sstate9: every bright "Load" texel is RGB `(206, 206,
/// 206)` (= entry `[15]` of the menu CLUT at VRAM `(208, 510)`).
/// The dialog-font atlas is whitewashed at load (see
/// `legaia_font::Font::load_paths`), so `color * texel = color` at
/// opaque texels - making the tint the source of truth for the
/// final pixel colour.
pub const SAVE_SELECT_TITLE_COLOR: [f32; 4] = [206.0 / 255.0, 206.0 / 255.0, 206.0 / 255.0, 1.0];

/// Retail PSX framebuffer position of the title-art atlas top-left.
/// Pinned via GPU primitive scan at sstate9: retail draws the title
/// quad at dst `(33, 6)..(287, 154)` sampling source `(0, 0, 254, 148)`
/// of the 256×256 title TIM (PROT 0888). Used as the anchor when
/// composing the title-screen on the canonical 320×240 stage.
pub const TITLE_ART_POS: (i32, i32) = (33, 6);
/// Retail-pinned size the title quad is drawn at - same as its source
/// sub-rect dimensions (no scaling).
pub const TITLE_ART_SIZE: (i32, i32) = (254, 148);

/// Source rects for the save-menu atlas, mirrored from
/// [`legaia_asset::title_pak::OVERLAY_SYSTEM_UI_PANEL_*`] (9-slice
/// panel chrome) + `OVERLAY_SAVE_MENU_BAND_SLOT[12]` (pills). Passed
/// into [`save_select_chrome_draws_for`] so it can build SpriteDraws
/// without depending on the engine-core build of the atlas.
///
/// The 9-slice panel tiles were pinned via
/// `scripts/pcsx-redux/scan_panel_prims.py` against the PCSX-Redux
/// sstate9 RAM dump - retail draws the panel as 14 separate
/// `GP0_TEXTURED_SPRITE` primitives sampling CLUT row 2 of the
/// system-UI sprite sheet at `PROT.DAT[0x018E0]`.
#[derive(Debug, Clone, Copy)]
pub struct SaveMenuAtlasRects {
    /// Panel top-left corner tile (4x4, CLUT row 2 of system-UI TIM).
    pub panel_tl: (u32, u32, u32, u32),
    /// Panel top-right corner tile (4x4).
    pub panel_tr: (u32, u32, u32, u32),
    /// Panel bottom-left corner tile (4x4).
    pub panel_bl: (u32, u32, u32, u32),
    /// Panel bottom-right corner tile (4x4).
    pub panel_br: (u32, u32, u32, u32),
    /// Panel top edge tile (24x4) - repeated horizontally between
    /// the top corners with a 1-wide remainder if the panel width
    /// doesn't divide evenly.
    pub panel_top: (u32, u32, u32, u32),
    /// Panel bottom edge tile (24x4).
    pub panel_bot: (u32, u32, u32, u32),
    /// Panel left edge tile (4x21).
    pub panel_left: (u32, u32, u32, u32),
    /// Panel right edge tile (4x21).
    pub panel_right: (u32, u32, u32, u32),
    /// SLOT 1 pill source rect (CLUT 7, bright blue).
    pub slot1: (u32, u32, u32, u32),
    /// SLOT 2 pill source rect (CLUT 7, bright blue).
    pub slot2: (u32, u32, u32, u32),
    /// Pointing-finger cursor sprite (16x16, CLUT row 7 of the
    /// system-UI TIM). Retail renders this to the left of the active
    /// slot pill in the SaveSelect menu.
    pub cursor: (u32, u32, u32, u32),
    /// Panel interior fill tile (32x29, gradient-baked). Retail draws
    /// this as 3 gouraud-shaded textured-quad primitives sampling
    /// the marbled-blue stippled region of the system-UI TIM. The
    /// atlas builder pre-bakes the gouraud gradient so the engine
    /// can draw the tile as a regular SpriteDraw, tiled horizontally
    /// 2× full-width + 1× 17-wide-remainder.
    pub panel_interior: (u32, u32, u32, u32),
    /// Load-screen empty-cell frame sprite (32x32, 20x20 blue hollow
    /// frame centered with 6px transparent margin). Used by the slot-
    /// preview screen to draw the 5x3 grid of save-slot boxes. When
    /// `None`, the slot-preview falls back to a solid blue rect.
    pub load_empty_frame: Option<(u32, u32, u32, u32)>,
    /// Up to 3 character portrait sub-rects (16x16 each, decoded
    /// from PROT.DAT[0x1AC90..0x1AF30]). Index = char_id (0=Vahn,
    /// 1=Noa, 2=Gala). `None` for char_ids past the 3-portrait atlas.
    pub load_portrait_by_char: [Option<(u32, u32, u32, u32)>; 3],
}

/// Build [`SpriteDraw`]s for the retail save-screen chrome (9-slice
/// panel frame + slot pills) anchored at the supplied stage origin.
///
/// Retail composes the 81×29 panel from 14 textured-sprite primitives
/// - 4 corners (4×4 each), top + bottom edges (24×4 repeated 3× with
///   a 1×4 remainder), and left + right edges (4×21). This function
///   reproduces that composition exactly, pulling tiles from the
///   system-UI sprite sheet at the byte-pinned source rects in
///   `legaia_asset::title_pak::OVERLAY_SYSTEM_UI_PANEL_*`.
///
/// No interior fill is drawn - retail leaves the middle of the
/// 9-slice frame empty so the dimmed title art behind shows through.
///
/// Layout (positions in stage pixels, relative to `stage_origin`):
/// ```text
///   ┌──────────┐
///   │   Load   │           ← panel @ SAVE_SELECT_PANEL_POS, 81x29
///   └──────────┘
///                  SLOT 1  ← pill @ SAVE_SELECT_SLOT1_POS
///                  SLOT 2  ← stacked at +SAVE_SELECT_SLOT_PITCH_Y
/// ```
///
/// `pills` lists the slot indices whose pills are drawn. Pills are
/// rendered at `pill_anchor + (0, slot_index * PITCH)`, and slot
/// index `0` uses the SLOT 1 sprite while every other index falls
/// back to the SLOT 2 sprite. Retail draws all pills during Browsing
/// (`&[0, 1]`) but shows only the selected pill once a slot has been
/// confirmed (`&[selected_slot]`) - the NowChecking dialog and
/// SlotPreview grid both hide the non-selected pills.
///
/// `pill_anchor` is the framebuffer top-left of the slot-index-0
/// pill. Pass [`SAVE_SELECT_SLOT1_POS`] during Browsing and
/// [`SAVE_SELECT_SLOT1_POS_LOAD_ACTIVE`] during the Load-active
/// states (NowChecking / SlotPreview), matching retail's pill
/// relocation up under the Load panel once a slot is committed.
///
/// `stage_scale` multiplies every dst dimension so callers that
/// upscale a 256x256 stage into a larger surface keep the chrome
/// in lockstep with the title-art bands.
pub fn save_select_chrome_draws_for(
    rects: &SaveMenuAtlasRects,
    pills: &[u8],
    pill_anchor: (i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let scale = stage_scale.max(1);
    let mut out: Vec<SpriteDraw> = Vec::new();
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

    let push = |out: &mut Vec<SpriteDraw>,
                src: (u32, u32, u32, u32),
                dst_stage_x: i32,
                dst_stage_y: i32,
                dst_w_stage: i32,
                dst_h_stage: i32,
                color: [f32; 4]| {
        out.push(SpriteDraw {
            dst: (
                stage_origin.0 + dst_stage_x * scale as i32,
                stage_origin.1 + dst_stage_y * scale as i32,
                (dst_w_stage as u32) * scale,
                (dst_h_stage as u32) * scale,
            ),
            src,
            color,
        });
    };

    // 9-slice panel composition. All dst coords are stage pixels;
    // the byte-perfect retail dimensions are (81 wide, 29 tall) with
    // 4-pixel corners + 24-wide edge tiles repeated. Pinned via
    // GP0 primitive scan - see `project_load_screen_panel_source_pinned`.
    let (panel_x, panel_y) = SAVE_SELECT_PANEL_POS;
    let (panel_w, panel_h) = SAVE_SELECT_PANEL_SIZE;
    let corner_w = rects.panel_tl.2 as i32; // 4
    let corner_h = rects.panel_tl.3 as i32; // 4
    let edge_w = rects.panel_top.2 as i32; // 24
    let edge_h = rects.panel_top.3 as i32; // 4
    let v_edge_h = rects.panel_left.3 as i32; // 21

    // --- Panel interior (drawn FIRST so the 9-slice border draws on top) ---
    // Retail emits 3 textured-gouraud quads sampling the same 32x29
    // marbled-blue region from the system-UI TIM with a vertical
    // gray gradient pre-baked into the atlas tile. The quads tile
    // horizontally to cover the full 81-wide panel: 2 full 32-wide
    // copies + 1 17-wide remainder.
    let interior_w = rects.panel_interior.2 as i32; // 32
    let interior_h = rects.panel_interior.3 as i32; // 29
    let mut x_int = panel_x;
    let interior_right = panel_x + panel_w;
    while x_int < interior_right {
        let remaining = interior_right - x_int;
        let this_w = remaining.min(interior_w);
        // Narrow the src rect's width when we're on the last
        // (partial) tile so engines sample only the columns retail
        // actually covers.
        let (sx, sy, _, sh) = rects.panel_interior;
        let src = (sx, sy, this_w as u32, sh);
        push(&mut out, src, x_int, panel_y, this_w, interior_h, white);
        x_int += this_w;
    }

    // --- Corners (4 tiles) ---
    push(
        &mut out,
        rects.panel_tl,
        panel_x,
        panel_y,
        corner_w,
        corner_h,
        white,
    );
    push(
        &mut out,
        rects.panel_tr,
        panel_x + panel_w - corner_w,
        panel_y,
        corner_w,
        corner_h,
        white,
    );
    push(
        &mut out,
        rects.panel_bl,
        panel_x,
        panel_y + panel_h - corner_h,
        corner_w,
        corner_h,
        white,
    );
    push(
        &mut out,
        rects.panel_br,
        panel_x + panel_w - corner_w,
        panel_y + panel_h - corner_h,
        corner_w,
        corner_h,
        white,
    );

    // --- Top + bottom edges (repeating 24-wide tiles with remainder) ---
    let edge_span = panel_w - 2 * corner_w; // 73 pixels between corners
    let full_tiles = edge_span / edge_w; // 3 full 24-wide tiles
    let remainder = edge_span - full_tiles * edge_w; // 1 pixel remainder
    let edge_y_top = panel_y;
    let edge_y_bot = panel_y + panel_h - edge_h;
    let mut x = panel_x + corner_w;
    for _ in 0..full_tiles {
        push(
            &mut out,
            rects.panel_top,
            x,
            edge_y_top,
            edge_w,
            edge_h,
            white,
        );
        push(
            &mut out,
            rects.panel_bot,
            x,
            edge_y_bot,
            edge_w,
            edge_h,
            white,
        );
        x += edge_w;
    }
    if remainder > 0 {
        // Sample only the first `remainder` columns of the edge tile
        // - retail dispatches this as a separate sprite with width
        // narrowed to the remainder.
        let (ux, uy, _, uh) = rects.panel_top;
        let top_rem = (ux, uy, remainder as u32, uh);
        let (bx, by, _, bh) = rects.panel_bot;
        let bot_rem = (bx, by, remainder as u32, bh);
        push(&mut out, top_rem, x, edge_y_top, remainder, edge_h, white);
        push(&mut out, bot_rem, x, edge_y_bot, remainder, edge_h, white);
    }

    // --- Left + right edges (single tall tile each) ---
    push(
        &mut out,
        rects.panel_left,
        panel_x,
        panel_y + corner_h,
        corner_w,
        v_edge_h,
        white,
    );
    push(
        &mut out,
        rects.panel_right,
        panel_x + panel_w - corner_w,
        panel_y + corner_h,
        corner_w,
        v_edge_h,
        white,
    );

    // --- Slot pills (atlas decoded with CLUT 7) at their natural row
    // positions anchored at `pill_anchor`. Each pill is drawn at
    // `pill_anchor + (0, slot_index*PITCH)` so retail-pinned positions
    // stay stable regardless of which subset of pills is currently
    // visible (selected-only during NowChecking / SlotPreview vs. all
    // pills during Browsing) AND so retail's Load-active relocation of
    // SLOT 1 under the Load panel is just a different `pill_anchor`.
    for &slot in pills {
        let dst_y = pill_anchor.1 + (slot as i32) * SAVE_SELECT_SLOT_PITCH_Y;
        let src = if slot == 0 { rects.slot1 } else { rects.slot2 };
        push(
            &mut out,
            src,
            pill_anchor.0,
            dst_y,
            src.2 as i32,
            src.3 as i32,
            white,
        );
    }

    out
}

/// Build the [`SpriteDraw`] for the pointing-finger cursor sprite.
/// Separate from [`save_select_chrome_draws_for`] so callers can
/// choose whether to draw the cursor (e.g. suppress during fade-out)
/// and where to anchor it (typically `cursor_row` selects which slot
/// pill the finger points at).
///
/// `cursor_row` is the 0-indexed pill the cursor sits next to.
pub fn save_select_cursor_draw_for(
    rects: &SaveMenuAtlasRects,
    cursor_row: usize,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> SpriteDraw {
    let scale = stage_scale.max(1);
    let src = rects.cursor;
    // Cursor at retail's byte-pinned framebuffer position when
    // pointing at SLOT 1; shifts down by SAVE_SELECT_SLOT_PITCH_Y
    // per pill row.
    let dst_stage_x = SAVE_SELECT_CURSOR_POS.0;
    let dst_stage_y = SAVE_SELECT_CURSOR_POS.1 + (cursor_row as i32) * SAVE_SELECT_SLOT_PITCH_Y;
    SpriteDraw {
        dst: (
            stage_origin.0 + dst_stage_x * scale as i32,
            stage_origin.1 + dst_stage_y * scale as i32,
            src.2 * scale,
            src.3 * scale,
        ),
        src,
        color: [1.0, 1.0, 1.0, 1.0],
    }
}

// -----------------------------------------------------------------------
// Generic 9-slice panel composition + "Now checking" dialog + slot-preview
// grid + slot-info panel rendering.
//
// All positions are stage pixels (32x240 boot-UI stage); `stage_scale`
// upscales to surface pixels. Pinned against the captured retail
// framebuffer in `captures/slot_info_dump/.../now_checking_fb.png`
// (sstate9 → CROSS → ~30 vsyncs) and `slot_info_fb.png` (~170 vsyncs).
// -----------------------------------------------------------------------

/// Compose a 9-slice panel at arbitrary `(dst_x, dst_y, dst_w, dst_h)`
/// stage pixels into `out`. Tiles the top/bottom 24-wide edges with a
/// remainder, and tiles the left/right 4×21 edges vertically with a
/// remainder. Used by both [`save_select_chrome_draws_for`] (which has
/// its own legacy code path that retains byte-exact behaviour) and
/// [`now_checking_panel_draws_for`].
///
/// Interior fill: a horizontal tiling of `rects.panel_interior` with
/// the per-tile width narrowed on the last (partial) column. The
/// retail engine emits the interior FIRST (3 gouraud-shaded quads
/// covering 32+32+17 of the 81-wide panel), then the border on top.
fn nine_slice_panel_into(
    out: &mut Vec<SpriteDraw>,
    rects: &SaveMenuAtlasRects,
    dst_stage: (i32, i32, i32, i32), // (x, y, w, h)
    stage_origin: (i32, i32),
    stage_scale: u32,
) {
    let scale = stage_scale.max(1) as i32;
    let white = [1.0, 1.0, 1.0, 1.0];
    let (px, py, pw, ph) = dst_stage;

    let push = |out: &mut Vec<SpriteDraw>,
                src: (u32, u32, u32, u32),
                sx: i32,
                sy: i32,
                sw: i32,
                sh: i32| {
        out.push(SpriteDraw {
            dst: (
                stage_origin.0 + sx * scale,
                stage_origin.1 + sy * scale,
                (sw as u32) * scale as u32,
                (sh as u32) * scale as u32,
            ),
            src,
            color: white,
        });
    };

    let cw = rects.panel_tl.2 as i32;
    let ch = rects.panel_tl.3 as i32;
    let edge_w = rects.panel_top.2 as i32;
    let edge_h = rects.panel_top.3 as i32;
    let v_edge_h = rects.panel_left.3 as i32;

    // Interior fill - horizontal tiling.
    let int_w = rects.panel_interior.2 as i32;
    let int_h = rects.panel_interior.3 as i32;
    // Match interior tile height to actual panel height (retail's panel
    // is 29 tall but here we may want 32+). If panel is taller than the
    // 29-tall interior tile, stretch vertically by emitting a single
    // sprite with full height.
    let interior_h = ph.min(int_h.max(ph));
    let mut x_int = px;
    while x_int < px + pw {
        let remaining = px + pw - x_int;
        let this_w = remaining.min(int_w);
        let (sx, sy, _, sh) = rects.panel_interior;
        let actual_sh = sh.min(interior_h as u32);
        let src = (sx, sy, this_w as u32, actual_sh);
        push(out, src, x_int, py, this_w, interior_h);
        x_int += this_w;
    }

    // Four corners.
    push(out, rects.panel_tl, px, py, cw, ch);
    push(out, rects.panel_tr, px + pw - cw, py, cw, ch);
    push(out, rects.panel_bl, px, py + ph - ch, cw, ch);
    push(out, rects.panel_br, px + pw - cw, py + ph - ch, cw, ch);

    // Top + bottom edges with remainder.
    let edge_span = pw - 2 * cw;
    let full_tiles = edge_span / edge_w;
    let remainder = edge_span - full_tiles * edge_w;
    let edge_y_top = py;
    let edge_y_bot = py + ph - edge_h;
    let mut x = px + cw;
    for _ in 0..full_tiles {
        push(out, rects.panel_top, x, edge_y_top, edge_w, edge_h);
        push(out, rects.panel_bot, x, edge_y_bot, edge_w, edge_h);
        x += edge_w;
    }
    if remainder > 0 {
        let (ux, uy, _, uh) = rects.panel_top;
        let top_rem = (ux, uy, remainder as u32, uh);
        let (bx, by, _, bh) = rects.panel_bot;
        let bot_rem = (bx, by, remainder as u32, bh);
        push(out, top_rem, x, edge_y_top, remainder, edge_h);
        push(out, bot_rem, x, edge_y_bot, remainder, edge_h);
    }

    // Left + right edges. The source tile is 4x21; tile vertically
    // with a remainder for taller-than-21 interiors.
    let vert_span = ph - 2 * ch;
    let v_full = vert_span / v_edge_h;
    let v_rem = vert_span - v_full * v_edge_h;
    let mut y = py + ch;
    for _ in 0..v_full {
        push(out, rects.panel_left, px, y, cw, v_edge_h);
        push(out, rects.panel_right, px + pw - cw, y, cw, v_edge_h);
        y += v_edge_h;
    }
    if v_rem > 0 {
        let (lx, ly, lw, _) = rects.panel_left;
        let left_rem = (lx, ly, lw, v_rem as u32);
        let (rx, ry, rw, _) = rects.panel_right;
        let right_rem = (rx, ry, rw, v_rem as u32);
        push(out, left_rem, px, y, cw, v_rem);
        push(out, right_rem, px + pw - cw, y, cw, v_rem);
    }
}

/// Compose the retail 9-slice window chrome (interior fill + border) for
/// **any** menu window at an arbitrary `(x, y, w, h)` stage rect.
///
/// This is the reusable primitive shared by every faithful menu panel.
/// All of Legaia's pause-menu windows (field menu, item list, status,
/// equipment, spell list) are framed by the same bordered-window sprite:
/// the 9-slice tiles of the system-UI sprite sheet at `PROT.DAT[0x018E0]`
/// (CLUT row 2). They all pull chrome from the same [`SaveMenuAtlasRects`]
/// the save screen already builds; the tile composition (4 corners +
/// tiled edges + tiled interior) is identical and only the destination
/// rect changes per window.
///
/// `dst_stage` is `(x, y, w, h)` in stage pixels; `stage_origin` +
/// `stage_scale` place and upscale the stage into the surface exactly as
/// [`save_select_chrome_draws_for`] does. Returns the border/interior
/// [`SpriteDraw`]s only - text, cursors, and portraits are layered on top
/// by the caller.
pub fn menu_window_chrome_draws_for(
    rects: &SaveMenuAtlasRects,
    dst_stage: (i32, i32, i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let mut out = Vec::with_capacity(24);
    nine_slice_panel_into(&mut out, rects, dst_stage, stage_origin, stage_scale);
    out
}

/// Retail PSX framebuffer placement of the "Now checking" dialog panel.
/// Pinned via gold-border pixel scan on
/// `captures/slot_info_dump/2026-05-18T09-04-46Z/slot_info_fb.png`:
/// dialog gold borders at fb-y rows 97 (top) and 135 (bottom), spanning
/// fb-x 70..249 (width 180, height 39). The dialog is horizontally
/// centered on the 320-wide stage (`(320 - 180) / 2 = 70`).
pub const NOW_CHECKING_PANEL_POS: (i32, i32) = (70, 97);
pub const NOW_CHECKING_PANEL_SIZE: (u32, u32) = (180, 39);

/// Retail slide-in start position for the "Now checking" dialog's
/// **center x** before it has slid into place. From Ghidra trace
/// `FUN_801e1c1c(0, DAT_801ef160, 0x1a0, 0x70, 0xa0, 0x70)` - slide
/// from `(0x1a0=416, 0x70=112)` to target `(0xa0=160, 0x70=112)`. The
/// dialog starts off-screen to the right and slides left over 16
/// frames. Engine code interpolates `slide_offset_x = (start - target) *
/// (1 - t/4096)`, where `t = session.slide_anim_t()`.
pub const NOW_CHECKING_SLIDE_START_X: i32 = 416;
pub const NOW_CHECKING_SLIDE_TARGET_X: i32 = 160;

/// Center X used by retail's dialog renderer for every messagebox
/// text line. Pinned via Ghidra: every `FUN_801E3EE0(string, x, y)`
/// call in `overlay_save_ui_select_801dd35c.txt` passes
/// `x = 0xA0 = 160` (= stage horizontal center) and renders the
/// glyphs at `(x - text_width/2, y + 7)`. The +7 offset is baked
/// into the renderer itself (see `overlay_menu_801e3ee0.txt`).
pub const DIALOG_TEXT_CENTER_X: i32 = 160;
/// "Now checking." line: retail `FUN_801E3EE0(string, 0xA0, 0x60)`
/// → text top y = 0x60 + 7 = 103. Source:
/// `overlay_save_ui_select_801dd35c.txt:1054`.
pub const NOW_CHECKING_TEXT_LINE1_Y: i32 = 103;
/// "Do not remove MEMORY CARD" line: retail
/// `FUN_801E3EE0(string, 0xA0, 0x70)` → text top y = 0x70 + 7 = 119.
/// Source: `overlay_save_ui_select_801dd35c.txt:809`.
pub const NOW_CHECKING_TEXT_LINE2_Y: i32 = 119;
/// Backwards-compat: left-edge positions derived from
/// `center_x - retail_text_width / 2` for the two lines (computed
/// at runtime in `now_checking_text_draws_for` from the actual
/// font metrics). Kept as inert constants for callers that don't
/// have a font reference handy.
pub const NOW_CHECKING_TEXT_LINE1: (i32, i32) = (122, NOW_CHECKING_TEXT_LINE1_Y);
pub const NOW_CHECKING_TEXT_LINE2: (i32, i32) = (78, NOW_CHECKING_TEXT_LINE2_Y);

/// Build [`SpriteDraw`]s for the "Now checking" dialog's 9-slice
/// panel only (no text). `slide_offset` is added to the panel
/// position so callers can drive the retail slide-in animation
/// (Ghidra-pinned: dialog slides from x=416 to x=160 over 16 frames
/// via `FUN_801E1C1C` mode 0). Pass `(0, 0)` for the static
/// fully-arrived case.
pub fn now_checking_panel_draws_for(
    rects: &SaveMenuAtlasRects,
    stage_origin: (i32, i32),
    stage_scale: u32,
    slide_offset: (i32, i32),
) -> Vec<SpriteDraw> {
    let mut out = Vec::with_capacity(16);
    let (px, py) = NOW_CHECKING_PANEL_POS;
    let (pw, ph) = NOW_CHECKING_PANEL_SIZE;
    nine_slice_panel_into(
        &mut out,
        rects,
        (
            px + slide_offset.0,
            py + slide_offset.1,
            pw as i32,
            ph as i32,
        ),
        stage_origin,
        stage_scale,
    );
    out
}

/// Build [`TextDraw`]s for the "Now checking. Do not remove MEMORY
/// CARD" two-line dialog text. Each line is **horizontally centered
/// on stage x = [`DIALOG_TEXT_CENTER_X`]** matching retail's
/// `FUN_801E3EE0(string, center_x, top_y)` renderer
/// (`overlay_menu_801e3ee0.txt`), with the layout's left edge
/// computed as `center_x - text_width / 2` from the actual font
/// metrics rather than hard-coded.
pub fn now_checking_text_draws_for(
    font: &legaia_font::Font,
    stage_origin: (i32, i32),
    stage_scale: u32,
    slide_offset: (i32, i32),
) -> Vec<TextDraw> {
    let scale = stage_scale.max(1);
    let color = SAVE_SELECT_TITLE_COLOR;
    let mut out = Vec::with_capacity(40);

    let emit_centered = |out: &mut Vec<TextDraw>, text: &str, top_y: i32| {
        let layout = font.layout_ascii(text);
        let left_x = DIALOG_TEXT_CENTER_X - (layout.advance_x as i32 / 2) + slide_offset.0;
        let top_y = top_y + slide_offset.1;
        for g in &layout.glyphs {
            let sx = left_x + g.dst_x;
            let sy = top_y + g.dst_y;
            out.push(TextDraw {
                dst: (
                    stage_origin.0 + sx * scale as i32,
                    stage_origin.1 + sy * scale as i32,
                    g.width * scale,
                    g.height * scale,
                ),
                src: (g.atlas_x, g.atlas_y, g.width, g.height),
                color,
            });
        }
    };

    emit_centered(&mut out, "Now checking.", NOW_CHECKING_TEXT_LINE1_Y);
    emit_centered(
        &mut out,
        "Do not remove MEMORY CARD",
        NOW_CHECKING_TEXT_LINE2_Y,
    );
    out
}

/// Retail PSX framebuffer placement of the slot-preview 5×3 grid.
/// Mirror of `legaia_asset::title_pak::OVERLAY_LOAD_SLOT_GRID_*` -
/// retail-pinned via per-row/per-column blue-outline scan on
/// `slot_info_fb.png`: cell visible top-left corners at fb-y rows
/// 35 (row 0), 55 (row 1), 75 (row 2) and fb-x columns 104, 144,
/// 184, 224, 264 (col 0..4). Pitch X = 40, pitch Y = 20.
pub const SLOT_GRID_ORIGIN: (i32, i32) = (104, 35);
pub const SLOT_GRID_PITCH_X: i32 = 40;
pub const SLOT_GRID_PITCH_Y: i32 = 20;
pub const SLOT_GRID_COLS: usize = 5;
pub const SLOT_GRID_ROWS: usize = 3;

/// Per-cell view passed into [`slot_preview_grid_draws_for`]. Each
/// memory-card block becomes one cell; `present=false` cells render
/// as plain empty frames. When a save is present, `portrait_char_id`
/// (= lead party member's char_id) selects which 16×16 portrait
/// (0=Vahn, 1=Noa, 2=Gala) is drawn inside the frame; `None` falls
/// back to the empty frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct SlotGridCell {
    pub present: bool,
    pub portrait_char_id: Option<u8>,
}

/// Build [`SpriteDraw`]s for the 5×3 slot-preview grid. Each cell
/// gets the empty-frame sprite (32×32 with 20×20 visible border).
/// Filled cells additionally get a 16×16 portrait centred in the
/// frame. The cursor sprite sits to the left of the currently
/// selected cell.
pub fn slot_preview_grid_draws_for(
    rects: &SaveMenuAtlasRects,
    cells: &[SlotGridCell],
    cursor_slot: u8,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let scale = stage_scale.max(1) as i32;
    let white = [1.0, 1.0, 1.0, 1.0];
    let mut out = Vec::with_capacity(SLOT_GRID_COLS * SLOT_GRID_ROWS + 2);

    let push = |out: &mut Vec<SpriteDraw>,
                src: (u32, u32, u32, u32),
                sx: i32,
                sy: i32,
                sw: i32,
                sh: i32| {
        out.push(SpriteDraw {
            dst: (
                stage_origin.0 + sx * scale,
                stage_origin.1 + sy * scale,
                (sw as u32) * scale as u32,
                (sh as u32) * scale as u32,
            ),
            src,
            color: white,
        });
    };

    let max_slots = (SLOT_GRID_COLS * SLOT_GRID_ROWS).min(cells.len());
    for (slot, cell) in cells.iter().take(max_slots).enumerate() {
        let col = slot % SLOT_GRID_COLS;
        let row = slot / SLOT_GRID_COLS;
        // Empty-frame sprite top-left in stage pixels. The 32×32
        // sprite has a 6px transparent margin; the visible 20×20
        // frame's top-left should land at (origin.x + col*pitch_x,
        // origin.y + row*pitch_y). So sprite origin = grid pos - 6.
        let cell_x = SLOT_GRID_ORIGIN.0 + (col as i32) * SLOT_GRID_PITCH_X;
        let cell_y = SLOT_GRID_ORIGIN.1 + (row as i32) * SLOT_GRID_PITCH_Y;
        if let Some(frame) = rects.load_empty_frame {
            // The full 32×32 sprite is drawn with its top-left at
            // (cell_x - 6, cell_y - 6) so the visible 20×20 border
            // sits at the cell position. Engines may instead sample
            // sub-rect (6, 6, 20, 20) and skip the margin - both
            // produce the same on-screen pixels.
            push(&mut out, frame, cell_x - 6, cell_y - 6, 32, 32);
        }
        if cell.present
            && let Some(char_id) = cell.portrait_char_id
            && let Some(portrait) = rects
                .load_portrait_by_char
                .get(char_id as usize)
                .copied()
                .flatten()
        {
            // Portrait centred inside the 20×20 visible frame
            // (16×16 portrait + 2px margin each side).
            push(&mut out, portrait, cell_x + 2, cell_y + 2, 16, 16);
        }
    }

    // Cursor sprite to the left of the currently-selected cell.
    // Retail pin: in `slot_info_fb.png` the pointing-finger cursor
    // bbox sits at fb-x 90..105 (16 wide) pointing at cell (0, 0) at
    // fb-x 104. That puts the cursor's right edge 1 px shy of the
    // cell's left edge - i.e. `cursor_x = cell_x - 14` (not -16).
    let cursor_col = (cursor_slot as usize) % SLOT_GRID_COLS;
    let cursor_row = (cursor_slot as usize) / SLOT_GRID_COLS;
    let cursor_x = SLOT_GRID_ORIGIN.0 + (cursor_col as i32) * SLOT_GRID_PITCH_X - 14;
    let cursor_y = SLOT_GRID_ORIGIN.1 + (cursor_row as i32) * SLOT_GRID_PITCH_Y;
    push(&mut out, rects.cursor, cursor_x, cursor_y, 16, 16);

    out
}

/// Retail PSX framebuffer placement of the slot-info panel (bottom
/// of stage), parked / fully-slid-in. Pinned to FUN_801E08D8's
/// `FUN_801e36c4(0xA0, local_34, 0x11c, 0x40)` call: panel chrome
/// top at `local_34` (= 138 when `DAT_801ef1a0 = 0x1000`), width 293,
/// height 77. Matches the visual gold-border scan in `slot_info_fb.png`.
pub const SLOT_INFO_PANEL_POS: (i32, i32) = (11, 138);
pub const SLOT_INFO_PANEL_SIZE: (u32, u32) = (293, 77);
/// Panel-y origin when fully slid-in (= `local_34` at anim_t=0x1000).
pub const SLOT_INFO_PANEL_PARKED_Y: i32 = 138;

/// Per-element offsets relative to the panel-y origin (= `local_34`),
/// derived from the Ghidra trace of `FUN_801E08D8`. The renderer adds
/// the live `panel_y` (interpolated through the slide-in animation) to
/// every offset.
///
/// Title-row offsets (`local_34 + 4` in retail = panel_y + 4):
/// - `SLOT_INFO_NO_OFFSET`: "No." badge (retail emits a sprite via
///   `FUN_801E3FF0` modes 2/3 at `(8, local_34 - 8)` with a CLUT row
///   selected by `DAT_801e5062 = slot_index << 4`. The engine renders
///   it as text at the same screen position, glyph-baseline corrected).
/// - `SLOT_INFO_LOCATION_OFFSET`: kingdom name string.
/// - `SLOT_INFO_TIME_LABEL_OFFSET`: "Time " prefix.
/// - `SLOT_INFO_TIME_VALUE_OFFSET`: HH:MM:SS digits. Retail splits the
///   digits across three calls (hours / minutes / seconds, with
///   sprite-colon separators) at x=236/252/260/276/284; the engine
///   renders one packed string at the leftmost x for simplicity.
pub const SLOT_INFO_NO_OFFSET: (i32, i32) = (8, -8);
pub const SLOT_INFO_LOCATION_OFFSET: (i32, i32) = (48, 4);
pub const SLOT_INFO_TIME_LABEL_OFFSET: (i32, i32) = (208, 4);
pub const SLOT_INFO_TIME_VALUE_OFFSET: (i32, i32) = (236, 4);

/// Per-character row offsets (column 0 of the 3-column slot grid;
/// retail loops `iVar4 = 0x10 + i*0x60` for columns 0/1/2 at
/// x = 16/112/208). For slots with one party member (Vahn-only starter
/// state) only column 0 renders. Y offsets relative to the panel y
/// origin via the retail `s3 = local_34 + 0x14` (= 158) per-character
/// base, then `s3 + N` for each row.
pub const SLOT_INFO_PORTRAIT_OFFSET: (i32, i32) = (16, 16);
pub const SLOT_INFO_NAME_OFFSET: (i32, i32) = (40, 20);
pub const SLOT_INFO_LV_LABEL_OFFSET: (i32, i32) = (16, 33);
pub const SLOT_INFO_LV_VALUE_OFFSET: (i32, i32) = (48, 33);
pub const SLOT_INFO_HP_LABEL_OFFSET: (i32, i32) = (16, 46);
pub const SLOT_INFO_HP_VALUE_OFFSET: (i32, i32) = (32, 46);
pub const SLOT_INFO_MP_LABEL_OFFSET: (i32, i32) = (16, 59);
pub const SLOT_INFO_MP_VALUE_OFFSET: (i32, i32) = (40, 59);

/// Plain-data view of the per-slot info passed to the info-panel
/// renderer. Engines build one from the `SlotSnapshot` of the
/// currently-focused slot plus a `Party::from_retail_sc_block` lift
/// for the leader's HP/MP.
#[derive(Debug, Clone, Copy)]
pub struct SlotInfoView<'a> {
    pub slot_no: u8,
    pub location: &'a str,
    pub play_time: &'a str,
    pub leader_name: &'a str,
    pub leader_level: u8,
    pub leader_hp: (u16, u16),
    pub leader_mp: (u16, u16),
    pub leader_char_id: u8,
}

/// Build the chrome [`SpriteDraw`]s for the slot-info panel (9-slice
/// frame + optional leader portrait, no text). Pair with
/// [`slot_info_panel_text_draws_for`] for the labels.
///
/// `panel_y_offset` is the slide-in delta from the parked y
/// (positive = pushed below parked, used while the panel slides up).
/// Engine callers compute it from `interpolate_anim((0, OFFSCREEN),
/// (0, PARKED), session.info_panel_slide_anim_t()).1 - PARKED`.
pub fn slot_info_panel_draws_for(
    rects: &SaveMenuAtlasRects,
    info: Option<&SlotInfoView<'_>>,
    panel_y_offset: i32,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let mut out = Vec::with_capacity(20);
    let (px, py_base) = SLOT_INFO_PANEL_POS;
    let py = py_base + panel_y_offset;
    let (pw, ph) = SLOT_INFO_PANEL_SIZE;
    nine_slice_panel_into(
        &mut out,
        rects,
        (px, py, pw as i32, ph as i32),
        stage_origin,
        stage_scale,
    );

    // Leader portrait (16x16) inside the info panel - drawn only
    // when a save is present at the current slot. Position pinned
    // from FUN_801E08D8's `FUN_801e3ff0(0, _, iVar4=16, s3-4=154)`
    // with s3 = local_34 + 20: portrait top-left at (16, panel_y+16).
    if let Some(info) = info
        && let Some(portrait) = rects
            .load_portrait_by_char
            .get(info.leader_char_id as usize)
            .copied()
            .flatten()
    {
        let scale = stage_scale.max(1) as i32;
        let px = SLOT_INFO_PORTRAIT_OFFSET.0;
        let pyy = py_base + SLOT_INFO_PORTRAIT_OFFSET.1 + panel_y_offset;
        out.push(SpriteDraw {
            dst: (
                stage_origin.0 + px * scale,
                stage_origin.1 + pyy * scale,
                16 * scale as u32,
                16 * scale as u32,
            ),
            src: portrait,
            color: [1.0, 1.0, 1.0, 1.0],
        });
    }
    out
}

/// Build [`TextDraw`]s for the slot-info panel labels (No., kingdom
/// name, time, character stats). Returns empty when `info` is `None`.
/// `panel_y_offset` matches the value passed to
/// [`slot_info_panel_draws_for`].
pub fn slot_info_panel_text_draws_for(
    font: &legaia_font::Font,
    info: Option<&SlotInfoView<'_>>,
    panel_y_offset: i32,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<TextDraw> {
    let Some(info) = info else { return Vec::new() };
    let scale = stage_scale.max(1);
    let color = SAVE_SELECT_TITLE_COLOR;
    let panel_y = SLOT_INFO_PANEL_PARKED_Y + panel_y_offset;
    let mut out = Vec::with_capacity(80);

    let emit_at = |out: &mut Vec<TextDraw>, text: &str, base: (i32, i32)| {
        let layout = font.layout_ascii(text);
        for g in &layout.glyphs {
            let sx = base.0 + g.dst_x;
            let sy = base.1 + g.dst_y;
            out.push(TextDraw {
                dst: (
                    stage_origin.0 + sx * scale as i32,
                    stage_origin.1 + sy * scale as i32,
                    g.width * scale,
                    g.height * scale,
                ),
                src: (g.atlas_x, g.atlas_y, g.width, g.height),
                color,
            });
        }
    };

    // Title row.
    emit_at(
        &mut out,
        &format!("No.{}", info.slot_no),
        (SLOT_INFO_NO_OFFSET.0, panel_y + SLOT_INFO_NO_OFFSET.1),
    );
    emit_at(
        &mut out,
        info.location,
        (
            SLOT_INFO_LOCATION_OFFSET.0,
            panel_y + SLOT_INFO_LOCATION_OFFSET.1,
        ),
    );
    emit_at(
        &mut out,
        "Time",
        (
            SLOT_INFO_TIME_LABEL_OFFSET.0,
            panel_y + SLOT_INFO_TIME_LABEL_OFFSET.1,
        ),
    );
    emit_at(
        &mut out,
        info.play_time,
        (
            SLOT_INFO_TIME_VALUE_OFFSET.0,
            panel_y + SLOT_INFO_TIME_VALUE_OFFSET.1,
        ),
    );

    // Character row (column 0 only - multi-character party expansion
    // would re-iterate at base_x += 96).
    emit_at(
        &mut out,
        info.leader_name,
        (SLOT_INFO_NAME_OFFSET.0, panel_y + SLOT_INFO_NAME_OFFSET.1),
    );
    emit_at(
        &mut out,
        "LV",
        (
            SLOT_INFO_LV_LABEL_OFFSET.0,
            panel_y + SLOT_INFO_LV_LABEL_OFFSET.1,
        ),
    );
    emit_at(
        &mut out,
        &format!("{}", info.leader_level),
        (
            SLOT_INFO_LV_VALUE_OFFSET.0,
            panel_y + SLOT_INFO_LV_VALUE_OFFSET.1,
        ),
    );
    emit_at(
        &mut out,
        "HP",
        (
            SLOT_INFO_HP_LABEL_OFFSET.0,
            panel_y + SLOT_INFO_HP_LABEL_OFFSET.1,
        ),
    );
    emit_at(
        &mut out,
        &format!("{}/{}", info.leader_hp.0, info.leader_hp.1),
        (
            SLOT_INFO_HP_VALUE_OFFSET.0,
            panel_y + SLOT_INFO_HP_VALUE_OFFSET.1,
        ),
    );
    emit_at(
        &mut out,
        "MP",
        (
            SLOT_INFO_MP_LABEL_OFFSET.0,
            panel_y + SLOT_INFO_MP_LABEL_OFFSET.1,
        ),
    );
    emit_at(
        &mut out,
        &format!("{}/{}", info.leader_mp.0, info.leader_mp.1),
        (
            SLOT_INFO_MP_VALUE_OFFSET.0,
            panel_y + SLOT_INFO_MP_VALUE_OFFSET.1,
        ),
    );
    out
}

/// Build [`TextDraw`]s for the save-select panel.
///
/// Retail layout (positions in stage pixels - pairs with
/// [`save_select_chrome_draws_for`] for the panel / pill sprites):
/// ```text
///   ┌──────────┐
///   │   Load   │           ← title word centered inside panel
///   └──────────┘
///                  SLOT 1  ← pill (sprite, label baked in)
///              >   SLOT 2  ← cursor arrow points at selected pill
/// ```
///
/// The function emits ONLY text: the panel/pill chrome lives on the
/// sprite-overlay layer via [`save_select_chrome_draws_for`]. The
/// `font`'s tinted glyphs are used for the title word, the cursor
/// arrow, and confirm-prompt overlays.
///
/// `cursor` selects the highlighted row. When `confirm` is `Some`,
/// the Yes/No prompt is rendered below the slot stack with the
/// highlighted option determined by the second tuple element (0 =
/// Yes, 1 = No).
///
/// `rows` is retained for API compatibility - the row count and
/// label strings drive cursor placement; per-slot Lv/play-time/gold
/// details are deliberately not rendered (retail's load screen
/// surfaces those on a separate sub-screen).
/// `emit_text_cursor` controls whether to emit an ASCII `>` cursor
/// glyph next to the active pill. When the sprite cursor (the
/// pointing-finger from the system-UI TIM) is being emitted by
/// [`save_select_cursor_draw_for`] alongside this call, pass `false`
/// to avoid drawing both. When the save-menu atlas isn't available
/// (no disc / atlas build failed), pass `true` to fall back to the
/// text-glyph cursor so the player still sees a visual selection
/// indicator.
#[allow(clippy::too_many_arguments)]
pub fn save_select_draws_for(
    font: &legaia_font::Font,
    title: &str,
    rows: &[SaveSelectRow<'_>],
    cursor: usize,
    confirm: Option<(&str, u8)>,
    stage_origin: (i32, i32),
    stage_scale: u32,
    emit_text_cursor: bool,
) -> Vec<TextDraw> {
    const LINE_H: i32 = 14;
    let scale = stage_scale.max(1);
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];

    let mut out = Vec::new();

    // Helper: emit a layout's glyphs scaled by `glyph_scale` (in
    // addition to the stage origin/scale already applied to `pen`).
    // Used for the panel title + cursor so they read large enough to
    // match the chrome's stage_scale (the chrome sprites are blitted
    // at stage_scale × source pixels, so glyphs should follow).
    let emit_scaled = |out: &mut Vec<TextDraw>,
                       layout: &legaia_font::Layout,
                       pen_stage: (i32, i32),
                       glyph_scale: u32,
                       color: [f32; 4]| {
        let gs = glyph_scale as i32;
        let pen_screen = (
            stage_origin.0 + pen_stage.0 * scale as i32,
            stage_origin.1 + pen_stage.1 * scale as i32,
        );
        for g in &layout.glyphs {
            out.push(TextDraw {
                dst: (
                    pen_screen.0 + g.dst_x * gs,
                    pen_screen.1 + g.dst_y * gs,
                    g.width * glyph_scale,
                    g.height * glyph_scale,
                ),
                src: (g.atlas_x, g.atlas_y, g.width, g.height),
                color,
            });
        }
    };

    // Title word ("Load" / "Save") drawn from the dialog-font
    // stencil. Retail emits one textured-sprite primitive per glyph
    // from the VRAM-resident dialog font at byte-pinned dst
    // positions starting at `SAVE_SELECT_TITLE_POS`, sampling the
    // bright-text CLUT entry `SAVE_SELECT_TITLE_COLOR` at VRAM
    // `(208, 510)`. The engine font's layout (variable advances
    // from `dialog_font_widths.csv` + `INTER_GLYPH_PAD = 1`) is
    // byte-equal to retail's per-glyph dst deltas, so a plain
    // `font.layout_ascii` placed at the pinned origin lines up 1:1.
    //
    // Unit discipline: the layout runs in STAGE pixels and
    // `glyph_screen_scale = stage_scale`, so each engine glyph
    // pixel becomes exactly `stage_scale` screen pixels - matching
    // the chrome sprites composed on the same 320x240 stage.
    let title_l = font.layout_ascii(title);
    emit_scaled(
        &mut out,
        &title_l,
        SAVE_SELECT_TITLE_POS,
        scale,
        SAVE_SELECT_TITLE_COLOR,
    );

    // Cursor arrow next to the selected slot pill. Pills sit at
    // SAVE_SELECT_SLOT1_POS + i*SAVE_SELECT_SLOT_PITCH_Y; arrow goes
    // to the left of the pill by SAVE_SELECT_CURSOR_X_OFFSET.
    // Skipped when the caller is also emitting the sprite cursor.
    if emit_text_cursor && !rows.is_empty() {
        let cursor_row = cursor.min(rows.len().saturating_sub(1));
        let cur_layout = font.layout_ascii(">");
        let cx = SAVE_SELECT_SLOT1_POS.0 + SAVE_SELECT_CURSOR_X_OFFSET;
        let cy = SAVE_SELECT_SLOT1_POS.1 + (cursor_row as i32) * SAVE_SELECT_SLOT_PITCH_Y;
        emit_scaled(&mut out, &cur_layout, (cx, cy), scale, gold);
    }

    if let Some((prompt, c_cursor)) = confirm {
        // Confirm prompt sits below the pill stack. Each row is
        // SAVE_SELECT_SLOT_PITCH_Y tall.
        let n = rows.len() as i32;
        let prompt_y = SAVE_SELECT_SLOT1_POS.1 + n * SAVE_SELECT_SLOT_PITCH_Y + LINE_H;
        let p_l = font.layout_ascii(prompt);
        out.extend(text_draws_for(
            &p_l,
            (
                stage_origin.0 + SAVE_SELECT_SLOT1_POS.0 * scale as i32,
                stage_origin.1 + prompt_y * scale as i32,
            ),
            white,
        ));
        for (i, opt) in ["Yes", "No"].iter().enumerate() {
            let x = SAVE_SELECT_SLOT1_POS.0 + 12 + i as i32 * 32;
            let y = prompt_y + LINE_H;
            let color = if i as u8 == c_cursor { gold } else { white };
            if i as u8 == c_cursor {
                let cur = font.layout_ascii(">");
                out.extend(text_draws_for(
                    &cur,
                    (
                        stage_origin.0 + (x - 8) * scale as i32,
                        stage_origin.1 + y * scale as i32,
                    ),
                    color,
                ));
            }
            let l = font.layout_ascii(opt);
            out.extend(text_draws_for(
                &l,
                (
                    stage_origin.0 + x * scale as i32,
                    stage_origin.1 + y * scale as i32,
                ),
                color,
            ));
        }
    }

    out
}

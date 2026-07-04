use crate::*;

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
    /// Raw (un-gradient-baked) marbled filigree interior tile (32x29).
    /// The pause-menu chrome tiles this in 2D as the window interior,
    /// darkened by a per-draw colour, matching retail's navy damask.
    /// Distinct from `panel_interior`, which is the save screen's
    /// gouraud-baked variant.
    pub panel_filigree: (u32, u32, u32, u32),
    /// Status-panel stat labels (LV / HP / MP), 16x10 each, from CLUT
    /// row 1 of the system-UI TIM (the `0x800732a4` icon-record
    /// palette). Drawn as sprites in place of ASCII glyphs on the
    /// status page.
    pub label_lv: (u32, u32, u32, u32),
    pub label_hp: (u32, u32, u32, u32),
    pub label_mp: (u32, u32, u32, u32),
    /// Status-page AP gauge pieces (CLUT row 4 of the system-UI TIM):
    /// left cap w/ red "AP" chip (24x16), trough body (56x16), value
    /// box (16x16) and right arrow tip (7x16). Retail composes the
    /// gauge from these four 1:1 sprites at the `FUN_801D33D8` bar
    /// anchor `(WX+0x40, WY+0x2d)`.
    pub gauge_cap: (u32, u32, u32, u32),
    pub gauge_trough: (u32, u32, u32, u32),
    pub gauge_box: (u32, u32, u32, u32),
    pub gauge_tip: (u32, u32, u32, u32),
    /// Red value-digit strip ("0".."9"): ten 6x6 cells at a 6-px
    /// pitch (ICO records `0x6C..=0x75`). Digit `d` is the sub-rect
    /// at `x + 6*d`.
    pub gauge_digits: (u32, u32, u32, u32),
    /// The "100" glyph shown at a full 100 AP (16x6, ICO record
    /// `0x6B`).
    pub gauge_100: (u32, u32, u32, u32),
    /// Meter-fill gradient column (2x6), stretched horizontally to
    /// `value/2` px inside the trough (the `FUN_8002c0b0` gouraud
    /// fill).
    pub gauge_fill: (u32, u32, u32, u32),
    /// Status-page equipment pictograms (12x12, CLUT row 8): the
    /// UV/CLUT-table records for the menu overlay's fixed slot icon
    /// codes (weapon fist / helmet / body armor / boot / Goods ring).
    pub icon_weapon: (u32, u32, u32, u32),
    pub icon_helmet: (u32, u32, u32, u32),
    pub icon_armor: (u32, u32, u32, u32),
    pub icon_boot: (u32, u32, u32, u32),
    pub icon_goods: (u32, u32, u32, u32),
    /// "Condition" pager arrows (16x16 solid triangles, CLUT row 7 of
    /// the system-UI TIM - the `FUN_8002b994` sprite-table kinds 2/3).
    /// Drawn flanking the status screen's window-27 label.
    pub pager_left: (u32, u32, u32, u32),
    pub pager_right: (u32, u32, u32, u32),
    /// Field-menu tab-banner plaque pieces (CLUT row 12 of the system-UI
    /// TIM): left cap (8x20), 16x20 body tile (repeated across the tab
    /// window's content width) and right cap (8x20). Consumed by
    /// [`crate::tab_banner_draws`].
    pub tab_cap_l: (u32, u32, u32, u32),
    pub tab_body: (u32, u32, u32, u32),
    pub tab_cap_r: (u32, u32, u32, u32),
    /// Status summary window ATR element icons (28x12), character order
    /// Vahn / Noa / Gala (system-UI extension strip, VRAM-row-500 CLUT).
    pub atr_icons: [(u32, u32, u32, u32); 3],
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

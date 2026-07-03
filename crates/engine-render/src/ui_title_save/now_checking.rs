use crate::*;

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

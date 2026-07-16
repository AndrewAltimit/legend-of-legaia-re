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
        false,
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

// ---------------------------------------------------------------------------
// Confirm dialog ("Do you wish to save?" / "load?" / "overwrite?")
// ---------------------------------------------------------------------------

/// Retail slide endpoints of the save screen's **confirm dialog** - the
/// "Do you wish to load? / save? / overwrite?" messagebox. Pinned from
/// `FUN_801E1C1C` mode 3 (timer `DAT_801ef1a4`), which slides it from
/// `(160, 344)` - below the stage - up to `(160, 88)`. Same 12-bit
/// fixed-point interpolation as every other save-UI slide.
pub const CONFIRM_DIALOG_CENTER_X: i32 = 160;
/// Companion to [`CONFIRM_DIALOG_CENTER_X`]: the y the dialog slides from.
pub const CONFIRM_DIALOG_SLIDE_START_Y: i32 = 344;
/// The y the dialog parks at (retail mode-3 target).
pub const CONFIRM_DIALOG_SLIDE_TARGET_Y: i32 = 88;

/// Panel size of the confirm dialog.
///
/// **Inferred, not independently pinned.** Only the mode-3 slide endpoints
/// are traced; the dialog is drawn as the same messagebox the "Now checking"
/// panel is (that one's rect came from a framebuffer scan), so it reuses
/// [`NOW_CHECKING_PANEL_SIZE`] and the same "panel sits `PANEL_TEXT_RISE`
/// above the mode's y" relationship. Replace with a scanned rect when a
/// capture parked on the prompt is available.
pub const CONFIRM_DIALOG_SIZE: (u32, u32) = NOW_CHECKING_PANEL_SIZE;

/// Offset from a messagebox's mode y to its panel top. Derived from the
/// "Now checking" pins: mode-0's y is `0x70 = 112` and its scanned panel top
/// is 97.
const PANEL_TEXT_RISE: i32 = 15;
/// First text line's offset from the mode y, from the same pins (line 1 at
/// 103 vs mode y 112).
const DIALOG_LINE1_DY: i32 = -9;
/// Second text line's offset from the mode y (line 2 at 119 vs 112).
const DIALOG_LINE2_DY: i32 = 7;
/// Half-width of the gap between the `Yes` and `No` options on the prompt's
/// second row.
const CONFIRM_OPTION_GAP: i32 = 28;

/// Panel top-left of the confirm dialog for a given slide `y` (the live
/// interpolation of [`CONFIRM_DIALOG_SLIDE_START_Y`] ->
/// [`CONFIRM_DIALOG_SLIDE_TARGET_Y`]).
fn confirm_dialog_rect(slide_y: i32) -> (i32, i32, i32, i32) {
    let (w, h) = CONFIRM_DIALOG_SIZE;
    (
        CONFIRM_DIALOG_CENTER_X - (w as i32) / 2,
        slide_y - PANEL_TEXT_RISE,
        w as i32,
        h as i32,
    )
}

/// Build the [`SpriteDraw`]s for the confirm dialog's 9-slice panel.
/// `slide_y` is the dialog's live y (interpolate
/// [`CONFIRM_DIALOG_SLIDE_START_Y`] -> [`CONFIRM_DIALOG_SLIDE_TARGET_Y`]
/// against the session's slide timer; pass the target for the static case).
pub fn confirm_dialog_panel_draws_for(
    rects: &SaveMenuAtlasRects,
    slide_y: i32,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let mut out = Vec::with_capacity(16);
    nine_slice_panel_into(
        &mut out,
        rects,
        confirm_dialog_rect(slide_y),
        stage_origin,
        stage_scale,
        false,
    );
    out
}

/// Build the [`TextDraw`]s for the confirm dialog: the `prompt` on the first
/// line and a `Yes` / `No` row on the second, both horizontally centred on
/// [`CONFIRM_DIALOG_CENTER_X`] the way retail's `FUN_801E3EE0` centres every
/// messagebox line. `cursor` selects the highlighted option (0 = Yes,
/// 1 = No).
pub fn confirm_dialog_text_draws_for(
    font: &legaia_font::Font,
    prompt: &str,
    cursor: u8,
    slide_y: i32,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<TextDraw> {
    let scale = stage_scale.max(1);
    let mut out = Vec::with_capacity(48);

    let mut emit = |text: &str, left_x: i32, top_y: i32, color: [f32; 4]| {
        let layout = font.layout_ascii(text);
        for g in &layout.glyphs {
            out.push(TextDraw {
                dst: (
                    stage_origin.0 + (left_x + g.dst_x) * scale as i32,
                    stage_origin.1 + (top_y + g.dst_y) * scale as i32,
                    g.width * scale,
                    g.height * scale,
                ),
                src: (g.atlas_x, g.atlas_y, g.width, g.height),
                color,
            });
        }
    };

    let centered = |font: &legaia_font::Font, text: &str| {
        CONFIRM_DIALOG_CENTER_X - (font.layout_ascii(text).advance_x as i32 / 2)
    };
    emit(
        prompt,
        centered(font, prompt),
        slide_y + DIALOG_LINE1_DY,
        SAVE_SELECT_TITLE_COLOR,
    );

    // Yes / No, flanking the centre; the picked one takes the bright menu
    // ink, the other stays dim.
    let row_y = slide_y + DIALOG_LINE2_DY;
    let dim: [f32; 4] = [
        SAVE_SELECT_TITLE_COLOR[0] * 0.55,
        SAVE_SELECT_TITLE_COLOR[1] * 0.55,
        SAVE_SELECT_TITLE_COLOR[2] * 0.55,
        1.0,
    ];
    for (i, opt) in ["Yes", "No"].iter().enumerate() {
        let w = font.layout_ascii(opt).advance_x as i32;
        let x = if i == 0 {
            CONFIRM_DIALOG_CENTER_X - CONFIRM_OPTION_GAP - w
        } else {
            CONFIRM_DIALOG_CENTER_X + CONFIRM_OPTION_GAP
        };
        let color = if i as u8 == cursor {
            SAVE_SELECT_TITLE_COLOR
        } else {
            dim
        };
        emit(opt, x, row_y, color);
    }
    out
}

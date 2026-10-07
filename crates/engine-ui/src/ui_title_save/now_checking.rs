use crate::*;

/// Retail PSX framebuffer placement of the "Now checking" dialog panel,
/// parked. Retail draws it with `FUN_801E36C4(160, 97, 169, 26)` (args traced
/// live; the `169` is the text-derived width retail computes for
/// "Do not remove MEMORY CARD"), which lands the 9-slice footprint at
/// `(66, 95, 185, 42)` - see [`messagebox_rect`]. Pinned against the live
/// GP0 draw list (edge tiles x=66 / x=247, rows 95..137).
///
/// The slide start / target are `center_x` values, so a caller drives the
/// slide by passing `slide_offset.0 = center_x - 160` (see
/// [`now_checking_panel_draws_for`]).
pub const NOW_CHECKING_PANEL_POS: (i32, i32) = (66, 95);
/// Companion to [`NOW_CHECKING_PANEL_POS`]. `169 + 16` x `26 + 16`: the panel
/// drawer inflates the centre rect by a uniform 8px on every side.
pub const NOW_CHECKING_PANEL_SIZE: (u32, u32) = (185, 42);

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
/// "Now checking." line: mode 0 of `FUN_801E1C1C` centres its first
/// line at `param_4 - 0x11` = 112 - 17 = 95, and the wrapper's +7
/// lands the glyph tops at y = 102 (GP0-dump-pinned).
pub const NOW_CHECKING_TEXT_LINE1_Y: i32 = 102;
/// "Do not remove MEMORY CARD" line: mode 0's second line at
/// `param_4 - 1` = 111, +7 → glyph tops at y = 118 (GP0-dump-pinned).
pub const NOW_CHECKING_TEXT_LINE2_Y: i32 = 118;
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
    card_banner_text_draws_for(
        font,
        ("Now checking.", "Do not remove MEMORY CARD"),
        stage_origin,
        stage_scale,
        slide_offset,
    )
}

/// The two centred lines of a card-operation messagebox on the "Now
/// checking" panel's rows (the card-read beat).
pub fn card_banner_text_draws_for(
    font: &legaia_font::Font,
    lines: (&str, &str),
    stage_origin: (i32, i32),
    stage_scale: u32,
    slide_offset: (i32, i32),
) -> Vec<TextDraw> {
    let mut out = Vec::with_capacity(48);
    emit_centered_text(
        &mut out,
        font,
        lines.0,
        DIALOG_TEXT_CENTER_X + slide_offset.0,
        NOW_CHECKING_TEXT_LINE1_Y + slide_offset.1,
        stage_origin,
        stage_scale,
    );
    emit_centered_text(
        &mut out,
        font,
        lines.1,
        DIALOG_TEXT_CENTER_X + slide_offset.0,
        NOW_CHECKING_TEXT_LINE2_Y + slide_offset.1,
        stage_origin,
        stage_scale,
    );
    out
}

/// `FUN_801E3EE0(text, center_x, y)`'s placement: the string centred on
/// `center_x`, glyph tops at `top_y` (the caller passes retail's `y + 7`).
fn emit_centered_text(
    out: &mut Vec<TextDraw>,
    font: &legaia_font::Font,
    text: &str,
    center_x: i32,
    top_y: i32,
    stage_origin: (i32, i32),
    stage_scale: u32,
) {
    let scale = stage_scale.max(1);
    let layout = font.layout_ascii(text);
    let left_x = center_x - (layout.advance_x as i32 / 2);
    for g in &layout.glyphs {
        out.push(TextDraw {
            dst: (
                stage_origin.0 + (left_x + g.dst_x) * scale as i32,
                stage_origin.1 + (top_y + g.dst_y) * scale as i32,
                g.width * scale,
                g.height * scale,
            ),
            src: (g.atlas_x, g.atlas_y, g.width, g.height),
            color: SAVE_SELECT_TITLE_COLOR,
        });
    }
}

/// The save / load commit beat's messagebox, as a host hands it over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardBannerView<'a> {
    pub lines: (&'a str, &'a str),
    /// The write / read panel (`true`) or the result line.
    pub work: bool,
    /// The write panel's slide timer, `0..=0x1000`.
    pub slide_t: u16,
    /// The write panel's progress timer `_DAT_801F01D0`, `0..=0x1000`
    /// (`+0x20` a frame): the fill's width and its green ramp.
    pub progress_t: u16,
}

/// The block badge - record 3 (`No.`) and record 2 (the numeral cell
/// `block`) of the save UI's sprite-record table, each a `0x2C` quad at
/// neutral modulation. `pen` is the `No.` quad's top-left; the numeral sits
/// `0x16` right of it (both call sites place the pair that way).
fn block_badge_into(
    out: &mut Vec<SpriteDraw>,
    block: u8,
    pen: (i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) {
    let neutral = (0x80, 0x80, 0x80);
    out.push(save_ui_record_quad(
        legaia_asset::title_pak::SAVE_MENU_ATLAS_NO_LABEL,
        neutral,
        pen,
        stage_origin,
        stage_scale,
    ));
    if let Some(cell) = legaia_asset::title_pak::save_menu_atlas_block_digit(u32::from(block)) {
        out.push(save_ui_record_quad(
            cell,
            neutral,
            (pen.0 + BLOCK_BADGE_DIGIT_DX, pen.1),
            stage_origin,
            stage_scale,
        ));
    }
}

/// The block badge as a draw list - `No.` at `pen`, the numeral for `block`
/// (`0..15`) `0x16` right of it. Every save-UI badge (the info panel's title
/// row, the confirm prompt, the write panel) is this pair.
pub fn block_badge_draws_for(
    block: u8,
    pen: (i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let mut out = Vec::with_capacity(2);
    block_badge_into(&mut out, block, pen, stage_origin, stage_scale);
    out
}

/// The numeral's x past the `No.` label's: `0x16` in both of
/// `FUN_801E1C1C`'s badge pairs (mode 3 `s5 - 0x16` / `s5`, mode 4
/// `x - 0x55` / `x - 0x3F`).
const BLOCK_BADGE_DIGIT_DX: i32 = 0x16;
/// The write panel's badge: `No.` at `(x - 0x55, 0x54)` (`0x801E2904`).
const CARD_WORK_BADGE: (i32, i32) = (-0x55, 0x54);
/// The progress tube (record 4) at `(x - 0x34, 0x90)` (`0x801E2BF4` /
/// `0x801E2BC8`).
const CARD_WORK_TUBE: (i32, i32) = (-0x34, 0x90);
/// The progress fill `FUN_801E2DC4(x - 0x2C, 0x95)`: one `0x3C` quad
/// `w = t * 0x58 >> 12` wide (the tube's 88-pixel interior at `t = 0x1000`)
/// and `6` tall, every vertex `(0xBC, t * 0xFF >> 12, 0)`.
const CARD_WORK_FILL: (i32, i32, i32, i32) = (-0x2C, 0x95, 0x58, 6);
/// The fill's fixed red channel.
const CARD_WORK_FILL_RED: u8 = 0xBC;

/// The write panel's progress tube and fill at panel centre `x`.
fn card_work_progress_into(
    out: &mut Vec<SpriteDraw>,
    x: i32,
    progress_t: u16,
    stage_origin: (i32, i32),
    stage_scale: u32,
) {
    use legaia_asset::title_pak::{SAVE_MENU_ATLAS_PROGRESS_TEXEL, SAVE_MENU_ATLAS_PROGRESS_TUBE};
    let t = i32::from(progress_t.min(0x1000));
    let (fx, fy, fw, fh) = CARD_WORK_FILL;
    let w = (t * fw) >> 12;
    if w > 0 {
        let scale = stage_scale.max(1);
        let (tx, ty, _, _) = SAVE_MENU_ATLAS_PROGRESS_TEXEL;
        let green = ((t * 0xFF) >> 12) as f32 / 128.0;
        // The fill is drawn first: retail links it into the same ordering
        // table bucket after the tube, so the tube's opaque rim lands on it.
        out.push(SpriteDraw {
            dst: (
                stage_origin.0 + (x + fx) * scale as i32,
                stage_origin.1 + fy * scale as i32,
                w as u32 * scale,
                fh as u32 * scale,
            ),
            src: (tx + 1, ty + 1, 2, 2),
            color: [f32::from(CARD_WORK_FILL_RED) / 128.0, green, 0.0, 1.0],
        });
    }
    out.push(save_ui_record_quad(
        SAVE_MENU_ATLAS_PROGRESS_TUBE,
        (0x80, 0x80, 0x80),
        (x + CARD_WORK_TUBE.0, CARD_WORK_TUBE.1),
        stage_origin,
        stage_scale,
    ));
}

/// The write / read panel: `FUN_801E1C1C` mode 4 (PROT 0899
/// `0x801E28EC..`), slid on `_DAT_801F01CC` from x `576` to the stage centre
/// at a fixed `y = 0x50`. Its box is `FUN_801E36C4(x, 0x50, 0x11C, 0x20)`
/// (`0x801E2BAC..0x801E2BB8`); the message, copied behind a two-space lead,
/// is centred at `x + 0x1A` on `y = 0x50` and "Do not remove MEMORY CARD" at
/// `x` on `y = 0x60` (`0x801E2B88..0x801E2BA8`), each `+7` to the glyph top.
pub const CARD_WORK_PANEL: (i32, i32, i32, i32) = (0x50, 0x11C, 0x20, 0x1A);
/// The write panel's slide start x (`(576, 112) -> (160, 112)`).
pub const CARD_WORK_SLIDE_START_X: i32 = 576;
/// The result line: `FUN_801E3EE0(msg, 0xA0, 0x60)` and a box
/// `FUN_801E36C4(0xA0, 0x60, 13 * ((len + 1) / 2), 0xD)` around it
/// (`0x801DF934..0x801DF9B0`) - the width is the text drawer's own
/// `(strlen + 1) / 2` return times 13.
pub const CARD_RESULT_Y: i32 = 0x60;

/// Sprites + text for [`CardBannerView`], at retail's geometry. `block` is
/// the picked block (the grid cell): the write / read panel carries its
/// `No.` badge, as `FUN_801E1C1C` mode 4 draws it.
pub fn card_banner_draws_for(
    font: &legaia_font::Font,
    rects: Option<&SaveMenuAtlasRects>,
    banner: &CardBannerView<'_>,
    block: Option<u8>,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> (Vec<SpriteDraw>, Vec<TextDraw>) {
    let mut sprites = Vec::new();
    let mut texts = Vec::new();
    if banner.work {
        let (y, w, h, lead) = CARD_WORK_PANEL;
        let x = CARD_WORK_SLIDE_START_X
            + (NOW_CHECKING_SLIDE_TARGET_X - CARD_WORK_SLIDE_START_X) * i32::from(banner.slide_t)
                / 0x1000;
        if let Some(rects) = rects {
            nine_slice_panel_into(
                &mut sprites,
                rects,
                messagebox_rect(x, y, w, h),
                stage_origin,
                stage_scale,
                false,
            );
            card_work_progress_into(
                &mut sprites,
                x,
                banner.progress_t,
                stage_origin,
                stage_scale,
            );
            if let Some(block) = block {
                block_badge_into(
                    &mut sprites,
                    block,
                    (x + CARD_WORK_BADGE.0, CARD_WORK_BADGE.1),
                    stage_origin,
                    stage_scale,
                );
            }
        }
        emit_centered_text(
            &mut texts,
            font,
            banner.lines.0,
            x + lead,
            y + 7,
            stage_origin,
            stage_scale,
        );
        emit_centered_text(
            &mut texts,
            font,
            banner.lines.1,
            x,
            y + 0x10 + 7,
            stage_origin,
            stage_scale,
        );
    } else {
        let x = DIALOG_TEXT_CENTER_X;
        let half = (banner.lines.0.len() as i32 + 1) / 2;
        if let Some(rects) = rects {
            nine_slice_panel_into(
                &mut sprites,
                rects,
                messagebox_rect(x, CARD_RESULT_Y, 13 * half, 0xD),
                stage_origin,
                stage_scale,
                false,
            );
        }
        emit_centered_text(
            &mut texts,
            font,
            banner.lines.0,
            x,
            CARD_RESULT_Y + 7,
            stage_origin,
            stage_scale,
        );
    }
    (sprites, texts)
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

/// Geometry of retail's messagebox panel drawer
/// `FUN_801E36C4(center_x, y, w, h)`: it forwards
/// `func_0x8002c69c(center_x - w/2 - 2, y + 6, w, h)` (see
/// `ghidra/scripts/funcs/overlay_save_ui_select_801e36c4.txt`), and the box
/// emitter inflates its centre rect by a uniform **8px** on every side -
/// the same inflation the dialog reading box uses. So the drawn 9-slice
/// footprint is:
///
/// ```text
/// footprint = (center_x - w/2 - 10, y - 2, w + 16, h + 16)
/// ```
///
/// GP0-dump-pinned on two panels at once: the header tab
/// `(48, 6, 65, 13)` predicts `(6, 4, 81, 29)` = exactly the Load panel's
/// 14-sprite composition, and the parked "Now checking" dialog
/// `(160, 97, 169, 26)` predicts `(66, 95, 185, 42)` = the live dump's
/// edge-tile extents. (An earlier `+14 / -9 / -1` model, measured off
/// gold-border pixel scans, was 1px short on every side - the outermost
/// tile ring reads as background in a framebuffer scan.)
fn messagebox_rect(center_x: i32, y: i32, w: i32, h: i32) -> (i32, i32, i32, i32) {
    (center_x - w / 2 - 10, y - 2, w + 16, h + 16)
}

/// The confirm dialog is **two** panels, not one: a near-full-width prompt bar
/// and a small box below it holding the stacked `Yes` / `No` rows. Both come
/// from `FUN_801E1C1C` mode 3, whose panel calls were traced live on a parked
/// prompt as `FUN_801E36C4(160, y, 284, 13)` and
/// `FUN_801E36C4(160, y + 32, 42, 26)`.
const CONFIRM_PROMPT_PANEL: (i32, i32) = (284, 13);
/// Companion to [`CONFIRM_PROMPT_PANEL`]: the `Yes`/`No` box below it.
const CONFIRM_OPTIONS_PANEL: (i32, i32) = (42, 26);
/// Offset from the dialog's slide y to the options box's y (mode 3's
/// `param_4 + 0x20`).
const CONFIRM_OPTIONS_DY: i32 = 32;
/// Retail's centring text emitter `FUN_801E3EE0(text, x, y)` draws glyphs at
/// `(x - width/2, y + 7)`; the `+7` is baked into the emitter
/// (GP0-dump-corroborated by the "No data" caption at
/// `local_34 + 0x18 + 7` and the confirm prompt at `slide_y + 7`).
const DIALOG_TEXT_BASELINE_DY: i32 = 7;
/// The prompt is centred at `param_3 + 0x1a`, right of the dialog's centre -
/// the left of the bar carries the `No.NN` block badge.
const CONFIRM_PROMPT_CENTER_DX: i32 = 26;
/// Both option rows are centred at `param_3 + 4` (they are stacked, not
/// flanking), at `param_4 + 0x20` and `param_4 + 0x30`.
const CONFIRM_OPTION_CENTER_DX: i32 = 4;
/// `Yes` row offset from the slide y (mode 3's `param_4 + 0x20`).
const CONFIRM_OPTION_YES_DY: i32 = 32;
/// `No` row offset from the slide y (mode 3's `param_4 + 0x30`).
const CONFIRM_OPTION_NO_DY: i32 = 48;

/// Prompt-bar rect of the confirm dialog for a given slide `y`.
/// At the parked `y = 88` this is `(8, 86, 300, 29)` (the gold border
/// scan reads the ring one pixel inside the footprint).
fn confirm_prompt_rect(slide_y: i32) -> (i32, i32, i32, i32) {
    let (w, h) = CONFIRM_PROMPT_PANEL;
    messagebox_rect(CONFIRM_DIALOG_CENTER_X, slide_y, w, h)
}

/// `Yes`/`No` box rect of the confirm dialog for a given slide `y`.
/// At the parked `y = 88` this is `(129, 118, 58, 42)`.
fn confirm_options_rect(slide_y: i32) -> (i32, i32, i32, i32) {
    let (w, h) = CONFIRM_OPTIONS_PANEL;
    messagebox_rect(CONFIRM_DIALOG_CENTER_X, slide_y + CONFIRM_OPTIONS_DY, w, h)
}

/// Build the [`SpriteDraw`]s for the confirm dialog's two 9-slice panels.
/// `slide_y` is the dialog's live y (interpolate
/// [`CONFIRM_DIALOG_SLIDE_START_Y`] -> [`CONFIRM_DIALOG_SLIDE_TARGET_Y`]
/// against the session's slide timer; pass the target for the static case).
pub fn confirm_dialog_panel_draws_for(
    rects: &SaveMenuAtlasRects,
    slide_y: i32,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let mut out = Vec::with_capacity(32);
    for rect in [confirm_prompt_rect(slide_y), confirm_options_rect(slide_y)] {
        nine_slice_panel_into(&mut out, rects, rect, stage_origin, stage_scale, false);
    }
    out
}

/// The confirm prompt's block badge: `FUN_801E1C1C` mode 3 seats the
/// numeral at `s5 = 0xA0 - 0x5A` - or `0xA0 - 0x3A` on the one arm that asks
/// "Do you wish to save?", whose shorter line leaves the badge closer
/// (`0x801E25B8` / `0x801E25BC`) - and `No.` `0x16` left of it, both at the
/// slide `y + 4` (`0x801E262C..0x801E2630`). The prompt choice and the shift
/// are one branch on retail, so the prompt text selects the shift here.
pub fn confirm_dialog_badge_draws_for(
    prompt: &str,
    block: u8,
    slide_y: i32,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let numeral_x = CONFIRM_DIALOG_CENTER_X
        + if prompt == CONFIRM_PROMPT_FREE_BLOCK {
            -0x3A
        } else {
            -0x5A
        };
    block_badge_draws_for(
        block,
        (numeral_x - BLOCK_BADGE_DIGIT_DX, slide_y + 4),
        stage_origin,
        stage_scale,
    )
}

/// The free-block question - the one confirm whose badge sits `0x20` right.
pub const CONFIRM_PROMPT_FREE_BLOCK: &str = "Do you wish to save?";

/// Build the [`TextDraw`]s for the confirm dialog: the `prompt` across the
/// bar, then `Yes` and `No` **stacked** in the box below it. Retail centres
/// each line with `FUN_801E3EE0(text, x, y)` (glyphs at `x - width/2`,
/// `y + 7`), the prompt at the dialog's centre `+26` and both option rows at
/// centre `+4`. `cursor` selects the highlighted option (0 = Yes, 1 = No).
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

    // Retail's emitter centres on the passed x and drops the glyphs 7px:
    // (x - width/2, y + 7).
    let centered_at = |font: &legaia_font::Font, text: &str, center_x: i32| {
        center_x - (font.layout_ascii(text).advance_x as i32 / 2)
    };
    emit(
        prompt,
        centered_at(
            font,
            prompt,
            CONFIRM_DIALOG_CENTER_X + CONFIRM_PROMPT_CENTER_DX,
        ),
        slide_y + DIALOG_TEXT_BASELINE_DY,
        SAVE_SELECT_TITLE_COLOR,
    );

    // Yes over No, both centred on the same x inside the small box; the
    // picked one takes the bright menu ink, the other stays dim.
    let dim: [f32; 4] = [
        SAVE_SELECT_TITLE_COLOR[0] * 0.55,
        SAVE_SELECT_TITLE_COLOR[1] * 0.55,
        SAVE_SELECT_TITLE_COLOR[2] * 0.55,
        1.0,
    ];
    let option_center_x = CONFIRM_DIALOG_CENTER_X + CONFIRM_OPTION_CENTER_DX;
    for (i, (opt, dy)) in [("Yes", CONFIRM_OPTION_YES_DY), ("No", CONFIRM_OPTION_NO_DY)]
        .iter()
        .enumerate()
    {
        let color = if i as u8 == cursor {
            SAVE_SELECT_TITLE_COLOR
        } else {
            dim
        };
        emit(
            opt,
            centered_at(font, opt, option_center_x),
            slide_y + dy + DIALOG_TEXT_BASELINE_DY,
            color,
        );
    }
    out
}

/// Panel the **refusal notice** uses: the confirm dialog's prompt bar
/// without its `Yes`/`No` box, because the notice takes no answer beyond an
/// acknowledgement.
const REFUSAL_PANEL: (i32, i32) = CONFIRM_PROMPT_PANEL;

/// The y the refusal notice parks at - the confirm dialog's own target, so
/// the box lands where the player's eye already is when the commit fails.
pub const REFUSAL_NOTICE_Y: i32 = CONFIRM_DIALOG_SLIDE_TARGET_Y;

/// Build the [`SpriteDraw`]s for a save-screen **refusal notice** panel.
///
/// This is the port's own screen: retail's save UI answers a card it cannot
/// use through the card driver's result word, and both of this port's hosts
/// carry a second backend whose failures that word does not model. The panel
/// is the confirm dialog's prompt bar, reused so the notice is the shape the
/// player has already seen on this screen.
pub fn save_refusal_panel_draws_for(
    rects: &SaveMenuAtlasRects,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let mut out = Vec::with_capacity(16);
    let (w, h) = REFUSAL_PANEL;
    let rect = messagebox_rect(CONFIRM_DIALOG_CENTER_X, REFUSAL_NOTICE_Y, w, h);
    nine_slice_panel_into(&mut out, rects, rect, stage_origin, stage_scale, false);
    out
}

/// Build the [`TextDraw`]s for a save-screen refusal notice: one line,
/// centred on the dialog's own centre through retail's centring emitter
/// (glyphs at `x - width/2`, `y + 7`).
pub fn save_refusal_text_draws_for(
    font: &legaia_font::Font,
    message: &str,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<TextDraw> {
    let scale = stage_scale.max(1);
    let layout = font.layout_ascii(message);
    let left_x = CONFIRM_DIALOG_CENTER_X - (layout.advance_x as i32 / 2);
    let top_y = REFUSAL_NOTICE_Y + DIALOG_TEXT_BASELINE_DY;
    let mut out = Vec::with_capacity(layout.glyphs.len());
    for g in &layout.glyphs {
        out.push(TextDraw {
            dst: (
                stage_origin.0 + (left_x + g.dst_x) * scale as i32,
                stage_origin.1 + (top_y + g.dst_y) * scale as i32,
                g.width * scale,
                g.height * scale,
            ),
            src: (g.atlas_x, g.atlas_y, g.width, g.height),
            color: SAVE_SELECT_TITLE_COLOR,
        });
    }
    out
}

#[cfg(test)]
mod refusal_notice_tests {
    use super::*;

    #[test]
    fn the_notice_reuses_the_confirm_prompt_bar() {
        // Same rect the confirm dialog parks its prompt bar at, so the
        // notice is a shape the player has already seen on this screen.
        assert_eq!(REFUSAL_NOTICE_Y, CONFIRM_DIALOG_SLIDE_TARGET_Y);
        assert_eq!(REFUSAL_PANEL, CONFIRM_PROMPT_PANEL);
    }

    #[test]
    fn the_message_becomes_glyph_quads_centred_on_the_dialog() {
        let font = legaia_font::synthetic_for_tests();
        let msg = "This MEMORY CARD cannot be written.";
        let out = save_refusal_text_draws_for(&font, msg, (0, 0), 1);
        assert!(!out.is_empty());
        let left = out.iter().map(|d| d.dst.0).min().unwrap();
        let right = out.iter().map(|d| d.dst.0 + d.dst.2 as i32).max().unwrap();
        // Centred on the dialog centre, within a glyph's width.
        let centre = (left + right) / 2;
        assert!((centre - CONFIRM_DIALOG_CENTER_X).abs() <= 8, "{centre}");
    }

    #[test]
    fn the_stage_transform_carries_the_notice_with_it() {
        let font = legaia_font::synthetic_for_tests();
        let a = save_refusal_text_draws_for(&font, "no", (0, 0), 1);
        let b = save_refusal_text_draws_for(&font, "no", (7, 9), 3);
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(y.dst.0, 7 + x.dst.0 * 3);
            assert_eq!(y.dst.1, 9 + x.dst.1 * 3);
        }
    }
}

#[cfg(test)]
mod messagebox_geometry_tests {
    use super::*;

    /// The panel drawer's geometry, checked against the live GP0 draw
    /// list. Each case is `FUN_801E36C4(center_x, y, w, h)` args traced
    /// live off the running game, paired with the 9-slice footprint the
    /// GPU actually received.
    #[test]
    fn messagebox_rect_matches_retail_captures() {
        // The Load/Save header tab (slide mode 1, held at (48, 6)):
        // the model must predict the Load panel's byte-pinned 14-sprite
        // composition at (6, 4) size 81x29.
        assert_eq!(messagebox_rect(48, 6, 65, 13), (6, 4, 81, 29));

        // "Now checking" (mode 0) mid-slide, center_x = 240. The archived
        // capture that pinned this dialog caught it part-way in; its
        // gold-border scan read left = 147 = one pixel inside the
        // footprint the model predicts.
        assert_eq!(messagebox_rect(240, 97, 169, 26), (146, 95, 185, 42));

        // The same dialog parked (center_x = 160): the live GP0 dump has
        // edge tiles at x = 66 / 247 and rows 95..137.
        assert_eq!(messagebox_rect(160, 97, 169, 26), (66, 95, 185, 42));
    }

    /// The published NowChecking constants must BE the parked rect, and the
    /// slide-offset path must reproduce the mid-slide capture: the caller
    /// passes `center_x - 160` as the x offset, so `center_x = 240` has to
    /// land the footprint at 146 (gold border at 147).
    #[test]
    fn now_checking_constants_match_the_capture() {
        assert_eq!(
            (
                NOW_CHECKING_PANEL_POS,
                (
                    NOW_CHECKING_PANEL_SIZE.0 as i32,
                    NOW_CHECKING_PANEL_SIZE.1 as i32
                )
            ),
            ((66, 95), (185, 42))
        );
        let mid_slide_center_x = 240;
        let slide_offset_x = mid_slide_center_x - NOW_CHECKING_SLIDE_TARGET_X;
        assert_eq!(NOW_CHECKING_PANEL_POS.0 + slide_offset_x, 146);
    }

    /// The confirm dialog is two panels, both riding the mode-3 slide.
    /// Footprints at the parked rest position (slide target y = 88).
    #[test]
    fn confirm_dialog_panels_match_parked_capture() {
        let y = CONFIRM_DIALOG_SLIDE_TARGET_Y;
        assert_eq!(confirm_prompt_rect(y), (8, 86, 300, 29));
        assert_eq!(confirm_options_rect(y), (129, 118, 58, 42));
    }

    /// Both panels ride the slide together: at the start of the slide the
    /// dialog is below the 240-line stage, which is what makes it slide *up*
    /// into view.
    #[test]
    fn confirm_dialog_panels_start_offstage() {
        let y = CONFIRM_DIALOG_SLIDE_START_Y;
        let (_, prompt_top, _, _) = confirm_prompt_rect(y);
        let (_, options_top, _, _) = confirm_options_rect(y);
        assert!(prompt_top >= 240, "prompt starts below the stage");
        assert!(options_top > prompt_top, "options box rides below the bar");
    }

    /// Retail stacks Yes over No at a single centre inside the small box -
    /// it does not flank the dialog centre. Pinning this because the box is
    /// only 42px wide: any flanking layout would sit outside its own panel.
    #[test]
    fn confirm_options_are_stacked_at_one_centre() {
        let font = legaia_font::synthetic_for_tests();
        let draws = confirm_dialog_text_draws_for(
            &font,
            "Do you wish to save?",
            0,
            CONFIRM_DIALOG_SLIDE_TARGET_Y,
            (0, 0),
            1,
        );
        assert!(!draws.is_empty());

        // Group glyph rows by y; the two option rows must be 16px apart (mode
        // 3's +0x20 / +0x30) and share a horizontal span.
        let row_of = |dy: i32| -> Vec<&TextDraw> {
            let want = CONFIRM_DIALOG_SLIDE_TARGET_Y + dy + DIALOG_TEXT_BASELINE_DY;
            draws.iter().filter(|d| d.dst.1 == want).collect()
        };
        let yes = row_of(CONFIRM_OPTION_YES_DY);
        let no = row_of(CONFIRM_OPTION_NO_DY);
        assert!(!yes.is_empty(), "Yes row present at slide_y+32+7");
        assert!(!no.is_empty(), "No row present at slide_y+48+7");

        let left = |row: &[&TextDraw]| row.iter().map(|d| d.dst.0).min().unwrap();
        // Both rows centre on the same x, so their left edges differ only by
        // the two words' width difference - they never straddle the centre.
        let (yes_l, no_l) = (left(&yes), left(&no));
        assert!(
            (yes_l - no_l).abs() < 12,
            "Yes/No are stacked at one centre, got left x {yes_l} vs {no_l}"
        );

        // And both sit inside the options panel they are drawn in.
        let (px, _, pw, _) = confirm_options_rect(CONFIRM_DIALOG_SLIDE_TARGET_Y);
        for d in yes.iter().chain(no.iter()) {
            assert!(
                d.dst.0 >= px && d.dst.0 < px + pw,
                "option glyph at x={} escapes the options panel {px}..{}",
                d.dst.0,
                px + pw
            );
        }
    }
}

#[cfg(test)]
mod card_work_sprite_tests {
    use super::*;
    use legaia_asset::title_pak::{
        SAVE_MENU_ATLAS_NO_LABEL, SAVE_MENU_ATLAS_PROGRESS_TUBE, save_menu_atlas_block_digit,
    };

    /// The parked write panel (`x = 0xA0`): `No.` at `(0x4B, 0x54)`, the
    /// numeral `0x16` right of it, the tube at `(0x6C, 0x90)` and the fill
    /// at `(0x74, 0x95)` - `t * 0x58 >> 12` wide, so 44 px at half time.
    #[test]
    fn the_parked_write_panel_places_retail_badge_tube_and_fill() {
        let mut out = Vec::new();
        card_work_progress_into(&mut out, 0xA0, 0x800, (0, 0), 1);
        block_badge_into(&mut out, 4, (0xA0 - 0x55, 0x54), (0, 0), 1);
        let fill = out[0];
        assert_eq!(fill.dst, (0x74, 0x95, 44, 6));
        assert!((fill.color[0] - 0xBC as f32 / 128.0).abs() < 1e-6);
        assert!((fill.color[1] - (0x7F as f32 / 128.0)).abs() < 1e-6);
        assert_eq!(fill.color[2], 0.0);
        assert_eq!(out[1].src, SAVE_MENU_ATLAS_PROGRESS_TUBE);
        assert_eq!((out[1].dst.0, out[1].dst.1), (0x6C, 0x90));
        assert_eq!(out[2].src, SAVE_MENU_ATLAS_NO_LABEL);
        assert_eq!((out[2].dst.0, out[2].dst.1), (0x4B, 0x54));
        assert_eq!(out[3].src, save_menu_atlas_block_digit(4).unwrap());
        assert_eq!((out[3].dst.0, out[3].dst.1), (0x61, 0x54));
    }

    /// No fill quad before the timer has moved; the full bar is the tube's
    /// 88-pixel interior.
    #[test]
    fn the_fill_grows_from_nothing_to_the_tube_interior() {
        let mut out = Vec::new();
        card_work_progress_into(&mut out, 0xA0, 0, (0, 0), 1);
        assert_eq!(out.len(), 1, "tube only");
        out.clear();
        card_work_progress_into(&mut out, 0xA0, 0x1000, (0, 0), 1);
        assert_eq!(out[0].dst.2, 88);
    }

    /// The confirm's badge: numeral at `0xA0 - 0x5A`, or `0xA0 - 0x3A` on the
    /// free-block "save?" question, `No.` `0x16` left, both at `y + 4`.
    #[test]
    fn the_confirm_badge_shifts_on_the_free_block_question() {
        let y = CONFIRM_DIALOG_SLIDE_TARGET_Y;
        let over = confirm_dialog_badge_draws_for("Do you wish to overwrite?", 2, y, (0, 0), 1);
        assert_eq!((over[1].dst.0, over[1].dst.1), (0xA0 - 0x5A, y + 4));
        assert_eq!(over[0].dst.0, 0xA0 - 0x5A - 0x16);
        let free = confirm_dialog_badge_draws_for(CONFIRM_PROMPT_FREE_BLOCK, 2, y, (0, 0), 1);
        assert_eq!(free[1].dst.0, 0xA0 - 0x3A);
    }
}

use crate::*;

// ---------------------------------------------------------------------------
// Name-entry overlay - retail-traced geometry.
//
// Every constant below is in 320x240 PSX-framebuffer "stage" pixels, pinned
// against the live retail draw stream (GP0 ring dump of the town01 opening
// name-entry screen) and the renderer `FUN_801E6B34`:
//
// - overlay base `(32, 99)` (the op-0x49 context's `+0xA/+0xC` pen);
// - grid window: 9-slice footprint `(24, 91, 272, 120)` (4 px border band,
//   filigree fill), the same pause-menu window skin family;
// - name-field window: footprint `(196, 71, 88, 28)` = the renderer's
//   `FUN_8002C69C(base_x + 0xAC, base_y - 0x14, 0x48, 0xC)` centre rect
//   inflated by the 8 px skin border;
// - glyph grid: origin `base + (4, 4)` = `(36, 103)`, 15 px column pitch,
//   14 px row pitch, ink 7 white;
// - control bar at `y = base + 92 = 191`: "BS" at `x = 36`, the quoted
//   default name at `x = 148`, "Select" at `x = 244`, ink 6 gold;
// - working name at `(208, 79)` ink 7 white, teal `_` caret 6 px after the
//   name (ink 5), 75%-duty blink (`frame & 0x18`);
// - hand cursor 16x16: grid cell -> `(16 + col*15, 101 + row*14)`; control
//   anchors -> `x = {16, 128, 224}, y = 189`;
// - prompt "Select your name." at `(176, 32)` white; the confirm state
//   replaces it with "Is this name okay?" at `(172, 24)` plus stacked teal
//   "Yes" `(204, 38)` / "No" `(204, 50)` rows, hand at `x = 174` on the
//   selected row (opens on No).
// ---------------------------------------------------------------------------

/// Overlay base pen (the op-0x49 context `+0xA/+0xC`).
pub const NAME_ENTRY_BASE: (i32, i32) = (32, 99);
/// Grid-window 9-slice footprint (outer rect incl. the 4 px border band).
pub const NAME_ENTRY_GRID_WINDOW: (i32, i32, i32, i32) = (24, 91, 272, 120);
/// Name-field window 9-slice footprint.
pub const NAME_ENTRY_NAME_WINDOW: (i32, i32, i32, i32) = (196, 71, 88, 28);
/// Glyph-grid origin (`base + (4, 4)`).
pub const NAME_ENTRY_GRID_ORIGIN: (i32, i32) = (NAME_ENTRY_BASE.0 + 4, NAME_ENTRY_BASE.1 + 4);
/// Grid column pitch (`FUN_801E6B34`'s `col * 0xF`).
pub const NAME_ENTRY_COL_PITCH: i32 = 15;
/// Grid row pitch (`row * 0xE`).
pub const NAME_ENTRY_ROW_PITCH: i32 = 14;
/// Control-bar label x offsets from base (+4 pen shift applied), traced from
/// the `0x801F2B2C` position table `(0, 92) (112, 92) (208, 92)`.
pub const NAME_ENTRY_CONTROL_XS: [i32; 3] = [36, 148, 244];
/// Control-bar label baseline y (`base_y + 0x5C`).
pub const NAME_ENTRY_CONTROL_Y: i32 = NAME_ENTRY_BASE.1 + 92;
/// Working-name pen (`base + (0xB0, -0x14)`).
pub const NAME_ENTRY_NAME_PEN: (i32, i32) = (NAME_ENTRY_BASE.0 + 0xB0, NAME_ENTRY_BASE.1 - 0x14);
/// Editing-state prompt pen.
pub const NAME_ENTRY_PROMPT_PEN: (i32, i32) = (176, 32);
/// Confirm-state prompt pen.
pub const NAME_ENTRY_CONFIRM_PEN: (i32, i32) = (172, 24);
/// Confirm "Yes" row pen (the "No" row sits one 12 px pitch below).
pub const NAME_ENTRY_YES_PEN: (i32, i32) = (204, 38);
/// Confirm option row pitch.
pub const NAME_ENTRY_OPTION_PITCH: i32 = 12;

/// Renderer-agnostic view of the name-entry overlay (engine-core's
/// `name_entry::NameEntry` projected to primitives, so this crate stays
/// decoupled from engine-core). Built per frame by the shell.
pub struct NameEntryView<'a> {
    /// The six selectable glyph rows (`|` marks a non-selectable separator).
    pub grid_rows: &'a [&'a str],
    /// Working name buffer.
    pub name: &'a str,
    /// The template default name (the middle control's quoted label).
    pub default_name: &'a str,
    /// Highlighted glyph cell `(row, col)` when the cursor is in the grid.
    pub grid_cursor: Option<(usize, usize)>,
    /// Highlighted control button (0 = BS, 1 = default, 2 = Select) when the
    /// cursor is on the bar.
    pub control_cursor: Option<usize>,
    /// `true` while the "Is this name okay?" Yes/No prompt is showing.
    pub confirming: bool,
    /// Yes/No selection during the confirm prompt (`true` = Yes; retail
    /// opens on No).
    pub confirm_yes: bool,
    /// Caret blink state (the trailing `_` after the name; retail blinks it
    /// at 75% duty from `frame & 0x18`).
    pub caret_on: bool,
}

/// Build the name-entry overlay's [`TextDraw`]s in **stage pixels** (320x240
/// framebuffer space). The caller scales them into the surface with
/// [`scale_stage_text_draws`], the same transform the dialog box uses -
/// keeping text and the sprite chrome locked together.
///
/// Layout is the traced retail geometry (module doc): grid glyphs ink-7
/// white at 15/14 px pitch, gold control-bar labels, the working name +
/// blinking teal caret in the name-field box, and either the "Select your
/// name." prompt (editing) or "Is this name okay?" + stacked teal Yes/No
/// rows (confirming). The hand cursor and window chrome are sprites - see
/// [`name_entry_chrome_sprite_draws_for`].
///
/// PORT: FUN_801E6B34
pub fn name_entry_draws_for(font: &legaia_font::Font, view: &NameEntryView<'_>) -> Vec<TextDraw> {
    let white = MENU_TEXT_WHITE;
    let gold = MENU_TEXT_GOLD;
    let teal = MENU_TEXT_TEAL;

    let mut out = Vec::new();

    // Prompt line(s). Confirming replaces the select prompt entirely
    // (retail draws the confirm text + Yes/No where the prompt line was).
    if view.confirming {
        out.extend(text_draws_for(
            &font.layout_ascii("Is this name okay?"),
            NAME_ENTRY_CONFIRM_PEN,
            white,
        ));
        out.extend(text_draws_for(
            &font.layout_ascii("Yes"),
            NAME_ENTRY_YES_PEN,
            teal,
        ));
        out.extend(text_draws_for(
            &font.layout_ascii("No"),
            (
                NAME_ENTRY_YES_PEN.0,
                NAME_ENTRY_YES_PEN.1 + NAME_ENTRY_OPTION_PITCH,
            ),
            teal,
        ));
    } else {
        out.extend(text_draws_for(
            &font.layout_ascii("Select your name."),
            NAME_ENTRY_PROMPT_PEN,
            white,
        ));
    }

    // Working name (always shown; the caret only while editing).
    out.extend(text_draws_for(
        &font.layout_ascii(view.name),
        NAME_ENTRY_NAME_PEN,
        white,
    ));
    if !view.confirming && view.caret_on {
        let name_w = font.layout_ascii(view.name).advance_x as i32;
        // Retail draws the `_` caret 6 px after the name, ink 5 teal, only
        // while it still fits the 57 px name field.
        if name_w + 6 < 0x39 + 6 {
            out.extend(text_draws_for(
                &font.layout_ascii("_"),
                (NAME_ENTRY_NAME_PEN.0 + name_w + 6, NAME_ENTRY_NAME_PEN.1),
                teal,
            ));
        }
    }

    // Character grid: ink-7 white, skipping `|` separators and blank cells
    // (retail draws nothing for the selectable space cells).
    for (r, row) in view.grid_rows.iter().enumerate() {
        for (c, ch) in row.bytes().enumerate() {
            if ch == b'|' || ch == b' ' {
                continue;
            }
            let x = NAME_ENTRY_GRID_ORIGIN.0 + c as i32 * NAME_ENTRY_COL_PITCH;
            let y = NAME_ENTRY_GRID_ORIGIN.1 + r as i32 * NAME_ENTRY_ROW_PITCH;
            out.extend(text_draws_for(
                &font.layout_ascii(&(ch as char).to_string()),
                (x, y),
                white,
            ));
        }
    }

    // Control bar: gold labels "BS" / `` `<default>' `` / "Select" (the
    // middle label is the quoted template name - the retail button restores
    // it).
    let quoted = format!("`{}'", view.default_name);
    let labels: [&str; 3] = ["BS", &quoted, "Select"];
    for (i, label) in labels.iter().enumerate() {
        out.extend(text_draws_for(
            &font.layout_ascii(label),
            (NAME_ENTRY_CONTROL_XS[i], NAME_ENTRY_CONTROL_Y),
            gold,
        ));
    }

    out
}

/// Build the name-entry overlay's sprite chrome: the two filigree-filled
/// 9-slice windows (grid + name field, the pause-menu skin family) and the
/// pointing-hand cursor, positioned per the traced retail geometry. Layered
/// under the text exactly like the dialog chrome.
///
/// `stage_origin` / `stage_scale` are the shared boot-UI stage transform.
///
/// REF: FUN_801E6B34
/// REF: FUN_8002C69C
pub fn name_entry_chrome_sprite_draws_for(
    rects: &SaveMenuAtlasRects,
    view: &NameEntryView<'_>,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let mut out =
        menu_window_chrome_draws_for(rects, NAME_ENTRY_GRID_WINDOW, stage_origin, stage_scale);
    out.extend(menu_window_chrome_draws_for(
        rects,
        NAME_ENTRY_NAME_WINDOW,
        stage_origin,
        stage_scale,
    ));

    // Hand cursor. Grid cell: `(cell_x - 0x10, cell_y - 2)` with the cell at
    // `base + (col*15, 4 + row*14)`; control bar: the three label anchors at
    // `x = base + {0, 112, 208} - 0x10, y = 189`; confirm: `x = 174` on the
    // selected Yes/No row.
    let hand = if view.confirming {
        let row = if view.confirm_yes { 0 } else { 1 };
        Some((
            NAME_ENTRY_YES_PEN.0 - 30,
            NAME_ENTRY_YES_PEN.1 + row * NAME_ENTRY_OPTION_PITCH,
        ))
    } else if let Some((r, c)) = view.grid_cursor {
        Some((
            NAME_ENTRY_BASE.0 + c as i32 * NAME_ENTRY_COL_PITCH - 0x10,
            NAME_ENTRY_BASE.1 + 4 + r as i32 * NAME_ENTRY_ROW_PITCH - 2,
        ))
    } else {
        view.control_cursor.map(|i| {
            (
                NAME_ENTRY_BASE.0 + [0, 112, 208][i.min(2)] - 0x10,
                NAME_ENTRY_CONTROL_Y - 2,
            )
        })
    };
    if let Some((hx, hy)) = hand {
        let scale = stage_scale.max(1) as i32;
        let (_, _, w, h) = rects.cursor;
        out.push(SpriteDraw {
            dst: (
                stage_origin.0 + hx * scale,
                stage_origin.1 + hy * scale,
                w * stage_scale,
                h * stage_scale,
            ),
            src: rects.cursor,
            color: [1.0, 1.0, 1.0, 1.0],
        });
    }
    out
}

/// Build [`TextDraw`]s for one opening-cutscene narration page (a single
/// subtitle line), horizontally centered at `center_x` with its baseline at
/// `top_y`, both in surface pixels. The host calls this with the active
/// page text from [`legaia_engine_core::cutscene_narration::CutsceneNarration`]
/// each frame; an empty / completed narration draws nothing.
///
/// Centering is computed from the font metrics (`center_x - width / 2`), the
/// same scheme [`now_checking_text_draws_for`] uses for retail-style centered
/// dialog. Subtitles are bottom-anchored by the caller's `top_y`.
pub fn cutscene_narration_draws_for(
    font: &legaia_font::Font,
    text: &str,
    center_x: i32,
    top_y: i32,
    color: [f32; 4],
) -> Vec<TextDraw> {
    if text.is_empty() {
        return Vec::new();
    }
    let layout = font.layout_ascii(text);
    let left_x = center_x - (layout.advance_x as i32 / 2);
    text_draws_for(&layout, (left_x, top_y), color)
}

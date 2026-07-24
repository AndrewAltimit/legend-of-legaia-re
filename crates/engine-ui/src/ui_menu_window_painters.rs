//! Content painters for a block of the menu overlay's **window-descriptor
//! table** (`legaia_asset::menu_windows`, 52 records at overlay VA
//! `0x801E4738`).
//!
//! Each descriptor's `renderer_va` names the routine that fills that
//! window's content rect; the 9-slice frame around it is drawn by the
//! caller, not here (see `docs/subsystems/field-menu.md`). This module
//! ports the painters for the confirm / prompt / counter / options block:
//!
//! | window | routine | content |
//! |---|---|---|
//! | 5 | `FUN_801D61B0` | two-line prompt over a two-row choice |
//! | 6 | `FUN_801D6360` | six-row label list with a corner cursor |
//! | 7 | `FUN_801DCCB4` | one-line prompt with a substituted character |
//! | 24 | `FUN_801DCC20` | count field over a reserved sub-rect |
//! | 31 | `FUN_801DCE20` | label, wide number field, trailing label |
//! | 32 | `FUN_801DCF84` | pictogram + 8-digit counter |
//! | 33 | `FUN_801DCF14` | title tab whose text comes from a record |
//! | 43 | `FUN_801DCFE4` | plain title tab |
//! | 45 | `FUN_801DD028` | second pictogram + 8-digit counter |
//! | 46 | `FUN_801D603C` | one-line prompt over a two-row choice |
//!
//! ## What the port keeps and what it drops
//!
//! Every routine writes the shared draw-order word `DAT_8007B454` (values
//! 5, 6 and 7) before each primitive and some also stage the glyph-advance
//! byte `DAT_80073F20` (`0x10` for the wide pass, `0x0C` for the narrow
//! one). Both are properties of retail's ordering table and its fixed-cell
//! text writer; a host that composites in call order and lays glyphs out
//! through [`legaia_font`] has no use for either, so the port keeps only
//! the **geometry** - the pen positions, the field widths, and the
//! cursor-variant arithmetic.
//!
//! Strings are caller-supplied. The routines resolve theirs from overlay
//! literals and from live records, which are disc bytes; the port takes
//! `&str` so the text can come from the translation layer instead.
//!
//! Evidence: `ghidra/scripts/funcs/overlay_menu_801d61b0.txt`,
//! `overlay_menu_801d6360.txt`, `overlay_menu_801dccb4.txt`,
//! `overlay_menu_801dcc20.txt`, `overlay_menu_801dce20.txt`,
//! `overlay_menu_801dcf84.txt`, `overlay_menu_801dcf14.txt`,
//! `overlay_menu_801dcfe4.txt`, `overlay_menu_801dd028.txt`,
//! `overlay_menu_801d603c.txt` (PROT entry 0899, the menu overlay).

use crate::*;

/// A window's content rect, as the descriptor table stores it.
///
/// The painters read `+0xA`/`+0xC` as the content origin and `+0xE`/`+0x10`
/// as the content extent; those are exactly
/// `legaia_asset::menu_windows::MenuWindowDescriptor::rect`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PainterRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl PainterRect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
}

/// One pointing-hand / marker sprite request (`FUN_8002B994`).
///
/// `sprite` and `variant` are the routine's first two arguments verbatim -
/// the port does not interpret them, because the sprite bank they index is
/// a VRAM resource, not a layout fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PainterSprite {
    pub sprite: u8,
    pub variant: u8,
    pub x: i32,
    pub y: i32,
}

/// One pictogram request (`FUN_8002C488`): a bank id drawn at a pen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PainterPictogram {
    pub id: u8,
    pub x: i32,
    pub y: i32,
}

/// Row pitch the label painters step by (`addiu s0,s0,0xe`).
pub const PAINTER_ROW_PITCH: i32 = 0x0E;

/// Right-hand pictogram id of the primary counter window (id 32).
pub const COUNTER_PICTOGRAM_PRIMARY: u8 = 0x62;
/// Pictogram id of the secondary counter window (id 45).
pub const COUNTER_PICTOGRAM_SECONDARY: u8 = 0x66;
/// Digit width both counter windows request (`li a1,0x8`).
pub const COUNTER_DIGITS: i32 = 8;
/// Horizontal gap between a counter's pictogram and its digit field.
pub const COUNTER_DIGIT_INSET: i32 = 0x28;

/// Advance of one fixed-width digit cell in the number writer.
const NUM_CELL_W: i32 = 8;

/// Right-align `value` into a `digits`-wide fixed cell field.
///
/// The retail number writer (`FUN_80034B78`) takes the field width as its
/// second argument and fills cells from the right, so a short value leaves
/// the leading cells blank rather than shifting left.
fn digits_draws(
    font: &legaia_font::Font,
    value: u64,
    x: i32,
    y: i32,
    digits: i32,
    color: [f32; 4],
) -> Vec<TextDraw> {
    let s = value.to_string();
    let len = s.len() as i32;
    let mut out = Vec::new();
    for (i, ch) in s.chars().enumerate() {
        let cell = (digits - len + i as i32).max(0);
        let l = font.layout_ascii(&ch.to_string());
        out.extend(text_draws_for(&l, (x + cell * NUM_CELL_W, y), color));
    }
    out
}

// ---------------------------------------------------------------------
// Title tabs - windows 43 and 33
// ---------------------------------------------------------------------

/// Window 43: one label at the content origin, nothing else.
///
/// The shortest painter in the table - set the draw order, draw one
/// overlay-literal string at `(WX, WY)`, return.
///
/// PORT: FUN_801DCFE4
pub fn title_tab_draws_for(
    font: &legaia_font::Font,
    rect: PainterRect,
    label: &str,
) -> Vec<TextDraw> {
    let l = font.layout_ascii(label);
    text_draws_for(&l, (rect.x, rect.y), MENU_TEXT_WHITE)
}

/// Window 33: the same single label, but sourced from a live record.
///
/// `FUN_801DCF14` reads the record pointer `DAT_8007B450`, takes the byte
/// at `+2` as a skip count and starts the string at `record + 3 + skip` -
/// i.e. the name is stored behind a short variable-length prefix. The port
/// takes the resolved `&str`; [`title_record_text_offset`] is the offset
/// arithmetic on its own, so a host reading the record can reuse it.
///
/// The routine also saves the glyph-advance byte, forces it to the wide
/// `0x10` cell for this draw and restores it afterwards. The port lays the
/// label out proportionally, which is the engine-wide choice.
///
/// PORT: FUN_801DCF14
pub fn record_title_tab_draws_for(
    font: &legaia_font::Font,
    rect: PainterRect,
    label: &str,
) -> Vec<TextDraw> {
    let l = font.layout_ascii(label);
    text_draws_for(&l, (rect.x, rect.y), MENU_TEXT_WHITE)
}

/// Where window 33's title text starts inside the `DAT_8007B450` record.
///
/// REF: FUN_801DCF14 (`lbu v0,0x2(a1); addiu v0,v0,0x3; addu a0,a1,v0`)
pub fn title_record_text_offset(skip_byte: u8) -> usize {
    skip_byte as usize + 3
}

// ---------------------------------------------------------------------
// Counter windows - 32 and 45
// ---------------------------------------------------------------------

/// Windows 32 and 45: a pictogram plus an 8-digit right-aligned counter.
///
/// Both routines are the same shape and differ only in the pictogram id
/// and which global counter they read - `0x8008459C` for window 32,
/// `0x800845A4` for window 45. The pictogram sits two pixels below the
/// content origin and the digit field starts `0x28` to its right.
///
/// PORT: FUN_801DCF84
/// PORT: FUN_801DD028
pub fn counter_panel_draws_for(
    font: &legaia_font::Font,
    rect: PainterRect,
    pictogram: u8,
    value: u64,
) -> (Vec<TextDraw>, PainterPictogram) {
    let digits = digits_draws(
        font,
        value,
        rect.x + COUNTER_DIGIT_INSET,
        rect.y,
        COUNTER_DIGITS,
        MENU_TEXT_WHITE,
    );
    (
        digits,
        PainterPictogram {
            id: pictogram,
            x: rect.x,
            y: rect.y + 2,
        },
    )
}

// ---------------------------------------------------------------------
// Prompt windows - 7 and 31
// ---------------------------------------------------------------------

/// Window 7: one prompt line with a single substituted character, plus the
/// confirm cursor in the bottom-right corner.
///
/// `FUN_801DCCB4` patches byte `+1` of a small scratch string at
/// `0x801E46E4` with a byte lifted out of the `0x80084140` record block
/// (record stride `0x414`, field `+0x705`, indexed by the two `i16`
/// selectors at `0x8007BB70` and `0x8007BB78`) and then draws that string.
/// So the prompt is one fixed sentence with one live character in it - the
/// port takes the whole assembled line.
///
/// The cursor lands at a fixed inset from the content origin, not from the
/// window extent: `(WX + 0xE6, WY + 0xD)`.
///
/// PORT: FUN_801DCCB4
pub fn char_prompt_draws_for(
    font: &legaia_font::Font,
    rect: PainterRect,
    line: &str,
) -> (Vec<TextDraw>, PainterSprite) {
    let l = font.layout_ascii(line);
    (
        text_draws_for(&l, (rect.x, rect.y), MENU_TEXT_WHITE),
        PainterSprite {
            sprite: 1,
            variant: 1,
            x: rect.x + 0xE6,
            y: rect.y + 0x0D,
        },
    )
}

/// Window 31: a heading, then a wide number field with a trailing label on
/// the row below, then the same corner cursor as window 7.
///
/// `FUN_801DCE20` draws the heading at the content origin, drops one row
/// pitch, writes the `0x800845B4` counter as an 8-digit field at the left
/// margin and puts the unit label `0x40` to its right. The number is
/// emitted one draw-order step below the text, which is the only place in
/// this block where the two differ.
///
/// PORT: FUN_801DCE20
pub fn amount_prompt_draws_for(
    font: &legaia_font::Font,
    rect: PainterRect,
    heading: &str,
    value: u64,
    unit_label: &str,
) -> (Vec<TextDraw>, PainterSprite) {
    let row1 = rect.y + PAINTER_ROW_PITCH;
    let mut out = text_draws_for(
        &font.layout_ascii(heading),
        (rect.x, rect.y),
        MENU_TEXT_WHITE,
    );
    out.extend(digits_draws(
        font,
        value,
        rect.x,
        row1,
        COUNTER_DIGITS,
        MENU_TEXT_WHITE,
    ));
    out.extend(text_draws_for(
        &font.layout_ascii(unit_label),
        (rect.x + 0x40, row1),
        MENU_TEXT_WHITE,
    ));
    (
        out,
        PainterSprite {
            sprite: 1,
            variant: 1,
            x: rect.x + 0xE6,
            y: rect.y + 0x0D,
        },
    )
}

/// Window 24: a two-digit count field, drawn only when the selection index
/// is live, over a reserved sub-rect.
///
/// `FUN_801DCC20` guards the whole text pass on `DAT_801E46B0 > 0`; the
/// trailing `FUN_8002C69C` box at `(WX, WY + 0x38)` sized `0x90 x 0x28` is
/// emitted either way, which is why an empty selection still reserves the
/// space. The count itself comes back from `FUN_80042F4C`, a lookup on the
/// selection index, and lands at `WX + 0x80`.
///
/// PORT: FUN_801DCC20
pub fn count_panel_draws_for(
    font: &legaia_font::Font,
    rect: PainterRect,
    selection: Option<u64>,
) -> (Vec<TextDraw>, (i32, i32, i32, i32)) {
    let reserved = (rect.x, rect.y + 0x38, 0x90, 0x28);
    let draws = match selection {
        Some(count) => digits_draws(font, count, rect.x + 0x80, rect.y, 2, MENU_TEXT_WHITE),
        None => Vec::new(),
    };
    (draws, reserved)
}

// ---------------------------------------------------------------------
// Options / choice windows - 46 and 5
// ---------------------------------------------------------------------

/// The choice-state word both two-row option windows branch on
/// (`DAT_801E46D0`).
///
/// The bit layout, read off the branch chain the two routines share:
///
/// * `0x4000` - suppress every marker. Set while the group is inert.
/// * `0x2000` - the group is mid-change. The marker variant then depends
///   only on `0x1000`: clear gives variant `4`, set gives variant `0`, and
///   **both rows draw** because this arm never compares the row index.
/// * otherwise - the low 12 bits are the selected row index and only the
///   matching row draws, with variant `1` when bit `0x1000` is clear and
///   `0` when it is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChoiceFlags(pub u32);

impl ChoiceFlags {
    /// Marker variant for `row`, or `None` when that row draws no marker.
    ///
    /// PORT: FUN_801D603C (the branch chain; `FUN_801D61B0` repeats it
    /// verbatim for its own two rows)
    pub fn marker_variant(self, row: u32) -> Option<u8> {
        let f = self.0;
        if f & 0x4000 != 0 {
            return None;
        }
        if f & 0x2000 != 0 {
            return Some(if f & 0x1000 == 0 { 4 } else { 0 });
        }
        if f & 0xFFF != row {
            return None;
        }
        Some(((f >> 12) ^ 1) as u8 & 1)
    }
}

/// One row of a two-row choice group: its label pen and its marker pen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChoiceRow {
    pub label_pen: (i32, i32),
    pub marker_pen: (i32, i32),
}

/// Geometry of the shared two-row choice block.
///
/// Both windows put the marker column at `marker_x` and the label column
/// `0x14` further right, and step the second row down by one row pitch.
fn choice_rows(marker_x: i32, first_row_y: i32) -> [ChoiceRow; 2] {
    [
        ChoiceRow {
            label_pen: (marker_x + 0x14, first_row_y),
            marker_pen: (marker_x, first_row_y),
        },
        ChoiceRow {
            label_pen: (marker_x + 0x14, first_row_y + PAINTER_ROW_PITCH),
            marker_pen: (marker_x, first_row_y + PAINTER_ROW_PITCH),
        },
    ]
}

/// Emit the markers a choice group's state calls for.
fn choice_marker_sprites(rows: &[ChoiceRow; 2], flags: ChoiceFlags) -> Vec<PainterSprite> {
    rows.iter()
        .enumerate()
        .filter_map(|(i, r)| {
            flags.marker_variant(i as u32).map(|variant| PainterSprite {
                sprite: 0,
                variant,
                x: r.marker_pen.0,
                y: r.marker_pen.1,
            })
        })
        .collect()
}

/// Window 46: one heading line over a two-row choice group.
///
/// `FUN_801D603C` draws the heading at the content origin, then puts the
/// choice block's marker column at `WX + 0x30` with its first row one
/// `0x10` step below the heading.
///
/// PORT: FUN_801D603C
pub fn choice_panel_draws_for(
    font: &legaia_font::Font,
    rect: PainterRect,
    heading: &str,
    choices: [&str; 2],
    flags: ChoiceFlags,
) -> (Vec<TextDraw>, Vec<PainterSprite>) {
    let rows = choice_rows(rect.x + 0x30, rect.y + 0x10);
    let mut out = text_draws_for(
        &font.layout_ascii(heading),
        (rect.x, rect.y),
        MENU_TEXT_WHITE,
    );
    for (row, label) in rows.iter().zip(choices) {
        out.extend(text_draws_for(
            &font.layout_ascii(label),
            row.label_pen,
            MENU_TEXT_WHITE,
        ));
    }
    (out, choice_marker_sprites(&rows, flags))
}

/// Window 5: two heading lines over the same two-row choice group.
///
/// `FUN_801D61B0` steps `0x0E` between its two headings and then `0x10`
/// again before the choice block, and sets the marker column at
/// `WX + 0x3C` rather than `WX + 0x30`. Everything below that is the same
/// branch chain as window 46.
///
/// PORT: FUN_801D61B0
pub fn two_line_choice_panel_draws_for(
    font: &legaia_font::Font,
    rect: PainterRect,
    headings: [&str; 2],
    choices: [&str; 2],
    flags: ChoiceFlags,
) -> (Vec<TextDraw>, Vec<PainterSprite>) {
    let first_row_y = rect.y + PAINTER_ROW_PITCH + 0x10;
    let rows = choice_rows(rect.x + 0x3C, first_row_y);
    let mut out = Vec::new();
    for (i, h) in headings.iter().enumerate() {
        out.extend(text_draws_for(
            &font.layout_ascii(h),
            (rect.x, rect.y + i as i32 * PAINTER_ROW_PITCH),
            MENU_TEXT_WHITE,
        ));
    }
    for (row, label) in rows.iter().zip(choices) {
        out.extend(text_draws_for(
            &font.layout_ascii(label),
            row.label_pen,
            MENU_TEXT_WHITE,
        ));
    }
    (out, choice_marker_sprites(&rows, flags))
}

/// Window 6: six stacked labels with a cursor pinned to the bottom-right
/// **corner of the window extent**.
///
/// `FUN_801D6360` is the only painter in this block that reads `+0xE` and
/// `+0x10` at all: the six labels stack from the content origin at the row
/// pitch, and the cursor lands at `(WX + W - 0x10, WY + H - 0xE)`. Every
/// other painter here anchors on the origin alone, so this one is the only
/// one whose cursor moves when a window is resized.
///
/// PORT: FUN_801D6360
pub fn label_list_draws_for(
    font: &legaia_font::Font,
    rect: PainterRect,
    labels: &[&str],
) -> (Vec<TextDraw>, PainterSprite) {
    let mut out = Vec::new();
    for (i, label) in labels.iter().enumerate() {
        out.extend(text_draws_for(
            &font.layout_ascii(label),
            (rect.x, rect.y + i as i32 * PAINTER_ROW_PITCH),
            MENU_TEXT_WHITE,
        ));
    }
    (
        out,
        PainterSprite {
            sprite: 1,
            variant: 1,
            x: rect.x + rect.w - 0x10,
            y: rect.y + rect.h - PAINTER_ROW_PITCH,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // The rects are the disc records for these ids (menu-overlay window
    // table, `legaia_asset::menu_windows`); pinning them here keeps the
    // pen arithmetic checkable without a disc.
    const W5: PainterRect = PainterRect {
        x: 58,
        y: 82,
        w: 204,
        h: 56,
    };
    const W6: PainterRect = PainterRect {
        x: 50,
        y: 68,
        w: 216,
        h: 82,
    };
    const W32: PainterRect = PainterRect {
        x: 202,
        y: 20,
        w: 104,
        h: 10,
    };
    const W46: PainterRect = PainterRect {
        x: 16,
        y: 84,
        w: 104,
        h: 42,
    };

    #[test]
    fn record_title_text_starts_past_the_prefix() {
        assert_eq!(title_record_text_offset(0), 3);
        assert_eq!(title_record_text_offset(4), 7);
    }

    // -- FUN_801D603C / FUN_801D61B0 -----------------------------------

    #[test]
    fn suppress_bit_beats_every_other_arm() {
        let f = ChoiceFlags(0x4000 | 0x2000 | 0x1000);
        assert_eq!(f.marker_variant(0), None);
        assert_eq!(f.marker_variant(1), None);
    }

    #[test]
    fn mid_change_arm_marks_both_rows_and_ignores_the_index() {
        // Bit 0x1000 clear -> variant 4; the low bits are not compared.
        let f = ChoiceFlags(0x2000 | 7);
        assert_eq!(f.marker_variant(0), Some(4));
        assert_eq!(f.marker_variant(1), Some(4));
        // Bit 0x1000 set -> variant 0.
        let f = ChoiceFlags(0x2000 | 0x1000);
        assert_eq!(f.marker_variant(0), Some(0));
        assert_eq!(f.marker_variant(1), Some(0));
    }

    #[test]
    fn settled_arm_marks_only_the_selected_row() {
        let f = ChoiceFlags(1);
        assert_eq!(f.marker_variant(0), None);
        assert_eq!(f.marker_variant(1), Some(1));
        // Bit 0x1000 flips the settled variant to 0 - and it is part of
        // the index comparison's complement, not of the index itself.
        let f = ChoiceFlags(0x1000);
        assert_eq!(f.marker_variant(0), Some(0));
        assert_eq!(f.marker_variant(1), None);
    }

    #[test]
    fn the_two_choice_windows_differ_only_in_marker_column_and_first_row() {
        let flags = ChoiceFlags(0);
        let font = legaia_font::Font::placeholder();
        let (_, s46) = choice_panel_draws_for(&font, W46, "H", ["A", "B"], flags);
        assert_eq!((s46[0].x, s46[0].y), (W46.x + 0x30, W46.y + 0x10));

        let (_, s5) = two_line_choice_panel_draws_for(&font, W5, ["H", "I"], ["A", "B"], flags);
        assert_eq!(
            (s5[0].x, s5[0].y),
            (W5.x + 0x3C, W5.y + PAINTER_ROW_PITCH + 0x10)
        );
    }

    // -- FUN_801D6360 --------------------------------------------------

    #[test]
    fn label_list_cursor_is_the_only_extent_anchored_pen() {
        let font = legaia_font::Font::placeholder();
        let (_, cur) = label_list_draws_for(&font, W6, &["a", "b", "c", "d", "e", "f"]);
        assert_eq!(cur.x, W6.x + W6.w - 0x10);
        assert_eq!(cur.y, W6.y + W6.h - PAINTER_ROW_PITCH);
    }

    // -- FUN_801DCF84 / FUN_801DD028 -----------------------------------

    #[test]
    fn counter_pictogram_sits_two_below_the_origin() {
        let font = legaia_font::Font::placeholder();
        let (_, pic) = counter_panel_draws_for(&font, W32, COUNTER_PICTOGRAM_PRIMARY, 1234);
        assert_eq!((pic.id, pic.x, pic.y), (0x62, W32.x, W32.y + 2));
    }

    // -- FUN_801DCC20 --------------------------------------------------

    #[test]
    fn count_panel_reserves_its_sub_rect_even_with_no_selection() {
        let font = legaia_font::Font::placeholder();
        let rect = PainterRect::new(14, 108, 144, 40);
        let (draws, reserved) = count_panel_draws_for(&font, rect, None);
        assert!(draws.is_empty());
        assert_eq!(reserved, (14, 108 + 0x38, 0x90, 0x28));
        let (draws, _) = count_panel_draws_for(&font, rect, Some(7));
        assert!(!draws.is_empty());
    }

    // -- FUN_801DCCB4 / FUN_801DCE20 -----------------------------------

    #[test]
    fn both_prompt_windows_share_the_corner_cursor_inset() {
        let font = legaia_font::Font::placeholder();
        let rect = PainterRect::new(38, 100, 244, 28);
        let (_, a) = char_prompt_draws_for(&font, rect, "line");
        let (_, b) = amount_prompt_draws_for(&font, rect, "head", 12, "unit");
        assert_eq!((a.x, a.y), (rect.x + 0xE6, rect.y + 0x0D));
        assert_eq!((b.x, b.y), (a.x, a.y));
    }
}

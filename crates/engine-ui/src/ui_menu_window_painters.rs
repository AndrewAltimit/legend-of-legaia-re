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
//! | 34 | `FUN_801D4A80` | item name, owned count, description line |
//! | 36 | `FUN_801D56FC` | equip-target list gated on the character mask |
//! | 37 | `FUN_801D5944` | sell quantity + halved gold total |
//! | 43 | `FUN_801DCFE4` | plain title tab |
//! | 45 | `FUN_801DD028` | second pictogram + 8-digit counter |
//! | 46 | `FUN_801D603C` | one-line prompt over a two-row choice |
//!
//! Plus `FUN_801E4140` ([`guarded_box_rect`]), the bottom-clipped box emit
//! the block's fills go through.
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
//! `overlay_menu_801d603c.txt`, `overlay_menu_801d4a80.txt`,
//! `overlay_menu_801d56fc.txt`, `overlay_menu_801d5944.txt`,
//! `overlay_menu_801e4140.txt` (PROT entry 0899, the menu overlay).

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
/// NOT WIRED: no host walks the disc window table and dispatches on renderer_va; waived in scripts/ci/ui-host-drift-waivers.toml
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
/// NOT WIRED: no host walks the disc window table and dispatches on renderer_va; waived in scripts/ci/ui-host-drift-waivers.toml
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
/// NOT WIRED: no host walks the disc window table and dispatches on renderer_va; waived in scripts/ci/ui-host-drift-waivers.toml
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
/// NOT WIRED: no host walks the disc window table and dispatches on renderer_va; waived in scripts/ci/ui-host-drift-waivers.toml
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
/// NOT WIRED: no host walks the disc window table and dispatches on renderer_va; waived in scripts/ci/ui-host-drift-waivers.toml
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
/// NOT WIRED: no host walks the disc window table and dispatches on renderer_va; waived in scripts/ci/ui-host-drift-waivers.toml
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
    /// REF: FUN_801D603C (the branch chain; `FUN_801D61B0` repeats it
    /// verbatim for its own two rows - both are tagged on the painters
    /// that own them)
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
/// NOT WIRED: no host walks the disc window table and dispatches on renderer_va; waived in scripts/ci/ui-host-drift-waivers.toml
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
/// NOT WIRED: no host walks the disc window table and dispatches on renderer_va; waived in scripts/ci/ui-host-drift-waivers.toml
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
/// NOT WIRED: no host walks the disc window table and dispatches on renderer_va; waived in scripts/ci/ui-host-drift-waivers.toml
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

// ---------------------------------------------------------------------
// Shop / item windows - 34, 36, 37
// ---------------------------------------------------------------------

/// Where a window-34 description line comes from.
///
/// `FUN_801D4A80` branches on the item record's kind byte (`+0` of
/// `0x80074368 + id*12`). Kind `2` - the accessory ("Goods") class - does
/// **not** use the item's own description word. It reads the item-effect
/// record `0x800752C0 + effect*4`, takes the passive index at `+3`, and
/// only if that index is below `0x40` does it draw the passive's
/// description from `0x8007625C + index*12 + 8`. An accessory whose
/// passive index is `0x40` or above draws no description at all - the
/// bound is checked twice, unsigned then signed, and both arms bail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescriptionSource {
    /// The item record's own description word (`+8`).
    Item,
    /// Accessory passive `index`, whose description lives in the
    /// name/description table at `0x8007625C`.
    AccessoryPassive(u8),
    /// Kind `2` with an out-of-range passive index: nothing is drawn.
    None,
}

/// Item kind byte that routes a description through the passive table.
pub const ITEM_KIND_ACCESSORY: u8 = 2;
/// Passive indices at or above this draw no description.
pub const ACCESSORY_PASSIVE_LIMIT: u8 = 0x40;

/// Resolve which description a window-34 draw would use.
///
/// REF: FUN_801D4A80 (the kind-2 branch and its double bound check)
pub fn description_source(item_kind: u8, passive_index: u8) -> DescriptionSource {
    if item_kind != ITEM_KIND_ACCESSORY {
        return DescriptionSource::Item;
    }
    if passive_index >= ACCESSORY_PASSIVE_LIMIT {
        return DescriptionSource::None;
    }
    DescriptionSource::AccessoryPassive(passive_index)
}

/// Window 34: item name, owned count, and the description line below.
///
/// `FUN_801D4A80` draws nothing at all when the selection index
/// `DAT_801E46B0` is not positive. Otherwise the name goes at the content
/// origin, the two-digit owned count at `WX + 0x94`, and the description
/// one row pitch down at `WX + 8` - routed through
/// [`description_source`]. The owned count comes back from
/// `FUN_80042EE0`; the sentinel `0x100` means "not held" and draws `0`.
///
/// PORT: FUN_801D4A80
/// NOT WIRED: no host walks the disc window table and dispatches on renderer_va; waived in scripts/ci/ui-host-drift-waivers.toml
pub fn item_description_draws_for(
    font: &legaia_font::Font,
    rect: PainterRect,
    selected: bool,
    name: &str,
    owned: u8,
    description: &str,
) -> Vec<TextDraw> {
    if !selected {
        return Vec::new();
    }
    let mut out = text_draws_for(&font.layout_ascii(name), (rect.x, rect.y), MENU_TEXT_WHITE);
    out.extend(digits_draws(
        font,
        owned as u64,
        rect.x + 0x94,
        rect.y,
        2,
        MENU_TEXT_WHITE,
    ));
    if !description.is_empty() {
        out.extend(text_draws_for(
            &font.layout_ascii(description),
            (rect.x + 8, rect.y + PAINTER_ROW_PITCH),
            MENU_TEXT_WHITE,
        ));
    }
    out
}

/// Sentinel `FUN_80042EE0` returns when the selected item is not held.
pub const OWNED_COUNT_ABSENT: i32 = 0x100;

/// Owned count the window-34 draw uses for a lookup result.
///
/// REF: FUN_801D4A80 (`li v0,0x100; beq a0,v0 -> clear a0`)
pub fn owned_count_or_zero(lookup: i32, count: u8) -> u8 {
    if lookup == OWNED_COUNT_ABSENT {
        0
    } else {
        count
    }
}

/// One row of the window-36 equip-target list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EquipTargetRow {
    /// Party-member class byte (`0x80084598 + i`).
    pub member_class: u8,
    /// `false` when the selected equipment's character mask excludes this
    /// member. Retail still draws the row - it drops the draw-order word
    /// to `0` instead of skipping, which sinks the glyphs behind the rest
    /// of the frame rather than removing them.
    pub equippable: bool,
}

/// Character-mask bit for a party-member class.
///
/// `FUN_801D56FC` does not compute `1 << class`; it indexes a four-byte
/// overlay table at `0x801E43F0`. On the retail disc that table reads
/// `01 02 04 00`, so it agrees with the shift for classes `0..=2` and
/// gives class `3` a mask of **zero** - the fourth party member matches no
/// equipment through this window.
///
/// REF: FUN_801D56FC
pub fn equip_class_mask(member_class: u8) -> u8 {
    match member_class {
        0 => 1,
        1 => 2,
        2 => 4,
        _ => 0,
    }
}

/// Whether the equipment whose character mask is `equip_mask`
/// (`legaia_asset::equip_stats::EquipBonus::equip_mask`) can be worn by a
/// member of `member_class`.
///
/// REF: FUN_801D56FC (`and v0,s6,v0; bne v0,zero` - a zero result drops
/// the draw order to 0 rather than skipping the row)
pub fn equip_row_enabled(equip_mask: u8, member_class: u8) -> bool {
    equip_mask & equip_class_mask(member_class) != 0
}

/// Window 36: a header row plus one row per party member, each carrying
/// the shared choice marker.
///
/// `FUN_801D56FC` puts the header at `WX + 0x18` and its marker at
/// `WX + 4`; every member row repeats that pair one row pitch further
/// down, with row `i` taking marker index `i + 1`. The marker chain is the
/// same [`ChoiceFlags`] branch the options windows use, driven here by
/// `DAT_801E46C0` rather than `DAT_801E46D0`.
///
/// PORT: FUN_801D56FC
/// NOT WIRED: no host walks the disc window table and dispatches on renderer_va; waived in scripts/ci/ui-host-drift-waivers.toml
pub fn equip_target_list_draws_for(
    font: &legaia_font::Font,
    rect: PainterRect,
    header: &str,
    rows: &[(EquipTargetRow, &str)],
    flags: ChoiceFlags,
) -> (Vec<TextDraw>, Vec<PainterSprite>) {
    let label_x = rect.x + 0x18;
    let marker_x = rect.x + 4;
    let mut text = text_draws_for(
        &font.layout_ascii(header),
        (label_x, rect.y),
        MENU_TEXT_WHITE,
    );
    let mut sprites = Vec::new();
    if let Some(variant) = flags.marker_variant(0) {
        sprites.push(PainterSprite {
            sprite: 0,
            variant,
            x: marker_x,
            y: rect.y,
        });
    }
    for (i, (row, label)) in rows.iter().enumerate() {
        let y = rect.y + (i as i32 + 1) * PAINTER_ROW_PITCH;
        // A non-equippable row still emits its glyphs; only the draw
        // order changes, which this crate leaves to the host.
        let _ = row.equippable;
        text.extend(text_draws_for(
            &font.layout_ascii(label),
            (label_x, y),
            MENU_TEXT_WHITE,
        ));
        if let Some(variant) = flags.marker_variant(i as u32 + 1) {
            sprites.push(PainterSprite {
                sprite: 0,
                variant,
                x: marker_x,
                y,
            });
        }
    }
    (text, sprites)
}

/// Digit-field width the sell panel sizes its total to.
///
/// `FUN_801D5944` starts at 4 and widens on three unsigned comparisons
/// against the *unit* price: `>= 100` and `>= 1000` each add one, and
/// `>= 10000` **assigns** 5 before those two add to it. So the ladder is
/// 4 / 5 / 6 / 7 rather than a digit count - a four-digit price still
/// reserves six cells.
///
/// REF: FUN_801D5944
pub fn sell_total_digits(unit_price: u32) -> i32 {
    let mut n = 4;
    if unit_price >= 10_000 {
        n = 5;
    }
    if unit_price >= 1_000 {
        n += 1;
    }
    if unit_price >= 100 {
        n += 1;
    }
    n
}

/// The gold total window 37 writes: quantity times the unit price,
/// halved.
///
/// The arithmetic is `mult` then `sra ..,1` - an arithmetic shift, so an
/// odd product truncates toward negative infinity. Products are
/// non-negative here, which makes it plain integer halving: the retail
/// sell price is half the list price.
///
/// REF: FUN_801D5944
pub fn sell_total(quantity: u32, unit_price: u32) -> u32 {
    quantity.saturating_mul(unit_price) / 2
}

/// Window 37: the sell quantity panel.
///
/// `FUN_801D5944` draws nothing when the selection index is not positive.
/// Otherwise: a heading at the content origin, then one row `0x14` below
/// carrying the quantity at `WX + 0x10`, a separator glyph at `WX + 0x20`
/// and the held count at `WX + 0x28`. The gold pictogram and the total
/// are then right-packed against `WX + 0x88` / `WX + 0x94` by the digit
/// ladder ([`sell_total_digits`]), so a bigger total pushes both left.
///
/// PORT: FUN_801D5944
/// NOT WIRED: no host walks the disc window table and dispatches on renderer_va; waived in scripts/ci/ui-host-drift-waivers.toml
pub fn sell_quantity_draws_for(
    font: &legaia_font::Font,
    rect: PainterRect,
    selected: bool,
    heading: &str,
    quantity: u32,
    held: u32,
    unit_price: u32,
) -> (
    Vec<TextDraw>,
    Option<PainterPictogram>,
    Option<PainterSprite>,
) {
    if !selected {
        return (Vec::new(), None, None);
    }
    let row = rect.y + 0x14;
    let digits = sell_total_digits(unit_price);
    let pack = rect.x - 8;
    let mut out = text_draws_for(
        &font.layout_ascii(heading),
        (rect.x, rect.y),
        MENU_TEXT_WHITE,
    );
    out.extend(digits_draws(
        font,
        quantity as u64,
        rect.x + 0x10,
        row,
        2,
        MENU_TEXT_WHITE,
    ));
    out.extend(digits_draws(
        font,
        held as u64,
        rect.x + 0x28,
        row,
        2,
        MENU_TEXT_WHITE,
    ));
    out.extend(digits_draws(
        font,
        sell_total(quantity, unit_price) as u64,
        pack + 0x94 - digits * NUM_CELL_W,
        row,
        digits,
        MENU_TEXT_WHITE,
    ));
    (
        out,
        Some(PainterPictogram {
            id: COUNTER_PICTOGRAM_PRIMARY,
            x: pack + 0x88 - digits * NUM_CELL_W,
            y: row + 2,
        }),
        Some(PainterSprite {
            sprite: 0,
            variant: 1,
            x: rect.x - 4,
            y: row,
        }),
    )
}

// ---------------------------------------------------------------------
// Shared box emit
// ---------------------------------------------------------------------

/// Bottom clip the guarded box emit applies (`slti v0,s0,0xf1`).
///
/// The 320x240 display's last line is 239, so a `y` of 240 already
/// reserves nothing; retail's bound admits 240 and rejects 241.
pub const BOX_EMIT_MAX_Y: i32 = 0xF0;

/// The box a guarded fill would emit, or `None` when it is clipped away.
///
/// `FUN_801E4140` takes six arguments, forwards the first to the fill-mode
/// setter and the last four to the box writer, and runs neither when the
/// `y` argument exceeds `0xF0`. That single comparison is the whole
/// routine - it is a bottom-of-screen guard on an otherwise unconditional
/// pair of calls.
///
/// PORT: FUN_801E4140
/// NOT WIRED: no host walks the disc window table and dispatches on renderer_va; waived in scripts/ci/ui-host-drift-waivers.toml
pub fn guarded_box_rect(x: i32, y: i32, w: i32, h: i32) -> Option<(i32, i32, i32, i32)> {
    (y <= BOX_EMIT_MAX_Y).then_some((x, y, w, h))
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

    // -- FUN_801D4A80 --------------------------------------------------

    #[test]
    fn only_accessories_route_through_the_passive_table() {
        assert_eq!(description_source(0, 0), DescriptionSource::Item);
        assert_eq!(description_source(1, 0x7F), DescriptionSource::Item);
        assert_eq!(
            description_source(ITEM_KIND_ACCESSORY, 0x3F),
            DescriptionSource::AccessoryPassive(0x3F)
        );
        // The bound is exclusive, and out-of-range draws nothing at all.
        assert_eq!(
            description_source(ITEM_KIND_ACCESSORY, 0x40),
            DescriptionSource::None
        );
    }

    #[test]
    fn the_not_held_sentinel_reads_as_zero() {
        assert_eq!(owned_count_or_zero(OWNED_COUNT_ABSENT, 9), 0);
        assert_eq!(owned_count_or_zero(3, 9), 9);
    }

    #[test]
    fn item_panel_is_empty_without_a_selection() {
        let font = legaia_font::Font::placeholder();
        let rect = PainterRect::new(138, 166, 168, 38);
        assert!(item_description_draws_for(&font, rect, false, "n", 1, "d").is_empty());
        assert!(!item_description_draws_for(&font, rect, true, "n", 1, "d").is_empty());
    }

    // -- FUN_801D56FC --------------------------------------------------

    #[test]
    fn the_fourth_party_class_matches_no_equipment() {
        assert_eq!(equip_class_mask(0), 1);
        assert_eq!(equip_class_mask(1), 2);
        assert_eq!(equip_class_mask(2), 4);
        assert_eq!(equip_class_mask(3), 0);
        // Mask 7 = "any party member" still excludes class 3, because the
        // table entry is zero rather than a fourth bit.
        assert!(equip_row_enabled(7, 0));
        assert!(!equip_row_enabled(7, 3));
    }

    #[test]
    fn equip_list_rows_take_marker_index_one_and_up() {
        let font = legaia_font::Font::placeholder();
        let rect = PainterRect::new(138, 98, 168, 52);
        let rows = [
            (
                EquipTargetRow {
                    member_class: 0,
                    equippable: true,
                },
                "a",
            ),
            (
                EquipTargetRow {
                    member_class: 1,
                    equippable: false,
                },
                "b",
            ),
        ];
        // Settled on index 2 -> only the second member row is marked.
        let (_, sprites) = equip_target_list_draws_for(&font, rect, "h", &rows, ChoiceFlags(2));
        assert_eq!(sprites.len(), 1);
        assert_eq!(sprites[0].x, rect.x + 4);
        assert_eq!(sprites[0].y, rect.y + 2 * PAINTER_ROW_PITCH);
    }

    // -- FUN_801D5944 --------------------------------------------------

    #[test]
    fn the_digit_ladder_is_not_a_digit_count() {
        assert_eq!(sell_total_digits(0), 4);
        assert_eq!(sell_total_digits(99), 4);
        assert_eq!(sell_total_digits(100), 5);
        assert_eq!(sell_total_digits(999), 5);
        assert_eq!(sell_total_digits(1_000), 6);
        assert_eq!(sell_total_digits(9_999), 6);
        // The >= 10000 arm assigns 5 and then the two adds still apply.
        assert_eq!(sell_total_digits(10_000), 7);
    }

    #[test]
    fn the_sell_total_is_half_the_list_price() {
        assert_eq!(sell_total(1, 100), 50);
        assert_eq!(sell_total(3, 25), 37);
        assert_eq!(sell_total(0, 9_999), 0);
    }

    #[test]
    fn a_wider_total_pushes_the_pictogram_left() {
        let font = legaia_font::Font::placeholder();
        let rect = PainterRect::new(14, 46, 144, 33);
        let (_, cheap, _) = sell_quantity_draws_for(&font, rect, true, "h", 1, 9, 10);
        let (_, dear, _) = sell_quantity_draws_for(&font, rect, true, "h", 1, 9, 10_000);
        assert!(dear.unwrap().x < cheap.unwrap().x);
        let (draws, pic, cur) = sell_quantity_draws_for(&font, rect, false, "h", 1, 9, 10);
        assert!(draws.is_empty() && pic.is_none() && cur.is_none());
    }

    // -- FUN_801E4140 --------------------------------------------------

    #[test]
    fn the_box_emit_clips_below_the_display() {
        assert_eq!(guarded_box_rect(0, 0xF0, 8, 8), Some((0, 0xF0, 8, 8)));
        assert_eq!(guarded_box_rect(0, 0xF1, 8, 8), None);
    }

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

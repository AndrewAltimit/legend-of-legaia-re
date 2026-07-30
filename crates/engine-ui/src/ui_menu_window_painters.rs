//! Content painters for a block of the menu overlay's **window-descriptor
//! table** (`legaia_asset::menu_windows`, 52 records at overlay VA
//! `0x801E4738`).
//!
//! Each descriptor's `renderer_va` names the routine that fills that
//! window's content rect; the 9-slice frame around it is drawn by the
//! caller, not here (see `docs/subsystems/field-menu.md`).
//! [`crate::ui_menu_window_dispatch`] maps a parsed descriptor to the painter
//! below that draws it, the way the retail window walker maps the live
//! window's `+0x28` to a routine. This module ports the painters for the
//! tab / prompt / counter / shop-panel block:
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

/// The **accent pen** three painters here stage for exactly one field before
/// restoring the default.
///
/// Retail stages `_DAT_8007B454 = 6` around window 34's item name, window
/// 24's count and window 31's number, and `7` for everything else in the
/// block. `6` is the same staging id the compare panels use for a rising
/// stat, so it resolves to the same colour ([`compare_panel_ink`]).
///
/// [`compare_panel_ink`]: crate::compare_panel_ink
pub const PAINTER_INK_ACCENT: [f32; 4] = MENU_TEXT_GOLD;

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

// --- Separator glyph (`FUN_8003C1F8`) ---------------------------------
//
// `FUN_8003C1F8(glyph, x, y)` draws one symbol from the fixed-cell glyph
// page. Two independent uses pin the same id space: the records screen
// (`crate::ui_menu::records_screen`, whose fields are `:` / `/` / `.`) and
// window 37, which calls it with glyph `6` between its quantity and held
// counts (`li a0,0x6; addiu a1,s5,0x20` at `0x801D59D4`) - a `/` between
// "how many" and "how many you have", which is what that row reads as.

/// Separator glyph `6`: `/`.
pub const SEPARATOR_GLYPH_SLASH: u8 = 6;
/// Separator glyph `9`: `:`.
pub const SEPARATOR_GLYPH_COLON: u8 = 9;
/// Separator glyph `0xD`: `.`.
pub const SEPARATOR_GLYPH_DOT: u8 = 0x0D;

/// The character a separator-glyph id draws, or `None` for an id outside
/// the three the corpus pins.
pub fn separator_glyph_char(glyph: u8) -> Option<char> {
    match glyph {
        SEPARATOR_GLYPH_SLASH => Some('/'),
        SEPARATOR_GLYPH_COLON => Some(':'),
        SEPARATOR_GLYPH_DOT => Some('.'),
        _ => None,
    }
}

/// Draw one separator glyph at `pen`.
///
/// `pub(crate)` on purpose: this is a retail *primitive*, not a screen, and
/// the UI host-drift gate classifies every `pub fn` returning draws as a
/// screen builder. Keeping it crate-internal states what it is - the thing
/// screens call - and keeps the gate's surface a list of screens.
///
/// PORT: FUN_8003c1f8
pub(crate) fn separator_glyph_draws(
    font: &legaia_font::Font,
    glyph: u8,
    pen: (i32, i32),
    color: [f32; 4],
) -> Vec<TextDraw> {
    match separator_glyph_char(glyph) {
        Some(ch) => separator_glyph_char_draws(font, ch, pen, color),
        None => Vec::new(),
    }
}

/// [`separator_glyph_draws`] for a caller that already resolved the glyph to
/// a character.
pub(crate) fn separator_glyph_char_draws(
    font: &legaia_font::Font,
    ch: char,
    pen: (i32, i32),
    color: [f32; 4],
) -> Vec<TextDraw> {
    text_draws_for(&font.layout_ascii(&ch.to_string()), pen, color)
}

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
/// The five pause-menu tab renderers (`FUN_801DCA0C` / `CA50` / `CA94` /
/// `CAD8` / `CB1C`) are the same 17 instructions with a different string
/// pointer, so this one painter serves all six windows;
/// [`crate::ui_menu_window_dispatch`] resolves any of them to it.
///
/// PORT: FUN_801DCFE4
/// PORT: FUN_801DCA0C, FUN_801DCA50, FUN_801DCA94, FUN_801DCAD8, FUN_801DCB1C
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
/// The retail record is the armed op-`0x49` payload: `_DAT_8007B450` points
/// at the opcode's sub-op byte, so for a shop record (`[count][ids][name]`)
/// `record[2]` is `count` and the string starts one past the last id - the
/// vendor name `legaia_asset::shop_stock` decodes.
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
/// and which global counter they read - `0x8008459C` (party gold) for
/// window 32, `0x800845A4` (the casino coin bank) for window 45. The
/// pictogram sits two pixels below the content origin and the digit field
/// starts `0x28` to its right. Which of the two a descriptor names, and
/// therefore which live total a host feeds, is
/// [`crate::CounterSource`].
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
/// The record indexing is worth spelling out, because it is what a host
/// would have to reproduce: `0x8007BB70` is scaled by the `0x414` character
/// record stride (`(x<<6 + x)<<2 + x` then `<<2`) and `0x8007BB78` is added
/// as a plain byte offset, so the substituted glyph is
/// `record[0x13D + DAT_8007BB78]` off the `0x80084708` record base - not a
/// name byte, whose field is `+0x2A7`.
///
/// `+0x13D` is not an arbitrary field. It is the first entry of the
/// character's **learned-magic id list**, whose length byte sits one earlier
/// at `+0x13C` - the same byte the records page prints under "Magic"
/// (`crate::ui_menu::records_screen`, whose own rebase table pins the pair).
/// So `DAT_8007BB78` is a list index and the substituted byte is a spell id.
///
/// **Which flow opens it is pinned**, and it is narrower than "the magic
/// side": the two magic-cast sub-screens `FUN_801D9280` (all-targets) and
/// `FUN_801D9594` (single target), menu sub-screens `0x0F` / `0x10`. Both
/// run the same three beats - seed `_DAT_8007BB70` and `_DAT_8007BB78` to
/// `0xFF`, debit MP, call the effect-apply handler `FUN_800402F4`, and then
/// open this window (scripts `0x801E4D50` / `0x801E4D78`, each one command:
/// `01 07`) **only if the sentinel changed**, holding for a confirm /
/// cancel press before closing it again.
///
/// What changes the sentinel is a **spell level-up**. The apply handler's
/// two HP-heal arms compare a per-spell u16 threshold against the caster's
/// `+0x5D0` accumulator, bump the level byte at record `+0x161 + index` and
/// call the notification setter `FUN_80035C00(slot, index)`, which is the
/// pair `(gp+0x858, gp+0x860)` = `(_DAT_8007BB70, _DAT_8007BB78)`. So the
/// window says *this spell of this character went up a level*, and the
/// substituted glyph is that spell's id.
///
/// PORT: FUN_801DCCB4
/// REF: FUN_80035C00 - the `(slot, list index)` notification setter
///
/// Wired: the menu-cast leveling arm exists now -
/// `field_menu_dispatch::apply_spell_outcome` accrues the `FUN_800402F4`
/// heal grants into the caster record's per-spell XP accumulator (the same
/// `+0x8` array the battle summon path trains; `+0x5D0` off the
/// `0x80084140` save-context window is that array), runs the shared
/// threshold kernel (`magic_xp::accrue_and_level`) and returns the
/// `FUN_80035C00` pair as a `magic_xp::SpellLevelNotice`.
/// `MenuRuntime::arm_spell_level_notice` holds the beat; both hosts paint
/// this window off the disc-parsed id-7 rect while it is up and hold the
/// pad for a confirm / cancel press, exactly like the window-31 toast
/// below. The prompt line is engine-composed (the pinned battle-banner
/// sentence around the spell's name) - retail's own rodata sentence at
/// `0x801E46E4` is Sony bytes and stays on the disc.
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
/// pitch, writes the `0x800845B4` counter - the **Point Card** total (see
/// `docs/subsystems/shop.md`) - as an 8-digit field at the left margin in
/// the accent pen, and puts the unit label `0x40` to its right back in the
/// default pen.
///
/// **Which flow opens it** is on the disc, not a guess: the retail buy
/// commits hand the widget VM a script whose whole body is `01 1F` +
/// terminator - "open window `0x1F`" - immediately after crediting the
/// counter. `0x801E4EDC` is the quantity commit's (`FUN_801DB7F4` case 2),
/// `0x801E4EA8` the recipient picker's (`FUN_801DB380`), and both stall in
/// a following phase that returns to the buy list only on a confirm /
/// cancel press.
///
/// PORT: FUN_801DCE20
///
/// Wired: `engine-core::World::point_card` is the counter and
/// `MenuRuntime::point_card_toast` the beat; both hosts paint this window
/// while the toast is up, off the same disc-parsed rect as the shop's other
/// descriptor windows, and both label it with [`POINT_CARD_HEADING`] /
/// [`POINT_CARD_UNIT_LABEL`] below.
/// Window 31's heading line. Retail's literal (`0x801CEA40`) opens with the
/// character-substitution token the dialog codec resolves at draw time; the
/// port stages an engine-authored line in the same slot.
///
/// Both hosts draw this window, so the label lives beside the builder rather
/// than once per host - two host-local copies would be a divergence the
/// drift gate could only catch by pairing them.
pub const POINT_CARD_HEADING: &str = "Points earned";

/// The unit label window 31 puts `0x40` right of its number field
/// (`0x801CEA50`). Shared for the same reason as [`POINT_CARD_HEADING`].
pub const POINT_CARD_UNIT_LABEL: &str = "point(s).";

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
        PAINTER_INK_ACCENT,
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
/// `FUN_801DCC20` guards its whole body on `DAT_801E46B0 > 0`; the trailing
/// `FUN_8002C69C` box at `(WX, WY + 0x38)` sized `0x90 x 0x28` is emitted
/// either way, which is why an empty selection still reserves the space.
/// The count itself comes back from `FUN_80042F4C`, a lookup on the
/// selection index, and lands at `WX + 0x80` in the accent pen.
///
/// **This painter is only the delta.** The gated body opens with
/// `jal 0x801D0F1C` - the *shared item-info panel*, the same routine window
/// 17's `FUN_801DCB60` calls - so window 24 is the item-info window plus a
/// count. A host adopting it draws that panel into the same rect first;
/// its port is folded into [`crate::items_screen_draws_for`], which carries
/// the `FUN_801D0F1C` tag, rather than standing alone as a rect painter.
///
/// **The screen is pinned.** Window 24's rect is byte-identical to window
/// 17's, so which screen opens it could not be read off the geometry - but
/// it can be read off the open scripts. Exactly one command in the whole
/// menu overlay opens window `0x18`, and the same script opens windows `2`
/// and `0x19` with it: `0x801E4DC8`, run by menu sub-screen `0x13`
/// (`FUN_801D9C14`). Window 2 is the **Equip** title tab and window 25 the
/// active-character stat compare, so window 24 is the Equip screen's
/// item panel, not a second Items screen.
///
/// PORT: FUN_801DCC20
/// REF: FUN_801d0f1c - the shared item-info panel this painter is the delta
/// over. Ported in `crate::ui_menu::pause_lists`, not here.
/// NOT WIRED: what must exist first is the **Equip screen's item panel**,
/// NOT WIRED: and this painter is only its delta: the gated body opens with
/// NOT WIRED: `jal 0x801D0F1C`, the shared item-info panel window 17 also
/// NOT WIRED: draws, so a host adopting window 24 draws that panel into the
/// NOT WIRED: same rect first and adds this count on top. The engine's equip
/// NOT WIRED: screen (`engine-ui::ui_menu::equipment`) is a slot list with no
/// NOT WIRED: item-info panel at all, so there is no rect to add a count to
/// NOT WIRED: yet. Waived in scripts/ci/ui-host-drift-waivers.toml
pub fn count_panel_draws_for(
    font: &legaia_font::Font,
    rect: PainterRect,
    selection: Option<u64>,
) -> (Vec<TextDraw>, (i32, i32, i32, i32)) {
    let reserved = (rect.x, rect.y + 0x38, 0x90, 0x28);
    let draws = match selection {
        Some(count) => digits_draws(font, count, rect.x + 0x80, rect.y, 2, PAINTER_INK_ACCENT),
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
/// **This is the casino prize counter's Yes/No confirm**, not an options
/// screen. Exactly one command in the menu overlay opens window `0x2E`:
/// script `0x801E4F2C`, run from sub-screen `0x20` (`FUN_801DC1CC`, the
/// prize exchange) at the point where a chosen prize passes the coin and
/// held-cap gates. The sibling script `0x801E4F34` closes it again on No or
/// cancel, and the exchange seeds the cursor to row `1` first - retail's
/// "No is the default" convention.
///
/// PORT: FUN_801D603C
/// NOT WIRED: what must exist first is a **host that mounts the prize
/// NOT WIRED: exchange**. The session is ported and complete
/// NOT WIRED: (`engine-core::prize_exchange::PrizeExchangeSession`, whose
/// NOT WIRED: state 2 is exactly this confirm), but no host opens it, so
/// NOT WIRED: nothing reaches the confirm beat. The flags word is **not** a
/// NOT WIRED: gap: it is the shared cursor word `FUN_801D688C` maintains,
/// NOT WIRED: and its four-way decode is already ported live as
/// NOT WIRED: `engine-core::shop::shop_cursor_mode` - the same three bits,
/// NOT WIRED: the same arms as [`ChoiceFlags::marker_variant`]. Waived in
/// NOT WIRED: scripts/ci/ui-host-drift-waivers.toml
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
/// Its screen is a different one from window 46's. Script `0x801E4BD4`
/// (`05 00`, `01 05`) is the only opener, and it belongs to menu sub-screen
/// `3` (`FUN_801D6D38`): a two-row confirm seeded to row `1` = No, whose
/// Yes routes to sub-screen `0` (leave the menu) and whose No returns to
/// the pause root. The pause root reaches it from **cancel**, and only
/// under one condition - `_DAT_8007B450` non-null with its first byte
/// `0x0D`, the entry-context kind (`0x801d6cf8..0x801d6d18`). So this is
/// the "really leave?" gate on a scripted menu, not a general confirm.
///
/// PORT: FUN_801D61B0
/// NOT WIRED: what must exist first is the **entry-context kind**. Retail
/// NOT WIRED: keeps one global pointer whose first byte says which flow
/// NOT WIRED: opened the menu, and the port deliberately replaced it with a
/// NOT WIRED: per-context tagged park, so nothing holds a byte to compare
/// NOT WIRED: against `0x0D` - the same gap that gates the pause root's
/// NOT WIRED: entry-context row (see docs/tooling/live-audit-triage.md).
/// NOT WIRED: Until the op-`0x49` arm records its sub-op there is no
/// NOT WIRED: condition under which this screen opens. Waived in
/// NOT WIRED: scripts/ci/ui-host-drift-waivers.toml
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
/// The screen it belongs to is menu sub-screen `4` (`FUN_801DD1B8`, script
/// `0x801E4BE0`, the only opener of window `0x06`): it opens the window,
/// waits for a confirm / cancel press and routes back to the pause root.
/// That is a **notice panel**, which is why its only cursor is the corner
/// advance hand. `FUN_801DC6B4` selects sub-screen `4` as the menu's
/// *entry* screen when the entry-context byte is `0x0D`
/// (`0x801dc8d0..0x801dc8e4`) - the same kind whose exit confirm is
/// [`two_line_choice_panel_draws_for`]'s window 5.
///
/// PORT: FUN_801D6360
/// NOT WIRED: same missing prerequisite as window 5 - the **entry-context
/// NOT WIRED: kind**. The port replaced retail's single entry-context
/// NOT WIRED: pointer with a per-context tagged park, so no byte exists to
/// NOT WIRED: match against `0x0D`, and nothing selects this screen. The
/// NOT WIRED: six labels are also content the entry-context record owns, so
/// NOT WIRED: a host has neither the trigger nor the text. Waived in
/// NOT WIRED: scripts/ci/ui-host-drift-waivers.toml
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
/// `FUN_801D4A80` draws nothing at all when the selected **item id**
/// `DAT_801E46B0` is not positive - that word is the id the routine
/// multiplies by 12 to reach `0x80074368`, not a list row. Otherwise the
/// name goes at the content
/// origin, then the two-digit owned count at `WX + 0x94` - **both in the
/// accent pen**, which retail stages once before the name and restores only
/// after the count - and the description one row pitch down at `WX + 8` back
/// in the default pen, routed through [`description_source`]. The owned
/// count comes back from `FUN_80042EE0`; the sentinel `0x100` means "not
/// held" and draws `0`.
///
/// PORT: FUN_801D4A80
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
    let mut out = text_draws_for(
        &font.layout_ascii(name),
        (rect.x, rect.y),
        PAINTER_INK_ACCENT,
    );
    out.extend(digits_draws(
        font,
        owned as u64,
        rect.x + 0x94,
        rect.y,
        2,
        PAINTER_INK_ACCENT,
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
///
/// Wired on both hosts through the shared composition
/// [`crate::recipient_picker_draws_for`]: the shop's equipment-buy recipient
/// flow (`engine-core`'s `menu_runtime` + `shop::BuyRecipientSession`) opens
/// this window over the parked buy list, and each host resolves the rect off
/// the disc window table and hands the model to that builder.
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
/// `FUN_801D5944` draws nothing when the selected item id
/// `DAT_801E46B0` is not positive.
/// Otherwise: a heading at the content origin, then one row `0x14` below
/// carrying the quantity at `WX + 0x10`, a separator glyph at `WX + 0x20`
/// and the held count at `WX + 0x28`. The gold pictogram and the total
/// are then right-packed against `WX + 0x88` / `WX + 0x94` by the digit
/// ladder ([`sell_total_digits`]), so a bigger total pushes both left.
///
/// The unit price retail reads is the **item record's own** `+2` word
/// (`0x80074368 + id*12 + 2`), not the merchant's stock entry, so a bag item
/// the shop does not sell still prices correctly.
///
/// PORT: FUN_801D5944
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
    out.extend(separator_glyph_draws(
        font,
        SEPARATOR_GLYPH_SLASH,
        (rect.x + 0x20, row),
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
/// `FUN_801E4140` takes six arguments, runs nothing when the `y` argument
/// exceeds `0xF0`, and otherwise calls the fill-state setter `FUN_80034B6C`
/// and then the box writer `FUN_8002C69C(x, y, w, h)`. That single
/// comparison is the whole routine - it is a bottom-of-screen guard on an
/// otherwise unconditional pair of calls.
///
/// The decompiled C renders the first call as `func_0x80034b6c()` with no
/// arguments; the disassembly shows `a0` / `a1` untouched between the
/// prologue and the `jal`, so it in fact receives the caller's first two
/// arguments - the dropped-register-argument artifact, not a niladic call.
/// A live menu caller passes `(0x44, 0x02202020, …)`, i.e. a mode selector
/// and a packed RGB word, so this pair is a **shaded colour fill**, not the
/// gold 9-slice window border.
///
/// PORT: FUN_801E4140
/// REF: FUN_80034b6c - the fill-state setter this guard gates, which takes
/// the caller's mode selector and packed RGB word. Not ported.
/// REF: FUN_8002c69c - the box writer, which inflates its own rect by 8px on
/// every side. Not ported; the hosts draw their window chrome from the UI
/// atlas instead.
/// NOT WIRED: no host emits the block's box fills. What the hosts do draw is
/// NOT WIRED: the UI-atlas 9-slice window chrome, a different primitive from
/// NOT WIRED: this colour-fill pair, and they draw it at the already-inflated
/// NOT WIRED: frame rect - whereas `FUN_8002C69C` inflates its own argument
/// NOT WIRED: by 8px on every side, so this guard tests the *content* y.
/// NOT WIRED: Gating that chrome on this rect would clip on the wrong
/// NOT WIRED: coordinate and claim a guard over a draw it does not own.
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

    /// Window 31's three pens, off `FUN_801DCE20`: the heading at the
    /// content origin, the 8-cell bank one row pitch below it in the
    /// accent pen, the unit label `0x40` right of that, and the corner
    /// cursor at the fixed `(+0xE6, +0xD)` inset the whole notify family
    /// shares with window 7.
    #[test]
    fn amount_prompt_puts_its_number_a_row_below_the_heading() {
        let font = legaia_font::Font::placeholder();
        let (draws, cur) = amount_prompt_draws_for(&font, W32, "earned", 12, "pt");
        assert_eq!((cur.x, cur.y), (W32.x + 0xE6, W32.y + 0x0D));
        let row1 = W32.y + PAINTER_ROW_PITCH;
        // The 8-cell field is right-aligned, so "12" inks the last two
        // cells - never the field origin.
        let digits: Vec<i32> = draws
            .iter()
            .filter(|d| d.dst.1 == row1 && d.color == PAINTER_INK_ACCENT)
            .map(|d| d.dst.0)
            .collect();
        assert_eq!(digits.len(), 2);
        assert!(digits.iter().all(|x| *x > W32.x));
        // The unit label shares the number's row but keeps the default pen.
        assert!(
            draws
                .iter()
                .any(|d| d.dst.1 == row1 && d.dst.0 >= W32.x + 0x40 && d.color == MENU_TEXT_WHITE)
        );
        // The heading is the only thing on the origin row.
        assert!(draws.iter().any(|d| d.dst.1 == W32.y));
    }

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
    fn the_sell_row_carries_its_separator_glyph() {
        let font = legaia_font::Font::placeholder();
        let rect = PainterRect::new(14, 46, 144, 33);
        let (draws, _, _) = sell_quantity_draws_for(&font, rect, true, "h", 1, 9, 10);
        // Glyph 6 at (WX + 0x20, WY + 0x14) - one draw between the quantity
        // and the held count, which is what separates "how many" from "how
        // many you have".
        let sep = separator_glyph_draws(&font, SEPARATOR_GLYPH_SLASH, (0, 0), MENU_TEXT_WHITE);
        assert_eq!(sep.len(), 1);
        assert!(
            draws
                .iter()
                .any(|d| d.dst.0 == rect.x + 0x20 && d.dst.1 == rect.y + 0x14)
        );
    }

    #[test]
    fn only_the_three_pinned_separator_ids_resolve() {
        assert_eq!(separator_glyph_char(SEPARATOR_GLYPH_SLASH), Some('/'));
        assert_eq!(separator_glyph_char(SEPARATOR_GLYPH_COLON), Some(':'));
        assert_eq!(separator_glyph_char(SEPARATOR_GLYPH_DOT), Some('.'));
        assert_eq!(separator_glyph_char(0), None);
        let font = legaia_font::Font::placeholder();
        assert!(separator_glyph_draws(&font, 0, (0, 0), MENU_TEXT_WHITE).is_empty());
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

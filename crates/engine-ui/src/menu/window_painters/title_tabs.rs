//! Title tabs - windows 43 and 33.

use super::*;

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
/// [`crate::menu::window_dispatch`] resolves any of them to it.
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

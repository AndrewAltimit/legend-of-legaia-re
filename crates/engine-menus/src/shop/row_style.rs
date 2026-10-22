//! Shop list rows: ink rules, row metrics, cursor word, root command rows and the sell detail panel.

use super::*;

// ---------------------------------------------------------------------------
// Shop window *content* renderers (menu overlay).
//
// The shop panels are descriptor-table windows: the 9-slice frame is drawn by
// the window host, and each window's renderer VA draws content only, reading
// its content origin from the live window record's `+0xa` / `+0xc` (`WX`/`WY`).
// The kernels below are the data-derived halves of three of those renderers -
// ink selection, row geometry, cursor gating and the digit-field width law -
// with the retail glyph pushes left to the host's own text layer.
// ---------------------------------------------------------------------------

/// Normal white text ink staged into `_DAT_8007B454` before a string draw.
pub const SHOP_INK_NORMAL: u8 = 7;
/// Greyed / unavailable text ink.
pub const SHOP_INK_GREY: u8 = 0;
/// Accent ink a stock row takes when its record carries the non-zero
/// "already owned / restricted" marker at `+2`.
pub const SHOP_INK_MARKED: u8 = 6;
/// Ink the list kernel stages for a [`crate::menu_list_rows::CLASS_SHOP_ALT`]
/// row - the shop buy list's hoisted band (`li v0,0x5; sw v0,0x13c(gp)` at
/// `0x8003359C`).
pub const SHOP_INK_FEATURED: u8 = 5;

/// Row ink the kind-4 list kernel stages for one shop **buy** row.
///
/// Read off `FUN_80032A44`'s shared `0x3000` / `0xA000` arm
/// (`0x80033548..0x800335A0`), which is last-rule-wins like
/// the prize list's `legaia_engine_ui::prize_exchange::prize_row_ink` but
/// with a different rule set:
///
/// 1. ink starts at `7`;
/// 2. the row word's dim bit `0x800` -> `0`, **unless** the list is parked
///    (`_DAT_8007BB94 == 4`, a list left on screen behind a sub-screen),
///    which skips the test;
/// 3. class `0xA000` (the hoisted band) -> `5`, **even when dim** - the
///    class test runs after the dim test and overwrites it.
///
/// The dim bit is the builder's OR of `purse < price` and `held >= 99`
/// ([`crate::menu_list_rows::build_shop_buy_rows`]), recomputed here from
/// the live values. So an unaffordable hoisted row still draws in the
/// featured pen; only the confirm buzz (`0x800` again) refuses it.
///
/// PORT: FUN_80032A44 (the `0x3000`/`0xA000` row-ink arm, `0x80033548..0x800335A0`)
pub fn shop_buy_row_ink(featured: bool, held: i16, gold: i32, price: i32, parked: bool) -> u8 {
    let mut ink = SHOP_INK_NORMAL;
    let dim = gold < price || held >= SHOP_HELD_CAP as i16;
    if !parked && dim {
        ink = SHOP_INK_GREY;
    }
    if featured {
        ink = SHOP_INK_FEATURED;
    }
    ink
}

/// Vertical pitch between shop rows (retail `0xE`).
pub const SHOP_ROW_PITCH: i16 = 0x0E;
/// Text indent from the window content origin (retail `0x14`).
pub const SHOP_TEXT_INDENT: i16 = 0x14;
/// Price-field indent from the window content origin (retail `0x14 + 0x5C`).
pub const SHOP_PRICE_INDENT: i16 = 0x70;
/// Digit count of a stock row's price field.
pub const SHOP_PRICE_DIGITS: u8 = 6;

/// Decode a shop picker's cursor-state word into the hand-sprite mode for
/// `row`, or `None` when no hand draws on that row.
///
/// The word (`DAT_801E46BC` for the root picker, `_DAT_8007BB98` for the
/// stock list) packs the selection in its low 12 bits plus three flags, and
/// every shop renderer re-runs the same four-way decode per row:
///
/// * bit `0x4000` - cursor suppressed entirely (no row draws a hand);
/// * bit `0x2000` - parked/unfocused: the row-index gate is **bypassed**, so
///   every row draws, mode `4` or `0` by the blink bit;
/// * otherwise the low 12 bits must equal `row`, and the mode is the
///   inverted blink bit (`1` animated / `0` static).
///
/// PORT: FUN_801d4868 (per-row cursor gate, `0x801D48B4..0x801D4910`)
/// REF: FUN_801d5de0 - the casino prize list's renderer, which inlines the
/// same decode over `_DAT_8007BB98` at `0x801D5E40..0x801D5E9C`; the shop
/// shares the idiom, not the routine.
pub fn shop_cursor_mode(word: u32, row: u16) -> Option<u8> {
    if word & 0x4000 != 0 {
        return None;
    }
    let blink_off = (word & 0x1000) == 0;
    if word & 0x2000 != 0 {
        return Some(if blink_off { 4 } else { 0 });
    }
    if (word & 0xFFF) as u16 != row {
        return None;
    }
    Some(u8::from(blink_off))
}

/// One row of the shop root command window as retail lays it out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShopRootRow {
    /// Row index (`0` Buy, `1` Sell, `2` Quit).
    pub row: u16,
    /// Text pen, content-origin relative: `(WX + 0x14, WY + row * 0xE)`.
    pub text: (i16, i16),
    /// Text ink staged into `_DAT_8007B454` before the string draw.
    pub ink: u8,
    /// Hand-sprite mode + pen when the cursor draws on this row. The hand
    /// sits at the window origin X, not the text indent.
    pub cursor: Option<(u8, (i16, i16))>,
}

/// Build the three rows of the shop **root command window** (menu-overlay
/// window `0x2A`): Buy / Sell / Quit.
///
/// `bag_has_sellable` is the outcome of retail's inventory scan over the
/// `[id, count]` pair array at `0x80085958` across the active slot window
/// `_DAT_8007B5EA.._DAT_8007B5EC` - true when some slot has **both** bytes
/// non-zero. The subtlety the disassembly settles: the ink global is staged
/// to `7` once on entry and cleared to `0` *after* the Buy row has already
/// been drawn, so an empty bag greys **Sell and Quit together** - Buy always
/// renders white.
///
/// PORT: FUN_801d4868 (menu-overlay shop root command window content renderer)
pub fn shop_root_command_rows(
    window: (i16, i16),
    cursor_word: u32,
    bag_has_sellable: bool,
) -> [ShopRootRow; 3] {
    let (wx, wy) = window;
    let mut rows = [ShopRootRow {
        row: 0,
        text: (0, 0),
        ink: SHOP_INK_NORMAL,
        cursor: None,
    }; 3];
    for (i, out) in rows.iter_mut().enumerate() {
        let row = i as u16;
        let y = wy + row as i16 * SHOP_ROW_PITCH;
        out.row = row;
        out.text = (wx + SHOP_TEXT_INDENT, y);
        out.ink = if row == 0 || bag_has_sellable {
            SHOP_INK_NORMAL
        } else {
            SHOP_INK_GREY
        };
        out.cursor = shop_cursor_mode(cursor_word, row).map(|mode| (mode, (wx, y)));
    }
    rows
}

/// The two quantity windows' **content** - the pens, the digit ladder and
/// the separator - live with the draw in
/// `legaia_engine_ui::menu::window_painters`
/// (`buy_quantity_draws_for` / `sell_quantity_draws_for`), one builder per
/// window across both hosts. This module owns the two **sessions** that
/// step the number those windows print and the transaction they commit.
///
/// The accessory-passive index space is 64 slots; `0x40` is the
/// no-passive sentinel every non-passive row carries.
pub const PASSIVE_NONE: u8 = 0x40;

/// Resolve an item's accessory-passive index the way the shop / menu info
/// panels do: the item record's `+1` byte is an index into **one of two**
/// tables, chosen by the record's `+0` class byte.
///
/// * class `1` (equipment) - equipment record `+5`
///   (`0x80074F68 + idx*8`);
/// * anything else - item-effect record `+3` (`0x800752C0 + idx*4`).
///
/// A resolved index `>= 0x40` is the sentinel and yields `None`.
///
/// PORT: FUN_801d5ae8 (`0x801D5C5C..0x801D5CC8` - the panel runs this exact
/// chain **twice**, once for the passive's name and again for its
/// description, rather than caching the index)
///
/// Consumed by the shared window-39 draw both hosts call
/// (`legaia_engine_screens::sell_detail_window_draws`, closures off the
/// item-effect descriptors + `DiscEquipInfo::row_passive_index`). NB this chain differs
/// from the Items screen's
/// ([`crate::pause_screens::MenuTextTables::item_passive_text`], which
/// reads `legaia_game_tables::accessory_passive::AccessoryPassiveTable::passive_index`)
/// on the non-equipment arm: this port follows `FUN_801d5ae8` in sending
/// **any** non-`1` kind to the item-effect table, while the asset-crate
/// resolver (derived from `FUN_80042558`) accepts kind `2` only.
pub fn item_passive_index(
    kind: u8,
    subtype: u8,
    equip_passive: impl Fn(u8) -> u8,
    effect_passive: impl Fn(u8) -> u8,
) -> Option<u8> {
    let idx = if kind == 1 {
        equip_passive(subtype)
    } else {
        effect_passive(subtype)
    };
    (idx < PASSIVE_NONE).then_some(idx)
}

/// The price row of [`SellDetailPanel`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SellPriceRow {
    /// The sell price - exactly `buy_price >> 1`, printed as a 5-digit field.
    pub price: u16,
    /// "Price" label pen (ink `5`) at `(WX + 0x24, row_y)`.
    pub label_pen: (i16, i16),
    /// Currency glyph pen at `(WX + 0x54, row_y + 2)`.
    pub icon_pen: (i16, i16),
    /// Value pen (ink `7`) at `(WX + 0x64, row_y)`.
    pub value_pen: (i16, i16),
}

/// The item detail / sell panel's data-derived content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SellDetailPanel {
    /// Item-name pen (item record `+4`, ink `6`) at `(WX, WY)`.
    pub name_pen: (i16, i16),
    /// Description pen (item record `+8`, ink `7`) at `(WX, WY + 0xE)`.
    pub desc_pen: (i16, i16),
    /// Baseline of the price row, `WY + 0x2B`.
    pub price_row_y: i16,
    /// The price row when the item has a non-zero buy price. `None` renders
    /// the "Cannot sell" string at [`Self::cannot_sell_pen`] in ink `9`
    /// instead.
    pub sell: Option<SellPriceRow>,
    /// Pen of the "Cannot sell" string, `(WX + 0x50, price_row_y)`.
    pub cannot_sell_pen: (i16, i16),
    /// The item's accessory-passive index, when it has one.
    pub passive: Option<u8>,
    /// Passive-name pen (accessory record `+4`, ink `4`) at `(WX, WY + 0x45)`.
    pub passive_name_pen: (i16, i16),
    /// Passive-description pen (accessory record `+8`, ink `7`) at
    /// `(WX, WY + 0x55)`.
    pub passive_desc_pen: (i16, i16),
    /// The `0x90 x 0x28` shade box at `(WX, WY + 0x45)`. Drawn
    /// **unconditionally** - it is the one element that still appears when
    /// no item is staged.
    pub shade_box: ((i16, i16), (u16, u16)),
}

/// Build the **item detail / sell panel** content (menu-overlay window
/// renderer `FUN_801D5AE8`).
///
/// `staged` is the highlighted item id word `DAT_801E46B0`; retail gates the
/// whole body on it being **positive**, so id `0` (and any negative word)
/// leaves only the shade box. `price` is the item record's `+2` halfword.
///
/// PORT: FUN_801d5ae8 (menu-overlay item detail / sell panel content renderer)
///
/// This is **window 39** (rect `(14, 95, 144, 53)`, descriptor
/// `renderer_va = 0x801D5AE8`). Both play hosts lay the pens out for the
/// sell list (`legaia_engine_screens::sell_detail_window_draws`), where it
/// replaces window **34** (`item_description_draws_for`) - the buy-side info
/// window with no price row and no passive lines.
pub fn shop_sell_detail_panel(
    window: (i16, i16),
    staged: i32,
    price: u16,
    passive: Option<u8>,
) -> SellDetailPanel {
    let (wx, wy) = window;
    let price_row_y = wy + 0x2B;
    let mut panel = SellDetailPanel {
        name_pen: (wx, wy),
        desc_pen: (wx, wy + SHOP_ROW_PITCH),
        price_row_y,
        sell: None,
        cannot_sell_pen: (wx + 0x50, price_row_y),
        passive: None,
        passive_name_pen: (wx, wy + 0x45),
        passive_desc_pen: (wx, wy + 0x55),
        shade_box: ((wx, wy + 0x45), (0x90, 0x28)),
    };
    if staged <= 0 {
        return panel;
    }
    if price != 0 {
        panel.sell = Some(SellPriceRow {
            price: price >> 1,
            label_pen: (wx + 0x24, price_row_y),
            icon_pen: (wx + 0x54, price_row_y + 2),
            value_pen: (wx + 0x64, price_row_y),
        });
    }
    panel.passive = passive;
    panel
}

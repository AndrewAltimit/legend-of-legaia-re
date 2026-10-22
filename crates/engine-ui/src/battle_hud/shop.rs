//! The shop list rows and their draws.

use super::*;

/// One row in a shop or confirmation panel drawn by [`shop_draws_for`].
pub struct ShopRow<'a> {
    /// Display name for this row (item name, "Yes", "No", quantity digit, …).
    pub label: &'a str,
    /// Optional right-aligned price or value in gold. `None` for confirm /
    /// quantity rows where no price is shown.
    pub price: Option<u32>,
    /// Retail text ink for this row - the value the menu overlay stages into
    /// `_DAT_8007B454` before the string draw. `7` is normal white, `0` the
    /// greyed/unavailable pen and `6` the accent pen a stock row takes from
    /// its record's "already owned / restricted" marker; `5` the buy list's
    /// hoisted band. Callers derive it with
    /// `legaia_engine_menus::shop::shop_buy_row_ink` (buy list) or
    /// [`crate::prize_exchange::prize_row_ink`] (the casino prize list); rows
    /// with no retail ink of their own pass
    /// [`SHOP_INK_NORMAL`].
    pub ink: u8,
}

/// Normal white text ink (retail `_DAT_8007B454 == 7`).
pub const SHOP_INK_NORMAL: u8 = 7;
/// Greyed / unavailable text ink (retail `_DAT_8007B454 == 0`).
pub const SHOP_INK_GREY: u8 = 0;
/// Accent text ink (retail `_DAT_8007B454 == 6`).
pub const SHOP_INK_MARKED: u8 = 6;
/// The shop buy list's hoisted-band ink (retail `_DAT_8007B454 == 5`), which
/// the list kernel stages for a `0xA000` row; drawn in [`crate::MENU_TEXT_TEAL`],
/// the CLUT row staging value 5 selects.
pub const SHOP_INK_FEATURED: u8 = 5;

impl<'a> ShopRow<'a> {
    /// A row at the normal white ink.
    pub fn new(label: &'a str, price: Option<u32>) -> Self {
        Self {
            label,
            price,
            ink: SHOP_INK_NORMAL,
        }
    }
}

/// Build [`TextDraw`]s for a 2-D shop / confirmation panel.
///
/// Column geometry traced from `FUN_801d5de0`. That routine is the **casino
/// prize list's** row renderer, not the shop's - the `overlay_shop_save.bin`
/// dump filename names the image it was read from, not what it does (see
/// `docs/subsystems/shop.md`, "Row layout: whose list this is"). The stride and
/// column offsets below are its; the gold footer and the "shop" framing are the
/// port's, and the retail routine draws no footer at all.
///
/// ```text
/// [title]
/// > item name              1500G
///   item name               200G   ← unaffordable rows are dimmed
///   …
/// Gold: 9999G
/// ```
/// Column offsets relative to `pen`:
/// - cursor `>`: x + 0 (`CURSOR_X`)
/// - item name: x + 20 (`LABEL_X`, retail `0x14`)
/// - price (left-aligned): x + 112 (`PRICE_X`, retail `0x70`)
/// - line height: 14 px (`LINE_H`, retail `0x0E`)
///
/// Rows where `gold < price` are rendered dim; selected row has a
/// gold-coloured price. `gold = None` suppresses the gold footer line.
/// A row whose [`ShopRow::ink`] is not [`SHOP_INK_NORMAL`] overrides that
/// affordability derivation with its retail pen - `0` dim, `6` the accent
/// colour the retail stock list uses for owned/restricted stock.
///
/// A natural anchor for a PSX-style 320×240 surface is `(8, 140)`.
pub fn shop_draws_for<'a>(
    font: &legaia_font::Font,
    title: &str,
    rows: &[ShopRow<'a>],
    cursor: usize,
    gold: Option<i32>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    // Constants confirmed from FUN_801d5de0 - the casino prize list's row
    // renderer, read in the menu overlay. `overlay_shop_save` is an image name.
    const LINE_H: i32 = 14;
    const CURSOR_X: i32 = 0;
    const LABEL_X: i32 = 20; // retail 0x14
    const PRICE_X: i32 = 112; // retail 0x70, left edge of 6-digit price field

    // `SHOP_INK_NORMAL` (ink 7) is retail's default pen, which the font
    // CLUT resolves to (206, 206, 206) - not full-scale white.
    let white = crate::MENU_TEXT_WHITE;
    let dim: [f32; 4] = [0.55, 0.55, 0.55, 1.0];
    let gold_col: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let marked: [f32; 4] = [0.45, 0.68, 1.0, 1.0];

    let mut out = Vec::new();

    // Title line
    let title_layout = font.layout_ascii(title);
    out.extend(text_draws_for(&title_layout, pen, white));

    // Item rows
    for (i, row) in rows.iter().enumerate() {
        let row_y = pen.1 + LINE_H + i as i32 * LINE_H;
        let selected = i == cursor;

        // Retail dims rows the player cannot afford (gold < price).
        let can_afford = match (gold, row.price) {
            (Some(g), Some(p)) => g >= p as i32,
            _ => true,
        };
        // A retail ink other than the normal pen wins over the affordability
        // derivation: the stock list's `6` accent marks owned / restricted
        // stock the player *can* afford, and its `0` also covers a full stack.
        // The featured pen (`5`) overwrites the dim verdict in retail's
        // kernel, so an unaffordable featured row still draws teal.
        let (can_afford, ink_fg) = match row.ink {
            SHOP_INK_GREY => (false, Some(dim)),
            SHOP_INK_MARKED => (can_afford, Some(marked)),
            SHOP_INK_FEATURED => (can_afford, Some(crate::MENU_TEXT_TEAL)),
            _ => (can_afford, None),
        };
        let fg = ink_fg.unwrap_or(if !can_afford || !selected { dim } else { white });

        if selected {
            let cur_layout = font.layout_ascii(">");
            out.extend(text_draws_for(
                &cur_layout,
                (pen.0 + CURSOR_X, row_y),
                white,
            ));
        }

        let label_layout = font.layout_ascii(row.label);
        out.extend(text_draws_for(&label_layout, (pen.0 + LABEL_X, row_y), fg));

        if let Some(price) = row.price {
            let price_str = format!("{price}G");
            let price_layout = font.layout_ascii(&price_str);
            let price_fg = if !can_afford {
                dim
            } else if selected {
                gold_col
            } else {
                dim
            };
            out.extend(text_draws_for(
                &price_layout,
                (pen.0 + PRICE_X, row_y),
                price_fg,
            ));
        }
    }

    // Gold footer (retail FUN_801d0148: gold icon at panel_x, amount at x+40).
    if let Some(g) = gold {
        let gold_y = pen.1 + LINE_H + rows.len() as i32 * LINE_H + 4;
        let gold_str = format!("Gold: {g}G");
        let gold_layout = font.layout_ascii(&gold_str);
        out.extend(text_draws_for(&gold_layout, (pen.0, gold_y), gold_col));
    }

    out
}

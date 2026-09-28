//! Casino **prize-exchange** screen composition (menu-overlay sub-screen
//! `0x20`, `FUN_801DC1CC`) - the draw list both hosts blit while a
//! `PrizeExchangeSession` owns the pad.
//!
//! Retail's screen is four descriptor windows, each already ported:
//!
//! | Id | Renderer | Painter |
//! |---|---|---|
//! | 43 (`0x2B`) | `FUN_801DCFE4` | the "Exchange" title tab ([`title_tab_draws_for`]) |
//! | 44 (`0x2C`) | `FUN_801D5DE0` | the prize list - rows fed by the session (this module) |
//! | 45 (`0x2D`) | `FUN_801DD028` | the coin-bank counter ([`counter_panel_draws_for`]) |
//! | 46 (`0x2E`) | `FUN_801D603C` | the Yes/No confirm ([`choice_panel_draws_for`]) |
//!
//! Window 44's ink rule is retail's list rule, resolved by the host through
//! `legaia_engine_core::shop::shop_stock_row_ink` (the port of
//! `FUN_801D5DE0`'s ink arm) and carried on [`PrizeRow::ink`]: a stack at the
//! 99 cap greys, a non-zero record marker re-inks to the accent pen, and a
//! coin bank short of the price greys again - last rule wins. The confirm panel (46) draws
//! only while the session is in its Yes/No phase, cursor seeded to No -
//! retail's `DAT_801E46D0 = 1` convention.
//!
//! The composition takes the disc window table so every rect comes off the
//! descriptor records (`legaia_asset::menu_windows`), same as the shop
//! windows - with painter-authority dispatch: an id whose renderer moved
//! draws nothing rather than mis-drawing.
//!
//! REF: FUN_801DC1CC, FUN_801D5DE0, FUN_801DCFE4, FUN_801DD028, FUN_801D603C

use crate::ui_menu_window_dispatch::{
    COUNTER_PICTOGRAM_COINS, CounterSource, MenuWindowPainter, painter_at, painter_rect,
};
use crate::ui_menu_window_painters::{
    ChoiceFlags, PAINTER_ROW_PITCH, PainterPictogram, PainterSprite, choice_panel_draws_for,
    counter_panel_draws_for, title_tab_draws_for,
};
use crate::{MENU_TEXT_WHITE, TextDraw, text_draws_for};
use legaia_asset::menu_windows::MenuWindowTable;

/// Window ids of the exchange screen's four descriptor records.
pub const WIN_EXCHANGE_TAB: usize = 43;
pub const WIN_PRIZE_LIST: usize = 44;
pub const WIN_COIN_COUNTER: usize = 45;
pub const WIN_CONFIRM: usize = 46;

/// Row ink `0`: the grey pen `FUN_801D5DE0` stages for a row the redeem gate
/// refuses (short on coins, or a stack at the 99 cap).
pub const PRIZE_INK_GREY: u8 = 0;
/// Row ink `6`: the accent pen a non-zero record marker selects - drawn in
/// [`crate::MENU_TEXT_GOLD`], the CLUT row staging id 6 decodes to.
pub const PRIZE_INK_MARKED: u8 = 6;

/// Grey ink for a row the redeem gate would refuse.
pub const PRIZE_TEXT_GREY: [f32; 4] = [0.45, 0.45, 0.45, 1.0];

/// One visible prize row, projected by the host from its session +
/// item-name table.
#[derive(Debug, Clone)]
pub struct PrizeRow {
    /// Item name (resolved through the SCUS item-name table).
    pub name: String,
    /// Price in casino coins.
    pub price: u32,
    /// Party's held count of the item (the 99-cap ink input).
    pub held: u8,
    /// Retail row ink: `FUN_801D5DE0`'s last-rule-wins selection, which the
    /// host resolves with `legaia_engine_core::shop::shop_stock_row_ink`
    /// (held cap, the record's `+2` marker, the coin bank against the price).
    /// `0` greys the row, `6` is the marker's accent pen, anything else the
    /// normal pen.
    pub ink: u8,
}

/// The screen's full view state.
#[derive(Debug, Clone)]
pub struct PrizeExchangeView {
    pub rows: Vec<PrizeRow>,
    /// Browse cursor row index.
    pub cursor: usize,
    /// Live coin bank.
    pub coins: u32,
    /// `Some(row)` while the Yes/No confirm is up (`0` = Yes, `1` = No).
    pub confirm_cursor: Option<u8>,
}

/// Compose the exchange screen's draws off the disc window table. Returns
/// text draws, marker/cursor sprites and the coin pictogram request.
pub fn prize_exchange_draws_for(
    font: &legaia_font::Font,
    table: &MenuWindowTable,
    view: &PrizeExchangeView,
) -> (Vec<TextDraw>, Vec<PainterSprite>, Option<PainterPictogram>) {
    let mut text = Vec::new();
    let mut sprites = Vec::new();
    let mut pictogram = None;

    // 43: the "Exchange" tab (`RENDERER_TAB_EXCHANGE` shares the TitleTab
    // painter family).
    if let Some((d, _)) = painter_at(table, WIN_EXCHANGE_TAB, MenuWindowPainter::TitleTab) {
        text.extend(title_tab_draws_for(font, painter_rect(d), "Exchange"));
    }

    // 45: the coin-bank counter.
    if let Some((d, _)) = painter_at(
        table,
        WIN_COIN_COUNTER,
        MenuWindowPainter::Counter {
            pictogram: COUNTER_PICTOGRAM_COINS,
            source: CounterSource::CasinoCoins,
        },
    ) {
        let (digits, pict) = counter_panel_draws_for(
            font,
            painter_rect(d),
            COUNTER_PICTOGRAM_COINS,
            u64::from(view.coins),
        );
        text.extend(digits);
        pictogram = Some(pict);
    }

    // 44: the prize list. The descriptor names `FUN_801D5DE0`, which has no
    // painter variant (its content is the session's row walk), so the rect
    // comes straight off the record.
    if let Some(d) = table.window(WIN_PRIZE_LIST) {
        let rect = painter_rect(d);
        for (i, row) in view.rows.iter().enumerate() {
            let y = rect.y + (i as i32) * PAINTER_ROW_PITCH;
            let ink = match row.ink {
                PRIZE_INK_GREY => PRIZE_TEXT_GREY,
                PRIZE_INK_MARKED => crate::MENU_TEXT_GOLD,
                _ => MENU_TEXT_WHITE,
            };
            // Cursor marker column, then the name, then the right-ish price.
            if i == view.cursor && view.confirm_cursor.is_none() {
                text.extend(text_draws_for(&font.layout_ascii(">"), (rect.x, y), ink));
            }
            text.extend(text_draws_for(
                &font.layout_ascii(&row.name),
                (rect.x + 0x10, y),
                ink,
            ));
            text.extend(text_draws_for(
                &font.layout_ascii(&format!("{:>7}", row.price)),
                (rect.x + rect.w - 0x38, y),
                ink,
            ));
        }
        if view.rows.is_empty() {
            text.extend(text_draws_for(
                &font.layout_ascii("No prizes remain."),
                (rect.x + 0x10, rect.y),
                MENU_TEXT_WHITE,
            ));
        }
    }

    // 46: the Yes/No confirm, only while the session's confirm phase is up.
    if let Some(confirm) = view.confirm_cursor
        && let Some((d, _)) = painter_at(table, WIN_CONFIRM, MenuWindowPainter::ChoicePanel)
    {
        let (t, s) = choice_panel_draws_for(
            font,
            painter_rect(d),
            "Exchange for this prize?",
            ["Yes", "No"],
            ChoiceFlags(u32::from(confirm)),
        );
        text.extend(t);
        sprites.extend(s);
    }

    (text, sprites, pictogram)
}

// The casino **coin counter** (op-`0x49` sub-op 6, handler slot `0x25`) is
// not composed here: its windows are the field overlay's panel records `10`
// and `11`, not menu-overlay descriptor windows, and the engine lays them out
// (`legaia_engine_core::field_submode_screen::coin_counter_lines`). Both hosts
// draw those lines through `crate::ui_text_lines::pen_line_draws_for`.

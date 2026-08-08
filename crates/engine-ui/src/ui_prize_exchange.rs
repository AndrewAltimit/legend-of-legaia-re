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
//! Window 44's ink rule is retail's list rule (`FUN_801D5DE0` greys a row the
//! player cannot take): grey when the coin bank is short of the price or the
//! held stack is at the 99 cap, white otherwise. The confirm panel (46) draws
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

/// The retail per-stack cap the list ink greys at (the shop's `0x63`).
const HELD_CAP: u8 = 99;

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
            let refused = view.coins < row.price || row.held >= HELD_CAP;
            let ink = if refused {
                PRIZE_TEXT_GREY
            } else {
                MENU_TEXT_WHITE
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

/// View state of the casino **coin counter** (op-`0x49` sub-op 6, submode
/// handler slot `0x25` = `FUN_801F0ADC`) - the buy-coins-with-gold screen the
/// hosts draw off [`World::submode_screen`]'s counter cells.
///
/// [`World::submode_screen`]: ../../legaia_engine_core/world/struct.World.html
#[derive(Debug, Clone)]
pub struct CoinCounterView {
    /// The entered amount, LSB-cell-first
    /// ([`legaia_engine_vm::baka_hub_actors::CoinCounter::digits`]).
    pub digits: Vec<i8>,
    /// Which cell the up/down edits hit (wraps over the 6 editable cells).
    pub cursor: i32,
    /// This frame's affordable ceiling (`min(gold/100, bank headroom)`).
    pub ceiling: i32,
    /// Party gold.
    pub gold: i32,
    /// Live casino coin bank.
    pub coins: u32,
    /// `Some(yes_no)` while the Yes/No commit prompt is up (`0` = Yes,
    /// `1` = No - seeded to No, like the prize confirm).
    pub confirm_cursor: Option<u8>,
}

/// Compose the coin-counter screen: heading + digit entry row (cursor caret
/// under the edited cell), the affordability line, the coin-bank counter
/// (window 45) and - during the commit prompt - the Yes/No panel (window 46).
pub fn coin_counter_draws_for(
    font: &legaia_font::Font,
    table: &MenuWindowTable,
    view: &CoinCounterView,
) -> (Vec<TextDraw>, Vec<PainterSprite>, Option<PainterPictogram>) {
    let mut text = Vec::new();
    let mut sprites = Vec::new();
    let mut pictogram = None;

    // The heading + entry row anchor on the prize list's rect (the counter
    // shares the casino screen family; its own PANEL_COIN_IDLE records are
    // plain frames with no content renderer of their own).
    if let Some(d) = table.window(WIN_PRIZE_LIST) {
        let rect = painter_rect(d);
        text.extend(text_draws_for(
            &font.layout_ascii("Buy Coins - 100G each"),
            (rect.x, rect.y),
            MENU_TEXT_WHITE,
        ));
        // Digit cells, most significant first. The editable window is the
        // low 6 cells; the caret sits under the edited one.
        let n = view.digits.len();
        let digit_x = rect.x + 0x10;
        let digit_y = rect.y + PAINTER_ROW_PITCH;
        let cell_w = 0x0C;
        for (i, cell) in view.digits.iter().rev().enumerate() {
            let x = digit_x + (i as i32) * cell_w;
            text.extend(text_draws_for(
                &font.layout_ascii(&format!("{}", (*cell).clamp(0, 9))),
                (x, digit_y),
                MENU_TEXT_WHITE,
            ));
        }
        // Caret: cursor k edits cell index k (LSB-first), which renders at
        // display position n-1-k.
        let caret_pos = (n as i32 - 1 - view.cursor).clamp(0, n as i32 - 1);
        if view.confirm_cursor.is_none() {
            text.extend(text_draws_for(
                &font.layout_ascii("^"),
                (digit_x + caret_pos * cell_w, digit_y + PAINTER_ROW_PITCH),
                MENU_TEXT_WHITE,
            ));
        }
        text.extend(text_draws_for(
            &font.layout_ascii(&format!("Gold {}   Max {}", view.gold, view.ceiling.max(0))),
            (rect.x, digit_y + 2 * PAINTER_ROW_PITCH),
            MENU_TEXT_WHITE,
        ));
    }

    // 45: the coin-bank counter, shared with the prize screen.
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

    // The Yes/No commit prompt over the entry row.
    if let Some(confirm) = view.confirm_cursor
        && let Some((d, _)) = painter_at(table, WIN_CONFIRM, MenuWindowPainter::ChoicePanel)
    {
        let entered: i64 = view
            .digits
            .iter()
            .enumerate()
            .map(|(i, &c)| i64::from(c) * 10i64.pow(i as u32))
            .sum();
        let (t, s) = choice_panel_draws_for(
            font,
            painter_rect(d),
            &format!("Buy {} coins for {}G?", entered, entered * 100),
            ["Yes", "No"],
            ChoiceFlags(u32::from(confirm)),
        );
        text.extend(t);
        sprites.extend(s);
    }

    (text, sprites, pictogram)
}

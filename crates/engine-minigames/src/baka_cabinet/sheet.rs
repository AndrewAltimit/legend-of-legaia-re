//! The front-end and choice sheets: widget cells, labels and the pot placements.

/// The "PLAYER SELECT" banner at the pen state `0x0A` parks it on.
///
/// The banner is an effect actor (`FUN_801D6E04(0xC, 0x801DBA6C)`), so its
/// level is the actor's own; the port draws it at the descriptor's colour
/// (`0x80`) for as long as the cursor screen runs - an inference, since the
/// effect script's fade is not ported.
pub(super) fn player_select_banner() -> SheetCell {
    SheetCell {
        widget: WIDGET_PLAYER_SELECT,
        x: PLAYER_SELECT_PEN.0,
        y: PLAYER_SELECT_PEN.1,
        brightness: 0x80,
    }
}

// ------------------------------------------------------- the state machine

/// One widget draw on the "NEXT GAME / PAY OUT" sheet: `FUN_801D5ED0(x, y,
/// widget, brightness, 0x1000)` as the choice state issues it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SheetCell {
    /// HUD widget id.
    pub widget: u8,
    /// Screen centre.
    pub x: i32,
    pub y: i32,
    /// Colour scale (`0x80` = the descriptor's own RGB).
    pub brightness: i32,
}

/// "PRESS START" cell (widget `0x00`) - the attract card's blinking prompt.
pub const WIDGET_PRESS_START: u8 = 0x00;
/// "PLAYER SELECT" banner (widget `0x0C`).
pub const WIDGET_PLAYER_SELECT: u8 = 0x0C;
/// Where the attract arms draw [`WIDGET_PRESS_START`]
/// (`FUN_801D5ED0(0xA0, 0xCC, 0, level, 0x1000)` at `0x801CF778` /
/// `0x801CF830`).
pub const PRESS_START_PEN: (i32, i32) = (0xA0, 0xCC);
/// Where state `0x0A` parks the [`WIDGET_PLAYER_SELECT`] banner actor: the
/// spawn wrapper `FUN_801D6E04` seats it at the screen centre and the
/// cabinet rewrites `+0x14` / `+0x16` to `(0xA0, 0x20)` (`0x801CFA4C..0x801CFA60`).
pub const PLAYER_SELECT_PEN: (i32, i32) = (0xA0, 0x20);

/// "NEXT GAME" cell (widget `0x2C`).
pub const WIDGET_NEXT_GAME: u8 = 0x2C;
/// "PAY OUT" cell (widget `0x2D`).
pub const WIDGET_PAY_OUT: u8 = 0x2D;
/// "GET COIN" cell (widget `0x2E`).
pub const WIDGET_GET_COIN: u8 = 0x2E;
/// The lit cursor arrow (widget `0x30`).
pub const WIDGET_ARROW_LIT: u8 = 0x30;
/// The unlit cursor arrow (widget `0x31`).
pub const WIDGET_ARROW_DIM: u8 = 0x31;
/// Pen of the pot numeral on the sheet (`FUN_801D6F44(0x78, 0xCC, pot,
/// 0x80, 0)` at `0x801D0D28`).
pub const CHOICE_POT_PEN: (i32, i32) = (0x78, 0xCC);

/// PORT: FUN_801cf388 (`0x801D0C10`..`0x801D0D24`) - the "NEXT GAME / PAY
/// OUT" sheet's widget draws.
///
/// The picked side's label draws at `0xA0`, the other at `0x40`. The two
/// arrows sit at `x = 0x94` and `0xAC` and swap: the lit one (`0x30`) goes on
/// the picked side, blinking between `0x80` and `0xA0` on bit 4 of the blink
/// phase (`andi 0x10` / `sltiu 1` / `sll 5`), the unlit one (`0x31`) at
/// `0x40`. "GET COIN" sits under them at `(0x70, 0xCC)`; the pot numeral at
/// [`CHOICE_POT_PEN`] is the host's, off the live accumulator.
pub fn choice_sheet(menu_cursor: i32, blink: i32) -> [SheetCell; 5] {
    let lit = 0x80 | (i32::from(blink & 0x10 == 0) << 5);
    let cell = |widget, x, brightness| SheetCell {
        widget,
        x,
        y: 0x20,
        brightness,
    };
    let (left_arrow, right_arrow, next, pay) = if menu_cursor & 1 == 0 {
        (
            cell(WIDGET_ARROW_LIT, 0x94, lit),
            cell(WIDGET_ARROW_DIM, 0xAC, 0x40),
            0xA0,
            0x40,
        )
    } else {
        (
            cell(WIDGET_ARROW_DIM, 0x94, 0x40),
            cell(WIDGET_ARROW_LIT, 0xAC, lit),
            0x40,
            0xA0,
        )
    };
    [
        left_arrow,
        right_arrow,
        cell(WIDGET_NEXT_GAME, 0x4C, next),
        cell(WIDGET_PAY_OUT, 0x100, pay),
        SheetCell {
            widget: WIDGET_GET_COIN,
            x: 0x70,
            y: 0xCC,
            brightness: 0xA0,
        },
    ]
}

/// The text a glyph-less host draws for a sheet widget. The cells index the
/// PROT 1203 tally page; a play host without the duel VRAM resident
/// prints the cell's words at its
/// centre instead.
pub fn sheet_label(widget: u8) -> &'static str {
    match widget {
        WIDGET_NEXT_GAME => "NEXT GAME",
        WIDGET_PAY_OUT => "PAY OUT",
        WIDGET_GET_COIN => "GET COIN",
        WIDGET_ARROW_LIT | WIDGET_ARROW_DIM => "<>",
        WIDGET_PRESS_START => "PRESS START",
        WIDGET_PLAYER_SELECT => "PLAYER SELECT",
        _ => "?",
    }
}

/// The sheet as `(centre x, centre y, brightness, text)` rows for a
/// glyph-less host - the lit arrow points at the picked side.
pub fn choice_sheet_labels(cells: &[SheetCell]) -> Vec<(i32, i32, i32, String)> {
    cells
        .iter()
        .map(|c| {
            let text = match c.widget {
                WIDGET_ARROW_LIT if c.x < 0xA0 => "<".to_string(),
                WIDGET_ARROW_LIT => ">".to_string(),
                WIDGET_ARROW_DIM => "-".to_string(),
                w => sheet_label(w).to_string(),
            };
            (c.x, c.y, c.brightness, text)
        })
        .collect()
}

/// The pot numeral's glyph placements on the sheet: the `0x10` px coin
/// strip (`FUN_801D6F44`) at [`CHOICE_POT_PEN`], as `(x, y, digit)` for
/// `legaia_engine_ui::baka_strips::baka_digit_strip_draws_for`.
pub fn choice_pot_placements(pot: u32) -> Vec<(i32, i32, u8)> {
    crate::baka_fighter::coin_digit_cells(pot.min(i32::MAX as u32) as i32)
        .into_iter()
        .map(|c| {
            (
                CHOICE_POT_PEN.0 + c.x_offset as i32,
                CHOICE_POT_PEN.1,
                c.digit,
            )
        })
        .collect()
}

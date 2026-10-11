//! Counter windows - 32 and 45.

use super::*;

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

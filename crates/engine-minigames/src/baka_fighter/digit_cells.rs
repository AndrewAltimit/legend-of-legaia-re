//! The decimal number drawers: right-aligned score cells, coin digits and single digits.

// --- Decimal number drawers ------------------------------------------------

/// Cells in the Baka Fighter number drawers' fixed-width right-aligned field:
/// eight decimal places (`10_000_000` down to `1`). Leading-zero places are
/// blank; the units place always draws (so a zero value shows a single `0`).
pub const DIGIT_FIELD_CELLS: usize = 8;

/// Widget id of the 8px digit glyph (`FUN_801d69e4` / `FUN_801d6a18`).
pub const NUMBER_WIDGET: u8 = 0x13;
/// X advance per drawn cell for the 8px number drawer (`s1 += 8`).
pub const NUMBER_CELL_STRIDE: i16 = 8;
/// Widget id of the coin-count digit glyph (`FUN_801d6f44`, HUD widget 47).
pub const COIN_WIDGET: u8 = 0x2F;
/// X advance per drawn cell for the coin-strip drawer (`s1 += 0x10`).
pub const COIN_CELL_STRIDE: i16 = 0x10;
/// Base `u` texel column of the coin digit cell row (`u = 0x58 + digit*0x10`).
pub const COIN_U_BASE: u8 = 0x58;

/// One decimal glyph a Baka Fighter number drawer emits: which HUD widget to
/// draw, its place in the field, the x offset from the field's left edge, and
/// the `u` texel column patched into the widget descriptor for the digit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DigitCell {
    /// Field place, `0` (leftmost / highest) .. `DIGIT_FIELD_CELLS` (units).
    pub cell: usize,
    /// The decimal digit `0..=9` drawn here.
    pub digit: u8,
    /// HUD widget id drawn for this glyph.
    pub widget: u8,
    /// X offset from the field's left edge (`cell * stride`).
    pub x_offset: i16,
    /// The `u` texel column stamped into the widget descriptor.
    pub u: u8,
}

/// Lay out an integer as a right-aligned decimal field. Shared kernel of the
/// two overlay number drawers: the retail code stores each place's truncated
/// quotient `value / 10^(7-cell)` into a scratch array (skipping the ones that
/// come out zero, except the units place which is pre-seeded so it always
/// draws), then draws each surviving place as `quotient % 10`. Negative input
/// has no retail call site (score / coin counts are non-negative) and is
/// clamped to zero here.
fn field_cells(value: i32, widget: u8, stride: i16, u_of: impl Fn(u8) -> u8) -> Vec<DigitCell> {
    let value = value.max(0);
    let mut out = Vec::new();
    let mut divisor = 10_000_000i32;
    for cell in 0..DIGIT_FIELD_CELLS {
        let quotient = value / divisor;
        if quotient != 0 || cell == DIGIT_FIELD_CELLS - 1 {
            let digit = (quotient % 10) as u8;
            out.push(DigitCell {
                cell,
                digit,
                widget,
                x_offset: cell as i16 * stride,
                u: u_of(digit),
            });
        }
        divisor /= 10;
    }
    out
}

/// Wired: the duel HUD's score-total row in the play window lays its digits out
/// through this. The cells are drawn as font glyphs at their ported x offsets,
/// since the widget descriptor they patch has no sprite page behind it.
///
/// PORT: FUN_801d6a18 - the right-aligned 8px decimal number drawer.
///
/// Lays `value` out across the eight-place field as widget [`NUMBER_WIDGET`]
/// glyphs, each cell `8` px to the right of the last ([`NUMBER_CELL_STRIDE`])
/// with the digit's `u` column patched to `digit * 8` (`DAT_801d72e4`). Retail
/// also biases the glyph CLUT for the run (`DAT_801d72e2 = clut + 0x7d87`,
/// restored after) and draws through [`hud_widget_quad`] / `FUN_801d5ed0`; the
/// CLUT bias and OT-bucket scheduling are host-side, so this returns just the
/// per-digit cell layout.
pub fn right_aligned_number_cells(value: i32) -> Vec<DigitCell> {
    field_cells(value, NUMBER_WIDGET, NUMBER_CELL_STRIDE, |d| d * 8)
}

/// Wired: the duel HUD's prize row, alongside [`right_aligned_number_cells`].
///
/// PORT: FUN_801d6f44 - the coin-count digit-strip drawer (HUD widget 47).
///
/// Same right-aligned decimal decomposition as [`right_aligned_number_cells`],
/// but drawing widget [`COIN_WIDGET`] glyphs `0x10` px apart
/// ([`COIN_CELL_STRIDE`]) with the digit `u` column patched to
/// `0x58 + digit * 0x10` (`DAT_801d7514`) - the "GET COIN" numeral row on the
/// PROT 1203 tally sheet.
pub fn coin_digit_cells(value: i32) -> Vec<DigitCell> {
    field_cells(value, COIN_WIDGET, COIN_CELL_STRIDE, |d| {
        COIN_U_BASE.wrapping_add(d.wrapping_mul(0x10))
    })
}

/// Wired: the duel HUD's round-number glyph.
///
/// PORT: FUN_801d69e4 - the single 8px digit draw.
///
/// The one-glyph form the right-aligned drawer calls per place: patch widget
/// [`NUMBER_WIDGET`]'s `u` column to `digit << 3` (`DAT_801d72e4`) and draw it
/// through `FUN_801d5ed0`. Retail leaves the caller to position `x`; this
/// reports the cell with a zero x offset.
pub fn single_digit_cell(digit: u8) -> DigitCell {
    DigitCell {
        cell: 0,
        digit,
        widget: NUMBER_WIDGET,
        x_offset: 0,
        u: digit.wrapping_shl(3),
    }
}

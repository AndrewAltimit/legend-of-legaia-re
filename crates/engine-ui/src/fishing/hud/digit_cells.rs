//! The right-aligned decimal digit field.

/// One digit cell of an expanded [`HudDraw::Number`]: the digit value and the
/// screen slot it occupies. Retail draws these through a digit primitive
/// (`FUN_801d7dd8` / `FUN_801d7d44`) that is separate from the glyph emitter,
/// so they are their own draw type rather than a [`HudDraw::Glyph`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DigitCell {
    /// Screen x of this digit's slot.
    pub x: i32,
    /// Screen y (constant across the field).
    pub y: i32,
    /// The digit, `0..=9`.
    pub digit: i32,
}

/// Horizontal slot pitch of the two digit-field styles (`FUN_801d76e0`):
/// style `0` advances 8 px per slot, any other style 16 px.
pub const DIGIT_PITCH_NARROW: i32 = 8;
/// Wide digit-field pitch - see [`DIGIT_PITCH_NARROW`].
pub const DIGIT_PITCH_WIDE: i32 = 0x10;

/// The digit field is a fixed 8 slots wide; the value is right-aligned in it.
/// The same eight the dome overlay's readout fills - one width, because it is
/// one fill loop ([`crate::muscle_dome_hud::DECIMAL_SLOTS`]).
pub const DIGIT_FIELD_SLOTS: usize = crate::muscle_dome_hud::DECIMAL_SLOTS;

/// Expand a number into its digit cells - the layout half of the digit blitter
/// behind [`HudDraw::Number`].
///
/// The field is a fixed [`DIGIT_FIELD_SLOTS`]-slot row: slot `i` holds
/// `value / 10^(7 - i)` and is emitted only once that quotient is non-zero, so
/// leading zeros are *blank slots*, not drawn zeros, and the number ends up
/// right-aligned. Retail seeds the last slot with `0` before the fill loop,
/// which is what makes a `value` of zero draw a single `0` instead of nothing.
/// `style` selects the slot pitch ([`DIGIT_PITCH_NARROW`] /
/// [`DIGIT_PITCH_WIDE`]).
///
/// The **slot fill is not this routine's own**. `FUN_801d76e0` and the dome
/// overlay's `FUN_801d1308` open with the identical fill (same `-1` init, same
/// pre-seeded units slot, same `!= 0` store gate, same eight `/10` steps), so
/// it is taken from [`crate::muscle_dome_hud::decimal_slots`]. What the two
/// retail routines do *not* share is the emit half: this one selects between
/// two emitters on `style` and advances two pens (8 px / 16 px), while the dome
/// one has a single emitter and passes its digit by patching a widget
/// descriptor's texture column. That half stays here.
///
/// Negative `value` draws **nothing**, which is retail's behaviour and not a
/// guard: the fill leaves every slot at `-1` except the units one, which takes
/// the negative quotient, and both routines skip a negative slot at draw time
/// (`bltz` at `0x801D77E4` / `0x801D1408`).
// PORT: FUN_801d76e0 (8-slot right-aligned digit field: leading-zero blanking)
pub fn number_digit_cells(style: i32, x: i32, y: i32, value: i32) -> Vec<DigitCell> {
    let pitch = if style == 0 {
        DIGIT_PITCH_NARROW
    } else {
        DIGIT_PITCH_WIDE
    };

    crate::muscle_dome_hud::decimal_slots(value)
        .iter()
        .enumerate()
        .filter_map(|(i, slot)| {
            slot.map(|digit| DigitCell {
                x: x + i as i32 * pitch,
                y,
                digit: digit as i32,
            })
        })
        .collect()
}

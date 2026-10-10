//! The screen rect the widgets are laid out against.

/// Screen geometry the widgets draw against. Retail reads these from the
/// render scratchpad (`0x1F800388` X origin, `0x1F80038A` Y offset,
/// `0x1F80038E` height); the right edge is the literal `0x140`.
#[derive(Debug, Clone, Copy)]
pub struct Screen {
    /// Draw-area X origin (scratch `+0x74`).
    pub x0: i16,
    /// Y offset subtracted by the panel / letterbox draws (scratch `+0x76`).
    pub y_off: i16,
    /// Screen height (scratch `+0x7a`).
    pub height: i16,
}

/// Hardcoded right edge of every widget draw (retail literal `0x140`).
pub const SCREEN_W: i16 = 0x140;

/// The field's draw-area Y offset, scratch `0x1F80038A`: the field draws
/// into a `320 x 224` area four rows down its buffer, and the word holds
/// that `4` in every field-run frame. The image panel and the letterbox
/// take it off their script-space Y (`lhu a1,0x76(s1)` ... `subu v0,v0,a1`
/// at `0x801F86C0` / `0x801F86E4` in the panel handler) - their operands are
/// buffer rows, the packets draw-area rows.
pub const FIELD_DRAW_Y_OFFSET: i16 = 4;

impl Default for Screen {
    /// The field's screen. `height` is the port's `240`-row logical screen
    /// rather than retail's `228` (`0x1F80038E`), so a mask or a letterbox
    /// band that runs to the bottom edge also covers the rows the port draws
    /// below retail's area.
    fn default() -> Self {
        Screen {
            x0: 0,
            y_off: FIELD_DRAW_Y_OFFSET,
            height: 240,
        }
    }
}

//! A Spirit turn's AP bar and AP plate at the capture instant.
//!
//! The Spirit arm of the action seed sizes the AP bar (placement record
//! `0x0F`) off the acting member's live AGL `+0x154` less 6 and raises it
//! with the AP plate (`0x52`), whose value is the member's Spirit gauge
//! (`0x801E2F64..0x801E2FE0`); the spirit band then grows the bar one frame
//! step a pass toward `ctx[+0x6DC] - 6` and walks the plate toward
//! `ctx[+0x6DE]` (`0x801E5400..0x801E54A8`). Both live on the widgets the
//! glide records' handles name: the bar's width at `+0x0E`, the plate's
//! value at `+0x10`.
//!
//! A replay reaches the band on its own clock and with its own sizing, so the
//! image child seats both on the retail widgets' values
//! (`LEGAIA_SEAT_SPIRIT_GAUGE`), the bar's taken back across the display lag
//! while it grows.

use super::*;

/// The AP bar's and AP plate's glide records: the bar rises `(16, 234)` ->
/// `(16, 192)`, the plate slides `(328, 172)` -> `(208, 172)`.
const BAR_SEATS: [i16; 4] = [16, 192, 16, 234];
const PLATE_SEATS: [i16; 4] = [208, 172, 328, 172];
/// The handle table the glide records' widgets sit in (`ctx[+0x1074]`).
const HUD_GLIDE_HANDLES: u32 = 0x1074;
/// Action category byte `actor[+0x1DE]` of a Spirit action.
const SPIRIT_CATEGORY: u8 = 4;
/// The states a Spirit turn holds its bar and plate up through: the band's
/// entry `0x46` to the `0x51` fade-down's teardown.
const SPIRIT_STATES: std::ops::RangeInclusive<u8> = 0x46..=0x51;
const SPIRIT_BAR_WIDTH_BIAS: i32 = 6;

/// `(bar width, plate value)` as the displayed frame shows them, when the
/// capture is a Spirit turn with its bar and plate up.
pub(super) fn retail_spirit_gauge(
    ram: &[u8],
    ctx: u32,
    active: Option<u32>,
    lag: u16,
) -> Option<(i16, i16)> {
    let actor = active?;
    if game_anchors::u8_at(ram, actor + 0x1DE) != SPIRIT_CATEGORY
        || !SPIRIT_STATES.contains(&game_anchors::u8_at(ram, ctx + 7))
    {
        return None;
    }
    let widget = |seats: [i16; 4]| {
        (0..HUD_GLIDE_SLOTS).find_map(|s| {
            let r = ctx + HUD_GLIDE_TABLE + s * HUD_GLIDE_STRIDE;
            let rec: [i16; 4] =
                std::array::from_fn(|i| game_anchors::i16_at(ram, r + 4 + i as u32 * 2));
            let h = game_anchors::u32_at(ram, ctx + HUD_GLIDE_HANDLES + s * 4);
            (rec == seats && in_ram(h)).then_some(h)
        })
    };
    let bar = widget(BAR_SEATS)?;
    let plate = widget(PLATE_SEATS)?;
    let width = i32::from(game_anchors::i16_at(ram, bar + 0x0E));
    // The bar grows by the frame step a pass from its seed width; the
    // displayed frame is `lag` vsyncs behind it. Without the charge byte the
    // seed width is AGL less the bias, which the bar never shrinks below.
    let seed = i32::from(game_anchors::u16_at(ram, actor + 0x154)) - SPIRIT_BAR_WIDTH_BIAS;
    let width = if game_anchors::u8_at(ram, actor + 0x1F9) == 0 && width > seed {
        (width - i32::from(lag)).max(seed)
    } else {
        width
    };
    Some((width as i16, game_anchors::i16_at(ram, plate + 0x10)))
}

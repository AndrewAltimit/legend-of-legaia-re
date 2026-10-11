//! The shutter blackout's two bars as screen quads.
//!
//! The field overlay's shutter-blackout actor (`FUN_801DD784`, template
//! `0x801F2858`) runs a four-phase height envelope - the clock lives in
//! `legaia_engine_field::field_actor_timers::ShutterBars` - and emits two
//! black `POLY_F4` quads from the current height. This is the emit half:
//! the retail rectangle literals and the two rects a bar height produces,
//! which [`crate::screen_prim::cinematic_bar_prims`] turns into prims.

/// Right edge of both quads (retail literal `0x140`).
pub const SHUTTER_SCREEN_W: i16 = 0x140;

/// Bottom edge retail hardcodes (`0xE0` = 224). The engine's screen-overlay
/// stage is 240 lines, so the prim builder takes the stage height as a
/// parameter and this constant is what a parity check compares against.
pub const SHUTTER_RETAIL_SCREEN_H: i16 = 0xE0;

/// The top bar's Y bias (retail writes `-4` into both of its top vertices, so
/// the bar starts four lines above the screen and its visible height is
/// `bar - 4`).
pub const SHUTTER_TOP_BIAS: i16 = -4;

/// One bar as retail lays its four vertices out: `[x0, y0, x1, y1]` with the
/// quad spanning `x0..x1`, `y0..y1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarRect {
    pub x0: i16,
    pub y0: i16,
    pub x1: i16,
    pub y1: i16,
}

/// The two quads a bar height emits, top first.
///
/// REF: FUN_801DD784 (`0x801DD8F8..0x801DD9B4`) - two `0x05000000`-tagged
/// packets, command byte `0x28` (`POLY_F4`, opaque), with the colour bytes
/// **zeroed after** the `0x28808080` word is stored, so the fill is black and
/// not the mid-grey the immediate suggests.
///
/// `screen_h` is the stage bottom the second bar is flush with. Retail's
/// literal is [`SHUTTER_RETAIL_SCREEN_H`]; the engine's screen-overlay
/// stage is 240 lines, so the host passes its own display height and the bar
/// stays flush instead of floating 16 lines up.
pub fn shutter_bar_rects(bar: i16, screen_h: i16) -> [BarRect; 2] {
    [
        BarRect {
            x0: 0,
            y0: SHUTTER_TOP_BIAS,
            x1: SHUTTER_SCREEN_W,
            y1: bar + SHUTTER_TOP_BIAS,
        },
        BarRect {
            x0: 0,
            y0: screen_h - bar,
            x1: SHUTTER_SCREEN_W,
            y1: screen_h,
        },
    ]
}

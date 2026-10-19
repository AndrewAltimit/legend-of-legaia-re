//! The segmented gauge bar frames and their fill ramps.

/// The three-glyph frame of a gauge bar: a start cap, a body stretched over
/// the segment count, and an end cap. Both bar animators emit this triple
/// through the shared glyph emitter, then overlay the fill quad themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarFrame {
    /// The start-cap / body / end-cap glyph ids, in emit order.
    pub glyphs: [u32; 3],
    /// Screen position of each of the three glyphs.
    pub positions: [(i32, i32); 3],
    /// The body glyph's stretch factor in 12.4 fixed point (`segments << 12`),
    /// applied along the bar's axis.
    pub body_scale: i32,
    /// Length in pixels of the filled portion of the bar,
    /// `segments * value * 8 / 0x1000`.
    pub fill_len: i32,
    /// The fill quad's brightness ramp, `value * 0xff / 0x1000` - the bar
    /// brightens as it fills. Note the *glyph* frame is emitted at a fixed
    /// `0x80`; only the fill tracks the value.
    pub fill_brightness: i32,
    /// The RGB written to all four fill-quad vertices, selected by the
    /// `style` argument. `None` when retail writes no colour at all
    /// (see [`bar_frame`]); the four vertices always share one triple.
    pub fill_rgb: Option<(u8, u8, u8)>,
    /// Which axis the body stretch and the fill extent run along. Not a
    /// retail field - the two bar helpers *are* the two axes
    /// ([`bar_frame`] horizontal, [`power_bar_frame`] vertical), and a
    /// consumer needs to know which one it is holding to size the fill quad.
    pub axis: BarAxis,
}

impl BarFrame {
    /// The fill quad's screen rect `(x0, y0, x1, y1)` as the bar helpers write
    /// it into their `POLY_GT4` - or `None` when there is nothing to fill.
    ///
    /// - [`BarAxis::Horizontal`] (`FUN_801d1870`, `0x801D1974..0x801D19E0`):
    ///   `x + 8 ..= x + 8 + fill`, `y + 5 ..= y + 11` - an inset strip that
    ///   starts past the start cap.
    /// - [`BarAxis::Vertical`] (`FUN_801d1a90`, `0x801D1B10..0x801D1BB8`):
    ///   `x + 5 ..= x + 11`, from the bottom cap's `y` up by `fill`.
    ///
    /// Both helpers `AddPrim` the fill *before* their three frame glyphs into
    /// the same bucket, so the ordering-table walk draws it **over** the
    /// frame; the frame's palette is STP-clear, so a fill drawn under it would
    /// be hidden.
    pub fn fill_rect(&self) -> Option<(i32, i32, i32, i32)> {
        if self.fill_len <= 0 {
            return None;
        }
        let (x, y) = self.positions[0];
        Some(match self.axis {
            BarAxis::Horizontal => (x + 8, y + 5, x + 8 + self.fill_len, y + 0xb),
            BarAxis::Vertical => {
                let end = self.positions[2].1;
                (x + 5, end - self.fill_len, x + 0xb, end)
            }
        })
    }
}

/// The axis a [`BarFrame`] runs along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarAxis {
    /// `FUN_801d1870`: caps left and right, fill grows left-to-right.
    Horizontal,
    /// `FUN_801d1a90`: caps top and bottom, fill grows *upward* from the
    /// bottom cap.
    Vertical,
}

/// Constant red channel of the style-0 fill ramp (`li v0, 0xbc`).
pub const BAR_FILL_STYLE0_RED: u8 = 0xbc;

/// Resolve `FUN_801d1870`'s `param_1` style selector into the fill-quad
/// vertex colour, given the already-scaled brightness byte.
///
/// Retail branches three ways, and only the first two write anything:
///
/// - `0` - `(0xbc, brightness, 0)`: a constant red against the ramp.
/// - `1` - `(brightness, !brightness, 0)`: the ramp against its own
///   bitwise complement, so the bar crossfades as it fills.
/// - anything else - the colour stores are jumped over entirely
///   (`j LAB_801d1974`), leaving whatever the primitive buffer held.
///
/// The retail call sites use `0` for the depth gauge and `1` for the
/// tension gauge; the third arm is unreachable from them.
fn bar_fill_rgb(style: i32, brightness: i32) -> Option<(u8, u8, u8)> {
    let b = brightness as u8;
    match style {
        0 => Some((BAR_FILL_STYLE0_RED, b, 0)),
        1 => Some((b, !b, 0)),
        _ => None,
    }
}

/// Emit brightness of the bar frame glyphs - fixed, unlike the fill.
pub const BAR_FRAME_BRIGHTNESS: i32 = 0x80;

/// Fixed-point unit of the bar `value` (`0x1000` = completely full).
pub const BAR_VALUE_ONE: i32 = 0x1000;

/// The **horizontal** gauge bar behind [`HudDraw::Bar`] (depth / tension at
/// the retail call sites): caps at `x` and `x + segments*8 + 8` with the body
/// stretched between them, filling left-to-right.
///
/// `style` (retail `param_1`) selects the fill quad's colour ramp only - see
/// [`bar_fill_rgb`] - and moves no geometry.
///
/// The retail `>> 12` carries a `+0xfff` negative bias, which is just C
/// division truncating toward zero; the port divides directly.
// PORT: FUN_801d1870 (horizontal gauge bar: cap/body/cap frame + fill extent
// PORT: + the param_1 style ramp)
pub fn bar_frame(x: i32, y: i32, value: i32, segments: i32, style: i32) -> BarFrame {
    let fill_brightness = value * 0xff / BAR_VALUE_ONE;
    BarFrame {
        glyphs: [3, 4, 5],
        positions: [(x, y), (x + 8, y), (x + segments * 8 + 8, y)],
        body_scale: segments << 12,
        fill_len: segments * value * 8 / BAR_VALUE_ONE,
        fill_brightness,
        fill_rgb: bar_fill_rgb(style, fill_brightness),
        axis: BarAxis::Horizontal,
    }
}

/// The **vertical** gauge bar behind [`HudDraw::PowerBar`] (the casting-power
/// meter): the same cap/body/cap frame rotated onto the y axis, with glyph ids
/// `0`/`1`/`2` and the body stretched vertically. It fills *upward* - the fill
/// quad grows from the bottom cap at `y + segments*8 + 8` back toward the top.
///
/// Unlike [`bar_frame`] this helper takes **no** style argument: retail's
/// `FUN_801d1a90` is a four-argument function that stores `0xbc` into the
/// red channel unconditionally, i.e. it is permanently the style-0 ramp.
// PORT: FUN_801d1a90 (vertical power bar: cap/body/cap frame + upward fill)
pub fn power_bar_frame(x: i32, y: i32, value: i32, segments: i32) -> BarFrame {
    let end = y + segments * 8 + 8;
    let fill_brightness = value * 0xff / BAR_VALUE_ONE;
    BarFrame {
        glyphs: [0, 1, 2],
        positions: [(x, y), (x, y + 8), (x, end)],
        body_scale: segments << 12,
        fill_len: segments * value * 8 / BAR_VALUE_ONE,
        fill_brightness,
        fill_rgb: Some((BAR_FILL_STYLE0_RED, fill_brightness as u8, 0)),
        axis: BarAxis::Vertical,
    }
}

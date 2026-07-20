//! Fishing-minigame HUD: the retail draw-list layout and its host consumer.
//!
//! Two halves. The **layout** half is a port of the fishing overlay's draw
//! helpers (PROT 0972, `data\OTHER1`) - the persistent HUD `FUN_801d13f0`, the
//! catch HUD `FUN_801d1580`, the two gauge bars `FUN_801d1870` /
//! `FUN_801d1a90`, the digit field `FUN_801d76e0`, and the five one-shot
//! banner animators the mode driver's shared tail services. Each builder
//! returns [`HudDraw`] items carrying the retail call-site constants, so the
//! layout is decided here and nothing about it depends on a GPU backend.
//!
//! The **consumer** half is [`fishing_hud_draws_for`], the sibling of
//! `battle_hud_draws_for`: it turns a [`HudDraw`] list into the crate's
//! [`TextDraw`] / [`SpriteDraw`] quads a host renderer already knows how to
//! rasterise. Numbers and counts go through the ported digit field and are
//! drawn from the proportional font atlas; captions come from the host
//! (the retail strings are overlay rodata, not committed); glyph ids resolve
//! through a host-supplied atlas lookup, since the fishing sprite page is not
//! itself ported.
//!
//! The rules half of the minigame - the cast oscillator, the tension
//! tug-of-war, catch scoring, the prize exchange - lives in
//! `legaia_engine_core::fishing`, which owns the numeric kernels this module
//! only displays.

use crate::{SpriteDraw, TextDraw, text_draws_for};

/// The line-record base offset the catch-HUD length readout subtracts
/// (`FUN_801d1580`: `record - 300`, clamped at zero). Same retail literal as
/// the hook check's `record < gate + 300`, whose copy lives with the rules
/// kernels (`legaia_engine_core::fishing::RECORD_STRIKE_BASE`).
pub const RECORD_STRIKE_BASE: i32 = 300;

/// The point total the persistent HUD clamps its row to (`FUN_801d13f0`).
/// Same retail literal as the persistent counter's own clamp in
/// `FUN_801d5298`, whose copy lives with the rules kernels
/// (`legaia_engine_core::fishing::FISH_POINTS_CAP`).
pub const HUD_POINT_CAP: i32 = 999_999;

/// Glyph / digit brightness of the persistent + catch HUD rows
/// (`FUN_801d13f0` / `FUN_801d1580`: the `0x80` brightness argument).
pub const HUD_BRIGHTNESS: i32 = 0x80;
/// Full brightness (`0xff`): the hooked-gauge block and the banner sprites.
pub const HUD_BRIGHTNESS_FULL: i32 = 0xff;

/// Frames a slide banner stays live (`FUN_801d75dc` / `FUN_801d78ec`:
/// active while `frame < 0xc8`).
pub const BANNER_FRAMES: i32 = 0xc8;
/// Frames the strike splash stays live (`FUN_801d71d4`: `frame < 0x98`).
pub const SPLASH_FRAMES: i32 = 0x98;

/// One primitive of the fishing HUD draw list. Each variant models a call
/// into one of the overlay's shared draw helpers; the coordinates, glyph ids,
/// and brightness values are the retail call-site constants. Rendering is the
/// host's job - this module only decides *what* is drawn where.
// REF: FUN_801d63b0 (shared sprite-quad emitter)
// REF: FUN_801d26cc (the driver whose seed sites arm the banner timers)
// Each variant names the retail helper it stands for. Three of those
// helpers are themselves ported and carry their own PORT tags, so a
// variant resolves into them rather than replacing them: FUN_801d1870 /
// FUN_801d1a90 -> `bar_frame` / `power_bar_frame` (via
// `HudDraw::resolve_bar`), and FUN_801d76e0 -> `number_digit_cells`.
// FUN_801d63b0 is genuinely unported: it is a pure VRAM quad emitter with
// no decision content, so the variant just carries its call-site
// arguments and the host does the drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HudDraw {
    /// A number via the digit blitter `FUN_801d76e0`.
    Number {
        x: i32,
        y: i32,
        value: i32,
        brightness: i32,
    },
    /// A single sprite-quad glyph via the shared emitter `FUN_801d63b0`,
    /// drawn at `0x1000` (1.0) scale. `layer` is the emitter's first argument
    /// (`1` on the HUD rows, `0` on the banner sprites; a draw-class selector,
    /// not further pinned).
    Glyph {
        layer: i32,
        id: u32,
        x: i32,
        y: i32,
        brightness: i32,
    },
    /// A fixed-width count via the shared number primitive (`0x80034b78`).
    Count {
        value: i32,
        digits: u32,
        x: i32,
        y: i32,
    },
    /// A caption via the shared string primitive (`0x80036888`). The text
    /// bytes live in the overlay rodata (not committed); the variant names
    /// the string symbolically.
    Caption { text: HudCaption, x: i32, y: i32 },
    /// A gauge bar via `FUN_801d1870` (`style` 0 = depth, 1 = tension at the
    /// retail call sites; `step` is its per-segment argument).
    Bar {
        style: i32,
        x: i32,
        y: i32,
        value: i32,
        step: i32,
    },
    /// The casting-power meter bar via `FUN_801d1a90`.
    PowerBar {
        x: i32,
        y: i32,
        power: i32,
        step: i32,
    },
}

impl HudDraw {
    /// Resolve a bar variant into the concrete frame + fill its retail
    /// helper would build, routing [`HudDraw::Bar`] through [`bar_frame`]
    /// and [`HudDraw::PowerBar`] through [`power_bar_frame`].
    ///
    /// Returns `None` for every non-bar variant - those name emitters that
    /// carry no decision content and are left to the host.
    pub fn resolve_bar(self) -> Option<BarFrame> {
        match self {
            HudDraw::Bar {
                style,
                x,
                y,
                value,
                step,
            } => Some(bar_frame(x, y, value, step, style)),
            HudDraw::PowerBar { x, y, power, step } => Some(power_bar_frame(x, y, power, step)),
            _ => None,
        }
    }
}

/// Which overlay-rodata caption a [`HudDraw::Caption`] refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HudCaption {
    /// The selected rod/lure type label (three overlay strings, picked by the
    /// persistent index `_DAT_80084450` = 0/1/2; other values draw no label).
    RodName(u32),
    /// The "remaining" caption drawn before the lure count.
    LuresLeft,
    /// The trailing caption drawn after the lure count.
    LureCountSuffix,
}
/// The persistent fishing HUD (drawn every frame by the mode driver's shared
/// tail): the best-catch row (glyph `0x1a`), the capped point-total row
/// (glyph `0x1c`), the selected rod/lure label, and the lures-remaining
/// count. Retail reads `_DAT_80084458` / `_DAT_8008444c` / `_DAT_80084450`
/// and the live inventory count of the paired lure item
/// (`legaia_engine_core::fishing::lure_item_id`); here they are caller
/// parameters.
// PORT: FUN_801d13f0 (persistent HUD: best-catch + capped point rows, rod label, lure count)
pub fn persistent_hud_draws(
    points: i32,
    best_points: i32,
    rod_index: u32,
    lure_count: i32,
) -> Vec<HudDraw> {
    let mut d = vec![
        HudDraw::Number {
            x: 0x32,
            y: 0x08,
            value: best_points,
            brightness: HUD_BRIGHTNESS,
        },
        HudDraw::Glyph {
            layer: 1,
            id: 0x1a,
            x: 0x10,
            y: 0x08,
            brightness: HUD_BRIGHTNESS,
        },
        HudDraw::Number {
            x: 0x32,
            y: 0x16,
            value: points.min(HUD_POINT_CAP),
            brightness: HUD_BRIGHTNESS,
        },
        HudDraw::Glyph {
            layer: 1,
            id: 0x1c,
            x: 0x10,
            y: 0x16,
            brightness: HUD_BRIGHTNESS,
        },
    ];
    // Rod indices 0..=2 pick one of three overlay label strings; any other
    // index draws no label but still draws the count row.
    if rod_index <= 2 {
        d.push(HudDraw::Caption {
            text: HudCaption::RodName(rod_index),
            x: 0x98,
            y: 0x0c,
        });
    }
    d.push(HudDraw::Caption {
        text: HudCaption::LuresLeft,
        x: 0xf3,
        y: 0x0c,
    });
    d.push(HudDraw::Count {
        value: lure_count,
        digits: 4,
        x: 0x100,
        y: 0x0c,
    });
    d.push(HudDraw::Caption {
        text: HudCaption::LureCountSuffix,
        x: 0x12a,
        y: 0x0c,
    });
    d
}

/// The live values the catch HUD reads (retail globals -> parameters).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CatchHudState {
    /// Line length / catch record value (`DAT_801d927c`).
    pub record: i32,
    /// The second length term (`DAT_801d9178`): displayed alone as the lower
    /// readout and added into the total length in the same `>>9` scale (the
    /// cast line-projection component of the readout).
    pub line_extent: i32,
    /// Casting-power meter (`DAT_801d9274`).
    pub cast_power: i32,
    /// Line depth / sink value (`DAT_801d9298`).
    pub depth: i32,
    /// Tension gauge (`DAT_801d9168`).
    pub tension: i32,
    /// `DAT_801d91b4` - set at the hook; gates the depth + tension gauge
    /// block of the catch HUD.
    pub gauges_visible: bool,
}

/// The `DAT_801d9178` display term: `((x >> 8) + (x >>> 31)) >> 1` (an
/// arithmetic `/512` with round-toward-zero), clamped at zero - the exact
/// `FUN_801d1580` sequence.
pub fn extent_display(line_extent: i32) -> i32 {
    (((line_extent >> 8) + ((line_extent as u32) >> 31) as i32) >> 1).max(0)
}

/// The catch HUD's total-length readout, in tenths of a display unit:
/// `max(record - 300, 0) * 100 >> 9` plus [`extent_display`]. The HUD splits
/// it as `value / 10` (whole part) and `value % 10` (tenths digit).
pub fn length_display(record: i32, line_extent: i32) -> i32 {
    let past = (record - RECORD_STRIKE_BASE).max(0);
    let mut scaled = past * 100;
    if scaled < 0 {
        // Retail's negative-rounding adjust before the >>9; unreachable for
        // the clamped input, kept for the exact arithmetic.
        scaled += 0x1ff;
    }
    (scaled >> 9) + extent_display(line_extent)
}

/// The cast-power percent readout: `power * 100 >> 12` (percent of the
/// `0x1000` meter ceiling, with retail's negative-rounding adjust).
pub fn cast_power_percent(power: i32) -> i32 {
    let mut scaled = power * 100;
    if scaled < 0 {
        scaled += 0xfff;
    }
    scaled >> 12
}

/// The catch HUD, drawn while a cast is out: the total-length readout (its
/// whole and tenths digits, glyphs `0xb`/`0x10`), the extent readout (glyph
/// `0xa`), the cast-power percent (glyph `0xe`) plus the power bar, and -
/// once hooked ([`CatchHudState::gauges_visible`]) - the depth and tension
/// gauge bars (glyphs `8`/`9`). Retail also emits a debug length line behind
/// the global print flag `_DAT_8007b9b0`; that log call is not modeled.
// PORT: FUN_801d1580 (catch HUD: length/extent/power readouts + hooked gauge block)
pub fn catch_hud_draws(s: &CatchHudState) -> Vec<HudDraw> {
    let len = length_display(s.record, s.line_extent);
    let ext = extent_display(s.line_extent);
    let pct = cast_power_percent(s.cast_power);
    let b = HUD_BRIGHTNESS;
    let mut d = vec![
        HudDraw::Number {
            x: 0xda,
            y: 0x30,
            value: len / 10,
            brightness: b,
        },
        HudDraw::Number {
            x: 0xe8,
            y: 0x30,
            value: len % 10,
            brightness: b,
        },
        HudDraw::Glyph {
            layer: 1,
            id: 0xb,
            x: 0xd4,
            y: 0x30,
            brightness: b,
        },
        HudDraw::Glyph {
            layer: 1,
            id: 0x10,
            x: 0x114,
            y: 0x30,
            brightness: b,
        },
        HudDraw::Number {
            x: 0xda,
            y: 0xc0,
            value: ext / 10,
            brightness: b,
        },
        HudDraw::Number {
            x: 0xe8,
            y: 0xc0,
            value: ext % 10,
            brightness: b,
        },
        HudDraw::Glyph {
            layer: 1,
            id: 0xa,
            x: 0xd4,
            y: 0xc0,
            brightness: b,
        },
        HudDraw::Number {
            x: 0xe4,
            y: 0xb0,
            value: pct,
            brightness: b,
        },
        HudDraw::Glyph {
            layer: 1,
            id: 0xe,
            x: 0xd4,
            y: 0xb0,
            brightness: b,
        },
        HudDraw::PowerBar {
            x: 0x120,
            y: 0x40,
            power: s.cast_power,
            step: 0xc,
        },
    ];
    if s.gauges_visible {
        d.extend([
            HudDraw::Glyph {
                layer: 1,
                id: 8,
                x: 0x10,
                y: 0x80,
                brightness: HUD_BRIGHTNESS_FULL,
            },
            HudDraw::Bar {
                style: 0,
                x: 0x10,
                y: 0x90,
                value: s.depth,
                step: 10,
            },
            HudDraw::Glyph {
                layer: 1,
                id: 9,
                x: 0x10,
                y: 0xa0,
                brightness: HUD_BRIGHTNESS_FULL,
            },
            HudDraw::Bar {
                style: 1,
                x: 0x10,
                y: 0xb0,
                value: s.tension,
                step: 10,
            },
        ]);
    }
    d
}

/// Shared slide ramp of the two banner animators: `frame * 8` up to the
/// `0xa0` hold (reached at frame `0x14`), held until frame `0x8c`, then
/// `frame * 8 - 0x3c0` sliding off (the ramps join continuously at `0xa0`).
/// Retail leaves the value undefined for a negative frame (the timers only
/// ever pass `>= 1`); this clamps to the frame-0 value.
fn banner_slide(frame: i32) -> i32 {
    let mut v = frame.max(0) * 8;
    if frame >= 0x14 {
        v = 0xa0;
    }
    if frame >= 0x8c {
        v = frame * 8 - 0x3c0;
    }
    v
}

/// One frame of the banner that slides in from the **left** (glyph `7` at
/// `y = 0x78`, x = the slide ramp). Its timer (`DAT_801d9160`) is seeded at
/// the moment the fish hooks (`FUN_801d26cc`, alongside the gauge-block
/// enable). Returns the draw while active, `None` once the frame count
/// reaches [`BANNER_FRAMES`] (retail returns the active flag).
// PORT: FUN_801d78ec (hook banner: left slide-in, hold, slide-off)
pub fn banner_from_left_draw(frame: i32) -> Option<HudDraw> {
    if frame >= BANNER_FRAMES {
        return None;
    }
    Some(HudDraw::Glyph {
        layer: 0,
        id: 7,
        x: banner_slide(frame),
        y: 0x78,
        brightness: HUD_BRIGHTNESS_FULL,
    })
}

/// One frame of the banner that slides in from the **right** (glyph `0xd` at
/// `y = 0x78`, x = `0x140 -` the slide ramp - the mirrored trajectory of
/// [`banner_from_left_draw`], holding at the same `x = 0xa0`). Its timer
/// (`DAT_801d915c`) is seeded on the reel-in-complete path of the hooked
/// fight (`FUN_801d26cc`: record below `0x136` while hooked); while it runs,
/// the driver tail forces the from-left timer back to zero.
// PORT: FUN_801d75dc (reel-in banner: right slide-in, hold, slide-off)
pub fn banner_from_right_draw(frame: i32) -> Option<HudDraw> {
    if frame >= BANNER_FRAMES {
        return None;
    }
    Some(HudDraw::Glyph {
        layer: 0,
        id: 0xd,
        x: 0x140 - banner_slide(frame),
        y: 0x78,
        brightness: HUD_BRIGHTNESS_FULL,
    })
}

/// One frame of the miss / retry banner (glyph `0x19` at `y = 0x78`), the
/// mirrored trajectory of [`banner_from_left_draw`] over the shared ramp. Its
/// timer (`DAT_801d9268`) is the retry countdown the driver runs in state
/// `0x2d` before returning to the cast state. `None` once the frame count
/// reaches [`BANNER_FRAMES`] (retail returns the active flag, which is what
/// keeps the state machine parked).
// PORT: FUN_801d6f10 (miss/retry banner: right slide-in, hold, slide-off)
pub fn banner_miss_draw(frame: i32) -> Option<HudDraw> {
    if frame >= BANNER_FRAMES {
        return None;
    }
    Some(HudDraw::Glyph {
        layer: 0,
        id: 0x19,
        x: 0x140 - banner_slide(frame),
        y: 0x78,
        brightness: HUD_BRIGHTNESS_FULL,
    })
}

/// One frame of the auxiliary two-sided banner: the *same* glyph (`0xc`)
/// emitted twice, once at the ramp and once mirrored at `0x140 -` the ramp, so
/// the pair converges on the `0xa0` hold from both edges and parts again on
/// the way out. Its timer (`DAT_801d9164`) is what the driver's state `0x28`
/// waits on before returning to the main loop.
// PORT: FUN_801d7528 (auxiliary banner: mirrored converging glyph pair)
pub fn banner_converge_draws(frame: i32) -> Option<[HudDraw; 2]> {
    if frame >= BANNER_FRAMES {
        return None;
    }
    let x = banner_slide(frame);
    let glyph = |x: i32| HudDraw::Glyph {
        layer: 0,
        id: 0xc,
        x,
        y: 0x78,
        brightness: HUD_BRIGHTNESS_FULL,
    };
    Some([glyph(x), glyph(0x140 - x)])
}

/// The strike-splash brightness ramp: `frame * 8` up to the `0x80` hold
/// (reached at frame `0x10`), held until frame `0x88`, then
/// `0x80 - (frame - 0x88) * 8` fading out (zero exactly at the `0x98`
/// lifetime end).
pub fn splash_brightness(frame: i32) -> i32 {
    let mut a = frame.max(0) * 8;
    if frame >= 0x10 {
        a = 0x80;
    }
    if frame >= 0x88 {
        a = 0x80 - (frame - 0x88) * 8;
    }
    a
}

/// One frame of the strike splash: a two-glyph pair (`0x416` / `0x816`) at
/// `x = 0xa0` that rises one pixel every 32 frames from `y = 0x50` while
/// fading through [`splash_brightness`]. Its timer (`DAT_801d90f0`) is seeded
/// at the strike / hit event before the fish hooks (`FUN_801d26cc`, gated on
/// the gauge block not yet being up). `None` once the frame count reaches
/// [`SPLASH_FRAMES`].
// PORT: FUN_801d71d4 (strike splash: rising, fading two-glyph pair)
pub fn strike_splash_draws(frame: i32) -> Option<[HudDraw; 2]> {
    if frame >= SPLASH_FRAMES {
        return None;
    }
    let y = 0x50 - (frame.max(0) >> 5);
    let brightness = splash_brightness(frame);
    let glyph = |id: u32| HudDraw::Glyph {
        layer: 0,
        id,
        x: 0xa0,
        y,
        brightness,
    };
    Some([glyph(0x416), glyph(0x816)])
}

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
pub const DIGIT_FIELD_SLOTS: usize = 8;

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
/// Retail applies no negative guard; a negative `value` there yields negative
/// quotients. The port clamps at zero instead, since every call site passes a
/// count or a score.
// PORT: FUN_801d76e0 (8-slot right-aligned digit field: leading-zero blanking)
pub fn number_digit_cells(style: i32, x: i32, y: i32, value: i32) -> Vec<DigitCell> {
    let value = value.max(0);
    let pitch = if style == 0 {
        DIGIT_PITCH_NARROW
    } else {
        DIGIT_PITCH_WIDE
    };

    // Slot contents: `-1` = blank, else the quotient at that power of ten.
    let mut slots = [-1i32; DIGIT_FIELD_SLOTS];
    slots[DIGIT_FIELD_SLOTS - 1] = 0;
    let mut pow = 10_000_000i32;
    for slot in slots.iter_mut() {
        let q = value / pow;
        if q != 0 {
            *slot = q;
        }
        pow /= 10;
    }

    slots
        .iter()
        .enumerate()
        .filter_map(|(i, &q)| {
            (q >= 0).then_some(DigitCell {
                x: x + i as i32 * pitch,
                y,
                digit: q % 10,
            })
        })
        .collect()
}

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

/// One of the driver tail's auxiliary one-shot animation timers
/// (`DAT_801d9160` / `DAT_801d915c` / `DAT_801d90f0`): zero = idle, seeded to
/// `1` to start, advanced by the frame step (`DAT_1f800393`) each frame its
/// animator reports active, and reset to zero when the animation expires.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BannerTimer(pub i32);

impl BannerTimer {
    /// Seed the timer to `1` (the retail start value).
    pub fn start(&mut self) {
        self.0 = 1;
    }

    /// Force the timer idle (retail zeroes a timer to cancel its banner -
    /// e.g. the from-right banner cancels the from-left one while it runs).
    pub fn cancel(&mut self) {
        self.0 = 0;
    }

    /// `true` while the timer is running.
    pub fn is_active(&self) -> bool {
        self.0 != 0
    }

    /// Service one frame: while active, run `animator` on the current frame
    /// count; advance by `frame_step` if it drew, reset to idle if it
    /// expired. Returns the animator's draw output.
    // PORT: FUN_801cf3bc shared tail LAB_801d01a4 (banner-timer service loop)
    pub fn service<T>(
        &mut self,
        frame_step: i32,
        animator: impl FnOnce(i32) -> Option<T>,
    ) -> Option<T> {
        if self.0 == 0 {
            return None;
        }
        let out = animator(self.0);
        if out.is_some() {
            self.0 += frame_step.max(1);
        } else {
            self.0 = 0;
        }
        out
    }
}

// --- host consumer -----------------------------------------------------------

/// The caption strings a [`HudDraw::Caption`] resolves to.
///
/// The retail strings are fishing-overlay rodata (Sony bytes, not committed),
/// so the host supplies them - from a translation pack, or from
/// [`FishingCaptions::placeholder`] for a dev build.
#[derive(Debug, Clone, Copy)]
pub struct FishingCaptions<'a> {
    /// The three rod/lure labels, indexed by the persistent rod index.
    pub rod_names: [&'a str; 3],
    /// Caption drawn before the lure count.
    pub lures_left: &'a str,
    /// Caption drawn after the lure count.
    pub lure_count_suffix: &'a str,
}

impl FishingCaptions<'static> {
    /// Engine-side English placeholders. These are **not** the retail
    /// strings - they exist so a dev build draws a legible HUD before a
    /// translation pack is loaded.
    pub fn placeholder() -> Self {
        FishingCaptions {
            rod_names: ["Old Rod", "Deluxe Rod", "Legendary Rod"],
            lures_left: "Lures",
            lure_count_suffix: "left",
        }
    }
}

/// Where the consumer samples its non-text quads from.
///
/// The fishing sprite page itself is not ported - `FUN_801d63b0` is a bare
/// VRAM quad emitter, and its glyph ids index a page the host uploads. So
/// glyph ids resolve through `glyph_src`, and a glyph the host cannot place
/// is dropped rather than guessed at.
pub struct FishingHudAtlas<'a> {
    /// Atlas rect of a fully opaque texel, stretched to fill gauge bars.
    /// `None` when the host has no such texel to sample - the bar frames
    /// still emit, the fill quads are skipped.
    pub solid_src: Option<(u32, u32, u32, u32)>,
    /// Fishing glyph id -> atlas source rect; `None` drops the glyph.
    pub glyph_src: &'a dyn Fn(u32) -> Option<(u32, u32, u32, u32)>,
    /// Pixel thickness of a gauge bar's fill quad across its axis.
    pub bar_thickness: u32,
}

/// Map a retail brightness byte to a vertex tint.
///
/// `0x80` is the PSX GPU's neutral (unmodulated) vertex brightness, so it maps
/// to `1.0`; `0xff` is the same after the clamp. Below `0x80` the ramp is
/// linear, which is what carries the strike splash's fade-in / fade-out.
pub fn hud_tint(brightness: i32) -> [f32; 4] {
    let v = (brightness.max(0) as f32 / 128.0).min(1.0);
    [v, v, v, 1.0]
}

/// Quads for one resolved [`BarFrame`], the bar step of
/// [`fishing_hud_draws_for`]: the three frame glyphs at the fixed
/// [`BAR_FRAME_BRIGHTNESS`], then the fill quad stretched over
/// [`BarFrame::fill_len`] along the frame's axis. The vertical bar fills
/// *upward* from its bottom cap, matching `FUN_801d1a90`.
///
/// The fill takes its colour straight from [`BarFrame::fill_rgb`] - that
/// triple is already the brightness ramp, so it is not tinted a second time.
/// A frame whose `fill_rgb` is `None` (retail's third style arm, which writes
/// no colour at all) emits its glyphs and no fill.
fn bar_frame_draws(
    frame: &BarFrame,
    atlas: &FishingHudAtlas<'_>,
    origin: (i32, i32),
) -> Vec<SpriteDraw> {
    let mut out = Vec::new();
    for (glyph, pos) in frame.glyphs.iter().zip(frame.positions.iter()) {
        if let Some(src) = (atlas.glyph_src)(*glyph) {
            out.push(SpriteDraw {
                dst: (origin.0 + pos.0, origin.1 + pos.1, src.2, src.3),
                src,
                color: hud_tint(BAR_FRAME_BRIGHTNESS),
            });
        }
    }
    if let (Some((r, g, b)), Some(solid)) = (frame.fill_rgb, atlas.solid_src)
        && frame.fill_len > 0
    {
        let color = [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0];
        let thick = atlas.bar_thickness;
        let len = frame.fill_len as u32;
        let dst = match frame.axis {
            // Left-to-right from just past the start cap.
            BarAxis::Horizontal => (
                origin.0 + frame.positions[1].0,
                origin.1 + frame.positions[1].1,
                len,
                thick,
            ),
            // Upward from the bottom cap.
            BarAxis::Vertical => (
                origin.0 + frame.positions[2].0,
                origin.1 + frame.positions[2].1 - frame.fill_len,
                thick,
                len,
            ),
        };
        out.push(SpriteDraw {
            dst,
            src: solid,
            color,
        });
    }
    out
}

/// Turn a fishing HUD draw list into renderable quads - the fishing sibling
/// of `battle_hud_draws_for`, and the host consumer of every builder above.
///
/// Item handling:
/// - [`HudDraw::Number`] goes through the ported digit field
///   ([`number_digit_cells`]), so leading zeros stay blank and the value is
///   right-aligned in its 8-slot row; each cell is drawn from the
///   proportional font atlas.
/// - [`HudDraw::Count`] is the fixed-width variant. Its retail primitive
///   (`0x80034b78`) is unported, so the width is honoured by zero-padding -
///   an engine-side choice, not a pinned one.
/// - [`HudDraw::Caption`] resolves through `captions`; a [`HudCaption::RodName`]
///   whose index is out of range draws nothing, matching the builder's own
///   gate.
/// - [`HudDraw::Glyph`] resolves through the atlas, and is dropped when the
///   host cannot place that id.
/// - [`HudDraw::Bar`] / [`HudDraw::PowerBar`] route through
///   [`HudDraw::resolve_bar`] into [`bar_frame_draws`].
///
/// All coordinates are the retail stage-pixel constants offset by `origin`;
/// pass the result through [`crate::scale_stage_text_draws`] to place it in a
/// scaled surface.
pub fn fishing_hud_draws_for(
    font: &legaia_font::Font,
    items: &[HudDraw],
    captions: &FishingCaptions<'_>,
    atlas: &FishingHudAtlas<'_>,
    origin: (i32, i32),
) -> Vec<TextDraw> {
    let mut out = Vec::new();
    let text_at = |out: &mut Vec<TextDraw>, s: &str, x: i32, y: i32, brightness: i32| {
        if s.is_empty() {
            return;
        }
        let layout = font.layout_ascii(s);
        out.extend(text_draws_for(
            &layout,
            (origin.0 + x, origin.1 + y),
            hud_tint(brightness),
        ));
    };

    for item in items {
        match *item {
            HudDraw::Number {
                x,
                y,
                value,
                brightness,
            } => {
                for cell in number_digit_cells(0, x, y, value) {
                    let digit = [b'0' + cell.digit as u8];
                    let s = core::str::from_utf8(&digit).unwrap_or("0");
                    text_at(&mut out, s, cell.x, cell.y, brightness);
                }
            }
            HudDraw::Count {
                value,
                digits,
                x,
                y,
            } => {
                let w = digits as usize;
                let s = format!("{:0w$}", value.max(0), w = w);
                text_at(&mut out, &s, x, y, HUD_BRIGHTNESS);
            }
            HudDraw::Caption { text, x, y } => {
                let s = match text {
                    HudCaption::RodName(i) => {
                        captions.rod_names.get(i as usize).copied().unwrap_or("")
                    }
                    HudCaption::LuresLeft => captions.lures_left,
                    HudCaption::LureCountSuffix => captions.lure_count_suffix,
                };
                text_at(&mut out, s, x, y, HUD_BRIGHTNESS);
            }
            HudDraw::Glyph {
                id,
                x,
                y,
                brightness,
                ..
            } => {
                if let Some(src) = (atlas.glyph_src)(id) {
                    out.push(SpriteDraw {
                        dst: (origin.0 + x, origin.1 + y, src.2, src.3),
                        src,
                        color: hud_tint(brightness),
                    });
                }
            }
            HudDraw::Bar { .. } | HudDraw::PowerBar { .. } => {
                if let Some(frame) = item.resolve_bar() {
                    out.extend(bar_frame_draws(&frame, atlas, origin));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_variants_share_the_slide_ramp_and_expire_together() {
        // All four banner animators ride `banner_slide`; the two mirrored ones
        // hold at the same 0xa0 centre as the from-left banner.
        let hold = 0x20;
        assert_eq!(banner_slide(hold), 0xa0);
        let miss = banner_miss_draw(hold).expect("active mid-hold");
        match miss {
            HudDraw::Glyph { id, x, .. } => {
                assert_eq!(id, 0x19);
                assert_eq!(x, 0x140 - 0xa0);
            }
            _ => panic!("miss banner is a glyph draw"),
        }
        let pair = banner_converge_draws(hold).expect("active mid-hold");
        match (pair[0], pair[1]) {
            (HudDraw::Glyph { x: a, id: ia, .. }, HudDraw::Glyph { x: b, id: ib, .. }) => {
                assert_eq!((ia, ib), (0xc, 0xc), "same glyph both sides");
                assert_eq!(a + b, 0x140, "mirrored about the screen centre");
            }
            _ => panic!("converge banner is a glyph pair"),
        }
        // Both expire exactly at the shared lifetime.
        assert!(banner_miss_draw(BANNER_FRAMES - 1).is_some());
        assert!(banner_miss_draw(BANNER_FRAMES).is_none());
        assert!(banner_converge_draws(BANNER_FRAMES).is_none());
    }

    #[test]
    fn digit_field_blanks_leading_zeros_and_right_aligns() {
        let cells = number_digit_cells(0, 100, 50, 42);
        let digits: Vec<i32> = cells.iter().map(|c| c.digit).collect();
        assert_eq!(digits, vec![4, 2], "only significant digits are emitted");
        // Right-aligned in the 8-slot field: '4' lands in slot 6, '2' in 7.
        assert_eq!(cells[0].x, 100 + 6 * DIGIT_PITCH_NARROW);
        assert_eq!(cells[1].x, 100 + 7 * DIGIT_PITCH_NARROW);
        assert!(cells.iter().all(|c| c.y == 50));
    }

    #[test]
    fn digit_field_draws_a_lone_zero() {
        // The seeded last slot is what keeps a zero total visible.
        let cells = number_digit_cells(0, 0, 0, 0);
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].digit, 0);
        assert_eq!(cells[0].x, 7 * DIGIT_PITCH_NARROW);
    }

    #[test]
    fn digit_field_style_selects_the_slot_pitch() {
        let wide = number_digit_cells(1, 0, 0, 7);
        assert_eq!(wide[0].x, 7 * DIGIT_PITCH_WIDE);
    }

    #[test]
    fn digit_field_fills_every_slot_at_eight_digits() {
        let cells = number_digit_cells(0, 0, 0, 12_345_678);
        let digits: Vec<i32> = cells.iter().map(|c| c.digit).collect();
        assert_eq!(digits, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn bar_frames_span_their_segments_and_track_the_fill() {
        let segs = 8;
        let h = bar_frame(20, 40, BAR_VALUE_ONE, segs, 0);
        assert_eq!(h.glyphs, [3, 4, 5]);
        // Caps bracket the body along x; y is constant.
        assert_eq!(h.positions[0], (20, 40));
        assert_eq!(h.positions[2], (20 + segs * 8 + 8, 40));
        assert_eq!(h.fill_len, segs * 8, "full value fills every segment");
        assert_eq!(h.fill_brightness, 0xff);

        let v = power_bar_frame(20, 40, BAR_VALUE_ONE / 2, segs);
        assert_eq!(v.glyphs, [0, 1, 2]);
        // The vertical bar brackets along y instead, at a constant x.
        assert_eq!(v.positions[0], (20, 40));
        assert_eq!(v.positions[2], (20, 40 + segs * 8 + 8));
        assert_eq!(v.fill_len, segs * 8 / 2, "half value fills half the bar");
        assert_eq!(v.fill_brightness, 0x7f);
        assert_eq!(v.body_scale, segs << 12);

        // An empty bar still draws its frame, with nothing lit.
        let empty = bar_frame(0, 0, 0, segs, 0);
        assert_eq!((empty.fill_len, empty.fill_brightness), (0, 0));
    }

    #[test]
    fn bar_style_selects_the_fill_ramp_without_moving_geometry() {
        let segs = 8;
        let value = BAR_VALUE_ONE / 2; // brightness byte 0x7f
        let s0 = bar_frame(20, 40, value, segs, 0);
        let s1 = bar_frame(20, 40, value, segs, 1);

        // Style 0 holds the constant red against the ramp...
        assert_eq!(s0.fill_rgb, Some((BAR_FILL_STYLE0_RED, 0x7f, 0)));
        // ...style 1 runs the ramp against its own complement.
        assert_eq!(s1.fill_rgb, Some((0x7f, 0x80, 0)));
        // Blue is zero in both, and the geometry is identical.
        assert_eq!(s0.positions, s1.positions);
        assert_eq!(
            (s0.fill_len, s0.body_scale, s0.glyphs),
            (s1.fill_len, s1.body_scale, s1.glyphs)
        );

        // The complement tracks the ramp across its range.
        let full = bar_frame(0, 0, BAR_VALUE_ONE, segs, 1);
        assert_eq!(full.fill_rgb, Some((0xff, 0x00, 0)));
        let dark = bar_frame(0, 0, 0, segs, 1);
        assert_eq!(dark.fill_rgb, Some((0x00, 0xff, 0)));

        // Any other style jumps the colour stores entirely.
        assert_eq!(bar_frame(0, 0, value, segs, 2).fill_rgb, None);

        // The power bar is permanently the style-0 ramp.
        assert_eq!(
            power_bar_frame(0, 0, value, segs).fill_rgb,
            Some((BAR_FILL_STYLE0_RED, 0x7f, 0))
        );
    }

    #[test]
    fn hud_bar_variants_resolve_through_the_ported_helpers() {
        // The retail HUD uses style 0 for depth and style 1 for tension,
        // so the draw list exercises both ramps.
        let depth = HudDraw::Bar {
            style: 0,
            x: 0x10,
            y: 0x90,
            value: BAR_VALUE_ONE,
            step: 10,
        };
        let tension = HudDraw::Bar {
            style: 1,
            x: 0x10,
            y: 0xb0,
            value: BAR_VALUE_ONE,
            step: 10,
        };
        assert_eq!(
            depth.resolve_bar().unwrap(),
            bar_frame(0x10, 0x90, BAR_VALUE_ONE, 10, 0)
        );
        assert_eq!(
            tension.resolve_bar().unwrap().fill_rgb,
            Some((0xff, 0x00, 0))
        );

        let power = HudDraw::PowerBar {
            x: 0x120,
            y: 0x40,
            power: 0,
            step: 0xc,
        };
        assert_eq!(power.resolve_bar().unwrap().glyphs, [0, 1, 2]);

        // Non-bar variants carry no frame.
        assert!(
            HudDraw::Caption {
                text: HudCaption::LuresLeft,
                x: 0,
                y: 0,
            }
            .resolve_bar()
            .is_none()
        );
    }

    #[test]
    fn persistent_hud_caps_points_and_gates_the_rod_label() {
        let d = persistent_hud_draws(2_000_000, 1234, 2, 7);
        // Point total renders capped; the best-catch row is uncapped input.
        assert!(d.contains(&HudDraw::Number {
            x: 0x32,
            y: 0x16,
            value: HUD_POINT_CAP,
            brightness: HUD_BRIGHTNESS
        }));
        assert!(d.contains(&HudDraw::Number {
            x: 0x32,
            y: 0x08,
            value: 1234,
            brightness: HUD_BRIGHTNESS
        }));
        // Rod index 2 draws its label; the count row shows the lure count.
        assert!(d.contains(&HudDraw::Caption {
            text: HudCaption::RodName(2),
            x: 0x98,
            y: 0x0c
        }));
        assert!(d.contains(&HudDraw::Count {
            value: 7,
            digits: 4,
            x: 0x100,
            y: 0x0c
        }));
        // An out-of-range rod index draws no label but keeps the count row.
        let d = persistent_hud_draws(0, 0, 3, 0);
        assert!(!d.iter().any(|x| matches!(
            x,
            HudDraw::Caption {
                text: HudCaption::RodName(_),
                ..
            }
        )));
        assert!(d.contains(&HudDraw::Caption {
            text: HudCaption::LuresLeft,
            x: 0xf3,
            y: 0x0c
        }));
    }

    #[test]
    fn catch_hud_length_and_percent_arithmetic() {
        // record 812 -> 512 past the strike base -> 512*100 >> 9 = 100 tenths;
        // extent 1024 -> (1024 >> 8) >> 1 = 2 tenths -> 102 = "10.2".
        assert_eq!(length_display(812, 1024), 102);
        assert_eq!(extent_display(1024), 2);
        // Below the strike base the record term clamps to zero.
        assert_eq!(length_display(0, 0), 0);
        assert_eq!(length_display(299, 0), 0);
        // Negative extent clamps to zero (with retail's toward-zero rounding).
        assert_eq!(extent_display(-1), 0);
        assert_eq!(extent_display(-1024), 0);
        // Cast power percent: percent of the 0x1000 meter ceiling.
        assert_eq!(cast_power_percent(0x1000), 100);
        assert_eq!(cast_power_percent(0x800), 50);
        assert_eq!(cast_power_percent(0x20), 0);
        // The HUD splits the length into whole + tenths digits.
        let s = CatchHudState {
            record: 812,
            line_extent: 1024,
            cast_power: 0x800,
            ..Default::default()
        };
        let d = catch_hud_draws(&s);
        assert!(d.contains(&HudDraw::Number {
            x: 0xda,
            y: 0x30,
            value: 10,
            brightness: HUD_BRIGHTNESS
        }));
        assert!(d.contains(&HudDraw::Number {
            x: 0xe8,
            y: 0x30,
            value: 2,
            brightness: HUD_BRIGHTNESS
        }));
        assert!(d.contains(&HudDraw::Number {
            x: 0xe4,
            y: 0xb0,
            value: 50,
            brightness: HUD_BRIGHTNESS
        }));
        assert!(d.contains(&HudDraw::PowerBar {
            x: 0x120,
            y: 0x40,
            power: 0x800,
            step: 0xc
        }));
    }

    #[test]
    fn catch_hud_gauge_block_is_gated_on_hook() {
        let mut s = CatchHudState {
            depth: 0x300,
            tension: 0x700,
            ..Default::default()
        };
        // Not hooked: no gauge bars.
        assert!(
            !catch_hud_draws(&s)
                .iter()
                .any(|d| matches!(d, HudDraw::Bar { .. }))
        );
        // Hooked: the depth (style 0) + tension (style 1) bars appear.
        s.gauges_visible = true;
        let d = catch_hud_draws(&s);
        assert!(d.contains(&HudDraw::Bar {
            style: 0,
            x: 0x10,
            y: 0x90,
            value: 0x300,
            step: 10
        }));
        assert!(d.contains(&HudDraw::Bar {
            style: 1,
            x: 0x10,
            y: 0xb0,
            value: 0x700,
            step: 10
        }));
    }

    fn glyph_x(d: HudDraw) -> i32 {
        match d {
            HudDraw::Glyph { x, .. } => x,
            other => panic!("expected a glyph, got {other:?}"),
        }
    }

    #[test]
    fn banner_slide_ramp_and_lifetime() {
        // From the left: slide in at 8 px/frame, hold at 0xa0, slide off.
        assert_eq!(glyph_x(banner_from_left_draw(1).unwrap()), 8);
        assert_eq!(glyph_x(banner_from_left_draw(0x13).unwrap()), 0x98);
        assert_eq!(glyph_x(banner_from_left_draw(0x14).unwrap()), 0xa0);
        assert_eq!(glyph_x(banner_from_left_draw(0x8b).unwrap()), 0xa0);
        // The slide-off ramp joins continuously at the hold value.
        assert_eq!(glyph_x(banner_from_left_draw(0x8c).unwrap()), 0xa0);
        assert_eq!(glyph_x(banner_from_left_draw(0xc7).unwrap()), 0x278);
        assert!(banner_from_left_draw(0xc8).is_none());
        // From the right: the mirrored trajectory, holding at the same x.
        assert_eq!(glyph_x(banner_from_right_draw(1).unwrap()), 0x140 - 8);
        assert_eq!(glyph_x(banner_from_right_draw(0x20).unwrap()), 0xa0);
        assert!(banner_from_right_draw(0xc8).is_none());
    }

    #[test]
    fn strike_splash_rises_and_fades() {
        // Fade-in at 8/frame, hold at 0x80, fade-out from frame 0x88.
        assert_eq!(splash_brightness(1), 8);
        assert_eq!(splash_brightness(0x10), 0x80);
        assert_eq!(splash_brightness(0x87), 0x80);
        assert_eq!(splash_brightness(0x90), 0x40);
        // The pair rises one pixel every 32 frames from y = 0x50.
        let [a, b] = strike_splash_draws(0x40).unwrap();
        match (a, b) {
            (
                HudDraw::Glyph {
                    id: 0x416,
                    x: 0xa0,
                    y,
                    brightness,
                    ..
                },
                HudDraw::Glyph {
                    id: 0x816,
                    y: y2,
                    brightness: b2,
                    ..
                },
            ) => {
                assert_eq!(y, 0x50 - 2);
                assert_eq!((y, brightness), (y2, b2));
                assert_eq!(brightness, 0x80);
            }
            other => panic!("unexpected splash pair {other:?}"),
        }
        // Expires exactly at the lifetime end (brightness reaches 0 there).
        assert!(strike_splash_draws(SPLASH_FRAMES - 1).is_some());
        assert!(strike_splash_draws(SPLASH_FRAMES).is_none());
    }

    #[test]
    fn banner_timer_advances_while_active_and_resets() {
        let mut t = BannerTimer::default();
        assert!(!t.is_active());
        assert!(t.service(2, banner_from_left_draw).is_none());
        t.start();
        assert!(t.is_active());
        // Each serviced frame draws and advances by the frame step.
        assert!(t.service(2, banner_from_left_draw).is_some());
        assert_eq!(t.0, 3);
        // Run it out: the animator expires and the timer resets to idle.
        let mut frames = 0;
        while t.service(2, banner_from_left_draw).is_some() {
            frames += 1;
            assert!(frames < 1000, "timer failed to expire");
        }
        assert!(!t.is_active());
        // Cancel mirrors the tail's cross-banner zeroing.
        t.start();
        t.cancel();
        assert!(!t.is_active());
    }

    // --- consumer -----------------------------------------------------------

    /// A 16x16 cell per glyph id, so every id in the fishing HUD resolves.
    fn test_atlas_glyph(id: u32) -> Option<(u32, u32, u32, u32)> {
        Some(((id & 0xf) * 16, (id >> 4) * 16, 16, 16))
    }

    fn test_atlas() -> FishingHudAtlas<'static> {
        FishingHudAtlas {
            solid_src: Some((0, 0, 1, 1)),
            glyph_src: &test_atlas_glyph,
            bar_thickness: 8,
        }
    }

    #[test]
    fn consumer_renders_every_persistent_hud_item() {
        let font = legaia_font::synthetic_for_tests();
        let caps = FishingCaptions::placeholder();
        let atlas = test_atlas();
        let items = persistent_hud_draws(1234, 5678, 2, 7);
        let draws = fishing_hud_draws_for(&font, &items, &caps, &atlas, (0, 0));
        assert!(!draws.is_empty(), "the persistent HUD renders quads");

        // Every item kind in the list must contribute at least one quad -
        // this is what fails if a match arm is dropped or stubbed out.
        for item in &items {
            let one = fishing_hud_draws_for(&font, &[*item], &caps, &atlas, (0, 0));
            assert!(!one.is_empty(), "{item:?} produced no draws");
        }
    }

    #[test]
    fn consumer_renders_every_catch_hud_item_including_the_gauges() {
        let font = legaia_font::synthetic_for_tests();
        let caps = FishingCaptions::placeholder();
        let atlas = test_atlas();
        let items = catch_hud_draws(&CatchHudState {
            record: 812,
            line_extent: 1024,
            cast_power: 0x800,
            depth: 0x400,
            tension: 0x600,
            gauges_visible: true,
        });
        for item in &items {
            let one = fishing_hud_draws_for(&font, &[*item], &caps, &atlas, (0, 0));
            assert!(!one.is_empty(), "{item:?} produced no draws");
        }
        // The hooked gauge block puts both bars in the list, so the render
        // carries their fill quads on top of the cap/body/cap frames.
        let fills = fishing_hud_draws_for(&font, &items, &caps, &atlas, (0, 0))
            .into_iter()
            .filter(|d| Some(d.src) == atlas.solid_src)
            .count();
        assert_eq!(fills, 3, "power bar + depth bar + tension bar fills");
    }

    #[test]
    fn consumer_places_bar_fills_on_the_right_axis() {
        let atlas = test_atlas();
        // Horizontal: the fill starts just past the start cap and runs right.
        let h = bar_frame(0x10, 0xb0, BAR_VALUE_ONE / 2, 8, 1);
        let hd = bar_frame_draws(&h, &atlas, (0, 0));
        let hfill = hd.iter().find(|d| Some(d.src) == atlas.solid_src).unwrap();
        assert_eq!(hfill.dst.0, 0x10 + 8, "starts after the start cap");
        assert_eq!(hfill.dst.2, h.fill_len as u32, "length runs along x");
        assert_eq!(hfill.dst.3, atlas.bar_thickness);

        // Vertical: the fill grows upward from the bottom cap.
        let v = power_bar_frame(0x120, 0x40, BAR_VALUE_ONE / 2, 0xc);
        let vd = bar_frame_draws(&v, &atlas, (0, 0));
        let vfill = vd.iter().find(|d| Some(d.src) == atlas.solid_src).unwrap();
        let bottom = v.positions[2].1;
        assert_eq!(vfill.dst.1, bottom - v.fill_len, "top edge is bottom - len");
        assert_eq!(vfill.dst.3, v.fill_len as u32, "length runs along y");
        assert_eq!(vfill.dst.2, atlas.bar_thickness);
    }

    #[test]
    fn consumer_offsets_by_the_origin_and_ramps_the_tint() {
        let font = legaia_font::synthetic_for_tests();
        let caps = FishingCaptions::placeholder();
        let atlas = test_atlas();
        let glyph = HudDraw::Glyph {
            layer: 0,
            id: 7,
            x: 0x20,
            y: 0x78,
            brightness: HUD_BRIGHTNESS,
        };
        let at_origin = fishing_hud_draws_for(&font, &[glyph], &caps, &atlas, (0, 0));
        let moved = fishing_hud_draws_for(&font, &[glyph], &caps, &atlas, (10, 20));
        assert_eq!(moved[0].dst.0 - at_origin[0].dst.0, 10);
        assert_eq!(moved[0].dst.1 - at_origin[0].dst.1, 20);
        // 0x80 is the neutral vertex brightness; the splash's fade-in is
        // linear below it.
        assert_eq!(hud_tint(HUD_BRIGHTNESS)[0], 1.0);
        assert_eq!(hud_tint(HUD_BRIGHTNESS_FULL)[0], 1.0);
        assert_eq!(hud_tint(0x40)[0], 0.5);
        assert_eq!(hud_tint(0)[0], 0.0);
    }

    #[test]
    fn consumer_drops_glyphs_the_host_cannot_place() {
        let font = legaia_font::synthetic_for_tests();
        let caps = FishingCaptions::placeholder();
        let blind = FishingHudAtlas {
            solid_src: Some((0, 0, 1, 1)),
            glyph_src: &|_| None,
            bar_thickness: 8,
        };
        let items = persistent_hud_draws(1234, 5678, 2, 7);
        let draws = fishing_hud_draws_for(&font, &items, &caps, &blind, (0, 0));
        // Glyph rows vanish, but the numbers and captions still render.
        assert!(!draws.is_empty());
        let glyphs_only = [HudDraw::Glyph {
            layer: 1,
            id: 0x1a,
            x: 0,
            y: 0,
            brightness: HUD_BRIGHTNESS,
        }];
        assert!(fishing_hud_draws_for(&font, &glyphs_only, &caps, &blind, (0, 0)).is_empty());
    }

    #[test]
    fn consumer_right_aligns_numbers_and_zero_pads_counts() {
        let font = legaia_font::synthetic_for_tests();
        let caps = FishingCaptions::placeholder();
        let atlas = test_atlas();
        // A Number goes through the ported digit field: 42 occupies the last
        // two of the eight slots, so its leftmost quad sits six pitches in.
        let num = fishing_hud_draws_for(
            &font,
            &[HudDraw::Number {
                x: 0,
                y: 0,
                value: 42,
                brightness: HUD_BRIGHTNESS,
            }],
            &caps,
            &atlas,
            (0, 0),
        );
        assert_eq!(num.len(), 2, "two significant digits, no leading zeros");
        assert_eq!(num[0].dst.0, 6 * DIGIT_PITCH_NARROW);

        // A Count is the fixed-width field: 7 in four digits is "0007".
        let cnt = fishing_hud_draws_for(
            &font,
            &[HudDraw::Count {
                value: 7,
                digits: 4,
                x: 0,
                y: 0,
            }],
            &caps,
            &atlas,
            (0, 0),
        );
        assert_eq!(cnt.len(), 4, "zero-padded to the field width");
    }
}

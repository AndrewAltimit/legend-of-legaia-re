//! The persistent HUD and the catch HUD draw lists, and the readout arithmetic they share.

use super::*;

/// The persistent fishing HUD (drawn every frame by the mode driver's shared
/// tail): the best-catch row (glyph `0x1a`), the capped point-total row
/// (glyph `0x1c`), the selected rod/lure label, and the lures-remaining
/// count. Retail reads `_DAT_80084458` / `_DAT_8008444c` / `_DAT_80084450`
/// and the live inventory count of the paired lure item
/// (`legaia_engine_fishing::fishing::lure_item_id`); here they are caller
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

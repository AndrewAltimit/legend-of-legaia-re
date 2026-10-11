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
//! `legaia_engine_fishing::fishing`, which owns the numeric kernels this module
//! only displays.

mod banners;
mod bars;
mod digit_cells;
mod host_consumer;
mod hud_draws;

pub use banners::*;
pub use bars::*;
pub use digit_cells::*;
pub use host_consumer::*;
pub use hud_draws::*;

/// The line-record base offset the catch-HUD length readout subtracts
/// (`FUN_801d1580`: `record - 300`, clamped at zero). Same retail literal as
/// the hook check's `record < gate + 300`, whose copy lives with the rules
/// kernels (`legaia_engine_fishing::fishing::RECORD_STRIKE_BASE`).
pub const RECORD_STRIKE_BASE: i32 = 300;

/// The point total the persistent HUD clamps its row to (`FUN_801d13f0`).
/// Same retail literal as the persistent counter's own clamp in
/// `FUN_801d5298`, whose copy lives with the rules kernels
/// (`legaia_engine_fishing::fishing::FISH_POINTS_CAP`).
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
// FUN_801d63b0 itself is `crate::fishing::sprite`: the variant carries
// its call-site arguments and the sprite pass turns it into a quad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HudDraw {
    /// A number via the digit blitter `FUN_801d76e0`.
    Number {
        x: i32,
        y: i32,
        value: i32,
        brightness: i32,
    },
    /// A number in the **large** digit style: `FUN_801d76e0` with style `1`,
    /// whose cells sit 16 px apart and draw through the large-digit emitter
    /// `FUN_801d7d44` (two passes, additive then subtractive). Retail's one
    /// such field is the landed catch's points ([`catch_result_draws`]).
    // REF: FUN_801d7d44
    LargeNumber {
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
    /// The landed species' name (the species record's `+0x00` string),
    /// indexed by species id ([`catch_result_draws`]).
    FishName(u32),
}

#[cfg(test)]
mod tests;

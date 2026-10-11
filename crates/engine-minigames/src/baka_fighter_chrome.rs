//! Baka Fighter **round chrome** - the intro title card, the round banner and
//! the READY/FIGHT countdown that frame the duel proper.
//!
//! All three are frame-counter timelines over the same primitive: the HUD
//! textured-quad emitter `FUN_801D5ED0`, ported as
//! [`crate::baka_fighter::hud_widget_quad`] against the 51-record widget
//! descriptor table (`legaia_minigame_formats::baka_opponents::parse_baka_hud`). This
//! module keeps the *timelines* - which widget is drawn where, how bright and
//! how big on frame `t`, plus the CD-XA announcer lines and the screen fades
//! they fire - separate from the quad geometry, so a host can run them with no
//! renderer and no overlay image resident.
//!
//! Sourced from the Baka Fighter overlay (PROT 0976, link base `0x801CE818`);
//! see `docs/subsystems/minigame-baka-fighter.md`.
//!
//! ### The glyph strip
//!
//! Widget `5` of the descriptor table is a **digit/letter strip**: the drawing
//! wrappers select a cell by overwriting the widget's own `u` field (record 5
//! `+0x08`, runtime VA `0x801D71CC`) with `index * 24` before the emit. Two
//! call sites do it - `FUN_801D69A8` (this module) and the actor draw callback
//! `FUN_801D67F0` mode 2 - and both use the same 24-pixel cell pitch.

mod afterimage;
mod anim_slots;
mod announcer;
mod cameo;
mod countdown;
mod glyphs;
mod hud_strips;
mod impact;
mod intro_title;
mod keyframe_editor;
mod round_banner;
mod runner;
mod sprite_actor;

pub use afterimage::*;
pub use anim_slots::*;
pub use announcer::*;
pub use cameo::*;
pub use countdown::*;
pub use glyphs::*;
pub use hud_strips::*;
pub use impact::*;
pub use intro_title::*;
pub use keyframe_editor::*;
pub use round_banner::*;
pub use runner::*;
pub use sprite_actor::*;

/// One resolved chrome draw: a widget id plus the four emitter arguments.
///
/// Retail passes these straight to `FUN_801D5ED0(x, y, widget, brightness,
/// size)`; feed them to [`crate::baka_fighter::hud_widget_quad`] once the
/// descriptor table is parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromeDraw {
    /// Widget id into the HUD descriptor table.
    pub widget: u8,
    /// Screen centre of the quad.
    pub x: i16,
    pub y: i16,
    /// Colour scale, `0x80` = the descriptor's own RGB, `0xFF` = doubled.
    pub brightness: i32,
    /// Size scale, 20.12 fixed point (`0x1000` = pixel-exact cell size).
    pub size: i32,
    /// When `Some(n)`, the emitter first pages the [`GLYPH_WIDGET`] strip to
    /// cell `n` by writing `u = n * GLYPH_CELL_WIDTH` (`DAT_801D71CC`).
    pub glyph: Option<i32>,
}

impl ChromeDraw {
    const fn plain(widget: u8, x: i16, y: i16, brightness: i32, size: i32) -> Self {
        ChromeDraw {
            widget,
            x,
            y,
            brightness,
            size,
            glyph: None,
        }
    }
}

/// Full-screen tint push (`FUN_80024EE4(1, 1, rgb)`), one 8-bit grey level
/// replicated across the three channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenTint {
    pub grey: u8,
}

/// What one chrome frame produces.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChromeFrame {
    /// Quads to emit, in retail submit order.
    pub draws: Vec<ChromeDraw>,
    /// Announcer line started this frame, if any.
    pub xa: Option<XaCue>,
    /// Full-screen tint pushed this frame, if any.
    pub tint: Option<ScreenTint>,
    /// When `Some`, the frame rewrites widget `0x22`'s sibling CLUT field
    /// (record 34 `+0x06`, runtime VA `0x801D740E`) before drawing.
    pub banner_clut: Option<u16>,
}

#[cfg(test)]
mod tests;

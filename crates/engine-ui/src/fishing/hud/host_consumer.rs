//! The host consumer: captions, the HUD atlas and the projection of HUD draws into sprite / text batches.

use super::*;
use crate::{SpriteDraw, TextDraw, text_draws_for};

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
    /// The species names a [`HudCaption::FishName`] indexes; empty draws no
    /// name.
    pub fish_names: &'a [String],
}

impl<'a> FishingCaptions<'a> {
    /// The captions off the user's disc, as the engine resolved them
    /// (`World::minigames.fishing_captions`): the three lure labels, the
    /// caption before the count and the one after it.
    pub fn from_disc(lure_names: &'a [String; 3], lures_left: &'a str, suffix: &'a str) -> Self {
        FishingCaptions {
            rod_names: [&lure_names[0], &lure_names[1], &lure_names[2]],
            lures_left,
            lure_count_suffix: suffix,
            fish_names: &[],
        }
    }

    /// The same captions with the species names a landed catch's plate
    /// draws ([`HudCaption::FishName`]).
    pub fn with_fish_names(mut self, fish_names: &'a [String]) -> Self {
        self.fish_names = fish_names;
        self
    }
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
            fish_names: &[],
        }
    }
}

/// Where the consumer samples its non-text quads from.
///
/// A host that draws the sprite half itself (`crate::fishing::sprite`,
/// `FUN_801d63b0`'s quads over the venue's HUD page) sets
/// [`Self::sprites_drawn`]; otherwise glyph ids resolve through `glyph_src`,
/// and a glyph the host cannot place is dropped rather than guessed at.
pub struct FishingHudAtlas<'a> {
    /// Atlas rect of a fully opaque texel, stretched to fill gauge bars.
    /// `None` when the host has no such texel to sample - the bar frames
    /// still emit, the fill quads are skipped.
    pub solid_src: Option<(u32, u32, u32, u32)>,
    /// Fishing glyph id -> atlas source rect; `None` drops the glyph.
    pub glyph_src: &'a dyn Fn(u32) -> Option<(u32, u32, u32, u32)>,
    /// Pixel thickness of a gauge bar's fill quad across its axis.
    pub bar_thickness: u32,
    /// The host draws the sprite half of this list itself, as screen
    /// primitives over the venue's HUD page
    /// ([`crate::fishing::sprite::fishing_hud_sprite_prims`]): numbers,
    /// glyphs, bar frames and bar fills are then left out here, and only the
    /// captions and counts come back as quads.
    pub sprites_drawn: bool,
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
pub(super) fn bar_frame_draws(
    frame: &BarFrame,
    atlas: &FishingHudAtlas<'_>,
    origin: (i32, i32),
) -> Vec<SpriteDraw> {
    let mut out = Vec::new();
    for (glyph, pos) in frame.glyphs.iter().zip(frame.positions.iter()) {
        if atlas.sprites_drawn {
            break;
        }
        if let Some(src) = (atlas.glyph_src)(*glyph) {
            out.push(SpriteDraw {
                dst: (origin.0 + pos.0, origin.1 + pos.1, src.2, src.3),
                src,
                color: hud_tint(BAR_FRAME_BRIGHTNESS),
            });
        }
    }
    // With the sprite pass up the fill is its prim (drawn over the frame, as
    // retail's ordering does); only a text-only host fills here.
    if let (Some((r, g, b)), Some(solid)) = (frame.fill_rgb, atlas.solid_src)
        && frame.fill_len > 0
        && !atlas.sprites_drawn
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
/// - [`HudDraw::Count`] is the fixed-width variant, laid out the way the
///   shared number primitive `FUN_80034B78` does: right-aligned in
///   `digits` 8-px cells, leading cells blank.
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
            HudDraw::Number { .. } | HudDraw::LargeNumber { .. } | HudDraw::Glyph { .. }
                if atlas.sprites_drawn => {}
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
            HudDraw::LargeNumber {
                x,
                y,
                value,
                brightness,
            } => {
                for cell in number_digit_cells(1, x, y, value) {
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
                // `FUN_80034B78(value, digits, x, y)`: the value right-aligned
                // in `digits` 8-px cells, leading cells blank - a zero-padded
                // string here ran the count into the caption before it.
                out.extend(crate::menu::screens::num_field_draws(
                    font,
                    value.max(0) as u64,
                    origin.0 + x,
                    origin.1 + y,
                    digits as i32,
                    hud_tint(HUD_BRIGHTNESS),
                ));
            }
            HudDraw::Caption { text, x, y } => {
                let s = match text {
                    HudCaption::RodName(i) => {
                        captions.rod_names.get(i as usize).copied().unwrap_or("")
                    }
                    HudCaption::LuresLeft => captions.lures_left,
                    HudCaption::LureCountSuffix => captions.lure_count_suffix,
                    HudCaption::FishName(i) => captions
                        .fish_names
                        .get(i as usize)
                        .map(String::as_str)
                        .unwrap_or(""),
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

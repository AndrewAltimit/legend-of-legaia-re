//! The **minigame effect-part** draw builder: one materialisation of a pool
//! part, shared by every host.
//!
//! The pool itself is simulation and lives in
//! `legaia_engine_core::minigame_fx`; this is only its projection onto a
//! surface. Both halves used to sit inside the play window, which is why the
//! fishing splash, the wander ripples and the catch bursts existed on exactly
//! one of the port's three surfaces.
//!
//! # The placeholder is the port's, and says so
//!
//! Retail draws each part as a cell out of the spawning overlay's own sprite
//! page. No minigame host uploads those pages into engine VRAM, so a part
//! degrades here to a letterform keyed on its `+0x50` sprite id - the same
//! degradation the dance run's own parts take. The *seats* are the ported
//! spawn kernels' and the *fade* is the pool's ramp; only the glyph is
//! invented, and [`fx_part_glyph`] is the one place it is invented.
//!
//! `legaia-engine-core` is a sibling crate rather than a dependency, so the
//! input arrives as a plain [`FxPartView`] the way every other builder here
//! takes its view.

use crate::{TextDraw, scale_stage_text_draws, text_draws_for};

/// One live effect part, as a host hands it over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FxPartView {
    /// Stage pixel (retail 320x240 framebuffer) position.
    pub x: i32,
    pub y: i32,
    /// The part's `+0x50` sprite id.
    pub sprite: u16,
    /// `0..=0xFF` fade weight from the pool's ramp.
    pub fade: u8,
}

/// Placeholder letterform for a part's sprite id.
///
/// The three id bands the port spawns with are its own
/// (`legaia_engine_core::minigame_fx`'s `*_SPRITE_ID` constants); the two
/// dance ids are retail's - `0xb` is the sequence-clear banner's widget and
/// `0x16` a flanking star, which is why they read as words rather than marks.
pub fn fx_part_glyph(sprite: u16) -> &'static str {
    match sprite {
        0xb => "GOOD!",
        0x16 => "*",
        // The port's own producer ids: splash, ripple, burst.
        0x100 => "~",
        0x101 => "o",
        0x102 => "*",
        _ => "+",
    }
}

/// Tint a part draws at, before its fade is applied. The warm off-white the
/// play window used for every part since the pool existed.
pub const FX_PART_RGB: [f32; 3] = [1.0, 1.0, 0.8];

/// This frame's placeholder draws for a part list, in surface pixels.
///
/// `stage_origin` / `stage_scale` are the host's 320x240 stage transform, the
/// same pair every other stage-space builder here takes.
pub fn fx_part_draws(
    font: &legaia_font::Font,
    parts: &[FxPartView],
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<TextDraw> {
    let mut out: Vec<TextDraw> = Vec::new();
    for p in parts {
        let alpha = f32::from(p.fade) / 255.0;
        let color = [
            FX_PART_RGB[0],
            FX_PART_RGB[1],
            FX_PART_RGB[2],
            alpha.clamp(0.0, 1.0),
        ];
        let layout = font.layout_ascii(fx_part_glyph(p.sprite));
        out.extend(text_draws_for(&layout, (p.x, p.y), color));
    }
    scale_stage_text_draws(&mut out, stage_origin, stage_scale);
    out
}

/// One emitted **dance sprite part**, as a host hands it over: the arm
/// `legaia_engine_core::dance::sprite_part_emit` (`FUN_801d387c`) resolved,
/// already through its own `>> 3`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DanceSpritePartView {
    /// Stage pixel position the emit resolved.
    pub x: i32,
    pub y: i32,
    /// The part's `+0x50` sprite id.
    pub sprite: u16,
    /// `0..=0xFF` fade weight.
    pub fade: u8,
    /// The two-emit (shadowed) arm: draw the cell twice, the second copy
    /// offset a pixel and at half weight, which is what makes the pair read
    /// as a sprite over its own shadow.
    pub shadow: bool,
}

/// Dimming applied to a shadowed part's second (`0x800`) emit.
pub const DANCE_SHADOW_WEIGHT: f32 = 0.5;

/// This frame's placeholder draws for the dance run's own sprite parts.
///
/// Separate from [`fx_part_draws`] only in the shadowed arm: the dance
/// overlay's emit dispatch has one, and the generic pool's parts do not go
/// through that dispatch (see `legaia_engine_core::minigame_fx`).
pub fn dance_sprite_part_draws(
    font: &legaia_font::Font,
    parts: &[DanceSpritePartView],
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<TextDraw> {
    let mut out: Vec<TextDraw> = Vec::new();
    for p in parts {
        let alpha = f32::from(p.fade) / 255.0;
        let glyph = fx_part_glyph(p.sprite);
        let mut emit_at = |x: i32, y: i32, dim: f32| {
            let layout = font.layout_ascii(glyph);
            out.extend(text_draws_for(
                &layout,
                (x, y),
                [
                    FX_PART_RGB[0],
                    FX_PART_RGB[1],
                    FX_PART_RGB[2],
                    (alpha * dim).clamp(0.0, 1.0),
                ],
            ));
        };
        emit_at(p.x, p.y, 1.0);
        if p.shadow {
            emit_at(p.x + 1, p.y + 1, DANCE_SHADOW_WEIGHT);
        }
    }
    scale_stage_text_draws(&mut out, stage_origin, stage_scale);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_glyph_table_separates_the_producers() {
        assert_eq!(fx_part_glyph(0xb), "GOOD!");
        assert_eq!(fx_part_glyph(0x16), "*");
        assert_eq!(fx_part_glyph(0x100), "~");
        assert_eq!(fx_part_glyph(0x101), "o");
        assert_ne!(fx_part_glyph(0x100), fx_part_glyph(0x101));
    }

    #[test]
    fn a_shadowed_dance_part_emits_twice() {
        let font = legaia_font::Font::placeholder();
        let one = dance_sprite_part_draws(
            &font,
            &[DanceSpritePartView {
                x: 20,
                y: 18,
                sprite: 0x16,
                fade: 0xFF,
                shadow: false,
            }],
            (0, 0),
            1,
        );
        let two = dance_sprite_part_draws(
            &font,
            &[DanceSpritePartView {
                x: 20,
                y: 18,
                sprite: 0x16,
                fade: 0xFF,
                shadow: true,
            }],
            (0, 0),
            1,
        );
        assert_eq!(two.len(), one.len() * 2);
        assert!(two[two.len() - 1].color[3] < one[0].color[3]);
    }

    #[test]
    fn a_fully_faded_part_draws_transparent() {
        let font = legaia_font::Font::placeholder();
        let d = fx_part_draws(
            &font,
            &[FxPartView {
                x: 0,
                y: 0,
                sprite: 0x100,
                fade: 0,
            }],
            (0, 0),
            1,
        );
        assert!(!d.is_empty());
        assert_eq!(d[0].color[3], 0.0);
    }
}

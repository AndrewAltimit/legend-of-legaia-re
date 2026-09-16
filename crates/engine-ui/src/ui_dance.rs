//! The dance minigame's two **text** layers: the pre-song count-in banner and
//! the Disco King how-to tutorial's captions.
//!
//! Both are placeholder letterforms on retail's own seats. The count-in's
//! banner is **not text at all** in retail: it is record `0` of the 20-byte
//! sprite table at `0x801D46CC`, a `320 x 64` quad (half-extents `0xa0` /
//! `0x20` at the record's unit scale) that `FUN_801D2F38` seats by its centre.
//! The `a2 = 0` all three call sites pass is that record index. The tutorial's
//! caption strings are overlay rodata (Sony text the port does not read).
//!
//! So what is pinned here is the *geometry*: retail's own banner centre
//! ([`COUNTIN_CENTRE_X`], [`COUNTIN_SLIDE_CENTRE_Y`] /
//! [`COUNTIN_HOLD_CENTRE_Y`] - the `a1` immediates at the three calls), the
//! record's extents and texel seat ([`COUNTIN_SPRITE_HALF`],
//! [`COUNTIN_SPRITE_UV`]), the sliding x offsets and the brightness ramp of
//! `dance_countin_banner_envelope` (`FUN_801d2d98`), and the caption / option
//! / cursor seats of the tutorial actor (`FUN_801D0750`).
//!
//! **Disclosed, both hosts**: the banner draws as placeholder text rather than
//! that sprite. What is missing is not a draw call but the art - no host
//! stages the dance overlay's own VRAM page, so there is nothing for a
//! textured quad to sample. Retail also varies the record's `+0x0F` blend
//! byte per arm (`sb $v0, 0xf($t0)` = `1` for the sliding halves at
//! `0x801D2ED8`, `sb $zero, 0xf($t0)` = `0` for the hold at `0x801D2F0C`), so
//! the halves are semi-transparent and the merged banner is opaque; the
//! placeholder carries that as the halved brightness the envelope already
//! reports, not as a blend mode.
//!
//! # Why the views are mirrors rather than imports
//!
//! `legaia-engine-core` is a sibling of this crate, not a dependency, so the
//! two inputs arrive as plain [`DanceCountInView`] / [`DanceTutorialView`]
//! structs - the same seam `ui_overlay`'s `ValueCellView` uses for the battle
//! numeral kernel. The *simulation* is one kernel on one host-independent
//! clock (`World::tick_dance`); this is only its projection onto a surface,
//! and both hosts call it, so the banner cannot slide at different rates in
//! the window and on the play page.

use crate::{TextDraw, scale_stage_text_draws, text_draws_for};

/// Stage column the count-in banner is centred on (retail's `0xa0`, the
/// 320-wide stage's midpoint) - `addiu $s3, $zero, 0xa0` at `0x801D2EB0`,
/// then `s2 + s3` for the right half and `s3 - s2` for the left.
pub const COUNTIN_CENTRE_X: i32 = 0xA0;

/// Stage row the two **sliding** halves are centred on: `a1 = 0x77`
/// (`0x801D2EB8` and `0x801D2EE8`, the two `FUN_801D2F38` calls the slide arm
/// makes).
pub const COUNTIN_SLIDE_CENTRE_Y: i32 = 0x77;

/// Stage row the single **held** banner is centred on: `a1 = 0x78`
/// (`0x801D2F00`, the hold arm's one call). Retail moves the banner down a
/// pixel when the halves merge; the split is real, not a rounding artifact.
pub const COUNTIN_HOLD_CENTRE_Y: i32 = 0x78;

/// This frame's banner centre row.
pub fn countin_centre_y(hold: bool) -> i32 {
    match hold {
        true => COUNTIN_HOLD_CENTRE_Y,
        false => COUNTIN_SLIDE_CENTRE_Y,
    }
}

/// The banner's half-extents in stage pixels at unit scale, from **record 0**
/// of the dance overlay's 20-byte sprite table at `0x801D46CC` - bytes
/// `+0x0A` / `+0x0B` = `0xa0` / `0x20`.
///
/// `FUN_801D2F38` builds the quad as `centre -+ half` on each axis
/// (`0x801D31E0..0x801D3214`: `t4 - a0` / `t4 + a0`, `t5 - v0` / `t5 + v0`),
/// so the record covers `320 x 64` around its seat. The `a2 = 0` every call
/// site passes is this **record index**, not a coordinate.
pub const COUNTIN_SPRITE_HALF: (i32, i32) = (0xA0, 0x20);

/// Texel origin of that record's art (`+0x08` = `0x9048`) and its CBA
/// (`+0x06` = `0x7d0a`). Recorded so a host that stages the dance overlay's
/// VRAM page can draw the real banner instead of the placeholder below.
pub const COUNTIN_SPRITE_UV: (u8, u8) = (0x48, 0x90);
/// See [`COUNTIN_SPRITE_UV`].
pub const COUNTIN_SPRITE_CBA: u16 = 0x7D0A;

/// Width the left half's placeholder text is nudged left by so its run ends
/// at the centre.
const COUNTIN_LEFT_INSET: i32 = 40;
/// Rough placeholder glyph height, so the stand-in text's own centre can be
/// put where retail centres the sprite.
const COUNTIN_TEXT_HALF_H: i32 = 6;

/// One frame of the count-in banner, as `World::minigames.dance_countin_banner`
/// carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DanceCountInView {
    /// Horizontal offset of the two sliding halves from [`COUNTIN_CENTRE_X`].
    pub x_offset: i32,
    /// Brightness `0..=0xff`, already halved for the sliding halves.
    pub brightness: i32,
    /// `true` = the single centred banner; `false` = the two halves.
    pub hold: bool,
}

/// Draw the count-in banner for one frame, in surface pixels.
///
/// `origin` / `scale` are the caller's 320x240 stage transform, the same one
/// the rest of its minigame chrome uses.
pub fn dance_countin_draws_for(
    font: &legaia_font::Font,
    view: DanceCountInView,
    origin: (i32, i32),
    scale: u32,
) -> Vec<TextDraw> {
    let alpha = (view.brightness.clamp(0, 0xFF) as f32) / 255.0;
    let color = [1.0f32, 1.0, 1.0, alpha];
    let mut out: Vec<TextDraw> = Vec::new();
    // Retail seats the sprite by its CENTRE; the placeholder is text, whose
    // seat is a top-left, so it is lifted by its own half-height to land on
    // the same row.
    let top = countin_centre_y(view.hold) - COUNTIN_TEXT_HALF_H;
    if view.hold {
        out.extend(text_draws_for(
            &font.layout_ascii("READY... GO!"),
            (COUNTIN_CENTRE_X - COUNTIN_LEFT_INSET, top),
            color,
        ));
    } else {
        out.extend(text_draws_for(
            &font.layout_ascii("READY"),
            (COUNTIN_CENTRE_X - view.x_offset - COUNTIN_LEFT_INSET, top),
            color,
        ));
        out.extend(text_draws_for(
            &font.layout_ascii("GO!"),
            (COUNTIN_CENTRE_X + view.x_offset, top),
            color,
        ));
    }
    scale_stage_text_draws(&mut out, origin, scale);
    out
}

/// Stage row the practice feedback caption sits on.
pub const TUTORIAL_FEEDBACK_Y: i32 = 0x68;
/// Stage column it starts at.
pub const TUTORIAL_FEEDBACK_X: i32 = 8;

/// One frame of the Disco King tutorial, as
/// `legaia_engine_core::dance_tutorial::TutorialFrame` carries it.
#[derive(Debug, Clone, Copy, Default)]
pub struct DanceTutorialView<'a> {
    /// Caption line seats this step draws at.
    pub captions: &'a [(i16, i16)],
    /// The opening prompt's two option rows, when it is up.
    pub options: Option<[(i16, i16); 2]>,
    /// The option cursor's seat, when the prompt is up.
    pub cursor_pos: Option<(i16, i16)>,
    /// `Some(true)` = the praise line, `Some(false)` = the timing scold.
    pub feedback: Option<bool>,
}

/// Draw the tutorial's captions / options / cursor for one frame, in surface
/// pixels.
///
/// The caption strings are the overlay's own rodata and are not read, so each
/// line draws a numbered placeholder at retail's seat; the option labels and
/// the cursor glyph are this port's.
pub fn dance_tutorial_draws_for(
    font: &legaia_font::Font,
    view: DanceTutorialView<'_>,
    origin: (i32, i32),
    scale: u32,
) -> Vec<TextDraw> {
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let dim: [f32; 4] = [0.65, 0.72, 0.8, 1.0];
    let mut out: Vec<TextDraw> = Vec::new();
    for (i, &(cx, cy)) in view.captions.iter().enumerate() {
        let layout = font.layout_ascii(&format!("(Disco King, line {})", i + 1));
        out.extend(text_draws_for(
            &layout,
            (i32::from(cx), i32::from(cy)),
            white,
        ));
    }
    if let Some(opts) = view.options.as_ref() {
        for (label, &(ox, oy)) in ["Yes", "No thanks"].iter().zip(opts.iter()) {
            out.extend(text_draws_for(
                &font.layout_ascii(label),
                (i32::from(ox), i32::from(oy)),
                white,
            ));
        }
    }
    if let Some((cx, cy)) = view.cursor_pos {
        out.extend(text_draws_for(
            &font.layout_ascii(">"),
            (i32::from(cx), i32::from(cy)),
            white,
        ));
    }
    if let Some(praise) = view.feedback {
        let layout = font.layout_ascii(if praise {
            "(praise - right on the beat)"
        } else {
            "(scold - watch the timing)"
        });
        out.extend(text_draws_for(
            &layout,
            (TUTORIAL_FEEDBACK_X, TUTORIAL_FEEDBACK_Y),
            dim,
        ));
    }
    scale_stage_text_draws(&mut out, origin, scale);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The held banner is one centred run; the sliding pair is two, and both
    /// halves have flown clear of the centre at the full offset.
    #[test]
    fn the_hold_draws_one_run_and_the_slide_draws_two() {
        let font = legaia_font::synthetic_for_tests();
        let hold = dance_countin_draws_for(
            &font,
            DanceCountInView {
                x_offset: 0,
                brightness: 0xFF,
                hold: true,
            },
            (0, 0),
            1,
        );
        assert!(!hold.is_empty());
        let slide = dance_countin_draws_for(
            &font,
            DanceCountInView {
                x_offset: 0xB4,
                brightness: 0x40,
                hold: false,
            },
            (0, 0),
            1,
        );
        let leftmost = slide.iter().map(|d| d.dst.0).min().unwrap();
        let rightmost = slide.iter().map(|d| d.dst.0).max().unwrap();
        assert!(
            leftmost < COUNTIN_CENTRE_X - 0xB4,
            "the left half did not fly out: {leftmost}"
        );
        assert!(
            rightmost >= COUNTIN_CENTRE_X + 0xB4,
            "the right half did not fly out: {rightmost}"
        );
    }

    /// Retail seats the banner at its own two rows - `a1 = 0x77` sliding,
    /// `a1 = 0x78` held - not at a chosen `0x40`, and it seats the sprite by
    /// its CENTRE.
    #[test]
    fn the_banner_sits_on_retails_own_rows() {
        let font = legaia_font::synthetic_for_tests();
        let row = |hold: bool| {
            dance_countin_draws_for(
                &font,
                DanceCountInView {
                    x_offset: 0,
                    brightness: 0xFF,
                    hold,
                },
                (0, 0),
                1,
            )
            .iter()
            .map(|d| d.dst.1)
            .min()
            .unwrap()
        };
        assert_eq!(row(false), COUNTIN_SLIDE_CENTRE_Y - COUNTIN_TEXT_HALF_H);
        assert_eq!(row(true), COUNTIN_HOLD_CENTRE_Y - COUNTIN_TEXT_HALF_H);
        assert_eq!(countin_centre_y(false), 0x77);
        assert_eq!(countin_centre_y(true), 0x78);
        // The record's own extents - 320 x 64 around the seat.
        assert_eq!(COUNTIN_SPRITE_HALF, (0xA0, 0x20));
    }

    /// Brightness reaches the tint alpha, so the fade is visible rather than
    /// being dropped on the floor.
    #[test]
    fn brightness_becomes_the_tint_alpha() {
        let font = legaia_font::synthetic_for_tests();
        for (b, want) in [(0, 0.0f32), (0x80, 128.0 / 255.0), (0xFF, 1.0)] {
            let draws = dance_countin_draws_for(
                &font,
                DanceCountInView {
                    x_offset: 0,
                    brightness: b,
                    hold: true,
                },
                (0, 0),
                1,
            );
            assert!((draws[0].color[3] - want).abs() < 1e-3, "brightness {b}");
        }
    }

    /// The prompt frame draws its captions, both options and the cursor; a
    /// caption-only step draws none of the latter.
    #[test]
    fn the_prompt_adds_options_and_a_cursor_over_the_caption_rows() {
        let font = legaia_font::synthetic_for_tests();
        let captions = [(8i16, 0x78i16), (8, 0x88)];
        let plain = dance_tutorial_draws_for(
            &font,
            DanceTutorialView {
                captions: &captions,
                ..Default::default()
            },
            (0, 0),
            1,
        );
        let prompt = dance_tutorial_draws_for(
            &font,
            DanceTutorialView {
                captions: &captions,
                options: Some([(0x48, 0x98), (0x48, 0xA8)]),
                cursor_pos: Some((0x28, 0x98)),
                ..Default::default()
            },
            (0, 0),
            1,
        );
        assert!(prompt.len() > plain.len());
        assert!(prompt.iter().any(|d| d.dst.0 == 0x28));
    }

    /// The stage transform is applied, so a scaled host does not draw the
    /// banner at native size in the corner.
    #[test]
    fn the_stage_transform_reaches_every_quad() {
        let font = legaia_font::synthetic_for_tests();
        let view = DanceCountInView {
            x_offset: 0,
            brightness: 0xFF,
            hold: true,
        };
        let one = dance_countin_draws_for(&font, view, (0, 0), 1);
        let three = dance_countin_draws_for(&font, view, (10, 20), 3);
        assert_eq!(one.len(), three.len());
        assert_eq!(three[0].dst.0, 10 + one[0].dst.0 * 3);
        assert_eq!(three[0].dst.2, one[0].dst.2 * 3);
    }
}

//! The dance minigame's two **text** layers: the pre-song count-in banner and
//! the Disco King how-to tutorial's captions.
//!
//! Both are placeholder letterforms on retail's own seats. The count-in's
//! banner is **not text at all** in retail: it is record `0` of the 20-byte
//! sprite table at `0x801D46CC`, a `160 x 32` quad (its `+0x0A` / `+0x0B`
//! bytes `0xa0` / `0x20` are the texel cell's **full** width and height;
//! `FUN_801D2F38` halves them at the record's unit scale and the three calls
//! all pass scale `0x1000`, so the half-extents are `0x50` / `0x10`) that
//! the emitter seats by its centre. The `a2 = 0` all three call sites pass
//! is that record index. The tutorial's caption strings are overlay rodata
//! (Sony text the port does not read).
//!
//! So what is pinned here is the *geometry*: retail's own banner centre
//! ([`COUNTIN_CENTRE_X`], [`COUNTIN_SLIDE_CENTRE_Y`] /
//! [`COUNTIN_HOLD_CENTRE_Y`] - the `a1` immediates at the three calls), the
//! record's extents and texel seat ([`COUNTIN_SPRITE_HALF`],
//! [`COUNTIN_SPRITE_UV`]), the sliding x offsets and the brightness ramp of
//! `dance_countin_banner_envelope` (`FUN_801d2d98`), and the caption / option
//! / cursor seats of the tutorial actor (`FUN_801D0750`).
//!
//! The sprite draws for real now, on both hosts:
//! [`dance_countin_prims`] emits retail's own quads against the dance page,
//! and what unlocked it was the **art**, not a draw call. Neither host staged
//! the page, because the port runs the dance over whichever scene the player
//! walked in from rather than inside the hall the way retail does, so the
//! page's texels were the interrupted town's; the entry path now stages the
//! rects the run's own widget table names
//! (`legaia_engine_core::dance::stage_dance_hud_vram`). The text below stays
//! as the fallback for a run with no page - a chart-only session, or a host
//! whose staging soft-failed.
//!
//! Retail varies the record's `+0x0F` blend byte per arm (`sb $v0, 0xf($t0)`
//! = `1` for the sliding halves at `0x801D2ED8`, `sb $zero, 0xf($t0)` = `0`
//! for the hold at `0x801D2F0C`), so the halves are semi-transparent and the
//! merged banner is opaque. The quad path carries that as the blend mode it
//! is; the placeholder can only carry it as the halved brightness the
//! envelope already reports.
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

/// The banner's half-extents in stage pixels, from **record 0** of the dance
/// overlay's 20-byte sprite table at `0x801D46CC`: bytes `+0x0A` / `+0x0B`
/// = `0xa0` / `0x20` are the texel cell's full width and height
/// ([`COUNTIN_SPRITE_CELL`]), and `FUN_801D2F38` halves them -
/// `(w * scale) >> 13` at `0x801D3180..0x801D319C` with the record's own
/// `scale = 0x1000`, then `* arg_scale >> 12` with the `0x1000` every
/// count-in call stores at `sp + 0x10` (`0x801D2EE0`, `0x801D2EF8`,
/// `0x801D2F10`) - before building `centre -+ half` on each axis
/// (`0x801D31E0..0x801D3214`). So the record covers `160 x 32` around its
/// seat, not the `320 x 64` the bytes read as when taken for half-extents.
/// The `a2 = 0` every call site passes is this **record index**, not a
/// coordinate.
pub const COUNTIN_SPRITE_HALF: (i32, i32) = (0x50, 0x10);

/// The texel cell of that record - `+0x0A` / `+0x0B` - which is also the
/// quad's full extent at unit scale.
pub const COUNTIN_SPRITE_CELL: (i32, i32) = (0xA0, 0x20);

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

// -------------------------------------------------------- the real banner
//
// The retail art path. Everything above this line is the placeholder a host
// falls back to when the dance overlay's texture page is not resident.

/// The banner record's texture page (`+0x04` = `0x0008`, the 4bpp page at
/// halfword `(512, 0)`) - every HUD row of the table carries it, and PROT
/// 1230's own TIM set is where the page's texels come from
/// (`legaia_engine_core::dance::DANCE_HUD_ART_PROT_ENTRY`).
pub const COUNTIN_SPRITE_TPAGE: u16 = 0x0008;

/// The record's `+0x13` semi-transparency **rate**, which the emitter folds
/// into the texpage attribute as `tpage + abr * 0x20`. All 34 rows carry `1`,
/// i.e. the additive `B + F` equation, so the HUD composites additively over
/// whatever it is drawn on.
pub const COUNTIN_SPRITE_ABR: u8 = 1;

/// Ordering-table bucket the count-in banner links at.
///
/// Retail links every HUD emit into one bucket and forces that slot to `3`
/// (`DAT_801D5154`), so the whole dance HUD shares a depth. Keeping the
/// number here rather than at two call sites is what stops the native window
/// and the play page from stacking the banner differently against the other
/// screen-space primitives a frame carries.
pub const COUNTIN_OT: u32 = 3;

/// The count-in banner's art, as the run's own widget table carries it.
///
/// Mirrors `legaia_asset::dance_art::DanceWidget` record `0` plus its `+0x13`
/// ABR byte, in the same "views are mirrors rather than imports" seam the
/// [`DanceCountInView`] above uses. [`Default`] is the published record - the
/// immediates this module documents - so a host with no parsed table still
/// draws the right cell of the right page, and a host with one passes the
/// disc's own values through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DanceCountInArt {
    /// Texel origin of the cell inside the page.
    pub uv: (u8, u8),
    /// Cell extent in texels; the emitter's rect is **half-open**, so the far
    /// corner is `uv + cell`, not `uv + cell - 1`.
    pub cell: (u8, u8),
    /// CLUT id (CBA) - a palette column of the row-500 strip.
    pub clut: u16,
    /// Texpage attribute (TSB) before the ABR fold.
    pub tpage: u16,
    /// Semi-transparency rate, folded in as `tpage + abr * 0x20`.
    pub abr: u8,
}

impl Default for DanceCountInArt {
    fn default() -> Self {
        Self {
            uv: COUNTIN_SPRITE_UV,
            cell: (COUNTIN_SPRITE_CELL.0 as u8, COUNTIN_SPRITE_CELL.1 as u8),
            clut: COUNTIN_SPRITE_CBA,
            tpage: COUNTIN_SPRITE_TPAGE,
            abr: COUNTIN_SPRITE_ABR,
        }
    }
}

impl DanceCountInArt {
    /// Lift the art out of a parsed widget record and its ABR byte - what a
    /// host holding a real overlay image passes instead of [`Default`].
    pub fn from_widget(w: &legaia_asset::dance_art::DanceWidget, abr: u8) -> Self {
        Self {
            uv: (w.u, w.v),
            cell: (w.w, w.h),
            clut: w.clut,
            tpage: w.tpage,
            abr,
        }
    }
}

/// The count-in banner as screen-space PSX primitives - retail's own `160 x 32`
/// sprite off the dance page, rather than the placeholder text above.
///
/// PORT: FUN_801d2d98 (`0x801D2EAC`..`0x801D2F18`) - the animator's **emit**
/// half. The envelope half is
/// `legaia_engine_core::dance::dance_countin_banner_envelope`; this is the
/// three `FUN_801D2F38` calls it feeds, which the disassembly pins exactly:
///
/// - the sliding arm (`a0 == 0`) emits **two whole copies** of the cell, at
///   `(0xa0 + s2, 0x77)` and `(0xa0 - s2, 0x77)`, after poking the record's
///   `+0x0F` translucency byte to `1` (`sb $v0, 0xf($t0)` at `0x801D2ED8`) and
///   halving the brightness with the `bgez`-biased shift at
///   `0x801D2EC0`..`0x801D2EC8`. The "halves" are that pair of full copies,
///   not half-width art;
/// - the hold arm (`a0 != 0`) emits **one** copy at `(0xa0 - s2, 0x78)` with
///   `+0x0F` cleared (`sb $zero, 0xf($t0)` at `0x801D2F0C`) and the brightness
///   unhalved. `s2` is zero throughout the hold, so the seat is the centre -
///   but the instruction is a subtract, and reproducing it is what keeps the
///   two arms one formula;
/// - every call passes `a2 = 0` (`clear a2`), the record **index**, and
///   stores `0x1000` at `sp + 0x10`, the caller scale. With the record's own
///   `0x1000` that makes the half-extent exactly `cell / 2`
///   ([`COUNTIN_SPRITE_HALF`]).
///
/// The brightness arrives already halved for the sliding arm, because
/// [`DanceCountInView`] carries the envelope's output rather than its input.
/// Colour is the record's white top and bottom edges scaled by it
/// (`channel * brightness >> 8`), which lands on the PSX blend's passthrough
/// level `0x80` at the envelope's own flat `0x80`.
pub fn dance_countin_prims(
    view: DanceCountInView,
    art: DanceCountInArt,
    ot_index: u32,
) -> Vec<crate::screen_prim::ScreenPrim> {
    let level = |c: u8| (u32::from(c) * view.brightness.clamp(0, 0xFF) as u32) >> 8;
    let colour = (level(0xFF) << 16) | (level(0xFF) << 8) | level(0xFF);
    let (u0, v0) = art.uv;
    let (u1, v1) = (u0.wrapping_add(art.cell.0), v0.wrapping_add(art.cell.1));
    let (hw, hh) = (
        (i32::from(art.cell.0) / 2) as i16,
        (i32::from(art.cell.1) / 2) as i16,
    );
    let quad = |cx: i32, cy: i32, semi: bool| {
        let (x0, x1) = ((cx as i16) - hw, (cx as i16) + hw);
        let (y0, y1) = ((cy as i16) - hh, (cy as i16) + hh);
        crate::screen_prim::ScreenPrim::Textured(crate::screen_prim::ScreenQuad {
            xy: [(x0, y0), (x1, y0), (x0, y1), (x1, y1)],
            uv: [(u0, v0), (u1, v0), (u0, v1), (u1, v1)],
            clut: art.clut,
            tpage: art.tpage + u16::from(art.abr) * 0x20,
            color: colour,
            gouraud: None,
            semi_transparent: semi,
            ot_index,
            depth: None,
        })
    };
    let y = countin_centre_y(view.hold);
    if view.hold {
        vec![quad(COUNTIN_CENTRE_X - view.x_offset, y, false)]
    } else {
        vec![
            quad(COUNTIN_CENTRE_X + view.x_offset, y, true),
            quad(COUNTIN_CENTRE_X - view.x_offset, y, true),
        ]
    }
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
        // The record's own extents - the 160 x 32 cell, halved about the seat.
        assert_eq!(COUNTIN_SPRITE_CELL, (0xA0, 0x20));
        assert_eq!(COUNTIN_SPRITE_HALF.0 * 2, COUNTIN_SPRITE_CELL.0);
        assert_eq!(COUNTIN_SPRITE_HALF.1 * 2, COUNTIN_SPRITE_CELL.1);
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

    fn quad(p: &crate::screen_prim::ScreenPrim) -> crate::screen_prim::ScreenQuad {
        match p {
            crate::screen_prim::ScreenPrim::Textured(q) => *q,
            crate::screen_prim::ScreenPrim::Flat(_) => {
                panic!("the banner emits textured quads only")
            }
        }
    }

    /// The sliding arm emits **two whole copies** of the cell - the "halves"
    /// are that pair, not half-width art - and the hold arm emits one. A
    /// reading that halves the art instead draws a 80-px banner twice.
    #[test]
    fn the_sliding_arm_emits_two_whole_copies() {
        let art = DanceCountInArt::default();
        let slide = dance_countin_prims(
            DanceCountInView {
                x_offset: 0x30,
                brightness: 0x40,
                hold: false,
            },
            art,
            COUNTIN_OT,
        );
        assert_eq!(slide.len(), 2);
        for p in &slide {
            let q = quad(p);
            let w = i32::from(q.xy[1].0 - q.xy[0].0);
            let h = i32::from(q.xy[2].1 - q.xy[0].1);
            assert_eq!((w, h), COUNTIN_SPRITE_CELL, "each copy is the whole cell");
            assert_eq!(i32::from(q.xy[0].1), COUNTIN_SLIDE_CENTRE_Y - 0x10);
            assert!(q.semi_transparent, "the halves poke +0x0F to 1");
        }
        let centres: Vec<i32> = slide
            .iter()
            .map(|p| i32::from(quad(p).xy[0].0) + COUNTIN_SPRITE_HALF.0)
            .collect();
        assert!(centres.contains(&(COUNTIN_CENTRE_X + 0x30)));
        assert!(centres.contains(&(COUNTIN_CENTRE_X - 0x30)));

        let hold = dance_countin_prims(
            DanceCountInView {
                x_offset: 0,
                brightness: 0xFF,
                hold: true,
            },
            art,
            COUNTIN_OT,
        );
        assert_eq!(hold.len(), 1);
        let q = quad(&hold[0]);
        assert_eq!(i32::from(q.xy[0].1), COUNTIN_HOLD_CENTRE_Y - 0x10);
        assert!(!q.semi_transparent, "the merged banner clears +0x0F");
    }

    /// The texel rect is retail's **half-open** one (`u + w`, not
    /// `u + w - 1`): the dance emitter writes the sum straight out, unlike
    /// the battle numerals' inclusive cells, and picking the wrong
    /// convention shifts every column of a 160-texel banner.
    #[test]
    fn the_texel_rect_is_half_open_and_the_page_is_additive() {
        let art = DanceCountInArt::default();
        let q = quad(
            &dance_countin_prims(
                DanceCountInView {
                    x_offset: 0,
                    brightness: 0x80,
                    hold: true,
                },
                art,
                COUNTIN_OT,
            )[0],
        );
        assert_eq!(q.uv[0], COUNTIN_SPRITE_UV);
        assert_eq!(
            q.uv[3],
            (
                COUNTIN_SPRITE_UV
                    .0
                    .wrapping_add(COUNTIN_SPRITE_CELL.0 as u8),
                COUNTIN_SPRITE_UV
                    .1
                    .wrapping_add(COUNTIN_SPRITE_CELL.1 as u8),
            )
        );
        assert_eq!(q.clut, COUNTIN_SPRITE_CBA);
        // `tpage + abr * 0x20`, so the quad reports the additive equation.
        assert_eq!(q.tpage, COUNTIN_SPRITE_TPAGE + 0x20);
        assert_eq!(q.abr_mode(), 1, "B + F");
        // Brightness 0x80 on the record's white edges is the blend's
        // passthrough level.
        assert_eq!(q.color, 0x007F_7F7F);
    }

    /// A parsed record overrides the published immediates - the art is disc
    /// data, and a host that holds the table must be able to pass it.
    #[test]
    fn a_parsed_record_overrides_the_defaults() {
        let w = legaia_asset::dance_art::DanceWidget {
            scale: 0x1000,
            tpage: 0x0009,
            clut: 0x7D01,
            u: 0x10,
            v: 0x20,
            w: 0x40,
            h: 0x10,
            rgb_top: [0xFF, 0xFF, 0xFF],
            rgb_bottom: [0xFF, 0xFF, 0xFF],
            semi: 1,
        };
        let art = DanceCountInArt::from_widget(&w, 3);
        let q = quad(
            &dance_countin_prims(
                DanceCountInView {
                    x_offset: 0,
                    brightness: 0xFF,
                    hold: true,
                },
                art,
                COUNTIN_OT,
            )[0],
        );
        assert_eq!(q.uv[0], (0x10, 0x20));
        assert_eq!(q.clut, 0x7D01);
        assert_eq!(q.tpage, 0x0009 + 3 * 0x20);
        assert_eq!(i32::from(q.xy[1].0 - q.xy[0].0), 0x40);
    }
}

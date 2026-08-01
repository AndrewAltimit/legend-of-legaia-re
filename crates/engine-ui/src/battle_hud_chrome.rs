//! The two battle-screen surfaces that are widget-table records rather
//! than plate runs: the **top-of-screen message banner** and the **badge
//! cells** (status-element and element) the HUD blits out of the atlas.
//!
//! Both come out of the widget-class table at `SCUS_942.54` VA
//! `0x800732A4` (`legaia_asset::ui_widgets`,
//! `docs/subsystems/battle.md`), so the geometry here is disc data read
//! back rather than measurement:
//!
//! * the banner is record `0x03` - **class 0**, tile-set 0, sub-palette 2,
//!   seat bias `(-8, -8)`;
//! * the nine status badges are records `0x18..=0x20`, 48x16 cells with
//!   `bias == (0, 0)` and `chain == 0`, so each is one sprite seated at
//!   its caller's `(x, y)` verbatim;
//! * the eight plain element badges are records `0x8B..=0x92`, 20x12.
//!
//! ## The banner draws no interior fill
//!
//! Retail's display list for the banner carries the border sprites and the
//! glyph run and **nothing else** - the scene shows through it. The 32x32
//! blue-marbled patch record `0x03` carries as its own rect is the fill
//! the framed *menu* windows use, not this banner, which is why
//! [`message_banner_chrome_draws_for`] emits no
//! [`SaveMenuAtlasRects::dialog_fill`] quad while
//! [`crate::dialog_window_chrome_draws_for`] does.
//!
//! ## It shares its seat with the actor-name plaque
//!
//! Both surfaces sit on content pen `(16, 12)`. They are alternatives, not
//! layers: a frame draws the plaque or the banner, never both. The HUD
//! builder enforces that ([`crate::BattleHudFrame::banner`] wins), because
//! drawing both puts two text runs on the same pixels.

use crate::*;

/// Content pen of the banner - the same pen the actor-name plaque takes.
pub const BANNER_PEN: (i32, i32) = (16, 12);
/// Border width of the class-0 frame, all four sides.
pub const BANNER_BORDER: i32 = 4;
/// How far the content pen sits inside the interior, both axes.
pub const BANNER_PEN_INSET: i32 = 4;
/// Interior height of a one-line banner. Retail's captured frame is 28
/// tall: `4 + 20 + 4`.
pub const BANNER_INTERIOR_H: i32 = 20;
/// Row pitch when a message runs to more than one line - the pitch every
/// other in-battle text box uses.
pub const BANNER_ROW_PITCH: i32 = 14;

/// Interior rect of a banner whose measured content is `w` x `h`, at
/// [`BANNER_PEN`].
///
/// The pen sits [`BANNER_PEN_INSET`] inside the interior on both axes and
/// the interior's right edge lands on `pen.x + w`, which is the packet-read
/// "its right column starts at `16 + measured_width`".
pub const fn banner_interior(w: i32, h: i32) -> (i32, i32, i32, i32) {
    (
        BANNER_PEN.0 - BANNER_PEN_INSET,
        BANNER_PEN.1 - BANNER_PEN_INSET,
        w + BANNER_PEN_INSET,
        h,
    )
}

/// Whole drawn footprint of a banner whose measured content is `w` x `h`:
/// the interior inflated by [`BANNER_BORDER`] on every side.
///
/// For retail's captured single-line frame (`w` measured, `h = 20`) this is
/// origin `(8, 4)` and height `28`.
pub const fn banner_frame(w: i32, h: i32) -> (i32, i32, i32, i32) {
    let (ix, iy, iw, ih) = banner_interior(w, h);
    (
        ix - BANNER_BORDER,
        iy - BANNER_BORDER,
        iw + 2 * BANNER_BORDER,
        ih + 2 * BANNER_BORDER,
    )
}

/// Interior height for a message of `lines` rows: one row is retail's
/// captured 20, each further row adds the text pitch.
pub const fn banner_interior_h(lines: usize) -> i32 {
    BANNER_INTERIOR_H + (lines as i32 - 1) * BANNER_ROW_PITCH
}

/// Build the banner's frame sprites - the class-0 nine-slice, corners
/// first, then the four tiled edges.
///
/// `content` is the measured `(w, h)` of the message. Tiles come from the
/// gold border tile-set the save/load panel already samples
/// (`title_pak::OVERLAY_SYSTEM_UI_PANEL_*`, which **is** widget tile-set 0
/// at texels `(160, 0)`); each edge run clips its final tile to the
/// remainder, the same law a plate run's last body tile follows.
///
/// Emits **no interior fill** - see the module header.
pub fn message_banner_chrome_draws_for(
    rects: &SaveMenuAtlasRects,
    content: (i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let (fx, fy, fw, fh) = banner_frame(content.0, content.1);
    let (ix, iy, iw, ih) = banner_interior(content.0, content.1);
    let s = stage_scale as i32;
    let mut out = Vec::new();
    let mut blit = |src: (u32, u32, u32, u32), x: i32, y: i32, w: u32, h: u32| {
        if w == 0 || h == 0 {
            return;
        }
        out.push(SpriteDraw {
            dst: (
                stage_origin.0 + x * s,
                stage_origin.1 + y * s,
                w * stage_scale,
                h * stage_scale,
            ),
            src: (src.0, src.1, w, h),
            color: [1.0, 1.0, 1.0, 1.0],
        });
    };
    let b = BANNER_BORDER as u32;
    // Corners.
    blit(rects.panel_tl, fx, fy, b, b);
    blit(rects.panel_tr, fx + fw - BANNER_BORDER, fy, b, b);
    blit(rects.panel_bl, fx, fy + fh - BANNER_BORDER, b, b);
    blit(
        rects.panel_br,
        fx + fw - BANNER_BORDER,
        fy + fh - BANNER_BORDER,
        b,
        b,
    );
    // Top and bottom edges tile across the interior width from the
    // interior's own left edge, last tile clipped.
    let mut done = 0;
    while done < iw {
        let w = (rects.panel_top.2 as i32).min(iw - done) as u32;
        blit(rects.panel_top, ix + done, fy, w, b);
        blit(rects.panel_bot, ix + done, fy + fh - BANNER_BORDER, w, b);
        done += w as i32;
    }
    // Left and right columns tile down the interior height.
    let mut done = 0;
    while done < ih {
        let h = (rects.panel_left.3 as i32).min(ih - done) as u32;
        blit(rects.panel_left, fx, iy + done, b, h);
        blit(rects.panel_right, fx + fw - BANNER_BORDER, iy + done, b, h);
        done += h as i32;
    }
    out
}

/// The banner's text rows, in stage pixels: the message laid out at
/// [`BANNER_PEN`] on the [`BANNER_ROW_PITCH`].
pub fn message_banner_text_draws_for(font: &legaia_font::Font, text: &str) -> Vec<TextDraw> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let layout = font.layout_ascii(line);
        out.extend(text_draws_for(
            &layout,
            (BANNER_PEN.0, BANNER_PEN.1 + i as i32 * BANNER_ROW_PITCH),
            MENU_TEXT_WHITE,
        ));
    }
    out
}

/// Measured `(w, h)` of a message for [`banner_frame`]: the widest
/// rendered line, and the interior height its line count implies.
pub fn message_banner_content(font: &legaia_font::Font, text: &str) -> (i32, i32) {
    let w = text
        .lines()
        .map(|l| font.layout_ascii(l).advance_x as i32)
        .max()
        .unwrap_or(0);
    (w, banner_interior_h(text.lines().count().max(1)))
}

// ---------------------------------------------------------------------------
// Badge cells
// ---------------------------------------------------------------------------

/// Number of status-element badges - `FUN_8002C2E4`'s nine ladder outcomes.
pub const STATUS_BADGE_COUNT: usize = 9;
/// Number of plain element badges.
pub const ELEMENT_BADGE_COUNT: usize = 8;
/// Status badge cell size on the sheet and in the atlas.
pub const STATUS_BADGE_SIZE: (i32, i32) = (48, 16);
/// Element badge cell size.
pub const ELEMENT_BADGE_SIZE: (i32, i32) = (20, 12);

/// Where a status badge seats, **relative to a roster panel's top-left
/// corner**.
///
/// `FUN_8002C2E4`'s ladder arm calls `FUN_8002C488(pen.x + 0x33,
/// pen.y - 4, sprite)` (`addiu s1,s1,0x27` then `addiu a0,s1,0xc`), and the
/// panel's pen is the name seat `(+5, +4)` - so a matched ailment lands at
/// `(56, 0)`. The single-sprite path applies no bias of its own, so this
/// caller offset is the whole placement.
///
/// The no-ailment arm of the same ladder is the `LV` label at
/// `pen + (0x3B, 2)` = `(64, 6)`, which is the seat the HUD already draws
/// the level on - the two are alternatives on one seat, not neighbours.
pub const STATUS_BADGE_PANEL_SEAT: (i32, i32) = (56, 0);

/// Atlas cells for the badges the HUD blits, `None` per cell the atlas
/// could not bake (its palette source was outside the caller's slice).
///
/// Hosts fill this from `legaia_engine_core::save_menu_atlas`'s
/// `band_status_badges` / `band_element_badges`; the HUD falls back to its
/// labelled text tag for any `None`, so a host that cannot reach the art
/// still reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct BattleBadgeRects {
    /// Status-element badges in ladder order, sprite `0x18` first.
    pub status: [Option<(u32, u32, u32, u32)>; STATUS_BADGE_COUNT],
    /// Plain element badges, index `0..8`.
    pub element: [Option<(u32, u32, u32, u32)>; ELEMENT_BADGE_COUNT],
}

impl BattleBadgeRects {
    /// Cell for retail status sprite id `sprite` (`0x18..=0x20`).
    pub fn status_badge(&self, sprite: u8) -> Option<(u32, u32, u32, u32)> {
        if !(0x18..=0x20).contains(&sprite) {
            return None;
        }
        self.status[(sprite - 0x18) as usize]
    }
    /// Cell for element badge `index`.
    pub fn element_badge(&self, index: u8) -> Option<(u32, u32, u32, u32)> {
        self.element.get(index as usize).copied().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The packet-pinned frame: content pen `(16, 12)`, frame origin
    /// `(8, 4)`, 28 tall, right border column at `16 + measured_width`.
    #[test]
    fn the_banner_frame_is_the_captured_one() {
        let (fx, fy, _fw, fh) = banner_frame(200, BANNER_INTERIOR_H);
        assert_eq!((fx, fy), (8, 4));
        assert_eq!(fh, 28);
        let (ix, iy, iw, ih) = banner_interior(200, BANNER_INTERIOR_H);
        assert_eq!((ix, iy), (12, 8), "the interior starts 4 inside the frame");
        assert_eq!(ix + iw, BANNER_PEN.0 + 200, "right column at pen + width");
        assert_eq!(ih, BANNER_INTERIOR_H);
    }

    /// A second line grows the interior by the text pitch, nothing else.
    #[test]
    fn extra_rows_only_grow_the_interior() {
        assert_eq!(banner_interior_h(1), BANNER_INTERIOR_H);
        assert_eq!(banner_interior_h(2), BANNER_INTERIOR_H + BANNER_ROW_PITCH);
        let one = banner_frame(60, banner_interior_h(1));
        let two = banner_frame(60, banner_interior_h(2));
        assert_eq!((one.0, one.1, one.2), (two.0, two.1, two.2));
        assert_eq!(two.3 - one.3, BANNER_ROW_PITCH);
    }

    /// Retail's banner has no fill primitive, so the builder emits only
    /// border tiles - and every one of them samples a panel tile rect.
    #[test]
    fn the_banner_draws_border_tiles_and_no_fill() {
        let rects = SaveMenuAtlasRects {
            panel_tl: (160, 0, 4, 4),
            panel_tr: (188, 0, 4, 4),
            panel_bl: (160, 28, 4, 4),
            panel_br: (188, 28, 4, 4),
            panel_top: (164, 0, 24, 4),
            panel_bot: (164, 28, 24, 4),
            panel_left: (160, 4, 4, 21),
            panel_right: (188, 4, 4, 21),
            dialog_fill: (240, 200, 4, 32),
            ..Default::default()
        };
        let draws = message_banner_chrome_draws_for(&rects, (60, BANNER_INTERIOR_H), (0, 0), 1);
        assert!(!draws.is_empty());
        assert!(
            draws.iter().all(|d| d.src.0 != 240),
            "the banner must not emit the dialog fill"
        );
        // Four corners, then a tiled top/bottom pair and a left/right pair.
        assert_eq!(
            draws
                .iter()
                .filter(|d| d.src.2 == 4 && d.src.3 == 4)
                .count(),
            4,
            "exactly four corner tiles"
        );
        // Nothing leaves the frame.
        let (fx, fy, fw, fh) = banner_frame(60, BANNER_INTERIOR_H);
        for d in &draws {
            assert!(d.dst.0 >= fx && d.dst.1 >= fy);
            assert!(d.dst.0 + d.dst.2 as i32 <= fx + fw);
            assert!(d.dst.1 + d.dst.3 as i32 <= fy + fh);
        }
    }

    /// The ladder's two arms share one seat: the matched-ailment badge and
    /// the no-ailment `LV` label are both offsets off the panel name pen.
    #[test]
    fn the_badge_seat_is_the_ladder_caller_offset() {
        // pen = panel + (5, 4); ladder arm = pen + (0x33, -4).
        assert_eq!(STATUS_BADGE_PANEL_SEAT, (5 + 0x33, 4 - 4));
    }

    #[test]
    fn badge_lookup_is_bounded_by_the_ladder_band() {
        let mut r = BattleBadgeRects::default();
        r.status[0] = Some((0, 128, 48, 16));
        assert_eq!(r.status_badge(0x18), Some((0, 128, 48, 16)));
        assert_eq!(r.status_badge(0x19), None, "unbaked cell");
        assert_eq!(r.status_badge(0x21), None, "outside the band");
        assert_eq!(r.status_badge(0x00), None);
        assert_eq!(r.element_badge(8), None);
    }
}

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
//! ## The frame is filled
//!
//! A class-0 frame is drawn over a **fill** that covers the whole frame
//! rect: `FUN_8002BDC4`, which the layout dispatcher calls for every class-0
//! node (`jal` at `0x8002D7E8`), queues opaque gouraud textured quads
//! (`POLY_GT4`, code `0x3C`) of the widget record's own 32x32 blue-marbled
//! rect - texels `(128, 0)` on CLUT `(32, 511)` - before the border sprites.
//! The quads tile the frame in 32-pixel columns and 32-row bands from the
//! frame origin, last column and band clipped, texels 1:1 with pixels, and
//! the vertex grey runs from `0x40` at the frame's top to `0x88` at its
//! bottom in steps of `0x900 / height` per band. Two retail display lists
//! carry exactly that run: the message banner (`noa_levelup_banner`,
//! ten quads over `(8, 4)..(304, 32)`) and the intro labels (a
//! `karisto_sol_pre_encounter` intro frame, three per label). See
//! [`class0_fill_draws_at`].
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
/// Content-box width of the message banner: `+0x06` of every placement
/// record that carries one (`0x45..=0x4B`, `0x59`, `0x65`, `0x66`, all kind
/// `3` on seat `(16, 14)`). The box is fixed, not measured, which is why
/// every retail message frame spans `(8, 4)..(304, 32)` whatever its text.
pub const BANNER_BOX_W: i32 = 280;
/// Fill-tile edge: `FUN_8002BDC4` steps its columns by the record's `w`
/// and its bands by the record's `h`, both 32 for widget record `3`.
pub const CLASS0_FILL_TILE: i32 = 32;
/// Fill vertex grey at the frame's top edge (`li a2,0x40`, `0x8002BE38`).
pub const CLASS0_FILL_TOP: i32 = 0x40;
/// Fill vertex grey at the frame's bottom edge (`li a2,0x88`, `0x8002BE94`).
pub const CLASS0_FILL_BOTTOM: i32 = 0x88;
/// Interior height of a one-line banner. Retail's captured frame is 28
/// tall: `4 + 20 + 4`.
pub const BANNER_INTERIOR_H: i32 = 20;
/// Row pitch when a message runs to more than one line - the pitch every
/// other in-battle text box uses.
pub const BANNER_ROW_PITCH: i32 = 14;

/// Interior rect of a banner whose content box is `w` wide and whose
/// interior is `h` tall, at [`BANNER_PEN`].
///
/// The pen sits [`BANNER_PEN_INSET`] inside the interior on both axes, and
/// the interior is the content box grown by the inset on **both** sides:
/// `w + 8` wide, so the right border column starts at `pen.x + w + 4`.
pub const fn banner_interior(w: i32, h: i32) -> (i32, i32, i32, i32) {
    banner_interior_at(BANNER_PEN, w, h)
}

/// [`banner_interior`] for a class-0 frame on any content pen - the law is
/// the widget record's, not the seat's: the battle-intro enemy-name labels
/// wear the same frame on pen `(x, 48)`. `Moldy Worm` (66 wide) on pen
/// `(86, 48)` frames `(78, 40)..(159, 67)` in retail's display list, its top
/// edge tiled at `82 / 106 / 130` and clipped to 2 pixels at `154`.
pub const fn banner_interior_at(pen: (i32, i32), w: i32, h: i32) -> (i32, i32, i32, i32) {
    (
        pen.0 - BANNER_PEN_INSET,
        pen.1 - BANNER_PEN_INSET,
        w + 2 * BANNER_PEN_INSET,
        h,
    )
}

/// Whole drawn footprint of a banner whose measured content is `w` x `h`:
/// the interior inflated by [`BANNER_BORDER`] on every side.
///
/// For retail's captured single-line frame (`w` measured, `h = 20`) this is
/// origin `(8, 4)` and height `28`.
pub const fn banner_frame(w: i32, h: i32) -> (i32, i32, i32, i32) {
    banner_frame_at(BANNER_PEN, w, h)
}

/// [`banner_frame`] on any content pen ([`banner_interior_at`]).
pub const fn banner_frame_at(pen: (i32, i32), w: i32, h: i32) -> (i32, i32, i32, i32) {
    let (ix, iy, iw, ih) = banner_interior_at(pen, w, h);
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
/// `content` is the `(w, h)` of [`message_banner_content`]. Tiles come from the
/// gold border tile-set the save/load panel already samples
/// (`title_pak::OVERLAY_SYSTEM_UI_PANEL_*`, which **is** widget tile-set 0
/// at texels `(160, 0)`); each edge run clips its final tile to the
/// remainder, the same law a plate run's last body tile follows.
///
/// The fill goes first, under the border - see the module header.
pub fn message_banner_chrome_draws_for(
    rects: &SaveMenuAtlasRects,
    content: (i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    class0_frame_draws_at(rects, BANNER_PEN, content, stage_origin, stage_scale)
}

/// The class-0 frame's **fill**: the frame rect `(x, y, w, h)` covered by
/// the blue-marbled patch, drawn under the border.
///
/// PORT: FUN_8002BDC4 - columns of [`CLASS0_FILL_TILE`] from the frame's
/// left edge and bands of the same height from its top, the last of each
/// clipped, texels 1:1 with pixels (each band restarts at texel row 0).
/// Band `k`'s top grey is `0x40 + k * (0x900 / h)` and its bottom grey the
/// next band's top, the last band ending on `0x88`; the GPU interpolates
/// between them down the band, and the texel is modulated `texel * grey /
/// 128`.
///
/// A frame no taller than the baked tile (every one-line frame is 28) is
/// one band whose grey runs `0x40 -> 0x88` over its height, which is the
/// ramp `rects.panel_interior` is baked with - so it draws as whole-height
/// column sprites of that tile. A taller frame draws one-row strips of the
/// raw `rects.panel_filigree` tile, each tinted to its row's grey (the atlas
/// carries the patch's full 32 texel rows; a shorter tile wraps).
pub fn class0_fill_draws_at(
    rects: &SaveMenuAtlasRects,
    frame: (i32, i32, i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let (fx, fy, fw, fh) = frame;
    let s = stage_scale as i32;
    let mut out = Vec::new();
    if fw <= 0 || fh <= 0 {
        return out;
    }
    let mut push = |src: (u32, u32, u32, u32), x: i32, y: i32, w: i32, h: i32, grey: f32| {
        out.push(SpriteDraw {
            dst: (
                stage_origin.0 + x * s,
                stage_origin.1 + y * s,
                w as u32 * stage_scale,
                h as u32 * stage_scale,
            ),
            src,
            color: [grey, grey, grey, 1.0],
        });
    };
    let baked = rects.panel_interior;
    if baked.2 > 0 && fh <= baked.3 as i32 {
        let mut x = 0;
        while x < fw {
            let cw = CLASS0_FILL_TILE.min(fw - x).min(baked.2 as i32);
            push(
                (baked.0, baked.1, cw as u32, fh as u32),
                fx + x,
                fy,
                cw,
                fh,
                1.0,
            );
            x += cw;
        }
        return out;
    }
    let raw = rects.panel_filigree;
    if raw.2 == 0 || raw.3 == 0 {
        return out;
    }
    let step = 0x900 / fh;
    let mut band_y = 0;
    let mut top = CLASS0_FILL_TOP;
    while band_y < fh {
        let bh = CLASS0_FILL_TILE.min(fh - band_y);
        let bottom = if band_y + bh >= fh {
            CLASS0_FILL_BOTTOM
        } else {
            top + step
        };
        for row in 0..bh {
            let grey = top + (bottom - top) * row / bh;
            let tex_row = raw.1 + (row as u32 % raw.3);
            let mut x = 0;
            while x < fw {
                let cw = CLASS0_FILL_TILE.min(fw - x).min(raw.2 as i32);
                push(
                    (raw.0, tex_row, cw as u32, 1),
                    fx + x,
                    fy + band_y + row,
                    cw,
                    1,
                    grey as f32 / 128.0,
                );
                x += cw;
            }
        }
        band_y += bh;
        top = bottom;
    }
    out
}

/// The class-0 frame (widget record `3`) around `content` on content pen
/// `pen` - [`message_banner_chrome_draws_for`] is this on [`BANNER_PEN`], and
/// the battle-intro enemy-name labels are this on `(x, 48)`: retail spawns
/// both through `FUN_8003541C` with kind `3`
/// (`docs/subsystems/battle.md`, the intro-banner section).
pub fn class0_frame_draws_at(
    rects: &SaveMenuAtlasRects,
    pen: (i32, i32),
    content: (i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let (fx, fy, fw, fh) = banner_frame_at(pen, content.0, content.1);
    let (ix, iy, iw, ih) = banner_interior_at(pen, content.0, content.1);
    let s = stage_scale as i32;
    let mut out = class0_fill_draws_at(rects, (fx, fy, fw, fh), stage_origin, stage_scale);
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

/// The class-0 frame around a **measured text actor's** centre rect - the
/// skin every kind-`0x0D` box wears (the sparring-tutorial prompts and the
/// timed-fight strip, both registered through `FUN_8003541C` with an explicit
/// `(x, y, w, h)` and style word `0x44`).
///
/// It is the banner's frame, not the dialog reading box's: the
/// `v0_1_battle_command_menu` display list draws the lesson intro
/// (rect `(16, 14, 279, 10)`) as ten opaque `POLY_GT4` fill tiles from
/// texel `(128, 0)` under CLUT `(32, 511)`, grey `0x40` at the top and
/// `0x88` at the bottom, over `(8, 6)..(303, 32)`, with the gold edge
/// sprites of tile-set 0 inside that rect (left column at `(8, 10)`, `18`
/// tall; bottom run on row `28`). So the frame is the centre rect inflated
/// by 8 on every side - the pen sits [`BANNER_PEN_INSET`] inside an interior
/// that is itself [`BANNER_BORDER`] inside the frame - and the fill is the
/// blue marble patch, opaque, where the dialog box's is a translucent
/// gradient.
///
/// REF: FUN_8003541C, FUN_8002BDC4
pub fn text_actor_frame_draws_for(
    rects: &SaveMenuAtlasRects,
    rect: (i32, i32, i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let (x, y, w, h) = rect;
    class0_frame_draws_at(
        rects,
        (x, y),
        (w, h + 2 * BANNER_PEN_INSET),
        stage_origin,
        stage_scale,
    )
}

/// One banner line as the text engine lays it: the authoring escape `^X`
/// becomes the runtime icon escape `0xCE (X - 0x2D)`, the preprocessor
/// `FUN_80036514`'s rewrite, so a spell name carrying its element plate
/// (`^A Gimard` - [`legaia_font::Font::layout`] places escape `0x14`, the
/// fire plate, and advances its `20`) draws the plate in front of the text.
/// Every other byte passes through.
///
/// REF: FUN_80036514
pub fn banner_line_bytes(line: &str) -> Vec<u8> {
    let b = line.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'^' && i + 1 < b.len() && b[i + 1] >= 0x2D {
            out.push(legaia_font::ESCAPE_GLYPH_BYTE);
            out.push(b[i + 1] - 0x2D);
            i += 2;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// The banner's text rows, in stage pixels: the message laid out at
/// [`BANNER_PEN`] on the [`BANNER_ROW_PITCH`].
pub fn message_banner_text_draws_for(font: &legaia_font::Font, text: &str) -> Vec<TextDraw> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let layout = font.layout(&banner_line_bytes(line));
        out.extend(text_draws_for(
            &layout,
            (BANNER_PEN.0, BANNER_PEN.1 + i as i32 * BANNER_ROW_PITCH),
            MENU_TEXT_WHITE,
        ));
    }
    out
}

/// `(w, h)` of a message for [`banner_frame`]: the record's fixed
/// [`BANNER_BOX_W`] content box (grown only for a line wider than it, which
/// no retail message is), and the interior height its line count implies.
pub fn message_banner_content(font: &legaia_font::Font, text: &str) -> (i32, i32) {
    let w = text
        .lines()
        .map(|l| font.layout(&banner_line_bytes(l)).advance_x as i32)
        .max()
        .unwrap_or(0)
        .max(BANNER_BOX_W);
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

/// Vertical inset of the **fallback tag** inside a status badge's cell.
///
/// A host with no baked badge cell draws
/// [`crate::status_element_label`]'s tag in the cell's place instead, and it
/// has to land where the cell's own word does or the panel reads as if its
/// tag had slipped. In a retail frame the badge cell's drawn word occupies
/// rows `1..15` of the 16-row cell; a 12-px glyph run centred in that band
/// starts two rows in.
pub const STATUS_BADGE_TAG_DY: i32 = 2;

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

    /// `^X` is the authoring form of the icon escape: `FUN_80036514` turns
    /// it into `0xCE (X - 0x2D)`, so `^A` is the fire plate `0x14`.
    #[test]
    fn a_banner_line_expands_the_caret_icon_escape() {
        assert_eq!(
            banner_line_bytes("^A Gimard's"),
            [&[0xCE, 0x14][..], b" Gimard's"].concat()
        );
        assert_eq!(banner_line_bytes("No effect."), b"No effect.".to_vec());
    }

    /// The packet-pinned frames: content pen `(16, 12)` with the record's
    /// 280-wide box frames `(8, 4)` 296x28 (`noa_levelup_banner`), and the
    /// intro label `Moldy Worm` (66 wide) on pen `(86, 48)` frames
    /// `(78, 40)` 82x28 with its right column at `156`.
    #[test]
    fn the_banner_frame_is_the_captured_one() {
        assert_eq!(
            banner_frame(BANNER_BOX_W, BANNER_INTERIOR_H),
            (8, 4, 296, 28)
        );
        let (ix, iy, iw, ih) = banner_interior(200, BANNER_INTERIOR_H);
        assert_eq!((ix, iy), (12, 8), "the interior starts 4 inside the frame");
        assert_eq!(
            ix + iw,
            BANNER_PEN.0 + 200 + 4,
            "right column at pen + w + 4"
        );
        assert_eq!(ih, BANNER_INTERIOR_H);
        assert_eq!(
            banner_frame_at((86, 48), 66, BANNER_INTERIOR_H),
            (78, 40, 82, 28)
        );
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

    fn panel_rects() -> SaveMenuAtlasRects {
        SaveMenuAtlasRects {
            panel_tl: (160, 0, 4, 4),
            panel_tr: (188, 0, 4, 4),
            panel_bl: (160, 28, 4, 4),
            panel_br: (188, 28, 4, 4),
            panel_top: (164, 0, 24, 4),
            panel_bot: (164, 28, 24, 4),
            panel_left: (160, 4, 4, 21),
            panel_right: (188, 4, 4, 21),
            panel_interior: (128, 0, 32, 29),
            panel_filigree: (0, 200, 32, 29),
            dialog_fill: (240, 200, 4, 32),
            ..Default::default()
        }
    }

    /// The retail fill under the message banner: ten columns from the
    /// frame origin, 32 wide with an 8-wide remainder, each the full frame
    /// height, ahead of every border sprite - the run `FUN_8002BDC4` queued
    /// in `noa_levelup_banner`.
    #[test]
    fn the_banner_is_filled_under_its_border() {
        let rects = panel_rects();
        let draws =
            message_banner_chrome_draws_for(&rects, (BANNER_BOX_W, BANNER_INTERIOR_H), (0, 0), 1);
        let fill: Vec<_> = draws.iter().take_while(|d| d.src.0 == 128).collect();
        assert_eq!(fill.len(), 10, "ten fill columns");
        let xs: Vec<i32> = fill.iter().map(|d| d.dst.0).collect();
        assert_eq!(xs, vec![8, 40, 72, 104, 136, 168, 200, 232, 264, 296]);
        assert!(fill.iter().all(|d| d.dst.1 == 4 && d.dst.3 == 28));
        assert_eq!(fill.last().unwrap().dst.2, 8, "last column clipped");
        assert!(
            fill.iter().all(|d| d.src == (128, 0, d.dst.2, 28)),
            "fill samples the gradient-baked marbled tile 1:1"
        );
        assert!(
            draws[fill.len()..].iter().all(|d| d.src.0 >= 160),
            "the border follows the fill"
        );
        assert!(draws.iter().all(|d| d.src.0 != 240), "not the dialog fill");
    }

    /// The intro label's fill: three columns from `(78, 40)`, the last 18
    /// wide - the run in the `karisto_sol_pre_encounter` intro frame.
    #[test]
    fn the_intro_label_fill_is_the_captured_one() {
        let rects = panel_rects();
        let draws = class0_frame_draws_at(&rects, (86, 48), (66, BANNER_INTERIOR_H), (0, 0), 1);
        let fill: Vec<_> = draws.iter().take_while(|d| d.src.0 == 128).collect();
        let cols: Vec<(i32, u32)> = fill.iter().map(|d| (d.dst.0, d.dst.2)).collect();
        assert_eq!(cols, vec![(78, 32), (110, 32), (142, 18)]);
    }

    /// A frame taller than one band (`FUN_8002BDC4`'s 32 rows) ramps
    /// `0x40 -> 0x40 + 0x900 / h` over the first band and on to `0x88`
    /// over the last - the two-band run of a 58-tall frame retail queued
    /// as `0x40 -> 0x67`, `0x67 -> 0x88`.
    #[test]
    fn a_tall_fill_bands_its_gradient() {
        let rects = panel_rects();
        let draws = class0_fill_draws_at(&rects, (8, 152, 304, 58), (0, 0), 1);
        let grey_at = |y: i32| {
            draws
                .iter()
                .find(|d| d.dst.1 == y && d.dst.0 == 8)
                .map(|d| (d.color[0] * 128.0).round() as i32)
                .unwrap()
        };
        assert_eq!(grey_at(152), 0x40);
        assert_eq!(grey_at(184), 0x67, "band 2 starts on band 1's bottom");
        assert!(grey_at(209) < 0x88 && grey_at(209) > 0x84);
        assert!(draws.iter().all(|d| d.dst.3 == 1 && d.src.1 >= 200));
    }

    /// Nothing the banner draws leaves its frame.
    #[test]
    fn the_banner_stays_inside_its_frame() {
        let rects = panel_rects();
        let draws = message_banner_chrome_draws_for(&rects, (60, BANNER_INTERIOR_H), (0, 0), 1);
        // Four corners, then a tiled top/bottom pair and a left/right pair.
        assert_eq!(
            draws
                .iter()
                .filter(|d| d.src.2 == 4 && d.src.3 == 4)
                .count(),
            4,
            "exactly four corner tiles"
        );
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

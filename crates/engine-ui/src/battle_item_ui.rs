//! Battle **item window** - retail's state-`0x3C` surface, shared by both
//! play hosts.
//!
//! Every window rect below is packet-pinned out of a PCSX-Redux battle
//! save state's libgpu ordering table (the `battle_item_window` /
//! `battle_item_window_cursor1` captures walked into the item window from
//! `cort_evolved_battle_first_menu` by
//! `scripts/pcsx-redux/autorun_battle_item_window_capture.lua`): a RAM
//! image *is* the frame's display list, so the windows are read out of the
//! queued packet words. See `docs/subsystems/battle.md` § battle item
//! window.
//!
//! ## What the pins say
//!
//! * The **item-list window** is a tile grid of the system-UI window skin
//!   (widget page `(896, 256)`, CLUT row 511 sub-palette 2) spanning
//!   x `166..=313`, y `28..=164` - the stage rect [`LIST_WINDOW`].
//! * The **description window** is the same skin at x `8..=167`,
//!   y `122..=164` ([`DESC_WINDOW`]); it shows the highlighted item's
//!   info-window line ("Recover 200HP. Ally.").
//! * The **hand cursor** is the 16x16 pointing-finger sprite (CLUT row
//!   511 sub-palette 7 - the save-select / dialog hand) at
//!   `(167, 45 + 14*row)`: seat [`HAND_SEAT`], and the two captures one
//!   Down press apart pin the **row pitch** at 14 ([`ROW_PITCH`]).
//! * Eight rows fit a page ([`ROWS_PER_PAGE`]); the header reads
//!   `PAGE <n>/<pages>` with the value right-aligned to the count column.
//! * Counts are right-aligned at the interior's right edge
//!   ([`COUNT_RIGHT_X`]).
//! * A **breadcrumb trail** of gold tab plates (`Begin`, the acting
//!   member's name, `Item`) sits top-left - the same tab-banner 3-slice
//!   the pause menu's title tab uses.
//!
//! Content pens (row text, PAGE header, description line, breadcrumb
//! seats) are screenshot-read off the same captures rather than
//! packet-decoded - the glyph packets ride a different draw pass - and are
//! marked so below.
//!
//! The port draws the window with the shared 9-slice menu-window chrome
//! ([`crate::menu_window_chrome_draws_for`]) rather than re-tiling the
//! 32x32 grid: both compose the same system-UI window skin, and the
//! 9-slice is what every other faithful menu panel already goes through.
//!
//! During target select the same two windows stay up; the list column
//! swaps to the target roster (retail's state `0x64` runs its own party
//! panel there - not yet packet-pinned, so the swap is the port's stand-in
//! and is disclosed as such).

use crate::{SaveMenuAtlasRects, SpriteDraw, TextDraw, text_draws_for};

// ------------------------------------------------------------- pinned rects

/// Item-list window stage rect `(x, y, w, h)` - packet-pinned.
pub const LIST_WINDOW: (i32, i32, i32, i32) = (166, 28, 147, 136);
/// Description window stage rect - packet-pinned.
pub const DESC_WINDOW: (i32, i32, i32, i32) = (8, 122, 159, 42);
/// Hand-cursor seat for row 0 - packet-pinned (16x16 sprite top-left).
pub const HAND_SEAT: (i32, i32) = (167, 45);
/// List row pitch - packet-pinned (the hand moves 14 per Down press).
pub const ROW_PITCH: i32 = 14;
/// Rows one page shows (screenshot-read: eight rows fill the window).
pub const ROWS_PER_PAGE: usize = 8;
/// Row text pen for row 0 (screenshot-read).
pub const ROW_TEXT_SEAT: (i32, i32) = (186, 47);
/// Right edge the per-row count is right-aligned to (screenshot-read).
pub const COUNT_RIGHT_X: i32 = 307;
/// `PAGE` header label pen (screenshot-read); the `n/m` value is
/// right-aligned to [`COUNT_RIGHT_X`] on the same line.
pub const PAGE_SEAT: (i32, i32) = (248, 33);
/// Description line pen (screenshot-read).
pub const DESC_SEAT: (i32, i32) = (16, 132);
/// First breadcrumb tab's glyph pen (screenshot-read: the `Begin` plate
/// starts at x 10, and a tab plate insets its pen by the 8-px cap).
pub const CRUMB_SEAT0: (i32, i32) = (18, 14);
/// Gap between one breadcrumb plate's right cap and the next plate's left
/// cap (screenshot-read).
pub const CRUMB_GAP: i32 = 6;
/// Minimum interior width of a breadcrumb tab (screenshot-read off the
/// retail `Vahn` tab; a longer label widens its own tab - the engine font
/// is wider than retail's tab glyphs, so tabs are sized per label rather
/// than pinned to the retail plate widths).
pub const CRUMB_MIN_INTERIOR_W: i32 = 24;

/// Per-tab breadcrumb layout for the `Begin | <actor> | Item` trail:
/// `(label pen, interior width)` per tab, advancing left-to-right from
/// [`CRUMB_SEAT0`] with each tab sized to its own label.
pub fn crumb_layout(font: &legaia_font::Font, actor_name: &str) -> [((i32, i32), i32); 3] {
    let mut x = CRUMB_SEAT0.0;
    let mut out = [((0, 0), 0); 3];
    for (i, label) in ["Begin", actor_name, "Item"].into_iter().enumerate() {
        let interior = (font.layout_ascii(label).advance_x as i32).max(CRUMB_MIN_INTERIOR_W);
        out[i] = ((x, CRUMB_SEAT0.1), interior);
        // Advance past this tab's interior + right cap, the gap, and the
        // next tab's left cap.
        x += interior + 8 + CRUMB_GAP + 8;
    }
    out
}

// ----------------------------------------------------------------- palette

/// Row ink for the highlighted row.
pub const ROW_SELECTED: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// Row ink for an unselected admissible row.
pub const ROW_IDLE: [f32; 4] = [0.82, 0.85, 0.92, 1.0];
/// Row ink for a row the context filter refuses (kept visible, dimmed).
pub const ROW_DIMMED: [f32; 4] = [0.5, 0.5, 0.55, 1.0];
/// The `PAGE` label's green (retail draws it in the teal header ink).
pub const PAGE_LABEL_INK: [f32; 4] = [0.45, 0.95, 0.75, 1.0];
/// Ink of a dead target's row in the target column.
pub const ROW_DEAD: [f32; 4] = [1.0, 0.55, 0.55, 1.0];

// ------------------------------------------------------------------- views

/// One list row of the frame.
#[derive(Debug, Clone, Copy)]
pub struct BattleItemRowView<'a> {
    pub name: &'a str,
    pub count: u8,
    /// `false` draws the row dimmed (context filter refuses it).
    pub admissible: bool,
}

/// One target row of the target-select column.
#[derive(Debug, Clone, Copy)]
pub struct BattleItemTargetView<'a> {
    pub name: &'a str,
    pub hp: u16,
    pub hp_max: u16,
    pub alive: bool,
}

/// Everything one frame of the battle item window draws from.
#[derive(Debug, Clone, Copy, Default)]
pub struct BattleItemMenuFrame<'a> {
    pub rows: &'a [BattleItemRowView<'a>],
    /// Row index (into `rows`) under the item cursor, if any.
    pub cursor: Option<usize>,
    /// The highlighted item's info-window description line, if resolved.
    pub description: Option<&'a str>,
    /// Acting party member's display name (the middle breadcrumb).
    pub actor_name: &'a str,
    /// Target-select: when `Some`, the list column swaps to these rows
    /// with the hand on `.1`.
    pub targets: Option<(&'a [BattleItemTargetView<'a>], usize)>,
}

impl BattleItemMenuFrame<'_> {
    /// The page the cursor sits on (0-based).
    pub fn page(&self) -> usize {
        self.cursor.unwrap_or(0) / ROWS_PER_PAGE
    }

    /// Total pages the row list spans (minimum 1, like retail's header).
    pub fn pages(&self) -> usize {
        self.rows.len().div_ceil(ROWS_PER_PAGE).max(1)
    }
}

// ---------------------------------------------------------------- builders

fn scaled(
    dst: (i32, i32, u32, u32),
    src: (u32, u32, u32, u32),
    color: [f32; 4],
    stage_origin: (i32, i32),
    scale: i32,
) -> SpriteDraw {
    SpriteDraw {
        dst: (
            stage_origin.0 + dst.0 * scale,
            stage_origin.1 + dst.1 * scale,
            dst.2 * scale as u32,
            dst.3 * scale as u32,
        ),
        src,
        color,
    }
}

/// The window's sprite half: both 9-slice windows, the three breadcrumb
/// tab plates (sized per label via [`crumb_layout`]) and the hand cursor.
pub fn battle_item_window_sprites(
    font: &legaia_font::Font,
    rects: &SaveMenuAtlasRects,
    frame: &BattleItemMenuFrame<'_>,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let scale = stage_scale.max(1) as i32;
    let mut out = Vec::with_capacity(64);
    out.extend(crate::menu_window_chrome_draws_for(
        rects,
        LIST_WINDOW,
        stage_origin,
        stage_scale,
    ));
    out.extend(crate::menu_window_chrome_draws_for(
        rects,
        DESC_WINDOW,
        stage_origin,
        stage_scale,
    ));
    // Breadcrumb trail: three gold tab plates - the same 3-slice the pause
    // menu's title tab banner uses.
    for (pen, interior) in crumb_layout(font, frame.actor_name) {
        out.extend(crate::tab_banner_draws(
            rects,
            pen,
            interior,
            stage_origin,
            stage_scale,
        ));
    }
    // Hand cursor on the highlighted row of whichever column is live.
    let hand_row = match frame.targets {
        Some((_, t)) => Some(t),
        None => frame.cursor.map(|c| c % ROWS_PER_PAGE),
    };
    if let Some(row) = hand_row {
        let (_, _, w, h) = rects.cursor;
        out.push(scaled(
            (HAND_SEAT.0, HAND_SEAT.1 + row as i32 * ROW_PITCH, w, h),
            rects.cursor,
            [1.0, 1.0, 1.0, 1.0],
            stage_origin,
            scale,
        ));
    }
    out
}

fn emit(
    out: &mut Vec<TextDraw>,
    font: &legaia_font::Font,
    text: &str,
    pen: (i32, i32),
    ink: [f32; 4],
    stage_origin: (i32, i32),
    scale: i32,
) {
    let mut draws = text_draws_for(&font.layout_ascii(text), (0, 0), ink);
    for d in &mut draws {
        d.dst.0 = stage_origin.0 + (pen.0 + d.dst.0) * scale;
        d.dst.1 = stage_origin.1 + (pen.1 + d.dst.1) * scale;
        d.dst.2 *= scale as u32;
        d.dst.3 *= scale as u32;
    }
    out.extend(draws);
}

/// The window's text half: breadcrumb labels, the `PAGE n/m` header, the
/// visible rows (name + right-aligned count, or the target roster during
/// target select) and the description line.
pub fn battle_item_window_text(
    font: &legaia_font::Font,
    frame: &BattleItemMenuFrame<'_>,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<TextDraw> {
    let scale = stage_scale.max(1) as i32;
    let mut out = Vec::new();
    let white = [1.0, 1.0, 1.0, 1.0];

    for (label, (pen, _)) in ["Begin", frame.actor_name, "Item"]
        .into_iter()
        .zip(crumb_layout(font, frame.actor_name))
    {
        emit(&mut out, font, label, pen, white, stage_origin, scale);
    }

    match frame.targets {
        None => {
            // PAGE header.
            emit(
                &mut out,
                font,
                "PAGE",
                PAGE_SEAT,
                PAGE_LABEL_INK,
                stage_origin,
                scale,
            );
            let pages = format!("{}/{}", frame.page() + 1, frame.pages());
            let pw = font.layout_ascii(&pages).advance_x as i32;
            emit(
                &mut out,
                font,
                &pages,
                (COUNT_RIGHT_X - pw, PAGE_SEAT.1),
                white,
                stage_origin,
                scale,
            );
            // The cursor's page of rows.
            let base = frame.page() * ROWS_PER_PAGE;
            for (i, row) in frame.rows.iter().skip(base).take(ROWS_PER_PAGE).enumerate() {
                let y = ROW_TEXT_SEAT.1 + i as i32 * ROW_PITCH;
                let ink = if !row.admissible {
                    ROW_DIMMED
                } else if frame.cursor == Some(base + i) {
                    ROW_SELECTED
                } else {
                    ROW_IDLE
                };
                emit(
                    &mut out,
                    font,
                    row.name,
                    (ROW_TEXT_SEAT.0, y),
                    ink,
                    stage_origin,
                    scale,
                );
                let count = format!("{}", row.count);
                let cw = font.layout_ascii(&count).advance_x as i32;
                emit(
                    &mut out,
                    font,
                    &count,
                    (COUNT_RIGHT_X - cw, y),
                    ink,
                    stage_origin,
                    scale,
                );
            }
        }
        Some((targets, cursor)) => {
            // Target column in the list window - the port's stand-in for
            // retail's state-0x64 target panel (not yet packet-pinned).
            for (i, t) in targets.iter().take(ROWS_PER_PAGE).enumerate() {
                let y = ROW_TEXT_SEAT.1 + i as i32 * ROW_PITCH;
                let ink = if !t.alive {
                    ROW_DEAD
                } else if i == cursor {
                    ROW_SELECTED
                } else {
                    ROW_IDLE
                };
                emit(
                    &mut out,
                    font,
                    t.name,
                    (ROW_TEXT_SEAT.0, y),
                    ink,
                    stage_origin,
                    scale,
                );
                let hp = format!("{}/{}", t.hp, t.hp_max);
                let hw = font.layout_ascii(&hp).advance_x as i32;
                emit(
                    &mut out,
                    font,
                    &hp,
                    (COUNT_RIGHT_X - hw, y),
                    ink,
                    stage_origin,
                    scale,
                );
            }
        }
    }

    if let Some(desc) = frame.description {
        emit(&mut out, font, desc, DESC_SEAT, white, stage_origin, scale);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal atlas fixture: the 9-slice tiles at their real sizes (the
    /// pinned system-UI skin geometry) so the window composer runs its
    /// genuine tiling arithmetic, plus the hand-cursor cell.
    fn rects() -> SaveMenuAtlasRects {
        SaveMenuAtlasRects {
            panel_tl: (0, 0, 4, 4),
            panel_tr: (28, 0, 4, 4),
            panel_bl: (0, 25, 4, 4),
            panel_br: (28, 25, 4, 4),
            panel_top: (4, 0, 24, 4),
            panel_bot: (4, 25, 24, 4),
            panel_left: (0, 4, 4, 21),
            panel_right: (28, 4, 4, 21),
            panel_interior: (32, 0, 32, 29),
            panel_filigree: (64, 0, 32, 29),
            cursor: (152, 64, 16, 16),
            tab_cap_l: (96, 0, 8, 20),
            tab_body: (104, 0, 16, 20),
            tab_cap_r: (120, 0, 8, 20),
            ..Default::default()
        }
    }

    fn rows(n: usize) -> Vec<BattleItemRowView<'static>> {
        const NAMES: [&str; 4] = ["Healing Leaf", "Healing Flower", "Antidote", "Door of Wind"];
        (0..n)
            .map(|i| BattleItemRowView {
                name: NAMES[i % NAMES.len()],
                count: (i + 1) as u8,
                admissible: true,
            })
            .collect()
    }

    fn frame<'a>(rows: &'a [BattleItemRowView<'a>], cursor: usize) -> BattleItemMenuFrame<'a> {
        BattleItemMenuFrame {
            rows,
            cursor: Some(cursor),
            description: Some("Recover 200HP. Ally."),
            actor_name: "Vahn",
            targets: None,
        }
    }

    /// The frame prims exist: both packet-pinned windows are composed as
    /// full 9-slice panels (fill + 4 corners + edges), at the pinned rects.
    #[test]
    fn both_windows_draw_their_chrome_at_the_pinned_rects() {
        let r = rects();
        let all = rows(3);
        let sprites = battle_item_window_sprites(
            &legaia_font::Font::placeholder(),
            &r,
            &frame(&all, 0),
            (0, 0),
            1,
        );
        // A 9-slice panel is at least 9 sprites; two windows + 3 crumb
        // plates (3 sprites each) + the hand.
        let list_only = crate::menu_window_chrome_draws_for(&r, LIST_WINDOW, (0, 0), 1);
        let desc_only = crate::menu_window_chrome_draws_for(&r, DESC_WINDOW, (0, 0), 1);
        assert!(list_only.len() >= 9, "list window is a real 9-slice");
        assert!(desc_only.len() >= 9, "desc window is a real 9-slice");
        // The composed frame contains both panels verbatim, in order.
        let key = |s: &SpriteDraw| (s.dst, s.src);
        assert_eq!(
            sprites[..list_only.len()]
                .iter()
                .map(key)
                .collect::<Vec<_>>(),
            list_only.iter().map(key).collect::<Vec<_>>()
        );
        assert_eq!(
            sprites[list_only.len()..list_only.len() + desc_only.len()]
                .iter()
                .map(key)
                .collect::<Vec<_>>(),
            desc_only.iter().map(key).collect::<Vec<_>>()
        );
        // The pinned rects themselves - x 166..=313 / y 28..=164 and
        // x 8..=167 / y 122..=164, the packet grid's spans.
        assert_eq!(LIST_WINDOW, (166, 28, 147, 136));
        assert_eq!(DESC_WINDOW, (8, 122, 159, 42));
        // Every sprite of the frame stays on the 320x240 stage.
        for s in &sprites {
            assert!(s.dst.0 >= 0 && s.dst.0 + s.dst.2 as i32 <= 320, "{s:?}");
            assert!(s.dst.1 >= 0 && s.dst.1 + s.dst.3 as i32 <= 240, "{s:?}");
        }
    }

    /// The hand cursor rides the packet-pinned seat + pitch: row 0 at
    /// (167, 45), one Down press = 14 further, exactly the two captures.
    #[test]
    fn the_hand_cursor_sits_on_the_pinned_seat_and_pitch() {
        let r = rects();
        let all = rows(3);
        let hand_at = |cursor: usize| {
            let sprites = battle_item_window_sprites(
                &legaia_font::Font::placeholder(),
                &r,
                &frame(&all, cursor),
                (0, 0),
                1,
            );
            let hand = sprites.last().unwrap();
            assert_eq!(hand.src, r.cursor);
            (hand.dst.0, hand.dst.1)
        };
        assert_eq!(hand_at(0), (167, 45));
        assert_eq!(hand_at(1), (167, 59));
        assert_eq!(hand_at(2), (167, 73));
    }

    /// Rows page by eight and the header names the cursor's page.
    #[test]
    fn rows_page_by_eight_and_the_header_counts_pages() {
        let all = rows(11);
        let f0 = frame(&all, 0);
        assert_eq!((f0.page(), f0.pages()), (0, 2));
        let f9 = frame(&all, 9);
        assert_eq!((f9.page(), f9.pages()), (1, 2));
        // Page 2 draws the remainder (3 rows), and the hand wraps onto the
        // page-relative seat.
        let r = rects();
        let sprites =
            battle_item_window_sprites(&legaia_font::Font::placeholder(), &r, &f9, (0, 0), 1);
        let hand = sprites.last().unwrap();
        assert_eq!(hand.dst.1, 45 + ROW_PITCH); // row 9 = page row 1
        // An empty bag still has one page, like retail's PAGE 1/1 header.
        let empty = BattleItemMenuFrame {
            rows: &[],
            cursor: None,
            description: None,
            actor_name: "Vahn",
            targets: None,
        };
        assert_eq!(empty.pages(), 1);
    }

    /// Text half: rows land on the pinned pens, counts right-align to the
    /// count column, and the description line lands in the desc window.
    #[test]
    fn text_rows_land_on_the_pinned_pens() {
        let font = legaia_font::Font::placeholder();
        let all = rows(2);
        let draws = battle_item_window_text(&font, &frame(&all, 0), (0, 0), 1);
        assert!(!draws.is_empty());
        // Something draws on row 0's baseline and row 1's baseline.
        for row in 0..2i32 {
            let y = ROW_TEXT_SEAT.1 + row * ROW_PITCH;
            assert!(
                draws.iter().any(|d| d.dst.1 == y && d.dst.0 >= 0),
                "row {row} has no glyphs at y={y}"
            );
        }
        // Counts end at the pinned right edge.
        // Row 0's count is "1"; its pen is right-aligned so the advance
        // ends exactly at the pinned column edge.
        let count_pen = COUNT_RIGHT_X - font.layout_ascii("1").advance_x as i32;
        assert!(
            draws
                .iter()
                .any(|d| d.dst.0 == count_pen && d.dst.1 == ROW_TEXT_SEAT.1),
            "count column is right-aligned to x={COUNT_RIGHT_X}"
        );
        // Description line at its pen.
        assert!(draws.iter().any(|d| d.dst.1 == DESC_SEAT.1));
        // The three breadcrumb labels are drawn.
        assert!(draws.iter().any(|d| d.dst.1 == CRUMB_SEAT0.1));
    }

    /// Target select swaps the list column to the roster and moves the
    /// hand to the target cursor; the windows stay up.
    #[test]
    fn target_select_swaps_the_column_and_keeps_the_windows() {
        let r = rects();
        let font = legaia_font::Font::placeholder();
        let all = rows(2);
        let targets = [
            BattleItemTargetView {
                name: "Vahn",
                hp: 100,
                hp_max: 120,
                alive: true,
            },
            BattleItemTargetView {
                name: "Noa",
                hp: 0,
                hp_max: 90,
                alive: false,
            },
        ];
        let mut f = frame(&all, 1);
        f.targets = Some((&targets, 1));
        let sprites =
            battle_item_window_sprites(&legaia_font::Font::placeholder(), &r, &f, (0, 0), 1);
        // Windows still framed; hand on target row 1, not item row 1's page seat.
        let hand = sprites.last().unwrap();
        assert_eq!((hand.dst.0, hand.dst.1), (167, 59));
        let draws = battle_item_window_text(&font, &f, (0, 0), 1);
        // Roster names drawn in place of item rows; no PAGE header.
        let n = |s: &str| font.layout_ascii(s).glyphs.len();
        assert!(n("Vahn") > 0);
        let has_y = |y: i32| draws.iter().any(|d| d.dst.1 == y);
        assert!(has_y(ROW_TEXT_SEAT.1));
        assert!(has_y(ROW_TEXT_SEAT.1 + ROW_PITCH));
        assert!(
            !draws
                .iter()
                .any(|d| d.dst.1 == PAGE_SEAT.1 && d.dst.0 >= PAGE_SEAT.0),
            "no PAGE header during target select"
        );
    }

    /// The stage transform multiplies every prim like the sibling chrome
    /// builders.
    #[test]
    fn stage_scale_multiplies_every_prim() {
        let r = rects();
        let all = rows(2);
        let one = battle_item_window_sprites(
            &legaia_font::Font::placeholder(),
            &r,
            &frame(&all, 0),
            (0, 0),
            1,
        );
        let two = battle_item_window_sprites(
            &legaia_font::Font::placeholder(),
            &r,
            &frame(&all, 0),
            (7, 11),
            2,
        );
        assert_eq!(one.len(), two.len());
        for (a, b) in one.iter().zip(two.iter()) {
            assert_eq!(b.dst.0, 7 + a.dst.0 * 2);
            assert_eq!(b.dst.1, 11 + a.dst.1 * 2);
            assert_eq!(b.dst.2, a.dst.2 * 2);
            assert_eq!(b.dst.3, a.dst.3 * 2);
        }
    }
}

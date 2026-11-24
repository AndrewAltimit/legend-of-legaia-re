//! The run rules' screens: the New Game rules screen ([`RulesMenuView`]) and
//! the Boss Rush intermission and result ([`RulesPanelView`]).
//!
//! The New Game rules screen is drawn in the game's own menu style: the
//! pause menu's 9-slice windows over black, the carved title-tab plaques for
//! its four tabs, the pointing-hand cursor and the menu inks, so it reads as
//! the Options screen does. Its sprites sample the system-UI chrome atlas
//! ([`SaveMenuAtlasRects`]) and its labels the menu font, which both hosts
//! already draw for the pause menu. The Boss Rush screens stay one titled
//! list panel in the expansion screens' flat palette.
//!
//! Each view is plain rows (label, value) and a cursor, so this layer names
//! no run-rule type; `legaia_engine_screens::run_rules` projects the
//! sessions into it once for both hosts.

use legaia_render_kernels::screen_prim::ScreenPrim;

use crate::expansion_modes::Label;
use crate::expansion_modes::{
    Align, Canvas, LINE, STAGE_H, STAGE_W, ink, labels_to_draws, pen as text,
};
use crate::{SaveMenuAtlasRects, SpriteDraw, TextDraw};

/// One list panel.
#[derive(Debug, Clone, Copy)]
pub struct RulesPanelView<'a> {
    /// The heading.
    pub title: &'a str,
    /// A line under the heading (the run summary, the clock).
    pub subtitle: Option<&'a str>,
    /// `(label, value)` rows.
    pub rows: &'a [(String, String)],
    /// The selected row, `None` for a panel with no cursor.
    pub cursor: Option<usize>,
    /// The foot line (the selected row's help, an action's result).
    pub footer: Option<&'a str>,
    /// The key hint.
    pub hint: &'a str,
}

/// The most rows the panel shows at once; a longer list scrolls with the
/// cursor. Fewer show when a subtitle takes a line ([`visible_rows`]).
pub const VISIBLE_ROWS: usize = 11;

/// Where the foot line sits (its pen's top), stage pixels.
const FOOTER_Y: i32 = STAGE_H - 34;
/// Where the key hint sits.
const HINT_Y: i32 = STAGE_H - 16;
/// The panel's left edge; the panel is centred, so its right edge mirrors it.
const PANEL_X: i32 = 16;
/// A row's text inset from the panel edge.
const ROW_INSET: i32 = 10;
/// The share of the row a label may take; the value takes the rest, less a
/// gap, so a long value (a New Game+ source) never runs into its label.
const LABEL_W: i32 = 132;
/// The gap kept between a label's box and its value's.
const VALUE_GAP: i32 = 8;
/// A text box the width of the stage, less a margin each side.
const STAGE_TEXT_W: i32 = STAGE_W - 16;

/// The rows a panel shows when its list starts at `top`: as many as fit
/// above the foot line with a gap, at most [`VISIBLE_ROWS`].
fn visible_rows(top: i32) -> usize {
    (((FOOTER_Y - 4 - top - 8) / LINE).max(1) as usize).min(VISIBLE_ROWS)
}

/// A small solid triangle pointing up (`up`) or down, centred on `x`, its
/// flat side at `y`: the scroll marks beside a list with hidden rows. Built
/// from one-pixel rows, axis-aligned rects, so the page's 2D canvas (which
/// fills each flat prim as its bounding box) draws the same shape.
fn scroll_mark(c: &mut Canvas, x: i32, y: i32, up: bool) {
    for k in 0..4 {
        let half = 3 - k;
        let ry = if up { y - 1 - k } else { y + k };
        c.rect(x - half, ry, 2 * half + 1, 1, ink::HIGHLIGHT);
    }
}

fn compose(v: &RulesPanelView<'_>) -> Canvas {
    let mut c = Canvas::default();
    c.rect(0, 0, STAGE_W, STAGE_H, ink::BACKDROP);
    c.rect(0, 0, STAGE_W, 22, ink::BAR);
    c.text_in(
        v.title,
        STAGE_W / 2,
        6,
        STAGE_TEXT_W,
        text::GOLD,
        Align::Centre,
    );
    let mut y = 28;
    if let Some(s) = v.subtitle {
        c.text_in(s, STAGE_W / 2, y, STAGE_TEXT_W, text::DIM, Align::Centre);
        y += LINE;
    }
    let top = y + 2;
    let n = v.rows.len();
    let fit = visible_rows(top);
    let first = v.cursor.map_or(0, |cur| {
        cur.saturating_sub(fit - 1).min(n.saturating_sub(fit))
    });
    let shown = n.min(fit);
    let panel_w = STAGE_W - 2 * PANEL_X;
    let panel_h = shown as i32 * LINE + 8;
    c.panel(PANEL_X, top, panel_w, panel_h, ink::PANEL, ink::PANEL_EDGE);
    let label_x = PANEL_X + ROW_INSET;
    let value_x = PANEL_X + panel_w - ROW_INSET;
    let value_w = value_x - (label_x + LABEL_W + VALUE_GAP);
    for (k, (label, value)) in v.rows.iter().enumerate().skip(first).take(shown) {
        let ry = top + 4 + (k - first) as i32 * LINE;
        let selected = v.cursor == Some(k);
        if selected {
            // The band spans the glyph cell's inked rows (caps from row 2,
            // descenders and their shadow to row 13), so no descender
            // hangs off it.
            c.rect(PANEL_X + 2, ry, panel_w - 4, LINE + 1, ink::SELECT);
        }
        let colour = if selected { text::GOLD } else { text::WHITE };
        let label_w = if value.is_empty() {
            value_x - label_x
        } else {
            LABEL_W
        };
        c.text_in(label.clone(), label_x, ry, label_w, colour, Align::Left);
        if !value.is_empty() {
            c.text_in(value.clone(), value_x, ry, value_w, colour, Align::Right);
        }
    }
    // Rows hidden above or below the window: a mark in the panel's corner,
    // right of the values' column.
    let mark_x = PANEL_X + panel_w - 5;
    if first > 0 {
        scroll_mark(&mut c, mark_x, top + 7, true);
    }
    if first + shown < n {
        scroll_mark(&mut c, mark_x, top + panel_h - 7, false);
    }
    if let Some(f) = v.footer {
        c.text_in(
            f,
            STAGE_W / 2,
            FOOTER_Y,
            STAGE_TEXT_W,
            text::WHITE,
            Align::Centre,
        );
    }
    c.text_in(
        v.hint,
        STAGE_W / 2,
        HINT_Y,
        STAGE_TEXT_W,
        text::DIM,
        Align::Centre,
    );
    c
}

/// The panel's rects, in stage pixels.
pub fn rules_panel_prims(v: &RulesPanelView<'_>) -> Vec<ScreenPrim> {
    compose(v).prims
}

/// The panel's labels as text draws, in stage pixels.
pub fn rules_panel_texts(font: &legaia_font::Font, v: &RulesPanelView<'_>) -> Vec<TextDraw> {
    labels_to_draws(font, &compose(v).labels)
}

/// The panel's labels as strings, in draw order (tests, a text host).
pub fn rules_panel_strings(v: &RulesPanelView<'_>) -> Vec<String> {
    compose(v).labels.into_iter().map(|l| l.text).collect()
}

// ---------------------------------------------------------------------
// The New Game rules screen, in the menu style.
// ---------------------------------------------------------------------

/// One row of the rules menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulesMenuRow {
    pub label: String,
    /// The value column; empty for an action row (*Start*).
    pub value: String,
    /// Drawn in the menu's grey when changing it does nothing.
    pub enabled: bool,
}

/// The seed editor's letter grid, drawn as a window over the rows.
#[derive(Debug, Clone, Copy)]
pub struct SeedGridView<'a> {
    /// The seed as typed so far.
    pub text: &'a str,
    /// The cells' labels, row by row.
    pub cells: &'a [Vec<String>],
    /// The selected cell, `(row, column)`.
    pub cell: (usize, usize),
}

/// The New Game rules screen.
#[derive(Debug, Clone, Copy)]
pub struct RulesMenuView<'a> {
    /// The tab headings, left to right.
    pub tabs: &'a [&'a str],
    /// The tab on screen.
    pub tab: usize,
    pub rows: &'a [RulesMenuRow],
    pub cursor: usize,
    /// `(label, value)` facts listed under the rows in two columns (a
    /// difficulty preset's scales); not selectable.
    pub info: &'a [(String, String)],
    /// The help window's text, wrapped to its two lines.
    pub help: &'a str,
    /// The run's one-line summary under the help window.
    pub summary: &'a str,
    /// The key hint at the foot.
    pub hint: &'a str,
    pub seed_grid: Option<SeedGridView<'a>>,
}

/// The tab plaques' content width and pitch: four plaques (8-px caps each
/// side) across the stage with a 2-px gap.
const TAB_CONTENT_W: i32 = 58;
const TAB_PITCH: i32 = TAB_CONTENT_W + 16 + 2;
/// The tab plaques' content origin row.
const TAB_PEN_Y: i32 = 8;
/// The rows' window, `(x, y, w, h)` with its border.
pub const MENU_MAIN_FRAME: (i32, i32, i32, i32) = (8, 28, 304, 138);
/// The help window.
pub const MENU_HELP_FRAME: (i32, i32, i32, i32) = (8, 168, 304, 38);
/// The seed editor's window, over the rows.
pub const MENU_GRID_FRAME: (i32, i32, i32, i32) = (36, 40, 248, 124);
/// A window's content inset from its frame, across and down (the glyph
/// cell's two blank top rows make up the difference).
const INSET: i32 = 8;
const INSET_Y: i32 = 6;
/// The menu's row pitch (the Options screen's).
const MENU_LINE: i32 = 14;
/// Where a row's label starts; the hand sits 18 px left of it, as on the
/// Options screen.
const MENU_LABEL_X: i32 = MENU_MAIN_FRAME.0 + INSET + 18;
/// Where a row's value starts (the Options screen's `+140`).
const MENU_VALUE_X: i32 = MENU_LABEL_X + 140;
/// The rows the main window holds.
pub const MENU_VISIBLE_ROWS: usize = ((MENU_MAIN_FRAME.3 - 2 * INSET_Y) / MENU_LINE) as usize;
const SUMMARY_Y: i32 = 209;
const MENU_HINT_Y: i32 = 224;
/// A help line's pitch.
const HELP_LINE: i32 = 13;
/// Grid cell pitch.
const GRID_PITCH: i32 = 22;
/// The grid's first cell, its last (action) row's pitch.
const GRID_X: i32 = MENU_GRID_FRAME.0 + 24;
const GRID_Y: i32 = MENU_GRID_FRAME.1 + 26;
const GRID_ACTION_PITCH: i32 = 66;

/// What the menu composition lays out, in stage pixels.
#[derive(Debug, Default)]
struct MenuCanvas {
    /// 9-slice windows, back to front.
    windows: Vec<(i32, i32, i32, i32)>,
    /// Tab plaques: content origin, content width, whether it is the tab
    /// on screen.
    tabs: Vec<((i32, i32), i32, bool)>,
    /// Hand cursors' top-left corners.
    hands: Vec<(i32, i32)>,
    labels: Vec<Label>,
}

impl MenuCanvas {
    fn label(
        &mut self,
        text: impl Into<String>,
        x: i32,
        y: i32,
        w: i32,
        color: [f32; 4],
        align: Align,
    ) {
        self.labels.push(Label {
            text: text.into(),
            x,
            y,
            color,
            align,
            max_w: Some(w.max(0)),
        });
    }
}

/// `text` broken at spaces into lines no wider than `max_w` (measured with
/// `font`), at most `lines` of them; the last keeps what is left, and the
/// label box cuts it.
fn wrap(font: &legaia_font::Font, text: &str, max_w: i32, lines: usize) -> Vec<String> {
    let width = |s: &str| font.layout_ascii(s).advance_x as i32;
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let next = if cur.is_empty() {
            word.to_string()
        } else {
            format!("{cur} {word}")
        };
        if width(&next) <= max_w || cur.is_empty() || out.len() + 1 == lines {
            cur = next;
        } else {
            out.push(std::mem::take(&mut cur));
            cur = word.to_string();
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn compose_menu(v: &RulesMenuView<'_>, font: Option<&legaia_font::Font>) -> MenuCanvas {
    let white = crate::MENU_TEXT_WHITE;
    let gold = crate::MENU_TEXT_GOLD;
    let grey = crate::menu_ink_color(0);
    let mut c = MenuCanvas::default();

    for (i, name) in v.tabs.iter().enumerate() {
        let pen = (8 + i as i32 * TAB_PITCH + 8, TAB_PEN_Y);
        let active = i == v.tab;
        c.tabs.push((pen, TAB_CONTENT_W, active));
        c.label(
            *name,
            pen.0 + TAB_CONTENT_W / 2,
            pen.1,
            TAB_CONTENT_W + 12,
            if active { gold } else { grey },
            Align::Centre,
        );
    }

    c.windows.push(MENU_MAIN_FRAME);
    let top = MENU_MAIN_FRAME.1 + INSET_Y;
    let n = v.rows.len();
    let fit = MENU_VISIBLE_ROWS;
    let first = v.cursor.saturating_sub(fit - 1).min(n.saturating_sub(fit));
    let shown = n.min(fit);
    let value_w = MENU_MAIN_FRAME.0 + MENU_MAIN_FRAME.2 - INSET - MENU_VALUE_X;
    for (k, row) in v.rows.iter().enumerate().skip(first).take(shown) {
        let y = top + (k - first) as i32 * MENU_LINE;
        if k == v.cursor && v.seed_grid.is_none() {
            c.hands.push((MENU_LABEL_X - 18, y));
        }
        let (label_ink, value_ink) = if row.enabled {
            (white, gold)
        } else {
            (grey, grey)
        };
        let label_w = if row.value.is_empty() {
            value_w + 140
        } else {
            136
        };
        c.label(
            row.label.clone(),
            MENU_LABEL_X,
            y,
            label_w,
            label_ink,
            Align::Left,
        );
        if !row.value.is_empty() {
            c.label(
                row.value.clone(),
                MENU_VALUE_X,
                y,
                value_w,
                value_ink,
                Align::Left,
            );
        }
    }
    // Rows past the window: a gold mark at its right edge.
    let mark_x = MENU_MAIN_FRAME.0 + MENU_MAIN_FRAME.2 - INSET;
    if first > 0 {
        c.label("^", mark_x, top, 8, gold, Align::Right);
    }
    if first + shown < n {
        c.label(
            "v",
            mark_x,
            top + (shown as i32 - 1) * MENU_LINE,
            8,
            gold,
            Align::Right,
        );
    }
    // The facts under the rows, two columns.
    let info_top = top + shown as i32 * MENU_LINE + 6;
    for (k, (label, value)) in v.info.iter().enumerate() {
        let col = (k % 2) as i32;
        let y = info_top + (k / 2) as i32 * MENU_LINE;
        let x = MENU_LABEL_X + col * 140;
        c.label(label.clone(), x, y, 82, white, Align::Left);
        c.label(value.clone(), x + 84, y, 50, gold, Align::Left);
    }

    c.windows.push(MENU_HELP_FRAME);
    let help_x = MENU_HELP_FRAME.0 + INSET;
    let help_w = MENU_HELP_FRAME.2 - 2 * INSET;
    let help_lines = match font {
        Some(f) => wrap(f, v.help, help_w, 2),
        None => vec![v.help.to_string()],
    };
    for (k, line) in help_lines.into_iter().enumerate() {
        let y = MENU_HELP_FRAME.1 + 4 + k as i32 * HELP_LINE;
        c.label(line, help_x, y, help_w, white, Align::Left);
    }
    c.label(
        v.summary,
        STAGE_W / 2,
        SUMMARY_Y,
        STAGE_TEXT_W,
        gold,
        Align::Centre,
    );
    c.label(
        v.hint,
        STAGE_W / 2,
        MENU_HINT_Y,
        STAGE_TEXT_W,
        grey,
        Align::Centre,
    );

    if let Some(g) = v.seed_grid {
        // The grid's window covers the rows: their labels under it go, as
        // the Options popup's do (the hosts lay all chrome before all text).
        let (fx, fy, fw, fh) = MENU_GRID_FRAME;
        c.labels.retain(|l| {
            let (x0, x1) = l.span().unwrap_or((l.x, l.x));
            x1 <= fx || x0 >= fx + fw || l.y + MENU_LINE <= fy || l.y >= fy + fh
        });
        c.windows.push(MENU_GRID_FRAME);
        c.label("Seed", fx + INSET + 4, fy + 8, 40, gold, Align::Left);
        c.label(
            format!("{}_", g.text),
            fx + INSET + 48,
            fy + 8,
            fw - 2 * INSET - 52,
            white,
            Align::Left,
        );
        let last = g.cells.len().saturating_sub(1);
        for (r, row) in g.cells.iter().enumerate() {
            let pitch = if r == last {
                GRID_ACTION_PITCH
            } else {
                GRID_PITCH
            };
            for (col, cell) in row.iter().enumerate() {
                let x = GRID_X + col as i32 * pitch;
                let y = GRID_Y + r as i32 * MENU_LINE;
                let on = g.cell == (r, col);
                if on {
                    // Where retail's name-entry grid seats its hand.
                    c.hands.push((x - 16, y - 2));
                }
                c.label(
                    cell.clone(),
                    x,
                    y,
                    pitch - 2,
                    if on { gold } else { white },
                    Align::Left,
                );
            }
        }
    }
    c
}

/// The rules menu's black backdrop: the stage, under the windows (the
/// title or the field under it is covered).
pub fn rules_menu_prims() -> Vec<ScreenPrim> {
    let mut c = Canvas::default();
    c.rect(0, 0, STAGE_W, STAGE_H, [0, 0, 0, 255]);
    c.prims
}

/// The rules menu's chrome sprites (windows, tab plaques, hand cursors), in
/// surface pixels through the stage transform.
pub fn rules_menu_sprites(
    v: &RulesMenuView<'_>,
    rects: &SaveMenuAtlasRects,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let c = compose_menu(v, None);
    let mut out = Vec::new();
    for (pen, w, active) in &c.tabs {
        let mut plaque = crate::tab_banner_draws(rects, *pen, *w, stage_origin, stage_scale);
        if !active {
            // The tabs not on screen sit back, darkened.
            for d in &mut plaque {
                d.color = [0.5, 0.5, 0.5, 1.0];
            }
        }
        out.extend(plaque);
    }
    for &w in &c.windows {
        out.extend(crate::menu_window_chrome_draws_for(
            rects,
            w,
            stage_origin,
            stage_scale,
        ));
    }
    let scale = stage_scale.max(1);
    let (_, _, hw, hh) = rects.cursor;
    for &(x, y) in &c.hands {
        out.push(SpriteDraw {
            dst: (
                stage_origin.0 + x * scale as i32,
                stage_origin.1 + y * scale as i32,
                hw * scale,
                hh * scale,
            ),
            src: rects.cursor,
            color: [1.0, 1.0, 1.0, 1.0],
        });
    }
    out
}

/// The rules menu's labels as text draws, in stage pixels.
pub fn rules_menu_texts(font: &legaia_font::Font, v: &RulesMenuView<'_>) -> Vec<TextDraw> {
    labels_to_draws(font, &compose_menu(v, Some(font)).labels)
}

/// The rules menu's labels as strings, in draw order (tests, a text host).
pub fn rules_menu_strings(v: &RulesMenuView<'_>) -> Vec<String> {
    compose_menu(v, None)
        .labels
        .into_iter()
        .map(|l| l.text)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view<'a>(rows: &'a [(String, String)], cursor: usize) -> RulesPanelView<'a> {
        RulesPanelView {
            title: "New Game",
            subtitle: Some("Hard"),
            rows,
            cursor: Some(cursor),
            footer: Some("help"),
            hint: "X select",
        }
    }

    fn numbered(n: usize) -> Vec<(String, String)> {
        (0..n)
            .map(|i| (format!("Row {i}"), format!("v{i}")))
            .collect()
    }

    /// The flat prims' bounding boxes `(x0, y0, x1, y1)`.
    fn boxes(c: &Canvas) -> Vec<(i32, i32, i32, i32, [u8; 4])> {
        c.prims
            .iter()
            .filter_map(|p| match p {
                ScreenPrim::Flat(q) => {
                    let xs = q.xy.map(|(x, _)| i32::from(x));
                    let ys = q.xy.map(|(_, y)| i32::from(y));
                    Some((
                        *xs.iter().min()?,
                        *ys.iter().min()?,
                        *xs.iter().max()?,
                        *ys.iter().max()?,
                        q.color,
                    ))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_panel_lists_its_rows_and_scrolls_to_the_cursor() {
        let rows = numbered(20);
        let v = view(&rows, 15);
        let s = rules_panel_strings(&v);
        assert!(s.contains(&"Row 15".to_string()));
        assert!(!s.contains(&"Row 0".to_string()), "scrolled past the top");
        assert!(s.contains(&"New Game".to_string()) && s.contains(&"help".to_string()));
        assert!(!rules_panel_prims(&v).is_empty());
    }

    #[test]
    fn a_long_list_stays_above_the_foot_line() {
        // The panel's last row and its edge end above the help line, which
        // a full list once butted against.
        for subtitle in [None, Some("Hard")] {
            let rows = numbered(20);
            let v = RulesPanelView {
                subtitle,
                ..view(&rows, 0)
            };
            let c = compose(&v);
            let foot = c.labels.iter().find(|l| l.text == "help").unwrap().y;
            for (_, y0, _, y1, _) in boxes(&c).into_iter().skip(2) {
                assert!(
                    y0 < foot && y1 <= foot - 2,
                    "prim {y0}..{y1} reaches the foot {foot}"
                );
            }
            for l in c.labels.iter().filter(|l| l.text.starts_with("Row")) {
                assert!(
                    l.y + LINE <= foot - 2,
                    "row at {} reaches the foot {foot}",
                    l.y
                );
            }
        }
    }

    #[test]
    fn every_label_is_held_inside_the_stage_and_off_its_neighbour() {
        let rows = vec![
            (
                "New Game+".to_string(),
                "Card 1 #15 Somebody Long Lv 99".to_string(),
            ),
            ("Start".to_string(), String::new()),
        ];
        let v = RulesPanelView {
            subtitle: Some("Boss Rush / Nightmare / NG+12 / Hardcore / Seed JUGGERNAUT-9999"),
            ..view(&rows, 0)
        };
        let c = compose(&v);
        for l in &c.labels {
            let (x0, x1) = l.span().expect("every label has a box");
            assert!(x0 >= 0 && x1 <= STAGE_W, "{:?} spans {x0}..{x1}", l.text);
        }
        let label = c.labels.iter().find(|l| l.text == "New Game+").unwrap();
        let value = c
            .labels
            .iter()
            .find(|l| l.text.starts_with("Card"))
            .unwrap();
        assert!(label.span().unwrap().1 < value.span().unwrap().0);
    }

    #[test]
    fn hidden_rows_are_marked() {
        let marks = |rows: &[(String, String)], cursor| {
            boxes(&compose(&view(rows, cursor)))
                .into_iter()
                .filter(|b| b.4 == ink::HIGHLIGHT)
                .count()
        };
        let short = numbered(5);
        assert_eq!(marks(&short, 0), 0, "a list that fits has no mark");
        let long = numbered(20);
        assert_eq!(marks(&long, 0), 4, "rows below: one mark");
        assert_eq!(marks(&long, 10), 8, "rows above and below: two");
        assert_eq!(marks(&long, 19), 4, "rows above: one mark");
    }

    fn menu_rows(n: usize) -> Vec<RulesMenuRow> {
        (0..n)
            .map(|i| RulesMenuRow {
                label: format!("Row {i}"),
                value: format!("v{i}"),
                enabled: i != 1,
            })
            .collect()
    }

    fn menu_view<'a>(
        rows: &'a [RulesMenuRow],
        info: &'a [(String, String)],
        grid: Option<SeedGridView<'a>>,
    ) -> RulesMenuView<'a> {
        RulesMenuView {
            tabs: &["Run", "Difficulty", "New Game+", "Randomizer"],
            tab: 1,
            rows,
            cursor: 0,
            info,
            help: "Help text",
            summary: "Hard / Seed RIM-0042",
            hint: "Q/W tab  Z OK",
            seed_grid: grid,
        }
    }

    /// The label's box and its glyphs' inked rows (2..=13 of the cell)
    /// clear of the window's 4-px border.
    fn inside(l: &Label, f: (i32, i32, i32, i32)) -> bool {
        let (x0, x1) = l.span().unwrap();
        x0 >= f.0 + 4 && x1 <= f.0 + f.2 - 4 && l.y + 2 >= f.1 + 4 && l.y + 13 < f.1 + f.3 - 4
    }

    #[test]
    fn the_menu_keeps_each_label_in_its_window() {
        let rows = menu_rows(9);
        let info: Vec<(String, String)> = (0..4).map(|i| (format!("K{i}"), "x1".into())).collect();
        let c = compose_menu(&menu_view(&rows, &info, None), None);
        for l in &c.labels {
            let (x0, x1) = l.span().unwrap();
            assert!(
                x0 >= 0 && x1 <= STAGE_W && l.y + LINE <= STAGE_H,
                "{:?}",
                l.text
            );
            if l.text.starts_with("Row") || l.text.starts_with('v') {
                assert!(inside(l, MENU_MAIN_FRAME), "{:?} at {}", l.text, l.y);
            }
            if l.text == "Help text" {
                assert!(inside(l, MENU_HELP_FRAME));
                // The second line too.
                let second = Label {
                    y: l.y + HELP_LINE,
                    ..l.clone()
                };
                assert!(inside(&second, MENU_HELP_FRAME));
            }
        }
        assert_eq!(MENU_VISIBLE_ROWS, 9);
        assert_eq!(
            c.tabs.iter().filter(|t| t.2).count(),
            1,
            "one tab on screen"
        );
        assert_eq!(
            c.hands,
            vec![(MENU_LABEL_X - 18, MENU_MAIN_FRAME.1 + INSET_Y)]
        );
        let grey = crate::menu_ink_color(0);
        assert_eq!(
            c.labels.iter().find(|l| l.text == "Row 1").unwrap().color,
            grey
        );
        assert_eq!(c.windows, vec![MENU_MAIN_FRAME, MENU_HELP_FRAME]);
    }

    #[test]
    fn the_seed_grid_covers_the_rows_under_it() {
        let rows = menu_rows(9);
        let cells: Vec<Vec<String>> = vec![
            "ABCDEFGHIJ".chars().map(String::from).collect(),
            vec!["Del".into(), "Done".into()],
        ];
        let grid = SeedGridView {
            text: "RIM",
            cells: &cells,
            cell: (1, 1),
        };
        let c = compose_menu(&menu_view(&rows, &[], Some(grid)), None);
        assert_eq!(c.windows.last(), Some(&MENU_GRID_FRAME));
        assert_eq!(c.hands.len(), 1, "only the grid's hand");
        for l in c.labels.iter().filter(|l| l.text.starts_with("Row")) {
            assert!(!inside(l, MENU_GRID_FRAME), "{:?} shows through", l.text);
        }
        let done = c.labels.iter().find(|l| l.text == "Done").unwrap();
        assert_eq!(done.color, crate::MENU_TEXT_GOLD);
        assert!(inside(done, MENU_GRID_FRAME));
        assert!(c.labels.iter().any(|l| l.text == "RIM_"));
        let mut rects = SaveMenuAtlasRects::default();
        let corner = (0, 0, 4, 4);
        (
            rects.panel_tl,
            rects.panel_tr,
            rects.panel_bl,
            rects.panel_br,
        ) = (corner, corner, corner, corner);
        (rects.panel_top, rects.panel_bot) = ((0, 0, 24, 4), (0, 0, 24, 4));
        (rects.panel_left, rects.panel_right) = ((0, 0, 4, 21), (0, 0, 4, 21));
        (rects.panel_interior, rects.panel_filigree) = ((0, 0, 32, 29), (0, 0, 32, 29));
        (rects.tab_cap_l, rects.tab_body, rects.tab_cap_r) =
            ((0, 0, 8, 20), (0, 0, 16, 20), (0, 0, 8, 20));
        rects.cursor = (152, 64, 16, 16);
        let sprites = rules_menu_sprites(&menu_view(&rows, &[], Some(grid)), &rects, (0, 0), 1);
        let hands = sprites.iter().filter(|d| d.src == rects.cursor).count();
        assert_eq!(hands, 1);
        // Three tabs not on screen are darkened; the one on screen is not.
        assert!(sprites.iter().any(|d| d.color == [0.5, 0.5, 0.5, 1.0]));
        assert_eq!(rules_menu_prims().len(), 1);
    }
}

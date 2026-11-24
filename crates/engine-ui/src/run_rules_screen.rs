//! The run rules' screens: the New Game rules screen and the Boss Rush
//! intermission and result, one composition ([`RulesMenuView`]).
//!
//! They are drawn in the game's own menu style: the pause menu's 9-slice
//! windows over black, the carved title-tab plaques for the tabs (or the
//! screen's title), the pointing-hand cursor, the menu inks and the menu
//! font's own list arrows, so they read as the Options screen does. The
//! sprites sample the system-UI chrome atlas ([`SaveMenuAtlasRects`]) and
//! the labels the menu font, which both hosts already draw for the pause
//! menu.
//!
//! The view is plain rows (label, value) and a cursor, so this layer names
//! no run-rule type; `legaia_engine_screens::run_rules` projects the
//! sessions into it once for both hosts. The composition also answers where
//! a tap lands ([`rules_menu_hit`]), from the same geometry it draws.

use legaia_render_kernels::screen_prim::ScreenPrim;

use crate::expansion_modes::{Align, Canvas, Label, STAGE_H, STAGE_W, labels_to_draws};
use crate::{SaveMenuAtlasRects, SpriteDraw, TextDraw};

/// One row of a rules screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulesMenuRow {
    pub label: String,
    /// The value column; empty for an action row (*Start*).
    pub value: String,
    /// Drawn in the menu's grey when changing it does nothing.
    pub enabled: bool,
    /// Left / Right change it: the selected row shows arrows beside its
    /// value, and a tap on either steps it.
    pub changes: bool,
}

impl RulesMenuRow {
    /// A row Left / Right do not step (an action, a choice).
    pub fn action(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: String::new(),
            enabled: true,
            changes: false,
        }
    }
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

/// A list window over the rows (the New Game+ save list, the confirm
/// before *Begin*).
#[derive(Debug, Clone, Copy)]
pub struct ListWindowView<'a> {
    pub title: &'a str,
    /// `(label, value)` facts above the choices; not selectable.
    pub facts: &'a [(String, String)],
    pub rows: &'a [RulesMenuRow],
    pub cursor: usize,
}

/// One line of facts under the rows: its cells share the window's width.
pub type InfoLine = Vec<(String, String)>;

/// A rules screen.
#[derive(Debug, Clone, Copy)]
pub struct RulesMenuView<'a> {
    /// The tab headings, left to right; one heading is the screen's title
    /// on a plaque the window's width.
    pub tabs: &'a [&'a str],
    /// The tab on screen.
    pub tab: usize,
    pub rows: &'a [RulesMenuRow],
    /// The selected row, `None` for a screen with no cursor.
    pub cursor: Option<usize>,
    /// Facts listed under the rows (a preset's scales, the Boss Rush stages
    /// and record); not selectable. Lines past the window are dropped.
    pub info: &'a [InfoLine],
    /// The help window's text, wrapped to its two lines.
    pub help: &'a str,
    /// The run's one-line summary under the help window.
    pub summary: &'a str,
    /// The key hint at the foot.
    pub hint: &'a str,
    pub seed_grid: Option<SeedGridView<'a>>,
    pub list: Option<ListWindowView<'a>>,
}

/// What a tap on a rules screen lands on ([`rules_menu_hit`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuHit {
    Tab(usize),
    Row(usize),
    /// The arrow left of row `i`'s value.
    Prev(usize),
    /// The arrow right of row `i`'s value.
    Next(usize),
    /// Choice `i` of the list window.
    List(usize),
    /// Seed grid cell `(row, column)`.
    Cell(usize, usize),
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
/// The list window (save list, confirm), over the rows.
pub const MENU_LIST_FRAME: (i32, i32, i32, i32) = (20, 34, 280, 130);
/// A window's content inset from its frame, across and down (the glyph
/// cell's two blank top rows make up the difference).
const INSET: i32 = 8;
const INSET_Y: i32 = 6;
/// The menu's row pitch (the Options screen's).
const MENU_LINE: i32 = 14;
/// Where a row's label starts; the hand sits 18 px left of it, as on the
/// Options screen.
const MENU_LABEL_X: i32 = MENU_MAIN_FRAME.0 + INSET + 18;
/// The label's box: it stops short of the value's left arrow.
const MENU_LABEL_W: i32 = 124;
/// Where a row's value starts (the Options screen's `+140`), and its box,
/// which stops short of the right arrow.
const MENU_VALUE_X: i32 = MENU_LABEL_X + 140;
const MENU_VALUE_W: i32 = 106;
/// The value arrows' columns (12-px cells), and the scroll marks' (8-px).
const ARROW_PREV_X: i32 = MENU_VALUE_X - 14;
const ARROW_NEXT_X: i32 = MENU_VALUE_X + MENU_VALUE_W + 2;
const SCROLL_MARK_X: i32 = MENU_MAIN_FRAME.0 + MENU_MAIN_FRAME.2 - INSET - 6;
/// The rows the main window holds.
pub const MENU_VISIBLE_ROWS: usize = ((MENU_MAIN_FRAME.3 - 2 * INSET_Y) / MENU_LINE) as usize;
const SUMMARY_Y: i32 = 209;
const MENU_HINT_Y: i32 = 224;
/// A text box the width of the stage, less a margin each side.
const STAGE_TEXT_W: i32 = STAGE_W - 16;
/// A help line's pitch.
const HELP_LINE: i32 = 13;
/// Grid cell pitch.
const GRID_PITCH: i32 = 22;
/// The grid's first cell, its last (action) row's pitch.
const GRID_X: i32 = MENU_GRID_FRAME.0 + 24;
const GRID_Y: i32 = MENU_GRID_FRAME.1 + 26;
const GRID_ACTION_PITCH: i32 = 66;

/// A mark drawn from the menu font's own sprite cells, with a glyph to fall
/// back on in a font built without them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    /// More rows above / below (icons `0x67` / `0x68`, 8x8).
    ListUp,
    ListDown,
    /// The value arrows (the 12x12 command arrows in the gold ink).
    Prev,
    Next,
}

impl Mark {
    fn cell(self) -> u16 {
        use legaia_font::sprite_cell_ids as ids;
        match self {
            Mark::ListUp => ids::LIST_UP,
            Mark::ListDown => ids::LIST_DOWN,
            Mark::Prev => ids::ink_arrow(6, 3),
            Mark::Next => ids::ink_arrow(6, 2),
        }
    }

    fn fallback(self) -> &'static str {
        match self {
            Mark::ListUp => "^",
            Mark::ListDown => "v",
            Mark::Prev => "<",
            Mark::Next => ">",
        }
    }
}

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
    /// Font sprite marks: top-left corner and which.
    marks: Vec<((i32, i32), Mark)>,
    labels: Vec<Label>,
    /// Tap targets, front first: a tap takes the first that holds it.
    hits: Vec<((i32, i32, i32, i32), MenuHit)>,
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

    /// Drop the labels and marks under `frame` (a window drawn over them:
    /// the hosts lay all chrome before all text), the hand, and every tap
    /// target so far.
    fn cover(&mut self, frame: (i32, i32, i32, i32)) {
        let (fx, fy, fw, fh) = frame;
        let clear = |x0: i32, x1: i32, y: i32| {
            x1 <= fx || x0 >= fx + fw || y + MENU_LINE <= fy || y >= fy + fh
        };
        self.labels.retain(|l| {
            let (x0, x1) = l.span().unwrap_or((l.x, l.x));
            clear(x0, x1, l.y)
        });
        self.marks.retain(|((x, y), _)| clear(*x, *x + 12, *y));
        self.hands.clear();
        // The window takes the taps: nothing under it answers one.
        self.hits.clear();
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

/// The first row a window of `fit` rows shows with the cursor on `cursor`.
fn scroll_first(cursor: Option<usize>, n: usize, fit: usize) -> usize {
    cursor.map_or(0, |cur| {
        cur.saturating_sub(fit.saturating_sub(1))
            .min(n.saturating_sub(fit))
    })
}

fn compose_menu(v: &RulesMenuView<'_>, font: Option<&legaia_font::Font>) -> MenuCanvas {
    let white = crate::MENU_TEXT_WHITE;
    let gold = crate::MENU_TEXT_GOLD;
    let grey = crate::menu_ink_color(0);
    let mut c = MenuCanvas::default();

    // The tabs, or one plaque the window's width for a screen's title.
    let single = v.tabs.len() == 1;
    for (i, name) in v.tabs.iter().enumerate() {
        let (pen, w) = if single {
            ((MENU_MAIN_FRAME.0 + 8, TAB_PEN_Y), MENU_MAIN_FRAME.2 - 16)
        } else {
            ((8 + i as i32 * TAB_PITCH + 8, TAB_PEN_Y), TAB_CONTENT_W)
        };
        let active = single || i == v.tab;
        c.tabs.push((pen, w, active));
        c.label(
            *name,
            pen.0 + w / 2,
            pen.1,
            w + 12,
            if active { gold } else { grey },
            Align::Centre,
        );
        if !single {
            c.hits
                .push(((pen.0 - 8, pen.1 - 4, w + 16, 20), MenuHit::Tab(i)));
        }
    }

    c.windows.push(MENU_MAIN_FRAME);
    let top = MENU_MAIN_FRAME.1 + INSET_Y;
    let n = v.rows.len();
    let fit = MENU_VISIBLE_ROWS;
    let first = scroll_first(v.cursor, n, fit);
    let shown = n.min(fit);
    for (k, row) in v.rows.iter().enumerate().skip(first).take(shown) {
        let y = top + (k - first) as i32 * MENU_LINE;
        let selected = v.cursor == Some(k);
        if selected {
            c.hands.push((MENU_LABEL_X - 18, y));
        }
        let (label_ink, value_ink) = if row.enabled {
            (white, gold)
        } else {
            (grey, grey)
        };
        let label_w = if row.value.is_empty() {
            MENU_LABEL_W + 140
        } else {
            MENU_LABEL_W
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
                MENU_VALUE_W,
                value_ink,
                Align::Left,
            );
        }
        if row.changes {
            if selected {
                c.marks.push(((ARROW_PREV_X, y + 1), Mark::Prev));
                c.marks.push(((ARROW_NEXT_X, y + 1), Mark::Next));
            }
            c.hits
                .push(((ARROW_PREV_X - 4, y - 1, 18, MENU_LINE), MenuHit::Prev(k)));
            c.hits
                .push(((ARROW_NEXT_X - 2, y - 1, 18, MENU_LINE), MenuHit::Next(k)));
        }
        c.hits.push((
            (
                MENU_MAIN_FRAME.0 + 4,
                y - 1,
                MENU_MAIN_FRAME.2 - 8,
                MENU_LINE,
            ),
            MenuHit::Row(k),
        ));
    }
    // Rows past the window: the menu's own list arrows at its right edge.
    if first > 0 {
        c.marks.push(((SCROLL_MARK_X, top + 3), Mark::ListUp));
    }
    if first + shown < n {
        c.marks.push((
            (SCROLL_MARK_X, top + (shown as i32 - 1) * MENU_LINE + 3),
            Mark::ListDown,
        ));
    }
    // The facts under the rows, each line's cells sharing the width; the
    // lines past the window are dropped.
    let info_top = top + shown as i32 * MENU_LINE + 6;
    let bottom = MENU_MAIN_FRAME.1 + MENU_MAIN_FRAME.3 - INSET_Y - MENU_LINE;
    let info_w = 280;
    for (k, line) in v.info.iter().enumerate() {
        let y = info_top + k as i32 * MENU_LINE;
        if y > bottom || line.is_empty() {
            continue;
        }
        let cell_w = info_w / line.len() as i32;
        let value_dx = cell_w * 3 / 5;
        for (j, (label, value)) in line.iter().enumerate() {
            let x = MENU_LABEL_X + j as i32 * cell_w;
            c.label(label.clone(), x, y, value_dx - 2, white, Align::Left);
            c.label(
                value.clone(),
                x + value_dx,
                y,
                cell_w - value_dx - 4,
                gold,
                Align::Left,
            );
        }
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

    if let Some(l) = v.list {
        compose_list(&mut c, &l);
    }
    if let Some(g) = v.seed_grid {
        compose_grid(&mut c, &g);
    }
    c
}

/// The list window over the rows: its title, its facts, then its choices
/// with the hand on the cursor's, scrolling past the window.
fn compose_list(c: &mut MenuCanvas, l: &ListWindowView<'_>) {
    let white = crate::MENU_TEXT_WHITE;
    let gold = crate::MENU_TEXT_GOLD;
    let (fx, fy, fw, fh) = MENU_LIST_FRAME;
    c.cover(MENU_LIST_FRAME);
    c.windows.push(MENU_LIST_FRAME);
    let x = fx + INSET + 18;
    let value_x = x + 128;
    let value_w = fx + fw - INSET - value_x - 10;
    c.label(
        l.title,
        fx + fw / 2,
        fy + INSET_Y,
        fw - 2 * INSET,
        gold,
        Align::Centre,
    );
    let mut y = fy + INSET_Y + MENU_LINE + 4;
    for (label, value) in l.facts {
        c.label(label.clone(), x, y, 124, white, Align::Left);
        c.label(value.clone(), value_x, y, value_w, gold, Align::Left);
        y += MENU_LINE;
    }
    if !l.facts.is_empty() {
        y += 4;
    }
    let fit = ((fy + fh - INSET_Y - y) / MENU_LINE).max(1) as usize;
    let n = l.rows.len();
    let first = scroll_first(Some(l.cursor), n, fit);
    let shown = n.min(fit);
    for (k, row) in l.rows.iter().enumerate().skip(first).take(shown) {
        let ry = y + (k - first) as i32 * MENU_LINE;
        if k == l.cursor {
            c.hands.push((x - 18, ry));
        }
        let label_w = if row.value.is_empty() { fw - 44 } else { 124 };
        c.label(row.label.clone(), x, ry, label_w, white, Align::Left);
        if !row.value.is_empty() {
            c.label(row.value.clone(), value_x, ry, value_w, gold, Align::Left);
        }
        c.hits
            .push(((fx + 4, ry - 1, fw - 8, MENU_LINE), MenuHit::List(k)));
    }
    let mark_x = fx + fw - INSET - 6;
    if first > 0 {
        c.marks.push(((mark_x, y + 3), Mark::ListUp));
    }
    if first + shown < n {
        c.marks.push((
            (mark_x, y + (shown as i32 - 1) * MENU_LINE + 3),
            Mark::ListDown,
        ));
    }
}

/// The seed editor's window over the rows.
fn compose_grid(c: &mut MenuCanvas, g: &SeedGridView<'_>) {
    let white = crate::MENU_TEXT_WHITE;
    let gold = crate::MENU_TEXT_GOLD;
    let (fx, fy, fw, _) = MENU_GRID_FRAME;
    c.cover(MENU_GRID_FRAME);
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
            c.hits
                .push(((x - 6, y - 1, pitch, MENU_LINE), MenuHit::Cell(r, col)));
        }
    }
}

/// The rules screen's black backdrop: the stage, under the windows (the
/// title or the field under it is covered).
pub fn rules_menu_prims() -> Vec<ScreenPrim> {
    let mut c = Canvas::default();
    c.rect(0, 0, STAGE_W, STAGE_H, [0, 0, 0, 255]);
    c.prims
}

/// The rules screen's chrome sprites (windows, plaques, hand cursors), in
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
    // A window drawn over the rows (the list, the grid) is drawn after
    // them, so its frame covers what is under it.
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

/// The rules screen's labels and font marks as text draws, in stage pixels.
pub fn rules_menu_texts(font: &legaia_font::Font, v: &RulesMenuView<'_>) -> Vec<TextDraw> {
    let mut c = compose_menu(v, Some(font));
    let mut out = Vec::new();
    for &((x, y), mark) in &c.marks {
        match crate::arts_input::list_cell_draw(font, mark.cell(), (x, y), (0, 0), 1) {
            Some(d) => out.push(d),
            // A font without the sprite cells: the glyph.
            None => c.labels.push(Label {
                text: mark.fallback().into(),
                x,
                y,
                color: crate::MENU_TEXT_GOLD,
                align: Align::Left,
                max_w: None,
            }),
        }
    }
    let mut texts = labels_to_draws(font, &c.labels);
    texts.extend(out);
    texts
}

/// The rules screen's labels as strings, in draw order (tests, a text
/// host).
pub fn rules_menu_strings(v: &RulesMenuView<'_>) -> Vec<String> {
    compose_menu(v, None)
        .labels
        .into_iter()
        .map(|l| l.text)
        .collect()
}

/// What a tap at stage pixel `at` lands on, from the geometry the screen
/// draws: the front window's targets first (the list, the grid), else the
/// tabs, a value's arrows, then a row.
pub fn rules_menu_hit(v: &RulesMenuView<'_>, at: (i32, i32)) -> Option<MenuHit> {
    let c = compose_menu(v, None);
    let (x, y) = at;
    c.hits
        .iter()
        .find(|((hx, hy, hw, hh), _)| x >= *hx && x < hx + hw && y >= *hy && y < hy + hh)
        .map(|(_, h)| *h)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu_rows(n: usize) -> Vec<RulesMenuRow> {
        (0..n)
            .map(|i| RulesMenuRow {
                label: format!("Row {i}"),
                value: format!("v{i}"),
                enabled: i != 1,
                changes: true,
            })
            .collect()
    }

    fn menu_view<'a>(
        rows: &'a [RulesMenuRow],
        info: &'a [InfoLine],
        grid: Option<SeedGridView<'a>>,
    ) -> RulesMenuView<'a> {
        RulesMenuView {
            tabs: &["Run", "Difficulty", "New Game+", "Randomizer"],
            tab: 1,
            rows,
            cursor: Some(0),
            info,
            help: "Help text",
            summary: "Hard / Seed RIM-0042",
            hint: "Q/W tab  Z OK",
            seed_grid: grid,
            list: None,
        }
    }

    /// The label's box and its glyphs' inked rows (2..=13 of the cell)
    /// clear of the window's 4-px border.
    fn inside(l: &Label, f: (i32, i32, i32, i32)) -> bool {
        let (x0, x1) = l.span().unwrap();
        x0 >= f.0 + 4 && x1 <= f.0 + f.2 - 4 && l.y + 2 >= f.1 + 4 && l.y + 13 < f.1 + f.3 - 4
    }

    fn test_rects() -> SaveMenuAtlasRects {
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
        rects
    }

    #[test]
    fn the_menu_keeps_each_label_in_its_window() {
        let rows = menu_rows(9);
        let info: Vec<InfoLine> = vec![
            vec![("K0".into(), "x1".into()), ("K1".into(), "x1".into())],
            vec![("A".into(), "1".into()); 3],
        ];
        let c = compose_menu(&menu_view(&rows, &info, None), None);
        for l in &c.labels {
            let (x0, x1) = l.span().unwrap();
            assert!(
                x0 >= 0 && x1 <= STAGE_W && l.y + 13 <= STAGE_H,
                "{:?}",
                l.text
            );
            if l.text.starts_with("Row") || l.text.starts_with('v') {
                assert!(inside(l, MENU_MAIN_FRAME), "{:?} at {}", l.text, l.y);
            }
            if l.text == "Help text" {
                assert!(inside(l, MENU_HELP_FRAME));
                let second = Label {
                    y: l.y + HELP_LINE,
                    ..l.clone()
                };
                assert!(inside(&second, MENU_HELP_FRAME));
            }
        }
        // A label never runs into its value's arrows.
        let label = c.labels.iter().find(|l| l.text == "Row 0").unwrap();
        assert!(label.span().unwrap().1 <= ARROW_PREV_X);
        let value = c.labels.iter().find(|l| l.text == "v0").unwrap();
        assert!(value.span().unwrap().1 <= ARROW_NEXT_X);
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
        // The selected row's value arrows; the other rows show none.
        assert_eq!(
            c.marks
                .iter()
                .filter(|m| matches!(m.1, Mark::Prev | Mark::Next))
                .count(),
            2
        );
        let grey = crate::menu_ink_color(0);
        assert_eq!(
            c.labels.iter().find(|l| l.text == "Row 1").unwrap().color,
            grey
        );
        assert_eq!(c.windows, vec![MENU_MAIN_FRAME, MENU_HELP_FRAME]);
    }

    #[test]
    fn hidden_rows_are_marked_with_the_list_arrows() {
        let marks = |rows: &[RulesMenuRow], cursor| {
            let v = RulesMenuView {
                cursor: Some(cursor),
                ..menu_view(rows, &[], None)
            };
            compose_menu(&v, None)
                .marks
                .iter()
                .filter(|m| matches!(m.1, Mark::ListUp | Mark::ListDown))
                .map(|m| m.1)
                .collect::<Vec<_>>()
        };
        let short = menu_rows(5);
        assert!(marks(&short, 0).is_empty(), "a list that fits has no mark");
        let long = menu_rows(14);
        assert_eq!(marks(&long, 0), vec![Mark::ListDown]);
        assert_eq!(marks(&long, 10), vec![Mark::ListUp, Mark::ListDown]);
        assert_eq!(marks(&long, 13), vec![Mark::ListUp]);
    }

    #[test]
    fn taps_land_where_the_screen_draws() {
        let rows = menu_rows(3);
        let v = menu_view(&rows, &[], None);
        let row_y = MENU_MAIN_FRAME.1 + INSET_Y + MENU_LINE + 5;
        assert_eq!(rules_menu_hit(&v, (60, row_y)), Some(MenuHit::Row(1)));
        assert_eq!(
            rules_menu_hit(&v, (ARROW_PREV_X + 4, row_y)),
            Some(MenuHit::Prev(1))
        );
        assert_eq!(
            rules_menu_hit(&v, (ARROW_NEXT_X + 4, row_y)),
            Some(MenuHit::Next(1))
        );
        assert_eq!(
            rules_menu_hit(&v, (8 + 3 * TAB_PITCH + 20, 10)),
            Some(MenuHit::Tab(3))
        );
        assert_eq!(rules_menu_hit(&v, (160, 190)), None, "the help window");
        // A window over the rows takes every tap inside it.
        let choices = [RulesMenuRow::action("Begin"), RulesMenuRow::action("Back")];
        let facts = [("Difficulty".to_string(), "Hard".to_string())];
        let list = ListWindowView {
            title: "Begin with these rules?",
            facts: &facts,
            rows: &choices,
            cursor: 0,
        };
        let v = RulesMenuView {
            list: Some(list),
            ..v
        };
        let c = compose_menu(&v, None);
        let back = c.labels.iter().find(|l| l.text == "Back").unwrap();
        assert_eq!(
            rules_menu_hit(&v, (back.x + 4, back.y + 4)),
            Some(MenuHit::List(1))
        );
        assert_eq!(rules_menu_hit(&v, (60, row_y)), None);
        assert!(!c.labels.iter().any(|l| l.text == "Row 1"), "covered");
        assert_eq!(c.hands.len(), 1, "only the window's hand");
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
        let v = menu_view(&rows, &[], Some(grid));
        let c = compose_menu(&v, None);
        assert_eq!(c.windows.last(), Some(&MENU_GRID_FRAME));
        assert_eq!(c.hands.len(), 1, "only the grid's hand");
        for l in c.labels.iter().filter(|l| l.text.starts_with("Row")) {
            assert!(!inside(l, MENU_GRID_FRAME), "{:?} shows through", l.text);
        }
        let done = c.labels.iter().find(|l| l.text == "Done").unwrap();
        assert_eq!(done.color, crate::MENU_TEXT_GOLD);
        assert!(inside(done, MENU_GRID_FRAME));
        assert_eq!(
            rules_menu_hit(&v, (done.x + 2, done.y + 4)),
            Some(MenuHit::Cell(1, 1))
        );
        assert!(c.labels.iter().any(|l| l.text == "RIM_"));
        let rects = test_rects();
        let sprites = rules_menu_sprites(&v, &rects, (0, 0), 1);
        let hands = sprites.iter().filter(|d| d.src == rects.cursor).count();
        assert_eq!(hands, 1);
        // Three tabs not on screen are darkened; the one on screen is not.
        assert!(sprites.iter().any(|d| d.color == [0.5, 0.5, 0.5, 1.0]));
        assert_eq!(rules_menu_prims().len(), 1);
    }

    #[test]
    fn one_heading_is_a_title_plaque() {
        let rows = [RulesMenuRow::action("Next battle")];
        let v = RulesMenuView {
            tabs: &["BOSS RUSH  3 / 8"],
            tab: 0,
            ..menu_view(&rows, &[], None)
        };
        let c = compose_menu(&v, None);
        assert_eq!(c.tabs.len(), 1);
        assert!(c.tabs[0].2, "the title is never darkened");
        assert_eq!(c.tabs[0].1, MENU_MAIN_FRAME.2 - 16);
        assert!(!c.hits.iter().any(|h| matches!(h.1, MenuHit::Tab(_))));
    }
}

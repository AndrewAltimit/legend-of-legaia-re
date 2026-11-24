//! The run rules' screens: the New Game rules screen and the Boss Rush
//! intermission and result, as one titled list panel.
//!
//! The view is plain rows (label, value) and a cursor, so this layer names
//! no run-rule type; `legaia_engine_screens::run_rules` projects the
//! sessions into it once for both hosts. The panel draws over the whole
//! stage (the title or the field under it is covered), in the expansion
//! screens' palette.

use legaia_render_kernels::screen_prim::ScreenPrim;

use crate::TextDraw;
use crate::expansion_modes::{
    Align, Canvas, LINE, STAGE_H, STAGE_W, ink, labels_to_draws, pen as text,
};

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
}

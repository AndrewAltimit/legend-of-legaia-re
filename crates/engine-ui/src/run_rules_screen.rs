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
use crate::expansion_modes::{Canvas, LINE, STAGE_H, STAGE_W, ink, labels_to_draws, pen as text};

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

/// Rows the panel shows at once; a longer list scrolls with the cursor.
pub const VISIBLE_ROWS: usize = 11;

fn compose(v: &RulesPanelView<'_>) -> Canvas {
    let mut c = Canvas::default();
    c.rect(0, 0, STAGE_W, STAGE_H, ink::BACKDROP);
    c.rect(0, 0, STAGE_W, 22, ink::BAR);
    c.text_centre(v.title, STAGE_W / 2, 6, text::GOLD);
    let mut y = 28;
    if let Some(s) = v.subtitle {
        c.text_centre(s, STAGE_W / 2, y, text::DIM);
        y += LINE;
    }
    let top = y + 2;
    let n = v.rows.len();
    let first = v.cursor.map_or(0, |cur| {
        cur.saturating_sub(VISIBLE_ROWS - 1)
            .min(n.saturating_sub(VISIBLE_ROWS))
    });
    let shown = n.min(VISIBLE_ROWS);
    c.panel(
        16,
        top,
        STAGE_W - 32,
        shown as i32 * LINE + 8,
        ink::PANEL,
        ink::PANEL_EDGE,
    );
    for (k, (label, value)) in v.rows.iter().enumerate().skip(first).take(shown) {
        let ry = top + 4 + (k - first) as i32 * LINE;
        let selected = v.cursor == Some(k);
        if selected {
            c.rect(18, ry - 1, STAGE_W - 36, LINE, ink::SELECT);
        }
        let colour = if selected { text::GOLD } else { text::WHITE };
        c.text(label.clone(), 26, ry, colour);
        if !value.is_empty() {
            c.text_right(value.clone(), STAGE_W - 26, ry, colour);
        }
    }
    if let Some(f) = v.footer {
        c.text_centre(f, STAGE_W / 2, STAGE_H - 34, text::WHITE);
    }
    c.text_centre(v.hint, STAGE_W / 2, STAGE_H - 16, text::DIM);
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

    #[test]
    fn the_panel_lists_its_rows_and_scrolls_to_the_cursor() {
        let rows: Vec<(String, String)> = (0..20)
            .map(|i| (format!("Row {i}"), format!("v{i}")))
            .collect();
        let v = RulesPanelView {
            title: "New Game",
            subtitle: Some("Hard"),
            rows: &rows,
            cursor: Some(15),
            footer: Some("help"),
            hint: "X select",
        };
        let s = rules_panel_strings(&v);
        assert!(s.contains(&"Row 15".to_string()));
        assert!(!s.contains(&"Row 0".to_string()), "scrolled past the top");
        assert!(s.contains(&"New Game".to_string()) && s.contains(&"help".to_string()));
        assert!(!rules_panel_prims(&v).is_empty());
    }
}

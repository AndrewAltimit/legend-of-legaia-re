//! The expansion pack's item screen: a pack shop's counter (Buy / Sell) and
//! the pack bag (Items / Gear), with the equip step's stat compare - drawn
//! in the flat style of the other expansion screens, from one composition
//! for both halves.
//!
//! ```text
//! Tower Armory                                     48210 G
//!  [Buy]  Sell
//! +---------------------------------+ +----------------+
//! |> Rebirth Blade         64000 G  | | Forged in the  |
//! |  Phoenix Plume          9000 G  | | tower's heart. |
//! |  Healing Berry          3000 G  | | Weapon ATK +108|
//! +---------------------------------+ +----------------+
//! Bought 1 Rebirth Blade for 64000 G.
//! L/R: Buy/Sell  X: choose  O: leave
//! ```

use legaia_render_kernels::screen_prim::ScreenPrim;

use super::{Canvas, LINE, STAGE_H, STAGE_W, footer, header, ink, labels_to_draws, pen, wrap};
use crate::TextDraw;

/// One list row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemRowView {
    pub name: String,
    /// Right-aligned: a price or a count.
    pub right: String,
    /// `Weapon`, `E Vahn`, `Have 2`: shown for the selected row.
    pub sub: String,
    /// Greyed (unaffordable, none held).
    pub dim: bool,
}

/// One member row of the use / wear step.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemMemberView {
    pub name: String,
    /// HP / MP, or the slot and what it holds.
    pub line: String,
    pub dim: bool,
}

/// The equip step's compare panel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemCompareView {
    pub heading: String,
    /// `(label, now, after)`.
    pub rows: Vec<(String, u16, u16)>,
}

/// Everything the item screen draws.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemScreenView {
    pub title: String,
    pub gold: u32,
    pub tabs: Vec<String>,
    pub tab: usize,
    pub rows: Vec<ItemRowView>,
    pub cursor: usize,
    /// What an empty list says.
    pub empty: String,
    /// The selected row's description and stat lines.
    pub detail: Vec<String>,
    /// `(quantity, total)` while choosing how many.
    pub quantity: Option<(u8, u32)>,
    /// `(rows, cursor)` while choosing who.
    pub members: Option<(Vec<ItemMemberView>, usize)>,
    pub compare: Option<ItemCompareView>,
    pub message: Option<String>,
    pub footer: String,
}

/// The screen's flat prims, in stage pixels.
pub fn item_screen_prims(v: &ItemScreenView) -> Vec<ScreenPrim> {
    compose(v).prims
}

/// The screen's labels, in stage pixels.
pub fn item_screen_texts(font: &legaia_font::Font, v: &ItemScreenView) -> Vec<TextDraw> {
    labels_to_draws(font, &compose(v).labels)
}

/// The labels as plain strings, in draw order.
pub fn item_screen_strings(v: &ItemScreenView) -> Vec<String> {
    compose(v).labels.into_iter().map(|l| l.text).collect()
}

const LIST_X: i32 = 6;
const LIST_Y: i32 = 34;
const LIST_W: i32 = 190;
const LIST_ROWS: usize = 11;
const SIDE_X: i32 = 202;
const SIDE_W: i32 = 112;
const PANEL_H: i32 = LIST_ROWS as i32 * LINE + 8;
/// Characters per side-panel line (the dialog font averages ~6 px).
const SIDE_CHARS: usize = 17;

fn compose(v: &ItemScreenView) -> Canvas {
    let mut c = Canvas::default();
    c.rect(0, 0, STAGE_W, STAGE_H, ink::BACKDROP);
    header(&mut c, &v.title, &format!("{} G", v.gold));
    // Tabs.
    let mut x = LIST_X;
    for (i, t) in v.tabs.iter().enumerate() {
        let w = 8 + t.len() as i32 * 7;
        if i == v.tab {
            c.panel(x, 19, w, 13, ink::SELECT, ink::HIGHLIGHT);
            c.text(t.as_str(), x + 4, 18, pen::GOLD);
        } else {
            c.text(t.as_str(), x + 4, 18, pen::DIM);
        }
        x += w + 6;
    }
    // The list.
    c.panel(LIST_X, LIST_Y, LIST_W, PANEL_H, ink::PANEL, ink::PANEL_EDGE);
    if v.rows.is_empty() {
        c.text(v.empty.as_str(), LIST_X + 8, LIST_Y + 4, pen::DIM);
    }
    let top = v.cursor.saturating_sub(LIST_ROWS - 1);
    for (i, r) in v.rows.iter().enumerate().skip(top).take(LIST_ROWS) {
        let y = LIST_Y + 4 + (i - top) as i32 * LINE;
        let selected = i == v.cursor;
        if selected {
            c.rect(LIST_X + 2, y, LIST_W - 4, LINE, ink::SELECT);
            c.text(">", LIST_X + 4, y - 1, pen::GOLD);
        }
        let worn = r.sub.starts_with("E ");
        let ink_ = if r.dim {
            pen::DIM
        } else if worn {
            pen::GREEN
        } else {
            pen::WHITE
        };
        c.text(r.name.as_str(), LIST_X + 14, y - 1, ink_);
        c.text_right(r.right.as_str(), LIST_X + LIST_W - 6, y - 1, ink_);
    }
    if v.rows.len() > LIST_ROWS {
        c.text_right(
            format!("{}/{}", v.cursor + 1, v.rows.len()),
            LIST_X + LIST_W - 4,
            LIST_Y + PANEL_H,
            pen::DIM,
        );
    }
    // The side panel: the compare while wearing, else the detail.
    c.panel(SIDE_X, LIST_Y, SIDE_W, PANEL_H, ink::PANEL, ink::PANEL_EDGE);
    let mut y = LIST_Y + 3;
    if let Some(cmp) = &v.compare {
        for l in wrap(&cmp.heading, SIDE_CHARS) {
            c.text(l, SIDE_X + 5, y, pen::GOLD);
            y += LINE;
        }
        y += 2;
        for (label, now, after) in &cmp.rows {
            let ink_ = match after.cmp(now) {
                std::cmp::Ordering::Greater => pen::GREEN,
                std::cmp::Ordering::Less => pen::RED,
                std::cmp::Ordering::Equal => pen::WHITE,
            };
            c.text(label.as_str(), SIDE_X + 5, y, pen::DIM);
            c.text_right(format!("{now}"), SIDE_X + 62, y, pen::WHITE);
            c.text(">", SIDE_X + 67, y, pen::DIM);
            c.text_right(format!("{after}"), SIDE_X + SIDE_W - 6, y, ink_);
            y += LINE;
        }
    } else if let Some(r) = v.rows.get(v.cursor) {
        if !r.sub.is_empty() {
            c.text(r.sub.as_str(), SIDE_X + 5, y, pen::GOLD);
            y += LINE;
        }
        for d in &v.detail {
            for l in wrap(d, SIDE_CHARS) {
                if y > LIST_Y + PANEL_H - LINE {
                    break;
                }
                c.text(l, SIDE_X + 5, y, pen::WHITE);
                y += LINE;
            }
        }
    }
    // Who uses or wears it, over the list.
    if let Some((members, cursor)) = &v.members {
        let h = members.len() as i32 * (2 * LINE) + 8;
        let (mx, my) = (LIST_X + 10, LIST_Y + 16);
        c.panel(mx, my, LIST_W - 20, h, [16, 22, 36, 255], ink::HIGHLIGHT);
        for (i, m) in members.iter().enumerate() {
            let y = my + 4 + i as i32 * 2 * LINE;
            if i == *cursor {
                c.rect(mx + 2, y, LIST_W - 24, 2 * LINE, ink::SELECT);
                c.text(">", mx + 4, y - 1, pen::GOLD);
            }
            let ink_ = if m.dim { pen::DIM } else { pen::WHITE };
            c.text(m.name.as_str(), mx + 14, y - 1, ink_);
            c.text(m.line.as_str(), mx + 22, y - 1 + LINE, pen::DIM);
        }
    }
    // How many.
    if let Some((qty, total)) = v.quantity {
        let (w, h) = (170, 34);
        let (qx, qy) = ((STAGE_W - w) / 2, 92);
        c.panel(qx, qy, w, h, [16, 22, 36, 255], ink::HIGHLIGHT);
        c.text(format!("Quantity  < {qty} >"), qx + 8, qy + 3, pen::WHITE);
        c.text(format!("Total {total} G"), qx + 8, qy + 3 + LINE, pen::GOLD);
    }
    if let Some(m) = &v.message {
        c.rect(0, STAGE_H - 31, STAGE_W, 16, [10, 14, 24, 220]);
        c.text(m.as_str(), 6, STAGE_H - 31, pen::GOLD);
    }
    footer(&mut c, &v.footer);
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> ItemScreenView {
        ItemScreenView {
            title: "Tower Armory".into(),
            gold: 48210,
            tabs: vec!["Buy".into(), "Sell".into()],
            rows: vec![
                ItemRowView {
                    name: "Rebirth Blade".into(),
                    right: "64000 G".into(),
                    sub: "Have 0".into(),
                    dim: true,
                },
                ItemRowView {
                    name: "Healing Berry".into(),
                    right: "3000 G".into(),
                    ..Default::default()
                },
            ],
            detail: vec![
                "Forged in the tower's heart.".into(),
                "Weapon  ATK +108".into(),
            ],
            footer: "X: choose".into(),
            ..Default::default()
        }
    }

    #[test]
    fn the_counter_lists_rows_and_the_detail() {
        let s = item_screen_strings(&view());
        for want in [
            "Tower Armory",
            "48210 G",
            "Buy",
            "Rebirth Blade",
            "64000 G",
            "Have 0",
        ] {
            assert!(s.iter().any(|l| l == want), "{want} in {s:?}");
        }
        assert!(s.iter().any(|l| l == "Forged in the"), "wrapped: {s:?}");
    }

    #[test]
    fn the_compare_replaces_the_detail() {
        let mut v = view();
        v.compare = Some(ItemCompareView {
            heading: "Vahn: equip".into(),
            rows: vec![("ATK".into(), 180, 288)],
        });
        v.members = Some((
            vec![ItemMemberView {
                name: "Vahn".into(),
                line: "Weapon: Chaos Breaker".into(),
                dim: false,
            }],
            0,
        ));
        let s = item_screen_strings(&v);
        assert!(s.iter().any(|l| l == "288"));
        assert!(s.iter().any(|l| l == "Weapon: Chaos Breaker"));
        assert!(!s.iter().any(|l| l == "Have 0"), "the detail gives way");
        assert!(!item_screen_prims(&v).is_empty());
    }

    #[test]
    fn wrap_breaks_on_words() {
        assert_eq!(wrap("one two three four", 9), ["one two", "three", "four"]);
    }
}

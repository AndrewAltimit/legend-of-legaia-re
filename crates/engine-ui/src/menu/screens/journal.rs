use crate::*;

/// One objective under a selected [`JournalRowView`].
pub struct JournalObjectiveView<'a> {
    pub text: &'a str,
    pub progress: u32,
    pub target: u32,
}

/// One row of a Journal page: a quest, a challenge or a map.
pub struct JournalRowView<'a> {
    pub title: &'a str,
    /// The short tag at the row's right edge (`Active`, `Done`, `Best 1200`).
    pub tag: &'a str,
    /// The row is finished (a completed quest, a cleared challenge): its
    /// tag reads green.
    pub done: bool,
    /// Shown under the row while it is selected, wrapped to two lines.
    pub detail: &'a str,
    /// Shown under the selected row, one line each.
    pub objectives: &'a [JournalObjectiveView<'a>],
    /// Shown last under the selected row (`Reward: 100 G`); empty for none.
    pub reward: &'a str,
}

/// Bundle for [`journal_draws_for`].
pub struct JournalDrawArgs<'a> {
    /// `JOURNAL - <pack name> <version>`.
    pub heading: &'a str,
    pub tabs: &'a [&'a str],
    /// The selected tab.
    pub tab: usize,
    /// Owned trading cards the binder has not shown (shown when non-zero).
    pub cards_unseen: usize,
    pub rows: &'a [JournalRowView<'a>],
    pub cursor: usize,
    /// What an empty page says.
    pub empty: &'a str,
    /// The key line at the bottom.
    pub footer: &'a str,
}

/// The stage height the Journal's lines fill (the generic sub-window's
/// content height, less the heading, tab and footer lines).
const JOURNAL_BODY_LINES: usize = 11;
const JOURNAL_LINE_H: i32 = 13;
/// The width of the generic sub-window's content, in stage pixels.
const JOURNAL_WIDTH: i32 = 272;

/// Build [`TextDraw`]s for the expansion pack's Journal, inside the generic
/// sub-window frame (an engine extension; no retail screen draws it).
///
/// ```text
/// JOURNAL - Legend of Legaia: Rebirth 0.1.0
/// Quests  Challenges  Cards  Travel     New cards: 1
/// > The Rebirth Tower                           Active
///     The guide will open the tower to a party that
///     proves itself in battle and grows strong ...
///     Win three battles                            2/3
///     Reach level 40                              0/1
///     Reward: 10000 G
///   A Tower Wakes                                 Done
/// Left/Right: page   Up/Down: select   Circle: back
/// ```
///
/// Only the selected row expands; the list scrolls to keep it in view.
pub fn journal_draws_for(
    font: &legaia_font::Font,
    args: &JournalDrawArgs<'_>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let dim: [f32; 4] = [0.6, 0.6, 0.6, 1.0];
    let green: [f32; 4] = [0.5, 1.0, 0.5, 1.0];
    let width = |s: &str| font.layout_ascii(s).advance_x as i32;
    let fit = |s: &str, max: i32| -> String {
        if width(s) <= max {
            return s.to_string();
        }
        let mut out: String = s.to_string();
        while !out.is_empty() && width(&format!("{out}...")) > max {
            out.pop();
        }
        format!("{}...", out.trim_end())
    };
    let mut out = Vec::new();
    let mut put = |text: &str, x: i32, y: i32, color: [f32; 4]| {
        out.extend(text_draws_for(&font.layout_ascii(text), (x, y), color));
    };

    put(&fit(args.heading, JOURNAL_WIDTH), pen.0, pen.1, gold);
    let tabs_y = pen.1 + JOURNAL_LINE_H;
    let mut x = pen.0;
    for (i, t) in args.tabs.iter().enumerate() {
        put(t, x, tabs_y, if i == args.tab { gold } else { dim });
        x += width(t) + 12;
    }
    if args.cards_unseen > 0 {
        let s = format!("New cards: {}", args.cards_unseen);
        put(&s, pen.0 + JOURNAL_WIDTH - width(&s), tabs_y, white);
    }

    // The body as lines: (indent, text, right-aligned tag, ink, tag ink).
    struct Line {
        indent: i32,
        text: String,
        tag: String,
        ink: [f32; 4],
        tag_ink: [f32; 4],
    }
    let mut lines: Vec<Line> = Vec::new();
    let mut selected = (0, 0);
    for (i, row) in args.rows.iter().enumerate() {
        let sel = i == args.cursor;
        let start = lines.len();
        lines.push(Line {
            indent: 0,
            text: format!("{}{}", if sel { "> " } else { "  " }, row.title),
            tag: row.tag.to_string(),
            ink: if sel { gold } else { white },
            tag_ink: if row.done { green } else { dim },
        });
        if sel {
            let wrapped = font.wrap_bytes(row.detail.as_bytes(), (JOURNAL_WIDTH - 16) as u32);
            for l in wrapped
                .split(|&b| b == 0x7C)
                .filter(|l| !l.is_empty())
                .take(2)
            {
                lines.push(Line {
                    indent: 16,
                    text: String::from_utf8_lossy(l).into_owned(),
                    tag: String::new(),
                    ink: dim,
                    tag_ink: dim,
                });
            }
            for o in row.objectives {
                let done = o.progress >= o.target;
                lines.push(Line {
                    indent: 16,
                    text: o.text.to_string(),
                    tag: format!("{}/{}", o.progress, o.target),
                    ink: if done { green } else { white },
                    tag_ink: if done { green } else { white },
                });
            }
            if !row.reward.is_empty() {
                lines.push(Line {
                    indent: 16,
                    text: format!("Reward: {}", row.reward),
                    tag: String::new(),
                    ink: dim,
                    tag_ink: dim,
                });
            }
            selected = (start, lines.len());
        }
    }
    if lines.is_empty() {
        lines.push(Line {
            indent: 0,
            text: args.empty.to_string(),
            tag: String::new(),
            ink: dim,
            tag_ink: dim,
        });
    }
    // Scroll so the selected block is in view, its head first.
    let top = selected
        .1
        .saturating_sub(JOURNAL_BODY_LINES)
        .min(selected.0)
        .min(lines.len().saturating_sub(JOURNAL_BODY_LINES));
    let body_y = tabs_y + JOURNAL_LINE_H + 4;
    for (k, l) in lines.iter().skip(top).take(JOURNAL_BODY_LINES).enumerate() {
        let y = body_y + k as i32 * JOURNAL_LINE_H;
        let tag_w = if l.tag.is_empty() {
            0
        } else {
            width(&l.tag) + 8
        };
        let text = fit(&l.text, JOURNAL_WIDTH - l.indent - tag_w);
        put(&text, pen.0 + l.indent, y, l.ink);
        if !l.tag.is_empty() {
            put(&l.tag, pen.0 + JOURNAL_WIDTH - width(&l.tag), y, l.tag_ink);
        }
    }
    let foot_y = body_y + JOURNAL_BODY_LINES as i32 * JOURNAL_LINE_H + 4;
    put(&fit(args.footer, JOURNAL_WIDTH), pen.0, foot_y, dim);
    out
}

/// The stage pen [`journal_draws_for`] lays out from, inside the generic
/// sub-window frame.
pub const JOURNAL_PEN: (i32, i32) = (24, 22);

/// Draws for the one quest toast on screen: a single line centred along
/// the stage's top edge, under the place-name banner's row.
pub fn quest_toast_draws_for(font: &legaia_font::Font, text: &str) -> Vec<TextDraw> {
    const STAGE_W: i32 = 320;
    let layout = font.layout_ascii(text);
    let x = ((STAGE_W - layout.advance_x as i32) / 2).max(4);
    let shadow = text_draws_for(&layout, (x + 1, 25), [0.0, 0.0, 0.0, 0.85]);
    let mut out = shadow;
    out.extend(text_draws_for(&layout, (x, 24), [1.0, 0.9, 0.45, 1.0]));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_selected_row_expands_and_stays_in_view() {
        let font = legaia_font::Font::placeholder();
        let objectives = [JournalObjectiveView {
            text: "Win three battles",
            progress: 2,
            target: 3,
        }];
        let rows: Vec<JournalRowView<'_>> = (0..20)
            .map(|_| JournalRowView {
                title: "Quest",
                tag: "Active",
                done: false,
                detail: "",
                objectives: &objectives,
                reward: "10 G",
            })
            .collect();
        let args = JournalDrawArgs {
            heading: "JOURNAL",
            tabs: &["Quests", "Challenges", "Cards", "Travel"],
            tab: 0,
            cards_unseen: 2,
            rows: &rows,
            cursor: 19,
            empty: "Nothing yet",
            footer: "Circle: back",
        };
        let draws = journal_draws_for(&font, &args, JOURNAL_PEN);
        assert!(!draws.is_empty());
        let max_y = draws
            .iter()
            .map(|d| d.dst.1 + d.dst.3 as i32)
            .max()
            .unwrap();
        assert!(max_y <= 240, "the Journal fits the stage ({max_y})");
    }
}

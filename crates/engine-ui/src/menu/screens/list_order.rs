use crate::*;

/// One row of the list-reorder page.
pub struct ListOrderRowView<'a> {
    pub label: &'a str,
    /// `true` while this row is the one a confirm latched, waiting for its
    /// exchange partner.
    pub latched: bool,
}

/// Inputs for [`list_order_draws_for`] - the per-character list page of the
/// record screen (`FUN_801DA2A0`'s browse half).
pub struct ListOrderDrawArgs<'a> {
    pub title: &'a str,
    /// Every row of the list, in record order. The builder shows the page
    /// starting at `scroll_top`.
    pub rows: &'a [ListOrderRowView<'a>],
    pub cursor: usize,
    pub scroll_top: usize,
    /// Rows the page shows at once (retail's window is seven).
    pub page_rows: usize,
    /// `true` while a confirm on this page exchanges rows rather than just
    /// cueing - only the spell list's step does.
    pub reorderable: bool,
}

/// Build the list-reorder page.
///
/// The page is a window of `page_rows` rows with the hand on `cursor`; a
/// latched row keeps a mark while its exchange partner is picked, and the
/// footer says which of the two things a confirm will do, because the
/// screen has no other way to show that a press is now a swap.
pub fn list_order_draws_for(
    font: &legaia_font::Font,
    args: ListOrderDrawArgs<'_>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 14;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let dim: [f32; 4] = [0.55, 0.55, 0.55, 1.0];
    let mut out = text_draws_for(&font.layout_ascii(args.title), pen, gold);

    let end = (args.scroll_top + args.page_rows).min(args.rows.len());
    for (i, row) in args.rows[args.scroll_top.min(args.rows.len())..end]
        .iter()
        .enumerate()
    {
        let index = args.scroll_top + i;
        let y = pen.1 + LINE_H + i as i32 * LINE_H;
        if index == args.cursor {
            out.extend(text_draws_for(&font.layout_ascii(">"), (pen.0, y), gold));
        }
        let ink = if row.latched { gold } else { white };
        out.extend(text_draws_for(
            &font.layout_ascii(row.label),
            (pen.0 + 16, y),
            ink,
        ));
        if row.latched {
            out.extend(text_draws_for(
                &font.layout_ascii("<>"),
                (pen.0 + 128, y),
                gold,
            ));
        }
    }

    // Scroll marks: the page carries no scrollbar, so say when there is more
    // list above or below rather than leaving the window looking complete.
    if args.scroll_top > 0 {
        out.extend(text_draws_for(
            &font.layout_ascii("^"),
            (pen.0 + 144, pen.1 + LINE_H),
            dim,
        ));
    }
    if end < args.rows.len() {
        out.extend(text_draws_for(
            &font.layout_ascii("v"),
            (
                pen.0 + 144,
                pen.1 + LINE_H + (args.page_rows as i32 - 1) * LINE_H,
            ),
            dim,
        ));
    }

    let footer = if !args.reorderable {
        "BROWSE"
    } else if args.rows.iter().any(|r| r.latched) {
        "SWAP WITH"
    } else {
        "PICK TO MOVE"
    };
    out.extend(text_draws_for(
        &font.layout_ascii(footer),
        (pen.0, pen.1 + LINE_H + (args.page_rows as i32 + 1) * LINE_H),
        dim,
    ));
    out
}

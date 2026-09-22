//! The fishing **point-exchange sub-screen** - one composition, both hosts.
//!
//! The venue's prize counter is its own screen over the pond: retail frames
//! it with the overlay's menu-picker rect (`FUN_801d74b0`, centre `(0xA0,
//! 0x50)`, `0x68 x 0x50`) swaying on the idle sway triple (`FUN_801d03b0`),
//! and lists the rows the venue's page decodes with the unaffordable ones
//! greyed.
//!
//! The native window built that list inline in its HUD pass and the browser
//! play page shipped the rows as a JSON side-channel its own script drew, so
//! the screen existed on one host and a data feed on the other. What decides
//! a row's ink and its tag is the same three-way refusal on both - price,
//! the owned-stack cap and the one-time latch - and that is what lives here.
//!
//! The latch is the trap this module keeps separated: `is_available` folds
//! all three refusals together, so reading it as "already bought" prints
//! `sold` beside every prize a fresh save cannot yet afford. The tag reads
//! the latch on its own.

use crate::*;

/// One prize row, resolved for display.
pub struct ExchangeRowView<'a> {
    /// Item name, or the host's `item 0xNN` stand-in.
    pub name: &'a str,
    /// Price in fishing points.
    pub price: u32,
    /// How many of this item the party already holds.
    pub owned: u32,
    /// The row can be taken right now (price, stack cap and latch all pass).
    pub available: bool,
    /// The row is a one-time prize.
    pub one_time: bool,
    /// The one-time bit is **latched** - this prize has been taken.
    pub latched: bool,
}

/// Everything the sub-screen draws.
pub struct ExchangeView<'a> {
    /// `0` Buma, `1` Vidna.
    pub venue: u8,
    /// The live point pool.
    pub points: i32,
    /// Row the cursor sits on.
    pub cursor: usize,
    /// First row the venue's own scroll rule puts on screen.
    pub first_visible: usize,
    pub rows: &'a [ExchangeRowView<'a>],
}

/// Venue label for a venue index.
pub fn venue_name(venue: u8) -> &'static str {
    if venue == 0 { "Buma" } else { "Vidna" }
}

/// Vertical pitch between rows, in stage pixels.
pub const EXCHANGE_ROW_PITCH: i32 = 18;
/// Drop from the panel's top-left to the first row.
pub const EXCHANGE_LIST_TOP_DY: i32 = 18;

/// The tag printed at the end of a row: what kind of prize it is, and
/// whether it is gone.
///
/// `one_time && latched` is the only spelling of "sold": a one-time prize
/// the player cannot yet afford is still `one-time`, not `sold`.
pub fn exchange_row_tag(one_time: bool, latched: bool) -> &'static str {
    match (one_time, latched) {
        (true, true) => "sold",
        (true, false) => "one-time",
        (false, _) => "each",
    }
}

/// The screen's header line.
pub fn exchange_header_line(venue: u8, points: i32, prompt: &str) -> String {
    format!(
        "PRIZE EXCHANGE ({})  points {points}{prompt}",
        venue_name(venue)
    )
}

/// One row's line, at the column layout both hosts print.
pub fn exchange_row_line(row: &ExchangeRowView<'_>, on_cursor: bool) -> String {
    let cursor = if on_cursor { ">" } else { " " };
    let tag = exchange_row_tag(row.one_time, row.latched);
    let (name, price, owned) = (row.name, row.price, row.owned);
    format!("{cursor} {name:<18} {price:>6} pts  {tag}  (own {owned})")
}

/// Compose the whole sub-screen into [`TextDraw`]s at `pen`, the panel's
/// resolved top-left (sway already applied by the caller, which is the host
/// that holds the overlay's sway triple).
///
/// `prompt` is the host's own input legend - the two hosts bind different
/// keys - and is appended to the header rather than given a line of its own,
/// which is where both hosts already put it.
pub fn exchange_screen_draws_for(
    font: &legaia_font::Font,
    view: &ExchangeView<'_>,
    prompt: &str,
    pen: (i32, i32),
    ink: [f32; 4],
    dim: [f32; 4],
) -> Vec<TextDraw> {
    let mut out: Vec<TextDraw> = Vec::new();
    let head = exchange_header_line(view.venue, view.points, prompt);
    let layout = font.layout_ascii(&head);
    push_layout(&mut out, &layout, pen, ink);
    for (i, r) in view.rows.iter().enumerate().skip(view.first_visible) {
        let line = exchange_row_line(r, i == view.cursor);
        let layout = font.layout_ascii(&line);
        let y = pen.1 + EXCHANGE_LIST_TOP_DY + EXCHANGE_ROW_PITCH * (i - view.first_visible) as i32;
        push_layout(
            &mut out,
            &layout,
            (pen.0, y),
            if r.available { ink } else { dim },
        );
    }
    out
}

fn push_layout(
    out: &mut Vec<TextDraw>,
    layout: &legaia_font::Layout,
    pen: (i32, i32),
    color: [f32; 4],
) {
    for g in &layout.glyphs {
        out.push(TextDraw {
            dst: (pen.0 + g.dst_x, pen.1 + g.dst_y, g.width, g.height),
            src: (g.atlas_x, g.atlas_y, g.width, g.height),
            color,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(one_time: bool, latched: bool, available: bool) -> ExchangeRowView<'static> {
        ExchangeRowView {
            name: "Bait",
            price: 100,
            owned: 0,
            available,
            one_time,
            latched,
        }
    }

    #[test]
    fn an_unaffordable_one_time_prize_is_not_sold() {
        // The failure this module exists to stop: `available` is false for
        // three different reasons, and only the latch means "gone".
        assert_eq!(exchange_row_tag(true, false), "one-time");
        assert_eq!(exchange_row_tag(true, true), "sold");
        assert_eq!(exchange_row_tag(false, true), "each");
    }

    #[test]
    fn the_cursor_marks_exactly_one_row() {
        let rows = [row(false, false, true), row(false, false, true)];
        let a = exchange_row_line(&rows[0], true);
        let b = exchange_row_line(&rows[1], false);
        assert!(a.starts_with('>'));
        assert!(b.starts_with(' '));
    }

    #[test]
    fn rows_below_first_visible_are_not_drawn_and_the_rest_step_by_the_pitch() {
        let font = legaia_font::synthetic_for_tests();
        let rows = [
            row(false, false, true),
            row(false, false, true),
            row(false, false, true),
        ];
        let view = ExchangeView {
            venue: 1,
            points: 500,
            cursor: 2,
            first_visible: 1,
            rows: &rows,
        };
        let out = exchange_screen_draws_for(&font, &view, "", (10, 20), [1.0; 4], [0.5; 4]);
        let ys: Vec<i32> = out.iter().map(|d| d.dst.1).collect();
        let tops: std::collections::BTreeSet<i32> = ys.into_iter().collect();
        // Header + two rows, the skipped row absent.
        assert!(tops.contains(&(20 + EXCHANGE_LIST_TOP_DY)));
        assert!(tops.contains(&(20 + EXCHANGE_LIST_TOP_DY + EXCHANGE_ROW_PITCH)));
        assert!(!tops.contains(&(20 + EXCHANGE_LIST_TOP_DY + 2 * EXCHANGE_ROW_PITCH)));
    }

    #[test]
    fn an_unavailable_row_takes_the_dim_ink() {
        let font = legaia_font::synthetic_for_tests();
        let rows = [row(false, false, false)];
        let view = ExchangeView {
            venue: 0,
            points: 0,
            cursor: 0,
            first_visible: 0,
            rows: &rows,
        };
        let out = exchange_screen_draws_for(&font, &view, "", (0, 0), [1.0; 4], [0.25; 4]);
        let row_ink = out
            .iter()
            .filter(|d| d.dst.1 >= EXCHANGE_LIST_TOP_DY)
            .map(|d| d.color[0])
            .next()
            .unwrap();
        assert_eq!(row_ink, 0.25);
    }
}

//! The two shop **quantity windows**, composed the way both hosts compose
//! them.
//!
//! Retail's quantity screen is one number stepping in place, not a list, and
//! the window beside it prints `quantity / bound` with the running total
//! right-packed into a digit field chosen from the **unit price's**
//! magnitude. Two things this pins that a screenshot would not:
//!
//! * the value row's second number is the *bound*, not the unit price - the
//!   buy window's port printed the price there, which reads as a plausible
//!   `qty x price` line and is not what `FUN_801D5510` loads;
//! * the two windows pack their total differently. Window 35's pens are
//!   fixed and its field grows rightward; window 37 moves its pens left as
//!   the field widens, so the number's right edge stays pinned to the box.
//!
//! Both painters live in `engine-ui` and both hosts call them, so a drift
//! here would be a drift in one builder rather than between two copies.

use legaia_engine_ui::ui_menu_window_painters::{
    PainterRect, buy_quantity_draws_for, sell_quantity_draws_for, sell_total_digits,
};

/// Window 35's disc rect (`PROT 0899` file `0x15F20 + 35*0x10`).
const BUY_RECT: PainterRect = PainterRect {
    x: 138,
    y: 100,
    w: 168,
    h: 50,
};
/// Window 37's disc rect.
const SELL_RECT: PainterRect = PainterRect {
    x: 14,
    y: 46,
    w: 144,
    h: 33,
};

fn font() -> legaia_font::Font {
    legaia_font::Font::placeholder()
}

/// The leftmost x any draw in a set lands on, or `None` for an empty set.
fn min_x(draws: &[legaia_engine_ui::TextDraw]) -> Option<i32> {
    draws.iter().map(|d| d.dst.0).min()
}

/// Draws whose baseline is exactly `y`.
fn row_at(draws: &[legaia_engine_ui::TextDraw], y: i32) -> Vec<i32> {
    let mut xs: Vec<i32> = draws
        .iter()
        .filter(|d| d.dst.1 == y)
        .map(|d| d.dst.0)
        .collect();
    xs.sort_unstable();
    xs.dedup();
    xs
}

#[test]
fn the_buy_window_prints_quantity_over_its_bound() {
    let f = font();
    let (draws, pic, cur) = buy_quantity_draws_for(&f, BUY_RECT, Some(3), 7, 12, 250);
    assert!(!draws.is_empty(), "the window drew nothing");
    let row = BUY_RECT.y + 0x22;
    let xs = row_at(&draws, row);
    // A one-digit quantity right-packs into its two-cell field, so it lands
    // one cell past the pen; the separator sits on its own pen; the two-digit
    // bound fills its field from the pen.
    assert!(
        xs.contains(&(BUY_RECT.x + 0x18 + 8)),
        "the quantity is not right-packed on its pen: {xs:?}"
    );
    assert!(
        xs.contains(&(BUY_RECT.x + 0x28)),
        "no separator glyph between the two numbers: {xs:?}"
    );
    assert!(
        xs.contains(&(BUY_RECT.x + 0x30)) && xs.contains(&(BUY_RECT.x + 0x38)),
        "the bound does not fill its two-digit field: {xs:?}"
    );
    // The bound is what prints there, not the unit price: a one-digit bound
    // beside a three-digit price leaves the field's leading cell empty, which
    // a price-printing window could not do.
    let (single, _, _) = buy_quantity_draws_for(&f, BUY_RECT, Some(3), 7, 9, 250);
    let single = row_at(&single, row);
    assert!(
        single.contains(&(BUY_RECT.x + 0x38)) && !single.contains(&(BUY_RECT.x + 0x30)),
        "the second number is not the bound: {single:?}"
    );
    let pic = pic.expect("the total carries a currency pictogram");
    assert_eq!((pic.x, pic.y), (BUY_RECT.x + 0x58, BUY_RECT.y + 0x24));
    let cur = cur.expect("the stepper carries a hand");
    assert_eq!((cur.x, cur.y), (BUY_RECT.x + 4, row));
}

/// The held line is the window's one branch: a bag scan that found nothing
/// prints one string at the origin instead of a count plus a label.
#[test]
fn the_buy_window_swaps_its_held_line_on_the_none_sentinel() {
    let f = font();
    let (held, _, _) = buy_quantity_draws_for(&f, BUY_RECT, Some(3), 1, 9, 100);
    let (none, _, _) = buy_quantity_draws_for(&f, BUY_RECT, None, 1, 9, 100);
    let head = |d: &[legaia_engine_ui::TextDraw]| row_at(d, BUY_RECT.y);
    assert!(
        head(&held).iter().any(|x| *x >= BUY_RECT.x + 0x20),
        "the held count and its label sit right of the origin"
    );
    assert_eq!(
        head(&none).iter().min().copied(),
        Some(BUY_RECT.x),
        "the None line starts at the window origin"
    );
}

/// The total's field width steps on the unit price, not on the total, and
/// window 35 packs it from a **fixed** pen, so a wider field grows rightward
/// instead of shifting the number.
#[test]
fn the_buy_total_field_steps_on_the_unit_price_at_a_fixed_pen() {
    let f = font();
    assert_eq!(sell_total_digits(99), 4);
    assert_eq!(sell_total_digits(100), 5);
    assert_eq!(sell_total_digits(1_000), 6);
    assert_eq!(sell_total_digits(10_000), 7);
    let row = BUY_RECT.y + 0x22;
    // Both buys print a total whose leading digit sits two cells into the
    // field ("10" in 4 cells, "10000" in 7), so the leading cell is the same
    // x on both and only the trailing edge moves.
    let cheap = row_at(&buy_quantity_draws_for(&f, BUY_RECT, None, 1, 9, 10).0, row);
    let dear = row_at(
        &buy_quantity_draws_for(&f, BUY_RECT, None, 1, 9, 10_000).0,
        row,
    );
    let pen = BUY_RECT.x + 0x62;
    let lead = |xs: &[i32]| xs.iter().copied().filter(|x| *x >= pen).min();
    assert_eq!(
        lead(&cheap),
        lead(&dear),
        "the total's pen moved with its field: {cheap:?} vs {dear:?}"
    );
    assert!(
        dear.iter().max() > cheap.iter().max(),
        "the wider field does not reach further right: {cheap:?} vs {dear:?}"
    );
}

/// Window 37 is the mirror: its total's pen moves **left** as the field
/// widens, which is what keeps the number's right edge on the box.
#[test]
fn the_sell_total_pen_moves_left_as_its_field_widens() {
    let f = font();
    let cheap = sell_quantity_draws_for(&f, SELL_RECT, true, "How many?", 1, 9, 10);
    let dear = sell_quantity_draws_for(&f, SELL_RECT, true, "How many?", 1, 9, 10_000);
    let cheap_pic = cheap.1.expect("pictogram");
    let dear_pic = dear.1.expect("pictogram");
    assert!(
        dear_pic.x < cheap_pic.x,
        "the sell pictogram packs left with the field: {} vs {}",
        dear_pic.x,
        cheap_pic.x
    );
    assert_eq!(
        cheap_pic.x - dear_pic.x,
        (sell_total_digits(10_000) - sell_total_digits(10)) * 8,
        "the shift is exactly the extra cells"
    );
}

/// The sell window draws nothing when its caller reports no staged stack -
/// the `DAT_801E46B0 <= 0` early-out - while the buy window has no such arm
/// and always draws.
#[test]
fn the_sell_window_has_the_early_out_and_the_buy_window_does_not() {
    let f = font();
    let (text, pic, cur) = sell_quantity_draws_for(&f, SELL_RECT, false, "How many?", 1, 9, 10);
    assert!(text.is_empty() && pic.is_none() && cur.is_none());
    let (text, _, _) = buy_quantity_draws_for(&f, BUY_RECT, None, 1, 1, 0);
    assert!(min_x(&text).is_some(), "the buy window always draws");
}

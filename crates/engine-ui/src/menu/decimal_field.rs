//! The two fixed-cell decimal writers every menu number goes through:
//! blank-padded `FUN_80034B78` and zero-padded `FUN_80034E4C`, both
//! `(value, digits, x, y)`, each digit an 8x12 cell `uv = (d * 8, 0xD0)` off
//! the menu glyph page drawn by `FUN_8003C11C`.
//!
//! PORT: FUN_80034B78, FUN_80034E4C
//!
//! Both walk nine places over the `4 * 10^k` / `10^k` table at `0x80073DCC`
//! and decide each place's cell alone; neither right-aligns a string. A
//! non-zero digit always draws, at the pen, and moves it on 8 px. A zero draws
//! too once a digit has drawn (the latch `gp+0x15C`) if its place is inside
//! the field (`place <= digits`), and is skipped without a move outside it.
//! Before the first digit the blank-padded writer moves the pen 8 px for a
//! zero inside the field and draws the ones place whatever it holds; the
//! zero-padded writer raises the latch on entry, so every zero inside its
//! field draws. A value wider than its field therefore prints every non-zero
//! digit of the overflow from the pen rightwards, dropping the overflow's
//! zeros, and pushes the field's own cells right; it is never cut to the
//! field.

use crate::TextDraw;

/// The place table at `0x80073DCC`: `4 * 10^k` then `10^k`, ones last.
const PLACES: [(i32, i32); 9] = [
    (400_000_000, 100_000_000),
    (40_000_000, 10_000_000),
    (4_000_000, 1_000_000),
    (400_000, 100_000),
    (40_000, 10_000),
    (4_000, 1_000),
    (400, 100),
    (40, 10),
    (4, 1),
];

/// Pitch of a cell.
pub const DECIMAL_CELL_W: i32 = 8;

/// The cells one writer call draws, as `(dx, digit)`: the pen offset from
/// `x` and the digit whose cell (`u = digit * 8`) is drawn there. A place
/// above `10^8` can carry a "digit" past `9` (`value >= 10^9`), which retail
/// draws as whatever cell sits at that `u`.
pub fn decimal_field_cells(value: i32, digits: i32, zero_pad: bool) -> Vec<(i32, u8)> {
    let mut v = value;
    let mut latch = zero_pad;
    let mut x = 0;
    let mut out = Vec::new();
    for (i, (four, one)) in PLACES.into_iter().enumerate() {
        let place = 9 - i as i32;
        let mut d = 0u8;
        while v >= four {
            v -= four;
            d += 4;
        }
        while v >= one {
            v -= one;
            d += 1;
        }
        let draw = if d != 0 {
            true
        } else if latch {
            // `0x80034C3C` / `0x80034F14`: inside the field, or no field.
            digits >= place || digits == 0
        } else if place < 2 {
            true
        } else {
            if digits >= place {
                x += DECIMAL_CELL_W;
            }
            false
        };
        if draw {
            out.push((x, d));
            x += DECIMAL_CELL_W;
            latch = true;
        }
    }
    out
}

/// [`decimal_field_cells`] as menu numeral draws at `(x, y)`. A value past the
/// `i32` range the retail writers take saturates.
pub fn decimal_field_draws(
    font: &legaia_font::Font,
    value: i64,
    (x, y): (i32, i32),
    digits: i32,
    zero_pad: bool,
    color: [f32; 4],
) -> Vec<TextDraw> {
    let value = value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    let mut out = Vec::new();
    for (dx, d) in decimal_field_cells(value, digits, zero_pad) {
        // A cell past `9` is no numeral; the font has nothing to stand in.
        if let Some(ch) = char::from_digit(u32::from(d), 10) {
            out.extend(crate::numeral_cell_draws(font, ch, (x + dx, y), color));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(cells: &[(i32, u8)]) -> Vec<(i32, char)> {
        cells
            .iter()
            .map(|&(dx, d)| (dx / 8, char::from_digit(u32::from(d), 10).unwrap()))
            .collect()
    }

    #[test]
    fn a_value_inside_its_field_right_aligns() {
        assert_eq!(
            text(&decimal_field_cells(42, 4, false)),
            vec![(2, '4'), (3, '2')]
        );
        assert_eq!(text(&decimal_field_cells(0, 3, false)), vec![(2, '0')]);
        assert_eq!(
            text(&decimal_field_cells(7, 3, true)),
            vec![(0, '0'), (1, '0'), (2, '7')]
        );
    }

    #[test]
    fn an_overflow_prints_its_digits_and_drops_its_zeros() {
        // 10203 in two cells: the 1 and 2 above the field draw, the zero
        // between them does not, and the field's own 0 and 3 follow.
        assert_eq!(
            text(&decimal_field_cells(10_203, 2, false)),
            vec![(0, '1'), (1, '2'), (2, '0'), (3, '3')]
        );
        assert_eq!(
            text(&decimal_field_cells(1234, 2, true)),
            vec![(0, '1'), (1, '2'), (2, '3'), (3, '4')]
        );
    }

    #[test]
    fn a_negative_value_reads_as_zero() {
        assert_eq!(text(&decimal_field_cells(-5, 2, false)), vec![(1, '0')]);
        assert_eq!(
            text(&decimal_field_cells(-5, 2, true)),
            vec![(0, '0'), (1, '0')]
        );
    }
}

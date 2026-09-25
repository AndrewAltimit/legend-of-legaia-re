//! Pixel width of an encoded dialog / UI string, the way retail draws it.
//!
//! PORT: FUN_80036514, FUN_80036888
//!
//! Retail never measures the bytes a string is stored as. Every draw goes
//! through the substitution expander `FUN_80036514`, which rewrites the
//! author escapes (`0x5E X` -> `0xCE (X - 0x2D)`, `0xFF` -> `0xCF`) and
//! splices in the text behind each `0xC1..=0xC7` substitution token, and
//! only then does the single-line renderer `FUN_80036888` walk the result.
//! [`Font::measure`] follows the same two steps:
//!
//! 1. **Expand.** `0xC1..=0xC5` / `0xC7` tokens call the caller's
//!    [`MeasureOptions::expand`] hook, because what they splice in is runtime
//!    state (a party member's name, an item or spell name). A token the hook
//!    cannot resolve is recorded in [`TextMeasure::unresolved`] and adds no
//!    width, so a caller can tell a real width from a lower bound. `0xC0`
//!    and `0xC6` have no arm in retail's jump table and copy whatever the
//!    previous substitution pointed at; they are always unresolved here.
//!    A `0xC1` token *inside* an expansion is expanded once more, as retail
//!    does; no other nested token is.
//! 2. **Walk.** Each glyph byte advances `widths[c] + glyph_pad + 1`
//!    (`glyph_pad` is `DAT_800740E8`: `0` for menus and battle, `1` for the
//!    field dialog pager). `0x7C` starts a new line. `0xCF` (colour) is two
//!    bytes and adds nothing. `0xCE X` adds the escape table's advance for a
//!    string escape, and `8` px per digit for a numeric one (`FUN_80036888`
//!    steps the pen by `8` per digit after `FUN_80034B78` draws the number,
//!    not by the table's `32`).
//!
//! The walk stops at `0x00` and at any other byte below `0x20` - the MES
//! line terminators (`0x01..=0x1E`) and a `0x1F` line lead. Strip the lead
//! before measuring a `0x1F` dialog line, as the pager does (it passes the
//! row pointer `+ 1`).
//!
//! A width here is the pen advance, the number retail's own measurer
//! `FUN_80035F04` returns. The last glyph's ink ends one pixel (the fixed
//! gap) plus `glyph_pad` short of it, so comparing an advance against the
//! distance to the next column is the right test.

use crate::{EscapeTable, FIRST_CHAR, Font, NEWLINE};

/// `DAT_800740E8` value the field dialog pager (`FUN_801D84D0`) stores before
/// every row it draws (`sw v0,0x40e8(...)` with `v0 = 1`, e.g. at
/// `0x801D97D8`). Every other surface draws with `0`.
pub const DIALOG_GLYPH_PAD: u32 = 1;

/// Pen step per digit of a numeric `0xCE` escape in `FUN_80036888`.
pub const NUMERIC_DIGIT_PX: u32 = 8;

/// What a `0xCE` escape adds to the pen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscapeAdvance {
    /// A string escape: fixed advance from the escape table's `+2` byte.
    Fixed(u32),
    /// A numeric escape (`string_id == 0`): [`NUMERIC_DIGIT_PX`] per digit.
    Numeric,
}

/// The retail escape table's advance column (`0x80074050`, 38 entries),
/// expressed as the index ranges `docs/formats/dialog-font.md` tabulates.
/// `None` past index `0x25`, where retail reads whatever follows the table.
pub fn retail_escape_advance(index: u8) -> Option<EscapeAdvance> {
    Some(match index {
        0x00..=0x07 => EscapeAdvance::Fixed(16),
        0x08..=0x0A => EscapeAdvance::Fixed(12),
        0x0B..=0x0E => EscapeAdvance::Numeric,
        0x0F => EscapeAdvance::Fixed(38),
        0x10..=0x13 => EscapeAdvance::Fixed(12),
        0x14..=0x1C => EscapeAdvance::Fixed(20),
        0x1D..=0x25 => EscapeAdvance::Fixed(28),
        _ => return None,
    })
}

/// Caller hook that resolves a substitution token `(op, arg)` to the bytes
/// retail would splice in (`op` in `0xC0..=0xC7`). `None` = unknown.
pub type Expander<'a> = dyn Fn(u8, u8) -> Option<Vec<u8>> + 'a;

/// Knobs for [`Font::measure`].
pub struct MeasureOptions<'a> {
    /// `DAT_800740E8` for the surface being measured: `0` for menus and
    /// battle, [`DIALOG_GLYPH_PAD`] for the field dialog box.
    pub glyph_pad: u32,
    /// Digits assumed for a numeric `0xCE` escape. Default `4` (`32` px),
    /// which is the advance the escape table itself records.
    pub numeric_digits: u32,
    /// Escape table decoded off the disc; `None` uses
    /// [`retail_escape_advance`].
    pub escapes: Option<&'a EscapeTable>,
    /// Substitution resolver; `None` leaves every token unresolved.
    pub expand: Option<&'a Expander<'a>>,
}

impl Default for MeasureOptions<'_> {
    fn default() -> Self {
        Self {
            glyph_pad: 0,
            numeric_digits: 4,
            escapes: None,
            expand: None,
        }
    }
}

impl MeasureOptions<'_> {
    /// Options for a field dialog row (pad `1`).
    pub fn dialog() -> Self {
        Self {
            glyph_pad: DIALOG_GLYPH_PAD,
            ..Self::default()
        }
    }
}

/// Result of [`Font::measure`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextMeasure {
    /// Pen advance of each `0x7C`-separated line.
    pub line_widths: Vec<u32>,
    /// Widest line.
    pub max_px: u32,
    /// Substitution tokens `(op, arg)` the expander could not resolve. When
    /// non-empty, [`Self::max_px`] is a lower bound.
    pub unresolved: Vec<(u8, u8)>,
}

impl TextMeasure {
    /// Number of lines (`0x7C` count + 1).
    pub fn lines(&self) -> usize {
        self.line_widths.len()
    }
}

/// True for the two-byte opcode family `0xC0..=0xCF` after the author
/// aliases are folded (`FUN_80036514`'s `(b & 0xF0) == 0xC0` test).
fn is_two_byte(b: u8) -> bool {
    b & 0xF0 == 0xC0
}

/// Stage 1: the `FUN_80036514` expansion. Returns the expanded bytes and
/// records unresolved tokens.
fn expand(text: &[u8], opts: &MeasureOptions<'_>, unresolved: &mut Vec<(u8, u8)>) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let mut b = text[i];
        if b == 0 {
            break;
        }
        if b == 0x5E {
            b = 0xCE;
        } else if b == 0xFF {
            b = 0xCF;
        }
        if !is_two_byte(b) {
            out.push(b);
            i += 1;
            continue;
        }
        let Some(&raw_arg) = text.get(i + 1) else {
            break;
        };
        let arg = if text[i] == 0x5E {
            raw_arg.wrapping_sub(0x2D)
        } else {
            raw_arg
        };
        i += 2;
        match b {
            0xC1..=0xC5 | 0xC7 => {
                let Some(sub) = opts.expand.and_then(|f| f(b, arg)) else {
                    unresolved.push((b, arg));
                    continue;
                };
                let mut j = 0;
                while j < sub.len() && sub[j] != 0 {
                    if sub[j] == 0xC1 && j + 1 < sub.len() {
                        let inner = sub[j + 1];
                        match opts.expand.and_then(|f| f(0xC1, inner)) {
                            Some(name) => out.extend(name.iter().copied().take_while(|&c| c != 0)),
                            None => unresolved.push((0xC1, inner)),
                        }
                        j += 2;
                    } else {
                        out.push(sub[j]);
                        j += 1;
                    }
                }
            }
            0xC0 | 0xC6 => unresolved.push((b, arg)),
            // 0xC8..=0xCF: kept as two-byte ops for the walk.
            _ => {
                out.push(b);
                out.push(arg);
            }
        }
    }
    out
}

/// One drawn item of a measured string, at its pen position: what
/// [`Font::measure`] advanced over, for a caller that draws the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenItem {
    /// A glyph byte (after substitution) at pen `x` on `line`.
    Glyph { line: u32, x: u32, byte: u8 },
    /// A `0xCE` escape occupying `width` px from pen `x` on `line`.
    Escape { line: u32, x: u32, width: u32 },
}

impl Font {
    /// Pixel width of an encoded string as retail draws it. See the module
    /// docs for the exact rules.
    pub fn measure(&self, text: &[u8], opts: &MeasureOptions<'_>) -> TextMeasure {
        self.walk(text, opts, |_| {})
    }

    /// [`Self::measure`] plus every glyph and escape it advanced over, at
    /// the pen position the measure used - so a preview drawn from these
    /// items is exactly as wide as the measured width.
    pub fn pen_items(&self, text: &[u8], opts: &MeasureOptions<'_>) -> (Vec<PenItem>, TextMeasure) {
        let mut items = Vec::new();
        let m = self.walk(text, opts, |it| items.push(it));
        (items, m)
    }

    fn walk(
        &self,
        text: &[u8],
        opts: &MeasureOptions<'_>,
        mut emit: impl FnMut(PenItem),
    ) -> TextMeasure {
        let mut unresolved = Vec::new();
        let expanded = expand(text, opts, &mut unresolved);
        let mut line_widths = Vec::new();
        let mut pen: u32 = 0;
        let mut i = 0;
        while i < expanded.len() {
            let c = expanded[i];
            if c < FIRST_CHAR {
                break;
            }
            if c == NEWLINE {
                line_widths.push(pen);
                pen = 0;
                i += 1;
                continue;
            }
            if is_two_byte(c) {
                let arg = expanded.get(i + 1).copied().unwrap_or(0);
                if c == 0xCE {
                    let w = self.escape_px(arg, opts);
                    emit(PenItem::Escape {
                        line: line_widths.len() as u32,
                        x: pen,
                        width: w,
                    });
                    pen = pen.saturating_add(w);
                }
                i += 2;
                continue;
            }
            emit(PenItem::Glyph {
                line: line_widths.len() as u32,
                x: pen,
                byte: c,
            });
            pen = pen
                .saturating_add(self.advance_of(c))
                .saturating_add(opts.glyph_pad);
            i += 1;
        }
        line_widths.push(pen);
        let max_px = line_widths.iter().copied().max().unwrap_or(0);
        TextMeasure {
            line_widths,
            max_px,
            unresolved,
        }
    }

    fn escape_px(&self, index: u8, opts: &MeasureOptions<'_>) -> u32 {
        let adv = match opts.escapes.and_then(|t| t.entry(index)) {
            Some(e) if e.string_id == 0 => Some(EscapeAdvance::Numeric),
            Some(e) => Some(EscapeAdvance::Fixed(e.advance_px as u32)),
            None => retail_escape_advance(index),
        };
        match adv {
            Some(EscapeAdvance::Fixed(px)) => px,
            Some(EscapeAdvance::Numeric) => NUMERIC_DIGIT_PX.saturating_mul(opts.numeric_digits),
            None => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synthetic_for_tests;

    fn plain(font: &Font, s: &[u8]) -> u32 {
        s.iter().map(|&c| font.advance_of(c)).sum()
    }

    #[test]
    fn pen_items_match_the_measure() {
        let f = synthetic_for_tests();
        let text = [b'a', b'b', 0xCE, 0x10, b'|', b'c'];
        let opts = MeasureOptions::dialog();
        let (items, m) = f.pen_items(&text, &opts);
        assert_eq!(m, f.measure(&text, &opts));
        assert_eq!(items.len(), 4);
        assert_eq!(
            items[0],
            PenItem::Glyph {
                line: 0,
                x: 0,
                byte: b'a'
            }
        );
        let bx = f.advance_of(b'a') + 1;
        assert_eq!(
            items[1],
            PenItem::Glyph {
                line: 0,
                x: bx,
                byte: b'b'
            }
        );
        assert!(matches!(
            items[2],
            PenItem::Escape {
                line: 0,
                width: 12,
                ..
            }
        ));
        assert_eq!(
            items[3],
            PenItem::Glyph {
                line: 1,
                x: 0,
                byte: b'c'
            }
        );
    }

    #[test]
    fn plain_line_is_sum_of_advances() {
        let f = synthetic_for_tests();
        let m = f.measure(b"abc de", &MeasureOptions::default());
        assert_eq!(m.max_px, plain(&f, b"abc de"));
        assert_eq!(m.lines(), 1);
        assert!(m.unresolved.is_empty());
    }

    #[test]
    fn dialog_pad_adds_one_per_glyph_including_spaces() {
        let f = synthetic_for_tests();
        let base = f.measure(b"ab cd", &MeasureOptions::default()).max_px;
        let dlg = f.measure(b"ab cd", &MeasureOptions::dialog()).max_px;
        assert_eq!(dlg, base + 5);
    }

    #[test]
    fn newline_splits_and_max_is_widest() {
        let f = synthetic_for_tests();
        let m = f.measure(b"ab|abcd|a", &MeasureOptions::default());
        assert_eq!(m.lines(), 3);
        assert_eq!(m.max_px, plain(&f, b"abcd"));
        assert_eq!(m.line_widths[2], plain(&f, b"a"));
    }

    #[test]
    fn colour_op_is_free_and_ff_alias_too() {
        let f = synthetic_for_tests();
        let want = plain(&f, b"ab");
        let opts = MeasureOptions::default();
        assert_eq!(f.measure(&[b'a', 0xCF, 0x03, b'b'], &opts).max_px, want);
        assert_eq!(f.measure(&[b'a', 0xFF, 0x03, b'b'], &opts).max_px, want);
    }

    #[test]
    fn string_escape_uses_table_advance_and_caret_alias() {
        let f = synthetic_for_tests();
        let opts = MeasureOptions::default();
        // 0xCE 0x0F = the 38-px escape.
        assert_eq!(f.measure(&[0xCE, 0x0F], &opts).max_px, 38);
        // `^X` is 0xCE (X - 0x2D): 0x0F + 0x2D = 0x3C.
        assert_eq!(f.measure(&[0x5E, 0x3C], &opts).max_px, 38);
        assert_eq!(f.measure(&[0xCE, 0x00], &opts).max_px, 16);
    }

    #[test]
    fn numeric_escape_is_eight_px_per_digit() {
        let f = synthetic_for_tests();
        let mut opts = MeasureOptions::default();
        assert_eq!(f.measure(&[0xCE, 0x0B], &opts).max_px, 32);
        opts.numeric_digits = 6;
        assert_eq!(f.measure(&[0xCE, 0x0B], &opts).max_px, 48);
    }

    #[test]
    fn substitution_expands_through_hook() {
        let f = synthetic_for_tests();
        let hook = |op: u8, arg: u8| match (op, arg) {
            (0xC1, 0) => Some(b"Xyz".to_vec()),
            (0xC2, 5) => Some(vec![b'I', 0xC1, 0x00, b'!']),
            _ => None,
        };
        let opts = MeasureOptions {
            expand: Some(&hook),
            ..MeasureOptions::default()
        };
        let m = f.measure(&[b'a', 0xC1, 0x00, b'b'], &opts);
        assert_eq!(m.max_px, plain(&f, b"aXyzb"));
        assert!(m.unresolved.is_empty());
        // A nested 0xC1 inside an expansion is expanded once more.
        let m = f.measure(&[0xC2, 0x05], &opts);
        assert_eq!(m.max_px, plain(&f, b"IXyz!"));
    }

    #[test]
    fn unresolved_tokens_are_reported_and_free() {
        let f = synthetic_for_tests();
        let m = f.measure(
            &[b'a', 0xC3, 0x07, 0xC6, 0x01, b'b'],
            &MeasureOptions::default(),
        );
        assert_eq!(m.max_px, plain(&f, b"ab"));
        assert_eq!(m.unresolved, vec![(0xC3, 0x07), (0xC6, 0x01)]);
    }

    #[test]
    fn stops_at_terminators_below_0x20() {
        let f = synthetic_for_tests();
        let opts = MeasureOptions::default();
        assert_eq!(f.measure(b"ab\x00cd", &opts).max_px, plain(&f, b"ab"));
        assert_eq!(f.measure(b"ab\x1Ecd", &opts).max_px, plain(&f, b"ab"));
        assert_eq!(f.measure(&[0x1F, b'a'], &opts).max_px, 0);
    }

    #[test]
    fn decoded_escape_table_overrides_builtin() {
        use crate::EscapeEntry;
        let f = synthetic_for_tests();
        let table = EscapeTable {
            entries: vec![
                EscapeEntry {
                    string_id: 7,
                    advance_px: 9,
                    y_offset: 0,
                },
                EscapeEntry {
                    string_id: 0,
                    advance_px: 32,
                    y_offset: 0,
                },
            ],
        };
        let opts = MeasureOptions {
            escapes: Some(&table),
            numeric_digits: 2,
            ..MeasureOptions::default()
        };
        assert_eq!(f.measure(&[0xCE, 0x00], &opts).max_px, 9);
        assert_eq!(f.measure(&[0xCE, 0x01], &opts).max_px, 16);
    }

    #[test]
    fn builtin_escape_table_shape() {
        assert_eq!(retail_escape_advance(0x25), Some(EscapeAdvance::Fixed(28)));
        assert_eq!(retail_escape_advance(0x0E), Some(EscapeAdvance::Numeric));
        assert_eq!(retail_escape_advance(0x26), None);
    }
}

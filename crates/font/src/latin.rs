//! The Latin accent layout: which dialog-font byte cell stands for which
//! accented character, what each one folds to in plain ASCII, and how the
//! accent font ([`crate::accent_font`]) builds its glyph.
//!
//! This is the **one** table every consumer reads - the translation codec's
//! accent fold (`legaia_patcher::translation::markup`), the importer's accent
//! pass, the accent-font patch and the browser workbench's palette and
//! one-click fixes. Nothing else keeps a byte-to-accent map.
//!
//! The layout follows the byte values the official PAL discs write in their
//! text: IBM CP437 for `0x80..=0xA8` and `0xAD`, CP850 for the accented
//! capitals CP437 lacks (`0xB5..=0xB7`, `0xD2..=0xED`). Three cells are this
//! project's own choice, because the CP850 cell collides with the dialog
//! opcode window `0xC0..=0xCF` (a byte there is a two-byte token, never a
//! glyph) or has no CP437/CP850 home at all: `ã` at `0x9B`, `Ã` at `0xD0`,
//! `Œ` at `0x9E`. `œ` at `0x9C` and `Ÿ` at `0x9F` are where the retail NTSC
//! font page already draws those two letters. See
//! `docs/formats/dialog-font.md#accented-latin-cells`.
//!
//! No glyph bytes live here - only byte values, characters and recipes.

/// A diacritic the accent font draws above (or, for the cedilla, below) a
/// base letter taken from the user's own font page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Acute,
    Grave,
    Circumflex,
    Diaeresis,
    Tilde,
    Ring,
    Cedilla,
}

/// How the accent font builds one cell's glyph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recipe {
    /// Base letter (an ASCII byte) plus a mark. A base `i` is drawn dotless.
    Accent(u8, Mark),
    /// Two ASCII letters joined with a one-column overlap (`ae`, `OE`).
    Ligature(u8, u8),
    /// An ASCII glyph turned half a turn and dropped to the descender line
    /// (`?` -> inverted question mark).
    Inverted(u8),
    /// A glyph drawn from a fill mask defined here (`#` = ink). Only for
    /// characters with no ASCII base (sharp s, degree sign).
    Drawn(&'static [&'static str]),
}

/// One cell of the layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LatinCell {
    /// The byte a string carries for this character (`{xx}` in markup).
    pub byte: u8,
    /// The character, when the byte has a settled identity. `None` for the
    /// few legacy PAL bytes whose identity is not pinned - they keep their
    /// ASCII fold but take no typed input and draw nothing.
    pub ch: Option<char>,
    /// Plain-ASCII replacement (what `--fold-accents` writes).
    pub fold: &'static str,
    /// How the accent font draws it; `None` = fold only.
    pub recipe: Option<Recipe>,
}

use Mark::*;
use Recipe::*;

const fn c(byte: u8, ch: char, fold: &'static str, recipe: Recipe) -> LatinCell {
    LatinCell {
        byte,
        ch: Some(ch),
        fold,
        recipe: Some(recipe),
    }
}

const fn f(byte: u8, ch: Option<char>, fold: &'static str) -> LatinCell {
    LatinCell {
        byte,
        ch,
        fold,
        recipe: None,
    }
}

/// Sharp s, drawn here (no ASCII base carries it). Cap height, rows 2..=10
/// of the cell like the retail capitals.
const SHARP_S: &[&str] = &[
    ".##..", "#..#.", "#..#.", "#.#..", "#..#.", "#...#", "#...#", "#.##.", "#....",
];

/// Degree sign: a small raised ring.
const DEGREE: &[&str] = &[".#.", "#.#", ".#."];

/// The layout, sorted by byte.
pub const LATIN_CELLS: &[LatinCell] = &[
    c(0x80, 'Ç', "C", Accent(b'C', Cedilla)),
    c(0x81, 'ü', "u", Accent(b'u', Diaeresis)),
    c(0x82, 'é', "e", Accent(b'e', Acute)),
    c(0x83, 'â', "a", Accent(b'a', Circumflex)),
    c(0x84, 'ä', "a", Accent(b'a', Diaeresis)),
    c(0x85, 'à', "a", Accent(b'a', Grave)),
    c(0x86, 'å', "a", Accent(b'a', Ring)),
    c(0x87, 'ç', "c", Accent(b'c', Cedilla)),
    c(0x88, 'ê', "e", Accent(b'e', Circumflex)),
    c(0x89, 'ë', "e", Accent(b'e', Diaeresis)),
    c(0x8A, 'è', "e", Accent(b'e', Grave)),
    c(0x8B, 'ï', "i", Accent(b'i', Diaeresis)),
    c(0x8C, 'î', "i", Accent(b'i', Circumflex)),
    c(0x8D, 'ì', "i", Accent(b'i', Grave)),
    c(0x8E, 'Ä', "A", Accent(b'A', Diaeresis)),
    c(0x8F, 'Å', "A", Accent(b'A', Ring)),
    c(0x90, 'É', "E", Accent(b'E', Acute)),
    c(0x91, 'æ', "ae", Ligature(b'a', b'e')),
    c(0x92, 'Æ', "AE", Ligature(b'A', b'E')),
    c(0x93, 'ô', "o", Accent(b'o', Circumflex)),
    c(0x94, 'ö', "o", Accent(b'o', Diaeresis)),
    c(0x95, 'ò', "o", Accent(b'o', Grave)),
    c(0x96, 'û', "u", Accent(b'u', Circumflex)),
    c(0x97, 'ù', "u", Accent(b'u', Grave)),
    c(0x98, 'ÿ', "y", Accent(b'y', Diaeresis)),
    c(0x99, 'Ö', "O", Accent(b'O', Diaeresis)),
    c(0x9A, 'Ü', "U", Accent(b'U', Diaeresis)),
    c(0x9B, 'ã', "a", Accent(b'a', Tilde)),
    c(0x9C, 'œ', "oe", Ligature(b'o', b'e')),
    c(0x9E, 'Œ', "OE", Ligature(b'O', b'E')),
    c(0x9F, 'Ÿ', "Y", Accent(b'Y', Diaeresis)),
    c(0xA0, 'á', "a", Accent(b'a', Acute)),
    c(0xA1, 'í', "i", Accent(b'i', Acute)),
    c(0xA2, 'ó', "o", Accent(b'o', Acute)),
    c(0xA3, 'ú', "u", Accent(b'u', Acute)),
    c(0xA4, 'ñ', "n", Accent(b'n', Tilde)),
    c(0xA5, 'Ñ', "N", Accent(b'N', Tilde)),
    // Ordinal indicators: folded, not drawn (a raised underlined letter does
    // not survive a 14x15 cell legibly).
    f(0xA6, Some('ª'), "a"),
    f(0xA7, Some('º'), "o"),
    c(0xA8, '¿', "?", Inverted(b'?')),
    c(0xAD, '¡', "!", Inverted(b'!')),
    c(0xB5, 'Á', "A", Accent(b'A', Acute)),
    c(0xB6, 'Â', "A", Accent(b'A', Circumflex)),
    c(0xB7, 'À', "A", Accent(b'A', Grave)),
    c(0xD0, 'Ã', "A", Accent(b'A', Tilde)),
    f(0xD1, None, "A"),
    c(0xD2, 'Ê', "E", Accent(b'E', Circumflex)),
    c(0xD3, 'Ë', "E", Accent(b'E', Diaeresis)),
    c(0xD4, 'È', "E", Accent(b'E', Grave)),
    f(0xD5, None, "I"),
    c(0xD6, 'Í', "I", Accent(b'I', Acute)),
    c(0xD7, 'Î', "I", Accent(b'I', Circumflex)),
    c(0xD8, 'Ï', "I", Accent(b'I', Diaeresis)),
    c(0xDE, 'Ì', "I", Accent(b'I', Grave)),
    c(0xE0, 'Ó', "O", Accent(b'O', Acute)),
    c(0xE1, 'ß', "ss", Drawn(SHARP_S)),
    c(0xE2, 'Ô', "O", Accent(b'O', Circumflex)),
    c(0xE3, 'Ò', "O", Accent(b'O', Grave)),
    c(0xE4, 'õ', "o", Accent(b'o', Tilde)),
    c(0xE5, 'Õ', "O", Accent(b'O', Tilde)),
    c(0xE9, 'Ú', "U", Accent(b'U', Acute)),
    c(0xEA, 'Û', "U", Accent(b'U', Circumflex)),
    c(0xEB, 'Ù', "U", Accent(b'U', Grave)),
    c(0xED, 'Ý', "Y", Accent(b'Y', Acute)),
    c(0xF8, '°', "o", Drawn(DEGREE)),
];

/// Latin letters with no cell in the layout: they fold to ASCII and never
/// draw (Polish, Czech, Hungarian, Turkish, Nordic...). Kept beside the
/// layout so "does this character fold" has one answer.
pub const FOLD_ONLY: &[(char, &str)] = &[
    ('ą', "a"),
    ('Ą', "A"),
    ('ć', "c"),
    ('Ć', "C"),
    ('č', "c"),
    ('Č', "C"),
    ('ď', "d"),
    ('Ď', "D"),
    ('ę', "e"),
    ('Ę', "E"),
    ('ě', "e"),
    ('Ě', "E"),
    ('ğ', "g"),
    ('Ğ', "G"),
    ('ı', "i"),
    ('İ', "I"),
    ('ł', "l"),
    ('Ł', "L"),
    ('ń', "n"),
    ('Ń', "N"),
    ('ň', "n"),
    ('Ň', "N"),
    ('ő', "o"),
    ('Ő', "O"),
    ('ø', "o"),
    ('Ø', "O"),
    ('ř', "r"),
    ('Ř', "R"),
    ('ś', "s"),
    ('Ś', "S"),
    ('š', "s"),
    ('Š', "S"),
    ('ş', "s"),
    ('Ş', "S"),
    ('ť', "t"),
    ('Ť', "T"),
    ('ů', "u"),
    ('Ů', "U"),
    ('ű', "u"),
    ('Ű', "U"),
    ('ý', "y"),
    ('ź', "z"),
    ('Ź', "Z"),
    ('ż', "z"),
    ('Ż', "Z"),
    ('ž', "z"),
    ('Ž', "Z"),
    ('ð', "d"),
    ('Ð', "D"),
    ('þ', "th"),
    ('Þ', "Th"),
    ('«', "\""),
    ('»', "\""),
];

/// The cell a byte stands for, if the layout names it.
pub fn cell_for_byte(byte: u8) -> Option<&'static LatinCell> {
    LATIN_CELLS
        .binary_search_by_key(&byte, |c| c.byte)
        .ok()
        .map(|i| &LATIN_CELLS[i])
}

/// The cell a typed character maps to (only cells with a settled identity).
pub fn cell_for_char(ch: char) -> Option<&'static LatinCell> {
    LATIN_CELLS.iter().find(|c| c.ch == Some(ch))
}

/// ASCII fold of a byte cell (`0x82` -> `e`).
pub fn fold_for_byte(byte: u8) -> Option<&'static str> {
    cell_for_byte(byte).map(|c| c.fold)
}

/// ASCII fold of a typed character: a layout cell's fold, else the
/// fold-only list. `None` = not a Latin letter this layout knows.
pub fn fold_for_char(ch: char) -> Option<&'static str> {
    cell_for_char(ch)
        .map(|c| c.fold)
        .or_else(|| FOLD_ONLY.iter().find(|(c, _)| *c == ch).map(|(_, f)| *f))
}

/// The byte a typed character encodes to under the accent font: only cells
/// the font actually draws (a recipe), so a character that would land on an
/// undrawn cell folds instead.
pub fn drawn_byte_for_char(ch: char) -> Option<u8> {
    cell_for_char(ch)
        .filter(|c| c.recipe.is_some())
        .map(|c| c.byte)
}

/// Per-language character sets for an input palette: `(code, name,
/// characters)`. Every character is either a drawn cell or folds.
pub const LANGUAGE_SETS: &[(&str, &str, &str)] = &[
    ("es", "Spanish", "áéíóúüñÁÉÍÓÚÜÑ¿¡"),
    ("pt", "Portuguese", "áâãàçéêíóôõúüÁÂÃÀÇÉÊÍÓÔÕÚ"),
    ("fr", "French", "àâæçéèêëîïôœùûüÿÀÂÆÇÉÈÊËÎÏÔŒÙÛÜŸ"),
    ("de", "German", "äöüßÄÖÜ"),
    ("it", "Italian", "àèéìíîòóùúÀÈÉÌÍÎÒÓÙÚ°"),
    ("ca", "Catalan", "àçèéíïòóúüÀÇÈÉÍÏÒÓÚÜ"),
    ("nl", "Dutch", "áäéëíïóöúüÁÄÉËÍÏÓÖÚÜ"),
    ("sv", "Swedish", "åäöéÅÄÖÉ"),
    ("pl", "Polish", "ąćęłńóśźżĄĆĘŁŃÓŚŹŻ"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sorted_unique_and_outside_the_opcode_window() {
        for w in LATIN_CELLS.windows(2) {
            assert!(w[0].byte < w[1].byte, "{:02x} {:02x}", w[0].byte, w[1].byte);
        }
        for c in LATIN_CELLS {
            assert!(c.byte >= 0x80, "{:02x}", c.byte);
            assert!(
                !(0xC0..=0xCF).contains(&c.byte) && c.byte != 0xFF,
                "{:02x} is a two-byte opcode",
                c.byte
            );
            assert!(c.fold.is_ascii() && !c.fold.is_empty());
        }
    }

    #[test]
    fn characters_are_unique_and_round_trip() {
        for c in LATIN_CELLS {
            if let Some(ch) = c.ch {
                assert_eq!(cell_for_char(ch).unwrap().byte, c.byte);
                assert!(
                    !FOLD_ONLY.iter().any(|(x, _)| *x == ch),
                    "{ch} both a cell and fold-only"
                );
            }
        }
    }

    #[test]
    fn recipes_use_ascii_bases() {
        for c in LATIN_CELLS {
            match c.recipe {
                Some(Accent(b, _)) | Some(Inverted(b)) => assert!(b.is_ascii_graphic()),
                Some(Ligature(a, b)) => assert!(a.is_ascii_alphabetic() && b.is_ascii_alphabetic()),
                Some(Drawn(rows)) => assert!(rows.iter().all(|r| r.len() <= 12)),
                None => {}
            }
        }
    }

    #[test]
    fn every_palette_character_is_known() {
        for (code, _, chars) in LANGUAGE_SETS {
            for ch in chars.chars() {
                assert!(fold_for_char(ch).is_some(), "{code}: {ch}");
            }
        }
    }

    #[test]
    fn cp437_and_cp850_anchors() {
        assert_eq!(cell_for_byte(0x82).unwrap().ch, Some('é'));
        assert_eq!(cell_for_byte(0xE1).unwrap().fold, "ss");
        assert_eq!(cell_for_byte(0xD4).unwrap().ch, Some('È'));
        assert_eq!(drawn_byte_for_char('ª'), None);
        assert_eq!(fold_for_char('ł'), Some("l"));
        assert_eq!(fold_for_char('Ж'), None);
    }
}

//! Accented and other language-specific characters in a pack: what the disc
//! can draw, what folds, and the accent-font patch that makes accents draw.
//!
//! The byte layout, the ASCII folds and the glyph recipes are one table,
//! [`legaia_font::latin`]; this module only applies it to pack text.
//!
//! A pack chooses how its accents reach the disc with its `accents:` header
//! ([`AccentMode`]):
//!
//! - `strict` (the default, an empty header): a typed accent is a per-line
//!   encode error, reported with the fold and the cell it would take;
//! - `fold`: every typed accent (and every `{xx}` accent-cell escape) is
//!   written as its plain-ASCII fold - `Epee` for `Épée`;
//! - `font`: the import also writes the accent font
//!   ([`legaia_font::accent_font`]) to the disc, and every typed accent the
//!   font draws is encoded into its cell (`é` -> `{82}`). Letters the font has
//!   no cell for still fold.
//!
//! The browser workbench and the CLI read the same header, so a pack patches
//! the same way everywhere.

use std::borrow::Cow;

use anyhow::{Context, Result};
use legaia_font::accent_font::{
    self, AccentFontState, CellDraw, FontPage, build_accent_font, cell_draw,
};
use legaia_font::latin;
use serde::{Deserialize, Serialize};

use super::markup::{self, Target};
use super::pack::LanguagePack;
use crate::disc::DiscPatcher;

/// How a pack's accents reach the disc (the pack's `accents:` header).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccentMode {
    /// Typed accents are encode errors.
    #[default]
    Strict,
    /// Accents fold to plain ASCII.
    Fold,
    /// The accent font is written and accents encode into its cells.
    Font,
}

impl AccentMode {
    /// Parse the header value (`""` / `strict`, `fold`, `font`).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "" | "strict" => Some(Self::Strict),
            "fold" => Some(Self::Fold),
            "font" => Some(Self::Font),
            _ => None,
        }
    }

    /// The header value (`""` for strict, so an untouched pack stays
    /// byte-identical when re-saved).
    pub fn header(self) -> &'static str {
        match self {
            Self::Strict => "",
            Self::Fold => "fold",
            Self::Font => "font",
        }
    }

    /// The pack's mode; an unknown header value reads as strict.
    pub fn of(pack: &LanguagePack) -> Self {
        Self::parse(&pack.accents).unwrap_or_default()
    }
}

/// What a text transform changed - counts only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct AccentStats {
    /// Characters (typed or `{xx}` cells) replaced by an ASCII fold.
    pub folded: usize,
    /// Typed characters encoded into an accent-font cell.
    pub cells: usize,
    /// Lines the transform changed.
    pub lines: usize,
}

impl AccentStats {
    pub fn merge(&mut self, o: AccentStats) {
        self.folded += o.folded;
        self.cells += o.cells;
        self.lines += o.lines;
    }
}

/// Rewrite `markup` for `mode`, returning the new text, a map from each of
/// its characters to the source character it came from (so an encode error
/// in the rewritten text points back at what the translator typed), and the
/// counts. Two-byte tokens (`{c1:00}`) are never rewritten; a bare `{xx}`
/// accent-cell escape folds under [`AccentMode::Fold`].
pub fn transform(markup: &str, mode: AccentMode) -> (String, Vec<usize>, AccentStats) {
    let chars: Vec<char> = markup.chars().collect();
    let mut out = String::with_capacity(markup.len() + 8);
    let mut map = Vec::with_capacity(chars.len() + 8);
    let mut st = AccentStats::default();
    let push = |text: &str, src: usize, out: &mut String, map: &mut Vec<usize>| {
        for c in text.chars() {
            out.push(c);
            map.push(src);
        }
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '{' {
            let rest = &chars[i + 1..];
            let bare = match rest {
                [h1, h2, '}', ..] => h1
                    .to_digit(16)
                    .zip(h2.to_digit(16))
                    .map(|(a, b)| (a * 16 + b) as u8),
                _ => None,
            };
            if let Some(b) = bare {
                let fold = (mode == AccentMode::Fold && b >= 0x80 && !markup::is_two_byte_op(b))
                    .then(|| latin::fold_for_byte(b))
                    .flatten();
                match fold {
                    Some(f) => {
                        push(f, i, &mut out, &mut map);
                        st.folded += 1;
                    }
                    None => {
                        let esc: String = chars[i..i + 4].iter().collect();
                        push(&esc, i, &mut out, &mut map);
                    }
                }
                i += 4;
                continue;
            }
            if let [_, _, ':', _, _, '}', ..] = rest {
                let tok: String = chars[i..i + 7].iter().collect();
                push(&tok, i, &mut out, &mut map);
                i += 7;
                continue;
            }
            push("{", i, &mut out, &mut map);
            i += 1;
            continue;
        }
        let replaced = if mode == AccentMode::Strict || c.is_ascii() {
            None
        } else if mode == AccentMode::Font
            && let Some(b) = latin::drawn_byte_for_char(c)
        {
            st.cells += 1;
            Some(format!("{{{b:02x}}}"))
        } else if let Some(f) = latin::fold_for_char(c) {
            st.folded += 1;
            Some(f.to_string())
        } else {
            None
        };
        match replaced {
            Some(r) => push(&r, i, &mut out, &mut map),
            None => {
                out.push(c);
                map.push(i);
            }
        }
        i += 1;
    }
    if st.folded + st.cells > 0 {
        st.lines = 1;
    }
    (out, map, st)
}

/// Fold every typed accent and every `{xx}` accent-cell escape to ASCII.
pub fn fold_text(markup: &str) -> (String, AccentStats) {
    let (s, _, st) = transform(markup, AccentMode::Fold);
    (s, st)
}

/// Encode every typed accent the accent font draws into its cell escape;
/// fold the Latin letters it has no cell for.
pub fn cells_text(markup: &str) -> (String, AccentStats) {
    let (s, _, st) = transform(markup, AccentMode::Font);
    (s, st)
}

/// `markup` as import encodes it under `mode`.
pub fn prepare_text(markup: &str, mode: AccentMode) -> (Cow<'_, str>, AccentStats) {
    if mode == AccentMode::Strict {
        return (Cow::Borrowed(markup), AccentStats::default());
    }
    let (s, _, st) = transform(markup, mode);
    (Cow::Owned(s), st)
}

/// The pack as import encodes it: every filled translation run through
/// [`prepare_text`] for the pack's own [`AccentMode`]. Borrowed unchanged for
/// a strict pack.
pub fn prepared(pack: &LanguagePack) -> (Cow<'_, LanguagePack>, AccentStats) {
    let mode = AccentMode::of(pack);
    if mode == AccentMode::Strict {
        return (Cow::Borrowed(pack), AccentStats::default());
    }
    let mut out = pack.clone();
    let mut st = AccentStats::default();
    for es in out.sections.each_mut() {
        for e in es.iter_mut().filter(|e| e.is_filled()) {
            let (t, s) = prepare_text(&e.translation, mode);
            if s.lines > 0 {
                e.translation = t.into_owned();
            }
            st.merge(s);
        }
    }
    (Cow::Owned(out), st)
}

/// Why one character of a line will not draw as typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteKind {
    /// A typed accent the disc's font does not draw; it folds, or the
    /// accent font draws it.
    Accent,
    /// A typed Latin letter with no cell even in the accent font; it folds.
    FoldOnly,
    /// A character outside the Latin layout (Cyrillic, Greek, CJK, emoji):
    /// no glyph and no fold.
    NotLatin,
    /// An `{xx}` escape whose cell has no ink on this disc: it draws blank.
    CellBlank,
    /// An `{xx}` escape whose cell has ink but a zero advance: the next
    /// letter overprints it.
    CellOverprints,
}

/// One flagged character of a line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharNote {
    /// Character index into the markup string (in `char`s).
    pub index: usize,
    /// The character, or the escape (`{82}`).
    pub fragment: String,
    pub kind: NoteKind,
    /// Its ASCII fold, when it has one.
    pub fold: Option<String>,
    /// The cell it encodes to under the accent font.
    pub cell: Option<u8>,
    /// One-line reason for a translator.
    pub reason: String,
}

/// How a disc's font draws each byte, for [`analyze`].
pub trait DrawLookup {
    fn draw(&self, byte: u8) -> CellDraw;
}

impl<F: Fn(u8) -> CellDraw> DrawLookup for F {
    fn draw(&self, byte: u8) -> CellDraw {
        self(byte)
    }
}

/// Flag every character of `markup` that will not draw as typed under
/// `mode` on a disc whose font draws bytes as `font` says (`None` = the font
/// is unknown; escapes are then not judged). Lookalike punctuation the codec
/// folds silently (smart quotes, dashes) is not flagged.
pub fn analyze(markup: &str, mode: AccentMode, font: Option<&dyn DrawLookup>) -> Vec<CharNote> {
    let chars: Vec<char> = markup.chars().collect();
    let mut notes = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '{' {
            // A bare `{xx}` single-byte escape is a glyph cell; `{xx:yy}` is a
            // control token and never judged here.
            if let [h1, h2, '}', ..] = &chars[i + 1..]
                && let (Some(a), Some(b)) = (h1.to_digit(16), h2.to_digit(16))
            {
                let byte = (a * 16 + b) as u8;
                let glyph = byte >= 0x20 && !markup::is_two_byte_op(byte);
                if glyph && let Some(font) = font {
                    let draw = if mode == AccentMode::Font
                        && latin::cell_for_byte(byte).is_some_and(|c| c.recipe.is_some())
                    {
                        CellDraw::Draws
                    } else {
                        font.draw(byte)
                    };
                    let cell = latin::cell_for_byte(byte);
                    let what = cell
                        .and_then(|c| c.ch)
                        .map(|ch| format!(" ('{ch}')"))
                        .unwrap_or_default();
                    let fold = cell.map(|c| c.fold);
                    let kind = match draw {
                        CellDraw::Draws => None,
                        CellDraw::Blank => Some((
                            NoteKind::CellBlank,
                            format!(
                                "{{{byte:02x}}}{what} has no glyph in this disc's font - it draws blank"
                            ),
                        )),
                        CellDraw::Overprints => Some((
                            NoteKind::CellOverprints,
                            format!(
                                "{{{byte:02x}}}{what} has a glyph but no advance in this disc's font - \
                                 the next letter draws on top of it"
                            ),
                        )),
                    };
                    if let Some((kind, mut reason)) = kind {
                        if mode == AccentMode::Fold
                            && let Some(f) = fold
                        {
                            reason = format!("{{{byte:02x}}}{what} folds to '{f}' on import");
                        }
                        notes.push(CharNote {
                            index: i,
                            fragment: chars[i..i + 4].iter().collect(),
                            kind,
                            fold: fold.map(str::to_string),
                            cell: cell.filter(|c| c.recipe.is_some()).map(|c| c.byte),
                            reason,
                        });
                    }
                }
                i += 4;
                continue;
            }
            if let Some(end) = chars[i..].iter().position(|&c| c == '}') {
                i += end + 1;
                continue;
            }
        }
        if c.is_ascii() || markup::is_lookalike(c) {
            i += 1;
            continue;
        }
        let drawn = latin::drawn_byte_for_char(c);
        let fold = latin::fold_for_char(c);
        let note = match (drawn, fold) {
            (Some(b), Some(f)) => match mode {
                AccentMode::Font => None,
                AccentMode::Fold => Some((
                    NoteKind::Accent,
                    format!("'{c}' folds to '{f}' on import (the accent font would draw it)"),
                )),
                AccentMode::Strict => Some((
                    NoteKind::Accent,
                    format!(
                        "'{c}' is not in the retail NTSC font - fold it to '{f}', or turn on the \
                         accent font (it encodes as {{{b:02x}}})"
                    ),
                )),
            },
            (None, Some(f)) => Some((
                NoteKind::FoldOnly,
                if mode == AccentMode::Strict {
                    format!("'{c}' has no glyph, even in the accent font - fold it to '{f}'")
                } else {
                    format!(
                        "'{c}' has no glyph, even in the accent font - it folds to '{f}' on import"
                    )
                },
            )),
            (_, None) => Some((
                NoteKind::NotLatin,
                format!(
                    "'{c}' (U+{:04X}) is outside the Latin set the font can carry - no glyph and \
                     no fold; write it with Latin letters",
                    c as u32
                ),
            )),
        };
        if let Some((kind, reason)) = note {
            notes.push(CharNote {
                index: i,
                fragment: c.to_string(),
                kind,
                fold: fold.map(str::to_string),
                cell: drawn,
                reason,
            });
        }
        i += 1;
    }
    notes
}

/// `true` when a note means the line will not look as typed after import
/// under its mode (a fold is a change the translator chose, so it is not).
pub fn is_undrawable(n: &CharNote, mode: AccentMode) -> bool {
    match n.kind {
        NoteKind::NotLatin | NoteKind::CellBlank | NoteKind::CellOverprints => {
            !(mode == AccentMode::Fold && n.fold.is_some())
        }
        NoteKind::Accent | NoteKind::FoldOnly => mode == AccentMode::Strict,
    }
}

/// One flagged character with its pack coordinate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyedNote {
    pub section: String,
    pub key: String,
    #[serde(flatten)]
    pub note: CharNote,
}

/// Every flagged character of every filled translation, with its key.
pub fn pack_notes(pack: &LanguagePack, font: Option<&dyn DrawLookup>) -> Vec<KeyedNote> {
    let mode = AccentMode::of(pack);
    let mut out = Vec::new();
    for (section, es) in pack.sections.iter() {
        for e in es.iter().filter(|e| e.is_filled()) {
            for note in analyze(&e.translation, mode, font) {
                out.push(KeyedNote {
                    section: section.to_string(),
                    key: e.key.clone(),
                    note,
                });
            }
        }
    }
    out
}

/// The disc's font page and advance table (the TIM image at
/// [`legaia_font::FONT_TIM_PROT_DAT_OFFSET`] and the SCUS width table).
pub struct DiscFont {
    pub tim: Vec<u8>,
    pub scus: Vec<u8>,
    pub page: FontPage,
    pub widths: [u8; 256],
}

impl DiscFont {
    pub fn read(patcher: &DiscPatcher) -> Result<Self> {
        let tim = patcher
            .read_prot_bytes(
                legaia_font::FONT_TIM_PROT_DAT_OFFSET,
                legaia_font::FONT_TIM_LEN,
            )
            .context("read the dialog-font TIM")?;
        let scus = patcher
            .read_named_file("SCUS_942.54")
            .context("SCUS_942.54 not found in disc image")?;
        let page = FontPage::from_tim(&tim)?;
        let widths = accent_font::read_widths(&scus)?;
        Ok(Self {
            tim,
            scus,
            page,
            widths,
        })
    }

    pub fn state(&self) -> AccentFontState {
        accent_font::accent_font_state(&self.page, &self.widths)
    }

    pub fn draw(&self, byte: u8) -> CellDraw {
        cell_draw(&self.page, &self.widths, byte)
    }

    /// The TIM and SCUS with the accent font written in (for a preview font).
    pub fn patched(&self) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>)> {
        let f = build_accent_font(&self.page, &self.widths);
        let mut tim = self.tim.clone();
        f.page.write_into_tim(&mut tim)?;
        let mut scus = self.scus.clone();
        let off = accent_font::width_table_file_offset(&scus)?;
        scus[off..off + 256].copy_from_slice(&f.widths);
        Ok((tim, scus, f.cells))
    }
}

impl DrawLookup for DiscFont {
    fn draw(&self, byte: u8) -> CellDraw {
        DiscFont::draw(self, byte)
    }
}

/// What writing the accent font did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AccentFontReport {
    /// The disc already carried it (nothing written).
    pub already: bool,
    /// Cells drawn.
    pub cells: usize,
    /// Bytes written (page + advance table).
    pub bytes_written: usize,
}

/// Write the accent font onto the disc: the rebuilt font page into the TIM
/// in `PROT.DAT` and the new advances into `SCUS_942.54`, both same-size
/// in-place writes.
pub fn apply_accent_font(patcher: &mut DiscPatcher) -> Result<AccentFontReport> {
    let font = DiscFont::read(patcher)?;
    let (tim, scus, cells) = font.patched()?;
    if font.state() == AccentFontState::Applied {
        return Ok(AccentFontReport {
            already: true,
            cells: cells.len(),
            bytes_written: 0,
        });
    }
    let off = accent_font::width_table_file_offset(&scus)?;
    patcher.patch_named_file("SCUS_942.54", off as u64, &scus[off..off + 256])?;
    // Only the image rows that changed, to keep the touched sectors minimal.
    let first = tim
        .iter()
        .zip(&font.tim)
        .position(|(a, b)| a != b)
        .unwrap_or(0);
    let last = tim
        .iter()
        .zip(&font.tim)
        .rposition(|(a, b)| a != b)
        .map_or(0, |p| p + 1);
    let mut written = 256;
    if last > first {
        patcher.patch_named_file(
            "PROT.DAT",
            legaia_font::FONT_TIM_PROT_DAT_OFFSET + first as u64,
            &tim[first..last],
        )?;
        written += last - first;
    }
    Ok(AccentFontReport {
        already: false,
        cells: cells.len(),
        bytes_written: written,
    })
}

/// Encode check with the pack's accent mode applied first (the call a live
/// editor makes). Returns the encoded bytes or the codec's issues.
pub fn encode_as_imported(
    markup: &str,
    target: Target,
    mode: AccentMode,
) -> Result<Vec<u8>, Vec<markup::EncodeIssue>> {
    let (t, map, _) = transform(markup, mode);
    markup::encode(&t, target).map_err(|issues| {
        issues
            .into_iter()
            .map(|mut i| {
                i.position = map.get(i.position).copied().unwrap_or(i.position);
                i
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_text_folds_typed_and_escaped_accents_but_not_tokens() {
        let (s, st) = fold_text("{c1:00} Épée {82}t\u{e9} ß ł");
        assert_eq!(s, "{c1:00} Epee ete ss l");
        assert_eq!(st.folded, 6);
        assert_eq!(st.lines, 1);
    }

    #[test]
    fn cells_text_encodes_drawn_cells_and_folds_the_rest() {
        let (s, st) = cells_text("Épée ł ª");
        assert_eq!(s, "{90}p{82}e l a");
        assert_eq!(st.cells, 2);
        assert_eq!(st.folded, 2);
        let bytes = encode_as_imported("à{c1:00}", Target::Segment, AccentMode::Font).unwrap();
        assert_eq!(bytes, [0x85, 0xC1, 0x00]);
    }

    #[test]
    fn strict_mode_reports_with_fold_and_cell() {
        let n = analyze("Olá", AccentMode::Strict, None);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].index, 2);
        assert_eq!(n[0].kind, NoteKind::Accent);
        assert_eq!(n[0].fold.as_deref(), Some("a"));
        assert_eq!(n[0].cell, Some(0xA0));
        assert!(n[0].reason.contains("{a0}"));
        assert!(is_undrawable(&n[0], AccentMode::Strict));
        assert!(analyze("Olá", AccentMode::Font, None).is_empty());
        let fold = analyze("Olá", AccentMode::Fold, None);
        assert!(!is_undrawable(&fold[0], AccentMode::Fold));
    }

    #[test]
    fn non_latin_and_fold_only() {
        let n = analyze("Жł", AccentMode::Font, None);
        assert_eq!(n[0].kind, NoteKind::NotLatin);
        assert_eq!(n[1].kind, NoteKind::FoldOnly);
        assert!(is_undrawable(&n[0], AccentMode::Font));
        assert!(!is_undrawable(&n[1], AccentMode::Font));
    }

    #[test]
    fn escapes_judged_against_the_font() {
        let blank = |_: u8| CellDraw::Blank;
        let n = analyze("x{82}{c1:00}{01}", AccentMode::Strict, Some(&blank));
        // {82} is judged; the token and the control byte {01} are not.
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].kind, NoteKind::CellBlank);
        assert_eq!(n[0].fragment, "{82}");
        // Under the accent font a recipe cell draws.
        assert!(analyze("{82}", AccentMode::Font, Some(&blank)).is_empty());
        let over = |_: u8| CellDraw::Overprints;
        assert_eq!(
            analyze("{82}", AccentMode::Strict, Some(&over))[0].kind,
            NoteKind::CellOverprints
        );
    }

    #[test]
    fn lookalikes_are_not_flagged() {
        assert!(analyze("it\u{2019}s \u{2014} ok\u{2026}", AccentMode::Strict, None).is_empty());
    }

    #[test]
    fn errors_point_at_the_typed_character() {
        // The folded sharp s grows the text; the Cyrillic letter after it is
        // still reported at its own index.
        let e = encode_as_imported("\u{df}\u{df}\u{416}", Target::Segment, AccentMode::Fold)
            .unwrap_err();
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].position, 2);
        let e = encode_as_imported("\u{e9}\u{416}", Target::Segment, AccentMode::Font).unwrap_err();
        assert_eq!(e[0].position, 1);
    }

    #[test]
    fn fold_matches_the_escape_fold() {
        // The escape half of the fold is the lift's fold, byte for byte.
        let src = "{82}x{e1}{c1:00}{b3}{a6}";
        let (a, _) = fold_text(src);
        let (b, _) = markup::fold_high_glyphs(src);
        assert_eq!(a, b);
    }

    #[test]
    fn modes_parse() {
        assert_eq!(AccentMode::parse(""), Some(AccentMode::Strict));
        assert_eq!(AccentMode::parse("font"), Some(AccentMode::Font));
        assert_eq!(AccentMode::parse("nope"), None);
        assert_eq!(AccentMode::Fold.header(), "fold");
    }
}

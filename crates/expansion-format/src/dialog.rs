//! `dialog.toml`: dialogue lines.
//!
//! A line is text in the translation pipeline's markup (printable ASCII, a
//! `{xx}` byte escape, a `{op:arg}` two-byte token such as `{c1:00}` for a
//! party member's name), split into rows and pages: each text line is one
//! dialog-box row, and a blank line starts a new page. A line may end on a
//! menu of two to four choices; a choice can raise or clear pack flags, show
//! a reply and chain another line.
//!
//! ```toml
//! [lines.guide_offer]
//! speaker = "Tower Guide"
//! text = """
//! The tower has woken.
//! Will you climb it?"""
//! choices = [
//!     { label = "I will.", reply = "Then go.", set_flags = ["tower_accepted"] },
//!     { label = "Not yet." },
//! ]
//! ```
//!
//! [`encode_markup`] is the byte encoder; it is the dialog-segment subset of
//! `legaia_translate::markup::encode` (same grammar, same bytes), kept here
//! so the engine and the pack validator encode pack text without the patcher
//! stack.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// The `dialog.toml` section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DialogSection {
    /// Every line, keyed by its pack-unique id (`[lines.<id>]`).
    pub lines: BTreeMap<String, DialogLine>,
}

/// One dialogue line.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DialogLine {
    /// Who says it. Metadata for journals and tools; the box itself shows
    /// only [`Self::text`] (retail boxes carry no name plate).
    pub speaker: Option<String>,
    /// The text: one row per line, a blank line between pages.
    pub text: String,
    /// The menu the line ends on: empty for none, else two to four options.
    pub choices: Vec<DialogChoice>,
}

/// One menu option.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DialogChoice {
    /// The option's label.
    pub label: String,
    /// Text shown after the option is picked (rows and pages as
    /// [`DialogLine::text`]).
    pub reply: Option<String>,
    /// Pack flags raised when the option is picked.
    pub set_flags: Vec<String>,
    /// Pack flags cleared when the option is picked.
    pub clear_flags: Vec<String>,
    /// A line to show once this one ends.
    pub next: Option<String>,
    /// A `battle.toml` formation to fight once the talk ends.
    pub battle: Option<String>,
    /// A `battle.toml` boss to fight once the talk ends.
    pub boss: Option<String>,
}

/// Fewest options a menu takes.
pub const MIN_CHOICES: usize = 2;
/// Most options a menu takes (retail's four-option picker, `0x29`).
pub const MAX_CHOICES: usize = 4;

/// Split text into pages of rows: each line is a row, a blank line ends the
/// page. Leading, trailing and repeated blank lines make no empty page.
pub fn pages(text: &str) -> Vec<Vec<&str>> {
    let mut out = Vec::new();
    let mut page = Vec::new();
    for row in text.lines() {
        if row.trim().is_empty() {
            if !page.is_empty() {
                out.push(std::mem::take(&mut page));
            }
        } else {
            page.push(row);
        }
    }
    if !page.is_empty() {
        out.push(page);
    }
    out
}

/// One fragment of markup that has no byte in a dialog segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkupIssue {
    /// Character index into the markup (in `char`s).
    pub position: usize,
    /// The offending fragment.
    pub fragment: String,
    /// Why it cannot be encoded.
    pub reason: String,
}

impl fmt::Display for MarkupIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "char {} ({:?}): {}",
            self.position, self.fragment, self.reason
        )
    }
}

/// Whether `b` opens a two-byte token in dialog bytecode (`0xC0..=0xCF` and
/// the authoring aliases `0x5E` / `0xFF`; `docs/formats/mes.md`).
pub fn is_two_byte_op(b: u8) -> bool {
    b == 0x5E || (0xC0..=0xCF).contains(&b) || b == 0xFF
}

/// Typographic lookalikes folded onto retail glyphs before encoding.
fn fold_lookalike(c: char) -> Option<&'static str> {
    Some(match c {
        '\u{2018}' | '\u{2019}' | '\u{02BC}' => "'",
        '\u{201C}' | '\u{201D}' => "\"",
        '\u{2013}' | '\u{2014}' | '\u{2212}' => "-",
        '\u{2026}' => "...",
        '\u{00A0}' => " ",
        _ => return None,
    })
}

fn hex_val(c: char) -> Option<u8> {
    c.to_digit(16).map(|d| d as u8)
}

/// Encode one row of markup into dialog-segment bytes (no `0x1F` lead, no
/// terminator). Bytes `0x00..=0x1F` are refused: they would end the segment.
/// Every issue is reported at once.
pub fn encode_markup(markup: &str) -> Result<Vec<u8>, Vec<MarkupIssue>> {
    let chars: Vec<char> = markup.chars().collect();
    let mut out = Vec::with_capacity(chars.len());
    let mut issues = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '{' {
            let rest = &chars[i + 1..];
            let parsed = match rest {
                [h1, h2, '}', ..] => hex_val(*h1)
                    .zip(hex_val(*h2))
                    .map(|(a, b)| ((a << 4) | b, None, 4usize)),
                [h1, h2, ':', h3, h4, '}', ..] => {
                    match (hex_val(*h1), hex_val(*h2), hex_val(*h3), hex_val(*h4)) {
                        (Some(a), Some(b), Some(c2), Some(d)) => {
                            Some(((a << 4) | b, Some((c2 << 4) | d), 7usize))
                        }
                        _ => None,
                    }
                }
                _ => None,
            };
            let Some((op, arg, consumed)) = parsed else {
                issues.push(MarkupIssue {
                    position: i,
                    fragment: "{".into(),
                    reason: "unclosed or malformed escape (expected {xx} or {xx:yy})".into(),
                });
                i += 1;
                continue;
            };
            let frag: String = chars[i..i + consumed].iter().collect();
            // Only the opcode byte is held to the terminator rule: an
            // argument byte rides inside its token (`{c1:00}`).
            if op <= 0x1F {
                issues.push(MarkupIssue {
                    position: i,
                    fragment: frag.clone(),
                    reason: format!("byte 0x{op:02x} would end the dialog segment early"),
                });
            } else {
                out.push(op);
            }
            if let Some(a) = arg {
                if is_two_byte_op(op) {
                    out.push(a);
                } else {
                    issues.push(MarkupIssue {
                        position: i,
                        fragment: frag,
                        reason: format!("0x{op:02x} is not a two-byte opcode"),
                    });
                }
            }
            i += consumed;
            continue;
        }
        if c == '}' {
            issues.push(MarkupIssue {
                position: i,
                fragment: "}".into(),
                reason: "stray '}' (write {7d} for the glyph)".into(),
            });
        } else if (' '..='~').contains(&c) {
            out.push(c as u8);
        } else if let Some(s) = fold_lookalike(c) {
            out.extend_from_slice(s.as_bytes());
        } else {
            issues.push(MarkupIssue {
                position: i,
                fragment: c.to_string(),
                reason: "no glyph in the retail font".into(),
            });
        }
        i += 1;
    }
    if issues.is_empty() {
        Ok(out)
    } else {
        Err(issues)
    }
}

/// Why a line does not compile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogError {
    /// No line with that id.
    Unknown(String),
    /// The line, or a choice's reply, has no text where text is required.
    Empty,
    /// A menu with fewer than two or more than four options.
    ChoiceCount(usize),
    /// Text that has no bytes in a dialog segment: `(where, issues)`.
    Markup(String, Vec<MarkupIssue>),
}

impl fmt::Display for DialogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(id) => write!(f, "no line {id:?}"),
            Self::Empty => write!(f, "no text"),
            Self::ChoiceCount(n) => {
                write!(
                    f,
                    "{n} choices; a menu takes {MIN_CHOICES} to {MAX_CHOICES}"
                )
            }
            Self::Markup(at, issues) => {
                write!(f, "{at}: ")?;
                for (i, issue) in issues.iter().enumerate() {
                    if i > 0 {
                        write!(f, "; ")?;
                    }
                    write!(f, "{issue}")?;
                }
                Ok(())
            }
        }
    }
}

fn push_row(out: &mut Vec<u8>, row: &str, at: &str) -> Result<(), DialogError> {
    let bytes = encode_markup(row).map_err(|e| DialogError::Markup(at.to_string(), e))?;
    out.push(0x1F);
    out.extend_from_slice(&bytes);
    out.push(0x00);
    Ok(())
}

fn push_text(out: &mut Vec<u8>, text: &str, at: &str) -> Result<bool, DialogError> {
    let pages = pages(text);
    for (i, page) in pages.iter().enumerate() {
        if i > 0 {
            out.push(0x24);
        }
        for row in page {
            push_row(out, row, at)?;
        }
    }
    Ok(!pages.is_empty())
}

/// Compile `line` to the inline interaction-script bytes the field VM's
/// dialogue runner reads for an NPC talk, so a pack line types, pages and
/// opens its menu through the same pager as a retail one:
///
/// ```text
/// page    = row+                       row = 0x1F <encoded markup> 0x00
/// text    = page (0x24 page)*          0x24: next page, same box
/// line    = text 0x21                  no menu: the talk ends
///         | text <open> j0..jN-1 label*N branch*N
///   open  = 0x27 / 0x28 / 0x29         two / three / four options
///   jK    = i16 LE, relative to jK's own offset, to branch K
///   label = 0x1F <encoded label> 0x00  immediate-labels form
///   branch= [text] 0x21                the option's reply, then the end
/// ```
pub fn compile_line(line: &DialogLine) -> Result<Vec<u8>, DialogError> {
    let mut out = Vec::new();
    if !push_text(&mut out, &line.text, "text")? {
        return Err(DialogError::Empty);
    }
    if line.choices.is_empty() {
        out.push(0x21);
        return Ok(out);
    }
    let n = line.choices.len();
    if !(MIN_CHOICES..=MAX_CHOICES).contains(&n) {
        return Err(DialogError::ChoiceCount(n));
    }
    out.push(0x25 + n as u8); // 0x27 / 0x28 / 0x29
    let table = out.len();
    out.resize(table + n * 2, 0);
    for (k, c) in line.choices.iter().enumerate() {
        push_row(&mut out, &c.label, &format!("choices[{k}].label"))?;
    }
    for (k, c) in line.choices.iter().enumerate() {
        let entry = table + k * 2;
        let rel = i16::try_from(out.len() - entry).map_err(|_| DialogError::Empty)?;
        out[entry..entry + 2].copy_from_slice(&rel.to_le_bytes());
        if let Some(reply) = &c.reply {
            push_text(&mut out, reply, &format!("choices[{k}].reply"))?;
        }
        out.push(0x21);
    }
    Ok(out)
}

impl crate::validate::SectionRules for DialogSection {
    fn declare(&self, cx: &mut crate::validate::Checker<'_>) {
        for id in self.lines.keys() {
            cx.declare("line", id, &format!("lines.{id}"));
        }
    }

    fn check(&self, cx: &mut crate::validate::Checker<'_>) {
        for (id, line) in &self.lines {
            let at = format!("lines.{id}");
            if let Err(e) = compile_line(line) {
                cx.error(&at, e.to_string());
            }
            for (k, c) in line.choices.iter().enumerate() {
                let at = format!("{at}.choices[{k}]");
                if let Some(next) = &c.next {
                    cx.reference("line", next, &format!("{at}.next"));
                }
                if let Some(f) = &c.battle {
                    cx.reference("formation", f, &format!("{at}.battle"));
                }
                if let Some(b) = &c.boss {
                    cx.reference("boss", b, &format!("{at}.boss"));
                }
                if c.battle.is_some() && c.boss.is_some() {
                    cx.error(&at, "a choice starts a battle or a boss, not both");
                }
                for f in c.set_flags.iter().chain(&c.clear_flags) {
                    if f.trim().is_empty() {
                        cx.error(&at, "an empty pack flag id");
                    }
                }
            }
            for row in pages(&line.text).iter().flatten() {
                if row.chars().count() > ROW_WARN_CHARS {
                    cx.warning(
                        &format!("{at}.text"),
                        format!(
                            "row {row:?} is over {ROW_WARN_CHARS} characters; dialog boxes do not wrap"
                        ),
                    );
                }
            }
        }
    }
}

/// Row length past which the validator warns: roughly what fits the
/// standard dialog box at the proportional font's average advance.
pub const ROW_WARN_CHARS: usize = 36;

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str) -> DialogLine {
        DialogLine {
            text: text.into(),
            ..Default::default()
        }
    }

    #[test]
    fn lines_and_choices_parse() {
        let text = r#"
            [lines.offer]
            speaker = "Guide"
            text = "Row one\nRow two\n\nPage two"
            choices = [
                { label = "Yes", reply = "Good.", set_flags = ["ok"], next = "after" },
                { label = "No" },
            ]
            [lines.after]
            text = "Done."
        "#;
        let s: DialogSection = toml::from_str(text).unwrap();
        let l = &s.lines["offer"];
        assert_eq!(l.speaker.as_deref(), Some("Guide"));
        assert_eq!(l.choices.len(), 2);
        assert_eq!(l.choices[0].set_flags, vec!["ok".to_string()]);
        assert_eq!(l.choices[0].next.as_deref(), Some("after"));
        assert_eq!(
            pages(&l.text),
            vec![vec!["Row one", "Row two"], vec!["Page two"]]
        );
    }

    #[test]
    fn blank_lines_make_no_empty_pages() {
        assert_eq!(pages("\n\na\n\n\n\nb\n\n"), vec![vec!["a"], vec!["b"]]);
        assert!(pages("").is_empty());
    }

    #[test]
    fn markup_encodes_ascii_escapes_and_tokens() {
        assert_eq!(encode_markup("Hi, {c1:00}!").unwrap(), b"Hi, \xC1\x00!");
        assert_eq!(encode_markup("{7b}x{7d}").unwrap(), b"{x}");
        assert_eq!(encode_markup("a|b").unwrap(), b"a|b");
        assert_eq!(encode_markup("it\u{2019}s\u{2026}").unwrap(), b"it's...");
    }

    #[test]
    fn markup_reports_every_issue() {
        let issues = encode_markup("\u{00E9}{01}{zz").unwrap_err();
        assert_eq!(issues.len(), 3);
        assert_eq!(issues[0].position, 0);
        assert!(issues[1].reason.contains("0x01"));
    }

    #[test]
    fn a_token_on_a_one_byte_op_is_refused() {
        assert!(encode_markup("{41:42}").is_err());
    }

    #[test]
    fn a_plain_line_is_rows_pages_and_an_end() {
        let b = compile_line(&line("ab\ncd\n\nef")).unwrap();
        assert_eq!(
            b,
            [
                &[0x1F, b'a', b'b', 0x00, 0x1F, b'c', b'd', 0x00, 0x24][..],
                &[0x1F, b'e', b'f', 0x00, 0x21]
            ]
            .concat()
        );
    }

    #[test]
    fn a_menu_jumps_each_option_to_its_branch() {
        let mut l = line("Go?");
        l.choices = vec![
            DialogChoice {
                label: "Yes".into(),
                reply: Some("OK".into()),
                ..Default::default()
            },
            DialogChoice {
                label: "No".into(),
                ..Default::default()
            },
        ];
        let b = compile_line(&l).unwrap();
        // prompt
        assert_eq!(&b[..6], &[0x1F, b'G', b'o', b'?', 0x00, 0x27]);
        let table = 6;
        let target = |k: usize| {
            let e = table + k * 2;
            (e as isize + i16::from_le_bytes([b[e], b[e + 1]]) as isize) as usize
        };
        // labels follow the table
        assert_eq!(&b[10..15], &[0x1F, b'Y', b'e', b's', 0x00]);
        assert_eq!(&b[15..19], &[0x1F, b'N', b'o', 0x00]);
        assert_eq!(
            &b[target(0)..target(0) + 5],
            &[0x1F, b'O', b'K', 0x00, 0x21]
        );
        assert_eq!(b[target(1)], 0x21);
        assert_eq!(target(1), b.len() - 1);
    }

    #[test]
    fn the_open_byte_follows_the_option_count() {
        for (n, open) in [(2, 0x27), (3, 0x28), (4, 0x29)] {
            let mut l = line("?");
            l.choices = vec![DialogChoice::default(); n];
            l.choices.iter_mut().for_each(|c| c.label = "x".into());
            assert_eq!(compile_line(&l).unwrap()[3], open);
        }
        let mut l = line("?");
        l.choices = vec![DialogChoice::default(); 5];
        assert_eq!(compile_line(&l), Err(DialogError::ChoiceCount(5)));
    }

    #[test]
    fn bad_text_is_reported_where_it_is() {
        assert_eq!(compile_line(&line("\n\n")), Err(DialogError::Empty));
        match compile_line(&line("caf\u{00E9}")) {
            Err(DialogError::Markup(at, issues)) => {
                assert_eq!(at, "text");
                assert_eq!(issues.len(), 1);
            }
            other => panic!("{other:?}"),
        }
    }
}

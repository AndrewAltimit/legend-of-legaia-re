//! Reversible text markup <-> game-byte codec for the translation pipeline.
//!
//! The retail glyph atlas is indexed **by byte**: cells `0x20..=0xFF` in a
//! 16x14 VRAM tile-page, with `0x20..=0x7E` laid out as plain ASCII (see
//! `docs/formats/dialog-font.md`). Dialog bytecode additionally interleaves
//! 2-byte escape tokens (`0xC0..=0xCF`, plus the authoring aliases `0x5E` and
//! `0xFF` - see `docs/formats/mes.md`). This module round-trips those bytes
//! through a human-editable string form:
//!
//! - printable ASCII `0x20..=0x7E` maps to itself (`'|'` = the in-game
//!   newline glyph `0x7C`), except `{` / `}` which are reserved for escapes;
//! - a 2-byte token renders as `{op:arg}` in lowercase hex (`{c1:00}` =
//!   "substitute character name 0", `{cf:31}` = color change);
//! - any other byte renders as `{xx}` (`{01}` = the item-icon prefix,
//!   `{a4}` = a high-glyph tile).
//!
//! [`encode`] is the exact inverse and reports **per-character** errors for
//! anything outside the retail glyph set (accented Latin, Cyrillic, CJK, ...),
//! after first folding a small set of typographic lookalikes (smart quotes,
//! dashes, ellipsis) onto their ASCII glyphs. Full non-Latin support would
//! need a font patch and is out of scope - see `docs/tooling/translation.md`.

use std::fmt;

/// `true` for the opcode bytes that consume one argument byte in dialog
/// bytecode: the substitution / spacing / color block `0xC0..=0xCF` plus the
/// authoring-time aliases `0x5E` (spacing) and `0xFF` (skip-two). Mirrors the
/// stride table in `docs/formats/mes.md` (`legaia_mes::classify_byte`).
pub fn is_two_byte_op(b: u8) -> bool {
    b == 0x5E || (0xC0..=0xCF).contains(&b) || b == 0xFF
}

/// Decode raw game bytes (a dialog segment or a NUL-terminated SCUS string,
/// **without** its terminator) into the markup form. Total function - every
/// byte sequence decodes, and `encode(decode(bytes)) == bytes`.
pub fn decode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() + 8);
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if is_two_byte_op(b) {
            if let Some(&arg) = bytes.get(i + 1) {
                out.push_str(&format!("{{{b:02x}:{arg:02x}}}"));
                i += 2;
                continue;
            }
            // Dangling 2-byte op at end of buffer: emit as a bare escape so
            // the byte count still round-trips.
            out.push_str(&format!("{{{b:02x}}}"));
            i += 1;
            continue;
        }
        match b {
            0x7B => out.push_str("{7b}"),
            0x7D => out.push_str("{7d}"),
            0x20..=0x7E => out.push(b as char),
            _ => out.push_str(&format!("{{{b:02x}}}")),
        }
        i += 1;
    }
    out
}

/// Where the encoded bytes will land, which controls which byte values are
/// legal in the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// A `0x1F`-lead dialog segment (scene MAN / event script). Bytes
    /// `0x00..=0x1F` are forbidden - `0x00..=0x1E` would terminate the
    /// segment early and `0x1F` opens a new one.
    Segment,
    /// A NUL-terminated SCUS string. Only `0x00` (the terminator) is
    /// forbidden; control prefixes like the `{01}` item icon are legal.
    CString,
}

/// One character (or escape) of `encode` input that cannot be represented in
/// the retail glyph set / target byte space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodeIssue {
    /// Character index into the markup string (in `char`s).
    pub position: usize,
    /// Offending source fragment (a single char, or an escape like `{00}`).
    pub fragment: String,
    /// Human-readable reason.
    pub reason: String,
}

impl fmt::Display for EncodeIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "char {} ({:?}): {}",
            self.position, self.fragment, self.reason
        )
    }
}

/// Typographic lookalikes silently folded onto retail glyphs before
/// encodability is judged. Keeps AI / word-processor output ("it’s", “x”, —)
/// importable without hand fixing.
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

/// Encode markup back into game bytes for `target`. Returns every
/// per-character issue at once (not just the first) so a translator can fix a
/// whole line in one pass.
pub fn encode(markup: &str, target: Target) -> Result<Vec<u8>, Vec<EncodeIssue>> {
    let chars: Vec<char> = markup.chars().collect();
    let mut out = Vec::with_capacity(chars.len());
    let mut issues = Vec::new();
    let mut i = 0;
    let forbid = |b: u8| match target {
        Target::Segment => b <= 0x1F,
        Target::CString => b == 0x00,
    };
    let push_byte =
        |b: u8, pos: usize, frag: &str, out: &mut Vec<u8>, issues: &mut Vec<EncodeIssue>| {
            if forbid(b) {
                issues.push(EncodeIssue {
                    position: pos,
                    fragment: frag.to_string(),
                    reason: format!(
                        "byte 0x{b:02x} is forbidden here (it would terminate the {} early)",
                        match target {
                            Target::Segment => "dialog segment",
                            Target::CString => "string",
                        }
                    ),
                });
            } else {
                out.push(b);
            }
        };
    while i < chars.len() {
        let c = chars[i];
        if c == '{' {
            // Escape: {xx} or {xx:yy}, lowercase/uppercase hex accepted.
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
            match parsed {
                Some((op, arg, consumed)) => {
                    let frag: String = chars[i..i + consumed].iter().collect();
                    // Only the opcode byte is checked against the terminator
                    // policy - an *argument* byte rides inside its 2-byte
                    // token and may legally be any value (e.g. `{c1:00}`,
                    // `{ce:14}`).
                    push_byte(op, i, &frag, &mut out, &mut issues);
                    if let Some(a) = arg {
                        if is_two_byte_op(op) {
                            out.push(a);
                        } else {
                            issues.push(EncodeIssue {
                                position: i,
                                fragment: frag.clone(),
                                reason: format!(
                                    "0x{op:02x} is not a 2-byte opcode - write the bytes \
                                     separately as {{{op:02x}}}{{{a:02x}}}"
                                ),
                            });
                        }
                    }
                    i += consumed;
                }
                None => {
                    issues.push(EncodeIssue {
                        position: i,
                        fragment: "{".to_string(),
                        reason: "malformed escape - expected {xx} or {xx:yy} with hex digits"
                            .to_string(),
                    });
                    i += 1;
                }
            }
            continue;
        }
        if c == '}' {
            issues.push(EncodeIssue {
                position: i,
                fragment: "}".to_string(),
                reason: "stray '}' - literal braces must be written {7b} / {7d}".to_string(),
            });
            i += 1;
            continue;
        }
        if let Some(folded) = fold_lookalike(c) {
            for fc in folded.chars() {
                push_byte(fc as u8, i, folded, &mut out, &mut issues);
            }
            i += 1;
            continue;
        }
        let cp = c as u32;
        if (0x20..=0x7E).contains(&cp) {
            push_byte(cp as u8, i, &c.to_string(), &mut out, &mut issues);
            i += 1;
            continue;
        }
        issues.push(EncodeIssue {
            position: i,
            fragment: c.to_string(),
            reason: format!(
                "'{c}' (U+{cp:04X}) is not in the retail glyph set - only printable ASCII \
                 0x20..0x7E renders; non-Latin text needs a font patch (out of scope)"
            ),
        });
        i += 1;
    }
    if issues.is_empty() {
        Ok(out)
    } else {
        Err(issues)
    }
}

/// ASCII fold for the accented high-glyph cells of the **PAL** atlas, keyed by
/// the raw byte the official discs use. The layout is IBM CP437 for the cells
/// CP437 carries, plus the game-specific capital block around `0xD0..=0xD6`
/// (see `docs/tooling/pal-localizations.md`).
///
/// This exists for the official-localization lift: the NTSC font has no glyph
/// in those cells, so lifted FR/DE/IT text either needs a font patch or must be
/// folded onto the plain-ASCII glyphs the USA disc does have.
///
/// Every fold is one byte in, one byte out except `ss` for `0xE1` (sharp s),
/// which grows the line by one byte and may therefore push a tight line over
/// its budget.
fn high_glyph_fold(b: u8) -> Option<&'static str> {
    Some(match b {
        0x80 => "C", // C-cedilla
        0x81 => "u", // u-diaeresis
        0x82 => "e", // e-acute
        0x83 => "a", // a-circumflex
        0x84 => "a", // a-diaeresis
        0x85 => "a", // a-grave
        0x86 => "a", // a-ring
        0x87 => "c", // c-cedilla
        0x88 => "e", // e-circumflex
        0x89 => "e", // e-diaeresis
        0x8A => "e", // e-grave
        0x8B => "i", // i-diaeresis
        0x8C => "i", // i-circumflex
        0x8D => "i", // i-grave
        0x8E => "A", // A-diaeresis
        0x8F => "A", // A-ring
        0x90 => "E", // E-acute
        0x91 => "ae",
        0x92 => "AE",
        0x93 => "o", // o-circumflex
        0x94 => "o", // o-diaeresis
        0x95 => "o", // o-grave
        0x96 => "u", // u-circumflex
        0x97 => "u", // u-grave
        0x98 => "y", // y-diaeresis
        0x99 => "O", // O-diaeresis
        0x9A => "U", // U-diaeresis
        0xA0 => "a", // a-acute
        0xA1 => "i", // i-acute
        0xA2 => "o", // o-acute
        0xA3 => "u", // u-acute
        0xA4 => "n", // n-tilde
        0xA5 => "N", // N-tilde
        0xD0 => "A", // game-specific capital block
        0xD1 => "A",
        0xD2 => "E",
        0xD3 => "E",
        0xD4 => "E", // Italian E-grave
        0xD5 => "I",
        0xD6 => "I",
        0xE1 => "ss", // sharp s - the one fold that grows the line
        _ => return None,
    })
}

/// Outcome of [`fold_high_glyphs`] - counts only, never text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FoldStats {
    /// High-glyph escapes replaced with an ASCII equivalent.
    pub folded: usize,
    /// High-cell escapes with no accent fold, left as raw bytes. Mostly the
    /// retail atlas's own non-accent symbol cells (which the USA disc uses in
    /// its spell names, so they render as-is); the remainder is the odd byte in
    /// a marginal raw-carrier segment.
    pub unmapped: usize,
}

impl FoldStats {
    pub fn merge(&mut self, other: FoldStats) {
        self.folded += other.folded;
        self.unmapped += other.unmapped;
    }
}

/// Fold a markup string's accented high-glyph escapes onto plain ASCII.
///
/// Operates on the escape form ([`decode`]'s output), so `{82}` becomes `e`
/// and every 2-byte control token (`{c1:00}`, `{cf:31}`) is left untouched -
/// only bare single-byte escapes in the accent cells are rewritten.
pub fn fold_high_glyphs(markup: &str) -> (String, FoldStats) {
    let chars: Vec<char> = markup.chars().collect();
    let mut out = String::with_capacity(markup.len());
    let mut stats = FoldStats::default();
    let mut i = 0;
    while i < chars.len() {
        // Only bare `{xx}` escapes are candidates; `{xx:yy}` is a control token.
        if chars[i] == '{'
            && let [h1, h2, '}', ..] = &chars[i + 1..]
            && let Some(b) = hex_val(*h1).zip(hex_val(*h2)).map(|(a, b)| (a << 4) | b)
            && b >= 0x80
            && !is_two_byte_op(b)
        {
            match high_glyph_fold(b) {
                Some(ascii) => {
                    out.push_str(ascii);
                    stats.folded += 1;
                }
                None => {
                    out.push_str(&format!("{{{b:02x}}}"));
                    stats.unmapped += 1;
                }
            }
            i += 4;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    (out, stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_maps_accents_and_leaves_controls_alone() {
        // "Ep{82}e" - the e-acute cell folds; the color token survives verbatim.
        let (s, st) = fold_high_glyphs("{cf:31}Ep{82}e{c2:79}");
        assert_eq!(s, "{cf:31}Epee{c2:79}");
        assert_eq!(st.folded, 1);
        assert_eq!(st.unmapped, 0);
        // Sharp s folds to two characters.
        let (s, _) = fold_high_glyphs("Gru{e1}");
        assert_eq!(s, "Gruss");
        // An unknown high cell is preserved as a raw byte, and counted.
        let (s, st) = fold_high_glyphs("x{b3}");
        assert_eq!(s, "x{b3}");
        assert_eq!(st.unmapped, 1);
    }

    #[test]
    fn folded_text_is_encodable() {
        let bytes = [b'E', b'p', 0x82, b'e'];
        let (folded, _) = fold_high_glyphs(&decode(&bytes));
        assert_eq!(encode(&folded, Target::Segment).unwrap(), b"Epee");
        // Unfolded, the same line still encodes - as the raw high byte, which
        // needs a font patch to render.
        assert_eq!(encode(&decode(&bytes), Target::Segment).unwrap(), bytes);
    }

    #[test]
    fn ascii_round_trips_identity() {
        let src = b"Lezam: I am called Lezam.";
        let m = decode(src);
        assert_eq!(m, "Lezam: I am called Lezam.");
        assert_eq!(encode(&m, Target::Segment).unwrap(), src);
    }

    #[test]
    fn escapes_round_trip() {
        let src = [0xC1, 0x00, b'H', b'i', 0xCF, 0x31, 0x01, 0xA4];
        let m = decode(&src);
        assert_eq!(m, "{c1:00}Hi{cf:31}{01}{a4}");
        assert_eq!(encode(&m, Target::CString).unwrap(), src);
    }

    #[test]
    fn braces_and_pipe() {
        let src = [0x7B, 0x7C, 0x7D];
        let m = decode(&src);
        assert_eq!(m, "{7b}|{7d}");
        assert_eq!(encode(&m, Target::Segment).unwrap(), src);
    }

    /// Full retail glyph-set property: every terminator-free byte stream
    /// round-trips `encode(decode(x)) == x` under the `CString` policy.
    #[test]
    fn round_trip_property_full_byte_space() {
        let mut x = 0x1234_5678_9ABC_DEF0u64;
        for len in 0..64usize {
            let mut buf = Vec::with_capacity(len);
            while buf.len() < len {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                let b = x as u8;
                if b != 0 {
                    buf.push(b);
                }
            }
            let m = decode(&buf);
            let back = encode(&m, Target::CString).unwrap_or_else(|e| {
                panic!("encode failed for {buf:02x?} -> {m:?}: {e:?}");
            });
            assert_eq!(back, buf, "markup {m:?}");
        }
        // And exhaustively for every single byte 0x01..=0xFF.
        for b in 1u8..=0xFF {
            let buf = [b];
            let m = decode(&buf);
            assert_eq!(encode(&m, Target::CString).unwrap(), buf, "byte {b:02x}");
        }
    }

    #[test]
    fn dangling_two_byte_op_round_trips() {
        let src = [b'A', 0xC1];
        let m = decode(&src);
        assert_eq!(m, "A{c1}");
        assert_eq!(encode(&m, Target::Segment).unwrap(), src);
    }

    #[test]
    fn non_latin_reports_every_offender() {
        let err = encode("héllo wörld", Target::Segment).unwrap_err();
        assert_eq!(err.len(), 2);
        assert!(err[0].fragment == "é");
        assert!(err[1].fragment == "ö");
        assert!(err[0].reason.contains("font patch"));
    }

    #[test]
    fn smart_punctuation_folds() {
        let bytes = encode(
            "it\u{2019}s \u{201C}x\u{201D} \u{2014} y\u{2026}",
            Target::Segment,
        )
        .expect("lookalikes fold");
        assert_eq!(bytes, b"it's \"x\" - y...");
    }

    #[test]
    fn terminator_bytes_rejected_per_target() {
        // {00} illegal everywhere; {05} legal in CString, illegal in Segment.
        assert!(encode("{00}", Target::CString).is_err());
        assert!(encode("{00}", Target::Segment).is_err());
        assert!(encode("{05}", Target::CString).is_ok());
        assert!(encode("{05}", Target::Segment).is_err());
        assert!(encode("{1f}", Target::Segment).is_err());
    }

    #[test]
    fn malformed_escape_reported() {
        // The malformed escape reports once, then the scanner resumes past
        // the '{' - the trailing '}' reports as stray.
        let err = encode("a {zz} b", Target::Segment).unwrap_err();
        assert_eq!(err.len(), 2);
        assert!(err[0].reason.contains("malformed"));
        assert!(err[1].reason.contains("stray"));
        let err = encode("a } b", Target::Segment).unwrap_err();
        assert!(err[0].reason.contains("stray"));
    }
}

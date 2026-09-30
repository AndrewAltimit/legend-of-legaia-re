//! The Japanese build's dialog text: **count-led Shift-JIS** lines.
//!
//! The Japanese disc (`SCPS_100.59`) frames field dialog differently from the
//! Latin builds. A Latin line is `0x1F <glyph bytes> 0x00`; a Japanese line is
//! a single **count byte** `N` (`0x01..=0x1F`) followed by exactly `N`
//! two-byte tokens and no terminator - the next byte is the next line's count
//! or the post-page control byte, the same stream position a Latin line's
//! successor occupies. A zero count is an empty row (the Latin `1F 20 00`
//! spacer). A token is either a Shift-JIS double-byte character or an escape
//! whose first byte is `0xF0..=0xFF`: `F1 xx` draws a party member's name
//! (`F1 63` the party leader) and `F2 xx` an item name, the Japanese
//! counterparts of the Latin `C1 xx` / `C2 xx`; `F5`, `F7`, `FE` and `FF` also
//! occur and are carried as escapes. The packets a script instruction carries
//! (an op-`0x4C` `E1` balloon, an op-`0x49` sub-0 inline MES or a shop
//! record's vendor name) keep a trailing `0x00` after their `N` tokens, where
//! the Latin form keeps it after its glyphs.
//!
//! This is why a Latin exporter finds nothing on the Japanese disc: there is
//! no `0x1F` lead, and the count byte reads as a terminator. The script walk
//! here ([`man_lines`]) is the Latin walk (`legaia_asset::field_disasm`) with
//! those four instruction shapes sized the Japanese way, run over the same
//! per-record windows (`man_edit::record_script_windows`).
//!
//! Keys stay disc coordinates: a line's `text_off` is its first token byte
//! (the byte after the count), its `len` is `2 * N`. Decoded text is Unicode
//! with every byte pair that does not round-trip through Shift-JIS written as
//! a `{xx:yy}` escape, so [`encode`]`(`[`decode`]`(b)) == b` for every line.

use legaia_asset::{field_disasm, man_edit};

use super::segments::Segment;

/// Highest count byte a line may carry: every byte below `0x20` opens a line
/// (the pager's `(b & 0x7F) < 0x20` test), and the Latin builds kept `0x1F`
/// as their lead.
pub const MAX_COUNT: u8 = 0x1F;

/// `true` for the first byte of a two-byte substitution / layout escape.
pub fn is_escape(b: u8) -> bool {
    b >= 0xF0
}

/// `true` for a Shift-JIS double-byte lead in the JIS X 0208 rows the game's
/// font carries.
pub fn is_char_lead(b: u8) -> bool {
    matches!(b, 0x81..=0x9F | 0xE0..=0xEF)
}

/// The Unicode character a two-byte token draws, when the pair decodes and
/// re-encodes to exactly itself.
fn char_of(pair: [u8; 2]) -> Option<char> {
    if !is_char_lead(pair[0]) {
        return None;
    }
    let (s, had_errors) = encoding_rs::SHIFT_JIS.decode_without_bom_handling(&pair);
    if had_errors {
        return None;
    }
    let mut it = s.chars();
    let c = it.next()?;
    if it.next().is_some() || c.is_ascii() {
        return None;
    }
    let (back, _, bad) = encoding_rs::SHIFT_JIS.encode(&s);
    (!bad && back.as_ref() == pair).then_some(c)
}

/// Decode a line's token bytes (without its count byte) into pack markup:
/// Unicode for round-tripping characters, `{xx:yy}` for everything else
/// (escapes and non-round-tripping pairs), `{xx}` for a dangling odd byte.
pub fn decode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let (pairs, rest) = bytes.as_chunks::<2>();
    for pair in pairs {
        match char_of([pair[0], pair[1]]) {
            Some(c) => out.push(c),
            None => out.push_str(&format!("{{{:02x}:{:02x}}}", pair[0], pair[1])),
        }
    }
    if let [b] = rest {
        out.push_str(&format!("{{{b:02x}}}"));
    }
    out
}

/// Encode pack markup back into token bytes: the inverse of [`decode`]. Every
/// non-escape character must be a Shift-JIS double-byte character.
pub fn encode(text: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '{' {
            let close = text[i..]
                .find('}')
                .ok_or_else(|| format!("unclosed escape at {i}"))?;
            let body = &text[i + 1..i + close];
            for part in body.split(':') {
                let b = u8::from_str_radix(part, 16)
                    .map_err(|_| format!("bad escape '{{{body}}}' at {i}"))?;
                out.push(b);
            }
            while chars.peek().is_some_and(|&(j, _)| j <= i + close) {
                chars.next();
            }
            continue;
        }
        let mut buf = [0u8; 4];
        let (bytes, _, bad) = encoding_rs::SHIFT_JIS.encode(c.encode_utf8(&mut buf));
        if bad || bytes.len() != 2 {
            return Err(format!(
                "'{c}' at {i} is not a Shift-JIS double-byte character"
            ));
        }
        out.extend_from_slice(&bytes);
    }
    Ok(out)
}

/// A count-led line at `lead`: `Some(N)` when `N` is in range and the buffer
/// holds its `2 * N` token bytes.
fn line_at(buf: &[u8], lead: usize) -> Option<usize> {
    let n = *buf.get(lead)?;
    if !(1..=MAX_COUNT).contains(&n) {
        return None;
    }
    let n = n as usize;
    (lead + 1 + 2 * n <= buf.len()).then_some(n)
}

/// The token bytes of the count-led line at `lead`, if it is one.
pub fn line_tokens(buf: &[u8], lead: usize) -> Option<&[u8]> {
    let n = line_at(buf, lead)?;
    buf.get(lead + 1..lead + 1 + 2 * n)
}

/// `true` when a line's tokens read as text: every token is a round-tripping
/// character or an escape. A line of escapes alone is text too - a picker
/// whose options are party members' names is three one-token `F1 xx` lines.
/// A count byte the walk reaches over operand bytes fails this.
pub fn qualifies(tokens: &[u8]) -> bool {
    !tokens.is_empty()
        && tokens.len().is_multiple_of(2)
        && tokens
            .as_chunks::<2>()
            .0
            .iter()
            .all(|p| char_of(*p).is_some() || is_escape(p[0]))
}

/// One instruction of the Japanese walk: its size, and the count byte of the
/// line it carries (if any).
pub fn step(man: &[u8], pc: usize) -> Option<(usize, Option<usize>)> {
    let b = *man.get(pc)?;
    // Bare line.
    if let Some(n) = line_at(man, pc) {
        return Some((1 + 2 * n, Some(pc)));
    }
    // A zero count is an empty row (the Latin `1F 20 00` spacer line): a
    // letter's blank line between two paragraphs.
    if b == 0 {
        return Some((1, None));
    }
    // `4C E1` balloon: [4C E1 N tokens 00].
    if b == 0x4C && man.get(pc + 1) == Some(&0xE1) {
        let lead = pc + 2;
        if let Some(n) = line_at(man, lead)
            && man.get(lead + 1 + 2 * n) == Some(&0)
        {
            return Some((3 + 2 * n + 1, Some(lead)));
        }
    }
    // `49 00` sub-0: a shop record [49 00 len args.. count ids.. N name 00]
    // or an inline MES [49 00 len args.. N tokens 00].
    if b == 0x49 && man.get(pc + 1) == Some(&0) {
        let length = *man.get(pc + 2)? as usize;
        let count_off = pc + 3 + length;
        let count = *man.get(count_off)? as usize;
        let name_lead = count_off + 1 + count;
        if (1..=20).contains(&count)
            && man
                .get(count_off + 1..name_lead)
                .is_some_and(|ids| !ids.contains(&0))
            && let Some(n) = line_at(man, name_lead)
            && man.get(name_lead + 1 + 2 * n) == Some(&0)
            && qualifies(&man[name_lead + 1..name_lead + 1 + 2 * n])
        {
            return Some((name_lead + 1 + 2 * n + 1 - pc, Some(name_lead)));
        }
        if let Some(n) = line_at(man, count_off)
            && man.get(count_off + 1 + 2 * n) == Some(&0)
        {
            return Some((count_off + 1 + 2 * n + 1 - pc, Some(count_off)));
        }
    }
    match field_disasm::decode(man, pc) {
        Ok(insn) if insn.size > 0 => Some((insn.size, None)),
        _ => None,
    }
}

/// Every count-led line a clean script walk of a **decompressed Japanese
/// scene MAN** reaches, whose tokens read as text ([`qualifies`]), in offset
/// order. `text_off` is the first token byte, `len` the token bytes.
pub fn man_lines(man: &[u8]) -> Vec<Segment> {
    walk_leads(man)
        .into_iter()
        .filter_map(|lead| {
            let n = line_at(man, lead)?;
            let seg = Segment {
                text_off: lead + 1,
                len: 2 * n,
            };
            qualifies(&man[seg.text_off..seg.text_off + seg.len]).then_some(seg)
        })
        .collect()
}

/// Every count byte the Japanese script walk reaches as a line lead, before
/// the [`qualifies`] text test - the denominator of the coverage report.
pub fn walk_leads(man: &[u8]) -> Vec<usize> {
    let mut leads = Vec::new();
    for (pc0, end) in man_edit::record_script_windows(man) {
        let mut pc = pc0;
        while pc < end {
            let Some((size, lead)) = step(man, pc) else {
                break;
            };
            if let Some(lead) = lead {
                leads.push(lead);
            }
            pc += size;
        }
    }
    leads.sort_unstable();
    leads.dedup();
    leads
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_encode_round_trips_characters_and_escapes() {
        // "鍵が" + party-name escape + a non-round-tripping pair.
        let bytes = [0x8C, 0xAE, 0x82, 0xAA, 0xF1, 0x00, 0x81, 0x00];
        let text = decode(&bytes);
        assert_eq!(text, "鍵が{f1:00}{81:00}");
        assert_eq!(encode(&text).unwrap(), bytes);
    }

    #[test]
    fn qualifies_needs_only_characters_and_escapes() {
        assert!(qualifies(&[0x8C, 0xAE, 0xF1, 0x00]));
        assert!(qualifies(&[0xF1, 0x00])); // a name-substitution label
        assert!(!qualifies(&[0x26, 0x02, 0x8C, 0xAE])); // opcode bytes
        assert!(!qualifies(&[0x8C])); // odd length
    }

    #[test]
    fn step_sizes_bare_lines_and_balloons() {
        // Two bare lines then a post-page byte.
        let man = [0x01, 0x8C, 0xAE, 0x02, 0x82, 0xAA, 0x82, 0xAA, 0x24];
        assert_eq!(step(&man, 0), Some((3, Some(0))));
        assert_eq!(step(&man, 3), Some((5, Some(3))));
        // Balloon: 4C E1 N tokens 00.
        let balloon = [0x4C, 0xE1, 0x01, 0x8C, 0xAE, 0x00, 0x21];
        assert_eq!(step(&balloon, 0), Some((6, Some(2))));
    }

    #[test]
    fn step_sizes_a_shop_record() {
        // 49 00 len=0 count=2 ids name(N=1) 00
        let shop = [
            0x49, 0x00, 0x00, 0x02, 0x10, 0x11, 0x01, 0x8C, 0xAE, 0x00, 0x21,
        ];
        assert_eq!(step(&shop, 0), Some((10, Some(6))));
    }
}

//! Parser for Legaia MES (asset type `0x04`) blobs.
//!
//! # Container layout
//!
//! Two distinct on-disc layouts have been observed:
//!
//! - [`Format::Compact`] starts with magic `0x00000404` (LE bytes
//!   `04 04 00 00`), followed by header / runtime-patched pointers, an
//!   i16 array, a `(count + page_count + total_size)` triple, then a u24
//!   LE offset table, then bytecode at the end. Used for short message
//!   sets (~4 KB and below).
//!
//! - [`Format::Records`] has no fixed magic. It's a stream of
//!   variable-stride records marked by recurring `0x44 0x78` markers.
//!   Used for large NPC-dialog sets (~14 KB+ in the captured sample).
//!
//! # Bytecode encoding
//!
//! Reverse-engineered from the four SCUS interpreter functions
//! (`FUN_8003CA38` / `FUN_80036044` / `FUN_80036888` / `FUN_80036514`).
//! See [`docs/formats/mes.md`](../../../docs/formats/mes.md) for the full
//! per-byte table. Summary:
//!
//! - `0x00..0x1E` - end-of-message (loop terminator in the byte walker).
//! - `0x1F..0x5D`, `0x5F..0xBF` (excl. `0xC0..0xCF`), `0xD0..0xFE` -
//!   single-byte glyph indices.
//! - `0x5E XX` - input alias for `0xCE (XX-0x2D)`. Normalized in the
//!   iterator.
//! - `0xC0`, `0xC6`, `0xC8..0xCD` - 2-byte wide glyphs.
//! - `0xC1 XX` - substitute character name.
//! - `0xC2 XX` / `0xC4 XX` - substitute item name.
//! - `0xC3 XX` - substitute magic name.
//! - `0xC5 XX` - substitute spell name.
//! - `0xC7 XX` - substitute quest / terrain name.
//! - `0xCE XX` - spacing op (width-only, no glyph).
//! - `0xCF XX` - skip 2 bytes (`XX` is rendered alone).
//! - `0xFF` - input alias for `0xCF`. Normalized in the iterator.
//! - `0x80..0x9F` - surfaced as [`Token::Control`]: the per-byte SCUS
//!   walker treats these as glyphs, but the dialog window pager
//!   `FUN_801D84D0` tests `(byte & 0x7F) < 0x20` to halt on them. Most
//!   likely page-break / wait-for-input markers. Surface them so callers
//!   can route to whichever behaviour matches their renderer.
//!
//! Use [`parse`] for container detection + structural parse, and
//! [`iter_tokens`] to stream the bytecode.

#![forbid(unsafe_code)]

pub mod interp;
pub use interp::{
    DialogPlayer, EventStats, Interpreter, MesEvent, PlayerState, SubstituteKind,
    extract_all_messages, extract_message,
};

use anyhow::{Result, bail};
use serde::Serialize;

/// On-disc magic for [`Format::Compact`] (u32 LE = `0x00000404`).
pub const COMPACT_MAGIC: u32 = 0x0000_0404;

/// Recurring 2-byte marker that delimits records in [`Format::Records`].
pub const RECORD_MARKER: [u8; 2] = [0x44, 0x78];

/// Two distinct MES blob layouts observed in real RAM captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    /// Header + offset table + bytecode region. Starts with magic
    /// `0x00000404`. Smaller blobs (~4 KB).
    Compact,
    /// Variable-stride records marked by `0x44 0x78`. No fixed magic.
    /// Larger blobs (~14 KB+).
    Records,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::Compact => "compact",
            Format::Records => "records",
        }
    }
}

/// Detect which on-disc format a blob uses. Returns `None` if the
/// buffer matches neither pattern.
pub fn detect_format(buf: &[u8]) -> Option<Format> {
    if buf.len() >= 4 {
        let m = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        if m == COMPACT_MAGIC {
            return Some(Format::Compact);
        }
    }
    let marker_hits = (0..buf.len().saturating_sub(2))
        .filter(|&i| buf[i..i + 2] == RECORD_MARKER)
        .count();
    if marker_hits >= 4 {
        Some(Format::Records)
    } else {
        None
    }
}

/// 16-byte runtime header inside a [`Format::Compact`] blob at offset
/// `0x28`. Runtime patches these on load - for static parsing we just
/// expose the raw values.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct RuntimeHeader {
    pub back_ptr: u32,
    pub forward_ptr: u32,
    pub expanded_size: u32,
    pub count: u32,
}

/// One detected record-marker boundary in a [`Format::Records`] blob.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct RecordBoundary {
    pub offset: usize,
}

/// Parsed MES blob (whatever we can statically pull).
#[derive(Debug, Clone, Serialize)]
pub struct MesBlob {
    pub format: Format,
    pub size: usize,
    pub runtime_header: Option<RuntimeHeader>,
    pub offset_table: Option<Vec<u32>>,
    pub bytecode_offset: Option<usize>,
    pub records: Option<Vec<RecordBoundary>>,
}

/// Header layout constants for [`Format::Compact`].
pub mod compact {
    pub const RUNTIME_HDR_OFFSET: usize = 0x28;
    pub const I16_ARRAY_OFFSET: usize = 0x38;
    pub const PRE_TABLE_HDR_OFFSET: usize = 0x58;
    pub const OFFSET_TABLE_OFFSET: usize = 0x62;
    pub const OFFSET_TABLE_END: usize = 0xC8;
}

/// Detect format and parse what we can. Always returns `Ok` with whatever
/// fields could be filled - the structure is partial by design.
pub fn parse(buf: &[u8]) -> Result<MesBlob> {
    let format = detect_format(buf).ok_or_else(|| {
        anyhow::anyhow!(
            "buffer doesn't match any known MES format (first 4 bytes: {:02X?})",
            &buf.get(0..4).unwrap_or(&[])
        )
    })?;
    match format {
        Format::Compact => parse_compact(buf),
        Format::Records => parse_records(buf),
    }
}

fn parse_compact(buf: &[u8]) -> Result<MesBlob> {
    if buf.len() < compact::OFFSET_TABLE_END {
        bail!(
            "compact MES blob too small: need >= {} bytes, got {}",
            compact::OFFSET_TABLE_END,
            buf.len()
        );
    }
    let runtime_header = RuntimeHeader {
        back_ptr: u32_at(buf, compact::RUNTIME_HDR_OFFSET),
        forward_ptr: u32_at(buf, compact::RUNTIME_HDR_OFFSET + 4),
        expanded_size: u32_at(buf, compact::RUNTIME_HDR_OFFSET + 8),
        count: u32_at(buf, compact::RUNTIME_HDR_OFFSET + 12),
    };
    let mut offset_table = Vec::new();
    let mut i = compact::OFFSET_TABLE_OFFSET;
    while i + 3 <= compact::OFFSET_TABLE_END.min(buf.len()) {
        offset_table.push(u24_at(buf, i));
        i += 3;
    }
    Ok(MesBlob {
        format: Format::Compact,
        size: buf.len(),
        runtime_header: Some(runtime_header),
        offset_table: Some(offset_table),
        bytecode_offset: Some(compact::OFFSET_TABLE_END),
        records: None,
    })
}

fn parse_records(buf: &[u8]) -> Result<MesBlob> {
    let mut records = Vec::new();
    let mut i = 0usize;
    while i + 2 <= buf.len() {
        if buf[i..i + 2] == RECORD_MARKER {
            records.push(RecordBoundary { offset: i });
        }
        i += 1;
    }
    Ok(MesBlob {
        format: Format::Records,
        size: buf.len(),
        runtime_header: None,
        offset_table: None,
        bytecode_offset: None,
        records: Some(records),
    })
}

fn u32_at(buf: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
}

fn u24_at(buf: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], 0])
}

/// One logical operation in a MES bytecode stream. Encoding mirrors the
/// SCUS interpreter functions (`FUN_8003CA38` / `FUN_80036044` /
/// `FUN_80036888` / `FUN_80036514`); see crate-level docs for the byte
/// table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Token {
    /// Bytes `0x00..=0x1E`. The byte walker stops here. Carries the raw
    /// byte so callers can distinguish (e.g. 0x00 = standard end vs
    /// non-zero terminators that future analysis may differentiate).
    EndOfMessage(u8),
    /// Single-byte glyph index. Bytes `0x1F..=0x5D`, `0x5F..=0x7F`,
    /// `0xA0..=0xBF`, `0xD0..=0xFE` (i.e. anywhere outside the special
    /// ranges). The byte itself is the font tile index.
    Glyph(u8),
    /// 2-byte wide glyph: opcode byte (`0xC0`, `0xC6`, or `0xC8..=0xCD`)
    /// followed by its index byte. Stride 2.
    WideGlyph(u8, u8),
    /// Variable substitution: opcode byte `0xC1..=0xC5` or `0xC7`,
    /// followed by an index into the corresponding name table. Stride 2.
    Substitute { kind: SubstituteOpcode, arg: u8 },
    /// `0xCE XX` - spacing op. The renderer applies horizontal offset
    /// without emitting a glyph. Also produced by the input alias
    /// `0x5E XX` (rewritten by the substitution expander to
    /// `0xCE (XX-0x2D)`); the iterator does that normalisation
    /// transparently.
    Spacing(u8),
    /// `0xCF XX` - skip 2 bytes. The `0xFF` input alias for `0xCF` is
    /// normalised in the iterator (the synthetic arg byte is `0`).
    SkipTwo(u8),
    /// Bytes `0x80..=0x9F`. The per-byte SCUS walker treats these as
    /// glyphs, but the dialog window pager `FUN_801D84D0` halts on them
    /// (test `(byte & 0x7F) < 0x20`). Most likely page-break / wait-for-
    /// input markers. Surfaced typed so the [`crate::interp::DialogPlayer`]
    /// can route them to a page-pause without losing the byte value.
    Control(u8),
    /// Truncated 2-byte token at end of buffer (opcode read, arg byte
    /// missing). Carries the opcode so callers can re-sync.
    Truncated(u8),
}

/// Tag for [`Token::Substitute`] / [`MesEvent::Substitute`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SubstituteOpcode {
    /// `0xC1 XX` - substitute character name. Records live at
    /// `0x80084708 + XX*0x414`; `XX = 99` resolves to the current party
    /// leader (`DAT_80084597`).
    CharacterName,
    /// `0xC2 XX` - substitute item name from `PTR_DAT_8007436C[XX*3]`.
    ItemName,
    /// `0xC3 XX` - substitute magic name from `PTR_s_Magic_800754D0[XX*3]`.
    MagicName,
    /// `0xC4 XX` - substitute item name (different consumer site than
    /// `0xC2`; same `PTR_DAT_8007436C` table).
    ItemNameAlt,
    /// `0xC5 XX` - substitute spell name from 2D table at
    /// `DAT_80075EC4`, keyed by `(XX>>6, XX&0x3F)`.
    SpellName,
    /// `0xC7 XX` - substitute terrain / quest name from
    /// `DAT_80073F24 + XX*8`.
    QuestName,
}

impl SubstituteOpcode {
    pub fn raw_byte(self) -> u8 {
        match self {
            SubstituteOpcode::CharacterName => 0xC1,
            SubstituteOpcode::ItemName => 0xC2,
            SubstituteOpcode::MagicName => 0xC3,
            SubstituteOpcode::ItemNameAlt => 0xC4,
            SubstituteOpcode::SpellName => 0xC5,
            SubstituteOpcode::QuestName => 0xC7,
        }
    }
}

impl Token {
    /// Number of bytes this token occupied in the input stream (after
    /// alias normalisation: `0x5E XX` and `0xFF` both have stride 2).
    pub fn byte_len(self) -> usize {
        match self {
            Token::EndOfMessage(_) | Token::Glyph(_) | Token::Control(_) | Token::Truncated(_) => 1,
            Token::WideGlyph(_, _)
            | Token::Substitute { .. }
            | Token::Spacing(_)
            | Token::SkipTwo(_) => 2,
        }
    }

    /// `true` for [`Token::EndOfMessage`]. Used by callers that want to
    /// halt the iteration at end of message.
    pub fn is_terminal(self) -> bool {
        matches!(self, Token::EndOfMessage(_))
    }
}

/// Greedy bytecode walker. Starting at `start`, emit one [`Token`] at
/// a time until end of buffer. Stops naturally at the buffer end; an
/// [`Token::EndOfMessage`] is just data, not a hard stop, so the caller
/// can gather multiple messages in sequence.
pub fn iter_tokens(buf: &[u8], start: usize) -> TokenIter<'_> {
    TokenIter { buf, pos: start }
}

/// Iterator returned by [`iter_tokens`].
#[derive(Debug)]
pub struct TokenIter<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> TokenIter<'a> {
    pub fn pos(&self) -> usize {
        self.pos
    }
}

impl<'a> Iterator for TokenIter<'a> {
    type Item = (usize, Token);

    fn next(&mut self) -> Option<(usize, Token)> {
        let start_pos = self.pos;
        let op = *self.buf.get(self.pos)?;
        let token = classify_byte(op, self.buf, self.pos);
        self.pos += token.byte_len();
        Some((start_pos, token))
    }
}

/// Classify a single byte at `pos` into a [`Token`]. Reads up to one
/// byte ahead for 2-byte tokens. Aliases `0x5E`/`0xFF` are normalised
/// here so the consumer never sees them.
fn classify_byte(op: u8, buf: &[u8], pos: usize) -> Token {
    match op {
        // Hard terminator.
        0x00..=0x1E => Token::EndOfMessage(op),

        // Pager-control range (0x80..0x9F). Per-byte walker treats
        // these as glyphs; pager halts on them. Surface typed.
        0x80..=0x9F => Token::Control(op),

        // 0x5E XX -> alias for 0xCE (XX - 0x2D).
        0x5E => match buf.get(pos + 1) {
            Some(&arg) => Token::Spacing(arg.wrapping_sub(0x2D)),
            None => Token::Truncated(op),
        },

        // 0xFF -> alias for 0xCF. Per FUN_80036514, the substitution
        // expander rewrites the byte to 0xCF and then the surrounding
        // dispatch consumes the next byte as the arg (the standard
        // 2-byte 0xCF stride). Net: 0xFF is a 2-byte token.
        0xFF => match buf.get(pos + 1) {
            Some(&arg) => Token::SkipTwo(arg),
            None => Token::Truncated(op),
        },

        // Substitution opcodes (single arg byte).
        0xC1 => match buf.get(pos + 1) {
            Some(&arg) => Token::Substitute {
                kind: SubstituteOpcode::CharacterName,
                arg,
            },
            None => Token::Truncated(op),
        },
        0xC2 => match buf.get(pos + 1) {
            Some(&arg) => Token::Substitute {
                kind: SubstituteOpcode::ItemName,
                arg,
            },
            None => Token::Truncated(op),
        },
        0xC3 => match buf.get(pos + 1) {
            Some(&arg) => Token::Substitute {
                kind: SubstituteOpcode::MagicName,
                arg,
            },
            None => Token::Truncated(op),
        },
        0xC4 => match buf.get(pos + 1) {
            Some(&arg) => Token::Substitute {
                kind: SubstituteOpcode::ItemNameAlt,
                arg,
            },
            None => Token::Truncated(op),
        },
        0xC5 => match buf.get(pos + 1) {
            Some(&arg) => Token::Substitute {
                kind: SubstituteOpcode::SpellName,
                arg,
            },
            None => Token::Truncated(op),
        },
        0xC7 => match buf.get(pos + 1) {
            Some(&arg) => Token::Substitute {
                kind: SubstituteOpcode::QuestName,
                arg,
            },
            None => Token::Truncated(op),
        },

        // 0xCE XX - spacing op.
        0xCE => match buf.get(pos + 1) {
            Some(&arg) => Token::Spacing(arg),
            None => Token::Truncated(op),
        },

        // 0xCF XX - skip 2 bytes.
        0xCF => match buf.get(pos + 1) {
            Some(&arg) => Token::SkipTwo(arg),
            None => Token::Truncated(op),
        },

        // 2-byte wide glyphs (no substitution): 0xC0, 0xC6, 0xC8..0xCD.
        0xC0 | 0xC6 | 0xC8..=0xCD => match buf.get(pos + 1) {
            Some(&arg) => Token::WideGlyph(op, arg),
            None => Token::Truncated(op),
        },

        // Everything else is a 1-byte glyph: 0x1F..0x5D, 0x5F..0x7F,
        // 0xA0..0xBF, 0xD0..0xFE.
        _ => Token::Glyph(op),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_compact_by_magic() {
        let mut buf = vec![0u8; 256];
        buf[0..4].copy_from_slice(&COMPACT_MAGIC.to_le_bytes());
        assert_eq!(detect_format(&buf), Some(Format::Compact));
    }

    #[test]
    fn detects_records_by_marker_count() {
        let mut buf = vec![0xAAu8; 256];
        for i in (10..200).step_by(40) {
            buf[i] = 0x44;
            buf[i + 1] = 0x78;
        }
        assert_eq!(detect_format(&buf), Some(Format::Records));
    }

    #[test]
    fn returns_none_for_random_data() {
        // Use 0x99 as a fill byte. 0xAA happens to be in the 0xA0..0xBF
        // glyph range and *also* contains no `0x44 0x78` markers, so the
        // old test passed; 0x99 is in the Control range but the detector
        // only cares about magic/marker count, not token semantics.
        let buf = [0x99u8; 128];
        assert_eq!(detect_format(&buf), None);
    }

    #[test]
    fn compact_parses_runtime_header_and_offset_table() {
        let mut buf = vec![0u8; 512];
        buf[0..4].copy_from_slice(&COMPACT_MAGIC.to_le_bytes());
        buf[0x28..0x2C].copy_from_slice(&0x12345678u32.to_le_bytes());
        buf[0x2C..0x30].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
        buf[0x30..0x34].copy_from_slice(&0x00001CD4u32.to_le_bytes());
        buf[0x34..0x38].copy_from_slice(&100u32.to_le_bytes());
        buf[0x62] = 0xAA;
        buf[0x63] = 0xBB;
        buf[0x64] = 0xCC;

        let parsed = parse(&buf).expect("parse");
        assert_eq!(parsed.format, Format::Compact);
        let rh = parsed.runtime_header.unwrap();
        assert_eq!(rh.back_ptr, 0x12345678);
        assert_eq!(rh.forward_ptr, 0xDEADBEEF);
        assert_eq!(rh.expanded_size, 0x1CD4);
        assert_eq!(rh.count, 100);
        let table = parsed.offset_table.unwrap();
        assert_eq!(table[0], 0x00CCBBAA);
        assert_eq!(parsed.bytecode_offset, Some(compact::OFFSET_TABLE_END));
    }

    #[test]
    fn records_collects_all_marker_offsets() {
        let mut buf = vec![0u8; 256];
        let want = [10, 50, 100, 150, 200];
        for &off in &want {
            buf[off] = RECORD_MARKER[0];
            buf[off + 1] = RECORD_MARKER[1];
        }
        let parsed = parse(&buf).expect("parse");
        assert_eq!(parsed.format, Format::Records);
        let recs = parsed.records.unwrap();
        let got: Vec<usize> = recs.iter().map(|r| r.offset).collect();
        assert_eq!(got, want);
    }

    // --- Token classification ---------------------------------------------

    #[test]
    fn iter_tokens_classifies_glyphs_in_each_glyph_range() {
        // 0x21 (low ASCII), 0x40 (mid), 0xA5 (high), 0xD8 (highest range).
        let buf = [0x21, 0x40, 0xA5, 0xD8, 0x00];
        let toks: Vec<Token> = iter_tokens(&buf, 0).map(|(_, t)| t).collect();
        assert_eq!(
            toks,
            vec![
                Token::Glyph(0x21),
                Token::Glyph(0x40),
                Token::Glyph(0xA5),
                Token::Glyph(0xD8),
                Token::EndOfMessage(0x00),
            ]
        );
    }

    #[test]
    fn iter_tokens_classifies_substitutions() {
        let buf = [
            0xC1, 0x05, // character name 5
            0xC2, 0x10, // item name 0x10
            0xC3, 0x20, // magic name 0x20
            0xC4, 0x11, // item-name alt
            0xC5, 0x42, // spell name 0x42
            0xC7, 0x07, // quest name 7
            0x00,
        ];
        let toks: Vec<Token> = iter_tokens(&buf, 0).map(|(_, t)| t).collect();
        assert_eq!(
            toks,
            vec![
                Token::Substitute {
                    kind: SubstituteOpcode::CharacterName,
                    arg: 0x05
                },
                Token::Substitute {
                    kind: SubstituteOpcode::ItemName,
                    arg: 0x10
                },
                Token::Substitute {
                    kind: SubstituteOpcode::MagicName,
                    arg: 0x20
                },
                Token::Substitute {
                    kind: SubstituteOpcode::ItemNameAlt,
                    arg: 0x11
                },
                Token::Substitute {
                    kind: SubstituteOpcode::SpellName,
                    arg: 0x42
                },
                Token::Substitute {
                    kind: SubstituteOpcode::QuestName,
                    arg: 0x07
                },
                Token::EndOfMessage(0x00),
            ]
        );
    }

    #[test]
    fn iter_tokens_classifies_wide_glyphs() {
        // 0xC0 + 0xC6 + 0xC8..=0xCD are all 2-byte wide glyphs.
        let buf = [0xC0, 0x10, 0xC6, 0x20, 0xCA, 0x30, 0x00];
        let toks: Vec<Token> = iter_tokens(&buf, 0).map(|(_, t)| t).collect();
        assert_eq!(
            toks,
            vec![
                Token::WideGlyph(0xC0, 0x10),
                Token::WideGlyph(0xC6, 0x20),
                Token::WideGlyph(0xCA, 0x30),
                Token::EndOfMessage(0x00),
            ]
        );
    }

    #[test]
    fn iter_tokens_classifies_spacing_and_skip() {
        let buf = [0xCE, 0x42, 0xCF, 0x99, 0x00];
        let toks: Vec<Token> = iter_tokens(&buf, 0).map(|(_, t)| t).collect();
        assert_eq!(
            toks,
            vec![
                Token::Spacing(0x42),
                Token::SkipTwo(0x99),
                Token::EndOfMessage(0x00),
            ]
        );
    }

    #[test]
    fn iter_tokens_normalises_5e_alias_to_spacing() {
        // 0x5E XX -> Spacing(XX - 0x2D). Per FUN_80036514.
        let buf = [0x5E, 0x40, 0x00];
        let toks: Vec<Token> = iter_tokens(&buf, 0).map(|(_, t)| t).collect();
        assert_eq!(
            toks,
            vec![Token::Spacing(0x40 - 0x2D), Token::EndOfMessage(0x00)]
        );
    }

    #[test]
    fn iter_tokens_normalises_ff_alias_to_skiptwo() {
        // 0xFF aliases to 0xCF and consumes the next byte as arg
        // (2-byte stride end-to-end, matching the substitution
        // expander's rewrite + 0xCF dispatch).
        let buf = [0xFF, 0x42, 0x00];
        let toks: Vec<Token> = iter_tokens(&buf, 0).map(|(_, t)| t).collect();
        assert_eq!(toks, vec![Token::SkipTwo(0x42), Token::EndOfMessage(0x00)]);
    }

    #[test]
    fn iter_tokens_truncated_lone_ff() {
        // 0xFF at end of buffer with no arg byte -> Truncated.
        let buf = [0xFF];
        let toks: Vec<Token> = iter_tokens(&buf, 0).map(|(_, t)| t).collect();
        assert_eq!(toks, vec![Token::Truncated(0xFF)]);
    }

    #[test]
    fn iter_tokens_classifies_control_range() {
        // 0x80..0x9F surface as Control bytes.
        let buf = [0x80, 0x88, 0x9F, 0x00];
        let toks: Vec<Token> = iter_tokens(&buf, 0).map(|(_, t)| t).collect();
        assert_eq!(
            toks,
            vec![
                Token::Control(0x80),
                Token::Control(0x88),
                Token::Control(0x9F),
                Token::EndOfMessage(0x00),
            ]
        );
    }

    #[test]
    fn iter_tokens_handles_truncated_2byte_opcode() {
        let buf = [0xC1];
        let toks: Vec<Token> = iter_tokens(&buf, 0).map(|(_, t)| t).collect();
        assert_eq!(toks, vec![Token::Truncated(0xC1)]);
    }

    #[test]
    fn iter_tokens_treats_eom_as_data_not_iterator_stop() {
        // Two messages back-to-back; the iterator surfaces both ends.
        let buf = [0x21, 0x00, 0x22, 0x00];
        let toks: Vec<Token> = iter_tokens(&buf, 0).map(|(_, t)| t).collect();
        assert_eq!(
            toks,
            vec![
                Token::Glyph(0x21),
                Token::EndOfMessage(0x00),
                Token::Glyph(0x22),
                Token::EndOfMessage(0x00),
            ]
        );
    }

    #[test]
    fn token_byte_len_matches_iter_advance() {
        // Empirically: classify_byte's stride equals what iter_tokens uses
        // to advance pos. This exercises every variant.
        let cases: Vec<(u8, &[u8])> = vec![
            (0x00, &[0x00]),
            (0x21, &[0x21]),
            (0x80, &[0x80]),
            (0xC1, &[0xC1, 0x05]),
            (0xCE, &[0xCE, 0x42]),
            (0xCF, &[0xCF, 0x99]),
            (0xC0, &[0xC0, 0x10]),
            (0xFF, &[0xFF]),
        ];
        for (op, buf) in cases {
            let tok = classify_byte(op, buf, 0);
            // For Truncated cases (e.g. lone 0xC1), byte_len is 1.
            assert!(tok.byte_len() <= buf.len() || matches!(tok, Token::Truncated(_)));
        }
    }

    // --- Adversarial-input fuzz -------------------------------------------

    /// `detect_format` / `parse` / `iter_tokens` run against arbitrary
    /// LZS-decoded PROT bytes by the asset scanner and web viewer. Random
    /// soup of every length 0..512, plus a copy forced to carry the compact
    /// magic, must never panic.
    #[test]
    fn parsers_never_panic_on_random_bytes() {
        for seed in 0u64..400 {
            let mut x = seed.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(7);
            let n = (seed % 512) as usize;
            let mut buf = Vec::with_capacity(n);
            for _ in 0..n {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                buf.push(x as u8);
            }
            let _ = detect_format(&buf);
            let _ = parse(&buf);
            // Walk tokens from a few start offsets - the iterator must
            // terminate (every token advances pos by >= 1) and never index OOB.
            for &start in &[0usize, buf.len() / 2, buf.len().saturating_sub(1)] {
                let _: Vec<_> = iter_tokens(&buf, start).collect();
            }
            // Force the compact magic so the compact path runs on junk too.
            if buf.len() >= 4 {
                let mut compact = buf.clone();
                compact[0..4].copy_from_slice(&COMPACT_MAGIC.to_le_bytes());
                let _ = parse(&compact);
                let _ = extract_all_messages(&compact);
                let _ = extract_message(&compact, 0);
            }
        }
    }

    /// Token iteration always terminates: every emitted token consumes at
    /// least one input byte, so the total emitted equals exactly the bytes
    /// walked. (Guards against a future stride-0 token reintroducing a hang.)
    #[test]
    fn token_iteration_always_makes_progress() {
        let buf: Vec<u8> = (0u16..=255).map(|b| b as u8).collect();
        let mut last = 0usize;
        let mut it = iter_tokens(&buf, 0);
        while let Some((pos, _)) = it.next() {
            assert!(it.pos() > pos, "token at {pos} did not advance the cursor");
            last = it.pos();
        }
        assert!(last >= buf.len());
    }
}

//! Partial parser for Legaia MES (asset type `0x04`) blobs.
//!
//! What we know (from `project_mes_blob_extracted.md`):
//!
//! * MES is the SCUS asset-type byte `0x04` in the dispatcher
//!   `FUN_8001f05c`. The dispatcher just allocates a 4-byte-aligned buffer
//!   and decodes the payload (LZS or raw); the bytecode interpreter and
//!   format-specific parsing live in an overlay we haven't fully reversed.
//!
//! * **Two distinct on-disc layouts have been observed** in real RAM
//!   captures (a town-init blob and an in-dialog blob):
//!
//!   - [`Format::Compact`] starts with magic `0x00000404` (LE bytes
//!     `04 04 00 00`) followed by 36 zero bytes, a 16-byte header of
//!     runtime-patched pointers, a 32-byte i16 array, an 8-byte
//!     "count + size" word pair, then a u24 LE offset table, then
//!     bytecode at the end. Used for short message sets (~4 KB).
//!
//!   - [`Format::Records`] has no fixed magic. It's a stream of
//!     variable-stride records marked by recurring `0x44 0x78` markers
//!     (typically every 20–36 bytes). Used for large NPC-dialog sets
//!     (~14 KB+ in the captured sample).
//!
//! * Bytecode tokens observed (in `Format::Compact`'s bytecode region):
//!
//!   - `0x00`: end-of-message terminator (15.4% of bytes — very common).
//!   - `0x61 XX`: print glyph `XX` (confirmed by an observed sequential
//!     run `61 9D 61 9E 61 9F ... 61 AA` — clearly an alphabet sequence).
//!   - `0x65 XX`: similar single-byte-arg opcode (likely "small numeric"
//!     or "wait N frames" — semantics not yet confirmed).
//!   - `0x4C XX`: 2-byte token (1-byte arg) — recurring control.
//!   - `0x26 XX YY`: 3-byte token (2-byte arg) — possibly "page break"
//!     when arg is `0xFEFF`.
//!   - `0x21 0x21 0x26 0xFE 0xFF`: recurring 5-byte sequence — likely a
//!     fixed page-break / message-boundary marker.
//!
//! All other opcodes are emitted as [`Token::Unknown`] with the raw
//! byte; future reverse-engineering of the bytecode interpreter (when a
//! dialog-rendering overlay is captured) will fill in the meanings.
//!
//! # What this crate does NOT do
//!
//! * Decode the bytecode to readable text. The glyph→character mapping
//!   needs a font tile sheet that hasn't been located yet.
//! * Validate offset tables against the bytecode region. The offset
//!   table base/encoding (u24 LE vs another stride) is empirical and
//!   not yet cross-checked against the interpreter.
//! * Handle `Format::Records` beyond locating record boundaries.
//!
//! Use [`parse`] to detect the format and pull what we know; use
//! [`iter_tokens`] to stream the bytecode for downstream analysis.

#![forbid(unsafe_code)]

pub mod interp;
pub use interp::{
    DialogPlayer, EventStats, Interpreter, MesEvent, PlayerState, extract_all_messages,
    extract_message,
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
///
/// Detection is strict-first: if the first u32 is the [`COMPACT_MAGIC`]
/// it's compact, regardless of marker count. Only blobs without that
/// magic are checked for the records pattern (≥ 4 marker hits).
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

/// 12-byte "runtime header" inside a [`Format::Compact`] blob at offset
/// `0x28`. Runtime patches these on load — for static parsing we just
/// expose the raw values.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct RuntimeHeader {
    /// `+0x28`: back-pointer (set by allocator on load).
    pub back_ptr: u32,
    /// `+0x2C`: forward-pointer to expanded buffer end.
    pub forward_ptr: u32,
    /// `+0x30`: expanded buffer size (u32 LE).
    pub expanded_size: u32,
    /// `+0x34`: count of something (entries? pages? — unconfirmed).
    pub count: u32,
}

/// One detected record-marker boundary in a [`Format::Records`] blob.
/// Records are NOT all the same size — store the start offset and let
/// the caller compute lengths from successive boundaries.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct RecordBoundary {
    /// Byte offset of the first byte of the record marker (`0x44`).
    pub offset: usize,
}

/// Parsed MES blob (whatever we can statically pull).
#[derive(Debug, Clone, Serialize)]
pub struct MesBlob {
    pub format: Format,
    pub size: usize,
    /// `Some` only for [`Format::Compact`].
    pub runtime_header: Option<RuntimeHeader>,
    /// `Some` only for [`Format::Compact`]. Each entry is a u24 LE value
    /// from the offset table region. Bases / interpretation are
    /// empirical — the interpreter that consumes these is overlay-resident.
    pub offset_table: Option<Vec<u32>>,
    /// `Some` only for [`Format::Compact`]. Where the bytecode region
    /// begins (best-effort: end of the offset table).
    pub bytecode_offset: Option<usize>,
    /// `Some` only for [`Format::Records`]. Boundaries of each
    /// `0x44 0x78` marker hit.
    pub records: Option<Vec<RecordBoundary>>,
}

/// Header layout constants for [`Format::Compact`]. Derived from the
/// 3893-byte town-init blob extracted from a save state. Exposed as
/// public API so callers (and future overlay-side reverse-engineering
/// scripts) can reference the same constants we used to parse.
pub mod compact {
    /// 16-byte runtime header at `+0x28` (back-ptr / forward-ptr /
    /// size / count) — runtime-patched by the allocator on load.
    pub const RUNTIME_HDR_OFFSET: usize = 0x28;
    /// 32-byte i16 array at `+0x38` (coords / offsets — semantics
    /// unconfirmed; observed values look like a row of x-coordinates).
    pub const I16_ARRAY_OFFSET: usize = 0x38;
    /// `u16 + u16 + u32` at `+0x58` immediately preceding the offset
    /// table. Often (count, page_count, total_size).
    pub const PRE_TABLE_HDR_OFFSET: usize = 0x58;
    /// Offset-table region begins here. Entries are u24 LE values.
    /// The 2 bytes at `+0x60..+0x62` are zero padding (likely so the
    /// u24 entries are 2-byte-aligned within the wider header layout).
    pub const OFFSET_TABLE_OFFSET: usize = 0x62;
    /// Empirical end of the offset table in the captured sample;
    /// matches the first byte of the bytecode region.
    pub const OFFSET_TABLE_END: usize = 0xC8;
}

/// Detect format and parse what we can. Always returns `Ok` with whatever
/// fields could be filled — the structure is partial by design.
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
    // Offset table: u24 LE entries between OFFSET_TABLE_OFFSET and
    // OFFSET_TABLE_END. We expose the raw values; consumers decide
    // whether to drop zero entries / skip header padding.
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

/// Bytecode token (one logical operation). Lengths are best-effort:
/// known opcodes use the lengths we've inferred; everything else is
/// emitted as [`Token::Unknown`] with a single byte so the caller can
/// re-sync at the next byte if a span is misclassified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Token {
    /// `0x00`: end-of-message terminator. The most common single byte
    /// in observed bytecode (~15% of bytes in the text region).
    End,
    /// `0x61 XX`: "print glyph XX". The XX is a glyph index into a
    /// font tile sheet that hasn't been located yet.
    Glyph(u8),
    /// `0x65 XX`: 2-byte token with 1-byte arg, semantics unconfirmed
    /// but seen alongside [`Token::Glyph`] tokens.
    Op65(u8),
    /// `0x4C XX`: 2-byte control (1-byte arg) — semantics unconfirmed.
    Op4c(u8),
    /// `0x26 XX YY`: 3-byte control (2-byte little-endian arg). When
    /// arg is `0xFEFF` this is likely a page-break / message-boundary
    /// marker; the recurring 5-byte sequence `21 21 26 FE FF` makes
    /// that interpretation plausible.
    Op26 { arg: u16 },
    /// Any opcode we haven't classified. Always 1 byte so callers can
    /// re-sync at the next position.
    Unknown(u8),
}

impl Token {
    /// Number of bytes this token occupies in the input stream.
    pub fn byte_len(self) -> usize {
        match self {
            Token::End | Token::Unknown(_) => 1,
            Token::Glyph(_) | Token::Op65(_) | Token::Op4c(_) => 2,
            Token::Op26 { .. } => 3,
        }
    }
}

/// Greedy bytecode walker. Starting at `start`, emit one [`Token`] at
/// a time until end of buffer. Stops naturally at the buffer end; an
/// [`Token::End`] is just data, not a hard stop, so the caller can
/// gather multiple messages in sequence.
pub fn iter_tokens(buf: &[u8], start: usize) -> TokenIter<'_> {
    TokenIter { buf, pos: start }
}

/// Iterator returned by [`iter_tokens`]. Tracks position so callers can
/// inspect [`TokenIter::pos`] after iteration.
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
        let token = match op {
            0x00 => Token::End,
            0x61 => match self.buf.get(self.pos + 1) {
                Some(&arg) => Token::Glyph(arg),
                None => Token::Unknown(op),
            },
            0x65 => match self.buf.get(self.pos + 1) {
                Some(&arg) => Token::Op65(arg),
                None => Token::Unknown(op),
            },
            0x4C => match self.buf.get(self.pos + 1) {
                Some(&arg) => Token::Op4c(arg),
                None => Token::Unknown(op),
            },
            0x26 => {
                let lo = self.buf.get(self.pos + 1).copied();
                let hi = self.buf.get(self.pos + 2).copied();
                match (lo, hi) {
                    (Some(l), Some(h)) => Token::Op26 {
                        arg: u16::from_le_bytes([l, h]),
                    },
                    _ => Token::Unknown(op),
                }
            }
            other => Token::Unknown(other),
        };
        self.pos += token.byte_len();
        Some((start_pos, token))
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
        // Spread 5 record markers across a buffer.
        let mut buf = vec![0xAAu8; 256];
        for i in (10..200).step_by(40) {
            buf[i] = 0x44;
            buf[i + 1] = 0x78;
        }
        assert_eq!(detect_format(&buf), Some(Format::Records));
    }

    #[test]
    fn returns_none_for_random_data() {
        let buf = [0xAAu8; 128];
        assert_eq!(detect_format(&buf), None);
    }

    #[test]
    fn compact_parses_runtime_header_and_offset_table() {
        let mut buf = vec![0u8; 512];
        buf[0..4].copy_from_slice(&COMPACT_MAGIC.to_le_bytes());
        // Runtime header at +0x28: back, forward, expanded_size, count
        buf[0x28..0x2C].copy_from_slice(&0x12345678u32.to_le_bytes());
        buf[0x2C..0x30].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
        buf[0x30..0x34].copy_from_slice(&0x00001CD4u32.to_le_bytes());
        buf[0x34..0x38].copy_from_slice(&100u32.to_le_bytes());
        // First u24 entry in offset table at +0x62 (2 zero pad bytes
        // at +0x60..+0x61 — see `compact::OFFSET_TABLE_OFFSET`).
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
        // Place markers at known offsets — 4 hits triggers detection.
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

    #[test]
    fn iter_tokens_classifies_known_opcodes() {
        // Build a tiny bytecode program: glyph A, op65, op4c, op26 page-break, end.
        let buf = [0x61, 0x9D, 0x65, 0x01, 0x4C, 0x20, 0x26, 0xFE, 0xFF, 0x00];
        let toks: Vec<Token> = iter_tokens(&buf, 0).map(|(_, t)| t).collect();
        assert_eq!(
            toks,
            vec![
                Token::Glyph(0x9D),
                Token::Op65(0x01),
                Token::Op4c(0x20),
                Token::Op26 { arg: 0xFFFE },
                Token::End,
            ]
        );
    }

    #[test]
    fn iter_tokens_emits_unknown_for_unrecognized_byte() {
        let buf = [0x99, 0x61, 0x9D];
        let toks: Vec<Token> = iter_tokens(&buf, 0).map(|(_, t)| t).collect();
        assert_eq!(toks, vec![Token::Unknown(0x99), Token::Glyph(0x9D)]);
    }

    #[test]
    fn iter_tokens_handles_truncated_2byte_opcode() {
        // Buffer ends mid-token — we emit Unknown(op) so the caller sees
        // something rather than silently dropping the partial.
        let buf = [0x61];
        let toks: Vec<Token> = iter_tokens(&buf, 0).map(|(_, t)| t).collect();
        assert_eq!(toks, vec![Token::Unknown(0x61)]);
    }
}

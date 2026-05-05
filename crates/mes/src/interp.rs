//! MES bytecode interpreter.
//!
//! Reads tokens from [`crate::iter_tokens`] and surfaces higher-level
//! [`MesEvent`]s the engine can consume: glyphs, page breaks, end-of-message
//! sentinels. Selecting a specific message uses the offset table from
//! [`crate::Format::Compact`].
//!
//! The interpreter is intentionally permissive — unknown opcodes surface as
//! [`MesEvent::Unknown`] and don't halt the stream. This matches the retail
//! dialog renderer's "skip unknown control" behaviour we observed at runtime
//! (no crashes on malformed input; the renderer just keeps walking).
//!
//! The opcode catalogue here mirrors the partial reverse in `crate::Token` —
//! the rest of the dispatch lives in the dialog overlay (uncaptured), so
//! some semantics are inferred from observed bytecode patterns and runtime
//! behaviour rather than a Ghidra-traced switch.

use crate::{Format, MesBlob, Token, TokenIter, iter_tokens, parse};
use anyhow::{Result, bail};
use serde::Serialize;

/// Higher-level event surfaced by [`Interpreter::next_event`]. The engine
/// consumes these to drive dialog rendering: glyphs feed the font renderer,
/// page breaks pause for input, end-of-message tears down the dialog window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum MesEvent {
    /// `0x61 XX` — print one glyph. The XX is a glyph index into the dialog
    /// font tile sheet (extraction is still blocked — see
    /// `docs/formats/dialog-font.md`).
    Glyph(u8),
    /// `0x00` — end-of-message sentinel. The interpreter stops here unless
    /// [`Interpreter::run_past_end`] is set; the renderer typically tears
    /// the dialog window down on this event.
    EndOfMessage,
    /// `0x26 0xFE 0xFF` — page break. Inferred from the recurring 5-byte
    /// sequence `21 21 26 FE FF` and the renderer's "wait for input" UX.
    /// The renderer pauses for player input before continuing to the next
    /// page.
    PageBreak,
    /// `0x65 XX` — semantics unconfirmed. Surfaced as a typed event with the
    /// raw arg so consumers can dispatch by table once the dispatch is
    /// reversed.
    Op65 { arg: u8 },
    /// `0x4C XX` — semantics unconfirmed. Same caveat as `Op65`.
    Op4c { arg: u8 },
    /// `0x26 XX YY` with arg != `0xFEFF`. Probably another control with a
    /// 2-byte argument; surfaced as-is.
    Op26 { arg: u16 },
    /// Any opcode we haven't classified. Always 1 byte so the interpreter
    /// re-syncs at the next byte (matches the in-format `Token::Unknown`).
    Unknown { opcode: u8 },
}

impl MesEvent {
    /// `true` if this event should halt the interpreter (renderer would tear
    /// down the dialog window).
    pub fn is_terminal(&self) -> bool {
        matches!(self, MesEvent::EndOfMessage)
    }
}

/// MES bytecode interpreter. Walks tokens and emits [`MesEvent`]s.
///
/// Construction: [`Interpreter::new_compact`] uses the offset table to seed
/// the program counter; [`Interpreter::new_at`] takes a raw byte offset for
/// callers that want to walk the bytecode without going through the offset
/// table.
#[derive(Debug)]
pub struct Interpreter<'a> {
    inner: TokenIter<'a>,
    /// Set to true to keep walking past `EndOfMessage` (useful for streaming
    /// multi-message blobs without re-seeking). Default false.
    pub run_past_end: bool,
    /// Set to true to coalesce a `PageBreak` (0x26 0xFEFF) into a sequence
    /// of two events: `Glyph(0x21)` `Glyph(0x21)` are *not* dropped — they
    /// are the punctuation that the page-break sequence `21 21 26 FE FF`
    /// happens to begin with. Default true.
    pub emit_punctuation_before_page_break: bool,
    /// Whether the interpreter has emitted a terminal event. After this is
    /// set, subsequent calls to [`Interpreter::next_event`] return `None`
    /// unless `run_past_end` is set.
    halted: bool,
}

impl<'a> Interpreter<'a> {
    /// Build an interpreter that walks bytecode starting at the offset for
    /// message `index` in a [`Format::Compact`] blob.
    ///
    /// The offset table holds u24 LE values whose interpretation is
    /// empirical (the dialog overlay isn't fully reversed). We treat them
    /// as byte offsets into the bytecode region (which begins at
    /// [`crate::compact::OFFSET_TABLE_END`]). Engines that find this
    /// interpretation wrong can use [`Interpreter::new_at`] with a literal
    /// PC.
    pub fn new_compact(blob: &'a MesBlob, buf: &'a [u8], message_index: usize) -> Result<Self> {
        if blob.format != Format::Compact {
            bail!("new_compact requires a compact blob, got {:?}", blob.format);
        }
        let table = blob.offset_table.as_ref().ok_or_else(|| {
            anyhow::anyhow!("compact blob has no offset table — was it parsed correctly?")
        })?;
        let entry = table
            .get(message_index)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("message index {message_index} out of range"))?;
        let bytecode_offset = blob.bytecode_offset.ok_or_else(|| {
            anyhow::anyhow!("compact blob has no bytecode offset — parse failure?")
        })?;
        let pc = bytecode_offset + entry as usize;
        if pc >= buf.len() {
            bail!(
                "computed pc {pc} (bytecode @ {bytecode_offset} + entry {entry}) is past buffer end {}",
                buf.len()
            );
        }
        Ok(Self::new_at(buf, pc))
    }

    /// Build an interpreter that walks bytecode starting at `pc` (a byte
    /// offset into `buf`).
    pub fn new_at(buf: &'a [u8], pc: usize) -> Self {
        Self {
            inner: iter_tokens(buf, pc),
            run_past_end: false,
            emit_punctuation_before_page_break: true,
            halted: false,
        }
    }

    /// Current PC — byte offset of the next token to read. Mirrors
    /// [`TokenIter::pos`].
    pub fn pc(&self) -> usize {
        self.inner.pos()
    }

    /// Pull one [`MesEvent`]. Returns `None` once the interpreter has
    /// halted on `EndOfMessage` (unless `run_past_end` was set), or when
    /// the underlying buffer runs out.
    pub fn next_event(&mut self) -> Option<MesEvent> {
        if self.halted && !self.run_past_end {
            return None;
        }
        let (_, tok) = self.inner.next()?;
        let ev = self.event_from_token(tok);
        if ev.is_terminal() {
            self.halted = true;
        }
        Some(ev)
    }

    /// Drain all events into a `Vec`. Convenience wrapper for tests / one-
    /// shot extraction. Halts at `EndOfMessage` unless `run_past_end` is
    /// set.
    pub fn collect_events(&mut self) -> Vec<MesEvent> {
        let mut out = Vec::new();
        while let Some(ev) = self.next_event() {
            let terminal = ev.is_terminal();
            out.push(ev);
            if terminal && !self.run_past_end {
                break;
            }
        }
        out
    }

    /// Build a glyph-index string from the bytecode. Useful for diffing
    /// captures: every glyph emits its u8 index as a hex byte; control
    /// events render as bracketed names. Until the font is extracted, this
    /// is the closest thing we have to "render this message."
    pub fn render_summary(events: &[MesEvent]) -> String {
        let mut out = String::new();
        for ev in events {
            match ev {
                MesEvent::Glyph(idx) => {
                    out.push_str(&format!("{idx:02X} "));
                }
                MesEvent::EndOfMessage => out.push_str("[END]"),
                MesEvent::PageBreak => out.push_str("[PAGE]"),
                MesEvent::Op65 { arg } => out.push_str(&format!("[op65:{arg:02X}]")),
                MesEvent::Op4c { arg } => out.push_str(&format!("[op4c:{arg:02X}]")),
                MesEvent::Op26 { arg } => out.push_str(&format!("[op26:{arg:04X}]")),
                MesEvent::Unknown { opcode } => out.push_str(&format!("[?{opcode:02X}]")),
            }
        }
        out.trim_end().to_string()
    }

    fn event_from_token(&self, token: Token) -> MesEvent {
        match token {
            Token::End => MesEvent::EndOfMessage,
            Token::Glyph(g) => MesEvent::Glyph(g),
            Token::Op65(arg) => MesEvent::Op65 { arg },
            Token::Op4c(arg) => MesEvent::Op4c { arg },
            Token::Op26 { arg } => {
                if arg == 0xFFFE {
                    MesEvent::PageBreak
                } else {
                    MesEvent::Op26 { arg }
                }
            }
            Token::Unknown(opcode) => MesEvent::Unknown { opcode },
        }
    }
}

/// Convenience: parse a buffer, look up message at `index`, and stream all
/// events into a Vec. Equivalent to:
///
/// ```text
/// let blob = parse(buf)?;
/// let mut interp = Interpreter::new_compact(&blob, buf, index)?;
/// interp.collect_events()
/// ```
pub fn extract_message(buf: &[u8], index: usize) -> Result<Vec<MesEvent>> {
    let blob = parse(buf)?;
    let mut interp = Interpreter::new_compact(&blob, buf, index)?;
    Ok(interp.collect_events())
}

/// Yield every message in a [`Format::Compact`] blob, by walking the offset
/// table from start to end. Stops at the first table entry that points past
/// the buffer end (such entries are typically padding zeros).
pub fn extract_all_messages(buf: &[u8]) -> Result<Vec<Vec<MesEvent>>> {
    let blob = parse(buf)?;
    if blob.format != Format::Compact {
        bail!(
            "extract_all_messages requires a compact blob, got {:?}",
            blob.format
        );
    }
    let table_len = blob.offset_table.as_ref().map(|t| t.len()).unwrap_or(0);
    let mut out = Vec::new();
    for i in 0..table_len {
        let mut interp = match Interpreter::new_compact(&blob, buf, i) {
            Ok(it) => it,
            Err(_) => break,
        };
        out.push(interp.collect_events());
    }
    Ok(out)
}

/// Counted summary of an event sequence — useful for QA / corpus stats.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct EventStats {
    pub glyphs: usize,
    pub page_breaks: usize,
    pub op65: usize,
    pub op4c: usize,
    pub op26: usize,
    pub unknowns: usize,
    pub end_of_message: usize,
}

impl EventStats {
    pub fn from_events(events: &[MesEvent]) -> Self {
        let mut s = Self::default();
        for ev in events {
            match ev {
                MesEvent::Glyph(_) => s.glyphs += 1,
                MesEvent::PageBreak => s.page_breaks += 1,
                MesEvent::Op65 { .. } => s.op65 += 1,
                MesEvent::Op4c { .. } => s.op4c += 1,
                MesEvent::Op26 { .. } => s.op26 += 1,
                MesEvent::Unknown { .. } => s.unknowns += 1,
                MesEvent::EndOfMessage => s.end_of_message += 1,
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::COMPACT_MAGIC;

    /// Build a minimal compact blob with one message at offset 0 of the
    /// bytecode region. Used to drive end-to-end tests of the interpreter.
    fn build_compact_with_program(prog: &[u8]) -> Vec<u8> {
        let mut buf = vec![0u8; crate::compact::OFFSET_TABLE_END + prog.len()];
        buf[0..4].copy_from_slice(&COMPACT_MAGIC.to_le_bytes());
        // First offset table entry → 0 (start of bytecode region).
        buf[crate::compact::OFFSET_TABLE_OFFSET] = 0;
        buf[crate::compact::OFFSET_TABLE_OFFSET + 1] = 0;
        buf[crate::compact::OFFSET_TABLE_OFFSET + 2] = 0;
        // Second entry → past the program (so `extract_all_messages` stops
        // cleanly at the second entry).
        let past_end = (prog.len() + 1000) as u32;
        buf[crate::compact::OFFSET_TABLE_OFFSET + 3] = past_end as u8;
        buf[crate::compact::OFFSET_TABLE_OFFSET + 4] = (past_end >> 8) as u8;
        buf[crate::compact::OFFSET_TABLE_OFFSET + 5] = (past_end >> 16) as u8;
        // Bytecode at OFFSET_TABLE_END.
        buf[crate::compact::OFFSET_TABLE_END..].copy_from_slice(prog);
        buf
    }

    #[test]
    fn interpreter_emits_glyphs_and_terminator() {
        // Program: glyph A, glyph B, end.
        let prog = [0x61, 0x9D, 0x61, 0x9E, 0x00];
        let buf = build_compact_with_program(&prog);
        let evs = extract_message(&buf, 0).unwrap();
        assert_eq!(
            evs,
            vec![
                MesEvent::Glyph(0x9D),
                MesEvent::Glyph(0x9E),
                MesEvent::EndOfMessage,
            ]
        );
    }

    #[test]
    fn interpreter_recognises_page_break() {
        // Program: glyph, page break (26 FE FF), glyph, end.
        let prog = [0x61, 0x9D, 0x26, 0xFE, 0xFF, 0x61, 0x9E, 0x00];
        let buf = build_compact_with_program(&prog);
        let evs = extract_message(&buf, 0).unwrap();
        assert_eq!(
            evs,
            vec![
                MesEvent::Glyph(0x9D),
                MesEvent::PageBreak,
                MesEvent::Glyph(0x9E),
                MesEvent::EndOfMessage,
            ]
        );
    }

    #[test]
    fn interpreter_recognises_recurring_page_break_sequence() {
        // The observed `21 21 26 FE FF` pattern: the 21s are punctuation
        // glyphs (likely `!!`), then the page break.
        let prog = [0x21, 0x21, 0x26, 0xFE, 0xFF, 0x00];
        let buf = build_compact_with_program(&prog);
        let evs = extract_message(&buf, 0).unwrap();
        assert_eq!(
            evs,
            vec![
                MesEvent::Unknown { opcode: 0x21 },
                MesEvent::Unknown { opcode: 0x21 },
                MesEvent::PageBreak,
                MesEvent::EndOfMessage,
            ]
        );
    }

    #[test]
    fn interpreter_halts_at_end_of_message() {
        // Two messages back to back; the first `End` should halt.
        let prog = [0x61, 0x9D, 0x00, 0x61, 0x9E, 0x00];
        let buf = build_compact_with_program(&prog);
        let evs = extract_message(&buf, 0).unwrap();
        assert_eq!(evs, vec![MesEvent::Glyph(0x9D), MesEvent::EndOfMessage]);
    }

    #[test]
    fn interpreter_run_past_end_keeps_walking() {
        let prog = [0x61, 0x9D, 0x00, 0x61, 0x9E, 0x00];
        let buf = build_compact_with_program(&prog);
        let blob = parse(&buf).unwrap();
        let mut interp = Interpreter::new_compact(&blob, &buf, 0).unwrap();
        interp.run_past_end = true;
        let evs = interp.collect_events();
        assert_eq!(
            evs,
            vec![
                MesEvent::Glyph(0x9D),
                MesEvent::EndOfMessage,
                MesEvent::Glyph(0x9E),
                MesEvent::EndOfMessage,
            ]
        );
    }

    #[test]
    fn interpreter_classifies_unknown_op() {
        let prog = [0x99, 0x00];
        let buf = build_compact_with_program(&prog);
        let evs = extract_message(&buf, 0).unwrap();
        assert_eq!(
            evs,
            vec![MesEvent::Unknown { opcode: 0x99 }, MesEvent::EndOfMessage,]
        );
    }

    #[test]
    fn interpreter_op65_and_op4c() {
        let prog = [0x65, 0x42, 0x4C, 0x07, 0x00];
        let buf = build_compact_with_program(&prog);
        let evs = extract_message(&buf, 0).unwrap();
        assert_eq!(
            evs,
            vec![
                MesEvent::Op65 { arg: 0x42 },
                MesEvent::Op4c { arg: 0x07 },
                MesEvent::EndOfMessage,
            ]
        );
    }

    #[test]
    fn interpreter_op26_with_non_pagebreak_arg_passes_through() {
        // arg = 0x1234 (not 0xFFFE)
        let prog = [0x26, 0x34, 0x12, 0x00];
        let buf = build_compact_with_program(&prog);
        let evs = extract_message(&buf, 0).unwrap();
        assert_eq!(
            evs,
            vec![MesEvent::Op26 { arg: 0x1234 }, MesEvent::EndOfMessage,]
        );
    }

    #[test]
    fn render_summary_formats_each_event_type() {
        let evs = vec![
            MesEvent::Glyph(0x9D),
            MesEvent::Glyph(0xAA),
            MesEvent::Op65 { arg: 0x07 },
            MesEvent::Op4c { arg: 0x42 },
            MesEvent::Op26 { arg: 0x1234 },
            MesEvent::PageBreak,
            MesEvent::Unknown { opcode: 0x99 },
            MesEvent::EndOfMessage,
        ];
        let s = Interpreter::render_summary(&evs);
        assert!(s.contains("9D"));
        assert!(s.contains("AA"));
        assert!(s.contains("[op65:07]"));
        assert!(s.contains("[op4c:42]"));
        assert!(s.contains("[op26:1234]"));
        assert!(s.contains("[PAGE]"));
        assert!(s.contains("[?99]"));
        assert!(s.contains("[END]"));
    }

    #[test]
    fn event_stats_counts_each_type() {
        let evs = vec![
            MesEvent::Glyph(0x9D),
            MesEvent::Glyph(0xAA),
            MesEvent::PageBreak,
            MesEvent::Op65 { arg: 0x07 },
            MesEvent::Unknown { opcode: 0x99 },
            MesEvent::EndOfMessage,
        ];
        let s = EventStats::from_events(&evs);
        assert_eq!(s.glyphs, 2);
        assert_eq!(s.page_breaks, 1);
        assert_eq!(s.op65, 1);
        assert_eq!(s.unknowns, 1);
        assert_eq!(s.end_of_message, 1);
    }

    #[test]
    fn extract_all_messages_walks_table_entries() {
        // Build a blob with two messages at offsets 0 and 5.
        let prog = [0x61, 0x9D, 0x00, 0x00, 0x00, 0x61, 0x9E, 0x00];
        let mut buf = build_compact_with_program(&prog);
        // Override second offset table entry to point at 5.
        buf[crate::compact::OFFSET_TABLE_OFFSET + 3] = 5;
        buf[crate::compact::OFFSET_TABLE_OFFSET + 4] = 0;
        buf[crate::compact::OFFSET_TABLE_OFFSET + 5] = 0;
        let messages = extract_all_messages(&buf).unwrap();
        // First message: glyph A + end.
        assert_eq!(
            messages[0],
            vec![MesEvent::Glyph(0x9D), MesEvent::EndOfMessage]
        );
        // Second message: glyph B + end.
        assert_eq!(
            messages[1],
            vec![MesEvent::Glyph(0x9E), MesEvent::EndOfMessage]
        );
    }

    #[test]
    fn new_compact_rejects_records_blob() {
        // Build a records blob (>= 4 marker hits).
        let mut buf = vec![0xAAu8; 256];
        for i in (10..200).step_by(40) {
            buf[i] = crate::RECORD_MARKER[0];
            buf[i + 1] = crate::RECORD_MARKER[1];
        }
        let blob = parse(&buf).unwrap();
        let err = Interpreter::new_compact(&blob, &buf, 0).unwrap_err();
        assert!(err.to_string().contains("compact"));
    }

    #[test]
    fn new_compact_rejects_out_of_range_index() {
        let prog = [0x00];
        let buf = build_compact_with_program(&prog);
        let blob = parse(&buf).unwrap();
        // table has many entries, but the highest valid index is at the
        // u24 entry that we wrote 0 + the second past-end one.
        let large = blob.offset_table.as_ref().unwrap().len() + 100;
        let err = Interpreter::new_compact(&blob, &buf, large).unwrap_err();
        assert!(err.to_string().contains("out of range"));
    }
}

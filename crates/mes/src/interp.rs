//! MES bytecode interpreter.
//!
//! Reads tokens from [`crate::iter_tokens`] and surfaces higher-level
//! [`MesEvent`]s the engine can consume: glyphs, substitution requests,
//! spacing ops, page-control bytes, end-of-message sentinels.
//!
//! The opcode catalogue mirrors the SCUS interpreter functions
//! (`FUN_8003CA38` / `FUN_80036044` / `FUN_80036888` / `FUN_80036514`).
//! Substitution opcodes carry a [`SubstituteKind`] so engines can route
//! them to the right name table without having to re-decode the raw byte.

use crate::{Format, MesBlob, SubstituteOpcode, Token, TokenIter, iter_tokens, parse};
use anyhow::{Result, bail};
use serde::Serialize;

/// Semantic kind of a `0xC1..=0xC5` / `0xC7` substitution. Distinct from
/// [`SubstituteOpcode`] only to keep the event API stable if we later
/// need to merge `0xC2` and `0xC4` (both = item name) into one variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubstituteKind {
    /// `0xC1 XX`. `XX = 99` resolves to current party leader.
    CharacterName,
    /// `0xC2 XX` and `0xC4 XX` (different consumer sites, same table).
    ItemName,
    /// `0xC3 XX`.
    MagicName,
    /// `0xC5 XX`. Lookup is a 2D table at `DAT_80075EC4` keyed by
    /// `(XX>>6, XX&0x3F)`.
    SpellName,
    /// `0xC7 XX`.
    QuestName,
}

impl From<SubstituteOpcode> for SubstituteKind {
    fn from(op: SubstituteOpcode) -> Self {
        match op {
            SubstituteOpcode::CharacterName => SubstituteKind::CharacterName,
            SubstituteOpcode::ItemName | SubstituteOpcode::ItemNameAlt => SubstituteKind::ItemName,
            SubstituteOpcode::MagicName => SubstituteKind::MagicName,
            SubstituteOpcode::SpellName => SubstituteKind::SpellName,
            SubstituteOpcode::QuestName => SubstituteKind::QuestName,
        }
    }
}

/// Higher-level event surfaced by [`Interpreter::next_event`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum MesEvent {
    /// Print one single-byte glyph.
    Glyph(u8),
    /// Print one wide (2-byte) glyph. The pair `(opcode, arg)` matches
    /// the source bytes - the renderer typically uses the arg as a tile
    /// index within the table selected by opcode.
    WideGlyph(u8, u8),
    /// Substitute a name (character / item / magic / spell / quest) into
    /// the glyph stream. Engines route by `kind` to look up the
    /// corresponding name in the right table.
    Substitute { kind: SubstituteKind, arg: u8 },
    /// `0xCE XX` - apply horizontal spacing without emitting a glyph.
    Spacing(u8),
    /// `0xCF XX` - render `XX` as a single glyph alone (the `0xCF`
    /// prefix is just a "skip me" marker for the surrounding logic).
    SkipTwo(u8),
    /// Bytes `0x80..=0x9F`. Most likely page-break / wait-for-input
    /// markers per the dialog window pager `FUN_801D84D0` test
    /// `(byte & 0x7F) < 0x20`. Exact semantics per byte value are still
    /// unconfirmed, so the byte is surfaced verbatim.
    Control(u8),
    /// `0x00..=0x1E` - end of message. The interpreter halts here unless
    /// `run_past_end` is set; the renderer typically tears the dialog
    /// window down on this event.
    EndOfMessage(u8),
    /// Truncated 2-byte token at end of buffer. Carries the raw opcode
    /// so the caller can re-sync.
    Truncated(u8),
}

impl MesEvent {
    pub fn is_terminal(&self) -> bool {
        matches!(self, MesEvent::EndOfMessage(_))
    }
}

/// MES bytecode interpreter. Walks tokens and emits [`MesEvent`]s.
#[derive(Debug)]
pub struct Interpreter<'a> {
    inner: TokenIter<'a>,
    /// Set to true to keep walking past `EndOfMessage` (useful for
    /// streaming multi-message blobs without re-seeking). Default false.
    pub run_past_end: bool,
    /// Whether the interpreter has emitted a terminal event.
    halted: bool,
}

impl<'a> Interpreter<'a> {
    /// Build an interpreter that walks bytecode starting at the offset
    /// for message `index` in a [`Format::Compact`] blob.
    pub fn new_compact(blob: &'a MesBlob, buf: &'a [u8], message_index: usize) -> Result<Self> {
        if blob.format != Format::Compact {
            bail!("new_compact requires a compact blob, got {:?}", blob.format);
        }
        let table = blob.offset_table.as_ref().ok_or_else(|| {
            anyhow::anyhow!("compact blob has no offset table - was it parsed correctly?")
        })?;
        let entry = table
            .get(message_index)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("message index {message_index} out of range"))?;
        let bytecode_offset = blob.bytecode_offset.ok_or_else(|| {
            anyhow::anyhow!("compact blob has no bytecode offset - parse failure?")
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
            halted: false,
        }
    }

    /// Current PC - byte offset of the next token to read.
    pub fn pc(&self) -> usize {
        self.inner.pos()
    }

    /// Pull one [`MesEvent`].
    pub fn next_event(&mut self) -> Option<MesEvent> {
        if self.halted && !self.run_past_end {
            return None;
        }
        let (_, tok) = self.inner.next()?;
        let ev = event_from_token(tok);
        if ev.is_terminal() {
            self.halted = true;
        }
        Some(ev)
    }

    /// Drain all events into a `Vec`. Halts at `EndOfMessage` unless
    /// `run_past_end` is set.
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

    /// Build a hex-glyph string from the event stream. Useful for diffing
    /// captures: every glyph emits its u8 index as a hex byte; control
    /// events render as bracketed names. Until the font is decoded, this
    /// is the closest thing we have to "render this message."
    pub fn render_summary(events: &[MesEvent]) -> String {
        let mut out = String::new();
        for ev in events {
            match ev {
                MesEvent::Glyph(idx) => out.push_str(&format!("{idx:02X} ")),
                MesEvent::WideGlyph(op, arg) => out.push_str(&format!("{op:02X}{arg:02X} ")),
                MesEvent::Substitute { kind, arg } => {
                    let tag = match kind {
                        SubstituteKind::CharacterName => "char",
                        SubstituteKind::ItemName => "item",
                        SubstituteKind::MagicName => "magic",
                        SubstituteKind::SpellName => "spell",
                        SubstituteKind::QuestName => "quest",
                    };
                    out.push_str(&format!("[{tag}:{arg:02X}]"));
                }
                MesEvent::Spacing(n) => out.push_str(&format!("[sp:{n:02X}]")),
                MesEvent::SkipTwo(n) => out.push_str(&format!("[skip:{n:02X}]")),
                MesEvent::Control(b) => out.push_str(&format!("[ctl:{b:02X}]")),
                MesEvent::EndOfMessage(_) => out.push_str("[END]"),
                MesEvent::Truncated(op) => out.push_str(&format!("[trunc:{op:02X}]")),
            }
        }
        out.trim_end().to_string()
    }
}

fn event_from_token(token: Token) -> MesEvent {
    match token {
        Token::EndOfMessage(b) => MesEvent::EndOfMessage(b),
        Token::Glyph(g) => MesEvent::Glyph(g),
        Token::WideGlyph(op, arg) => MesEvent::WideGlyph(op, arg),
        Token::Substitute { kind, arg } => MesEvent::Substitute {
            kind: kind.into(),
            arg,
        },
        Token::Spacing(n) => MesEvent::Spacing(n),
        Token::SkipTwo(n) => MesEvent::SkipTwo(n),
        Token::Control(b) => MesEvent::Control(b),
        Token::Truncated(op) => MesEvent::Truncated(op),
    }
}

/// Convenience: parse, look up message at `index`, drain all events.
pub fn extract_message(buf: &[u8], index: usize) -> Result<Vec<MesEvent>> {
    let blob = parse(buf)?;
    let mut interp = Interpreter::new_compact(&blob, buf, index)?;
    Ok(interp.collect_events())
}

/// Yield every message in a [`Format::Compact`] blob.
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

/// A frame-paced dialog player. Wraps an [`Interpreter`] and surfaces
/// one "step" per call to [`DialogPlayer::tick`], emitting a
/// [`PlayerState`] the renderer can drive: print one more glyph (per the
/// typewriter speed), hold-on-page-break-until-input, finish-on-end.
///
/// The retail equivalent is the dialog window pager `FUN_801D84D0`.
/// This is a clean-room port of that *behaviour*: when the player sees
/// a `Control` byte (`0x80..=0x9F`) it pauses for engine input; when it
/// sees `EndOfMessage` it terminates.
#[derive(Debug)]
pub struct DialogPlayer<'a> {
    interp: Interpreter<'a>,
    /// Frames between glyph emits. 1 = one glyph per call to `tick`.
    pub glyphs_per_frame: u8,
    tick_count: u64,
    waiting_for_input: bool,
    done: bool,
}

/// One frame of dialog playback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerState {
    Idle,
    Glyph(u8),
    /// Wide-glyph tile: opcode + arg. Engines render this as a single
    /// glyph from whichever table the opcode picks.
    WideGlyph(u8, u8),
    PageBreak,
    WaitingForInput,
    /// Control / spacing / substitution / skip / truncated event - the
    /// engine routes the side-effect (substitute name into buffer,
    /// apply spacing, etc.). `Control` events from the `0x80..0x9F`
    /// range are surfaced here too if `pause_on_control` is set; those
    /// also trigger a `PageBreak` state on the same tick.
    Control(MesEvent),
    Done,
}

impl<'a> DialogPlayer<'a> {
    pub fn new(interp: Interpreter<'a>) -> Self {
        Self {
            interp,
            glyphs_per_frame: 1,
            tick_count: 0,
            waiting_for_input: false,
            done: false,
        }
    }

    /// Set the typewriter pacing. `0` is treated as `1`.
    pub fn set_glyphs_per_frame(&mut self, n: u8) {
        self.glyphs_per_frame = n.max(1);
    }

    /// Advance the player by one frame.
    pub fn tick(&mut self) -> PlayerState {
        if self.done {
            return PlayerState::Done;
        }
        if self.waiting_for_input {
            return PlayerState::WaitingForInput;
        }
        self.tick_count += 1;
        if !self.tick_count.is_multiple_of(self.glyphs_per_frame as u64) {
            return PlayerState::Idle;
        }
        match self.interp.next_event() {
            Some(MesEvent::Glyph(g)) => PlayerState::Glyph(g),
            Some(MesEvent::WideGlyph(op, arg)) => PlayerState::WideGlyph(op, arg),
            Some(MesEvent::Control(_)) => {
                self.waiting_for_input = true;
                PlayerState::PageBreak
            }
            Some(MesEvent::EndOfMessage(_)) => {
                self.done = true;
                PlayerState::Done
            }
            Some(ev) => PlayerState::Control(ev),
            None => {
                self.done = true;
                PlayerState::Done
            }
        }
    }

    /// Player has been paused on a page break; resume.
    pub fn advance_page(&mut self) {
        self.waiting_for_input = false;
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    pub fn is_waiting_for_input(&self) -> bool {
        self.waiting_for_input
    }
}

/// Per-message validation outcome.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct MessageValidation {
    pub message_index: usize,
    pub events: usize,
    pub stats: EventStats,
    pub looks_valid: bool,
}

/// Walk every message in a [`Format::Compact`] blob and report parse
/// health.
pub fn validate_compact(buf: &[u8]) -> Result<Vec<MessageValidation>> {
    let messages = extract_all_messages(buf)?;
    let mut out = Vec::with_capacity(messages.len());
    for (i, evs) in messages.iter().enumerate() {
        let stats = EventStats::from_events(evs);
        let total = evs.len();
        let truncated = stats.truncated;
        // Heuristic: <= 5% Truncated events == well-formed. A Truncated
        // event only occurs at end-of-buffer for an incomplete 2-byte
        // op, so a high count is a strong "this isn't real bytecode"
        // signal. (Old "Unknown" heuristic doesn't apply - every byte
        // now classifies into a typed Token.)
        let looks_valid = total == 0 || (truncated as f32 / total as f32) <= 0.05;
        out.push(MessageValidation {
            message_index: i,
            events: total,
            stats,
            looks_valid,
        });
    }
    Ok(out)
}

/// Counted summary of an event sequence.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct EventStats {
    pub glyphs: usize,
    pub wide_glyphs: usize,
    pub substitutes: usize,
    pub spacing: usize,
    pub skip_two: usize,
    pub controls: usize,
    pub end_of_message: usize,
    pub truncated: usize,
}

impl EventStats {
    pub fn from_events(events: &[MesEvent]) -> Self {
        let mut s = Self::default();
        for ev in events {
            match ev {
                MesEvent::Glyph(_) => s.glyphs += 1,
                MesEvent::WideGlyph(_, _) => s.wide_glyphs += 1,
                MesEvent::Substitute { .. } => s.substitutes += 1,
                MesEvent::Spacing(_) => s.spacing += 1,
                MesEvent::SkipTwo(_) => s.skip_two += 1,
                MesEvent::Control(_) => s.controls += 1,
                MesEvent::EndOfMessage(_) => s.end_of_message += 1,
                MesEvent::Truncated(_) => s.truncated += 1,
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::COMPACT_MAGIC;

    fn build_compact_with_program(prog: &[u8]) -> Vec<u8> {
        let mut buf = vec![0u8; crate::compact::OFFSET_TABLE_END + prog.len()];
        buf[0..4].copy_from_slice(&COMPACT_MAGIC.to_le_bytes());
        buf[crate::compact::OFFSET_TABLE_OFFSET] = 0;
        buf[crate::compact::OFFSET_TABLE_OFFSET + 1] = 0;
        buf[crate::compact::OFFSET_TABLE_OFFSET + 2] = 0;
        let past_end = (prog.len() + 1000) as u32;
        buf[crate::compact::OFFSET_TABLE_OFFSET + 3] = past_end as u8;
        buf[crate::compact::OFFSET_TABLE_OFFSET + 4] = (past_end >> 8) as u8;
        buf[crate::compact::OFFSET_TABLE_OFFSET + 5] = (past_end >> 16) as u8;
        buf[crate::compact::OFFSET_TABLE_END..].copy_from_slice(prog);
        buf
    }

    #[test]
    fn interpreter_emits_glyphs_and_terminator() {
        let prog = [0x21, 0x22, 0x00];
        let buf = build_compact_with_program(&prog);
        let evs = extract_message(&buf, 0).unwrap();
        assert_eq!(
            evs,
            vec![
                MesEvent::Glyph(0x21),
                MesEvent::Glyph(0x22),
                MesEvent::EndOfMessage(0x00),
            ]
        );
    }

    #[test]
    fn interpreter_emits_substitutes_with_kind() {
        let prog = [0xC1, 0x05, 0xC3, 0x10, 0xC4, 0x20, 0x00];
        let buf = build_compact_with_program(&prog);
        let evs = extract_message(&buf, 0).unwrap();
        assert_eq!(
            evs,
            vec![
                MesEvent::Substitute {
                    kind: SubstituteKind::CharacterName,
                    arg: 0x05,
                },
                MesEvent::Substitute {
                    kind: SubstituteKind::MagicName,
                    arg: 0x10,
                },
                MesEvent::Substitute {
                    kind: SubstituteKind::ItemName, // C4 collapses to ItemName
                    arg: 0x20,
                },
                MesEvent::EndOfMessage(0x00),
            ]
        );
    }

    #[test]
    fn interpreter_emits_spacing_and_skip() {
        let prog = [0xCE, 0x42, 0xCF, 0x99, 0x00];
        let buf = build_compact_with_program(&prog);
        let evs = extract_message(&buf, 0).unwrap();
        assert_eq!(
            evs,
            vec![
                MesEvent::Spacing(0x42),
                MesEvent::SkipTwo(0x99),
                MesEvent::EndOfMessage(0x00),
            ]
        );
    }

    #[test]
    fn interpreter_emits_wide_glyph() {
        let prog = [0xC0, 0x10, 0xCA, 0x20, 0x00];
        let buf = build_compact_with_program(&prog);
        let evs = extract_message(&buf, 0).unwrap();
        assert_eq!(
            evs,
            vec![
                MesEvent::WideGlyph(0xC0, 0x10),
                MesEvent::WideGlyph(0xCA, 0x20),
                MesEvent::EndOfMessage(0x00),
            ]
        );
    }

    #[test]
    fn interpreter_emits_control_byte() {
        // 0x80..0x9F are "Control" events (likely page-break / wait).
        let prog = [0x21, 0x88, 0x22, 0x00];
        let buf = build_compact_with_program(&prog);
        let evs = extract_message(&buf, 0).unwrap();
        assert_eq!(
            evs,
            vec![
                MesEvent::Glyph(0x21),
                MesEvent::Control(0x88),
                MesEvent::Glyph(0x22),
                MesEvent::EndOfMessage(0x00),
            ]
        );
    }

    #[test]
    fn interpreter_halts_at_end_of_message() {
        let prog = [0x21, 0x00, 0x22, 0x00];
        let buf = build_compact_with_program(&prog);
        let evs = extract_message(&buf, 0).unwrap();
        assert_eq!(
            evs,
            vec![MesEvent::Glyph(0x21), MesEvent::EndOfMessage(0x00),]
        );
    }

    #[test]
    fn interpreter_run_past_end_keeps_walking() {
        let prog = [0x21, 0x00, 0x22, 0x00];
        let buf = build_compact_with_program(&prog);
        let blob = parse(&buf).unwrap();
        let mut interp = Interpreter::new_compact(&blob, &buf, 0).unwrap();
        interp.run_past_end = true;
        let evs = interp.collect_events();
        assert_eq!(
            evs,
            vec![
                MesEvent::Glyph(0x21),
                MesEvent::EndOfMessage(0x00),
                MesEvent::Glyph(0x22),
                MesEvent::EndOfMessage(0x00),
            ]
        );
    }

    #[test]
    fn render_summary_formats_each_event_type() {
        let evs = vec![
            MesEvent::Glyph(0x9D),
            MesEvent::WideGlyph(0xC0, 0xAA),
            MesEvent::Substitute {
                kind: SubstituteKind::CharacterName,
                arg: 0x05,
            },
            MesEvent::Spacing(0x40),
            MesEvent::SkipTwo(0x99),
            MesEvent::Control(0x88),
            MesEvent::Truncated(0xC1),
            MesEvent::EndOfMessage(0x00),
        ];
        let s = Interpreter::render_summary(&evs);
        assert!(s.contains("9D"));
        assert!(s.contains("C0AA"));
        assert!(s.contains("[char:05]"));
        assert!(s.contains("[sp:40]"));
        assert!(s.contains("[skip:99]"));
        assert!(s.contains("[ctl:88]"));
        assert!(s.contains("[trunc:C1]"));
        assert!(s.contains("[END]"));
    }

    #[test]
    fn event_stats_counts_each_type() {
        let evs = vec![
            MesEvent::Glyph(0x21),
            MesEvent::Glyph(0x22),
            MesEvent::WideGlyph(0xC0, 0x10),
            MesEvent::Substitute {
                kind: SubstituteKind::CharacterName,
                arg: 0x00,
            },
            MesEvent::Spacing(0x10),
            MesEvent::SkipTwo(0x99),
            MesEvent::Control(0x88),
            MesEvent::Truncated(0xC1),
            MesEvent::EndOfMessage(0x00),
        ];
        let s = EventStats::from_events(&evs);
        assert_eq!(s.glyphs, 2);
        assert_eq!(s.wide_glyphs, 1);
        assert_eq!(s.substitutes, 1);
        assert_eq!(s.spacing, 1);
        assert_eq!(s.skip_two, 1);
        assert_eq!(s.controls, 1);
        assert_eq!(s.truncated, 1);
        assert_eq!(s.end_of_message, 1);
    }

    #[test]
    fn extract_all_messages_walks_table_entries() {
        let prog = [0x21, 0x00, 0x00, 0x00, 0x00, 0x22, 0x00];
        let mut buf = build_compact_with_program(&prog);
        // Override second offset table entry to point at index 5.
        buf[crate::compact::OFFSET_TABLE_OFFSET + 3] = 5;
        buf[crate::compact::OFFSET_TABLE_OFFSET + 4] = 0;
        buf[crate::compact::OFFSET_TABLE_OFFSET + 5] = 0;
        let messages = extract_all_messages(&buf).unwrap();
        assert_eq!(
            messages[0],
            vec![MesEvent::Glyph(0x21), MesEvent::EndOfMessage(0x00)]
        );
        assert_eq!(
            messages[1],
            vec![MesEvent::Glyph(0x22), MesEvent::EndOfMessage(0x00)]
        );
    }

    // --- DialogPlayer / validate_compact ----------------------------------

    fn player_at_start(buf: &[u8]) -> DialogPlayer<'_> {
        let interp = Interpreter::new_at(buf, crate::compact::OFFSET_TABLE_END);
        DialogPlayer::new(interp)
    }

    #[test]
    fn dialog_player_emits_one_glyph_per_tick_at_unity_pace() {
        let prog = [0x21, 0x22, 0x00];
        let buf = build_compact_with_program(&prog);
        let mut player = player_at_start(&buf);
        assert_eq!(player.tick(), PlayerState::Glyph(0x21));
        assert_eq!(player.tick(), PlayerState::Glyph(0x22));
        assert_eq!(player.tick(), PlayerState::Done);
        assert!(player.is_done());
    }

    #[test]
    fn dialog_player_paces_glyphs_at_3_frames_per_glyph() {
        let prog = [0x21, 0x22, 0x00];
        let buf = build_compact_with_program(&prog);
        let mut player = player_at_start(&buf);
        player.set_glyphs_per_frame(3);
        assert_eq!(player.tick(), PlayerState::Idle);
        assert_eq!(player.tick(), PlayerState::Idle);
        assert_eq!(player.tick(), PlayerState::Glyph(0x21));
        assert_eq!(player.tick(), PlayerState::Idle);
        assert_eq!(player.tick(), PlayerState::Idle);
        assert_eq!(player.tick(), PlayerState::Glyph(0x22));
    }

    #[test]
    fn dialog_player_pauses_on_control_byte() {
        // Control byte 0x88 (in 0x80..0x9F) triggers PageBreak.
        let prog = [0x21, 0x88, 0x22, 0x00];
        let buf = build_compact_with_program(&prog);
        let mut player = player_at_start(&buf);
        assert_eq!(player.tick(), PlayerState::Glyph(0x21));
        assert_eq!(player.tick(), PlayerState::PageBreak);
        assert_eq!(player.tick(), PlayerState::WaitingForInput);
        assert_eq!(player.tick(), PlayerState::WaitingForInput);
        assert!(player.is_waiting_for_input());
        player.advance_page();
        assert!(!player.is_waiting_for_input());
        assert_eq!(player.tick(), PlayerState::Glyph(0x22));
        assert_eq!(player.tick(), PlayerState::Done);
    }

    #[test]
    fn dialog_player_emits_wide_glyph() {
        let prog = [0xC0, 0xAA, 0x00];
        let buf = build_compact_with_program(&prog);
        let mut player = player_at_start(&buf);
        assert_eq!(player.tick(), PlayerState::WideGlyph(0xC0, 0xAA));
        assert_eq!(player.tick(), PlayerState::Done);
    }

    #[test]
    fn dialog_player_routes_substitute_through_control() {
        let prog = [0xC1, 0x05, 0x00];
        let buf = build_compact_with_program(&prog);
        let mut player = player_at_start(&buf);
        match player.tick() {
            PlayerState::Control(MesEvent::Substitute {
                kind: SubstituteKind::CharacterName,
                arg: 0x05,
            }) => {}
            other => panic!("expected Substitute control, got {other:?}"),
        }
        assert_eq!(player.tick(), PlayerState::Done);
    }

    #[test]
    fn dialog_player_treats_glyphs_per_frame_zero_as_one() {
        let prog = [0x21, 0x00];
        let buf = build_compact_with_program(&prog);
        let mut player = player_at_start(&buf);
        player.set_glyphs_per_frame(0);
        assert_eq!(player.glyphs_per_frame, 1);
        assert_eq!(player.tick(), PlayerState::Glyph(0x21));
    }

    #[test]
    fn dialog_player_after_done_keeps_returning_done() {
        let prog = [0x00];
        let buf = build_compact_with_program(&prog);
        let mut player = player_at_start(&buf);
        assert_eq!(player.tick(), PlayerState::Done);
        for _ in 0..5 {
            assert_eq!(player.tick(), PlayerState::Done);
        }
    }

    #[test]
    fn validate_compact_flags_clean_messages_as_valid() {
        // Build a blob with one clean message of 2 glyphs + end.
        let prog = [0x21, 0x22, 0x00];
        let buf = build_compact_with_program(&prog);
        let report = validate_compact(&buf).unwrap();
        // First message: clean.
        assert!(report[0].looks_valid);
        assert_eq!(report[0].stats.glyphs, 2);
        assert_eq!(report[0].stats.truncated, 0);
    }
}

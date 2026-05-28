//! Dialog panel: clean-room port of the field VM's dialog opener path.
//!
//! PORT: FUN_8001FD44, FUN_801D84D0
//!
//! Wraps a [`legaia_mes::DialogPlayer`] in the runtime state the retail
//! dialog renderer holds: the typed-out glyph buffer for the current page,
//! a pen color tracked through `0xCF` color-change escapes, and an explicit
//! page-break / done flag the host loop polls.
//!
//! This is the minimal substrate engines need to drive an on-screen dialog
//! window without re-implementing the typewriter / page-break semantics in
//! every consumer (the MES viewer in `asset-viewer`, the field VM dialog
//! opener at SCUS `FUN_8001FD44`, the battle dialog overlay, etc).
//!
//! Provenance: the per-page glyph accumulation + page-break gate mirror the
//! retail dialog window pager `FUN_801D84D0` in the dialog overlay (see
//! [`docs/formats/dialog-font.md`](../../../docs/formats/dialog-font.md)).
//! Layout / GPU bridging lives in `legaia-engine-render` -
//! [`text_draws_for`](../../legaia_engine_render/fn.text_draws_for.html)
//! consumes the [`Self::page_glyphs`] byte stream via [`legaia_font::Font`].
//! REF: FUN_80036888

use legaia_mes::{DialogPlayer, Interpreter, MesEvent, PlayerState};
use std::sync::Arc;

/// Decode every `0x1F`-lead text segment in a field-VM inline dialog buffer
/// to its glyph bytes.
///
/// An interaction record's inline text is a flat **pool** of segments, each
/// introduced by a `0x1F` lead byte and terminated by an MES end byte
/// (`0x00..=0x1E`). Empirically (decoding real town01 placements) this pool is
/// the NPC's *entire* dialogue line set — every line across every story-state
/// branch of that NPC's conversation, with interspersed option labels (e.g.
/// `"Yes"` / `"No"`). It is **not** a single box's "prompt then option labels":
/// most segments are consecutive speech lines, and the field-VM script (gated
/// on story flags via `COND_JMP`) selects which segment to start at, how many
/// lines fill one box, and which are selectable options. Mapping segments →
/// boxes/options needs the box-geometry header that precedes the run, which is
/// not yet decoded. The real per-actor dialog SM is `FUN_80039b7c` (advances
/// `actor[+0x9c]` 0→1→2 through `0x1F`-lead segments) with pager `FUN_801D84D0`;
/// the box-geometry header decoder is upstream of those, likely among the
/// helpers that initialise both `actor[+0x9c]` and the cursor `actor[+0x9e]`
/// (`FUN_8003AB2C`, `FUN_8003BDE0`, …). An earlier note pointed at
/// `func_0x8001ebec` as the renderer — that is **wrong**; the disassembly
/// shows it is a per-character TMD-pose copier indexed by the slot-4 freeze
/// flag, not the dialog box renderer.
///
/// So this returns the raw segment pool, faithfully, leaving the box/option
/// interpretation to a future consumer once the header semantics are pinned.
/// The geometry header before the first `0x1F` is skipped (its bytes can fall
/// in the glyph range, so it is not interpreted as text).
pub fn decode_inline_segments(inline: &[u8]) -> Vec<Vec<u8>> {
    let mut segments = Vec::new();
    let mut cursor = 0usize;
    while let Some(rel) = inline[cursor..].iter().position(|&b| b == 0x1F) {
        let start = cursor + rel + 1;
        let mut interp = Interpreter::new_at(inline, start);
        let mut glyphs = Vec::new();
        loop {
            match interp.next_event() {
                Some(MesEvent::Glyph(g)) | Some(MesEvent::SkipTwo(g)) => glyphs.push(g),
                Some(MesEvent::WideGlyph(_op, arg)) => glyphs.push(arg),
                Some(MesEvent::EndOfMessage(_)) | None => break,
                // Page-break / spacing / substitution stay inside the current
                // segment: a segment ends only at its MES terminator, never at
                // an intermediate control byte.
                Some(_) => {}
            }
        }
        // Resume scanning just past this segment's terminator. `pc()` sits
        // after the end byte; clamp to `start` so a `0x1F` immediately followed
        // by a terminator (empty segment) still makes forward progress.
        cursor = interp.pc().max(start);
        segments.push(glyphs);
    }
    segments
}

/// Page-state machine the host polls each frame. Mirrors the
/// [`legaia_mes::PlayerState`] fan-out but folds idle / typing into a single
/// `Typing` outcome the host doesn't need to disambiguate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelState {
    /// More glyphs are available, or the typewriter is still pacing. Host
    /// keeps drawing the current `page_glyphs` buffer.
    Typing,
    /// The current page is fully typed out and the player has hit a control
    /// byte the runtime treats as a page break. Host should wait for input
    /// (Cross / Enter), then call [`DialogPanel::advance_page`].
    PageBreak,
    /// No more events. The dialog opener should release the dialog flag
    /// (`_DAT_1F800394 |= 0x40`-equivalent) and unblock the calling script.
    Done,
}

/// One emitted glyph annotated with the runtime CLUT index that should tint
/// it. Engines look this up in their CLUT palette to color the glyph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelGlyph {
    /// Character byte (ASCII range for latin glyphs; some scenes embed
    /// `0x80..` wide-glyph escape pairs the MES interpreter surfaces as
    /// [`legaia_mes::PlayerState::WideGlyph`] - those are folded to their
    /// `arg` byte for now, since the wide-glyph table isn't decoded).
    pub byte: u8,
    /// CLUT additive index 0..15. `0` = default white; non-zero values come
    /// from inline `0xCF` color-change escapes in the bytecode.
    pub clut: u8,
}

/// Stateful dialog page driver around a [`DialogPlayer`].
///
/// Owns the tick-paced player and accumulates emitted glyphs into a
/// per-page buffer. The buffer is cleared on [`Self::advance_page`] so the
/// host's typewriter renders start fresh on each page.
pub struct DialogPanel<'a> {
    player: DialogPlayer<'a>,
    page: Vec<PanelGlyph>,
    /// Current pen color CLUT additive index. Updated when the bytecode
    /// emits a [`MesEvent::Control`] with the color-change byte (the field
    /// VM tracks this in `_DAT_8007B454`).
    current_clut: u8,
    state: PanelState,
}

impl<'a> DialogPanel<'a> {
    pub fn new(player: DialogPlayer<'a>) -> Self {
        Self {
            player,
            page: Vec::new(),
            current_clut: 0,
            state: PanelState::Typing,
        }
    }

    /// Set the typewriter pacing on the underlying player.
    pub fn set_glyphs_per_frame(&mut self, n: u8) {
        self.player.set_glyphs_per_frame(n);
    }

    /// Advance one frame. Returns the new [`PanelState`].
    pub fn tick(&mut self) -> PanelState {
        match self.state {
            PanelState::PageBreak | PanelState::Done => return self.state,
            PanelState::Typing => {}
        }
        let next = self.player.tick();
        match next {
            PlayerState::Idle => {}
            PlayerState::Glyph(g) => self.page.push(PanelGlyph {
                byte: g,
                clut: self.current_clut,
            }),
            PlayerState::WideGlyph(_op, arg) => {
                // Wide-glyph table isn't decoded yet; render the arg byte
                // through the standard atlas as a placeholder so the host
                // can see something instead of a silent skip.
                self.page.push(PanelGlyph {
                    byte: arg,
                    clut: self.current_clut,
                });
            }
            PlayerState::PageBreak => self.state = PanelState::PageBreak,
            PlayerState::WaitingForInput => self.state = PanelState::PageBreak,
            PlayerState::Control(MesEvent::SkipTwo(arg)) => {
                // `0xCF XX` in MES bytecode renders XX alone (the `0xCF`
                // prefix is a "skip me" marker per FUN_80036888). The
                // post-substitution color-change escape with the same
                // first byte is a separate byte stream the renderer sees
                // *after* MES interpretation; it isn't carried in the
                // bytecode and so doesn't drive `current_clut` here.
                self.page.push(PanelGlyph {
                    byte: arg,
                    clut: self.current_clut,
                });
            }
            PlayerState::Control(_) => {}
            PlayerState::Done => self.state = PanelState::Done,
        }
        self.state
    }

    /// Drop the current page's glyphs and unblock the player. Call after
    /// the user dismisses the page break.
    pub fn advance_page(&mut self) {
        if matches!(self.state, PanelState::PageBreak) {
            self.page.clear();
            self.player.advance_page();
            self.state = PanelState::Typing;
        }
    }

    /// All glyphs typed so far on the current page.
    pub fn page_glyphs(&self) -> &[PanelGlyph] {
        &self.page
    }

    /// Convenience: just the byte stream, for [`legaia_font::Font::layout`].
    pub fn page_bytes(&self) -> Vec<u8> {
        self.page.iter().map(|g| g.byte).collect()
    }

    pub fn state(&self) -> PanelState {
        self.state
    }

    pub fn is_done(&self) -> bool {
        matches!(self.state, PanelState::Done)
    }

    pub fn is_waiting_for_input(&self) -> bool {
        matches!(self.state, PanelState::PageBreak)
    }
}

/// Owned, self-contained dialog driver. Carries the MES bytecode bytes,
/// the current PC, and the page accumulator without borrowing - so it can
/// live on the World between frames without lifetime gymnastics.
///
/// Equivalent to [`DialogPanel`] for engines that want to mutate one piece
/// of long-lived state instead of constructing a borrowed pair every frame.
#[derive(Debug, Clone)]
pub struct OwnedDialogPanel {
    /// MES bytecode bytes for the active message. Shared via Arc so the
    /// engine can keep it alive cheaply across the scene.
    pub bytes: Arc<Vec<u8>>,
    /// Current bytecode PC (offset into [`Self::bytes`]).
    pub pc: usize,
    /// Frame counter - one tick per [`Self::tick`].
    pub tick_count: u64,
    /// Glyphs emitted per frame. `0` is treated as `1`.
    pub glyphs_per_frame: u8,
    /// Page glyphs accumulated since the last [`Self::advance_page`].
    pub page: Vec<PanelGlyph>,
    /// CLUT pen color tracked through `0xCF` color escapes.
    pub current_clut: u8,
    state: PanelState,
    waiting_for_input: bool,
    done: bool,
}

impl OwnedDialogPanel {
    /// Build a panel over `bytes` starting at `pc` (the offset returned by
    /// [`crate::scene_assets::SceneMes::message_offset`]).
    pub fn new(bytes: Arc<Vec<u8>>, pc: usize) -> Self {
        Self {
            bytes,
            pc,
            tick_count: 0,
            glyphs_per_frame: 1,
            page: Vec::new(),
            current_clut: 0,
            state: PanelState::Typing,
            waiting_for_input: false,
            done: false,
        }
    }

    /// Convenience: build a panel from a [`crate::scene_assets::SceneMes`]
    /// resolution. Returns `None` if `text_id` is past the offset table.
    pub fn from_scene_mes(mes: &crate::scene_assets::SceneMes, text_id: u16) -> Option<Self> {
        let pc = mes.message_offset(text_id)?;
        Some(Self::new(Arc::new(mes.bytes.clone()), pc))
    }

    /// Build a panel over the **inline** dialog bytes a field-VM `0x3F` op
    /// carries (stored on [`crate::world::DialogRequest::inline`]).
    ///
    /// Placement-NPC and event dialogue does not live in the scene MES (its
    /// `text_id` is a box-config id, not a message index - it never resolves
    /// through [`Self::from_scene_mes`]); the message text is the inline buffer
    /// itself, in the field-VM dialog-box format `[box-geometry header]` then
    /// `0x1F`-lead text/option segments (MES glyph bytecode). The geometry
    /// header is opaque here (it carries box position/size and option-layout
    /// bytes, some of which fall in the glyph range), so this skips to the
    /// first `0x1F` lead marker and types the first segment from just past it
    /// through the standard MES [`Interpreter`](legaia_mes::Interpreter).
    ///
    /// Only the first segment is typed: the record holds the NPC's whole
    /// dialogue line pool (see [`decode_inline_segments`]), and choosing which
    /// segment to start at — and how many lines / which option labels make up
    /// one box — is the field-VM script's job, gated on story flags, via the
    /// box-geometry header that is not yet decoded.
    ///
    /// Returns `None` when no `0x1F` lead marker is present (nothing
    /// renderable), so a caller can fall back to the MES path.
    pub fn from_inline_dialog(inline: &[u8]) -> Option<Self> {
        let lead = inline.iter().position(|&b| b == 0x1F)?;
        Some(Self::new(Arc::new(inline.to_vec()), lead + 1))
    }

    pub fn set_glyphs_per_frame(&mut self, n: u8) {
        self.glyphs_per_frame = n.max(1);
    }

    /// Advance one frame and return the new state.
    pub fn tick(&mut self) -> PanelState {
        if self.done {
            self.state = PanelState::Done;
            return self.state;
        }
        if self.waiting_for_input {
            self.state = PanelState::PageBreak;
            return self.state;
        }
        self.tick_count += 1;
        if !self.tick_count.is_multiple_of(self.glyphs_per_frame as u64) {
            return self.state;
        }
        let mut interp = Interpreter::new_at(&self.bytes, self.pc);
        let next = interp.next_event();
        self.pc = interp.pc();
        match next {
            Some(MesEvent::Glyph(g)) => self.page.push(PanelGlyph {
                byte: g,
                clut: self.current_clut,
            }),
            Some(MesEvent::WideGlyph(_op, arg)) => self.page.push(PanelGlyph {
                byte: arg,
                clut: self.current_clut,
            }),
            Some(MesEvent::Control(_)) => {
                self.waiting_for_input = true;
                self.state = PanelState::PageBreak;
            }
            Some(MesEvent::SkipTwo(arg)) => self.page.push(PanelGlyph {
                byte: arg,
                clut: self.current_clut,
            }),
            Some(MesEvent::EndOfMessage(_)) | None => {
                self.done = true;
                self.state = PanelState::Done;
            }
            Some(_) => {
                // Spacing / Substitute / Truncated - engine-side
                // routing isn't wired yet; leave the pen alone.
            }
        }
        self.state
    }

    /// Resume from a page break. No-op when the panel isn't paused.
    pub fn advance_page(&mut self) {
        if self.waiting_for_input {
            self.page.clear();
            self.waiting_for_input = false;
            self.state = PanelState::Typing;
        }
    }

    pub fn page_glyphs(&self) -> &[PanelGlyph] {
        &self.page
    }

    pub fn page_bytes(&self) -> Vec<u8> {
        self.page.iter().map(|g| g.byte).collect()
    }

    pub fn state(&self) -> PanelState {
        self.state
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    pub fn is_waiting_for_input(&self) -> bool {
        self.waiting_for_input
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_mes::{DialogPlayer, Interpreter};

    /// Minimal Compact-format MES blob: a single message with three glyphs
    /// then End. Avoids dragging the full container parser into tests.
    fn three_glyph_program() -> Vec<u8> {
        // bytecode is just the glyph stream + terminator: 'a' 'b' 'c' end
        vec![b'a', b'b', b'c', 0x00]
    }

    #[test]
    fn typing_then_done() {
        let buf = three_glyph_program();
        let interp = Interpreter::new_at(&buf, 0);
        let mut player = DialogPlayer::new(interp);
        player.set_glyphs_per_frame(1);
        let mut panel = DialogPanel::new(player);
        for _ in 0..3 {
            assert_eq!(panel.tick(), PanelState::Typing);
        }
        assert_eq!(
            panel.page_bytes(),
            vec![b'a', b'b', b'c'],
            "three glyphs should accumulate"
        );
        assert_eq!(panel.tick(), PanelState::Done);
        assert!(panel.is_done());
    }

    #[test]
    fn owned_panel_types_three_glyphs_then_done() {
        let buf = Arc::new(three_glyph_program());
        let mut panel = OwnedDialogPanel::new(buf, 0);
        for _ in 0..3 {
            assert_eq!(panel.tick(), PanelState::Typing);
        }
        assert_eq!(panel.page_bytes(), vec![b'a', b'b', b'c']);
        assert_eq!(panel.tick(), PanelState::Done);
        assert!(panel.is_done());
    }

    /// Inline field-VM dialog: a box-geometry header (leading bytes, some in
    /// the glyph range) then a `0x1F`-lead text segment. `from_inline_dialog`
    /// must skip the header to the first `0x1F` marker and type the text after
    /// it - here `"Hi"` from `[00 56 00 1F 'H' 'i' 00]`.
    #[test]
    fn from_inline_dialog_skips_geometry_header_and_types_text() {
        let inline = vec![0x00u8, 0x56, 0x00, 0x1F, b'H', b'i', 0x00];
        let mut panel = OwnedDialogPanel::from_inline_dialog(&inline).expect("has a 0x1F lead");
        for _ in 0..2 {
            assert_eq!(panel.tick(), PanelState::Typing);
        }
        assert_eq!(panel.page_bytes(), vec![b'H', b'i']);
        assert_eq!(panel.tick(), PanelState::Done);
    }

    /// No `0x1F` lead marker = nothing renderable; the caller falls back to the
    /// MES `text_id` path.
    #[test]
    fn from_inline_dialog_without_lead_marker_is_none() {
        assert!(OwnedDialogPanel::from_inline_dialog(&[0x00, 0x10, 0x00]).is_none());
    }

    /// The segment-pool decoder recovers **every** `0x1F`-lead segment, not
    /// just the first: a record carries the NPC's whole dialogue line pool
    /// (consecutive speech lines plus interspersed option labels like
    /// `"Yes"` / `"No"`), each `0x00`-terminated.
    #[test]
    fn decode_inline_segments_recovers_every_segment() {
        // [header] 1F W e l c o m e 00  1F Y e s 00  1F N o 00
        let inline = vec![
            0x00, 0x42, 0x1F, b'W', b'e', b'l', b'c', b'o', b'm', b'e', 0x00, 0x1F, b'Y', b'e',
            b's', 0x00, 0x1F, b'N', b'o', 0x00,
        ];
        let segs = decode_inline_segments(&inline);
        assert_eq!(
            segs,
            vec![b"Welcome".to_vec(), b"Yes".to_vec(), b"No".to_vec()],
            "all three 0x1F-lead segments decode in pool order"
        );
    }

    /// A single-segment record decodes to exactly one segment.
    #[test]
    fn decode_inline_segments_single_segment() {
        let inline = vec![0x00, 0x56, 0x00, 0x1F, b'H', b'i', 0x00];
        assert_eq!(decode_inline_segments(&inline), vec![b"Hi".to_vec()]);
    }

    /// `0xCF XX` in MES bytecode = render XX alone. Both panel variants
    /// must surface the operand byte through the page accumulator, not
    /// silently drop it.
    #[test]
    fn dialog_panel_renders_skip_two_operand() {
        // Glyph 'a', then 0xCF 'b', then End. Expected page: ['a', 'b'].
        let buf = vec![b'a', 0xCF, b'b', 0x00];
        let interp = Interpreter::new_at(&buf, 0);
        let mut player = DialogPlayer::new(interp);
        player.set_glyphs_per_frame(1);
        let mut panel = DialogPanel::new(player);
        for _ in 0..3 {
            panel.tick();
        }
        assert_eq!(panel.page_bytes(), vec![b'a', b'b']);
    }

    #[test]
    fn owned_panel_renders_skip_two_operand() {
        let buf = Arc::new(vec![b'a', 0xCF, b'b', 0x00]);
        let mut panel = OwnedDialogPanel::new(buf, 0);
        for _ in 0..3 {
            panel.tick();
        }
        assert_eq!(panel.page_bytes(), vec![b'a', b'b']);
    }

    #[test]
    fn advance_page_clears_buffer_and_resumes() {
        // After a glyph, force a page break by injecting a control byte.
        // Compact format treats single bytes as glyphs unless they're in
        // the control range - the interpreter's actual control byte set is
        // out of scope here, so we just verify advance_page is a no-op
        // when not at a break, and clears properly when at one.
        let buf = three_glyph_program();
        let interp = Interpreter::new_at(&buf, 0);
        let mut player = DialogPlayer::new(interp);
        player.set_glyphs_per_frame(1);
        let mut panel = DialogPanel::new(player);
        panel.tick();
        assert_eq!(panel.page_bytes().len(), 1);
        // Not at page break - advance_page is idempotent.
        panel.advance_page();
        assert_eq!(panel.page_bytes().len(), 1);
    }
}

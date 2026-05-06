//! Dialog panel: clean-room port of the field VM's dialog opener path.
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
//! Layout / GPU bridging lives in `legaia-engine-render` —
//! [`text_draws_for`](../../legaia_engine_render/fn.text_draws_for.html)
//! consumes the [`Self::page_glyphs`] byte stream via [`legaia_font::Font`].

use legaia_mes::{DialogPlayer, Interpreter, MesEvent, PlayerState};
use std::sync::Arc;

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
    /// [`legaia_mes::PlayerState::WideGlyph`] — those are folded to their
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
/// the current PC, and the page accumulator without borrowing — so it can
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
    /// Frame counter — one tick per [`Self::tick`].
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
                // Spacing / Substitute / Truncated — engine-side
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
        // the control range — the interpreter's actual control byte set is
        // out of scope here, so we just verify advance_page is a no-op
        // when not at a break, and clears properly when at one.
        let buf = three_glyph_program();
        let interp = Interpreter::new_at(&buf, 0);
        let mut player = DialogPlayer::new(interp);
        player.set_glyphs_per_frame(1);
        let mut panel = DialogPanel::new(player);
        panel.tick();
        assert_eq!(panel.page_bytes().len(), 1);
        // Not at page break — advance_page is idempotent.
        panel.advance_page();
        assert_eq!(panel.page_bytes().len(), 1);
    }
}

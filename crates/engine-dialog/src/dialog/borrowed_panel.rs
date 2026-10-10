//! `DialogPanel`: the borrowing panel over one MES interpreter, with the panel state and glyph types both panels share.

use super::*;

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

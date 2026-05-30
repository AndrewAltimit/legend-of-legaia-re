//! Faithful runner for an actor's inline interaction script.
//!
//! The simplified [`crate::dialog::OwnedDialogPanel`] types one `0x1F` text
//! segment and resolves a picker locally, but it never *executes* the field-VM
//! bytecode that surrounds the segments — the prologue's story-flag tests, the
//! `SET`/`CLEAR` flag ops, and the scene-change a branch handler runs after a
//! choice. This runner closes that gap by driving the inline script through the
//! real ported field VM ([`legaia_engine_vm::field::step`]) and only pausing to
//! show a box when the VM lands on a text segment.
//!
//! It mirrors the retail dialog state machine `FUN_80039B7C`, which runs the
//! field-VM dispatcher `FUN_801DE840` on the inline stream (`actor[+0x90]` base,
//! `actor[+0x9E]` PC) and transitions into the pager only when the dispatcher
//! leaves the PC on a byte where `& 0x7F < 0x20` (a `0x1F` lead or a `0x00..1E`
//! terminator). Between boxes the field VM's side effects — flag writes
//! (`system_flag_set`/`_clear`), the choice-selected branch jump
//! (`FUN_80038050`, applied by the host on confirm), scene changes — run
//! through the World host exactly as the scene script's do.
//!
//! The stepping itself lives on [`crate::world::World`] (it needs the
//! `FieldHostImpl` borrow); this module holds the resumable state.

use std::sync::Arc;

use legaia_engine_vm::field::FieldCtx;

use crate::dialog::OwnedDialogPanel;

/// Maximum field-VM steps to run between text boxes in one tick — bounds a
/// pathological inline script that never reaches a text segment or end.
pub const INLINE_DIALOGUE_STEP_BUDGET: u32 = 256;

/// Resumable state for one running inline interaction script.
#[derive(Debug)]
pub struct InlineDialogue {
    /// The actor's inline interaction-script bytes (field-VM bytecode with
    /// `0x1F`-lead text segments + pickers). Shared cheaply across the panel.
    pub bytecode: Arc<Vec<u8>>,
    /// Per-script field-VM context (flag word, move state, ...).
    pub ctx: FieldCtx,
    /// Current bytecode PC the VM resumes from when no box is open.
    pub pc: usize,
    /// The on-screen box, while one is being shown.
    pub panel: Option<OwnedDialogPanel>,
    /// `true` once the script reached an end terminator or an op the runner
    /// can't advance past.
    pub done: bool,
    /// Records the most recent option index the player picked (for hosts /
    /// tests that want to observe which branch was taken).
    pub last_choice: Option<usize>,
}

impl InlineDialogue {
    /// Start running `bytecode` from `pc`. The stored `DialogRequest.inline`
    /// begins at the first `0x1F` segment, so callers pass `pc = 0`.
    pub fn new(bytecode: Arc<Vec<u8>>, pc: usize) -> Self {
        Self {
            bytecode,
            ctx: FieldCtx::default(),
            pc,
            panel: None,
            done: false,
            last_choice: None,
        }
    }

    /// Convenience constructor from an owned inline buffer.
    pub fn from_inline(inline: Vec<u8>) -> Self {
        Self::new(Arc::new(inline), 0)
    }

    /// The glyph bytes of the box currently being typed (empty if no box).
    pub fn page_bytes(&self) -> Vec<u8> {
        self.panel
            .as_ref()
            .map(|p| p.page_bytes())
            .unwrap_or_default()
    }

    /// `true` when a box is open and awaiting a confirm / choice.
    pub fn waiting(&self) -> bool {
        self.panel
            .as_ref()
            .is_some_and(|p| p.is_waiting_for_input())
    }

    /// `true` when a box is open and it is a multiple-choice menu.
    pub fn menu_active(&self) -> bool {
        self.panel.as_ref().is_some_and(|p| p.menu_active())
    }

    pub fn is_done(&self) -> bool {
        self.done
    }
}

//! Faithful runner for an actor's inline interaction script.
//!
//! The simplified [`crate::dialog::OwnedDialogPanel`] types one `0x1F` text
//! segment and resolves a picker locally, but it never *executes* the field-VM
//! bytecode that surrounds the segments - the prologue's story-flag tests, the
//! `SET`/`CLEAR` flag ops, and the scene-change a branch handler runs after a
//! choice. This runner closes that gap by driving the inline script through the
//! real ported field VM ([`legaia_engine_vm::field::step`]) and only pausing to
//! show a box when the VM lands on a text segment.
//!
//! It mirrors the retail dialog state machine `FUN_80039B7C`, which runs the
//! field-VM dispatcher `FUN_801DE840` on the inline stream (`actor[+0x90]` base,
//! `actor[+0x9E]` PC) and transitions into the pager only when the dispatcher
//! leaves the PC on a byte where `& 0x7F < 0x20` (a `0x1F` lead or a `0x00..1E`
//! terminator). Between boxes the field VM's side effects - flag writes
//! (`system_flag_set`/`_clear`), the choice-selected branch jump
//! (`FUN_80038050`, applied by the host on confirm), scene changes - run
//! through the World host exactly as the scene script's do.
//!
//! The stepping itself lives on [`crate::world::World`] (it needs the
//! `FieldHostImpl` borrow); this module holds the resumable state.

use std::sync::Arc;

use legaia_engine_vm::field::FieldCtx;

use crate::dialog::OwnedDialogPanel;

/// Maximum field-VM steps to run between text boxes in one tick - bounds a
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
    /// When the runner is started with a prologue (`pc` points before the first
    /// text segment), this holds the offset of that first `0x1F` segment. If the
    /// prologue terminates - hits a `Halt`/`Unknown` op or a non-`0x1F`
    /// terminator - before opening any box, the runner resumes here instead of
    /// ending, so prologue execution is never worse than the truncated path.
    /// Consumed (set to `None`) the first time a box opens or the fallback fires.
    pub fallback_segment_pc: Option<usize>,
    /// The field-NPC placement slot this interaction record belongs to, when
    /// known (set by the interact dispatch). The world's step loop exposes it
    /// to the field-VM host so the prologue's `0x4C 0x51` NPC-run ops can
    /// walk the right actor. `None` for hand-started scripts.
    pub npc_slot: Option<u8>,
    /// When this runner executes a placed **prop's** bind record (a door
    /// touch, a cupboard interact), the prop's [`crate::field_env::PropAnimBank`]
    /// anchor key. The stepping loop then bridges the executing context to the
    /// prop's live actor state - `ctx.local_flags` is the actor's `+0x62`
    /// anim-control word, `ctx.flags` its `+0x10` class word - exactly as
    /// retail's dialog SM (`FUN_80039B7C`) runs the dispatcher on the touched
    /// actor's own record, and parks (instead of ending) on the waitable ops
    /// (`2D 08` until the clip's end latch). `None` for NPC conversations.
    pub prop_anchor: Option<(u8, u8)>,
    /// Consecutive frames a prop-bound run has stayed parked on a waitable op
    /// (`2D 08` end-latch spin, `4A` frame wait). The prop stepper bounds it
    /// so a decode drift can never soft-lock the engaged player.
    pub park_frames: u32,
    /// Per-byte "an instruction was executed here" map over
    /// [`Self::bytecode`]. Interaction records are **resident conversation
    /// drivers**: every story-state branch exits by jumping to a shared tail
    /// that loops back to the top selector, and retail parks there until the
    /// next talk. A VM `Advance` jumping backward onto an already-executed PC
    /// is that loop-back - the end of ONE conversation pass - so the runner
    /// ends there instead of replaying the branch forever. Cleared on every
    /// picker commit so menu records (which re-emit their menu by jumping
    /// back after a branch reply) still cycle - a user choice is progress.
    pub visited: Vec<bool>,
}

impl InlineDialogue {
    /// Start running `bytecode` from `pc`. The stored `DialogRequest.inline`
    /// begins at the first `0x1F` segment, so callers pass `pc = 0`.
    pub fn new(bytecode: Arc<Vec<u8>>, pc: usize) -> Self {
        let visited = vec![false; bytecode.len()];
        Self {
            bytecode,
            ctx: FieldCtx::default(),
            pc,
            panel: None,
            done: false,
            last_choice: None,
            fallback_segment_pc: None,
            npc_slot: None,
            prop_anchor: None,
            park_frames: 0,
            visited,
        }
    }

    /// Convenience constructor from an owned inline buffer.
    pub fn from_inline(inline: Vec<u8>) -> Self {
        Self::new(Arc::new(inline), 0)
    }

    /// Start running the full interaction record `bytecode` from `entry_pc` (the
    /// record's `script_pc0`) so the **interaction prologue** - the field-VM
    /// bytecode before the first text segment - executes first. The prologue's
    /// `SysFlag.Test`/`JmpRel` chain selects which segment the box opens at per
    /// story state. `first_segment` is the offset of the first `0x1F`; if the
    /// prologue can't reach a segment the runner falls back to it. Mirrors retail
    /// `FUN_80039B7C` state 0 calling the dispatcher on the record from
    /// `actor[+0x9E]` rather than from the first segment.
    pub fn with_prologue(bytecode: Arc<Vec<u8>>, entry_pc: usize, first_segment: usize) -> Self {
        let visited = vec![false; bytecode.len()];
        Self {
            bytecode,
            ctx: FieldCtx::default(),
            pc: entry_pc,
            panel: None,
            done: false,
            last_choice: None,
            fallback_segment_pc: Some(first_segment),
            npc_slot: None,
            prop_anchor: None,
            park_frames: 0,
            visited,
        }
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

    /// The decoded option picker of the open menu box, for rendering the
    /// option labels (`None` unless a menu box is open).
    pub fn picker(&self) -> Option<&legaia_mes::Picker> {
        self.panel.as_ref().and_then(|p| p.picker())
    }

    /// Highlighted option index of the open menu box.
    pub fn picker_cursor(&self) -> usize {
        self.panel.as_ref().map_or(0, |p| p.picker_cursor())
    }

    pub fn is_done(&self) -> bool {
        self.done
    }
}

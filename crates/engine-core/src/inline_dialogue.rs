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

/// Consecutive parked ticks a clip-end spin (`2D <bit>` / `AD <target> <bit>`)
/// may hold an NPC conversation before the runner gives up and ends it.
///
/// A **net, not a mechanism.** The bit a spin waits on is written by the poked
/// actor's clip cursor in [`crate::field_env::PropAnim::tick`], so a spin whose
/// target the port resolves to a cursor drains in that clip's own frame count -
/// tens of frames, never this. What the timeout still covers is a spin the
/// port cannot pair with a cursor at all - a record's **own-context** `2D`
/// waiting on a bit some unmodelled subsystem writes, or a cross-context one
/// whose target was never poked with a clip. Those must not leave the player
/// standing in a box forever.
pub const INLINE_SPIN_PARK_TIMEOUT: u32 = 600;

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
    /// Consecutive frames a run has stayed parked on a waitable op (`2D 08`
    /// end-latch spin, `4A` frame wait). Both steppers bound it so an
    /// unresolvable wait can never soft-lock the engaged player.
    pub park_frames: u32,
    /// Per-byte "this pass has already been here" map over
    /// [`Self::bytecode`]. Interaction records are **resident conversation
    /// drivers**: every story-state branch exits by jumping to a shared tail
    /// that loops back to the top selector, and retail parks there until the
    /// next talk. A VM `Advance` jumping backward onto an already-visited PC
    /// is that loop-back - the end of ONE conversation pass - so the runner
    /// ends there instead of replaying the branch forever.
    ///
    /// **Text segments count.** A `0x1F` lead is marked when its box opens,
    /// not only when an opcode executes: a record's tail commonly jumps back
    /// onto the opening *line* rather than onto an opcode, and marking only
    /// opcodes made the detector structurally unable to see that. Four of Rim
    /// Elm's thirty-six talkable placements loop exactly that way, and each
    /// replayed without limit - an NPC conversation with no exit. A box is
    /// identified by its segment PC, so that is what the map has to carry.
    ///
    /// The picker commit clears the map only **from the branch target
    /// forward** (never before the prompt), because the branch may legitimately
    /// re-tread PCs this pass ran - a user choice is progress. Clearing the
    /// whole map disarms the detector for the rest of the conversation, which
    /// is the same defect in its second shape.
    pub visited: Vec<bool>,
    /// Where this conversation left the record's cursor when it ended at a
    /// box whose post-box byte does not continue the talk
    /// ([`TalkDispatch::EndParked`]) - retail's `actor[+0x9E]`, which the
    /// next talk on the same actor resumes from. `None` for every other end.
    pub parked_pc: Option<usize>,
}

/// What the dialog SM does with the byte after a finished text box: the
/// return value and cursor step of `FUN_80038050` (SCUS), which
/// `FUN_80039B7C` calls once a box's lines are scanned (`jal 0x80038050` at
/// `0x80039C84`) and which ends the talk on `0`
/// (`bne v0,zero,0x80039D64` at `0x80039C8C`; the fall-through clears the
/// actor's `+0x10 & 0x100`, zeroes `+0x9C` and releases the player lock).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TalkDispatch {
    /// The SM keeps running the record from the given cursor: `0x24`,
    /// `0x25` and `0x48` step one byte, `0x4C FF` two and `0x4C FE xx`
    /// three (the jump table at `0x80010F38` sends `0x24` / `0x25` / `0x48`
    /// to `0x800380A8`, `0x4C` to `0x80038104`). The option bytes
    /// `0x27..=0x2A` also continue; the runner resolves them through the
    /// picker instead.
    Continue(usize),
    /// `0x21`: the cursor steps past it and the talk ends
    /// (`0x8003809C`).
    EndAfter(usize),
    /// Any other byte - a `0x26` jump, an opcode - takes the table's default
    /// (`0x80038150`): the talk ends with the cursor left **on** it, not
    /// executed. The next talk on the actor starts there.
    EndParked(usize),
}

/// Classify the byte at `pc` (the one after a finished box) the way
/// `FUN_80038050` does. The byte is read raw: an `0x80`-prefixed byte is not
/// in the table and parks.
///
/// PORT: FUN_80038050 (the post-box classification; the option-jump apply
/// for `0x27..=0x2A` is `legaia_mes::Picker::jump_target`)
pub fn talk_dispatch(bytes: &[u8], pc: usize) -> TalkDispatch {
    match bytes.get(pc).copied() {
        Some(0x21) => TalkDispatch::EndAfter(pc + 1),
        Some(0x24 | 0x25 | 0x48) => TalkDispatch::Continue(pc + 1),
        Some(0x27..=0x2A) => TalkDispatch::Continue(pc),
        Some(0x4C) => match bytes.get(pc + 1).copied() {
            Some(0xFF) => TalkDispatch::Continue(pc + 2),
            Some(0xFE) => TalkDispatch::Continue(pc + 3),
            _ => TalkDispatch::EndParked(pc),
        },
        _ => TalkDispatch::EndParked(pc),
    }
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
            parked_pc: None,
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
            parked_pc: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_post_box_byte_decides_whether_the_talk_goes_on() {
        let b = [
            0x26, 0x9D, 0xFE, 0x21, 0x24, 0x25, 0x48, 0x4C, 0xFF, 0x4C, 0xFE, 0x07, 0x4C, 0x10,
            0x28, 0xA4,
        ];
        assert_eq!(
            talk_dispatch(&b, 0),
            TalkDispatch::EndParked(0),
            "a jump parks"
        );
        assert_eq!(talk_dispatch(&b, 3), TalkDispatch::EndAfter(4));
        assert_eq!(talk_dispatch(&b, 4), TalkDispatch::Continue(5));
        assert_eq!(talk_dispatch(&b, 5), TalkDispatch::Continue(6));
        assert_eq!(talk_dispatch(&b, 6), TalkDispatch::Continue(7));
        assert_eq!(talk_dispatch(&b, 7), TalkDispatch::Continue(9));
        assert_eq!(talk_dispatch(&b, 9), TalkDispatch::Continue(12));
        assert_eq!(talk_dispatch(&b, 12), TalkDispatch::EndParked(12));
        assert_eq!(talk_dispatch(&b, 14), TalkDispatch::Continue(14));
        assert_eq!(
            talk_dispatch(&b, 15),
            TalkDispatch::EndParked(15),
            "raw, not masked"
        );
        assert_eq!(talk_dispatch(&b, 99), TalkDispatch::EndParked(99));
    }
}

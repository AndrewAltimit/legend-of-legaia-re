//! Opening-cutscene timeline executor.
//!
//! The opening prologue scene (`opdeene`) carries a scripted cutscene timeline
//! in its scene MAN's third record partition (partition 2) - a field-VM record
//! (retail "Opening", dispatched by `FUN_8003BDE0`) that stages the closing
//! camera path + actor `MoveTo`s and ends with `GFLAG_SET 26`, the write the
//! `town01` hand-off gate (`FUN_801D1344`) waits on.
//!
//! [`CutsceneTimeline`] is a *spawned* field-VM context that runs that record
//! frame-by-frame, separate from the scene-entry system script on
//! [`crate::world::World::field_ctx`]. Running it through the same field VM
//! ([`legaia_engine_vm::field::step`]) makes the camera Configure ops (`0x45`)
//! and actor MoveTo ops (`0x23`) fire by execution - emitting the same camera /
//! move [`crate::field_events::FieldEvent`]s the runtime camera
//! ([`crate::camera::Camera`]) already folds in - and lets `GFLAG_SET 26` fire
//! by execution instead of a static MAN-walk derivation. The driver lives in
//! [`crate::world::World::step_cutscene_timeline`]; this type is just the
//! cursor + halt bookkeeping around the VM step.
//!
//! ## Clean-room boundary
//!
//! No Sony bytes live here. The record body is sliced from the user's disc MAN
//! at runtime and handed in; this module only holds the per-context cursor.
//!
//! ## Approximate by design
//!
//! The spawned context targets the camera / lead-actor anchor (retail
//! cross-context target `0xF8`, `_DAT_8007C364`). Because the engine runs a
//! single shared field VM, cross-context (`0x80`-bit) ops operate on this
//! context rather than resolving a distinct per-target context, and the inline
//! narration op (`0xCC 0xF8 0x80 N`, which retail routes to the `FUN_8003C764`
//! text-balloon path) is presented separately by
//! [`crate::cutscene_narration::CutsceneNarration`] - so its actor-allocator
//! host hook is suppressed while the timeline steps (see
//! [`crate::world::World::step_cutscene_timeline`]). The result is a faithful
//! GFLAG-by-execution + camera-event stream, with an approximate camera path
//! until the remaining op-`0x45` eye/distance params are pinned.

use legaia_engine_vm::field::FieldCtx;

/// One executed instruction in a timeline op-stream trace.
///
/// Recorded by [`crate::world::World::step_cutscene_timeline`] when the
/// timeline's [`CutsceneTimeline::trace_enabled`] flag is set. The trace is the
/// engine VM's *authoritative* decode of the record bytecode - it follows the
/// real per-op PC stride, so it never drifts the way a linear disassembler does
/// through the variable-width `0x4C` menu-control op. Used to correlate which
/// field-VM op opens a downstream UI (e.g. the `town01` opening's name-entry
/// prompt) against a save-state oracle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceEntry {
    /// Byte offset of the opcode in the record bytecode.
    pub pc: usize,
    /// Raw opcode byte, including the `0x80` cross-context (extended) bit.
    pub opcode_byte: u8,
    /// Decoded opcode (`opcode_byte & 0x7F`).
    pub opcode: u8,
    /// PC after the step (the resume / advance target).
    pub next_pc: usize,
    /// How the VM resolved this step.
    pub result: TraceResult,
}

/// The [`legaia_engine_vm::field::StepResult`] discriminant for a [`TraceEntry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceResult {
    /// Normal advance to the next instruction.
    Advance,
    /// Per-frame yield (the VM ran until a `YIELD`).
    Yield,
    /// Held at PC (WAIT_FRAMES / conditional hold / un-advanceable op).
    Halt,
    /// A host hook fired but the op needs more support to advance.
    Pending,
    /// Unknown / out-of-range opcode.
    Unknown,
}

/// An inline narration block inside a timeline record: the byte offset of its
/// introducing op (`0xCC 0xF8 0x80 N`), the offset just past its last page's
/// terminator, and the decoded pages.
///
/// Retail routes the introducing op to the on-screen-text spawner
/// (`FUN_8003C764`): a caption child context is spawned over the inline pages
/// and the *parent* timeline halt-suspends at the op until the child exhausts
/// them - so the choreography around a block runs between blocks, never under
/// them. [`crate::world::World::step_cutscene_timeline`] mirrors that: when the
/// timeline PC reaches `op_offset` it installs the pages on the
/// [`crate::cutscene_narration::CutsceneNarration`] presenter and parks until
/// the presenter completes, then resumes at `end`.
// REF: FUN_8003C764
#[derive(Debug, Clone)]
pub struct NarrationSite {
    /// Byte offset of the introducing `0x4C` narration op in the record body.
    pub op_offset: usize,
    /// Byte offset just past the block (the next opcode after the pages).
    pub end: usize,
    /// The decoded subtitle pages, in display order.
    pub pages: Vec<String>,
    /// Presentation form: a crawl suspends the timeline while the roller
    /// plays; a static title card installs (or, when its pages are blank,
    /// clears) the card overlay and the timeline continues.
    pub kind: legaia_asset::cutscene_text::NarrationKind,
}

/// A spawned field-VM context running the `opdeene` cutscene-timeline record.
///
/// Built by [`crate::world::World::load_cutscene_timeline_from_man`] from the
/// partition-2 record that issues `GFLAG_SET 26`; stepped by
/// [`crate::world::World::step_cutscene_timeline`] until it executes that write
/// (the timeline's terminal op) or its safety cap forces it complete.
#[derive(Debug, Clone)]
pub struct CutsceneTimeline {
    /// The spawned cutscene context (camera / lead-actor anchor). Its
    /// `script_id` is set to the system channel (`0xFB`) so cross-context
    /// (`0x80`-bit) ops keep running after the record's first `YIELD` sets the
    /// context halt bit - see the `step` prelude halt carve-out.
    pub ctx: FieldCtx,
    /// The partition-2 record body, sliced from its `script_start` so relative
    /// jumps wrap against the record base (retail `buffer_base = script_start`).
    /// Shared (`Arc`) so an inline dialog panel can page over the same bytes
    /// while the timeline is parked at the segment.
    pub bytecode: std::sync::Arc<Vec<u8>>,
    /// Current byte offset into [`Self::bytecode`]. Starts at the record's
    /// first-opcode offset (`pc0`, the named-record header end).
    pub pc: usize,
    /// Set once the timeline completes - it executed its closing `GFLAG_SET 26`,
    /// hit an op it cannot advance past, or exceeded its frame cap.
    pub done: bool,
    /// Frames the timeline has been stepping (for the safety cap).
    pub frames: u32,
    /// When `true`, [`crate::world::World::step_cutscene_timeline`] appends a
    /// [`TraceEntry`] per executed instruction to [`Self::trace`]. Off by
    /// default (no overhead on the normal opening path); turned on for the RE
    /// op-stream correlation harness.
    pub trace_enabled: bool,
    /// The recorded op stream when [`Self::trace_enabled`] is set.
    pub trace: Vec<TraceEntry>,
    /// When `true`, this timeline's terminal op is the `opdeene` prologue's
    /// `GFLAG_SET 26`, so completing it (or hitting the frame cap) arms the
    /// `town01` hand-off. The `town01` opening timeline sets this `false` - it
    /// drives the establishing shot + name-entry handoff, not a scene change,
    /// so it must never arm a prologue hand-off. See
    /// [`crate::world::World::step_cutscene_timeline`].
    pub arms_prologue_handoff: bool,
    /// The record's inline narration blocks, in script order (parsed once at
    /// install). The stepper suspends the timeline at each block's op until
    /// the narration presenter finishes its pages - the retail caption-child
    /// suspend - then resumes past the block.
    pub narration_blocks: Vec<NarrationSite>,
    /// `Some(op_offset)` while the timeline is held at that narration block.
    /// Two hold shapes share this field, disambiguated by
    /// [`Self::narration_pending_open`]:
    /// - a **blocking** block (the last crawl before the scene transition) that
    ///   has opened its roller and is waiting for it to scroll out before the
    ///   PC advances past it (`narration_pending_open == false`); and
    /// - a block reached while a PRIOR roller is still scrolling, held until
    ///   that roller drains so a second roller doesn't stack over it
    ///   (`narration_pending_open == true` - the pre-step gate then re-enters
    ///   the block's op to open it).
    ///
    /// A **non-blocking** crawl (any block that is not the last) opens its
    /// roller and lets the PC continue into the camera-cut / fade / wait ops
    /// that play UNDER the scrolling text (retail spawns the roller as a child
    /// context and keeps executing the parent timeline), so it never sets this.
    pub narration_pc: Option<usize>,
    /// See [`Self::narration_pc`]: distinguishes "held, waiting to OPEN this
    /// block once a prior roller drains" (`true`) from "opened, waiting for
    /// THIS block's roller to scroll out" (`false`).
    pub narration_pending_open: bool,
    /// An open inline dialog box (`0x1F`-lead glyph segment reached by the
    /// record's own flow, e.g. the Mei walk-on beat's conversation). While
    /// `Some`, the timeline is parked at the segment lead - the retail dialog
    /// state machine's `pc byte & 0x7F < 0x20` transition - and the stepper
    /// routes pad input to the panel (confirm advances / dismisses, Up/Down
    /// move a picker cursor). On dismissal the timeline resumes at the
    /// panel's final PC (past the consumed segment).
    pub dialog: Option<crate::dialog::OwnedDialogPanel>,
    /// Per-byte "an instruction was executed here" map over
    /// [`Self::bytecode`], kept for the timeline's whole life. A backward
    /// jump into an already-executed PC means the record's linear
    /// choreography has wrapped - the on-disc records have no end opcode;
    /// they either park in a tight `Nop`+`JmpRel`-to-self spin or loop as a
    /// **resident** actor-driver context (e.g. the town01 Mei beat re-enters
    /// its conversation loop from the top). Retail leaves that context
    /// looping as a *parallel* context; the engine's modal timeline
    /// completes there instead so control returns to the player.
    pub visited: Vec<bool>,
}

impl CutsceneTimeline {
    /// System-channel id for the spawned context (see [`Self::ctx`]).
    const SYSTEM_SCRIPT_ID: u16 = 0xFB;

    /// Build a timeline over `bytecode` starting at `pc` (the record's
    /// first-opcode offset). The context is seeded on the system channel so
    /// cross-context ops survive the first `YIELD`.
    pub fn new(bytecode: Vec<u8>, pc: usize) -> Self {
        let ctx = FieldCtx {
            script_id: Self::SYSTEM_SCRIPT_ID,
            ..FieldCtx::default()
        };
        let visited = vec![false; bytecode.len()];
        Self {
            ctx,
            bytecode: std::sync::Arc::new(bytecode),
            pc,
            done: false,
            frames: 0,
            trace_enabled: false,
            trace: Vec::new(),
            arms_prologue_handoff: false,
            narration_blocks: Vec::new(),
            narration_pc: None,
            narration_pending_open: false,
            dialog: None,
            visited,
        }
    }

    /// `true` once the timeline has completed.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Enable op-stream tracing (see [`Self::trace`]). Returns `self` for
    /// builder-style use on the RE correlation harness.
    pub fn with_trace(mut self) -> Self {
        self.trace_enabled = true;
        self
    }

    /// Mark this timeline as the `opdeene` prologue (its terminal `GFLAG_SET 26`
    /// arms the `town01` hand-off; see [`Self::arms_prologue_handoff`]).
    pub fn arming_prologue_handoff(mut self) -> Self {
        self.arms_prologue_handoff = true;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_seeds_system_channel_and_pc() {
        let tl = CutsceneTimeline::new(vec![0x21, 0x2E, 0x1A], 1);
        assert_eq!(tl.ctx.script_id, CutsceneTimeline::SYSTEM_SCRIPT_ID);
        assert_eq!(tl.pc, 1);
        assert!(!tl.is_done());
        assert_eq!(tl.frames, 0);
        assert!(!tl.trace_enabled);
    }

    #[test]
    fn with_trace_enables_tracing() {
        let tl = CutsceneTimeline::new(vec![0x21], 0).with_trace();
        assert!(tl.trace_enabled);
        assert!(tl.trace.is_empty());
    }
}

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
    pub bytecode: Vec<u8>,
    /// Current byte offset into [`Self::bytecode`]. Starts at the record's
    /// first-opcode offset (`pc0`, the named-record header end).
    pub pc: usize,
    /// Set once the timeline completes - it executed its closing `GFLAG_SET 26`,
    /// hit an op it cannot advance past, or exceeded its frame cap.
    pub done: bool,
    /// Frames the timeline has been stepping (for the safety cap).
    pub frames: u32,
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
        Self {
            ctx,
            bytecode,
            pc,
            done: false,
            frames: 0,
        }
    }

    /// `true` once the timeline has completed.
    pub fn is_done(&self) -> bool {
        self.done
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
    }
}

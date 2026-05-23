//! Synthetic (disc-free, CI) coverage for the opening-cutscene timeline
//! executor: the spawned field-VM context that fires the Rim Elm hand-off bit
//! by execution.
//!
//! These exercise the [`World::step_cutscene_timeline`] mechanism with
//! hand-authored field-VM bytecode (no Sony bytes), so they run in CI without
//! `LEGAIA_DISC_BIN`. The disc-gated `opdeene_timeline_execution` test drives
//! the real `opdeene` record.

use legaia_engine_core::cutscene_timeline::CutsceneTimeline;
use legaia_engine_core::world::{PROLOGUE_HANDOFF_FLAG, SceneMode, World};

/// A timeline ending in `GFLAG_SET 26` sets the hand-off bit by execution in a
/// single tick, then reports complete.
#[test]
fn timeline_fires_handoff_bit_by_execution() {
    let mut w = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    // `[GFLAG_SET 26]` = op 0x2E operand 0x1A (bit 26), then a YIELD so the
    // run-until-yield loop ends cleanly on the same frame.
    w.cutscene_timeline = Some(CutsceneTimeline::new(vec![0x2E, 0x1A, 0x37], 0));
    assert!(w.cutscene_timeline_active());
    assert_eq!(w.story_flags & PROLOGUE_HANDOFF_FLAG, 0);

    let _ = w.tick();

    assert!(
        w.story_flags & PROLOGUE_HANDOFF_FLAG != 0,
        "executing GFLAG_SET 26 arms the hand-off bit"
    );
    assert!(
        !w.cutscene_timeline_active(),
        "the timeline completes once it sets the hand-off bit"
    );
    // The actor-allocator guard is inactive once stepping has finished.
    assert!(!w.in_cutscene_timeline);
}

/// A timeline that can never reach its closing op (it holds on an
/// unimplemented opcode) is still forced complete by the frame-cap safety net,
/// which arms the hand-off statically so the prologue can't stall.
#[test]
fn timeline_safety_net_arms_when_execution_stalls() {
    let mut w = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    // Opcode 0x05 is below the valid range and the field VM holds on it
    // (returns `Halt` at the same PC each step), so the timeline never reaches
    // a `GFLAG_SET`. The frame cap eventually forces completion.
    w.cutscene_timeline = Some(CutsceneTimeline::new(vec![0x05], 0));

    let mut ticks = 0u32;
    // Generous cap above the timeline's internal frame cap.
    while w.cutscene_timeline_active() && ticks < 4000 {
        let _ = w.tick();
        ticks += 1;
    }

    assert!(
        !w.cutscene_timeline_active(),
        "a stalled timeline is forced complete by the frame cap (ticked {ticks})"
    );
    assert!(
        w.story_flags & PROLOGUE_HANDOFF_FLAG != 0,
        "the safety net arms the hand-off bit when execution can't reach it"
    );
}

/// A completed timeline does not re-fire or re-run on subsequent ticks.
#[test]
fn completed_timeline_is_idempotent() {
    let mut w = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    w.cutscene_timeline = Some(CutsceneTimeline::new(vec![0x2E, 0x1A, 0x37], 0));
    let _ = w.tick();
    assert!(!w.cutscene_timeline_active());

    // Clear the bit and tick again: a done timeline must not re-execute and
    // re-set it.
    w.story_flags &= !PROLOGUE_HANDOFF_FLAG;
    let _ = w.tick();
    assert_eq!(
        w.story_flags & PROLOGUE_HANDOFF_FLAG,
        0,
        "a completed timeline does not re-run"
    );
}

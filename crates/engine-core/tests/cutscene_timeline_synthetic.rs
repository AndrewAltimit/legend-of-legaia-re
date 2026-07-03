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

/// A timeline executing `GFLAG_SET 26` sets the hand-off bit by execution but
/// KEEPS RUNNING - the real `opdeene` record arms the bit near its top
/// (`+0x17`) and then stages the vignette choreography, so "bit armed" must
/// not complete the timeline. It completes when the record reaches a terminal
/// state (here: running off the end).
#[test]
fn timeline_fires_handoff_bit_by_execution() {
    let mut w = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    // `[GFLAG_SET 26][YIELD]` = op 0x2E operand 0x1A (bit 26), then a YIELD
    // ending the first frame slice. `arming_prologue_handoff` marks this an
    // opdeene-style timeline.
    w.cutscene_timeline =
        Some(CutsceneTimeline::new(vec![0x2E, 0x1A, 0x37], 0).arming_prologue_handoff());
    assert!(w.cutscene_timeline_active());
    assert_eq!(w.story_flags & PROLOGUE_HANDOFF_FLAG, 0);

    let _ = w.tick();

    assert!(
        w.story_flags & PROLOGUE_HANDOFF_FLAG != 0,
        "executing GFLAG_SET 26 arms the hand-off bit"
    );
    assert!(
        w.cutscene_timeline_active(),
        "the timeline KEEPS RUNNING after arming the bit (the record stages \
         the vignettes after its top-of-record GFLAG_SET)"
    );
    // The actor-allocator guard is inactive once stepping has finished.
    assert!(!w.in_cutscene_timeline);

    // The next tick resumes past the YIELD, runs off the record end, and
    // completes.
    let _ = w.tick();
    assert!(
        !w.cutscene_timeline_active(),
        "the timeline completes when the record reaches a terminal state"
    );
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
    // Opcode 0x05 is below the valid range, so the timeline never reaches a
    // `GFLAG_SET`. An opdeene-style (`arming_prologue_handoff`) timeline that
    // can't reach its closing op is forced complete and arms the hand-off
    // statically as a safety net.
    w.cutscene_timeline = Some(CutsceneTimeline::new(vec![0x05], 0).arming_prologue_handoff());

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
    w.cutscene_timeline =
        Some(CutsceneTimeline::new(vec![0x2E, 0x1A, 0x37], 0).arming_prologue_handoff());
    // Tick 1 arms the bit + yields; tick 2 runs off the record end and
    // completes.
    let _ = w.tick();
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

/// A `town01`-opening-style timeline (not arming a hand-off) opens the
/// name-entry overlay when it executes op `0x49` STATE_RESUME, suspends while
/// the overlay is up, then resumes (and completes) once the name commits.
#[test]
fn opening_timeline_op49_opens_name_entry_then_resumes() {
    let mut w = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    // Mark this the new-game opening so the timeline's op-0x49 opens name entry.
    w.prologue_naming_pending = true;
    // `[0x49 0x03 0x00]` = STATE_RESUME sub-op 3 (the name-entry handoff form).
    w.cutscene_timeline = Some(CutsceneTimeline::new(vec![0x49, 0x03, 0x00], 0));

    let _ = w.tick();
    assert!(
        w.name_entry_active(),
        "executing op-0x49 in the opening timeline opens name entry"
    );
    assert!(w.prologue_naming_armed, "the op-49 hook armed the handoff");
    assert_eq!(
        w.cutscene_timeline.as_ref().map(|t| t.pc),
        Some(0),
        "the timeline parks on the op-0x49"
    );
    assert_eq!(
        w.story_flags & PROLOGUE_HANDOFF_FLAG,
        0,
        "the opening timeline never arms a prologue scene hand-off"
    );

    // Frozen while the overlay is up.
    let frames = w.cutscene_timeline.as_ref().unwrap().frames;
    let _ = w.tick();
    assert!(w.name_entry_active());
    assert_eq!(
        w.cutscene_timeline.as_ref().unwrap().frames,
        frames,
        "the timeline does not advance while name entry is open"
    );

    // Simulate the name committing: the overlay closes, op-0x49 reports Done,
    // the timeline resumes past it and (running off the end) completes + drops.
    w.name_entry = None;
    let mut ticks = 0;
    while w.cutscene_timeline.is_some() && ticks < 100 {
        let _ = w.tick();
        ticks += 1;
    }
    assert!(
        w.cutscene_timeline.is_none(),
        "the opening timeline resumes and completes after the name commits"
    );
}

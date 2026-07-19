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

/// Tick the world until one **retail display frame** has elapsed.
///
/// The sim clock runs at 100 Hz but spawned-record contexts are paced off the
/// 60 Hz retail-frame sub-clock (`World::step_spawned_record_contexts`), so
/// "advance the timeline one step" is "tick until `field_frame_step` fires",
/// not "call `tick()` once".
fn step_frame(w: &mut World) {
    for _ in 0..8 {
        let _ = w.tick();
        if w.field_frame_step == 1 {
            return;
        }
    }
    panic!("no retail display frame within 8 sim ticks");
}

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

    step_frame(&mut w);

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
    step_frame(&mut w);
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
        step_frame(&mut w);
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
    step_frame(&mut w);
    step_frame(&mut w);
    assert!(!w.cutscene_timeline_active());

    // Clear the bit and tick again: a done timeline must not re-execute and
    // re-set it.
    w.story_flags &= !PROLOGUE_HANDOFF_FLAG;
    step_frame(&mut w);
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

    step_frame(&mut w);
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
    step_frame(&mut w);
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
        step_frame(&mut w);
        ticks += 1;
    }
    assert!(
        w.cutscene_timeline.is_none(),
        "the opening timeline resumes and completes after the name commits"
    );
}

/// A cross-context walk-to-tile yield against the player anchor
/// (`C7 F8 <tx> <tz> <mode>`) PARKS the timeline and plays the walk out over
/// frames - retail saves the yield-op pointer into the target actor and the
/// per-frame walk kernel (`FUN_8003774C` case 0x47) glides it to the decoded
/// tile at `0x80 >> (2 + (mode & 7))` units per frame, resuming the record on
/// arrival. The town01 Mei beat's `C7 F8 12 1A 33` / `C7 46 11 1A 33` pair is
/// the disc case; this pins the player arm disc-free.
#[test]
fn walk_to_tile_yield_parks_and_glides_the_player() {
    let mut w = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    w.install_field_player(0);
    let slot = w.player_actor_slot.expect("player installed") as usize;
    // Seat two tiles east of the walk target (18,26) = (2368, 3392).
    w.actors[slot].move_state.world_x = 18 * 128 + 0x40 + 256;
    w.actors[slot].move_state.world_z = 26 * 128 + 0x40;
    // `[C7 F8 12 1A 33][GFLAG_SET 16]`: the flag write is the arrival proof.
    w.cutscene_timeline = Some(CutsceneTimeline::new(
        vec![0xC7, 0xF8, 0x12, 0x1A, 0x33, 0x2E, 0x10],
        0,
    ));

    step_frame(&mut w);
    let tl = w.cutscene_timeline.as_ref().expect("timeline still up");
    assert!(
        tl.walk_wait.is_some(),
        "the walk-to-tile yield parks the timeline on a TimelineWalk"
    );
    // mode 0x33 -> bits 3 -> 0x80 >> 5 = 4 units per tick; 256 units takes
    // ~64 ticks. After a few ticks the player has moved but not arrived.
    for _ in 0..10 {
        step_frame(&mut w);
    }
    let slot = w.player_actor_slot.unwrap() as usize;
    let mid_x = w.actors[slot].move_state.world_x;
    assert!(
        mid_x < 18 * 128 + 0x40 + 256 && mid_x > 18 * 128 + 0x40,
        "the walk plays out over frames (glide, not a snap): x={mid_x}"
    );
    let mut ticks = 0u32;
    while w.cutscene_timeline.is_some() && ticks < 400 {
        step_frame(&mut w);
        ticks += 1;
    }
    assert!(
        w.cutscene_timeline.is_none(),
        "the record resumes past the yield on arrival and completes"
    );
    let ms = &w.actors[slot].move_state;
    assert_eq!(
        (ms.world_x, ms.world_z),
        (18 * 128 + 0x40, 26 * 128 + 0x40),
        "the walk lands exactly on the decoded tile centre"
    );
}

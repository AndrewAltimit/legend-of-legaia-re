//! The retail actor tick-cadence law, and the ambient facing channel that
//! rides on it.
//!
//! Retail resolves `DAT_1F800393` once per frame (`FUN_80016B6C`) as
//! `max(adaptive_frameskip, DAT_8007B9D8)` and runs the per-actor pool once
//! per *game tick* - a span of that many vsyncs. The field floor is `2`
//! (installed by the scene loader `FUN_801D6704`), so ordinary field play
//! advances the actor pool every second vsync with a `frame_delta` of 2.
//!
//! The property that makes this safe to change is **cadence invariance**:
//! everything retail measures as a duration accumulates `DAT_1F800393`
//! rather than `1` (`t = min(t + dt, d)`), so a budget denominated in vsyncs
//! is reached after the same number of vsyncs at every cadence. What a
//! cadence change moves is the *sample rate* - retail emits a pose every
//! `frame_step` vsyncs, so a higher cadence draws proportionally fewer
//! intermediate poses across an identical wall-clock span.
//!
//! These tests pin exactly that: same endpoint, same elapsed time, fewer
//! samples. A change that moves any duration is a regression, not a tuning
//! opportunity.

use super::*;

/// Sim ticks to run. The sim clocks at 100 Hz and marks a retail vsync on
/// the ~60 % of ticks that cross it, so 200 sim ticks = 120 vsyncs - an even
/// multiple of both cadences under test, which keeps the comparison free of
/// partial-tick remainders.
const SIM_TICKS: usize = 200;
const EXPECTED_VSYNCS: i32 = 120;

/// A world whose only job is to integrate one actor's acceleration. Title
/// mode keeps the tick to the actor pool alone (no field / battle dispatch),
/// so the trace is the dispatcher's arithmetic and nothing else.
///
/// `accel[0] = 64` makes the default-movement arm's `accel * product >> 6`
/// integrate exactly `product` units per firing tick, so the accumulated
/// `motion_x` reads directly as "vsyncs of motion applied".
fn cadence_world(frame_step: u8) -> World {
    let mut w = World::new();
    w.mode = SceneMode::Title;
    w.frame_step = frame_step;
    w.actors[0].active = true;
    w.actors[0].set_physics_dispatch(0x00);
    w.actors[0].physics.accel = [64, 0, 0];
    // Keep the countdown far from zero so the kill arm never fires.
    w.actors[0].physics.timer = i16::MAX;
    w
}

/// Run `SIM_TICKS` and return `(final motion_x, distinct intermediate poses)`.
/// A "pose" is a tick on which the dispatcher actually advanced the actor -
/// exactly what a renderer would have a new frame of motion to draw.
fn run(frame_step: u8) -> (i16, usize) {
    let mut w = cadence_world(frame_step);
    let mut poses = 0usize;
    let mut prev = w.actors[0].physics.motion_x;
    for _ in 0..SIM_TICKS {
        w.tick();
        let now = w.actors[0].physics.motion_x;
        if now != prev {
            poses += 1;
            prev = now;
        }
    }
    (w.actors[0].physics.motion_x, poses)
}

/// The headline property. Cadence 1 and cadence 2 travel the **same
/// distance** over the same wall-clock span - the duration does not move -
/// while cadence 2 produces **half the intermediate poses**.
///
/// This is the whole of the field-motion divergence the cadence change
/// closes: retail's motion between two endpoints is chunkier than a port
/// ticking every vsync, never faster or slower.
#[test]
fn cadence_conserves_duration_and_halves_the_pose_sample_rate() {
    let (dist_full, poses_full) = run(1);
    let (dist_field, poses_field) = run(2);

    // Same wall clock, same displacement. `accel*product>>6` with accel 64
    // is `product` per tick, so this is literally "vsyncs elapsed".
    assert_eq!(
        dist_full as i32, EXPECTED_VSYNCS,
        "cadence 1 integrates one unit per vsync"
    );
    assert_eq!(
        dist_field, dist_full,
        "DURATION MUST NOT MOVE: a cadence change is a sample-rate change. \
         If this fails, the gate and the scalars are out of step - fix the \
         pairing, do not retune the expectation."
    );

    // Sample rate is what changed, and it changed by exactly the cadence.
    assert_eq!(
        poses_full, EXPECTED_VSYNCS as usize,
        "cadence 1 poses once per vsync"
    );
    assert_eq!(
        poses_field,
        poses_full / 2,
        "cadence 2 emits a pose every second vsync"
    );
}

/// Generalisation of the above across the whole `1..=4` range
/// `FUN_80016B6C` can produce: displacement is invariant, pose count scales
/// as `1 / cadence`.
#[test]
fn every_retail_cadence_lands_the_same_distance() {
    let (baseline, base_poses) = run(1);
    for cadence in 2..=4u8 {
        let (dist, poses) = run(cadence);
        assert_eq!(
            dist, baseline,
            "cadence {cadence} must cover the same ground as cadence 1"
        );
        assert_eq!(
            poses,
            base_poses / usize::from(cadence),
            "cadence {cadence} must emit 1/{cadence} of the poses"
        );
    }
}

/// The dispatcher's countdown timer is a duration, so it too is
/// cadence-invariant: `common_pre_update` drains it by `frame_delta * speed`,
/// which is the same total per vsync at any cadence.
#[test]
fn dispatcher_timers_drain_in_vsyncs_not_ticks() {
    for cadence in 1..=4u8 {
        let mut w = cadence_world(cadence);
        w.actors[0].physics.timer = 600;
        for _ in 0..SIM_TICKS {
            w.tick();
        }
        assert_eq!(
            i32::from(w.actors[0].physics.timer),
            600 - EXPECTED_VSYNCS,
            "cadence {cadence} must drain the timer by elapsed vsyncs"
        );
    }
}

// ------------------------------------------------------------------
// Ambient facing channel (FUN_80038158 ops 0x04 / 0x0D)
// ------------------------------------------------------------------

/// Install a one-variant ambient channel on `slot` running `code`.
fn with_ambient(w: &mut World, slot: u8, code: Vec<u8>, retail_heading: u16) {
    w.field_npc_ambient.insert(
        slot,
        FieldNpcAmbient {
            variants: vec![(legaia_asset::man_motion::SELECTOR_DEFAULT, code)],
            live: None,
            vm: vm::ambient_motion::AmbientMotion::new(u32::from(slot), retail_heading),
        },
    );
}

/// An idle NPC turns: the `0x04` ramp walks its heading onto the compass
/// point and the result reaches `field_npc_headings`, converted out of the
/// retail heading space into the engine's `render_26` space (`+0x800`).
#[test]
fn ambient_ramp_turns_a_standing_npc_and_reaches_the_render_heading() {
    let mut w = World::new();
    // `[04, lut=2 increasing, 8 frames]`: retail 0x000 -> 0x400.
    with_ambient(&mut w, 5, vec![0x04, 0x02, 0x08], 0x000);
    for _ in 0..8 {
        w.tick_field_npc_ambient();
    }
    let expected = (0x400u16.wrapping_add(0x800)) & 0x0FFF;
    assert_eq!(
        w.field_npc_headings.get(&5).copied(),
        Some(expected as i16),
        "the ambient turn's endpoint is mirrored into the render heading"
    );
}

/// The `yaw_written`-equivalent gate. An NPC parked in a `0x05` wait op must
/// leave alone whatever heading another writer posted - the interact
/// "face the speaker" bearing being the case that regressed before.
#[test]
fn an_idle_ambient_channel_does_not_clobber_a_posed_heading() {
    let mut w = World::new();
    // A long wait: the channel ticks every frame but never moves `+0x26`.
    with_ambient(&mut w, 5, vec![0x05, 0x7F], 0x000);
    // Another writer poses the NPC (an interact bearing).
    w.field_npc_headings.insert(5, 0x0A00);
    for _ in 0..16 {
        w.tick_field_npc_ambient();
    }
    assert_eq!(
        w.field_npc_headings.get(&5).copied(),
        Some(0x0A00),
        "a waiting ambient channel must not re-stamp its stale heading"
    );
}

/// Op `0x04`'s cursor is `addiu a0, a0, 1` - unit-per-tick, **not** scaled by
/// `DAT_1F800393` - so its budget is denominated in actor ticks and the leg
/// takes the same number of ticks at every cadence. Op `0x0D`'s wait cursor
/// *is* scalar-driven, which is what keeps it in lockstep with the ramp
/// scheduler. Both readings are retail; this pins the pair.
#[test]
fn ambient_ops_respond_to_cadence_the_way_retail_does() {
    for cadence in [1u8, 2, 4] {
        // 0x04: 8 stepping ticks regardless of the scalar.
        let mut w = World::new();
        w.frame_step = cadence;
        with_ambient(&mut w, 1, vec![0x04, 0x02, 0x08], 0x000);
        for _ in 0..8 {
            w.tick_field_npc_ambient();
        }
        assert_eq!(
            w.field_npc_ambient[&1].vm.pc, 0,
            "0x04 is still mid-leg after 8 ticks at cadence {cadence}"
        );
        w.tick_field_npc_ambient();
        assert_eq!(
            w.field_npc_ambient[&1].vm.pc, 3,
            "0x04 retires on its 9th tick at cadence {cadence}"
        );

        // 0x0D: a 32-vsync duration retires in 32/cadence stepping ticks
        // plus the terminal one - the duration in VSYNCS is invariant.
        let mut w = World::new();
        w.frame_step = cadence;
        with_ambient(&mut w, 1, vec![0x0D, 0x02, 0x20, 0x00], 0x000);
        let mut ticks = 0;
        for _ in 0..128 {
            w.tick_field_npc_ambient();
            ticks += 1;
            if usize::from(w.field_npc_ambient[&1].vm.pc) >= 4 {
                break;
            }
        }
        assert_eq!(
            ticks,
            32 / usize::from(cadence) + 1,
            "0x0D at cadence {cadence} stays in lockstep with its ramp"
        );
        let expected = (0x400u16.wrapping_add(0x800)) & 0x0FFF;
        assert_eq!(
            w.field_npc_headings.get(&1).copied(),
            Some(expected as i16),
            "0x0D lands on its compass point at cadence {cadence}"
        );
    }
}

/// Retail's interpreter preamble re-selects the live variant every tick
/// against `DAT_80085758`, so a story flag flipping mid-scene swaps the
/// stream - and the swap reseeds the cursor rather than resuming the old
/// variant's offset into new bytecode.
#[test]
fn variant_selection_follows_the_live_system_flag_bank() {
    let mut w = World::new();
    w.field_npc_ambient.insert(
        3,
        FieldNpcAmbient {
            variants: vec![
                // Flag-gated variant (system flag 0x10) turns to lut 4.
                (0x0010, vec![0x04, 0x04, 0x08]),
                // Default variant just waits.
                (legaia_asset::man_motion::SELECTOR_DEFAULT, vec![0x05, 0x7F]),
            ],
            live: None,
            vm: vm::ambient_motion::AmbientMotion::new(3, 0x000),
        },
    );
    // Flag clear: the default variant runs, so nothing turns.
    w.tick_field_npc_ambient();
    assert_eq!(w.field_npc_ambient[&3].live, Some(1), "default variant");
    assert!(!w.field_npc_headings.contains_key(&3));

    // Flag set: the gated variant takes over from its own first op.
    w.system_flag_set(0x10);
    w.tick_field_npc_ambient();
    assert_eq!(w.field_npc_ambient[&3].live, Some(0), "gated variant wins");
    assert_eq!(
        w.field_npc_ambient[&3].vm.pc, 0,
        "a swap reseeds the cursor at the new variant's first op"
    );
    assert!(
        w.field_npc_headings.contains_key(&3),
        "the gated variant's ramp turns the NPC"
    );
}

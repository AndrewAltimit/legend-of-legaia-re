//! Who writes the clip-end latch a conversation's cross-context spin waits on.
//!
//! The recurring field-VM idiom `A2 <t> <clip>` / `AC <t> 08` / `AD <t> 08` is
//! "play it, clear the end latch, spin until it re-latches"
//! ([`docs/subsystems/script-vm.md`]). Bit `8` of the clip-control word
//! `actor+0x62` is that latch, and what sets it is the per-frame anim tick
//! `FUN_800204F8` when a clip cursor reaches an end - ported at
//! [`legaia_engine_core::field_env::PropAnim::tick`]. A `0x80`-prefix target
//! makes the word belong to *another* actor than the record being dispatched
//! (`FUN_8003C83C`: `0xF8` resolves to the live player), so the runner's job is
//! to bind that actor's word around the op and then wait - not to write the
//! bit.
//!
//! No test asserted that ownership before, which is exactly what let a
//! stand-in write inside the runner reproduce the observable beat while
//! modelling the wrong data path. These tests pin it from both sides: the
//! latch appears on the poked actor's cursor and never on the record's own
//! flag word, and a cursor that cannot reach its end never lets the spin
//! through no matter how long the runner is driven.
//!
//! Disc-free: the records here are hand-written field-VM bytecode.

use legaia_engine_core::field_env::{ANIM_END, ANIM_HOLD, ANIM_SPAWN_RATE, PLAYER_ANCHOR_TARGET};
use legaia_engine_core::world::World;

/// The story flag the beat *behind* the spin sets - the observable that says
/// the spin fell through.
const BEAT_FLAG: u16 = 7;

/// The retock innkeeper's gesture shape, targeted at `target`: un-hold the
/// clip, poke it, clear the end latch, spin on it, and only then run the beat
/// the conversation continues with.
fn gesture_record(target: u8, clip: u8) -> Vec<u8> {
    let mut r = Vec::new();
    r.extend_from_slice(&[0xAC, target, 0x01]); // clear HOLD on the poked +0x62
    r.extend_from_slice(&[0xA2, target, clip]); // ExecMove: bind the clip
    r.extend_from_slice(&[0xAC, target, 0x08]); // clear the end latch
    r.extend_from_slice(&[0xAD, target, 0x08]); // spin until the tick re-latches
    r.extend_from_slice(&[0x50, BEAT_FLAG as u8]); // the beat behind the spin
    r.push(0x21); // park
    r
}

/// The record's own flag word, as an observer between frames sees it.
fn record_flags(w: &World) -> u16 {
    w.inline_dialogue.as_ref().map_or(0, |d| d.ctx.local_flags)
}

fn player_cursor(w: &World) -> i16 {
    w.field_prop_bank
        .actor_clip(PLAYER_ANCHOR_TARGET)
        .expect("the ExecMove bound the player's clip cursor")
        .cursor
}

/// The spin falls through because the **clip cursor** latched, and the record's
/// own flag word never carries the latch at all.
#[test]
fn the_clip_cursor_latches_and_the_runner_only_waits() {
    let mut w = World::new();
    w.start_inline_dialogue(gesture_record(PLAYER_ANCHOR_TARGET, 4));

    // Frame 1: the slice runs the un-hold, the poke and the latch clear, then
    // parks on the spin.
    w.step_inline_dialogue(false, false, false);
    assert!(
        !w.system_flag_test(BEAT_FLAG),
        "the beat behind the spin must not run on the frame the spin is reached"
    );
    let clip = w
        .field_prop_bank
        .actor_clip(PLAYER_ANCHOR_TARGET)
        .expect("`A2 F8 <clip>` bound a cursor for the player anchor");
    assert_eq!(clip.anim_id, 4, "the poke bound the clip it names");
    assert_eq!(clip.cursor, 0, "the binder zeroes +0x68 on a clip change");
    assert_eq!(
        clip.flags & ANIM_HOLD,
        0,
        "`AC F8 01` cleared HOLD on the PLAYER's +0x62, not on the record's"
    );
    assert!(!clip.at_end(), "`AC F8 08` cleared the end latch");

    // Drive it out. Every frame: the record's own word must stay free of the
    // latch bit - a runner that fabricated it would show up right here.
    let mut frames = 1;
    while !w.system_flag_test(BEAT_FLAG) && frames < 400 {
        assert_eq!(
            record_flags(&w) & ANIM_END,
            0,
            "frame {frames}: the runner wrote the latch into the record's own \
             flag word - that is the stand-in this test exists to forbid"
        );
        w.step_inline_dialogue(false, false, false);
        frames += 1;
    }
    assert!(
        w.system_flag_test(BEAT_FLAG),
        "the spin never fell through - the clip's end latch never landed"
    );
    assert!(
        w.field_prop_bank
            .actor_clip(PLAYER_ANCHOR_TARGET)
            .is_some_and(|a| a.at_end()),
        "what let the spin through is the latch on the player's clip cursor"
    );
    // And it waited for the clip rather than resolving on the spot: the cursor
    // had to walk the clip's whole span at the actor rate.
    assert!(
        frames > 2,
        "the spin resolved in {frames} frames - that is not a clip playing out"
    );
}

/// A cursor that cannot reach an end never lets the spin through. This is the
/// ownership proof: if anything but [`PropAnim::tick`] could set the latch, the
/// conversation would advance here.
#[test]
fn a_stalled_clip_never_lets_the_spin_through() {
    let mut w = World::new();
    w.start_inline_dialogue(gesture_record(PLAYER_ANCHOR_TARGET, 4));
    w.step_inline_dialogue(false, false, false);

    for frame in 0..200 {
        // Stall the cursor: rate 0 means the tick advances nothing, so no end
        // is ever reached. (Held every frame because the record's own ops are
        // free to write the control word.)
        w.field_prop_bank
            .actor_clip_mut(PLAYER_ANCHOR_TARGET)
            .expect("cursor bound")
            .rate = 0;
        w.step_inline_dialogue(false, false, false);
        assert!(
            !w.system_flag_test(BEAT_FLAG),
            "frame {frame}: the spin fell through with the clip stalled - \
             something other than the clip tick wrote the end latch"
        );
        assert_eq!(
            record_flags(&w) & ANIM_END,
            0,
            "frame {frame}: the latch bit appeared on the record's own word"
        );
    }
    assert!(
        w.inline_dialogue.as_ref().is_some_and(|d| !d.is_done()),
        "the spin is still parked (the timeout net is far longer than this)"
    );
}

/// The cursor advances exactly one step per frame whichever of the world's two
/// drivers reaches it - the field frame's prop-layer tick, or a host that runs
/// only the conversation. Two advances in a frame would latch the end early;
/// none would leave the spin waiting on a clip that never moves.
#[test]
fn the_cursor_advances_once_per_frame_under_either_driver() {
    let mut w = World::new();
    w.start_inline_dialogue(gesture_record(PLAYER_ANCHOR_TARGET, 4));
    w.step_inline_dialogue(false, false, false);
    let c0 = player_cursor(&w);

    // A full field frame: the prop layer ticks the clips, then the runner runs
    // its slice.
    w.tick_prop_interactions();
    w.step_inline_dialogue(false, false, false);
    let c1 = player_cursor(&w);
    assert_eq!(
        c1 - c0,
        ANIM_SPAWN_RATE,
        "a field frame advanced the cursor twice (once per driver)"
    );

    // A conversation-only frame: no prop-layer tick, so the runner's own tick
    // is what moves it - by the same one step.
    w.step_inline_dialogue(false, false, false);
    assert_eq!(
        player_cursor(&w) - c1,
        ANIM_SPAWN_RATE,
        "a conversation-only frame left the cursor standing still"
    );
}

/// A poke at a **third** actor (neither the dispatching record nor the player)
/// lands on that actor's own cursor, and its spin waits on that actor's latch.
/// The residual this does not cover is rendering: an NPC's on-screen clip is
/// the host's, so the engine cursor is a rate-matched twin of it, not the same
/// object.
#[test]
fn a_third_actor_poke_binds_that_actor_s_own_cursor() {
    const OTHER: u8 = 0x05;
    let mut w = World::new();
    w.start_inline_dialogue(gesture_record(OTHER, 9));

    w.step_inline_dialogue(false, false, false);
    let clip = w
        .field_prop_bank
        .actor_clip(OTHER)
        .expect("`A2 05 <clip>` bound a cursor for target 5");
    assert_eq!(clip.anim_id, 9);
    assert!(!clip.at_end(), "its own `AC 05 08` cleared its own latch");
    assert!(
        w.field_prop_bank.actor_clip(PLAYER_ANCHOR_TARGET).is_none(),
        "a poke at another actor must not touch the player's cursor"
    );

    let mut frames = 1;
    while !w.system_flag_test(BEAT_FLAG) && frames < 400 {
        assert_eq!(record_flags(&w) & ANIM_END, 0);
        w.step_inline_dialogue(false, false, false);
        frames += 1;
    }
    assert!(
        w.system_flag_test(BEAT_FLAG),
        "the third-actor spin never fell through"
    );
    assert!(
        w.field_prop_bank
            .actor_clip(OTHER)
            .is_some_and(|a| a.at_end()),
        "the latch landed on the poked actor's cursor"
    );
}

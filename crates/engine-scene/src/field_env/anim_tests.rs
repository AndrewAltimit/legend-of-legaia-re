#[test]
fn a_stream_drives_only_its_records_first_placement() {
    let records = [Some(5), None, Some(8), Some(5), Some(8), Some(9)];
    assert_eq!(
        stream_bound_draws(&records),
        vec![true, false, true, false, false, true]
    );
}

use super::*;

/// A 30-frame clip - the length of Rim Elm's door / cupboard swing.
fn door(flags: u16, rate: i16) -> PropAnim {
    PropAnim {
        anim_id: 1,
        frames: 30,
        scaled_step: false,
        step_div: 2,
        cursor: 0,
        flags,
        rate,
    }
}

/// The state `0x4C 0x35` leaves a prop in - the closed door. Retail's live
/// Rim Elm actors read `+0x62 = 0x001F`, cursor `0`: the restart has been
/// consumed, hold + clamp are set, and the cursor never moves.
#[test]
fn reset_hold_freezes_the_clip_at_frame_zero() {
    let mut a = door(ANIM_SPAWN_FLAGS, 16);
    AnimCmd::ResetHold.apply(&mut a);
    a.tick(); // consumes the restart request
    assert_eq!(a.flags, 0x001F, "the live actors' resting +0x62");
    for _ in 0..120 {
        a.tick();
        assert_eq!(a.frame(), 0, "a held clip must not advance");
    }
}

/// Clearing hold with clamp set plays the clip forward once and stops on
/// the last frame, latching the end bit. Retail's open door reads
/// `+0x62 = 0x011D`, cursor `479` = `30 * 16 - 1`.
#[test]
fn clearing_hold_plays_forward_and_clamps_open() {
    let mut a = door(ANIM_SPAWN_FLAGS, 16);
    AnimCmd::ResetHold.apply(&mut a);
    a.tick();
    // The house-door touch pass: `2c 07`, `2c 01`, `2b 03`, `2c 08`.
    for c in [
        AnimCmd::ClearBit(7),
        AnimCmd::ClearBit(1),
        AnimCmd::SetBit(3),
        AnimCmd::ClearBit(8),
    ] {
        c.apply(&mut a);
    }
    for _ in 0..60 {
        a.tick();
    }
    assert_eq!(a.cursor, 479, "clamped at the last frame (30 * 16 - 1)");
    assert_eq!(a.frame(), 29);
    assert_eq!(a.flags, 0x011D, "the live open door's +0x62");
    assert!(a.at_end());
}

/// The cupboard's second segment (`2b 07`, `2c 01`, `2b 03`) plays the
/// clip backwards to frame 0 and clamps there - retail's `+0x62 = 0x019D`.
#[test]
fn setting_reverse_plays_the_clip_shut() {
    let mut a = door(0x011D, 16);
    a.cursor = 479;
    for c in [AnimCmd::SetBit(7), AnimCmd::ClearBit(1), AnimCmd::SetBit(3)] {
        c.apply(&mut a);
    }
    for _ in 0..60 {
        a.tick();
    }
    assert_eq!(a.cursor, 0);
    assert_eq!(a.flags, 0x019D, "the live closing door's +0x62");
}

/// The template flags an actor is born with loop, so an NPC idle clip runs
/// forever - which is why only the props' own `0x4C 0x35` freezes them.
#[test]
fn the_spawn_flags_loop() {
    let mut a = door(ANIM_SPAWN_FLAGS, 8);
    let mut wrapped = false;
    let mut last = 0;
    for _ in 0..200 {
        a.tick();
        if a.frame() < last {
            wrapped = true;
        }
        last = a.frame();
    }
    assert!(wrapped, "no clamp bit -> the cursor wraps");
}

/// The house-door script shape, byte for byte from a retail `town01`
/// partition-0 record (`主人公の家`, anim 1), decodes to: a spawn pass that
/// sets the rate and resets+holds, and a one-segment touch pass that plays
/// the clip forward and waits for the end.
#[test]
fn a_house_door_record_decodes_to_open_and_wait() {
    // `[u8 n=0][u8 anim=1]` header, then the script.
    let rec: &[u8] = &[
        0x00, 0x01, // header: no name, anim 1
        0x25, // prologue marker
        0x4C, 0x41, 0x20, 0x00, 0x00, 0x00, // rate <- 0x20
        0x4C, 0x35, // reset + hold
        0x21, // park
        0x36, 0x00, 0x80, 0x2E, 0x00, // SFX (the door creak)
        0x2C, 0x07, // clear reverse
        0x2C, 0x01, // clear hold
        0x2B, 0x03, // set clamp
        0x31, 0x00, // (actor local flag, not an anim bit)
        0x2C, 0x08, // clear the end latch
        0x2D, 0x08, // wait for the end latch
        0x21, // park
        0x26, 0xED, 0xFF, // jump back
    ];
    let p = decode_prop_program(rec, 2);
    assert_eq!(p.spawn, vec![AnimCmd::Rate(0x20), AnimCmd::ResetHold]);
    assert_eq!(p.touch.len(), 1);
    assert!(p.touch[0].wait_for_end);
    assert_eq!(
        p.touch[0].cmds,
        vec![
            AnimCmd::ClearBit(7),
            AnimCmd::ClearBit(1),
            AnimCmd::SetBit(3),
            AnimCmd::ClearBit(8),
        ]
    );
    assert!(p.animates());

    // The spawn pass leaves it closed; the touch pass swings it open.
    let mut a = PropAnim::spawned(1, 30, false, 2);
    for c in &p.spawn {
        c.apply(&mut a);
    }
    a.tick();
    assert_eq!(a.frame(), 0);
    assert_eq!(a.rate, 16, "0x20 >> 1");
    for c in &p.touch[0].cmds {
        c.apply(&mut a);
    }
    for _ in 0..40 {
        a.tick();
    }
    assert_eq!(a.frame(), 29, "the door ends fully open");
}

/// A record with no animation commands past its park (the locked drawer,
/// the clock) decodes to a program that never moves the clip.
#[test]
fn a_static_prop_record_decodes_to_no_animation() {
    let rec: &[u8] = &[
        0x00, 0x03, // header: no name, anim 3
        0x25, 0x31, 0x00, 0x21, 0x21, 0x26, 0xFE, 0xFF,
    ];
    let p = decode_prop_program(rec, 2);
    assert!(p.spawn.is_empty());
    assert!(!p.animates(), "nothing in the touch pass touches +0x62");
}

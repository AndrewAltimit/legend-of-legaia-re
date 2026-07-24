//! The scripted countdown timer, end to end through the world tick.
//!
//! Field VM `0x4C 0xD3` (`SCHEDULE_TIMED_FLAGS`) arms it; retail's
//! `FUN_801D2EBC` drains it once per display frame, raising a below-threshold
//! system flag on the way down and an expiry flag at zero, and decomposes the
//! remaining count into the MM:SS.ff readout in the same pass.
//!
//! This exercises the join the two ports make through `World`: the installer
//! hook (`FieldHost::op4c_n_d_sub3_party_setup` ->
//! `World::schedule_timed_flags`) and the per-frame drain
//! (`World::tick_escape_timer`, driving
//! `legaia_engine_vm::escape_timer::EscapeTimer`).
//!
//! Bytecode is hand-authored and the world is synthetic - no Sony bytes, no
//! disc gate.

use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::escape_timer::TimerInk;

/// The retail-frame sub-clock only fires on ~60 % of sim ticks, so a test that
/// wants `n` drained frames has to run more ticks than that. Run until the
/// world's own retail-frame counter has advanced `frames`.
fn advance_retail_frames(world: &mut World, frames: u64) {
    let target = world.field_frames + frames;
    let mut guard = 0;
    while world.field_frames < target {
        let _ = world.tick();
        guard += 1;
        assert!(guard < 100_000, "retail-frame clock did not advance");
    }
}

fn armed_world(duration: u32, threshold: u32, expiry: u16, below: u16) -> World {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    let ab = (u32::from(expiry) << 16) | u32::from(below);
    world.schedule_timed_flags(ab, duration, threshold);
    world
}

#[test]
fn the_installer_arms_the_counter_and_the_tick_drains_it() {
    let mut world = armed_world(2400, 910, 0x04C7, 0x0123);
    assert!(world.escape_timer.armed);
    assert_eq!(world.escape_timer.remaining, 2400);
    assert_eq!(world.escape_timer.warn_threshold, 910);

    advance_retail_frames(&mut world, 100);
    assert_eq!(
        world.escape_timer.remaining, 2300,
        "one count per retail frame"
    );
    assert!(world.escape_timer.armed);
    assert!(
        !world.system_flag_test(0x0123),
        "the below-threshold flag stays low above the threshold"
    );
    assert!(!world.system_flag_test(0x04C7));
}

#[test]
fn crossing_the_threshold_raises_the_below_flag_only() {
    let mut world = armed_world(120, 60, 0x04C7, 0x0123);
    advance_retail_frames(&mut world, 59);
    assert!(!world.system_flag_test(0x0123), "still above the threshold");

    advance_retail_frames(&mut world, 2);
    assert!(
        world.system_flag_test(0x0123),
        "below-threshold flag raised"
    );
    assert!(!world.system_flag_test(0x04C7), "expiry has not fired");
    assert!(world.escape_timer.armed);
}

#[test]
fn running_out_raises_the_expiry_flag_and_disarms() {
    let mut world = armed_world(30, 20, 0x04C7, 0x0123);
    advance_retail_frames(&mut world, 30);
    assert!(world.system_flag_test(0x04C7), "expiry flag raised");
    assert!(!world.escape_timer.armed, "the timer disarms at zero");
    assert!(
        world.escape_timer_hud.is_none() || world.escape_timer.remaining <= 0,
        "a disarmed timer stops producing a readout"
    );

    // A disarmed timer does not keep counting.
    let settled = world.escape_timer.remaining;
    advance_retail_frames(&mut world, 10);
    assert_eq!(world.escape_timer.remaining, settled);
}

#[test]
fn the_tick_publishes_the_hud_readout_and_its_ink() {
    // 2400 frames = 0:40.00, and 2400 > 0x707 so the readout is the safe ink.
    let mut world = armed_world(2401, 910, 0x04C7, 0x0123);
    advance_retail_frames(&mut world, 1);
    assert_eq!(
        world.escape_timer_hud,
        Some((0, 40, 0, TimerInk::Safe)),
        "the drain publishes the decomposition retail computes in the same pass"
    );

    // Under 0x707 the ink switches to the warning colour.
    let mut world = armed_world(0x708, 0, 0x04C7, 0x0123);
    advance_retail_frames(&mut world, 1);
    let (_, _, _, ink) = world.escape_timer_hud.expect("readout published");
    assert_eq!(ink, TimerInk::Warning);
}

#[test]
fn a_zero_duration_leaves_the_timer_disarmed() {
    let mut world = armed_world(0, 100, 0x04C7, 0x0123);
    assert!(!world.escape_timer.armed);
    advance_retail_frames(&mut world, 10);
    assert!(world.escape_timer_hud.is_none());
    assert!(
        !world.system_flag_test(0x04C7),
        "nothing fires while disarmed"
    );
}

#[test]
fn a_modal_dialog_freezes_the_countdown() {
    let mut world = armed_world(600, 100, 0x04C7, 0x0123);
    advance_retail_frames(&mut world, 5);
    let before = world.escape_timer.remaining;
    assert!(before < 600);

    world.current_dialog = Some(legaia_engine_core::world::DialogRequest {
        text_id: 0,
        inline: Vec::new(),
        world_x: 0,
        world_z: 0,
        depth_id: 0,
    });
    advance_retail_frames(&mut world, 20);
    assert_eq!(
        world.escape_timer.remaining, before,
        "a busy frame leaves the counter alone"
    );

    world.current_dialog = None;
    advance_retail_frames(&mut world, 3);
    assert!(world.escape_timer.remaining < before, "and resumes after");
}

#[test]
fn rearming_replaces_the_whole_triple() {
    let mut world = armed_world(600, 100, 0x04C7, 0x0123);
    advance_retail_frames(&mut world, 10);
    world.schedule_timed_flags(0x0055_0044, 90, 30);
    assert_eq!(world.escape_timer.remaining, 90);
    assert_eq!(world.escape_timer.warn_threshold, 30);
    assert_eq!(world.escape_timer_flag_word, 0x0055_0044);
    advance_retail_frames(&mut world, 90);
    assert!(world.system_flag_test(0x0055), "the new expiry flag fires");
    assert!(
        !world.system_flag_test(0x04C7),
        "the replaced flag word does not"
    );
}

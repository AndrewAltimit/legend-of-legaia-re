//! A cast film's gate seat of the recording's frame phase
//! (`World::seat_battle_frame_phase`): the next frame opens `left` ticks on,
//! keeping the current frame's id and step.

use super::*;

#[test]
fn the_phase_seat_opens_the_next_frame_early_and_keeps_the_id() {
    let mut w = World::new();
    w.enter_battle(3, 1);
    w.toggles.battle_sm_frame_cadence = true;
    w.clock.display_frames = 300;
    w.battle.frame_clock.set_step(300, 3);
    let now = w.clock.display_frames;
    let id = w.battle.frame_clock.frame_of(now);
    assert_eq!(
        w.battle.frame_clock.frame_of(now + 2),
        id,
        "fresh frame holds 3"
    );
    assert!(w.seat_battle_frame_phase(2));
    let c = w.battle.frame_clock;
    assert_eq!(c.frame_of(now), id);
    assert_eq!(c.frame_of(now + 1), id);
    assert_eq!(c.frame_of(now + 2), id + 1);
    assert_eq!(c.frame_of(now + 5), id + 2);
    assert_eq!(c.step, 3);
    // A whole step, or none, seats nothing.
    assert!(!w.seat_battle_frame_phase(3));
    assert!(!w.seat_battle_frame_phase(0));
}

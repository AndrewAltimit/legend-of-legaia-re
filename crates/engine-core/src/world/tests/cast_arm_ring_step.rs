//! A module arm's frame step against the cost ring: a cheaper arm after a
//! dear one runs the ring's step until the dear entries leave the sixteen-frame
//! history (`FUN_80016B6C`'s maximum, `0x800170D0..0x800170F4`).
//!
//! Kiss of Death (`0x6E`) on `battle_map01_lippian_kiss_of_death_mid_cast`'s
//! recording: arm 4's frames read the clamp, and arm 5 - the victim alone, `2`
//! by its table - runs step `3` for `45` vsyncs (fifteen passes) before `2`.

use super::*;
use legaia_engine_battle_vm::battle_action::ActionState;

fn kiss_of_death_band() -> World {
    let mut w = World::new();
    w.enter_battle(3, 1);
    w.toggles.battle_sm_frame_cadence = true;
    w.casting.capture_spell = Some(0x6E);
    w.battle_ctx.action_state = ActionState::MagicCapturePhase2.as_byte();
    w
}

fn passes(w: &mut World, phase: u8, n: usize) -> Vec<u8> {
    w.casting.module_phase = phase;
    (0..n)
        .map(|_| {
            w.apply_cast_arm_frame_step();
            w.battle.frame_clock.step
        })
        .collect()
}

#[test]
fn a_cheap_arm_after_a_dear_one_runs_the_rings_step_until_it_empties() {
    let mut w = kiss_of_death_band();
    assert!(
        passes(&mut w, 4, 20).iter().all(|&s| s == 4),
        "arm 4 reads the clamp"
    );
    let tail = passes(&mut w, 5, 24);
    // The first arm-5 pass runs on the step arm 4's last clamped frame left.
    assert_eq!(tail[0], 4);
    assert!(tail[1..16].iter().all(|&s| s == 3), "{tail:?}");
    assert!(tail[16..].iter().all(|&s| s == 2), "{tail:?}");
}

#[test]
fn a_dearer_arm_raises_the_step_on_its_first_pass() {
    let mut w = kiss_of_death_band();
    passes(&mut w, 5, 20);
    assert_eq!(w.battle.frame_clock.step, 2);
    assert_eq!(passes(&mut w, 4, 1), vec![4]);
}

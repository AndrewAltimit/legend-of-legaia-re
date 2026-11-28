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
            // The pass opens on the step the last one left, then pushes its
            // own frame's cost.
            w.take_pending_frame_step();
            let step = w.battle.frame_clock.step;
            w.apply_cast_arm_frame_step();
            // A second claim of the same frame (the frame tail's effect
            // scene graphs) leaves the step for the next pass.
            w.take_pending_frame_step();
            assert_eq!(w.battle.frame_clock.step, step);
            w.clock.display_frames += w.battle.frame_clock.span();
            step
        })
        .collect()
}

#[test]
fn a_cheap_arm_after_a_dear_one_runs_the_rings_step_until_it_empties() {
    let mut w = kiss_of_death_band();
    let head = passes(&mut w, 4, 20);
    assert!(head[1..].iter().all(|&s| s == 4), "arm 4 reads the clamp");
    let tail = passes(&mut w, 5, 24);
    // The first arm-5 pass runs on the step arm 4's last clamped frame left.
    assert_eq!(tail[0], 4);
    assert!(tail[1..16].iter().all(|&s| s == 3), "{tail:?}");
    assert!(tail[16..].iter().all(|&s| s == 2), "{tail:?}");
}

/// A dearer arm's first pass runs on the step the frames before it left and
/// raises the step from its second: retail's frame-end pass stores the step
/// the frame's cost reads for the next frame (`0x80017124..0x800171D8`). Nova's
/// arm 3 on `party_basic_attack_vs_gobu_gobu` runs `1`, then `4`.
#[test]
fn a_dearer_arm_raises_the_step_from_its_second_pass() {
    let mut w = kiss_of_death_band();
    passes(&mut w, 5, 20);
    assert_eq!(passes(&mut w, 4, 3), vec![2, 4, 4]);
}

/// A replay's seeded step speaks for its own state alone: once the SM has
/// left it, the band's arm tables time the cast. `theeder_summon_mid_cast`'s
/// seed holds step `3` on `0x33`, and it held every band arm on
/// the ordinary step while the seed sat in the clock.
#[test]
fn a_seed_for_another_state_leaves_the_arm_tables_their_step() {
    let mut w = kiss_of_death_band();
    w.seed_battle_frame_step(3, ActionState::SummonFadeIn.as_byte());
    w.apply_battle_frame_step_seed();
    let head = passes(&mut w, 4, 6);
    assert!(head[1..].iter().all(|&s| s == 4), "{head:?}");
}

/// An arm whose table `4` is the capture module's own line
/// (`CAPTURE_ARM_LINE_CLAMPS`) holds it while the line's polls load the frame
/// and prices its own work once they stop: Call Wave's arm 2 on
/// `garmel_zeto_pre_call_wave`'s recording reads the clamp to vsync `530`,
/// three after the line's `Pause`, and runs `3` behind the clamped entries
/// from there. An arm whose `4` is its work keeps the table past the line.
#[test]
fn a_line_clamped_arm_prices_its_work_once_the_line_stops_loading() {
    let mut w = kiss_of_death_band();
    w.casting.capture_spell = Some(0x55);
    w.audio.battle_xa_load_frames = 400;
    w.audio.battle_xa_frame_load = true;
    w.audio.battle_xa_poll_load = false;
    let under = passes(&mut w, 2, 12);
    assert!(under[1..].iter().all(|&s| s == 4), "{under:?}");
    w.audio.battle_xa_load_frames = 0;
    let past = passes(&mut w, 2, 12);
    // The first pass past the line still runs the step the last loaded one left.
    assert_eq!(past[0], 4);
    assert!(past[1..].iter().all(|&s| s == 3), "{past:?}");

    // Thunder Storm's arm 6 runs `4` on its work past its line.
    let mut w = kiss_of_death_band();
    w.casting.capture_spell = Some(0x77);
    let held = passes(&mut w, 6, 12);
    assert!(held[1..].iter().all(|&s| s == 4), "{held:?}");
}

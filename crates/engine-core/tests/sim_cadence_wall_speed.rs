//! The simulation clock's denomination, measured rather than asserted.
//!
//! **One `World::tick` is one retail display frame (vsync).** Both hosts drive
//! it at exactly 60 Hz wall - the native window's fixed-timestep accumulator
//! (`legaia_engine_render::EngineWindow::drain_ticks`, `TICK_DT = 1.0/60.0`)
//! and the browser play page (`site/js/play-app.js`, `TICK_DT = 1000/60`) -
//! so a tick count is a retail frame count and `ticks / 60` is seconds.
//!
//! # The retail number these pin against
//!
//! Retail's field frame pump `FUN_801D1344` calls the locomotion controller
//! `FUN_801D01B0` **once per game tick** (`jal 0x801D01B0` at `0x801D16F4`,
//! no cadence gate), and a game tick spans `DAT_1F800393` vsyncs - the byte
//! `FUN_80016B6C` resolves per frame at `0x80017044..0x800171D8` and then
//! `VSync(n)`-waits on. The controller's travel budget for the call is
//!
//! ```text
//! speed = ((base_step * player[+0x72]) >> 12) * DAT_1F800393
//! ```
//!
//! (`0x801D0564..0x801D05C4`: `mult s4,v0`; `sra s4,t1,0xc`;
//! `lbu v1,0x7f(a1)` with `a1 = 0x1F800314`; `mult s4,v1`), and the step loop
//! then consumes it 2 units at a time. The `DAT_1F800393` factor is what makes
//! the wall speed **cadence-invariant**: at the field floor of 2 retail runs
//! the controller 30x a second for `2 * base_step` each, at cadence 1 it runs
//! it 60x for `base_step` each, and both land on
//!
//! | base step | selector (`0x801D0334..0x801D03E0`) | units/vsync | **units/second** |
//! |---|---|---|---|
//! | `5` | forced slow (`_DAT_8007B6A8`) | 5 | 300 |
//! | `8` | plain walk | 8 | **480** |
//! | `0xC` | run | 12 | **720** |
//! | `0x18` | debug turbo | 24 | 1440 |
//!
//! with `player[+0x72] = 0x1000` (1.0, seeded by `FUN_8003AEB0`) cancelling
//! the `>> 12`. One collision tile is 128 units, so a retail walk crosses
//! 3.75 tiles a second.
//!
//! The engine takes the fine-grained half of the identity - the controller
//! once per vsync with the scalar at 1 - so the same 480 units/second come out
//! of 60 calls of 8 rather than 30 of 16.
//!
//! # Why this file exists
//!
//! The sub-clock used to be denominated against a `SIM_HZ = 100` that no host
//! ever met, which withheld 2 of every 5 retail frames from the *gated*
//! consumers (narration crawl, cutscene timeline, effect pool, escape timer,
//! NPC motion, CLUT / ambient banks, timed sound release) while the *ungated*
//! ones ran at the correct 60. Nothing caught it because the only wall-time
//! oracle in the tree divided ticks by 100 *and* measured a consumer emitting
//! 0.6 frames per tick, so the two errors cancelled in seconds and left the
//! unit wrong. These tests measure the rate directly, in units per second, so
//! a future re-denomination cannot hide inside a compensating constant.
//!
//! Disc-free: a synthetic `World` with an open collision grid. No gating.

use legaia_engine_core::input;
use legaia_engine_core::world::{SceneMode, World};

/// Retail display frames per second - the unit `World::tick` is denominated in.
const RETAIL_FPS: u32 = 60;

/// Retail walk speed in world units per second of wall time (`base_step` 8 per
/// vsync). See the module doc for the derivation.
const RETAIL_WALK_UNITS_PER_SEC: i32 = 8 * RETAIL_FPS as i32;
/// Retail run speed (`base_step` 0xC per vsync).
const RETAIL_RUN_UNITS_PER_SEC: i32 = 12 * RETAIL_FPS as i32;

/// A field world with the player at `slot 0`, an open grid, and no height
/// controller (so `world_y` cannot perturb the X/Z measurement).
fn walking_world() -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.install_field_player(0);
    w.actors[0].move_state.world_x = 4096;
    w.actors[0].move_state.world_z = 4096;
    w
}

/// Hold `pad` for `ticks` sim ticks and return the `(dx, dz)` travelled.
fn hold(w: &mut World, pad: u16, ticks: u32) -> (i32, i32) {
    let start = (
        w.actors[0].move_state.world_x as i32,
        w.actors[0].move_state.world_z as i32,
    );
    for _ in 0..ticks {
        w.set_pad(pad);
        let _ = w.tick();
    }
    (
        w.actors[0].move_state.world_x as i32 - start.0,
        w.actors[0].move_state.world_z as i32 - start.1,
    )
}

/// The headline measurement: a second of held d-pad covers retail's walk
/// distance, and it is a **rate** - two seconds cover twice as far.
#[test]
fn the_player_walks_retails_480_world_units_per_second() {
    let mut w = walking_world();
    let (dx, dz) = hold(&mut w, input::PadButton::Up.mask(), RETAIL_FPS);
    assert_eq!(dx, 0, "screen-up is a pure +Z walk at the default camera");
    assert_eq!(
        dz,
        RETAIL_WALK_UNITS_PER_SEC,
        "one second of held walk must cover retail's 8 units/vsync x 60 = \
         {RETAIL_WALK_UNITS_PER_SEC} units. A sim clock denominated at 100 Hz \
         reads {} here; one gated at 0.6 reads {}.",
        RETAIL_WALK_UNITS_PER_SEC * 100 / 60,
        RETAIL_WALK_UNITS_PER_SEC * 6 / 10,
    );

    // It is a rate, not a one-second constant.
    let (_, dz2) = hold(&mut w, input::PadButton::Up.mask(), RETAIL_FPS * 2);
    assert_eq!(
        dz2,
        RETAIL_WALK_UNITS_PER_SEC * 2,
        "two seconds cover twice the ground"
    );
}

/// The run arm of the same selector, so the oracle pins the *selector* and not
/// one row of it. Square is the engine's run modifier (latched in
/// `World::set_pad`), and the Field Move option XOR leaves Walk as the default.
#[test]
fn the_player_runs_retails_720_world_units_per_second() {
    let mut w = walking_world();
    let pad = input::PadButton::Up.mask() | input::PadButton::Square.mask();
    let (dx, dz) = hold(&mut w, pad, RETAIL_FPS);
    assert_eq!(dx, 0);
    assert_eq!(
        dz, RETAIL_RUN_UNITS_PER_SEC,
        "one second of held run must cover retail's 0xC units/vsync x 60"
    );
    // Non-vacuity: the run arm has to be distinguishable from the walk arm,
    // or this test would pass against a locomotion that ignored the modifier.
    let mut walking = walking_world();
    let (_, walk_dz) = hold(&mut walking, input::PadButton::Up.mask(), RETAIL_FPS);
    assert!(
        dz > walk_dz,
        "the run modifier must actually change the selector ({dz} vs {walk_dz})"
    );
}

/// In world units a collision tile is `0x80`, so the retail walk crosses
/// 3.75 tiles a second. Stated separately because it is the figure a human
/// can check against a stopwatch and a map.
#[test]
fn a_walking_second_crosses_three_and_three_quarter_collision_tiles() {
    let mut w = walking_world();
    let (_, dz) = hold(&mut w, input::PadButton::Up.mask(), RETAIL_FPS * 4);
    // 4 s at 480 u/s = 1920 units = 15 tiles of 128.
    assert_eq!(dz, 1920);
    assert_eq!(dz / 0x80, 15, "15 collision tiles in 4 seconds");
}

/// The denomination itself: every sim tick is a retail display frame, so the
/// retail-frame counter and the sim-tick counter advance in lockstep and the
/// sub-clock step never withholds a frame.
///
/// This is the direct guard on the defect. Under the old `SIM_HZ = 100`
/// premise `field_frame_step` was `0` on 40 % of ticks and `field_frames`
/// reached 60 after 100 ticks.
#[test]
fn one_sim_tick_is_exactly_one_retail_display_frame() {
    let mut w = walking_world();
    let mut fired = 0u64;
    for _ in 0..300 {
        let _ = w.tick();
        assert_eq!(
            w.field_frame_step, 1,
            "every sim tick maps to a retail display frame"
        );
        fired += 1;
    }
    assert_eq!(w.field_frames, fired, "300 ticks = 300 retail frames");
    assert_eq!(w.frame, fired, "and the sim-tick counter agrees");
}

/// The two halves of the frame must share one denominator. A *gated* consumer
/// (anything behind `field_frame_step`) and an *ungated* one (the locomotion
/// controller) have to advance the same number of retail frames over the same
/// wall-clock span - that equality is exactly what was broken when the gated
/// side ran at 36 Hz and the ungated side at 60.
///
/// The escape timer stands in for the gated side: `World::tick_escape_timer`
/// drains it one retail frame per firing of the sub-clock.
#[test]
fn gated_and_ungated_consumers_share_one_denominator() {
    let mut w = walking_world();
    // Arm the scripted countdown (`0x4C 0xD3`) with a duration longer than the
    // run so it never disarms: flag word 0, 10 000 frames, no warn threshold.
    w.schedule_timed_flags(0, 10_000, 0);
    let start = w.escape_timer.remaining;

    let ticks = RETAIL_FPS * 3;
    let (_, dz) = hold(&mut w, input::PadButton::Up.mask(), ticks);

    let gated_frames = start - w.escape_timer.remaining;
    let ungated_frames = dz / 8; // locomotion committed `base_step` per frame

    assert_eq!(
        gated_frames, ticks as i32,
        "the gated side must advance one retail frame per sim tick"
    );
    assert_eq!(
        ungated_frames, ticks as i32,
        "the ungated side must advance one retail frame per sim tick"
    );
    assert_eq!(
        gated_frames, ungated_frames,
        "ONE DENOMINATOR: a gated consumer and an ungated one must cover the \
         same number of retail frames over the same wall-clock span. This is \
         the incoherence the 100 Hz sub-clock created - 36 Hz on one side, \
         60 Hz on the other."
    );
}

/// Cadence invariance, the property that lets the engine run the controller
/// once per vsync where retail runs it once per game tick: moving
/// `World::frame_step` (retail `DAT_1F800393`'s resolved value, the field
/// floor being 2) must not move the player's wall speed.
///
/// The engine's locomotion reads `move_ramp_ratio`, not `frame_step`, so this
/// asserts the two never get accidentally coupled - a coupling would multiply
/// the walk speed by the cadence instead of leaving it alone.
#[test]
fn the_walk_speed_is_invariant_under_the_retail_cadence() {
    for cadence in 1..=4u8 {
        let mut w = walking_world();
        w.frame_step = cadence;
        w.frame_step_floor = cadence;
        let (_, dz) = hold(&mut w, input::PadButton::Up.mask(), RETAIL_FPS);
        assert_eq!(
            dz, RETAIL_WALK_UNITS_PER_SEC,
            "cadence {cadence} must not move the player's wall speed - retail \
             scales the per-call budget by DAT_1F800393 precisely so it does not"
        );
    }
}

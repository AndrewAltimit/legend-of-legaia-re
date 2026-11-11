//! The camera vertical-offset channel `FUN_801DA390` eases, driven end to end
//! through the field VM's op `0x4C` outer-nibble-4 sub-9 - the opcode that is
//! the retail writer of both globals the easing reads.
//!
//! The pairing is what these tests pin: sub-9 posts `scene_ctrl[+0x4A]`, and
//! `World::tick` walks `_DAT_8007BCAC` toward
//! `scene_ctrl[+0x4A] - player[+0x16]` at the step rule
//! `legaia_engine_field::camera_ease::ease_step` encodes. Before the writer existed the
//! kernel had no target and no accumulator, so nothing exercised either half.

use legaia_engine_core::world::*;
use legaia_engine_mode::mode::SceneMode;

/// A field world with a live player actor standing on the `0` floor.
fn world_with_player() -> World {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.clock.display_frame_step = 1;
    world.player_actor_slot = Some(0);
    world.actors[0].active = true;
    world.actors[0].move_state.world_x = 2624;
    world.actors[0].move_state.world_y = 0;
    world.actors[0].move_state.world_z = 2624;
    world
}

/// `4C 49 <target> 00 00` - the default arm (`_DAT_1F800394` bits 24/25 clear).
fn sub9_default(target: i16) -> Vec<u8> {
    let t = target.to_le_bytes();
    vec![0x4C, 0x49, t[0], t[1], 0x00, 0x00]
}

/// The accumulator starts where the per-scene initialiser leaves it
/// (`FUN_801D6704` `0x801D67B8`: `li v0,0x3c` / `sw v0,-0x4354(v1)`).
#[test]
fn the_accumulator_starts_at_the_initialisers_seed() {
    assert_eq!(
        World::new().camera.offset_ease,
        legaia_engine_field::camera_ease::CAMERA_OFFSET_EASE_SEED
    );
}

/// Sub-9's default arm posts the scene offset, and the per-frame easing then
/// closes the gap - monotonically, and landing exactly on the target rather
/// than oscillating around it.
#[test]
fn op_4c_49_posts_the_offset_and_the_frame_easing_walks_to_it() {
    let mut world = world_with_player();
    world.load_field_script(sub9_default(0x200));
    let _ = world.tick();
    assert_eq!(
        world.camera.scene_offset, 0x200,
        "the sub-9 default arm is the writer of scene_ctrl[+0x4A]"
    );

    let start = world.camera.offset_ease;
    assert!(start < 0x200, "the seed is below the posted target");
    let mut prev = start;
    let mut arrived = None;
    // A standing player's offset creeps one unit a game pass.
    for frame in 0..1200 {
        let _ = world.tick();
        let cur = world.camera.offset_ease;
        assert!(
            cur >= prev,
            "frame {frame}: eased backwards {prev} -> {cur}"
        );
        assert!(cur <= 0x200, "frame {frame}: overshot to {cur}");
        prev = cur;
        if cur == 0x200 && arrived.is_none() {
            arrived = Some(frame);
        }
    }
    assert!(
        arrived.is_some(),
        "the easing must arrive; it stalled at {prev}"
    );
    assert_eq!(world.camera.offset_ease, 0x200, "and then hold");
}

/// The target is a *difference*: the player's `+0x16` footing is subtracted
/// from the posted offset, so standing on a lower floor raises the settled
/// accumulator by exactly that much. This is the half the old "camera yaw"
/// reading of this channel got wrong - a yaw would not track the floor.
#[test]
fn the_settled_value_is_the_posted_offset_minus_the_players_footing() {
    let mut settled = Vec::new();
    for footing in [0i16, -192, 64] {
        let mut world = world_with_player();
        world.actors[0].move_state.world_y = footing;
        world.load_field_script(sub9_default(0x100));
        for _ in 0..1200 {
            let _ = world.tick();
        }
        settled.push(world.camera.offset_ease);
    }
    assert_eq!(settled, vec![0x100, 0x100 + 192, 0x100 - 64]);
}

/// The step lands once a game pass, and its fast arm is live: with scratchpad
/// bit 17 raised (`2E 11`) a held direction pins it at twelve a pass, where a
/// standing player otherwise creeps one.
#[test]
fn a_held_direction_under_bit_17_pins_the_step_at_twelve_a_pass() {
    let run = |fast: bool, held: bool| {
        let mut world = world_with_player();
        world.load_field_script(sub9_default(0x200));
        let _ = world.tick();
        if fast {
            world.flags.story_flags |= legaia_engine_field::camera_ease::PAD_FAST_ARM;
        }
        // A wall on every side would be simpler; the player is held instead,
        // so the direction moves nothing.
        world.actors[0].move_state.flags |= 0x0008_0000;
        if held {
            world.set_pad(legaia_engine_system::input::PadButton::Up.mask());
        }
        let start = world.camera.offset_ease;
        let step = usize::from(world.clock.frame_step.max(1));
        for _ in 0..step * 10 {
            let _ = world.tick();
        }
        world.camera.offset_ease - start
    };
    assert_eq!(run(false, false), 10, "one unit a pass");
    assert_eq!(run(false, true), 10, "the pad alone is not the fast arm");
    assert_eq!(run(true, false), 10, "nor the flag alone");
    assert_eq!(run(true, true), 120, "both: twelve a pass");
}

/// Bit 24 of the scratchpad flag word is read by both routines, and that is
/// why sub-9's bit-24 arm posts the accumulator itself: the same bit is
/// `FUN_801DA390`'s input lock (`0x801DA398`), so the per-frame easing returns
/// before its first store while it is raised. The arm writes
/// `scene_ctrl[+0x4A] = target + player[+0x16]` and `_DAT_8007BCAC = target`
/// (`0x801E1498..0x801E15B0`), which is the same pair the easing would have
/// converged to.
#[test]
fn the_bit_24_arm_posts_both_globals_because_the_easing_is_locked_out() {
    let mut world = world_with_player();
    world.actors[0].move_state.world_y = -192;
    world.flags.story_flags = legaia_engine_field::camera_ease::PAD_INPUT_LOCKED;
    world.load_field_script(sub9_default(0x200));
    let _ = world.tick();
    assert_eq!(
        world.camera.scene_offset,
        0x200 - 192,
        "scene ctrl +0x4A takes target + player[+0x16]"
    );
    assert_eq!(
        world.camera.offset_ease, 0x200,
        "and the accumulator target"
    );
    for _ in 0..64 {
        let _ = world.tick();
    }
    assert_eq!(
        world.camera.offset_ease, 0x200,
        "the lock keeps the easing off the accumulator"
    );
}

/// The delta arm (`_DAT_1F800394` bit 25) does not ease at all: it snaps the
/// accumulator to the value the easing would otherwise walk to, in the same
/// instruction stream that posts the offset.
#[test]
fn the_delta_arm_snaps_the_accumulator_instead_of_easing_to_it() {
    let mut world = world_with_player();
    world.actors[0].move_state.world_y = -192;
    world.flags.story_flags = 0x0200_0000;
    world.load_field_script(sub9_default(0x100));
    let _ = world.tick();
    assert_eq!(world.camera.scene_offset, 0x100);
    assert_eq!(
        world.camera.offset_ease,
        0x100 + 192,
        "one frame, not the ~30 the easing would take"
    );
}

/// With a non-zero `ticks` operand the bit-24 arm hands both stores to the
/// property-ramp pool (`FUN_8003C5F0` at `0x801E1584` and `0x801E175C`): the
/// accumulator walks from its live value to `target` over `ticks` frame steps
/// while the input lock keeps the easing off it. `concnow`'s door-hop record
/// climbs it about a unit a vsync from `163` to `266`.
#[test]
fn the_timed_bit_24_arm_ramps_the_accumulator_over_its_ticks() {
    let mut world = world_with_player();
    world.actors[0].move_state.world_y = -192;
    world.flags.story_flags = legaia_engine_field::camera_ease::PAD_INPUT_LOCKED;
    let start = world.camera.offset_ease;
    let mut script = sub9_default(0x10A);
    script[4] = 100;
    world.load_field_script(script);
    let mut seen = vec![world.camera.offset_ease];
    for _ in 0..200 {
        let _ = world.tick();
        seen.push(world.camera.offset_ease);
    }
    assert!(seen.windows(2).all(|w| w[0] <= w[1]), "a monotonic climb");
    assert_eq!(*seen.last().unwrap(), 0x10A, "landing on the target");
    let first_landed = seen.iter().position(|&v| v == 0x10A).unwrap();
    assert!(
        (90..=110).contains(&first_landed),
        "over the operand's hundred frame steps, not at once ({first_landed}, from {start})"
    );
    assert_eq!(world.camera.scene_offset, 0x10A - 192);
}

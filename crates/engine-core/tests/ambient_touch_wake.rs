//! Bumping a town NPC **wakes it out of its ambient wait** - the motion VM's
//! one-slot touch mailbox, end to end.
//!
//! Retail wires this in two halves. The producer is the actor box test
//! `FUN_801CFC40`, called three times per step by the per-axis collision
//! `FUN_801CFE4C`; an overlap tail-calls `FUN_8003D038`, which stores the
//! touched actor's `+0x50` into `DAT_80073F1C` unless its `0x801C6470` record
//! byte is the `0x8C` sentinel. The consumer is the head of the ambient motion
//! VM's `0x05` wait arm (`FUN_80038158` at `0x8003882C`): when the mailbox
//! names the actor's own `+0x50`, it rewrites the wait cursor to
//! `duration - DAT_1F800393` so the countdown immediately below expires the
//! wait on that frame, and clears the mailbox.
//!
//! The port had the producer (`legaia_engine_vm::motion_vm::post_touch`) and
//! the wait op, but not the mailbox between them: an NPC parked in a `0x05`
//! wait ignored the player walking into it for the whole authored countdown.
//!
//! The two cases differ **only** in where the NPC stands, so a heading that
//! moves in one and not the other is the contact and nothing else.
//!
//! Disc-free: the world is synthetic, so nothing here is gated on
//! `LEGAIA_DISC_BIN`.

use legaia_engine_core::input::PadButton;
use legaia_engine_core::world::{FieldNpcAmbient, SceneMode, World};
use legaia_engine_vm::ambient_motion::AmbientMotion;

/// `World::field_collision_grid` is a `0x80 x 0x80` byte grid; the
/// crate-private constants are mirrored here because this is an integration
/// test.
const GRID_LEN: usize = 0x80 * 0x80;

/// Wait `0x60` ticks, then ramp the facing to compass point 2 over 8 frames.
/// The wait is far longer than the test runs, so **any** heading movement is
/// the wake and nothing else.
const PROGRAM: [u8; 5] = [0x05, 0x60, 0x04, 0x02, 0x08];

/// The op-`0x17` default-move id. The wake's third gate (and `post_touch`'s
/// only guard) reads this byte out of the `0x801C6470` arena; the `0x8C`
/// sentinel suppresses both.
const DEFAULT_MOVE: [u8; 2] = [0x03, 0x00];

/// A field world with the player at `(320, 320)` and one ambient NPC parked
/// in [`PROGRAM`] at `npc`.
fn world_with_npc_at(npc: (i16, i16)) -> World {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    world.actors[0].move_state.world_x = 320;
    world.actors[0].move_state.world_z = 320;
    world.field_collision_grid = vec![0u8; GRID_LEN];

    let mut vm = AmbientMotion::new(1, 0x000).with_position(npc.0, npc.1);
    vm.default_move = DEFAULT_MOVE;
    world.field_npc_positions.insert(1, npc);
    world.field_npc_headings.insert(1, 0);
    world.field_npc_ambient.insert(
        1,
        FieldNpcAmbient {
            variants: vec![(0xFFFF, PROGRAM.to_vec())],
            live: None,
            vm,
            walks: false,
        },
    );
    world
}

/// One frame of held pad (the movement step the retail probe rides on), then
/// the pad released so the rest of the run is pure ambient ticking.
fn run(world: &mut World, frames: usize) {
    world.set_pad(PadButton::Up.mask());
    let _ = world.tick();
    world.set_pad(0);
    for _ in 1..frames {
        let _ = world.tick();
    }
}

fn npc_heading(world: &World) -> u16 {
    world.field_npc_ambient[&1].vm.heading
}

fn npc_pc(world: &World) -> u16 {
    world.field_npc_ambient[&1].vm.pc
}

/// Baseline: an NPC the player never reaches stays in its wait. Without this
/// the wired case proves nothing - a `0x60`-tick wait that expired on its own
/// inside the run would look exactly like a wake.
#[test]
fn an_untouched_npc_holds_its_wait_for_the_authored_countdown() {
    let mut world = world_with_npc_at((320, 900));
    run(&mut world, 40);
    assert_eq!(npc_pc(&world), 0, "still parked on the 0x05 wait");
    assert_eq!(
        npc_heading(&world),
        0x000,
        "and therefore never reached the facing ramp behind it"
    );
}

/// Wired: the same NPC standing inside the player's contact box wakes, and the
/// op behind the wait runs.
#[test]
fn walking_into_an_npc_ends_its_ambient_wait() {
    let mut world = world_with_npc_at((320, 350));
    run(&mut world, 40);
    assert_ne!(
        npc_pc(&world),
        0,
        "the contact post must expire the wait (mailbox never reached the VM)"
    );
    assert_ne!(
        npc_heading(&world),
        0x000,
        "and the op behind the wait must run - the NPC turns on being bumped"
    );
    assert_eq!(
        world.field_npc_ambient[&1].vm.pending_touch, None,
        "the wait arm consumes the mailbox (retail's 0xFF sentinel store)"
    );
}

/// The `0x801C6470` sentinel gate, at host level: an NPC whose stream has not
/// run an op-`0x17` carries `0x8C` in the arena, and `FUN_8003D038` drops the
/// post outright - so it is bumped without waking.
#[test]
fn an_npc_with_no_default_move_record_is_not_woken_by_a_bump() {
    let mut world = world_with_npc_at((320, 350));
    world.field_npc_ambient.get_mut(&1).unwrap().vm.default_move =
        [legaia_engine_vm::ambient_motion::DEFAULT_MOVE_UNSET; 2];
    run(&mut world, 40);
    assert_eq!(
        npc_pc(&world),
        0,
        "the suppressed post must leave the wait running"
    );
    assert_eq!(npc_heading(&world), 0x000);
}

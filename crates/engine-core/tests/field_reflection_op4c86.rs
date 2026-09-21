//! Field-VM `4C 86` / `4C 87` - the reflection controller's install and
//! teardown, end to end through the world's own field-VM host.
//!
//! The install is fifteen bytes and its **last** operand byte is a
//! cross-context actor id, not a rotation axis: the arm at `0x801E21E0`
//! resolves that byte through `FUN_8003C83C` and calls
//! `FUN_801E573C(executing_ctx, resolved_actor, w0..w5)`, which seats a
//! controller whose tick (`FUN_801E5154`) reads the resolved actor and
//! writes the executing context's own actor.
//!
//! The bytes are `concnow`'s `p1[1]`:
//! `4C 86 00 00 A0 37 1E 00 5A 00 26 00 6E 00 F8` - no X mirror, a Z plane
//! at `0x37A0`, the tracking rect tiles `30..38` x `90..110`, reflecting
//! `0xF8` (the player).
//!
//! REF: FUN_801E573C (the spawner), FUN_801E5154 (the tick),
//! REF: FUN_8003CF40 (the `4C 87` retire sweep)

use legaia_engine_core::actor_handler::ActorHandler;
use legaia_engine_core::field_channels::FieldChannel;
use legaia_engine_core::world::{EasedMoveTarget, World};
use legaia_engine_vm::field::FieldCtx;
use legaia_engine_vm::field_actor_reflect::REFLECT_HANDLER;

const INSTALL: [u8; 15] = [
    0x4C, 0x86, 0x00, 0x00, 0xA0, 0x37, 0x1E, 0x00, 0x5A, 0x00, 0x26, 0x00, 0x6E, 0x00, 0xF8,
];
const TEARDOWN: [u8; 2] = [0x4C, 0x87];

const MIRROR_Z: i16 = 0x37A0;
/// A world position inside the shipped rect (tiles 30..38 x 90..110).
const IN_RECT: (i16, i16) = (34 * 128, 100 * 128);

/// A world whose executing context is placement 5, with the player seated
/// and one resolvable cross-context channel.
fn world_with_mirror_actor() -> World {
    let mut w = World::new();
    w.player_actor_slot = Some(0);
    w.actors[0].active = true;
    w.actors[0].move_state.world_x = IN_RECT.0;
    w.actors[0].move_state.world_z = IN_RECT.1;
    w.actors[0].move_state.render_26 = 0x0200;
    w.field_vm.channels = vec![FieldChannel {
        placement_index: 5,
        ctx: FieldCtx {
            script_id: 0x2A,
            ..FieldCtx::default()
        },
        record_offset: 0,
        pc: 0,
        done: false,
        object_bind: false,
    }];
    w.field_vm.executing_channel = Some(5);
    // The image stands somewhere before the first tick moves it.
    w.npcs.positions.insert(5, (0, 0));
    w
}

fn controllers(w: &World) -> Vec<usize> {
    w.actors
        .iter()
        .enumerate()
        .filter(|(_, a)| a.active && a.handler == ActorHandler::Reflection)
        .map(|(i, _)| i)
        .collect()
}

#[test]
fn the_install_is_fifteen_bytes_and_seats_a_controller() {
    let mut w = world_with_mirror_actor();
    w.field_bytecode = INSTALL.to_vec();
    w.field_pc = 0;
    w.step_field().expect("the script stepped");
    assert_eq!(w.field_pc, 15);

    let seated = controllers(&w);
    assert_eq!(seated.len(), 1);
    let link = w.actors[seated[0]].reflection.expect("a pair was formed");
    assert_eq!(w.actors[seated[0]].handler.va(), REFLECT_HANDLER);
    // `+0x90` is the executing script's own actor and `+0x94` the byte's.
    assert_eq!(link.destination, EasedMoveTarget::Placement(5));
    assert_eq!(link.source, EasedMoveTarget::Player);
    assert_eq!(link.controller.mirror_x, 0);
    assert_eq!(link.controller.mirror_z, MIRROR_Z);
    assert_eq!(
        (
            link.controller.min_tile_x,
            link.controller.min_tile_z,
            link.controller.max_tile_x,
            link.controller.max_tile_z
        ),
        (30, 90, 38, 110)
    );
    assert_eq!(w.actors[seated[0]].state_54, 0);
}

#[test]
fn the_pool_pass_mirrors_the_named_actor_onto_the_scripts_own() {
    let mut w = world_with_mirror_actor();
    w.field_bytecode = INSTALL.to_vec();
    w.field_pc = 0;
    w.step_field().expect("the script stepped");

    w.tick_handler_actors(1);
    let (x, z) = w.npcs.positions[&5];
    // The `(0, zz)` arm: X copied, Z reflected in the plane, facing flipped.
    assert_eq!(x, IN_RECT.0);
    assert_eq!(z, 2 * MIRROR_Z - IN_RECT.1);
    assert_eq!(w.npcs.headings[&5] as i32 & 0xFFF, (0x800 - 0x200) & 0xFFF);
    // And the image sits on the far side of the plane from the player.
    assert!(z > MIRROR_Z && IN_RECT.1 < MIRROR_Z);

    // It tracks: move the player one tile and the image follows.
    w.actors[0].move_state.world_z = IN_RECT.1 + 128;
    w.tick_handler_actors(1);
    assert_eq!(w.npcs.positions[&5].1, 2 * MIRROR_Z - (IN_RECT.1 + 128));
}

#[test]
fn the_mirror_stops_tracking_once_the_source_leaves_the_rect() {
    let mut w = world_with_mirror_actor();
    w.field_bytecode = INSTALL.to_vec();
    w.field_pc = 0;
    w.step_field().expect("the script stepped");
    w.tick_handler_actors(1);
    let held = w.npcs.positions[&5];

    // Tile 111 is one past the rect's `max_tile_z` of 110.
    w.actors[0].move_state.world_z = 111 * 128;
    w.tick_handler_actors(1);
    assert_eq!(w.npcs.positions[&5], held, "the image froze where it was");
    assert_eq!(controllers(&w).len(), 1, "and the controller is still live");
}

#[test]
fn an_unresolvable_source_seats_nothing_but_still_advances_fifteen() {
    let mut w = world_with_mirror_actor();
    // `0x2B` names no channel here; retail's `beqz s7` skips the spawner
    // with the PC already past the instruction.
    let mut bytes = INSTALL;
    bytes[14] = 0x2B;
    w.field_bytecode = bytes.to_vec();
    w.field_pc = 0;
    w.step_field().expect("the script stepped");
    assert_eq!(w.field_pc, 15);
    assert!(controllers(&w).is_empty());
}

#[test]
fn a_named_channel_resolves_to_its_placement() {
    // A second channel is the source: naming the executing placement itself
    // would pair one actor with itself, which seats nothing.
    let mut w = world_with_mirror_actor();
    w.field_vm.channels.push(FieldChannel {
        placement_index: 6,
        ctx: FieldCtx {
            script_id: 0x2B,
            ..FieldCtx::default()
        },
        record_offset: 0,
        pc: 0,
        done: false,
        object_bind: false,
    });
    w.npcs.positions.insert(6, IN_RECT);
    let mut bytes = INSTALL;
    bytes[14] = 0x2B; // the channel seated at placement 6
    w.field_bytecode = bytes.to_vec();
    w.field_pc = 0;
    w.step_field().expect("the script stepped");
    let link = w.actors[controllers(&w)[0]]
        .reflection
        .expect("a pair was formed");
    assert_eq!(link.destination, EasedMoveTarget::Placement(5));
    assert_eq!(link.source, EasedMoveTarget::Placement(6));
}

#[test]
fn the_teardown_retires_every_controller_and_advances_two() {
    let mut w = world_with_mirror_actor();
    w.field_bytecode = INSTALL.to_vec();
    w.field_pc = 0;
    w.step_field().expect("the script stepped");
    assert_eq!(controllers(&w).len(), 1);

    w.field_bytecode = TEARDOWN.to_vec();
    w.field_pc = 0;
    w.step_field().expect("the script stepped");
    // Two bytes, not a park: `4C 87`'s arm tail-jumps to the shared exit
    // whose `addiu s8,s8,2` rides the sweep call's delay slot.
    assert_eq!(w.field_pc, 2);
    // The sweep sets the kill bit; the pool pass collects the slot.
    assert_eq!(w.tick_handler_actors(1), 1);
    assert!(controllers(&w).is_empty());
}

#[test]
fn a_dead_end_tears_the_controller_down() {
    let mut w = world_with_mirror_actor();
    w.field_bytecode = INSTALL.to_vec();
    w.field_pc = 0;
    w.step_field().expect("the script stepped");

    // Retail's first test is `either end's +0x10 & 8`; an end the engine can
    // no longer resolve is that dead actor.
    w.npcs.positions.remove(&5);
    assert_eq!(w.tick_handler_actors(1), 1);
    assert!(controllers(&w).is_empty());
}

#[test]
fn a_player_raised_record_still_mirrors_onto_its_own_placement() {
    // Every shipped `4C 86` sits in a talk record the player raised, so the
    // executing context carries the `+0x10` player bit - and names `0xF8`.
    // Retail's `a0` is still that record's own context, not the player: read
    // the bit as "the context IS the player" and the pair is the player with
    // themself, and the tick walks them onto the mirror line every frame.
    let mut w = world_with_mirror_actor();
    w.field_ctx.flags |= 0x0100_0000;
    w.field_bytecode = INSTALL.to_vec();
    w.field_pc = 0;
    w.step_field().expect("the script stepped");
    assert_eq!(w.field_pc, 15);

    let seated = controllers(&w);
    assert_eq!(seated.len(), 1);
    let link = w.actors[seated[0]].reflection.expect("a pair was formed");
    assert_eq!(link.destination, EasedMoveTarget::Placement(5));
    assert_eq!(link.source, EasedMoveTarget::Player);

    let before = (
        w.actors[0].move_state.world_x,
        w.actors[0].move_state.world_z,
    );
    for _ in 0..8 {
        w.tick_handler_actors(1);
    }
    // The image moved; the player did not.
    assert_eq!(w.npcs.positions[&5].1, 2 * MIRROR_Z - IN_RECT.1);
    assert_eq!(
        (
            w.actors[0].move_state.world_x,
            w.actors[0].move_state.world_z
        ),
        before
    );
}

#[test]
fn a_pair_whose_two_ends_are_one_actor_seats_nothing() {
    // No executing channel and the player bit up: the only seat left for the
    // image is the player, which is also the named source. Retail forms no
    // such record; the engine refuses it rather than strand the player.
    let mut w = world_with_mirror_actor();
    w.field_vm.executing_channel = None;
    w.field_ctx.flags |= 0x0100_0000;
    w.field_bytecode = INSTALL.to_vec();
    w.field_pc = 0;
    w.step_field().expect("the script stepped");
    assert_eq!(w.field_pc, 15);
    assert!(controllers(&w).is_empty());
}

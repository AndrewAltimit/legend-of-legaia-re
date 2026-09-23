//! Field-VM op `0x4C` sub-1 sub-op `0x14` - the actor clone, end to end
//! through the world's own field-VM host.
//!
//! Two things are pinned here. The **instruction is eight bytes**: retail's
//! arm at `0x801E0E80` reads `lbu a0,6(s6)` and leaves through
//! `j 0x801E3624` with `addiu s8,s8,1` in the delay slot, on top of the
//! `addiu s8,s8,7` the nibble entry already did. And the clone it seats is a
//! pool actor whose per-frame body is `FUN_801D820C`, so the world's own
//! handler pass ticks it out and retires it.
//!
//! The bytes are the ones `vozz` issues at `p2[13] + 0x0A2E`:
//! `4C 14 32 28 1E 99 01 08` - three of them eight frames apart against
//! cross-context target `0x08`.
//!
//! REF: FUN_801D835C (the clone helper), FUN_801D820C (the clone's tick)

use legaia_engine_core::actor_handler::ActorHandler;
use legaia_engine_core::field_actor_clone::{CLONE_HANDLER, clone_plan, modulation_rgb};
use legaia_engine_core::field_channels::FieldChannel;
use legaia_engine_core::world::World;
use legaia_engine_vm::field::FieldCtx;

const CLONE_INSN: [u8; 8] = [0x4C, 0x14, 0x32, 0x28, 0x1E, 0x99, 0x01, 0x08];

/// A world with one resolvable cross-context channel (`script_id 0x08`)
/// standing at a known spot.
fn world_with_source() -> World {
    let mut w = World::new();
    w.field_vm.channels = vec![FieldChannel {
        placement_index: 3,
        ctx: FieldCtx {
            script_id: 0x08,
            world_x: 0x0140,
            world_y: 0x0020,
            world_z: 0x0280,
            field_24: 0x11,
            field_26: 0x22,
            field_28: 0x33,
            ..FieldCtx::default()
        },
        record_offset: 0,
        pc: 0,
        done: false,
        object_bind: false,
    }];
    w
}

fn clones(w: &World) -> Vec<usize> {
    w.actors
        .iter()
        .enumerate()
        .filter(|(_, a)| a.active && a.handler == ActorHandler::ClipFade)
        .map(|(i, _)| i)
        .collect()
}

#[test]
fn the_instruction_is_eight_bytes_and_seats_a_clone() {
    let mut w = world_with_source();
    w.field_bytecode = CLONE_INSN.to_vec();
    w.field_pc = 0;
    w.step_field().expect("the script stepped");

    // Eight, not seven: a seven-byte advance would leave the PC on the
    // source-id byte and decode the rest of the script one byte out.
    assert_eq!(w.field_pc, 8);

    let seated = clones(&w);
    assert_eq!(seated.len(), 1, "one clone per instruction");
    let a = &w.actors[seated[0]];
    assert_eq!(a.handler.va(), CLONE_HANDLER);
    // The transform is the source's, copied verbatim.
    assert_eq!(
        (
            a.move_state.world_x,
            a.move_state.world_y,
            a.move_state.world_z
        ),
        (0x0140, 0x0020, 0x0280)
    );
    assert_eq!(
        (
            a.move_state.render_24,
            a.move_state.render_26,
            a.move_state.render_28
        ),
        (0x11, 0x22, 0x33)
    );
    // `+0x54` is the operand's `s16`, `+0x74` its `u24`.
    assert_eq!(a.physics.timer, 0x0199);
    assert_eq!(a.modulation_rgb, Some(modulation_rgb(0x001E_2832)));
    assert_eq!(a.physics.focal_envelope, 0);
}

#[test]
fn the_clone_reads_the_sources_live_position_not_its_spawn_tile() {
    let mut w = world_with_source();
    // A walk leg moved placement 3 since the MAN seeded the channel.
    w.npcs.positions.insert(3, (0x0500, 0x0600));
    w.field_bytecode = CLONE_INSN.to_vec();
    w.field_pc = 0;
    w.step_field().expect("the script stepped");

    let seated = clones(&w);
    assert_eq!(seated.len(), 1);
    let a = &w.actors[seated[0]];
    assert_eq!(a.move_state.world_x, 0x0500);
    assert_eq!(a.move_state.world_z, 0x0600);
    // Y still comes off the channel - the engine's NPC map is (x, z).
    assert_eq!(a.move_state.world_y, 0x0020);
}

#[test]
fn an_unresolvable_source_seats_nothing_but_still_advances_eight() {
    let mut w = World::new();
    w.field_bytecode = CLONE_INSN.to_vec();
    w.field_pc = 0;
    w.step_field().expect("the script stepped");
    // Retail's `beqz s5,0x801e0eb0` skips the helper and falls into the same
    // exit, so the extra byte is consumed either way.
    assert_eq!(w.field_pc, 8);
    assert!(clones(&w).is_empty());
}

#[test]
fn the_world_handler_pass_ticks_the_clone_out_and_retires_it() {
    let mut w = world_with_source();
    w.field_bytecode = CLONE_INSN.to_vec();
    w.field_pc = 0;
    w.step_field().expect("the script stepped");
    let slot = clones(&w)[0];

    // The accumulator is `rate` per vsync until it reaches `0x1000`.
    let lifetime = clone_plan(Default::default(), 0x001E_2832, 0x0199).lifetime_vsyncs() as usize;
    assert_eq!(lifetime, 11);

    for frame in 1..lifetime {
        let retired = w.tick_handler_actors(1);
        assert_eq!(retired, 0, "frame {frame} is still inside the clip");
        assert!(w.actors[slot].active);
        assert_eq!(
            w.actors[slot].physics.focal_envelope,
            (0x199 * frame) as i16
        );
    }

    // The frame the accumulator fills, the tick pins it one below full,
    // raises the retire bit, and the end-of-pass sweep collects the slot.
    assert_eq!(w.tick_handler_actors(1), 1);
    assert!(!w.actors[slot].active);
    assert_eq!(w.actors[slot].physics.focal_envelope, 0x0FFF);
}

#[test]
fn a_burst_seats_one_clone_per_instruction() {
    let mut w = world_with_source();
    let mut bc = Vec::new();
    for _ in 0..3 {
        bc.extend_from_slice(&CLONE_INSN);
    }
    w.field_bytecode = bc;
    w.field_pc = 0;
    for _ in 0..3 {
        w.step_field().expect("the script stepped");
    }
    assert_eq!(w.field_pc, 24);
    assert_eq!(clones(&w).len(), 3);
}

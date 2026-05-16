//! End-to-end coverage for the field-VM actor-spawn → materialize flow.
//!
//! This is the integration-tier companion to the unit test in
//! `world.rs::field_op_4c_n8_sub0_then_materialize_flow_end_to_end`. It runs
//! in CI (no disc data required) and locks in the public API contract the
//! engine drivers rely on:
//!
//!  - [`FIELD_SPAWN_START_SLOT`] is the start-slot every driver
//!    (`SceneHost::tick`, the asset-viewer field runner) passes to
//!    [`World::materialize_actor_spawns`]. The constant brackets the
//!    party + scripted-actor reservation; tests below assume slot
//!    `FIELD_SPAWN_START_SLOT` is unallocated on a fresh world so the
//!    materializer lands there.
//!  - A field-VM `0x4C 0x80` opcode pushes one entry per packet onto
//!    `pending_actor_spawns`, emits an `ActorAllocate` event, and the
//!    subsequent `materialize_actor_spawns` call drains the queue into
//!    real actor slots with `ActorSpawned` events. Field-VM regressions
//!    that fail to wire either half of this chain will fail here.

use legaia_engine_core::field_events::FieldEvent;
use legaia_engine_core::world::{FIELD_SPAWN_START_SLOT, SceneMode, World};

/// Build a minimal field-VM record exercising op `0x4C 0x80`:
///   - 1 child packet, body `[0xAA, 0xBB]`, packet terminator `0x00`.
///
/// Matches the `[opcode, which, count, body..., 0x00]` encoding the
/// dispatcher's halt-acquire prelude (`FUN_8003CA38` packet-length walker
/// mirrored by `legaia_engine_vm::field_helpers::packet_length`) expects.
fn op_4c_80_one_packet_record() -> Vec<u8> {
    vec![0x4C, 0x80, 0x01, 0xAA, 0xBB, 0x00]
}

#[test]
fn field_op_4c_80_then_materialize_lands_actor_in_default_start_slot() {
    let mut world = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    world.load_field_record(&op_4c_80_one_packet_record());

    let _ = world.tick();

    // The opcode queued exactly one record on `pending_actor_spawns`.
    assert_eq!(world.pending_actor_spawns.len(), 1);
    assert_eq!(world.pending_actor_spawns[0], &[0xAA, 0xBB]);

    let allocated = world.materialize_actor_spawns(FIELD_SPAWN_START_SLOT);
    assert_eq!(allocated, 1);
    assert!(world.pending_actor_spawns.is_empty());

    // Slot `FIELD_SPAWN_START_SLOT` is the first inactive slot in the
    // allocation range on a default world, so the spawn lands there.
    let slot = FIELD_SPAWN_START_SLOT as usize;
    assert!(
        world.actors[slot].active,
        "expected actor[{slot}] to be active after materialize"
    );
    assert_eq!(
        world.actors[slot].spawn_record.as_deref(),
        Some(&[0xAA, 0xBB][..]),
        "spawn_record should mirror the opcode's child-packet bytes"
    );
    // `kind` / `variant` default to 0 until a record encoding is pinned -
    // see project_fun_801d77f4_actor_allocator memory note.
    assert_eq!(world.actors[slot].kind, 0);
    assert_eq!(world.actors[slot].variant, 0);

    // The event queue carries both halves of the chain in emission order:
    // the `ActorAllocate` from the opcode and the `ActorSpawned` from the
    // materializer.
    let events = world.drain_field_events();
    let mut saw_alloc = false;
    let mut saw_spawned = false;
    for ev in &events {
        match ev {
            FieldEvent::ActorAllocate { records } => {
                assert_eq!(records, &[vec![0xAA, 0xBB]]);
                saw_alloc = true;
            }
            FieldEvent::ActorSpawned {
                slot: s, record, ..
            } => {
                assert_eq!(*s, FIELD_SPAWN_START_SLOT);
                assert_eq!(record, &[0xAA, 0xBB]);
                saw_spawned = true;
            }
            _ => {}
        }
    }
    assert!(saw_alloc, "expected ActorAllocate event, got {events:?}");
    assert!(saw_spawned, "expected ActorSpawned event, got {events:?}");
}

/// Drivers (`SceneHost::tick`, asset-viewer field runner) call
/// [`World::materialize_actor_spawns`] every frame, including when no
/// spawn is pending. The call must be cheap and side-effect-free on an
/// idle world.
#[test]
fn materialize_actor_spawns_with_empty_queue_is_noop() {
    let mut world = World::default();
    world.actors[FIELD_SPAWN_START_SLOT as usize].active = false;

    let allocated = world.materialize_actor_spawns(FIELD_SPAWN_START_SLOT);
    assert_eq!(allocated, 0);
    assert!(world.drain_field_events().is_empty());
    assert!(!world.actors[FIELD_SPAWN_START_SLOT as usize].active);
}

/// Two `0x4C 0x80` packets in the same record should yield two
/// consecutive materialized actors.
#[test]
fn two_packets_materialize_into_two_consecutive_slots() {
    let mut world = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    // count=2, packet A = [0x40], packet B = [0x50, 0x51], each followed by
    // a 0x00 terminator. Body bytes are all >= 0x1F (the
    // `field_helpers::packet_length` terminator threshold).
    let bytecode = vec![0x4C, 0x80, 0x02, 0x40, 0x00, 0x50, 0x51, 0x00];
    world.load_field_record(&bytecode);
    let _ = world.tick();

    assert_eq!(world.pending_actor_spawns.len(), 2);
    let allocated = world.materialize_actor_spawns(FIELD_SPAWN_START_SLOT);
    assert_eq!(allocated, 2);

    let a = FIELD_SPAWN_START_SLOT as usize;
    let b = a + 1;
    assert!(world.actors[a].active);
    assert!(world.actors[b].active);
    assert_eq!(world.actors[a].spawn_record.as_deref(), Some(&[0x40][..]));
    assert_eq!(
        world.actors[b].spawn_record.as_deref(),
        Some(&[0x50, 0x51][..])
    );
}

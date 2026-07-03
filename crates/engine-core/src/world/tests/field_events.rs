use super::*;

/// Dialog-advance host hook (`op 0x4C n5 sub-4`): when `current_dialog`
/// is set, the VM halts at the poll site. A just-pressed Cross /
/// Circle clears the request inline and unblocks the VM the same
/// frame, with a `DialogDismissed` event surfaced for downstream
/// HUD consumers.
#[test]
fn dialog_advance_halts_then_clears_on_just_pressed_cross() {
    use crate::input::PadButton;

    let mut world = World::new();
    world.mode = SceneMode::Field;

    // Open dialogue via the field-interact path (the real opener), then arm a
    // poll (4C 54) followed by a sentinel op.
    // 0x3E 0x05 0x03: field-interact (op0<100) on actor slot 3 -> opens its
    //   seeded inline dialogue (3 bytes).
    // 0x4C 0x54: dialog-advance poll (2 bytes).
    // 0x00: sentinel that makes `step_field` advance further once the dialog
    //   clears.
    world
        .field_npc_dialog
        .insert(3, vec![0x1F, b'h', b'i', 0x00]);
    let bc = vec![0x3E, 0x05, 0x03, 0x4C, 0x54, 0x00];
    world.load_field_script(bc);

    // Tick 1: open the dialog. The 4C 54 poll runs next tick.
    let _ = world.tick();
    assert!(world.current_dialog.is_some(), "dialog should be open");

    // No buttons pressed: the poll halts at the same PC.
    world.input.set_pad(0);
    let pc_before = world.field_pc;
    let _ = world.tick();
    assert!(
        world.current_dialog.is_some(),
        "dialog persists with no input"
    );
    assert_eq!(
        world.field_pc, pc_before,
        "VM should halt at the poll PC while dialog is active"
    );

    // Cross just-pressed: the host clears the request inline and
    // advances PC by 2 (past the poll).
    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    assert!(
        world.current_dialog.is_none(),
        "dialog should clear on just-pressed Cross",
    );
    let evs = world.drain_field_events();
    assert!(
        evs.iter().any(|e| matches!(e, FieldEvent::DialogDismissed)),
        "expected DialogDismissed event, got {evs:?}",
    );
    assert!(
        world.field_pc > pc_before,
        "VM should advance past poll PC ({} > {})",
        world.field_pc,
        pc_before,
    );
}

/// Dialog-advance hook returns `false` (advance) when no dialog is
/// active. Mirrors the retail dispatcher's behavior when
/// `FUN_801D65D8(0)` returns zero (dialog done).
#[test]
fn dialog_advance_no_op_when_no_dialog() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // Just the poll + sentinel - no preceding 0x3F.
    let bc = vec![0x4C, 0x54, 0x00];
    world.load_field_script(bc);
    let pc_before = world.field_pc;
    let _ = world.tick();
    assert!(
        world.field_pc > pc_before,
        "VM should advance immediately when no dialog is showing",
    );
}

/// Op 0x3A (add_money) clamps to `[0, 9_999_999]` and emits `AddMoney`.
#[test]
fn field_op_3a_clamps_and_emits_add_money() {
    let mut world = World::new();
    world.money = 100;
    world.mode = SceneMode::Field;
    // 0x3A op0=0xFF op1=0xFF op2=0xFF (24-bit -1) → delta = -1.
    // The op handler reads the 3-byte payload; sign-extend to i32.
    let bytecode = vec![0x3A, 0xFF, 0xFF, 0xFF];
    world.load_field_script(bytecode);
    let _ = world.tick();
    assert!(world.money >= 0, "money clamps to non-negative");
    let evs = world.drain_field_events();
    assert!(
        evs.iter().any(|e| matches!(e, FieldEvent::AddMoney { .. })),
        "expected AddMoney event, got {evs:?}"
    );
}

/// Op 0x3C (party_add) appends to `party_actor_slots` and seeds the
/// leader on the empty-party path.
#[test]
fn field_op_3c_party_add_first_member_becomes_leader() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // 0x3C + char_id (op0).
    let bytecode = vec![0x3C, 0x07];
    world.load_field_script(bytecode);
    let _ = world.tick();
    assert_eq!(world.party_actor_slots, vec![Some(7)]);
    assert_eq!(world.party_leader_slot, Some(7));
    let evs = world.drain_field_events();
    assert!(
        evs.iter().any(|e| matches!(
            e,
            FieldEvent::PartyAdd {
                char_id: 7,
                accepted: true
            }
        )),
        "expected PartyAdd event, got {evs:?}"
    );
}

/// Drain helper empties the queue.
#[test]
fn drain_field_events_empties_queue() {
    let mut world = World::new();
    world
        .pending_field_events
        .push(FieldEvent::GiveItem { item_id: 1 });
    let drained = world.drain_field_events();
    assert_eq!(drained.len(), 1);
    assert!(world.pending_field_events.is_empty());
}

/// Op `0x4C 0x80` (actor allocator) walks `count` variable-length
/// records using the `FUN_8003CA38` packet-length rule, emits one
/// `ActorAllocate` event, and queues each record's bytecode in
/// `pending_actor_spawns`. Encoding here: count=2, two records each
/// terminated by `0x00`.
#[test]
fn field_op_4c_n8_sub0_walks_records_and_queues_spawns() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // [4C, 0x80, 2, 0x40, 0x41, 0x00, 0xC1, 0x42, 0x00]
    //   record 0 = [0x40, 0x41] (two normal tokens, terminator 0x00)
    //   record 1 = [0xC1, 0x42] (escape pair via 0xCx high nibble)
    let bytecode = vec![0x4C, 0x80, 0x02, 0x40, 0x41, 0x00, 0xC1, 0x42, 0x00];
    world.load_field_script(bytecode);
    let _ = world.tick();
    // PC should land at byte 3 (the first record's first byte) - the
    // retail VM advances PC by exactly 3 regardless of how many
    // records the host consumes.
    assert_eq!(world.field_pc, 3);
    // Pending queue should hold both records, in emission order.
    let spawns = world.drain_actor_spawns();
    assert_eq!(spawns.len(), 2);
    assert_eq!(spawns[0], vec![0x40, 0x41]);
    assert_eq!(spawns[1], vec![0xC1, 0x42]);
    // The event queue should also carry one ActorAllocate with both
    // records.
    let evs = world.drain_field_events();
    let allocate = evs
        .iter()
        .find_map(|e| match e {
            FieldEvent::ActorAllocate { records } => Some(records.clone()),
            _ => None,
        })
        .expect("expected ActorAllocate event");
    assert_eq!(allocate.len(), 2);
    assert_eq!(allocate[0], vec![0x40, 0x41]);
    assert_eq!(allocate[1], vec![0xC1, 0x42]);
}

/// `count = 0` is a legal degenerate case - no records walked, no
/// event payload, but the event is still emitted to mark the
/// allocator call site.
#[test]
fn field_op_4c_n8_sub0_zero_count_emits_empty_event() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    let bytecode = vec![0x4C, 0x80, 0x00];
    world.load_field_script(bytecode);
    let _ = world.tick();
    assert_eq!(world.field_pc, 3);
    assert!(world.drain_actor_spawns().is_empty());
    let evs = world.drain_field_events();
    assert!(
        evs.iter().any(|e| matches!(
            e,
            FieldEvent::ActorAllocate { records } if records.is_empty()
        )),
        "expected empty ActorAllocate event, got {evs:?}"
    );
}

/// `drain_actor_spawns` empties the queue.
#[test]
fn drain_actor_spawns_empties_queue() {
    let mut world = World::new();
    world.pending_actor_spawns.push(vec![0xAA, 0xBB]);
    let drained = world.drain_actor_spawns();
    assert_eq!(drained, vec![vec![0xAA, 0xBB]]);
    assert!(world.pending_actor_spawns.is_empty());
}

/// `materialize_actor_spawns` allocates a fresh slot from
/// `start_slot..MAX_ACTORS`, populates it with the queued record, and
/// emits an `ActorSpawned` event.
#[test]
fn materialize_actor_spawns_allocates_slot_and_emits_event() {
    let mut world = World::new();
    world.pending_actor_spawns.push(vec![0x10, 0x20, 0x30]);
    let allocated = world.materialize_actor_spawns(8);
    assert_eq!(allocated, 1);
    assert!(world.pending_actor_spawns.is_empty());
    assert!(world.actors[8].active);
    assert_eq!(
        world.actors[8].spawn_record.as_deref(),
        Some(&[0x10, 0x20, 0x30][..])
    );
    assert_eq!(world.actors[8].kind, 0);
    assert_eq!(world.actors[8].variant, 0);
    let evs = world.drain_field_events();
    let spawned = evs
        .iter()
        .find_map(|e| match e {
            FieldEvent::ActorSpawned {
                slot,
                kind,
                variant,
                record,
            } => Some((*slot, *kind, *variant, record.clone())),
            _ => None,
        })
        .expect("expected ActorSpawned event");
    assert_eq!(spawned, (8u8, 0u16, 0u16, vec![0x10, 0x20, 0x30]));
}

/// `materialize_actor_spawns` allocates consecutive inactive slots
/// when several spawn requests are queued.
#[test]
fn materialize_actor_spawns_fills_consecutive_inactive_slots() {
    let mut world = World::new();
    world.pending_actor_spawns.push(vec![0xAA]);
    world.pending_actor_spawns.push(vec![0xBB]);
    world.pending_actor_spawns.push(vec![0xCC]);
    let allocated = world.materialize_actor_spawns(4);
    assert_eq!(allocated, 3);
    assert!(world.actors[4].active);
    assert!(world.actors[5].active);
    assert!(world.actors[6].active);
    assert_eq!(world.actors[4].spawn_record.as_deref(), Some(&[0xAA][..]));
    assert_eq!(world.actors[5].spawn_record.as_deref(), Some(&[0xBB][..]));
    assert_eq!(world.actors[6].spawn_record.as_deref(), Some(&[0xCC][..]));
}

/// Slots below `start_slot` are reserved - even when they are
/// inactive, the materializer doesn't touch them.
#[test]
fn materialize_actor_spawns_skips_reserved_low_slots() {
    let mut world = World::new();
    // Slot 0 is inactive but reserved (start_slot=10).
    world.pending_actor_spawns.push(vec![0xDE, 0xAD]);
    world.materialize_actor_spawns(10);
    assert!(!world.actors[0].active);
    assert!(world.actors[10].active);
}

/// Mirrors retail's "pool exhausted → bail silently" branch of
/// `FUN_801D77F4`. When no inactive slot is available in the
/// allocation range, the record is dropped and a `ActorSpawnFailed`
/// event is emitted instead of `ActorSpawned`.
#[test]
fn materialize_actor_spawns_emits_failure_when_pool_exhausted() {
    let mut world = World::new();
    // Make every slot from index 60 upward active.
    for slot in 60..MAX_ACTORS {
        world.actors[slot].active = true;
    }
    world.pending_actor_spawns.push(vec![0xEE]);
    let allocated = world.materialize_actor_spawns(60);
    assert_eq!(allocated, 0);
    let evs = world.drain_field_events();
    assert!(evs.iter().any(|e| matches!(
        e,
        FieldEvent::ActorSpawnFailed { record } if record == &[0xEE]
    )));
}

/// End-to-end: a field-VM `0x4C 0x80` opcode followed by
/// `materialize_actor_spawns` should land both events
/// (`ActorAllocate` from the opcode, `ActorSpawned` from the
/// materializer) and leave the actor slot populated.
#[test]
fn field_op_4c_n8_sub0_then_materialize_flow_end_to_end() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // One record `[0x40, 0x41]` terminated by `0x00`.
    let bytecode = vec![0x4C, 0x80, 0x01, 0x40, 0x41, 0x00];
    world.load_field_script(bytecode);
    let _ = world.tick();
    let allocated = world.materialize_actor_spawns(16);
    assert_eq!(allocated, 1);
    assert!(world.actors[16].active);
    assert_eq!(
        world.actors[16].spawn_record.as_deref(),
        Some(&[0x40, 0x41][..])
    );
    let evs = world.drain_field_events();
    // Both the ActorAllocate (from the opcode) and ActorSpawned (from
    // the materializer) should appear in emission order.
    let kinds: Vec<&'static str> = evs
        .iter()
        .filter_map(|e| match e {
            FieldEvent::ActorAllocate { .. } => Some("alloc"),
            FieldEvent::ActorSpawned { .. } => Some("spawned"),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, vec!["alloc", "spawned"]);
}

/// Op `0x4C 0xD8` is the synchronous-spawn sibling of the halt-acquire
/// `0x4C 0x80` path. The dispatcher decodes
/// `[0x4C, 0xD8, vdf_idx, tmd_lo, tmd_hi, kind_lo, kind_hi, var_lo, var_hi]`
/// into `(vdf_idx, [tmd_idx, kind, variant])` and calls the
/// FieldHostImpl override directly - no queue. The actor slot must
/// come out active with `kind` / `variant` mirrored from the operand,
/// and a single `ActorSpawned` event must surface in the queue.
#[test]
fn field_op_4c_d8_spawns_actor_synchronously_with_kind_variant() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // `[0x4C, 0xD8, vdf_idx=0x07, tmd=0x0102, kind=0xABCD, variant=0xBEEF, 0x00]`.
    // Trailing 0x00 is a HALT so the VM doesn't run off the end.
    let bytecode = vec![0x4C, 0xD8, 0x07, 0x02, 0x01, 0xCD, 0xAB, 0xEF, 0xBE, 0x00];
    world.load_field_script(bytecode);
    let _ = world.tick();

    let slot = FIELD_SPAWN_START_SLOT as usize;
    assert!(
        world.actors[slot].active,
        "0x4C 0xD8 should have spawned synchronously into slot {slot}",
    );
    assert_eq!(world.actors[slot].kind, 0xABCD);
    assert_eq!(world.actors[slot].variant, 0xBEEF);
    // 0x4C 0xD8 doesn't carry packet bytes in the bytecode - the
    // record lives in the VDF buffer at runtime - so spawn_record
    // stays `None` until the VDF / global TMD lift lands.
    assert!(world.actors[slot].spawn_record.is_none());

    let evs = world.drain_field_events();
    let spawned: Vec<_> = evs
        .iter()
        .filter_map(|e| match e {
            FieldEvent::ActorSpawned {
                slot: s,
                kind,
                variant,
                record,
            } => Some((*s, *kind, *variant, record.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        spawned,
        vec![(FIELD_SPAWN_START_SLOT, 0xABCDu16, 0xBEEFu16, Vec::new())]
    );
    // No ActorAllocate event - that one is exclusively the
    // queue-based 0x4C 0x80 path.
    assert!(
        !evs.iter()
            .any(|e| matches!(e, FieldEvent::ActorAllocate { .. })),
        "0x4C 0xD8 must not emit ActorAllocate; got {evs:?}"
    );
    // And nothing was queued on the pending_actor_spawns side - the
    // synchronous path doesn't go through the materializer.
    assert!(world.pending_actor_spawns.is_empty());
}

/// `0x4C 0xD8` with a populated VDF buffer should copy the indexed
/// body bytes onto the spawned actor's `spawn_record` (mirror of
/// retail `actor[+0x4C] = VDF_body_ptr`) and surface them in the
/// `ActorSpawned` event payload.
#[test]
fn field_op_4c_d8_with_vdf_buffer_populates_spawn_record() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // VDF buffer with two records:
    //   header:  count = 2
    //   table:   offsets[0] = 12, offsets[1] = 16
    //   body 0:  [0xDE, 0xAD, 0xBE, 0xEF] @ off 12 (4 bytes -> 16)
    //   body 1:  [0xCA, 0xFE, 0xBA, 0xBE, 0x42] @ off 16 (to EOB)
    let mut vdf = Vec::new();
    vdf.extend_from_slice(&2u32.to_le_bytes()); // count
    vdf.extend_from_slice(&12u32.to_le_bytes()); // offsets[0]
    vdf.extend_from_slice(&16u32.to_le_bytes()); // offsets[1]
    vdf.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
    vdf.extend_from_slice(&[0xCA, 0xFE, 0xBA, 0xBE, 0x42]);
    world.set_vdf_buffer(Some(vdf));

    // Sanity-check the lookup helper.
    assert_eq!(
        world.vdf_record_bytes(0),
        Some(&[0xDE, 0xAD, 0xBE, 0xEF][..])
    );
    assert_eq!(
        world.vdf_record_bytes(1),
        Some(&[0xCA, 0xFE, 0xBA, 0xBE, 0x42][..])
    );
    assert_eq!(world.vdf_record_bytes(2), None); // idx >= count

    // `[0x4C, 0xD8, vdf_idx=0x01, tmd=0x0102, kind=0x1111, variant=0x2222, 0x00]`.
    let bytecode = vec![0x4C, 0xD8, 0x01, 0x02, 0x01, 0x11, 0x11, 0x22, 0x22, 0x00];
    world.load_field_script(bytecode);
    let _ = world.tick();

    let slot = FIELD_SPAWN_START_SLOT as usize;
    assert!(world.actors[slot].active);
    assert_eq!(world.actors[slot].kind, 0x1111);
    assert_eq!(world.actors[slot].variant, 0x2222);
    assert_eq!(
        world.actors[slot].spawn_record.as_deref(),
        Some(&[0xCA, 0xFE, 0xBA, 0xBE, 0x42][..]),
        "spawn_record should mirror VDF body 1"
    );

    let evs = world.drain_field_events();
    let spawned: Vec<_> = evs
        .iter()
        .filter_map(|e| match e {
            FieldEvent::ActorSpawned { record, .. } => Some(record.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(spawned, vec![vec![0xCA, 0xFE, 0xBA, 0xBE, 0x42]]);
}

/// `0x4C 0xD8` with a populated global TMD pool should write a
/// matching `Arc<GlobalTmd>` onto the spawned actor's `tmd_ref`
/// (mirror of retail `actor[+0x48] = DAT_8007C018[tmd_idx]`).
/// Indices the pool hasn't seen leave `tmd_ref` at `None` rather
/// than aborting the spawn.
#[test]
fn field_op_4c_d8_with_global_tmd_pool_populates_tmd_ref() {
    let mut world = World::new();
    world.mode = SceneMode::Field;

    // Install a stub TMD at pool slot 5. The Tmd doesn't need to
    // represent realistic mesh data - the host hook only does an
    // Arc::clone and stores the result.
    let stub = std::sync::Arc::new(GlobalTmd {
        tmd: legaia_tmd::Tmd {
            header: legaia_tmd::Header {
                id: 0x8000_0002,
                flags: 1,
                nobj: 0,
                flist_bit_set: true,
            },
            objects: Vec::new(),
        },
        raw: vec![
            0x02, 0x00, 0x00, 0x80, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
    });
    let stub_ptr = std::sync::Arc::as_ptr(&stub);
    world.set_global_tmd(5, stub.clone());

    // `[0x4C, 0xD8, vdf_idx=0x00, tmd=0x0005, kind=0x1111, variant=0x2222, 0x00]`.
    let bytecode = vec![0x4C, 0xD8, 0x00, 0x05, 0x00, 0x11, 0x11, 0x22, 0x22, 0x00];
    world.load_field_script(bytecode);
    let _ = world.tick();

    let slot = FIELD_SPAWN_START_SLOT as usize;
    assert!(world.actors[slot].active);
    let tmd_ref = world.actors[slot]
        .tmd_ref
        .as_ref()
        .expect("tmd_ref should mirror DAT_8007C018[5]");
    assert_eq!(
        std::sync::Arc::as_ptr(tmd_ref),
        stub_ptr,
        "tmd_ref should reference the installed pool entry by Arc identity",
    );

    // A second spawn with an unpopulated index leaves tmd_ref at None.
    let bytecode2 = vec![0x4C, 0xD8, 0x00, 0x09, 0x00, 0x33, 0x33, 0x44, 0x44, 0x00];
    world.load_field_script(bytecode2);
    let _ = world.tick();
    let slot2 = slot + 1;
    assert!(world.actors[slot2].active);
    assert!(
        world.actors[slot2].tmd_ref.is_none(),
        "empty pool slot should not populate tmd_ref",
    );
}

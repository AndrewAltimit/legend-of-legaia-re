//! The actor allocator's initialisation (`FUN_80020DE0`) and the model
//! setter's re-stage (`FUN_80024E08` through `FUN_80020F88`), as the field
//! allocator and op `4C 50` reach them.

use super::*;

#[test]
fn a_reused_slot_does_not_inherit_its_previous_occupant() {
    let mut w = World::new();
    let first = FIELD_SPAWN_START_SLOT as usize;
    // A retired handler actor left in the first auto-spawn slot.
    {
        let a = &mut w.actors[first];
        a.active = false;
        a.handler = legaia_engine_field::actor_handler::ActorHandler::Reflection;
        a.state_54 = 7;
        a.move_state.world_x = 1234;
        a.move_state.flags = 0x0008_0000;
    }
    let slot = w.spawn_field_actor(-1, 0, 3, 4).expect("slot");
    assert_eq!(slot, first);
    let a = &w.actors[slot];
    assert!(a.active);
    assert_eq!(
        a.handler,
        legaia_engine_field::actor_handler::ActorHandler::default()
    );
    assert_eq!(a.state_54, 0);
    assert_eq!(a.move_state.world_x, 0);
    // The engaged bit is gone; bit `2` is the one the allocator ORs onto
    // every descriptor's flags (`ori v0,v0,0x2` at `0x80020ED8`).
    assert_eq!(a.move_state.flags, 2);
    assert_eq!(a.move_state.field_72, 0x1000, "render scale +0x72");
    assert_eq!(a.move_state.field_6a, 0x10, "clip rate +0x6A");
    assert_eq!(a.move_state.field_74, 0x0080_8080, "colour +0x74");
    assert_eq!((a.kind, a.variant), (3, 4));
}

#[test]
fn op_4c_50_on_a_placement_re_stages_its_live_model() {
    let mut w = World::new();
    let mut ctx = FieldCtx::default();
    w.field_vm.executing_channel = Some(5);
    {
        let mut host = FieldHostImpl { world: &mut w };
        match vm::field::step(&mut host, &mut ctx, &[0x4C, 0x50, 0x21, 0x00], 0) {
            FieldStepResult::Advance { next_pc } => assert_eq!(next_pc, 4),
            other => panic!("4C 50 should advance 4 bytes, got {other:?}"),
        }
    }
    assert_eq!(ctx.model_id, 0x21);
    assert_eq!(w.field_npc_live_model(5), Some(0x21));
    // No executing placement (the player, the system): no placement swap.
    w.field_vm.executing_channel = None;
    let mut ctx = FieldCtx::default();
    let mut host = FieldHostImpl { world: &mut w };
    vm::field::step(&mut host, &mut ctx, &[0x4C, 0x50, 0x22, 0x00], 0);
    assert_eq!(w.field_npc_live_model(5), Some(0x21));
}

/// Field op `4C ED` writes the dither word `_DAT_8007BA66` (`sh` at
/// `0x801E351C`) over its boot value of `1` (`0x8001D520`).
#[test]
fn op_4c_ed_writes_the_dither_word() {
    let mut w = World::new();
    assert!(w.toggles.draw_dither);
    for (operand, want) in [(0u8, false), (1, true)] {
        let mut ctx = FieldCtx::default();
        let mut host = FieldHostImpl { world: &mut w };
        match vm::field::step(&mut host, &mut ctx, &[0x4C, 0xED, operand], 0) {
            FieldStepResult::Advance { next_pc } => assert_eq!(next_pc, 3),
            other => panic!("4C ED should advance 3, got {other:?}"),
        }
        assert_eq!(w.toggles.draw_dither, want);
    }
}

/// A `4C D2` re-seat allocates its actor afresh (`FUN_8003A1E4` through
/// `FUN_80020DE0`): the model is the header's again, the tilt and face are
/// zero and the playing clip is `-1`, whatever the retired actor wore.
#[test]
fn a_4c_d2_reseat_draws_the_header_model_not_the_retired_swap() {
    use legaia_engine_field::field_channels::CONTEXT_RETIRE_BIT;
    // Record 0 is the scene controller; record 1 the placement, model 5.
    let man = super::script_teleport::man_bytes_with_placement_records(&[
        [0x00, 0, 0x01, 0x01],
        [0x05, 0, 0x03, 0x04],
    ]);
    let man_file = legaia_scene_formats::man_section::parse(&man).expect("fixture parses");
    let mut w = World::new();
    w.seed_field_channels(&man_file, &man);
    assert!(
        w.field_vm.channels.iter().any(|c| c.placement_index == 1),
        "the placement seats a channel"
    );
    // What the actor wore before it retired: a `4C 50` swap, a tilt, a face
    // row and a playing clip.
    w.set_field_npc_live_model(1, 0x4B);
    w.npcs.clip_bones.insert(1, 3);
    w.npcs.tilts.insert(1, (0x40, -0x40));
    w.npcs.faces.insert(1, (2, 0x800));
    w.npcs.clip_current.insert(1, 7);
    for c in w.field_vm.channels.iter_mut() {
        if c.placement_index == 1 {
            c.ctx.flags |= CONTEXT_RETIRE_BIT;
        }
    }
    w.sweep_field_channel_lifetimes();
    assert!(w.field_placement_retired(1), "the context retired");
    assert_eq!(
        w.field_npc_live_model(1),
        Some(0x4B),
        "a retired actor's swap stays until something re-seats the slot"
    );
    // `4C D2 01` (channel = N0 + index, N0 = 0).
    w.field_vm.pending_placement_respawns.push(1);
    w.sweep_field_channel_lifetimes();
    assert!(!w.field_placement_retired(1), "re-seated");
    assert_eq!(w.field_npc_live_model(1), None, "the header model draws");
    assert!(!w.npcs.clip_bones.contains_key(&1));
    assert!(!w.npcs.tilts.contains_key(&1), "+0x24 / +0x28 zero");
    assert!(!w.npcs.faces.contains_key(&1), "+0x6D / +0x7A zero");
    assert!(!w.npcs.clip_current.contains_key(&1), "+0x5E = -1");
}

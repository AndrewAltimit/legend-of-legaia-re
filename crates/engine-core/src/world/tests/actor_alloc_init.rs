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
        a.handler = crate::actor_handler::ActorHandler::Reflection;
        a.state_54 = 7;
        a.move_state.world_x = 1234;
        a.move_state.flags = 0x0008_0000;
    }
    let slot = w.spawn_field_actor(-1, 0, 3, 4).expect("slot");
    assert_eq!(slot, first);
    let a = &w.actors[slot];
    assert!(a.active);
    assert_eq!(a.handler, crate::actor_handler::ActorHandler::default());
    assert_eq!(a.state_54, 0);
    assert_eq!(a.move_state.world_x, 0);
    assert_eq!(a.move_state.flags, 0);
    assert_eq!(a.move_state.field_72, 0x1000, "render scale +0x72");
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

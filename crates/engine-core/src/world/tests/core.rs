use super::*;

#[test]
fn world_starts_with_inactive_actors() {
    let world = World::new();
    assert_eq!(world.actors.len(), MAX_ACTORS);
    assert!(world.actors.iter().all(|a| !a.active));
}

#[test]
fn actor_vm_spawn_default_runs_through_world() {
    let mut world = World::new();
    // Pre-set default position for actor 7.
    world.actors[7].default_pos = ActorVmPosition::new(100, 50);
    // Bytecode: SpawnDefault actor 7, then End.
    let bc = {
        let mut v = vec![];
        v.extend_from_slice(
            &Insn {
                opcode: 0x01,
                operand_b: 7,
                operand_w: 0,
            }
            .encode(),
        );
        v.extend_from_slice(&[0u8; 4]);
        v
    };
    let pc = world.run_actor_bytecode(&bc).unwrap();
    assert_eq!(pc, 4);
    assert!(world.actors[7].active);
    assert_eq!(world.actors[7].move_state.world_x, 100);
}

#[test]
fn actor_vm_set_field_1d_writes_when_actor_exists() {
    let mut world = World::new();
    world.actors[3].active = true;
    let bc = {
        let mut v = vec![];
        v.extend_from_slice(
            &Insn {
                opcode: 0x03,
                operand_b: 3,
                operand_w: 0xFF42,
            }
            .encode(),
        );
        v.extend_from_slice(&[0u8; 4]);
        v
    };
    world.run_actor_bytecode(&bc).unwrap();
    assert_eq!(world.actors[3].field_1d, 0x42);
}

#[test]
fn move_vm_step_writes_world_state() {
    let mut world = World::new();
    world.actors[0].active = true;
    // Bytecode: WORLD_SET (op 0x07) x=100, y=50, z=10, then HALT.
    let bc: Vec<u16> = vec![0x0007, 100, 50, 10, 0x0008];
    let res = world.step_move_vm(0, &bc);
    // First step is WORLD_SET (Advance), then we'd need to call again for HALT.
    assert!(matches!(res, vm::move_vm::StepResult::Advance));
    assert_eq!(world.actors[0].move_state.world_x, 100);
    assert_eq!(world.actors[0].move_state.world_y, 50);
}

#[test]
fn world_tick_in_battle_mode_runs_state_machine() {
    let mut world = World::new();
    world.mode = SceneMode::Battle;
    // Mark all actors alive so end-of-action doesn't immediately wipe.
    for a in &mut world.actors {
        a.battle.liveness = 1;
    }
    world.battle_ctx.action_state = vm::battle_action::ActionState::Begin.as_byte();
    world.battle_ctx.queued_action = 5;
    let out = world.tick();
    assert!(matches!(out, Some(StepOutcome::Transition { .. })));
    assert_eq!(
        world.battle_ctx.action_state,
        vm::battle_action::ActionState::PreActionWait.as_byte()
    );
}

#[test]
fn world_tick_in_title_mode_returns_none() {
    let mut world = World::new();
    world.mode = SceneMode::Title;
    let out = world.tick();
    assert!(out.is_none());
    assert_eq!(world.frame, 1);
}

#[test]
fn next_rng_is_deterministic() {
    let mut a = World::new();
    let mut b = World::new();
    let seq_a: Vec<_> = (0..10).map(|_| a.next_rng()).collect();
    let seq_b: Vec<_> = (0..10).map(|_| b.next_rng()).collect();
    assert_eq!(seq_a, seq_b);
    // And not all zero.
    assert!(seq_a.iter().any(|&x| x != 0));
}

#[test]
fn battle_party_wipe_signals_end_via_world() {
    let mut world = World::new();
    world.mode = SceneMode::Battle;
    // Kill all party.
    for i in 0..3 {
        world.actors[i].battle.liveness = 0;
    }
    // Mark monsters alive.
    for i in 3..8 {
        world.actors[i].battle.liveness = 1;
    }
    world.battle_ctx.action_state = vm::battle_action::ActionState::EndOfAction.as_byte();
    let out = world.tick();
    assert_eq!(out, Some(StepOutcome::BattleComplete));
    assert_eq!(world.battle_end, Some(BattleEndCause::PartyWipe));
}

#[test]
fn ensure_actor_is_idempotent_and_writes_default_pos() {
    let mut world = World::new();
    world.ensure_actor(2, ActorVmPosition::new(7, 11));
    assert!(world.actors[2].active);
    assert_eq!(world.actors[2].default_pos, ActorVmPosition::new(7, 11));
    // Calling again with new pos updates it but doesn't reset the actor.
    world.actors[2].field_1d = 0xAB;
    world.ensure_actor(2, ActorVmPosition::new(13, 17));
    assert_eq!(world.actors[2].default_pos, ActorVmPosition::new(13, 17));
    assert_eq!(world.actors[2].field_1d, 0xAB);
}

#[test]
fn effect_pool_persists_then_terminates_over_lifetime() {
    let mut world = World::new();
    // Mark slot 0 active by setting child_count > 0 so the tick walker
    // visits it.
    world.effect_pool.master_slots[0].child_count = 4;

    // The effect must survive each work tick until the fixed lifetime
    // budget is spent - it no longer terminates on the first tick.
    let lifetime = vm::effect_vm::DEFAULT_EFFECT_LIFETIME_FRAMES;
    for frame in 1..lifetime {
        world.tick_effects();
        assert_eq!(
            world.effect_pool.master_slots[0].child_count, 4,
            "effect retired early at frame {frame}"
        );
        assert_eq!(world.effect_pool.master_slots[0].field_14, frame as i32);
    }
    // The tick that reaches the budget retires the slot.
    world.tick_effects();
    assert_eq!(world.effect_pool.master_slots[0].child_count, 0);
}

#[test]
fn world_tick_in_field_mode_steps_field_vm() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // Bytecode: 0x37 YIELD. Should set ctx.flags |= 0x400 + advance PC
    // past the yield.
    world.load_field_script(vec![0x37, 0x00]);
    let _ = world.tick();
    assert_eq!(world.field_ctx.flags & 0x400, 0x400, "halt bit set");
    assert!(
        world.field_pc > 0,
        "field_pc should advance after yield, got {}",
        world.field_pc
    );
}

#[test]
fn world_tick_field_mode_no_bytecode_is_noop() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // No bytecode loaded. Tick should not panic and should not advance
    // field_pc.
    let _ = world.tick();
    assert_eq!(world.field_pc, 0);
}

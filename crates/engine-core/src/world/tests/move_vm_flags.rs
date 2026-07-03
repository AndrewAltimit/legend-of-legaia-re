use super::*;

#[test]
fn effect_pool_tick_decrements_state_byte() {
    let mut world = World::new();
    world.effect_pool.master_slots[0].child_count = 4;
    // state >= 8 → write back state - 8 and skip.
    world.effect_pool.master_slots[0].state = 12;
    world.tick_effects();
    assert_eq!(world.effect_pool.master_slots[0].state, 4);
    // Slot still active.
    assert_eq!(world.effect_pool.master_slots[0].child_count, 4);
}

// --- move-VM host wiring (round 5) ------------------------------------

#[test]
fn move_vm_global_predicate_round_trips_through_world() {
    let mut world = World::new();
    world.actors[0].active = true;
    // Move bytecode: 0x2F sub-op 0x08 (set predicate to 1), then HALT.
    world.set_move_bytecode(0, Some(vec![0x002F, 0x0008, 0x0008]));
    let _ = world.step_move_vm(0, &world.move_bytecode[0].clone());
    assert_eq!(
        world.move_predicate, 1,
        "ext sub-op 0x08 should set move_predicate to 1"
    );
}

#[test]
fn move_vm_global_counter_set_and_get() {
    let mut world = World::new();
    world.actors[0].active = true;
    // 0x2F sub-op 0x0F clears counter, then HALT.
    world.move_counter = 5;
    world.set_move_bytecode(0, Some(vec![0x002F, 0x000F, 0x0008]));
    let _ = world.step_move_vm(0, &world.move_bytecode[0].clone());
    assert_eq!(world.move_counter, 0);
}

#[test]
fn move_vm_slot_table_save_and_load_round_trip() {
    let mut world = World::new();
    world.actors[0].active = true;
    world.actors[0].move_state.world_x = 0x1234u16 as i16;
    world.actors[0].move_state.world_y = 0x5678u16 as i16;
    world.actors[0].move_state.world_z = 0x9ABCu16 as i16;
    world.actors[0].move_state.world_y_mirror = 0xDEF0u16 as i16;
    world.actors[0].move_state.field_86 = 0x0003; // slot index = 3
    // 0x2F sub-op 0x11 - save world coords into slot 3, then HALT.
    world.set_move_bytecode(0, Some(vec![0x002F, 0x0011, 0x0008]));
    let _ = world.step_move_vm(0, &world.move_bytecode[0].clone());
    // Verify the bytes landed in slot 3.
    let lo = u32::from_le_bytes(world.move_slot_table[3][0..4].try_into().unwrap());
    let hi = u32::from_le_bytes(world.move_slot_table[3][4..8].try_into().unwrap());
    assert_eq!(lo & 0xFFFF, 0x1234);
    assert_eq!((lo >> 16) & 0xFFFF, 0x5678);
    assert_eq!(hi & 0xFFFF, 0x9ABC);
    assert_eq!((hi >> 16) & 0xFFFF, 0xDEF0);
}

#[test]
fn move_vm_bytecode_write_persists_after_step() {
    let mut world = World::new();
    world.actors[0].active = true;
    world.actors[0].move_state.world_x = 100;
    world.actors[0].move_state.world_y = 200;
    world.actors[0].move_state.world_z = 50;
    // 0x2F sub-op 0x04 - write actor world XYZ to bytecode at
    // pc + op[2] + 3. With pc=0 and op[2]=2, target indices are 5/6/7.
    let bc = vec![
        0x002F, 0x0004, 0x0002, 0xCAFE, 0xCAFE, 0x0000, 0x0000, 0x0000,
    ];
    world.set_move_bytecode(0, Some(bc.clone()));
    let _ = world.step_move_vm(0, &bc);
    // After step, the world's stored bytecode should reflect the writes.
    assert_eq!(world.move_bytecode[0][5], 100u16);
    assert_eq!(world.move_bytecode[0][6], 200u16);
    assert_eq!(world.move_bytecode[0][7], 50u16);
}

#[test]
fn move_vm_bytecode_inplace_add_sees_prior_step_writes() {
    // 0x2F sub-op 0x1E does buffer[pc + op[2] + 4] += op[3].
    // After two consecutive steps each adding 5, the slot should hold 10
    // (proving the world flushes deferred writes between steps).
    let mut world = World::new();
    world.actors[0].active = true;
    // Two 0x1E ops back-to-back, each pointing at the same operand slot.
    // Each op is size 1 (default_arm), so we step it twice.
    // Slot 4 from instruction at pc=0 lands at index 4.
    let bc = vec![0x002F, 0x001E, 0, 5, 0]; // op[2]=0, op[3]=5
    world.set_move_bytecode(0, Some(bc.clone()));
    // First step: bytecode[0 + 0 + 4] (= 0) += 5 → 5.
    let _ = world.step_move_vm(0, &bc);
    assert_eq!(world.move_bytecode[0][4], 5);
    // Step again with a fresh-cloned bytecode read of the world's buffer.
    let bc2 = world.move_bytecode[0].clone();
    // PC has advanced; reset for the same op to fire again.
    world.actors[0].move_state.pc = 0;
    let _ = world.step_move_vm(0, &bc2);
    assert_eq!(
        world.move_bytecode[0][4], 10,
        "second 0x1E should see flushed write from first step"
    );
}

// --- system flag bank (round 6) -------------------------------------

#[test]
fn system_flag_set_and_test_round_trips_through_world() {
    let mut world = World::new();
    world.system_flag_set(0);
    world.system_flag_set(7);
    world.system_flag_set(15);
    world.system_flag_set(255);
    assert!(world.system_flag_test(0));
    assert!(world.system_flag_test(7));
    assert!(world.system_flag_test(15));
    assert!(world.system_flag_test(255));
    assert!(!world.system_flag_test(1));
    assert!(!world.system_flag_test(254));
    // Out-of-bounds idx returns false.
    assert!(!world.system_flag_test(256));
    assert!(!world.system_flag_test(0xFFFF));
}

#[test]
fn system_flag_clear_only_touches_target_bit() {
    let mut world = World::new();
    world.system_flag_set(3);
    world.system_flag_set(4);
    world.system_flag_clear(3);
    assert!(!world.system_flag_test(3));
    assert!(world.system_flag_test(4));
}

#[test]
fn move_vm_ext_query_flag_bank_reads_world_system_flags() {
    let mut world = World::new();
    world.actors[0].active = true;
    world.system_flag_set(42);
    // Bytecode: 0x2F sub-op 0x13 - predicate-true → default_arm (size 1),
    // predicate-false → size 4.
    let bc = vec![0x002F, 0x0013, 42];
    world.set_move_bytecode(0, Some(bc.clone()));
    let _ = world.step_move_vm(0, &bc);
    // Predicate true → PC advanced by 1.
    assert_eq!(world.actors[0].move_state.pc, 1);
    // Now clear and re-run - predicate false → PC += 4.
    world.system_flag_clear(42);
    world.actors[0].move_state.pc = 0;
    let _ = world.step_move_vm(0, &bc);
    assert_eq!(world.actors[0].move_state.pc, 4);
}

#[test]
fn move_vm_ext_set_flag_bank_writes_world_system_flags() {
    let mut world = World::new();
    world.actors[0].active = true;
    // Bytecode: 0x2F sub-op 0x1C - set flag bank (idx = op_w(2)).
    let bc = vec![0x002F, 0x001C, 100];
    world.set_move_bytecode(0, Some(bc.clone()));
    assert!(!world.system_flag_test(100));
    let _ = world.step_move_vm(0, &bc);
    assert!(world.system_flag_test(100));
}

#[test]
fn field_vm_system_flag_set_routes_to_world() {
    // Field-VM 0x5x default-route SET - `[0x50 | nibble, idx_byte]`.
    // idx encoding: `((opcode_byte & 0x8F) << 8) | idx_byte`. For raw
    // opcode 0x50, top bit clear, low nibble 0 → idx = idx_byte.
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.load_field_script(vec![0x50, 42]);
    let _ = world.tick();
    assert!(
        world.system_flag_test(42),
        "0x50 default-route should set system flag 42"
    );
}

#[test]
fn field_vm_system_flag_set_with_low_nibble_includes_high_byte() {
    // 0x52 with low-nibble 2 → idx = (0x02 << 8) | idx_byte.
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.load_field_script(vec![0x52, 7]);
    let _ = world.tick();
    assert!(
        world.system_flag_test(0x0207),
        "0x52 default-route should set system flag 0x0207"
    );
}

#[test]
fn field_vm_system_flag_clear_routes_to_world() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.system_flag_set(99);
    // 0x60 CLEAR with operand 99.
    world.load_field_script(vec![0x60, 99]);
    let _ = world.tick();
    assert!(!world.system_flag_test(99));
}

#[test]
fn field_vm_system_flag_test_takes_jump_when_bit_set() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.system_flag_set(33);
    // 0x70 TEST with idx=33, jump delta = 10.
    world.load_field_script(vec![0x70, 33, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let _ = world.tick();
    // pc was 0; header_size = 1; +1 (idx byte) + delta(10) = 12.
    assert_eq!(world.field_pc, 12);
}

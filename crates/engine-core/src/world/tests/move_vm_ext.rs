use super::*;

#[test]
fn field_vm_extra_flags_op42_reads_world() {
    // Op 0x42 mode=0 - host.extra_flags() & (1 << (op1 & 0x1F)) test.
    // Set bit 5 in extra_flags; op_42 with op1=5 should take the jump.
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.extra_flags = 1 << 5;
    // [0x42, mode=0, op1=5, lo=4, hi=0] - header_size + 4 = 5 byte total
    // for skip path; jump path = pc + header_size + 2 + delta.
    world.load_field_script(vec![0x42, 0, 5, 4, 0]);
    let _ = world.tick();
    // With extra_flags bit 5 set, predicate is true → jump.
    // Jump target = 0 + 1 (header) + 2 + 4 = 7.
    assert_eq!(world.field_pc, 7, "extra_flags-true 0x42 should take jump");
}

#[test]
fn move_vm_ext_set_8007b9d8_writes_world_field() {
    let mut world = World::new();
    world.actors[0].active = true;
    // 0x2F sub-op 0x2F - `_DAT_8007B9D8 = (i32) op[1]`. Note: op[1] in
    // sub-op space = sub-op selector 0x2F itself, op[2] = the value.
    // Per the move_vm port, ext sub-op 0x2F passes op[1] (the sub-op
    // word's "next slot" in the operand stream).
    let bc = vec![0x002F, 0x002F, 0xCAFE];
    world.set_move_bytecode(0, Some(bc.clone()));
    let _ = world.step_move_vm(0, &bc);
    // Whatever the sub-op handler writes, world.move_dat_8007b9d8 should
    // pick up a non-zero value.
    assert_ne!(world.move_dat_8007b9d8, 0);
}

#[test]
fn ext_compute_angle_matches_quadrant_when_player_set() {
    // Place actor at origin, player due-east; angle should be ~0 mod 4096
    // (positive X direction = angle 0 in the dz.atan2(dx) convention).
    let mut world = World::new();
    world.actors[0].active = true;
    world.actors[0].move_state.world_x = 0;
    world.actors[0].move_state.world_z = 0;
    world.actors[1].active = true;
    world.actors[1].move_state.world_x = 100;
    world.actors[1].move_state.world_z = 0;
    world.player_actor_slot = Some(1);
    // Drive ext sub-op 0x3A: VM writes the angle into bytecode at
    // `state.pc + op_w(2) + 3`. With pc=0 and op_w(2)=0, dst = u16[3].
    let bc = vec![0x002F, 0x003A, 0, 0xFFFF, 0xFFFF];
    world.set_move_bytecode(0, Some(bc.clone()));
    let _ = world.step_move_vm(0, &bc);
    // angle 0 (player due-east) should produce ~0 in the dst slot.
    assert_eq!(
        world.move_bytecode[0][3], 0,
        "angle to due-east player should be 0"
    );
}

#[test]
fn ext_compute_angle_returns_zero_when_no_player() {
    // No player slot designated → ext_compute_angle returns 0.
    let mut world = World::new();
    world.actors[0].active = true;
    let bc = vec![0x002F, 0x003A, 0, 0xFFFF];
    world.set_move_bytecode(0, Some(bc.clone()));
    let _ = world.step_move_vm(0, &bc);
    assert_eq!(world.move_bytecode[0][3], 0);
}

#[test]
fn ext_party_member_lookup_returns_table_position() {
    let mut world = World::new();
    world.actors[0].active = true;
    // Party member at index 1 = world actor slot 5 with a known position.
    world.actors[5].active = true;
    world.actors[5].move_state.world_x = 100;
    world.actors[5].move_state.world_y = 50;
    world.actors[5].move_state.world_z = 200;
    world.party_actor_slots = vec![None, Some(5), None];
    // Sub-op 0x3B: dst = pc + op_w(3) + 4. We use op_w(2)=1 (party slot 1)
    // and op_w(3)=0 so dst = u16[4..7].
    let bc = vec![
        0x002F, 0x003B, 0x0001, 0x0000, 0xAAAA, 0xAAAA, 0xAAAA, 0xAAAA,
    ];
    world.set_move_bytecode(0, Some(bc.clone()));
    let _ = world.step_move_vm(0, &bc);
    assert_eq!(world.move_bytecode[0][4], 100u16);
    assert_eq!(world.move_bytecode[0][5], 50u16);
    assert_eq!(world.move_bytecode[0][6], 200u16);
}

#[test]
fn ext_party_member_lookup_skips_when_none() {
    // No party table entry → 0x3B returns size-4 (skip), pre-clears dst.
    let mut world = World::new();
    world.actors[0].active = true;
    let bc = vec![0x002F, 0x003B, 0x0000, 0x0000, 0xAAAA, 0xAAAA, 0xAAAA];
    world.set_move_bytecode(0, Some(bc.clone()));
    let _ = world.step_move_vm(0, &bc);
    // Dst slots pre-cleared even when lookup returns None.
    assert_eq!(world.move_bytecode[0][4], 0);
    assert_eq!(world.move_bytecode[0][5], 0);
    assert_eq!(world.move_bytecode[0][6], 0);
}

#[test]
fn ext_fade_color_records_pending_request() {
    let mut world = World::new();
    world.actors[0].active = true;
    // Sub-op 0x3C: r=0xAB, g=0xCD, b=0xEF, ticks=4 (ramp).
    let bc = vec![0x002F, 0x003C, 0x00AB, 0x00CD, 0x00EF, 0x0004];
    world.set_move_bytecode(0, Some(bc.clone()));
    let _ = world.step_move_vm(0, &bc);
    assert_eq!(
        world.pending_fade,
        Some(FadeRequest {
            rgb: [0xAB, 0xCD, 0xEF],
            ticks: 4
        })
    );
}

#[test]
fn move_player_world_xyz_reads_designated_player_slot() {
    let mut world = World::new();
    world.actors[2].active = true;
    world.actors[2].move_state.world_x = 100;
    world.actors[2].move_state.world_y = 200;
    world.actors[2].move_state.world_z = 300;
    world.player_actor_slot = Some(2);
    // No direct API to read move_player_world_xyz; verify by stepping
    // sub-op 0x39 (squared-distance "inside radius" predicate). With
    // actor 0 at origin and player at (100, _, 300), dist_sq = 100²+300² =
    // 100000 - predicate fails for r=10 (r² = 100), passes for r=400
    // (r² = 160000).
    world.actors[0].active = true;
    // Predicate fail → PC += 4.
    let bc = vec![0x002F, 0x0039, 10, 0, 0, 0];
    world.set_move_bytecode(0, Some(bc.clone()));
    let _ = world.step_move_vm(0, &bc);
    assert_eq!(
        world.actors[0].move_state.pc, 4,
        "small-radius 0x39 should fail"
    );
    // Predicate pass → PC += 1.
    world.actors[0].move_state.pc = 0;
    let bc2 = vec![0x002F, 0x0039, 400, 0, 0, 0];
    world.set_move_bytecode(0, Some(bc2.clone()));
    let _ = world.step_move_vm(0, &bc2);
    assert_eq!(
        world.actors[0].move_state.pc, 1,
        "large-radius 0x39 should pass"
    );
}

// --- Field-event emission ---------------------------------------------

/// Op 0x35 sub-1 (start BGM) emits `FieldEvent::Bgm` and pins
/// `current_bgm`. Encoding: `[0x35, lo, hi, sub_op]`.
#[test]
fn field_op_35_sub1_emits_bgm_event_and_pins_current() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // text_id = 0x42 (LE), sub_op = 1 (start field BGM).
    let bytecode = vec![0x35, 0x42, 0x00, 0x01];
    world.load_field_script(bytecode);
    let _ = world.tick();
    let evs = world.drain_field_events();
    assert!(
        evs.iter().any(|e| matches!(
            e,
            FieldEvent::Bgm {
                sub_op: 1,
                text_id: 0x42
            }
        )),
        "expected Bgm event, got {evs:?}"
    );
    assert_eq!(world.current_bgm, Some(0x42));
}

/// Op 0x3F is the **named scene-change** (not dialog): it stages a pending
/// named scene transition from the inline destination name. Encoding:
/// `[0x3F, idx_lo, idx_hi, name_len, <name bytes>, entry_x, entry_z, dir]`.
#[test]
fn field_op_3f_stages_named_scene_transition() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // idx = 60, name_len = 4 ("dolk"), entry_x = 0x01, entry_z = 0x02, dir = 0x03.
    let mut bytecode = vec![0x3F, 60, 0x00, 4];
    bytecode.extend_from_slice(b"dolk");
    bytecode.extend_from_slice(&[0x01, 0x02, 0x03]);
    world.load_field_script(bytecode);
    let _ = world.tick();
    assert_eq!(
        world.pending_named_scene_transition,
        Some(("dolk".to_string(), 0x01, 0x02, 0x03)),
        "0x3F must stage a named scene transition to the inline destination"
    );
    // It is NOT a dialog opener.
    assert!(
        world.current_dialog.is_none(),
        "0x3F must not open a dialog box"
    );
}

/// A 0x3F whose inline "name" is a text-desync phantom (non-CDNAME bytes)
/// stages no transition but still advances the PC.
#[test]
fn field_op_3f_rejects_phantom_name() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    let mut bytecode = vec![0x3F, 0x00, 0x00, 4];
    bytecode.extend_from_slice(b"Hi! ");
    bytecode.extend_from_slice(&[0x00, 0x00, 0x00]);
    world.load_field_script(bytecode);
    let _ = world.tick();
    assert!(world.pending_named_scene_transition.is_none());
}

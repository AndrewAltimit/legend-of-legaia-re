use super::*;

#[test]
fn collect_sprite_requests_emits_one_per_active_actor_with_frame() {
    let mut world = World::default();
    // Slot 0: active + sprite frame at (10, 20) world coords.
    world.actors[0].active = true;
    world.actors[0].move_state.world_x = 100;
    world.actors[0].move_state.world_z = 200;
    world.set_actor_sprite(
        0,
        Some(SpriteFrame {
            atlas_src: (0, 0, 16, 24),
            tint: [1.0, 1.0, 1.0, 1.0],
            anchor_y: -8,
        }),
    );
    // Slot 1: active but no frame - shouldn't emit.
    world.actors[1].active = true;
    // Slot 2: frame but inactive - shouldn't emit.
    world.set_actor_sprite(
        2,
        Some(SpriteFrame {
            atlas_src: (16, 0, 16, 24),
            tint: [1.0; 4],
            anchor_y: 0,
        }),
    );

    let requests = world.collect_sprite_requests();
    assert_eq!(requests.len(), 1);
    let r = &requests[0];
    assert_eq!(r.actor_slot, 0);
    assert_eq!(r.world_x, 100);
    // anchor_y subtracts from world_z (z + (-8)) = 192.
    assert_eq!(r.world_y, 192);
    assert_eq!(r.atlas_src, (0, 0, 16, 24));
}

#[test]
fn set_actor_sprite_with_none_clears_existing_frame() {
    let mut world = World::default();
    world.actors[0].active = true;
    world.set_actor_sprite(
        0,
        Some(SpriteFrame {
            atlas_src: (0, 0, 8, 8),
            ..Default::default()
        }),
    );
    assert!(world.actors[0].sprite_frame.is_some());
    world.set_actor_sprite(0, None);
    assert!(world.actors[0].sprite_frame.is_none());
}

#[test]
fn load_field_record_skips_frame_divider_sentinel() {
    let mut world = World::new();
    // Record opens with FFFF 0000 frame divider.
    let record = vec![0xFF, 0xFF, 0x00, 0x00, 0x37, 0x00];
    world.load_field_record(&record);
    assert_eq!(world.field_pc, 4, "frame divider should bump pc to 4");
    assert_eq!(world.field_bytecode.len(), 6);
}

#[test]
fn load_field_record_without_sentinel_starts_at_zero() {
    let mut world = World::new();
    let record = vec![0x37, 0x00];
    world.load_field_record(&record);
    assert_eq!(world.field_pc, 0);
}

/// Field VM op 0x3E with `op0 >= 100` is the scene-transition arm
/// (`map_id = op0 - 100`). The world's `FieldHostImpl` records the
/// request in `pending_scene_transition` for `SceneHost::tick` to
/// drain on the next frame boundary.
#[test]
fn field_scene_transition_writes_pending_map_id() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // Bytecode: opcode 0x3E, op0 = 105 (map_id 5), then 4 padding
    // bytes (op0 + 4 trailing operand bytes per the dispatcher math).
    let bytecode = vec![0x3E, 105, 0, 0, 0, 0];
    world.load_field_script(bytecode);
    let _ = world.tick();
    assert_eq!(world.pending_scene_transition, Some(5));
}

/// `op0 < 100` is the field_interact arm - should NOT trigger a
/// scene transition.
#[test]
fn field_op_3e_low_op0_does_not_request_scene_transition() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    let bytecode = vec![0x3E, 50, 7];
    world.load_field_script(bytecode);
    let _ = world.tick();
    assert_eq!(world.pending_scene_transition, None);
}

/// Field-VM op `0x4C 0xE2` (FMV trigger) records the FMV index in
/// `World::pending_fmv_trigger` AND emits a `FieldEvent::FmvTrigger`
/// for engines to drain. Retail handler at `0x801E30E4` writes the
/// s16 to `_DAT_8007BA78` and pokes next-game-mode = 0x1A; the
/// world mirrors the request via these two channels.
#[test]
fn field_op_4c_e2_records_pending_fmv_trigger() {
    use crate::cutscene::{STR_INIT_GAME_MODE, fmv_index_to_str_filename};
    use crate::field_events::FieldEvent;

    let mut world = World::new();
    world.mode = SceneMode::Field;
    // `[0x4C, 0xE2, 0x03, 0x00, 0, 0]` → fmv_id 3 → MV4.STR.
    let bytecode = vec![0x4C, 0xE2, 0x03, 0x00, 0, 0];
    world.load_field_script(bytecode);
    let _ = world.tick();
    assert_eq!(world.pending_fmv_trigger, Some(3));
    let events = world.drain_field_events();
    assert!(events.contains(&FieldEvent::FmvTrigger { fmv_id: 3 }));
    assert_eq!(fmv_index_to_str_filename(3), Some("MOV/MV4.STR"));
    assert_eq!(STR_INIT_GAME_MODE, 26);
}

/// The FMV trigger transitions Field → Cutscene one frame later (retail's
/// main dispatcher reads the next-game-mode global the frame after the
/// field-VM op writes it), exposes the active FMV + its `MV*.STR` path,
/// and suspends the field VM while it plays. `finish_cutscene` returns to
/// the field.
#[test]
fn field_fmv_trigger_drives_field_cutscene_field_flow() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // fmv_id 3 → MV4.STR (a playable slot).
    world.load_field_script(vec![0x4C, 0xE2, 0x03, 0x00, 0, 0]);

    // Frame 1: op fires, records the pending trigger; still in Field.
    let _ = world.tick();
    assert_eq!(world.mode, SceneMode::Field);
    assert_eq!(world.pending_fmv_trigger, Some(3));
    assert_eq!(world.active_fmv(), None);

    // Frame 2: the pending trigger is consumed at the top of the tick and
    // the world flips into the cutscene mode for the resolved FMV.
    let _ = world.tick();
    assert_eq!(world.mode, SceneMode::Cutscene);
    assert_eq!(world.pending_fmv_trigger, None);
    assert_eq!(world.active_fmv(), Some(3));
    assert_eq!(world.active_fmv_str_filename(), Some("MOV/MV4.STR"));

    // While the FMV plays the field VM is suspended (no further field
    // stepping); ticking keeps the world in Cutscene until the host ends
    // playback.
    let _ = world.tick();
    assert_eq!(world.mode, SceneMode::Cutscene);
    assert_eq!(world.active_fmv(), Some(3));

    // Host signals playback complete → back to the field.
    world.finish_cutscene();
    assert_eq!(world.mode, SceneMode::Field);
    assert_eq!(world.active_fmv(), None);
}

/// An FMV id whose runtime slot points at a cut/missing path is drained
/// without entering the cutscene mode - the engine treats it as a no-op
/// and the field keeps running.
#[test]
fn field_fmv_trigger_cut_path_is_a_noop() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // fmv_id 7 → slots 5..=11 are dev-only cut paths (no retail STR).
    world.load_field_script(vec![0x4C, 0xE2, 0x07, 0x00, 0, 0]);

    let _ = world.tick(); // op fires
    assert_eq!(world.pending_fmv_trigger, Some(7));
    let _ = world.tick(); // pending consumed
    assert_eq!(
        world.mode,
        SceneMode::Field,
        "cut path does not enter cutscene"
    );
    assert_eq!(world.pending_fmv_trigger, None, "pending still drained");
    assert_eq!(world.active_fmv(), None);
}

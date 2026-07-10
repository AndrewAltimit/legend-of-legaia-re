//! Round 18/19/20 coverage: 0x4C n8 actor-allocator, nE camera, nD/n5 dialog, VRAM STP, STATE_RESUME-entangled sub-cases. Extracted verbatim from `field/tests.rs`.

use super::*;

// -- Round 18 - 0x4C n8 actor-allocator + nE camera + nD/n5 dialog ---

#[test]
fn op_4c_n_8_sub_1_set_model_anim_advances_pc_by_9() {
    // [4C, 0x81, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE]
    // model_id (LE24) = 0x563412
    // anim_frame (LE16) = 0x9A78
    // tween_frames (LE16) = 0xDEBC
    let bytecode = [0x4Cu8, 0x81, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 9 });
    assert_eq!(host.n_8_sub_1_set_model_calls.len(), 1);
    let (model_id, anim_frame, tween_frames) = host.n_8_sub_1_set_model_calls[0];
    assert_eq!(model_id, 0x0056_3412);
    assert_eq!(anim_frame, 0x9A78);
    assert_eq!(tween_frames, 0xDEBC);
}

#[test]
fn op_4c_n_8_sub_1_truncated_buffer_returns_unknown() {
    // Only 8 bytes - sub-1 needs 9 total.
    let bytecode = [0x4Cu8, 0x81, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
    assert!(host.n_8_sub_1_set_model_calls.is_empty());
}

#[test]
fn op_4c_n_8_sub_6_actor_set_rotation_advances_15() {
    // [4C, 0x86, ... 12 bytes for 6 LE16 ..., actor_id]
    let bytecode = [
        0x4Cu8, 0x86, // opcode + sub-op
        0x10, 0x00, 0x20, 0x00, 0x30, 0x00, // x, y, z = 0x10, 0x20, 0x30
        0x40, 0x00, 0x50, 0x00, 0x60, 0x00, // rx, ry, rz = 0x40, 0x50, 0x60
        0x07, // actor_id = 7
    ];
    let mut host = TestHost {
        n_8_sub_6_actor_present: true,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 15 });
    assert_eq!(host.n_8_sub_6_actor_set_rotation_calls.len(), 1);
    let (actor_id, position, rotation) = host.n_8_sub_6_actor_set_rotation_calls[0];
    assert_eq!(actor_id, 7);
    assert_eq!(position, [0x10, 0x20, 0x30]);
    assert_eq!(rotation, [0x40, 0x50, 0x60]);
}

#[test]
fn op_4c_n_8_sub_6_advances_15_even_when_actor_missing() {
    // The original short-circuits to `return param_2 + 0xF` when the
    // actor lookup fails - PC still advances by 15.
    let bytecode = [
        0x4Cu8, 0x86, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF,
    ];
    let mut host = TestHost {
        n_8_sub_6_actor_present: false, // actor lookup fails
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 15 });
    // Hook still fires (the host records the call); the actor_present
    // bool just controls the return value.
    assert_eq!(host.n_8_sub_6_actor_set_rotation_calls.len(), 1);
}

#[test]
fn op_4c_n_8_sub_6_signed_decode_round_trip() {
    // Negative LE16 should decode through `as i16` correctly.
    let bytecode = [
        0x4Cu8, 0x86, 0xFF, 0xFF, // x = -1
        0x00, 0x80, // y = -32768
        0xFE, 0xFF, // z = -2
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // rx/ry/rz = 0
        0x00,
    ];
    let mut host = TestHost {
        n_8_sub_6_actor_present: true,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    step(&mut host, &mut ctx, &bytecode, 0);
    let (_, position, _) = host.n_8_sub_6_actor_set_rotation_calls[0];
    assert_eq!(position, [-1, -32768, -2]);
}

#[test]
fn op_4c_n_8_sub_b_jumps_when_actor_type_present() {
    // [4C, 0x8B, type=0x12, target_lo=0x34, target_hi=0x12] → jump 0x1234
    let bytecode = [0x4Cu8, 0x8B, 0x12, 0x34, 0x12];
    let mut host = TestHost::default();
    host.n_8_sub_b_present_types.insert(0x12);
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x1234 });
}

#[test]
fn op_4c_n_8_sub_b_advances_5_when_no_actor() {
    let bytecode = [0x4Cu8, 0x8B, 0x12, 0x34, 0x12];
    let mut host = TestHost::default(); // no types registered
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
}

#[test]
fn op_4c_n_8_sub_b_truncated_buffer_returns_unknown() {
    let bytecode = [0x4Cu8, 0x8B, 0x12, 0x34]; // missing target hi byte
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
}

#[test]
fn op_4c_n_8_sub_d_empty_slot_advances_6() {
    let bytecode = [0x4Cu8, 0x8D, 0x05, 0xAB, 0x34, 0x12];
    let host_state = ActorSearchResult::EmptySlot;
    let mut host = TestHost {
        n_8_sub_d_search_result: host_state,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    let queries = host.n_8_sub_d_queries.borrow();
    assert_eq!(queries.as_slice(), &[(0x05, 0xAB)]);
}

#[test]
fn op_4c_n_8_sub_d_found_jumps_to_le_target() {
    let bytecode = [0x4Cu8, 0x8D, 0x05, 0xAB, 0x78, 0x56];
    let mut host = TestHost {
        n_8_sub_d_search_result: ActorSearchResult::Found,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x5678 });
}

#[test]
fn op_4c_n_8_sub_d_no_match_halts_at_pc() {
    let bytecode = [0x4Cu8, 0x8D, 0x05, 0xAB, 0x34, 0x12];
    let mut host = TestHost {
        n_8_sub_d_search_result: ActorSearchResult::NoMatch,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}

#[test]
fn op_4c_n_8_sub_d_truncated_buffer_returns_unknown() {
    let bytecode = [0x4Cu8, 0x8D, 0x05, 0xAB, 0x34]; // missing target hi
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
}

#[test]
fn op_4c_n_e_sub_3_advances_3_and_records_actor() {
    let bytecode = [0x4Cu8, 0xE3, 0x09];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    // Raw asm (0x801E3108): both exit paths advance s8 by 3 (the
    // player path in the `j 0x801E00BC` branch-delay slot).
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(host.n_e_sub_3_camera_syncs, vec![9]);
}

#[test]
fn op_4c_n_e_sub_3_truncated_returns_unknown() {
    let bytecode = [0x4Cu8, 0xE3]; // missing actor_id
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
    assert!(host.n_e_sub_3_camera_syncs.is_empty());
}

#[test]
fn op_4c_n_e_sub_7_camera_animate_decodes_le24_then_le16() {
    // 7-byte instruction: [opcode, sub-op, t0, t1, t2, d0, d1].
    // target = LE24(0x12, 0x34, 0x56) = 0x563412
    // duration = LE16(0xEF, 0xCD) = 0xCDEF
    let bytecode = [0x4Cu8, 0xE7, 0x12, 0x34, 0x56, 0xEF, 0xCD];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
    assert_eq!(host.n_e_sub_7_camera_animates, vec![(0x0056_3412, 0xCDEF)]);
}

#[test]
fn op_4c_n_e_sub_7_truncated_buffer_returns_unknown() {
    // 6 bytes - last byte is missing.
    let bytecode = [0x4Cu8, 0xE7, 0x12, 0x34, 0x56, 0xEF];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
}

#[test]
fn op_4c_n_e_sub_8_camera_zoom_decodes_four_le16() {
    // 10-byte instruction: [opcode, sub-op, x0, x1, y0, y1, z0, z1, m0,
    // m1]. zoom_x = LE16(0x40, 0x00) = 0x40, zoom_y = 0x08, zoom_z = 4,
    // mode = 0 (the default-zoom triplet at line 7315-7317 of the
    // dispatcher dump).
    let bytecode = [0x4Cu8, 0xE8, 0x40, 0x00, 0x08, 0x00, 0x04, 0x00, 0x00, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 10 });
    assert_eq!(host.n_e_sub_8_camera_zooms, vec![(0x40, 0x08, 0x04, 0)]);
}

#[test]
fn op_4c_n_e_sub_8_camera_zoom_signed_mode_round_trip() {
    // Mode is i16 - verify negative / sign-bit values flow through.
    let bytecode = [0x4Cu8, 0xE8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(host.n_e_sub_8_camera_zooms, vec![(0, 0, 0, -1)]);
}

#[test]
fn op_4c_n_e_sub_8_truncated_buffer_returns_unknown() {
    // 9 bytes - needs 10.
    let bytecode = [0x4Cu8, 0xE8, 0x40, 0x00, 0x08, 0x00, 0x04, 0x00, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
}

#[test]
fn op_4c_n_d_sub_0_field_se_trigger_advances_6() {
    // [4C, 0xD0, 0x34, 0x12, 0x78, 0x56] → a = 0x1234, b = 0x5678.
    let bytecode = [0x4Cu8, 0xD0, 0x34, 0x12, 0x78, 0x56];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.n_d_sub_0_se_triggers, vec![(0x1234, 0x5678)]);
}

#[test]
fn op_4c_n_d_sub_0_truncated_buffer_returns_unknown() {
    let bytecode = [0x4Cu8, 0xD0, 0x34, 0x12, 0x78]; // 5 bytes - needs 6
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
}

#[test]
fn op_4c_n_5_sub_3_dialog_wait_halts_at_pc_plus_2() {
    // [4C, 0x53] → halt-style return with PC = 0 + 2 = 2.
    let bytecode = [0x4Cu8, 0x53];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 2 });
    assert_eq!(host.n_5_sub_3_dialog_waits, 1);
}

#[test]
fn op_4c_n_5_sub_4_dialog_advance_advances_when_done() {
    // Default: dialog_active = false → advance PC by 2.
    let bytecode = [0x4Cu8, 0x54];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.n_5_sub_4_polls, 1);
}

#[test]
fn op_4c_n_5_sub_4_dialog_advance_halts_when_active() {
    // dialog_active = true → halt at PC.
    let bytecode = [0x4Cu8, 0x54];
    let mut host = TestHost {
        n_5_sub_4_dialog_active: true,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(host.n_5_sub_4_polls, 1);
}

#[test]
fn op_4c_n_e_sub_4_uses_shared_tile_center_helper() {
    // Verifies the round-18 tile_center helper is wired in: 0x10 → 0x840,
    // 0x90 → 0x880, 0x00 → 0. This is the same case the round-17 inline
    // closure verified - confirm round-18's lift to a shared helper
    // didn't change semantics.
    let bytecode = [0x4Cu8, 0xE4, 0x10, 0x90, 0x00, 0x10, 0x00, 0x00, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    step(&mut host, &mut ctx, &bytecode, 0);
    let bboxes = host.n_e_sub_4_bboxes.borrow();
    assert_eq!(bboxes.len(), 1);
    assert_eq!(bboxes[0], [0x840, 0x880, 0, 0x840]);
}

#[test]
fn op_4c_n_d_sub_4_vram_stp_set_advances_6_bytes() {
    // [4C, 0xD4, x_lo, x_hi, y_lo, y_hi] = 6 bytes; original returns
    // iVar47 + 6. next_pc = 0 + 1 (header_size) + 5 = 6.
    let bytecode = [0x4Cu8, 0xD4, 0x80, 0x00, 0xEF, 0x01]; // x=0x0080, y=0x01EF
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.n_d_sub_4_vram_stp_set_calls, vec![(0x0080, 0x01EF)]);
}

#[test]
fn op_4c_n_d_sub_5_vram_stp_clear_advances_6_bytes() {
    // Sister of sub-4 with STP-clear semantics; same 6-byte encoding.
    let bytecode = [0x4Cu8, 0xD5, 0xC0, 0x01, 0x10, 0x00]; // x=0x01C0, y=0x0010
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.n_d_sub_5_vram_stp_clear_calls, vec![(0x01C0, 0x0010)]);
}

#[test]
fn op_4c_n_e_sub_f_halts_at_pc() {
    // Sub-F has no case in the original; falls through to halt.
    let bytecode = [0x4Cu8, 0xEF];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}

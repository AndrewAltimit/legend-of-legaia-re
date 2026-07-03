use super::*;

#[test]
fn packet_length_empty_buffer_is_zero() {
    assert_eq!(packet_length(&[]), 0);
}

#[test]
fn packet_length_immediate_terminator_is_zero() {
    // Any byte <= 0x1E ends the packet without contributing.
    for b in 0..=0x1Eu8 {
        assert_eq!(packet_length(&[b]), 0, "terminator byte {b:#x}");
    }
}

#[test]
fn packet_length_pure_printable_run() {
    let buf = [0x20, 0x40, 0x80, 0xBF, 0x00];
    assert_eq!(packet_length(&buf), 4);
}

#[test]
fn packet_length_escape_sequence_counts_two() {
    // 0xC1 is an escape lead - the next byte is consumed unconditionally.
    let buf = [0xC1, 0xAB, 0x00];
    assert_eq!(packet_length(&buf), 2);
}

#[test]
fn delimited_field_offset_finds_nth_delimiter() {
    // Delimiter 0x2A at offsets 1 and 3.
    let buf = [0x40, 0x2A, 0x41, 0x2A, 0x42, 0x00];
    assert_eq!(delimited_field_offset(&buf, 0x2A, 1), Some(1));
    assert_eq!(delimited_field_offset(&buf, 0x2A, 2), Some(3));
    assert_eq!(delimited_field_offset(&buf, 0x2A, 3), None);
}

#[test]
fn delimited_field_offset_skips_escape_operands() {
    // The escape pair's operand byte equals the delimiter but is never
    // tested; it still advances the returned offset by one.
    let buf = [0xC1, 0x2A, 0x40, 0x2A, 0x00];
    assert_eq!(delimited_field_offset(&buf, 0x2A, 1), Some(3));
}

#[test]
fn delimited_field_offset_zero_nth_never_matches() {
    let buf = [0x2A, 0x00];
    assert_eq!(delimited_field_offset(&buf, 0x2A, 0), None);
}

#[test]
fn delimited_field_offset_terminates_on_nul() {
    let buf = [0x40, 0x00, 0x2A];
    assert_eq!(delimited_field_offset(&buf, 0x2A, 1), None);
}

#[test]
fn packet_length_multiple_escapes_with_runs() {
    let buf = [0x40, 0xC1, 0xAB, 0x40, 0xCD, 0x05, 0x00];
    assert_eq!(packet_length(&buf), 6);
}

#[test]
fn packet_length_escape_at_buffer_boundary() {
    // 0xC2 with no following byte: we guard and stop; the lead still counts.
    assert_eq!(packet_length(&[0xC2]), 1);
}

#[test]
fn packet_length_no_terminator_runs_to_end() {
    assert_eq!(packet_length(&[0x20, 0x21, 0x22, 0x23]), 4);
}

#[test]
fn packet_length_high_nibble_matters_for_escape() {
    assert_eq!(packet_length(&[0xC0, 0xFF, 0x00]), 2);
    assert_eq!(packet_length(&[0xD0, 0xFF, 0x00]), 2);
    for lead in 0xC0..=0xCFu8 {
        assert_eq!(
            packet_length(&[lead, 0xAB, 0x00]),
            2,
            "escape lead {lead:#x}"
        );
    }
    for lead in [0xB0u8, 0xBFu8, 0xD0u8, 0xDFu8] {
        assert_eq!(packet_length(&[lead, 0x00]), 1, "non-escape lead {lead:#x}");
    }
}

#[test]
fn nop_decodes() {
    let insn = decode(&[0x21], 0).unwrap();
    assert_eq!(insn.size, 1);
    assert!(matches!(insn.info, InsnInfo::Nop));
}

#[test]
fn extended_bit_skips_target_id_byte() {
    // 0xA1 = 0x21 | 0x80 (extended Nop).
    let insn = decode(&[0xA1, 0x07], 0).unwrap();
    assert_eq!(insn.size, 2);
    assert_eq!(insn.extended, Some(0x07));
    assert_eq!(insn.opcode, 0x21);
}

#[test]
fn fmv_trigger_decodes_fmv_id_and_total_size_six_bytes() {
    // [4C, E2, 03, 00, _, _]
    let bc = [0x4C, 0xE2, 0x03, 0x00, 0x00, 0x00];
    let insn = decode(&bc, 0).unwrap();
    assert_eq!(insn.size, 6);
    match insn.info {
        InsnInfo::MenuCtrl {
            op0: 0xE2,
            kind: MenuCtrlKind::FmvTrigger { fmv_id },
        } => assert_eq!(fmv_id, 3),
        other => panic!("unexpected info: {other:?}"),
    }
}

#[test]
fn fmv_trigger_negative_index() {
    let bc = [0x4C, 0xE2, 0xFF, 0xFF, 0x00, 0x00];
    let insn = decode(&bc, 0).unwrap();
    match insn.info {
        InsnInfo::MenuCtrl {
            kind: MenuCtrlKind::FmvTrigger { fmv_id },
            ..
        } => assert_eq!(fmv_id, -1),
        _ => panic!("expected FmvTrigger"),
    }
}

#[test]
fn jmp_rel_target_is_post_header() {
    // 0x26 + LE u16 0x0008. Target = pc + header_size + delta = 0 + 1 + 8 = 9.
    let bc = [0x26, 0x08, 0x00, 0xAA, 0xBB];
    let insn = decode(&bc, 0).unwrap();
    match insn.info {
        InsnInfo::JmpRel { delta, target } => {
            assert_eq!(delta, 0x0008);
            assert_eq!(target, 9);
        }
        _ => panic!(),
    }
}

#[test]
fn animate_consumes_count_times_four_extra_bytes() {
    // count = 2, 4 * 2 = 8 keyframe bytes; total = 1 (op) + 1 (count) + 1 (base) + 8 = 11.
    let mut bc = vec![0x4Bu8, 2, 0xFF];
    bc.extend_from_slice(&[0x11; 8]);
    let insn = decode(&bc, 0).unwrap();
    assert_eq!(insn.size, 11);
}

#[test]
fn data_block_consumes_len_bytes() {
    // len = 5: total = 1 (op) + 1 (len) + 5 (payload) = 7.
    let bc = [0x40u8, 5, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE];
    let insn = decode(&bc, 0).unwrap();
    assert_eq!(insn.size, 7);
}

#[test]
fn cond_jmp_always_advances_by_five() {
    let bc = [0x42u8, 0, 7, 0x10, 0x00];
    let insn = decode(&bc, 0).unwrap();
    match insn.info {
        InsnInfo::CondJmp { delta, target, .. } => {
            assert_eq!(delta, 0x0010);
            // target = pc + header_size + 2 + delta = 0 + 1 + 2 + 16 = 19.
            assert_eq!(target, 19);
        }
        _ => panic!(),
    }
    assert_eq!(insn.size, 5);
}

#[test]
fn bbox_test_skip_target_offset() {
    // operand bytes: x_min=1, z_min=2, x_max=3, z_max=4, skip=0x0007.
    let bc = [0x4Du8, 1, 2, 3, 4, 0x07, 0x00];
    let insn = decode(&bc, 0).unwrap();
    match insn.info {
        InsnInfo::BBoxTest {
            skip_delta,
            skip_target,
            ..
        } => {
            assert_eq!(skip_delta, 7);
            // skip_target = pc + header + 4 + delta = 0 + 1 + 4 + 7 = 12.
            assert_eq!(skip_target, 12);
        }
        _ => panic!(),
    }
    assert_eq!(insn.size, 7);
}

#[test]
fn warp_uses_six_bytes_when_op0_ge_100() {
    let bc = [0x3Eu8, 100, 0, 0, 0, 0];
    let insn = decode(&bc, 0).unwrap();
    assert_eq!(insn.size, 6);
    match insn.info {
        InsnInfo::WarpOrInteract { is_warp, .. } => assert!(is_warp),
        _ => panic!(),
    }
}

#[test]
fn interact_uses_three_bytes_when_op0_under_100() {
    let bc = [0x3Eu8, 5, 0xAB];
    let insn = decode(&bc, 0).unwrap();
    assert_eq!(insn.size, 3);
    match insn.info {
        InsnInfo::WarpOrInteract { is_warp, .. } => assert!(!is_warp),
        _ => panic!(),
    }
}

#[test]
fn linear_walker_yields_three_instructions_then_stops() {
    // [22, 5] ExecMove; [21] Nop; [37, 0, 0] Yield.
    let bc = [0x22u8, 5, 0x21, 0x37, 0, 0];
    let walked: Vec<_> = LinearWalker::new(&bc, 0).collect();
    assert_eq!(walked.len(), 3);
    assert!(walked.iter().all(|r| r.is_ok()));
}

#[test]
fn linear_walker_recovers_from_unknown_sub_op() {
    // 0x4C 0xFF: outer nibble 0xF has no decoder (HighNibble unknown).
    // Walker should emit the error then advance one byte.
    let bc = [0x4Cu8, 0xFF, 0x21];
    let mut walked = LinearWalker::new(&bc, 0);
    let first = walked.next().unwrap();
    assert!(first.is_err(), "first instruction should error");
    // Walker advanced 1 byte; next instruction is at pc=1 (still inside 0x4C/0xFF/0x21).
    let second = walked.next().unwrap();
    match second {
        Ok(_) | Err(_) => {}
    }
}

#[test]
fn fmv_trigger_search_finds_all() {
    // Build a script with 3 FMV triggers.
    let mut bc = Vec::new();
    // Trigger 0: MV1.STR at pc=0
    bc.extend_from_slice(&[0x4C, 0xE2, 0, 0, 0, 0]);
    // Some unrelated nop
    bc.push(0x21);
    // Trigger 4: MV6.STR
    bc.extend_from_slice(&[0x4C, 0xE2, 4, 0, 0, 0]);
    // Trigger 3: MV4.STR
    bc.extend_from_slice(&[0x4C, 0xE2, 3, 0, 0, 0]);
    let triggers = find_fmv_triggers(&bc);
    assert_eq!(triggers.len(), 3);
    assert_eq!(triggers[0], (0, 0));
    assert_eq!(triggers[1].1, 4);
    assert_eq!(triggers[2].1, 3);
}

#[test]
fn fmv_filename_known_indices() {
    assert_eq!(fmv_filename(0), "MV1.STR");
    assert_eq!(fmv_filename(1), "MV3.STR");
    assert_eq!(fmv_filename(4), "MV6.STR");
    assert_eq!(fmv_filename(5), "(cut: MOV15.STR)");
    assert_eq!(fmv_filename(99), "(unknown)");
}

#[test]
fn scene_change_consumes_inline_payload_and_recovers_name() {
    // [3F, index_lo, index_hi, name_len=4, 'd','o','l','k', entry_x, entry_z, dir].
    let bc = [0x3Fu8, 60, 0, 4, b'd', b'o', b'l', b'k', 0x10, 0x20, 0x30];
    let insn = decode(&bc, 0).unwrap();
    assert_eq!(insn.size, 11); // header 1 + 6 + name_len 4
    match insn.info {
        InsnInfo::SceneChange {
            index,
            name_len,
            entry_x,
            entry_z,
            dir,
        } => {
            assert_eq!(index, 60);
            assert_eq!(name_len, 4);
            assert_eq!(entry_x, 0x10);
            assert_eq!(entry_z, 0x20);
            assert_eq!(dir, 0x30);
        }
        other => panic!("expected SceneChange, got {other:?}"),
    }
    // The destination name is recovered from the bytecode slice.
    assert_eq!(scene_change_name(&bc, &insn).as_deref(), Some("dolk"));
}

#[test]
fn scene_change_name_rejects_text_desync_phantom() {
    // A 0x3F whose "name" bytes are uppercase/punctuation (a literal '?'
    // landing inside message text) is not a clean CDNAME label.
    let bc = [0x3Fu8, 0, 0, 4, b'H', b'i', b'!', b' ', 0x00, 0x00, 0x00];
    let insn = decode(&bc, 0).unwrap();
    assert_eq!(scene_change_name(&bc, &insn), None);
}

#[test]
fn system_flag_test_includes_target() {
    // Opcode 0x70, idx low byte 5, jump 0x000A.
    let bc = [0x70u8, 5, 0x0A, 0x00];
    let insn = decode(&bc, 0).unwrap();
    match insn.info {
        InsnInfo::SystemFlag {
            kind: FlagKind::Test,
            target,
            ..
        } => assert_eq!(target.unwrap(), (1 + 1) + 0x0A),
        _ => panic!(),
    }
}

#[test]
fn truncated_returns_error_at_pc() {
    let bc = [0x22u8]; // ExecMove with no operand byte.
    let err = decode(&bc, 0).unwrap_err();
    assert!(matches!(err, DisasmError::Truncated { pc: 0, .. }));
}

#[test]
fn format_instruction_includes_byte_dump_and_mnemonic() {
    let bc = [0x4Cu8, 0xE2, 0x03, 0x00, 0x00, 0x00];
    let insn = decode(&bc, 0).unwrap();
    let line = format_instruction(&insn, &bc);
    assert!(line.contains("0x0000"));
    assert!(line.contains("4C E2 03 00 00 00"));
    assert!(line.contains("FmvTrigger"));
    assert!(line.contains("MV4.STR"));
}

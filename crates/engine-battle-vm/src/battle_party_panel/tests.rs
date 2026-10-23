use super::*;

/// The two ring stamps share [`cross_out_mark`]'s screen span and differ
/// only in the texels and CLUT (`0x801DBD7C..0x801DBDBC`,
/// `0x801DBF3C..0x801DBF7C`).
#[test]
fn ring_stamps_sample_their_own_cells() {
    let x = cross_out_mark(0xA0, 0x42);
    let rot = rot_stamp_on_chip(0xA0, 0x42);
    assert_eq!(rot.xy, x.xy);
    assert_eq!(rot.uv[0], (0x50, 0x60));
    assert_eq!(rot.uv[3], (0x6F, 0x77));
    assert_eq!(rot.clut, 0x770B);
    let curse = curse_plate_on_chip(0xF8, 0x42);
    assert_eq!(curse.xy, cross_out_mark(0xF8, 0x42).xy);
    assert_eq!(curse.uv[0], (0x78, 0x60));
    assert_eq!(curse.uv[3], (0xB7, 0x6F));
    assert_eq!(curse.clut, 0x7700);
    assert_eq!((rot.tpage, curse.tpage), (7, 7));
}

/// `FUN_801DBDDC` is 1:1 at the favoured cost and widens symmetrically.
#[test]
fn arts_stamp_widens_by_half_the_cost_excess() {
    let q = rot_stamp_on_arts_chip(0xCC, 0x22, 0x1E);
    assert_eq!(q.xy[0], (0xD4, 0x1A));
    assert_eq!(q.xy[3], (0xF3, 0x31));
    let wide = rot_stamp_on_arts_chip(0xCC, 0x22, 0x28);
    assert_eq!(wide.xy[0].0, 0xD4 - 5);
    assert_eq!(wide.xy[3].0, 0xF3 + 5);
    // A sub-30 cost narrows (the halfword store keeps the low 16 bits).
    let narrow = rot_stamp_on_arts_chip(0xCC, 0x22, 0x1C);
    assert_eq!((narrow.xy[0].0, narrow.xy[3].0), (0xD5, 0xF2));
}

/// The side stamps stay pinned to the edge nearest the D-pad: the Left
/// stamp's right edge and the Right stamp's left edge do not move with
/// the cost, as the chips' do not.
#[test]
fn arts_entry_stamps_follow_the_limb_bits() {
    assert!(arts_entry_rot_stamps(0x1007, [30; 4]).is_empty());
    for c in [30u8, 36, 40] {
        let s = arts_entry_rot_stamps(0x08, [c, 30, 30, 30]);
        assert_eq!(s.len(), 1);
        let q = rot_stamp_on_arts_chip(s[0].0, s[0].1, s[0].2);
        assert_eq!(q.xy[1].0, 0xCB, "left stamp's right edge at cost {c}");
        let s = arts_entry_rot_stamps(0x10, [30, 30, 30, c]);
        let q = rot_stamp_on_arts_chip(s[0].0, s[0].1, s[0].2);
        assert_eq!(q.xy[0].0, 0xFC, "right stamp's left edge at cost {c}");
    }
    let legs = arts_entry_rot_stamps(0x20, [40; 4]);
    assert_eq!(legs, vec![(0xCC, 0x22, 0x1E), (0xCC, 0x62, 0x1E)]);
    assert_eq!(arts_entry_rot_stamps(0x38, [30; 4]).len(), 4);
}

/// A synthetic image with both defeat pieces planted at their VAs.
/// Placeholder text only.
fn defeat_image(team: &[u8], solo: &[u8]) -> Vec<u8> {
    let mut img = vec![0u8; (DEFEAT_SOLO_SUFFIX_VA - OVERLAY_0898_LINK_BASE) as usize + 0x40];
    let put = |img: &mut Vec<u8>, va: u32, s: &[u8]| {
        let o = (va - OVERLAY_0898_LINK_BASE) as usize;
        img[o..o + s.len()].copy_from_slice(s);
        img[o + s.len()] = 0;
    };
    put(&mut img, DEFEAT_TEAM_VA, team);
    put(&mut img, DEFEAT_SOLO_SUFFIX_VA, solo);
    img
}

#[test]
fn defeat_text_parses_both_pieces_and_names_the_lead() {
    let t = DefeatText::parse(&defeat_image(b"\xC1\x01 TEAM", b" SOLO")).expect("parses");
    assert_eq!(t.team_tail, " TEAM");
    assert_eq!(t.solo_suffix, " SOLO");
    assert_eq!(t.compose(ResultSubject::Lead(1), "A"), "A SOLO");
    assert_eq!(
        t.compose(ResultSubject::LeadsTeam { escape_operand: 0 }, "A"),
        "A TEAM"
    );
}

#[test]
fn defeat_text_refuses_a_team_string_without_the_escape() {
    assert!(DefeatText::parse(&defeat_image(b"xx TEAM", b" SOLO")).is_none());
    assert!(DefeatText::parse(&defeat_image(b"\xC1\x01 T\x01", b" SOLO")).is_none());
    assert!(DefeatText::parse(&[0u8; 16]).is_none());
}

#[test]
fn defeat_text_keeps_an_accent_cell() {
    let t = DefeatText::parse(&defeat_image(b"\xC1\x01 \x82quipe", b" SOLO")).expect("parses");
    assert_eq!(t.team_tail, " \u{82}quipe");
}

fn victory_image(team: &[u8], sentence: &[u8]) -> Vec<u8> {
    let mut img = vec![0u8; 0x28800];
    for (va, b) in [(VICTORY_TEAM_VA, team), (VICTORY_REPORT_VA, sentence)] {
        let o = (va - OVERLAY_0898_LINK_BASE) as usize;
        img[o..o + b.len()].copy_from_slice(b);
        img[o + b.len()] = 0;
    }
    img
}

#[test]
fn victory_text_splits_the_sentence_and_its_report_runs() {
    let t = VictoryText::parse(&victory_image(
        b"\xC1\x01's crew",
        b" won!|Got     Exp and     G.",
    ))
    .expect("parses");
    assert_eq!(t.opening(ResultSubject::Lead(1), "A"), "A won!");
    assert_eq!(
        t.opening(ResultSubject::LeadsTeam { escape_operand: 0 }, "A"),
        "A's crew won!"
    );
    assert_eq!(t.report_runs(), ["Got", "Exp and", "G."]);
    // No line break, or no escape: not the victory sentence.
    assert!(VictoryText::parse(&victory_image(b"\xC1\x01's", b" won!")).is_none());
    assert!(VictoryText::parse(&victory_image(b"xx's", b" won!|G.")).is_none());
}

#[test]
fn a_moved_victory_sentence_reads_through_its_site() {
    let mut img = victory_image(b"\xC1\x01's crew", b"stale|x");
    let put = |img: &mut Vec<u8>, va: u32, b: &[u8]| {
        let o = (va - OVERLAY_0898_LINK_BASE) as usize;
        img[o..o + b.len()].copy_from_slice(b);
    };
    // `lui a1,0x801f ; ... ; addiu a1,a1,0x6100`.
    put(&mut img, 0x801D_8544, &0x3C05_801Fu32.to_le_bytes());
    put(&mut img, 0x801D_854C, &0x24A5_6100u32.to_le_bytes());
    put(&mut img, 0x801F_6100, b" gagne !|Obtenu  XP et  Or.\0");
    let t = VictoryText::parse(&img).expect("parses");
    assert_eq!(t.opening_tail, " gagne !");
    assert_eq!(t.report_runs(), ["Obtenu", "XP et", "Or."]);
}

#[test]
fn name_pointer_matches_retails_constant() {
    // Retail's literal is `0x8008459B + id * 0x414`.
    for id in 1u8..=4 {
        let retail = 0x8008_459Bu32 + id as u32 * 0x414;
        assert_eq!(name_field_ptr(id), retail, "id {id}");
    }
}

#[test]
fn name_pointer_is_the_records_display_name_field() {
    // Slot 0's record starts at CHAR_RECORD_BASE; its name is +0x2A7.
    assert_eq!(name_field_ptr(1), CHAR_RECORD_BASE + NAME_OFFSET);
    assert_eq!(
        name_field_ptr(2),
        CHAR_RECORD_BASE + CHAR_RECORD_STRIDE + NAME_OFFSET
    );
}

#[test]
fn open_writes_the_handle_and_raises_the_flag() {
    let s = LabelState::opened(0x1234);
    assert_eq!(s.flags, 0x80);
    assert_eq!(s.handle, 0x1234);
    assert!(s.is_open());
}

#[test]
fn teardown_clears_the_handle_but_not_the_kind_byte() {
    let open = LabelState {
        kind: 9,
        flags: 0x80,
        phase: 5,
        handle: 0x1234,
    };
    let closed = open.torn_down();
    assert_eq!(closed.handle, 0);
    assert_eq!(closed.flags, 0);
    assert_eq!(closed.phase, 0);
    assert_eq!(closed.kind, 9, "the teardown never writes +0x00");
    assert!(!closed.is_open());
}

#[test]
fn open_and_teardown_are_a_matched_pair_on_one_block() {
    let s = LabelState::opened(0x40).torn_down();
    assert_eq!(s, LabelState::default().torn_down());
}

#[test]
fn open_selector_is_zero_based() {
    assert_eq!(open_selector(1), 0);
    assert_eq!(open_selector(3), 2);
    // Retail does not guard the subtraction.
    assert_eq!(open_selector(0), -1);
}

#[test]
fn open_layout_args_carry_the_negative_register_argument() {
    assert_eq!(OPEN_LAYOUT_ARGS.len(), 8);
    assert_eq!(OPEN_LAYOUT_ARGS[3], -0x92, "a3 is signed");
    assert_eq!(&OPEN_LAYOUT_ARGS[4..], &[0x24, 0x8A, 0x90, 3]);
}

#[test]
fn strip_is_suppressed_by_a_nonzero_gate() {
    assert!(strip_draws(0));
    assert!(!strip_draws(1));
    assert!(!strip_draws(-1));
}

#[test]
fn strip_is_a_one_to_one_blit_of_a_sixty_four_by_sixteen_cell() {
    let q = cross_out_mark(100, 80);
    assert_eq!(q.xy[0], (92, 76));
    assert_eq!(q.xy[3], (155, 91));
    assert_eq!(q.xy[1].0 - q.xy[0].0, (q.uv[1].0 - q.uv[0].0) as i16);
    assert_eq!(q.xy[2].1 - q.xy[0].1, (q.uv[2].1 - q.uv[0].1) as i16);
    assert_eq!(q.xy[1].0 - q.xy[0].0, 0x3F);
    assert_eq!(q.xy[2].1 - q.xy[0].1, 0x0F);
    assert_eq!(q.clut, STRIP_CLUT);
    assert_eq!(q.tpage, STRIP_TPAGE);
    assert_eq!(q.tag, STRIP_OT_TAG);
    assert_eq!(q.code_colour, STRIP_CODE_COLOUR);
}

#[test]
fn strip_corners_share_edges() {
    let q = cross_out_mark(0, 0);
    assert_eq!(q.xy[0].1, q.xy[1].1);
    assert_eq!(q.xy[2].1, q.xy[3].1);
    assert_eq!(q.xy[0].0, q.xy[2].0);
    assert_eq!(q.xy[1].0, q.xy[3].0);
    assert_eq!(q.uv[0].1, q.uv[1].1);
    assert_eq!(q.uv[0].0, q.uv[2].0);
}

#[test]
fn strip_samples_a_different_atlas_row_than_the_value_readout_label() {
    // FUN_801E805C's label is CLUT 0x7703 / tpage 0x27 at v 0xE0..0xEF;
    // this one is CLUT 0x7704 / tpage 7 at v 0x60..0x6F. Same shape, two
    // distinct atlas cells - so the two are not the same blit.
    assert_ne!(STRIP_CLUT, crate::battle_value_readout::GLYPH_CLUT);
    assert_ne!(STRIP_TPAGE, crate::battle_value_readout::GLYPH_TPAGE);
}

#[test]
fn solo_party_is_centred_and_a_pair_splits_outward() {
    assert_eq!(panel_anchors(1), Some((0x72, None)));
    let (a, b) = panel_anchors(2).unwrap();
    let b = b.unwrap();
    assert!(a < 0x72 && b > 0x72, "the pair straddles the solo anchor");
    assert_eq!((a, b), (0x3F, 0xA5));
}

#[test]
fn a_full_party_shifts_the_row_left() {
    assert_eq!(panel_anchors(3), Some((0x0C, Some(0x72))));
    // Each larger party moves the primary anchor further left.
    let one = panel_anchors(1).unwrap().0;
    let two = panel_anchors(2).unwrap().0;
    let three = panel_anchors(3).unwrap().0;
    assert!(three < two && two < one);
}

#[test]
fn party_sizes_outside_one_to_three_take_no_arm() {
    assert_eq!(panel_anchors(0), None);
    assert_eq!(panel_anchors(4), None);
    assert_eq!(panel_anchors(0xFF), None);
}

#[test]
fn every_party_actor_is_reset_to_target_the_first_monster() {
    assert_eq!(PARTY_RESET.target_slot, 3);
    assert_eq!(PARTY_RESET.category, 0);
}

#[test]
fn portrait_cells_are_the_id_plus_a_fixed_bias() {
    assert_eq!(portrait_cell(1), 0x33);
    assert_eq!(portrait_cell(3), 0x35);
    assert_eq!(portrait_cell(0), 0x32);
}

#[test]
fn build_arm_keys_on_the_second_slot() {
    assert_eq!(build_arm(0), BuildArm::Solo);
    assert_eq!(build_arm(1), BuildArm::Roster);
    assert_eq!(build_arm(0xFF), BuildArm::Roster);
}

#[test]
fn the_roster_arm_patches_out_of_declaration_order() {
    let mut declared = LABEL_BUFFERS;
    let mut measured = MEASURE_ORDER;
    declared.sort_unstable();
    measured.sort_unstable();
    assert_eq!(declared, measured);
    assert_ne!(LABEL_BUFFERS, MEASURE_ORDER);
    assert_eq!(RESULT_BUFFERS.map(|(b, _)| b), LABEL_BUFFERS);
}

#[test]
fn a_lone_lead_is_named_and_a_party_is_the_leads_team() {
    assert_eq!(result_subject([2, 0, 0]), ResultSubject::Lead(2));
    // Every patch reads the FIRST seat, whoever sits in the others.
    assert_eq!(
        result_subject([1, 2, 3]),
        ResultSubject::LeadsTeam { escape_operand: 0 }
    );
    assert_eq!(
        result_subject([3, 1, 2]),
        ResultSubject::LeadsTeam { escape_operand: 2 }
    );
    // The discriminator is the SECOND seat, not the party size.
    assert_eq!(result_subject([1, 0, 3]), ResultSubject::Lead(1));
}

#[test]
fn a_named_lead_resolves_to_the_records_display_name() {
    let ResultSubject::Lead(id) = result_subject([3, 0, 0]) else {
        panic!("a lone lead is named");
    };
    assert_eq!(
        name_field_ptr(id),
        CHAR_RECORD_BASE + 2 * CHAR_RECORD_STRIDE + NAME_OFFSET
    );
}

#[test]
fn published_buffers_point_into_the_context() {
    for (slot, buf) in PUBLISHED_BUFFERS {
        assert!(slot > 0x600, "placement-table offset {slot:#x}");
        assert!(buf < 0x200, "context offset {buf:#x}");
    }
}

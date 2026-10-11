use super::*;

#[test]
fn power_shift_matches_kernel() {
    // 0x02ee = 750 -> >>2 = 187 (the kernel's roll modulus base).
    let mut raw = [0u8; MOVE_POWER_RECORD_STRIDE];
    raw[4] = 0xc8; // +0x04 counter_init = 0x00c8
    raw[0x0d] = 0x4b; // +0x0d sound cue = 0x4b
    let r = MoveRecord {
        index: 1,
        power_raw: 0x02ee,
        raw,
    };
    assert_eq!(r.power(), 187);
    assert_eq!(r.counter_init(), 0x00c8);
    assert_eq!(r.sound_cue_id(), 0x4b);
    // Sign preserved (arithmetic shift).
    let n = MoveRecord {
        index: 0,
        power_raw: -32,
        raw: [0; MOVE_POWER_RECORD_STRIDE],
    };
    assert_eq!(n.power(), -8);
}

#[test]
fn residual_field_accessors() {
    // Record shaped after disc record 3 (move id 0x29): pow 6000, +2=250,
    // +6=480, +8=0x20, +9=1, +a=1, +c='C', +d=0x4d, +12=27 8e 8d 00,
    // +16=28 64 9d 00.
    let mut raw = [0u8; MOVE_POWER_RECORD_STRIDE];
    raw[0..2].copy_from_slice(&6000i16.to_le_bytes());
    raw[2..4].copy_from_slice(&250i16.to_le_bytes());
    raw[6..8].copy_from_slice(&480u16.to_le_bytes());
    raw[8] = 0x20;
    raw[9] = 1;
    raw[0x0a] = 1;
    raw[0x0b] = 0;
    raw[0x0c] = b'C';
    raw[0x0d] = 0x4d;
    raw[0x12..0x16].copy_from_slice(&[0x27, 0x8e, 0x8d, 0x00]);
    raw[0x16..0x1a].copy_from_slice(&[0x28, 0x64, 0x9d, 0x00]);
    let r = MoveRecord {
        index: 3,
        power_raw: 6000,
        raw,
    };
    assert_eq!(r.power(), 1500);
    assert_eq!(r.strike_y_offset(), 250);
    assert_eq!(r.phase_duration(), 480);
    assert_eq!(r.homing_speed(), 0x20);
    assert!(r.effect_tracks_strike());
    assert_eq!(r.impact_effect(), 1);
    assert_eq!(r.trail_texture_page(), 0);
    assert_eq!(r.annotation_tag(), Some('C'));
    assert_eq!(r.sound_cue_id(), 0x4d);
    assert_eq!(r.contact_effects(), vec![0x27, 0x8e, 0x8d]);
    assert_eq!(r.launch_effects(), vec![0x28, 0x64, 0x9d]);

    // 0xFF skip-sentinel terminates the list (disc record 40 / 0x74).
    let mut raw2 = [0u8; MOVE_POWER_RECORD_STRIDE];
    raw2[0x12..0x16].copy_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    raw2[0x0e] = 0xff;
    let r2 = MoveRecord {
        index: 40,
        power_raw: 0,
        raw: raw2,
    };
    assert!(r2.contact_effects().is_empty());
    assert_eq!(r2.list_mode(), 0xff);
    // A record with no printable +0x0c tag yields None.
    assert_eq!(r2.annotation_tag(), None);
}

#[test]
fn id_index_map_resolves_move_ids() {
    // Synthetic 0898-shaped buffer: map at its offset, full table after it.
    let mut buf =
        vec![0u8; MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE * MOVE_POWER_TABLE_LEN];
    // map[4] = 1 (the guard + first mapped move), map[5] = 2, map[0x10] = 0xff.
    buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 1;
    buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 5] = 2;
    buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 0x10] = MOVE_ID_INDEX_NONE;
    // table record 1 power 0x02ee, record 2 power 0x09c4.
    buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE] = 0xee;
    buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE + 1] = 0x02;
    buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE * 2] = 0xc4;
    buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE * 2 + 1] = 0x09;

    let map = parse_id_index_map(&buf).expect("map parses");
    let table = parse(&buf).expect("table parses");
    assert_eq!(index_for_move_id(&map, 4), Some(1));
    assert_eq!(index_for_move_id(&map, 5), Some(2));
    assert_eq!(index_for_move_id(&map, 0), None); // map[0] == 0 -> no record
    assert_eq!(index_for_move_id(&map, 0x10), None); // 0xff sentinel
    assert_eq!(
        record_for_move_id(&table, &map, 4).map(|r| r.power()),
        Some(187)
    );
    assert_eq!(
        record_for_move_id(&table, &map, 5).map(|r| r.power()),
        Some(625)
    );
}

#[test]
fn parse_at_reads_stride_and_guards() {
    // Synthetic: record 0 empty, record 1 has power_raw 0x02ee.
    let mut buf = vec![0u8; 16 + MOVE_POWER_RECORD_STRIDE * 3];
    let off = 16;
    // record 1 (index 1) +0 = 0x02ee
    buf[off + MOVE_POWER_RECORD_STRIDE] = 0xee;
    buf[off + MOVE_POWER_RECORD_STRIDE + 1] = 0x02;
    let recs = parse_at(&buf, off, 3).expect("parses");
    assert_eq!(recs.len(), 3);
    assert!(recs[0].is_empty());
    assert_eq!(recs[1].power_raw, 0x02ee);
    assert_eq!(recs[1].power(), 187);

    // Guard: a non-empty record-0 (offset lands off the table) -> None.
    assert!(parse_at(&buf, off + 1, 3).is_none());
    // Guard: slice too short -> None.
    assert!(parse_at(&buf, buf.len() - 4, 2).is_none());
}

#[test]
fn effect_list_entry_classifies_each_dispatch_arm() {
    use EffectListEntry::*;
    assert_eq!(EffectListEntry::classify(0x00), Terminator);
    assert_eq!(EffectListEntry::classify(0x01), Spawn(0x01));
    assert_eq!(EffectListEntry::classify(0x28), Spawn(0x28));
    assert_eq!(EffectListEntry::classify(0x63), Spawn(0x63)); // 99, last spawn
    assert_eq!(EffectListEntry::classify(0x64), FixedFlash); // 100
    assert_eq!(EffectListEntry::classify(0x65), Skip); // 101: unused no-op
    assert_eq!(EffectListEntry::classify(0x7F), Skip); // 127: unused no-op
    assert_eq!(EffectListEntry::classify(0x80), AltEffect(0x00)); // high bit
    assert_eq!(EffectListEntry::classify(0x9d), AltEffect(0x1d));
    assert_eq!(EffectListEntry::classify(0xFE), AltEffect(0x7E));
    assert_eq!(EffectListEntry::classify(0xFF), Skip); // skip sentinel
}

#[test]
fn effect_aux_table_offsets_and_extent() {
    // Pinned against the move-power table base (same overlay link mapping).
    assert_eq!(EFFECT_PROTO_TABLE_FILE_OFFSET, 0x27B0C);
    assert_eq!(EFFECT_CLUT_TABLE_FILE_OFFSET, 0x27C00);
    // The prototype table is bounded by the SFX table that follows it.
    assert_eq!(EFFECT_AUX_TABLE_LEN, 61);
    assert_eq!(
        EFFECT_PROTO_TABLE_FILE_OFFSET + EFFECT_AUX_TABLE_LEN * 4,
        EFFECT_CLUT_TABLE_FILE_OFFSET
    );
}

#[test]
fn impact_effect_table_offset_and_parse() {
    assert_eq!(IMPACT_EFFECT_TABLE_FILE_OFFSET, 0x26BBC);
    // A 0898-shaped buffer with the map guard + a known pointer at index 0.
    let mut buf = vec![0u8; IMPACT_EFFECT_TABLE_FILE_OFFSET + IMPACT_EFFECT_TABLE_LEN * 4];
    buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 1; // map guard
    buf[IMPACT_EFFECT_TABLE_FILE_OFFSET..IMPACT_EFFECT_TABLE_FILE_OFFSET + 4]
        .copy_from_slice(&0x801F_5A00u32.to_le_bytes());
    let table = parse_impact_effect_table(&buf).expect("impact table parses");
    assert_eq!(table.len(), 5);
    assert_eq!(table[0], 0x801F_5A00);
    // Guard: a bad map -> no table.
    buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 0;
    assert!(parse_impact_effect_table(&buf).is_none());
}

#[test]
fn effect_key_from_each_dispatch_arm() {
    assert_eq!(
        EffectKey::from_entry(EffectListEntry::Spawn(0x27)),
        Some(EffectKey::Proto3D(0x27))
    );
    assert_eq!(
        EffectKey::from_entry(EffectListEntry::AltEffect(0x1d)),
        Some(EffectKey::Efect2D(0x1d))
    );
    assert_eq!(
        EffectKey::from_entry(EffectListEntry::FixedFlash),
        Some(EffectKey::Flash)
    );
    assert_eq!(EffectKey::from_entry(EffectListEntry::Terminator), None);
    assert_eq!(EffectKey::from_entry(EffectListEntry::Skip), None);
}

#[test]
fn effect_trigger_index_inverts_lists() {
    // Two 0898-shaped records: record 1 (mapped by move id 4) with a contact
    // list [0x27, 0x8e, 0x8d, 0x00] and a launch list [0x28, 0x64, 0x9d, 0x00];
    // record 2 (NO owning move id) with a contact list [0x27, 0x00, ..].
    let mut buf =
        vec![0u8; MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE * MOVE_POWER_TABLE_LEN];
    buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 1; // map guard + move id 4 -> record 1
    let r1 = MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE;
    buf[r1] = 0x10; // non-empty (power low byte)
    buf[r1 + 0x12..r1 + 0x16].copy_from_slice(&[0x27, 0x8e, 0x8d, 0x00]);
    buf[r1 + 0x16..r1 + 0x1a].copy_from_slice(&[0x28, 0x64, 0x9d, 0x00]);
    let r2 = MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE * 2;
    buf[r2] = 0x20; // non-empty, but no move id maps to record 2
    buf[r2 + 0x12..r2 + 0x16].copy_from_slice(&[0x27, 0x00, 0x00, 0x00]);

    let table = parse(&buf).expect("table parses");
    let map = parse_id_index_map(&buf).expect("map parses");
    let index = effect_trigger_index(&table, &map);

    // Proto3D 0x27 fires from record 1's contact (move 4) AND record 2's
    // contact (no move id -> None), keyed on (space, id).
    let p27 = index.get(&EffectKey::Proto3D(0x27)).expect("proto3d 0x27");
    assert!(p27.contains(&Trigger {
        move_id: Some(4),
        record_idx: 1,
        fired: EffectFired::Contact,
    }));
    assert!(p27.contains(&Trigger {
        move_id: None,
        record_idx: 2,
        fired: EffectFired::Contact,
    }));
    // Efect2D 0x0e (0x8e & 0x7f) from record 1 contact.
    assert_eq!(
        index.get(&EffectKey::Efect2D(0x0e)).map(|v| v.as_slice()),
        Some(
            [Trigger {
                move_id: Some(4),
                record_idx: 1,
                fired: EffectFired::Contact,
            }]
            .as_slice()
        )
    );
    // Launch list: Proto3D 0x28, Flash singleton, Efect2D 0x1d.
    for key in [
        EffectKey::Proto3D(0x28),
        EffectKey::Flash,
        EffectKey::Efect2D(0x1d),
    ] {
        let triggers = index.get(&key).unwrap_or_else(|| panic!("{key:?}"));
        assert!(triggers.contains(&Trigger {
            move_id: Some(4),
            record_idx: 1,
            fired: EffectFired::Launch,
        }));
    }
    // The impact-effect id space is NOT in the index: a Proto3D key for the
    // impact selector value would be wrong. Record 1's +0x0a is untouched (0),
    // so no spurious key. Flash never appears for contact here.
    assert!(
        !index
            .get(&EffectKey::Flash)
            .unwrap()
            .iter()
            .any(|t| t.fired == EffectFired::Contact)
    );
}

/// A 0898-shaped buffer: valid move-power map guard, room for all three aux
/// tables, and a cue-group table that passes the shape guard.
fn synthetic_0898() -> Vec<u8> {
    let mut buf = vec![0u8; CUE_GROUP_TABLE_FILE_OFFSET + CUE_GROUP_TABLE_LEN * CUE_GROUP_STRIDE];
    buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 1; // map guard
    buf
}

#[test]
fn effect_aux_tables_parse_synthetic() {
    let mut buf = synthetic_0898();
    // proto[0x28] = 0xCAFEBABE, sfx[0x28] = 0x4d.
    let pb = EFFECT_PROTO_TABLE_FILE_OFFSET + 0x28 * 4;
    buf[pb..pb + 4].copy_from_slice(&0xCAFE_BABEu32.to_le_bytes());
    buf[EFFECT_CLUT_TABLE_FILE_OFFSET + 0x28] = 0x4d;

    let aux = EffectAuxTables::parse(&buf).expect("aux tables parse");
    assert_eq!(aux.effect_proto(0x28), Some(0xCAFE_BABE));
    assert_eq!(aux.effect_clut_x(0x28), Some(0x4d));
    assert_eq!(aux.effect_clut_x(0x00), Some(0)); // silent
    assert_eq!(aux.effect_proto(EFFECT_AUX_TABLE_LEN as u8), None); // out of range
    assert_eq!(aux.proto().len(), EFFECT_AUX_TABLE_LEN);
    assert_eq!(aux.clut_map().len(), EFFECT_AUX_TABLE_LEN);

    // Guard: an overlay that fails the move-power map guard yields no tables.
    let mut bad = buf.clone();
    bad[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 0;
    assert!(EffectAuxTables::parse(&bad).is_none());
}

#[test]
fn cue_group_table_parses_and_is_shape_guarded() {
    let mut buf = synthetic_0898();
    // Group 3: two cues, one effect id and one actor id.
    let g3 = CUE_GROUP_TABLE_FILE_OFFSET + 3 * CUE_GROUP_STRIDE;
    buf[g3] = 2;
    buf[g3 + 1] = 0x0C;
    buf[g3 + 2] = 0x94; // actor cue: 0x80 | 0x14

    let aux = EffectAuxTables::parse(&buf).expect("aux tables parse");
    assert_eq!(aux.cue_group(3), Some((2, [0x0C, 0x94, 0, 0])));
    assert_eq!(aux.cue_group(0), Some((0, [0; 4])), "an empty group");
    assert_eq!(aux.cue_group(CUE_GROUP_TABLE_LEN as u8), None);
    assert_eq!(
        aux.cue_group_bytes().len(),
        CUE_GROUP_TABLE_LEN * CUE_GROUP_STRIDE
    );
    // The flat region is what the expander indexes.
    assert_eq!(aux.cue_group_bytes()[3 * CUE_GROUP_STRIDE], 2);

    // A count above the record's own capacity is not this table.
    let mut bad = buf.clone();
    bad[g3] = 5;
    assert!(EffectAuxTables::parse(&bad).is_none());
    // Nor is an effect cue id that cannot index the two tables above. The
    // actor flag exempts an id from that bound, so set it on the low bits.
    let mut bad = buf.clone();
    bad[g3 + 1] = EFFECT_AUX_TABLE_LEN as u8;
    assert!(EffectAuxTables::parse(&bad).is_none());
    let mut ok = buf.clone();
    ok[g3 + 1] = CUE_ACTOR_FLAG | EFFECT_AUX_TABLE_LEN as u8;
    assert!(EffectAuxTables::parse(&ok).is_some());
}

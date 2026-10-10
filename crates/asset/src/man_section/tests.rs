use super::*;

/// Build a minimal MAN buffer with N0=1, N1=2, N2=1 and a custom
/// section_0 payload.
fn build_synthetic(
    depth_lut: [i16; 16],
    section_0_body: &[u8],
    section_n_lengths: [u32; 4],
) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.push(0xB2);
    buf.push(0x01); // status low bit + bit 0x100 -> 0x01B2
    for v in depth_lut {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    // N0=1, N1=2, N2=1
    buf.extend_from_slice(&1i16.to_le_bytes());
    buf.extend_from_slice(&2i16.to_le_bytes());
    buf.extend_from_slice(&1i16.to_le_bytes());
    // 3-byte u24 at 0x28 == data-region offset where section 0 lands.
    // We'll place section 0 immediately after the record table.
    let data_region_start = 0x2B + 3 * (1 + 2 + 1);
    let u24_at_28 = 0u32; // section 0 right at the start of the data region
    buf.extend_from_slice(&[
        (u24_at_28 & 0xFF) as u8,
        ((u24_at_28 >> 8) & 0xFF) as u8,
        ((u24_at_28 >> 16) & 0xFF) as u8,
    ]);
    // Record table - 4 records of 3 bytes each, all zero offsets
    for _ in 0..4 {
        buf.extend_from_slice(&[0, 0, 0]);
    }
    assert_eq!(buf.len(), data_region_start);

    // Section 0: u24 length + body
    let s0_len = section_0_body.len() as u32;
    buf.extend_from_slice(&[
        (s0_len & 0xFF) as u8,
        ((s0_len >> 8) & 0xFF) as u8,
        ((s0_len >> 16) & 0xFF) as u8,
    ]);
    buf.extend_from_slice(section_0_body);

    // Sections 1..=4: length-prefixed with caller-provided lengths and
    // dummy bytes.
    for &ln in &section_n_lengths {
        buf.extend_from_slice(&[
            (ln & 0xFF) as u8,
            ((ln >> 8) & 0xFF) as u8,
            ((ln >> 16) & 0xFF) as u8,
        ]);
        for i in 0..ln as usize {
            buf.push(0xAA ^ i as u8);
        }
    }
    // Section 5 terminator: 3 zero bytes.
    buf.extend_from_slice(&[0, 0, 0]);
    buf
}

#[test]
fn actor_placement_decodes_position_model_and_script_offset() {
    // Minimal MAN: N0=1, N1=2 (partition-1 record 0 = scene controller,
    // record 1 = one placed actor), N2=0.
    let mut buf = vec![0u8; 0x2B];
    buf[0] = 0xB2;
    buf[1] = 0x01;
    buf[0x22..0x24].copy_from_slice(&1i16.to_le_bytes()); // N0
    buf[0x24..0x26].copy_from_slice(&2i16.to_le_bytes()); // N1
    buf[0x26..0x28].copy_from_slice(&0i16.to_le_bytes()); // N2
    // Section 0 sits 8 bytes into the data region (after the placement record).
    buf[0x28] = 8;
    // Record table: 3 records (N0+N1+N2) of 3 bytes; all point at data
    // region offset 0 (the placement record - record 0 / P1[0] reuse it,
    // they aren't exercised here).
    buf.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0]);
    // data region (offset 0x34): the placement record for P1[1].
    //   local_count=1, 2 local bytes, model=5, actions=2,
    //   tile_x=0x83 (tile 3 + half), tile_z=0x04 (tile 4), script halt 0x21
    buf.extend_from_slice(&[0x01, 0xAA, 0xBB, 0x05, 0x02, 0x83, 0x04, 0x21]);
    // Six zero-length section prefixes (sections 0..=4 + terminator).
    buf.extend_from_slice(&[0u8; 18]);

    let man = parse(&buf).expect("parse");
    assert_eq!(man.header.partition_counts, [1, 2, 0]);
    let placements = man.actor_placements(&buf);
    assert_eq!(
        placements.len(),
        1,
        "record 0 is the controller; only record 1 is a placement"
    );
    let p = &placements[0];
    assert_eq!(p.index, 1);
    assert_eq!(p.local_count, 1);
    assert_eq!(p.model_index, 5);
    assert!(!p.special_model);
    assert_eq!(p.anim_id, 2);
    assert_eq!(p.tile_x, 3);
    assert_eq!(p.tile_z, 4);
    assert_eq!(p.world_x, 3 * 128 + 128, "X bit-7 set shifts a half-tile");
    assert_eq!(p.world_z, 4 * 128 + 64, "Z bit-7 clear -> tile centre");
    assert_eq!(
        p.script_pc0,
        1 + 2 + 4,
        "1 prefix + local_count*2 + 4 header"
    );
    assert_eq!(p.record_offset, man.data_region_offset);
}

#[test]
fn actor_placement_flags_special_model() {
    // A model byte >= 0xF0 marks the special-model (lead-actor) slot.
    let mut buf = vec![0u8; 0x2B];
    buf[0] = 0xB2;
    buf[0x22..0x24].copy_from_slice(&1i16.to_le_bytes()); // N0
    buf[0x24..0x26].copy_from_slice(&2i16.to_le_bytes()); // N1
    buf[0x28] = 6; // section 0 after the 6-byte placement record
    buf.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0]);
    // local_count=0, model=0xF2, actions=0, tile_x=0, tile_z=0
    buf.extend_from_slice(&[0x00, 0xF2, 0x00, 0x00, 0x00, 0x21]);
    buf.extend_from_slice(&[0u8; 18]);

    let man = parse(&buf).expect("parse");
    let p = &man.actor_placements(&buf)[0];
    assert_eq!(p.model_index, 0xF2);
    assert!(p.special_model);
    assert_eq!(p.script_pc0, 1 + 4, "local_count 0 -> script at +5");
}

#[test]
fn parses_synthetic_man() {
    let lut = [
        1, 48, 96, 128, 192, 240, 288, 336, 384, 432, 480, 528, 576, 624, 672, 720,
    ];
    // Build a section-0 body that decodes as 1 formation + 0 conditions
    // + 0 regions: stride=8, count=1, then 8 bytes:
    //   [0,0,0, count=2, ids=4,4, padding 2]
    let section_0_body: Vec<u8> = vec![
        0x08, 0x04, 0x0C, 0x01, // header: f_stride=8, c_stride=4, r_stride=12, f_count=1
        0x00, 0x00, 0x00, 0x02, 0x04, 0x04, 0x00, 0x00, // formation 0
        0x00, // condition_count=0
        0x00, // region_count=0
    ];
    let buf = build_synthetic(lut, &section_0_body, [5, 6, 7, 8]);

    let man = parse(&buf).expect("parse");
    assert_eq!(man.header.status_flags, 0x01B2);
    assert!(man.header.low_flag);
    assert_eq!(man.header.depth_lut, lut);
    assert_eq!(man.header.partition_counts, [1, 2, 1]);
    assert_eq!(man.header.total_records(), 4);
    assert_eq!(man.header.u24_at_28, 0);
    assert_eq!(man.data_region_offset, 0x2B + 3 * 4);

    // Section 0 lands at data region + u24_at_28 = data_region.
    assert_eq!(man.sections[0].offset, man.data_region_offset);
    assert_eq!(man.sections[0].length as usize, section_0_body.len());

    // Sections 1..=4 chain.
    for i in 0..4 {
        let lengths = [5, 6, 7, 8];
        assert_eq!(man.sections[i + 1].length, lengths[i]);
    }
    // Section 5 is the terminator.
    assert!(man.sections[5].is_terminator());

    // Encounter-section body equals what we wrote.
    let body = man.encounter_section_body(&buf).unwrap();
    assert_eq!(body, section_0_body.as_slice());

    // Decode the interior.
    let es = parse_encounter_section(body).unwrap();
    assert_eq!(es.formation_stride, 8);
    assert_eq!(es.condition_stride, 4);
    assert_eq!(es.region_stride, 12);
    assert_eq!(es.formation_count, 1);
    assert_eq!(es.condition_count, 0);
    assert_eq!(es.region_count, 0);

    let formations: Vec<_> = formation_records(body, &es).collect();
    assert_eq!(formations.len(), 1);
    let f = formations[0].expect("formation 0 parses");
    assert_eq!(f.monster_count, 2);
    assert_eq!(f.monster_ids, [4, 4, 0, 0]);
}

/// Section-0 body shaped like a retail scene with three flag-gated
/// region groups: 1 formation, conditions
/// `[0x0141 -> 1][0x0147 -> 2][default -> 1]`, 4 regions.
fn gated_encounter_body() -> Vec<u8> {
    let mut b: Vec<u8> = vec![
        0x08, 0x04, 0x0C, 0x01, // strides f/c/r + formation_count
        0x00, 0x00, 0x00, 0x01, 0x04, 0x00, 0x00, 0x00, // formation 0
        0x03, // condition_count
        0x41, 0x01, 0x01, 0x00, // cond 0: flag 0x0141, 1 region
        0x47, 0x01, 0x02, 0x00, // cond 1: flag 0x0147, 2 regions
        0xFF, 0xFF, 0x01, 0x00, // cond 2: default, 1 region
        0x04, // region_count
    ];
    // region i: aabb 0..128 whole-map, rate = 10*(i+1), formations [i..+1)
    for i in 0..4u8 {
        b.extend_from_slice(&[0, 0, 128, 128, 10 * (i + 1), 0, i, 1, 0, 0, 0, 0]);
    }
    b
}

#[test]
fn condition_records_partition_the_region_array() {
    let body = gated_encounter_body();
    let es = parse_encounter_section(&body).expect("parse");
    assert_eq!(es.condition_count, 3);
    assert_eq!(es.region_count, 4);
    assert_eq!(es.total_bytes(), body.len(), "section fully consumed");

    let conds: Vec<_> = condition_records(&body, &es).map(|c| c.unwrap()).collect();
    assert_eq!(conds[0].flag_id, 0x0141);
    assert_eq!(conds[0].region_count, 1);
    assert_eq!(conds[2].flag_id, CONDITION_DEFAULT_FLAG);
    assert!(conds[2].is_default());
    // The group counts tile the region array exactly - the invariant that
    // holds across every retail scene bundle.
    let summed: usize = conds.iter().map(|c| c.region_count as usize).sum();
    assert_eq!(summed, es.region_count as usize);
}

#[test]
fn active_group_follows_the_story_flag_walk() {
    let body = gated_encounter_body();
    let es = parse_encounter_section(&body).expect("parse");

    // No flag set -> the trailing default group, NOT region 0.
    assert_eq!(active_region_group(&body, &es, |_| false), Some((3, 1)));
    // First condition's flag set -> its own group wins.
    assert_eq!(
        active_region_group(&body, &es, |f| f == 0x0141),
        Some((0, 1))
    );
    // Second condition's flag set (first clear) -> the cursor has skipped
    // condition 0's single region.
    assert_eq!(
        active_region_group(&body, &es, |f| f == 0x0147),
        Some((1, 2))
    );
    // Both set -> the walk stops at the *first* match, in table order.
    assert_eq!(
        active_region_group(&body, &es, |f| f == 0x0141 || f == 0x0147),
        Some((0, 1))
    );
}

#[test]
fn active_group_selects_the_regions_a_roll_can_see() {
    let body = gated_encounter_body();
    let es = parse_encounter_section(&body).expect("parse");
    let (first, count) = active_region_group(&body, &es, |_| false).unwrap();
    let regions: Vec<_> = region_records_in(&body, &es, first, count)
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].rate_increment, 40, "region 3, not region 0");
    // The flat view still exposes every authored variant.
    assert_eq!(region_records(&body, &es).count(), 4);
}

#[test]
fn active_group_is_none_when_the_walk_falls_off_the_end() {
    // A condition list with no default sentinel and no flag set is
    // retail's "return without rolling" exit.
    let mut body: Vec<u8> = vec![
        0x08, 0x04, 0x0C, 0x00, // no formations
        0x01, // condition_count
        0x10, 0x00, 0x01, 0x00, // cond 0: flag 0x0010, 1 region
        0x01, // region_count
    ];
    body.extend_from_slice(&[0, 0, 128, 128, 24, 0, 0, 1, 0, 0, 0, 0]);
    let es = parse_encounter_section(&body).expect("parse");
    assert_eq!(active_region_group(&body, &es, |_| false), None);
    assert_eq!(
        active_region_group(&body, &es, |f| f == 0x0010),
        Some((0, 1))
    );
}

#[test]
fn rejects_truncated_man() {
    let buf = vec![0u8; 0x10];
    assert!(matches!(parse(&buf), Err(ManError::BufferTooSmall { .. })));
}

#[test]
fn rejects_negative_partition_count() {
    let lut = [0i16; 16];
    let section_0_body: Vec<u8> = vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    let mut buf = build_synthetic(lut, &section_0_body, [0, 0, 0, 0]);
    // Patch N0 to -1.
    buf[0x22] = 0xFF;
    buf[0x23] = 0xFF;
    let err = parse(&buf).unwrap_err();
    assert!(matches!(
        err,
        ManError::NegativePartitionCount { idx: 0, value: -1 }
    ));
}

#[test]
fn scene_entry_script_resolves_partition1_first_record() {
    // Hand-build a ManFile whose partition-1 first record points at a
    // crafted script block: [u8 N=2][N*2 local bytes][4-byte header]
    // [opcode...]. The first opcode is at +1 + 2*2 + 4 = +9.
    let data_region_offset = 0x40usize;
    let p1_0 = 0x10u32;
    let script_start = data_region_offset + p1_0 as usize; // 0x50
    let mut man = vec![0u8; 0x80];
    man[script_start] = 2; // local-entry count N
    man[script_start + 9] = 0x25; // first real opcode

    let mf = ManFile {
        header: ManHeader {
            status_flags: 0,
            low_flag: false,
            depth_lut: [0; 16],
            partition_counts: [1, 1, 0],
            u24_at_28: 0,
        },
        partitions: [vec![0], vec![p1_0], vec![]],
        data_region_offset,
        sections: [SectionRef {
            offset: 0,
            length: 0,
        }; SECTION_COUNT],
    };
    let (start, pc0) = mf.scene_entry_script(&man).expect("entry script resolves");
    assert_eq!(start, script_start);
    assert_eq!(pc0, 9);
    assert_eq!(man[start + pc0], 0x25);

    // Empty partition 1 -> no entry script.
    let mut mf_empty = mf.clone();
    mf_empty.partitions[1].clear();
    mf_empty.header.partition_counts[1] = 0;
    assert!(mf_empty.scene_entry_script(&man).is_none());
}

#[test]
fn formation_record_parser_clamps_to_payload() {
    // stride=8 record: [0,0,0, count=1, id=7, 0, 0, 0] - count=1 and
    // 3 trailing pad bytes.
    let rec = [0, 0, 0, 1, 7, 0, 0, 0];
    let f = FormationRecord::parse(&rec).unwrap();
    assert_eq!(f.monster_count, 1);
    assert_eq!(f.monster_ids, [7, 0, 0, 0]);
    assert_eq!(f.trailing_byte_count, 3);
}

#[test]
fn formation_record_rejects_excess_count() {
    let rec = [0, 0, 0, 5, 1, 2, 3, 4, 5];
    assert!(FormationRecord::parse(&rec).is_none());
}

#[test]
fn region_record_parses_aabb_and_range() {
    let rec = [10, 20, 30, 40, 7, 0, 3, 5, 0xCC, 0xCC, 0xCC, 0xCC];
    let r = RegionRecord::parse(&rec).unwrap();
    assert_eq!(r.aabb_x_min, 10);
    assert_eq!(r.aabb_y_min, 20);
    assert_eq!(r.aabb_x_max, 30);
    assert_eq!(r.aabb_y_max, 40);
    assert_eq!(r.rate_increment, 7);
    assert_eq!(r.formation_range_base, 3);
    assert_eq!(r.formation_range_count, 5);
}

#[test]
fn synthetic_actor_placement_record_offset() {
    let lut = [0i16; 16];
    let body = vec![0x08, 0x04, 0x0C, 0x00, 0x00, 0x00];
    let mut buf = build_synthetic(lut, &body, [0, 0, 0, 0]);
    // Patch partition-1 record [0] to point at byte 4 in the data
    // region. Records start at 0x2B and partition 1 begins after
    // N0=1 records (so record 0 of partition 1 is at offset 0x2B + 3 = 0x2E).
    let p1_record0 = 0x2B + 3;
    buf[p1_record0] = 0x04;
    buf[p1_record0 + 1] = 0;
    buf[p1_record0 + 2] = 0;
    let man = parse(&buf).unwrap();
    let abs = man.actor_placement_record_offset(0, buf.len()).unwrap();
    assert_eq!(abs, man.data_region_offset + 4);
}

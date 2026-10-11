use super::*;

/// Build a synthetic player file with one flagged section record whose
/// pool block finishes the record exactly. `clut_n` and the trailing
/// slack are knobs so the validity gate can be exercised.
fn synth_player_file(clut_n: usize, extra_tail: usize) -> Vec<u8> {
    let rect = SECTION_TEXTURE_RECTS[0];
    // Decoded record: 0x20-byte header, pool right after it.
    let pool = 0x20usize;
    let mut decoded = vec![0u8; pool];
    decoded[0x00..0x04].copy_from_slice(&0x14u32.to_le_bytes()); // frame_off
    decoded[0x0C..0x10].copy_from_slice(&(pool as u32).to_le_bytes()); // pool
    decoded[0x12..0x14].copy_from_slice(&1u16.to_le_bytes()); // upload flag
    decoded.extend_from_slice(&0x30u16.to_le_bytes()); // clut_x
    decoded.extend_from_slice(&(clut_n as u16).to_le_bytes());
    for i in 0..clut_n {
        // Non-zero entries so the STP pass has something to do.
        decoded.extend_from_slice(&((i as u16 + 1) | 0x0100).to_le_bytes());
    }
    decoded.extend(std::iter::repeat_n(0xA5u8, rect.pixel_bytes()));
    decoded.extend(std::iter::repeat_n(0u8, extra_tail));

    let stream = legaia_lzs::compress_optimal(&decoded);
    let slot_size = ((4 + stream.len()).div_ceil(0x800) * 0x800) as u32;

    // record[0]'s own stream: a tiny buffer with two pool offsets that
    // deliberately do NOT satisfy the exact-extent rule, so this
    // fixture yields exactly one row.
    let r0 = vec![0u8; 0x40];
    let r0_stream = legaia_lzs::compress_optimal(&r0);

    let desc_off = 0x10 + r0_stream.len().div_ceil(4) * 4 + 0x10;
    let mut buf = Vec::new();
    buf.extend_from_slice(&(desc_off as u32).to_le_bytes());
    buf.extend_from_slice(&0x10u32.to_le_bytes()); // clut_a_off
    buf.extend_from_slice(&0x20u32.to_le_bytes()); // clut_b_off
    buf.extend_from_slice(&(r0.len() as u32).to_le_bytes()); // budget
    buf.extend_from_slice(&r0_stream);
    buf.resize(desc_off, 0);
    buf.extend_from_slice(&1u32.to_le_bytes()); // id
    buf.extend_from_slice(&0u32.to_le_bytes()); // offset
    buf.extend_from_slice(&slot_size.to_le_bytes());
    // A second, id = 0 record so the table declares a section boundary.
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&slot_size.to_le_bytes());
    buf.extend_from_slice(&0x800u32.to_le_bytes());
    buf.extend_from_slice(&[0u8; 12]); // terminator
    while !buf.len().is_multiple_of(0x800) {
        buf.push(0);
    }
    let data_base = buf.len();
    buf.extend_from_slice(&(decoded.len() as u32).to_le_bytes());
    buf.extend_from_slice(&stream);
    buf.resize(data_base + slot_size as usize, 0);
    // The id = 0 slot: a plausible dec_size prefix, contents irrelevant.
    buf.extend_from_slice(&0x40u32.to_le_bytes());
    buf.resize(data_base + slot_size as usize + 0x800, 0);
    buf
}

#[test]
fn catalogs_a_synthetic_flagged_section() {
    let file = synth_player_file(32, 0);
    let mut id = 0u32;
    let rows = build_from_file(863, &file, &mut id);
    assert_eq!(rows.len(), 1, "one flagged section, no record[0] hits");
    let b = &rows[0];
    assert_eq!(b.entry_index, 863);
    assert_eq!(b.record_index, 0);
    assert_eq!(b.record_id, 1);
    assert_eq!(b.section, 0);
    assert_eq!(b.pool_offset, 0x20);
    assert_eq!((b.width, b.height, b.bpp), (128, 128, 4));
    assert_eq!(b.clut_entries, 32);
    assert_eq!(b.clut_count, 2);
    assert_eq!(b.clut_x, 0x30);
    assert_eq!(b.byte_len, 4 + 32 * 2 + 128 * 128 / 2);
    assert_eq!(b.slot(), BattleTextureSlot::Section(0));
    assert!(!b.is_record0());
    // Without a name table the label still carries the character.
    assert_eq!(b.label, "Vahn - equip 0x01");

    // With one, it carries the searchable equipment name.
    let names = ItemNameTable::from_names(
        (0..256u16)
            .map(|i| (i == 1).then(|| "Ra-Seru Meta $1".to_string()))
            .collect(),
    );
    let mut id = 0u32;
    let named = build_from_file_with_names(863, &file, &mut id, Some(&names));
    assert_eq!(named[0].label, "Vahn - Ra-Seru Meta $1");
}

#[test]
fn labels_name_the_character_the_section_and_the_equipment() {
    // The four entries are the four party files; anything else falls
    // back to naming the entry rather than inventing a character.
    assert_eq!(character_of_entry(864), Some("Noa"));
    assert_eq!(character_of_entry(12), None);
    assert_eq!(
        label_for(864, 14, 0x11, 2, None),
        "Noa - equip 0x11",
        "no name table: the id is still the search key"
    );
    let names = ItemNameTable::from_names(
        (0..256u16)
            .map(|i| (i == 0x11).then(|| "Ra-Seru Terra $8".to_string()))
            .collect(),
    );
    assert_eq!(
        label_for(864, 14, 0x11, 2, Some(&names)),
        "Noa - Ra-Seru Terra $8"
    );
    // Section defaults and record[0] blocks have no equipment to name.
    assert_eq!(
        label_for(864, 49, 0, 4, Some(&names)),
        "Noa - section 4 default"
    );
    assert_eq!(
        label_for(863, -1, 0, 1, Some(&names)),
        "Vahn - shared record[0] block 1"
    );
    assert_eq!(label_for(12, 0, 5, 0, None), "entry 12 - equip 0x05");
}

#[test]
fn exact_extent_gate_rejects_a_short_record() {
    // Same block, but the record carries 16 trailing bytes past it -
    // so the declared extent no longer finishes the record.
    let file = synth_player_file(32, 16);
    let mut id = 0u32;
    assert!(build_from_file(863, &file, &mut id).is_empty());
}

#[test]
fn partial_palette_runs_are_rejected() {
    // 20 entries is neither one palette nor two.
    let file = synth_player_file(20, 0);
    let mut id = 0u32;
    assert!(build_from_file(863, &file, &mut id).is_empty());
}

/// A player file whose `record[0]` stream carries the two chained
/// header blocks: `clut_a` runs exactly into `clut_b`, which finishes
/// the record. `a_clut_n = 0` mirrors retail `PLAYER1`'s pixel-only
/// first block.
fn synth_with_record0(a_clut_n: usize, b_clut_n: usize) -> Vec<u8> {
    let mut r0 = vec![0u8; 0x40]; // lead-in the header offsets skip
    let push_block = |buf: &mut Vec<u8>, clut_n: usize, rect: TextureRect, x: u16| {
        buf.extend_from_slice(&x.to_le_bytes());
        buf.extend_from_slice(&(clut_n as u16).to_le_bytes());
        for i in 0..clut_n {
            buf.extend_from_slice(&((i as u16 + 1) | 0x0200).to_le_bytes());
        }
        buf.extend(std::iter::repeat_n(0x5Au8, rect.pixel_bytes()));
    };
    let clut_a_off = r0.len();
    push_block(&mut r0, a_clut_n, RECORD0_TEXTURE_RECTS[0], 0);
    let clut_b_off = r0.len();
    push_block(&mut r0, b_clut_n, RECORD0_TEXTURE_RECTS[1], 64);

    let r0_stream = legaia_lzs::compress_optimal(&r0);
    let desc_off = 0x10 + r0_stream.len().div_ceil(4) * 4 + 0x10;
    let mut buf = Vec::new();
    buf.extend_from_slice(&(desc_off as u32).to_le_bytes());
    buf.extend_from_slice(&(clut_a_off as u32).to_le_bytes());
    buf.extend_from_slice(&(clut_b_off as u32).to_le_bytes());
    buf.extend_from_slice(&(r0.len() as u32).to_le_bytes());
    buf.extend_from_slice(&r0_stream);
    buf.resize(desc_off, 0);
    // One id = 0 record so the table parses; its slot holds no texture.
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&0x800u32.to_le_bytes());
    buf.extend_from_slice(&[0u8; 12]);
    while !buf.len().is_multiple_of(0x800) {
        buf.push(0);
    }
    let data_base = buf.len();
    buf.extend_from_slice(&0x40u32.to_le_bytes());
    buf.resize(data_base + 0x800, 0);
    buf
}

#[test]
fn record0_blocks_are_admitted_when_they_chain() {
    let file = synth_with_record0(0, 32);
    let mut id = 0u32;
    let rows = build_from_file(865, &file, &mut id);
    assert_eq!(rows.len(), 2, "both header blocks, got {rows:?}");
    assert!(rows.iter().all(|b| b.is_record0()));
    assert_eq!(rows[0].section, 0);
    assert_eq!(
        rows[0].clut_entries, 0,
        "a pixel-only block is still a block"
    );
    assert_eq!(rows[0].clut_count, 0);
    assert_eq!(rows[0].slot(), BattleTextureSlot::Record0(0));
    assert_eq!(rows[1].section, 1);
    assert_eq!(rows[1].clut_entries, 32);
    assert_eq!(rows[1].clut_x, 64);
    // Rect 0 is 32 halfwords wide, rect 1 is 32 as well - both 128 px.
    assert_eq!((rows[0].width, rows[0].height), (128, 128));

    // The paletteless block decodes through the assembled row.
    let row = assemble_clut_row(&file).expect("assemble row");
    assert_eq!(row.len(), CLUT_ROW_ENTRIES);
    let r = resolve_block(&file, BattleTextureSlot::Record0(0), 0).expect("resolve");
    assert_eq!(r.upload.palette_count(), 0);
    assert!(
        r.upload.rgba(0).is_err(),
        "no block-local palette to decode with"
    );
    let rgba = r
        .upload
        .rgba_with_palette(&row[64..80])
        .expect("borrowed row palette");
    assert_eq!(rgba.len(), 128 * 128 * 4);
}

#[test]
fn record0_blocks_that_do_not_chain_are_rejected() {
    // Grow block 0's palette without moving `clut_b_off`: the chain
    // breaks and neither block may be admitted on block 0's word.
    let mut file = synth_with_record0(0, 32);
    let a_off = u32::from_le_bytes(file[4..8].try_into().unwrap()) as usize;
    let budget = u32::from_le_bytes(file[12..16].try_into().unwrap()) as usize;
    let mut r0 = legaia_lzs::decompress(&file[0x10..], budget).unwrap();
    r0[a_off + 2..a_off + 4].copy_from_slice(&16u16.to_le_bytes());
    let stream = legaia_lzs::compress_optimal(&r0);
    file[0x10..0x10 + stream.len()].copy_from_slice(&stream);
    let mut id = 0u32;
    let rows = build_from_file(865, &file, &mut id);
    assert_eq!(rows.len(), 1, "only the still-chained block 1 survives");
    assert_eq!(rows[0].section, 1);
}

#[test]
fn non_pack_input_contributes_nothing() {
    let mut id = 0u32;
    assert!(build_from_file(0, &vec![0xAAu8; 0x20000], &mut id).is_empty());
    assert!(build_from_file(0, &[], &mut id).is_empty());
}

#[test]
fn resolve_round_trips_the_synthetic_block() {
    let file = synth_player_file(32, 0);
    let r = resolve_block(&file, BattleTextureSlot::Section(0), 1).expect("resolve");
    assert_eq!(r.section, 0);
    assert_eq!(r.record_index, Some(0));
    assert_eq!(r.pool_offset, 0x20);
    assert_eq!(r.upload.palette_count(), 2);
    assert_eq!(r.upload.pixel_width(), 128);
    assert_eq!(r.upload.pixel_height(), 128);
    assert_eq!(r.upload.clut_x, 0x30);
    // Every pixel byte is 0xA5 -> nibbles 5 and 10 alternating.
    let rgba = r.upload.rgba(0).expect("rgba");
    assert_eq!(rgba.len(), 128 * 128 * 4);
    // The block finishes its record, and the write budget is the
    // slot footprint less the dec_size prefix.
    assert_eq!(r.pool_offset + r.upload.block_bytes(), r.decoded.len());
    assert!(r.stream_consumed <= r.stream_capacity);
}

#[test]
fn resolve_reports_an_unflagged_record_as_dead_pool() {
    let mut file = synth_player_file(32, 0);
    // Clear the flag inside the *decoded* record by re-encoding it.
    let pack = battle_data_pack::detect(&file).unwrap();
    let off = pack.records[0].file_offset(pack.data_base);
    let dec_size = u32::from_le_bytes(file[off..off + 4].try_into().unwrap()) as usize;
    let mut decoded = legaia_lzs::decompress(&file[off + 4..], dec_size).unwrap();
    decoded[0x12..0x14].copy_from_slice(&0u16.to_le_bytes());
    let stream = legaia_lzs::compress_optimal(&decoded);
    file[off + 4..off + 4 + stream.len()].copy_from_slice(&stream);
    let err = resolve_block(&file, BattleTextureSlot::Section(0), 0)
        .unwrap_err()
        .to_string();
    assert!(err.contains("not flagged"), "{err}");
}

#[test]
fn slot_selector_parses_both_spellings() {
    use std::str::FromStr;
    assert_eq!(
        BattleTextureSlot::from_str("header0").unwrap(),
        BattleTextureSlot::Record0(0)
    );
    assert_eq!(
        BattleTextureSlot::from_str("HEADER1").unwrap(),
        BattleTextureSlot::Record0(1)
    );
    assert_eq!(
        BattleTextureSlot::from_str(" 14 ").unwrap(),
        BattleTextureSlot::Section(14)
    );
    assert_eq!(BattleTextureSlot::Section(14).to_string(), "14");
    assert_eq!(BattleTextureSlot::Record0(1).to_string(), "header1");
    assert!(BattleTextureSlot::from_str("header2").is_err());
    assert!(BattleTextureSlot::from_str("nope").is_err());
}

#[test]
fn tsv_round_trips_header_and_row() {
    let cat = vec![BattleTextureBlock {
        id: 0,
        entry_index: 864,
        record_index: 14,
        record_id: 0x11,
        section: 2,
        pool_offset: 0x3784,
        width: 128,
        height: 128,
        bpp: 4,
        clut_entries: 32,
        clut_count: 2,
        clut_x: 208,
        byte_len: 8260,
        fnv1a: 0xdead_beef_0000_0001,
        label: "Noa - Ra-Seru Terra $8".into(),
    }];
    let tsv = to_tsv(&cat);
    let header = tsv.lines().next().unwrap();
    assert!(header.starts_with("id\tentry_index\trecord_index\trecord_id\tsection\t"));
    assert!(header.ends_with("\tfnv1a\tlabel"));
    assert!(tsv.contains(
        "0\t864\t14\t17\t2\t14212\t128\t128\t4\t32\t2\t208\t8260\tdeadbeef00000001\t\
             Noa - Ra-Seru Terra $8\n"
    ));
    let r = rollup(&cat);
    assert_eq!(r.count, 1);
    assert_ne!(r.digest, 0);
}

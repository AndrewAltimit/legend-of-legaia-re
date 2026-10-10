use super::*;

/// Build a minimal valid scene asset table with caller-chosen type sequence.
fn synth(types: [u8; 7], total_size: usize) -> Vec<u8> {
    synth_n(&types, total_size)
}

/// Build a table with a caller-chosen descriptor count (6 or 7).
fn synth_n(types: &[u8], total_size: usize) -> Vec<u8> {
    let count = types.len() as u32;
    let mut buf = Vec::with_capacity(total_size);
    buf.extend_from_slice(&count.to_le_bytes());
    // +0x04 = sum of the descriptor sizes below (each 0x100).
    buf.extend_from_slice(&(count * 0x100).to_le_bytes());
    let mut data_off: u32 = header_end(count);
    for &t in types {
        let sz: u32 = 0x100;
        let type_size = ((t as u32) << 24) | sz;
        buf.extend_from_slice(&type_size.to_le_bytes());
        buf.extend_from_slice(&data_off.to_le_bytes());
        data_off += sz;
    }
    buf.resize(total_size.max(buf.len()), 0);
    buf
}

/// A DATA_FIELD chunk: `(type << 24) | size`, then the body.
fn chunk(type_byte: u8, body: &[u8]) -> Vec<u8> {
    let mut v = ((u32::from(type_byte) << 24) | body.len() as u32)
        .to_le_bytes()
        .to_vec();
    v.extend_from_slice(body);
    v
}

#[test]
fn a_streamed_pac_registers_every_mesh_chunk_member_in_order() {
    // Pack of two members: count, two word offsets, then the bodies.
    let mut pack = Vec::new();
    pack.extend_from_slice(&2u32.to_le_bytes());
    pack.extend_from_slice(&3u32.to_le_bytes());
    pack.extend_from_slice(&5u32.to_le_bytes());
    pack.extend_from_slice(&[0xA1; 8]);
    pack.extend_from_slice(&[0xB2; 8]);
    let mut pac = chunk(0x01, &[0; 12]); // a TIM chunk registers nothing
    pac.extend(chunk(0x02, &pack));
    pac.extend(chunk(0x09, &[0xC3; 8]));
    pac.extend_from_slice(&0u32.to_le_bytes()); // zero-size terminator
    pac.extend(chunk(0x09, &[0xD4; 8])); // past the terminator: unread
    let pool = pac_mesh_pool(&pac);
    let firsts: Vec<u8> = pool.iter().map(|m| m.bytes[0]).collect();
    assert_eq!(firsts, [0xA1, 0xB2, 0xC3]);
    assert_eq!(pool[2].slot, 2);
}

#[test]
fn only_a_type_0x14_descriptor_streams_the_pac() {
    assert!(streams_scene_pac(&synth([1, 3, 4, 5, 6, 7, 0x14], 0x10000)));
    assert!(!streams_scene_pac(&synth([1, 2, 3, 4, 5, 6, 7], 0x10000)));
}

#[test]
fn move_descriptor_finds_index_4_for_canonical_layout() {
    let buf = synth([1, 2, 3, 4, 5, 6, 7], 0x10000);
    let s = detect(&buf).expect("should detect");
    let d = s
        .move_descriptor()
        .expect("Move slot is present at index 4");
    assert_eq!(d.type_byte, 0x05);
    assert_eq!(s.move_descriptor_index(), Some(4));
}

#[test]
fn move_descriptor_handles_skip_move_variant() {
    // Tuple `(1, 2, 3, 4, 6, 7, 0x14)` skips Move.
    let buf = synth([1, 2, 3, 4, 6, 7, 0x14], 0x10000);
    let s = detect(&buf).expect("should detect");
    assert!(s.move_descriptor().is_none());
    assert!(s.move_descriptor_index().is_none());
}

#[test]
fn detects_canonical_scene_bundle() {
    let buf = synth([1, 2, 3, 4, 5, 6, 7], 0x10000);
    let s = detect(&buf).expect("should detect");
    assert_eq!(s.count, 7);
    assert_eq!(s.descriptors[0].type_byte, 1);
    assert_eq!(s.descriptors[6].type_byte, 7);
    assert_eq!(s.descriptors[0].data_offset, header_end(7));
}

#[test]
fn detects_count6_town_variant() {
    // Early standalone towns (town01 / town0c) use a 6-descriptor table
    // whose first descriptor is anchored at 0x38 (= 8 + 6*8). The MAN is
    // descriptor index 1 (town01) / 2 (town0c).
    let buf = synth_n(&[0x02, 0x03, 0x05, 0x06, 0x07, 0x14], 0x8000);
    let s = detect(&buf).expect("count-6 table should detect");
    assert_eq!(s.count, 6);
    assert_eq!(s.descriptors[0].data_offset, header_end(6));
    assert_eq!(s.descriptors[0].data_offset, 0x38);
    // The MAN descriptor (type 0x03) resolves through `used()`.
    let man = s.used().iter().find(|d| d.type_byte == 0x03);
    assert!(man.is_some(), "count-6 table exposes its MAN descriptor");
    // Padding slot is not surfaced.
    assert_eq!(s.used().len(), 6);
}

#[test]
fn detects_variant_with_flag_sentinel() {
    // (1, 3, 4, 5, 6, 7, 0x14) - 7 entries observed in the corpus.
    let buf = synth([1, 3, 4, 5, 6, 7, 0x14], 0x10000);
    assert!(detect(&buf).is_some());
}

#[test]
fn detects_leading_flag_variant() {
    // (10, 2, 3, 4, 5, 6, 7) - 1 entry observed.
    let buf = synth([10, 2, 3, 4, 5, 6, 7], 0x10000);
    assert!(detect(&buf).is_some());
}

#[test]
fn rejects_buffer_smaller_than_header() {
    assert!(detect(&[0u8; 16]).is_none());
    assert!(detect(&[0u8; 63]).is_none());
}

#[test]
fn rejects_wrong_count() {
    let mut buf = synth([1, 2, 3, 4, 5, 6, 7], 0x10000);
    // Patch count from 7 to 8.
    buf[0..4].copy_from_slice(&8u32.to_le_bytes());
    assert!(detect(&buf).is_none());
}

#[test]
fn rejects_unknown_type_byte() {
    let mut buf = synth([1, 2, 3, 4, 5, 6, 7], 0x10000);
    // Patch descriptor[0].type_byte to an unknown value (0x55).
    buf[8 + 3] = 0x55;
    assert!(detect(&buf).is_none());
}

#[test]
fn rejects_oversized_asset() {
    let mut buf = synth([1, 2, 3, 4, 5, 6, 7], 0x10000);
    // Set descriptor[0].size to 0x00FF_FFFF - exceeds the 4 MB cap.
    let big = (1u32 << 24) | 0x00FF_FFFF;
    buf[8..12].copy_from_slice(&big.to_le_bytes());
    assert!(detect(&buf).is_none());
}

#[test]
fn rejects_first_descriptor_not_at_header_end() {
    let mut buf = synth([1, 2, 3, 4, 5, 6, 7], 0x10000);
    // Patch descriptor[0].data_offset from 0x40 to 0x80.
    buf[12..16].copy_from_slice(&0x80u32.to_le_bytes());
    assert!(detect(&buf).is_none());
}

#[test]
fn accepts_extended_footprint_offset_past_indexed_size() {
    // Real-world: descriptor offsets past desc[0] are file-relative
    // against the EXTENDED bundle footprint, which often runs past the
    // TOC-indexed view. The detector runs on raw PROT bytes (which may
    // be either view), so it only sanity-checks against MAX_DATA_OFFSET
    // rather than the local buffer length. E.g. `0588_juui1.BIN`'s
    // indexed view is 67584 B but desc[4].data_offset is 177413.
    let mut buf = synth([1, 2, 3, 4, 5, 6, 7], 0x100);
    // Patch descriptor[6].data_offset to a 256 KB value - well past the
    // 256-byte buffer but within MAX_DATA_OFFSET.
    buf[8 + 6 * 8 + 4..8 + 6 * 8 + 8].copy_from_slice(&0x0004_0000u32.to_le_bytes());
    assert!(detect(&buf).is_some());
}

#[test]
fn rejects_pointer_shaped_offset() {
    // Reject descriptor offsets shaped like a RAM pointer (0x80...).
    let mut buf = synth([1, 2, 3, 4, 5, 6, 7], 0x100);
    buf[8 + 6 * 8 + 4..8 + 6 * 8 + 8].copy_from_slice(&0x801C_0000u32.to_le_bytes());
    assert!(detect(&buf).is_none());
}

#[test]
fn rejects_random_bytes() {
    let buf: Vec<u8> = (0..=255u8).cycle().take(0x100).collect();
    assert!(detect(&buf).is_none());
}

#[test]
fn slots_reproduce_positional_walk() {
    // The walk is positional: slot i is the i-th descriptor; payload at
    // base + data_offset; type from the high byte. Mirror FUN_80020224.
    let buf = synth([1, 2, 3, 4, 5, 6, 7], 0x10000);
    let s = detect(&buf).expect("detect");
    let slots: Vec<_> = s.slots().collect();
    assert_eq!(slots.len(), 7);
    // First descriptor's payload anchors at the byte past the header.
    assert_eq!(slots[0].slot, 0);
    assert_eq!(slots[0].data_offset, header_end(7));
    assert_eq!(slots[0].asset_type, AssetType::TimList);
    assert_eq!(slots[4].asset_type, AssetType::Move);
    // Descriptor offsets advance by each declared size (synth uses 0x100).
    assert_eq!(slots[1].data_offset, header_end(7) + 0x100);
}

#[test]
fn payload_range_is_base_relative() {
    let buf = synth([1, 2, 3, 4, 5, 6, 7], 0x10000);
    let s = detect(&buf).expect("detect");
    // With table base 0, slot 0's payload is [0x40, 0x40+0x100).
    let r0 = s.payload_range(0, 0).expect("slot 0");
    assert_eq!(r0, 0x40..0x140);
    // A non-zero base (the scripted variant) shifts every payload.
    let r0b = s.payload_range(0, 0x800).expect("slot 0 @ base 0x800");
    assert_eq!(r0b, 0x840..0x940);
    // Out-of-range slot.
    assert!(s.payload_range(7, 0).is_none());
}

#[test]
fn resolve_handles_bare_table_at_offset_zero() {
    let buf = synth([1, 2, 3, 4, 5, 6, 7], 0x10000);
    let r = resolve(&buf).expect("resolve bare");
    assert_eq!(r.table_base, 0);
    assert_eq!(r.table.count, 7);
    // payload_range against the resolved base lands inside the entry.
    let span = r.table.payload_range(0, r.table_base).unwrap();
    assert_eq!(span.start, 0x40);
}

#[test]
fn resolve_handles_prescript_prefixed_variant() {
    // Build a scripted scene-asset-table via the sibling module's synth
    // path: prescript at 0, table at the next 0x800 boundary.
    use crate::scene_scripted_asset_table;
    let inner = synth([1, 2, 3, 4, 5, 6, 7], 0x200);
    // Hand-assemble a [u16 count=1][u16 off=4][record...] prescript, pad
    // to 0x800, then append the bare table.
    let mut buf = Vec::new();
    buf.extend_from_slice(&1u16.to_le_bytes()); // count
    buf.extend_from_slice(&4u16.to_le_bytes()); // offsets[0] = 2 + 1*2
    buf.extend_from_slice(&[0xFF, 0xFF, 0x00, 0x00]); // one record body
    buf.resize(0x800, 0);
    buf.extend_from_slice(&inner);

    // The sibling detector must agree the table is at 0x800.
    let scripted = scene_scripted_asset_table::detect(&buf).expect("scripted detect");
    assert_eq!(scripted.asset_table_offset, 0x800);

    let r = resolve(&buf).expect("resolve scripted");
    assert_eq!(r.table_base, 0x800);
    assert_eq!(r.table.count, 7);
    // The first slot's payload is base-relative: 0x800 + 0x40.
    let span = r.table.payload_range(0, r.table_base).unwrap();
    assert_eq!(span.start, 0x840);
    // And the byte at that offset is real table payload, not prescript.
    assert!(span.start < buf.len());
}

#[test]
fn accepts_real_world_head_pattern_izumi() {
    // 0031_izumi.BIN head, verbatim through descriptor 6.
    // Descriptor 0: type_size = 0x01025c94, off = 0x40 → type=0x01, size=0x025c94.
    let mut buf = vec![
        0x07, 0x00, 0x00, 0x00, // count = 7
        0x28, 0xF2, 0x04, 0x00, // total_decompressed_size = 0x0004F228
        0x94, 0x5C, 0x02, 0x01, 0x40, 0x00, 0x00, 0x00, // desc 0
        0xA8, 0xE5, 0x01, 0x02, 0xC1, 0x3A, 0x01, 0x00, // desc 1
        0xBC, 0x40, 0x00, 0x03, 0xBE, 0x28, 0x02, 0x00, // desc 2
        0x28, 0x00, 0x00, 0x04, 0x5C, 0x49, 0x02, 0x00, // desc 3
        0x04, 0x64, 0x00, 0x05, 0x65, 0x49, 0x02, 0x00, // desc 4
        0x04, 0x00, 0x00, 0x06, 0x02, 0x68, 0x02, 0x00, // desc 5
        0x00, 0x0B, 0x00, 0x07, 0x05, 0x68, 0x02, 0x00, // desc 6
    ];
    // Pad enough that all descriptor offsets fit (plus trailing slack).
    buf.resize(0x30000, 0);
    let s = detect(&buf).expect("real-world izumi pattern should detect");
    assert_eq!(s.descriptors[0].type_byte, 1);
    assert_eq!(s.descriptors[1].type_byte, 2);
    assert_eq!(s.descriptors[6].type_byte, 7);
    assert_eq!(s.descriptors[0].data_offset, 0x40);
    // `+0x04` is the sum of the seven decompressed sizes, not an offset.
    assert_eq!(s.total_decompressed_size, 0x0004_F228);
    assert_eq!(s.descriptor_size_sum(), 0x0004_F228);
    assert!(s.total_size_is_consistent());
    // ...and it is larger than the entry it sits in (159744 bytes) - the
    // property that rules out "file size" / "sector count" readings.
    assert!(s.total_decompressed_size as usize > 159_744);
}

#[test]
fn synthetic_tables_carry_a_consistent_total() {
    let buf = synth([1, 2, 3, 4, 5, 6, 7], 0x4000);
    let s = detect(&buf).expect("synthetic detect");
    assert!(s.total_size_is_consistent());
    assert_eq!(s.descriptor_size_sum(), 7 * 0x100);
}

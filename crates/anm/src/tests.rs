use super::*;

fn synthetic(records: &[&[u8]]) -> Vec<u8> {
    let count = records.len();
    let table_end = 4 + count * 4;
    let mut offs = Vec::with_capacity(count);
    let mut acc = table_end;
    for r in records {
        offs.push(acc);
        acc += r.len();
    }
    let mut out = Vec::with_capacity(acc);
    out.extend_from_slice(&(count as u32).to_le_bytes());
    for o in &offs {
        out.extend_from_slice(&(*o as u32).to_le_bytes());
    }
    for r in records {
        out.extend_from_slice(r);
    }
    out
}

#[test]
fn parses_synthetic_two_records() {
    let buf = synthetic(&[&[0xAA; 16], &[0xBB; 32]]);
    let pack = parse(&buf).unwrap();
    assert_eq!(pack.count, 2);
    assert_eq!(pack.records[0].size, 16);
    assert_eq!(pack.records[1].size, 32);
    assert_eq!(record_bytes(&buf, &pack.records[0]), &[0xAA; 16][..]);
    assert_eq!(record_bytes(&buf, &pack.records[1]), &[0xBB; 32][..]);
}

#[test]
fn empty_count_returns_no_records() {
    let buf = 0u32.to_le_bytes();
    let pack = parse(&buf).unwrap();
    assert_eq!(pack.count, 0);
    assert!(pack.records.is_empty());
}

#[test]
fn rejects_offset_inside_table() {
    let mut buf = Vec::new();
    buf.extend_from_slice(&1u32.to_le_bytes()); // count = 1
    buf.extend_from_slice(&4u32.to_le_bytes()); // offset = 4 - points INTO table
    buf.extend_from_slice(&[0xAA; 16]);
    assert!(parse(&buf).is_err());
}

#[test]
fn rejects_offset_past_end() {
    let mut buf = Vec::new();
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&1000u32.to_le_bytes());
    assert!(parse(&buf).is_err());
}

#[test]
fn rejects_non_monotonic_offsets() {
    let mut buf = Vec::new();
    buf.extend_from_slice(&2u32.to_le_bytes());
    buf.extend_from_slice(&100u32.to_le_bytes());
    buf.extend_from_slice(&50u32.to_le_bytes());
    buf.extend_from_slice(&[0u8; 200]);
    assert!(parse(&buf).is_err());
}

#[test]
fn record_header_marker_check() {
    // Synth a header with the canonical marker + flag=0x0002.
    let mut rec = vec![0u8; RECORD_HEADER_SIZE];
    rec[0] = 0x0A; // a = 0x0A
    rec[2] = 0x1E; // b = 0x1E
    rec[4..6].copy_from_slice(&RECORD_MARKER_1.to_le_bytes());
    rec[6..8].copy_from_slice(&0x0002u16.to_le_bytes());
    let h = RecordHeader::from_bytes(&rec).unwrap();
    assert_eq!(h.a, 0x0A);
    assert_eq!(h.b, 0x1E);
    assert!(h.marker_ok);
    assert!(h.flag_known);
    assert_eq!(h.flag, 0x0002);
}

#[test]
fn record_header_accepts_alternate_flag() {
    // Same canonical marker but flag = 0x0004 (also observed in real data).
    let mut rec = vec![0u8; RECORD_HEADER_SIZE];
    rec[4..6].copy_from_slice(&RECORD_MARKER_1.to_le_bytes());
    rec[6..8].copy_from_slice(&0x0004u16.to_le_bytes());
    let h = RecordHeader::from_bytes(&rec).unwrap();
    assert!(h.marker_ok);
    assert!(h.flag_known);
}

#[test]
fn record_bytecode_histogram_skips_header() {
    // 8-byte header followed by 4 bytes of bytecode: 0xAA 0xBB 0xAA 0xCC.
    let mut rec = vec![0u8; RECORD_HEADER_SIZE];
    rec.extend_from_slice(&[0xAA, 0xBB, 0xAA, 0xCC]);
    let h = record_bytecode_histogram(&rec);
    assert_eq!(h[0xAA_usize], 2);
    assert_eq!(h[0xBB_usize], 1);
    assert_eq!(h[0xCC_usize], 1);
    // Header bytes (zeros) should not have been counted.
    assert_eq!(h[0], 0);
}

#[test]
fn record_bytecode_histogram_empty_record() {
    let rec = vec![0u8; RECORD_HEADER_SIZE]; // header only
    let h = record_bytecode_histogram(&rec);
    assert_eq!(h.iter().sum::<u32>(), 0);
}

#[test]
fn pack_bytecode_histogram_aggregates_records() {
    let mut r0 = vec![0u8; RECORD_HEADER_SIZE];
    r0.extend_from_slice(&[0xAA, 0xBB]);
    let mut r1 = vec![0u8; RECORD_HEADER_SIZE];
    r1.extend_from_slice(&[0xAA, 0xCC]);
    let buf = synthetic(&[&r0, &r1]);
    let pack = parse(&buf).unwrap();
    let h = pack_bytecode_histogram(&buf, &pack);
    assert_eq!(h[0xAA_usize], 2);
    assert_eq!(h[0xBB_usize], 1);
    assert_eq!(h[0xCC_usize], 1);
}

#[test]
fn top_k_sorts_by_count_then_byte() {
    let mut hist = [0u32; 256];
    hist[0x10] = 5;
    hist[0x20] = 5;
    hist[0x30] = 3;
    let top = top_k(&hist, 3);
    // Count 5 wins; tie broken by ascending byte value.
    assert_eq!(top[0], (0x10, 5));
    assert_eq!(top[1], (0x20, 5));
    assert_eq!(top[2], (0x30, 3));
}

#[test]
fn top_k_skips_zero_counts() {
    let mut hist = [0u32; 256];
    hist[0x42] = 1;
    let top = top_k(&hist, 100);
    assert_eq!(top, vec![(0x42, 1)]);
}

#[test]
fn record_bytecode_bigram_counts_pairs_after_header() {
    // Build a record whose body is `[0x10, 0x20, 0x10, 0x30]`. Body
    // bigrams: (0x10,0x20), (0x20,0x10), (0x10,0x30).
    let mut rec = vec![0u8; RECORD_HEADER_SIZE];
    rec.extend_from_slice(&[0x10, 0x20, 0x10, 0x30]);
    let hist = record_bytecode_bigram(&rec);
    assert_eq!(hist[0x10][0x20], 1);
    assert_eq!(hist[0x20][0x10], 1);
    assert_eq!(hist[0x10][0x30], 1);
}

#[test]
fn pack_bytecode_top_bigrams_returns_sorted_triples() {
    let buf = synthetic(&[
        &[0xAAu8; RECORD_HEADER_SIZE]
            .iter()
            .copied()
            .chain([0x10, 0x20, 0x10, 0x20])
            .collect::<Vec<_>>(),
        &[0xBBu8; RECORD_HEADER_SIZE]
            .iter()
            .copied()
            .chain([0x10, 0x20])
            .collect::<Vec<_>>(),
    ]);
    let pack = parse(&buf).unwrap();
    let bigrams = pack_bytecode_top_bigrams(&buf, &pack, 3);
    assert!(bigrams.iter().any(|&(a, b, _)| a == 0x10 && b == 0x20));
    // Top bigram should be (0x10, 0x20) with count 3 (2 in record 0,
    // 1 in record 1).
    assert_eq!(bigrams[0], (0x10, 0x20, 3));
}

#[test]
fn bone_keyframe_parses_24_bytes() {
    let mut buf = vec![0u8; BoneKeyframe::SIZE];
    // src_pos = (1, 2, 3); dst_pos = (10, 20, 30)
    buf[0..2].copy_from_slice(&1i16.to_le_bytes());
    buf[2..4].copy_from_slice(&2i16.to_le_bytes());
    buf[4..6].copy_from_slice(&3i16.to_le_bytes());
    buf[6..8].copy_from_slice(&10i16.to_le_bytes());
    buf[8..10].copy_from_slice(&20i16.to_le_bytes());
    buf[10..12].copy_from_slice(&30i16.to_le_bytes());
    // src_rot = (0, 0, 0); dst_rot = (1024, -1024, 0)
    buf[18..20].copy_from_slice(&1024i16.to_le_bytes());
    buf[20..22].copy_from_slice(&(-1024i16).to_le_bytes());
    let kf = BoneKeyframe::from_bytes(&buf).unwrap();
    assert_eq!(kf.src_pos, [1, 2, 3]);
    assert_eq!(kf.dst_pos, [10, 20, 30]);
    assert_eq!(kf.dst_rot, [1024, -1024, 0]);
}

#[test]
fn bone_keyframe_lerp_at_endpoints() {
    let kf = BoneKeyframe {
        src_pos: [10, 20, 30],
        dst_pos: [110, 220, 330],
        src_rot: [0, 0, 0],
        dst_rot: [256, -256, 0],
    };
    let (p0, r0) = kf.interpolate(0);
    assert_eq!(p0, [10, 20, 30]);
    assert_eq!(r0, [0, 0, 0]);
    let (p256, _) = kf.interpolate(255);
    // At factor=255, we get src + delta*(255/256), one short of full.
    assert!(p256[0] >= 109 && p256[0] <= 110);
}

#[test]
fn keyframe_reader_walks_n_bones() {
    // Build a record with 2 bones: 8-byte header + 16 OUTPUT bytes
    // + 48 keyframe bytes = 72 bytes total.
    let mut record = vec![0u8; RECORD_HEADER_SIZE + 8 * 2 + BoneKeyframe::SIZE * 2];
    record[4..6].copy_from_slice(&RECORD_MARKER_1.to_le_bytes());
    // Bone 0 keyframe at offset 8 + 16 = 24
    let off0 = 24;
    record[off0..off0 + 2].copy_from_slice(&100i16.to_le_bytes()); // src_pos.x
    record[off0 + 6..off0 + 8].copy_from_slice(&200i16.to_le_bytes()); // dst_pos.x
    // Bone 1 keyframe at offset 24 + 24 = 48
    let off1 = 48;
    record[off1..off1 + 2].copy_from_slice(&300i16.to_le_bytes());

    let reader = KeyframeReader::parse(&record, 2).unwrap();
    assert_eq!(reader.bone_count(), 2);
    let kf0 = reader.keyframe(0).unwrap();
    assert_eq!(kf0.src_pos[0], 100);
    assert_eq!(kf0.dst_pos[0], 200);
    let kf1 = reader.keyframe(1).unwrap();
    assert_eq!(kf1.src_pos[0], 300);
    assert!(reader.keyframe(2).is_none());
    assert_eq!(reader.iter().count(), 2);
}

#[test]
fn keyframe_reader_rejects_undersized_record() {
    let record = vec![0u8; 32]; // way too small for 4 bones
    assert!(KeyframeReader::parse(&record, 4).is_err());
}

#[test]
fn fits_exactly_validates_size_relation() {
    assert!(KeyframeReader::fits_exactly(8 + 32 * 5, 5));
    assert!(!KeyframeReader::fits_exactly(8 + 32 * 5 + 1, 5));
}

#[test]
fn infer_bone_count_round_trips() {
    assert_eq!(KeyframeReader::infer_bone_count(8 + 32 * 7), Some(7));
    assert_eq!(KeyframeReader::infer_bone_count(8), Some(0));
    assert_eq!(KeyframeReader::infer_bone_count(8 + 33), None);
    assert_eq!(KeyframeReader::infer_bone_count(2), None);
}

#[test]
fn peel_preamble_round_trip() {
    // Build: 16-byte preamble (zeros) with expanded_size = payload_len,
    // then synthetic payload.
    let payload = synthetic(&[&[0xCC; 8]]);
    let mut full = vec![0u8; PREAMBLE_SIZE];
    full[PREAMBLE_EXPANDED_SIZE_OFFSET..PREAMBLE_EXPANDED_SIZE_OFFSET + 4]
        .copy_from_slice(&(payload.len() as u32).to_le_bytes());
    full.extend_from_slice(&payload);
    let peeled = peel_preamble(&full).unwrap();
    let pack = parse(peeled).unwrap();
    assert_eq!(pack.count, 1);
    assert_eq!(record_bytes(peeled, &pack.records[0]), &[0xCC; 8][..]);
}

// --- Adversarial-input fuzz (bulk scanner / web viewer feed arbitrary
//     bytes to `parse`; a panic there takes down the disc load) --------

/// `parse` on pseudo-random byte soup of every length 0..512 must only
/// ever return `Ok`/`Err`, never panic (no capacity-overflow on a huge
/// `count`, no out-of-bounds on a bogus offset table).
#[test]
fn parse_random_bytes_never_panics() {
    for seed in 0u64..400 {
        let mut x = seed
            .wrapping_mul(0x9E3779B97F4A7C15)
            .wrapping_add(0x1234_5678);
        let n = (seed % 512) as usize;
        let mut buf = Vec::with_capacity(n);
        for _ in 0..n {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            buf.push(x as u8);
        }
        let _ = parse(&buf);
        // peel_preamble + the keyframe reader on the same junk too.
        let _ = peel_preamble(&buf);
        let _ = KeyframeReader::parse(&buf, (x as usize) & 0xFFFF);
    }
}

/// A count word claiming far more records than the buffer can hold must
/// be rejected without trying to allocate `count` entries up front.
#[test]
fn huge_count_word_is_err_not_capacity_overflow() {
    let mut buf = Vec::new();
    buf.extend_from_slice(&u32::MAX.to_le_bytes()); // count = 0xFFFFFFFF
    buf.extend_from_slice(&[0u8; 32]);
    assert!(parse(&buf).is_err());
}

/// `KeyframeReader::parse` with a colossal bone count must reject via
/// checked arithmetic, not overflow-panic on `8 * bone_count`.
#[test]
fn keyframe_reader_huge_bone_count_is_err() {
    let rec = vec![0u8; 64];
    assert!(KeyframeReader::parse(&rec, usize::MAX).is_err());
    assert!(KeyframeReader::parse(&rec, usize::MAX / 2).is_err());
}
